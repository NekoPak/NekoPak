//! Per-user OS credentials. No plaintext fallback. Android files contain only
//! authenticated ciphertext sealed by a non-exportable Android Keystore key.
use crate::error;
use nekopak_domain::{ClientError, ErrorCode};
use zeroize::Zeroizing;

pub(crate) trait SessionVault: Send + Sync {
    fn load(&self) -> Result<Option<Zeroizing<String>>, ClientError>;
    fn save(&self, value: &str) -> Result<(), ClientError>;
    fn delete(&self) -> Result<(), ClientError>;
    fn load_accounts(&self) -> Result<Option<Zeroizing<String>>, ClientError> {
        Err(unavailable())
    }
    fn save_accounts(&self, _value: &str) -> Result<(), ClientError> {
        Err(unavailable())
    }
    fn load_account(&self, _id: &str) -> Result<Option<Zeroizing<String>>, ClientError> {
        Err(unavailable())
    }
    fn save_account(&self, _id: &str, _value: &str) -> Result<(), ClientError> {
        Err(unavailable())
    }
    fn delete_account(&self, _id: &str) -> Result<(), ClientError> {
        Err(unavailable())
    }
    fn aux_keys(&self) -> Result<Vec<String>, ClientError> {
        Err(unavailable())
    }
    fn load_aux(&self, _key: &str) -> Result<Option<Zeroizing<String>>, ClientError> {
        Err(unavailable())
    }
    fn save_aux(&self, _key: &str, _value: &str) -> Result<(), ClientError> {
        Err(unavailable())
    }
    fn delete_aux(&self, _key: &str) -> Result<(), ClientError> {
        Err(unavailable())
    }
    #[cfg(test)]
    fn clear_aux(&self) -> Result<(), ClientError> {
        Ok(())
    }
}

fn unavailable() -> ClientError {
    error(
        ErrorCode::PermissionDenied,
        "系统凭据库不可用，未保存会话。请检查当前用户的凭据管理器后重试。",
    )
}

pub(crate) struct OsVault {
    pub target: String,
}

// Auxiliary index updates are read/modify/write transactions within this
// process. Session locks alone do not cover concurrent transfer workers.
static AUXILIARY_UPDATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl OsVault {
    fn account(&self, id: &str) -> Result<Self, ClientError> {
        if !crate::accounts::valid_account_id(id) {
            return Err(unavailable());
        }
        Ok(Self {
            target: format!("{}/accounts/{id}", self.target),
        })
    }
    fn accounts_index(&self) -> Self {
        Self {
            target: format!("{}/accounts/index", self.target),
        }
    }
    fn auxiliary(&self, key: &str) -> Result<Self, ClientError> {
        if key.is_empty()
            || key.len() > 64
            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(unavailable());
        }
        Ok(Self {
            target: format!("{}/uploads/{key}", self.target),
        })
    }
    fn auxiliary_index(&self) -> Self {
        Self {
            target: format!("{}/uploads/index", self.target),
        }
    }
    fn auxiliary_keys(&self) -> Result<Vec<String>, ClientError> {
        let keys: Vec<String> = self
            .auxiliary_index()
            .load()?
            .map(|s| serde_json::from_str(&s).map_err(|_| unavailable()))
            .transpose()
            .map(|v| v.unwrap_or_default())?;
        if keys.len() > 32
            || keys.iter().collect::<std::collections::BTreeSet<_>>().len() != keys.len()
        {
            return Err(unavailable());
        }
        for key in &keys {
            self.auxiliary(key)?;
        }
        Ok(keys)
    }
}

#[cfg(target_os = "android")]
#[path = "vault_android.rs"]
mod android;

