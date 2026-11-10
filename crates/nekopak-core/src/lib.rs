mod catalog;
mod cloud;
mod downloads;
mod history;
mod intents;
mod media;
mod rates;
mod retry;
mod settings;
pub use media::TransferOptions;
mod jobs;
mod local;
mod mounts;
mod reference_archive;
pub mod store;
mod streams;
mod sync;
mod torrent;
pub mod updates;
mod upload_tree;
mod uploads;
pub use mounts::MountConfig;
use nekopak_domain::*;
use nekopak_provider::{CloudTaskRequest, ProviderPort, ProviderRouter, RequestContext};
use nekopak_provider_mock::{MockProvider, Scenario};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, sync::Arc};
use store::Store;
use tokio::sync::broadcast;
pub use torrent::{TorrentPreview, MAX_TORRENT_BYTES};

pub fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("UTC time is RFC3339")
}

pub struct AppService {
    torrent_imports: std::sync::Mutex<std::collections::HashMap<String, torrent::PendingTorrent>>,
    foreground: std::sync::atomic::AtomicBool,
    store: Option<Store>,
    intents: Option<intents::IntentStore>,
    download_gate: std::sync::Mutex<()>,
    download_rates: rates::Rates,
    verified_downloads:
        std::sync::Mutex<std::collections::HashMap<String, (std::time::SystemTime, u64)>>,
    download_cancellations: std::sync::Mutex<std::collections::HashMap<String, RequestContext>>,
    scans: std::sync::Mutex<std::collections::HashMap<String, RequestContext>>,
    scan_events: broadcast::Sender<serde_json::Value>,
    cloud_gate: tokio::sync::Mutex<()>,
    sync_gate: tokio::sync::Mutex<()>,
    sync_auto_gate: tokio::sync::Mutex<()>,
    data_root: std::path::PathBuf,
    media_controls:
        std::sync::Mutex<std::collections::HashMap<String, nekopak_media::native::RenderControl>>,
    media_busy: std::sync::atomic::AtomicBool,
    upload_busy: std::sync::atomic::AtomicBool,
    upload_tree_gate: tokio::sync::Mutex<()>,
    reference_gate: tokio::sync::Mutex<()>,
    streams: std::sync::Mutex<std::collections::HashMap<String, RequestContext>>,
    cache_gate: Arc<std::sync::Mutex<()>>,
    stream_epoch: std::sync::atomic::AtomicU64,
    mounts: std::sync::Mutex<std::collections::HashMap<String, mounts::Mounted>>,
    mount_gate: Arc<tokio::sync::Mutex<()>>,
    mount_epoch: std::sync::atomic::AtomicU64,
    storage_error: Option<ClientError>,
    router: ProviderRouter,
    provider: Arc<dyn ProviderPort>,
    events: broadcast::Sender<TransferEvent>,
}
impl AppService {
    pub fn mock(path: &Path, scenario: Scenario) -> Self {
        Self::with_provider(path, Arc::new(MockProvider::new(scenario)))
    }
    pub fn with_provider(path: &Path, provider: Arc<dyn ProviderPort>) -> Self {
        Self::with_backends(path, provider.clone(), vec![provider])
    }
    pub fn with_backends(
        path: &Path,
        provider: Arc<dyn ProviderPort>,
        providers: Vec<Arc<dyn ProviderPort>>,
    ) -> Self {
        let (mut store, mut storage_error) = match Store::open(path) {
            Ok(store) => (Some(store), None),
            Err(error) => (None, Some(error)),
        };
        let intents = if store.is_some() {
            let mut filename = path.as_os_str().to_os_string();
            filename.push(".actions.sqlite3");
            let initialize = (|| {
                if store
                    .as_ref()
                    .ok_or_else(|| ClientError::new(ErrorCode::Internal, "存储不可用。"))?
                    .actions_initialized()?
                    && !Path::new(&filename).exists()
                {
                    return Err(ClientError::new(
                        ErrorCode::IntegrityFailed,
                        "操作记录文件缺失，已保留任务数据库；未创建空记录或重发请求。",
                    ));
                }
                let journal = intents::IntentStore::open(Path::new(&filename))?;
                store
                    .as_ref()
                    .ok_or_else(|| ClientError::new(ErrorCode::Internal, "存储不可用。"))?
                    .mark_actions_initialized()?;
                Ok(journal)
            })();
            match initialize {
                Ok(intents) => Some(intents),
                Err(error) => {
                    store = None;
                    storage_error = Some(error);
                    None
                }
            }
        } else {
            None
        };
        let (events, _) = broadcast::channel(256);
        let (scan_events, _) = broadcast::channel(64);
        Self {
            torrent_imports: Default::default(),
            foreground: std::sync::atomic::AtomicBool::new(true),
            store,
            intents,
            download_gate: Default::default(),
            download_rates: Default::default(),
            verified_downloads: Default::default(),
            download_cancellations: Default::default(),
            scans: Default::default(),
            scan_events,
            cloud_gate: tokio::sync::Mutex::new(()),
            sync_gate: tokio::sync::Mutex::new(()),
            sync_auto_gate: tokio::sync::Mutex::new(()),
            data_root: path.parent().unwrap_or_else(|| Path::new(".")).to_owned(),
            media_controls: Default::default(),
            media_busy: Default::default(),
            upload_busy: Default::default(),
            upload_tree_gate: Default::default(),
            reference_gate: Default::default(),
            streams: Default::default(),
            cache_gate: Arc::new(std::sync::Mutex::new(())),
            stream_epoch: std::sync::atomic::AtomicU64::new(0),
            mounts: Default::default(),
            mount_gate: Arc::new(tokio::sync::Mutex::new(())),
            mount_epoch: std::sync::atomic::AtomicU64::new(0),
            storage_error,
            router: ProviderRouter::new(providers),
            provider,
            events,
        }
    }
    fn store(&self) -> Result<&Store, ClientError> {
        self.store.as_ref().ok_or_else(|| {
            self.storage_error
                .clone()
                .unwrap_or_else(|| ClientError::new(ErrorCode::Internal, "Storage unavailable."))
        })
    }
    pub fn app_status(&self) -> AppStatus {
        AppStatus {
            system_locale: None,
            version: env!("CARGO_PKG_VERSION").into(),
            contract_version: CONTRACT_VERSION.into(),
            mode: if self.provider.id() == BackendId::Mock {
                "mock"
            } else {
                "rust_compat_preview"
            }
            .into(),
            storage: StorageStatus {
                status: if self.store.is_some() {
                    "ready"
                } else {
                    "recovery_required"
                }
                .into(),
                schema_version: store::SCHEMA_VERSION,
                error: self.storage_error.clone(),
            },
            features: BTreeMap::from([
                ("mock".into(), self.provider.id() == BackendId::Mock),
                (
                    "real_providers".into(),
                    self.provider.id() != BackendId::Mock,
                ),
                ("real_transfers".into(), self.provider.supports_downloads()),
                (
                    "settings".into(),
                    self.provider.id() == BackendId::RustCompat,
                ),
                (
                    "auth_sign_in".into(),
                    self.provider.supports_password_auth(),
                ),
                (
                    "auth_logout".into(),
                    self.provider.id() == BackendId::RustCompat,
                ),
                (
                    "segment_download".into(),
                    self.provider.id() == BackendId::RustCompat,
                ),
                (
                    "sync".into(),
                    self.provider.id() == BackendId::RustCompat && !cfg!(target_os = "android"),
                ),
                (
                    "sync_bidirectional".into(),
                    self.provider.id() == BackendId::RustCompat && !cfg!(target_os = "android"),
                ),
                ("mount".into(), cfg!(windows)),
                ("experimental_network".into(), false),
                ("telemetry".into(), false),
                ("local_sources".into(), !cfg!(target_os = "android")),
                ("media_session".into(), true),
                (
                    "app_updates".into(),
                    cfg!(any(
                        all(windows, target_arch = "x86_64"),
                        all(target_os = "android", target_arch = "aarch64")
                    )) || cfg!(debug_assertions),
                ),
                (
                    "torrent_import".into(),
                    self.provider.id() == BackendId::RustCompat,
                ),
                ("local_media_probe".into(), true),
                ("local_clip".into(), true),
                ("ui_prefs".into(), true),
                (
                    "accounts".into(),
                    self.provider.id() == BackendId::RustCompat,
                ),
                (
                    "share_preview".into(),
                    self.provider.id() == BackendId::RustCompat,
                ),
                (
                    "upload_directory".into(),
                    self.provider.id() == BackendId::RustCompat && !cfg!(target_os = "android"),
                ),
                ("upload".into(), !cfg!(target_os = "android")),
                ("mobile".into(), cfg!(target_os = "android")),
                ("desktop_window".into(), !cfg!(target_os = "android")),
                ("tray".into(), !cfg!(target_os = "android")),
                ("directory_picker".into(), !cfg!(target_os = "android")),
                (
                    "transfer_open_directory".into(),
                    !cfg!(target_os = "android"),
                ),
                ("transfer_export".into(), cfg!(target_os = "android")),
                ("media_external".into(), !cfg!(target_os = "android")),
                ("player_config".into(), !cfg!(target_os = "android")),
                ("background_transfers".into(), !cfg!(target_os = "android")),
                ("autostart".into(), cfg!(windows)),
                (
                    "reference_archive".into(),
                    self.provider.id() == BackendId::RustCompat,
                ),
            ]),
        }
    }
    pub fn subscribe(&self) -> broadcast::Receiver<TransferEvent> {
        self.events.subscribe()
    }
    fn emit(&self, task: &TransferTask) {
        // Broadcast has no subscribers during startup/headless runs; persisted snapshots remain authoritative.
        let _ = self.events.send(TransferEvent::from(task));
    }
    pub fn uses_native_transfers(&self) -> bool {
        self.provider.id() != BackendId::Mock
    }
    pub async fn backend_discover(
        &self,
        context: &RequestContext,
    ) -> Result<Vec<CapabilityReport>, ClientError> {
        self.router.discover(context).await
    }
    pub async fn files_list(
        &self,
        parent: &str,
        cursor: Option<&str>,
        limit: Option<u32>,
        context: &RequestContext,
    ) -> Result<Page<RemoteEntry>, ClientError> {
        if parent.starts_with("local:") {
            return self.local_list(parent, cursor, limit);
        }
        let limit = page_limit(limit)?;
        let provider = self
            .router
            .select(Operation::List, self.provider.id(), context)
            .await?;
        context.run(provider.list(parent, cursor, limit)).await
    }
    pub async fn files_stat(
        &self,
        id: &str,
        context: &RequestContext,
    ) -> Result<RemoteEntry, ClientError> {
        if id.starts_with("local:") {
            return self.local_stat(id);
        }
        let provider = self
            .router
            .select(Operation::Stat, self.provider.id(), context)
            .await?;
        context.run(provider.stat(id)).await
    }
    pub async fn files_query(
        &self,
        query: &FileQuery,
        cursor: Option<&str>,
        limit: Option<u32>,
        context: &RequestContext,
    ) -> Result<Page<RemoteEntry>, ClientError> {
        let provider = self
            .router
            .select(Operation::List, self.provider.id(), context)
            .await?;
        context
            .run(provider.query_files(query, cursor, page_limit(limit)?))
            .await
    }
    pub async fn shares_list(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
        context: &RequestContext,
    ) -> Result<Page<ShareInfo>, ClientError> {
        context
            .run(self.provider.list_shares(cursor, page_limit(limit)?))
            .await
    }
    pub async fn files_mutate(
        &self,
        request: &FileMutation,
        key: &str,
        context: &RequestContext,
    ) -> Result<OperationResult, ClientError> {
        let scope = context.run(self.provider.account_scope()).await?;
        self.files_mutate_at(request, key, &scope, context).await
    }
    pub(crate) async fn files_mutate_at(
        &self,
        request: &FileMutation,
        key: &str,
        scope: &str,
        context: &RequestContext,
    ) -> Result<OperationResult, ClientError> {
        self.store()?;
        self.check_scope(scope, context).await?;
        let key = scoped_key("files", key)?;
        let fingerprint = fingerprint(&[&serde_json::to_string(request)
            .map_err(|_| ClientError::new(ErrorCode::Internal, "无法创建操作记录。"))?]);
        let journal = self
            .intents
            .as_ref()
            .ok_or_else(|| ClientError::new(ErrorCode::Internal, "操作记录不可用。"))?;
        if let intents::Intent::Completed(result) = journal.begin(scope, &key, &fingerprint)? {
            return Ok(result);
        }
        let result = context
            .run(self.provider.mutate_files(request, scope))
            .await
            .map_err(|mut error| {
                if matches!(
                    error.code,
                    ErrorCode::TransientNetwork | ErrorCode::Cancelled | ErrorCode::IntegrityFailed
                ) {
                    error.code = ErrorCode::Conflict;
                    error.retryable = false;
                    error.message =
                        "云端操作结果尚未确认，请刷新列表核对；此请求不会自动重复提交。".into();
                    error
                        .details
                        .get_or_insert_with(Default::default)
                        .insert("outcome_unknown".into(), true.into());
                }
                error
            });
        journal.finish(scope, &key, &result)?;
        result
    }
    pub async fn verification_status(
        &self,
        id: Option<&str>,
        context: &RequestContext,
    ) -> Result<Option<VerificationState>, ClientError> {
        context.run(self.provider.verification_status(id)).await
    }
    pub async fn verification_url(
        &self,
        id: &str,
        context: &RequestContext,
    ) -> Result<String, ClientError> {
        context.run(self.provider.verification_url(id)).await
    }
    pub async fn verification_complete(
        &self,
        id: &str,
        context: &RequestContext,
    ) -> Result<(), ClientError> {
        context.run(self.provider.verification_complete(id)).await
    }
    pub async fn verification_cancel(
        &self,
        id: &str,
        context: &RequestContext,
    ) -> Result<(), ClientError> {
        context.run(self.provider.verification_cancel(id)).await
    }
    /// Bounded read-only ancestry lookup; never treats missing parents as a root directory.
    pub async fn files_path(
        &self,
        id: &str,
        context: &RequestContext,
    ) -> Result<Vec<RemoteEntry>, ClientError> {
        if id.starts_with("local:") {
            return self.local_path(id);
        }
        let provider = self
            .router
            .select(Operation::Stat, self.provider.id(), context)
            .await?;
        context
            .run(async {
                let mut path = Vec::new();
                let mut next = id.to_owned();
                let mut seen = std::collections::BTreeSet::new();
                while next != "root" {
                    if path.len() >= 128 || !seen.insert(next.clone()) {
                        return Err(ClientError::new(
                            ErrorCode::IntegrityFailed,
                            "The remote ancestry is cyclic or exceeds the safe depth.",
                        ));
                    }
                    let entry = provider.stat(&next).await?;
                    next = entry.parent_id.clone().ok_or_else(|| {
                        ClientError::new(
                            ErrorCode::IntegrityFailed,
                            "The provider did not supply the parent needed to resolve this path.",
                        )
                    })?;
                    path.push(entry);
                }
                path.reverse();
                Ok(path)
            })
            .await
    }
    pub async fn quota_get(&self, context: &RequestContext) -> Result<QuotaReport, ClientError> {
        let provider = self
            .router
            .select(Operation::Quota, self.provider.id(), context)
            .await?;
        context.run(provider.quota()).await
    }
    pub async fn auth_status(&self, context: &RequestContext) -> Result<AuthState, ClientError> {
        context.run(self.provider.auth_status()).await
    }
    pub async fn accounts_list(
        &self,
        context: &RequestContext,
    ) -> Result<Vec<SavedAccount>, ClientError> {
        context.run(self.provider.accounts_list()).await
    }
    fn cancel_media_sessions(&self) -> Result<(), ClientError> {
        let streams = self
            .streams
            .lock()
            .map_err(|_| ClientError::new(ErrorCode::Internal, "媒体会话暂不可用。"))?;
        self.stream_epoch
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        for control in streams.values() {
            control.cancel();
        }
        for control in self
            .media_controls
            .lock()
            .map_err(|_| ClientError::new(ErrorCode::Internal, "媒体队列暂不可用。"))?
            .values()
        {
            control.cancel();
        }
        Ok(())
    }
    pub async fn account_switch(
        &self,
        id: &str,
        context: &RequestContext,
    ) -> Result<AuthState, ClientError> {
        let _mount_guard = self.mount_account_change_guard().await?;
        self.cancel_media_sessions()?;
        context.run(self.provider.account_switch(id)).await
    }
    pub async fn account_remove(
        &self,
        id: &str,
        context: &RequestContext,
    ) -> Result<(), ClientError> {
        let _mount_guard = self.mount_account_change_guard().await?;
        self.cancel_media_sessions()?;
        context.run(self.provider.account_remove(id)).await
    }
    pub fn subscribe_auth(&self) -> Option<broadcast::Receiver<AuthChangedEvent>> {
        self.provider.subscribe_auth()
    }
    pub async fn auth_sign_in(
        &self,
        username: &str,
        password: &str,
        context: &RequestContext,
    ) -> Result<SignInResult, ClientError> {
        let _mount_guard = self.mount_account_change_guard().await?;
        self.cancel_media_sessions()?;
        context.run(self.provider.sign_in(username, password)).await
    }
    pub async fn auth_verify(
        &self,
        challenge_id: &str,
        code: &str,
        password: Option<&str>,
        context: &RequestContext,
    ) -> Result<SignInResult, ClientError> {
        let _mount_guard = self.mount_account_change_guard().await?;
        self.cancel_media_sessions()?;
        context
            .run(self.provider.verify_sign_in(challenge_id, code, password))
            .await
    }
    pub async fn auth_logout(
        &self,
        backend: BackendId,
        context: &RequestContext,
    ) -> Result<(), ClientError> {
        let _mount_guard = self.mount_account_change_guard().await?;
        if backend != self.provider.id() {
            return Err(nekopak_provider::unsupported(backend));
        }
        {
            let streams = self
                .streams
                .lock()
                .map_err(|_| ClientError::new(ErrorCode::Internal, "媒体会话暂不可用。"))?;
            self.stream_epoch
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            for control in streams.values() {
                control.cancel();
            }
        }
        for control in self
            .media_controls
            .lock()
            .map_err(|_| ClientError::new(ErrorCode::Internal, "媒体队列暂不可用。"))?
            .values()
        {
            control.cancel();
        }
        context.run(self.provider.logout()).await
    }
    pub fn transfers_list(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Page<TransferTask>, ClientError> {
        let mut page = self.store()?.list(cursor, page_limit(limit)?)?;
        let mut visible = Vec::new();
        for task in page.items {
            if !self.transfer_history_hidden(&task.id)? {
                visible.push(task);
            }
        }
        page.items = visible;
        Ok(page)
    }
    pub fn task(&self, id: &str) -> Result<TransferTask, ClientError> {
        self.store()?.get(id)
    }
    pub fn set_foreground(&self, foreground: bool) {
        self.foreground
            .store(foreground, std::sync::atomic::Ordering::SeqCst);
    }
    pub fn is_foreground(&self) -> bool {
        self.foreground.load(std::sync::atomic::Ordering::SeqCst)
    }
    /// Android activity suspension never cancels server-side cloud work. Local
    /// verification can finish its atomic commit, but no next step is scheduled.
    pub fn pause_local_transfers(&self) -> Result<(), ClientError> {
        let active = {
            // Serialize the snapshot with new task insertion. A task that
            // finishes preparing after this snapshot observes background state
            // while holding the same gate and starts paused.
            let _gate = self
                .download_gate
                .lock()
                .map_err(|_| ClientError::new(ErrorCode::Internal, "下载队列暂不可用。"))?;
            self.store()?.active()?
        };
        for task in active {
            if task.kind != TaskKind::CloudDownload
                && matches!(
                    task.state,
                    TaskState::Queued | TaskState::Running | TaskState::Retrying
                )
            {
                match self.transfer_control(&task.id, TransferAction::Pause) {
                    Ok(()) => {}
                    Err(error) if error.code == ErrorCode::Conflict => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(())
    }
    pub fn transfer_control(&self, id: &str, action: TransferAction) -> Result<(), ClientError> {
        if let Some(control) = self
            .media_controls
            .lock()
            .map_err(|_| ClientError::new(ErrorCode::Internal, "媒体队列暂不可用。"))?
            .get(id)
        {
            if matches!(action, TransferAction::Pause | TransferAction::Cancel) {
                control.cancel();
            }
        }
        let _guard = self
            .download_gate
            .lock()
            .map_err(|_| ClientError::new(ErrorCode::Internal, "下载队列暂不可用。"))?;
        if matches!(action, TransferAction::Pause | TransferAction::Cancel) {
            if let Some(context) = self
                .download_cancellations
                .lock()
                .map_err(|_| ClientError::new(ErrorCode::Internal, "下载队列暂不可用。"))?
                .get(id)
            {
                context.cancel();
            }
        }
        let task = self.store()?.control(id, action)?;
        self.clear_transfer_retry(id)?;
        self.download_rates.remove(id);
        self.emit(&task);
        Ok(())
    }
    pub async fn transfer_start(
        &self,
        remote_id: &str,
        destination: &str,
        idempotency_key: Option<&str>,
        context: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        if self.provider.id() != BackendId::Mock {
            if remote_id.starts_with("shared:") {
                let scope = self.account_scope(context).await?;
                let id = self
                    .retain_shared_source(remote_id, &scope, idempotency_key, context)
                    .await?;
                return self
                    .start_native_download_at(&id, destination, idempotency_key, &scope, context)
                    .await;
            }
            return self
                .start_native_download(remote_id, destination, idempotency_key, context)
                .await;
        }
        self.ensure_mock()?;
        // The simulation writes no destination files. Reject paths that would be dangerous for the future downloader.
        validate_destination(destination)?;
        let fingerprint = fingerprint(&["download", remote_id, destination]);
        let key = idempotency_key
            .map(|key| scoped_key("download", key))
            .transpose()?;
        if let Some(task) = key
            .as_deref()
            .map(|key| self.store()?.lookup_key(key, &fingerprint))
            .transpose()?
            .flatten()
        {
            return Ok(TaskRef { task_id: task.id });
        }
        let provider = self
            .router
            .select(Operation::Download, self.provider.id(), context)
            .await?;
        let entry = context.run(provider.stat(remote_id)).await?;
        if entry.kind != EntryKind::File {
            return Err(ClientError::new(
                ErrorCode::Unsupported,
                "Folder downloads are not implemented.",
            ));
        }
        let task = new_task(remote_id.into(), TaskKind::Download, Some(entry.size_bytes));
        let task = self
            .store()?
            .insert(task, key.as_deref().map(|key| (key, fingerprint.as_str())))?;
        self.emit(&task);
        Ok(TaskRef { task_id: task.id })
    }
    pub(crate) async fn transfer_start_at(
        &self,
        id: &str,
        destination: &str,
        key: Option<&str>,
        scope: &str,
        ctx: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        self.start_native_download_at(id, destination, key, scope, ctx)
            .await
    }
    pub async fn cloud_task_create(
        &self,
        source_url: &str,
        parent_id: &str,
        idempotency_key: &str,
        context: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        if self.provider.id() != BackendId::Mock {
            return self
                .create_native_cloud(source_url, parent_id, idempotency_key, context)
                .await;
        }
        self.ensure_mock()?;
        validate_source(source_url)?;
        let key = scoped_key("cloud", idempotency_key)?;
        let fingerprint = fingerprint(&["cloud", source_url, parent_id]);
        if let Some(task) = self.store()?.lookup_key(&key, &fingerprint)? {
            return Ok(TaskRef { task_id: task.id });
        }
        let provider = self
            .router
            .select(Operation::CloudTask, self.provider.id(), context)
            .await?;
        let remote_id = context
            .run(provider.create_cloud_task(&CloudTaskRequest {
                source_url: source_url.into(),
                parent_id: parent_id.into(),
                idempotency_key: key.clone(),
            }))
            .await?;
        let task = self.store()?.insert(
            new_task(
                remote_id,
                TaskKind::CloudDownload,
                Some(DecimalU64(100_000_000)),
            ),
            Some((&key, &fingerprint)),
        )?;
        self.emit(&task);
        Ok(TaskRef { task_id: task.id })
    }
    fn ensure_mock(&self) -> Result<(), ClientError> {
        self.store()?;
        if self.provider.id() != BackendId::Mock {
            return Err(ClientError::new(
                ErrorCode::Unsupported,
                "Real transfer execution is not implemented in M0.",
            ));
        }
        Ok(())
    }
    /// Called by the host worker. Never holds the database mutex across provider work.
    pub async fn tick_periodic(&self) -> Result<(), ClientError> {
        if self.provider.id() == BackendId::Mock {
            self.tick().await
        } else if self.is_foreground() {
            self.tick_cloud().await
        } else {
            Ok(())
        }
    }

    pub async fn tick(&self) -> Result<(), ClientError> {
        if !self.is_foreground() {
            return Ok(());
        }
        self.recover_transfer_retries()?;
        if self.provider.id() != BackendId::Mock {
            self.tick_cloud().await?;
            return self.tick_native_download().await;
        }
        for task in self.store()?.active()? {
            if !task.simulated || task.backend != BackendId::Mock {
                continue;
            }
            let result = RequestContext::default()
                .run(self.provider.simulate_step(&task))
                .await;
            let result = match result {
                Ok(step) => {
                    self.store()?
                        .update(&task, step.state, step.transferred_bytes, step.error)
                }
                Err(error) => self.store()?.update(
                    &task,
                    TaskState::Failed,
                    task.transferred_bytes.0,
                    Some(error),
                ),
            };
            match result {
                Ok(task) => self.emit(&task),
                Err(error) if error.code == ErrorCode::Conflict => {} // A concurrent pause/cancel won the revision check.
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

fn new_task(remote_id: String, kind: TaskKind, total_bytes: Option<DecimalU64>) -> TransferTask {
    let timestamp = now();
    TransferTask {
        id: uuid::Uuid::new_v4().to_string(),
        remote_id,
        kind,
        state: TaskState::Queued,
        transferred_bytes: DecimalU64(0),
        received_bytes: None,
        total_bytes,
        speed_bytes_per_sec: None,
        backend: BackendId::Mock,
        error: None,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        revision: DecimalU64(0),
        simulated: true,
        name: None,
        destination: None,
        progress_percent: None,
        quality_label: None,
        clip: None,
    }
}
fn fingerprint(fields: &[&str]) -> String {
    let mut hash = Sha256::new();
    for field in fields {
        hash.update((field.len() as u64).to_le_bytes());
        hash.update(field.as_bytes());
    }
    format!("{:x}", hash.finalize())
}
fn scoped_key(scope: &str, key: &str) -> Result<String, ClientError> {
    if key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
    {
        return Err(ClientError::new(
            ErrorCode::Conflict,
            "Idempotency key must be 1–128 ASCII letters, digits, dash, dot, colon or underscore.",
        ));
    }
    Ok(format!("{scope}:{key}"))
}
pub fn validate_source(source: &str) -> Result<(), ClientError> {
    let invalid = || {
        ClientError::new(
            ErrorCode::Conflict,
            "Enter a valid HTTP(S) URL without credentials or a magnet URI with an info hash.",
        )
    };
    if source.len() > 8192 || source.chars().any(char::is_control) {
        return Err(invalid());
    }
    let url = url::Url::parse(source).map_err(|_| invalid())?;
    match url.scheme() {
        "http" | "https"
            if url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none() =>
        {
            Ok(())
        }
        "magnet"
            if url.query_pairs().any(|(key, value)| {
                key == "xt"
                    && value.strip_prefix("urn:btih:").is_some_and(|hash| {
                        (hash.len() == 40 && hash.bytes().all(|c| c.is_ascii_hexdigit()))
                            || (hash.len() == 32
                                && hash
                                    .bytes()
                                    .all(|c| c.is_ascii_uppercase() || (b'2'..=b'7').contains(&c)))
                    })
            }) =>
        {
            Ok(())
        }
        _ => Err(invalid()),
    }
}
pub fn validate_destination(destination: &str) -> Result<(), ClientError> {
    let invalid = || {
        ClientError::new(ErrorCode::Conflict, "Use an absolute local path without traversal, reserved device names or special Windows path prefixes.")
    };
    if destination.is_empty()
        || destination.len() > 32_000
        || destination.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    let path = destination.replace('\\', "/");
    if path.starts_with("//") {
        return Err(invalid());
    }
    let drive = path.as_bytes().get(1) == Some(&b':')
        && path.as_bytes()[0].is_ascii_alphabetic()
        && path.as_bytes().get(2) == Some(&b'/');
    if !drive && !path.starts_with('/') {
        return Err(invalid());
    }
    let relative = if drive { &path[3..] } else { &path[1..] };
    for component in relative.split('/') {
        let stem = component
            .split('.')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'));
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with([' ', '.'])
            || component.contains([':', '*', '?', '"', '<', '>', '|'])
            || reserved
        {
            return Err(invalid());
        }
    }
    Ok(())
}
