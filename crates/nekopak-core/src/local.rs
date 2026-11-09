//! Explicit read-only local sources; opaque IDs never authorize paths outside a registered root.
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Clone, Serialize, Deserialize)]
pub struct LocalSource {
    pub source_id: String,
    pub root_id: String,
    pub name: String,
    pub path: String,
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorCode::PermissionDenied,
        "本地目录未映射、路径已改变或包含不安全的链接。",
    )
}
fn local_id(source: &str, path: &Path) -> Result<String, ClientError> {
    let text = path.to_str().ok_or_else(invalid)?;
    Ok(format!(
        "local:{source}:{}",
        URL_SAFE_NO_PAD.encode(text.as_bytes())
    ))
}
impl AppService {
    pub fn local_sources_list(&self) -> Result<Vec<LocalSource>, ClientError> {
        self.records()?
            .record_list("device", "local-sources")?
            .into_iter()
            .map(|(_, v)| serde_json::from_str(&v).map_err(|_| invalid()))
            .collect()
    }
    pub fn local_sources_add(&self, path: &str) -> Result<LocalSource, ClientError> {
        validate_destination(path)?;
        let root = Path::new(path);
        let _lock = downloads::lock_parents(root)?;
        if !root.is_dir() {
            return Err(invalid());
        }
        let path = root.canonicalize().map_err(|_| invalid())?;
        let display = path
            .to_string_lossy()
            .trim_start_matches("\\\\?\\")
            .to_owned();
        for source in self.local_sources_list()? {
            if source.path.eq_ignore_ascii_case(&display) {
                return Ok(source);
            }
        }
        let source_id = uuid::Uuid::new_v4().to_string();
        let source = LocalSource {
            root_id: format!("local:{source_id}:"),
            source_id,
            name: root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Local files")
                .into(),
            path: display,
        };
        self.records()?.record_put(
            "device",
            "local-sources",
            &source.source_id,
            &serde_json::to_string(&source).map_err(|_| invalid())?,
        )?;
        Ok(source)
    }
    pub fn local_sources_remove(&self, id: &str) -> Result<(), ClientError> {
        self.records()?
            .record_remove("device", "local-sources", id)?;
        {
            let streams = self.streams.lock().map_err(|_| invalid())?;
            self.stream_epoch
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            for control in streams.values() {
                control.cancel();
            }
        }
        for control in self.media_controls.lock().map_err(|_| invalid())?.values() {
            control.cancel();
        }
        Ok(())
    }
    pub(crate) fn local_resolve(
        &self,
        id: &str,
    ) -> Result<(LocalSource, PathBuf, PathBuf), ClientError> {
        let mut parts = id.splitn(3, ':');
        if parts.next() != Some("local") {
            return Err(invalid());
        }
        let source_id = parts.next().ok_or_else(invalid)?;
        let raw = String::from_utf8(
            URL_SAFE_NO_PAD
                .decode(parts.next().ok_or_else(invalid)?)
                .map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?;
        let relative = PathBuf::from(&raw);
        if relative.is_absolute()
            || relative
                .components()
                .any(|p| !matches!(p, std::path::Component::Normal(_)))
        {
            return Err(invalid());
        }
        let source: LocalSource = serde_json::from_str(
            &self
                .records()?
                .record_get("device", "local-sources", source_id)?
                .ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())?;
        let root = PathBuf::from(&source.path);
        let path = root.join(&relative);
        let mut check = root;
        let _lock = downloads::lock_parents(&check)?;
        for component in relative.components() {
            check.push(component);
            let metadata = std::fs::symlink_metadata(&check).map_err(|_| invalid())?;
            if metadata.file_type().is_symlink() {
                return Err(invalid());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(invalid());
                }
            }
        }
        Ok((source, path, relative))
    }
    pub fn local_stat(&self, id: &str) -> Result<RemoteEntry, ClientError> {
        let (source, path, relative) = self.local_resolve(id)?;
        let meta = std::fs::metadata(&path).map_err(|_| invalid())?;
        if !meta.is_file() && !meta.is_dir() {
            return Err(invalid());
        }
        let modified = meta
            .modified()
            .map_err(|_| invalid())?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| invalid())?;
        let parent = if relative.as_os_str().is_empty() {
            None
        } else {
            Some(local_id(
                &source.source_id,
                relative.parent().unwrap_or_else(|| Path::new("")),
            )?)
        };
        Ok(RemoteEntry {
            id: id.into(),
            parent_id: parent,
            name: if relative.as_os_str().is_empty() {
                source.name
            } else {
                path.file_name()
                    .and_then(|s| s.to_str())
                    .ok_or_else(invalid)?
                    .into()
            },
            kind: if meta.is_dir() {
                EntryKind::Folder
            } else {
                EntryKind::File
            },
            size_bytes: DecimalU64(if meta.is_file() { meta.len() } else { 0 }),
            modified_at: Some(
                time::OffsetDateTime::from_unix_timestamp(modified.as_secs() as i64)
                    .map_err(|_| invalid())?
                    .format(&time::format_description::well_known::Rfc3339)
                    .map_err(|_| invalid())?,
            ),
            etag: Some(format!("{}:{}", meta.len(), modified.as_nanos())),
            hash: None,
            mime_type: None,
            starred: None,
        })
    }
    pub fn local_list(
        &self,
        id: &str,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Page<RemoteEntry>, ClientError> {
        let (source, path, relative) = self.local_resolve(id)?;
        let limit = page_limit(limit)?;
        let mut entries = Vec::new();
        for item in std::fs::read_dir(&path).map_err(|_| invalid())? {
            let item = item.map_err(|_| invalid())?;
            let id = local_id(&source.source_id, &relative.join(item.file_name()))?;
            match self.local_stat(&id) {
                Ok(entry) => entries.push(entry),
                Err(_) => continue,
            }
            if entries.len() > 100_000 {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "本地目录超过单目录安全上限。",
                ));
            }
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        let stamp = fingerprint(&[&serde_json::to_string(&entries).map_err(|_| invalid())?]);
        let start = if let Some(c) = cursor {
            let (a, b) = c.split_once(':').ok_or_else(invalid)?;
            if a != stamp {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "本地目录已改变，请刷新。",
                ));
            }
            b.parse::<usize>().map_err(|_| invalid())?
        } else {
            0
        };
        if start > entries.len() {
            return Err(invalid());
        }
        let next = (start + limit < entries.len()).then(|| format!("{stamp}:{}", start + limit));
        Ok(Page {
            items: entries.into_iter().skip(start).take(limit).collect(),
            next_cursor: next,
        })
    }
    pub fn local_path(&self, id: &str) -> Result<Vec<RemoteEntry>, ClientError> {
        let (source, _, relative) = self.local_resolve(id)?;
        let mut current = PathBuf::new();
        let mut result = vec![self.local_stat(&local_id(&source.source_id, &current)?)?];
        for part in relative.components() {
            current.push(part);
            result.push(self.local_stat(&local_id(&source.source_id, &current)?)?);
        }
        Ok(result)
    }
}
pub(crate) fn open_readonly(path: &Path) -> Result<std::fs::File, ClientError> {
    let _parents = downloads::lock_parents(path.parent().ok_or_else(invalid)?)?;
    let metadata = std::fs::symlink_metadata(path).map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid());
        }
        options.share_mode(1).custom_flags(0x00200000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).map_err(|_| invalid())
}
pub(crate) fn read_identity(file: &std::fs::File) -> Result<String, ClientError> {
    let metadata = file.metadata().map_err(|_| invalid())?;
    let modified = metadata
        .modified()
        .map_err(|_| invalid())?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| invalid())?;
    Ok(format!(
        "{}:{}:{}",
        downloads::file_identity(file)?,
        metadata.len(),
        modified.as_nanos()
    ))
}
