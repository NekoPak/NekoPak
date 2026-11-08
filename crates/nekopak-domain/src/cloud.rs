//! Additive native cloud protocols; never contain API/session credentials.
use crate::{ClientError, DecimalU64, RemoteEntry, TaskState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CloudTaskSnapshot {
    pub id: String,
    pub name: String,
    pub state: TaskState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<DecimalU64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ClientError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudResourceFile {
    pub index: u32,
    pub path: String,
    pub size_bytes: DecimalU64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudResourcePreview {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<DecimalU64>,
    pub files: Vec<CloudResourceFile>,
    /// False until a protocol for selecting indices at creation is verified.
    pub selection_supported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreateShareRequest {
    pub ids: Vec<String>,
    pub with_code: bool,
    /// 0 = no expiry; supported service options are 1, 7 and 30 days.
    pub expires_in_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BulkResult {
    pub succeeded: Vec<String>,
    pub failed: Vec<crate::MutationFailure>,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SharedFolder {
    pub access_ref: String,
    pub share_id: String,
    pub title: String,
    pub parent_id: String,
    pub entries: Vec<RemoteEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UploadPrepared {
    pub upload_ref: String,
    pub remote_id: String,
    pub total_bytes: DecimalU64,
    pub gcid: String,
    pub completed: bool,
    pub part_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub path: String,
    pub name: String,
    pub kind: crate::EntryKind,
    pub size_bytes: DecimalU64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveListing {
    pub file_id: String,
    pub path: String,
    pub format: String,
    pub supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub encrypted: bool,
    pub needs_password: bool,
    pub entries: Vec<ArchiveEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveJob {
    pub job_id: String,
    pub state: crate::TaskState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
}
