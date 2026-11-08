use crate::AuthStatus;
use serde::{Deserialize, Serialize};

/// Public local identity only. Sessions and login identifiers remain in the OS vault.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedAccount {
    pub id: String,
    pub display_name: String,
    pub active: bool,
    pub state: AuthStatus,
}
