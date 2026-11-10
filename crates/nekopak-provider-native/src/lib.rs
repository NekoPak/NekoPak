//! Direct HTTP compatibility provider. Not an official public REST API.
//! Protocol source: docs/BACKEND_API_ADAPTER.md; no upstream client code/secrets copied.
mod accounts;
mod archive;
mod captcha;
mod cloud;
mod download;
mod drive;
mod files;
mod http;
mod network;
mod profile;
mod refs;
mod shared_media;
mod shares;
mod upload;
mod vault;
mod wire;

use async_trait::async_trait;
use captcha::{HumanCaptcha, VerifiedCaptcha};
use http::{AuthHeaders, HttpApi};
use nekopak_domain::*;
use nekopak_provider::{unsupported, ProviderPort};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::{broadcast, Mutex};
use vault::{OsVault, SessionVault};
use zeroize::{Zeroize, Zeroizing};

pub(crate) fn error(code: ErrorCode, message: &'static str) -> ClientError {
    let mut error = ClientError::new(code, message).for_backend(BackendId::RustCompat);
    error.request_id = Some(uuid::Uuid::new_v4().to_string());
    error
}
fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}
fn timestamp(seconds: i64) -> String {
    OffsetDateTime::from_unix_timestamp(seconds)
        .expect("bounded timestamp")
        .format(&Rfc3339)
        .expect("UTC timestamp")
}
fn expired() -> ClientError {
    error(ErrorCode::AuthExpired, "登录已过期，请重新登录。")
}

/// Public Web protocol metadata; optional explicit overrides are for development only.
struct ClientProfile {
    id: String,
    secret: Option<Zeroizing<String>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredSession {
    schema_version: u32,
    access_token: String,
    refresh_token: String,
    expires_at: i64,
    device_id: String,
    display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    user_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    account_id: String,
}
impl Drop for StoredSession {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}
impl StoredSession {
    fn validate(&self) -> Result<(), ClientError> {
        if self.schema_version != 1
            || !valid_token(&self.access_token)
            || !valid_token(&self.refresh_token)
            || self.device_id.len() != 32
            || !self.device_id.bytes().all(|b| b.is_ascii_hexdigit())
            || self.expires_at <= 0
            || self.expires_at > now() + 366 * 86400
            || self.display_name.len() > 256
            || self.display_name.chars().any(char::is_control)
            || self.user_id.len() > 256
            || self.user_id.chars().any(char::is_control)
            || (!self.account_id.is_empty() && !accounts::valid_account_id(&self.account_id))
        {
            return Err(wire::invalid());
        }
        Ok(())
    }
}
fn valid_token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: String,
    refresh_token: String,
    expires_in: u32,
    token_type: String,
    #[serde(default)]
    sub: String,
}
impl Drop for TokenReply {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}

struct PendingChallenge {
    id: String,
    username: String,
    expires_at: i64,
    callback: HumanCaptcha,
    verified: Option<VerifiedCaptcha>,
}
#[derive(Clone)]
struct Cursor {
    id: String,
    parent: String,
    limit: usize,
    token: String,
    seen: BTreeSet<String>,
    epoch: u64,
}
struct Session {
    network_generation: u64,
    loaded: bool,
    stored: Option<StoredSession>,
    device_id: String,
    pending: Option<PendingChallenge>,
    expired: bool,
    epoch: u64,
    cursors: VecDeque<Cursor>,
    drive: drive::DriveCaptcha,
    shares: BTreeMap<String, shares::ShareAccess>,
    archives: BTreeMap<String, archive::ArchiveAccess>,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            network_generation: 0,
            loaded: false,
            stored: None,
            device_id: uuid::Uuid::new_v4().simple().to_string(),
            pending: None,
            expired: false,
            epoch: 0,
            cursors: VecDeque::new(),
            drive: Default::default(),
            shares: Default::default(),
            archives: Default::default(),
        }
    }
}
struct Snapshot {
    access: Zeroizing<String>,
    device: String,
    epoch: u64,
    user_id: String,
}

pub struct NativeProvider {
    read_cache: std::sync::Mutex<download::ReadCache>,
    profile: Option<ClientProfile>,
    http: HttpApi,
    vault: Arc<dyn SessionVault>,
    vault_supported: bool,
    session: Mutex<Session>,
    auth_events: broadcast::Sender<AuthChangedEvent>,
}
impl NativeProvider {
    /// Explicit offline Android acceptance only. Exercises the actual Rust-JNI
    /// bridge from a native worker with unique synthetic records; no HTTP or
    /// account data is accessed. This API does not exist in release builds.
    #[cfg(all(target_os = "android", debug_assertions))]
    pub fn android_vault_self_test() -> Result<(), ClientError> {
        std::thread::Builder::new()
            .name("nekopak-vault-selftest".into())
            .spawn(vault::android_self_test)
            .map_err(|_| error(ErrorCode::Internal, "Android 凭据库离线自检未完成。"))?
            .join()
            .map_err(|_| error(ErrorCode::Internal, "Android 凭据库离线自检未完成。"))?
    }

