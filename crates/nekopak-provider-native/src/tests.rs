use super::*;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex as StdMutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

#[derive(Default)]
struct MemoryVault {
    value: StdMutex<Option<Zeroizing<String>>>,
    fail_save: AtomicBool,
    fail_base_save: AtomicBool,
    fail_delete: AtomicBool,
    accounts_index: StdMutex<Option<Zeroizing<String>>>,
    accounts: StdMutex<BTreeMap<String, Zeroizing<String>>>,
    auxiliary: StdMutex<BTreeMap<String, Zeroizing<String>>>,
}
impl SessionVault for MemoryVault {
    fn load_accounts(&self) -> Result<Option<Zeroizing<String>>, ClientError> {
        Ok(self.accounts_index.lock().unwrap().clone())
    }
    fn save_accounts(&self, value: &str) -> Result<(), ClientError> {
        if self.fail_save.load(Ordering::SeqCst) {
            return Err(error(ErrorCode::PermissionDenied, "test vault unavailable"));
        }
        *self.accounts_index.lock().unwrap() = Some(Zeroizing::new(value.into()));
        Ok(())
    }
    fn load_account(&self, id: &str) -> Result<Option<Zeroizing<String>>, ClientError> {
        Ok(self.accounts.lock().unwrap().get(id).cloned())
    }
    fn save_account(&self, id: &str, value: &str) -> Result<(), ClientError> {
        if self.fail_save.load(Ordering::SeqCst) {
            return Err(error(ErrorCode::PermissionDenied, "test vault unavailable"));
        }
        self.accounts
            .lock()
            .unwrap()
            .insert(id.into(), Zeroizing::new(value.into()));
        Ok(())
    }
    fn delete_account(&self, id: &str) -> Result<(), ClientError> {
        self.accounts.lock().unwrap().remove(id);
        Ok(())
    }
    fn aux_keys(&self) -> Result<Vec<String>, ClientError> {
        Ok(self.auxiliary.lock().unwrap().keys().cloned().collect())
    }
    fn load_aux(&self, key: &str) -> Result<Option<Zeroizing<String>>, ClientError> {
        Ok(self.auxiliary.lock().unwrap().get(key).cloned())
    }
    fn save_aux(&self, key: &str, value: &str) -> Result<(), ClientError> {
        self.auxiliary
            .lock()
            .unwrap()
            .insert(key.into(), Zeroizing::new(value.into()));
        Ok(())
    }
    fn delete_aux(&self, key: &str) -> Result<(), ClientError> {
        self.auxiliary.lock().unwrap().remove(key);
        Ok(())
    }
    fn load(&self) -> Result<Option<Zeroizing<String>>, ClientError> {
        Ok(self.value.lock().unwrap().clone())
    }
    fn save(&self, value: &str) -> Result<(), ClientError> {
        if self.fail_save.load(Ordering::SeqCst) || self.fail_base_save.load(Ordering::SeqCst) {
            return Err(error(ErrorCode::PermissionDenied, "test vault unavailable"));
        }
        *self.value.lock().unwrap() = Some(Zeroizing::new(value.into()));
        Ok(())
    }
    fn delete(&self) -> Result<(), ClientError> {
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(error(ErrorCode::PermissionDenied, "test vault unavailable"));
        }
        *self.value.lock().unwrap() = None;
        Ok(())
    }
}
struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Value,
}
struct Reply {
    status: u16,
    body: String,
    headers: String,
    delay: Duration,
}
fn reply(status: u16, body: Value) -> Reply {
    Reply {
        status,
        body: body.to_string(),
        headers: String::new(),
        delay: Duration::ZERO,
    }
}
fn tokens(access: &str) -> Value {
    json!({"access_token":access,"refresh_token":"test-refresh","token_type":"Bearer","expires_in":3600,"sub":"test-sub"})
}
fn file(id: &str) -> Value {
    json!({"id":id,"parent_id":"","name":"文档😀.mp4","kind":"drive#file","size":"18446744073709551615",
        "modified_time":"2026-11-08T10:00:00+08:00","phase":"PHASE_TYPE_COMPLETE","trashed":false,
        "web_content_link":"https://private.example/secret?token=not-for-ipc", "hash":"untyped-gcid",
        "medias":[{"link":{"url":"https://private.example/video"}}]})
}
fn standard(request: &Request) -> Reply {
    match request.path.split('?').next().unwrap() {
        "/v1/shield/captcha/init" => reply(200, json!({"captcha_token":"test-captcha"})),
        "/v1/auth/signin" => reply(200, tokens("test-access")),
        "/v1/auth/token" => reply(200, tokens("refreshed-access")),
        "/drive/v1/files" => reply(200, json!({"files":[file("file-1")]})),
        "/drive/v1/files/file-1" => reply(200, file("file-1")),
        _ => reply(404, json!({"error":"file_not_found"})),
    }
}
struct Server {
    origin: String,
    requests: Arc<StdMutex<Vec<Request>>>,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn new(handler: impl Fn(&Request) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let seen = requests.clone();
        let handler = Arc::new(handler);
        let task = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let handler = handler.clone();
                let seen = seen.clone();
                children.spawn(async move {
                    let mut bytes = Vec::new();
                    let end = loop {
                        let mut buffer = [0; 4096];
                        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer)).await.unwrap().unwrap();
                        if n == 0 { return; }
                        bytes.extend_from_slice(&buffer[..n]);
                        assert!(bytes.len() < 65536);
                        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") { break end + 4; }
                    };
                    let text = std::str::from_utf8(&bytes[..end]).unwrap();
                    let mut lines = text.lines();
                    let mut first = lines.next().unwrap().split_whitespace();
                    let method = first.next().unwrap().to_owned();
                    let path = first.next().unwrap().to_owned();
                    let headers: HashMap<String, String> = lines.filter_map(|l| l.split_once(':'))
                        .map(|(k,v)| (k.to_ascii_lowercase(),v.trim().to_owned())).collect();
                    let length: usize = headers.get("content-length").map(|s| s.parse().unwrap()).unwrap_or(0);
                    assert!(length < 32768);
                    while bytes.len() < end + length {
                        let mut buffer = [0;4096];
                        let n = stream.read(&mut buffer).await.unwrap();
                        if n == 0 { return; }
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                    let request = Request { method, path, headers, body: if length == 0 { Value::Null } else { serde_json::from_slice(&bytes[end..end+length]).unwrap() } };
                    let response = handler(&request);
                    seen.lock().unwrap().push(request);
                    tokio::time::sleep(response.delay).await;
                    let output = format!("HTTP/1.1 {} Test\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n{}\r\n{}",
                        response.status, response.body.len(), response.headers, response.body);
                    let _ = stream.write_all(output.as_bytes()).await;
                });
            }
        });
        Self {
            origin,
            requests,
            task,
        }
    }
    fn provider(&self) -> (Arc<NativeProvider>, Arc<MemoryVault>) {
        let vault = Arc::new(MemoryVault::default());
        let (auth_events, _) = broadcast::channel(32);
        (
            Arc::new(NativeProvider {
                read_cache: Default::default(),
                profile: Some(ClientProfile {
                    id: profile::WEB_CLIENT_ID.into(),
                    secret: None,
                }),
                http: HttpApi::loopback(&self.origin),
                vault: vault.clone(),
                vault_supported: true,
                session: Mutex::new(Session::default()),
                auth_events,
            }),
            vault,
        )
    }
}
async fn login(provider: &NativeProvider) {
    assert_eq!(
        provider
            .sign_in("tester@example.test", "one-use-password")
            .await
            .unwrap(),
        SignInResult::SignedIn
    );
}

fn account_reply(request: &Request) -> Reply {
    if request.path == "/v1/auth/signin" {
        let username = request.body["username"].as_str().unwrap();
        let mut response = tokens(&format!("synthetic-access-{username}"));
        response["sub"] = json!(format!("synthetic-subject-{username}"));
        response["refresh_token"] = json!(format!("synthetic-refresh-{username}"));
        reply(200, response)
    } else {
        standard(request)
    }
}

async fn login_account(provider: &NativeProvider, username: &str) -> String {
    assert_eq!(
        provider
            .sign_in(username, "request-only-password")
            .await
            .unwrap(),
        SignInResult::SignedIn
    );
    provider.auth_status().await.unwrap().account_id.unwrap()
}

