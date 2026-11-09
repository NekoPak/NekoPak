//! Durable native downloads: verified ranges, private temporary file, explicit restart and no overwrite.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
};

const BLOCK: u64 = 1024 * 1024;
#[derive(Serialize, Deserialize)]
struct Receipt {
    length: u64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
struct Download {
    scope: String,
    identity: String,
    destination: PathBuf,
    part: PathBuf,
    total: u64,
    receipts: Vec<Receipt>,
    publish_started: bool,
    file_identity: Option<String>,
}
fn fault(code: ErrorCode, message: &'static str) -> ClientError {
    ClientError::new(code, message).for_backend(BackendId::RustCompat)
}
fn io_fault(e: std::io::Error) -> ClientError {
    let code = if e.kind() == std::io::ErrorKind::StorageFull {
        ErrorCode::DiskFull
    } else if e.kind() == std::io::ErrorKind::AlreadyExists {
        ErrorCode::Conflict
    } else {
        ErrorCode::PermissionDenied
    };
    fault(
        code,
        "无法写入下载目录，请检查空间、文件占用或同名文件；已确认进度会保留。",
    )
}

/// Registration lives for exactly one worker step, including when its future
/// is dropped. The same gate as pause/cancel closes the late-registration race.
pub(crate) struct TransferRequest<'a> {
    service: &'a AppService,
    id: String,
}
impl Drop for TransferRequest<'_> {
    fn drop(&mut self) {
        if let Ok(mut requests) = self.service.download_cancellations.lock() {
            requests.remove(&self.id);
        }
    }
}

