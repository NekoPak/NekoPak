//! Share creation/cancellation and bounded browsing; all tokens stay inside Rust.
use super::*;
use crate::cloud::{submitted_error, task_snapshot};

pub(super) struct ShareAccess {
    pub epoch: u64,
    pub expires_at: i64,
    share_id: String,
    title: String,
    token: Zeroizing<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredShareAccess {
    schema_version: u32,
    scope: String,
    share_id: String,
    title: String,
    token: String,
}
impl Drop for StoredShareAccess {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

fn retained_key(reference: &str) -> Result<&str, ClientError> {
    let id = reference.strip_prefix("share-").ok_or_else(wire::invalid)?;
    uuid::Uuid::parse_str(id).map_err(|_| wire::invalid())?;
    Ok(reference)
}

impl NativeProvider {
    pub async fn native_share_open(
        &self,
        share_id: &str,
        pass_code: &str,
    ) -> Result<SharedFolder, ClientError> {
        wire::validate_id(share_id)?;
        if pass_code.len() > 64 || pass_code.chars().any(char::is_control) {
            return Err(error(ErrorCode::Conflict, "分享提取码无效。"));
        }
        let mut url = url::Url::parse(&format!("{}/drive/v1/share", self.http.drive_origin))
            .map_err(|_| wire::invalid())?;
        url.query_pairs_mut()
            .append_pair("share_id", share_id)
            .append_pair("pass_code", pass_code)
            .append_pair("limit", "100")
            .append_pair("thumbnail_size", "SIZE_SMALL");
        let (value, epoch) = self.get(url.as_str()).await?;
        if value.get("share_status").and_then(Value::as_str) != Some("OK") {
            return Err(error(
                ErrorCode::PermissionDenied,
                "分享不可用或提取码不正确。",
            ));
        }
        let token = Zeroizing::new(
            value
                .get("pass_code_token")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        );
        if token.len() > 4096 || token.chars().any(char::is_control) {
            return Err(wire::invalid());
        }
        let title = value
            .get("title")
            .and_then(Value::as_str)
            .filter(|s| s.len() <= 1024 && !s.chars().any(char::is_control))
            .unwrap_or("分享")
            .to_owned();
        let access_ref = uuid::Uuid::new_v4().to_string();
        {
            let mut session = self.session.lock().await;
            if session.epoch != epoch {
                return Err(expired());
            }
            session
                .shares
                .retain(|_, s| s.epoch == epoch && s.expires_at > now());
            if session.shares.len() >= 32 {
                return Err(error(ErrorCode::Conflict, "打开的分享过多，请稍后重试。"));
            }
            session.shares.insert(
                access_ref.clone(),
                ShareAccess {
                    epoch,
                    expires_at: now() + 300,
                    share_id: share_id.into(),
                    title,
                    token,
                },
            );
        }
        self.native_share_folder(&access_ref, "root", None, 100)
            .await
    }
    pub(super) async fn share_access(
        &self,
        access_ref: &str,
    ) -> Result<(String, String, Zeroizing<String>, u64), ClientError> {
        let session = self.session.lock().await;
        if access_ref.starts_with("share-") {
            let key = retained_key(access_ref)?;
            let raw = self.vault.load_aux(key)?.ok_or_else(|| {
                error(ErrorCode::NotFound, "分享来源凭据已移除，请重新打开分享。")
            })?;
            let stored: StoredShareAccess =
                serde_json::from_str(&raw).map_err(|_| wire::invalid())?;
            if stored.schema_version != 1
                || !accounts::valid_account_id(&stored.scope)
                || stored.token.len() > 4096
                || stored.token.chars().any(char::is_control)
                || stored.title.len() > 1024
                || stored.title.chars().any(char::is_control)
            {
                return Err(wire::invalid());
            }
            wire::validate_id(&stored.share_id)?;
            let epoch = session.epoch;
            drop(session);
            if self.checked_scope(&stored.scope).await? != epoch {
                return Err(expired());
            }
            return Ok((
                stored.share_id.clone(),
                stored.title.clone(),
                Zeroizing::new(stored.token.clone()),
                epoch,
            ));
        }
        let access = session
            .shares
            .get(access_ref)
            .filter(|s| s.epoch == session.epoch && s.expires_at > now())
            .ok_or_else(|| error(ErrorCode::Conflict, "分享预览已过期，请重新打开分享。"))?;
        Ok((
            access.share_id.clone(),
            access.title.clone(),
            access.token.clone(),
            access.epoch,
        ))
    }
    pub(super) async fn native_share_retain(&self, reference: &str) -> Result<String, ClientError> {
        let (share_id, title, token, epoch) = self.share_access(reference).await?;
        let scope = self.account_scope().await?;
        let session = self.session.lock().await;
        if session.epoch != epoch {
            return Err(expired());
        }
        let key = format!("share-{}", uuid::Uuid::new_v4());
        let access = StoredShareAccess {
            schema_version: 1,
            scope,
            share_id,
            title,
            token: token.to_string(),
        };
        let raw = Zeroizing::new(serde_json::to_string(&access).map_err(|_| wire::invalid())?);
        self.vault.save_aux(&key, &raw)?;
        Ok(key)
    }
    pub(super) async fn native_share_release(&self, reference: &str) -> Result<(), ClientError> {
        let key = retained_key(reference)?;
        let (_, _, _, epoch) = self.share_access(reference).await?;
        let session = self.session.lock().await;
        if session.epoch != epoch {
            return Err(expired());
        }
        self.vault.delete_aux(key)
    }
    pub async fn native_share_folder(
        &self,
        access_ref: &str,
        parent: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<SharedFolder, ClientError> {
        wire::validate_id(parent)?;
        let (share_id, title, token, epoch) = self.share_access(access_ref).await?;
        let mut url = url::Url::parse(&format!("{}/drive/v1/share/detail", self.http.drive_origin))
            .map_err(|_| wire::invalid())?;
        url.query_pairs_mut()
            .append_pair("share_id", &share_id)
            .append_pair("parent_id", if parent == "root" { "" } else { parent })
            .append_pair("pass_code_token", &token)
            .append_pair("thumbnail_size", "SIZE_SMALL");
        let (value, next_cursor) = self
            .page_request(&format!("share:{access_ref}:{parent}"), url, cursor, limit)
            .await?;
        if self.session.lock().await.epoch != epoch {
            return Err(expired());
        }
        check_share_status(&value, false)?;
        let rows = value
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(wire::invalid)?;
        if rows.len() > limit {
            return Err(wire::invalid());
        }
        let mut entries = Vec::new();
        let mut ids = BTreeSet::new();
        for row in rows {
            let mut row = row.clone();
            row["parent_id"] = if parent == "root" { "" } else { parent }.into();
            let entry = wire::entry(&row)?;
            if !ids.insert(entry.id.clone()) {
                return Err(wire::invalid());
            }
            entries.push(entry);
        }
        Ok(SharedFolder {
            access_ref: access_ref.into(),
            share_id,
            title,
            parent_id: parent.into(),
            entries,
            next_cursor,
        })
    }
    pub async fn native_share_restore(
        &self,
        access_ref: &str,
        ids: &[String],
        parent: &str,
        scope: &str,
    ) -> Result<BulkResult, ClientError> {
        validate_ids(ids)?;
        wire::validate_id(parent)?;
        let (share_id, _, token, epoch) = self.share_access(access_ref).await?;
        if self.checked_scope(scope).await? != epoch {
            return Err(expired());
        }
        let body = json!({"share_id":share_id,"pass_code_token":token.as_str(),"file_ids":ids,"parent_id":if parent=="root"{""}else{parent},"specify_parent_id":true});
        let (value, _) = self
            .drive_request_at(
                Method::POST,
                &format!("{}/drive/v1/share/restore", self.http.drive_origin),
                Some(&body),
                Some(epoch),
            )
            .await
            .map_err(submitted_error)?;
        check_share_status(&value, false).map_err(submitted_error)?;
        // Unlike cancellation, a no-task empty response cannot be checked using
        // source IDs (restored copies receive new IDs). Require a task receipt.
        if value
            .get("task_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .is_none()
            && value
                .get("restore_task_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .is_none()
            && value.get("task").is_none()
        {
            return Err(submitted_error(wire::invalid()));
        }
        self.confirm_remote_action(&value, epoch).await?;
        Ok(BulkResult {
            succeeded: ids.to_vec(),
            failed: vec![],
            replayed: false,
        })
    }
    pub async fn native_share_create(
        &self,
        request: &CreateShareRequest,
        scope: &str,
    ) -> Result<ShareInfo, ClientError> {
        validate_ids(&request.ids)?;
        if !matches!(request.expires_in_days, 0 | 1 | 7 | 30) {
            return Err(error(
                ErrorCode::Conflict,
                "分享有效期须为永久、1、7或30天。",
            ));
        }
        let epoch = self.checked_scope(scope).await?;
        // Verify accessible selection and retain a harmless display name before writing.
        let mut title = None;
        for id in &request.ids {
            let (value, _) = self
                .drive_request_at(
                    Method::GET,
                    &format!("{}/drive/v1/files/{id}", self.http.drive_origin),
                    None,
                    Some(epoch),
                )
                .await?;
            let entry = wire::entry(&value)?;
            if entry.id != *id {
                return Err(wire::invalid());
            }
            title.get_or_insert(entry.name);
        }
        let body = json!({"file_ids":request.ids,"share_to":if request.with_code{"encryptedlink"}else{"publiclink"},"expiration_days":if request.expires_in_days==0{-1}else{request.expires_in_days as i64},"pass_code_option":if request.with_code{"REQUIRED"}else{"NOT_REQUIRED"}});
        let (value, _) = self
            .drive_request_at(
                Method::POST,
                &format!("{}/drive/v1/share", self.http.drive_origin),
                Some(&body),
                Some(epoch),
            )
            .await
            .map_err(submitted_error)?;
        let share_id = wire::text(&value, "share_id", 256).map_err(submitted_error)?;
        wire::validate_id(&share_id).map_err(submitted_error)?;
        let url = wire::text(&value, "share_url", 8192).map_err(submitted_error)?;
        validate_share_url(&url).map_err(submitted_error)?;
        let code = value
            .get("pass_code")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(|s| {
                if s.len() > 64 || s.chars().any(char::is_control) {
                    Err(wire::invalid())
                } else {
                    Ok(s.to_owned())
                }
            })
            .transpose()
            .map_err(submitted_error)?;
        if request.with_code && code.is_none() {
            return Err(submitted_error(wire::invalid()));
        }
        Ok(ShareInfo {
            share_id,
            title: title.unwrap_or_else(|| "分享".into()),
            item_count: request.ids.len() as u32,
            url,
            code,
            created_at: timestamp(now()),
            expires_at: None,
            state: "active".into(),
            expires_known: request.expires_in_days == 0,
            view_count: None,
            save_count: None,
        })
    }

    pub async fn native_shares_cancel(
        &self,
        ids: &[String],
        scope: &str,
    ) -> Result<BulkResult, ClientError> {
        validate_ids(ids)?;
        let epoch = self.checked_scope(scope).await?;
        let (value, _) = self
            .drive_request_at(
                Method::POST,
                &format!("{}/drive/v1/share:batchDelete", self.http.drive_origin),
                Some(&json!({"ids":ids})),
                Some(epoch),
            )
            .await
            .map_err(submitted_error)?;
        self.confirm_remote_action(&value, epoch).await?;
        // Verify all requested shares absent or inactive through complete paginated list.
        let mut active = BTreeSet::new();
        let mut cursor = None;
        let mut complete = false;
        for _ in 0..256 {
            let page = self.native_shares(cursor.as_deref(), 100).await?;
            for item in page.items {
                if ids.contains(&item.share_id) && item.state == "active" {
                    active.insert(item.share_id);
                }
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                complete = true;
                break;
            }
        }
        if !complete {
            return Err(error(
                ErrorCode::Conflict,
                "分享列表未读取完，取消结果尚未确认。",
            ));
        }
        if self.checked_scope(scope).await? != epoch {
            return Err(expired());
        }
        Ok(BulkResult {
            succeeded: ids
                .iter()
                .filter(|id| !active.contains(*id))
                .cloned()
                .collect(),
            failed: active
                .into_iter()
                .map(|id| MutationFailure {
                    id,
                    error: error(ErrorCode::Conflict, "此分享仍有效，请稍后刷新核对。"),
                })
                .collect(),
            replayed: false,
        })
    }

    pub(super) async fn confirm_remote_action(
        &self,
        value: &Value,
        epoch: u64,
    ) -> Result<(), ClientError> {
        let task = value.get("task").filter(|v| !v.is_null());
        let id = value
            .get("task_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                value
                    .get("restore_task_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| task.and_then(|v| v.get("id")).and_then(Value::as_str));
        if let Some(id) = id {
            wire::validate_id(id)?;
            for _ in 0..10 {
                let (v, _) = self
                    .drive_request_at(
                        Method::GET,
                        &format!("{}/drive/v1/tasks/{id}", self.http.drive_origin),
                        None,
                        Some(epoch),
                    )
                    .await?;
                let snapshot = task_snapshot(&v)?;
                if snapshot.id != id {
                    return Err(wire::invalid());
                }
                match snapshot.state {
                    TaskState::Completed => return Ok(()),
                    TaskState::Failed | TaskState::Cancelled => {
                        return Err(error(ErrorCode::Conflict, "云端操作未成功，请刷新核对。"))
                    }
                    _ => tokio::time::sleep(std::time::Duration::from_millis(400)).await,
                }
            }
            return Err(error(
                ErrorCode::Conflict,
                "云端操作仍未完成，请稍后刷新核对。",
            ));
        }
        if task.is_some()
            || value
                .get("task_id")
                .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
            || value
                .get("restore_task_id")
                .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
        {
            return Err(wire::invalid());
        }
        Ok(())
    }
}

pub(super) fn check_share_status(value: &Value, required: bool) -> Result<(), ClientError> {
    match value.get("share_status").and_then(Value::as_str) {
        Some("OK") => Ok(()),
        None if !required => Ok(()),
        Some(_) => Err(error(
            ErrorCode::PermissionDenied,
            "分享已失效、受限或需要重新验证提取码。",
        )),
        None => Err(wire::invalid()),
    }
}

pub(super) fn validate_ids(ids: &[String]) -> Result<(), ClientError> {
    if ids.is_empty() || ids.len() > 500 || ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
        return Err(error(ErrorCode::Conflict, "请选择1至500个不同的项目。"));
    }
    for id in ids {
        wire::validate_id(id)?;
        if id == "root" {
            return Err(error(ErrorCode::Conflict, "不能修改或分享网盘根目录。"));
        }
    }
    Ok(())
}
pub(super) fn validate_share_url(raw: &str) -> Result<(), ClientError> {
    let url = url::Url::parse(raw).map_err(|_| wire::invalid())?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || url.fragment().is_some()
        || !matches!(
            url.host_str(),
            Some(
                "mypikpak.com"
                    | "www.mypikpak.com"
                    | "mypikpak.net"
                    | "www.mypikpak.net"
                    | "pikpak.me"
                    | "www.pikpak.me"
                    | "pikpakdrive.com"
                    | "www.pikpakdrive.com"
            )
        )
        || !url.path().starts_with("/s/")
    {
        return Err(wire::invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::tests::Server;
    #[tokio::test]
    async fn create_share_validates_response_and_cancellation_checks_remote_state() {
        let server=Server::new(|r|match (r.method.as_str(),r.path.split('?').next().unwrap()){
            ("GET","/drive/v1/files/file-1")=>(200,json!({"id":"file-1","kind":"drive#file","name":"测试.txt","size":"10","parent_id":"","phase":"PHASE_TYPE_COMPLETE"})),
            ("POST","/drive/v1/share")=>{assert_eq!(r.body["share_to"],"encryptedlink");assert_eq!(r.body["expiration_days"],7);(200,json!({"share_id":"share-1","share_url":"https://mypikpak.com/s/share-1","pass_code":"abcd"}))},
            ("POST","/drive/v1/share:batchDelete")=>(200,json!({})),
            ("GET","/drive/v1/share/list")=>(200,json!({"data":[]})),_ => (404,json!({"error":"file_not_found"}))}).await;
        let p = server.provider().await;
        let scope = p.account_scope().await.unwrap();
        let share = p
            .native_share_create(
                &CreateShareRequest {
                    ids: vec!["file-1".into()],
                    with_code: true,
                    expires_in_days: 7,
                },
                &scope,
            )
            .await
            .unwrap();
        assert_eq!(share.code.as_deref(), Some("abcd"));
        assert!(!share.expires_known);
        let result = p
            .native_shares_cancel(&[share.share_id], &scope)
            .await
            .unwrap();
        assert_eq!(result.succeeded, ["share-1"]);
        assert!(result.failed.is_empty());
    }
    #[test]
    fn unsafe_share_links_and_invalid_selections_are_rejected() {
        assert!(validate_share_url("https://evil.test/s/id").is_err());
        assert!(validate_share_url("https://mypikpak.com.evil.test/s/id").is_err());
        assert!(validate_ids(&["root".into()]).is_err());
        assert!(validate_ids(&["one".into(), "one".into()]).is_err());
    }
    #[tokio::test]
    async fn shared_folder_tokens_stay_in_rust_and_restore_waits_for_remote_task() {
        let server=Server::new(|r|match(r.method.as_str(),r.path.split('?').next().unwrap()){
            ("GET","/drive/v1/share")=>(200,json!({"share_status":"OK","pass_code_token":"private-share-token","title":"Shared files"})),
            ("GET","/drive/v1/share/detail")=>{assert!(r.path.contains("pass_code_token=private-share-token"));(200,json!({"files":[{"id":"shared-1","name":"shared.txt","kind":"drive#file","size":"12"}]}))},
            ("POST","/drive/v1/share/restore")=>{assert_eq!(r.body["pass_code_token"],"private-share-token");(200,json!({"task_id":"restore-1"}))},
            ("GET","/drive/v1/tasks/restore-1")=>(200,json!({"id":"restore-1","phase":"PHASE_TYPE_COMPLETE"})),_=>(404,json!({"error":"file_not_found"}))}).await;
        let p = server.provider().await;
        let scope = p.account_scope().await.unwrap();
        let folder = p.native_share_open("share-1", "abcd").await.unwrap();
        assert!(!serde_json::to_string(&folder)
            .unwrap()
            .contains("private-share-token"));
        assert_eq!(folder.entries.len(), 1);
        let result = p
            .native_share_restore(&folder.access_ref, &["shared-1".into()], "root", &scope)
            .await
            .unwrap();
        assert_eq!(result.succeeded, ["shared-1"]);
        p.logout().await.unwrap();
        assert!(p
            .native_share_folder(&folder.access_ref, "root", None, 100)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn retained_share_authorization_survives_memory_reset_and_rejects_another_account() {
        let server=Server::new(|r| match r.path.split('?').next().unwrap() {
            "/drive/v1/share" => (200,json!({"share_status":"OK","pass_code_token":"retained-private-token","title":"Shared"})),
            "/drive/v1/share/detail" => (200,json!({"share_status":"OK","files":[]})),
            _ => (404,json!({"error":"file_not_found"})),
        }).await;
        let p = server.provider().await;
        let folder = p.share_open("share-one", "abcd").await.unwrap();
        let retained = p.share_retain(&folder.access_ref).await.unwrap();
        assert!(retained.starts_with("share-"));
        assert!(!retained.contains("retained-private-token"));
        let active = p.session.lock().await.stored.as_ref().unwrap().clone();
        *p.session.lock().await = Session::default();
        p.auth_status().await.unwrap();
        assert!(p.share_folder(&retained, "root", None, 100).await.is_ok());
        let mut other = active.clone();
        other.user_id = "another-user".into();
        other.account_id = String::new();
        {
            let mut session = p.session.lock().await;
            p.commit(&mut session, other).unwrap();
            session.invalidate_account_handles();
        }
        assert_eq!(
            p.share_folder(&retained, "root", None, 100)
                .await
                .unwrap_err()
                .code,
            ErrorCode::AuthExpired
        );
        assert!(p.share_release(&retained).await.is_err());
        p.account_switch(&active.account_id).await.unwrap();
        assert!(p.share_folder(&retained, "root", None, 100).await.is_ok());
        p.share_release(&retained).await.unwrap();
        assert!(p.vault.load_aux(&retained).unwrap().is_none());
    }

    #[tokio::test]
    async fn share_restore_uses_verified_destination_and_restore_task_receipt() {
        let server=Server::new(|r|match(r.method.as_str(),r.path.split('?').next().unwrap()){
            ("GET","/drive/v1/share")=>(200,json!({"share_status":"OK","pass_code_token":"private-share-token"})),
            ("GET","/drive/v1/share/detail")=>(200,json!({"share_status":"OK","files":[]})),
            ("POST","/drive/v1/share/restore")=>{assert_eq!(r.body["parent_id"],"target-folder");assert_eq!(r.body["specify_parent_id"],true);assert!(r.body.get("to").is_none());(200,json!({"share_status":"OK","restore_status":"RESTORE_START","restore_task_id":"restore-task"}))},
            ("GET","/drive/v1/tasks/restore-task")=>(200,json!({"id":"restore-task","phase":"PHASE_TYPE_COMPLETE"})),
            _=>(404,json!({"error":"file_not_found"})),
        }).await;
        let p = server.provider().await;
        let folder = p.share_open("share-one", "").await.unwrap();
        let scope = p.account_scope().await.unwrap();
        assert_eq!(
            p.share_restore(
                &folder.access_ref,
                &["shared-file".into()],
                "target-folder",
                &scope
            )
            .await
            .unwrap()
            .succeeded,
            ["shared-file"]
        );
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.path == "/drive/v1/share/restore")
                .count(),
            1
        );
    }
}
