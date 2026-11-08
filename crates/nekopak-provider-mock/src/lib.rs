//! Offline deterministic simulator. No HTTP client, credentials, filesystem writes, or external tools.
use async_trait::async_trait;
use nekopak_domain::*;
use nekopak_provider::{CloudTaskRequest, ProviderPort, SimulationStep};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, VecDeque},
    sync::Mutex,
    time::Duration,
};

pub const FIXTURE_TIME: &str = "2026-11-07T00:00:00Z";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    #[default]
    Free,
    Premium,
    Unauthorized,
    QuotaExhausted,
    Offline,
    LargeDirectory,
    NameConflict,
    DisconnectAt37,
    IntegrityFailure,
    DiskFull,
}
impl Scenario {
    pub const ALL: [Self; 10] = [
        Self::Free,
        Self::Premium,
        Self::Unauthorized,
        Self::QuotaExhausted,
        Self::Offline,
        Self::LargeDirectory,
        Self::NameConflict,
        Self::DisconnectAt37,
        Self::IntegrityFailure,
        Self::DiskFull,
    ];
}

#[derive(Debug, Clone)]
pub enum Fault {
    Http(u16),
    Timeout,
    Quota,
    AccountRestricted,
    DiskFull,
    Integrity,
}

pub struct MockProvider {
    scenario: Scenario,
    data: Mutex<MockData>,
}
struct MockData {
    entries: Vec<RemoteEntry>,
    generation: u64,
    faults: VecDeque<(Operation, Fault)>,
    disconnected: BTreeSet<String>,
}

impl MockProvider {
    pub fn new(scenario: Scenario) -> Self {
        Self {
            scenario,
            data: Mutex::new(MockData {
                entries: entries(scenario),
                generation: 1,
                faults: VecDeque::new(),
                disconnected: BTreeSet::new(),
            }),
        }
    }
    pub fn inject(&self, operation: Operation, fault: Fault) {
        self.data
            .lock()
            .expect("mock lock")
            .faults
            .push_back((operation, fault));
    }
    pub fn change_file(&self, id: &str, name: &str, size: u64) -> Result<(), ClientError> {
        let mut data = self.data.lock().map_err(|_| internal())?;
        data.generation += 1;
        let generation = data.generation;
        let entry = data
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(not_found)?;
        entry.name = name.into();
        entry.size_bytes = DecimalU64(size);
        entry.etag = Some(format!("mock-v{generation}"));
        Ok(())
    }
    async fn check(&self, operation: Operation) -> Result<(), ClientError> {
        match self.scenario {
            Scenario::Unauthorized => return Err(ClientError::from_http(401, BackendId::Mock)),
            Scenario::Offline => {
                return Err(ClientError::new(
                    ErrorCode::TransientNetwork,
                    "Mock network is offline.",
                )
                .for_backend(BackendId::Mock))
            }
            Scenario::QuotaExhausted
                if matches!(
                    operation,
                    Operation::Download | Operation::CloudTask | Operation::Upload
                ) =>
            {
                return Err(quota_error())
            }
            _ => {}
        }
        let fault = {
            let mut data = self.data.lock().map_err(|_| internal())?;
            let index = data.faults.iter().position(|(op, _)| *op == operation);
            index
                .and_then(|index| data.faults.remove(index))
                .map(|(_, fault)| fault)
        };
        match fault {
            Some(Fault::Http(status)) => Err(ClientError::from_http(status, BackendId::Mock)),
            Some(Fault::Timeout) => {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok(())
            }
            Some(Fault::Quota) => Err(quota_error()),
            Some(Fault::AccountRestricted) => Err(ClientError::new(
                ErrorCode::AccountRestricted,
                "Mock account is restricted.",
            )),
            Some(Fault::DiskFull) => {
                Err(ClientError::new(ErrorCode::DiskFull, "Mock disk is full."))
            }
            Some(Fault::Integrity) => Err(ClientError::new(
                ErrorCode::IntegrityFailed,
                "Mock integrity verification failed.",
            )),
            None => Ok(()),
        }
    }
}

