//! Local account switching. Every manifest and session lives in the OS credential vault.
//! The existing root credential remains authoritative for the active account.
use super::*;

// Even with deletion tombstones the manifest fits Windows' credential blob limit.
const MAX_ACCOUNTS: usize = 8;

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountIndex {
    schema_version: u32,
    accounts: Vec<AccountRecord>,
    #[serde(default)]
    removing: Vec<String>,
    #[serde(default)]
    preferences: AuthPreferences,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuthPreferences {
    pub(super) remember_account: bool,
    pub(super) restore_session: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) username: Option<String>,
}
impl Default for AuthPreferences {
    fn default() -> Self {
        Self {
            remember_account: true,
            restore_session: true,
            username: None,
        }
    }
}
impl AuthPreferences {
    fn validate(&self) -> Result<(), ClientError> {
        if self.username.as_ref().is_some_and(|name| {
            name.len() > 254
                || !name.contains('@')
                || name.chars().any(|c| c.is_control() || c.is_whitespace())
        }) {
            return Err(error(ErrorCode::Conflict, "记住的账号必须是有效邮箱。"));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountRecord {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    /// False reserves an entry before the active credential is written. A missing
    /// reserved entry after interruption has never contained a saved session.
    archived: bool,
}

pub(super) fn valid_account_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn decode(raw: &str) -> Result<StoredSession, ClientError> {
    let stored: StoredSession = serde_json::from_str(raw).map_err(|_| wire::invalid())?;
    stored.validate()?;
    Ok(stored)
}

fn encode(stored: &StoredSession) -> Result<Zeroizing<String>, ClientError> {
    Ok(Zeroizing::new(
        serde_json::to_string(stored).map_err(|_| wire::invalid())?,
    ))
}

impl AccountIndex {
    fn load(vault: &dyn SessionVault) -> Result<Self, ClientError> {
        let Some(raw) = vault.load_accounts()? else {
            return Ok(Self {
                schema_version: 1,
                ..Default::default()
            });
        };
        let index: Self = serde_json::from_str(&raw).map_err(|_| wire::invalid())?;
        index.preferences.validate()?;
        if index.schema_version != 1
            || index.accounts.len() > MAX_ACCOUNTS
            || index.removing.len() > MAX_ACCOUNTS
            || index
                .accounts
                .iter()
                .map(|a| &a.id)
                .collect::<BTreeSet<_>>()
                .len()
                != index.accounts.len()
            || index.removing.iter().collect::<BTreeSet<_>>().len() != index.removing.len()
            || index.accounts.iter().any(|a| {
                !valid_account_id(&a.id)
                    || a.scope
                        .as_ref()
                        .is_some_and(|scope| !valid_account_id(scope))
            })
            || index
                .removing
                .iter()
                .any(|id| !index.accounts.iter().any(|a| &a.id == id))
        {
            return Err(wire::invalid());
        }
        Ok(index)
    }

    fn save(&self, vault: &dyn SessionVault) -> Result<(), ClientError> {
        let raw = Zeroizing::new(serde_json::to_string(self).map_err(|_| wire::invalid())?);
        vault.save_accounts(&raw)
    }
}

impl Session {
    /// Revoke all outstanding response handles, including same-account reauthentication.
    pub(super) fn invalidate_account_handles(&mut self) {
        self.epoch += 1;
        self.pending = None;
        self.cursors.clear();
        self.drive = Default::default();
        self.shares.clear();
        self.archives.clear();
    }
}

impl NativeProvider {
    pub(super) async fn account_preferences(&self) -> Result<Value, ClientError> {
        self.profile()?;
        let _session = self.session.lock().await;
        let index = AccountIndex::load(self.vault.as_ref())?;
        serde_json::to_value(index.preferences).map_err(|_| wire::invalid())
    }
    pub(super) async fn update_account_preferences(
        &self,
        value: &Value,
    ) -> Result<Value, ClientError> {
        self.profile()?;
        let mut preferences: AuthPreferences = serde_json::from_value(value.clone())
            .map_err(|_| error(ErrorCode::Conflict, "登录偏好格式无效。"))?;
        if !preferences.remember_account {
            preferences.username = None;
        }
        preferences.validate()?;
        let _session = self.session.lock().await;
        let mut index = AccountIndex::load(self.vault.as_ref())?;
        index.preferences = preferences;
        index.save(self.vault.as_ref())?;
        serde_json::to_value(index.preferences).map_err(|_| wire::invalid())
    }
    pub(super) fn remember_username(&self, username: &str) -> Result<(), ClientError> {
        let mut index = AccountIndex::load(self.vault.as_ref())?;
        if index.preferences.remember_account {
            index.preferences.username = Some(username.into());
            index.preferences.validate()?;
            index.save(self.vault.as_ref())?;
        }
        Ok(())
    }
    fn stored_scope(&self, stored: &StoredSession) -> Result<Option<String>, ClientError> {
        Ok((!stored.user_id.is_empty()).then(|| {
            format!(
                "{:x}",
                Sha256::digest(format!(
                    "{}\0{}",
                    self.profile
                        .as_ref()
                        .map(|p| p.id.as_str())
                        .unwrap_or("unconfigured"),
                    stored.user_id
                ))
            )
        }))
    }

    fn ensure_account_id(
        &self,
        stored: &mut StoredSession,
        index: &AccountIndex,
    ) -> Result<(), ClientError> {
        if !stored.account_id.is_empty() {
            return Ok(());
        }
        let scope = self.stored_scope(stored)?;
        stored.account_id = if let Some(scope) = scope {
            index
                .accounts
                .iter()
                .find(|a| a.scope.as_ref() == Some(&scope))
                .map(|a| a.id.clone())
                .unwrap_or(scope)
        } else {
            // Legacy credentials without a remote subject retain a stable identity
            // until /user/me supplies it. No username or access token is hashed.
            format!(
                "{:x}",
                Sha256::digest(format!(
                    "legacy-account\0{}\0{}",
                    self.profile()?.id,
                    stored.device_id
                ))
            )
        };
        Ok(())
    }

    fn read_archived(&self, id: &str) -> Result<Option<StoredSession>, ClientError> {
        let stored = self
            .vault
            .load_account(id)?
            .map(|raw| decode(&raw))
            .transpose()?;
        if stored.as_ref().is_some_and(|s| s.account_id != id) {
            return Err(wire::invalid());
        }
        Ok(stored)
    }

    fn track_account(
        &self,
        index: &mut AccountIndex,
        stored: &StoredSession,
    ) -> Result<(), ClientError> {
        let scope = self.stored_scope(stored)?;
        if let Some(record) = index
            .accounts
            .iter_mut()
            .find(|a| a.id == stored.account_id)
        {
            if scope.is_some() {
                record.scope = scope;
            }
        } else {
            if index.accounts.len() >= MAX_ACCOUNTS {
                return Err(error(
                    ErrorCode::Conflict,
                    "最多保存 8 个账号，请先移除不再使用的账号。",
                ));
            }
            index.accounts.push(AccountRecord {
                id: stored.account_id.clone(),
                scope,
                archived: false,
            });
        }
        Ok(())
    }

    fn prune_empty_reservations(
        &self,
        index: &mut AccountIndex,
        active: Option<&str>,
    ) -> Result<(), ClientError> {
        let mut empty = BTreeSet::new();
        for record in &index.accounts {
            if !record.archived
                && active != Some(record.id.as_str())
                && self.read_archived(&record.id)?.is_none()
            {
                empty.insert(record.id.clone());
            }
        }
        index.accounts.retain(|a| !empty.contains(&a.id));
        Ok(())
    }

    /// Resume a recorded deletion before admitting a session after restart. The
    /// tombstone is written first, so failure never resurrects deleted tokens.
    fn finish_removals(&self, index: &mut AccountIndex) -> Result<(), ClientError> {
        for id in index.removing.clone() {
            let record = index
                .accounts
                .iter()
                .find(|a| a.id == id)
                .ok_or_else(wire::invalid)?;
            if let Some(raw) = self.vault.load()? {
                let mut active = decode(&raw)?;
                self.ensure_account_id(&mut active, index)?;
                if active.account_id == id {
                    self.vault.delete()?;
                }
            }
            self.vault.delete_account(&id)?;
            if let Some(scope) = &record.scope {
                // Upload entries retain their original namespace and are removed
                // only if their protected ownership metadata matches this account.
                for key in self.vault.aux_keys()? {
                    let Some(raw) = self.vault.load_aux(&key)? else {
                        continue;
                    };
                    #[derive(Deserialize)]
                    struct Owner {
                        scope: String,
                    }
                    let owner: Owner = serde_json::from_str(&raw).map_err(|_| wire::invalid())?;
                    if owner.scope == *scope {
                        self.vault.delete_aux(&key)?;
                    }
                }
            }
            index.accounts.retain(|a| a.id != id);
            index.removing.retain(|a| a != &id);
            index.save(self.vault.as_ref())?;
        }
        Ok(())
    }

    pub(super) fn load_account_session(&self, session: &mut Session) -> Result<(), ClientError> {
        if session.loaded {
            return Ok(());
        }
        let mut index = AccountIndex::load(self.vault.as_ref())?;
        self.finish_removals(&mut index)?;
        if let Some(raw) = self.vault.load()? {
            let mut stored = decode(&raw)?;
            let migrating = stored.account_id.is_empty();
            self.ensure_account_id(&mut stored, &index)?;
            self.track_account(&mut index, &stored)?;
            if !index.preferences.restore_session {
                // Archive before clearing activation: opting out of automatic
                // restore must not erase the only recoverable account session.
                self.vault
                    .save_account(&stored.account_id, &encode(&stored)?)?;
                index
                    .accounts
                    .iter_mut()
                    .find(|r| r.id == stored.account_id)
                    .ok_or_else(wire::invalid)?
                    .archived = true;
                index.save(self.vault.as_ref())?;
                self.vault.delete()?;
                session.loaded = true;
                return Ok(());
            }
            // Register before upgrading the original credential. A failed write
            // leaves its complete legacy session readable and recoverable.
            index.save(self.vault.as_ref())?;
            if migrating {
                self.vault.save(&encode(&stored)?)?;
            }
            session.device_id.clone_from(&stored.device_id);
            session.stored = Some(stored);
        }
        session.loaded = true;
        Ok(())
    }

    pub(super) fn commit_account_session(
        &self,
        session: &mut Session,
        mut stored: StoredSession,
    ) -> Result<(), ClientError> {
        stored.validate()?;
        let mut index = AccountIndex::load(self.vault.as_ref())?;
        if !index.removing.is_empty() {
            return Err(error(
                ErrorCode::Conflict,
                "账号凭据清理尚未完成，请重试移除操作。",
            ));
        }
        self.ensure_account_id(&mut stored, &index)?;
        self.prune_empty_reservations(
            &mut index,
            session.stored.as_ref().map(|s| s.account_id.as_str()),
        )?;
        self.track_account(&mut index, &stored)?;
        if let Some(previous) = &session.stored {
            self.track_account(&mut index, previous)?;
        }
        // All credentials are discoverable through the protected index before
        // writing a new slot. The base write below is the sole activation point.
        index.save(self.vault.as_ref())?;
        if let Some(previous) = &session.stored {
            if previous.account_id != stored.account_id {
                self.vault
                    .save_account(&previous.account_id, &encode(previous)?)?;
                let record = index
                    .accounts
                    .iter_mut()
                    .find(|a| a.id == previous.account_id)
                    .ok_or_else(wire::invalid)?;
                record.archived = true;
                index.save(self.vault.as_ref())?;
            }
        }
        self.vault.save(&encode(&stored)?)?;
        session.device_id.clone_from(&stored.device_id);
        session.stored = Some(stored);
        session.expired = false;
        Ok(())
    }

    pub(super) fn resume_account_removals(&self) -> Result<(), ClientError> {
        let mut index = AccountIndex::load(self.vault.as_ref())?;
        self.finish_removals(&mut index)
    }

    pub(super) fn remove_account_session(
        &self,
        session: &mut Session,
        id: &str,
        expired_session: bool,
    ) -> Result<(), ClientError> {
        if !valid_account_id(id) {
            return Err(error(ErrorCode::Conflict, "账号标识无效。"));
        }
        let mut index = AccountIndex::load(self.vault.as_ref())?;
        if !index.accounts.iter().any(|a| a.id == id) {
            return Err(error(ErrorCode::NotFound, "该本地账号已移除。"));
        }
        let active = session.stored.as_ref().is_some_and(|s| s.account_id == id);
        if !index.removing.iter().any(|a| a == id) {
            index.removing.push(id.into());
        }
        index.save(self.vault.as_ref())?;
        if active {
            session.invalidate_account_handles();
            session.stored = None;
            session.expired = expired_session;
            session.device_id = uuid::Uuid::new_v4().simple().to_string();
        }
        let result = self.finish_removals(&mut index);
        if active {
            self.emit(
                if expired_session {
                    AuthStatus::Expired
                } else {
                    AuthStatus::SignedOut
                },
                result.as_ref().err().cloned(),
            );
        }
        result
    }

    pub(super) async fn native_accounts_list(&self) -> Result<Vec<SavedAccount>, ClientError> {
        self.profile()?;
        let mut session = self.session.lock().await;
        self.load(&mut session)?;
        let index = AccountIndex::load(self.vault.as_ref())?;
        let mut accounts = Vec::new();
        for record in &index.accounts {
            if index.removing.contains(&record.id) {
                continue;
            }
            let active = session
                .stored
                .as_ref()
                .filter(|s| s.account_id == record.id);
            let archived;
            let stored = if let Some(active) = active {
                active
            } else {
                archived = self.read_archived(&record.id)?;
                match &archived {
                    Some(stored) => stored,
                    None if !record.archived => continue,
                    None => {
                        return Err(error(
                            ErrorCode::IntegrityFailed,
                            "已保存的账号会话缺失，未重置账号列表。",
                        ))
                    }
                }
            };
            accounts.push(SavedAccount {
                id: record.id.clone(),
                display_name: mask_username(&stored.display_name),
                active: active.is_some(),
                state: if stored.expires_at > now() + 30 {
                    AuthStatus::SignedIn
                } else {
                    AuthStatus::Expired
                },
            });
        }
        Ok(accounts)
    }

    pub(super) async fn native_account_switch(&self, id: &str) -> Result<AuthState, ClientError> {
        self.profile()?;
        if !valid_account_id(id) {
            return Err(error(ErrorCode::Conflict, "账号标识无效。"));
        }
        let mut session = self.session.lock().await;
        self.load(&mut session)?;
        if session.stored.as_ref().is_some_and(|s| s.account_id == id) {
            if session
                .stored
                .as_ref()
                .is_some_and(|s| s.expires_at <= now() + 30)
            {
                self.refresh(&mut session).await?;
            }
            session.invalidate_account_handles();
            let state = self.state_of(&session);
            self.emit(state.state, None);
            return Ok(state);
        }
        let index = AccountIndex::load(self.vault.as_ref())?;
        if !index.accounts.iter().any(|a| a.id == id) || index.removing.iter().any(|a| a == id) {
            return Err(error(ErrorCode::NotFound, "该本地账号已移除。"));
        }
        let mut next = self
            .read_archived(id)?
            .ok_or_else(|| error(ErrorCode::NotFound, "账号会话不可用，请重新登录。"))?;
        if next.expires_at <= now() + 30 {
            next = match self.refresh_stored(&next).await {
                Ok(next) => next,
                Err(failure) if failure.code == ErrorCode::AuthExpired => {
                    self.remove_account_session(&mut session, id, true)?;
                    return Err(failure);
                }
                Err(failure) => return Err(failure),
            };
            // A rotated refresh token must survive even if activating this account
            // later fails while archiving the previous account.
            self.vault.save_account(id, &encode(&next)?)?;
        }
        self.commit(&mut session, next)?;
        session.invalidate_account_handles();
        let state = self.state_of(&session);
        self.emit(AuthStatus::SignedIn, None);
        Ok(state)
    }

    pub(super) async fn native_account_remove(&self, id: &str) -> Result<(), ClientError> {
        self.profile()?;
        let mut session = self.session.lock().await;
        self.load(&mut session)?;
        self.remove_account_session(&mut session, id, false)
    }
}
