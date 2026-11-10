//! User-visible history removal preserves task receipts and idempotency proofs.
//! It never removes downloaded bytes or issues a remote cancel/delete request.
use super::*;
use nekopak_provider::RequestContext;

impl AppService {
    pub(crate) fn transfer_history_hidden(&self, id: &str) -> Result<bool, ClientError> {
        match self
            .records()?
            .record_get("device", "hidden-transfer", id)?
        {
            None => Ok(false),
            Some(value) if value == "true" => Ok(true),
            Some(_) => Err(ClientError::new(
                ErrorCode::IntegrityFailed,
                "任务历史记录损坏，未覆盖。",
            )),
        }
    }
    async fn authorize_history(
        &self,
        task: &TransferTask,
        ctx: &RequestContext,
    ) -> Result<(), ClientError> {
        if task.backend == BackendId::Mock {
            return Ok(());
        }
        let scope = match task.kind {
            TaskKind::CloudDownload => self.cloud_scope(&task.id)?,
            TaskKind::SegmentDownload => self.media_scope(&task.id)?,
            TaskKind::Upload => self.upload_scope(&task.id)?,
            TaskKind::Download => self.download_scope(&task.id)?,
        };
        self.check_scope(&scope, ctx).await
    }
    pub async fn transfer_forget(&self, id: &str, ctx: &RequestContext) -> Result<(), ClientError> {
        let task = self.store()?.get(id)?;
        self.authorize_history(&task, ctx).await?;
        if !task.state.is_terminal() {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "请先暂停或取消进行中的任务；只有已结束的记录可以移除。",
            ));
        }
        let _gate = self
            .download_gate
            .lock()
            .map_err(|_| ClientError::new(ErrorCode::Internal, "任务队列暂不可用。"))?;
        if self.store()?.get(id)?.revision != task.revision {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "任务状态已改变，请刷新后重试。",
            ));
        }
        // Validate an existing marker before updating it; a remove request is
        // not permission to silently replace damaged persistent metadata.
        self.transfer_history_hidden(id)?;
        self.records()?
            .record_put("device", "hidden-transfer", id, "true")?;
        self.download_rates.remove(id);
        Ok(())
    }
    pub async fn transfer_clear_finished(&self, ctx: &RequestContext) -> Result<u32, ClientError> {
        let expected_scope = self.history_account_scope(ctx).await?;
        let mut cursor = None;
        let mut removed = 0u32;
        loop {
            let page = self
                .visible_transfers(cursor.as_deref(), Some(500), ctx)
                .await?;
            if self.history_account_scope(ctx).await? != expected_scope {
                return Err(ClientError::new(
                    ErrorCode::AuthExpired,
                    "账号已改变，任务历史清理已停止。",
                ));
            }
            for task in page.items {
                if task.state.is_terminal() {
                    self.transfer_forget(&task.id, ctx).await?;
                    removed = removed.checked_add(1).ok_or_else(|| {
                        ClientError::new(ErrorCode::Conflict, "任务数量超出范围。")
                    })?;
                }
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        Ok(removed)
    }
    async fn history_account_scope(
        &self,
        ctx: &RequestContext,
    ) -> Result<Option<String>, ClientError> {
        if self.provider.id() == BackendId::Mock {
            return Ok(None);
        }
        match self.account_scope(ctx).await {
            Ok(scope) => Ok(Some(scope)),
            // Local-source history is also available while signed out.
            Err(error) if error.code == ErrorCode::AuthExpired => Ok(None),
            Err(error) => Err(error),
        }
    }
    pub async fn auth_preferences_get(
        &self,
        ctx: &RequestContext,
    ) -> Result<serde_json::Value, ClientError> {
        ctx.run(self.provider.auth_preferences_get()).await
    }
    pub async fn auth_preferences_update(
        &self,
        value: &serde_json::Value,
        ctx: &RequestContext,
    ) -> Result<serde_json::Value, ClientError> {
        ctx.run(self.provider.auth_preferences_update(value)).await
    }
    pub async fn network_domains_probe(
        &self,
        ctx: &RequestContext,
    ) -> Result<serde_json::Value, ClientError> {
        ctx.run(self.provider.network_domains_probe()).await
    }
    pub fn network_reconnect(&self) -> Result<(), ClientError> {
        self.settings_get()?;
        self.provider.network_reconnect()?;
        self.retry_waiting_now()
    }
}