    /// Isolated development harness only. Not compiled into release; never uses the OS vault.
    #[cfg(debug_assertions)]
    pub fn synthetic_loopback(origin: &str) -> Result<Self, ClientError> {
        let url = url::Url::parse(origin).map_err(|_| wire::invalid())?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || url.path() != "/"
            || url.query().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(wire::invalid());
        }
        #[derive(Default)]
        struct TestVault(
            std::sync::Mutex<Option<Zeroizing<String>>>,
            std::sync::Mutex<BTreeMap<String, Zeroizing<String>>>,
            std::sync::Mutex<Option<Zeroizing<String>>>,
            std::sync::Mutex<BTreeMap<String, Zeroizing<String>>>,
        );
        impl SessionVault for TestVault {
            fn load_accounts(&self) -> Result<Option<Zeroizing<String>>, ClientError> {
                Ok(self.2.lock().map_err(|_| wire::invalid())?.clone())
            }
            fn save_accounts(&self, value: &str) -> Result<(), ClientError> {
                *self.2.lock().map_err(|_| wire::invalid())? = Some(Zeroizing::new(value.into()));
                Ok(())
            }
            fn load_account(&self, id: &str) -> Result<Option<Zeroizing<String>>, ClientError> {
                Ok(self.3.lock().map_err(|_| wire::invalid())?.get(id).cloned())
            }
            fn save_account(&self, id: &str, value: &str) -> Result<(), ClientError> {
                self.3
                    .lock()
                    .map_err(|_| wire::invalid())?
                    .insert(id.into(), Zeroizing::new(value.into()));
                Ok(())
            }
            fn delete_account(&self, id: &str) -> Result<(), ClientError> {
                self.3.lock().map_err(|_| wire::invalid())?.remove(id);
                Ok(())
            }
            fn aux_keys(&self) -> Result<Vec<String>, ClientError> {
                Ok(self
                    .1
                    .lock()
                    .map_err(|_| wire::invalid())?
                    .keys()
                    .cloned()
                    .collect())
            }
            fn load_aux(&self, key: &str) -> Result<Option<Zeroizing<String>>, ClientError> {
                Ok(self
                    .1
                    .lock()
                    .map_err(|_| wire::invalid())?
                    .get(key)
                    .cloned())
            }
            fn save_aux(&self, key: &str, value: &str) -> Result<(), ClientError> {
                self.1
                    .lock()
                    .map_err(|_| wire::invalid())?
                    .insert(key.into(), Zeroizing::new(value.into()));
                Ok(())
            }
            fn delete_aux(&self, key: &str) -> Result<(), ClientError> {
                self.1.lock().map_err(|_| wire::invalid())?.remove(key);
                Ok(())
            }
            #[cfg(test)]
            fn clear_aux(&self) -> Result<(), ClientError> {
                self.1.lock().map_err(|_| wire::invalid())?.clear();
                Ok(())
            }
            fn load(&self) -> Result<Option<Zeroizing<String>>, ClientError> {
                Ok(self.0.lock().map_err(|_| wire::invalid())?.clone())
            }
            fn save(&self, value: &str) -> Result<(), ClientError> {
                *self.0.lock().map_err(|_| wire::invalid())? = Some(Zeroizing::new(value.into()));
                Ok(())
            }
            fn delete(&self) -> Result<(), ClientError> {
                *self.0.lock().map_err(|_| wire::invalid())? = None;
                Ok(())
            }
        }
        let (auth_events, _) = broadcast::channel(32);
        Ok(Self {
            read_cache: Default::default(),
            profile: Some(ClientProfile {
                id: profile::WEB_CLIENT_ID.into(),
                secret: None,
            }),
            http: HttpApi::loopback(origin),
            vault: Arc::new(TestVault::default()),
            vault_supported: true,
            session: Mutex::new(Session::default()),
            auth_events,
        })
    }
    /// The host controls the vault namespace. It never accepts an API origin from IPC or env.
    pub fn from_environment(namespace: &str) -> Result<Self, ClientError> {
        let id = std::env::var("NEKOPAK_API_CLIENT_ID").ok();
        let secret = std::env::var("NEKOPAK_API_CLIENT_SECRET")
            .ok()
            .map(Zeroizing::new);
        let profile = match id {
            Some(id)
                if !id.is_empty()
                    && id.len() <= 256
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)) =>
            {
                if secret.as_ref().is_some_and(|s| !valid_token(s)) {
                    return Err(configuration());
                }
                Some(ClientProfile { id, secret })
            }
            None if secret.is_none() => Some(ClientProfile {
                id: profile::WEB_CLIENT_ID.into(),
                secret: None,
            }),
            _ => return Err(configuration()),
        };
        Self::with_profile(namespace, profile)
    }
    /// Explicit offline configuration for diagnostics and deterministic fixtures.
    pub fn unconfigured(namespace: &str) -> Result<Self, ClientError> {
        Self::with_profile(namespace, None)
    }
    /// Built-in public Web metadata, with no private client secret or account credentials.
    pub fn public_web(namespace: &str) -> Result<Self, ClientError> {
        Self::with_profile(
            namespace,
            Some(ClientProfile {
                id: profile::WEB_CLIENT_ID.into(),
                secret: None,
            }),
        )
    }
    fn with_profile(namespace: &str, profile: Option<ClientProfile>) -> Result<Self, ClientError> {
        let identity = profile
            .as_ref()
            .map(|p| p.id.as_str())
            .unwrap_or("unconfigured");
        let target = format!(
            "NekoPak/rust_compat/{:x}",
            Sha256::digest(format!("{namespace}\0{identity}"))
        );
        let (auth_events, _) = broadcast::channel(32);
        let http = HttpApi::production(identity)?;
        Ok(Self {
            read_cache: Default::default(),
            profile,
            http,
            vault: Arc::new(OsVault { target }),
            vault_supported: cfg!(any(windows, target_os = "android")),
            session: Mutex::new(Session::default()),
            auth_events,
        })
    }
    fn profile(&self) -> Result<&ClientProfile, ClientError> {
        if !self.vault_supported {
            return Err(configuration());
        }
        self.profile.as_ref().ok_or_else(configuration)
    }
    fn emit(&self, state: AuthStatus, error: Option<ClientError>) {
        if let Ok(mut cache) = self.read_cache.lock() {
            cache.clear_sources();
        }
        let _ = self.auth_events.send(AuthChangedEvent { state, error });
    }
    fn load(&self, session: &mut Session) -> Result<(), ClientError> {
        let network_generation = self
            .http
            .network_generation
            .load(std::sync::atomic::Ordering::Acquire);
        if session.network_generation != network_generation {
            // A browser verification created for the previous route must not
            // poison sign-in after a proxy/root change. Tokens remain stored;
            // the user may request a fresh challenge on the new route.
            session.pending = None;
            session.drive = Default::default();
            session.network_generation = network_generation;
        }
        self.load_account_session(session)
    }
    fn commit(&self, session: &mut Session, stored: StoredSession) -> Result<(), ClientError> {
        self.commit_account_session(session, stored)
    }
    async fn refresh_stored(&self, stored: &StoredSession) -> Result<StoredSession, ClientError> {
        let profile = self.profile()?;
        #[derive(Serialize)]
        struct Refresh<'a> {
            client_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            client_secret: Option<&'a str>,
            grant_type: &'a str,
            refresh_token: &'a str,
        }
        let request = Refresh {
            client_id: &profile.id,
            client_secret: profile.secret.as_deref().map(|v| v.as_str()),
            grant_type: "refresh_token",
            refresh_token: &stored.refresh_token,
        };
        let bytes = self
            .http
            .request(
                Method::POST,
                &format!("{}/v1/auth/token", self.http.auth_origin),
                AuthHeaders::device(&stored.device_id),
                Some(&request),
                false,
            )
            .await?;
        let mut next = token_session(&bytes, &stored.device_id, &stored.display_name)?;
        next.account_id.clone_from(&stored.account_id);
        if next.user_id.is_empty() {
            next.user_id.clone_from(&stored.user_id);
        } else if !stored.user_id.is_empty() && next.user_id != stored.user_id {
            return Err(wire::invalid());
        }
        Ok(next)
    }
    async fn refresh(&self, session: &mut Session) -> Result<(), ClientError> {
        let stored = session.stored.as_ref().ok_or_else(expired)?;
        match self.refresh_stored(stored).await {
            Ok(next) => self.commit(session, next),
            Err(failure) if failure.code == ErrorCode::AuthExpired => {
                self.expire_active_session(session)?;
                Err(failure)
            }
            Err(failure) => Err(failure),
        }
    }
    fn expire_active_session(&self, session: &mut Session) -> Result<(), ClientError> {
        let id = session
            .stored
            .as_ref()
            .ok_or_else(expired)?
            .account_id
            .clone();
        let result = self.remove_account_session(session, &id, true);
        // Even if the OS vault is temporarily unavailable, a rejected token may
        // never be reused by this process. Its cleanup failure is still reported.
        if session.stored.is_some() {
            session.invalidate_account_handles();
            session.stored = None;
            session.expired = true;
            self.emit(AuthStatus::Expired, result.as_ref().err().cloned());
        }
        result
    }
    async fn snapshot(&self) -> Result<Snapshot, ClientError> {
        self.profile()?;
        let mut session = self.session.lock().await;
        self.load(&mut session)?;
        let stored = session.stored.as_ref().ok_or_else(expired)?;
        if stored.expires_at <= now() + 30 {
            self.refresh(&mut session).await?;
        }
        snapshot_of(&session)
    }
    async fn get(&self, url: &str) -> Result<(Value, u64), ClientError> {
        self.drive_request(Method::GET, url, None).await
    }
    async fn drive_request(
        &self,
        method: Method,
        url: &str,
        body: Option<&Value>,
    ) -> Result<(Value, u64), ClientError> {
        self.drive_request_at(method, url, body, None).await
    }
    async fn drive_request_at(
        &self,
        method: Method,
        url: &str,
        body: Option<&Value>,
        expected_epoch: Option<u64>,
    ) -> Result<(Value, u64), ClientError> {
        let mut snapshot = self.snapshot().await?;
        if expected_epoch.is_some_and(|epoch| epoch != snapshot.epoch) {
            return Err(expired());
        }
        let mut refreshed = false;
        let mut renewed_captcha = false;
        loop {
            let captcha = self.drive_token(&snapshot, &method, url).await?;
            let response = self
                .http
                .request::<Value>(
                    method.clone(),
                    url,
                    AuthHeaders {
                        bearer: Some(&snapshot.access),
                        captcha_token: Some(&captcha),
                        ..AuthHeaders::device(&snapshot.device)
                    },
                    body,
                    false,
                )
                .await;
            match response {
                Err(error)
                    if error.code == ErrorCode::AuthExpired
                        && !refreshed
                        && method == Method::GET =>
                {
                    refreshed = true;
                    let mut session = self.session.lock().await;
                    if session.epoch != snapshot.epoch {
                        return Err(expired());
                    }
                    let current = session.stored.as_ref().ok_or_else(expired)?;
                    // Another reader may already have refreshed this exact token.
                    if current.access_token == *snapshot.access {
                        self.refresh(&mut session).await?;
                    }
                    snapshot = snapshot_of(&session)?;
                }
                Err(error) if error.code == ErrorCode::VerificationRequired => {
                    let mut session = self.session.lock().await;
                    if session.epoch != snapshot.epoch {
                        return Err(expired());
                    }
                    if session
                        .drive
                        .token
                        .as_ref()
                        .is_some_and(|current| *current.token == *captcha)
                    {
                        session.drive.token = None;
                    } else if session.drive.pending_id().is_none() {
                        drop(session);
                        if method == Method::GET && !renewed_captcha {
                            renewed_captcha = true;
                            continue;
                        }
                        return Err(error);
                    }
                    drop(session);
                    if method == Method::GET && !renewed_captcha {
                        renewed_captcha = true;
                        continue;
                    }
                    // Expose recovery even if the freshly initialized token is rejected.
                    // Mutations are never automatically replayed here.
                    return Err(self
                        .require_drive_verification(&snapshot, &method, url)
                        .await?);
                }
                result => {
                    let mut session = self.session.lock().await;
                    if session.epoch != snapshot.epoch {
                        return Err(expired());
                    }
                    if result
                        .as_ref()
                        .is_err_and(|e| e.code == ErrorCode::AuthExpired)
                        && session
                            .stored
                            .as_ref()
                            .is_some_and(|s| s.access_token == *snapshot.access)
                    {
                        self.expire_active_session(&mut session)?;
                    }
                    drop(session);
                    let bytes = result?;
                    return Ok((
                        if bytes.is_empty() {
                            json!({})
                        } else {
                            serde_json::from_slice(&bytes).map_err(|_| wire::invalid())?
                        },
                        snapshot.epoch,
                    ));
                }
            }
        }
    }
    async fn exchange(
        &self,
        session: &mut Session,
        username: &str,
        password: &str,
    ) -> Result<SignInResult, ClientError> {
        let profile = self.profile()?;
        let init = json!({"client_id":profile.id, "device_id":session.device_id,
            "action":"POST:/v1/auth/signin", "meta":{"email":username}});
        let bytes = self
            .http
            .request(
                Method::POST,
                &format!("{}/v1/shield/captcha/init", self.http.auth_origin),
                AuthHeaders::device(&session.device_id),
                Some(&init),
                true,
            )
            .await?;
        #[derive(Deserialize)]
        struct Captcha {
            #[serde(default)]
            captcha_token: String,
            #[serde(default)]
            url: String,
        }
        let mut captcha: Captcha = serde_json::from_slice(&bytes).map_err(|_| wire::invalid())?;
        let captcha_token = Zeroizing::new(std::mem::take(&mut captcha.captcha_token));
        if !captcha.url.is_empty() {
            validate_challenge_url(&captcha.url)?;
            let id = uuid::Uuid::new_v4().to_string();
            let expires_at = now() + 300;
            let callback = HumanCaptcha::listen().await?;
            let url = callback.challenge_url(&captcha.url)?;
            session.pending = Some(PendingChallenge {
                id: id.clone(),
                username: username.into(),
                expires_at,
                callback,
                verified: None,
            });
            if session.stored.is_none() {
                self.emit(AuthStatus::Pending, None);
            }
            return Ok(SignInResult::Challenge {
                challenge: SignInChallenge {
                    challenge_id: id,
                    kind: "captcha".into(),
                    url,
                    expires_at: timestamp(expires_at),
                    requires_password: true,
                },
            });
        }
        if !valid_token(&captcha_token) {
            return Err(wire::invalid());
        }
        self.complete_sign_in(session, username, password, &captcha_token)
            .await
    }

    async fn complete_sign_in(
        &self,
        session: &mut Session,
        username: &str,
        password: &str,
        captcha_token: &str,
    ) -> Result<SignInResult, ClientError> {
        let profile = self.profile()?;
        #[derive(Serialize)]
        struct SignIn<'a> {
            client_id: &'a str,
            username: &'a str,
            password: &'a str,
        }
        let request = SignIn {
            client_id: &profile.id,
            username,
            password,
        };
        let bytes = self
            .http
            .request(
                Method::POST,
                &format!("{}/v1/auth/signin", self.http.auth_origin),
                AuthHeaders {
                    captcha_token: Some(captcha_token),
                    ..AuthHeaders::device(&session.device_id)
                },
                Some(&request),
                true,
            )
            .await?;
        let mut stored = token_session(&bytes, &session.device_id, &mask_username(username))?;
        if stored.user_id.is_empty() {
            // Do not identify two new accounts by the same device or masked name.
            // Older responses may omit sub; verify the server's account identity
            // before committing or replacing any active credential.
            let bytes = self
                .http
                .request::<Value>(
                    Method::GET,
                    &format!("{}/v1/user/me", self.http.auth_origin),
                    AuthHeaders {
                        bearer: Some(&stored.access_token),
                        ..AuthHeaders::device(&stored.device_id)
                    },
                    None,
                    false,
                )
                .await?;
            let value: Value = serde_json::from_slice(&bytes).map_err(|_| wire::invalid())?;
            stored.user_id = wire::text(&value, "sub", 256)?;
        }
        self.remember_username(username)?;
        self.commit(session, stored)?;
        session.invalidate_account_handles();
        self.emit(AuthStatus::SignedIn, None);
        Ok(SignInResult::SignedIn)
    }
}

