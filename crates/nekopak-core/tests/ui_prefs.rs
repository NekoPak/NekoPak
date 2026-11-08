use async_trait::async_trait;
use nekopak_core::AppService;
use nekopak_domain::*;
use nekopak_provider::{ProviderPort, RequestContext};
use std::sync::{Arc, Mutex};

/// A memory-only provider used to exercise account identity checks around the
/// persisted preference record. It deliberately has no network implementation.
struct MemoryAccounts {
    account: Mutex<String>,
}

impl MemoryAccounts {
    fn new(account: &str) -> Arc<Self> {
        Arc::new(Self {
            account: Mutex::new(account.into()),
        })
    }

    fn switch(&self, account: &str) {
        *self.account.lock().unwrap() = account.into();
    }
}

#[async_trait]
impl ProviderPort for MemoryAccounts {
    fn id(&self) -> BackendId {
        BackendId::RustCompat
    }

    async fn discover(&self) -> Result<CapabilityReport, ClientError> {
        unreachable!("the preference tests do not discover a backend")
    }

    async fn list(
        &self,
        _: &str,
        _: Option<&str>,
        _: usize,
    ) -> Result<Page<RemoteEntry>, ClientError> {
        unreachable!("the preference tests do not list files")
    }

    async fn stat(&self, _: &str) -> Result<RemoteEntry, ClientError> {
        unreachable!("the preference tests do not stat files")
    }

    async fn quota(&self) -> Result<QuotaReport, ClientError> {
        unreachable!("the preference tests do not read quota")
    }

    async fn account_scope(&self) -> Result<String, ClientError> {
        Ok(self.account.lock().unwrap().clone())
    }

    async fn account_scope_for(&self, id: &str) -> Result<String, ClientError> {
        let current = self.account.lock().unwrap().clone();
        if current != id {
            return Err(ClientError::new(
                ErrorCode::AuthExpired,
                "账号已改变，操作已停止。",
            ));
        }
        Ok(current)
    }

    async fn auth_status(&self) -> Result<AuthState, ClientError> {
        let account = self.account.lock().unwrap().clone();
        Ok(AuthState {
            backend: self.id(),
            authenticated: true,
            account_id: Some(account),
            credential_owner: "test".into(),
            account_tier: AccountTier::Unknown,
            message: String::new(),
            state: AuthStatus::SignedIn,
            display_name: None,
        })
    }
}

fn prefs(name: &str) -> serde_json::Value {
    serde_json::json!({
        "pins": [{"id": format!("folder-{name}"), "name": name}],
        "view": "grid",
        "sort": {"key": "name", "dir": "asc"},
        "density": "comfortable",
        "gridSize": "m",
        "foldersFirst": true,
    })
}

#[tokio::test]
async fn account_preferences_are_isolated_and_stale_update_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let provider = MemoryAccounts::new("account-a");
    let app = AppService::with_provider(&temp.path().join("tasks.db"), provider.clone());
    let ctx = RequestContext::default();

    app.ui_prefs_update(&prefs("Alice"), "account-a", &ctx)
        .await
        .unwrap();
    let initial = app.ui_prefs_get(&ctx).await.unwrap();
    assert_eq!(initial["account_id"], "account-a");
    assert_eq!(initial["prefs"]["pins"][0]["name"], "Alice");

    provider.switch("account-b");
    let fresh = app.ui_prefs_get(&ctx).await.unwrap();
    assert_eq!(fresh["account_id"], "account-b");
    assert_eq!(fresh["prefs"]["pins"].as_array().unwrap().len(), 0);

    let stale = app
        .ui_prefs_update(&prefs("stale-Alice"), "account-a", &ctx)
        .await
        .unwrap_err();
    assert_eq!(stale.code, ErrorCode::AuthExpired);

    app.ui_prefs_update(&prefs("Bob"), "account-b", &ctx)
        .await
        .unwrap();
    assert_eq!(
        app.ui_prefs_get(&ctx).await.unwrap()["prefs"]["pins"][0]["name"],
        "Bob"
    );

    provider.switch("account-a");
    let restored = app.ui_prefs_get(&ctx).await.unwrap();
    assert_eq!(restored["account_id"], "account-a");
    assert_eq!(restored["prefs"]["pins"][0]["name"], "Alice");
}

#[tokio::test]
async fn account_preferences_reject_unknown_account_without_creating_a_record() {
    let temp = tempfile::tempdir().unwrap();
    let provider = MemoryAccounts::new("account-a");
    let app = AppService::with_provider(&temp.path().join("tasks.db"), provider.clone());
    let ctx = RequestContext::default();

    let error = app
        .ui_prefs_update(&prefs("unknown"), "account-missing", &ctx)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::AuthExpired);
    assert_eq!(
        app.ui_prefs_get(&ctx).await.unwrap()["prefs"]["pins"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}
