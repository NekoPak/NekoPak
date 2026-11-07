//! Transport-independent contract v0.1. All byte counts and revisions are decimal strings.
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::{collections::BTreeMap, fmt};

pub const CONTRACT_VERSION: &str = "0.1";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct DecimalU64(pub u64);

impl Serialize for DecimalU64 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for DecimalU64 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || !value.bytes().all(|byte| byte.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return Err(de::Error::custom("expected canonical decimal u64 string"));
        }
        value.parse().map(Self).map_err(de::Error::custom)
    }
}

macro_rules! wire_enum {
    ($name:ident, $case:literal, [$($variant:ident),+ $(,)?]) => {
        #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
        #[serde(rename_all = $case)]
        pub enum $name { $($variant),+ }
    };
}

wire_enum!(
    BackendId,
    "snake_case",
    [OfficialCli, RustCompat, Webdav, Rclone, Mock]
);
wire_enum!(
    Operation,
    "snake_case",
    [
        List,
        Stat,
        Search,
        Mkdir,
        Rename,
        Move,
        Copy,
        Trash,
        Upload,
        Download,
        CloudTask,
        StreamUrl,
        ReadRange,
        Quota,
        SegmentDownload
    ]
);
wire_enum!(
    BackendStatus,
    "snake_case",
    [Ready, NeedsLogin, NotInstalled, Unavailable]
);
wire_enum!(AccountTier, "snake_case", [Free, Premium, Unknown]);
wire_enum!(EntryKind, "snake_case", [File, Folder]);
wire_enum!(HashAlgorithm, "lowercase", [Md5, Sha1, Sha256]);
wire_enum!(
    TaskKind,
    "snake_case",
    [Download, Upload, CloudDownload, SegmentDownload]
);
wire_enum!(
    TaskState,
    "snake_case",
    [Queued, Running, Paused, Retrying, Verifying, Completed, Failed, Cancelled]
);
wire_enum!(TransferAction, "snake_case", [Pause, Resume, Cancel, Retry]);
wire_enum!(
    ErrorCode,
    "SCREAMING_SNAKE_CASE",
    [
        AuthExpired,
        AccountRestricted,
        PermissionDenied,
        QuotaExceeded,
        NotFound,
        Unsupported,
        TransientNetwork,
        DiskFull,
        IntegrityFailed,
        Conflict,
        Cancelled,
        Internal
    ]
);
wire_enum!(
    SyncMode,
    "snake_case",
    [UploadOnly, DownloadOnly, Bidirectional]
);
wire_enum!(
    SyncOp,
    "snake_case",
    [Upload, Download, Mkdir, KeepBoth, Delete, Skip]
);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<BackendId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<BTreeMap<String, serde_json::Value>>,
}