#[async_trait]
impl ProviderPort for MockProvider {
    fn id(&self) -> BackendId {
        BackendId::Mock
    }
    async fn auth_status(&self) -> Result<AuthState, ClientError> {
        Ok(AuthState {
            backend: BackendId::Mock,
            authenticated: false,
            account_id: None,
            credential_owner: "none".into(),
            account_tier: AccountTier::Unknown,
            message: "Mock mode uses no account or credentials.".into(),
            state: AuthStatus::SignedOut,
            display_name: None,
        })
    }
    async fn discover(&self) -> Result<CapabilityReport, ClientError> {
        Ok(CapabilityReport {
            backend: BackendId::Mock,
            status: if self.scenario == Scenario::Unauthorized { BackendStatus::NeedsLogin } else { BackendStatus::Ready },
            operations: [Operation::List, Operation::Stat, Operation::Download, Operation::CloudTask, Operation::Quota].into_iter().map(|op| (op, true)).chain([(Operation::ReadRange, false), (Operation::SegmentDownload, false), (Operation::Upload, false), (Operation::Trash, false)]).collect(),
            account_tier: Some(if self.scenario == Scenario::Premium { AccountTier::Premium } else { AccountTier::Free }),
            version: Some("mock-0.1".into()),
            limitations: vec!["OFFLINE SIMULATION: all entries, quotas and transfers are synthetic; no file is downloaded.".into(), "Real providers, random reads, sync and mounting are not implemented.".into()],
            checked_at: FIXTURE_TIME.into(),
        })
    }
    async fn list(
        &self,
        parent: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<RemoteEntry>, ClientError> {
        self.check(Operation::List).await?;
        if !(1..=500).contains(&limit) {
            return Err(conflict());
        }
        let data = self.data.lock().map_err(|_| internal())?;
        if parent != "root"
            && !data
                .entries
                .iter()
                .any(|entry| entry.id == parent && entry.kind == EntryKind::Folder)
        {
            return Err(not_found());
        }
        let offset = if let Some(cursor) = cursor {
            let parts: Vec<_> = cursor.split(':').collect();
            if parts.len() != 4
                || parts[0] != "m1"
                || parts[1] != data.generation.to_string()
                || parts[2] != parent
            {
                return Err(conflict());
            }
            parts[3].parse::<usize>().map_err(|_| conflict())?
        } else {
            0
        };
        let matching: Vec<_> = data
            .entries
            .iter()
            .filter(|entry| entry.parent_id.as_deref() == Some(parent))
            .collect();
        if offset > matching.len() {
            return Err(conflict());
        }
        let items: Vec<_> = matching
            .iter()
            .skip(offset)
            .take(limit)
            .map(|entry| (*entry).clone())
            .collect();
        let end = offset + items.len();
        Ok(Page {
            items,
            next_cursor: (end < matching.len())
                .then(|| format!("m1:{}:{parent}:{end}", data.generation)),
        })
    }
    async fn stat(&self, id: &str) -> Result<RemoteEntry, ClientError> {
        self.check(Operation::Stat).await?;
        self.data
            .lock()
            .map_err(|_| internal())?
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
            .ok_or_else(not_found)
    }
    async fn quota(&self) -> Result<QuotaReport, ClientError> {
        self.check(Operation::Quota).await?;
        let exhausted = self.scenario == Scenario::QuotaExhausted;
        Ok(QuotaReport {
            backend: BackendId::Mock,
            account_tier: if self.scenario == Scenario::Premium { AccountTier::Premium } else { AccountTier::Free },
            source: "mock".into(),
            dimensions: ["download", "upload", "cloud_download"].into_iter().map(|dimension| QuotaDimension {
                used_bytes: None,
                dimension: dimension.into(), period: "synthetic_month".into(), limit_bytes: Some(DecimalU64(100_000_000_000)), remaining_bytes: Some(DecimalU64(if exhausted { 0 } else { 42_000_000_000 })), resets_at: None,
            }).collect(),
            checked_at: FIXTURE_TIME.into(),
            limitations: vec!["Synthetic fixture values; not live account quota or a statement of service limits.".into()],
        })
    }
    async fn create_cloud_task(&self, request: &CloudTaskRequest) -> Result<String, ClientError> {
        self.check(Operation::CloudTask).await?;
        if request.parent_id != "root" {
            let folder = self.stat(&request.parent_id).await?;
            if folder.kind != EntryKind::Folder {
                return Err(conflict());
            }
        }
        // Stable fake remote ID makes retries side-effect free. Never retain the URL or its query.
        Ok(format!(
            "mock-cloud-{:x}",
            Sha256::digest(request.idempotency_key.as_bytes())
        ))
    }
    async fn simulate_step(&self, task: &TransferTask) -> Result<SimulationStep, ClientError> {
        self.check(Operation::Download).await?;
        let total = task.total_bytes.unwrap_or(DecimalU64(100_000_000)).0;
        let current = task.transferred_bytes.0;
        let mut next = SimulationStep {
            state: task.state,
            transferred_bytes: current,
            error: None,
        };
        match task.state {
            TaskState::Queued | TaskState::Retrying => next.state = TaskState::Running,
            TaskState::Running => {
                if self.scenario == Scenario::DiskFull {
                    return Err(ClientError::new(
                        ErrorCode::DiskFull,
                        "Simulated disk exhaustion; no files were written.",
                    ));
                }
                let mut data = self.data.lock().map_err(|_| internal())?;
                let breakpoint = ((total as u128 * 37) / 100) as u64;
                let increment = (total / 10).max(1);
                let proposed = current.saturating_add(increment).min(total);
                if self.scenario == Scenario::DisconnectAt37
                    && current < breakpoint
                    && proposed >= breakpoint
                    && data.disconnected.insert(task.id.clone())
                {
                    next.transferred_bytes = breakpoint;
                    next.state = TaskState::Failed;
                    next.error = Some(ClientError::new(
                        ErrorCode::TransientNetwork,
                        "Simulated connection loss at 37%; explicit retry is available.",
                    ));
                } else {
                    next.transferred_bytes = proposed;
                    if proposed == total {
                        next.state = TaskState::Verifying;
                    }
                }
            }
            TaskState::Verifying => {
                if self.scenario == Scenario::IntegrityFailure {
                    return Err(ClientError::new(
                        ErrorCode::IntegrityFailed,
                        "Simulated integrity check failed.",
                    ));
                }
                next.state = TaskState::Completed;
            }
            _ => {}
        }
        Ok(next)
    }
}

fn internal() -> ClientError {
    ClientError::new(ErrorCode::Internal, "Mock state is unavailable.")
}
fn conflict() -> ClientError {
    ClientError::new(
        ErrorCode::Conflict,
        "Invalid or stale page/entry; refresh from the first page.",
    )
}
fn not_found() -> ClientError {
    ClientError::new(ErrorCode::NotFound, "Mock entry was not found.").for_backend(BackendId::Mock)
}
fn quota_error() -> ClientError {
    ClientError::new(ErrorCode::QuotaExceeded, "Mock quota is exhausted.")
        .for_backend(BackendId::Mock)
}

pub fn entries(scenario: Scenario) -> Vec<RemoteEntry> {
    let names = [
        "资料 📁",
        "示例 video.mp4",
        "CON",
        "文件 with spaces.txt",
        "e\u{301}.txt",
        "超长名称",
        "零字节.txt",
        "大文件.bin",
    ];
    let mut result: Vec<_> = names
        .into_iter()
        .enumerate()
        .map(|(i, name)| RemoteEntry {
            id: if i == 0 {
                "documents".into()
            } else {
                format!("file-{i}")
            },
            parent_id: Some("root".into()),
            name: if i == 5 {
                format!("{}.txt", name.repeat(70))
            } else {
                name.into()
            },
            kind: if i == 0 {
                EntryKind::Folder
            } else {
                EntryKind::File
            },
            size_bytes: DecimalU64(match i {
                0 | 6 => 0,
                7 => u64::MAX,
                _ => 100_000_000,
            }),
            modified_at: Some(FIXTURE_TIME.into()),
            etag: Some("mock-v1".into()),
            hash: None,
            mime_type: None,
            starred: None,
        })
        .collect();
    let mut child = result[1].clone();
    child.id = "nested-file".into();
    child.parent_id = Some("documents".into());
    child.name = "说明.txt".into();
    result.push(child);
    if scenario == Scenario::NameConflict {
        let mut duplicate = result[1].clone();
        duplicate.id = "conflict-file".into();
        result.push(duplicate);
    }
    if scenario == Scenario::LargeDirectory {
        for i in 8..10_000 {
            let mut entry = result[1].clone();
            entry.id = format!("large-{i:05}");
            entry.name = format!("条目-{i:05}.txt");
            result.push(entry);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use nekopak_provider::{ProviderRouter, RequestContext};
    use std::sync::Arc;
    #[tokio::test]
    async fn large_directory_is_complete_without_duplicates() {
        let provider = MockProvider::new(Scenario::LargeDirectory);
        let mut ids = BTreeSet::new();
        let mut cursor = None;
        loop {
            let page = provider.list("root", cursor.as_deref(), 137).await.unwrap();
            for entry in page.items {
                assert!(ids.insert(entry.id));
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(ids.len(), 10_000);
    }
    #[tokio::test]
    async fn changed_files_invalidate_pagination() {
        let provider = MockProvider::new(Scenario::Free);
        let page = provider.list("root", None, 2).await.unwrap();
        provider.change_file("file-1", "renamed", 999).unwrap();
        assert_eq!(
            provider
                .list("root", page.next_cursor.as_deref(), 2)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(provider.stat("file-1").await.unwrap().name, "renamed");
    }
    #[tokio::test]
    async fn injected_errors_and_timeout_are_deterministic() {
        let provider = MockProvider::new(Scenario::Free);
        for (status, code) in [
            (401, ErrorCode::AuthExpired),
            (403, ErrorCode::PermissionDenied),
            (404, ErrorCode::NotFound),
            (429, ErrorCode::TransientNetwork),
        ] {
            provider.inject(Operation::List, Fault::Http(status));
            assert_eq!(
                provider.list("root", None, 10).await.unwrap_err().code,
                code
            );
        }
        provider.inject(Operation::List, Fault::Timeout);
        let context = RequestContext::new(Duration::from_millis(5));
        assert_eq!(
            context
                .run(provider.list("root", None, 10))
                .await
                .unwrap_err()
                .code,
            ErrorCode::TransientNetwork
        );
        assert!(provider.list("root", None, 10).await.is_ok());
    }
    #[tokio::test]
    async fn router_uses_capabilities_and_does_not_fallback_after_dispatch() {
        let provider = Arc::new(MockProvider::new(Scenario::Free));
        let router = ProviderRouter::new(vec![provider.clone()]);
        let context = RequestContext::default();
        assert!(router
            .select(Operation::ReadRange, BackendId::Mock, &context)
            .await
            .is_err());
        provider.inject(Operation::List, Fault::Http(403));
        let selected = router
            .select(Operation::List, BackendId::Mock, &context)
            .await
            .unwrap();
        assert_eq!(
            selected.list("root", None, 5).await.unwrap_err().code,
            ErrorCode::PermissionDenied
        );
    }
}
