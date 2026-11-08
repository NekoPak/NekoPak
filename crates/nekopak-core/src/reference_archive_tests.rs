use super::*;
use async_trait::async_trait;
use nekopak_provider::{DownloadChunk, DownloadInfo, DownloadObserver};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};

struct MemoryCloud {
    files: Mutex<BTreeMap<String, RecoveryMetadata>>,
    content: Mutex<Vec<u8>>,
    scope: Mutex<String>,
    trash_calls: AtomicUsize,
    restore_calls: AtomicUsize,
    create_calls: AtomicUsize,
    pending: AtomicBool,
    lost_create: AtomicBool,
    corrupt_bytes: AtomicBool,
}
fn entry(id: &str, parent: &str, name: &str, kind: EntryKind, bytes: &[u8]) -> RemoteEntry {
    RemoteEntry {
        id: id.into(),
        parent_id: Some(parent.into()),
        name: name.into(),
        kind,
        size_bytes: DecimalU64(bytes.len() as u64),
        modified_at: None,
        etag: None,
        hash: (kind == EntryKind::File).then(|| ContentHash {
            algorithm: HashAlgorithm::Gcid,
            value: gcid(bytes),
        }),
        mime_type: None,
        starred: None,
    }
}
impl MemoryCloud {
    fn new() -> Arc<Self> {
        let folder = entry("folder", "root", "Original", EntryKind::Folder, b"");
        let file = entry(
            "original",
            "folder",
            "original.bin",
            EntryKind::File,
            b"content",
        );
        Arc::new(Self {
            files: Mutex::new(
                [
                    (
                        folder.id.clone(),
                        RecoveryMetadata {
                            entry: folder,
                            trashed: false,
                        },
                    ),
                    (
                        file.id.clone(),
                        RecoveryMetadata {
                            entry: file,
                            trashed: false,
                        },
                    ),
                ]
                .into(),
            ),
            content: Mutex::new(vec![]),
            scope: Mutex::new("scope".into()),
            trash_calls: AtomicUsize::new(0),
            restore_calls: AtomicUsize::new(0),
            create_calls: AtomicUsize::new(0),
            pending: AtomicBool::new(false),
            lost_create: AtomicBool::new(false),
            corrupt_bytes: AtomicBool::new(false),
        })
    }
    fn add_late_file(&self) {
        let file = entry("late", "folder", "late.bin", EntryKind::File, b"new");
        self.files.lock().unwrap().insert(
            file.id.clone(),
            RecoveryMetadata {
                entry: file,
                trashed: false,
            },
        );
    }
}
#[async_trait]
impl ProviderPort for MemoryCloud {
    fn id(&self) -> BackendId {
        BackendId::RustCompat
    }
    async fn account_scope(&self) -> Result<String, ClientError> {
        Ok(self.scope.lock().unwrap().clone())
    }
    async fn account_scope_for(&self, id: &str) -> Result<String, ClientError> {
        if id != "account" {
            return Err(invalid());
        }
        self.account_scope().await
    }
    async fn auth_status(&self) -> Result<AuthState, ClientError> {
        Ok(AuthState {
            backend: self.id(),
            authenticated: true,
            account_id: Some("account".into()),
            credential_owner: "test".into(),
            account_tier: AccountTier::Unknown,
            message: String::new(),
            state: AuthStatus::SignedIn,
            display_name: None,
        })
    }
    async fn discover(&self) -> Result<CapabilityReport, ClientError> {
        unreachable!()
    }
    async fn list(
        &self,
        parent: &str,
        _: Option<&str>,
        _: usize,
    ) -> Result<Page<RemoteEntry>, ClientError> {
        Ok(Page {
            items: self
                .files
                .lock()
                .unwrap()
                .values()
                .filter(|m| !m.trashed && m.entry.parent_id.as_deref() == Some(parent))
                .map(|m| m.entry.clone())
                .collect(),
            next_cursor: None,
        })
    }
    async fn stat(&self, id: &str) -> Result<RemoteEntry, ClientError> {
        self.files
            .lock()
            .unwrap()
            .get(id)
            .filter(|m| !m.trashed)
            .map(|m| m.entry.clone())
            .ok_or_else(|| ClientError::new(ErrorCode::NotFound, "gone"))
    }
    async fn quota(&self) -> Result<QuotaReport, ClientError> {
        Ok(QuotaReport {
            backend: self.id(),
            account_tier: AccountTier::Unknown,
            source: "test".into(),
            dimensions: vec![QuotaDimension {
                dimension: "storage".into(),
                period: "total".into(),
                used_bytes: None,
                limit_bytes: Some(DecimalU64(1_000_000)),
                remaining_bytes: Some(DecimalU64(900_000)),
                resets_at: None,
            }],
            checked_at: now(),
            limitations: vec![],
        })
    }
    async fn recovery_metadata(
        &self,
        id: &str,
        scope: &str,
    ) -> Result<RecoveryMetadata, ClientError> {
        if *self.scope.lock().unwrap() != scope {
            return Err(ClientError::new(ErrorCode::AuthExpired, "changed"));
        }
        if id == "recreated" && self.pending.load(Ordering::SeqCst) {
            return Err(invalid());
        }
        self.files
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| ClientError::new(ErrorCode::NotFound, "gone"))
    }
    async fn mutate_files(
        &self,
        request: &FileMutation,
        scope: &str,
    ) -> Result<OperationResult, ClientError> {
        assert_eq!(scope, self.scope.lock().unwrap().as_str());
        let mut created = vec![];
        match request.op {
            MutateOp::Mkdir => {
                let t = request.target.as_ref().unwrap();
                let folder = entry(
                    "manifests",
                    t.parent_id.as_deref().unwrap(),
                    t.name.as_deref().unwrap(),
                    EntryKind::Folder,
                    b"",
                );
                self.files.lock().unwrap().insert(
                    folder.id.clone(),
                    RecoveryMetadata {
                        entry: folder.clone(),
                        trashed: false,
                    },
                );
                created.push(folder);
            }
            MutateOp::Trash | MutateOp::Restore => {
                let mut files = self.files.lock().unwrap();
                for id in &request.ids {
                    let file = files.get_mut(id).unwrap();
                    assert_eq!(file.entry.kind, EntryKind::File);
                    file.trashed = request.op == MutateOp::Trash;
                }
                if request.op == MutateOp::Trash {
                    self.trash_calls.fetch_add(1, Ordering::SeqCst);
                } else {
                    self.restore_calls.fetch_add(1, Ordering::SeqCst);
                }
            }
            _ => panic!("unexpected destructive mutation"),
        }
        Ok(OperationResult {
            op: request.op,
            succeeded: request.ids.clone(),
            failed: vec![],
            created,
            replayed: false,
        })
    }
    async fn upload_prepare(
        &self,
        path: &str,
        parent: &str,
        scope: &str,
    ) -> Result<UploadPrepared, ClientError> {
        assert_eq!(scope, self.scope.lock().unwrap().as_str());
        let bytes = std::fs::read(path).unwrap();
        let file = entry(
            "uploaded-manifest",
            parent,
            Path::new(path).file_name().unwrap().to_str().unwrap(),
            EntryKind::File,
            &bytes,
        );
        let response = UploadPrepared {
            upload_ref: String::new(),
            remote_id: file.id.clone(),
            total_bytes: file.size_bytes,
            gcid: file.hash.as_ref().unwrap().value.clone(),
            completed: true,
            part_bytes: 8 * 1024 * 1024,
        };
        *self.content.lock().unwrap() = bytes;
        self.files.lock().unwrap().insert(
            file.id.clone(),
            RecoveryMetadata {
                entry: file,
                trashed: false,
            },
        );
        Ok(response)
    }
    async fn download_info(&self, id: &str) -> Result<DownloadInfo, ClientError> {
        Ok(DownloadInfo {
            entry: self.stat(id).await?,
            identity: "manifest-version".into(),
            account_scope: self.account_scope().await?,
        })
    }
    async fn download_chunk(
        &self,
        id: &str,
        _: &str,
        scope: &str,
        start: u64,
        length: u64,
        _: DownloadObserver,
    ) -> Result<DownloadChunk, ClientError> {
        assert_eq!(id, "uploaded-manifest");
        assert_eq!(scope, self.scope.lock().unwrap().as_str());
        let bytes = self.content.lock().unwrap();
        let mut result = bytes[start as usize..(start + length) as usize].to_vec();
        if self.corrupt_bytes.load(Ordering::SeqCst) && !result.is_empty() {
            result[0] ^= 1;
        }
        Ok(DownloadChunk {
            bytes: result,
            total: bytes.len() as u64,
        })
    }
    async fn create_from_content(
        &self,
        expected: &RemoteEntry,
        scope: &str,
    ) -> Result<ContentCreateReceipt, ClientError> {
        assert_eq!(scope, self.scope.lock().unwrap().as_str());
        self.create_calls.fetch_add(1, Ordering::SeqCst);
        if self.lost_create.load(Ordering::SeqCst) {
            return Err(ClientError::new(
                ErrorCode::TransientNetwork,
                "lost response",
            ));
        }
        let mut recreated = expected.clone();
        recreated.id = "recreated".into();
        self.files.lock().unwrap().insert(
            recreated.id.clone(),
            RecoveryMetadata {
                entry: recreated,
                trashed: false,
            },
        );
        Ok(ContentCreateReceipt {
            file_id: "recreated".into(),
            completed: !self.pending.load(Ordering::SeqCst),
        })
    }
}
async fn prepare(app: &AppService) -> ReferenceManifest {
    app.reference_create(&["folder".into()], "plan", &RequestContext::default())
        .await
        .unwrap()
}
async fn upload(app: &AppService, id: &str) {
    app.reference_store_manifest(id, "store", &RequestContext::default())
        .await
        .unwrap();
    app.run_upload().await.unwrap();
    app.run_upload().await.unwrap();
}