impl SessionVault for OsVault {
    fn load(&self) -> Result<Option<Zeroizing<String>>, ClientError> {
        self.platform_load()
    }
    fn save(&self, value: &str) -> Result<(), ClientError> {
        self.platform_save(value)
    }
    fn delete(&self) -> Result<(), ClientError> {
        self.platform_delete()
    }
    fn load_accounts(&self) -> Result<Option<Zeroizing<String>>, ClientError> {
        self.accounts_index().load()
    }
    fn save_accounts(&self, value: &str) -> Result<(), ClientError> {
        self.accounts_index().save(value)
    }
    fn load_account(&self, id: &str) -> Result<Option<Zeroizing<String>>, ClientError> {
        self.account(id)?.load()
    }
    fn save_account(&self, id: &str, value: &str) -> Result<(), ClientError> {
        self.account(id)?.save(value)
    }
    fn delete_account(&self, id: &str) -> Result<(), ClientError> {
        self.account(id)?.delete()
    }
    fn aux_keys(&self) -> Result<Vec<String>, ClientError> {
        self.auxiliary_keys()
    }
    fn load_aux(&self, key: &str) -> Result<Option<Zeroizing<String>>, ClientError> {
        self.auxiliary(key)?.load()
    }
    fn save_aux(&self, key: &str, value: &str) -> Result<(), ClientError> {
        let _guard = AUXILIARY_UPDATE.lock().map_err(|_| unavailable())?;
        let item = self.auxiliary(key)?;
        let mut keys = self.auxiliary_keys()?;
        if !keys.iter().any(|k| k == key) {
            if keys.len() >= 32 {
                return Err(error(
                    ErrorCode::Conflict,
                    "保留的传输来源过多，请先完成或取消已有任务。",
                ));
            }
            keys.push(key.into());
        }
        // Index first: a crash can leave a harmless missing item, never an untracked secret.
        self.auxiliary_index()
            .save(&serde_json::to_string(&keys).map_err(|_| unavailable())?)?;
        item.save(value)
    }
    fn delete_aux(&self, key: &str) -> Result<(), ClientError> {
        let _guard = AUXILIARY_UPDATE.lock().map_err(|_| unavailable())?;
        self.auxiliary(key)?.delete()?;
        let mut keys = self.auxiliary_keys()?;
        keys.retain(|k| k != key);
        if keys.is_empty() {
            self.auxiliary_index().delete()
        } else {
            self.auxiliary_index()
                .save(&serde_json::to_string(&keys).map_err(|_| unavailable())?)
        }
    }
    #[cfg(test)]
    fn clear_aux(&self) -> Result<(), ClientError> {
        let _guard = AUXILIARY_UPDATE.lock().map_err(|_| unavailable())?;
        for key in self.auxiliary_keys()? {
            self.auxiliary(&key)?.delete()?;
        }
        self.auxiliary_index().delete()
    }
}

