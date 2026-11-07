use crate::process::{error, Output};
use nekopak_domain::*;
use serde_json::Value;
use std::collections::BTreeSet;
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};

pub fn parse_output(output: Output) -> Result<Value, ClientError> {
    if output.success {
        return serde_json::from_slice(&output.stdout).map_err(|_| invalid());
    }
    // Go may prepend diagnostic lines to stderr. Only parse a complete final JSON object.
    let stderr = std::str::from_utf8(&output.stderr).map_err(|_| invalid())?;
    let payload = serde_json::from_str::<Value>(stderr).ok().or_else(|| {
        stderr
            .match_indices('\n')
            .filter_map(|(index, _)| {
                let suffix = stderr[index + 1..].trim_start();
                suffix
                    .starts_with('{')
                    .then(|| serde_json::from_str::<Value>(suffix).ok())
                    .flatten()
            })
            .next()
    });
    let upstream = payload.as_ref().and_then(|value| value.get("error"));
    let code = upstream
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_uppercase();
    let (code, message) = match code.as_str() {
        "UNAUTHENTICATED"
        | "UNAUTHORIZED"
        | "TOKEN_EXPIRED"
        | "TOKEN_REFRESH_FAILED"
        | "CREDENTIALS_ERROR" => (
            ErrorCode::AuthExpired,
            "Authorize the external PikPak CLI profile before browsing.",
        ),
        "ACCESS_DENIED" | "PERMISSION_DENIED" => (
            ErrorCode::PermissionDenied,
            "The CLI session lacks permission for this resource.",
        ),
        "ACCOUNT_RESTRICTED" | "ACCOUNT_BANNED" => (
            ErrorCode::AccountRestricted,
            "The service has restricted this account.",
        ),
        "FILE_NOT_FOUND" | "INVALID_FILE_ID" | "INVALID_PARENT_ID" => (
            ErrorCode::NotFound,
            "The remote file or folder does not exist.",
        ),
        "TRANSFER_QUOTA_EXCEEDED"
        | "CONNECTED_APPS_QUOTA_EXCEEDED"
        | "FILE_SPACE_NOT_ENOUGH"
        | "TASK_DAILY_CREATE_LIMIT"
        | "DAILY_CREATE_LIMIT_FREE" => (
            ErrorCode::QuotaExceeded,
            "The service reports a storage or transfer quota limit.",
        ),
        "TIMEOUT" | "REQUEST_FAILED" | "NETWORK_ERROR" | "TOO_MANY_REQUESTS" | "RATE_LIMITED" => (
            ErrorCode::TransientNetwork,
            "The CLI request failed because of a temporary network or rate limit condition.",
        ),
        "SERVICE_UNAVAILABLE" | "UNSUPPORTED" => (
            ErrorCode::Unsupported,
            "This CLI or service operation is currently unavailable.",
        ),
        "INVALID_ARGUMENT" | "USAGE_ERROR" => (
            ErrorCode::Conflict,
            "The CLI rejected the request arguments.",
        ),
        "FILESYSTEM_ERROR" => (
            ErrorCode::PermissionDenied,
            "The CLI could not access its local profile.",
        ),
        _ => (
            ErrorCode::Internal,
            "The CLI failed with an unrecognized response. Raw diagnostics were withheld.",
        ),
    };
    Err(error(code, message))
}

pub fn auth(value: Value) -> Result<AuthState, ClientError> {
    let authenticated = value
        .get("logged_in")
        .and_then(Value::as_bool)
        .ok_or_else(invalid)?;
    Ok(AuthState { backend: BackendId::OfficialCli, authenticated, credential_owner: "external_cli".into(), account_tier: AccountTier::Unknown,
        message: if authenticated { "The external CLI reports an authorized session. Live account behavior is still a preview." } else { "Run pikpak auth login in this explicitly configured profile, then refresh. NekoPak never receives your password or token." }.into() })
}