impl AppService {
    pub(crate) fn register_transfer_request(
        &self,
        task: &TransferTask,
        context: &RequestContext,
    ) -> Result<Option<TransferRequest<'_>>, ClientError> {
        let _gate = self
            .download_gate
            .lock()
            .map_err(|_| fault(ErrorCode::Internal, "下载队列暂不可用。"))?;
        {
            let mut requests = self
                .download_cancellations
                .lock()
                .map_err(|_| fault(ErrorCode::Internal, "下载队列暂不可用。"))?;
            if requests.contains_key(&task.id) {
                return Ok(None);
            }
            requests.insert(task.id.clone(), context.clone());
        }
        let registration = TransferRequest {
            service: self,
            id: task.id.clone(),
        };
        let current = self.store()?.get(&task.id)?;
        if !self.is_foreground()
            || current.revision != task.revision
            || !matches!(
                current.state,
                TaskState::Running | TaskState::Retrying | TaskState::Verifying
            )
        {
            context.cancel();
            return Ok(None);
        }
        Ok(Some(registration))
    }
    pub(crate) fn download_scope(&self, id: &str) -> Result<String, ClientError> {
        Ok(self.download_record(id)?.scope)
    }
    fn journal(&self) -> Result<&intents::IntentStore, ClientError> {
        self.intents
            .as_ref()
            .ok_or_else(|| fault(ErrorCode::Internal, "下载记录不可用。"))
    }
    fn download_record(&self, id: &str) -> Result<Download, ClientError> {
        let payload = self.journal()?.download_get(id)?.ok_or_else(|| {
            fault(
                ErrorCode::IntegrityFailed,
                "下载记录缺失，未重新创建临时文件。",
            )
        })?;
        serde_json::from_str(&payload).map_err(|_| {
            fault(
                ErrorCode::IntegrityFailed,
                "下载记录无法读取，已保留原文件。",
            )
        })
    }
    fn save_download(&self, id: &str, download: &Download) -> Result<(), ClientError> {
        self.journal()?.download_put(
            id,
            &serde_json::to_string(download)
                .map_err(|_| fault(ErrorCode::Internal, "无法保存下载进度。"))?,
        )
    }
    pub(super) async fn start_native_download(
        &self,
        id: &str,
        directory: &str,
        key: Option<&str>,
        context: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        let scope = context.run(self.provider.account_scope()).await?;
        self.start_native_download_at(id, directory, key, &scope, context)
            .await
    }
    pub(crate) async fn start_native_download_at(
        &self,
        id: &str,
        directory: &str,
        key: Option<&str>,
        expected_scope: &str,
        context: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        self.store()?;
        validate_destination(directory)?;
        let directory = Path::new(directory);
        let _parents = lock_parents(directory)?;
        if !directory.is_dir() {
            return Err(fault(
                ErrorCode::Conflict,
                "下载目录不存在，请在设置中选择现有目录。",
            ));
        }
        let info = context.run(self.provider.download_info(id)).await?;
        if info.account_scope != expected_scope {
            return Err(fault(
                ErrorCode::AuthExpired,
                "账号已改变，未创建下载任务。",
            ));
        }
        let fingerprint = fingerprint(&[
            &info.account_scope,
            id,
            directory.to_string_lossy().as_ref(),
        ]);
        let key = key
            .map(|key| scoped_key(&format!("download:{}", info.account_scope), key))
            .transpose()?;
        if let Some(existing) = key
            .as_deref()
            .map(|key| self.store()?.lookup_key(key, &fingerprint))
            .transpose()?
            .flatten()
        {
            return Ok(TaskRef {
                task_id: existing.id,
            });
        }
        let name = safe_name(&info.entry.name);
        let destination = directory.join(&name);
        if destination.try_exists().map_err(io_fault)? {
            return Err(fault(
                ErrorCode::Conflict,
                "下载目录已有同名文件，请更换目录或重命名现有文件。",
            ));
        }
        let mut task = new_task(id.into(), TaskKind::Download, Some(info.entry.size_bytes));
        task.backend = BackendId::RustCompat;
        task.simulated = false;
        task.name = Some(name);
        task.destination = Some(destination.to_string_lossy().into_owned());
        let part = directory.join(format!(".nekopak-{}.part", task.id));
        if part.try_exists().map_err(io_fault)? {
            return Err(fault(ErrorCode::Conflict, "临时文件已存在，未覆盖。"));
        }
        self.check_scope(expected_scope, context).await?;
        self.save_download(
            &task.id,
            &Download {
                scope: info.account_scope,
                identity: info.identity,
                destination,
                part,
                total: info.entry.size_bytes.0,
                receipts: vec![],
                publish_started: false,
                file_identity: None,
            },
        )?;
        let _gate = self
            .download_gate
            .lock()
            .map_err(|_| fault(ErrorCode::Internal, "下载队列暂不可用。"))?;
        if !self.is_foreground() {
            task.state = TaskState::Paused;
        }
        let task = self
            .store()?
            .insert(task, key.as_deref().map(|key| (key, fingerprint.as_str())))?;
        self.emit(&task);
        Ok(TaskRef { task_id: task.id })
    }
    pub async fn visible_transfers(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
        context: &RequestContext,
    ) -> Result<Page<TransferTask>, ClientError> {
        let mut page = self.transfers_list(cursor, limit)?;
        if self.provider.id() != BackendId::Mock {
            let scope = if context
                .run(self.provider.auth_status())
                .await?
                .authenticated
            {
                Some(context.run(self.provider.account_scope()).await?)
            } else {
                None
            };
            let mut visible = Vec::new();
            for mut task in page.items {
                let task_scope = if task.kind == TaskKind::CloudDownload {
                    self.cloud_scope(&task.id)?
                } else if task.kind == TaskKind::SegmentDownload {
                    self.media_scope(&task.id)?
                } else if task.kind == TaskKind::Upload {
                    self.upload_scope(&task.id)?
                } else {
                    self.download_record(&task.id)?.scope
                };
                if task_scope == "local" || scope.as_deref() == Some(task_scope.as_str()) {
                    task.speed_bytes_per_sec = if task.state == TaskState::Running {
                        self.download_rates.speed(&task.id)
                    } else {
                        None
                    };
                    visible.push(task);
                }
            }
            page.items = visible;
        }
        Ok(page)
    }
    pub async fn control_visible_transfer(
        &self,
        id: &str,
        action: TransferAction,
        context: &RequestContext,
    ) -> Result<(), ClientError> {
        if self.provider.id() != BackendId::Mock
            && self.store()?.get(id)?.kind == TaskKind::CloudDownload
        {
            return self.control_cloud(id, action, context).await;
        }
        if self.provider.id() != BackendId::Mock
            && self.store()?.get(id)?.kind == TaskKind::SegmentDownload
        {
            self.check_scope(&self.media_scope(id)?, context).await?;
            return self.transfer_control(id, action);
        }
        if self.provider.id() != BackendId::Mock && self.store()?.get(id)?.kind == TaskKind::Upload
        {
            self.check_scope(&self.upload_scope(id)?, context).await?;
            return self.transfer_control(id, action);
        }
        if self.provider.id() != BackendId::Mock {
            let scope = context.run(self.provider.account_scope()).await?;
            if self.download_record(id)?.scope != scope {
                return Err(fault(ErrorCode::PermissionDenied, "此任务不属于当前账号。"));
            }
        }
        self.transfer_control(id, action)
    }
    /// Resolve a task-owned directory; IPC never accepts an arbitrary path to open.
    pub async fn transfer_directory(
        &self,
        id: &str,
        context: &RequestContext,
    ) -> Result<PathBuf, ClientError> {
        let task = self.store()?.get(id)?;
        if task.kind == TaskKind::SegmentDownload {
            return self.media_directory(id, context).await;
        }
        if task.backend != BackendId::RustCompat || task.kind != TaskKind::Download {
            return Err(fault(ErrorCode::Unsupported, "此任务没有本地下载目录。"));
        }
        let download = self.download_record(id)?;
        if context.run(self.provider.account_scope()).await? != download.scope {
            return Err(fault(ErrorCode::PermissionDenied, "此任务不属于当前账号。"));
        }
        let parent = download
            .destination
            .parent()
            .ok_or_else(|| fault(ErrorCode::IntegrityFailed, "下载目录记录无效。"))?;
        validate_destination(&parent.to_string_lossy())?;
        let _parents = lock_parents(parent)?;
        Ok(parent.to_owned())
    }
    /// Resolve only a completed task belonging to the current account. The
    /// returned read-only handle pins the validated file across a system picker.
    pub async fn completed_transfer_file(
        &self,
        id: &str,
        context: &RequestContext,
    ) -> Result<(String, File, u64), ClientError> {
        let task = self.store()?.get(id)?;
        if task.state != TaskState::Completed
            || task.simulated
            || !matches!(task.kind, TaskKind::Download | TaskKind::SegmentDownload)
        {
            return Err(fault(ErrorCode::Conflict, "只能导出已完成的本地下载。"));
        }
        let directory = self.transfer_directory(id, context).await?;
        let destination = task
            .destination
            .as_deref()
            .map(Path::new)
            .ok_or_else(|| fault(ErrorCode::IntegrityFailed, "下载路径记录缺失。"))?;
        if destination.parent() != Some(directory.as_path()) {
            return Err(fault(ErrorCode::IntegrityFailed, "下载路径记录不一致。"));
        }
        let file = crate::local::open_readonly(destination)?;
        let length = file.metadata().map_err(io_fault)?.len();
        if task.total_bytes != Some(DecimalU64(length)) || task.transferred_bytes.0 != length {
            return Err(fault(
                ErrorCode::IntegrityFailed,
                "下载文件长度已改变，未导出。",
            ));
        }
        let name = destination
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| fault(ErrorCode::IntegrityFailed, "下载文件名无效。"))?;
        Ok((name.into(), file, length))
    }
    pub(super) async fn tick_native_download(&self) -> Result<(), ClientError> {
        if !self.is_foreground() {
            return Ok(());
        }
        let active = self.store()?.active()?;
        let Some(task) = active
            .into_iter()
            .find(|t| t.backend == BackendId::RustCompat && t.kind == TaskKind::Download)
        else {
            return Ok(());
        };
        let result = self.step_native_download(&task).await;
        if result.is_err() {
            self.download_rates.remove(&task.id);
        }
        match result {
            Ok(()) => {}
            Err(e) if e.code == ErrorCode::Conflict => {
                if self.store()?.get(&task.id)?.revision == task.revision {
                    let updated = self.store()?.update(
                        &task,
                        TaskState::Failed,
                        task.transferred_bytes.0,
                        Some(e),
                    )?;
                    self.emit(&updated);
                }
            }
            Err(e) => {
                let current = self.store()?.get(&task.id)?;
                if current.revision == task.revision {
                    let state = if e.code == ErrorCode::AuthExpired
                        && current.state != TaskState::Verifying
                    {
                        TaskState::Paused
                    } else {
                        TaskState::Failed
                    };
                    let updated = self.store()?.update(
                        &current,
                        state,
                        current.transferred_bytes.0,
                        Some(e),
                    )?;
                    self.emit(&updated);
                }
            }
        }
        Ok(())
    }
    async fn step_native_download(&self, task: &TransferTask) -> Result<(), ClientError> {
        let mut download = self.download_record(&task.id)?;
        if task.total_bytes != Some(DecimalU64(download.total)) {
            return Err(fault(ErrorCode::IntegrityFailed, "下载长度记录不一致。"));
        }
        let scope = RequestContext::new(std::time::Duration::from_secs(45))
            .run(self.provider.account_scope())
            .await?;
        if scope != download.scope {
            return Err(fault(
                ErrorCode::AuthExpired,
                "请登录创建此任务的账号后继续下载。",
            ));
        }
        if task.state == TaskState::Queued {
            let _gate = self
                .download_gate
                .lock()
                .map_err(|_| fault(ErrorCode::Internal, "下载队列暂不可用。"))?;
            if !self.is_foreground() || self.store()?.get(&task.id)?.revision != task.revision {
                return Ok(());
            }
            let updated =
                self.store()?
                    .update(task, TaskState::Running, task.transferred_bytes.0, None)?;
            self.emit(&updated);
            self.download_rates.remove(&task.id);
            return Ok(());
        }
        if task.state == TaskState::Verifying {
            if self.store()?.get(&task.id)?.revision != task.revision {
                return Ok(());
            }
            let _parents = lock_parents(
                download
                    .destination
                    .parent()
                    .ok_or_else(|| fault(ErrorCode::IntegrityFailed, "下载路径记录不完整。"))?,
            )?;
            let recovery = download.publish_started && download.destination.exists();
            let mut file = open_part(
                if recovery {
                    &download.destination
                } else {
                    &download.part
                },
                false,
            )?;
            if download.file_identity.as_ref() != Some(&file_identity(&file)?) {
                return Err(fault(
                    ErrorCode::IntegrityFailed,
                    "临时文件身份发生变化，未发布。",
                ));
            }
            if file.metadata().map_err(io_fault)?.len() != download.total {
                return Err(fault(
                    ErrorCode::IntegrityFailed,
                    "最终文件长度不符，未发布或标记完成。",
                ));
            }
            self.verify_download_receipts(&mut file, &download, download.total, task)
                .await?;
            let _guard = self
                .download_gate
                .lock()
                .map_err(|_| fault(ErrorCode::Internal, "下载队列暂不可用。"))?;
            if self.store()?.get(&task.id)?.revision != task.revision {
                return Ok(());
            }
            if !recovery {
                download.publish_started = true;
                self.save_download(&task.id, &download)?;
                file.sync_all().map_err(io_fault)?;
                publish(&file, &download.part, &download.destination)?;
            }
            let updated = self
                .store()?
                .update(task, TaskState::Completed, download.total, None)?;
            self.verified_downloads
                .lock()
                .map_err(|_| fault(ErrorCode::Internal, "下载校验状态不可用。"))?
                .remove(&task.id);
            self.emit(&updated);
            return Ok(());
        }
        let start = task.transferred_bytes.0;
        if start > download.total {
            return Err(fault(ErrorCode::IntegrityFailed, "下载进度超过文件长度。"));
        }
        if start == download.total && download.publish_started {
            let updated = self
                .store()?
                .update(task, TaskState::Verifying, start, None)?;
            self.emit(&updated);
            return Ok(());
        }
        let settings = self.settings_get()?;
        let rate = settings["bandwidth_limit_kib_s"].as_u64().unwrap_or(0);
        let block = if rate > 0 {
            (rate * 512).clamp(1024, BLOCK)
        } else {
            BLOCK
        };
        let bytes = if start < download.total {
            let context = RequestContext::new(std::time::Duration::from_secs(90));
            let Some(_registration) = self.register_transfer_request(task, &context)? else {
                return Ok(());
            };
            let observer = self.download_rates.start(&task.id, task.revision.0, start);
            let result = context
                .run(self.provider.download_chunk(
                    &task.remote_id,
                    &download.identity,
                    &download.scope,
                    start,
                    block,
                    observer,
                ))
                .await;
            let chunk = result?;
            if chunk.total != download.total || chunk.bytes.is_empty() {
                return Err(fault(ErrorCode::IntegrityFailed, "下载长度发生变化。"));
            }
            chunk.bytes
        } else {
            vec![]
        };
        if rate > 0 {
            tokio::time::sleep(std::time::Duration::from_secs_f64(
                bytes.len() as f64 / (rate as f64 * 1024.0),
            ))
            .await;
        }
        if self.store()?.get(&task.id)?.revision != task.revision {
            return Ok(());
        }
        let _parents = lock_parents(
            download
                .part
                .parent()
                .ok_or_else(|| fault(ErrorCode::IntegrityFailed, "下载路径记录不完整。"))?,
        )?;
        let mut committed = 0;
        download.receipts.retain(|r| {
            committed += r.length;
            committed <= start
        });
        if download.receipts.iter().map(|r| r.length).sum::<u64>() != start {
            return Err(fault(
                ErrorCode::IntegrityFailed,
                "下载块记录与进度不一致，已停止写入。",
            ));
        }
        let mut file = open_part(&download.part, download.file_identity.is_none())?;
        let identity = file_identity(&file)?;
        if let Some(expected) = &download.file_identity {
            if *expected != identity {
                return Err(fault(
                    ErrorCode::IntegrityFailed,
                    "临时文件已被替换，未写入。",
                ));
            }
        } else {
            download.file_identity = Some(identity);
            self.save_download(&task.id, &download)?;
        }
        // On resume/restart only bytes with a durable receipt are accepted.
        let stamp = file
            .metadata()
            .map_err(io_fault)?
            .modified()
            .map_err(io_fault)?;
        let cached = self
            .verified_downloads
            .lock()
            .map_err(|_| fault(ErrorCode::Internal, "下载校验状态不可用。"))?
            .get(&task.id)
            .copied();
        if cached != Some((stamp, start)) {
            self.verify_download_receipts(&mut file, &download, start, task)
                .await?;
        }
        let _guard = self
            .download_gate
            .lock()
            .map_err(|_| fault(ErrorCode::Internal, "下载队列暂不可用。"))?;
        if self.store()?.get(&task.id)?.revision != task.revision {
            return Ok(());
        }
        file.set_len(start).map_err(io_fault)?;
        file.seek(SeekFrom::Start(start)).map_err(io_fault)?;
        file.write_all(&bytes).map_err(io_fault)?;
        file.sync_all().map_err(io_fault)?;
        if !bytes.is_empty() {
            download.receipts.push(Receipt {
                length: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(&bytes)),
            });
        }
        self.save_download(&task.id, &download)?;
        let transferred = start + bytes.len() as u64;
        let updated = self.store()?.update(
            task,
            if transferred == download.total {
                TaskState::Verifying
            } else {
                TaskState::Running
            },
            transferred,
            None,
        )?;
        self.verified_downloads
            .lock()
            .map_err(|_| fault(ErrorCode::Internal, "下载校验状态不可用。"))?
            .insert(
                task.id.clone(),
                (
                    file.metadata()
                        .map_err(io_fault)?
                        .modified()
                        .map_err(io_fault)?,
                    transferred,
                ),
            );
        self.emit(&updated);
        if updated.state != TaskState::Running {
            self.download_rates.remove(&task.id);
        }
        Ok(())
    }
    async fn verify_download_receipts(
        &self,
        file: &mut File,
        download: &Download,
        length: u64,
        task: &TransferTask,
    ) -> Result<(), ClientError> {
        if file.metadata().map_err(io_fault)?.len() < length {
            return Err(fault(
                ErrorCode::IntegrityFailed,
                "临时文件短于已确认进度，未静默重置。",
            ));
        }
        file.seek(SeekFrom::Start(0)).map_err(io_fault)?;
        let mut sum = 0_u64;
        for (index, receipt) in download.receipts.iter().enumerate() {
            if self.store()?.get(&task.id)?.revision != task.revision {
                return Err(fault(ErrorCode::Cancelled, "下载已暂停或取消。"));
            }
            if index % 16 == 0
                && RequestContext::new(std::time::Duration::from_secs(45))
                    .run(self.provider.account_scope())
                    .await?
                    != download.scope
            {
                return Err(fault(ErrorCode::AuthExpired, "账号已改变，校验已停止。"));
            }
            if receipt.length > BLOCK || receipt.length == 0 {
                return Err(fault(ErrorCode::IntegrityFailed, "下载块记录损坏。"));
            }
            let mut bytes = vec![0; receipt.length as usize];
            file.read_exact(&mut bytes).map_err(io_fault)?;
            if format!("{:x}", Sha256::digest(&bytes)) != receipt.sha256 {
                return Err(fault(
                    ErrorCode::IntegrityFailed,
                    "临时文件校验失败，未追加或发布内容。",
                ));
            }
            sum = sum
                .checked_add(receipt.length)
                .ok_or_else(|| fault(ErrorCode::IntegrityFailed, "下载长度溢出。"))?;
            tokio::task::yield_now().await;
        }
        if sum != length {
            return Err(fault(ErrorCode::IntegrityFailed, "下载块总长与进度不符。"));
        }
        Ok(())
    }
}

