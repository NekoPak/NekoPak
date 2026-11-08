//! Persistent remote jobs and share intents. A dropped response never repeats a write.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Serialize, Deserialize)]
struct CloudRecord {
    scope: String,
    last_poll: i64,
}
fn damaged() -> ClientError {
    ClientError::new(
        ErrorCode::IntegrityFailed,
        "云端任务记录不完整，未重发请求。",
    )
}
pub(crate) fn unknown_write(mut error: ClientError) -> ClientError {
    if matches!(
        error.code,
        ErrorCode::TransientNetwork
            | ErrorCode::Cancelled
            | ErrorCode::Internal
            | ErrorCode::IntegrityFailed
    ) {
        error.code = ErrorCode::Conflict;
        error.retryable = false;
        error.message = "云端操作结果尚未确认，请刷新核对；不会重复提交。".into();
        error
            .details
            .get_or_insert_with(Default::default)
            .insert("outcome_unknown".into(), true.into());
    }
    error
}
fn timestamp() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

impl AppService {
    pub async fn share_preview_entry(
        &self,
        reference: &str,
        id: &str,
        ctx: &RequestContext,
    ) -> Result<RemoteEntry, ClientError> {
        let mut info = ctx
            .run(self.provider.share_source_info(reference, id))
            .await?;
        info.entry.id = format!("shared:{reference}:{id}");
        info.entry.parent_id = None;
        Ok(info.entry)
    }
    pub(crate) async fn retain_shared_source(
        &self,
        id: &str,
        scope: &str,
        key: Option<&str>,
        ctx: &RequestContext,
    ) -> Result<String, ClientError> {
        let Some(raw) = id.strip_prefix("shared:") else {
            return Ok(id.into());
        };
        let (reference, remote) = raw.split_once(':').ok_or_else(damaged)?;
        if reference.starts_with("share-") {
            return Ok(id.into());
        }
        let record = key.map(|k| fingerprint(&[k, id]));
        if let Some(key) = &record {
            if let Some(saved) = self.records()?.record_get(scope, "shared-sources", key)? {
                return serde_json::from_str(&saved).map_err(|_| damaged());
            }
        }
        self.check_scope(scope, ctx).await?;
        let retained = ctx.run(self.provider.share_retain(reference)).await?;
        let next = format!("shared:{retained}:{remote}");
        self.check_scope(scope, ctx).await?;
        if let Some(key) = record {
            self.records()?.record_put(
                scope,
                "shared-sources",
                &key,
                &serde_json::to_string(&next).map_err(|_| damaged())?,
            )?;
        }
        Ok(next)
    }
    pub async fn cleanup_shared_sources(&self, ctx: &RequestContext) -> Result<(), ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        let mut cursor = None;
        let mut active = std::collections::BTreeSet::new();
        let mut terminal = std::collections::BTreeSet::new();
        loop {
            let page = self.store()?.list(cursor.as_deref(), 500)?;
            for task in page.items {
                if let Some((reference, _)) = task
                    .remote_id
                    .strip_prefix("shared:")
                    .and_then(|s| s.split_once(':'))
                    .filter(|(r, _)| r.starts_with("share-"))
                {
                    let owner = if task.kind == TaskKind::SegmentDownload {
                        self.media_scope(&task.id)?
                    } else if task.kind == TaskKind::Download {
                        self.download_scope(&task.id)?
                    } else {
                        continue;
                    };
                    if owner == scope {
                        if matches!(task.state, TaskState::Completed | TaskState::Cancelled) {
                            terminal.insert(reference.to_owned());
                        } else {
                            active.insert(reference.to_owned());
                        }
                    }
                }
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        for reference in terminal.difference(&active) {
            if self
                .records()?
                .record_get(&scope, "released-share", reference)?
                .is_some()
            {
                continue;
            }
            match ctx.run(self.provider.share_release(reference)).await {
                Ok(()) => {
                    self.records()?
                        .record_put(&scope, "released-share", reference, "true")?
                }
                Err(error) if matches!(error.code, ErrorCode::NotFound | ErrorCode::Conflict) => {
                    self.records()?
                        .record_put(&scope, "released-share", reference, "true")?;
                }
                Err(_) => continue,
            }
        }
        Ok(())
    }
    pub async fn share_open(
        &self,
        id: &str,
        code: &str,
        ctx: &RequestContext,
    ) -> Result<SharedFolder, ClientError> {
        ctx.run(self.provider.share_open(id, code)).await
    }
    pub async fn share_folder(
        &self,
        reference: &str,
        parent: &str,
        cursor: Option<&str>,
        limit: Option<u32>,
        ctx: &RequestContext,
    ) -> Result<SharedFolder, ClientError> {
        ctx.run(
            self.provider
                .share_folder(reference, parent, cursor, page_limit(limit)?),
        )
        .await
    }
    pub async fn share_restore(
        &self,
        reference: &str,
        ids: &[String],
        parent: &str,
        key: &str,
        ctx: &RequestContext,
    ) -> Result<BulkResult, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        let key = scoped_key("share-restore", key)?;
        let fp = fingerprint(&[
            reference,
            &serde_json::to_string(ids).map_err(|_| damaged())?,
            parent,
        ]);
        if let Some(value) = self.records()?.begin_value(&scope, &key, &fp)? {
            let mut result: BulkResult = serde_json::from_value(value).map_err(|_| damaged())?;
            result.replayed = true;
            return Ok(result);
        }
        let result = ctx
            .run(self.provider.share_restore(reference, ids, parent, &scope))
            .await
            .map_err(unknown_write);
        self.records()?.finish_value(
            &scope,
            &key,
            &result.as_ref().map(|v| json!(v)).map_err(Clone::clone),
        )?;
        result
    }
    pub(crate) fn cloud_scope(&self, id: &str) -> Result<String, ClientError> {
        let payload = self
            .records()?
            .record_get("tasks", "cloud", id)?
            .ok_or_else(damaged)?;
        serde_json::from_str::<CloudRecord>(&payload)
            .map(|r| r.scope)
            .map_err(|_| damaged())
    }
    pub(crate) async fn create_native_cloud(
        &self,
        source: &str,
        parent: &str,
        key: &str,
        ctx: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        validate_source(source)?;
        let scope = ctx.run(self.provider.account_scope()).await?;
        let key = scoped_key("cloud", key)?;
        let fp = fingerprint(&[source, parent]);
        let replay = self.records()?.begin_value(&scope, &key, &fp)?;
        let snapshot: CloudTaskSnapshot = if let Some(value) = replay {
            serde_json::from_value(value).map_err(|_| damaged())?
        } else {
            let result = ctx
                .run(self.provider.cloud_create(source, parent, &scope))
                .await
                .map_err(unknown_write);
            self.records()?.finish_value(
                &scope,
                &key,
                &result
                    .as_ref()
                    .map(|v| serde_json::to_value(v).expect("cloud snapshot serializes"))
                    .map_err(Clone::clone),
            )?;
            result?
        };
        self.check_scope(&scope, ctx).await?;
        let task_key = format!("cloud:{scope}:{key}");
        if let Some(task) = self.store()?.lookup_key(&task_key, &fp)? {
            return Ok(TaskRef { task_id: task.id });
        }
        let mut task = new_task(
            snapshot.id.clone(),
            TaskKind::CloudDownload,
            snapshot.total_bytes,
        );
        task.backend = BackendId::RustCompat;
        task.simulated = false;
        task.name = Some(snapshot.name.clone());
        task.destination = Some(parent.into());
        self.records()?.record_put(
            "tasks",
            "cloud",
            &task.id,
            &serde_json::to_string(&CloudRecord {
                scope,
                last_poll: 0,
            })
            .map_err(|_| damaged())?,
        )?;
        let task = self.store()?.insert(task, Some((&task_key, &fp)))?;
        let task = self.store()?.update_cloud(&task, &snapshot)?;
        self.emit(&task);
        Ok(TaskRef { task_id: task.id })
    }
    pub(crate) async fn tick_cloud(&self) -> Result<(), ClientError> {
        let Ok(_guard) = self.cloud_gate.try_lock() else {
            return Ok(());
        };
        for task in self
            .store()?
            .active()?
            .into_iter()
            .filter(|t| t.kind == TaskKind::CloudDownload && !t.simulated)
        {
            let mut record: CloudRecord = serde_json::from_str(
                &self
                    .records()?
                    .record_get("tasks", "cloud", &task.id)?
                    .ok_or_else(damaged)?,
            )
            .map_err(|_| damaged())?;
            if timestamp() - record.last_poll < 3 {
                continue;
            }
            record.last_poll = timestamp();
            self.records()?.record_put(
                "tasks",
                "cloud",
                &task.id,
                &serde_json::to_string(&record).map_err(|_| damaged())?,
            )?;
            let ctx = RequestContext::new(Duration::from_secs(45));
            if self.check_scope(&record.scope, &ctx).await.is_err() {
                continue;
            }
            match ctx
                .run(self.provider.cloud_task(&task.remote_id, &record.scope))
                .await
            {
                Ok(snapshot) => match self.store()?.update_cloud(&task, &snapshot) {
                    Ok(updated) => self.emit(&updated),
                    Err(e) if e.code == ErrorCode::Conflict => {}
                    Err(e) => return Err(e),
                },
                Err(error) => {
                    let snapshot = CloudTaskSnapshot {
                        id: task.remote_id.clone(),
                        name: task.name.clone().unwrap_or_default(),
                        state: if matches!(
                            error.code,
                            ErrorCode::AuthExpired | ErrorCode::VerificationRequired
                        ) {
                            TaskState::Paused
                        } else {
                            TaskState::Retrying
                        },
                        progress: task.progress_percent,
                        file_id: None,
                        total_bytes: task.total_bytes,
                        created_at: None,
                        error: Some(error),
                    };
                    match self.store()?.update_cloud(&task, &snapshot) {
                        Ok(updated) => self.emit(&updated),
                        Err(e) if e.code == ErrorCode::Conflict => {}
                        Err(e) => return Err(e),
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) async fn control_cloud(
        &self,
        id: &str,
        action: TransferAction,
        ctx: &RequestContext,
    ) -> Result<(), ClientError> {
        let _guard = self.cloud_gate.lock().await;
        let task = self.store()?.get(id)?;
        let scope = self.cloud_scope(id)?;
        self.check_scope(&scope, ctx).await?;
        if task.state == TaskState::Completed {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "已完成任务不能再次控制。",
            ));
        }
        if !matches!(action, TransferAction::Retry | TransferAction::Cancel) {
            return Err(ClientError::new(
                ErrorCode::Unsupported,
                "云端任务支持取消与重试，不支持本地暂停。",
            ));
        }
        // Revision binds one explicit control intent. Unknown outcome keeps the
        // same key and cannot turn a second click into a second remote mutation.
        let key = format!("cloud-control:{}:{}:{action:?}", task.id, task.revision.0);
        let fp = fingerprint(&[&task.remote_id, &format!("{action:?}")]);
        if self.records()?.begin_value(&scope, &key, &fp)?.is_none() {
            let result = ctx
                .run(self.provider.cloud_control(&task.remote_id, action, &scope))
                .await
                .map(|_| Value::Null)
                .map_err(unknown_write);
            self.records()?.finish_value(&scope, &key, &result)?;
            result?;
        }
        let snapshot = if action == TransferAction::Cancel {
            CloudTaskSnapshot {
                id: task.remote_id.clone(),
                name: task.name.clone().unwrap_or_default(),
                state: TaskState::Cancelled,
                progress: task.progress_percent,
                file_id: None,
                total_bytes: task.total_bytes,
                created_at: None,
                error: None,
            }
        } else {
            ctx.run(self.provider.cloud_task(&task.remote_id, &scope))
                .await?
        };
        let updated = self.store()?.update_cloud(&task, &snapshot)?;
        self.emit(&updated);
        Ok(())
    }
    pub async fn cloud_preview(
        &self,
        source: &str,
        ctx: &RequestContext,
    ) -> Result<Value, ClientError> {
        validate_source(source)?;
        let scope = ctx.run(self.provider.account_scope()).await?;
        let preview = ctx.run(self.provider.cloud_preview(source)).await?;
        self.check_scope(&scope, ctx).await?;
        let id = uuid::Uuid::new_v4().to_string();
        let expires = timestamp() + 300;
        self.records()?.record_put(
            &scope,
            "preview",
            &id,
            &json!({"fingerprint":fingerprint(&[source]),"expires":expires}).to_string(),
        )?;
        Ok(
            json!({"preview_id":id,"source_kind":if source.starts_with("magnet:"){"magnet"}else{"http"},"title":preview.title,"total_bytes":preview.total_bytes,"files":preview.files,"selection_supported":preview.selection_supported,"expires_at":time::OffsetDateTime::from_unix_timestamp(expires).map_err(|_|damaged())?.format(&time::format_description::well_known::Rfc3339).map_err(|_|damaged())?}),
        )
    }
    pub async fn validate_preview(
        &self,
        source: &str,
        id: &str,
        ctx: &RequestContext,
    ) -> Result<(), ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        let value: Value = serde_json::from_str(
            &self
                .records()?
                .record_get(&scope, "preview", id)?
                .ok_or_else(|| ClientError::new(ErrorCode::Conflict, "资源预览已失效。"))?,
        )
        .map_err(|_| damaged())?;
        if value["fingerprint"] != fingerprint(&[source])
            || value["expires"].as_i64().is_none_or(|t| t < timestamp())
        {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "链接已改变或预览已过期，请重新预览。",
            ));
        }
        Ok(())
    }
    pub async fn share_create(
        &self,
        request: &CreateShareRequest,
        key: &str,
        ctx: &RequestContext,
    ) -> Result<ShareInfo, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        let key = scoped_key("share-create", key)?;
        let fp = fingerprint(&[&serde_json::to_string(request).map_err(|_| damaged())?]);
        if let Some(value) = self.records()?.begin_value(&scope, &key, &fp)? {
            return serde_json::from_value(value).map_err(|_| damaged());
        }
        let result = ctx
            .run(self.provider.share_create(request, &scope))
            .await
            .map_err(unknown_write);
        self.records()?.finish_value(
            &scope,
            &key,
            &result
                .as_ref()
                .map(|v| serde_json::to_value(v).expect("share serializes"))
                .map_err(Clone::clone),
        )?;
        result
    }
    pub async fn shares_cancel(
        &self,
        ids: &[String],
        key: &str,
        ctx: &RequestContext,
    ) -> Result<BulkResult, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        let key = scoped_key("share-cancel", key)?;
        let fp = fingerprint(&[&serde_json::to_string(ids).map_err(|_| damaged())?]);
        if let Some(value) = self.records()?.begin_value(&scope, &key, &fp)? {
            let mut result: BulkResult = serde_json::from_value(value).map_err(|_| damaged())?;
            result.replayed = true;
            return Ok(result);
        }
        let result = ctx
            .run(self.provider.shares_cancel(ids, &scope))
            .await
            .map_err(unknown_write);
        self.records()?.finish_value(
            &scope,
            &key,
            &result
                .as_ref()
                .map(|v| serde_json::to_value(v).expect("bulk result serializes"))
                .map_err(Clone::clone),
        )?;
        result
    }
}