#[tokio::test]
async fn accounts_add_switch_restart_and_remove_preserve_other_accounts_and_uploads() {
    let server = Server::new(account_reply).await;
    let (provider, vault) = server.provider();
    let first = login_account(&provider, "alice@example.test").await;
    let first_scope = provider.account_scope().await.unwrap();
    vault
        .save_aux(
            "first-upload",
            &json!({"scope":first_scope,"secret":"first-upload-secret"}).to_string(),
        )
        .unwrap();
    let first_epoch = provider.session.lock().await.epoch;
    let second = login_account(&provider, "amy@example.test").await;
    let second_scope = provider.account_scope().await.unwrap();
    assert_ne!(first, second, "matching masked names cannot merge accounts");
    assert_eq!(first_scope, first);
    assert_eq!(second_scope, second);
    assert_eq!(
        provider.account_scope_for(&first).await.unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        provider.account_scope_for(&second).await.unwrap(),
        second_scope
    );
    assert!(provider.session.lock().await.epoch > first_epoch);
    vault
        .save_aux(
            "second-upload",
            &json!({"scope":second_scope,"secret":"second-upload-secret"}).to_string(),
        )
        .unwrap();
    let listed = provider.accounts_list().await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed.iter().filter(|a| a.active).count(), 1);
    let public = serde_json::to_string(&listed).unwrap();
    for secret in [
        "alice",
        "amy",
        "example.test",
        "synthetic-subject",
        "synthetic-access",
        "synthetic-refresh",
        "request-only-password",
    ] {
        assert!(!public.contains(secret));
    }
    {
        let mut session = provider.session.lock().await;
        session.cursors.push_back(Cursor {
            id: "old-page".into(),
            parent: "root".into(),
            limit: 100,
            token: "private-cursor".into(),
            seen: Default::default(),
            epoch: first_epoch,
        });
        session.drive.token = Some(VerifiedCaptcha {
            token: Zeroizing::new("second-drive-captcha".into()),
            expires_at: tokio::time::Instant::now() + Duration::from_secs(60),
        });
    }
    let switched = provider.account_switch(&first).await.unwrap();
    assert_eq!(switched.account_id.as_deref(), Some(first.as_str()));
    assert!(provider.session.lock().await.cursors.is_empty());
    assert!(provider.session.lock().await.drive.token.is_none());
    assert_eq!(provider.account_scope().await.unwrap(), first_scope);
    assert!(vault.load_aux("first-upload").unwrap().is_some());
    assert!(vault.load_aux("second-upload").unwrap().is_some());
    *provider.session.lock().await = Session::default();
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(first.as_str())
    );
    assert_eq!(provider.accounts_list().await.unwrap().len(), 2);
    provider.account_remove(&second).await.unwrap();
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(first.as_str())
    );
    assert!(vault.load_aux("first-upload").unwrap().is_some());
    assert!(vault.load_aux("second-upload").unwrap().is_none());
    let third = login_account(&provider, "bob@example.test").await;
    assert_ne!(first, third);
    provider.logout().await.unwrap();
    let accounts = provider.accounts_list().await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, first);
    assert!(!accounts[0].active);
    assert!(vault.load().unwrap().is_none());
    provider.account_switch(&first).await.unwrap();
    assert!(provider.auth_status().await.unwrap().authenticated);
    provider.logout().await.unwrap();
    assert!(provider.accounts_list().await.unwrap().is_empty());
    assert!(vault.load_aux("first-upload").unwrap().is_none());
}

#[tokio::test]
async fn accounts_failed_add_and_switch_keep_original_active_session() {
    let server = Server::new(|request| {
        if request.path == "/v1/auth/signin" && request.body["username"] == "denied@example.test" {
            reply(401, json!({"error":"invalid_credentials"}))
        } else {
            account_reply(request)
        }
    })
    .await;
    let (provider, vault) = server.provider();
    let first = login_account(&provider, "alice@example.test").await;
    let epoch = provider.session.lock().await.epoch;
    assert!(provider
        .sign_in("denied@example.test", "bad-password")
        .await
        .is_err());
    assert_eq!(provider.session.lock().await.epoch, epoch);
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(first.as_str())
    );
    vault.fail_base_save.store(true, Ordering::SeqCst);
    assert_eq!(
        provider
            .sign_in("bob@example.test", "request-only-password")
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(first.as_str())
    );
    assert_eq!(provider.accounts_list().await.unwrap().len(), 1);
    vault.fail_base_save.store(false, Ordering::SeqCst);
    *provider.session.lock().await = Session::default();
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(first.as_str())
    );
    let second = login_account(&provider, "bob@example.test").await;
    vault.fail_base_save.store(true, Ordering::SeqCst);
    assert!(provider.account_switch(&first).await.is_err());
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(second.as_str())
    );
    assert_eq!(provider.accounts_list().await.unwrap().len(), 2);
    vault.fail_base_save.store(false, Ordering::SeqCst);
    *provider.session.lock().await = Session::default();
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(second.as_str())
    );
}

#[tokio::test]
async fn accounts_challenge_preserves_old_account_until_verified_and_refresh_does_not_drop_challenge(
) {
    let server = Server::new(|request| {
        if request.path == "/v1/shield/captcha/init"
            && request.body["meta"]["email"] == "bob@example.test"
        {
            reply(
                200,
                json!({"url":"https://user.mypikpak.com/challenge","captcha_token":"not-verified"}),
            )
        } else if request.path == "/v1/auth/token" {
            let mut response = tokens("refreshed-old-access");
            response["sub"] = json!("synthetic-subject-alice@example.test");
            reply(200, response)
        } else {
            account_reply(request)
        }
    })
    .await;
    let (provider, vault) = server.provider();
    let first = login_account(&provider, "alice@example.test").await;
    let epoch = provider.session.lock().await.epoch;
    let SignInResult::Challenge { challenge } = provider
        .sign_in("bob@example.test", "discarded-password")
        .await
        .unwrap()
    else {
        panic!("expected challenge")
    };
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(first.as_str())
    );
    assert_eq!(provider.session.lock().await.epoch, epoch);
    assert!(provider
        .verify_sign_in(&challenge.challenge_id, "", Some("new-password"))
        .await
        .is_err());
    provider
        .session
        .lock()
        .await
        .stored
        .as_mut()
        .unwrap()
        .expires_at = now();
    assert!(provider.auth_status().await.unwrap().authenticated);
    assert_eq!(
        provider.session.lock().await.pending.as_ref().unwrap().id,
        challenge.challenge_id
    );
    complete_human_verification(&challenge, "verified-account-add").await;
    assert_eq!(
        provider
            .verify_sign_in(&challenge.challenge_id, "", Some("new-password"))
            .await
            .unwrap(),
        SignInResult::SignedIn
    );
    let second = provider.auth_status().await.unwrap().account_id.unwrap();
    assert_ne!(first, second);
    assert_eq!(provider.accounts_list().await.unwrap().len(), 2);
    let all_storage = format!(
        "{} {:?}",
        vault.load().unwrap().unwrap().as_str(),
        vault.accounts.lock().unwrap().keys()
    );
    assert!(!all_storage.contains("password"));
    assert!(provider.session.lock().await.pending.is_none());
}

#[tokio::test]
async fn accounts_legacy_migration_and_corrupt_index_do_not_erase_sessions() {
    let server = Server::new(account_reply).await;
    let (provider, vault) = server.provider();
    let mut stored = token_session(
        &serde_json::to_vec(&tokens("legacy-access")).unwrap(),
        "0123456789abcdef0123456789abcdef",
        "t***",
    )
    .unwrap();
    stored.user_id.clear();
    vault
        .save(&serde_json::to_string(&stored).unwrap())
        .unwrap();
    let before = vault.load().unwrap().unwrap();
    vault.fail_base_save.store(true, Ordering::SeqCst);
    assert!(provider.auth_status().await.is_err());
    assert_eq!(vault.load().unwrap().unwrap().as_str(), before.as_str());
    vault.fail_base_save.store(false, Ordering::SeqCst);
    let state = provider.auth_status().await.unwrap();
    let id = state.account_id.unwrap();
    assert_eq!(provider.accounts_list().await.unwrap().len(), 1);
    *provider.session.lock().await = Session::default();
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(id.as_str())
    );
    *vault.accounts_index.lock().unwrap() = Some(Zeroizing::new("broken-index".into()));
    *provider.session.lock().await = Session::default();
    let credential = vault.load().unwrap().unwrap();
    assert_eq!(
        provider.auth_status().await.unwrap_err().code,
        ErrorCode::IntegrityFailed
    );
    assert_eq!(vault.load().unwrap().unwrap().as_str(), credential.as_str());
    assert_eq!(
        vault.load_accounts().unwrap().unwrap().as_str(),
        "broken-index"
    );
}

#[tokio::test]
async fn accounts_interrupted_remove_is_finished_on_restart_without_resurrection() {
    let server = Server::new(account_reply).await;
    let (provider, vault) = server.provider();
    let first = login_account(&provider, "alice@example.test").await;
    let second = login_account(&provider, "bob@example.test").await;
    vault.fail_delete.store(true, Ordering::SeqCst);
    assert_eq!(
        provider.logout().await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    assert!(!provider.auth_status().await.unwrap().authenticated);
    assert_eq!(provider.accounts_list().await.unwrap().len(), 1);
    *provider.session.lock().await = Session::default();
    assert_eq!(
        provider.auth_status().await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    vault.fail_delete.store(false, Ordering::SeqCst);
    assert!(!provider.auth_status().await.unwrap().authenticated);
    assert!(provider
        .accounts_list()
        .await
        .unwrap()
        .iter()
        .all(|a| a.id != second));
    provider.account_switch(&first).await.unwrap();
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(first.as_str())
    );
}

#[tokio::test]
async fn accounts_expired_switch_rejection_preserves_active_account_and_does_not_retry() {
    let fail = Arc::new(AtomicBool::new(false));
    let reject = fail.clone();
    let server = Server::new(move |request| {
        if request.path == "/v1/auth/token" {
            if reject.load(Ordering::SeqCst) {
                reply(400, json!({"error":"invalid_grant"}))
            } else {
                reply(503, json!({"error":"unavailable"}))
            }
        } else {
            account_reply(request)
        }
    })
    .await;
    let (provider, vault) = server.provider();
    let first = login_account(&provider, "alice@example.test").await;
    provider
        .session
        .lock()
        .await
        .stored
        .as_mut()
        .unwrap()
        .expires_at = now();
    let second = login_account(&provider, "bob@example.test").await;
    assert_eq!(
        provider.account_switch(&first).await.unwrap_err().code,
        ErrorCode::TransientNetwork
    );
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(second.as_str())
    );
    assert_eq!(provider.accounts_list().await.unwrap().len(), 2);
    assert!(vault.load_account(&first).unwrap().is_some());
    fail.store(true, Ordering::SeqCst);
    assert_eq!(
        provider.account_switch(&first).await.unwrap_err().code,
        ErrorCode::AuthExpired
    );
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(second.as_str())
    );
    assert_eq!(provider.accounts_list().await.unwrap().len(), 1);
    assert!(vault.load_account(&first).unwrap().is_none());
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/v1/auth/token")
            .count(),
        2
    );
}

