//! Durable recovery with bounded backoff for idempotent read-only transfer steps.
//!
//! Only ordinary and segment downloads are eligible. Uploads and remote write
//! operations deliberately remain under their existing unknown-outcome policy.
use crate::AppService;
use nekopak_domain::{ClientError, ErrorCode, TaskKind, TaskState, TransferTask};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

const SCOPE: &str = "transfer-retry";
const KIND: &str = "task";
const MAX_ATTEMPTS: u32 = 6;

fn now_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetryRecord {
    attempts: u32,
    next_at: i64,
    #[serde(default)]
    not_before: i64,
}

fn retryable(task: &TransferTask, error: &ClientError) -> bool {
    error.code == ErrorCode::TransientNetwork
        && error.retryable
        && matches!(task.kind, TaskKind::Download | TaskKind::SegmentDownload)
        && matches!(
            task.state,
            TaskState::Queued | TaskState::Running | TaskState::Retrying
        )
}

fn parse(raw: &str) -> Result<RetryRecord, ClientError> {
    serde_json::from_str::<RetryRecord>(raw)
        .ok()
        .filter(|record| {
            (1..=MAX_ATTEMPTS).contains(&record.attempts)
                && record.next_at > 0
                && (0..=record.next_at).contains(&record.not_before)
        })
        .ok_or_else(|| {
            ClientError::new(
                ErrorCode::IntegrityFailed,
                "传输重试记录损坏，已停止自动重试。",
            )
        })
}

impl AppService {
    /// Keep temporary network outages recoverable at a capped retry rate.
    /// Returns true when the error was handled as a scheduled retry,
    /// and false when the caller should preserve
    /// its normal failure handling.
    pub(crate) fn schedule_transfer_retry(
        &self,
        task: &TransferTask,
        error: ClientError,
    ) -> Result<bool, ClientError> {
        if !retryable(task, &error) {
            return Ok(false);
        }
        let _gate = self
            .download_gate
            .lock()
            .map_err(|_| ClientError::new(ErrorCode::Internal, "下载队列暂不可用。"))?;
        let current = self.store()?.get(&task.id)?;
        if current.revision != task.revision {
            return Ok(true);
        }
        let attempts = self
            .records()?
            .record_get(SCOPE, KIND, &task.id)?
            .map(|raw| parse(&raw))
            .transpose()?
            .map(|record| record.attempts.saturating_add(1).min(MAX_ATTEMPTS))
            .unwrap_or(1);
        let delay = match attempts {
            1 => 1,
            2 => 2,
            3 => 4,
            4 => 8,
            5 => 16,
            _ => 30,
        };
        let current_time = now_ms();
        let not_before = error
            .details
            .as_ref()
            .and_then(|details| details.get("retry_after_seconds"))
            .and_then(serde_json::Value::as_u64)
            .map(|seconds| {
                current_time.saturating_add(
                    i64::try_from(seconds)
                        .unwrap_or(i64::MAX)
                        .saturating_mul(1000),
                )
            })
            .unwrap_or(0);
        let record = RetryRecord {
            attempts,
            next_at: (current_time + delay * 1000).max(not_before),
            not_before,
        };
        self.records()?.record_put(
            SCOPE,
            KIND,
            &task.id,
            &serde_json::to_string(&record)
                .map_err(|_| ClientError::new(ErrorCode::Internal, "无法保存重试计划。"))?,
        )?;
        let retrying = self.store()?.update(
            &current,
            TaskState::Retrying,
            current.transferred_bytes.0,
            Some(error),
        )?;
        self.emit(&retrying);
        Ok(true)
    }

    /// Gate the normal worker path until a retry is due. The record remains
    /// until bytes make progress so repeated failures retain their backoff.
    pub(crate) fn retry_ready(&self, task: &TransferTask) -> Result<bool, ClientError> {
        let Some(raw) = self.records()?.record_get(SCOPE, KIND, &task.id)? else {
            return Ok(true);
        };
        let record = parse(&raw)?;
        Ok(record.next_at <= now_ms())
    }

    /// Explicit pause/cancel/retry/resume clears an automatic retry plan. A
    /// user action must never be silently overridden by an old network retry.
    pub(crate) fn clear_transfer_retry(&self, id: &str) -> Result<(), ClientError> {
        self.records()?.record_remove(SCOPE, KIND, id)
    }

    /// Store recovery pauses in-flight work. Restore only tasks that have an
    /// automatic retry plan and were paused by process recovery, never a manual
    /// pause (which clears the plan before changing state).
    pub(crate) fn recover_transfer_retries(&self) -> Result<(), ClientError> {
        if !self.is_foreground() {
            return Ok(());
        }
        let _gate = self
            .download_gate
            .lock()
            .map_err(|_| ClientError::new(ErrorCode::Internal, "下载队列暂不可用。"))?;
        for (id, raw) in self.records()?.record_list(SCOPE, KIND)? {
            let _record = parse(&raw)?;
            let current = self.store()?.get(&id)?;
            if current.state == TaskState::Paused
                && current.error.as_ref().is_some_and(|e| e.code == ErrorCode::Conflict
                    && e.message == "The previous process stopped during this task; progress was retained. Resume explicitly.")
            {
                let queued = self.store()?.update(&current, TaskState::Queued, current.transferred_bytes.0, None)?;
                self.emit(&queued);
            } else if current.state.is_terminal() {
                self.clear_transfer_retry(&id)?;
            }
        }
        Ok(())
    }

