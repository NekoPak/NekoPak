//! Per-user OS credentials. No file-based or plaintext fallback.
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
    impl SessionVault for OsVault {
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
            for key in self.auxiliary_keys()? {
                self.auxiliary(&key)?.delete()?;
            }
            self.auxiliary_index().delete()
        }
        fn load(&self) -> Result<Option<Zeroizing<String>>, ClientError> {
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
        fn save(&self, value: &str) -> Result<(), ClientError> {
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
        fn delete(&self) -> Result<(), ClientError> {
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

#[cfg(not(windows))]
impl SessionVault for OsVault {
    fn load(&self) -> Result<Option<Zeroizing<String>>, ClientError> {
        let _ = &self.target;
        Err(unavailable())
    }
    fn save(&self, _value: &str) -> Result<(), ClientError> {
        Err(unavailable())
    }
    fn delete(&self) -> Result<(), ClientError> {
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
