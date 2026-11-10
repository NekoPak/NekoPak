//! Account-scoped DRIVE verification. Public protocol evidence: DRIVE_PROTOCOL_RESEARCH.md.
use super::*;
use std::time::Duration;

#[derive(Default)]
pub(super) struct DriveCaptcha {
    pub token: Option<VerifiedCaptcha>,
    pending: Option<DriveChallenge>,
}
impl DriveCaptcha {
    pub fn pending_id(&self) -> Option<&str> {
        self.pending.as_ref().map(|p| p.id.as_str())
    }
}

struct DriveChallenge {
    id: String,
    action: String,
    expires_at: i64,
    url: Option<String>,
    callback: Option<HumanCaptcha>,
}

fn required(id: &str) -> ClientError {
    let mut failure = error(
        ErrorCode::VerificationRequired,
        "PikPak 需要完成安全验证，当前登录会话会保留。",
    );
    failure.details = Some([("verification_id".into(), id.into())].into());
    failure
}

pub(super) fn action(method: &Method, url: &str) -> Result<String, ClientError> {
    let url = url::Url::parse(url).map_err(|_| wire::invalid())?;
    Ok(format!("{}:{}", method.as_str(), url.path()))
}

impl NativeProvider {
    pub(super) async fn drive_subject(&self, snapshot: &Snapshot) -> Result<String, ClientError> {
        if !snapshot.user_id.is_empty() {
            return Ok(snapshot.user_id.clone());
        }
        let mut session = self.session.lock().await;
        if session.epoch != snapshot.epoch {
            return Err(expired());
        }
        let stored = session.stored.as_ref().ok_or_else(expired)?;
        if !stored.user_id.is_empty() {
            return Ok(stored.user_id.clone());
        }
        let bytes = self
            .http
            .request::<Value>(
                Method::GET,
                &format!("{}/v1/user/me", self.http.auth_origin),
                AuthHeaders {
                    bearer: Some(&snapshot.access),
                    ..AuthHeaders::device(&snapshot.device)
                },
                None,
                false,
            )
            .await?;
        let response: Value = serde_json::from_slice(&bytes).map_err(|_| wire::invalid())?;
        let subject = response
            .get("sub")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
            .ok_or_else(wire::invalid)?
            .to_owned();
        let mut stored = session.stored.as_ref().ok_or_else(expired)?.clone();
        stored.user_id = subject.clone();
        self.commit(&mut session, stored)?;
        Ok(subject)
    }

    pub(super) async fn drive_token(
        &self,
        snapshot: &Snapshot,
        method: &Method,
        url: &str,
    ) -> Result<Zeroizing<String>, ClientError> {
        let subject = self.drive_subject(snapshot).await?;
        let action = action(method, url)?;
        let mut session = self.session.lock().await;
        if session.epoch != snapshot.epoch {
            return Err(expired());
        }
        if let Some(pending) = &session.drive.pending {
            if pending.expires_at > now() {
                return Err(required(&pending.id));
            }
            session.drive.pending = None;
        }
        if let Some(token) = &session.drive.token {
            if token.expires_at > tokio::time::Instant::now() {
                return Ok(token.token.clone());
            }
        }
        self.init_drive(&mut session, &subject, &action).await?;
        if let Some(pending) = &session.drive.pending {
            return Err(required(&pending.id));
        }
        session
            .drive
            .token
            .as_ref()
            .map(|t| t.token.clone())
            .ok_or_else(wire::invalid)
    }

    async fn init_drive(
        &self,
        session: &mut Session,
        subject: &str,
        action: &str,
    ) -> Result<(), ClientError> {
        let profile = self.profile()?;
        let request = json!({"client_id":profile.id,"device_id":session.device_id,"action":action,
            "meta":profile::drive_metadata(&profile.id, &session.device_id, subject)});
        let bytes = self
            .http
            .request(
                Method::POST,
                &format!("{}/v1/shield/captcha/init", self.http.auth_origin),
                AuthHeaders::device(&session.device_id),
                Some(&request),
                false,
            )
            .await?;
        #[derive(Deserialize)]
        struct Init {
            #[serde(default)]
            captcha_token: String,
            #[serde(default)]
            url: String,
            #[serde(default)]
            expires_in: Option<u64>,
        }
        let mut init: Init = serde_json::from_slice(&bytes).map_err(|_| wire::invalid())?;
        let token = Zeroizing::new(std::mem::take(&mut init.captcha_token));
        session.drive.token = None;
        if !init.url.is_empty() {
            validate_challenge_url(&init.url)?;
            let callback = HumanCaptcha::listen().await?;
            let url = callback.challenge_url(&init.url)?;
            session.drive.pending = Some(DriveChallenge {
                id: uuid::Uuid::new_v4().to_string(),
                action: action.into(),
                expires_at: now() + 300,
                url: Some(url),
                callback: Some(callback),
            });
        } else {
            if !valid_token(&token) {
                return Err(wire::invalid());
            }
            let ttl = init.expires_in.unwrap_or(60).min(3600);
            if ttl == 0 {
                return Err(wire::invalid());
            }
            session.drive.token = Some(VerifiedCaptcha {
                token,
                expires_at: tokio::time::Instant::now()
                    + Duration::from_secs(ttl.saturating_sub(30).max(1)),
            });
            session.drive.pending = None;
        }
        Ok(())
    }