#[tokio::test]
async fn accounts_expired_switch_refreshes_target_without_replacing_transfer_scope() {
    let server = Server::new(|request| {
        if request.path == "/v1/auth/token" {
            assert_eq!(
                request.body["refresh_token"],
                "synthetic-refresh-alice@example.test"
            );
            let mut response = tokens("renewed-alice-access");
            response["sub"] = json!("synthetic-subject-alice@example.test");
            response["refresh_token"] = json!("rotated-alice-refresh");
            reply(200, response)
        } else {
            account_reply(request)
        }
    })
    .await;
    let (provider, vault) = server.provider();
    let first = login_account(&provider, "alice@example.test").await;
    let scope = provider.account_scope().await.unwrap();
    provider
        .session
        .lock()
        .await
        .stored
        .as_mut()
        .unwrap()
        .expires_at = now();
    let second = login_account(&provider, "bob@example.test").await;
    vault.fail_base_save.store(true, Ordering::SeqCst);
    assert_eq!(
        provider.account_switch(&first).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(second.as_str())
    );
    assert!(vault
        .load_account(&first)
        .unwrap()
        .unwrap()
        .contains("rotated-alice-refresh"));
    vault.fail_base_save.store(false, Ordering::SeqCst);
    assert_eq!(
        provider
            .account_switch(&first)
            .await
            .unwrap()
            .account_id
            .as_deref(),
        Some(first.as_str())
    );
    assert_eq!(provider.account_scope().await.unwrap(), scope);
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/v1/auth/token")
            .count(),
        1
    );
}

#[tokio::test]
async fn accounts_switch_rejects_in_flight_old_metadata() {
    let started = Arc::new(tokio::sync::Notify::new());
    let notify = started.clone();
    let server = Server::new(move |request| {
        let mut result = account_reply(request);
        if request.path == "/drive/v1/files/file-1" {
            result.delay = Duration::from_millis(100);
            notify.notify_one();
        }
        result
    })
    .await;
    let (provider, _) = server.provider();
    let first = login_account(&provider, "alice@example.test").await;
    let second = login_account(&provider, "bob@example.test").await;
    let work = provider.clone();
    let read = tokio::spawn(async move { work.stat("file-1").await });
    started.notified().await;
    assert_eq!(
        provider
            .account_switch(&first)
            .await
            .unwrap()
            .account_id
            .as_deref(),
        Some(first.as_str())
    );
    assert_eq!(
        read.await.unwrap().unwrap_err().code,
        ErrorCode::AuthExpired
    );
    assert_ne!(first, second);
}

#[tokio::test]
async fn accounts_reauthentication_updates_one_account_and_invalidates_old_handles() {
    let server = Server::new(account_reply).await;
    let (provider, _) = server.provider();
    let first = login_account(&provider, "alice@example.test").await;
    let second = login_account(&provider, "bob@example.test").await;
    let epoch = provider.session.lock().await.epoch;
    let mut events = provider.subscribe_auth().unwrap();
    assert_eq!(login_account(&provider, "alice@example.test").await, first);
    assert!(provider.session.lock().await.epoch > epoch);
    assert_eq!(events.recv().await.unwrap().state, AuthStatus::SignedIn);
    assert_eq!(provider.accounts_list().await.unwrap().len(), 2);
    assert_eq!(
        provider
            .account_switch(&second)
            .await
            .unwrap()
            .account_id
            .as_deref(),
        Some(second.as_str())
    );
    assert_eq!(events.recv().await.unwrap().state, AuthStatus::SignedIn);
}

#[tokio::test]
async fn accounts_missing_subject_is_checked_before_saving_a_new_account() {
    let server = Server::new(|request| {
        if request.path == "/v1/auth/signin" {
            let mut response = tokens("new-no-subject-access");
            response.as_object_mut().unwrap().remove("sub");
            reply(200, response)
        } else if request.path == "/v1/user/me" {
            assert_eq!(
                request.headers["authorization"],
                "Bearer new-no-subject-access"
            );
            reply(200, json!({"sub":"resolved-account-subject"}))
        } else {
            account_reply(request)
        }
    })
    .await;
    let (provider, _) = server.provider();
    let id = login_account(&provider, "alice@example.test").await;
    assert_eq!(id, provider.account_scope().await.unwrap());
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/v1/user/me")
            .count(),
        1
    );
}

#[tokio::test]
async fn custom_proxy_routes_requests_and_disabling_it_restores_direct_connections() {
    let direct = Server::new(standard).await;
    let proxy = Server::new(|request| {
        let url = url::Url::parse(&request.path).unwrap();
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        match url.path() {
            "/v1/shield/captcha/init" => reply(200, json!({"captcha_token":"test-captcha"})),
            "/v1/auth/signin" => reply(200, tokens("proxy-access")),
            _ => reply(404, json!({"error":"file_not_found"})),
        }
    })
    .await;
    let (provider, _) = direct.provider();
    provider
        .configure_network("user_provided", Some(&proxy.origin), true)
        .unwrap();
    login(&provider).await;
    assert!(direct.requests.lock().unwrap().is_empty());
    assert_eq!(proxy.requests.lock().unwrap().len(), 2);
    provider.logout().await.unwrap();
    provider.configure_network("none", None, false).unwrap();
    login(&provider).await;
    assert_eq!(direct.requests.lock().unwrap().len(), 2);
    assert_eq!(proxy.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn native_login_metadata_and_logout_keep_secrets_inside_core() {
    let server = Server::new(standard).await;
    let (provider, vault) = server.provider();
    let mut events = provider.subscribe_auth().unwrap();
    assert_eq!(
        provider.discover().await.unwrap().status,
        BackendStatus::NeedsLogin
    );
    assert_eq!(
        provider.list("root", None, 100).await.unwrap_err().code,
        ErrorCode::AuthExpired
    );
    assert!(server.requests.lock().unwrap().is_empty());
    login(&provider).await;
    let status = provider.auth_status().await.unwrap();
    assert!(status.authenticated);
    assert_eq!(status.display_name.as_deref(), Some("t***"));
    assert_eq!(status.account_tier, AccountTier::Unknown);
    assert_eq!(status.credential_owner, "os_vault");
    let page = provider.list("root", None, 100).await.unwrap();
    assert_eq!(page.items[0].size_bytes.0, u64::MAX);
    assert_eq!(page.items[0].parent_id.as_deref(), Some("root"));
    assert_eq!(
        page.items[0].modified_at.as_deref(),
        Some("2026-11-08T02:00:00Z")
    );
    assert!(page.items[0].hash.is_none());
    let public = format!(
        "{}{}{}",
        serde_json::to_string(&page).unwrap(),
        serde_json::to_string(&status).unwrap(),
        serde_json::to_string(&events.recv().await.unwrap()).unwrap()
    );
    for private in [
        "one-use-password",
        "test-access",
        "test-refresh",
        "not-for-ipc",
        "example.test",
        "web_content_link",
        "medias",
    ] {
        assert!(!public.contains(private));
    }
    assert!(!vault.load().unwrap().unwrap().contains("one-use-password"));
    let caps = provider.discover().await.unwrap();
    assert!(caps.supports(Operation::List));
    assert!(caps.supports(Operation::CloudTask));
    assert!(caps.supports(Operation::SegmentDownload));
    for op in [Operation::ReadRange, Operation::StreamUrl] {
        assert!(!caps.supports(op));
    }
    {
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[1].body["password"], "one-use-password");
        assert_eq!(
            requests[1].body,
            json!({"client_id":profile::WEB_CLIENT_ID,"username":"tester@example.test","password":"one-use-password"})
        );
        assert_eq!(
            requests[0].body["meta"],
            json!({"email":"tester@example.test"})
        );
        assert_eq!(requests[1].headers["x-captcha-token"], "test-captcha");
        assert_eq!(requests[1].headers["x-client-id"], profile::WEB_CLIENT_ID);
        assert_eq!(requests[1].headers["x-client-version"], "1.0.0");
        assert_eq!(requests[1].headers["x-sdk-version"], "8.1.4");
        assert_eq!(requests[1].headers["x-protocol-version"], "301");
        assert_eq!(
            requests[1].headers["user-agent"],
            concat!("NekoPak/", env!("CARGO_PKG_VERSION"))
        );
        let drive = requests
            .iter()
            .find(|r| r.path.starts_with("/drive/v1/files?"))
            .unwrap();
        assert!(drive.path.contains("parent_id=&"));
        assert!(drive.headers.contains_key("authorization"));
        assert_eq!(drive.headers["x-captcha-token"], "test-captcha");
        assert!(!drive.headers.contains_key("x-client-version"));
        let init = requests
            .iter()
            .find(|r| r.body["action"] == "GET:/drive/v1/files")
            .unwrap();
        assert_eq!(init.body["meta"]["user_id"], "test-sub");
        assert_eq!(init.body["meta"]["client_version"], "2.0.0");
        assert_eq!(
            requests[0].body["device_id"],
            requests[1].headers["x-device-id"]
        );
    }
    provider.logout().await.unwrap();
    assert!(vault.load().unwrap().is_none());
    assert!(!provider.auth_status().await.unwrap().authenticated);
    assert_eq!(events.recv().await.unwrap().state, AuthStatus::SignedOut);
}

#[tokio::test]
async fn invalid_password_never_creates_a_session_or_echoes_server_diagnostics() {
    let server = Server::new(|r| if r.path == "/v1/auth/signin" { reply(400,json!({"error":"invalid_grant","error_description":"one-use-password access_token=bad"})) } else { standard(r) }).await;
    let (provider, vault) = server.provider();
    let error = provider
        .sign_in("tester@example.test", "one-use-password")
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert_eq!(error.details.unwrap()["reason"], "invalid_credentials");
    assert!(!error.message.contains("one-use-password"));
    assert!(vault.load().unwrap().is_none());
    assert!(!provider.auth_status().await.unwrap().authenticated);
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn gateway_failures_and_captcha_rejections_are_not_reported_as_wrong_password() {
    for (stage, status, upstream, expected) in [
        (
            "/v1/shield/captcha/init",
            400,
            "invalid_grant",
            ErrorCode::VerificationRequired,
        ),
        (
            "/v1/shield/captcha/init",
            400,
            "invalid_credentials",
            ErrorCode::VerificationRequired,
        ),
        (
            "/v1/auth/signin",
            503,
            "invalid_credentials",
            ErrorCode::TransientNetwork,
        ),
    ] {
        let server = Server::new(move |request| {
            if request.path == stage {
                reply(
                    status,
                    json!({"error":upstream,"error_description":"PRIVATE_PASSWORD"}),
                )
            } else {
                standard(request)
            }
        })
        .await;
        let (provider, vault) = server.provider();
        let failure = provider
            .sign_in("tester@example.test", "request-only-password")
            .await
            .unwrap_err();
        assert_eq!(failure.code, expected);
        assert_ne!(
            failure.details.as_ref().and_then(|d| d.get("reason")),
            Some(&json!("invalid_credentials"))
        );
        assert!(!serde_json::to_string(&failure).unwrap().contains("PRIVATE"));
        assert!(vault.load().unwrap().is_none());
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.path == stage)
                .count(),
            1,
            "Do not replay authentication requests after a gateway response"
        );
    }
}