fn token_session(
    bytes: &[u8],
    device: &str,
    display_name: &str,
) -> Result<StoredSession, ClientError> {
    let mut reply: TokenReply = serde_json::from_slice(bytes).map_err(|_| wire::invalid())?;
    if !reply.token_type.eq_ignore_ascii_case("bearer")
        || reply.expires_in == 0
        || reply.expires_in > 366 * 86400
    {
        return Err(wire::invalid());
    }
    let session = StoredSession {
        schema_version: 1,
        access_token: std::mem::take(&mut reply.access_token),
        refresh_token: std::mem::take(&mut reply.refresh_token),
        expires_at: now() + reply.expires_in as i64,
        device_id: device.into(),
        display_name: display_name.into(),
        user_id: std::mem::take(&mut reply.sub),
        account_id: String::new(),
    };
    session.validate()?;
    Ok(session)
}
fn snapshot_of(session: &Session) -> Result<Snapshot, ClientError> {
    let stored = session.stored.as_ref().ok_or_else(expired)?;
    Ok(Snapshot {
        access: Zeroizing::new(stored.access_token.clone()),
        device: session.device_id.clone(),
        epoch: session.epoch,
        user_id: stored.user_id.clone(),
    })
}
fn configuration() -> ClientError {
    error(
        ErrorCode::Unsupported,
        "当前版本尚未配置可用的账号登录，请查看开发文档中的接入状态。",
    )
}
fn validate_login(username: &str, password: &str) -> Result<(), ClientError> {
    if username.len() > 254
        || !username.contains('@')
        || username
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
        || password.is_empty()
        || password.len() > 4096
        || password.contains('\0')
    {
        return Err(error(
            ErrorCode::Conflict,
            "请输入邮箱和密码；当前尚不支持手机号登录。",
        ));
    }
    Ok(())
}
fn mask_username(username: &str) -> String {
    // Even the domain is omitted: public events never identify the full account.
    format!("{}***", username.chars().next().unwrap_or('*'))
}
fn validate_challenge_url(value: &str) -> Result<(), ClientError> {
    let url = url::Url::parse(value).map_err(|_| wire::invalid())?;
    let host = url.host_str().unwrap_or("");
    if value.len() > 8192
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || url.fragment().is_some()
        || !(host == "mypikpak.com" || host.ends_with(".mypikpak.com"))
        || url.query_pairs().any(|(key, _)| {
            matches!(
                key.as_ref(),
                "access_token" | "refresh_token" | "client_secret" | "password"
            )
        })
    {
        return Err(wire::invalid());
    }
    Ok(())
}

