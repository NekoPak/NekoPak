//! Cloud archive browse/extract protocol, independently implemented from public request facts.
use super::*;

pub(super) struct ArchiveAccess {
    pub epoch: u64,
    pub expires_at: i64,
    file_id: String,
    password: Zeroizing<String>,
}

fn archive_path(path: &str) -> Result<(), ClientError> {
    if path.len() > 8192
        || path.starts_with('/')
        || path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
        || path.split('/').any(|p| p == ".." || p == ".")
    {
        return Err(error(ErrorCode::Conflict, "压缩包路径无效。"));
    }
    Ok(())
}
impl NativeProvider {
    async fn archive_metadata(
        &self,
        id: &str,
        epoch: u64,
    ) -> Result<(String, String, String), ClientError> {
        wire::validate_id(id)?;
        let (value, _) = self
            .drive_request_at(
                Method::GET,
                &format!("{}/drive/v1/files/{id}", self.http.drive_origin),
                None,
                Some(epoch),
            )
            .await?;
        let entry = wire::entry(&value)?;
        if entry.id != id || entry.kind != EntryKind::File {
            return Err(wire::invalid());
        }
        let hash = wire::text(&value, "hash", 256)?;
        if !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(wire::invalid());
        }
        Ok((
            hash,
            entry.name,
            entry.parent_id.unwrap_or_else(|| "root".into()),
        ))
    }
    async fn archive_password(
        &self,
        id: &str,
        reference: Option<&str>,
        epoch: u64,
    ) -> Result<Zeroizing<String>, ClientError> {
        let Some(reference) = reference else {
            return Ok(Zeroizing::new(String::new()));
        };
        let session = self.session.lock().await;
        let access = session
            .archives
            .get(reference)
            .filter(|a| a.file_id == id && a.epoch == epoch && a.expires_at > now())
            .ok_or_else(|| error(ErrorCode::Conflict, "压缩包解锁已过期，请重新输入密码。"))?;
        Ok(access.password.clone())
    }
    async fn archive_response(
        &self,
        id: &str,
        path: &str,
        password: &str,
        epoch: u64,
    ) -> Result<(Value, String), ClientError> {
        archive_path(path)?;
        let (hash, name, _) = self.archive_metadata(id, epoch).await?;
        let (value, _) = self
            .drive_request_at(
                Method::POST,
                &format!("{}/decompress/v1/list", self.http.drive_origin),
                Some(&json!({"gcid":hash,"path":path,"file_id":id,"password":password})),
                Some(epoch),
            )
            .await?;
        Ok((value, name))
    }
    pub async fn native_archive_list(
        &self,
        id: &str,
        path: &str,
        password_ref: Option<&str>,
    ) -> Result<ArchiveListing, ClientError> {
        let snapshot = self.snapshot().await?;
        let password = self
            .archive_password(id, password_ref, snapshot.epoch)
            .await?;
        let (value, name) = self
            .archive_response(id, path, &password, snapshot.epoch)
            .await?;
        parse_listing(id, path, &name, &value)
    }
    pub async fn native_archive_unlock(
        &self,
        id: &str,
        password: &str,
    ) -> Result<String, ClientError> {
        if password.is_empty() || password.len() > 1024 || password.contains('\0') {
            return Err(error(ErrorCode::Conflict, "压缩包密码无效。"));
        }
        let snapshot = self.snapshot().await?;
        let (value, name) = self
            .archive_response(id, "", password, snapshot.epoch)
            .await?;
        let listing = parse_listing(id, "", &name, &value)?;
        if listing.needs_password || !listing.supported {
            return Err(error(
                ErrorCode::PermissionDenied,
                "压缩包密码不正确或格式不可读取。",
            ));
        }
        let mut session = self.session.lock().await;
        if session.epoch != snapshot.epoch {
            return Err(expired());
        }
        let epoch = session.epoch;
        session
            .archives
            .retain(|_, a| a.epoch == epoch && a.expires_at > now());
        if session.archives.len() >= 16 {
            return Err(error(
                ErrorCode::Conflict,
                "打开的加密压缩包过多，请稍后重试。",
            ));
        }
        let reference = uuid::Uuid::new_v4().to_string();
        session.archives.insert(
            reference.clone(),
            ArchiveAccess {
                epoch,
                expires_at: now() + 900,
                file_id: id.into(),
                password: Zeroizing::new(password.into()),
            },
        );
        Ok(reference)
    }
    pub async fn native_archive_extract(
        &self,
        id: &str,
        paths: &[String],
        parent: &str,
        password_ref: Option<&str>,
        scope: &str,
    ) -> Result<ArchiveJob, ClientError> {
        let epoch = self.checked_scope(scope).await?;
        let (hash, _, source_parent) = self.archive_metadata(id, epoch).await?;
        if source_parent != parent {
            return Err(error(
                ErrorCode::Unsupported,
                "当前服务解压到压缩包所在目录；请选择该目录，完成后可移动。",
            ));
        }
        if paths.len() > 500 {
            return Err(error(ErrorCode::Conflict, "一次最多选择500个压缩包项目。"));
        }
        for path in paths {
            archive_path(path)?
        }
        let password = self.archive_password(id, password_ref, epoch).await?;
        let mut selected = Vec::new();
        // Resolve selected paths back to service-issued indices and metadata;
        // never let frontend supply indices/gcid or inject another archive's rows.
        let mut folders = BTreeMap::<String, Vec<&String>>::new();
        for path in paths {
            let folder = path.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            folders.entry(folder.into()).or_default().push(path);
        }
        for (folder, paths) in folders {
            let (value, name) = self.archive_response(id, &folder, &password, epoch).await?;
            let listing = parse_listing(id, &folder, &name, &value)?;
            if listing.needs_password || !listing.supported {
                return Err(error(ErrorCode::PermissionDenied, "压缩包需要有效密码。"));
            }
            let rows = value
                .get("files")
                .and_then(Value::as_array)
                .ok_or_else(wire::invalid)?;
            for path in paths {
                let name = path.rsplit('/').next().unwrap_or("");
                let row = rows
                    .iter()
                    .find(|r| r.get("filename").and_then(Value::as_str) == Some(name))
                    .ok_or_else(|| error(ErrorCode::NotFound, "所选压缩包项目已不可用。"))?;
                let index = wire::number(row.get("index").ok_or_else(wire::invalid)?)?;
                selected.push(json!({"index":index.0,"filename":wire::text(row,"filename",1024)?,"filesize":row.get("filesize").cloned().unwrap_or(Value::String("0".into())),"mime_type":row.get("mime_type").cloned().unwrap_or(Value::String(String::new())),"gcid":row.get("gcid").cloned().unwrap_or(Value::String(String::new())),"kind":row.get("kind").cloned().ok_or_else(wire::invalid)?,"path":folder}));
            }
        }
        let(value,_)=self.drive_request_at(Method::POST,&format!("{}/decompress/v1/decompress",self.http.drive_origin),Some(&json!({"gcid":hash,"password":password.as_str(),"file_id":id,"files":selected,"default_parent":true})),Some(epoch)).await.map_err(crate::cloud::submitted_error)?;
        if value.get("status").and_then(Value::as_str) != Some("OK") {
            return Err(error(
                ErrorCode::Conflict,
                "服务未接受解压任务，请核对格式、密码和空间。",
            ));
        }
        let job_id = wire::text(&value, "task_id", 256).map_err(crate::cloud::submitted_error)?;
        wire::validate_id(&job_id)?;
        Ok(ArchiveJob {
            job_id,
            state: TaskState::Queued,
            progress: None,
            file_id: None,
        })
    }
    pub async fn native_archive_job(
        &self,
        id: &str,
        scope: &str,
    ) -> Result<ArchiveJob, ClientError> {
        wire::validate_id(id)?;
        let epoch = self.checked_scope(scope).await?;
        let (value, _) = self
            .drive_request_at(
                Method::GET,
                &format!(
                    "{}/decompress/v1/progress?task_id={id}",
                    self.http.drive_origin
                ),
                None,
                Some(epoch),
            )
            .await?;
        let state = match value.get("phase").and_then(Value::as_str) {
            Some("PHASE_TYPE_PENDING") => TaskState::Queued,
            Some("PHASE_TYPE_RUNNING") => TaskState::Running,
            Some("PHASE_TYPE_COMPLETE") => TaskState::Completed,
            Some("PHASE_TYPE_ERROR") => TaskState::Failed,
            _ => return Err(wire::invalid()),
        };
        let progress = value
            .get("progress")
            .filter(|v| !v.is_null())
            .map(|v| {
                let p = wire::number(v)?.0;
                if p > 100 {
                    return Err(wire::invalid());
                }
                Ok(p as u32)
            })
            .transpose()?;
        let file_id = value
            .get("file_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| {
                wire::validate_id(s)?;
                Ok(s.to_owned())
            })
            .transpose()?;
        Ok(ArchiveJob {
            job_id: id.into(),
            state,
            progress,
            file_id,
        })
    }
}
fn parse_listing(
    id: &str,
    path: &str,
    name: &str,
    value: &Value,
) -> Result<ArchiveListing, ClientError> {
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(wire::invalid)?;
    let needs_password = matches!(status, "PASS_WORD_EMPTY" | "PASS_WORD_ERROR");
    let supported = status == "OK" || needs_password;
    let mut entries = Vec::new();
    if status == "OK" {
        let rows = value
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(wire::invalid)?;
        if rows.len() > 10000 {
            return Err(error(
                ErrorCode::Unsupported,
                "压缩包单目录条目过多，请使用本地解压工具。",
            ));
        }
        let mut names = BTreeSet::new();
        for row in rows {
            let name = wire::text(row, "filename", 1024)?;
            archive_path(&name)?;
            if name.contains('/') || name.is_empty() || !names.insert(name.clone()) {
                return Err(wire::invalid());
            }
            let kind = match row.get("kind").and_then(Value::as_str) {
                Some("drive#folder") => EntryKind::Folder,
                Some("drive#file") => EntryKind::File,
                _ => return Err(wire::invalid()),
            };
            let size_bytes = if kind == EntryKind::Folder {
                DecimalU64(0)
            } else {
                wire::number(row.get("filesize").ok_or_else(wire::invalid)?)?
            };
            entries.push(ArchiveEntry {
                path: if path.is_empty() {
                    name.clone()
                } else {
                    format!("{path}/{name}")
                },
                name,
                kind,
                size_bytes,
                modified_at: None,
            });
        }
    }
    let suffix = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let format = if matches!(suffix.as_str(), "zip" | "rar" | "7z") {
        suffix
    } else {
        "unknown".into()
    };
    Ok(ArchiveListing {
        file_id: id.into(),
        path: path.into(),
        format,
        supported,
        reason: (!supported).then(|| "服务暂不能读取此压缩包格式。".into()),
        encrypted: needs_password,
        needs_password,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::tests::Server;
    #[tokio::test]
    async fn archive_password_is_an_opaque_reference_and_extraction_is_a_remote_job() {
        let server=Server::new(|r|match r.path.split('?').next().unwrap(){"/drive/v1/files/zip-1"=>(200,json!({"id":"zip-1","name":"sample.zip","kind":"drive#file","parent_id":"","size":"90","hash":"aabb","phase":"PHASE_TYPE_COMPLETE"})),"/decompress/v1/list"=>if r.body["password"]=="test-secret"{(200,json!({"status":"OK","files":[{"index":"1","filename":"file.txt","filesize":"12","kind":"drive#file","path":""}]}))}else{(200,json!({"status":"PASS_WORD_EMPTY","files":[]}))},"/decompress/v1/decompress"=>{assert_eq!(r.body["password"],"test-secret");assert_eq!(r.body["files"][0]["index"],1);(200,json!({"status":"OK","task_id":"extract-1"}))},"/decompress/v1/progress"=>(200,json!({"phase":"PHASE_TYPE_COMPLETE","progress":100,"file_id":"folder-new"})),_=>(404,json!({"error":"file_not_found"}))}).await;
        let p = server.provider().await;
        let scope = p.account_scope().await.unwrap();
        assert!(
            p.native_archive_list("zip-1", "", None)
                .await
                .unwrap()
                .needs_password
        );
        let reference = p
            .native_archive_unlock("zip-1", "test-secret")
            .await
            .unwrap();
        assert_ne!(reference, "test-secret");
        let listing = p
            .native_archive_list("zip-1", "", Some(&reference))
            .await
            .unwrap();
        assert_eq!(listing.entries[0].size_bytes, DecimalU64(12));
        let job = p
            .native_archive_extract(
                "zip-1",
                &["file.txt".into()],
                "root",
                Some(&reference),
                &scope,
            )
            .await
            .unwrap();
        assert_eq!(job.state, TaskState::Queued);
        assert_eq!(
            p.native_archive_job(&job.job_id, &scope)
                .await
                .unwrap()
                .state,
            TaskState::Completed
        );
        p.logout().await.unwrap();
        assert!(p
            .native_archive_list("zip-1", "", Some(&reference))
            .await
            .is_err());
    }
    #[test]
    fn unsafe_archive_paths_are_rejected() {
        for path in ["../x", "/x", "folder/../x", "C:/x", "x\\y"] {
            assert!(archive_path(path).is_err())
        }
    }
}
