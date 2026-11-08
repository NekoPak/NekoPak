use crate::{ClientError, RemoteEntry};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileQuery {
    Starred,
    Trash,
    Recent,
    Search(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutateOp {
    Mkdir,
    Rename,
    Move,
    Copy,
    Trash,
    Restore,
    Star,
    Unstar,
    DeleteForever,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MutateTarget {
    pub parent_id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileMutation {
    pub op: MutateOp,
    pub ids: Vec<String>,
    pub target: Option<MutateTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationFailure {
    pub id: String,
    pub error: ClientError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationResult {
    pub op: MutateOp,
    pub succeeded: Vec<String>,
    pub failed: Vec<MutationFailure>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub created: Vec<RemoteEntry>,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShareInfo {
    pub share_id: String,
    pub title: String,
    pub item_count: u32,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub state: String,
    pub expires_known: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save_count: Option<u32>,
}
