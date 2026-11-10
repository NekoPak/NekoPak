//! Original-file bytes only. CDN requests never carry API authorization or CAPTCHA headers.
use super::*;
use nekopak_provider::{DownloadChunk, DownloadInfo, DownloadObserver};
use std::time::Duration;

impl NativeProvider {
    async fn download_metadata(&self, id: &str) -> Result<(DownloadInfo, String), ClientError> {
        self.representation_metadata(id, "original").await
    }
    async fn representation_metadata(
        &self,
        id: &str,
        quality: &str,
    ) -> Result<(DownloadInfo, String), ClientError> {
        wire::validate_id(id)?;
        let snapshot = self.snapshot().await?;
        let subject = self.drive_subject(&snapshot).await?;
        let scope = format!(
            "{:x}",
            Sha256::digest(format!("{}\0{}", self.profile()?.id, subject))
        );
        let (value, _) = self
            .drive_request_at(
                Method::GET,
                &format!("{}/drive/v1/files/{id}?usage=FETCH", self.http.drive_origin),
                None,
                Some(snapshot.epoch),
            )
            .await?;
        let mut entry = wire::entry(&value)?;
        if entry.id != id || entry.kind != EntryKind::File {
            return Err(error(ErrorCode::Conflict, "请选择一个文件下载。"));
        }
        let version = value
            .get("hash")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or(entry.modified_at.as_deref())
            .ok_or_else(|| {
                error(
                    ErrorCode::IntegrityFailed,
                    "服务没有提供可核对的文件版本，暂不能安全续传。",
                )
            })?;
        let mut representation_version = String::new();
        let url = if quality != "original" {
            let media = value
                .get("medias")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items
                        .iter()
                        .find(|m| m["media_id"] == quality && m["is_origin"] != true)
                })
                .ok_or_else(|| error(ErrorCode::NotFound, "所选画质不存在，未切换到其他画质。"))?;
            if media["need_more_quota"] == true || media["is_visible"] == false {
                return Err(error(
                    ErrorCode::PermissionDenied,
                    "当前账号无法读取所选画质。",
                ));
            }
            let location = wire::text(&media["link"], "url", 16384)?;
            // Probe the exact representation length; a transcode never inherits
            // the original file's byte size. The signed URL remains in Rust.
            let (probe, etag) = self
                .fetch_range(&location, None, 0, 1, None, DownloadObserver::default())
                .await?;
            entry.size_bytes = DecimalU64(probe.total);
            let etag = etag.filter(|s| !s.starts_with("W/")).ok_or_else(|| {
                error(
                    ErrorCode::Unsupported,
                    "所选转码未提供可验证的版本标识，暂不能安全缓存或导出。",
                )
            })?;
            representation_version = format!("{quality}\0{etag}");
            entry.etag = Some(etag);
            location
        } else if entry.size_bytes.0 == 0 {
            String::new()
        } else {
            wire::text(&value, "web_content_link", 16384)?
        };
        let identity = format!(
            "{:x}",
            Sha256::digest(if quality == "original" {
                format!("{id}\0{}\0{version}", entry.size_bytes.0)
            } else {
                format!(
                    "{id}\0{}\0{version}\0{representation_version}",
                    entry.size_bytes.0
                )
            })
        );
        Ok((
            DownloadInfo {
                entry,
                identity,
                account_scope: scope,
            },
            url,
        ))
    }
    pub(super) async fn native_download_info(&self, id: &str) -> Result<DownloadInfo, ClientError> {
        Ok(self.download_metadata(id).await?.0)
    }
    pub(super) async fn native_download_chunk(
        &self,
        id: &str,
        identity: &str,
        scope: &str,
        start: u64,
        length: u64,
        observer: DownloadObserver,
    ) -> Result<DownloadChunk, ClientError> {
        self.native_media_chunk(id, "original", identity, scope, start, length, observer)
            .await
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn native_media_chunk(
        &self,
        id: &str,
        quality: &str,
        identity: &str,
        scope: &str,
        start: u64,
        length: u64,
        observer: DownloadObserver,
    ) -> Result<DownloadChunk, ClientError> {
        if length == 0 || length > 4 * 1024 * 1024 {
            return Err(wire::invalid());
        }
        if self.account_scope().await? != scope {
            return Err(expired());
        }
        let (info, location) = self.representation_metadata(id, quality).await?;
        if info.identity != identity || info.account_scope != scope {
            return Err(error(
                ErrorCode::IntegrityFailed,
                "远端文件或账号已变化，已停止续传并保留临时文件。",
            ));
        }
        if start >= info.entry.size_bytes.0 {
            return Err(wire::invalid());
        }
        let end = start
            .saturating_add(length - 1)
            .min(info.entry.size_bytes.0 - 1);
        let (result, _) = self
            .fetch_range(
                &location,
                Some(info.entry.size_bytes.0),
                start,
                end - start + 1,
                info.entry.etag.as_deref(),
                observer,
            )
            .await?;
        if self.account_scope().await? != scope {
            return Err(expired());
        }
        Ok(result)
    }
    pub(super) async fn native_media_info(
        &self,
        id: &str,
        quality: &str,
    ) -> Result<DownloadInfo, ClientError> {
        Ok(self.representation_metadata(id, quality).await?.0)
    }
    pub(super) async fn native_media_options(
        &self,
        id: &str,
    ) -> Result<Vec<nekopak_provider::Representation>, ClientError> {
        wire::validate_id(id)?;
        let (value, _) = self
            .get(&format!(
                "{}/drive/v1/files/{id}?usage=FETCH",
                self.http.drive_origin
            ))
            .await?;
        let entry = wire::entry(&value)?;
        let mut options = vec![nekopak_provider::Representation {
            id: "original".into(),
            label: "原画".into(),
            width: None,
            height: None,
            size_bytes: Some(entry.size_bytes),
            container: "original".into(),
            readable: value["web_content_link"]
                .as_str()
                .is_some_and(|s| !s.is_empty()),
            unreadable_reason: None,
        }];
        if let Some(items) = value["medias"].as_array() {
            for m in items {
                let dimension =
                    |key: &str| m["video"][key].as_u64().and_then(|n| u32::try_from(n).ok());
                if m["is_origin"] == true {
                    options[0].width = dimension("width");
                    options[0].height = dimension("height");
                    continue;
                }
                let id = wire::text(m, "media_id", 256)?;
                wire::validate_id(&id)?;
                let readable = m["need_more_quota"] != true
                    && m["is_visible"] != false
                    && m["link"]["url"].as_str().is_some_and(|s| !s.is_empty());
                options.push(nekopak_provider::Representation {
                    id,
                    label: dimension("height")
                        .map(|h| format!("{h}P"))
                        .unwrap_or_else(|| "转码".into()),
                    width: dimension("width"),
                    height: dimension("height"),
                    size_bytes: None,
                    container: "mp4".into(),
                    readable,
                    unreadable_reason: (!readable)
                        .then(|| "当前画质尚不可读取或需要服务权限。".into()),
                });
            }
        }
        Ok(options)
    }
    pub(super) async fn fetch_range(
        &self,
        location: &str,
        total: Option<u64>,
        start: u64,
        length: u64,
        expected_etag: Option<&str>,
        observer: DownloadObserver,
    ) -> Result<(DownloadChunk, Option<String>), ClientError> {
        let mut location = location.to_owned();
        let end = start
            .checked_add(length)
            .and_then(|n| n.checked_sub(1))
            .ok_or_else(wire::invalid)?;
        for _ in 0..4 {
            self.http.check_url(&location)?;
            let url = url::Url::parse(&location).map_err(|_| wire::invalid())?;
            let test_loopback = cfg!(debug_assertions)
                && self.http.drive_origin.starts_with("http://127.0.0.1:")
                && url.host_str() == Some("127.0.0.1");
            if (!test_loopback && url.scheme() != "https")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                return Err(wire::invalid());
            }
            let host = url.host_str().ok_or_else(wire::invalid)?;
            let port = url.port_or_known_default().ok_or_else(wire::invalid)?;
            if !test_loopback && port != 443 {
                return Err(wire::invalid());
            }
            let addresses: Vec<_> = tokio::net::lookup_host((host, port))
                .await
                .map_err(|_| network())?
                .collect();
            if addresses.is_empty()
                || (!test_loopback && addresses.iter().any(|a| !public_ip(a.ip())))
            {
                return Err(error(
                    ErrorCode::PermissionDenied,
                    "下载地址不是可用的公共 HTTPS 地址。",
                ));
            }
            let builder = download_client_builder()?.resolve_to_addrs(host, &addresses);
            let client = if test_loopback {
                builder.no_proxy()
            } else {
                self.http.cdn_proxy(builder)?
            }
            .build()
            .map_err(|_| network())?;
            let mut request = client
                .get(url.clone())
                .header("range", format!("bytes={start}-{end}"))
                .header("accept-encoding", "identity");
            if let Some(etag) = expected_etag {
                request = request.header("if-match", etag);
            }
            let mut response = request.send().await.map_err(|_| network())?;
            if response.status().is_redirection() {
                let redirect = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(wire::invalid)?;
                location = url.join(redirect).map_err(|_| wire::invalid())?.into();
                continue;
            }
            let status = response.status().as_u16();
            if matches!(status, 412 | 416) {
                return Err(error(
                    ErrorCode::IntegrityFailed,
                    "下载来源版本或范围已改变，未拼接不同内容。",
                ));
            }
            if !response.status().is_success() {
                // Classify the HTTP failure before requiring the success
                // response's ETag. Error pages need not carry a content ETag.
                let mut failure = ClientError::from_http(status, BackendId::RustCompat);
                if failure.code == ErrorCode::TransientNetwork {
                    if let Some(seconds) = response
                        .headers()
                        .get("retry-after")
                        .and_then(|value| value.to_str().ok())
                        .and_then(http::retry_after_seconds)
                    {
                        failure.details =
                            Some([("retry_after_seconds".into(), seconds.into())].into());
                    }
                }
                return Err(failure);
            }
            let requested = end - start + 1;
            let etag = response
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .filter(|v| v.len() <= 1024 && !v.chars().any(char::is_control))
                .map(str::to_owned);
            if expected_etag.is_some_and(|expected| etag.as_deref() != Some(expected)) {
                return Err(error(
                    ErrorCode::IntegrityFailed,
                    "转码版本发生变化，未拼接不同内容。",
                ));
            }
            let mut actual_total = total;
            if response.status().as_u16() == 206 {
                let range = response
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(wire::invalid)?;
                let prefix = format!("bytes {start}-{end}/");
                let declared = range
                    .strip_prefix(&prefix)
                    .and_then(|v| v.parse::<u64>().ok())
                    .filter(|n| *n > end)
                    .ok_or_else(wire::invalid)?;
                if total.is_some_and(|n| n != declared) {
                    return Err(error(
                        ErrorCode::IntegrityFailed,
                        "下载范围与请求不一致，未写入临时文件。",
                    ));
                }
                actual_total = Some(declared);
            } else if response.status().as_u16() != 200 || start != 0 || total != Some(requested) {
                return Err(error(
                    ErrorCode::IntegrityFailed,
                    "服务器未返回请求的文件范围，未追加错误内容。",
                ));
            }
            if response
                .headers()
                .get("content-encoding")
                .is_some_and(|v| v != "identity")
                || response.content_length().is_some_and(|n| n != requested)
            {
                return Err(wire::invalid());
            }
            let mut bytes = Vec::with_capacity(requested as usize);
            while let Some(chunk) = response.chunk().await.map_err(|_| network())? {
                if bytes.len() + chunk.len() > requested as usize {
                    return Err(wire::invalid());
                }
                bytes.extend_from_slice(&chunk);
                observer.received(bytes.len() as u64);
            }
            if bytes.len() != requested as usize {
                return Err(error(
                    ErrorCode::IntegrityFailed,
                    "下载内容长度不完整，未写入未确认的数据。",
                ));
            }
            return Ok((
                DownloadChunk {
                    bytes,
                    total: actual_total.ok_or_else(wire::invalid)?,
                },
                etag,
            ));
        }
        Err(error(
            ErrorCode::TransientNetwork,
            "下载地址重定向次数过多。",
        ))
    }
}