#[tokio::test]
async fn verified_cloud_manifest_precedes_file_only_trash_and_restore() {
    let dir = tempfile::tempdir().unwrap();
    let p = MemoryCloud::new();
    let app = AppService::with_provider(&dir.path().join("tasks.db"), p.clone());
    let ctx = RequestContext::default();
    let manifest = prepare(&app).await;
    assert_eq!(manifest.entries.len(), 2);
    assert_eq!(p.trash_calls.load(Ordering::SeqCst), 0);
    assert!(app
        .reference_archive(&manifest.manifest_id, true, "archive", &ctx)
        .await
        .is_err());
    assert_eq!(p.trash_calls.load(Ordering::SeqCst), 0);
    upload(&app, &manifest.manifest_id).await;
    p.add_late_file();
    assert_eq!(
        app.reference_status(&manifest.manifest_id, &ctx)
            .await
            .unwrap()
            .state,
        "verified"
    );
    assert!(app
        .reference_archive(&manifest.manifest_id, false, "archive", &ctx)
        .await
        .is_err());
    let archived = app
        .reference_archive(&manifest.manifest_id, true, "archive", &ctx)
        .await
        .unwrap();
    assert_eq!(archived.state, "archived");
    assert_eq!(p.trash_calls.load(Ordering::SeqCst), 1);
    assert!(!p.files.lock().unwrap()["folder"].trashed);
    assert!(!p.files.lock().unwrap()["late"].trashed);
    let restored = app
        .reference_restore(&manifest.manifest_id, false, "restore", &ctx)
        .await
        .unwrap();
    assert_eq!(restored.state, "restored");
    assert_eq!(p.restore_calls.load(Ordering::SeqCst), 1);
    app.reference_restore(&manifest.manifest_id, false, "retry", &ctx)
        .await
        .unwrap();
    assert_eq!(p.restore_calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn corrupt_uploaded_bytes_and_changed_original_never_trash() {
    let dir = tempfile::tempdir().unwrap();
    let p = MemoryCloud::new();
    let app = AppService::with_provider(&dir.path().join("tasks.db"), p.clone());
    let ctx = RequestContext::default();
    let manifest = prepare(&app).await;
    upload(&app, &manifest.manifest_id).await;
    p.corrupt_bytes.store(true, Ordering::SeqCst);
    assert!(app
        .reference_archive(&manifest.manifest_id, true, "archive", &ctx)
        .await
        .is_err());
    assert_eq!(p.trash_calls.load(Ordering::SeqCst), 0);
    p.corrupt_bytes.store(false, Ordering::SeqCst);
    p.files
        .lock()
        .unwrap()
        .get_mut("original")
        .unwrap()
        .entry
        .name = "changed.bin".into();
    assert!(app
        .reference_archive(&manifest.manifest_id, true, "archive", &ctx)
        .await
        .is_err());
    assert_eq!(p.trash_calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn explicit_recreate_persists_pending_id_and_unknown_results_are_never_repeated() {
    let dir = tempfile::tempdir().unwrap();
    let p = MemoryCloud::new();
    let app = AppService::with_provider(&dir.path().join("tasks.db"), p.clone());
    let ctx = RequestContext::default();
    let manifest = prepare(&app).await;
    p.files.lock().unwrap().remove("original");
    let denied = app
        .reference_restore(&manifest.manifest_id, false, "deny", &ctx)
        .await
        .unwrap();
    assert_eq!(denied.state, "partial");
    assert_eq!(p.create_calls.load(Ordering::SeqCst), 0);
    p.pending.store(true, Ordering::SeqCst);
    let pending = app
        .reference_restore(&manifest.manifest_id, true, "allow", &ctx)
        .await
        .unwrap();
    let item = pending
        .entries
        .iter()
        .find(|i| i.entry.kind == EntryKind::File)
        .unwrap();
    assert_eq!(item.state, "pending_upload");
    assert_eq!(item.current_id.as_deref(), Some("recreated"));
    app.reference_restore(&manifest.manifest_id, true, "retry", &ctx)
        .await
        .unwrap();
    assert_eq!(p.create_calls.load(Ordering::SeqCst), 1);
    let dir2 = tempfile::tempdir().unwrap();
    let p2 = MemoryCloud::new();
    let app2 = AppService::with_provider(&dir2.path().join("tasks.db"), p2.clone());
    let m2 = prepare(&app2).await;
    p2.files.lock().unwrap().remove("original");
    p2.lost_create.store(true, Ordering::SeqCst);
    app2.reference_restore(&m2.manifest_id, true, "first", &ctx)
        .await
        .unwrap();
    app2.reference_restore(&m2.manifest_id, true, "second", &ctx)
        .await
        .unwrap();
    assert_eq!(p2.create_calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn missing_hash_and_another_account_are_rejected_without_replacing_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let p = MemoryCloud::new();
    let app = AppService::with_provider(&dir.path().join("tasks.db"), p.clone());
    let ctx = RequestContext::default();
    let manifest = prepare(&app).await;
    p.files
        .lock()
        .unwrap()
        .get_mut("original")
        .unwrap()
        .entry
        .hash = None;
    assert!(app
        .reference_create(&["original".into()], "bad", &ctx)
        .await
        .is_err());
    *p.scope.lock().unwrap() = "another".into();
    assert!(app
        .reference_status(&manifest.manifest_id, &ctx)
        .await
        .is_err());
    assert_eq!(p.trash_calls.load(Ordering::SeqCst), 0);
    *p.scope.lock().unwrap() = "scope".into();
    assert_eq!(app.reference_list(&ctx).await.unwrap().len(), 1);
}

#[tokio::test]
async fn persisted_manifest_recovers_after_restart_and_corruption_is_not_reset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let p = MemoryCloud::new();
    let app = AppService::with_provider(&path, p.clone());
    let ctx = RequestContext::default();
    let manifest = prepare(&app).await;
    upload(&app, &manifest.manifest_id).await;
    drop(app);
    let app = AppService::with_provider(&path, p.clone());
    assert_eq!(
        app.reference_status(&manifest.manifest_id, &ctx)
            .await
            .unwrap()
            .state,
        "verified"
    );
    let mut stored = app.load_reference(&manifest.manifest_id, "scope").unwrap();
    stored.manifest.entries[1].entry.name = "tampered".into();
    let raw = encode(&stored).unwrap();
    app.records()
        .unwrap()
        .record_put("scope", "references", &manifest.manifest_id, &raw)
        .unwrap();
    assert_eq!(
        app.reference_archive(&manifest.manifest_id, true, "archive", &ctx)
            .await
            .unwrap_err()
            .code,
        ErrorCode::IntegrityFailed
    );
    assert_eq!(p.trash_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        app.records()
            .unwrap()
            .record_get("scope", "references", &manifest.manifest_id)
            .unwrap()
            .unwrap(),
        raw
    );
}

#[tokio::test]
async fn recorded_creation_receipt_recovers_before_name_conflict_after_crash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let p = MemoryCloud::new();
    let app = AppService::with_provider(&path, p.clone());
    let ctx = RequestContext::default();
    let manifest = prepare(&app).await;
    let original = manifest
        .entries
        .iter()
        .find(|i| i.entry.kind == EntryKind::File)
        .unwrap()
        .entry
        .clone();
    p.files.lock().unwrap().remove(&original.id);
    let key = fingerprint(&["reference-recreate", &manifest.manifest_id, &original.id]);
    let fp = fingerprint(&[&encode(&original).unwrap()]);
    assert!(app
        .records()
        .unwrap()
        .begin_value("scope", &key, &fp)
        .unwrap()
        .is_none());
    let receipt = p.create_from_content(&original, "scope").await.unwrap();
    app.records()
        .unwrap()
        .finish_value("scope", &key, &Ok(serde_json::json!(receipt)))
        .unwrap();
    // Crash point: remote result is durable but manifest has no current_id yet.
    drop(app);
    let app = AppService::with_provider(&path, p.clone());
    let restored = app
        .reference_restore(&manifest.manifest_id, true, "retry", &ctx)
        .await
        .unwrap();
    assert_eq!(restored.state, "restored");
    assert_eq!(p.create_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        restored
            .entries
            .iter()
            .find(|i| i.entry.kind == EntryKind::File)
            .unwrap()
            .current_id
            .as_deref(),
        Some("recreated")
    );
}
