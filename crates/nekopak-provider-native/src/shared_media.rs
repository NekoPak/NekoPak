//! Read shared originals without restoring them into the account.
//! Independent protocol facts: docs/SHARED_MEDIA_PROTOCOL_RESEARCH.md.
use super::*;
use nekopak_provider::{DownloadChunk, DownloadInfo, DownloadObserver};

pub(super) fn shared_id(id: &str) -> Result<Option<(&str, &str)>, ClientError> {
    let Some(raw) = id.strip_prefix("shared:") else {
        return Ok(None);
    };
    let (reference, remote) = raw.split_once(':').ok_or_else(wire::invalid)?;
    let reference_id = reference.strip_prefix("share-").unwrap_or(reference);
    let parsed = uuid::Uuid::parse_str(reference_id).map_err(|_| wire::invalid())?;
    if parsed.to_string() != reference_id {
        return Err(wire::invalid());
    }
    wire::validate_id(remote)?;
    Ok(Some((reference, remote)))
}

impl NativeProvider {
    pub(super) async fn encoded_share_info(&self, id: &str) -> Result<DownloadInfo, ClientError> {
        let (reference, remote) = shared_id(id)?.ok_or_else(wire::invalid)?;
        let mut info = self.native_share_source_info(reference, remote).await?;
        info.entry.id = id.into();
        // The owner's actual parent is not a navigable folder of this account.
        info.entry.parent_id = None;
        Ok(info)
    }
    async fn shared_metadata(
        &self,
        reference: &str,
        id: &str,
    ) -> Result<(DownloadInfo, Zeroizing<String>, u64), ClientError> {
        wire::validate_id(id)?;
        let (share_id, _, token, epoch) = self.share_access(reference).await?;
        let snapshot = self.snapshot().await?;
        if snapshot.epoch != epoch {
            return Err(expired());
        }
        let subject = self.drive_subject(&snapshot).await?;
        let scope = format!(
            "{:x}",
            Sha256::digest(format!("{}\0{}", self.profile()?.id, subject))
        );
        let mut url = url::Url::parse(&format!(
            "{}/drive/v1/share/file_info",
            self.http.drive_origin
        ))
        .map_err(|_| wire::invalid())?;
        url.query_pairs_mut()
            .append_pair("share_id", &share_id)
            .append_pair("file_id", id)
            .append_pair("pass_code_token", &token);
        let (value, _) = self
            .drive_request_at(Method::GET, url.as_str(), None, Some(epoch))
            .await?;
        shares::check_share_status(&value, true)?;
        let file = value.get("file_info").ok_or_else(wire::invalid)?;
        let entry = wire::entry(file)?;
        if entry.id != id || entry.kind != EntryKind::File {
            return Err(error(ErrorCode::Conflict, "请选择分享中的文件。"));
        }
        // Use only the original representation. Permission or quota restrictions
        // are not bypassed by taking a different link from the response.
        if file
            .get("medias")
            .and_then(Value::as_array)
            .is_some_and(|rows| {
                rows.iter().any(|media| {
                    media["is_origin"] == true
                        && (media["is_visible"] == false || media["need_more_quota"] == true)
                })
            })
        {
            return Err(error(
                ErrorCode::PermissionDenied,
                "当前账号无法读取该分享文件的原始内容。",
            ));
        }
        let version = entry
            .hash
            .as_ref()
            .map(|h| h.value.as_str())
            .or(entry.modified_at.as_deref())
            .ok_or_else(|| {
                error(
                    ErrorCode::IntegrityFailed,
                    "分享未提供可核对的内容版本，暂不能安全读取。",
                )
            })?;
        let location =
            if entry.size_bytes.0 == 0 {
                Zeroizing::new(String::new())
            } else {
                Zeroizing::new(wire::text(file, "web_content_link", 16384).map_err(|_| {
                    error(ErrorCode::Unsupported, "此分享没有提供可读取的原文件链接。")
                })?)
            };
        let identity = format!(
            "{:x}",
            Sha256::digest(format!(
                "share-original\0{share_id}\0{id}\0{}\0{version}",
                entry.size_bytes.0
            ))
        );
        if self.session.lock().await.epoch != epoch {
            return Err(expired());
        }
        Ok((
            DownloadInfo {
                entry,
                identity,
                account_scope: scope,
            },
            location,
            epoch,
        ))
    }

