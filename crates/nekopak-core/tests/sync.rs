use async_trait::async_trait;
use nekopak_core::AppService;
use nekopak_domain::*;
use nekopak_provider::{
    DownloadChunk, DownloadInfo, DownloadObserver, ProviderPort, RequestContext,
};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

struct Remote {
    files: Mutex<Vec<RemoteEntry>>,
    bytes: Mutex<Vec<u8>>,
    fail: AtomicBool,
    scope: Mutex<String>,
}
fn root() -> RemoteEntry {
    RemoteEntry {
        id: "root".into(),
        parent_id: None,
        name: "Drive".into(),
        kind: EntryKind::Folder,
        size_bytes: DecimalU64(0),
        modified_at: None,
        etag: None,
        hash: None,
        mime_type: None,
        starred: None,
    }
}
fn file(bytes: &[u8]) -> RemoteEntry {
    RemoteEntry {
        id: "one".into(),
        parent_id: Some("root".into()),
        name: "example.txt".into(),
        kind: EntryKind::File,
        size_bytes: DecimalU64(bytes.len() as u64),
        modified_at: Some("2026-11-08T00:00:00Z".into()),
        etag: Some(format!("{:x}", Sha256::digest(bytes))),
        hash: Some(ContentHash {
            algorithm: HashAlgorithm::Sha256,
            value: format!("{:x}", Sha256::digest(bytes)),
        }),
        mime_type: None,
        starred: None,
    }
}
#[async_trait]
impl ProviderPort for Remote {
    fn id(&self) -> BackendId {
        BackendId::RustCompat
    }
    fn supports_downloads(&self) -> bool {
        true
    }
    async fn account_scope(&self) -> Result<String, ClientError> {
        Ok(self.scope.lock().unwrap().clone())
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
        if self.fail.load(Ordering::SeqCst) {
            return Err(ClientError::new(
                ErrorCode::TransientNetwork,
                "Synthetic outage",
            ));
        }
        Ok(Page {
            items: self
                .files
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.parent_id.as_deref() == Some(parent))
                .cloned()
                .collect(),
            next_cursor: None,
        })
    }
    async fn stat(&self, id: &str) -> Result<RemoteEntry, ClientError> {
        if id == "root" {
            Ok(root())
        } else {
            self.files
                .lock()
                .unwrap()
                .iter()
                .find(|f| f.id == id)
                .cloned()
                .ok_or_else(|| ClientError::new(ErrorCode::NotFound, "missing"))
        }
    }
    async fn quota(&self) -> Result<QuotaReport, ClientError> {
        unreachable!()
    }
    async fn download_info(&self, id: &str) -> Result<DownloadInfo, ClientError> {
        let entry = self.stat(id).await?;
        Ok(DownloadInfo {
            identity: entry.etag.clone().unwrap(),
            entry,
            account_scope: self.account_scope().await?,
        })
    }
    async fn download_chunk(
        &self,
        _: &str,
        identity: &str,
        _: &str,
        start: u64,
        length: u64,
        observer: DownloadObserver,
    ) -> Result<DownloadChunk, ClientError> {
        let all = self.bytes.lock().unwrap().clone();
        assert_eq!(identity, format!("{:x}", Sha256::digest(&all)));
        let bytes = all[start as usize..(start + length).min(all.len() as u64) as usize].to_vec();
        observer.received(bytes.len() as u64);
        Ok(DownloadChunk {
            bytes,
            total: all.len() as u64,
        })
    }
    async fn auth_status(&self) -> Result<AuthState, ClientError> {
        Ok(AuthState {
            backend: self.id(),
            authenticated: true,
            account_id: None,
            credential_owner: "test".into(),
            account_tier: AccountTier::Unknown,
            message: String::new(),
            state: AuthStatus::SignedIn,
            display_name: None,
        })
    }
}
fn database(root: &Path) -> std::path::PathBuf {
    let state = root.join("state");
    std::fs::create_dir_all(&state).unwrap();
    state.join("tasks.db")
}
fn config(path: &Path) -> SyncConfig {
    SyncConfig {
        config_id: String::new(),
        name: "safe sync".into(),
        mode: SyncMode::DownloadOnly,
        local_path: path.to_string_lossy().into_owned(),
        remote_id: "root".into(),
        remote_path: "/".into(),
        exclude_globs: vec!["*.tmp".into()],
        allow_deletes: false,
        first_run: true,
        auto_enabled: false,
        interval_seconds: 300,
        last_applied_at: None,
    }
}
fn provider() -> Arc<Remote> {
    Arc::new(Remote {
        files: Mutex::new(vec![file(b"version-one")]),
        bytes: Mutex::new(b"version-one".to_vec()),
        fail: AtomicBool::new(false),
        scope: Mutex::new("account-a".into()),
    })
}
async fn finish(app: &AppService, job: &str, ctx: &RequestContext) {
    for _ in 0..30 {
        app.sync_reconcile(ctx).await.unwrap();
        app.tick().await.unwrap();
        let state = app.sync_run_status(job, ctx).await.unwrap();
        if state["state"] == "completed" {
            return;
        }
        assert_ne!(state["state"], "failed", "{state}");
    }
    panic!("sync did not complete")
}