#[tokio::test]
async fn temporary_refresh_failure_keeps_saved_session_and_recovers_without_password_or_restart() {
    let offline = Arc::new(AtomicBool::new(true));
    let flag = offline.clone();
    let server = Server::new(move |request| {
        if request.path == "/v1/auth/token" && flag.load(Ordering::SeqCst) {
            reply(503, json!({"error":"invalid_grant"}))
        } else {
            standard(request)
        }
    })
    .await;
    let (provider, vault) = server.provider();
    login(&provider).await;
    {
        let mut session = provider.session.lock().await;
        session.stored.as_mut().unwrap().expires_at = now();
        vault
            .save(&serde_json::to_string(session.stored.as_ref().unwrap()).unwrap())
            .unwrap();
        *session = Session::default();
    }
    let before = vault.load().unwrap().unwrap();
    let requests_before = server.requests.lock().unwrap().len();
    let restored = provider.auth_status().await.unwrap();
    assert!(restored.authenticated);
    assert_eq!(
        provider.discover().await.unwrap().status,
        BackendStatus::Ready
    );
    assert_eq!(
        server.requests.lock().unwrap().len(),
        requests_before,
        "local session restoration must not require the old network route"
    );
    assert_eq!(
        provider.stat("file-1").await.unwrap_err().code,
        ErrorCode::TransientNetwork
    );
    assert_eq!(vault.load().unwrap().unwrap().as_str(), before.as_str());
    offline.store(false, Ordering::SeqCst);
    provider.configure_network("none", None, false).unwrap();
    assert_eq!(
        provider.auth_status().await.unwrap().account_id,
        restored.account_id
    );
    provider.stat("file-1").await.unwrap();
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/v1/auth/signin")
            .count(),
        1
    );
}

#[tokio::test]
async fn failed_transport_is_rebuilt_for_a_later_attempt_without_restarting_or_replaying_the_password(
) {
    let server = Server::new(standard).await;
    let (provider, vault) = server.provider();
    let unused = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = unused.local_addr().unwrap();
    drop(unused);
    // Model a connection pool captured while the system proxy was unavailable.
    // The configured tuple remains direct; the failed transport must be rebuilt
    // for the next user attempt, without changing global environment/OS proxy.
    provider.http.replace_transport_for_test(
        reqwest::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::all(format!("http://{address}")).unwrap())
            .connect_timeout(Duration::from_millis(100))
            .build()
            .unwrap(),
    );
    let failure = provider
        .sign_in("tester@example.test", "request-only-password")
        .await
        .unwrap_err();
    assert_eq!(failure.code, ErrorCode::TransientNetwork);
    assert!(server.requests.lock().unwrap().is_empty());
    assert!(vault.load().unwrap().is_none());
    login(&provider).await;
    assert!(provider.auth_status().await.unwrap().authenticated);
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/v1/auth/signin")
            .count(),
        1
    );
}

#[tokio::test]
async fn reconnect_discards_stale_challenges_but_keeps_active_and_remembered_account() {
    let server = Server::new(|request| {
        if request.path == "/v1/shield/captcha/init"
            && request.body["meta"].get("user_id").is_some()
        {
            reply(
                200,
                json!({"url":"https://user.mypikpak.com/challenge?id=network-route"}),
            )
        } else {
            standard(request)
        }
    })
    .await;
    let (provider, vault) = server.provider();
    login(&provider).await;
    let previous = provider.auth_status().await.unwrap().account_id.unwrap();
    assert_eq!(
        provider.stat("file-1").await.unwrap_err().code,
        ErrorCode::VerificationRequired
    );
    assert!(provider.verification_status(None).await.unwrap().is_some());
    let raw = vault.load().unwrap().unwrap();
    provider.network_reconnect().unwrap();
    assert!(provider.verification_status(None).await.unwrap().is_none());
    assert_eq!(
        provider.auth_status().await.unwrap().account_id.as_deref(),
        Some(previous.as_str())
    );
    assert_eq!(vault.load().unwrap().unwrap().as_str(), raw.as_str());
    assert_eq!(
        provider.auth_preferences_get().await.unwrap()["username"],
        "tester@example.test"
    );
}

#[tokio::test]
async fn remembered_login_preferences_are_vault_only_and_restore_can_be_disabled() {
    let server = Server::new(standard).await;
    let (provider, vault) = server.provider();
    assert_eq!(
        provider.auth_preferences_get().await.unwrap(),
        json!({"remember_account":true,"restore_session":true})
    );
    provider.auth_preferences_update(&json!({"remember_account":true,"restore_session":false,"username":"tester@example.test"})).await.unwrap();
    login(&provider).await;
    let first = provider.auth_status().await.unwrap().account_id.unwrap();
    assert!(!vault
        .load_accounts()
        .unwrap()
        .unwrap()
        .contains("one-use-password"));
    *provider.session.lock().await = Session::default();
    assert!(!provider.auth_status().await.unwrap().authenticated);
    let saved = provider.accounts_list().await.unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].id, first);
    assert!(!saved[0].active);
    provider.account_switch(&first).await.unwrap();
    assert!(provider.auth_status().await.unwrap().authenticated);
    let clear = provider.auth_preferences_update(&json!({"remember_account":false,"restore_session":true,"username":"should-not-remain@example.test"})).await.unwrap();
    assert!(clear.get("username").is_none());
    assert!(!vault
        .load_accounts()
        .unwrap()
        .unwrap()
        .contains("should-not-remain"));
    *provider.session.lock().await = Session::default();
    assert!(provider.auth_status().await.unwrap().authenticated);
    for invalid in [
        json!({"remember_account":true,"restore_session":true,"username":"not-an-email"}),
        json!({"remember_account":true,"restore_session":true,"password":"must-never-persist"}),
    ] {
        assert_eq!(
            provider
                .auth_preferences_update(&invalid)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
    }
}