pub fn list(
    value: Value,
    parent: &str,
    previous_cursor: Option<&str>,
    limit: usize,
) -> Result<Page<RemoteEntry>, ClientError> {
    let files = value
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    if files.len() > limit {
        return Err(invalid());
    }
    let mut ids = BTreeSet::new();
    let mut items = Vec::new();
    for value in files {
        let item = entry(value)?;
        if !ids.insert(item.id.clone()) || item.parent_id.as_deref() != Some(parent) {
            return Err(invalid());
        }
        items.push(item);
    }
    let next_cursor = match value.get("next_page_token") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if value.is_empty() => None,
        Some(Value::String(value)) => {
            validate_cursor(value)?;
            Some(value.clone())
        }
        _ => return Err(invalid()),
    };
    if next_cursor.is_some() && (items.is_empty() || next_cursor.as_deref() == previous_cursor) {
        return Err(invalid());
    }
    Ok(Page { items, next_cursor })
}

pub fn entry(value: &Value) -> Result<RemoteEntry, ClientError> {
    if value.get("trashed").and_then(Value::as_bool) == Some(true)
        || value
            .get("phase")
            .and_then(Value::as_str)
            .is_some_and(|phase| phase != "PHASE_TYPE_COMPLETE")
    {
        return Err(invalid());
    }
    let id = required_string(value, "id", 256)?;
    validate_id(&id, false)?;
    let parent = match value.get("parent_id") {
        Some(Value::String(parent)) if parent.is_empty() => Some("root".into()),
        Some(Value::String(parent)) => {
            validate_id(parent, false)?;
            Some(parent.clone())
        }
        None | Some(Value::Null) => None,
        _ => return Err(invalid()),
    };
    let kind = match value.get("kind").and_then(Value::as_str) {
        Some("drive#file") => EntryKind::File,
        Some("drive#folder") => EntryKind::Folder,
        _ => return Err(invalid()),
    };
    let size_bytes = match value.get("size") {
        None | Some(Value::Null) if kind == EntryKind::Folder => DecimalU64(0),
        Some(value) => {
            serde_json::from_value::<DecimalU64>(value.clone()).map_err(|_| invalid())?
        }
        _ => return Err(invalid()),
    };
    let modified_at = match value.get("modified_time") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(
            OffsetDateTime::parse(value, &Rfc3339)
                .map_err(|_| invalid())?
                .to_offset(UtcOffset::UTC)
                .format(&Rfc3339)
                .map_err(|_| invalid())?,
        ),
        _ => return Err(invalid()),
    };
    let mime_type = match value.get("mime_type") {
        Some(Value::String(value))
            if !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control) =>
        {
            Some(value.clone())
        }
        None | Some(Value::Null) | Some(Value::String(_)) => None,
        _ => return Err(invalid()),
    };
    Ok(RemoteEntry {
        id,
        parent_id: parent,
        name: required_string(value, "name", 4096)?,
        kind,
        size_bytes,
        modified_at,
        etag: None,
        hash: None,
        mime_type,
    })
}
fn required_string(value: &Value, key: &str, max: usize) -> Result<String, ClientError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty() && text.len() <= max && !text.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(invalid)
}
pub fn validate_id(id: &str, root_allowed: bool) -> Result<(), ClientError> {
    if (root_allowed && id == "root")
        || (!id.is_empty()
            && id.len() <= 256
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            && !id.starts_with('-'))
    {
        return Ok(());
    }
    Err(error(
        ErrorCode::Conflict,
        "Invalid remote ID; select an ID returned by the file browser.",
    ))
}
pub fn validate_cursor(cursor: &str) -> Result<(), ClientError> {
    if cursor.is_empty() || cursor.len() > 4096 || cursor.chars().any(char::is_control) {
        return Err(error(ErrorCode::Conflict, "Invalid CLI page cursor."));
    }
    Ok(())
}
pub fn invalid() -> ClientError {
    error(
        ErrorCode::IntegrityFailed,
        "The CLI response does not match the reviewed protocol; no partial results were accepted.",
    )
}