#[tokio::test]
async fn download_sync_waits_for_real_transfer_then_persists_baseline_and_backs_up_updates() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("local");
    std::fs::create_dir(&local).unwrap();
    let remote = provider();
    let db = database(dir.path());
    let mut app = AppService::with_provider(&db, remote.clone());
    let ctx = RequestContext::default();
    let cfg = app.sync_config_save(&config(&local), &ctx).await.unwrap();
    app.sync_scan(&cfg.config_id, &ctx).await.unwrap();
    let plan = app.sync_plan(&cfg.config_id, &ctx).await.unwrap();
    assert_eq!(plan.actions.len(), 1);
    assert!(!plan.has_deletes);
    let job = app.sync_apply(&plan.plan_id, false, &ctx).await.unwrap();
    assert_eq!(
        app.sync_apply(&plan.plan_id, false, &ctx)
            .await
            .unwrap()
            .task_id,
        job.task_id
    );
    app.sync_reconcile(&ctx).await.unwrap();
    assert!(app.sync_configs_list(&ctx).await.unwrap()[0].first_run);
    finish(&app, &job.task_id, &ctx).await;
    assert_eq!(
        std::fs::read(local.join("example.txt")).unwrap(),
        b"version-one"
    );
    assert!(!app.sync_configs_list(&ctx).await.unwrap()[0].first_run);
    *remote.bytes.lock().unwrap() = b"version-two".to_vec();
    *remote.files.lock().unwrap() = vec![file(b"version-two")];
    app.sync_scan(&cfg.config_id, &ctx).await.unwrap();
    let plan = app.sync_plan(&cfg.config_id, &ctx).await.unwrap();
    assert!(!plan.has_unresolved_conflicts);
    let job = app.sync_apply(&plan.plan_id, false, &ctx).await.unwrap();
    app.sync_reconcile(&ctx).await.unwrap();
    // Crash window: child committed, aggregate still has its original step.
    for _ in 0..8 {
        app.tick().await.unwrap();
    }
    drop(app);
    app = AppService::with_provider(&db, remote.clone());
    finish(&app, &job.task_id, &ctx).await;
    assert_eq!(
        std::fs::read(local.join("example.txt")).unwrap(),
        b"version-two"
    );
    assert_eq!(
        std::fs::read(
            local
                .join(".nekopak-backup")
                .join(&job.task_id)
                .join("example.txt")
        )
        .unwrap(),
        b"version-one"
    );
    drop(app);
    let app = AppService::with_provider(&db, remote.clone());
    app.sync_scan(&cfg.config_id, &ctx).await.unwrap();
    assert!(app
        .sync_plan(&cfg.config_id, &ctx)
        .await
        .unwrap()
        .actions
        .is_empty());
    *remote.scope.lock().unwrap() = "account-b".into();
    assert!(app.sync_configs_list(&ctx).await.unwrap().is_empty());
    assert!(app.sync_run_status(&job.task_id, &ctx).await.is_err());
}