#[tokio::test]
async fn domain_measurements_keep_drive_results_when_auth_is_unreachable() {
    for auth_usable in [true, false] {
        let server = Server::new(move |request| {
            assert_eq!(request.method, "GET");
            assert!(!request.headers.contains_key("authorization"));
            assert!(!request.headers.contains_key("x-device-id"));
            if request.path == "/auth" && !auth_usable {
                reply(503, json!({"error":"unavailable"}))
            } else {
                let mut response = reply(401, json!({"error":"unauthenticated","error_code":16}));
                response.delay = Duration::from_millis(15);
                response
            }
        })
        .await;
        let result = network::probe_endpoints(
            Ok(reqwest::Client::builder().no_proxy().build().unwrap()),
            "synthetic.test",
            &format!("{}/drive", server.origin),
            &format!("{}/auth", server.origin),
        )
        .await;
        assert_eq!(result["usable"], auth_usable);
        assert!(result["latency_ms"].as_u64().unwrap() >= 10);
        assert!(result["warm_latency_ms"].as_u64().unwrap() >= 10);
        assert_eq!(result["auth_latency_ms"].is_number(), auth_usable);
        assert_eq!(server.requests.lock().unwrap().len(), 3);
    }
}

#[tokio::test]
async fn unauthenticated_domain_probe_requires_real_gateway_shape_and_sends_no_account_headers() {
    for (status, value, accepted) in [
        (
            401,
            json!({"error":"unauthenticated","error_code":16}),
            true,
        ),
        (200, json!({"error_code":16}), false),
        (401, json!({"error":"other","error_code":9}), false),
    ] {
        let server = Server::new(move |request| {
            assert_eq!(request.method, "GET");
            for key in [
                "authorization",
                "x-device-id",
                "x-captcha-token",
                "x-client-id",
            ] {
                assert!(!request.headers.contains_key(key));
            }
            reply(status, value.clone())
        })
        .await;
        let http = HttpApi::production(profile::WEB_CLIENT_ID).unwrap();
        let response = network::gateway_check(
            &http.fresh_probe_client().unwrap(),
            &format!("{}/drive/v1/about", server.origin),
        )
        .await;
        // Production probe clients are HTTPS-only; the HTTP fixture is rejected
        // before reaching it. Use an explicitly loopback-only test client below.
        assert!(response.is_err());
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        assert_eq!(
            network::gateway_check(&client, &format!("{}/drive/v1/about", server.origin))
                .await
                .is_ok(),
            accepted
        );
    }
}

#[tokio::test]
async fn drive_security_verification_is_actionable_without_logging_out() {
    let server = Server::new(|r| {
        if r.path.starts_with("/drive/v1/files") {
            reply(403, json!({"error":"captcha_required","error_code":9}))
        } else if r.path == "/v1/shield/captcha/init"
            && r.body["action"].as_str().is_some_and(|a| a.starts_with("GET:"))
        {
            reply(200, json!({"url":"https://user.mypikpak.com/challenge?id=drive", "captcha_token":"not-yet-verified"}))
        } else {
            standard(r)
        }
    }).await;
    let (provider, vault) = server.provider();
    login(&provider).await;
    let failure = provider.list("root", None, 100).await.unwrap_err();
    let public = serde_json::to_value(&failure).unwrap();
    assert_eq!(
        public["code"], "VERIFICATION_REQUIRED",
        "A signed-in drive challenge must offer verification, not a dead-end permission error"
    );
    assert!(public["details"]["verification_id"].is_string());
    assert!(provider.auth_status().await.unwrap().authenticated);
    assert!(vault.load().unwrap().is_some());
    for secret in [
        "test-access",
        "test-refresh",
        "not-yet-verified",
        "https://user.mypikpak.com/challenge",
    ] {
        assert!(!public.to_string().contains(secret));
    }
}

#[tokio::test]
async fn drive_human_callback_resumes_listing_without_password_or_new_login() {
    let server = Server::new(|r| {
        if r.path == "/v1/shield/captcha/init" && r.body["meta"].get("user_id").is_some() {
            reply(
                200,
                json!({"captcha_token":"pending","url":"https://user.mypikpak.com/challenge"}),
            )
        } else if r.path.starts_with("/drive/") {
            assert_eq!(r.headers["x-captcha-token"], "drive-verified");
            standard(r)
        } else {
            standard(r)
        }
    })
    .await;
    let (provider, _) = server.provider();
    login(&provider).await;
    let e = provider.list("root", None, 100).await.unwrap_err();
    let id = e.details.unwrap()["verification_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        provider
            .verification_status(Some(&id))
            .await
            .unwrap()
            .unwrap()
            .state,
        "pending"
    );
    assert!(provider.verification_complete(&id).await.is_err());
    let url = provider.verification_url(&id).await.unwrap();
    complete_human_verification(
        &SignInChallenge {
            challenge_id: id.clone(),
            url,
            kind: "captcha".into(),
            expires_at: timestamp(now() + 300),
            requires_password: false,
        },
        "drive-verified",
    )
    .await;
    assert_eq!(
        provider
            .verification_status(Some(&id))
            .await
            .unwrap()
            .unwrap()
            .state,
        "verified"
    );
    provider.verification_complete(&id).await.unwrap();
    assert_eq!(
        provider.list("root", None, 100).await.unwrap().items.len(),
        1
    );
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/v1/auth/signin")
            .count(),
        1
    );
}

#[tokio::test]
async fn legacy_session_loads_subject_once_and_preserves_it_when_refresh_omits_sub() {
    let server = Server::new(|r| match r.path.as_str() {
        "/v1/user/me" => reply(200, json!({"sub":"legacy-sub"})),
        "/v1/auth/token" => {
            let mut t = tokens("refreshed-access");
            t.as_object_mut().unwrap().remove("sub");
            reply(200, t)
        }
        _ => standard(r),
    })
    .await;
    let (provider, _) = server.provider();
    login(&provider).await;
    {
        let mut s = provider.session.lock().await;
        s.stored.as_mut().unwrap().user_id.clear();
    }
    provider.list("root", None, 100).await.unwrap();
    provider.stat("file-1").await.unwrap();
    {
        let mut s = provider.session.lock().await;
        s.stored.as_mut().unwrap().expires_at = now();
    }
    provider.auth_status().await.unwrap();
    assert_eq!(
        provider
            .session
            .lock()
            .await
            .stored
            .as_ref()
            .unwrap()
            .user_id,
        "legacy-sub"
    );
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/v1/user/me")
            .count(),
        1
    );
}

#[tokio::test]
async fn flat_lists_use_real_file_ids_and_keep_empty_search_pages_continuable() {
    let server=Server::new(|r|{
        if r.path.starts_with("/drive/v1/events") {return reply(200,json!({"events":[{"id":"event-1","file_id":"file-1","created_time":"2026-11-08T02:00:00Z","reference_resource":file("file-1")}]}));}
        if r.path.starts_with("/drive/v1/files?") {
            let url=url::Url::parse(&format!("http://localhost{}",r.path)).unwrap();
            let query:HashMap<_,_>=url.query_pairs().into_owned().collect();
            assert_eq!(query["parent_id"],"*");
            let filters:Value=serde_json::from_str(&query["filters"]).unwrap();
            let mut f=file("file-1");f["parent_id"]="original-parent".into();
            if filters["trashed"]["eq"]==true {f["trashed"]=true.into();return reply(200,json!({"files":[f]}));}
            if filters.get("system_tag").is_some(){assert_eq!(filters["system_tag"]["in"],"STAR");return reply(200,json!({"files":[f]}));}
            return reply(200,json!({"files":[f],"next_page_token":"later"}));
        }standard(r)
    }).await;
    let (provider, _) = server.provider();
    login(&provider).await;
    let starred = provider
        .query_files(&FileQuery::Starred, None, 100)
        .await
        .unwrap();
    assert_eq!(starred.items[0].starred, Some(true));
    assert_eq!(
        provider
            .query_files(&FileQuery::Trash, None, 100)
            .await
            .unwrap()
            .items[0]
            .parent_id
            .as_deref(),
        Some("original-parent")
    );
    assert_eq!(
        provider
            .query_files(&FileQuery::Recent, None, 100)
            .await
            .unwrap()
            .items[0]
            .id,
        "file-1"
    );
    let search = provider
        .query_files(&FileQuery::Search("absent".into()), None, 100)
        .await
        .unwrap();
    assert!(search.items.is_empty());
    assert!(search.next_cursor.is_some());
}

