use async_trait::async_trait;
use nekopak_core::AppService;
use nekopak_domain::*;
use nekopak_provider::{ProviderPort, RequestContext};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
struct Cloud {
    scope: Mutex<String>,
    creates: AtomicUsize,
    shares: AtomicUsize,
    fail: bool,
}
fn snapshot() -> CloudTaskSnapshot {
    CloudTaskSnapshot {
        id: "remote-task".into(),
        name: "Cloud download".into(),
        state: TaskState::Completed,
        progress: Some(100),
        file_id: Some("file".into()),
        total_bytes: None,
        created_at: None,
        error: None,
    }
}
#[async_trait]
impl ProviderPort for Cloud {
    fn id(&self) -> BackendId {
        BackendId::RustCompat
    }
    async fn discover(&self) -> Result<CapabilityReport, ClientError> {
        unreachable!()
    }
    async fn list(
        &self,
        _: &str,
        _: Option<&str>,
        _: usize,
    ) -> Result<Page<RemoteEntry>, ClientError> {
        unreachable!()
    }
    async fn stat(&self, _: &str) -> Result<RemoteEntry, ClientError> {
        unreachable!()
    }
    async fn quota(&self) -> Result<QuotaReport, ClientError> {
        unreachable!()
    }
    async fn account_scope(&self) -> Result<String, ClientError> {
        Ok(self.scope.lock().unwrap().clone())
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
    async fn cloud_create(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<CloudTaskSnapshot, ClientError> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(ClientError::new(
                ErrorCode::TransientNetwork,
                "synthetic lost response",
            ))
        } else {
            Ok(snapshot())
        }
    }
    async fn cloud_task(&self, _: &str, _: &str) -> Result<CloudTaskSnapshot, ClientError> {
        Ok(snapshot())
    }
    async fn share_create(
        &self,
        _: &CreateShareRequest,
        _: &str,
    ) -> Result<ShareInfo, ClientError> {
        self.shares.fetch_add(1, Ordering::SeqCst);
        Ok(ShareInfo {
            share_id: "share".into(),
            title: "one".into(),
            item_count: 1,
            url: "https://mypikpak.com/s/example".into(),
            code: None,
            created_at: "2026-11-08T00:00:00Z".into(),
            expires_at: None,
            state: "active".into(),
            expires_known: true,
            view_count: None,
            save_count: None,
        })
    }
}
fn provider(fail: bool) -> Arc<Cloud> {
    Arc::new(Cloud {
        scope: Mutex::new("one".into()),
        creates: AtomicUsize::new(0),
        shares: AtomicUsize::new(0),
        fail,
    })
}
#[tokio::test]
async fn cloud_unknown_total_completes_and_replay_survives_restart_without_recreating() {
    let dir = tempfile::tempdir().unwrap();
    let p = provider(false);
    let db = dir.path().join("tasks.db");
    let app = AppService::with_provider(&db, p.clone());
    let ctx = RequestContext::default();
    let task = app
        .cloud_task_create(
            "https://example.test/movie?secret=signed-value",
            "root",
            "once",
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Completed);
    assert_eq!(app.task(&task.task_id).unwrap().total_bytes, None);
    drop(app);
    let app = AppService::with_provider(&db, p.clone());
    assert_eq!(
        app.cloud_task_create(
            "https://example.test/movie?secret=signed-value",
            "root",
            "once",
            &ctx
        )
        .await
        .unwrap()
        .task_id,
        task.task_id
    );
    assert_eq!(p.creates.load(Ordering::SeqCst), 1);
    assert_eq!(
        app.visible_transfers(None, None, &ctx)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    *p.scope.lock().unwrap() = "two".into();
    assert!(app
        .visible_transfers(None, None, &ctx)
        .await
        .unwrap()
        .items
        .is_empty());
    drop(app);
    for file in std::fs::read_dir(dir.path()).unwrap() {
        let b = std::fs::read(file.unwrap().path()).unwrap();
        assert!(!String::from_utf8_lossy(&b).contains("signed-value"));
    }
}
#[tokio::test]
async fn lost_cloud_response_is_not_replayed_and_shares_use_durable_intents() {
    let dir = tempfile::tempdir().unwrap();
    let p = provider(true);
    let db = dir.path().join("tasks.db");
    let app = AppService::with_provider(&db, p.clone());
    let ctx = RequestContext::default();
    for _ in 0..2 {
        assert_eq!(
            app.cloud_task_create("https://example.test/a", "root", "once", &ctx)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
    }
    assert_eq!(p.creates.load(Ordering::SeqCst), 1);
    let request = CreateShareRequest {
        ids: vec!["one".into()],
        expires_in_days: 7,
        with_code: false,
    };
    app.share_create(&request, "same", &ctx).await.unwrap();
    drop(app);
    let app = AppService::with_provider(&db, p.clone());
    app.share_create(&request, "same", &ctx).await.unwrap();
    assert_eq!(p.shares.load(Ordering::SeqCst), 1);
}
