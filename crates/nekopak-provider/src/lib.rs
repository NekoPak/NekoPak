use async_trait::async_trait;
use nekopak_domain::*;
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::{broadcast, watch};

#[derive(Clone)]
pub struct RequestContext {
    cancelled: watch::Sender<bool>,
    pub timeout: Duration,
}
impl Default for RequestContext {
    fn default() -> Self {
        Self::new(Duration::from_secs(15))
    }
}
impl RequestContext {
    pub fn new(timeout: Duration) -> Self {
        let (cancelled, _) = watch::channel(false);
        Self { cancelled, timeout }
    }
    pub fn cancel(&self) {
        self.cancelled.send_replace(true);
    }
    pub async fn run<T>(
        &self,
        work: impl Future<Output = Result<T, ClientError>>,
    ) -> Result<T, ClientError> {
        let mut receiver = self.cancelled.subscribe();
        tokio::select! {
            biased;
            _ = receiver.wait_for(|cancelled| *cancelled) => Err(ClientError::new(ErrorCode::Cancelled, "The request was cancelled.")),
            result = tokio::time::timeout(self.timeout, work) => result.unwrap_or_else(|_| Err(ClientError::new(ErrorCode::TransientNetwork, "The provider request timed out."))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CloudTaskRequest {
    pub source_url: String,
    pub parent_id: String,
    pub idempotency_key: String,
}

pub struct SimulationStep {
    pub state: TaskState,
    pub transferred_bytes: u64,
    pub error: Option<ClientError>,
}

pub struct DownloadInfo {
    pub entry: RemoteEntry,
    pub identity: String,
    pub account_scope: String,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Representation {
    pub id: String,
    pub label: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size_bytes: Option<DecimalU64>,
    pub container: String,
    pub readable: bool,
    pub unreadable_reason: Option<String>,
}
pub struct DownloadChunk {
    pub bytes: Vec<u8>,
    pub total: u64,
}

#[derive(Clone)]
pub struct DownloadObserver(Arc<dyn Fn(u64) + Send + Sync>);
impl DownloadObserver {
    pub fn new(callback: impl Fn(u64) + Send + Sync + 'static) -> Self {
        Self(Arc::new(callback))
    }
    pub fn received(&self, bytes: u64) {
        (self.0)(bytes);
    }
}
impl Default for DownloadObserver {
    fn default() -> Self {
        Self::new(|_| {})
    }
}

#[async_trait]
pub trait ProviderPort: Send + Sync {
    fn id(&self) -> BackendId;
    fn configure_server_domain(&self, _domain: &str) -> Result<(), ClientError> {
        Ok(())
    }
    fn network_reconnect(&self) -> Result<(), ClientError> {
        Ok(())
    }
    async fn network_domains_probe(&self) -> Result<serde_json::Value, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn auth_preferences_get(&self) -> Result<serde_json::Value, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn auth_preferences_update(
        &self,
        _value: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        Err(unsupported(self.id()))
    }
    fn supports_password_auth(&self) -> bool {
        false
    }
    fn subscribe_auth(&self) -> Option<broadcast::Receiver<AuthChangedEvent>> {
        None
    }
    async fn sign_in(&self, _username: &str, _password: &str) -> Result<SignInResult, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn verify_sign_in(
        &self,
        _challenge_id: &str,
        _code: &str,
        _password: Option<&str>,
    ) -> Result<SignInResult, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn logout(&self) -> Result<(), ClientError> {
        Err(unsupported(self.id()))
    }
    async fn verification_status(
        &self,
        _id: Option<&str>,
    ) -> Result<Option<VerificationState>, ClientError> {
        Ok(None)
    }
    async fn verification_url(&self, _id: &str) -> Result<String, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn verification_complete(&self, _id: &str) -> Result<(), ClientError> {
        Err(unsupported(self.id()))
    }
    async fn verification_cancel(&self, _id: &str) -> Result<(), ClientError> {
        Err(unsupported(self.id()))
    }
    /// Stable local digest of the authenticated account, never an access token.
    async fn account_scope(&self) -> Result<String, ClientError> {
        Err(unsupported(self.id()))
    }
    /// Validate the caller's account identity before returning its local storage scope.
    async fn account_scope_for(&self, _id: &str) -> Result<String, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn accounts_list(&self) -> Result<Vec<SavedAccount>, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn account_switch(&self, _id: &str) -> Result<AuthState, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn account_remove(&self, _id: &str) -> Result<(), ClientError> {
        Err(unsupported(self.id()))
    }
    async fn query_files(
        &self,
        _query: &FileQuery,
        _cursor: Option<&str>,
        _limit: usize,
    ) -> Result<Page<RemoteEntry>, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn mutate_files(
        &self,
        _request: &FileMutation,
        _account_scope: &str,
    ) -> Result<OperationResult, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn list_shares(
        &self,
        _cursor: Option<&str>,
        _limit: usize,
    ) -> Result<Page<ShareInfo>, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn download_info(&self, _id: &str) -> Result<DownloadInfo, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn share_source_info(
        &self,
        _access_ref: &str,
        _id: &str,
    ) -> Result<DownloadInfo, ClientError> {
        Err(unsupported(self.id()))
    }
    /// Persist only protected share authorization; returns an opaque local source reference.
    async fn share_retain(&self, _access_ref: &str) -> Result<String, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn share_release(&self, _access_ref: &str) -> Result<(), ClientError> {
        Err(unsupported(self.id()))
    }
    #[allow(clippy::too_many_arguments)]
    async fn share_chunk(
        &self,
        _access_ref: &str,
        _id: &str,
        _identity: &str,
        _scope: &str,
        _start: u64,
        _length: u64,
        _observer: DownloadObserver,
    ) -> Result<DownloadChunk, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn media_options(&self, _id: &str) -> Result<Vec<Representation>, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn media_info(&self, id: &str, quality: &str) -> Result<DownloadInfo, ClientError> {
        if quality == "original" {
            self.download_info(id).await
        } else {
            Err(unsupported(self.id()))
        }
    }
    #[allow(clippy::too_many_arguments)]
    async fn media_chunk(
        &self,
        id: &str,
        quality: &str,
        identity: &str,
        scope: &str,
        start: u64,
        length: u64,
        observer: DownloadObserver,
    ) -> Result<DownloadChunk, ClientError> {
        if quality == "original" {
            self.download_chunk(id, identity, scope, start, length, observer)
                .await
        } else {
            Err(unsupported(self.id()))
        }
    }
    async fn download_chunk(
        &self,
        _id: &str,
        _identity: &str,
        _scope: &str,
        _start: u64,
        _length: u64,
        _observer: DownloadObserver,
    ) -> Result<DownloadChunk, ClientError> {
        Err(unsupported(self.id()))
    }
    fn supports_downloads(&self) -> bool {
        false
    }
    fn configure_network(
        &self,
        _proxy_mode: &str,
        _proxy_url: Option<&str>,
        _compatibility: bool,
    ) -> Result<(), ClientError> {
        Ok(())
    }
    fn set_domain_blocking(&self, _enabled: bool) -> Result<(), ClientError> {
        Ok(())
    }
    async fn discover(&self) -> Result<CapabilityReport, ClientError>;
    async fn list(
        &self,
        parent: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<RemoteEntry>, ClientError>;
    async fn stat(&self, id: &str) -> Result<RemoteEntry, ClientError>;
    async fn recovery_metadata(
        &self,
        _id: &str,
        _scope: &str,
    ) -> Result<RecoveryMetadata, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn create_from_content(
        &self,
        _entry: &RemoteEntry,
        _scope: &str,
    ) -> Result<ContentCreateReceipt, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn quota(&self) -> Result<QuotaReport, ClientError>;
    async fn auth_status(&self) -> Result<AuthState, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn create_cloud_task(&self, _request: &CloudTaskRequest) -> Result<String, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn cloud_create(
        &self,
        _source: &str,
        _parent: &str,
        _scope: &str,
    ) -> Result<CloudTaskSnapshot, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn cloud_task(&self, _id: &str, _scope: &str) -> Result<CloudTaskSnapshot, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn cloud_control(
        &self,
        _id: &str,
        _action: TransferAction,
        _scope: &str,
    ) -> Result<(), ClientError> {
        Err(unsupported(self.id()))
    }
    async fn cloud_preview(&self, _source: &str) -> Result<CloudResourcePreview, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn share_create(
        &self,
        _request: &CreateShareRequest,
        _scope: &str,
    ) -> Result<ShareInfo, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn shares_cancel(
        &self,
        _ids: &[String],
        _scope: &str,
    ) -> Result<BulkResult, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn upload_prepare(
        &self,
        _path: &str,
        _parent: &str,
        _scope: &str,
    ) -> Result<UploadPrepared, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn upload_open(
        &self,
        _reference: &str,
        _scope: &str,
    ) -> Result<UploadPrepared, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn upload_part(
        &self,
        _reference: &str,
        _part: u32,
        _bytes: Vec<u8>,
        _scope: &str,
    ) -> Result<String, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn upload_finish(
        &self,
        _reference: &str,
        _etags: &[String],
        _scope: &str,
    ) -> Result<RemoteEntry, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn upload_forget(&self, _reference: &str, _scope: &str) -> Result<(), ClientError> {
        Err(unsupported(self.id()))
    }
    async fn archive_list(
        &self,
        _id: &str,
        _path: &str,
        _password: Option<&str>,
    ) -> Result<ArchiveListing, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn archive_unlock(&self, _id: &str, _password: &str) -> Result<String, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn archive_extract(
        &self,
        _id: &str,
        _paths: &[String],
        _parent: &str,
        _password: Option<&str>,
        _scope: &str,
    ) -> Result<ArchiveJob, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn archive_job(&self, _id: &str, _scope: &str) -> Result<ArchiveJob, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn share_open(&self, _id: &str, _code: &str) -> Result<SharedFolder, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn share_folder(
        &self,
        _reference: &str,
        _parent: &str,
        _cursor: Option<&str>,
        _limit: usize,
    ) -> Result<SharedFolder, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn share_restore(
        &self,
        _reference: &str,
        _ids: &[String],
        _parent: &str,
        _scope: &str,
    ) -> Result<BulkResult, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn simulate_step(&self, _task: &TransferTask) -> Result<SimulationStep, ClientError> {
        Err(unsupported(self.id()))
    }
    async fn read_range(&self, _id: &str, _start: u64, _end: u64) -> Result<Vec<u8>, ClientError> {
        Err(unsupported(self.id()))
    }
}

pub fn unsupported(backend: BackendId) -> ClientError {
    ClientError::new(
        ErrorCode::Unsupported,
        "This provider does not support the requested operation.",
    )
    .for_backend(backend)
}

pub struct ProviderRouter {
    providers: Vec<Arc<dyn ProviderPort>>,
}
impl ProviderRouter {
    pub fn new(providers: Vec<Arc<dyn ProviderPort>>) -> Self {
        Self { providers }
    }
    pub async fn discover(
        &self,
        context: &RequestContext,
    ) -> Result<Vec<CapabilityReport>, ClientError> {
        let mut reports = Vec::new();
        for provider in &self.providers {
            reports.push(context.run(provider.discover()).await?);
        }
        Ok(reports)
    }
    /// Selection happens BEFORE dispatch. Errors from an invoked provider are never replayed elsewhere.
    pub async fn select(
        &self,
        operation: Operation,
        selected: BackendId,
        context: &RequestContext,
    ) -> Result<Arc<dyn ProviderPort>, ClientError> {
        let provider = self
            .providers
            .iter()
            .find(|provider| provider.id() == selected)
            .ok_or_else(|| unsupported(selected))?;
        let report = context.run(provider.discover()).await?;
        match report.status {
            BackendStatus::NeedsLogin => {
                return Err(ClientError::new(
                    ErrorCode::AuthExpired,
                    "Authorize this backend before continuing.",
                )
                .for_backend(selected))
            }
            BackendStatus::Ready => {}
            _ => return Err(unsupported(selected)),
        }
        if !report.supports(operation) {
            return Err(unsupported(selected));
        }
        Ok(provider.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancellation_and_timeout_are_bounded() {
        let context = RequestContext::new(Duration::from_millis(5));
        let error = context
            .run(std::future::pending::<Result<(), ClientError>>())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::TransientNetwork);
        context.cancel();
        let error = context.run(async { Ok(()) }).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Cancelled);
    }
}