/// Debug APK acceptance hook. The caller must explicitly opt in on an isolated
/// device; no frontend command or production build exposes this operation.
#[cfg(all(target_os = "android", debug_assertions))]
pub(crate) fn android_self_test() -> Result<(), ClientError> {
    const ACCOUNT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const AUXILIARY: &str = "synthetic-upload";
    let vault = OsVault {
        target: format!("NekoPak/android-offline-selftest/{}", uuid::Uuid::new_v4()),
    };
    struct Cleanup<'a>(&'a OsVault);
    impl Cleanup<'_> {
        fn clean(&self) -> Result<(), ClientError> {
            // Evaluate every cleanup attempt even when one fails. Only the
            // exact five records created in this unique namespace are touched.
            let results = [
                self.0.delete(),
                self.0.accounts_index().delete(),
                self.0.account(ACCOUNT).and_then(|item| item.delete()),
                self.0.auxiliary(AUXILIARY).and_then(|item| item.delete()),
                self.0.auxiliary_index().delete(),
            ];
            if results.iter().all(Result::is_ok) {
                Ok(())
            } else {
                Err(unavailable())
            }
        }
    }
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            let _ = self.clean();
        }
    }
    fn ensure(condition: bool) -> Result<(), ClientError> {
        if condition {
            Ok(())
        } else {
            Err(unavailable())
        }
    }
    let cleanup = Cleanup(&vault);
    ensure(vault.load()?.is_none())?;
    vault.save("synthetic-jni-session-one")?;
    ensure(vault.load()?.as_deref().map(String::as_str) == Some("synthetic-jni-session-one"))?;
    vault.save("synthetic-jni-session-two-测试")?;
    let reopened = OsVault {
        target: vault.target.clone(),
    };
    ensure(
        reopened.load()?.as_deref().map(String::as_str) == Some("synthetic-jni-session-two-测试"),
    )?;
    vault.save_accounts("synthetic-jni-account-index")?;
    ensure(
        vault.load_accounts()?.as_deref().map(String::as_str)
            == Some("synthetic-jni-account-index"),
    )?;
    vault.save_account(ACCOUNT, "synthetic-jni-account-session")?;
    ensure(
        reopened
            .load_account(ACCOUNT)?
            .as_deref()
            .map(String::as_str)
            == Some("synthetic-jni-account-session"),
    )?;
    ensure(vault.save_account("../invalid", "synthetic").is_err())?;
    vault.save_aux(AUXILIARY, "synthetic-jni-transfer-source")?;
    ensure(reopened.aux_keys()? == [AUXILIARY])?;
    ensure(
        reopened.load_aux(AUXILIARY)?.as_deref().map(String::as_str)
            == Some("synthetic-jni-transfer-source"),
    )?;
    vault.delete_aux(AUXILIARY)?;
    ensure(reopened.load_aux(AUXILIARY)?.is_none() && reopened.aux_keys()?.is_empty())?;
    vault.delete_account(ACCOUNT)?;
    ensure(reopened.load_account(ACCOUNT)?.is_none())?;
    cleanup.clean()?;
    ensure(reopened.load()?.is_none() && reopened.load_accounts()?.is_none())?;
    Ok(())
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_NOT_FOUND},
        Security::Credentials::{
            CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW,
            CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
        },
    };
    use zeroize::Zeroize;

    struct Allocation(*mut CREDENTIALW);
    impl Drop for Allocation {
        fn drop(&mut self) {
            // SAFETY: CredReadW owns this allocation; its documented blob is writable
            // until CredFree. Clear its secret bytes before releasing it once.
            unsafe {
                let credential = &mut *self.0;
                if !credential.CredentialBlob.is_null()
                    && credential.CredentialBlobSize <= CRED_MAX_CREDENTIAL_BLOB_SIZE
                {
                    std::slice::from_raw_parts_mut(
                        credential.CredentialBlob,
                        credential.CredentialBlobSize as usize,
                    )
                    .zeroize();
                }
                CredFree(self.0.cast());
            }
        }
    }

    impl OsVault {
        fn target(&self) -> Result<Vec<u16>, ClientError> {
            if self.target.is_empty() || self.target.len() > 1024 || self.target.contains('\0') {
                return Err(unavailable());
            }
            Ok(self.target.encode_utf16().chain(Some(0)).collect())
        }
    }
    impl OsVault {
        pub(super) fn platform_load(&self) -> Result<Option<Zeroizing<String>>, ClientError> {
            let target = self.target()?;
            let mut pointer = ptr::null_mut();
            // SAFETY: target is NUL terminated; pointer is an output owned by CredFree.
            if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pointer) } == 0 {
                return if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                    Ok(None)
                } else {
                    Err(unavailable())
                };
            }
            if pointer.is_null() {
                return Err(unavailable());
            }
            let allocation = Allocation(pointer);
            // SAFETY: allocation remains alive throughout the bounded read.
            let credential = unsafe { &*allocation.0 };
            if credential.CredentialBlob.is_null()
                || credential.CredentialBlobSize == 0
                || credential.CredentialBlobSize > CRED_MAX_CREDENTIAL_BLOB_SIZE
            {
                return Err(unavailable());
            }
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
            };
            let text = std::str::from_utf8(bytes).map_err(|_| unavailable())?;
            Ok(Some(Zeroizing::new(text.to_owned())))
        }
        pub(super) fn platform_save(&self, value: &str) -> Result<(), ClientError> {
            if value.is_empty() || value.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
                return Err(unavailable());
            }
            let mut target = self.target()?;
            let mut username: Vec<u16> = "NekoPak".encode_utf16().chain(Some(0)).collect();
            let mut bytes = Zeroizing::new(value.as_bytes().to_vec());
            let credential = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: target.as_mut_ptr(),
                CredentialBlobSize: bytes.len() as u32,
                CredentialBlob: bytes.as_mut_ptr(),
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                UserName: username.as_mut_ptr(),
                ..Default::default()
            };
            // SAFETY: CredWriteW copies these bounded buffers synchronously. All
            // pointers live through the call. Storage is scoped to the current user.
            if unsafe { CredWriteW(&credential, 0) } == 0 {
                return Err(unavailable());
            }
            Ok(())
        }
        pub(super) fn platform_delete(&self) -> Result<(), ClientError> {
            let target = self.target()?;
            // SAFETY: target is a validated NUL-terminated name owned by this app.
            if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0
                && unsafe { GetLastError() } != ERROR_NOT_FOUND
            {
                return Err(unavailable());
            }
            Ok(())
        }
    }
}

