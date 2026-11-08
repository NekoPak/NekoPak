use crate::{ClientError, RemoteEntry};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferenceItem {
    pub path: String,
    pub entry: RemoteEntry,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ClientError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferenceManifest {
    pub manifest_id: String,
    pub account_id: String,
    pub created_at: String,
    pub content_sha256: String,
    pub state: String,
    pub roots: Vec<String>,
    pub entries: Vec<ReferenceItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_manifest_id: Option<String>,
}

/// Internal provider result preserving trash state without exposing arbitrary params.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryMetadata {
    pub entry: RemoteEntry,
    pub trashed: bool,
}

/// Every accepted creation preserves its ID, including an upload-needed result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentCreateReceipt {
    pub file_id: String,
    pub completed: bool,
}