pub(crate) fn safe_name(name: &str) -> String {
    let mut name: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(180)
        .collect();
    name = name.trim_end_matches([' ', '.']).to_owned();
    if name.is_empty() {
        name = "download".into();
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")))
    {
        name.insert(0, '_');
    }
    name
}
pub(crate) fn open_part(path: &Path, create: bool) -> Result<File, ClientError> {
    if !create {
        let m = std::fs::symlink_metadata(path).map_err(io_fault)?;
        if m.file_type().is_symlink() || !m.is_file() {
            return Err(fault(
                ErrorCode::PermissionDenied,
                "临时文件类型异常，未写入。",
            ));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x400 != 0 {
                return Err(fault(
                    ErrorCode::PermissionDenied,
                    "不向重解析点写入下载内容。",
                ));
            }
        }
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(create);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .access_mode(0x80000000 | 0x40000000 | 0x00010000)
            .share_mode(1)
            .custom_flags(0x00200000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(io_fault)?;
    file.try_lock()
        .map_err(|_| fault(ErrorCode::Conflict, "另一个进程正在使用临时文件。"))?;
    Ok(file)
}
pub(crate) fn publish(file: &File, part: &Path, destination: &Path) -> Result<(), ClientError> {
    #[cfg(windows)]
    {
        use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            FileRenameInfo, SetFileInformationByHandle, FILE_RENAME_INFO,
        };
        let _ = part;
        let target: Vec<u16> = destination.as_os_str().encode_wide().collect();
        let tail = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
        let size = tail + (target.len() + 1) * 2;
        let mut buffer = vec![0u64; size.div_ceil(8)];
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        // SAFETY: u64 storage meets FILE_RENAME_INFO alignment and includes the full UTF-16 tail.
        // ReplaceIfExists remains zero. Rename operates on the locked file handle, not a raced source path.
        unsafe {
            (*info).FileNameLength = (target.len() * 2) as u32;
            std::ptr::copy_nonoverlapping(
                target.as_ptr(),
                buffer.as_mut_ptr().cast::<u8>().add(tail).cast::<u16>(),
                target.len(),
            );
            if SetFileInformationByHandle(
                file.as_raw_handle(),
                FileRenameInfo,
                info.cast(),
                size as u32,
            ) == 0
            {
                return Err(io_fault(std::io::Error::last_os_error()));
            }
        }
        Ok(())
    }
    #[cfg(target_os = "android")]
    {
        // Android SELinux forbids hard links in app_data_file. renameat2's
        // NOREPLACE flag gives atomic publication without ever overwriting an
        // existing destination. syscall avoids a newer Bionic symbol on API24.
        use std::os::unix::ffi::OsStrExt;
        let source = std::ffi::CString::new(part.as_os_str().as_bytes())
            .map_err(|_| fault(ErrorCode::Conflict, "下载路径无效。"))?;
        let target = std::ffi::CString::new(destination.as_os_str().as_bytes())
            .map_err(|_| fault(ErrorCode::Conflict, "下载路径无效。"))?;
        file.sync_all().map_err(io_fault)?;
        // SAFETY: both NUL-terminated paths live through the syscall; flags and
        // directory descriptors are the platform's constants, no pointers escape.
        let result = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD,
                source.as_ptr(),
                libc::AT_FDCWD,
                target.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result != 0 {
            return Err(io_fault(std::io::Error::last_os_error()));
        }
        File::open(
            destination
                .parent()
                .ok_or_else(|| fault(ErrorCode::Conflict, "下载路径无效。"))?,
        )
        .and_then(|directory| directory.sync_all())
        .map_err(io_fault)
    }
    #[cfg(all(not(windows), not(target_os = "android")))]
    {
        let _ = file;
        std::fs::hard_link(part, destination).map_err(io_fault)?;
        std::fs::remove_file(part).map_err(io_fault)
    }
}

pub(crate) fn file_identity(file: &File) -> Result<String, ClientError> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        // SAFETY: handle remains owned by File; the API initializes this output on success.
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) };
        if ok == 0 {
            return Err(io_fault(std::io::Error::last_os_error()));
        }
        let info = unsafe { info.assume_init() };
        Ok(format!(
            "{:x}:{:x}:{:x}",
            info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
        ))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = file.metadata().map_err(io_fault)?;
        Ok(format!("{}:{}", m.dev(), m.ino()))
    }
}