    /// A user-requested reconnect may wake safe, already-scheduled reads. It
    /// does not resume manual pauses or bypass a server's Retry-After.
    pub(crate) fn retry_waiting_now(&self) -> Result<(), ClientError> {
        let _gate = self
            .download_gate
            .lock()
            .map_err(|_| ClientError::new(ErrorCode::Internal, "下载队列暂不可用。"))?;
        for (id, raw) in self.records()?.record_list(SCOPE, KIND)? {
            let mut record = parse(&raw)?;
            if self.store()?.get(&id)?.state == TaskState::Retrying {
                record.next_at = now_ms().max(record.not_before);
                self.records()?.record_put(
                    SCOPE,
                    KIND,
                    &id,
                    &serde_json::to_string(&record)
                        .map_err(|_| ClientError::new(ErrorCode::Internal, "无法保存重试计划。"))?,
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::new_task;
    use nekopak_domain::{DecimalU64, TransferAction};
    use nekopak_provider_mock::Scenario;

    fn network() -> ClientError {
        ClientError::new(
            ErrorCode::TransientNetwork,
            "synthetic network interruption",
        )
    }
    fn insert(app: &AppService, kind: TaskKind) -> TransferTask {
        let mut task = new_task("synthetic-file".into(), kind, Some(DecimalU64(100)));
        task.backend = nekopak_domain::BackendId::RustCompat;
        task.simulated = false;
        app.store().unwrap().insert(task, None).unwrap()
    }

    #[test]
    fn network_outages_remain_recoverable_with_capped_backoff_and_never_replay_writes() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::mock(&dir.path().join("queue.db"), Scenario::Free);
        let mut task = insert(&app, TaskKind::Download);
        for attempt in 1..=MAX_ATTEMPTS {
            assert!(app.schedule_transfer_retry(&task, network()).unwrap());
            task = app.task(&task.id).unwrap();
            assert_eq!(task.state, TaskState::Retrying);
            let record = parse(
                &app.records()
                    .unwrap()
                    .record_get(SCOPE, KIND, &task.id)
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(record.attempts, attempt);
            assert!(!app.retry_ready(&task).unwrap());
        }
        for _ in 0..20 {
            app.schedule_transfer_retry(&task, network()).unwrap();
            task = app.task(&task.id).unwrap();
            assert_eq!(task.state, TaskState::Retrying);
            let raw = app
                .records()
                .unwrap()
                .record_get(SCOPE, KIND, &task.id)
                .unwrap()
                .unwrap();
            let record = parse(&raw).unwrap();
            assert_eq!(record.attempts, MAX_ATTEMPTS);
            assert!(record.next_at >= now_ms() + 29_000);
        }
        app.retry_waiting_now().unwrap();
        assert!(app.retry_ready(&task).unwrap());
        let upload = insert(&app, TaskKind::Upload);
        assert!(!app.schedule_transfer_retry(&upload, network()).unwrap());
        let original = insert(&app, TaskKind::Download);
        assert!(!app
            .schedule_transfer_retry(
                &original,
                ClientError::new(ErrorCode::IntegrityFailed, "synthetic mismatch")
            )
            .unwrap());
    }

    #[test]
    fn automatic_retry_respects_the_server_wait_including_explicit_reconnect() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::mock(&dir.path().join("queue.db"), Scenario::Free);
        let task = insert(&app, TaskKind::Download);
        let mut error = network();
        error.details = Some([("retry_after_seconds".into(), 120.into())].into());
        let earliest = now_ms() + 120_000;
        app.schedule_transfer_retry(&task, error).unwrap();
        let read = || {
            parse(
                &app.records()
                    .unwrap()
                    .record_get(SCOPE, KIND, &task.id)
                    .unwrap()
                    .unwrap(),
            )
            .unwrap()
        };
        assert!(
            read().next_at >= earliest,
            "must not retry before the server allows it"
        );
        app.retry_waiting_now().unwrap();
        assert!(
            read().next_at >= earliest,
            "reconnecting must preserve server throttling"
        );
    }

    #[test]
    fn pending_retry_survives_restart_but_explicit_pause_and_cancel_are_never_resumed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queue.db");
        let app = AppService::mock(&path, Scenario::Free);
        let task = insert(&app, TaskKind::Download);
        app.schedule_transfer_retry(&task, network()).unwrap();
        let id = task.id;
        drop(app);
        let app = AppService::mock(&path, Scenario::Free);
        assert_eq!(app.task(&id).unwrap().state, TaskState::Paused);
        app.recover_transfer_retries().unwrap();
        assert_eq!(app.task(&id).unwrap().state, TaskState::Queued);
        assert!(!app.retry_ready(&app.task(&id).unwrap()).unwrap());
        app.transfer_control(&id, TransferAction::Pause).unwrap();
        app.recover_transfer_retries().unwrap();
        assert_eq!(app.task(&id).unwrap().state, TaskState::Paused);
        assert!(app
            .records()
            .unwrap()
            .record_get(SCOPE, KIND, &id)
            .unwrap()
            .is_none());
        let task = insert(&app, TaskKind::SegmentDownload);
        app.schedule_transfer_retry(&task, network()).unwrap();
        app.transfer_control(&task.id, TransferAction::Cancel)
            .unwrap();
        app.recover_transfer_retries().unwrap();
        assert_eq!(app.task(&task.id).unwrap().state, TaskState::Cancelled);
    }

    #[test]
    fn invalid_retry_metadata_is_preserved_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::mock(&dir.path().join("queue.db"), Scenario::Free);
        let task = insert(&app, TaskKind::Download);
        let damaged = r#"{"attempts":0,"next_at":-1}"#;
        app.records()
            .unwrap()
            .record_put(SCOPE, KIND, &task.id, damaged)
            .unwrap();
        assert_eq!(
            app.retry_ready(&task).unwrap_err().code,
            ErrorCode::IntegrityFailed
        );
        assert_eq!(
            app.records()
                .unwrap()
                .record_get(SCOPE, KIND, &task.id)
                .unwrap()
                .unwrap(),
            damaged
        );
    }
}