impl ClientError {
    /// Messages are static, reviewed text. Never pass upstream error bodies, URLs or secrets.
    pub fn new(code: ErrorCode, message: &'static str) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: code == ErrorCode::TransientNetwork,
            backend: None,
            request_id: None,
            details: None,
        }
    }

    pub fn for_backend(mut self, backend: BackendId) -> Self {
        self.backend = Some(backend);
        self
    }

    /// Deliberately accepts no response body: private upstream diagnostics stay out of IPC.
    pub fn from_http(status: u16, backend: BackendId) -> Self {
        let (code, message) = match status {
            401 => (
                ErrorCode::AuthExpired,
                "Authorization expired; authorize again.",
            ),
            403 => (
                ErrorCode::PermissionDenied,
                "The account lacks permission for this operation.",
            ),
            404 => (ErrorCode::NotFound, "The requested entry no longer exists."),
            408 | 429 | 500..=599 => (
                ErrorCode::TransientNetwork,
                "The service is temporarily unavailable or rate limited.",
            ),
            _ => (
                ErrorCode::Internal,
                "The provider returned an unexpected response.",
            ),
        };
        Self::new(code, message).for_backend(backend)
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ClientError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityReport {
    pub backend: BackendId,
    pub status: BackendStatus,
    pub operations: BTreeMap<Operation, bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_tier: Option<AccountTier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub limitations: Vec<String>,
    pub checked_at: String,
}
impl CapabilityReport {
    pub fn supports(&self, operation: Operation) -> bool {
        self.status == BackendStatus::Ready && self.operations.get(&operation) == Some(&true)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentHash {
    pub algorithm: HashAlgorithm,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub name: String,
    pub kind: EntryKind,
    pub size_bytes: DecimalU64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<ContentHash>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransferTask {
    pub id: String,
    pub remote_id: String,
    pub kind: TaskKind,
    pub state: TaskState,
    pub transferred_bytes: DecimalU64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<DecimalU64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_bytes_per_sec: Option<f64>,
    pub backend: BackendId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ClientError>,
    pub created_at: String,
    pub updated_at: String,
    /// Monotonic, global persistence revision for snapshot/event reconciliation.
    pub revision: DecimalU64,
    pub simulated: bool,
}

impl TaskState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
    pub fn can_transition_to(self, next: Self) -> bool {
        use TaskState::*;
        matches!(
            (self, next),
            (Queued, Running | Paused | Cancelled | Failed)
                | (Running, Paused | Retrying | Verifying | Failed | Cancelled)
                | (Paused, Queued | Cancelled)
                | (Retrying, Queued | Running | Paused | Failed | Cancelled)
                | (Verifying, Completed | Failed | Cancelled)
                | (Failed, Queued | Cancelled)
        )
    }
    pub fn control(self, action: TransferAction) -> Result<Self, ClientError> {
        use TaskState::*;
        let next = match action {
            TransferAction::Pause if matches!(self, Queued | Running | Retrying | Paused) => Paused,
            TransferAction::Resume if matches!(self, Paused | Queued) => Queued,
            TransferAction::Cancel if self != Completed => Cancelled,
            TransferAction::Retry if matches!(self, Failed | Queued) => Queued,
            _ => {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "The task cannot perform this action in its current state.",
                ))
            }
        };
        Ok(next)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskRef {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentDownloadRequest {
    pub remote_id: String,
    pub destination: String,
    pub start_ms: DecimalU64,
    pub end_ms: DecimalU64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_duration_ms: Option<DecimalU64>,
    /// Absence means original; never silently switch media quality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub representation_id: Option<String>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentDownloadPlan {
    pub plan_id: String,
    pub remote_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub representation_id: Option<String>,
    pub start_ms: DecimalU64,
    pub end_ms: DecimalU64,
    pub output_format: String,
    pub stream_copy: bool,
    pub starts_at_previous_keyframe: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aligned_start_ms: Option<DecimalU64>,
    pub subtitle_warning: String,
    pub requires_random_read: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncPlan {
    pub plan_id: String,
    pub base_revision: String,
    pub source: String,
    pub destination: String,
    pub mode: SyncMode,
    pub actions: Vec<SyncAction>,
    pub warnings: Vec<String>,
    pub has_unresolved_conflicts: bool,
    pub has_deletes: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncAction {
    pub id: String,
    pub op: SyncOp,
    pub path: String,
    pub reason: String,
    pub destructive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuotaDimension {
    pub dimension: String,
    pub period: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_bytes: Option<DecimalU64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_bytes: Option<DecimalU64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuotaReport {
    pub backend: BackendId,
    pub account_tier: AccountTier,
    pub source: String,
    pub dimensions: Vec<QuotaDimension>,
    pub checked_at: String,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppStatus {
    pub version: String,
    pub contract_version: String,
    pub mode: String,
    pub storage: StorageStatus,
    pub features: BTreeMap<String, bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageStatus {
    pub status: String,
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ClientError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthState {
    pub backend: BackendId,
    pub authenticated: bool,
    /// NekoPak does not read, store or expose the external CLI's tokens.
    pub credential_owner: String,
    pub account_tier: AccountTier,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CliProbeReport {
    pub backend: BackendId,
    pub status: BackendStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_sha256: Option<String>,
    pub verified_commands: Vec<String>,
    pub live_account_verified: bool,
    pub limitations: Vec<String>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransferEvent {
    pub task_id: String,
    pub state: TaskState,
    pub transferred_bytes: DecimalU64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<DecimalU64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_bytes_per_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ClientError>,
    pub revision: DecimalU64,
    pub updated_at: String,
    pub simulated: bool,
}
impl From<&TransferTask> for TransferEvent {
    fn from(task: &TransferTask) -> Self {
        Self {
            task_id: task.id.clone(),
            state: task.state,
            transferred_bytes: task.transferred_bytes,
            total_bytes: task.total_bytes,
            speed_bytes_per_sec: task.speed_bytes_per_sec,
            error: task.error.clone(),
            revision: task.revision,
            updated_at: task.updated_at.clone(),
            simulated: task.simulated,
        }
    }
}

pub fn page_limit(limit: Option<u32>) -> Result<usize, ClientError> {
    match limit.unwrap_or(100) {
        value @ 1..=500 => Ok(value as usize),
        _ => Err(ClientError::new(
            ErrorCode::Conflict,
            "Page limit must be between 1 and 500.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn u64_is_lossless_and_rejects_ambiguous_wire_values() {
        for value in [0, 9_007_199_254_740_993, u64::MAX] {
            let json = serde_json::to_string(&DecimalU64(value)).unwrap();
            assert_eq!(json, format!("\"{value}\""));
            assert_eq!(serde_json::from_str::<DecimalU64>(&json).unwrap().0, value);
        }
        for invalid in [
            "42",
            "\"01\"",
            "\"-1\"",
            "\"1e3\"",
            "\"18446744073709551616\"",
            "\"+1\"",
        ] {
            assert!(serde_json::from_str::<DecimalU64>(invalid).is_err());
        }
    }
    #[test]
    fn state_machine_never_completes_unverified_work() {
        assert!(!TaskState::Running.can_transition_to(TaskState::Completed));
        assert!(TaskState::Verifying.can_transition_to(TaskState::Completed));
        for state in [TaskState::Completed, TaskState::Cancelled] {
            assert!(state.control(TransferAction::Resume).is_err());
            assert!(!state.can_transition_to(TaskState::Running));
        }
        assert_eq!(
            TaskState::Paused.control(TransferAction::Pause).unwrap(),
            TaskState::Paused
        );
    }
    #[test]
    fn errors_map_without_ambiguous_permission_or_quota_retry() {
        assert_eq!(
            ClientError::from_http(401, BackendId::Mock).code,
            ErrorCode::AuthExpired
        );
        assert_eq!(
            ClientError::from_http(403, BackendId::Mock).code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            ClientError::from_http(404, BackendId::Mock).code,
            ErrorCode::NotFound
        );
        assert!(ClientError::from_http(429, BackendId::Mock).retryable);
        assert!(!ClientError::new(ErrorCode::QuotaExceeded, "Quota exhausted.").retryable);
    }
}