fn network() -> ClientError {
    error(
        ErrorCode::TransientNetwork,
        "文件连接中断，请重试；已确认的下载进度会保留。",
    )
}
pub(super) fn download_client_builder() -> Result<reqwest::ClientBuilder, ClientError> {
    let builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(45))
        .user_agent(concat!("NekoPak/", env!("CARGO_PKG_VERSION")));
    #[cfg(windows)]
    let builder = builder.use_preconfigured_tls(
        native_tls::TlsConnector::builder()
            .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
            .build()
            .map_err(|_| network())?,
    );
    #[cfg(not(windows))]
    let builder = builder.min_tls_version(reqwest::tls::Version::TLS_1_2);
    Ok(builder)
}
pub(super) fn public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v) => {
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_unspecified()
                && !v.is_multicast()
                && !v.is_broadcast()
                && !v.is_documentation()
                && v.octets()[0] != 0
                && v.octets()[0] < 224
                && !(v.octets()[0] == 100 && (64..=127).contains(&v.octets()[1]))
        }
        std::net::IpAddr::V6(v) => v.to_ipv4_mapped().map(|v| public_ip(v.into())).unwrap_or(
            !v.is_loopback()
                && !v.is_unspecified()
                && !v.is_multicast()
                && !v.is_unique_local()
                && !v.is_unicast_link_local(),
        ),
    }
}
