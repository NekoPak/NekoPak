//! Resumable OSS uploads. Service-issued credentials live only in the OS vault.
use super::*;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::{path::Path, time::Duration};
use tokio::io::AsyncReadExt;

const PART_BYTES: u64 = 8 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
struct UploadSession {
    scope: String,
    remote_id: String,
    total: u64,
    gcid: String,
    endpoint: String,
    bucket: String,
    key: String,
    access_key_id: String,
    access_key_secret: String,
    security_token: String,
    upload_id: Option<String>,
    expires_at: Option<i64>,
    completing: bool,
}
impl Drop for UploadSession {
    fn drop(&mut self) {
        self.access_key_secret.zeroize();
        self.security_token.zeroize();
        self.access_key_id.zeroize();
    }
}

impl NativeProvider {
    pub async fn native_upload_prepare(
        &self,
        local_path: &str,
        parent: &str,
        scope: &str,
    ) -> Result<UploadPrepared, ClientError> {
        wire::validate_id(parent)?;
        let epoch = self.checked_scope(scope).await?;
        let path = Path::new(local_path);
        validate_local(path)?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control))
            .ok_or_else(wire::invalid)?;
        let (total, gcid) = local_gcid(path).await?;
        let body = json!({"kind":"drive#file","name":name,"size":total.to_string(),"hash":gcid,"upload_type":"UPLOAD_TYPE_RESUMABLE","objProvider":{"provider":"UPLOAD_TYPE_UNKNOWN"},"parent_id":if parent=="root"{""}else{parent},"folder_type":""});
        let (value, _) = self
            .drive_request_at(
                Method::POST,
                &format!("{}/drive/v1/files", self.http.drive_origin),
                Some(&body),
                Some(epoch),
            )
            .await
            .map_err(crate::cloud::submitted_error)?;
        let file = value.get("file").ok_or_else(wire::invalid)?;
        let remote_id = wire::text(file, "id", 256)?;
        wire::validate_id(&remote_id)?;
        if file.get("phase").and_then(Value::as_str) == Some("PHASE_TYPE_COMPLETE") {
            self.confirm_uploaded(&remote_id, total, &gcid, epoch)
                .await?;
            return Ok(UploadPrepared {
                upload_ref: String::new(),
                remote_id,
                total_bytes: DecimalU64(total),
                gcid,
                completed: true,
                part_bytes: PART_BYTES,
            });
        }
        let params = value
            .get("resumable")
            .and_then(|v| v.get("params"))
            .ok_or_else(wire::invalid)?;
        let expiry = params
            .get("expiration")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| {
                OffsetDateTime::parse(s, &Rfc3339)
                    .map(|v| v.unix_timestamp())
                    .map_err(|_| wire::invalid())
            })
            .transpose()?;
        let plan = UploadSession {
            scope: scope.into(),
            remote_id: remote_id.clone(),
            total,
            gcid: gcid.clone(),
            endpoint: wire::text(params, "endpoint", 2048)?,
            bucket: wire::text(params, "bucket", 256)?,
            key: wire::text(params, "key", 2048)?,
            access_key_id: wire::text(params, "access_key_id", 512)?,
            access_key_secret: wire::text(params, "access_key_secret", 1024)?,
            security_token: wire::text(params, "security_token", 4096)?,
            upload_id: None,
            expires_at: expiry,
            completing: false,
        };
        validate_plan(&plan)?;
        let upload_ref = uuid::Uuid::new_v4().to_string();
        {
            let session = self.session.lock().await;
            if session.epoch != epoch {
                return Err(expired());
            }
            self.save_upload(&upload_ref, &plan)?;
        }
        // Persist the unstarted receipt before contacting OSS. Missing UploadId
        // after a network loss is unknown; it must never silently initiate twice.
        let response = self
            .oss_request(&plan, Method::POST, "uploads", Vec::new())
            .await
            .map_err(crate::cloud::submitted_error)?;
        let raw = response_body(response).await?;
        let upload_id = xml_text(&raw, "UploadId")
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 256
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
            .ok_or_else(wire::invalid)?;
        let mut plan = plan;
        plan.upload_id = Some(upload_id);
        {
            let session = self.session.lock().await;
            if session.epoch != epoch {
                return Err(expired());
            }
            self.save_upload(&upload_ref, &plan)?;
        }
        Ok(UploadPrepared {
            upload_ref,
            remote_id,
            total_bytes: DecimalU64(total),
            gcid,
            completed: false,
            part_bytes: PART_BYTES,
        })
    }
    pub async fn native_upload_open(
        &self,
        upload_ref: &str,
        scope: &str,
    ) -> Result<UploadPrepared, ClientError> {
        let epoch = self.checked_scope(scope).await?;
        let plan = self.load_upload(upload_ref, scope)?;
        let completed = match self
            .confirm_uploaded(&plan.remote_id, plan.total, &plan.gcid, epoch)
            .await
        {
            Ok(_) => true,
            Err(failure) if failure.code == ErrorCode::Conflict => false,
            Err(failure) => return Err(failure),
        };
        if !completed && plan.upload_id.is_none() {
            return Err(error(
                ErrorCode::Conflict,
                "上传初始化结果未知，保留远端任务，不会重新创建。",
            ));
        }
        Ok(UploadPrepared {
            upload_ref: upload_ref.into(),
            remote_id: plan.remote_id.clone(),
            total_bytes: DecimalU64(plan.total),
            gcid: plan.gcid.clone(),
            completed,
            part_bytes: PART_BYTES,
        })
    }
    pub async fn native_upload_part(
        &self,
        upload_ref: &str,
        part_number: u32,
        bytes: Vec<u8>,
        scope: &str,
    ) -> Result<String, ClientError> {
        let epoch = self.checked_scope(scope).await?;
        let plan = self.load_upload(upload_ref, scope)?;
        if plan.completing {
            return Err(error(
                ErrorCode::Conflict,
                "上传正在确认完成，不能再写分片。",
            ));
        }
        let parts = plan.total.div_ceil(PART_BYTES).max(1);
        if part_number == 0 || u64::from(part_number) > parts || parts > 10000 {
            return Err(wire::invalid());
        }
        let start = u64::from(part_number - 1) * PART_BYTES;
        let expected = plan.total.saturating_sub(start).min(PART_BYTES);
        if bytes.len() as u64 != expected {
            return Err(error(
                ErrorCode::IntegrityFailed,
                "上传分片长度不匹配，未发送。",
            ));
        }
        let upload_id = plan
            .upload_id
            .as_deref()
            .ok_or_else(|| error(ErrorCode::Conflict, "上传初始化尚未确认。"))?;
        let response = self
            .oss_request(
                &plan,
                Method::PUT,
                &format!("partNumber={part_number}&uploadId={upload_id}"),
                bytes,
            )
            .await?;
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(wire::invalid)?
            .trim_matches('"')
            .to_owned();
        if etag.len() != 32 || !etag.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(wire::invalid());
        }
        response_body(response).await?;
        if self.session.lock().await.epoch != epoch {
            return Err(expired());
        }
        Ok(etag)
    }
    pub async fn native_upload_finish(
        &self,
        upload_ref: &str,
        etags: &[String],
        scope: &str,
    ) -> Result<RemoteEntry, ClientError> {
        let epoch = self.checked_scope(scope).await?;
        let mut plan = self.load_upload(upload_ref, scope)?;
        if plan.completing {
            let entry = self
                .confirm_uploaded(&plan.remote_id, plan.total, &plan.gcid, epoch)
                .await?;
            return Ok(entry);
        }
        if etags.len() as u64 != plan.total.div_ceil(PART_BYTES).max(1)
            || etags
                .iter()
                .any(|s| s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(wire::invalid());
        }
        let mut xml = String::from("<CompleteMultipartUpload>");
        for (index, etag) in etags.iter().enumerate() {
            xml.push_str(&format!(
                "<Part><PartNumber>{}</PartNumber><ETag>\"{}\"</ETag></Part>",
                index + 1,
                etag
            ));
        }
        xml.push_str("</CompleteMultipartUpload>");
        let upload_id = plan.upload_id.clone().ok_or_else(wire::invalid)?;
        plan.completing = true;
        {
            let session = self.session.lock().await;
            if session.epoch != epoch {
                return Err(expired());
            }
            self.save_upload(upload_ref, &plan)?;
        }
        let response = self
            .oss_request(
                &plan,
                Method::POST,
                &format!("uploadId={upload_id}"),
                xml.into_bytes(),
            )
            .await
            .map_err(crate::cloud::submitted_error)?;
        let text = response_body(response).await?;
        if !text.contains("<CompleteMultipartUploadResult") || text.contains("<Error") {
            return Err(crate::cloud::submitted_error(wire::invalid()));
        }
        for _ in 0..10 {
            match self
                .confirm_uploaded(&plan.remote_id, plan.total, &plan.gcid, epoch)
                .await
            {
                Ok(entry) => return Ok(entry),
                Err(failure) if failure.code == ErrorCode::Conflict => {
                    tokio::time::sleep(Duration::from_millis(400)).await
                }
                Err(failure) => return Err(failure),
            }
        }
        Err(error(
            ErrorCode::Conflict,
            "上传字节已提交，服务仍在处理；请稍后查询，不会重新上传。",
        ))
    }
    pub async fn native_upload_forget(
        &self,
        upload_ref: &str,
        scope: &str,
    ) -> Result<(), ClientError> {
        let epoch = self.checked_scope(scope).await?;
        let plan = self.load_upload(upload_ref, scope)?;
        if let Some(id) = &plan.upload_id {
            if !plan.completing {
                let response = self
                    .oss_request(&plan, Method::DELETE, &format!("uploadId={id}"), Vec::new())
                    .await?;
                response_body(response).await?;
            }
        }
        let session = self.session.lock().await;
        if session.epoch != epoch {
            return Err(expired());
        }
        self.vault.delete_aux(upload_ref)
    }
    fn save_upload(&self, upload_ref: &str, plan: &UploadSession) -> Result<(), ClientError> {
        let text = Zeroizing::new(serde_json::to_string(plan).map_err(|_| wire::invalid())?);
        self.vault.save_aux(upload_ref, &text)
    }
    fn load_upload(&self, upload_ref: &str, scope: &str) -> Result<UploadSession, ClientError> {
        let text = self.vault.load_aux(upload_ref)?.ok_or_else(|| {
            error(
                ErrorCode::Conflict,
                "上传凭据已清除或不可用，未创建重复上传。",
            )
        })?;
        let plan: UploadSession = serde_json::from_str(&text).map_err(|_| wire::invalid())?;
        validate_plan(&plan)?;
        if plan.scope != scope {
            return Err(expired());
        }
        Ok(plan)
    }
    async fn confirm_uploaded(
        &self,
        id: &str,
        total: u64,
        gcid: &str,
        epoch: u64,
    ) -> Result<RemoteEntry, ClientError> {
        let (value, _) = self
            .drive_request_at(
                Method::GET,
                &format!("{}/drive/v1/files/{id}", self.http.drive_origin),
                None,
                Some(epoch),
            )
            .await?;
        if value.get("phase").and_then(Value::as_str) != Some("PHASE_TYPE_COMPLETE") {
            return Err(error(ErrorCode::Conflict, "远端上传仍在处理中。"));
        }
        let entry = wire::entry(&value)?;
        if entry.id != id
            || entry.size_bytes.0 != total
            || value
                .get("hash")
                .and_then(Value::as_str)
                .is_none_or(|s| !s.eq_ignore_ascii_case(gcid))
        {
            return Err(error(
                ErrorCode::IntegrityFailed,
                "远端上传大小或内容标识不匹配。",
            ));
        }
        Ok(entry)
    }
    async fn oss_request(
        &self,
        plan: &UploadSession,
        method: Method,
        query: &str,
        body: Vec<u8>,
    ) -> Result<reqwest::Response, ClientError> {
        if plan.expires_at.is_some_and(|t| t <= now()) {
            return Err(error(
                ErrorCode::AuthExpired,
                "临时上传授权已到期，已保留任务和远端文件；不会自动重新创建。",
            ));
        }
        let mut url = url::Url::parse(&if plan.endpoint.contains("://") {
            plan.endpoint.clone()
        } else {
            format!("https://{}", plan.endpoint)
        })
        .map_err(|_| wire::invalid())?;
        let test = cfg!(debug_assertions)
            && self.http.drive_origin.starts_with("http://127.0.0.1:")
            && url.host_str() == Some("127.0.0.1");
        if !test && (url.scheme() != "https" || url.port_or_known_default() != Some(443)) {
            return Err(wire::invalid());
        }
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(wire::invalid());
        }
        let host = url.host_str().ok_or_else(wire::invalid)?.to_owned();
        let port = url.port_or_known_default().ok_or_else(wire::invalid)?;
        let addresses: Vec<_> = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|_| upload_network())?
            .collect();
        if addresses.is_empty()
            || (!test
                && addresses
                    .iter()
                    .any(|a| !crate::download::public_ip(a.ip())))
        {
            return Err(error(
                ErrorCode::PermissionDenied,
                "上传目标不是公共HTTPS服务。",
            ));
        }
        url.path_segments_mut()
            .map_err(|_| wire::invalid())?
            .clear()
            .extend(plan.key.split('/'));
        url.set_query(Some(query));
        self.http.check_url(url.as_str())?;
        let date = httpdate::fmt_http_date(std::time::SystemTime::now());
        let canonical = format!(
            "{method}\n\napplication/octet-stream\n{date}\nx-oss-security-token:{}\n/{}/{}?{query}",
            plan.security_token, plan.bucket, plan.key
        );
        let mut mac = Hmac::<Sha1>::new_from_slice(plan.access_key_secret.as_bytes())
            .map_err(|_| wire::invalid())?;
        mac.update(canonical.as_bytes());
        let signature =
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        let mut authorization = reqwest::header::HeaderValue::from_str(&format!(
            "OSS {}:{signature}",
            plan.access_key_id
        ))
        .map_err(|_| wire::invalid())?;
        authorization.set_sensitive(true);
        let mut token = reqwest::header::HeaderValue::from_str(&plan.security_token)
            .map_err(|_| wire::invalid())?;
        token.set_sensitive(true);
        let builder =
            crate::download::download_client_builder()?.resolve_to_addrs(&host, &addresses);
        let client = if test {
            builder.no_proxy()
        } else {
            self.http.cdn_proxy(builder)?
        }
        .build()
        .map_err(|_| upload_network())?;
        let response = client
            .request(method, url)
            .header(reqwest::header::AUTHORIZATION, authorization)
            .header("x-oss-security-token", token)
            .header("date", date)
            .header("content-type", "application/octet-stream")
            .body(body)
            .send()
            .await
            .map_err(|_| upload_network())?;
        if !response.status().is_success() {
            return Err(if response.status().as_u16() == 403 {
                error(ErrorCode::AuthExpired, "临时上传授权失效，请核对任务状态。")
            } else {
                upload_network()
            });
        }
        Ok(response)
    }
}
fn validate_plan(plan: &UploadSession) -> Result<(), ClientError> {
    wire::validate_id(&plan.remote_id)?;
    if plan.bucket.is_empty()
        || !plan
            .bucket
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || plan.key.is_empty()
        || plan.key.starts_with('/')
        || plan
            .key
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        || plan
            .key
            .chars()
            .any(|c| c.is_control() || matches!(c, '?' | '#' | '\\'))
        || !valid_token(&plan.access_key_id)
        || !valid_token(&plan.access_key_secret)
        || !valid_token(&plan.security_token)
        || plan.gcid.len() != 40
        || !plan.gcid.bytes().all(|b| b.is_ascii_hexdigit())
        || plan.total.div_ceil(PART_BYTES) > 10000
    {
        return Err(wire::invalid());
    }
    Ok(())
}
fn validate_local(path: &Path) -> Result<(), ClientError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(error(ErrorCode::Conflict, "请选择本地绝对文件路径。"));
    }
    for p in path.ancestors() {
        let metadata = std::fs::symlink_metadata(p)
            .map_err(|_| error(ErrorCode::PermissionDenied, "无法读取上传文件。"))?;
        if metadata.file_type().is_symlink() {
            return Err(error(ErrorCode::PermissionDenied, "不上传符号链接。"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(error(ErrorCode::PermissionDenied, "不上传重解析点。"));
            }
        }
    }
    Ok(())
}
async fn local_gcid(path: &Path) -> Result<(u64, String), ClientError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|_| upload_local())?;
    let before = file.metadata().await.map_err(|_| upload_local())?;
    if !before.is_file() {
        return Err(upload_local());
    }
    let total = before.len();
    let block = if total <= 128 * 1024 * 1024 {
        256 * 1024
    } else if total <= 256 * 1024 * 1024 {
        512 * 1024
    } else if total <= 512 * 1024 * 1024 {
        1024 * 1024
    } else {
        2 * 1024 * 1024
    };
    let mut buffer = vec![0; block];
    let mut remaining = total;
    let mut hash = Sha1::new();
    while remaining > 0 {
        let take = (remaining as usize).min(block);
        file.read_exact(&mut buffer[..take])
            .await
            .map_err(|_| upload_local())?;
        hash.update(Sha1::digest(&buffer[..take]));
        remaining -= take as u64;
        tokio::task::yield_now().await;
    }
    let after = file.metadata().await.map_err(|_| upload_local())?;
    if after.len() != total || before.modified().ok() != after.modified().ok() {
        return Err(error(ErrorCode::Conflict, "上传源文件已变化，请重新选择。"));
    }
    Ok((total, format!("{:X}", hash.finalize())))
}
fn upload_local() -> ClientError {
    error(
        ErrorCode::PermissionDenied,
        "无法读取完整上传文件，未确认成功。",
    )
}
fn upload_network() -> ClientError {
    error(
        ErrorCode::TransientNetwork,
        "上传连接失败，已保留已确认分片。",
    )
}
async fn response_body(mut response: reqwest::Response) -> Result<String, ClientError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| upload_network())? {
        if body.len() + chunk.len() > 1024 * 1024 {
            return Err(wire::invalid());
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| wire::invalid())
}
fn xml_text(text: &str, name: &str) -> Option<String> {
    let start = text.find(&format!("<{name}>"))? + name.len() + 2;
    let tail = &text[start..];
    let end = tail.find(&format!("</{name}>"))?;
    let value = &tail[..end];
    if value.contains(['<', '>', '&']) {
        None
    } else {
        Some(value.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::tests::Server;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex as StdMutex,
    };
    use tokio::{io::AsyncWriteExt, net::TcpListener};
    #[tokio::test]
    async fn multipart_upload_sends_real_bytes_and_confirms_remote_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upload.txt");
        let bytes = b"real synthetic upload bytes";
        std::fs::write(&path, bytes).unwrap();
        let (total, gcid) = local_gcid(&path).await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let oss_origin = format!("http://{}", listener.local_addr().unwrap());
        let complete = Arc::new(AtomicBool::new(false));
        let finished = complete.clone();
        let received = Arc::new(StdMutex::new(Vec::<u8>::new()));
        let received_clone = received.clone();
        let oss = tokio::spawn(async move {
            for _ in 0..3 {
                let (mut s, _) = listener.accept().await.unwrap();
                let mut all = Vec::new();
                let header_end = loop {
                    let mut b = [0; 4096];
                    let n = s.read(&mut b).await.unwrap();
                    assert_ne!(n, 0);
                    all.extend_from_slice(&b[..n]);
                    if let Some(i) = all.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let h = String::from_utf8(all[..header_end].to_vec()).unwrap();
                assert!(h
                    .to_lowercase()
                    .contains("authorization: oss synthetic-key:"));
                assert!(!h.contains("Bearer"));
                assert!(!h.to_lowercase().contains("x-captcha-token"));
                assert!(!h.to_lowercase().contains("x-device-id"));
                let len = h
                    .lines()
                    .filter_map(|l| l.split_once(':'))
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                    .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                    .unwrap_or(0);
                while all.len() < header_end + len {
                    let mut b = [0; 4096];
                    let n = s.read(&mut b).await.unwrap();
                    all.extend_from_slice(&b[..n]);
                }
                let body = &all[header_end..header_end + len];
                let (first, etag, text) = if h.starts_with("POST /object?uploads ") {
                    ("200 OK","","<InitiateMultipartUploadResult><UploadId>upload-1</UploadId></InitiateMultipartUploadResult>")
                } else if h.starts_with("PUT /object?partNumber=1&uploadId=upload-1 ") {
                    received_clone.lock().unwrap().extend_from_slice(body);
                    (
                        "200 OK",
                        "ETag: \"0123456789abcdef0123456789abcdef\"\r\n",
                        "",
                    )
                } else {
                    assert!(h.starts_with("POST /object?uploadId=upload-1 "));
                    assert!(String::from_utf8_lossy(body).contains("<PartNumber>1</PartNumber>"));
                    finished.store(true, Ordering::SeqCst);
                    ("200 OK","","<CompleteMultipartUploadResult><ETag>all</ETag></CompleteMultipartUploadResult>")
                };
                s.write_all(format!("HTTP/1.1 {first}\r\nContent-Length: {}\r\n{etag}Connection: close\r\n\r\n{text}",text.len()).as_bytes()).await.unwrap();
            }
        });
        let hash = gcid.clone();
        let server=Server::new(move|r|match(r.method.as_str(),r.path.split('?').next().unwrap()){
            ("POST","/drive/v1/files")=>{assert_eq!(r.body["size"],total.to_string());assert_eq!(r.body["hash"],hash);(200,json!({"file":{"id":"uploaded-1","phase":"PHASE_TYPE_PENDING"},"resumable":{"params":{"endpoint":oss_origin,"bucket":"synthetic-bucket","key":"object","access_key_id":"synthetic-key","access_key_secret":"synthetic-secret","security_token":"synthetic-security-token"}}}))},
            ("GET","/drive/v1/files/uploaded-1")=>(200,json!({"id":"uploaded-1","name":"upload.txt","kind":"drive#file","size":total.to_string(),"hash":hash,"parent_id":"","phase":if complete.load(Ordering::SeqCst){"PHASE_TYPE_COMPLETE"}else{"PHASE_TYPE_PENDING"}})),_ => (404,json!({"error":"file_not_found"}))}).await;
        let p = server.provider().await;
        let scope = p.account_scope().await.unwrap();
        let prepared = p
            .native_upload_prepare(path.to_str().unwrap(), "root", &scope)
            .await
            .unwrap();
        assert!(!prepared.completed);
        let restored = p
            .native_upload_open(&prepared.upload_ref, &scope)
            .await
            .unwrap();
        assert_eq!(restored.gcid, gcid);
        assert!(p
            .native_upload_part(&prepared.upload_ref, 1, vec![0], &scope)
            .await
            .is_err());
        let etag = p
            .native_upload_part(&prepared.upload_ref, 1, bytes.to_vec(), &scope)
            .await
            .unwrap();
        let entry = p
            .native_upload_finish(&prepared.upload_ref, &[etag], &scope)
            .await
            .unwrap();
        assert_eq!(entry.size_bytes.0, total);
        assert_eq!(*received.lock().unwrap(), bytes);
        assert!(p.vault.load_aux(&prepared.upload_ref).unwrap().is_some());
        assert!(
            p.native_upload_open(&prepared.upload_ref, &scope)
                .await
                .unwrap()
                .completed
        );
        p.native_upload_forget(&prepared.upload_ref, &scope)
            .await
            .unwrap();
        assert!(p.vault.load_aux(&prepared.upload_ref).unwrap().is_none());
        oss.await.unwrap();
    }
    #[tokio::test]
    async fn gcid_uses_block_digests_not_a_flat_file_hash() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("gcid.bin");
        let data = vec![7; 300000];
        std::fs::write(&p, &data).unwrap();
        let (_, hash) = local_gcid(&p).await.unwrap();
        let mut expected = Sha1::new();
        expected.update(Sha1::digest(&data[..262144]));
        expected.update(Sha1::digest(&data[262144..]));
        assert_eq!(hash, format!("{:X}", expected.finalize()));
        assert_ne!(hash, format!("{:X}", Sha1::digest(&data)));
    }
}
