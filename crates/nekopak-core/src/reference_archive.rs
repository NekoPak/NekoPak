//! Verified manifests gate any destructive reference-archive action.
use super::*;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::PathBuf,
};

const MAX_ITEMS: usize = 8_000;
#[cfg(test)]
#[path = "reference_archive_tests.rs"]
mod tests;
const MAX_BYTES: usize = 8 * 1024 * 1024;
fn invalid() -> ClientError {
    ClientError::new(
        ErrorCode::IntegrityFailed,
        "归档清单或文件身份不一致，已保留原数据。",
    )
}
fn conflict(message: &'static str) -> ClientError {
    ClientError::new(ErrorCode::Conflict, message)
}
fn encode<T: Serialize>(value: &T) -> Result<String, ClientError> {
    serde_json::to_string(value).map_err(|_| invalid())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredReference {
    schema_version: u32,
    scope: String,
    manifest: ReferenceManifest,
    upload_path: Option<PathBuf>,
    remote_folder: Option<String>,
    content_gcid: Option<String>,
}
#[derive(Serialize)]
struct ManifestDocument<'a> {
    schema_version: u32,
    manifest_id: &'a str,
    created_at: &'a str,
    roots: &'a [String],
    entries: Vec<(&'a str, &'a RemoteEntry)>,
}
fn manifest_bytes(manifest: &ReferenceManifest) -> Result<Vec<u8>, ClientError> {
    let document = ManifestDocument {
        schema_version: 1,
        manifest_id: &manifest.manifest_id,
        created_at: &manifest.created_at,
        roots: &manifest.roots,
        entries: manifest
            .entries
            .iter()
            .map(|i| (i.path.as_str(), &i.entry))
            .collect(),
    };
    let raw = serde_json::to_vec_pretty(&document).map_err(|_| invalid())?;
    if raw.len() > MAX_BYTES {
        return Err(conflict("归档清单过大，请分批归档。"));
    }
    Ok(raw)
}
fn same_content(expected: &RemoteEntry, current: &RemoteEntry, check_id: bool) -> bool {
    (!check_id || expected.id == current.id)
        && expected.parent_id == current.parent_id
        && expected.name == current.name
        && expected.kind == current.kind
        && expected.size_bytes == current.size_bytes
        && expected.hash == current.hash
}
fn gcid(bytes: &[u8]) -> String {
    let mut digest = Sha1::new();
    for block in bytes.chunks(262144) {
        digest.update(Sha1::digest(block));
    }
    format!("{:X}", digest.finalize())
}
fn verify_local_manifest(path: &Path, expected: &[u8]) -> Result<(), ClientError> {
    let mut file = local::open_readonly(path)?;
    if file.metadata().map_err(|_| invalid())?.len() != expected.len() as u64 {
        return Err(invalid());
    }
    let mut read = vec![0; expected.len()];
    file.read_exact(&mut read).map_err(|_| invalid())?;
    if read != expected {
        return Err(invalid());
    }
    Ok(())
}

