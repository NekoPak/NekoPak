//! Native mount configuration and account/version-checked callbacks.
use super::*;
use nekopak_media::native::RangeSource;
use nekopak_mount::{CancellationToken, MountEntry, MountSnapshot, ReadSource};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(windows)]
use std::path::PathBuf;
use std::{sync::atomic::Ordering, time::Duration};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountConfig {
    #[serde(default)]
    pub config_id: String,
    pub mode: String,
    pub remote_id: String,
    #[serde(default)]
    pub local_path: Option<String>,
    #[serde(default)]
    pub drive_letter: Option<String>,
}
pub(crate) enum NativeMount {
    #[cfg(windows)]
    Cloud(nekopak_mount::CloudFilesMount),
    #[cfg(windows)]
    Drive(nekopak_mount::WinFspMount),
    #[cfg(test)]
    Fake(Arc<std::sync::atomic::AtomicBool>),
}
pub(crate) struct Mounted {
    scope: String,
    config: MountConfig,
    native: NativeMount,
}
impl Mounted {
    fn stop(&mut self) -> Result<(), ClientError> {
        match &mut self.native {
            #[cfg(windows)]
            NativeMount::Cloud(m) => m.stop(),
            #[cfg(windows)]
            NativeMount::Drive(m) => m.stop(),
            #[cfg(test)]
            NativeMount::Fake(connected) => {
                connected.store(false, Ordering::Release);
                Ok(())
            }
            #[cfg(not(any(windows, test)))]
            _ => Err(nekopak_provider::unsupported(BackendId::RustCompat)),
        }
    }
    fn connected(&self) -> bool {
        match &self.native {
            #[cfg(windows)]
            NativeMount::Cloud(m) => m.is_connected(),
            #[cfg(windows)]
            NativeMount::Drive(m) => m.is_connected(),
            #[cfg(test)]
            NativeMount::Fake(connected) => connected.load(Ordering::Acquire),
            #[cfg(not(any(windows, test)))]
            _ => false,
        }
    }
}
struct PinnedEntry {
    metadata: RemoteEntry,
    source_identity: String,
}
struct Source {
    service: std::sync::Weak<AppService>,
    runtime: tokio::runtime::Handle,
    scope: String,
    epoch: u64,
    entries: BTreeMap<String, PinnedEntry>,
}
impl ReadSource for Source {
    fn read_exact(
        &self,
        entry: &MountEntry,
        offset: u64,
        length: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, ClientError> {
        cancel.check()?;
        let expected = self.entries.get(&entry.remote_id).ok_or_else(invalid)?;
        if entry.size != expected.metadata.size_bytes.0
            || entry.identity != fingerprint(&[&self.scope, &expected.source_identity])
        {
            return Err(invalid());
        }
        let service = self.service.upgrade().ok_or_else(invalid)?;
        let result = self.runtime.block_on(async {
            if service.mount_epoch.load(Ordering::SeqCst) != self.epoch { return Err(stale()); }
            let ctx = RequestContext::new(Duration::from_secs(40));
            let operation = async {
                service.check_scope(&self.scope, &ctx).await?;
                let source = service
                    .media_source(&entry.remote_id, "original", &ctx)
                    .await?;
                if !same_file(source.entry(), &expected.metadata)
                    || source.identity() != expected.source_identity
                {
                    return Err(invalid());
                }
                let bytes = source.read_exact(offset, length).await?;
                service.check_scope(&self.scope, &ctx).await?;
                if service.mount_epoch.load(Ordering::SeqCst) != self.epoch { return Err(stale()); }
                cancel.check()?;
                Ok(bytes)
            };
            // One deadline covers metadata, identity checks, cache/network and scope.
            // Cancellation covers all stages, including obtaining a fresh media source.
            let operation = ctx.run(operation);
            tokio::pin!(operation);
            loop {
                tokio::select! {
                    result = &mut operation => return result,
                    _ = tokio::time::sleep(Duration::from_millis(25)) => {
                        cancel.check()?;
                        if service.mount_epoch.load(Ordering::SeqCst) != self.epoch { return Err(stale()); }
                    }
                }
            }
        });
        // The last AppService owner must never be released on a native callback
        // thread: its mounts would synchronously wait for that same callback.
        self.runtime.spawn_blocking(move || drop(service));
        result
    }
}
fn same_file(a: &RemoteEntry, b: &RemoteEntry) -> bool {
    a.id == b.id
        && a.kind == b.kind
        && a.size_bytes == b.size_bytes
        && a.hash == b.hash
        && a.etag == b.etag
        && a.modified_at == b.modified_at
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorCode::IntegrityFailed,
        "挂载来源或配置已改变，读取已停止。",
    )
}
fn stale() -> ClientError {
    ClientError::new(
        ErrorCode::Cancelled,
        "账号或挂载状态已改变，本次挂载未保留。",
    )
}
fn conflict() -> ClientError {
    ClientError::new(
        ErrorCode::Conflict,
        "此配置或目标已挂载，请先停止现有挂载。",
    )
}