    pub(super) async fn native_share_source_info(
        &self,
        reference: &str,
        id: &str,
    ) -> Result<DownloadInfo, ClientError> {
        Ok(self.shared_metadata(reference, id).await?.0)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn native_share_chunk(
        &self,
        reference: &str,
        id: &str,
        identity: &str,
        scope: &str,
        start: u64,
        length: u64,
        observer: DownloadObserver,
    ) -> Result<DownloadChunk, ClientError> {
        if length == 0 || length > 4 * 1024 * 1024 {
            return Err(wire::invalid());
        }
        let epoch = self.checked_scope(scope).await?;
        let (info, location, read_epoch) = self.shared_metadata(reference, id).await?;
        if epoch != read_epoch || info.account_scope != scope || info.identity != identity {
            return Err(error(
                ErrorCode::IntegrityFailed,
                "分享内容或账号已变化，未继续拼接数据。",
            ));
        }
        if start >= info.entry.size_bytes.0 {
            return Err(wire::invalid());
        }
        let take = length.min(info.entry.size_bytes.0 - start);
        let (result, _) = self
            .fetch_range(
                &location,
                Some(info.entry.size_bytes.0),
                start,
                take,
                None,
                observer,
            )
            .await?;
        if self.session.lock().await.epoch != epoch {
            return Err(expired());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::tests::Server;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn metadata(location: &str, hash: &str) -> Value {
        let gcid = if hash == "changed" {
            "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"
        } else {
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        };
        json!({"share_status":"OK","file_info":{"id":"shared-file","name":"shared.bin","kind":"drive#file","size":"12","hash":gcid,"phase":"PHASE_TYPE_COMPLETE","web_content_link":location}})
    }

    #[tokio::test]
    async fn encoded_share_reads_preserve_source_ids_and_reject_writes_or_quality_substitution() {
        let server = Server::new(|request| match request.path.split('?').next().unwrap() {
            "/drive/v1/share" => (200, json!({"share_status":"OK"})),
            "/drive/v1/share/detail" => (200, json!({"share_status":"OK","files":[]})),
            "/drive/v1/share/file_info" => {
                (200, metadata("https://cdn.invalid/original", "original"))
            }
            _ => (404, json!({"error":"file_not_found"})),
        })
        .await;
        let provider = server.provider().await;
        let share = provider.share_open("share-one", "").await.unwrap();
        let retained = provider.share_retain(&share.access_ref).await.unwrap();
        let id = format!("shared:{retained}:shared-file");
        let info = provider.download_info(&id).await.unwrap();
        assert_eq!(info.entry.id, id);
        assert_eq!(info.entry.parent_id, None);
        assert_eq!(provider.stat(&id).await.unwrap().id, id);
        assert_eq!(
            provider.media_info(&id, "original").await.unwrap().identity,
            info.identity
        );
        assert_eq!(provider.media_options(&id).await.unwrap()[0].id, "original");
        assert_eq!(
            provider.media_info(&id, "1080p").await.err().unwrap().code,
            ErrorCode::Unsupported
        );
        assert!(provider
            .mutate_files(
                &FileMutation {
                    op: MutateOp::Trash,
                    ids: vec![id],
                    target: None
                },
                &info.account_scope
            )
            .await
            .is_err());
        assert!(server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path.starts_with("/drive/"))
            .all(|r| r.method == "GET"));
        assert!(shared_id("shared:bad:remote").is_err());
        assert!(shared_id(&format!("shared:{retained}:one:two")).is_err());
    }

    #[tokio::test]
    async fn shared_original_reads_real_ranges_without_restoring_or_forwarding_api_secrets() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let location = format!(
            "http://{}/shared?signature=private-cdn-signature",
            listener.local_addr().unwrap()
        );
        let cdn = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buf = [0; 2048];
                let n = stream.read(&mut buf).await.unwrap();
                assert_ne!(n, 0);
                bytes.extend_from_slice(&buf[..n]);
                if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let headers = String::from_utf8(bytes).unwrap().to_ascii_lowercase();
            assert!(headers.contains("range: bytes=3-7"));
            for private in [
                "authorization:",
                "x-captcha",
                "x-device",
                "x-client",
                "pass_code_token",
                "private-share-token",
            ] {
                assert!(!headers.contains(private));
            }
            stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 3-7/12\r\nConnection: close\r\n\r\ndefgh").await.unwrap();
        });
        let server = Server::new(move |request| match request.path.split('?').next().unwrap() {
            "/drive/v1/share" => (200,json!({"share_status":"OK","pass_code_token":"private-share-token","title":"Shared"})),
            "/drive/v1/share/detail" => (200,json!({"share_status":"OK","files":[{"id":"shared-file","name":"shared.bin","kind":"drive#file","size":"12"}]})),
            "/drive/v1/share/file_info" => {
                let parsed = url::Url::parse(&format!("http://localhost{}",request.path)).unwrap();
                let query: BTreeMap<_,_> = parsed.query_pairs().into_owned().collect();
                assert_eq!(query.get("share_id").unwrap(), "share-one");
                assert_eq!(query.get("file_id").unwrap(), "shared-file");
                assert_eq!(query.get("pass_code_token").unwrap(), "private-share-token");
                (200,metadata(&location, "content-version-one"))
            },
            _ => (404,json!({"error":"file_not_found"})),
        }).await;
        let provider = server.provider().await;
        let share = provider.share_open("share-one", "abcd").await.unwrap();
        let info = provider
            .share_source_info(&share.access_ref, "shared-file")
            .await
            .unwrap();
        let public = serde_json::to_string(&info.entry).unwrap();
        for private in [
            "private-cdn",
            "web_content_link",
            "private-share-token",
            "signature",
        ] {
            assert!(!public.contains(private));
        }
        let chunk = provider
            .share_chunk(
                &share.access_ref,
                "shared-file",
                &info.identity,
                &info.account_scope,
                3,
                5,
                DownloadObserver::default(),
            )
            .await
            .unwrap();
        assert_eq!(chunk.bytes, b"defgh");
        assert_eq!(chunk.total, 12);
        assert!(server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path.starts_with("/drive/"))
            .all(|r| r.method == "GET"));
        cdn.await.unwrap();
        provider.logout().await.unwrap();
        assert!(provider
            .share_source_info(&share.access_ref, "shared-file")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn changed_or_revoked_share_is_rejected_before_any_cdn_read() {
        let changed = Arc::new(AtomicBool::new(false));
        let flag = changed.clone();
        let revoked = Arc::new(AtomicBool::new(false));
        let revoke = revoked.clone();
        let server = Server::new(
            move |request| match request.path.split('?').next().unwrap() {
                "/drive/v1/share" => (
                    200,
                    json!({"share_status":"OK","pass_code_token":"private-share-token"}),
                ),
                "/drive/v1/share/detail" => (200, json!({"files":[]})),
                "/drive/v1/share/file_info" if revoke.load(Ordering::SeqCst) => {
                    (200, json!({"share_status":"DELETED","file_info":{}}))
                }
                "/drive/v1/share/file_info" => (
                    200,
                    metadata(
                        "https://cdn.invalid/file?token=secret",
                        if flag.load(Ordering::SeqCst) {
                            "changed"
                        } else {
                            "original"
                        },
                    ),
                ),
                _ => (404, json!({"error":"file_not_found"})),
            },
        )
        .await;
        let provider = server.provider().await;
        let share = provider.share_open("share-one", "").await.unwrap();
        let info = provider
            .share_source_info(&share.access_ref, "shared-file")
            .await
            .unwrap();
        changed.store(true, Ordering::SeqCst);
        assert_eq!(
            provider
                .share_chunk(
                    &share.access_ref,
                    "shared-file",
                    &info.identity,
                    &info.account_scope,
                    0,
                    1,
                    DownloadObserver::default()
                )
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::IntegrityFailed
        );
        revoked.store(true, Ordering::SeqCst);
        assert_eq!(
            provider
                .share_source_info(&share.access_ref, "shared-file")
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::PermissionDenied
        );
        assert!(server
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|r| !r.path.contains("restore")));
    }

    #[tokio::test]
    async fn restricted_original_and_missing_direct_link_do_not_select_a_transcode() {
        let restricted = Arc::new(AtomicBool::new(true));
        let flag = restricted.clone();
        let server = Server::new(move |request| match request.path.split('?').next().unwrap() {
            "/drive/v1/share" => (200,json!({"share_status":"OK"})),
            "/drive/v1/share/detail" => (200,json!({"files":[]})),
            "/drive/v1/share/file_info" => {
                let mut value = metadata("https://cdn.invalid/forbidden", "original");
                if flag.load(Ordering::SeqCst) {
                    value["file_info"]["medias"] = json!([{"is_origin":true,"is_visible":false,"need_more_quota":true,"link":{"url":"https://cdn.invalid/forbidden"}}]);
                } else {
                    value["file_info"].as_object_mut().unwrap().remove("web_content_link");
                    value["file_info"]["medias"] = json!([{"is_origin":false,"is_visible":true,"link":{"url":"https://cdn.invalid/transcode"}}]);
                }
                (200,value)
            },
            _ => (404,json!({"error":"file_not_found"})),
        }).await;
        let provider = server.provider().await;
        let share = provider.share_open("share-one", "").await.unwrap();
        assert_eq!(
            provider
                .share_source_info(&share.access_ref, "shared-file")
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::PermissionDenied
        );
        restricted.store(false, Ordering::SeqCst);
        assert_eq!(
            provider
                .share_source_info(&share.access_ref, "shared-file")
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::Unsupported
        );
    }
}