#[async_trait]
impl ProviderPort for NativeProvider {
    fn preferred_download_chunk_size(&self) -> u64 {
        4 * 1024 * 1024
    }
    fn id(&self) -> BackendId {
        BackendId::RustCompat
    }
    fn supports_password_auth(&self) -> bool {
        self.profile.is_some() && self.vault_supported
    }
    fn subscribe_auth(&self) -> Option<broadcast::Receiver<AuthChangedEvent>> {
        Some(self.auth_events.subscribe())
    }
    fn network_reconnect(&self) -> Result<(), ClientError> {
        self.http.reconnect()
    }
    fn configure_server_domain(&self, domain: &str) -> Result<(), ClientError> {
        self.http.set_server_domain(domain)
    }
    async fn network_domains_probe(&self) -> Result<Value, ClientError> {
        self.probe_server_domains().await
    }
    async fn auth_preferences_get(&self) -> Result<Value, ClientError> {
        self.account_preferences().await
    }
    async fn auth_preferences_update(&self, value: &Value) -> Result<Value, ClientError> {
        self.update_account_preferences(value).await
    }
    async fn verification_status(
        &self,
        id: Option<&str>,
    ) -> Result<Option<VerificationState>, ClientError> {
        self.drive_verification_status(id).await
    }
    async fn verification_url(&self, id: &str) -> Result<String, ClientError> {
        self.drive_verification_url(id).await
    }
    async fn verification_complete(&self, id: &str) -> Result<(), ClientError> {
        self.finish_drive_verification(id).await
    }
    async fn verification_cancel(&self, id: &str) -> Result<(), ClientError> {
        let mut session = self.session.lock().await;
        if session.drive.pending_id() == Some(id) {
            session.drive = Default::default();
        }
        Ok(())
    }
    async fn account_scope(&self) -> Result<String, ClientError> {
        let snapshot = self.snapshot().await?;
        let subject = self.drive_subject(&snapshot).await?;
        Ok(format!(
            "{:x}",
            Sha256::digest(format!("{}\0{}", self.profile()?.id, subject))
        ))
    }
    async fn accounts_list(&self) -> Result<Vec<SavedAccount>, ClientError> {
        self.native_accounts_list().await
    }
    async fn account_scope_for(&self, id: &str) -> Result<String, ClientError> {
        self.profile()?;
        let mut session = self.session.lock().await;
        self.load(&mut session)?;
        let stored = session
            .stored
            .as_ref()
            .filter(|s| s.account_id == id)
            .ok_or_else(|| error(ErrorCode::Conflict, "账号已切换，请刷新后重试。"))?;
        if stored.user_id.is_empty() {
            return Err(error(
                ErrorCode::Conflict,
                "账号身份尚未确认，请读取网盘后重试。",
            ));
        }
        Ok(format!(
            "{:x}",
            Sha256::digest(format!("{}\0{}", self.profile()?.id, stored.user_id))
        ))
    }
    async fn account_switch(&self, id: &str) -> Result<AuthState, ClientError> {
        self.native_account_switch(id).await
    }
    async fn account_remove(&self, id: &str) -> Result<(), ClientError> {
        self.native_account_remove(id).await
    }
    async fn sign_in(&self, username: &str, password: &str) -> Result<SignInResult, ClientError> {
        self.profile()?;
        validate_login(username, password)?;
        let mut session = self.session.lock().await;
        self.load(&mut session)?;
        session.pending = None;
        self.exchange(&mut session, username, password).await
    }
    async fn verify_sign_in(
        &self,
        challenge_id: &str,
        code: &str,
        password: Option<&str>,
    ) -> Result<SignInResult, ClientError> {
        self.profile()?;
        if !code.is_empty() {
            return Err(unsupported(self.id()));
        }
        let mut session = self.session.lock().await;
        self.load(&mut session)?;
        let pending = session.pending.as_mut().ok_or_else(expired)?;
        if pending.id != challenge_id || pending.expires_at <= now() {
            return Err(error(
                ErrorCode::Conflict,
                "安全验证已过期或不属于此次登录，请返回重新登录。",
            ));
        }
        let password =
            password.ok_or_else(|| error(ErrorCode::Conflict, "请重新输入密码以完成登录。"))?;
        validate_login(&pending.username, password)?;
        if pending.verified.is_none() {
            pending.verified = pending.callback.take_verified()?;
        }
        let verified = pending.verified.as_ref().ok_or_else(|| {
            error(
                ErrorCode::Conflict,
                "尚未收到安全验证结果。请在浏览器完成验证后，再返回继续登录。",
            )
        })?;
        if verified.expires_at <= tokio::time::Instant::now() {
            return Err(error(
                ErrorCode::Conflict,
                "安全验证结果已过期，请返回重新登录。",
            ));
        }
        let token = verified.token.clone();
        let username = pending.username.clone();
        // The original password was discarded; only the newly supplied one is used.
        self.complete_sign_in(&mut session, &username, password, &token)
            .await
    }
    async fn logout(&self) -> Result<(), ClientError> {
        let mut session = self.session.lock().await;
        self.load(&mut session)?;
        if let Some(id) = session.stored.as_ref().map(|s| s.account_id.clone()) {
            self.remove_account_session(&mut session, &id, false)
        } else {
            self.resume_account_removals()?;
            session.invalidate_account_handles();
            session.expired = false;
            self.emit(AuthStatus::SignedOut, None);
            Ok(())
        }
    }
    async fn auth_status(&self) -> Result<AuthState, ClientError> {
        let mut session = self.session.lock().await;
        if self.supports_password_auth() {
            self.load(&mut session)?;
            if session
                .stored
                .as_ref()
                .is_some_and(|s| s.expires_at <= now() + 30)
            {
                self.refresh(&mut session).await?;
            }
        }
        if session
            .pending
            .as_ref()
            .is_some_and(|p| p.expires_at <= now())
        {
            session.pending = None;
        }
        Ok(self.state_of(&session))
    }
    async fn discover(&self) -> Result<CapabilityReport, ClientError> {
        let auth = self.auth_status().await;
        let status = if !self.supports_password_auth() || auth.is_err() {
            BackendStatus::Unavailable
        } else if auth.as_ref().is_ok_and(|s| s.authenticated) {
            BackendStatus::Ready
        } else {
            BackendStatus::NeedsLogin
        };
        let ready = status == BackendStatus::Ready;
        let operations = BTreeMap::from([
            (Operation::List, ready),
            (Operation::Stat, ready),
            (Operation::Search, ready),
            (Operation::Mkdir, ready),
            (Operation::Rename, ready),
            (Operation::Move, ready),
            (Operation::Copy, ready),
            (Operation::Trash, ready),
            (Operation::Star, ready),
            (Operation::Restore, ready),
            (Operation::Upload, ready),
            (Operation::Download, ready),
            (Operation::CloudTask, ready),
            (Operation::StreamUrl, false),
            (Operation::ReadRange, false),
            (Operation::Quota, ready),
            (Operation::SegmentDownload, ready),
            (Operation::FolderDownload, ready),
            (Operation::Duplicates, ready),
            (Operation::SpaceSummary, ready),
            (Operation::Share, ready),
            (Operation::LinkPreview, ready),
            (Operation::Archive, ready),
            (Operation::DeleteForever, ready),
            (Operation::MediaProbe, ready),
            (Operation::ClipDownload, ready),
            (Operation::QualityDownload, ready),
            (Operation::ExternalPlayer, ready),
        ]);
        let mut limitations = vec!["Direct client-protocol preview; live Free/Premium acceptance has not been completed.".into(),
            "Native metadata, verification, file operations and quota paths are implemented; live account acceptance remains separate. Range and media are not enabled.".into()];
        if !self.supports_password_auth() {
            limitations.push("Windows Credential Manager and a valid protocol profile are required; the normal Windows build includes the public Web login profile.".into());
        }
        if let Err(error) = auth {
            limitations.push(error.message);
        }
        Ok(CapabilityReport {
            backend: self.id(),
            status,
            operations,
            account_tier: Some(AccountTier::Unknown),
            version: Some("native-http-preview-0.2".into()),
            limitations,
            checked_at: timestamp(now()),
        })
    }
    async fn list(
        &self,
        parent: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<RemoteEntry>, ClientError> {
        wire::validate_id(parent)?;
        page_limit(Some(u32::try_from(limit).map_err(|_| wire::invalid())?))?;
        let previous = if let Some(id) = cursor {
            let session = self.session.lock().await;
            Some(
                session
                    .cursors
                    .iter()
                    .find(|c| {
                        c.id == id
                            && c.parent == parent
                            && c.limit == limit
                            && c.epoch == session.epoch
                    })
                    .cloned()
                    .ok_or_else(|| {
                        error(ErrorCode::Conflict, "分页游标已失效，请重新读取目录。")
                    })?,
            )
        } else {
            None
        };
        let mut url = url::Url::parse(&format!("{}/drive/v1/files", self.http.drive_origin))
            .map_err(|_| wire::invalid())?;
        url.query_pairs_mut()
            .append_pair("parent_id", if parent == "root" { "" } else { parent })
            .append_pair("limit", &limit.to_string())
            .append_pair(
                "filters",
                r#"{"trashed":{"eq":false},"phase":{"eq":"PHASE_TYPE_COMPLETE"}}"#,
            );
        if let Some(previous) = &previous {
            url.query_pairs_mut()
                .append_pair("page_token", &previous.token);
        }
        let (value, epoch) = self.get(url.as_str()).await?;
        let (items, next) = wire::list(&value, parent, limit)?;
        let mut session = self.session.lock().await;
        if session.epoch != epoch || previous.as_ref().is_some_and(|c| c.epoch != epoch) {
            return Err(expired());
        }
        let next_cursor = if let Some(token) = next {
            let mut seen = previous.map(|c| c.seen).unwrap_or_default();
            if !seen.insert(token.clone())
                || seen.len() > 256
                || seen.iter().map(String::len).sum::<usize>() > 32768
            {
                return Err(error(
                    ErrorCode::IntegrityFailed,
                    "服务返回重复或异常分页游标，未接受不完整目录。",
                ));
            }
            let id = uuid::Uuid::new_v4().to_string();
            if session.cursors.len() >= 128 {
                session.cursors.pop_front();
            }
            session.cursors.push_back(Cursor {
                id: id.clone(),
                parent: parent.into(),
                limit,
                token,
                seen,
                epoch,
            });
            Some(id)
        } else {
            None
        };
        Ok(Page { items, next_cursor })
    }
    async fn stat(&self, id: &str) -> Result<RemoteEntry, ClientError> {
        if shared_media::shared_id(id)?.is_some() {
            return Ok(self.encoded_share_info(id).await?.entry);
        }
        wire::validate_id(id)?;
        let (value, _) = self
            .get(&format!("{}/drive/v1/files/{id}", self.http.drive_origin))
            .await?;
        let entry = wire::entry(&value)?;
        if entry.id != id {
            return Err(wire::invalid());
        }
        Ok(entry)
    }
    async fn quota(&self) -> Result<QuotaReport, ClientError> {
        self.native_quota().await
    }
    async fn query_files(
        &self,
        query: &FileQuery,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<RemoteEntry>, ClientError> {
        self.native_query(query, cursor, limit).await
    }
    async fn mutate_files(
        &self,
        request: &FileMutation,
        account_scope: &str,
    ) -> Result<OperationResult, ClientError> {
        self.native_mutate(request, account_scope).await
    }
    async fn recovery_metadata(
        &self,
        id: &str,
        scope: &str,
    ) -> Result<RecoveryMetadata, ClientError> {
        self.native_recovery_metadata(id, scope).await
    }
    async fn create_from_content(
        &self,
        entry: &RemoteEntry,
        scope: &str,
    ) -> Result<ContentCreateReceipt, ClientError> {
        self.native_create_from_content(entry, scope).await
    }
    async fn list_shares(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<ShareInfo>, ClientError> {
        self.native_shares(cursor, limit).await
    }
    async fn cloud_create(
        &self,
        source: &str,
        parent: &str,
        scope: &str,
    ) -> Result<CloudTaskSnapshot, ClientError> {
        self.native_cloud_create(source, parent, scope).await
    }
    async fn cloud_task(&self, id: &str, scope: &str) -> Result<CloudTaskSnapshot, ClientError> {
        self.native_cloud_task(id, scope).await
    }
    async fn cloud_control(
        &self,
        id: &str,
        action: TransferAction,
        scope: &str,
    ) -> Result<(), ClientError> {
        self.native_cloud_control(id, action, scope).await
    }
    async fn cloud_preview(&self, source: &str) -> Result<CloudResourcePreview, ClientError> {
        self.native_cloud_preview(source).await
    }
    async fn share_create(
        &self,
        request: &CreateShareRequest,
        scope: &str,
    ) -> Result<ShareInfo, ClientError> {
        self.native_share_create(request, scope).await
    }
    async fn shares_cancel(&self, ids: &[String], scope: &str) -> Result<BulkResult, ClientError> {
        self.native_shares_cancel(ids, scope).await
    }
    async fn upload_prepare(
        &self,
        path: &str,
        parent: &str,
        scope: &str,
    ) -> Result<UploadPrepared, ClientError> {
        self.native_upload_prepare(path, parent, scope).await
    }
    async fn upload_open(
        &self,
        reference: &str,
        scope: &str,
    ) -> Result<UploadPrepared, ClientError> {
        self.native_upload_open(reference, scope).await
    }
    async fn upload_part(
        &self,
        reference: &str,
        part: u32,
        bytes: Vec<u8>,
        scope: &str,
    ) -> Result<String, ClientError> {
        self.native_upload_part(reference, part, bytes, scope).await
    }
    async fn upload_finish(
        &self,
        reference: &str,
        etags: &[String],
        scope: &str,
    ) -> Result<RemoteEntry, ClientError> {
        self.native_upload_finish(reference, etags, scope).await
    }
    async fn upload_forget(&self, reference: &str, scope: &str) -> Result<(), ClientError> {
        self.native_upload_forget(reference, scope).await
    }
    async fn archive_list(
        &self,
        id: &str,
        path: &str,
        password: Option<&str>,
    ) -> Result<ArchiveListing, ClientError> {
        self.native_archive_list(id, path, password).await
    }
    async fn archive_unlock(&self, id: &str, password: &str) -> Result<String, ClientError> {
        self.native_archive_unlock(id, password).await
    }
    async fn archive_extract(
        &self,
        id: &str,
        paths: &[String],
        parent: &str,
        password: Option<&str>,
        scope: &str,
    ) -> Result<ArchiveJob, ClientError> {
        self.native_archive_extract(id, paths, parent, password, scope)
            .await
    }
    async fn archive_job(&self, id: &str, scope: &str) -> Result<ArchiveJob, ClientError> {
        self.native_archive_job(id, scope).await
    }
    async fn share_open(&self, id: &str, code: &str) -> Result<SharedFolder, ClientError> {
        self.native_share_open(id, code).await
    }
    async fn share_folder(
        &self,
        reference: &str,
        parent: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<SharedFolder, ClientError> {
        self.native_share_folder(reference, parent, cursor, limit)
            .await
    }
    async fn share_restore(
        &self,
        reference: &str,
        ids: &[String],
        parent: &str,
        scope: &str,
    ) -> Result<BulkResult, ClientError> {
        self.native_share_restore(reference, ids, parent, scope)
            .await
    }
    async fn share_source_info(
        &self,
        access_ref: &str,
        id: &str,
    ) -> Result<nekopak_provider::DownloadInfo, ClientError> {
        self.native_share_source_info(access_ref, id).await
    }
    async fn share_retain(&self, access_ref: &str) -> Result<String, ClientError> {
        self.native_share_retain(access_ref).await
    }
    async fn share_release(&self, access_ref: &str) -> Result<(), ClientError> {
        self.native_share_release(access_ref).await
    }
    async fn share_chunk(
        &self,
        access_ref: &str,
        id: &str,
        identity: &str,
        scope: &str,
        start: u64,
        length: u64,
        observer: nekopak_provider::DownloadObserver,
    ) -> Result<nekopak_provider::DownloadChunk, ClientError> {
        self.native_share_chunk(access_ref, id, identity, scope, start, length, observer)
            .await
    }
    fn supports_downloads(&self) -> bool {
        true
    }
    fn configure_network(
        &self,
        mode: &str,
        proxy: Option<&str>,
        compatibility: bool,
    ) -> Result<(), ClientError> {
        self.http.set_network(mode, proxy, compatibility)
    }
    fn set_domain_blocking(&self, enabled: bool) -> Result<(), ClientError> {
        self.http.set_domain_blocking(enabled);
        Ok(())
    }
    async fn download_info(&self, id: &str) -> Result<nekopak_provider::DownloadInfo, ClientError> {
        if shared_media::shared_id(id)?.is_some() {
            return self.encoded_share_info(id).await;
        }
        self.native_download_info(id).await
    }
    async fn media_options(
        &self,
        id: &str,
    ) -> Result<Vec<nekopak_provider::Representation>, ClientError> {
        if shared_media::shared_id(id)?.is_some() {
            let info = self.encoded_share_info(id).await?;
            return Ok(vec![nekopak_provider::Representation {
                id: "original".into(),
                label: "原画".into(),
                width: None,
                height: None,
                size_bytes: Some(info.entry.size_bytes),
                container: "original".into(),
                readable: true,
                unreadable_reason: None,
            }]);
        }
        self.native_media_options(id).await
    }
    async fn media_info(
        &self,
        id: &str,
        quality: &str,
    ) -> Result<nekopak_provider::DownloadInfo, ClientError> {
        if shared_media::shared_id(id)?.is_some() {
            if quality != "original" {
                return Err(error(
                    ErrorCode::Unsupported,
                    "分享预览当前仅支持原始内容，未替换所选画质。",
                ));
            }
            return self.encoded_share_info(id).await;
        }
        self.native_media_info(id, quality).await
    }
    async fn media_chunk(
        &self,
        id: &str,
        quality: &str,
        identity: &str,
        scope: &str,
        start: u64,
        length: u64,
        observer: nekopak_provider::DownloadObserver,
    ) -> Result<nekopak_provider::DownloadChunk, ClientError> {
        if let Some((reference, remote)) = shared_media::shared_id(id)? {
            if quality != "original" {
                return Err(error(
                    ErrorCode::Unsupported,
                    "分享预览当前仅支持原始内容，未替换所选画质。",
                ));
            }
            return self
                .native_share_chunk(reference, remote, identity, scope, start, length, observer)
                .await;
        }
        self.native_media_chunk(id, quality, identity, scope, start, length, observer)
            .await
    }
    async fn download_chunk(
        &self,
        id: &str,
        identity: &str,
        scope: &str,
        start: u64,
        length: u64,
        observer: nekopak_provider::DownloadObserver,
    ) -> Result<nekopak_provider::DownloadChunk, ClientError> {
        if let Some((reference, remote)) = shared_media::shared_id(id)? {
            return self
                .native_share_chunk(reference, remote, identity, scope, start, length, observer)
                .await;
        }
        self.native_download_chunk(id, identity, scope, start, length, observer)
            .await
    }
}

impl NativeProvider {
    fn state_of(&self, session: &Session) -> AuthState {
        let state = if session.stored.is_some() {
            AuthStatus::SignedIn
        } else if session.pending.is_some() {
            AuthStatus::Pending
        } else if session.expired {
            AuthStatus::Expired
        } else {
            AuthStatus::SignedOut
        };
        AuthState {
            backend: self.id(),
            authenticated: session.stored.is_some(),
            account_id: session.stored.as_ref().map(|s| s.account_id.clone()),
            credential_owner: if self.supports_password_auth() {
                "os_vault"
            } else {
                "none"
            }
            .into(),
            account_tier: AccountTier::Unknown,
            state,
            display_name: session
                .stored
                .as_ref()
                .map(|s| mask_username(&s.display_name)),
            message: if !self.supports_password_auth() {
                "当前版本尚未配置可用的账号登录。"
            } else if session.stored.is_some() {
                "会话已保存到系统凭据库；账号等级尚未查询。"
            } else {
                "请输入邮箱和密码登录。"
            }
            .into(),
        }
    }
}

#[cfg(test)]
mod tests;
