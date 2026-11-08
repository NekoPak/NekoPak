use crate::{ClientError, ContentHash, DecimalU64, EntryKind, RemoteEntry};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedEntry {
    pub id: String,
    pub name: Option<String>,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRef {
    pub task_ids: Vec<String>,
    pub skipped: Vec<SkippedEntry>,
    pub expanded_files: u32,
    pub group_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRef {
    pub scan_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateEntry {
    #[serde(flatten)]
    pub entry: RemoteEntry,
    pub path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub group_id: String,
    #[serde(rename = "match")]
    pub match_kind: String,
    pub hash: ContentHash,
    pub size_bytes: DecimalU64,
    pub entries: Vec<DuplicateEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedPath {
    pub path: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateReport {
    pub scan_id: String,
    pub folder_id: String,
    pub folder_path: String,
    pub state: String,
    pub groups: Vec<DuplicateGroup>,
    pub files_scanned: u32,
    pub folders_scanned: u32,
    pub reclaimable_bytes: DecimalU64,
    pub unhashed_files: u32,
    pub skipped: Vec<SkippedPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ClientError>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceNode {
    pub id: String,
    pub name: String,
    pub kind: EntryKind,
    pub size_bytes: DecimalU64,
    pub file_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceSummary {
    pub folder_id: String,
    pub total_bytes: DecimalU64,
    pub file_count: u32,
    pub folder_count: u32,
    pub children: Vec<SpaceNode>,
    pub exactness: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub computed_at: String,
}
