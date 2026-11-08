use nekopak_domain::{ClientError, ErrorCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

pub type Result<T> = std::result::Result<T, ClientError>;
pub(crate) fn error(code: ErrorCode, message: &'static str) -> ClientError {
    ClientError::new(code, message)
}
pub(crate) fn invalid() -> ClientError {
    error(ErrorCode::IntegrityFailed, "挂载目录或读取内容未通过校验。")
}
pub(crate) fn cancelled() -> ClientError {
    error(ErrorCode::Cancelled, "挂载读取已取消。")
}

/// A relative Windows-safe path, with `/` separators. Root is implicit, never an entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MountEntry {
    pub path: String,
    pub remote_id: String,
    /// Stable account + object + representation + version identity, never a signed URL.
    pub identity: String,
    pub size: u64,
    pub directory: bool,
    /// UTC Windows FILETIME ticks, or zero when not provided by the source.
    pub modified_filetime: u64,
}

/// Immutable, complete snapshot. An incomplete/failed remote listing must never construct one.
#[derive(Debug)]
pub struct MountSnapshot {
    entries: BTreeMap<String, MountEntry>,
    fingerprint: String,
    total: u64,
}
impl MountSnapshot {
    pub fn new(entries: Vec<MountEntry>) -> Result<Self> {
        if entries.len() > 100_000 {
            return Err(invalid());
        }
        let mut map = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut total = 0u64;
        for entry in entries {
            validate_relative_path(&entry.path)?;
            if entry.remote_id.is_empty()
                || entry.remote_id.len() > 4096
                || entry.identity.is_empty()
                || entry.identity.len() > 4096
                || entry.size > i64::MAX as u64
                || (entry.directory && entry.size != 0)
                || !ids.insert(entry.remote_id.clone())
            {
                return Err(invalid());
            }
            total = total.checked_add(entry.size).ok_or_else(invalid)?;
            if map.insert(path_key(&entry.path), entry).is_some() {
                return Err(error(ErrorCode::Conflict, "挂载目录存在大小写冲突。"));
            }
        }
        for entry in map.values() {
            if let Some((parent, _)) = entry.path.rsplit_once('/') {
                if !map.get(&path_key(parent)).is_some_and(|p| p.directory) {
                    return Err(invalid());
                }
            }
        }
        let encoded = serde_json::to_vec(&map).map_err(|_| invalid())?;
        if encoded.len() > 32 * 1024 * 1024 {
            return Err(error(
                ErrorCode::Unsupported,
                "挂载目录元数据超过当前安全上限。",
            ));
        }
        Ok(Self {
            entries: map,
            fingerprint: format!("{:x}", Sha256::digest(&encoded)),
            total,
        })
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn total_bytes(&self) -> u64 {
        self.total
    }
    pub fn entries(&self) -> impl Iterator<Item = &MountEntry> {
        self.entries.values()
    }
    pub fn get(&self, path: &str) -> Option<&MountEntry> {
        self.entries.get(&path_key(path))
    }
    pub fn children(&self, directory: &str) -> Vec<&MountEntry> {
        let parent = path_key(directory);
        self.entries
            .values()
            .filter(|e| path_key(e.path.rsplit_once('/').map(|p| p.0).unwrap_or("")) == parent)
            .collect()
    }
    pub fn by_remote_id(&self, id: &str) -> Option<&MountEntry> {
        self.entries.values().find(|e| e.remote_id == id)
    }
}

pub(crate) fn path_key(path: &str) -> String {
    path.to_uppercase()
}
pub fn validate_relative_path(path: &str) -> Result<()> {
    if path.is_empty() || path.encode_utf16().count() > 30_000 {
        return Err(invalid());
    }
    for name in path.split('/') {
        let stem = name.split('.').next().unwrap_or("").to_uppercase();
        if name.is_empty()
            || matches!(name, "." | "..")
            || name.ends_with([' ', '.'])
            || name.encode_utf16().count() > 255
            || name
                .chars()
                .any(|c| c < ' ' || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
            || matches!(
                stem.as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
            )
            || (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.chars().count() == 4
                && stem
                    .chars()
                    .last()
                    .is_some_and(|c| matches!(c, '1'..='9' | '¹' | '²' | '³'))
            || name.to_ascii_lowercase().starts_with(".nekopak-")
        {
            return Err(invalid());
        }
    }
    Ok(())
}

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);
impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(cancelled())
        } else {
            Ok(())
        }
    }
}

/// Implement in Rust Core using its authenticated, account/version-checked cache.
/// Each call MUST enforce entry.identity/size and honor cancellation + a finite timeout.
/// A synchronous bridge may block a callback worker, never a Tokio runtime worker.
pub trait ReadSource: Send + Sync + 'static {
    fn read_exact(
        &self,
        entry: &MountEntry,
        offset: u64,
        length: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>>;
}

