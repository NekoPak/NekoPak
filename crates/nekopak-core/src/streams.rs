//! Loopback-only streams. Cloud signed URLs and credentials never cross IPC.
use super::*;
use nekopak_media::native::RangeSource;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

impl AppService {
    pub async fn media_session_open(
        self: &Arc<Self>,
        id: &str,
        quality: Option<&str>,
    ) -> Result<Value, ClientError> {
        let epoch = self.stream_epoch.load(std::sync::atomic::Ordering::SeqCst);
        let ctx = RequestContext::new(Duration::from_secs(90));
        let source = self
            .media_source(id, quality.unwrap_or("original"), &ctx)
            .await?;
        let mime = media_mime(&source.name());
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| ClientError::new(ErrorCode::Internal, "无法启动本机媒体连接。"))?;
        let port = listener
            .local_addr()
            .map_err(|_| ClientError::new(ErrorCode::Internal, "无法取得本机媒体端口。"))?
            .port();
        let session_id = uuid::Uuid::new_v4().to_string();
        let path = format!("/{}/{}/content", session_id, uuid::Uuid::new_v4());
        let name = source.name();
        let source = Arc::new(source);
        let control = RequestContext::new(Duration::from_secs(24 * 3600));
        {
            let mut sessions = self
                .streams
                .lock()
                .map_err(|_| ClientError::new(ErrorCode::Internal, "媒体会话暂不可用。"))?;
            if self.stream_epoch.load(std::sync::atomic::Ordering::SeqCst) != epoch {
                return Err(ClientError::new(
                    ErrorCode::Cancelled,
                    "账号或映射已改变，播放未启动。",
                ));
            }
            if sessions.len() >= 8 {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "请先关闭部分播放窗口。",
                ));
            }
            sessions.insert(session_id.clone(), control.clone());
        }
        let id = session_id.clone();
        let service = Arc::downgrade(self);
        let stream_path = path.clone();
        tokio::spawn(async move {
            let semaphore = Arc::new(tokio::sync::Semaphore::new(4));
            let mut children = tokio::task::JoinSet::new();
            let _: Result<(), ClientError> = control
                .run(async {
                    loop {
                        let (socket, peer) = listener.accept().await.map_err(|_| {
                            ClientError::new(ErrorCode::Internal, "本机播放连接结束。")
                        })?;
                        if !peer.ip().is_loopback() {
                            continue;
                        }
                        let Ok(permit) = semaphore.clone().try_acquire_owned() else {
                            continue;
                        };
                        let source = source.clone();
                        let path = stream_path.clone();
                        let control = control.clone();
                        children.spawn(async move {
                            let _permit = permit;
                            let _ = control
                                .run(serve(socket, source.as_ref(), port, &path, mime))
                                .await;
                        });
                        while children.try_join_next().is_some() {}
                    }
                })
                .await;
            children.abort_all();
            while children.join_next().await.is_some() {}
            if let Some(service) = service.upgrade() {
                if let Ok(mut sessions) = service.streams.lock() {
                    sessions.remove(&id);
                }
            }
        });
        Ok(
            json!({"session_id":session_id,"url":format!("http://127.0.0.1:{port}{path}"),"mime_type":mime,"name":name}),
        )
    }
    pub fn media_session_close(&self, id: &str) -> Result<(), ClientError> {
        if let Some(control) = self
            .streams
            .lock()
            .map_err(|_| ClientError::new(ErrorCode::Internal, "媒体会话暂不可用。"))?
            .remove(id)
        {
            control.cancel();
        }
        Ok(())
    }
}
fn media_mime(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "ts" => "video/mp2t",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    }
}
async fn serve(
    mut socket: TcpStream,
    source: &dyn RangeSource,
    port: u16,
    path: &str,
    mime: &str,
) -> Result<(), ClientError> {
    let error = || ClientError::new(ErrorCode::TransientNetwork, "本机媒体连接已结束。");
    let mut request = Vec::new();
    loop {
        let mut buf = [0; 2048];
        let n = tokio::time::timeout(Duration::from_secs(5), socket.read(&mut buf))
            .await
            .map_err(|_| error())?
            .map_err(|_| error())?;
        if n == 0 {
            return Ok(());
        }
        request.extend_from_slice(&buf[..n]);
        if request.len() > 16384 {
            return Err(error());
        }
        if request.windows(4).any(|b| b == b"\r\n\r\n") {
            break;
        }
    }
    let request = std::str::from_utf8(&request).map_err(|_| error())?;
    let mut lines = request.lines();
    let first = lines.next().unwrap_or("");
    let mut words = first.split_whitespace();
    let method = words.next().unwrap_or("");
    let target = words.next().unwrap_or("");
    let headers: Vec<_> = lines
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim()))
        .collect();
    let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
    let host = format!("127.0.0.1:{port}");
    if !matches!(method, "GET" | "HEAD")
        || target != path
        || header("host") != Some(host.as_str())
        || header("origin").is_some_and(|o| {
            !matches!(
                o,
                "http://tauri.localhost" | "https://tauri.localhost" | "tauri://localhost"
            )
        })
        || headers.iter().filter(|(k, _)| k == "range").count() > 1
    {
        socket
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .map_err(|_| error())?;
        return Ok(());
    }
    let len = source.len();
    let range = parse_range(header("range"), len);
    let Some((start, end, partial)) = range else {
        socket.write_all(format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.map_err(|_|error())?;
        return Ok(());
    };
    let count = if len == 0 { 0 } else { end - start + 1 };
    let mut output=format!("HTTP/1.1 {}\r\nContent-Type: {mime}\r\nContent-Length: {count}\r\nAccept-Ranges: bytes\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n",if partial{"206 Partial Content"}else{"200 OK"});
    if partial {
        output.push_str(&format!("Content-Range: bytes {start}-{end}/{len}\r\n"));
    }
    if let Some(origin) = header("origin") {
        output.push_str(&format!("Access-Control-Allow-Origin: {origin}\r\n"));
    }
    output.push_str("\r\n");
    socket
        .write_all(output.as_bytes())
        .await
        .map_err(|_| error())?;
    if method == "GET" {
        let mut at = start;
        while at < start + count {
            let size = (start + count - at).min(1024 * 1024) as usize;
            let bytes = source.read_exact(at, size).await?;
            socket.write_all(&bytes).await.map_err(|_| error())?;
            at += size as u64;
        }
    }
    Ok(())
}
fn parse_range(range: Option<&str>, len: u64) -> Option<(u64, u64, bool)> {
    let Some(range) = range else {
        return Some((0, len.saturating_sub(1), false));
    };
    if len == 0 {
        return None;
    }
    let (start, end) = range.strip_prefix("bytes=")?.split_once('-')?;
    if end.contains(',') {
        return None;
    }
    let (start, end) = if start.is_empty() {
        let suffix = end.parse::<u64>().ok()?;
        if suffix == 0 {
            return None;
        }
        (len.saturating_sub(suffix), len - 1)
    } else {
        let start = start.parse::<u64>().ok()?;
        let end = if end.is_empty() {
            len - 1
        } else {
            end.parse::<u64>().ok()?.min(len - 1)
        };
        (start, end)
    };
    (start <= end && start < len).then_some((start, end, true))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn range_semantics_are_bounded() {
        assert_eq!(parse_range(Some("bytes=2-5"), 10), Some((2, 5, true)));
        assert_eq!(parse_range(Some("bytes=-3"), 10), Some((7, 9, true)));
        assert_eq!(parse_range(Some("bytes=9-"), 10), Some((9, 9, true)));
        for r in ["bytes=10-", "bytes=-0", "bytes=2-1", "bytes=0-1,4-5"] {
            assert!(parse_range(Some(r), 10).is_none());
        }
    }
}