#[tokio::test]
async fn native_download_rejects_bad_ranges_and_sends_no_api_credentials_to_cdn() {
    for mode in ["good", "wrong-range", "ignored-range", "truncated"] {
        let origin = Arc::new(StdMutex::new(String::new()));
        let endpoint = origin.clone();
        let server = Server::new(move |r| {
            if r.path.starts_with("/drive/v1/files/file-1") {
                let mut f = file("file-1");
                f["size"] = "6".into();
                f["hash"] = "stable-content".into();
                f["web_content_link"] = format!("{}/content", endpoint.lock().unwrap()).into();
                return reply(200, f);
            }
            if r.path == "/content" {
                assert!(!r.headers.contains_key("authorization"));
                assert!(!r.headers.contains_key("x-captcha-token"));
                assert!(!r.headers.contains_key("x-device-id"));
                assert_eq!(r.headers["range"], "bytes=2-4");
                return Reply {
                    status: if mode == "ignored-range" { 200 } else { 206 },
                    body: if mode == "truncated" { "cd" } else { "cde" }.into(),
                    headers: format!(
                        "Content-Range: {}\r\n",
                        if mode == "wrong-range" {
                            "bytes 0-2/6"
                        } else {
                            "bytes 2-4/6"
                        }
                    ),
                    delay: Duration::ZERO,
                };
            }
            standard(r)
        })
        .await;
        *origin.lock().unwrap() = server.origin.clone();
        let (provider, _) = server.provider();
        login(&provider).await;
        let info = provider.download_info("file-1").await.unwrap();
        let result = provider
            .download_chunk(
                "file-1",
                &info.identity,
                &info.account_scope,
                2,
                3,
                nekopak_provider::DownloadObserver::default(),
            )
            .await;
        if mode == "good" {
            assert_eq!(result.unwrap().bytes, b"cde");
        } else {
            assert_eq!(result.err().unwrap().code, ErrorCode::IntegrityFailed);
        }
    }
}

#[tokio::test]
async fn download_source_reuse_requires_strong_etags_and_reconnect_invalidates_the_lease() {
    for validator in [Some("\"stable\""), Some("W/\"weak\""), None] {
        let origin = Arc::new(StdMutex::new(String::new()));
        let endpoint = origin.clone();
        let reject = Arc::new(AtomicBool::new(false));
        let rejected = reject.clone();
        let server = Server::new(move |request| {
            if request.path.starts_with("/drive/v1/files/file-1") {
                let mut entry = file("file-1");
                entry["size"] = "6".into();
                entry["hash"] = "stable-content".into();
                entry["web_content_link"] = format!("{}/content", endpoint.lock().unwrap()).into();
                return reply(200, entry);
            }
            if request.path == "/content" {
                if rejected.load(Ordering::SeqCst) {
                    return reply(412, json!({}));
                }
                let range = request.headers["range"].strip_prefix("bytes=").unwrap();
                if range == "2-3" && validator == Some("\"stable\"") {
                    assert_eq!(request.headers["if-match"], "\"stable\"");
                } else {
                    assert!(!request.headers.contains_key("if-match"));
                }
                return Reply {
                    status: 206,
                    body: "ab".into(),
                    headers: format!(
                        "Content-Range: bytes {range}/6\r\n{}",
                        validator
                            .map(|v| format!("ETag: {v}\r\n"))
                            .unwrap_or_default()
                    ),
                    delay: Duration::ZERO,
                };
            }
            standard(request)
        })
        .await;
        *origin.lock().unwrap() = server.origin.clone();
        let (provider, _) = server.provider();
        login(&provider).await;
        let info = provider.download_info("file-1").await.unwrap();
        for offset in [0, 2] {
            provider
                .download_chunk(
                    "file-1",
                    &info.identity,
                    &info.account_scope,
                    offset,
                    2,
                    Default::default(),
                )
                .await
                .unwrap();
        }
        let count = server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path.starts_with("/drive/v1/files/file-1"))
            .count();
        assert_eq!(
            count,
            if validator == Some("\"stable\"") {
                2
            } else {
                3
            }
        );
        provider.network_reconnect().unwrap();
        provider
            .download_chunk(
                "file-1",
                &info.identity,
                &info.account_scope,
                4,
                2,
                Default::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.path.starts_with("/drive/v1/files/file-1"))
                .count(),
            count + 1
        );
        reject.store(true, Ordering::SeqCst);
        assert_eq!(
            provider
                .download_chunk(
                    "file-1",
                    &info.identity,
                    &info.account_scope,
                    0,
                    2,
                    Default::default()
                )
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::IntegrityFailed
        );
    }
}