async fn stop_owned(mut mounted: Mounted) -> Result<(), ClientError> {
    tokio::task::spawn_blocking(move || mounted.stop())
        .await
        .map_err(|_| invalid())?
}
impl AppService {
    /// The epoch is updated under the same short lock used by final insertion.
    /// It invalidates starts already doing remote IO without waiting for that IO.
    fn invalidate_mounts(&self) -> Result<Vec<Mounted>, ClientError> {
        let mut mounts = self.mounts.lock().map_err(|_| invalid())?;
        self.mount_epoch.fetch_add(1, Ordering::SeqCst);
        Ok(mounts.drain().map(|(_, mounted)| mounted).collect())
    }
    async fn stop_owned_mounts(mounts: Vec<Mounted>) -> Result<(), ClientError> {
        let mut failure = None;
        for mounted in mounts {
            if let Err(error) = stop_owned(mounted).await {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
    pub async fn stop_all_mounts(&self) -> Result<(), ClientError> {
        let mounts = self.invalidate_mounts()?;
        Self::stop_owned_mounts(mounts).await?;
        let _gate = self.mount_gate.lock().await;
        Self::stop_owned_mounts(self.invalidate_mounts()?).await
    }
    /// Hold this guard across login/logout/switch mutations to prevent a new
    /// mount from slipping into the window between stop_all and session change.
    pub(crate) async fn mount_account_change_guard(
        &self,
    ) -> Result<tokio::sync::OwnedMutexGuard<()>, ClientError> {
        let mounts = self.invalidate_mounts()?;
        Self::stop_owned_mounts(mounts).await?;
        let guard = self.mount_gate.clone().lock_owned().await;
        Self::stop_owned_mounts(self.invalidate_mounts()?).await?;
        Ok(guard)
    }
    pub async fn mount_capabilities(&self) -> Result<Value, ClientError> {
        let caps = tokio::task::spawn_blocking(nekopak_mount::native_capabilities)
            .await
            .map_err(|_| invalid())?;
        let check = |id: &str, label: &str, available: bool, detail: String| json!({"id":id,"label":label,"status":if available{"pass"}else{"fail"},"detail":detail});
        Ok(
            json!({"platform":if cfg!(windows){"windows"}else{"unknown"},"checked_at":now(),"modes":[
                {"mode":"cloud_files_sync_root","available":caps.cloud_files.available,"checks":[check("cfapi","Windows Cloud Files",caps.cloud_files.available,caps.cloud_files.reason.unwrap_or_else(||"系统 API 可用；开始时校验所选目录与真实随机读取。".into()))]},
                {"mode":"winfsp_drive","available":caps.winfsp.available,"checks":[check("winfsp","WinFsp",caps.winfsp.available,caps.winfsp.reason.unwrap_or_else(||"已检测到运行时接口；开始时校验实际驱动与随机读取。".into()))],"notice":nekopak_mount::WINFSP_NOTICE,"notice_url":nekopak_mount::WINFSP_URL}
            ]}),
        )
    }
    pub async fn mount_configs_list(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<MountConfig>, ClientError> {
        let scope = self.account_scope(ctx).await?;
        self.records()?
            .record_list(&scope, "mount-config")?
            .into_iter()
            .map(|(_, s)| serde_json::from_str(&s).map_err(|_| invalid()))
            .collect()
    }
    pub async fn mount_config_save(
        &self,
        config: &MountConfig,
        ctx: &RequestContext,
    ) -> Result<MountConfig, ClientError> {
        let _gate = ctx.run(async { Ok(self.mount_gate.lock().await) }).await?;
        let scope = self.account_scope(ctx).await?;
        let mut config = config.clone();
        validate_config(&mut config, &self.data_root)?;
        if config.config_id.is_empty() {
            config.config_id = uuid::Uuid::new_v4().to_string();
        }
        if self
            .mounts
            .lock()
            .map_err(|_| invalid())?
            .contains_key(&config.config_id)
        {
            return Err(conflict());
        }
        self.records()?.record_put(
            &scope,
            "mount-config",
            &config.config_id,
            &serde_json::to_string(&config).map_err(|_| invalid())?,
        )?;
        Ok(config)
    }
    async fn mount_inputs(
        self: &Arc<Self>,
        config: &MountConfig,
        scope: &str,
        epoch: u64,
        ctx: &RequestContext,
    ) -> Result<(Arc<MountSnapshot>, Arc<Source>), ClientError> {
        let tree = self.walk(&config.remote_id, scope, ctx, None).await?;
        let mut entries = BTreeMap::new();
        let mut files = Vec::new();
        for item in tree {
            if self.mount_epoch.load(Ordering::SeqCst) != epoch {
                return Err(stale());
            }
            let directory = item.entry.kind == EntryKind::Folder;
            let identity = if directory {
                fingerprint(&[scope, &item.entry.id])
            } else {
                let source = self.media_source(&item.entry.id, "original", ctx).await?;
                if !same_file(source.entry(), &item.entry) {
                    return Err(invalid());
                }
                let identity = source.identity().to_owned();
                if identity.is_empty() {
                    return Err(invalid());
                }
                entries.insert(
                    item.entry.id.clone(),
                    PinnedEntry {
                        metadata: item.entry.clone(),
                        source_identity: identity.clone(),
                    },
                );
                fingerprint(&[scope, &identity])
            };
            files.push(MountEntry {
                path: item.relative.join("/"),
                remote_id: item.entry.id,
                identity,
                size: if directory {
                    0
                } else {
                    item.entry.size_bytes.0
                },
                directory,
                modified_filetime: 0,
            });
        }
        self.check_scope(scope, ctx).await?;
        Ok((
            Arc::new(MountSnapshot::new(files)?),
            Arc::new(Source {
                service: Arc::downgrade(self),
                runtime: tokio::runtime::Handle::current(),
                scope: scope.into(),
                epoch,
                entries,
            }),
        ))
    }
    pub async fn mount_start(
        self: &Arc<Self>,
        id: &str,
        read_only: bool,
        ctx: &RequestContext,
    ) -> Result<Value, ClientError> {
        if !read_only {
            return Err(ClientError::new(
                ErrorCode::Unsupported,
                "挂载当前只支持只读。",
            ));
        }
        let _gate = ctx.run(async { Ok(self.mount_gate.lock().await) }).await?;
        let epoch = self.mount_epoch.load(Ordering::SeqCst);
        let scope = self.account_scope(ctx).await?;
        let mut config: MountConfig = serde_json::from_str(
            &self
                .records()?
                .record_get(&scope, "mount-config", id)?
                .ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())?;
        validate_config(&mut config, &self.data_root)?;
        if config.config_id != id {
            return Err(invalid());
        }
        {
            let mounts = self.mounts.lock().map_err(|_| invalid())?;
            if mounts.contains_key(id) || mounts.values().any(|m| same_target(&m.config, &config)) {
                return Err(conflict());
            }
        }
        let (snapshot, source) = self.mount_inputs(&config, &scope, epoch, ctx).await?;
        if self.mount_epoch.load(Ordering::SeqCst) != epoch {
            return Err(stale());
        }
        let copy = config.clone();
        let native = tokio::task::spawn_blocking(move || start_native(copy, snapshot, source))
            .await
            .map_err(|_| invalid())??;
        let mounted = Mounted {
            scope: scope.clone(),
            config,
            native,
        };
        if let Err(error) = self.check_scope(&scope, ctx).await {
            stop_owned(mounted).await?;
            return Err(error);
        }
        self.commit_mount(id, mounted, epoch).await
    }
    async fn commit_mount(
        &self,
        id: &str,
        mounted: Mounted,
        epoch: u64,
    ) -> Result<Value, ClientError> {
        let mut rejected = Some(mounted);
        let result = {
            let mut mounts = self.mounts.lock().map_err(|_| invalid())?;
            let mounted = rejected.as_ref().ok_or_else(invalid)?;
            if self.mount_epoch.load(Ordering::SeqCst) != epoch {
                Err(stale())
            } else if mounts.contains_key(id)
                || mounts
                    .values()
                    .any(|m| same_target(&m.config, &mounted.config))
            {
                Err(conflict())
            } else if !mounted.connected() {
                Err(ClientError::new(
                    ErrorCode::TransientNetwork,
                    "挂载连接已停止。",
                ))
            } else {
                let output = state(mounted);
                mounts.insert(id.into(), rejected.take().ok_or_else(invalid)?);
                Ok(output)
            }
        };
        if let Some(mounted) = rejected {
            stop_owned(mounted).await?;
        }
        result
    }
    pub async fn mount_status(&self, id: &str, ctx: &RequestContext) -> Result<Value, ClientError> {
        let scope = self.account_scope(ctx).await?;
        let mounts = self.mounts.lock().map_err(|_| invalid())?;
        Ok(mounts
            .get(id)
            .filter(|m| m.scope == scope)
            .map(state)
            .unwrap_or_else(|| json!({"id":id,"status":"stopped","read_only":true})))
    }
    pub async fn mount_stop(&self, id: &str, ctx: &RequestContext) -> Result<(), ClientError> {
        let _gate = ctx.run(async { Ok(self.mount_gate.lock().await) }).await?;
        let scope = self.account_scope(ctx).await?;
        let mounted = {
            let mut mounts = self.mounts.lock().map_err(|_| invalid())?;
            if mounts.get(id).is_some_and(|m| m.scope != scope) {
                return Err(ClientError::new(
                    ErrorCode::PermissionDenied,
                    "挂载不属于当前账号。",
                ));
            }
            mounts.remove(id)
        };
        if let Some(mounted) = mounted {
            stop_owned(mounted).await?;
        }
        Ok(())
    }
}
fn start_native(
    config: MountConfig,
    snapshot: Arc<MountSnapshot>,
    source: Arc<Source>,
) -> Result<NativeMount, ClientError> {
    #[cfg(windows)]
    {
        if config.mode == "cloud_files_sync_root" {
            Ok(NativeMount::Cloud(nekopak_mount::CloudFilesMount::start(
                &PathBuf::from(config.local_path.ok_or_else(invalid)?),
                snapshot,
                source,
            )?))
        } else {
            Ok(NativeMount::Drive(nekopak_mount::WinFspMount::start(
                config.drive_letter.as_deref().ok_or_else(invalid)?,
                snapshot,
                source,
            )?))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (config, snapshot, source);
        Err(ClientError::new(
            ErrorCode::Unsupported,
            "当前平台没有原生挂载。",
        ))
    }
}
fn validate_config(config: &mut MountConfig, data_root: &Path) -> Result<(), ClientError> {
    if config.remote_id.is_empty() || config.remote_id.len() > 4096 || config.config_id.len() > 128
    {
        return Err(invalid());
    }
    match config.mode.as_str() {
        "cloud_files_sync_root" => {
            let path = config.local_path.as_deref().ok_or_else(invalid)?;
            validate_destination(path)?;
            let path = Path::new(path);
            let parent = path.parent().ok_or_else(invalid)?;
            let _locks = downloads::lock_parents(parent)?;
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(invalid)?;
            nekopak_mount::validate_relative_path(name)?;
            let target = parent.canonicalize().map_err(|_| invalid())?.join(name);
            let data = data_root.canonicalize().map_err(|_| invalid())?;
            if paths_overlap(&target, &data) {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "挂载目录不能与应用数据目录重叠。",
                ));
            }
            config.drive_letter = None;
        }
        "winfsp_drive" => {
            let drive = config.drive_letter.as_deref().unwrap_or("P:");
            if drive.len() != 2
                || !drive.as_bytes()[0].is_ascii_alphabetic()
                || !drive.ends_with(':')
            {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "请输入有效盘符，如 P:。",
                ));
            }
            config.drive_letter = Some(drive.to_ascii_uppercase());
            config.local_path = None;
        }
        _ => return Err(invalid()),
    }
    Ok(())
}
fn normalized(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_uppercase()
}
fn paths_overlap(a: &Path, b: &Path) -> bool {
    let a = normalized(a);
    let b = normalized(b);
    a == b || a.starts_with(&(b.clone() + "\\")) || b.starts_with(&(a + "\\"))
}
fn same_target(a: &MountConfig, b: &MountConfig) -> bool {
    if a.mode != b.mode {
        return false;
    }
    if a.mode == "winfsp_drive" {
        a.drive_letter
            .as_deref()
            .zip(b.drive_letter.as_deref())
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
    } else {
        a.local_path
            .as_deref()
            .zip(b.local_path.as_deref())
            .is_some_and(|(a, b)| paths_overlap(Path::new(a), Path::new(b)))
    }
}
fn state(mounted: &Mounted) -> Value {
    json!({"id":mounted.config.config_id,"mode":mounted.config.mode,"status":if mounted.connected(){"mounted"}else{"stopped"},"read_only":true,"mount_point":if mounted.config.mode=="cloud_files_sync_root"{mounted.config.local_path.clone()}else{mounted.config.drive_letter.clone()}})
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use nekopak_provider::{DownloadChunk, DownloadInfo, DownloadObserver};
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize},
        Mutex,
    };
    struct Remote {
        scope: Mutex<String>,
        version: AtomicUsize,
        block_info: AtomicBool,
        info_entered: AtomicBool,
    }
    fn file() -> RemoteEntry {
        RemoteEntry {
            id: "one".into(),
            parent_id: Some("root".into()),
            name: "one.bin".into(),
            kind: EntryKind::File,
            size_bytes: DecimalU64(8192),
            modified_at: None,
            etag: None,
            hash: None,
            mime_type: None,
            starred: None,
        }
    }
    #[async_trait]
    impl ProviderPort for Remote {
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
        async fn auth_status(&self) -> Result<AuthState, ClientError> {
            unreachable!()
        }
        async fn stat(&self, _: &str) -> Result<RemoteEntry, ClientError> {
            Ok(file())
        }
        async fn list(
            &self,
            _: &str,
            _: Option<&str>,
            _: usize,
        ) -> Result<Page<RemoteEntry>, ClientError> {
            Ok(Page {
                items: vec![file()],
                next_cursor: None,
            })
        }
        async fn download_info(&self, _: &str) -> Result<DownloadInfo, ClientError> {
            self.info_entered.store(true, Ordering::Release);
            if self.block_info.load(Ordering::Acquire) {
                std::future::pending::<()>().await;
            }
            Ok(DownloadInfo {
                entry: file(),
                identity: format!("version-{}", self.version.load(Ordering::Acquire)),
                account_scope: self.account_scope().await?,
            })
        }
        async fn download_chunk(
            &self,
            _: &str,
            identity: &str,
            scope: &str,
            start: u64,
            length: u64,
            _: DownloadObserver,
        ) -> Result<DownloadChunk, ClientError> {
            if identity != format!("version-{}", self.version.load(Ordering::Acquire))
                || scope != self.account_scope().await?
            {
                return Err(invalid());
            }
            Ok(DownloadChunk {
                bytes: (start..start + length).map(|n| (n % 251) as u8).collect(),
                total: 8192,
            })
        }
    }
    fn setup() -> (tempfile::TempDir, Arc<AppService>, Arc<Remote>) {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("app-data");
        std::fs::create_dir(&data).unwrap();
        let provider = Arc::new(Remote {
            scope: Mutex::new("account-one".into()),
            version: AtomicUsize::new(1),
            block_info: AtomicBool::new(false),
            info_entered: AtomicBool::new(false),
        });
        let app = Arc::new(AppService::with_provider(
            &data.join("tasks.db"),
            provider.clone(),
        ));
        (temp, app, provider)
    }
    fn config() -> MountConfig {
        MountConfig {
            config_id: "config-one".into(),
            mode: "winfsp_drive".into(),
            remote_id: "root".into(),
            local_path: None,
            drive_letter: Some("P:".into()),
        }
    }
    fn fake(id: &str, drive: &str) -> (Mounted, Arc<AtomicBool>) {
        let connected = Arc::new(AtomicBool::new(true));
        let mut config = config();
        config.config_id = id.into();
        config.drive_letter = Some(drive.into());
        (
            Mounted {
                scope: "account-one".into(),
                config,
                native: NativeMount::Fake(connected.clone()),
            },
            connected,
        )
    }
    #[tokio::test]
    async fn config_accepts_new_child_but_rejects_data_overlap_and_is_account_scoped() {
        let (temp, app, provider) = setup();
        let ctx = RequestContext::default();
        let mut config = config();
        config.mode = "cloud_files_sync_root".into();
        config.local_path = Some(temp.path().join("new-root").to_string_lossy().into_owned());
        let saved = app.mount_config_save(&config, &ctx).await.unwrap();
        assert!(saved.drive_letter.is_none());
        assert!(!Path::new(saved.local_path.as_ref().unwrap()).exists());
        assert_eq!(app.mount_configs_list(&ctx).await.unwrap().len(), 1);
        config.local_path = Some(app.data_root.join("CACHE").to_string_lossy().into_owned());
        assert!(app.mount_config_save(&config, &ctx).await.is_err());
        *provider.scope.lock().unwrap() = "account-two".into();
        assert!(app.mount_configs_list(&ctx).await.unwrap().is_empty());
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pinned_source_rejects_same_size_replacement_without_metadata_hash_or_mtime() {
        let (_temp, app, provider) = setup();
        let ctx = RequestContext::default();
        let (snapshot, source) = app
            .mount_inputs(&config(), "account-one", 0, &ctx)
            .await
            .unwrap();
        let entry = snapshot.get("one.bin").unwrap().clone();
        let s = source.clone();
        let e = entry.clone();
        let read = tokio::task::spawn_blocking(move || {
            s.read_exact(&e, 4096, 128, &CancellationToken::default())
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            read,
            (4096..4224).map(|n| (n % 251) as u8).collect::<Vec<_>>()
        );
        provider.version.store(2, Ordering::Release);
        let s = source.clone();
        let e = entry.clone();
        assert_eq!(
            tokio::task::spawn_blocking(move || s.read_exact(
                &e,
                0,
                1,
                &CancellationToken::default()
            ))
            .await
            .unwrap()
            .unwrap_err()
            .code,
            ErrorCode::IntegrityFailed
        );
        provider.version.store(1, Ordering::Release);
        *provider.scope.lock().unwrap() = "another-account".into();
        assert_eq!(
            tokio::task::spawn_blocking(move || source.read_exact(
                &entry,
                0,
                1,
                &CancellationToken::default()
            ))
            .await
            .unwrap()
            .unwrap_err()
            .code,
            ErrorCode::AuthExpired
        );
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancellation_covers_metadata_and_source_does_not_keep_service_alive() {
        let (_temp, app, provider) = setup();
        let ctx = RequestContext::default();
        let (snapshot, source) = app
            .mount_inputs(&config(), "account-one", 0, &ctx)
            .await
            .unwrap();
        let entry = snapshot.get("one.bin").unwrap().clone();
        provider.block_info.store(true, Ordering::Release);
        provider.info_entered.store(false, Ordering::Release);
        let cancel = CancellationToken::default();
        let c = cancel.clone();
        let s = source.clone();
        let e = entry.clone();
        let reader = tokio::task::spawn_blocking(move || s.read_exact(&e, 0, 1, &c));
        tokio::time::timeout(Duration::from_secs(2), async {
            while !provider.info_entered.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        cancel.cancel();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), reader)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        let weak = Arc::downgrade(&app);
        drop(app);
        tokio::time::timeout(Duration::from_secs(2), async {
            while weak.upgrade().is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(tokio::task::spawn_blocking(move || source.read_exact(
            &entry,
            0,
            1,
            &CancellationToken::default()
        ))
        .await
        .unwrap()
        .is_err());
    }
    #[tokio::test]
    async fn stale_or_duplicate_commit_stops_only_new_mount_and_status_tracks_dispatcher() {
        let (_temp, app, _) = setup();
        let ctx = RequestContext::default();
        let (first, first_connected) = fake("first", "P:");
        app.commit_mount("first", first, 0).await.unwrap();
        let (duplicate, duplicate_connected) = fake("second", "p:");
        assert_eq!(
            app.commit_mount("second", duplicate, 0)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert!(!duplicate_connected.load(Ordering::Acquire));
        assert!(first_connected.load(Ordering::Acquire));
        first_connected.store(false, Ordering::Release);
        assert_eq!(
            app.mount_status("first", &ctx).await.unwrap()["status"],
            "stopped"
        );
        let stale_epoch = app.mount_epoch.load(Ordering::SeqCst);
        app.stop_all_mounts().await.unwrap();
        let (late, connected) = fake("late", "Q:");
        assert_eq!(
            app.commit_mount("late", late, stale_epoch)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        assert!(!connected.load(Ordering::Acquire));
        assert!(app.mounts.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn account_change_guard_holds_start_gate_and_invalidates_pending_generation() {
        let (_temp, app, _) = setup();
        let epoch = app.mount_epoch.load(Ordering::SeqCst);
        let guard = app.mount_account_change_guard().await.unwrap();
        assert_ne!(app.mount_epoch.load(Ordering::SeqCst), epoch);
        assert!(app.mount_gate.try_lock().is_err());
        let short = RequestContext::new(Duration::from_millis(20));
        assert!(app.mount_config_save(&config(), &short).await.is_err());
        drop(guard);
        assert!(app.mount_gate.try_lock().is_ok());
    }
}