    pub(super) async fn require_drive_verification(
        &self,
        snapshot: &Snapshot,
        method: &Method,
        url: &str,
    ) -> Result<ClientError, ClientError> {
        let action = action(method, url)?;
        let mut session = self.session.lock().await;
        if session.epoch != snapshot.epoch {
            return Err(expired());
        }
        let pending = session.drive.pending.get_or_insert_with(|| DriveChallenge {
            id: uuid::Uuid::new_v4().to_string(),
            action,
            expires_at: now() + 300,
            url: None,
            callback: None,
        });
        Ok(required(&pending.id))
    }

    pub(super) async fn drive_verification_status(
        &self,
        id: Option<&str>,
    ) -> Result<Option<VerificationState>, ClientError> {
        let mut session = self.session.lock().await;
        self.load(&mut session)?;
        let Some(pending) = session.drive.pending.as_ref() else {
            return Ok(None);
        };
        if id.is_some_and(|id| id != pending.id) {
            return Err(error(ErrorCode::Conflict, "安全验证已更新，请刷新后重试。"));
        }
        let expired = pending.expires_at <= now();
        if !expired {
            if let Some(callback) = &pending.callback {
                if let Some(verified) = callback.take_verified()? {
                    session.drive.token = Some(verified);
                }
            }
        }
        let pending = session.drive.pending.as_ref().ok_or_else(wire::invalid)?;
        let verified = session
            .drive
            .token
            .as_ref()
            .is_some_and(|t| t.expires_at > tokio::time::Instant::now());
        Ok(Some(VerificationState {
            verification_id: pending.id.clone(),
            state: if expired {
                "expired"
            } else if verified {
                "verified"
            } else {
                "pending"
            }
            .into(),
            expires_at: timestamp(pending.expires_at),
        }))
    }

    pub(super) async fn drive_verification_url(&self, id: &str) -> Result<String, ClientError> {
        let snapshot = self.snapshot().await?;
        let subject = self.drive_subject(&snapshot).await?;
        let mut session = self.session.lock().await;
        if session.epoch != snapshot.epoch {
            return Err(expired());
        }
        let pending = session
            .drive
            .pending
            .as_ref()
            .filter(|p| p.id == id)
            .ok_or_else(|| error(ErrorCode::Conflict, "安全验证已更新，请重试原操作。"))?;
        if pending.expires_at <= now() {
            return Err(error(
                ErrorCode::Conflict,
                "安全验证已过期，请重新读取以获取新的验证。",
            ));
        }
        if let Some(url) = &pending.url {
            return Ok(url.clone());
        }
        let action = pending.action.clone();
        self.init_drive(&mut session, &subject, &action).await?;
        if let Some(pending) = &mut session.drive.pending {
            pending.id = id.into();
            return pending.url.clone().ok_or_else(wire::invalid);
        }
        // The service issued a token without asking for human input.
        session.drive.pending = Some(DriveChallenge {
            id: id.into(),
            action,
            expires_at: now() + 300,
            url: None,
            callback: None,
        });
        Ok(String::new())
    }

    pub(super) async fn finish_drive_verification(&self, id: &str) -> Result<(), ClientError> {
        let status = self
            .drive_verification_status(Some(id))
            .await?
            .ok_or_else(|| error(ErrorCode::Conflict, "当前没有待完成的安全验证。"))?;
        if status.state != "verified" {
            return Err(error(
                ErrorCode::VerificationRequired,
                "尚未收到有效验证结果，请完成浏览器验证后再继续。",
            ));
        }
        let mut session = self.session.lock().await;
        if session.drive.pending.as_ref().is_none_or(|p| p.id != id) {
            return Err(expired());
        }
        if session
            .drive
            .token
            .as_ref()
            .is_none_or(|token| token.expires_at <= tokio::time::Instant::now())
        {
            return Err(error(
                ErrorCode::VerificationRequired,
                "安全验证结果已失效，请重新获取验证。",
            ));
        }
        session.drive.pending = None;
        Ok(())
    }
}