/// Actual nonsequential reads, never a capability flag alone. A fully empty tree
/// cannot establish random read support and is therefore not mountable yet.
pub fn verify_random_read(
    snapshot: &MountSnapshot,
    source: &dyn ReadSource,
    cancel: &CancellationToken,
) -> Result<()> {
    let entry = snapshot
        .entries()
        .filter(|e| !e.directory && e.size > 1)
        .max_by_key(|e| e.size)
        .ok_or_else(|| {
            error(
                ErrorCode::Unsupported,
                "请先选择包含可验证随机读取文件的目录。",
            )
        })?;
    let length = (entry.size / 2).min(4096) as usize;
    for at in [
        entry.size - length as u64,
        0,
        (entry.size - length as u64) / 2,
    ] {
        cancel.check()?;
        let bytes = source.read_exact(entry, at, length, cancel)?;
        cancel.check()?;
        if bytes.len() != length {
            return Err(invalid());
        }
    }
    Ok(())
}

/// Shared callback engine with bounded reads and request-specific cancellation.
pub struct ReadSession {
    pub snapshot: Arc<MountSnapshot>,
    source: Arc<dyn ReadSource>,
    stopped: AtomicBool,
    requests: Mutex<BTreeMap<u64, CancellationToken>>,
    cancelled: Mutex<BTreeSet<u64>>,
}
impl ReadSession {
    pub fn verified(
        snapshot: Arc<MountSnapshot>,
        source: Arc<dyn ReadSource>,
    ) -> Result<Arc<Self>> {
        verify_random_read(&snapshot, source.as_ref(), &CancellationToken::default())?;
        Ok(Arc::new(Self {
            snapshot,
            source,
            stopped: AtomicBool::new(false),
            requests: Mutex::new(BTreeMap::new()),
            cancelled: Mutex::new(BTreeSet::new()),
        }))
    }
    pub fn begin(self: &Arc<Self>, request: u64) -> Result<ReadRequest> {
        let mut requests = self.requests.lock().map_err(|_| invalid())?;
        if self.stopped.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        if self
            .cancelled
            .lock()
            .map_err(|_| invalid())?
            .remove(&request)
        {
            return Err(cancelled());
        }
        if requests.len() >= 16 {
            return Err(error(
                ErrorCode::TransientNetwork,
                "挂载读取繁忙，请稍后重试。",
            ));
        }
        if requests.contains_key(&request) {
            return Err(error(ErrorCode::Conflict, "重复挂载读取请求。"));
        }
        let cancel = CancellationToken::default();
        requests.insert(request, cancel.clone());
        Ok(ReadRequest {
            session: self.clone(),
            id: request,
            cancel,
        })
    }
    pub fn cancel(&self, request: u64) {
        if let Ok(requests) = self.requests.lock() {
            if let Some(c) = requests.get(&request) {
                c.cancel();
            } else if let Ok(mut cancelled) = self.cancelled.lock() {
                // A native cancel callback may run just before fetch registers
                // its request. Retain a bounded tombstone for that ordering.
                if cancelled.len() >= 1024 {
                    cancelled.pop_first();
                }
                cancelled.insert(request);
            }
        }
    }
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Ok(requests) = self.requests.lock() {
            for c in requests.values() {
                c.cancel();
            }
        }
    }
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
}
pub struct ReadRequest {
    session: Arc<ReadSession>,
    id: u64,
    cancel: CancellationToken,
}
impl ReadRequest {
    pub fn read(&self, entry: &MountEntry, offset: u64, length: usize) -> Result<Vec<u8>> {
        self.cancel.check()?;
        if self.session.snapshot.get(&entry.path) != Some(entry)
            || entry.directory
            || length > 4 * 1024 * 1024
            || offset
                .checked_add(length as u64)
                .is_none_or(|end| end > entry.size)
        {
            return Err(invalid());
        }
        let bytes = self
            .session
            .source
            .read_exact(entry, offset, length, &self.cancel)?;
        self.cancel.check()?;
        if bytes.len() != length {
            return Err(invalid());
        }
        Ok(bytes)
    }
    pub fn check(&self) -> Result<()> {
        self.cancel.check()
    }
}
impl Drop for ReadRequest {
    fn drop(&mut self) {
        if let Ok(mut requests) = self.session.requests.lock() {
            requests.remove(&self.id);
        }
    }
}

/// Cloud Files TRANSFER_DATA must be 4 KiB aligned, except the range ending at EOF.
pub fn hydration_range(offset: u64, length: u64, size: u64) -> Result<std::ops::Range<u64>> {
    if length == 0 || offset >= size {
        return Err(invalid());
    }
    let end = offset.checked_add(length).ok_or_else(invalid)?.min(size);
    Ok((offset / 4096 * 4096)
        ..end
            .checked_add(4095)
            .ok_or_else(invalid)?
            .div_euclid(4096)
            .saturating_mul(4096)
            .min(size))
}
