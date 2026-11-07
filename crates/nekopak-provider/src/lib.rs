use async_trait::async_trait;
use nekopak_domain::*;
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::watch;

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

#[async_trait]
pub trait ProviderPort: Send + Sync {
    fn id(&self) -> BackendId;
    async fn discover(&self) -> Result<CapabilityReport, ClientError>;
    async fn list(
        &self,
        parent: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<RemoteEntry>, ClientError>;
    async fn stat(&self, id: &str) -> Result<RemoteEntry, ClientError>;
    async fn quota(&self) -> Result<QuotaReport, ClientError>;
    async fn create_cloud_task(&self, _request: &CloudTaskRequest) -> Result<String, ClientError> {
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
