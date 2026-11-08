//! Authenticated media ranges, on-disk verified cache and durable native exports.
use super::*;
use nekopak_media::native::{self, ClipPlan, RangeFuture, RangeSource, RenderControl};
use nekopak_provider::{DownloadInfo, DownloadObserver};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::atomic::Ordering,
    time::Duration,
};
const BLOCK: u64 = 1024 * 1024;
fn invalid() -> ClientError {
    ClientError::new(
        ErrorCode::IntegrityFailed,
        "媒体来源、缓存或输出校验失败，未发布文件。",
    )
}
fn io(_: std::io::Error) -> ClientError {
    ClientError::new(ErrorCode::PermissionDenied, "无法访问媒体缓存或输出目录。")
}

pub(crate) struct CachedSource {
    provider: Arc<dyn ProviderPort>,
    info: DownloadInfo,
    quality: String,
    root: PathBuf,
    context: RequestContext,
    local: Option<PathBuf>,
    cache_gate: Arc<std::sync::Mutex<()>>,
    validated_at: tokio::sync::Mutex<std::time::Instant>,
}
impl CachedSource {
    pub(crate) fn entry(&self) -> &RemoteEntry {
        &self.info.entry
    }
    pub(crate) fn name(&self) -> String {
        self.info.entry.name.clone()
    }
}
impl RangeSource for CachedSource {
    fn len(&self) -> u64 {
        self.info.entry.size_bytes.0
    }
    fn identity(&self) -> &str {
        &self.info.identity
    }
    fn read_exact(&self, offset: u64, length: usize) -> RangeFuture<'_> {
        Box::pin(async move {
            if length > 32 * 1024 * 1024
                || offset
                    .checked_add(length as u64)
                    .is_none_or(|n| n > self.len())
            {
                return Err(invalid());
            }
            if let Some(path) = &self.local {
                let mut file = super::local::open_readonly(path)?;
                if super::local::read_identity(&file)? != self.info.identity {
                    return Err(invalid());
                }
                file.seek(SeekFrom::Start(offset)).map_err(io)?;
                let mut bytes = vec![0; length];
                file.read_exact(&mut bytes).map_err(io)?;
                return Ok(bytes);
            }
            if self.context.run(self.provider.account_scope()).await? != self.info.account_scope {
                return Err(ClientError::new(
                    ErrorCode::AuthExpired,
                    "账号已改变，媒体读取已停止。",
                ));
            }
            // Coalesce metadata checks while a demuxer reads adjacent samples.
            // Cache misses still validate the exact source on every HTTP range.
            let mut checked = self.validated_at.lock().await;
            if checked.elapsed() >= Duration::from_secs(2) {
                let current = self
                    .context
                    .run(self.provider.media_info(&self.info.entry.id, &self.quality))
                    .await?;
                if current.identity != self.info.identity
                    || current.account_scope != self.info.account_scope
                    || current.entry.size_bytes != self.info.entry.size_bytes
                {
                    return Err(invalid());
                }
                *checked = std::time::Instant::now();
            }
            drop(checked);
            let _parents = downloads::lock_parents(&self.root)?;
            let mut result = Vec::with_capacity(length);
            let mut at = offset;
            while result.len() < length {
                let base = at / BLOCK * BLOCK;
                let size = BLOCK.min(self.len() - base);
                let path = self.root.join(format!("{base}.block"));
                let cached = {
                    let _cache_guard = self.cache_gate.lock().map_err(|_| invalid())?;
                    if path.try_exists().map_err(io)? {
                        let mut f = downloads::open_part(&path, false)?;
                        if f.metadata().map_err(io)?.len() != size + 32 {
                            return Err(invalid());
                        }
                        let mut digest = [0u8; 32];
                        f.read_exact(&mut digest).map_err(io)?;
                        let mut bytes = vec![0; size as usize];
                        f.read_exact(&mut bytes).map_err(io)?;
                        if Sha256::digest(&bytes).as_slice() != digest {
                            return Err(invalid());
                        }
                        Some(bytes)
                    } else {
                        None
                    }
                };
                let bytes = if let Some(bytes) = cached {
                    bytes
                } else {
                    let chunk = self
                        .context
                        .run(self.provider.media_chunk(
                            &self.info.entry.id,
                            &self.quality,
                            &self.info.identity,
                            &self.info.account_scope,
                            base,
                            size,
                            DownloadObserver::default(),
                        ))
                        .await?;
                    if chunk.total != self.len() || chunk.bytes.len() != size as usize {
                        return Err(invalid());
                    }
                    let _cache_guard = self.cache_gate.lock().map_err(|_| invalid())?;
                    enforce_cache_budget(self.root.parent().ok_or_else(invalid)?, size + 32)?;
                    let temp = self.root.join(format!("{}.tmp", uuid::Uuid::new_v4()));
                    let mut file = downloads::open_part(&temp, true)?;
                    file.write_all(&Sha256::digest(&chunk.bytes)).map_err(io)?;
                    file.write_all(&chunk.bytes).map_err(io)?;
                    file.sync_all().map_err(io)?;
                    match downloads::publish(&file, &temp, &path) {
                        Ok(()) => {}
                        Err(error) if path.exists() => {
                            drop(file);
                            std::fs::remove_file(&temp).map_err(io)?;
                            let _ = error;
                        }
                        Err(e) => return Err(e),
                    }
                    chunk.bytes
                };
                let start = (at - base) as usize;
                let take = (length - result.len()).min(bytes.len() - start);
                result.extend_from_slice(&bytes[start..start + take]);
                at += take as u64;
            }
            Ok(result)
        })
    }
}
#[derive(Serialize, Deserialize)]
struct ExportRecord {
    scope: String,
    quality: String,
    plan: ClipPlan,
    destination: PathBuf,
    part: PathBuf,
    file_identity: Option<String>,
    result: Option<native::RenderResult>,
    publish_started: bool,
}
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TransferOptions {
    pub quality_id: Option<String>,
    pub quality_cap: Option<String>,
    pub clip: Option<ClipOptions>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClipOptions {
    pub start_ms: u64,
    pub end_ms: u64,
    pub align: String,
}

impl AppService {
    pub(crate) async fn media_source(
        &self,
        id: &str,
        quality: &str,
        ctx: &RequestContext,
    ) -> Result<CachedSource, ClientError> {
        if id.starts_with("local:") {
            if quality != "original" {
                return Err(ClientError::new(
                    ErrorCode::Unsupported,
                    "本地文件只有原始画质。",
                ));
            }
            let (_, path, _) = self.local_resolve(id)?;
            let file = super::local::open_readonly(&path)?;
            let identity = super::local::read_identity(&file)?;
            let entry = self.local_stat(id)?;
            let info = DownloadInfo {
                entry,
                identity,
                account_scope: "local".into(),
            };
            return Ok(CachedSource {
                provider: self.provider.clone(),
                info,
                quality: quality.into(),
                root: PathBuf::new(),
                context: ctx.clone(),
                local: Some(path),
                cache_gate: self.cache_gate.clone(),
                validated_at: tokio::sync::Mutex::new(std::time::Instant::now()),
            });
        }
        let info = ctx.run(self.provider.media_info(id, quality)).await?;
        let cache = self.data_root.join("media-cache");
        let _parents = downloads::lock_parents(&self.data_root)?;
        let _cache_guard = self.cache_gate.lock().map_err(|_| invalid())?;
        if !cache.exists() {
            std::fs::create_dir(&cache).map_err(io)?;
        }
        let _cache_lock = downloads::lock_parents(&cache)?;
        let root = cache.join(fingerprint(&[
            &info.account_scope,
            id,
            quality,
            &info.identity,
        ]));
        if !root.exists() {
            std::fs::create_dir(&root).map_err(io)?;
        }
        let _root_lock = downloads::lock_parents(&root)?;
        Ok(CachedSource {
            provider: self.provider.clone(),
            info,
            quality: quality.into(),
            root,
            context: ctx.clone(),
            local: None,
            cache_gate: self.cache_gate.clone(),
            validated_at: tokio::sync::Mutex::new(std::time::Instant::now()),
        })
    }
    async fn media_choices(
        &self,
        id: &str,
        ctx: &RequestContext,
    ) -> Result<Vec<nekopak_provider::Representation>, ClientError> {
        if id.starts_with("local:") {
            let entry = self.local_stat(id)?;
            return Ok(vec![nekopak_provider::Representation {
                id: "original".into(),
                label: "原画".into(),
                width: None,
                height: None,
                size_bytes: Some(entry.size_bytes),
                container: "original".into(),
                readable: true,
                unreadable_reason: None,
            }]);
        }
        ctx.run(self.provider.media_options(id)).await
    }
    pub async fn media_probe(&self, id: &str, ctx: &RequestContext) -> Result<Value, ClientError> {
        let options = self.media_choices(id, ctx).await?;
        let source = self.media_source(id, "original", ctx).await?;
        let index = ctx
            .run(native::probe(&source, native::Limits::default()))
            .await?;
        let video = index
            .info
            .tracks
            .iter()
            .find(|t| t.kind == native::TrackKind::Video);
        Ok(
            json!({"remote_id":id,"duration_ms":index.info.duration_ms,"container":index.info.container,"video_codec":video.map(|v|&v.codec),"width":video.and_then(|v|v.width),"height":video.and_then(|v|v.height),"qualities":options,"keyframes":index.info.keyframes,"preview":"unavailable","preview_reason":"当前无内置视频帧解码器；可使用播放预览。","tracks":index.info.tracks,"warnings":index.info.warnings,"probed_at":now()}),
        )
    }
    pub async fn media_align(
        &self,
        id: &str,
        quality: &str,
        start: u64,
        end: u64,
        ctx: &RequestContext,
    ) -> Result<Value, ClientError> {
        if end > 9_007_199_254_740_991 {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "时间超出可精确表示范围。",
            ));
        }
        let source = self.media_source(id, quality, ctx).await?;
        let index = ctx
            .run(native::probe(&source, native::Limits::default()))
            .await?;
        let plan = ctx
            .run(native::prepare_format(
                &source,
                &index,
                nekopak_media::ClipRange {
                    start_ms: start,
                    end_ms: end,
                },
                "mp4",
            ))
            .await?;
        Ok(
            json!({"start_ms":plan.aligned_start_ms,"end_ms":end,"quality_id":quality,"requested_start_ms":start,"estimated_bytes":plan.output_bytes.to_string(),"exactness":"exact","output_format":plan.output_format,"warnings":plan.warnings}),
        )
    }
    pub async fn media_start(
        &self,
        id: &str,
        directory: &str,
        options: &TransferOptions,
        key: Option<&str>,
        ctx: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        let scope = if id.starts_with("local:") {
            "local".to_owned()
        } else {
            ctx.run(self.provider.account_scope()).await?
        };
        self.media_start_at(id, directory, options, key, &scope, ctx)
            .await
    }
    pub(crate) async fn media_start_at(
        &self,
        id: &str,
        directory: &str,
        options: &TransferOptions,
        key: Option<&str>,
        expected_scope: &str,
        ctx: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        let retained = self
            .retain_shared_source(id, expected_scope, key, ctx)
            .await?;
        let id = retained.as_str();
        validate_destination(directory)?;
        let parent = Path::new(directory);
        let _lock = downloads::lock_parents(parent)?;
        let choices = self.media_choices(id, ctx).await?;
        let quality = if let Some(exact) = &options.quality_id {
            exact.clone()
        } else if let Some(cap) = options.quality_cap.as_deref().filter(|c| *c != "original") {
            let height = cap
                .strip_suffix('p')
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| ClientError::new(ErrorCode::Conflict, "无效的画质上限。"))?;
            let candidates: Vec<_> = choices
                .iter()
                .filter(|c| c.id != "original" && c.readable && c.height.is_some())
                .collect();
            candidates
                .iter()
                .filter(|c| c.height.unwrap() <= height)
                .max_by_key(|c| c.height)
                .or_else(|| candidates.iter().min_by_key(|c| c.height))
                .ok_or_else(|| ClientError::new(ErrorCode::Unsupported, "没有可读取的转码画质。"))?
                .id
                .clone()
        } else {
            "original".into()
        };
        if quality == "original" && options.clip.is_none() && !id.starts_with("local:") {
            return self
                .transfer_start_at(id, directory, key, expected_scope, ctx)
                .await;
        }
        let source = self.media_source(id, &quality, ctx).await?;
        if source.info.account_scope != expected_scope {
            return Err(ClientError::new(
                ErrorCode::AuthExpired,
                "账号已改变，未创建媒体任务。",
            ));
        }
        let fp = fingerprint(&[
            id,
            directory,
            &serde_json::to_string(options).map_err(|_| invalid())?,
        ]);
        let task_key = key
            .map(|k| scoped_key(&format!("media:{}", source.info.account_scope), k))
            .transpose()?;
        if let Some(existing) = task_key
            .as_deref()
            .map(|k| self.store()?.lookup_key(k, &fp))
            .transpose()?
            .flatten()
        {
            return Ok(TaskRef {
                task_id: existing.id,
            });
        }
        let index = ctx
            .run(native::probe(&source, native::Limits::default()))
            .await?;
        let range = if let Some(clip) = &options.clip {
            if clip.align != "previous_keyframe" || clip.end_ms > 9_007_199_254_740_991 {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "无效的视频时间范围或对齐方式。",
                ));
            }
            nekopak_media::ClipRange {
                start_ms: clip.start_ms,
                end_ms: clip.end_ms,
            }
        } else {
            nekopak_media::ClipRange {
                start_ms: 0,
                end_ms: index.info.duration_ms,
            }
        };
        let plan = ctx
            .run(native::prepare_format(&source, &index, range, "mp4"))
            .await?;
        let stem = Path::new(&source.info.entry.name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("video");
        let name = downloads::safe_name(&format!(
            "{stem}_{}_{}-{}.{}",
            quality, plan.aligned_start_ms, plan.end_ms, plan.output_format
        ));
        let destination = parent.join(&name);
        if destination.exists() {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "媒体目标文件已存在，未覆盖。",
            ));
        }
        let mut task = new_task(
            id.into(),
            TaskKind::SegmentDownload,
            Some(DecimalU64(plan.output_bytes)),
        );
        task.backend = BackendId::RustCompat;
        task.simulated = false;
        task.name = Some(name);
        task.destination = Some(destination.to_string_lossy().into_owned());
        task.quality_label = choices
            .iter()
            .find(|c| c.id == quality)
            .map(|c| c.label.clone());
        task.clip = options.clip.as_ref().map(|c| MediaTimeRange {
            start_ms: c.start_ms,
            end_ms: c.end_ms,
        });
        let record = ExportRecord {
            scope: source.info.account_scope,
            quality,
            plan,
            destination,
            part: parent.join(format!(".nekopak-{}.media.part", task.id)),
            file_identity: None,
            result: None,
            publish_started: false,
        };
        self.check_scope(expected_scope, ctx).await?;
        self.save_export(&task.id, &record)?;
        let task = self
            .store()?
            .insert(task, task_key.as_deref().map(|k| (k, fp.as_str())))?;
        self.emit(&task);
        Ok(TaskRef { task_id: task.id })
    }
    fn export(&self, id: &str) -> Result<ExportRecord, ClientError> {
        serde_json::from_str(
            &self
                .records()?
                .record_get("tasks", "media", id)?
                .ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())
    }
    fn save_export(&self, id: &str, record: &ExportRecord) -> Result<(), ClientError> {
        self.records()?.record_put(
            "tasks",
            "media",
            id,
            &serde_json::to_string(record).map_err(|_| invalid())?,
        )
    }
    pub(crate) fn media_scope(&self, id: &str) -> Result<String, ClientError> {
        Ok(self.export(id)?.scope)
    }
    pub(crate) async fn media_directory(
        &self,
        id: &str,
        ctx: &RequestContext,
    ) -> Result<PathBuf, ClientError> {
        let record = self.export(id)?;
        self.check_scope(&record.scope, ctx).await?;
        let parent = record.destination.parent().ok_or_else(invalid)?;
        let _lock = downloads::lock_parents(parent)?;
        Ok(parent.to_owned())
    }
    pub async fn run_media(self: &Arc<Self>) -> Result<(), ClientError> {
        if self.media_busy.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let result = self.run_one_media().await;
        self.media_busy.store(false, Ordering::SeqCst);
        result
    }
    async fn run_one_media(self: &Arc<Self>) -> Result<(), ClientError> {
        let ctx = RequestContext::new(Duration::from_secs(3600));
        let mut selected = None;
        for task in self
            .store()?
            .active()?
            .into_iter()
            .filter(|t| t.kind == TaskKind::SegmentDownload && !t.simulated)
        {
            let record = self.export(&task.id)?;
            if self.check_scope(&record.scope, &ctx).await.is_ok() {
                selected = Some((task, record));
                break;
            }
        }
        let Some((mut task, record)) = selected else {
            return Ok(());
        };
        if task.state == TaskState::Queued {
            task =
                self.store()?
                    .update(&task, TaskState::Running, task.transferred_bytes.0, None)?;
            self.emit(&task);
        }
        let service = Arc::downgrade(self);
        let task_id = task.id.clone();
        let control = RenderControl::with_progress(move |done, _| {
            if let Some(service) = service.upgrade() {
                if let Ok(current) = service.task(&task_id) {
                    if current.state == TaskState::Running && done > current.transferred_bytes.0 {
                        if let Ok(updated) = service.store().and_then(|store| {
                            store.update(&current, TaskState::Running, done, None)
                        }) {
                            service.emit(&updated);
                        }
                    }
                }
            }
        });
        self.media_controls
            .lock()
            .map_err(|_| invalid())?
            .insert(task.id.clone(), control.clone());
        let result = self.render_export(&task, record, &ctx, &control).await;
        self.media_controls
            .lock()
            .map_err(|_| invalid())?
            .remove(&task.id);
        if let Err(error) = result {
            let current = self.store()?.get(&task.id)?;
            if matches!(current.state, TaskState::Running | TaskState::Verifying) {
                let next = self.store()?.update(
                    &current,
                    TaskState::Failed,
                    current.transferred_bytes.0,
                    Some(error),
                )?;
                self.emit(&next);
            }
        }
        Ok(())
    }
    async fn render_export(
        &self,
        task: &TransferTask,
        mut record: ExportRecord,
        ctx: &RequestContext,
        control: &RenderControl,
    ) -> Result<(), ClientError> {
        let parent = record.destination.parent().ok_or_else(invalid)?;
        let _parents = downloads::lock_parents(parent)?;
        let recovery = record.publish_started && record.destination.exists();
        let mut file = downloads::open_part(
            if recovery {
                &record.destination
            } else {
                &record.part
            },
            record.file_identity.is_none(),
        )?;
        let identity = downloads::file_identity(&file)?;
        if record
            .file_identity
            .as_ref()
            .is_some_and(|v| *v != identity)
        {
            return Err(invalid());
        }
        if record.file_identity.is_none() {
            record.file_identity = Some(identity);
            self.save_export(&task.id, &record)?;
        }
        let result = if recovery {
            record.result.clone().ok_or_else(invalid)?
        } else {
            let source = self
                .media_source(&task.remote_id, &record.quality, ctx)
                .await?;
            native::render(&source, &record.plan, &mut file, control).await?
        };
        if control.is_cancelled() || self.store()?.get(&task.id)?.state != TaskState::Running {
            return Err(ClientError::new(
                ErrorCode::Cancelled,
                "媒体导出已暂停或取消。",
            ));
        }
        file.sync_all().map_err(io)?;
        file.seek(SeekFrom::Start(0)).map_err(io)?;
        let mut hash = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = vec![0; 1024 * 1024];
        loop {
            if control.is_cancelled() {
                return Err(ClientError::new(ErrorCode::Cancelled, "媒体校验已取消。"));
            }
            let n = file.read(&mut buffer).map_err(io)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
            bytes += n as u64;
            tokio::task::yield_now().await;
        }
        if bytes != result.output_bytes || format!("{:x}", hash.finalize()) != result.output_sha256
        {
            return Err(invalid());
        }
        self.check_scope(&record.scope, ctx).await?;
        let _gate = self.download_gate.lock().map_err(|_| invalid())?;
        let current = self.store()?.get(&task.id)?;
        if current.state != TaskState::Running || control.is_cancelled() {
            return Err(ClientError::new(
                ErrorCode::Cancelled,
                "媒体任务状态已改变。",
            ));
        }
        let verifying = self
            .store()?
            .update(&current, TaskState::Verifying, bytes, None)?;
        self.emit(&verifying);
        if !recovery {
            record.result = Some(result);
            record.publish_started = true;
            self.save_export(&task.id, &record)?;
            downloads::publish(&file, &record.part, &record.destination)?;
        }
        let completed = self
            .store()?
            .update(&verifying, TaskState::Completed, bytes, None)?;
        self.emit(&completed);
        Ok(())
    }
}
/// Evict only validated, application-owned blocks; user media/outputs are never
/// inside this namespace. Readers and writers hold cache_gate during disk IO.
fn enforce_cache_budget(root: &Path, additional: u64) -> Result<(), ClientError> {
    let mut used = 0u64;
    let mut count = 0usize;
    let root = root.canonicalize().map_err(io)?;
    let mut blocks = Vec::new();
    for directory in std::fs::read_dir(&root).map_err(io)? {
        let directory = directory.map_err(io)?;
        let metadata = directory.metadata().map_err(io)?;
        if !metadata.is_dir() || directory.file_type().map_err(io)?.is_symlink() {
            return Err(invalid());
        }
        let _lock = downloads::lock_parents(&directory.path())?;
        for file in std::fs::read_dir(directory.path()).map_err(io)? {
            count += 1;
            if count > 100_000 {
                return Err(invalid());
            }
            let file = file.map_err(io)?;
            if file.file_type().map_err(io)?.is_symlink() {
                return Err(invalid());
            }
            let metadata = file.metadata().map_err(io)?;
            if !metadata.is_file() {
                return Err(invalid());
            }
            let path = file.path().canonicalize().map_err(io)?;
            if !path.starts_with(&root) {
                return Err(invalid());
            }
            used = used.checked_add(metadata.len()).ok_or_else(invalid)?;
            let name = file.file_name().to_string_lossy().into_owned();
            if name
                .strip_suffix(".block")
                .is_some_and(|s| s.parse::<u64>().is_ok())
            {
                blocks.push((metadata.modified().map_err(io)?, path, metadata.len()));
            }
        }
    }
    blocks.sort_by_key(|v| v.0);
    for (_, path, size) in blocks {
        if used.saturating_add(additional) <= 512 * 1024 * 1024 {
            break;
        }
        let file = downloads::open_part(&path, false)?;
        drop(file);
        std::fs::remove_file(&path).map_err(io)?;
        used = used.saturating_sub(size);
    }
    if used.saturating_add(additional) > 512 * 1024 * 1024 {
        return Err(ClientError::new(
            ErrorCode::DiskFull,
            "媒体缓存已达到512 MiB上限且暂不能回收，请关闭播放后重试。",
        ));
    }
    Ok(())
}
