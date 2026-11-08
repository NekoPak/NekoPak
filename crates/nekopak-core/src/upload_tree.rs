//! Complete local upload plans, durable folder creation and account-scoped children.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, VecDeque},
    fs::{File, OpenOptions},
    path::{Component, PathBuf},
};

const MAX_ENTRIES: usize = 8_000;
const MAX_DEPTH: usize = 64;
const MAX_PLAN_BYTES: usize = 8 * 1024 * 1024;

fn damaged() -> ClientError {
    ClientError::new(
        ErrorCode::IntegrityFailed,
        "目录上传计划不完整或本地内容已变化，未继续上传。",
    )
}
fn conflict(message: &'static str) -> ClientError {
    ClientError::new(ErrorCode::Conflict, message)
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LocalItem {
    relative: String,
    kind: EntryKind,
    identity: String,
    size: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadTree {
    schema_version: u32,
    fingerprint: String,
    root: PathBuf,
    root_name: String,
    parent: String,
    items: Vec<LocalItem>,
    folders: BTreeMap<String, String>,
    tasks: BTreeMap<String, String>,
}

fn path_for(root: &Path, relative: &str) -> Result<PathBuf, ClientError> {
    if relative.is_empty() {
        return Ok(root.into());
    }
    if relative.len() > 16_384 || relative.split('/').count() > MAX_DEPTH {
        return Err(damaged());
    }
    let mut path = root.to_owned();
    for part in relative.split('/') {
        valid_name(part)?;
        path.push(part);
    }
    Ok(path)
}
fn valid_name(name: &str) -> Result<(), ClientError> {
    let path = Path::new(name);
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if name.is_empty()
        || name.len() > 1024
        || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        || reserved
        || name.chars().any(char::is_control)
        || name.ends_with(['.', ' '])
        || path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err(conflict("上传目录含不可安全映射的文件名。"));
    }
    Ok(())
}

fn directory_identity(path: &Path) -> Result<String, ClientError> {
    let _parents = downloads::lock_parents(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(3).custom_flags(0x02200000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0o400000);
    }
    let handle = options.open(path).map_err(|_| damaged())?;
    downloads::file_identity(&handle)
}
fn read_item(root: &Path, relative: &str) -> Result<LocalItem, ClientError> {
    let path = path_for(root, relative)?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| damaged())?;
    if metadata.file_type().is_symlink() {
        return Err(conflict("目录上传不跟随符号链接或重解析点。"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(conflict("目录上传不跟随符号链接或重解析点。"));
        }
    }
    let (kind, identity, size) = if metadata.is_dir() {
        (EntryKind::Folder, directory_identity(&path)?, 0)
    } else if metadata.is_file() {
        let file = local::open_readonly(&path)?;
        (
            EntryKind::File,
            local::read_identity(&file)?,
            metadata.len(),
        )
    } else {
        return Err(conflict("目录上传仅支持普通文件和目录。"));
    };
    Ok(LocalItem {
        relative: relative.into(),
        kind,
        identity,
        size,
    })
}
async fn scan(root: &Path, ctx: &RequestContext) -> Result<Vec<LocalItem>, ClientError> {
    let root_item = read_item(root, "")?;
    if root_item.kind != EntryKind::Folder {
        return Err(conflict("请选择一个本地目录。"));
    }
    let mut items = vec![root_item];
    let mut pending = VecDeque::from([String::new()]);
    let mut folded = BTreeSet::from([String::new()]);
    let mut budget = 0usize;
    while let Some(relative) = pending.pop_front() {
        ctx.run(async { Ok(()) }).await?;
        let path = path_for(root, &relative)?;
        let _locks = downloads::lock_parents(&path)?;
        let mut children = Vec::new();
        for entry in std::fs::read_dir(path).map_err(|_| damaged())? {
            let entry = entry.map_err(|_| damaged())?;
            let name = entry.file_name().into_string().map_err(|_| damaged())?;
            valid_name(&name)?;
            let child = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            if items.len() + children.len() >= MAX_ENTRIES || !folded.insert(child.to_lowercase()) {
                return Err(conflict("目录过大或存在大小写名称冲突，未上传不完整目录。"));
            }
            budget = budget.checked_add(child.len() + 512).ok_or_else(damaged)?;
            if budget > MAX_PLAN_BYTES {
                return Err(conflict("目录上传计划过大，请分批选择子目录。"));
            }
            children.push(read_item(root, &child)?);
        }
        children.sort_by(|a, b| a.relative.cmp(&b.relative));
        for child in children {
            if child.kind == EntryKind::Folder {
                pending.push_back(child.relative.clone());
            }
            items.push(child);
        }
        tokio::task::yield_now().await;
    }
    Ok(items)
}

impl UploadTree {
    fn validate(&self, root: &Path, parent: &str, fp: &str) -> Result<(), ClientError> {
        if self.schema_version != 1
            || self.root != root
            || self.parent != parent
            || self.fingerprint != fp
            || self.items.is_empty()
            || self.items.len() > MAX_ENTRIES
            || !self.items[0].relative.is_empty()
            || self.items[0].kind != EntryKind::Folder
        {
            return Err(damaged());
        }
        valid_name(&self.root_name)?;
        let mut folded = BTreeSet::new();
        let mut known = BTreeMap::new();
        for item in &self.items {
            path_for(root, &item.relative)?;
            if !folded.insert(item.relative.to_lowercase()) || item.identity.is_empty() {
                return Err(damaged());
            }
            if !item.relative.is_empty() {
                let parent = item.relative.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
                if known.get(parent) != Some(&EntryKind::Folder) {
                    return Err(damaged());
                }
            }
            known.insert(item.relative.as_str(), item.kind);
        }
        if self.folders.iter().any(|(relative, id)| {
            known.get(relative.as_str()) != Some(&EntryKind::Folder) || id.is_empty()
        }) || self.tasks.iter().any(|(relative, id)| {
            known.get(relative.as_str()) != Some(&EntryKind::File) || id.is_empty()
        }) {
            return Err(damaged());
        }
        Ok(())
    }
    fn result(&self, group: &str) -> BatchRef {
        BatchRef {
            task_ids: self.tasks.values().cloned().collect(),
            skipped: vec![],
            expanded_files: self.tasks.len() as u32,
            group_id: Some(group.into()),
        }
    }
}

impl AppService {
    /// Upload a complete directory plan. Reusing the same user-intent key resumes
    /// known folders and child tasks; it never reissues an unknown folder write.
    pub async fn upload_directory_start(
        &self,
        path: &str,
        parent: &str,
        key: &str,
        ctx: &RequestContext,
    ) -> Result<BatchRef, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        self.upload_directory_start_at(path, parent, key, &scope, ctx)
            .await
    }
    pub async fn upload_directory_start_at(
        &self,
        path: &str,
        parent: &str,
        key: &str,
        expected_scope: &str,
        ctx: &RequestContext,
    ) -> Result<BatchRef, ClientError> {
        validate_destination(path)?;
        if parent.is_empty() || parent.len() > 256 || parent.chars().any(char::is_control) {
            return Err(conflict("目标目录标识无效。"));
        }
        let _gate = ctx
            .run(async { Ok(self.upload_tree_gate.lock().await) })
            .await?;
        self.check_scope(expected_scope, ctx).await?;
        let scope = expected_scope.to_owned();
        let root = PathBuf::from(path);
        let _root_locks = downloads::lock_parents(&root)?;
        let name = root
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| conflict("请选择有名称的目录，不能直接上传磁盘根目录。"))?
            .to_owned();
        valid_name(&name)?;
        let fp = fingerprint(&[path, parent]);
        let record_key = scoped_key("upload-tree", key)?;
        let group = fingerprint(&[&scope, &record_key]);
        let mut plan = if let Some(raw) =
            self.records()?
                .record_get(&scope, "upload-tree", &record_key)?
        {
            if raw.len() > MAX_PLAN_BYTES {
                return Err(damaged());
            }
            let value: UploadTree = serde_json::from_str(&raw).map_err(|_| damaged())?;
            value.validate(&root, parent, &fp)?;
            value
        } else {
            let items = scan(&root, ctx).await?;
            let plan = UploadTree {
                schema_version: 1,
                fingerprint: fp,
                root: root.clone(),
                root_name: name,
                parent: parent.into(),
                items,
                folders: BTreeMap::new(),
                tasks: BTreeMap::new(),
            };
            // A second complete read catches additions/removals/identity changes
            // during enumeration before any network mutation is allowed.
            if scan(&root, ctx).await? != plan.items {
                return Err(damaged());
            }
            self.validate_upload_root_destination(parent, &plan.root_name, &scope, ctx)
                .await?;
            self.check_scope(&scope, ctx).await?;
            self.save_upload_tree(&scope, &record_key, &plan)?;
            plan
        };
        if plan.tasks.len()
            == plan
                .items
                .iter()
                .filter(|i| i.kind == EntryKind::File)
                .count()
            && plan.folders.len()
                == plan
                    .items
                    .iter()
                    .filter(|i| i.kind == EntryKind::Folder)
                    .count()
        {
            return Ok(plan.result(&group));
        }
        // A resumed intent refers to its original complete snapshot. Changing a
        // local tree requires a new user action, not a silently different retry.
        if scan(&root, ctx).await? != plan.items {
            return Err(damaged());
        }
        for item in plan.items.clone() {
            self.check_scope(&scope, ctx).await?;
            if read_item(&root, &item.relative)? != item {
                return Err(damaged());
            }
            let (parent_relative, name) = item
                .relative
                .rsplit_once('/')
                .unwrap_or(("", item.relative.as_str()));
            let target_parent = if item.relative.is_empty() {
                parent.to_owned()
            } else {
                plan.folders
                    .get(parent_relative)
                    .ok_or_else(damaged)?
                    .clone()
            };
            if item.kind == EntryKind::Folder {
                if let Some(id) = plan.folders.get(&item.relative) {
                    let actual = ctx.run(self.provider.stat(id)).await?;
                    self.check_scope(&scope, ctx).await?;
                    let wanted = if item.relative.is_empty() {
                        plan.root_name.as_str()
                    } else {
                        name
                    };
                    if actual.id != *id
                        || actual.kind != EntryKind::Folder
                        || actual.name != wanted
                        || actual.parent_id.as_deref() != Some(&target_parent)
                    {
                        return Err(damaged());
                    }
                    continue;
                }
                let folder_name = if item.relative.is_empty() {
                    plan.root_name.as_str()
                } else {
                    name
                };
                let mutation = FileMutation {
                    op: MutateOp::Mkdir,
                    ids: vec![],
                    target: Some(MutateTarget {
                        parent_id: Some(target_parent.clone()),
                        name: Some(folder_name.into()),
                    }),
                };
                let child_key = fingerprint(&["upload-tree-folder", &record_key, &item.relative]);
                let result = self
                    .files_mutate_at(&mutation, &child_key, &scope, ctx)
                    .await?;
                self.check_scope(&scope, ctx).await?;
                if !result.failed.is_empty() || result.created.len() != 1 {
                    return Err(damaged());
                }
                let folder = &result.created[0];
                if folder.kind != EntryKind::Folder
                    || folder.name != folder_name
                    || folder.parent_id.as_deref() != Some(&target_parent)
                {
                    return Err(damaged());
                }
                plan.folders.insert(item.relative, folder.id.clone());
            } else {
                if plan.tasks.contains_key(&item.relative) {
                    continue;
                }
                // Keep the original file identity locked until the queue has
                // captured the same source. Windows sharing forbids replacement.
                let local_path = path_for(&root, &item.relative)?;
                let file: File = local::open_readonly(&local_path)?;
                if local::read_identity(&file)? != item.identity {
                    return Err(damaged());
                }
                let child_key = fingerprint(&["upload-tree-file", &record_key, &item.relative]);
                let task = self
                    .upload_start_at(
                        local_path.to_str().ok_or_else(damaged)?,
                        &target_parent,
                        &child_key,
                        &scope,
                        ctx,
                    )
                    .await?;
                plan.tasks.insert(item.relative, task.task_id);
            }
            self.save_upload_tree(&scope, &record_key, &plan)?;
        }
        Ok(plan.result(&group))
    }
    fn save_upload_tree(
        &self,
        scope: &str,
        key: &str,
        plan: &UploadTree,
    ) -> Result<(), ClientError> {
        let encoded = serde_json::to_string(plan).map_err(|_| damaged())?;
        if encoded.len() > MAX_PLAN_BYTES {
            return Err(damaged());
        }
        self.records()?
            .record_put(scope, "upload-tree", key, &encoded)
    }
    async fn validate_upload_root_destination(
        &self,
        parent: &str,
        name: &str,
        scope: &str,
        ctx: &RequestContext,
    ) -> Result<(), ClientError> {
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        let mut count = 0;
        loop {
            self.check_scope(scope, ctx).await?;
            let page = ctx
                .run(self.provider.list(parent, cursor.as_deref(), 500))
                .await?;
            self.check_scope(scope, ctx).await?;
            for entry in page.items {
                count += 1;
                if count > MAX_ENTRIES || entry.parent_id.as_deref() != Some(parent) {
                    return Err(damaged());
                }
                if entry.name.to_lowercase() == name.to_lowercase() {
                    return Err(conflict(
                        "目标已有同名或大小写冲突的目录/文件，请更换上传位置。",
                    ));
                }
            }
            let Some(next) = page.next_cursor else {
                return Ok(());
            };
            if !seen.insert(next.clone()) || seen.len() > 128 {
                return Err(damaged());
            }
            cursor = Some(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex,
    };

    #[derive(Default)]
    struct Folders {
        scope: Mutex<String>,
        folders: Mutex<BTreeMap<String, RemoteEntry>>,
        calls: AtomicUsize,
        lose_reply: AtomicBool,
        switch_after_first: AtomicBool,
        destination_conflict: AtomicBool,
    }
    fn folder(id: &str, parent: &str, name: &str) -> RemoteEntry {
        RemoteEntry {
            id: id.into(),
            parent_id: Some(parent.into()),
            name: name.into(),
            kind: EntryKind::Folder,
            size_bytes: DecimalU64(0),
            modified_at: None,
            etag: None,
            hash: None,
            mime_type: None,
            starred: None,
        }
    }
    #[async_trait]
    impl ProviderPort for Folders {
        fn id(&self) -> BackendId {
            BackendId::RustCompat
        }
        async fn account_scope(&self) -> Result<String, ClientError> {
            Ok(self.scope.lock().unwrap().clone())
        }
        async fn auth_status(&self) -> Result<AuthState, ClientError> {
            Ok(AuthState {
                backend: self.id(),
                authenticated: true,
                account_id: Some("synthetic".into()),
                credential_owner: "test".into(),
                account_tier: AccountTier::Unknown,
                message: String::new(),
                state: AuthStatus::SignedIn,
                display_name: None,
            })
        }
        async fn discover(&self) -> Result<CapabilityReport, ClientError> {
            unreachable!()
        }
        async fn list(
            &self,
            parent: &str,
            _: Option<&str>,
            _: usize,
        ) -> Result<Page<RemoteEntry>, ClientError> {
            Ok(Page {
                items: if self.destination_conflict.load(Ordering::SeqCst) {
                    vec![folder("existing", parent, "SOURCE")]
                } else {
                    vec![]
                },
                next_cursor: None,
            })
        }
        async fn stat(&self, id: &str) -> Result<RemoteEntry, ClientError> {
            self.folders
                .lock()
                .unwrap()
                .get(id)
                .cloned()
                .ok_or_else(damaged)
        }
        async fn quota(&self) -> Result<QuotaReport, ClientError> {
            unreachable!()
        }
        async fn mutate_files(
            &self,
            request: &FileMutation,
            scope: &str,
        ) -> Result<OperationResult, ClientError> {
            assert_eq!(scope, self.scope.lock().unwrap().as_str());
            assert_eq!(request.op, MutateOp::Mkdir);
            let sequence = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            let target = request.target.as_ref().unwrap();
            let entry = folder(
                &format!("remote-{sequence}"),
                target.parent_id.as_deref().unwrap(),
                target.name.as_deref().unwrap(),
            );
            self.folders
                .lock()
                .unwrap()
                .insert(entry.id.clone(), entry.clone());
            if self.switch_after_first.swap(false, Ordering::SeqCst) {
                *self.scope.lock().unwrap() = "other".into();
            }
            if self.lose_reply.load(Ordering::SeqCst) {
                return Err(ClientError::new(
                    ErrorCode::TransientNetwork,
                    "synthetic lost response",
                ));
            }
            Ok(OperationResult {
                op: request.op,
                succeeded: vec![entry.id.clone()],
                failed: vec![],
                created: vec![entry],
                replayed: false,
            })
        }
    }
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, Arc<Folders>) {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir_all(source.join("nested")).unwrap();
        std::fs::create_dir(source.join("empty")).unwrap();
        std::fs::write(source.join("one.bin"), b"one").unwrap();
        std::fs::write(source.join("nested").join("two.bin"), b"two").unwrap();
        let db = temp.path().join("tasks.db");
        let provider = Arc::new(Folders::default());
        *provider.scope.lock().unwrap() = "account".into();
        (temp, source, db, provider)
    }
    #[tokio::test]
    async fn directory_plan_preserves_structure_empty_folders_and_replays_after_restart() {
        let (_temp, source, db, provider) = fixture();
        let app = AppService::with_provider(&db, provider.clone());
        let ctx = RequestContext::default();
        let batch = app
            .upload_directory_start(source.to_str().unwrap(), "root", "tree-once", &ctx)
            .await
            .unwrap();
        assert_eq!(batch.task_ids.len(), 2);
        assert_eq!(batch.expanded_files, 2);
        assert!(batch.skipped.is_empty());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 3);
        let folders = provider.folders.lock().unwrap().clone();
        let root = folders.values().find(|e| e.name == "source").unwrap();
        for name in ["nested", "empty"] {
            assert_eq!(
                folders
                    .values()
                    .find(|e| e.name == name)
                    .unwrap()
                    .parent_id
                    .as_deref(),
                Some(root.id.as_str())
            );
        }
        for id in &batch.task_ids {
            assert_eq!(app.task(id).unwrap().kind, TaskKind::Upload);
        }
        drop(app);
        let app = AppService::with_provider(&db, provider.clone());
        let replay = app
            .upload_directory_start(source.to_str().unwrap(), "root", "tree-once", &ctx)
            .await
            .unwrap();
        assert_eq!(replay.task_ids, batch.task_ids);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 3);
    }
    #[tokio::test]
    async fn unknown_folder_creation_is_not_replayed_and_account_change_resumes_known_receipt() {
        let (_temp, source, db, provider) = fixture();
        let app = AppService::with_provider(&db, provider.clone());
        let ctx = RequestContext::default();
        provider.lose_reply.store(true, Ordering::SeqCst);
        assert!(app
            .upload_directory_start(source.to_str().unwrap(), "root", "unknown", &ctx)
            .await
            .is_err());
        assert!(app
            .upload_directory_start(source.to_str().unwrap(), "root", "unknown", &ctx)
            .await
            .is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        provider.lose_reply.store(false, Ordering::SeqCst);
        provider.switch_after_first.store(true, Ordering::SeqCst);
        assert!(app
            .upload_directory_start(source.to_str().unwrap(), "root", "switch", &ctx)
            .await
            .is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
        *provider.scope.lock().unwrap() = "account".into();
        let done = app
            .upload_directory_start(source.to_str().unwrap(), "root", "switch", &ctx)
            .await
            .unwrap();
        assert_eq!(done.task_ids.len(), 2);
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            4,
            "committed root receipt must not be sent again"
        );
    }
    #[tokio::test]
    async fn destination_case_conflict_fails_before_remote_creation() {
        let (_temp, source, db, provider) = fixture();
        provider.destination_conflict.store(true, Ordering::SeqCst);
        let app = AppService::with_provider(&db, provider.clone());
        assert_eq!(
            app.upload_directory_start(
                source.to_str().unwrap(),
                "root",
                "tree",
                &RequestContext::default()
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::Conflict
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn local_change_after_interruption_is_not_silently_replanned() {
        let (_temp, source, db, provider) = fixture();
        provider.switch_after_first.store(true, Ordering::SeqCst);
        let app = AppService::with_provider(&db, provider.clone());
        assert!(app
            .upload_directory_start(
                source.to_str().unwrap(),
                "root",
                "tree",
                &RequestContext::default()
            )
            .await
            .is_err());
        std::fs::write(source.join("new.bin"), b"new").unwrap();
        *provider.scope.lock().unwrap() = "account".into();
        assert_eq!(
            app.upload_directory_start(
                source.to_str().unwrap(),
                "root",
                "tree",
                &RequestContext::default()
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::IntegrityFailed
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn directory_picker_account_change_is_rejected_before_scan_or_creation() {
        let (_temp, source, db, provider) = fixture();
        *provider.scope.lock().unwrap() = "new-account".into();
        let app = AppService::with_provider(&db, provider.clone());
        assert_eq!(
            app.upload_directory_start_at(
                source.to_str().unwrap(),
                "root",
                "picker",
                "old-account",
                &RequestContext::default()
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::AuthExpired
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn unsafe_and_nonportable_names_are_rejected() {
        for name in [
            "..",
            "../escape",
            "a/b",
            "a\\b",
            "a:",
            "CON",
            "NUL.txt",
            "tail.",
            "x*",
            "",
        ] {
            assert!(valid_name(name).is_err(), "{name}");
        }
        assert!(valid_name("文件😀.mp4").is_ok());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_in_tree_is_rejected_before_any_remote_write() {
        let (_temp, source, db, provider) = fixture();
        std::os::unix::fs::symlink(source.join("one.bin"), source.join("linked.bin")).unwrap();
        let app = AppService::with_provider(&db, provider.clone());
        assert!(app
            .upload_directory_start(
                source.to_str().unwrap(),
                "root",
                "tree",
                &RequestContext::default()
            )
            .await
            .is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }
}
