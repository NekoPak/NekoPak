pub mod store;
use nekopak_domain::*;
use nekopak_provider::{CloudTaskRequest, ProviderPort, ProviderRouter, RequestContext};
use nekopak_provider_mock::{MockProvider, Scenario};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, sync::Arc};
use store::Store;
use tokio::sync::broadcast;

pub fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("UTC time is RFC3339")
}

pub struct AppService {
    store: Option<Store>,
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
        let (store, storage_error) = match Store::open(path) {
            Ok(store) => (Some(store), None),
            Err(error) => (None, Some(error)),
        };
        let (events, _) = broadcast::channel(256);
        Self {
            store,
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
            version: env!("CARGO_PKG_VERSION").into(),
            contract_version: CONTRACT_VERSION.into(),
            mode: if self.provider.id() == BackendId::Mock {
                "mock"
            } else {
                "official_cli_preview"
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
                ("real_transfers".into(), false),
                ("segment_download".into(), false),
                ("sync".into(), false),
                ("mount".into(), false),
                ("experimental_network".into(), false),
                ("telemetry".into(), false),
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
        let provider = self
            .router
            .select(Operation::Stat, self.provider.id(), context)
            .await?;
        context.run(provider.stat(id)).await
    }
    /// Bounded read-only ancestry lookup; never treats missing parents as a root directory.
    pub async fn files_path(
        &self,
        id: &str,
        context: &RequestContext,
    ) -> Result<Vec<RemoteEntry>, ClientError> {
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
    pub fn transfers_list(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Page<TransferTask>, ClientError> {
        self.store()?.list(cursor, page_limit(limit)?)
    }
    pub fn task(&self, id: &str) -> Result<TransferTask, ClientError> {
        self.store()?.get(id)
    }
    pub fn transfer_control(&self, id: &str, action: TransferAction) -> Result<(), ClientError> {
        let task = self.store()?.control(id, action)?;
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
    pub async fn cloud_task_create(
        &self,
        source_url: &str,
        parent_id: &str,
        idempotency_key: &str,
        context: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
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
    pub async fn tick(&self) -> Result<(), ClientError> {
        if self.provider.id() != BackendId::Mock {
            return Ok(());
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
        total_bytes,
        speed_bytes_per_sec: None,
        backend: BackendId::Mock,
        error: None,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        revision: DecimalU64(0),
        simulated: true,
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