#[cfg(not(any(windows, target_os = "android")))]
impl OsVault {
    fn platform_load(&self) -> Result<Option<Zeroizing<String>>, ClientError> {
        let _ = &self.target;
        Err(unavailable())
    }
    fn platform_save(&self, _value: &str) -> Result<(), ClientError> {
        Err(unavailable())
    }
    fn platform_delete(&self) -> Result<(), ClientError> {
        Err(unavailable())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "explicit opt-in: writes only a unique synthetic Windows credential and deletes it"]
    fn windows_vault_roundtrip_and_delete() {
        let vault = OsVault {
            target: format!("NekoPak/test/{}", uuid::Uuid::new_v4()),
        };
        struct Cleanup<'a>(&'a OsVault);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                let _ = self.0.clear_aux();
                let _ = self.0.delete_account(&"a".repeat(64));
                let _ = self.0.accounts_index().delete();
                let _ = self.0.delete();
            }
        }
        let _cleanup = Cleanup(&vault);
        assert!(vault.load().unwrap().is_none());
        vault.save("synthetic-session-one").unwrap();
        assert_eq!(
            vault.load().unwrap().unwrap().as_str(),
            "synthetic-session-one"
        );
        vault.save("synthetic-session-two").unwrap();
        assert_eq!(
            vault.load().unwrap().unwrap().as_str(),
            "synthetic-session-two"
        );
        vault.delete().unwrap();
        let account_id = "a".repeat(64);
        vault.save_accounts("synthetic-account-index").unwrap();
        vault
            .save_account(&account_id, "synthetic-account-session")
            .unwrap();
        assert_eq!(
            vault.load_accounts().unwrap().unwrap().as_str(),
            "synthetic-account-index"
        );
        assert_eq!(
            vault.load_account(&account_id).unwrap().unwrap().as_str(),
            "synthetic-account-session"
        );
        assert!(vault.save_account("../invalid", "synthetic").is_err());
        vault.delete_account(&account_id).unwrap();
        assert!(vault.load_account(&account_id).unwrap().is_none());
        vault
            .save_aux("synthetic-upload-1", "synthetic-only-upload-secret")
            .unwrap();
        assert_eq!(
            vault
                .load_aux("synthetic-upload-1")
                .unwrap()
                .unwrap()
                .as_str(),
            "synthetic-only-upload-secret"
        );
        let reopened = OsVault {
            target: vault.target.clone(),
        };
        assert!(reopened.load_aux("synthetic-upload-1").unwrap().is_some());
        reopened.clear_aux().unwrap();
        assert!(vault.load_aux("synthetic-upload-1").unwrap().is_none());
        assert!(vault.load().unwrap().is_none());
        vault.delete().unwrap();
    }
}
