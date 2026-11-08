//! Account-scoped, bounded directory traversal shared by catalog tools and downloads.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, VecDeque},
    path::PathBuf,
    time::Duration,
};

const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 128;
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct WalkEntry {
    pub entry: RemoteEntry,
    pub relative: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct BatchPlan {
    fingerprint: String,
    entries: Vec<WalkEntry>,
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorCode::IntegrityFailed,
        "目录树不完整、循环或超过安全上限，未按空目录继续执行。",
    )
}
fn encode<T: Serialize>(value: &T) -> Result<String, ClientError> {
    serde_json::to_string(value).map_err(|_| invalid())
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, ClientError> {
    serde_json::from_str(value).map_err(|_| invalid())
}

impl AppService {
    pub async fn account_scope(&self, ctx: &RequestContext) -> Result<String, ClientError> {
        ctx.run(self.provider.account_scope()).await
    }
    pub(crate) fn records(&self) -> Result<&intents::IntentStore, ClientError> {
        self.store()?;
        self.intents.as_ref().ok_or_else(invalid)
    }
    pub(crate) async fn check_scope(
        &self,
        scope: &str,
        ctx: &RequestContext,
    ) -> Result<(), ClientError> {
        if scope == "local" {
            return Ok(());
        }
        if ctx.run(self.provider.account_scope()).await? != scope {
            return Err(ClientError::new(
                ErrorCode::AuthExpired,
                "账号已改变，操作已停止。",
            ));
        }
        Ok(())
    }
    pub(crate) async fn walk(
        &self,
        folder: &str,
        scope: &str,
        ctx: &RequestContext,
        scan: Option<&str>,
    ) -> Result<Vec<WalkEntry>, ClientError> {
        let mut pending = VecDeque::from([(folder.to_owned(), Vec::<String>::new())]);
        let mut seen = BTreeSet::from([folder.to_owned()]);
        let mut out = Vec::new();
        let mut files = 0u32;
        let mut folders = 1u32;
        while let Some((parent, path)) = pending.pop_front() {
            if path.len() > MAX_DEPTH {
                return Err(invalid());
            }
            let mut cursor = None;
            let mut cursors = BTreeSet::new();
            loop {
                self.check_scope(scope, ctx).await?;
                let page = ctx
                    .run(self.provider.list(&parent, cursor.as_deref(), 500))
                    .await?;
                for entry in page.items {
                    if out.len() >= MAX_ENTRIES
                        || entry.parent_id.as_deref() != Some(&parent)
                        || !seen.insert(entry.id.clone())
                    {
                        return Err(invalid());
                    }
                    let mut relative = path.clone();
                    relative.push(entry.name.clone());
                    if entry.kind == EntryKind::Folder {
                        folders += 1;
                        pending.push_back((entry.id.clone(), relative.clone()));
                    } else {
                        files += 1;
                    }
                    out.push(WalkEntry { entry, relative });
                }
                if let Some(id) = scan {
                    let _=self.scan_events.send(serde_json::json!({"scan_id":id,"state":"running","files_scanned":files,"folders_scanned":folders}));
                }
                let Some(next) = page.next_cursor else {
                    break;
                };
                if !cursors.insert(next.clone()) || cursors.len() > 1000 {
                    return Err(invalid());
                }
                cursor = Some(next);
            }
        }
        self.check_scope(scope, ctx).await?;
        Ok(out)
    }
    pub fn subscribe_scans(&self) -> broadcast::Receiver<serde_json::Value> {
        self.scan_events.subscribe()
    }
    pub async fn duplicates_scan(
        self: &Arc<Self>,
        folder: &str,
        ctx: &RequestContext,
    ) -> Result<ScanRef, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        let report = DuplicateReport {
            scan_id: uuid::Uuid::new_v4().to_string(),
            folder_id: folder.into(),
            folder_path: "/".into(),
            state: "running".into(),
            groups: vec![],
            files_scanned: 0,
            folders_scanned: 0,
            reclaimable_bytes: DecimalU64(0),
            unhashed_files: 0,
            skipped: vec![],
            completed_at: None,
            error: None,
        };
        self.records()?
            .record_put(&scope, "duplicates", &report.scan_id, &encode(&report)?)?;
        let context = RequestContext::new(Duration::from_secs(30 * 60));
        self.scans
            .lock()
            .map_err(|_| invalid())?
            .insert(report.scan_id.clone(), context.clone());
        let scan_id = report.scan_id.clone();
        let service = self.clone();
        tokio::spawn(async move {
            let mut report = report;
            let result = service
                .walk(&report.folder_id, &scope, &context, Some(&report.scan_id))
                .await;
            match result {
                Ok(entries) => match duplicate_groups(&entries) {
                    Ok((groups, bytes, unhashed)) => {
                        report.groups = groups;
                        report.reclaimable_bytes = DecimalU64(bytes);
                        report.unhashed_files = unhashed;
                        report.files_scanned = entries
                            .iter()
                            .filter(|e| e.entry.kind == EntryKind::File)
                            .count() as u32;
                        report.folders_scanned = entries.len() as u32 - report.files_scanned + 1;
                        report.state = "completed".into();
                        report.completed_at = Some(now());
                    }
                    Err(error) => {
                        report.state = "failed".into();
                        report.error = Some(error);
                    }
                },
                Err(error) => {
                    report.state = if error.code == ErrorCode::Cancelled {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into();
                    report.error = Some(error);
                }
            }
            if let Ok(journal) = service.records() {
                if let Ok(payload) = encode(&report) {
                    if journal
                        .record_put(&scope, "duplicates", &report.scan_id, &payload)
                        .is_ok()
                    {
                        // Events contain only the random scan id/counts, never another account's metadata.
                        let _=service.scan_events.send(serde_json::json!({"scan_id":report.scan_id,"state":report.state,"files_scanned":report.files_scanned,"folders_scanned":report.folders_scanned}));
                    }
                }
            }
            if let Ok(mut scans) = service.scans.lock() {
                scans.remove(&report.scan_id);
            }
        });
        Ok(ScanRef { scan_id })
    }
    pub async fn duplicates_report(
        &self,
        id: &str,
        ctx: &RequestContext,
    ) -> Result<DuplicateReport, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        let payload = self
            .records()?
            .record_get(&scope, "duplicates", id)?
            .ok_or_else(|| {
                ClientError::new(ErrorCode::NotFound, "扫描记录不存在或属于其他账号。")
            })?;
        let mut report: DuplicateReport = decode(&payload)?;
        if report.state == "running" && !self.scans.lock().map_err(|_| invalid())?.contains_key(id)
        {
            report.state = "cancelled".into();
            report.error = Some(ClientError::new(
                ErrorCode::Cancelled,
                "上次进程已结束，请重新发起扫描。",
            ));
            self.records()?
                .record_put(&scope, "duplicates", id, &encode(&report)?)?;
        }
        self.check_scope(&scope, ctx).await?;
        Ok(report)
    }
    pub async fn duplicates_cancel(
        &self,
        id: &str,
        ctx: &RequestContext,
    ) -> Result<(), ClientError> {
        self.duplicates_report(id, ctx).await?;
        if let Some(scan) = self.scans.lock().map_err(|_| invalid())?.get(id) {
            scan.cancel();
        }
        Ok(())
    }
    pub async fn files_space_summary(
        &self,
        folder: &str,
        ctx: &RequestContext,
    ) -> Result<SpaceSummary, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        let entries = self.walk(folder, &scope, ctx, None).await?;
        let mut children: BTreeMap<String, SpaceNode> = BTreeMap::new();
        let mut total = 0u64;
        let mut files = 0u32;
        let mut folders = 0u32;
        for e in &entries {
            if e.relative.len() == 1 {
                if children.contains_key(&e.relative[0]) {
                    return Err(invalid());
                }
                children.insert(
                    e.relative[0].clone(),
                    SpaceNode {
                        id: e.entry.id.clone(),
                        name: e.entry.name.clone(),
                        kind: e.entry.kind,
                        size_bytes: DecimalU64(0),
                        file_count: 0,
                        mime_type: e.entry.mime_type.clone(),
                    },
                );
            }
            if e.entry.kind == EntryKind::Folder {
                folders += 1;
                continue;
            }
            files += 1;
            total = total
                .checked_add(e.entry.size_bytes.0)
                .ok_or_else(invalid)?;
            let child = children.get_mut(&e.relative[0]).ok_or_else(invalid)?;
            child.size_bytes.0 = child
                .size_bytes
                .0
                .checked_add(e.entry.size_bytes.0)
                .ok_or_else(invalid)?;
            child.file_count += 1;
        }
        Ok(SpaceSummary {
            folder_id: folder.into(),
            total_bytes: DecimalU64(total),
            file_count: files,
            folder_count: folders,
            children: children.into_values().collect(),
            exactness: "exact".into(),
            note: None,
            computed_at: now(),
        })
    }
    pub async fn transfer_start_batch(
        &self,
        ids: &[String],
        destination: &str,
        key: &str,
        ctx: &RequestContext,
    ) -> Result<BatchRef, ClientError> {
        self.transfer_start_batch_options(ids, destination, key, None, ctx)
            .await
    }
    pub async fn transfer_start_batch_options(
        &self,
        ids: &[String],
        destination: &str,
        key: &str,
        options: Option<&TransferOptions>,
        ctx: &RequestContext,
    ) -> Result<BatchRef, ClientError> {
        if ids.is_empty() || ids.len() > 500 {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "一次请选择1至500个项目。",
            ));
        }
        validate_destination(destination)?;
        let base = Path::new(destination);
        let _parents = downloads::lock_parents(base)?;
        if !base.is_dir() {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "请选择已存在的下载目录。",
            ));
        }
        let scope = ctx.run(self.provider.account_scope()).await?;
        let key = scoped_key("batch", key)?;
        let fp = fingerprint(&[destination, &encode(&ids)?, &encode(&options)?]);
        let plan = if let Some(payload) = self.records()?.record_get(&scope, "batch", &key)? {
            let plan: BatchPlan = decode(&payload)?;
            if plan.fingerprint != fp {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "相同批次标识不能用于不同参数。",
                ));
            }
            plan
        } else {
            let mut entries = Vec::new();
            let mut selected = BTreeSet::new();
            for id in ids {
                self.check_scope(&scope, ctx).await?;
                let root = ctx.run(self.provider.stat(id)).await?;
                self.check_scope(&scope, ctx).await?;
                if root.id != *id {
                    return Err(invalid());
                }
                if selected.insert(root.id.clone()) {
                    entries.push(WalkEntry {
                        relative: vec![root.name.clone()],
                        entry: root.clone(),
                    });
                }
                if root.kind == EntryKind::Folder {
                    for mut e in self.walk(id, &scope, ctx, None).await? {
                        if selected.insert(e.entry.id.clone()) {
                            e.relative.insert(0, root.name.clone());
                            entries.push(e);
                        }
                        if entries.len() > MAX_ENTRIES {
                            return Err(invalid());
                        }
                    }
                }
            }
            let plan = BatchPlan {
                fingerprint: fp,
                entries,
            };
            self.check_scope(&scope, ctx).await?;
            self.records()?
                .record_put(&scope, "batch", &key, &encode(&plan)?)?;
            plan
        };
        let mut result = BatchRef {
            task_ids: vec![],
            skipped: vec![],
            expanded_files: 0,
            group_id: Some(fingerprint(&[&scope, &key])),
        };
        let mut targets = BTreeSet::new();
        for item in plan.entries {
            self.check_scope(&scope, ctx).await?;
            let relative: PathBuf = item
                .relative
                .iter()
                .map(|p| downloads::safe_name(p))
                .collect();
            let path = base.join(relative);
            if !targets.insert(path.to_string_lossy().to_lowercase()) {
                result.skipped.push(SkippedEntry {
                    id: item.entry.id,
                    name: Some(item.entry.name),
                    reason: "转换为本地名称后与其他项目冲突。".into(),
                });
                continue;
            }
            if item.entry.kind == EntryKind::Folder {
                ensure_directory(base, &path)?;
                continue;
            }
            let parent = path.parent().ok_or_else(invalid)?;
            ensure_directory(base, parent)?;
            let child_key = fingerprint(&[&key, &item.entry.id]);
            let is_video = item
                .entry
                .mime_type
                .as_deref()
                .is_some_and(|m| m.starts_with("video/"))
                || ["mp4", "mkv", "ts", "webm", "mov"].iter().any(|ext| {
                    item.entry
                        .name
                        .to_ascii_lowercase()
                        .ends_with(&format!(".{ext}"))
                });
            let started = if let Some(options) = options.filter(|_| is_video) {
                self.check_scope(&scope, ctx).await?;
                self.media_start_at(
                    &item.entry.id,
                    &parent.to_string_lossy(),
                    options,
                    Some(&child_key),
                    &scope,
                    ctx,
                )
                .await
            } else {
                self.transfer_start_at(
                    &item.entry.id,
                    &parent.to_string_lossy(),
                    Some(&child_key),
                    &scope,
                    ctx,
                )
                .await
            };
            match started {
                Ok(task) => {
                    result.task_ids.push(task.task_id);
                    if item.relative.len() > 1 {
                        result.expanded_files += 1;
                    }
                }
                Err(error) => {
                    result.skipped.push(SkippedEntry {
                        id: item.entry.id,
                        name: Some(item.entry.name),
                        reason: error.message,
                    });
                }
            }
        }
        Ok(result)
    }
}
fn ensure_directory(base: &Path, path: &Path) -> Result<(), ClientError> {
    let relative = path.strip_prefix(base).map_err(|_| invalid())?;
    let mut current = base.to_owned();
    for part in relative.components() {
        let _lock = downloads::lock_parents(&current)?;
        current.push(part);
        if !current.try_exists().map_err(|_| invalid())? {
            std::fs::create_dir(&current).map_err(|_| {
                ClientError::new(ErrorCode::PermissionDenied, "无法创建下载子目录。")
            })?;
        }
        let _lock = downloads::lock_parents(&current)?;
        if !current.is_dir() {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "下载子目录被同名文件占用。",
            ));
        }
    }
    Ok(())
}
fn duplicate_groups(entries: &[WalkEntry]) -> Result<(Vec<DuplicateGroup>, u64, u32), ClientError> {
    let mut groups: BTreeMap<String, Vec<DuplicateEntry>> = BTreeMap::new();
    let mut unhashed = 0;
    for e in entries.iter().filter(|e| e.entry.kind == EntryKind::File) {
        let Some(hash) = &e.entry.hash else {
            unhashed += 1;
            continue;
        };
        let key = format!(
            "{:?}:{}:{}",
            hash.algorithm,
            hash.value.to_ascii_uppercase(),
            e.entry.size_bytes.0
        );
        groups.entry(key).or_default().push(DuplicateEntry {
            entry: e.entry.clone(),
            path: format!("/{}", e.relative.join("/")),
        });
    }
    let mut out = Vec::new();
    let mut bytes = 0u64;
    for (key, entries) in groups.into_iter().filter(|(_, v)| v.len() > 1) {
        let size = entries[0].entry.size_bytes;
        bytes = bytes
            .checked_add(
                size.0
                    .checked_mul((entries.len() - 1) as u64)
                    .ok_or_else(invalid)?,
            )
            .ok_or_else(invalid)?;
        out.push(DuplicateGroup {
            group_id: fingerprint(&[&key]),
            match_kind: "hash".into(),
            hash: entries[0].entry.hash.clone().ok_or_else(invalid)?,
            size_bytes: size,
            entries,
        });
    }
    Ok((out, bytes, unhashed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use nekopak_provider::{DownloadChunk, DownloadInfo, DownloadObserver};
    use std::sync::Mutex;
    struct Tree {
        entries: Vec<RemoteEntry>,
        scope: Mutex<String>,
    }
    fn entry(id: &str, parent: &str, name: &str, kind: EntryKind, hash: bool) -> RemoteEntry {
        RemoteEntry {
            id: id.into(),
            parent_id: Some(parent.into()),
            name: name.into(),
            kind,
            size_bytes: DecimalU64(if kind == EntryKind::File { 4 } else { 0 }),
            modified_at: Some(now()),
            etag: None,
            hash: hash.then(|| ContentHash {
                algorithm: HashAlgorithm::Gcid,
                value: "A".repeat(40),
            }),
            mime_type: None,
            starred: None,
        }
    }
    fn tree() -> Arc<Tree> {
        Arc::new(Tree {
            entries: vec![
                entry("dir", "root", "目录 sample", EntryKind::Folder, false),
                entry("a", "dir", "copy.bin", EntryKind::File, true),
                entry("b", "dir", "another.bin", EntryKind::File, true),
                entry("c", "dir", "same name.bin", EntryKind::File, false),
            ],
            scope: Mutex::new("one".into()),
        })
    }
    #[async_trait]
    impl ProviderPort for Tree {
        fn id(&self) -> BackendId {
            BackendId::RustCompat
        }
        async fn account_scope(&self) -> Result<String, ClientError> {
            Ok(self.scope.lock().unwrap().clone())
        }
        async fn discover(&self) -> Result<CapabilityReport, ClientError> {
            unreachable!()
        }
        async fn quota(&self) -> Result<QuotaReport, ClientError> {
            unreachable!()
        }
        async fn stat(&self, id: &str) -> Result<RemoteEntry, ClientError> {
            self.entries
                .iter()
                .find(|e| e.id == id)
                .cloned()
                .ok_or_else(invalid)
        }
        async fn list(
            &self,
            parent: &str,
            cursor: Option<&str>,
            _: usize,
        ) -> Result<Page<RemoteEntry>, ClientError> {
            let items: Vec<_> = self
                .entries
                .iter()
                .filter(|e| e.parent_id.as_deref() == Some(parent))
                .cloned()
                .collect();
            let start = cursor.map(|c| c.parse::<usize>().unwrap()).unwrap_or(0);
            Ok(Page {
                items: items.iter().skip(start).take(1).cloned().collect(),
                next_cursor: (start + 1 < items.len()).then(|| (start + 1).to_string()),
            })
        }
        async fn download_info(&self, id: &str) -> Result<DownloadInfo, ClientError> {
            Ok(DownloadInfo {
                entry: self.stat(id).await?,
                identity: "same".into(),
                account_scope: self.account_scope().await?,
            })
        }
        async fn download_chunk(
            &self,
            _: &str,
            _: &str,
            _: &str,
            start: u64,
            length: u64,
            observer: DownloadObserver,
        ) -> Result<DownloadChunk, ClientError> {
            let bytes = b"test"[start as usize..(start + length).min(4) as usize].to_vec();
            observer.received(bytes.len() as u64);
            Ok(DownloadChunk { bytes, total: 4 })
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
    }
    #[tokio::test]
    async fn paged_scan_groups_only_same_content_and_isolates_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let provider = tree();
        let app = Arc::new(AppService::with_provider(
            &dir.path().join("tasks.db"),
            provider.clone(),
        ));
        let ctx = RequestContext::default();
        let scan = app.duplicates_scan("root", &ctx).await.unwrap();
        let mut report = None;
        for _ in 0..100 {
            let r = app.duplicates_report(&scan.scan_id, &ctx).await.unwrap();
            if r.state != "running" {
                report = Some(r);
                break;
            }
            tokio::task::yield_now().await;
        }
        let report = report.unwrap();
        assert_eq!(report.state, "completed");
        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.reclaimable_bytes.0, 4);
        assert_eq!(report.unhashed_files, 1);
        let space = app.files_space_summary("root", &ctx).await.unwrap();
        assert_eq!(space.total_bytes.0, 12);
        assert_eq!(space.children[0].file_count, 3);
        *provider.scope.lock().unwrap() = "two".into();
        assert_eq!(
            app.duplicates_report(&scan.scan_id, &ctx)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
    #[tokio::test]
    async fn recursive_batch_persists_plan_and_does_not_duplicate_tasks_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("tasks.db");
        let provider = tree();
        let app = AppService::with_provider(&db, provider.clone());
        let ctx = RequestContext::default();
        let request = vec!["dir".into()];
        let result = app
            .transfer_start_batch(&request, dir.path().to_str().unwrap(), "batch-one", &ctx)
            .await
            .unwrap();
        assert_eq!(result.task_ids.len(), 3);
        assert_eq!(result.expanded_files, 3);
        assert!(result.skipped.is_empty());
        for _ in 0..20 {
            app.tick().await.unwrap();
        }
        assert_eq!(
            std::fs::read(dir.path().join("目录 sample/copy.bin")).unwrap(),
            b"test"
        );
        drop(app);
        let app = AppService::with_provider(&db, provider);
        let replay = app
            .transfer_start_batch(&request, dir.path().to_str().unwrap(), "batch-one", &ctx)
            .await
            .unwrap();
        assert_eq!(replay.task_ids, result.task_ids);
        assert_eq!(app.transfers_list(None, None).unwrap().items.len(), 3);
        assert!(app
            .transfer_start_batch(
                &["a".into()],
                dir.path().to_str().unwrap(),
                "batch-one",
                &ctx
            )
            .await
            .is_err());
    }
    #[tokio::test]
    async fn cycle_is_rejected_without_claiming_complete_scan() {
        let dir = tempfile::tempdir().unwrap();
        let provider = Arc::new(Tree {
            entries: vec![
                entry("dir", "root", "loop", EntryKind::Folder, false),
                entry("root", "dir", "again", EntryKind::Folder, false),
            ],
            scope: Mutex::new("one".into()),
        });
        let app = AppService::with_provider(&dir.path().join("tasks.db"), provider);
        assert_eq!(
            app.files_space_summary("root", &RequestContext::default())
                .await
                .unwrap_err()
                .code,
            ErrorCode::IntegrityFailed
        );
        assert!(app.transfers_list(None, None).unwrap().items.is_empty());
    }
}