#[tokio::test]
async fn scan_failure_and_late_local_change_never_apply_or_delete() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("local");
    std::fs::create_dir(&local).unwrap();
    let remote = provider();
    let app = AppService::with_provider(&database(dir.path()), remote.clone());
    let ctx = RequestContext::default();
    let cfg = app.sync_config_save(&config(&local), &ctx).await.unwrap();
    remote.fail.store(true, Ordering::SeqCst);
    assert!(app.sync_scan(&cfg.config_id, &ctx).await.is_err());
    assert!(app.sync_plan(&cfg.config_id, &ctx).await.is_err());
    remote.fail.store(false, Ordering::SeqCst);
    app.sync_scan(&cfg.config_id, &ctx).await.unwrap();
    let plan = app.sync_plan(&cfg.config_id, &ctx).await.unwrap();
    std::fs::write(local.join("example.txt"), b"local-user-change").unwrap();
    assert_eq!(
        app.sync_apply(&plan.plan_id, false, &ctx)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        std::fs::read(local.join("example.txt")).unwrap(),
        b"local-user-change"
    );
    app.sync_scan(&cfg.config_id, &ctx).await.unwrap();
    let plan = app.sync_plan(&cfg.config_id, &ctx).await.unwrap();
    assert!(plan.has_unresolved_conflicts);
    assert!(!plan.has_deletes);
    assert!(app.sync_apply(&plan.plan_id, false, &ctx).await.is_err());
}

#[tokio::test]
async fn opt_in_remote_deletion_moves_local_file_to_backup_only_after_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("local");
    std::fs::create_dir(&local).unwrap();
    let remote = provider();
    let app = AppService::with_provider(&database(dir.path()), remote.clone());
    let ctx = RequestContext::default();
    let cfg = app.sync_config_save(&config(&local), &ctx).await.unwrap();
    app.sync_scan(&cfg.config_id, &ctx).await.unwrap();
    let plan = app.sync_plan(&cfg.config_id, &ctx).await.unwrap();
    let job = app.sync_apply(&plan.plan_id, false, &ctx).await.unwrap();
    finish(&app, &job.task_id, &ctx).await;
    let mut cfg = app.sync_configs_list(&ctx).await.unwrap().remove(0);
    cfg.allow_deletes = true;
    app.sync_config_save(&cfg, &ctx).await.unwrap();
    remote.files.lock().unwrap().clear();
    app.sync_scan(&cfg.config_id, &ctx).await.unwrap();
    let plan = app.sync_plan(&cfg.config_id, &ctx).await.unwrap();
    assert!(plan.has_deletes);
    assert!(app.sync_apply(&plan.plan_id, false, &ctx).await.is_err());
    let job = app.sync_apply(&plan.plan_id, true, &ctx).await.unwrap();
    finish(&app, &job.task_id, &ctx).await;
    assert!(!local.join("example.txt").exists());
    assert_eq!(
        std::fs::read(
            local
                .join(".nekopak-backup")
                .join(job.task_id)
                .join("example.txt")
        )
        .unwrap(),
        b"version-one"
    );
}

#[tokio::test]
async fn automatic_sync_is_opt_in_spaced_and_never_executes_deletion_plans() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("local");
    std::fs::create_dir(&local).unwrap();
    let remote = provider();
    let app = AppService::with_provider(&database(dir.path()), remote.clone());
    let ctx = RequestContext::default();
    let mut cfg = app.sync_config_save(&config(&local), &ctx).await.unwrap();
    assert!(app.sync_automatic(&ctx).await.unwrap().is_empty());
    assert!(app.sync_plan(&cfg.config_id, &ctx).await.is_err());
    cfg.auto_enabled = true;
    cfg.interval_seconds = 60;
    app.sync_config_save(&cfg, &ctx).await.unwrap();
    let events = app.sync_automatic(&ctx).await.unwrap();
    assert_eq!(events.len(), 1);
    let id = events[0]["job_id"].as_str().unwrap();
    assert!(app.sync_automatic(&ctx).await.unwrap().is_empty());
    finish(&app, id, &ctx).await;
    let mut cfg = app.sync_configs_list(&ctx).await.unwrap().remove(0);
    cfg.allow_deletes = true;
    app.sync_config_save(&cfg, &ctx).await.unwrap();
    remote.files.lock().unwrap().clear();
    let events = app.sync_automatic(&ctx).await.unwrap();
    assert_eq!(events[0]["state"], "needs_review");
    assert!(local.join("example.txt").is_file());
}