impl AppService {
    fn save_reference(&self, stored: &StoredReference) -> Result<(), ClientError> {
        let raw = encode(stored)?;
        if raw.len() > 2 * MAX_BYTES {
            return Err(invalid());
        }
        self.records()?.record_put(
            &stored.scope,
            "references",
            &stored.manifest.manifest_id,
            &raw,
        )?;
        let read = self
            .records()?
            .record_get(&stored.scope, "references", &stored.manifest.manifest_id)?
            .ok_or_else(invalid)?;
        if read != raw {
            return Err(invalid());
        }
        Ok(())
    }
    fn load_reference(&self, id: &str, scope: &str) -> Result<StoredReference, ClientError> {
        uuid::Uuid::parse_str(id).map_err(|_| invalid())?;
        let raw = self
            .records()?
            .record_get(scope, "references", id)?
            .ok_or_else(|| ClientError::new(ErrorCode::NotFound, "此账号没有该归档清单。"))?;
        if raw.len() > 2 * MAX_BYTES {
            return Err(invalid());
        }
        let stored: StoredReference = serde_json::from_str(&raw).map_err(|_| invalid())?;
        if stored.schema_version != 1
            || stored.scope != scope
            || stored.manifest.manifest_id != id
            || stored.manifest.entries.is_empty()
            || stored.manifest.entries.len() > MAX_ITEMS
            || stored.manifest.content_sha256
                != format!("{:x}", Sha256::digest(manifest_bytes(&stored.manifest)?))
        {
            return Err(invalid());
        }
        Ok(stored)
    }
    pub async fn reference_create(
        &self,
        ids: &[String],
        key: &str,
        ctx: &RequestContext,
    ) -> Result<ReferenceManifest, ClientError> {
        if ids.is_empty()
            || ids.len() > 500
            || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
            || ids.iter().any(|id| id == "root" || id.contains(':'))
        {
            return Err(conflict("请选择1至500个网盘文件或目录。"));
        }
        let _gate = ctx
            .run(async { Ok(self.reference_gate.lock().await) })
            .await?;
        let auth = ctx.run(self.provider.auth_status()).await?;
        let account = auth
            .account_id
            .ok_or_else(|| ClientError::new(ErrorCode::AuthExpired, "请先选择账号。"))?;
        let scope = ctx.run(self.provider.account_scope_for(&account)).await?;
        let key = scoped_key("reference-create", key)?;
        let fp = fingerprint(&[&encode(&ids)?]);
        if let Some(raw) = self
            .records()?
            .record_get(&scope, "reference-create", &key)?
        {
            let (old, id): (String, String) = serde_json::from_str(&raw).map_err(|_| invalid())?;
            if old != fp {
                return Err(conflict("同一归档请求不能用于不同文件。"));
            }
            return Ok(self.load_reference(&id, &scope)?.manifest);
        }
        let mut entries = Vec::new();
        let mut seen = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for id in ids {
            self.check_scope(&scope, ctx).await?;
            let root = ctx.run(self.provider.stat(id)).await?;
            self.check_scope(&scope, ctx).await?;
            if root.id != *id {
                return Err(invalid());
            }
            let mut items = vec![catalog::WalkEntry {
                entry: root.clone(),
                relative: vec![root.name.clone()],
            }];
            if root.kind == EntryKind::Folder {
                for mut child in self.walk(id, &scope, ctx, None).await? {
                    child.relative.insert(0, root.name.clone());
                    items.push(child);
                }
            }
            for item in items {
                if entries.len() >= MAX_ITEMS
                    || !seen.insert(item.entry.id.clone())
                    || item.entry.parent_id.is_none()
                    || item.relative.iter().any(|name| {
                        name.is_empty()
                            || matches!(name.as_str(), "." | "..")
                            || name.contains(['/', '\\'])
                            || name.chars().any(char::is_control)
                    })
                {
                    return Err(invalid());
                }
                let path = item.relative.join("/");
                if !paths.insert(path.to_lowercase()) {
                    return Err(conflict("归档路径存在名称冲突或重复选择。"));
                }
                if item.entry.kind == EntryKind::File
                    && item.entry.hash.as_ref().is_none_or(|h| {
                        h.algorithm != HashAlgorithm::Gcid
                            || h.value.len() != 40
                            || !h.value.bytes().all(|b| b.is_ascii_hexdigit())
                    })
                {
                    return Err(conflict("部分文件缺少完整GCID，未创建可处置原件的归档。"));
                }
                entries.push(ReferenceItem {
                    path,
                    entry: item.entry,
                    state: "recorded".into(),
                    current_id: None,
                    error: None,
                });
            }
        }
        self.check_scope(&scope, ctx).await?;
        let mut manifest = ReferenceManifest {
            manifest_id: uuid::Uuid::new_v4().to_string(),
            account_id: account,
            created_at: now(),
            content_sha256: String::new(),
            state: "prepared".into(),
            roots: ids.to_vec(),
            entries,
            manifest_task_id: None,
            remote_manifest_id: None,
        };
        manifest.content_sha256 = format!("{:x}", Sha256::digest(manifest_bytes(&manifest)?));
        let stored = StoredReference {
            schema_version: 1,
            scope: scope.clone(),
            manifest: manifest.clone(),
            upload_path: None,
            remote_folder: None,
            content_gcid: None,
        };
        self.save_reference(&stored)?;
        self.records()?.record_put(
            &scope,
            "reference-create",
            &key,
            &encode(&(fp, &manifest.manifest_id))?,
        )?;
        Ok(manifest)
    }
    pub async fn reference_list(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<ReferenceManifest>, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        self.records()?
            .record_list(&scope, "references")?
            .into_iter()
            .map(|(id, _)| self.load_reference(&id, &scope).map(|s| s.manifest))
            .collect()
    }
    pub async fn reference_status(
        &self,
        id: &str,
        ctx: &RequestContext,
    ) -> Result<ReferenceManifest, ClientError> {
        let _gate = ctx
            .run(async { Ok(self.reference_gate.lock().await) })
            .await?;
        let scope = ctx.run(self.provider.account_scope()).await?;
        let mut stored = self.load_reference(id, &scope)?;
        if stored.manifest.state == "uploading" {
            if let Some(task) = &stored.manifest.manifest_task_id {
                if self.store()?.get(task)?.state == TaskState::Completed {
                    self.verify_reference_upload(&mut stored, ctx).await?;
                }
            }
        }
        Ok(stored.manifest)
    }
    async fn reference_folder(
        &self,
        scope: &str,
        ctx: &RequestContext,
    ) -> Result<String, ClientError> {
        let name = "NekoPak Reference Manifests";
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        let mut found = None;
        loop {
            self.check_scope(scope, ctx).await?;
            let page = ctx
                .run(self.provider.list("root", cursor.as_deref(), 500))
                .await?;
            self.check_scope(scope, ctx).await?;
            for entry in page.items {
                if entry.name.eq_ignore_ascii_case(name) {
                    if entry.kind != EntryKind::Folder || entry.name != name || found.is_some() {
                        return Err(conflict("归档专用目录名称冲突，请先检查网盘根目录。"));
                    }
                    found = Some(entry.id);
                }
            }
            let Some(next) = page.next_cursor else {
                break;
            };
            if !seen.insert(next.clone()) || seen.len() > 1000 {
                return Err(invalid());
            }
            cursor = Some(next);
        }
        if let Some(id) = found {
            return Ok(id);
        }
        let result = self
            .files_mutate_at(
                &FileMutation {
                    op: MutateOp::Mkdir,
                    ids: vec![],
                    target: Some(MutateTarget {
                        parent_id: Some("root".into()),
                        name: Some(name.into()),
                    }),
                },
                "reference-manifests-directory",
                scope,
                ctx,
            )
            .await?;
        let folder = result
            .created
            .first()
            .filter(|e| {
                e.kind == EntryKind::Folder
                    && e.name == name
                    && e.parent_id.as_deref() == Some("root")
            })
            .ok_or_else(invalid)?;
        if !result.failed.is_empty() || result.created.len() != 1 {
            return Err(invalid());
        }
        Ok(folder.id.clone())
    }
    pub async fn reference_store_manifest(
        &self,
        id: &str,
        key: &str,
        ctx: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        scoped_key("reference-store", key)?;
        let _gate = ctx
            .run(async { Ok(self.reference_gate.lock().await) })
            .await?;
        let scope = ctx.run(self.provider.account_scope()).await?;
        let mut stored = self.load_reference(id, &scope)?;
        if let Some(task) = &stored.manifest.manifest_task_id {
            return Ok(TaskRef {
                task_id: task.clone(),
            });
        }
        let bytes = manifest_bytes(&stored.manifest)?;
        let root = self.data_root.join("reference-manifests");
        let _parents = downloads::lock_parents(&self.data_root)?;
        if !root.try_exists().map_err(|_| invalid())? {
            std::fs::create_dir(&root).map_err(|_| invalid())?;
        }
        let _root = downloads::lock_parents(&root)?;
        let path = root.join(format!("{id}.nekopak-reference.json"));
        if path.try_exists().map_err(|_| invalid())? {
            verify_local_manifest(&path, &bytes)?;
        } else {
            let temporary = root.join(format!("{id}.{}.tmp", uuid::Uuid::new_v4()));
            let mut file = downloads::open_part(&temporary, true)?;
            file.write_all(&bytes).map_err(|_| invalid())?;
            file.sync_all().map_err(|_| invalid())?;
            downloads::publish(&file, &temporary, &path)?;
        }
        // Keep the machine-generated file identity locked until the queue has
        // captured it; later verification reads the exact expected bytes again.
        let manifest_handle = local::open_readonly(&path)?;
        verify_local_manifest(&path, &bytes)?;
        let folder = match &stored.remote_folder {
            Some(id) => id.clone(),
            None => self.reference_folder(&scope, ctx).await?,
        };
        stored.upload_path = Some(path.clone());
        stored.remote_folder = Some(folder.clone());
        stored.content_gcid = Some(gcid(&bytes));
        self.save_reference(&stored)?;
        let task = self
            .upload_start_at(
                path.to_str().ok_or_else(invalid)?,
                &folder,
                &format!("reference-manifest-{id}"),
                &scope,
                ctx,
            )
            .await?;
        stored.manifest.manifest_task_id = Some(task.task_id.clone());
        stored.manifest.state = "uploading".into();
        self.save_reference(&stored)?;
        drop(manifest_handle);
        Ok(task)
    }
    async fn verify_reference_upload(
        &self,
        stored: &mut StoredReference,
        ctx: &RequestContext,
    ) -> Result<(), ClientError> {
        let bytes = manifest_bytes(&stored.manifest)?;
        let path = stored.upload_path.as_ref().ok_or_else(invalid)?;
        verify_local_manifest(path, &bytes)?;
        let task_id = stored
            .manifest
            .manifest_task_id
            .as_ref()
            .ok_or_else(|| conflict("请先上传归档清单。"))?;
        let task = self.store()?.get(task_id)?;
        if task.kind != TaskKind::Upload
            || task.state != TaskState::Completed
            || task.total_bytes != Some(DecimalU64(bytes.len() as u64))
            || task.simulated
            || task.backend != self.provider.id()
            || task.transferred_bytes != DecimalU64(bytes.len() as u64)
        {
            return Err(conflict("归档清单尚未完成上传，原文件未处置。"));
        }
        #[derive(Deserialize)]
        struct Receipt {
            scope: String,
            path: PathBuf,
            prepared: Option<UploadPrepared>,
        }
        let upload: Receipt = serde_json::from_str(
            &self
                .records()?
                .record_get("tasks", "uploads", task_id)?
                .ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())?;
        let receipt = upload.prepared.ok_or_else(invalid)?;
        if upload.scope != stored.scope
            || upload.path != *path
            || receipt.total_bytes.0 != bytes.len() as u64
            || stored.content_gcid.as_deref() != Some(receipt.gcid.as_str())
        {
            return Err(invalid());
        }
        let info = ctx
            .run(self.provider.download_info(&receipt.remote_id))
            .await?;
        if info.account_scope != stored.scope
            || info.entry.id != receipt.remote_id
            || info.entry.kind != EntryKind::File
            || info.entry.size_bytes.0 != bytes.len() as u64
            || info.entry.parent_id != stored.remote_folder
            || info.entry.name
                != path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(invalid)?
            || info.entry.hash.as_ref().is_none_or(|h| {
                h.algorithm != HashAlgorithm::Gcid || !h.value.eq_ignore_ascii_case(&receipt.gcid)
            })
        {
            return Err(invalid());
        }
        let mut read = Vec::with_capacity(bytes.len());
        while read.len() < bytes.len() {
            let chunk = ctx
                .run(self.provider.download_chunk(
                    &receipt.remote_id,
                    &info.identity,
                    &stored.scope,
                    read.len() as u64,
                    ((bytes.len() - read.len()).min(4 * 1024 * 1024)) as u64,
                    nekopak_provider::DownloadObserver::default(),
                ))
                .await?;
            if chunk.total != bytes.len() as u64
                || chunk.bytes.is_empty()
                || read.len() + chunk.bytes.len() > bytes.len()
            {
                return Err(invalid());
            }
            read.extend_from_slice(&chunk.bytes);
        }
        if read != bytes || format!("{:x}", Sha256::digest(&read)) != stored.manifest.content_sha256
        {
            return Err(invalid());
        }
        self.check_scope(&stored.scope, ctx).await?;
        stored.manifest.remote_manifest_id = Some(receipt.remote_id);
        if matches!(stored.manifest.state.as_str(), "prepared" | "uploading") {
            stored.manifest.state = "verified".into();
        }
        self.save_reference(stored)
    }
    pub async fn reference_archive(
        &self,
        id: &str,
        confirm_trash: bool,
        key: &str,
        ctx: &RequestContext,
    ) -> Result<ReferenceManifest, ClientError> {
        if !confirm_trash {
            return Err(conflict("移入回收站前需要明确确认；归档清单会保留。"));
        }
        scoped_key("reference-archive", key)?;
        let _gate = ctx
            .run(async { Ok(self.reference_gate.lock().await) })
            .await?;
        let scope = ctx.run(self.provider.account_scope()).await?;
        let mut stored = self.load_reference(id, &scope)?;
        if stored
            .manifest
            .entries
            .iter()
            .any(|item| item.state == "restored")
        {
            return Err(conflict("该清单已执行取回，如需再次归档请创建新清单。"));
        }
        self.verify_reference_upload(&mut stored, ctx).await?;
        // Check the complete recorded selection before the first trash request.
        for item in stored
            .manifest
            .entries
            .iter()
            .filter(|i| i.entry.kind == EntryKind::File)
        {
            let current = ctx
                .run(self.provider.recovery_metadata(&item.entry.id, &scope))
                .await?;
            if !same_content(&item.entry, &current.entry, true) {
                return Err(invalid());
            }
        }
        for index in 0..stored.manifest.entries.len() {
            let item = stored.manifest.entries[index].clone();
            if item.entry.kind == EntryKind::Folder {
                continue;
            }
            self.check_scope(&scope, ctx).await?;
            let result = self
                .archive_reference_item(&item.entry, id, &scope, ctx)
                .await;
            match result {
                Ok(()) => {
                    stored.manifest.entries[index].state = "trashed".into();
                    stored.manifest.entries[index].error = None;
                }
                Err(error) => {
                    stored.manifest.entries[index].state =
                        if error.details.as_ref().is_some_and(|d| {
                            d.get("outcome_unknown") == Some(&serde_json::Value::Bool(true))
                        }) {
                            "outcome_unknown"
                        } else {
                            "failed"
                        }
                        .into();
                    stored.manifest.entries[index].error = Some(error);
                    stored.manifest.state = "partial".into();
                    self.save_reference(&stored)?;
                    return Ok(stored.manifest);
                }
            }
            stored.manifest.state = "partial".into();
            self.save_reference(&stored)?;
        }
        stored.manifest.state = "archived".into();
        self.save_reference(&stored)?;
        Ok(stored.manifest)
    }
    async fn archive_reference_item(
        &self,
        entry: &RemoteEntry,
        id: &str,
        scope: &str,
        ctx: &RequestContext,
    ) -> Result<(), ClientError> {
        let current = ctx
            .run(self.provider.recovery_metadata(&entry.id, scope))
            .await?;
        if !same_content(entry, &current.entry, true) {
            return Err(invalid());
        }
        if current.trashed {
            return Ok(());
        }
        let key = fingerprint(&["reference-trash", id, &entry.id]);
        let outcome = self
            .files_mutate_at(
                &FileMutation {
                    op: MutateOp::Trash,
                    ids: vec![entry.id.clone()],
                    target: None,
                },
                &key,
                scope,
                ctx,
            )
            .await?;
        if !outcome.failed.is_empty() {
            return Err(conflict("部分归档原文件未移入回收站。"));
        }
        let check = ctx
            .run(self.provider.recovery_metadata(&entry.id, scope))
            .await?;
        if !check.trashed || !same_content(entry, &check.entry, true) {
            return Err(conflict("原文件移入回收站尚未确认，请刷新核对。"));
        }
        Ok(())
    }
    pub async fn reference_restore(
        &self,
        id: &str,
        allow_recreate: bool,
        key: &str,
        ctx: &RequestContext,
    ) -> Result<ReferenceManifest, ClientError> {
        scoped_key("reference-restore", key)?;
        let _gate = ctx
            .run(async { Ok(self.reference_gate.lock().await) })
            .await?;
        let scope = ctx.run(self.provider.account_scope()).await?;
        let mut stored = self.load_reference(id, &scope)?;
        for index in 0..stored.manifest.entries.len() {
            let item = stored.manifest.entries[index].clone();
            if item.entry.kind == EntryKind::Folder {
                continue;
            }
            self.check_scope(&scope, ctx).await?;
            match self
                .restore_reference_item(&item, id, allow_recreate, &scope, ctx)
                .await
            {
                Ok(receipt) => {
                    stored.manifest.entries[index].current_id = Some(receipt.file_id);
                    stored.manifest.entries[index].state = if receipt.completed {
                        "restored"
                    } else {
                        "pending_upload"
                    }
                    .into();
                    stored.manifest.entries[index].error = None;
                }
                Err(error) => {
                    stored.manifest.entries[index].state =
                        if error.details.as_ref().is_some_and(|d| {
                            d.get("outcome_unknown") == Some(&serde_json::Value::Bool(true))
                        }) {
                            "outcome_unknown"
                        } else {
                            "failed"
                        }
                        .into();
                    stored.manifest.entries[index].error = Some(error);
                }
            }
            stored.manifest.state = "partial".into();
            self.save_reference(&stored)?;
        }
        if stored
            .manifest
            .entries
            .iter()
            .filter(|i| i.entry.kind == EntryKind::File)
            .all(|i| i.state == "restored")
        {
            stored.manifest.state = "restored".into();
        }
        self.save_reference(&stored)?;
        Ok(stored.manifest)
    }
    async fn restore_reference_item(
        &self,
        item: &ReferenceItem,
        manifest: &str,
        allow_recreate: bool,
        scope: &str,
        ctx: &RequestContext,
    ) -> Result<ContentCreateReceipt, ClientError> {
        let id = item.current_id.as_deref().unwrap_or(&item.entry.id);
        if item.state == "pending_upload" && item.current_id.is_some() {
            return match ctx.run(self.provider.recovery_metadata(id, scope)).await {
                Ok(metadata)
                    if !metadata.trashed && same_content(&item.entry, &metadata.entry, false) =>
                {
                    Ok(ContentCreateReceipt {
                        file_id: id.into(),
                        completed: true,
                    })
                }
                Ok(_) => Err(invalid()),
                Err(error)
                    if matches!(error.code, ErrorCode::IntegrityFailed | ErrorCode::NotFound) =>
                {
                    Ok(ContentCreateReceipt {
                        file_id: id.into(),
                        completed: false,
                    })
                }
                Err(error) => Err(error),
            };
        }
        match ctx.run(self.provider.recovery_metadata(id, scope)).await {
            Ok(metadata) => {
                if !same_content(&item.entry, &metadata.entry, id == item.entry.id) {
                    return Err(invalid());
                }
                if !metadata.trashed {
                    return Ok(ContentCreateReceipt {
                        file_id: id.into(),
                        completed: true,
                    });
                }
                self.check_reference_capacity(item.entry.size_bytes.0, scope, ctx)
                    .await?;
                self.check_reference_target(&item.entry, id, scope, ctx)
                    .await?;
                let key = fingerprint(&["reference-untrash", manifest, id]);
                let outcome = self
                    .files_mutate_at(
                        &FileMutation {
                            op: MutateOp::Restore,
                            ids: vec![id.into()],
                            target: None,
                        },
                        &key,
                        scope,
                        ctx,
                    )
                    .await?;
                if !outcome.failed.is_empty() {
                    return Err(conflict("回收站恢复尚未确认。"));
                }
                let check = ctx.run(self.provider.recovery_metadata(id, scope)).await?;
                if check.trashed || !same_content(&item.entry, &check.entry, id == item.entry.id) {
                    return Err(invalid());
                }
                Ok(ContentCreateReceipt {
                    file_id: id.into(),
                    completed: true,
                })
            }
            Err(error) if error.code == ErrorCode::NotFound && id == item.entry.id => {
                if !allow_recreate {
                    return Err(conflict(
                        "回收站原对象已不可用；如需GCID重建请单独明确允许。",
                    ));
                }
                let key = fingerprint(&["reference-recreate", manifest, &item.entry.id]);
                let fp = fingerprint(&[&encode(&item.entry)?]);
                let receipt = if let Some(value) = self.records()?.lookup_value(scope, &key, &fp)? {
                    serde_json::from_value::<ContentCreateReceipt>(value).map_err(|_| invalid())?
                } else {
                    self.check_reference_capacity(item.entry.size_bytes.0, scope, ctx)
                        .await?;
                    self.check_reference_target(&item.entry, "", scope, ctx)
                        .await?;
                    if self.records()?.begin_value(scope, &key, &fp)?.is_some() {
                        return Err(invalid());
                    }
                    let result = ctx
                        .run(self.provider.create_from_content(&item.entry, scope))
                        .await
                        .map_err(cloud::unknown_write);
                    self.records()?.finish_value(
                        scope,
                        &key,
                        &result
                            .as_ref()
                            .map(|v| serde_json::json!(v))
                            .map_err(Clone::clone),
                    )?;
                    result?
                };
                if !receipt.completed {
                    return Ok(receipt);
                }
                match ctx
                    .run(self.provider.recovery_metadata(&receipt.file_id, scope))
                    .await
                {
                    Ok(check)
                        if !check.trashed && same_content(&item.entry, &check.entry, false) =>
                    {
                        Ok(receipt)
                    }
                    // Keep the durable new ID even when visibility/verification has
                    // not completed; never create it again as a retry.
                    _ => Ok(ContentCreateReceipt {
                        file_id: receipt.file_id,
                        completed: false,
                    }),
                }
            }
            Err(error) => Err(error),
        }
    }
    async fn check_reference_capacity(
        &self,
        bytes: u64,
        scope: &str,
        ctx: &RequestContext,
    ) -> Result<(), ClientError> {
        let quota = ctx.run(self.provider.quota()).await?;
        self.check_scope(scope, ctx).await?;
        let remaining = quota
            .dimensions
            .iter()
            .find(|d| d.dimension == "storage")
            .and_then(|d| d.remaining_bytes)
            .ok_or_else(|| conflict("当前容量不可确认，未提交归档恢复。"))?;
        if remaining.0 < bytes {
            return Err(ClientError::new(
                ErrorCode::QuotaExceeded,
                "当前可用容量不足，未提交归档恢复。",
            ));
        }
        Ok(())
    }
    async fn check_reference_target(
        &self,
        entry: &RemoteEntry,
        allowed_id: &str,
        scope: &str,
        ctx: &RequestContext,
    ) -> Result<(), ClientError> {
        let parent = entry.parent_id.as_deref().ok_or_else(invalid)?;
        if parent != "root" {
            let folder = ctx.run(self.provider.stat(parent)).await?;
            if folder.kind != EntryKind::Folder {
                return Err(invalid());
            }
        }
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        loop {
            let page = ctx
                .run(self.provider.list(parent, cursor.as_deref(), 500))
                .await?;
            self.check_scope(scope, ctx).await?;
            if page
                .items
                .iter()
                .any(|e| e.id != allowed_id && e.name.to_lowercase() == entry.name.to_lowercase())
            {
                return Err(conflict("原目录已有同名文件，未覆盖或生成编号副本。"));
            }
            let Some(next) = page.next_cursor else {
                return Ok(());
            };
            if !seen.insert(next.clone()) || seen.len() > 1000 {
                return Err(invalid());
            }
            cursor = Some(next);
        }
    }
}