pub(crate) fn lock_parents(directory: &Path) -> Result<Vec<File>, ClientError> {
    #[cfg(windows)]
    let mut parents = Vec::new();
    #[cfg(not(windows))]
    let parents = Vec::new();
    let mut current = PathBuf::new();
    for component in directory.components() {
        current.push(component);
        if matches!(component, std::path::Component::Prefix(_)) {
            continue;
        }
        let m = std::fs::symlink_metadata(&current).map_err(io_fault)?;
        if m.file_type().is_symlink() || !m.is_dir() {
            return Err(fault(
                ErrorCode::PermissionDenied,
                "下载目录含符号链接或非目录项，请选择本地实际目录。",
            ));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
            if m.file_attributes() & 0x400 != 0 {
                return Err(fault(
                    ErrorCode::PermissionDenied,
                    "下载目录含重解析点，请选择本地实际目录。",
                ));
            }
            let file = OpenOptions::new()
                .read(true)
                .share_mode(3)
                .custom_flags(0x02200000)
                .open(&current)
                .map_err(io_fault)?;
            if file.metadata().map_err(io_fault)?.file_attributes() & 0x400 != 0 {
                return Err(fault(
                    ErrorCode::PermissionDenied,
                    "下载目录在检查时发生变化。",
                ));
            }
            parents.push(file);
        }
    }
    Ok(parents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use nekopak_provider::{DownloadChunk, DownloadInfo};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct ByteProvider {
        bytes: Vec<u8>,
        scope: std::sync::Mutex<String>,
        fail_at: AtomicUsize,
    }
    #[async_trait]
    impl ProviderPort for ByteProvider {
        fn id(&self) -> BackendId {
            BackendId::RustCompat
        }
        fn supports_downloads(&self) -> bool {
            true
        }
        async fn discover(&self) -> Result<CapabilityReport, ClientError> {
            unreachable!()
        }
        async fn list(
            &self,
            _: &str,
            _: Option<&str>,
            _: usize,
        ) -> Result<Page<RemoteEntry>, ClientError> {
            unreachable!()
        }
        async fn stat(&self, _: &str) -> Result<RemoteEntry, ClientError> {
            unreachable!()
        }
        async fn quota(&self) -> Result<QuotaReport, ClientError> {
            unreachable!()
        }
        async fn auth_status(&self) -> Result<AuthState, ClientError> {
            Ok(AuthState {
                backend: self.id(),
                authenticated: true,
                account_id: None,
                credential_owner: "test".into(),
                account_tier: AccountTier::Unknown,
                message: String::new(),
                state: AuthStatus::SignedIn,
                display_name: None,
            })
        }
        async fn account_scope(&self) -> Result<String, ClientError> {
            Ok(self.scope.lock().unwrap().clone())
        }
        async fn download_info(&self, id: &str) -> Result<DownloadInfo, ClientError> {
            Ok(DownloadInfo {
                entry: RemoteEntry {
                    id: id.into(),
                    parent_id: Some("root".into()),
                    name: "真实下载样本.bin".into(),
                    kind: EntryKind::File,
                    size_bytes: DecimalU64(self.bytes.len() as u64),
                    modified_at: Some(now()),
                    etag: None,
                    hash: None,
                    mime_type: None,
                    starred: None,
                },
                identity: "fixed-file-version".into(),
                account_scope: self.account_scope().await?,
            })
        }
        async fn download_chunk(
            &self,
            _: &str,
            _: &str,
            scope: &str,
            start: u64,
            length: u64,
            observer: nekopak_provider::DownloadObserver,
        ) -> Result<DownloadChunk, ClientError> {
            assert_eq!(scope, self.account_scope().await?);
            if start as usize == self.fail_at.load(Ordering::Relaxed) {
                return Err(fault(ErrorCode::TransientNetwork, "测试连接中断"));
            }
            let end = (start + length).min(self.bytes.len() as u64) as usize;
            observer.received((end - start as usize) as u64);
            Ok(DownloadChunk {
                bytes: self.bytes[start as usize..end].to_vec(),
                total: self.bytes.len() as u64,
            })
        }
    }
    fn provider(size: usize) -> Arc<ByteProvider> {
        Arc::new(ByteProvider {
            bytes: (0..size).map(|i| (i % 251) as u8).collect(),
            scope: std::sync::Mutex::new("account-a".into()),
            fail_at: AtomicUsize::new(usize::MAX),
        })
    }
    async fn setup(size: usize) -> (tempfile::TempDir, Arc<ByteProvider>, AppService, TaskRef) {
        let directory = tempfile::tempdir().unwrap();
        let provider = provider(size);
        let app = AppService::with_provider(&directory.path().join("tasks.db"), provider.clone());
        let task = app
            .transfer_start(
                "file-1",
                directory.path().to_str().unwrap(),
                Some("user-intent"),
                &RequestContext::default(),
            )
            .await
            .unwrap();
        (directory, provider, app, task)
    }
    async fn finish(app: &AppService, id: &str) {
        for _ in 0..12 {
            if app.task(id).unwrap().state == TaskState::Completed {
                return;
            }
            app.tick().await.unwrap();
        }
        panic!("download did not finish: {:?}", app.task(id).unwrap());
    }
    #[tokio::test]
    async fn completed_export_requires_finished_current_account_and_unchanged_length() {
        let (directory, provider, app, task) = setup(127).await;
        let context = RequestContext::default();
        assert_eq!(
            app.completed_transfer_file(&task.task_id, &context)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        finish(&app, &task.task_id).await;
        let (name, mut handle, length) = app
            .completed_transfer_file(&task.task_id, &context)
            .await
            .unwrap();
        assert_eq!(name, "真实下载样本.bin");
        assert_eq!(length, 127);
        let mut bytes = vec![];
        handle.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, provider.bytes);
        drop(handle);
        *provider.scope.lock().unwrap() = "account-b".into();
        assert_eq!(
            app.completed_transfer_file(&task.task_id, &context)
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        *provider.scope.lock().unwrap() = "account-a".into();
        OpenOptions::new()
            .append(true)
            .open(directory.path().join(&name))
            .unwrap()
            .write_all(b"changed-after-download")
            .unwrap();
        assert_eq!(
            app.completed_transfer_file(&task.task_id, &context)
                .await
                .unwrap_err()
                .code,
            ErrorCode::IntegrityFailed
        );
    }
    #[tokio::test]
    async fn real_bytes_survive_pause_network_failure_and_restart() {
        let (directory, provider, app, task) = setup(BLOCK as usize * 2 + 31).await;
        app.tick().await.unwrap();
        app.tick().await.unwrap();
        assert_eq!(
            app.task(&task.task_id).unwrap().transferred_bytes,
            DecimalU64(BLOCK)
        );
        app.transfer_control(&task.task_id, TransferAction::Pause)
            .unwrap();
        app.tick().await.unwrap();
        assert_eq!(
            app.task(&task.task_id).unwrap().transferred_bytes,
            DecimalU64(BLOCK)
        );
        app.transfer_control(&task.task_id, TransferAction::Resume)
            .unwrap();
        app.tick().await.unwrap();
        provider.fail_at.store(BLOCK as usize, Ordering::Relaxed);
        app.tick().await.unwrap();
        assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Failed);
        provider.fail_at.store(usize::MAX, Ordering::Relaxed);
        app.transfer_control(&task.task_id, TransferAction::Retry)
            .unwrap();
        app.tick().await.unwrap();
        drop(app);
        let app = AppService::with_provider(&directory.path().join("tasks.db"), provider.clone());
        assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Paused);
        app.transfer_control(&task.task_id, TransferAction::Resume)
            .unwrap();
        finish(&app, &task.task_id).await;
        assert_eq!(
            std::fs::read(directory.path().join("真实下载样本.bin")).unwrap(),
            provider.bytes
        );
        assert!(app
            .start_native_download(
                "file-1",
                directory.path().to_str().unwrap(),
                Some("different"),
                &RequestContext::default()
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn native_transfer_snapshot_reports_measured_speed_and_clears_it_on_pause() {
        let (_directory, _, app, task) = setup(BLOCK as usize * 3).await;
        app.tick().await.unwrap();
        app.tick().await.unwrap();
        let snapshot = app
            .visible_transfers(None, None, &RequestContext::default())
            .await
            .unwrap();
        let running = snapshot
            .items
            .iter()
            .find(|t| t.id == task.task_id)
            .unwrap();
        assert!(
            running
                .speed_bytes_per_sec
                .is_some_and(|v| v.is_finite() && v > 0.0),
            "Real transferred bytes must produce a measured rate in the UI snapshot"
        );
        app.transfer_control(&task.task_id, TransferAction::Pause)
            .unwrap();
        let snapshot = app
            .visible_transfers(None, None, &RequestContext::default())
            .await
            .unwrap();
        assert!(snapshot.items[0].speed_bytes_per_sec.is_none());
    }
    #[tokio::test]
    async fn final_verification_rejects_appended_data_and_does_not_publish() {
        let (directory, _, app, task) = setup(64).await;
        app.tick().await.unwrap();
        app.tick().await.unwrap();
        let record = app.download_record(&task.task_id).unwrap();
        OpenOptions::new()
            .append(true)
            .open(record.part)
            .unwrap()
            .write_all(b"x")
            .unwrap();
        app.tick().await.unwrap();
        assert_eq!(
            app.task(&task.task_id).unwrap().error.unwrap().code,
            ErrorCode::IntegrityFailed
        );
        assert!(!directory.path().join("真实下载样本.bin").exists());
    }
    #[tokio::test]
    async fn crash_after_publish_recovers_without_redownloading_and_switching_account_does_not_kill_worker(
    ) {
        let (directory, provider, app, task) = setup(64).await;
        app.tick().await.unwrap();
        app.tick().await.unwrap();
        *provider.scope.lock().unwrap() = "account-b".into();
        app.tick().await.unwrap();
        assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Failed);
        assert!(app
            .visible_transfers(None, None, &RequestContext::default())
            .await
            .unwrap()
            .items
            .is_empty());
        *provider.scope.lock().unwrap() = "account-a".into();
        app.transfer_control(&task.task_id, TransferAction::Retry)
            .unwrap();
        app.tick().await.unwrap();
        app.tick().await.unwrap();
        let mut record = app.download_record(&task.task_id).unwrap();
        record.publish_started = true;
        app.save_download(&task.task_id, &record).unwrap();
        let file = open_part(&record.part, false).unwrap();
        publish(&file, &record.part, &record.destination).unwrap();
        drop(file);
        drop(app);
        let app = AppService::with_provider(&directory.path().join("tasks.db"), provider.clone());
        app.transfer_control(&task.task_id, TransferAction::Resume)
            .unwrap();
        finish(&app, &task.task_id).await;
        assert_eq!(std::fs::read(record.destination).unwrap(), provider.bytes);
    }
    #[tokio::test]
    async fn resumed_corruption_and_missing_intent_journal_fail_closed() {
        let (directory, provider, app, task) = setup(BLOCK as usize + 23).await;
        app.tick().await.unwrap();
        app.tick().await.unwrap();
        let record = app.download_record(&task.task_id).unwrap();
        OpenOptions::new()
            .write(true)
            .open(record.part)
            .unwrap()
            .write_all(b"bad")
            .unwrap();
        drop(app);
        let app = AppService::with_provider(&directory.path().join("tasks.db"), provider.clone());
        app.transfer_control(&task.task_id, TransferAction::Resume)
            .unwrap();
        app.tick().await.unwrap();
        app.tick().await.unwrap();
        assert_eq!(
            app.task(&task.task_id).unwrap().error.unwrap().code,
            ErrorCode::IntegrityFailed
        );
        drop(app);
        std::fs::remove_file(directory.path().join("tasks.db.actions.sqlite3")).unwrap();
        let app = AppService::with_provider(&directory.path().join("tasks.db"), provider);
        assert_eq!(app.app_status().storage.status, "recovery_required");
        assert!(!directory.path().join("tasks.db.actions.sqlite3").exists());
    }
    #[tokio::test]
    async fn zero_byte_file_and_final_name_collision_are_handled_without_overwriting() {
        let (directory, _, app, task) = setup(0).await;
        finish(&app, &task.task_id).await;
        assert_eq!(
            std::fs::metadata(directory.path().join("真实下载样本.bin"))
                .unwrap()
                .len(),
            0
        );
        let (directory, _, app, task) = setup(64).await;
        app.tick().await.unwrap();
        app.tick().await.unwrap();
        std::fs::write(directory.path().join("真实下载样本.bin"), b"keep-existing").unwrap();
        app.tick().await.unwrap();
        assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Failed);
        assert_eq!(
            std::fs::read(directory.path().join("真实下载样本.bin")).unwrap(),
            b"keep-existing"
        );
    }
    #[tokio::test]
    async fn verification_releases_control_lock_and_observes_cancellation() {
        let (directory, _, app, task) = setup(BLOCK as usize * 2).await;
        app.tick().await.unwrap();
        app.tick().await.unwrap();
        app.tick().await.unwrap();
        assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Verifying);
        let app = Arc::new(app);
        let other = app.clone();
        let id = task.task_id.clone();
        let verifier = tokio::spawn(async move { other.tick().await });
        tokio::task::yield_now().await;
        let control = app.clone();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tokio::task::spawn_blocking(move || {
                control.transfer_control(&id, TransferAction::Cancel)
            }),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        verifier.await.unwrap().unwrap();
        assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Cancelled);
        assert!(!directory.path().join("真实下载样本.bin").exists());
    }
}
