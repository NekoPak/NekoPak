//! Upload tasks hold only local identity and opaque vault references, never OSS secrets.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::atomic::Ordering,
    time::Duration,
};
#[derive(Serialize, Deserialize)]
struct UploadRecord {
    scope: String,
    path: PathBuf,
    identity: String,
    parent: String,
    prepared: Option<UploadPrepared>,
    etags: Vec<String>,
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorCode::IntegrityFailed,
        "上传源或任务记录发生变化，未继续写入远端。",
    )
}
impl AppService {
    pub async fn upload_start(
        &self,
        path: &str,
        parent: &str,
        key: &str,
        ctx: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        self.upload_start_at(path, parent, key, &scope, ctx).await
    }
    pub async fn upload_start_at(
        &self,
        path: &str,
        parent: &str,
        key: &str,
        expected_scope: &str,
        ctx: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        validate_destination(path)?;
        let file = local::open_readonly(Path::new(path))?;
        let identity = local::read_identity(&file)?;
        let total = file.metadata().map_err(|_| invalid())?.len();
        self.check_scope(expected_scope, ctx).await?;
        let scope = expected_scope.to_owned();
        let fp = fingerprint(&[path, parent, &identity]);
        let key = scoped_key(&format!("upload:{scope}"), key)?;
        if let Some(task) = self.store()?.lookup_key(&key, &fp)? {
            return Ok(TaskRef { task_id: task.id });
        }
        let mut task = new_task(String::new(), TaskKind::Upload, Some(DecimalU64(total)));
        task.simulated = false;
        task.backend = BackendId::RustCompat;
        task.name = Some(
            Path::new(path)
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(invalid)?
                .into(),
        );
        task.destination = Some(parent.into());
        let record = UploadRecord {
            scope,
            path: path.into(),
            identity,
            parent: parent.into(),
            prepared: None,
            etags: vec![],
        };
        self.save_upload_record(&task.id, &record)?;
        let task = self.store()?.insert(task, Some((&key, &fp)))?;
        self.emit(&task);
        Ok(TaskRef { task_id: task.id })
    }
    fn upload_record(&self, id: &str) -> Result<UploadRecord, ClientError> {
        serde_json::from_str(
            &self
                .records()?
                .record_get("tasks", "uploads", id)?
                .ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())
    }
    fn save_upload_record(&self, id: &str, record: &UploadRecord) -> Result<(), ClientError> {
        self.records()?.record_put(
            "tasks",
            "uploads",
            id,
            &serde_json::to_string(record).map_err(|_| invalid())?,
        )
    }
    pub(crate) fn upload_scope(&self, id: &str) -> Result<String, ClientError> {
        Ok(self.upload_record(id)?.scope)
    }
    pub async fn run_upload(&self) -> Result<(), ClientError> {
        if self.upload_busy.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let result = self.upload_step().await;
        self.upload_busy.store(false, Ordering::SeqCst);
        result
    }
    async fn upload_step(&self) -> Result<(), ClientError> {
        let ctx = RequestContext::new(Duration::from_secs(3600));
        let mut selected = None;
        for task in self
            .store()?
            .active()?
            .into_iter()
            .filter(|t| t.kind == TaskKind::Upload && !t.simulated)
        {
            let record = self.upload_record(&task.id)?;
            if self.check_scope(&record.scope, &ctx).await.is_ok() {
                selected = Some((task, record));
                break;
            }
        }
        let Some((task, record)) = selected else {
            return Ok(());
        };
        if task.state == TaskState::Queued {
            let updated =
                self.store()?
                    .update(&task, TaskState::Running, task.transferred_bytes.0, None)?;
            self.emit(&updated);
            return Ok(());
        }
        self.download_cancellations
            .lock()
            .map_err(|_| invalid())?
            .insert(task.id.clone(), ctx.clone());
        let result = self.upload_step_at(&task, record, &ctx).await;
        self.download_cancellations
            .lock()
            .map_err(|_| invalid())?
            .remove(&task.id);
        if let Err(error) = result {
            let current = self.store()?.get(&task.id)?;
            if current.revision == task.revision {
                let updated = self.store()?.update(
                    &current,
                    TaskState::Failed,
                    current.transferred_bytes.0,
                    Some(error),
                )?;
                self.emit(&updated);
            }
        }
        Ok(())
    }
    async fn upload_step_at(
        &self,
        task: &TransferTask,
        mut record: UploadRecord,
        ctx: &RequestContext,
    ) -> Result<(), ClientError> {
        let mut file = local::open_readonly(&record.path)?;
        if local::read_identity(&file)? != record.identity {
            return Err(invalid());
        }
        if record.prepared.is_none() {
            let key = format!("upload-create:{}", task.id);
            let fp = fingerprint(&[
                &record.path.to_string_lossy(),
                &record.identity,
                &record.parent,
            ]);
            let prepared: UploadPrepared =
                if let Some(result) = self.records()?.begin_value(&record.scope, &key, &fp)? {
                    serde_json::from_value(result).map_err(|_| invalid())?
                } else {
                    let result = ctx
                        .run(self.provider.upload_prepare(
                            &record.path.to_string_lossy(),
                            &record.parent,
                            &record.scope,
                        ))
                        .await
                        .map_err(cloud::unknown_write);
                    self.records()?.finish_value(
                        &record.scope,
                        &key,
                        &result
                            .as_ref()
                            .map(|v| serde_json::to_value(v).expect("upload metadata serializes"))
                            .map_err(Clone::clone),
                    )?;
                    result?
                };
            if Some(prepared.total_bytes) != task.total_bytes {
                return Err(invalid());
            }
            record.prepared = Some(prepared);
            self.save_upload_record(&task.id, &record)?;
        }
        let prepared = record.prepared.as_ref().ok_or_else(invalid)?;
        if prepared.part_bytes == 0 || prepared.part_bytes > 16 * 1024 * 1024 {
            return Err(invalid());
        }
        self.check_scope(&record.scope, ctx).await?;
        if local::read_identity(&file)? != record.identity {
            return Err(invalid());
        }
        let restored = if prepared.completed {
            let entry = ctx.run(self.provider.stat(&prepared.remote_id)).await?;
            if entry.size_bytes != prepared.total_bytes
                || entry.hash.as_ref().is_none_or(|h| {
                    h.algorithm != HashAlgorithm::Gcid
                        || !h.value.eq_ignore_ascii_case(&prepared.gcid)
                })
            {
                return Err(invalid());
            }
            prepared.clone()
        } else {
            ctx.run(
                self.provider
                    .upload_open(&prepared.upload_ref, &record.scope),
            )
            .await?
        };
        if restored.completed {
            self.check_scope(&record.scope, ctx).await?;
            let current = self.store()?.get(&task.id)?;
            if current.revision != task.revision {
                return Ok(());
            }
            let verifying = if current.state != TaskState::Verifying {
                self.store()?.update(
                    &current,
                    TaskState::Verifying,
                    prepared.total_bytes.0,
                    None,
                )?
            } else {
                current
            };
            let completed = self.store()?.update(
                &verifying,
                TaskState::Completed,
                prepared.total_bytes.0,
                None,
            )?;
            self.emit(&completed);
            if !prepared.upload_ref.is_empty() {
                ctx.run(
                    self.provider
                        .upload_forget(&prepared.upload_ref, &record.scope),
                )
                .await?;
            }
            return Ok(());
        }
        let total = prepared.total_bytes.0;
        let offset = record.etags.len() as u64 * prepared.part_bytes;
        if offset < total || (total == 0 && record.etags.is_empty()) {
            file.seek(SeekFrom::Start(offset)).map_err(|_| invalid())?;
            let mut bytes =
                vec![0; (total.saturating_sub(offset).min(prepared.part_bytes)) as usize];
            file.read_exact(&mut bytes).map_err(|_| invalid())?;
            let tag = ctx
                .run(self.provider.upload_part(
                    &prepared.upload_ref,
                    record.etags.len() as u32 + 1,
                    bytes,
                    &record.scope,
                ))
                .await?;
            if local::read_identity(&file)? != record.identity {
                return Err(invalid());
            }
            self.check_scope(&record.scope, ctx).await?;
            record.etags.push(tag);
            self.save_upload_record(&task.id, &record)?;
            let current = self.store()?.get(&task.id)?;
            if current.revision != task.revision {
                return Ok(());
            }
            let done = (record.etags.len() as u64
                * record.prepared.as_ref().ok_or_else(invalid)?.part_bytes)
                .min(total);
            let updated = self.store()?.update(
                &current,
                if done == total {
                    TaskState::Verifying
                } else {
                    TaskState::Running
                },
                done,
                None,
            )?;
            self.emit(&updated);
            return Ok(());
        }
        let entry = ctx
            .run(
                self.provider
                    .upload_finish(&prepared.upload_ref, &record.etags, &record.scope),
            )
            .await
            .map_err(cloud::unknown_write)?;
        self.check_scope(&record.scope, ctx).await?;
        if entry.id != prepared.remote_id || entry.size_bytes != prepared.total_bytes {
            return Err(invalid());
        }
        let current = self.store()?.get(&task.id)?;
        if current.revision != task.revision {
            return Ok(());
        }
        let verifying = if current.state != TaskState::Verifying {
            self.store()?
                .update(&current, TaskState::Verifying, total, None)?
        } else {
            current
        };
        let completed = self
            .store()?
            .update(&verifying, TaskState::Completed, total, None)?;
        self.emit(&completed);
        ctx.run(
            self.provider
                .upload_forget(&prepared.upload_ref, &record.scope),
        )
        .await?;
        Ok(())
    }
}
