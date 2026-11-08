//! Independent parsing of the supplied API specification. No upstream implementation copied.
use crate::error;
use nekopak_domain::*;
use serde_json::Value;
use std::collections::BTreeSet;
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};

pub fn check_response(
    status: u16,
    bytes: &[u8],
    retry_after: Option<u32>,
    signing_in: bool,
) -> Result<(), ClientError> {
    if (200..300).contains(&status) && bytes.is_empty() {
        return Ok(());
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| {
        if status >= 400 {
            let mut error = ClientError::from_http(status, BackendId::RustCompat);
            error.request_id = Some(uuid::Uuid::new_v4().to_string());
            error
        } else {
            invalid()
        }
    })?;
    let name = value.get("error").and_then(Value::as_str).unwrap_or("");
    if (200..300).contains(&status) && name.is_empty() {
        return Ok(());
    }
    let mut result = match name {
        "invalid_credentials"
        | "invalid_username_or_password"
        | "invalid_account_or_password"
        | "invalid_password" => invalid_credentials(),
        "invalid_grant" if signing_in => invalid_credentials(),
        "invalid_grant" | "unauthenticated" | "token_expired" => {
            error(ErrorCode::AuthExpired, "登录已过期，请重新登录。")
        }
        "account_restricted" | "account_banned" => error(
            ErrorCode::AccountRestricted,
            "此账号暂时受限，请在 PikPak 完成账号检查。",
        ),
        "captcha_invalid" | "captcha_required" | "captcha_expired" => error(
            ErrorCode::VerificationRequired,
            "PikPak 需要完成安全验证后才能继续此操作。",
        ),
        "file_not_found" => error(ErrorCode::NotFound, "文件或文件夹不存在。"),
        "file_space_not_enough" => quota_error("storage"),
        "monthly_download_size_exceeded" | "daily_download_size_exceeded" => {
            quota_error("download")
        }
        "monthly_upload_size_exceeded" => quota_error("upload"),
        "monthly_cloud_download_size_exceeded" => quota_error("cloud_download"),
        name if name.starts_with("task_daily_create_limit")
            || name == "daily_create_limit_free" =>
        {
            quota_error("cloud_download_daily")
        }
        "file_duplicated_name" | "file_rename_uncompleted" => error(
            ErrorCode::Conflict,
            "文件名称冲突或上次操作尚未完成，请刷新后核对。",
        ),
        _ => ClientError::from_http(status, BackendId::RustCompat),
    };
    if result.request_id.is_none() {
        result.request_id = Some(uuid::Uuid::new_v4().to_string());
    }
    if status == 429 {
        if let Some(seconds) = retry_after {
            result
                .details
                .get_or_insert_with(Default::default)
                .insert("retry_after_seconds".into(), seconds.into());
        }
    }
    Err(result)
}

pub fn invalid_credentials() -> ClientError {
    let mut result = error(
        ErrorCode::PermissionDenied,
        "账号或密码不正确，请重新输入。",
    );
    result.details = Some([("reason".into(), Value::from("invalid_credentials"))].into());
    result
}
fn quota_error(dimension: &str) -> ClientError {
    let mut result = error(
        ErrorCode::QuotaExceeded,
        "存储空间或服务配额不足。请查看账号额度。 ",
    );
    result.details = Some([("dimension".into(), Value::from(dimension))].into());
    result
}
pub fn invalid() -> ClientError {
    error(
        ErrorCode::IntegrityFailed,
        "接口响应不符合预期，未接受不完整数据。",
    )
}
pub fn validate_id(id: &str) -> Result<(), ClientError> {
    if id.is_empty()
        || id.len() > 256
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(error(
            ErrorCode::Conflict,
            "无效的文件标识，请刷新目录后重试。",
        ));
    }
    Ok(())
}
pub fn list(
    value: &Value,
    parent: &str,
    limit: usize,
) -> Result<(Vec<RemoteEntry>, Option<String>), ClientError> {
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
    let next = match value.get("next_page_token") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.is_empty() => None,
        Some(Value::String(s)) if s.len() <= 4096 && !s.chars().any(char::is_control) => {
            Some(s.clone())
        }
        _ => return Err(invalid()),
    };
    if next.is_some() && items.is_empty() {
        return Err(invalid());
    }
    Ok((items, next))
}
pub fn entry(value: &Value) -> Result<RemoteEntry, ClientError> {
    entry_in(value, false)
}
pub fn entry_in(value: &Value, allow_trashed: bool) -> Result<RemoteEntry, ClientError> {
    if (!allow_trashed && value.get("trashed").and_then(Value::as_bool) == Some(true))
        || value
            .get("phase")
            .and_then(Value::as_str)
            .is_some_and(|s| s != "PHASE_TYPE_COMPLETE")
    {
        return Err(invalid());
    }
    let id = text(value, "id", 256)?;
    validate_id(&id)?;
    let parent_id = match value.get("parent_id") {
        Some(Value::String(s)) if s.is_empty() => Some("root".into()),
        Some(Value::String(s)) => {
            validate_id(s)?;
            Some(s.clone())
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
        Some(v) => number(v)?,
        _ => return Err(invalid()),
    };
    let modified_at = match value.get("modified_time") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(
            OffsetDateTime::parse(s, &Rfc3339)
                .map_err(|_| invalid())?
                .to_offset(UtcOffset::UTC)
                .format(&Rfc3339)
                .map_err(|_| invalid())?,
        ),
        _ => return Err(invalid()),
    };
    let mime_type = match value.get("mime_type") {
        Some(Value::String(s))
            if !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control) =>
        {
            Some(s.clone())
        }
        None | Some(Value::Null) | Some(Value::String(_)) => None,
        _ => return Err(invalid()),
    };
    Ok(RemoteEntry {
        id,
        parent_id,
        name: text(value, "name", 4096)?,
        kind,
        size_bytes,
        modified_at,
        etag: None,
        hash: value
            .get("hash")
            .and_then(Value::as_str)
            .filter(|s| s.len() == 40 && s.bytes().all(|c| c.is_ascii_hexdigit()))
            .map(|s| ContentHash {
                algorithm: HashAlgorithm::Gcid,
                value: s.to_ascii_uppercase(),
            }),
        mime_type,
        starred: value.get("starred").and_then(Value::as_bool).or_else(|| {
            value.get("tags").and_then(Value::as_array).map(|tags| {
                tags.iter()
                    .any(|tag| tag.get("name").and_then(Value::as_str) == Some("STAR"))
            })
        }),
    })
}
pub fn text(value: &Value, key: &str, max: usize) -> Result<String, ClientError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(invalid)
}

pub fn number(value: &Value) -> Result<DecimalU64, ClientError> {
    if let Some(n) = value.as_u64() {
        return Ok(DecimalU64(n));
    }
    serde_json::from_value(value.clone()).map_err(|_| invalid())
}
pub fn next_token(value: &Value) -> Result<Option<String>, ClientError> {
    match value.get("next_page_token") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) if s.len() <= 4096 && !s.chars().any(char::is_control) => {
            Ok(Some(s.clone()))
        }
        _ => Err(invalid()),
    }
}