#[tokio::test]
async fn cdn_errors_preserve_retry_after_without_confusing_temporary_failure_with_etag_change() {
    for (status, expected) in [
        (503, ErrorCode::TransientNetwork),
        (429, ErrorCode::TransientNetwork),
        (403, ErrorCode::PermissionDenied),
        (404, ErrorCode::NotFound),
        (412, ErrorCode::IntegrityFailed),
    ] {
        let server = Server::new(move |request| {
            assert_eq!(request.path, "/content");
            assert!(!request.headers.contains_key("authorization"));
            Reply {
                status,
                body: "PRIVATE_CDN_DIAGNOSTIC".into(),
                headers: "Retry-After: 120\r\n".into(),
                delay: Duration::ZERO,
            }
        })
        .await;
        let (provider, _) = server.provider();
        let result = provider
            .fetch_range(
                &format!("{}/content", server.origin),
                Some(6),
                2,
                3,
                Some("\"expected-version\""),
                nekopak_provider::DownloadObserver::default(),
            )
            .await;
        let failure = result.err().unwrap();
        assert_eq!(failure.code, expected, "HTTP {status}");
        assert_eq!(failure.retryable, matches!(status, 429 | 503));
        if matches!(status, 429 | 503) {
            assert_eq!(
                failure.details.as_ref().unwrap()["retry_after_seconds"],
                120
            );
        }
        assert!(!serde_json::to_string(&failure)
            .unwrap()
            .contains("PRIVATE_CDN"));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn mutations_confirm_tasks_and_never_treat_post_accept_verification_as_safe_replay() {
    for response in [
        json!({"task":{"phase":"PHASE_TYPE_RUNNING"}}),
        json!({"task_id":42}),
        json!({"task":{"phase":"PHASE_TYPE_ERROR"}}),
    ] {
        let server = Server::new(move |r| {
            if r.path == "/drive/v1/files:batchCopy" {
                reply(200, response.clone())
            } else {
                standard(r)
            }
        })
        .await;
        let (provider, _) = server.provider();
        login(&provider).await;
        let scope = provider.account_scope().await.unwrap();
        assert!(provider
            .mutate_files(
                &FileMutation {
                    op: MutateOp::Copy,
                    ids: vec!["file-1".into()],
                    target: Some(MutateTarget {
                        parent_id: Some("root".into()),
                        name: None
                    })
                },
                &scope
            )
            .await
            .is_err());
    }
    let server = Server::new(|r| {
        if r.path == "/drive/v1/files:batchCopy" {
            reply(200, json!({"task_id":"remote-job"}))
        } else if r.path.starts_with("/drive/v1/tasks/") {
            reply(403, json!({"error":"captcha_required"}))
        } else {
            standard(r)
        }
    })
    .await;
    let (provider, _) = server.provider();
    login(&provider).await;
    let scope = provider.account_scope().await.unwrap();
    let e = provider
        .mutate_files(
            &FileMutation {
                op: MutateOp::Copy,
                ids: vec!["file-1".into()],
                target: None,
            },
            &scope,
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::VerificationRequired);
    assert!(e
        .details
        .as_ref()
        .unwrap()
        .get("safe_to_resubmit")
        .is_none());
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/drive/v1/files:batchCopy")
            .count(),
        1
    );
}

#[tokio::test]
async fn quota_preserves_actual_usage_and_does_not_exhaust_an_unlimited_daily_band() {
    let server=Server::new(|r|if r.path=="/drive/v1/about"{reply(200,json!({"quota":{"limit":"1000","usage":"200"}}))}else if r.path.starts_with("/vip/v1/quantity/list"){reply(200,json!({"base":{"download":{"total_assets":"100","assets":"120"},"download_daily":{"total_assets":"0","assets":"25"}}}))}else{standard(r)}).await;
    let (provider, _) = server.provider();
    login(&provider).await;
    let report = provider.quota().await.unwrap();
    assert_eq!(report.source, "account");
    assert_eq!(report.dimensions[1].used_bytes, Some(DecimalU64(120)));
    assert_eq!(report.dimensions[1].remaining_bytes, Some(DecimalU64(0)));
    assert_eq!(report.dimensions[4].used_bytes, Some(DecimalU64(25)));
    assert!(report.dimensions[4].limit_bytes.is_none());
    assert!(report.dimensions[4].remaining_bytes.is_none());
}

#[tokio::test]
async fn captcha_never_keeps_password_and_requires_fresh_entry_to_continue() {
    let inits = Arc::new(AtomicUsize::new(0));
    let count = inits.clone();
    let server=Server::new(move |r| if r.path == "/v1/shield/captcha/init" && count.fetch_add(1,Ordering::SeqCst)==0 {
        reply(200,json!({"url":"https://user.mypikpak.com/challenge?id=test","captcha_token":"pending-token"}))
    } else { standard(r) }).await;
    let (provider, vault) = server.provider();
    let SignInResult::Challenge { challenge } = provider
        .sign_in("tester@example.test", "discarded-password")
        .await
        .unwrap()
    else {
        panic!("expected challenge")
    };
    assert!(challenge.requires_password);
    assert!(vault.load().unwrap().is_none());
    assert_eq!(
        provider
            .verify_sign_in(&challenge.challenge_id, "", None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        provider
            .verify_sign_in("wrong-challenge", "", Some("new-password"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        provider
            .verify_sign_in(&challenge.challenge_id, "", Some("new-password"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert!(server
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|r| r.path != "/v1/auth/signin"));
    complete_human_verification(&challenge, "human-verified-token").await;
    assert_eq!(
        provider
            .verify_sign_in(&challenge.challenge_id, "", Some("new-password"))
            .await
            .unwrap(),
        SignInResult::SignedIn
    );
    let requests = server.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.path == "/v1/auth/signin")
            .count(),
        1
    );
    assert_eq!(requests.last().unwrap().body["password"], "new-password");
    assert_eq!(
        requests.last().unwrap().headers["x-captcha-token"],
        "human-verified-token"
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.path == "/v1/shield/captcha/init")
            .count(),
        1
    );
}

async fn complete_human_verification(challenge: &SignInChallenge, token: &str) {
    let url = url::Url::parse(&challenge.url).unwrap();
    let pairs: HashMap<_, _> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut redirect = url::Url::parse(&pairs["redirect_uri"]).unwrap();
    assert_eq!(redirect.host_str(), Some("127.0.0.1"));
    redirect
        .query_pairs_mut()
        .append_pair("state", &pairs["state"])
        .append_pair("captcha_token", token)
        .append_pair("expires_in", "300");
    let response = reqwest::Client::new().get(redirect).send().await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let body = response.text().await.unwrap();
    assert!(!body.contains(token));
}

#[tokio::test]
async fn human_callback_rejects_wrong_state_and_is_cancelled_on_logout() {
    let server = Server::new(|r| if r.path == "/v1/shield/captcha/init" {
        reply(200,json!({"url":"https://user.mypikpak.com/challenge","captcha_token":"unverified-token"}))
    } else { standard(r) }).await;
    let (provider, _) = server.provider();
    let SignInResult::Challenge { challenge } = provider
        .sign_in("test@example.test", "discarded")
        .await
        .unwrap()
    else {
        panic!("challenge expected")
    };
    let query: HashMap<_, _> = url::Url::parse(&challenge.url)
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut wrong = url::Url::parse(&query["redirect_uri"]).unwrap();
    wrong
        .query_pairs_mut()
        .append_pair("state", "wrong")
        .append_pair("captcha_token", "untrusted")
        .append_pair("expires_in", "300");
    assert_eq!(
        reqwest::Client::new()
            .get(wrong.clone())
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        400
    );
    assert_eq!(
        provider
            .verify_sign_in(&challenge.challenge_id, "", Some("new-pass"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    complete_human_verification(&challenge, "legitimate-fixture-token").await;
    provider.logout().await.unwrap();
    assert_eq!(
        provider
            .verify_sign_in(&challenge.challenge_id, "", Some("new-pass"))
            .await
            .unwrap_err()
            .code,
        ErrorCode::AuthExpired
    );
    assert!(server
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|r| r.path != "/v1/auth/signin"));
}

#[tokio::test]
#[ignore = "explicit live public CAPTCHA initialization only; no username, password or account sign-in"]
async fn public_login_handshake_without_credentials() {
    let client = HttpApi::production(profile::WEB_CLIENT_ID).unwrap();
    let device = uuid::Uuid::new_v4().simple().to_string();
    let body = json!({"client_id":profile::WEB_CLIENT_ID,"device_id":device,
        "action":format!("POST:{}/v1/auth/signin",http::AUTH_ORIGIN),"meta":{}});
    let bytes = client
        .request(
            Method::POST,
            &format!("{}/v1/shield/captcha/init", http::AUTH_ORIGIN),
            AuthHeaders::device(&device),
            Some(&body),
            true,
        )
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let has_token = value
        .get("captcha_token")
        .and_then(Value::as_str)
        .is_some_and(valid_token);
    let challenge_host = value
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(|s| url::Url::parse(s).unwrap().host_str().unwrap().to_owned());
    assert!(has_token || challenge_host.is_some());
    let report = json!({"date":"2026-11-08","scope":"Public CAPTCHA initialization; no username/password submitted, no sign-in or human challenge attempted",
        "endpoint":"https://user.mypikpak.com/v1/shield/captcha/init","http_status":200,"client_id":profile::WEB_CLIENT_ID,
        "captcha_token_present":has_token,"challenge_host":challenge_host,"expires_in":value.get("expires_in"),"token_values_recorded":false});
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/evidence/public-login-handshake.json");
    std::fs::write(output, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    println!("Public authentication handshake accepted; token values withheld.");
}

#[tokio::test]
async fn concurrent_unauthorized_reads_refresh_once_and_retry_only_reads() {
    let refreshes = Arc::new(AtomicUsize::new(0));
    let count = refreshes.clone();
    let server = Server::new(move |r| {
        if r.path == "/v1/auth/token" {
            count.fetch_add(1, Ordering::SeqCst);
        }
        if r.path.starts_with("/drive/")
            && r.headers
                .get("authorization")
                .is_some_and(|h| h == "Bearer test-access")
        {
            reply(401, json!({"error":"unauthenticated"}))
        } else {
            standard(r)
        }
    })
    .await;
    let (provider, _) = server.provider();
    login(&provider).await;
    let mut reads = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let provider = provider.clone();
        reads.spawn(async move { provider.stat("file-1").await });
    }
    while let Some(result) = reads.join_next().await {
        assert!(result.unwrap().is_ok());
    }
    assert_eq!(refreshes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn session_restoration_refreshes_without_password_and_revocation_erases_tokens() {
    let revoked = Arc::new(AtomicBool::new(false));
    let flag = revoked.clone();
    let server = Server::new(move |r| {
        if r.path == "/v1/auth/token" && flag.load(Ordering::SeqCst) {
            reply(400, json!({"error":"invalid_grant"}))
        } else {
            standard(r)
        }
    })
    .await;
    let (provider, vault) = server.provider();
    login(&provider).await;
    {
        let mut session = provider.session.lock().await;
        session.stored.as_mut().unwrap().expires_at = now() + 1;
        vault
            .save(&serde_json::to_string(session.stored.as_ref().unwrap()).unwrap())
            .unwrap();
        session.stored = None;
        session.loaded = false;
    }
    assert!(provider.auth_status().await.unwrap().authenticated);
    provider.stat("file-1").await.unwrap();
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/v1/auth/token")
            .count(),
        1
    );
    revoked.store(true, Ordering::SeqCst);
    provider
        .session
        .lock()
        .await
        .stored
        .as_mut()
        .unwrap()
        .expires_at = now() + 1;
    assert_eq!(
        provider.stat("file-1").await.unwrap_err().code,
        ErrorCode::AuthExpired
    );
    assert!(vault.load().unwrap().is_none());
    assert_eq!(
        provider.auth_status().await.unwrap().state,
        AuthStatus::Expired
    );
}

#[tokio::test]
async fn transient_refresh_is_not_replayed_and_preserves_recoverable_session() {
    let server = Server::new(|r| {
        if r.path == "/v1/auth/token" {
            reply(503, json!({"error":"unavailable"}))
        } else {
            standard(r)
        }
    })
    .await;
    let (provider, vault) = server.provider();
    login(&provider).await;
    provider
        .session
        .lock()
        .await
        .stored
        .as_mut()
        .unwrap()
        .expires_at = now() + 1;
    assert_eq!(
        provider.stat("file-1").await.unwrap_err().code,
        ErrorCode::TransientNetwork
    );
    assert!(vault.load().unwrap().is_some());
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/v1/auth/token")
            .count(),
        1
    );
}

#[tokio::test]
async fn logout_invalidates_in_flight_metadata_and_cursors() {
    let started = Arc::new(tokio::sync::Notify::new());
    let notify = started.clone();
    let server = Server::new(move |r| {
        let mut result = standard(r);
        if r.path == "/drive/v1/files/file-1" {
            result.delay = Duration::from_millis(100);
            notify.notify_one();
        }
        result
    })
    .await;
    let (provider, _) = server.provider();
    login(&provider).await;
    let work = provider.clone();
    let read = tokio::spawn(async move { work.stat("file-1").await });
    started.notified().await;
    provider.logout().await.unwrap();
    assert_eq!(
        read.await.unwrap().unwrap_err().code,
        ErrorCode::AuthExpired
    );
}

#[tokio::test]
async fn vault_failures_are_not_replaced_with_plaintext_or_success() {
    let server = Server::new(standard).await;
    let (provider, vault) = server.provider();
    vault.fail_save.store(true, Ordering::SeqCst);
    assert_eq!(
        provider
            .sign_in("tester@example.test", "password")
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert!(!provider.auth_status().await.unwrap().authenticated);
    assert!(vault.load().unwrap().is_none());
    *vault.value.lock().unwrap() = Some(Zeroizing::new("malformed-test-session".into()));
    provider.session.lock().await.loaded = false;
    assert_eq!(
        provider.auth_status().await.unwrap_err().code,
        ErrorCode::IntegrityFailed
    );
    assert_eq!(
        vault.load().unwrap().unwrap().as_str(),
        "malformed-test-session"
    );
}

#[tokio::test]
async fn pagination_tokens_are_scoped_and_cycles_fail_instead_of_ending_scan() {
    let pages = Arc::new(AtomicUsize::new(0));
    let count = pages.clone();
    let server=Server::new(move |r| if r.path.starts_with("/drive/v1/files?") {
        let index=count.fetch_add(1,Ordering::SeqCst);
        reply(200,json!({"files":[file(&format!("file-{index}"))],"next_page_token":if index==1 {"B"} else {"A"}}))
    } else { standard(r) }).await;
    let (provider, _) = server.provider();
    login(&provider).await;
    let a = provider
        .list("root", None, 100)
        .await
        .unwrap()
        .next_cursor
        .unwrap();
    assert_ne!(a, "A");
    assert_eq!(
        provider
            .list("different", Some(&a), 100)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        provider.list("root", Some(&a), 20).await.unwrap_err().code,
        ErrorCode::Conflict
    );
    let b = provider
        .list("root", Some(&a), 100)
        .await
        .unwrap()
        .next_cursor
        .unwrap();
    assert_eq!(
        provider.list("root", Some(&b), 100).await.unwrap_err().code,
        ErrorCode::IntegrityFailed
    );
    provider.logout().await.unwrap();
    assert_eq!(
        provider.list("root", Some(&a), 100).await.unwrap_err().code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn redirects_do_not_forward_credentials_or_retry_signin() {
    let server = Server::new(|r| {
        if r.path == "/v1/auth/signin" {
            let mut r = reply(307, json!({"error":"redirect"}));
            r.headers = "Location: https://example.invalid/steal\r\n".into();
            r
        } else {
            standard(r)
        }
    })
    .await;
    let (provider, vault) = server.provider();
    assert_eq!(
        provider
            .sign_in("tester@example.test", "password")
            .await
            .unwrap_err()
            .code,
        ErrorCode::Internal
    );
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    assert!(vault.load().unwrap().is_none());
}

#[test]
fn structured_errors_drop_untrusted_fields_and_never_suggest_retrying_writes() {
    for (status, name, expected) in [
        (401, "unauthenticated", ErrorCode::AuthExpired),
        (403, "denied", ErrorCode::PermissionDenied),
        (404, "file_not_found", ErrorCode::NotFound),
        (429, "too_many_requests", ErrorCode::TransientNetwork),
        (500, "internal", ErrorCode::TransientNetwork),
        (400, "file_space_not_enough", ErrorCode::QuotaExceeded),
        (
            400,
            "task_daily_create_limit_free",
            ErrorCode::QuotaExceeded,
        ),
        (400, "file_duplicated_name", ErrorCode::Conflict),
        (400, "file_rename_uncompleted", ErrorCode::Conflict),
        (400, "captcha_invalid", ErrorCode::VerificationRequired),
        (403, "account_restricted", ErrorCode::AccountRestricted),
        (400, "unknown_failure", ErrorCode::Internal),
    ] {
        let error=wire::check_response(status,json!({"error":name,"error_description":"PRIVATE_PASSWORD","url":"https://private/?token=PRIVATE_TOKEN"}).to_string().as_bytes(),Some(7),false).unwrap_err();
        assert_eq!(error.code, expected);
        assert!(error.request_id.is_some());
        assert!(!serde_json::to_string(&error).unwrap().contains("PRIVATE"));
        assert_eq!(error.retryable, expected == ErrorCode::TransientNetwork);
    }
}

#[test]
fn malformed_metadata_and_unsafe_verification_links_fail_closed() {
    for value in [
        json!({}),
        json!({"files":null}),
        json!({"files":[],"next_page_token":"A"}),
        json!({"files":[file("one"),file("one")]}),
    ] {
        assert!(wire::list(&value, "root", 100).is_err());
    }
    for size in [
        json!(42.5),
        json!("01"),
        json!("1e3"),
        json!("18446744073709551616"),
    ] {
        let mut entry = file("file-1");
        entry["size"] = size;
        assert!(wire::entry(&entry).is_err());
    }
    for url in [
        "http://user.mypikpak.com/",
        "https://mypikpak.com.evil.test/",
        "https://user:pass@mypikpak.com/",
        "https://mypikpak.com:444/",
        "https://mypikpak.com/?access_token=secret",
    ] {
        assert!(validate_challenge_url(url).is_err());
    }
    assert!(validate_login("12345678900", "password").is_err());
    assert!(wire::validate_id("../../secret").is_err());
}

#[tokio::test]
async fn unconfigured_native_provider_never_uses_mock_or_external_process() {
    let server = Server::new(standard).await;
    let (provider, _) = server.provider();
    let mut provider = Arc::try_unwrap(provider).ok().unwrap();
    provider.profile = None;
    let report = provider.discover().await.unwrap();
    assert_eq!(report.backend, BackendId::RustCompat);
    assert_eq!(report.status, BackendStatus::Unavailable);
    assert!(!report.operations.values().any(|v| *v));
    assert!(!provider.supports_password_auth());
    assert_eq!(
        provider
            .sign_in("test@example.test", "password")
            .await
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert!(server.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_rejected_refreshed_token_expires_session_without_another_retry() {
    let server = Server::new(|r| {
        if r.path.starts_with("/drive/") {
            reply(401, json!({"error":"unauthenticated"}))
        } else {
            standard(r)
        }
    })
    .await;
    let (provider, vault) = server.provider();
    login(&provider).await;
    assert_eq!(
        provider.stat("file-1").await.unwrap_err().code,
        ErrorCode::AuthExpired
    );
    assert!(vault.load().unwrap().is_none());
    assert_eq!(
        provider.auth_status().await.unwrap().state,
        AuthStatus::Expired
    );
    let requests = server.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.path == "/v1/auth/token")
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.path.starts_with("/drive/"))
            .count(),
        2
    );
}

#[tokio::test]
async fn cancelling_signin_cannot_commit_a_late_response() {
    let started = Arc::new(tokio::sync::Notify::new());
    let notify = started.clone();
    let server = Server::new(move |r| {
        let mut response = standard(r);
        if r.path == "/v1/auth/signin" {
            notify.notify_one();
            response.delay = Duration::from_millis(100);
        }
        response
    })
    .await;
    let (provider, vault) = server.provider();
    let work = provider.clone();
    let context = nekopak_provider::RequestContext::default();
    let worker = context.clone();
    let signin = tokio::spawn(async move {
        worker
            .run(work.sign_in("test@example.test", "synthetic-password"))
            .await
    });
    started.notified().await;
    context.cancel();
    assert_eq!(
        signin.await.unwrap().unwrap_err().code,
        ErrorCode::Cancelled
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(vault.load().unwrap().is_none());
    assert!(!provider.auth_status().await.unwrap().authenticated);
}

#[tokio::test]
async fn response_budget_rejects_oversized_metadata() {
    let server = Server::new(|r| {
        if r.path.starts_with("/drive/") {
            reply(200, json!({"unexpected": "x".repeat(4 * 1024 * 1024)}))
        } else {
            standard(r)
        }
    })
    .await;
    let (provider, _) = server.provider();
    login(&provider).await;
    assert_eq!(
        provider.stat("file-1").await.unwrap_err().code,
        ErrorCode::IntegrityFailed
    );
}

#[cfg(windows)]
#[tokio::test]
async fn windows_default_build_offers_native_sign_in() {
    // Unique namespace: no user's stored session can be read by this regression.
    let provider =
        NativeProvider::from_environment(&format!("default-login-test-{}", uuid::Uuid::new_v4()))
            .unwrap();
    assert!(
        provider.supports_password_auth(),
        "A normal install must offer native login without developer environment variables"
    );
    let report = provider.discover().await.unwrap();
    assert_eq!(report.backend, BackendId::RustCompat);
    assert_eq!(report.status, BackendStatus::NeedsLogin);
    assert_eq!(
        provider.auth_status().await.unwrap().state,
        AuthStatus::SignedOut
    );
}

#[tokio::test]
async fn connector_retry_is_bounded_and_never_replays_an_http_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let count = connections.clone();
    let reject = tokio::spawn(async move {
        for _ in 0..4 {
            let (stream, _) = listener.accept().await.unwrap();
            count.fetch_add(1, Ordering::SeqCst);
            drop(stream); // TLS cannot complete, so no HTTP/password is sent.
        }
    });
    let client = HttpApi::loopback(&format!("http://{address}"));
    let failure = client
        .request(
            Method::POST,
            &format!("https://{address}/v1/auth/signin"),
            AuthHeaders::device("0123456789abcdef0123456789abcdef"),
            Some(&json!({"password":"synthetic-not-sent"})),
            true,
        )
        .await
        .unwrap_err();
    reject.await.unwrap();
    assert_eq!(connections.load(Ordering::SeqCst), 4);
    assert_eq!(failure.details.as_ref().unwrap()["connect_attempts"], 4);
    assert!(!serde_json::to_string(&failure)
        .unwrap()
        .contains("synthetic-not-sent"));

    let server = Server::new(|r| {
        if r.path == "/v1/auth/signin" {
            reply(503, json!({"error":"unavailable"}))
        } else {
            standard(r)
        }
    })
    .await;
    let (provider, _) = server.provider();
    assert_eq!(
        provider
            .sign_in("test@example.test", "password")
            .await
            .unwrap_err()
            .code,
        ErrorCode::TransientNetwork
    );
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/v1/auth/signin")
            .count(),
        1
    );
}
