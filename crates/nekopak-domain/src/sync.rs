use crate::{ClientError, SyncMode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SyncConfig {
    pub config_id: String,
    pub name: String,
    pub mode: SyncMode,
    pub local_path: String,
    pub remote_id: String,
    pub remote_path: String,
    pub exclude_globs: Vec<String>,
    pub allow_deletes: bool,
    pub first_run: bool,
    #[serde(default)]
    pub auto_enabled: bool,
    #[serde(default = "default_interval")]
    pub interval_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_applied_at: Option<String>,
}
fn default_interval() -> u64 {
    300
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncEvent {
    pub id: String,
    pub config_id: String,
    pub at: String,
    pub kind: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ClientError>,
}
