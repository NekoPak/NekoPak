//! Durable remote-intent journal, separate from the existing task database schema.
use crate::store::storage_error;
use nekopak_domain::{ClientError, ErrorCode, OperationResult};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::{path::Path, sync::Mutex};

pub struct IntentStore {
    connection: Mutex<Connection>,
}
pub enum Intent {
    New,
    Completed(OperationResult),
}
const SCHEMA: &str = "CREATE TABLE intents (scope TEXT NOT NULL, key TEXT NOT NULL, fingerprint TEXT NOT NULL, outcome TEXT, PRIMARY KEY(scope,key));
CREATE TABLE downloads (id TEXT PRIMARY KEY, payload TEXT NOT NULL CHECK(json_valid(payload)));
CREATE TABLE settings (key TEXT PRIMARY KEY, payload TEXT NOT NULL CHECK(json_valid(payload)));
PRAGMA user_version=1;";
const MIGRATE_V2: &str = "CREATE TABLE records (scope TEXT NOT NULL, kind TEXT NOT NULL, id TEXT NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload)), PRIMARY KEY(scope,kind,id)); PRAGMA user_version=2;";
fn damaged() -> ClientError {
    ClientError::new(
        ErrorCode::IntegrityFailed,
        "操作记录损坏，已保留原文件；未自动清空或重发请求。",
    )
}

impl IntentStore {
    pub fn record_get(
        &self,
        scope: &str,
        kind: &str,
        id: &str,
    ) -> Result<Option<String>, ClientError> {
        self.connection
            .lock()
            .map_err(|_| damaged())?
            .query_row(
                "SELECT payload FROM records WHERE scope=?1 AND kind=?2 AND id=?3",
                params![scope, kind, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage_error)
    }
    pub fn record_put(
        &self,
        scope: &str,
        kind: &str,
        id: &str,
        payload: &str,
    ) -> Result<(), ClientError> {
        self.connection.lock().map_err(|_|damaged())?.execute(
            "INSERT INTO records(scope,kind,id,payload) VALUES(?1,?2,?3,?4) ON CONFLICT(scope,kind,id) DO UPDATE SET payload=excluded.payload",
            params![scope,kind,id,payload]).map_err(storage_error)?;
        Ok(())
    }
    pub fn record_list(
        &self,
        scope: &str,
        kind: &str,
    ) -> Result<Vec<(String, String)>, ClientError> {
        let connection = self.connection.lock().map_err(|_| damaged())?;
        let mut statement = connection
            .prepare("SELECT id,payload FROM records WHERE scope=?1 AND kind=?2 ORDER BY id")
            .map_err(storage_error)?;
        let rows = statement
            .query_map(params![scope, kind], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(storage_error)?;
        rows.map(|r| r.map_err(storage_error)).collect()
    }
    pub fn record_remove(&self, scope: &str, kind: &str, id: &str) -> Result<(), ClientError> {
        self.connection
            .lock()
            .map_err(|_| damaged())?
            .execute(
                "DELETE FROM records WHERE scope=?1 AND kind=?2 AND id=?3",
                params![scope, kind, id],
            )
            .map_err(storage_error)?;
        Ok(())
    }
    /// Generic operation outcomes share the durable no-replay boundary with file mutations.
    pub fn lookup_value(
        &self,
        scope: &str,
        key: &str,
        fingerprint: &str,
    ) -> Result<Option<serde_json::Value>, ClientError> {
        let connection = self.connection.lock().map_err(|_| damaged())?;
        let existing: Option<(String, Option<String>)> = connection
            .query_row(
                "SELECT fingerprint,outcome FROM intents WHERE scope=?1 AND key=?2",
                params![scope, key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        let Some((old, outcome)) = existing else {
            return Ok(None);
        };
        if old != fingerprint {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "相同操作标识不能用于不同参数。",
            ));
        }
        let payload = outcome.ok_or_else(|| {
            ClientError::new(
                ErrorCode::Conflict,
                "上次操作结果未知，没有重复提交。请刷新核对。",
            )
        })?;
        let result: Result<serde_json::Value, ClientError> =
            serde_json::from_str(&payload).map_err(|_| damaged())?;
        result.map(Some)
    }
    pub fn begin_value(
        &self,
        scope: &str,
        key: &str,
        fingerprint: &str,
    ) -> Result<Option<serde_json::Value>, ClientError> {
        let mut connection = self.connection.lock().map_err(|_| damaged())?;
        let tx = connection.transaction().map_err(storage_error)?;
        let existing: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT fingerprint,outcome FROM intents WHERE scope=?1 AND key=?2",
                params![scope, key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some((old, outcome)) = existing {
            if old != fingerprint {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "相同操作标识不能用于不同参数。",
                ));
            }
            let payload = outcome.ok_or_else(|| {
                ClientError::new(
                    ErrorCode::Conflict,
                    "上次操作结果未知，没有重复提交。请刷新核对。",
                )
            })?;
            let result: Result<serde_json::Value, ClientError> =
                serde_json::from_str(&payload).map_err(|_| damaged())?;
            return result.map(Some);
        }
        tx.execute(
            "INSERT INTO intents(scope,key,fingerprint) VALUES(?1,?2,?3)",
            params![scope, key, fingerprint],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(None)
    }
    pub fn finish_value(
        &self,
        scope: &str,
        key: &str,
        result: &Result<serde_json::Value, ClientError>,
    ) -> Result<(), ClientError> {
        let connection = self.connection.lock().map_err(|_| damaged())?;
        if result.as_ref().is_err_and(|e| {
            e.code == ErrorCode::VerificationRequired
                && e.details.as_ref().is_some_and(|d| {
                    d.get("safe_to_resubmit") == Some(&serde_json::Value::Bool(true))
                })
        }) {
            connection
                .execute(
                    "DELETE FROM intents WHERE scope=?1 AND key=?2",
                    params![scope, key],
                )
                .map_err(storage_error)?;
        } else {
            let payload = serde_json::to_string(result).map_err(|_| damaged())?;
            connection
                .execute(
                    "UPDATE intents SET outcome=?3 WHERE scope=?1 AND key=?2 AND outcome IS NULL",
                    params![scope, key, payload],
                )
                .map_err(storage_error)?;
        }
        Ok(())
    }
    pub fn download_get(&self, id: &str) -> Result<Option<String>, ClientError> {
        self.connection
            .lock()
            .map_err(|_| damaged())?
            .query_row("SELECT payload FROM downloads WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage_error)
    }
    pub fn download_put(&self, id: &str, payload: &str) -> Result<(), ClientError> {
        self.connection.lock().map_err(|_|damaged())?.execute("INSERT INTO downloads(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![id,payload]).map_err(storage_error)?;
        Ok(())
    }
    pub fn settings_get(&self) -> Result<Option<String>, ClientError> {
        self.connection
            .lock()
            .map_err(|_| damaged())?
            .query_row("SELECT payload FROM settings WHERE key='app'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage_error)
    }
    pub fn settings_put(&self, payload: &str) -> Result<(), ClientError> {
        self.connection.lock().map_err(|_|damaged())?.execute("INSERT INTO settings(key,payload) VALUES('app',?1) ON CONFLICT(key) DO UPDATE SET payload=excluded.payload",[payload]).map_err(storage_error)?;
        Ok(())
    }
    /// Caller holds the main store's process ownership lock for this database namespace.
    pub fn open(path: &Path) -> Result<Self, ClientError> {
        let existing = path.try_exists().map_err(|_| damaged())?;
        if existing {
            let check = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(storage_error)?;
            let integrity: String = check
                .query_row("PRAGMA integrity_check", [], |r| r.get(0))
                .map_err(storage_error)?;
            let version: u32 = check
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .map_err(storage_error)?;
            if integrity != "ok" || !matches!(version, 1 | 2) {
                return Err(damaged());
            }
            let reference = Connection::open_in_memory().map_err(storage_error)?;
            reference.execute_batch(SCHEMA).map_err(storage_error)?;
            if version == 2 {
                reference.execute_batch(MIGRATE_V2).map_err(storage_error)?;
            }
            fn definitions(c: &Connection) -> Result<Vec<(String, String)>, ClientError> {
                let mut statement = c
                    .prepare("SELECT name,sql FROM sqlite_master WHERE type='table' ORDER BY name")
                    .map_err(storage_error)?;
                let rows = statement
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                    .map_err(storage_error)?;
                rows.map(|r| r.map_err(storage_error)).collect()
            }
            if definitions(&check)? != definitions(&reference)? {
                return Err(damaged());
            }
        }
        let mut connection = Connection::open(path).map_err(storage_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(2))
            .map_err(storage_error)?;
        connection
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")
            .map_err(storage_error)?;
        if !existing {
            let tx = connection.transaction().map_err(storage_error)?;
            tx.execute_batch(SCHEMA).map_err(storage_error)?;
            tx.execute_batch(MIGRATE_V2).map_err(storage_error)?;
            tx.commit().map_err(storage_error)?;
        } else if connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .map_err(storage_error)?
            == 1
        {
            let tx = connection.transaction().map_err(storage_error)?;
            tx.execute_batch(MIGRATE_V2).map_err(storage_error)?;
            tx.commit().map_err(storage_error)?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    pub fn begin(&self, scope: &str, key: &str, fingerprint: &str) -> Result<Intent, ClientError> {
        let mut connection = self.connection.lock().map_err(|_| damaged())?;
        let tx = connection.transaction().map_err(storage_error)?;
        let existing: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT fingerprint,outcome FROM intents WHERE scope=?1 AND key=?2",
                params![scope, key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some((old, outcome)) = existing {
            if old != fingerprint {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "相同操作标识不能用于不同参数。",
                ));
            }
            let Some(outcome) = outcome else {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "上一次操作结果尚未确认，请刷新云盘核对。为避免重复操作，没有重新提交。",
                ));
            };
            let result: Result<OperationResult, ClientError> =
                serde_json::from_str(&outcome).map_err(|_| damaged())?;
            let mut result = result?;
            result.replayed = true;
            return Ok(Intent::Completed(result));
        }
        tx.execute(
            "INSERT INTO intents(scope,key,fingerprint) VALUES(?1,?2,?3)",
            params![scope, key, fingerprint],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(Intent::New)
    }
    pub fn finish(
        &self,
        scope: &str,
        key: &str,
        result: &Result<OperationResult, ClientError>,
    ) -> Result<(), ClientError> {
        let connection = self.connection.lock().map_err(|_| damaged())?;
        if result.as_ref().is_err_and(|e| {
            e.code == ErrorCode::VerificationRequired
                && e.details.as_ref().is_some_and(|d| {
                    d.get("safe_to_resubmit") == Some(&serde_json::Value::Bool(true))
                })
        }) {
            // Explicit service verification rejection / preflight failure means the mutation was not applied.
            connection
                .execute(
                    "DELETE FROM intents WHERE scope=?1 AND key=?2",
                    params![scope, key],
                )
                .map_err(storage_error)?;
        } else {
            let payload = serde_json::to_string(result).map_err(|_| damaged())?;
            connection
                .execute(
                    "UPDATE intents SET outcome=?3 WHERE scope=?1 AND key=?2 AND outcome IS NULL",
                    params![scope, key, payload],
                )
                .map_err(storage_error)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nekopak_domain::MutateOp;
    #[test]
    fn v1_migration_preserves_settings_and_intents_but_missing_tables_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v1.db");
        let c = Connection::open(&path).unwrap();
        c.execute_batch(SCHEMA).unwrap();
        c.execute(
            "INSERT INTO settings(key,payload) VALUES('app','{\"language\":\"en\"}')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO intents(scope,key,fingerprint) VALUES('one','unknown','hash')",
            [],
        )
        .unwrap();
        drop(c);
        let store = IntentStore::open(&path).unwrap();
        assert_eq!(
            store.settings_get().unwrap().unwrap(),
            "{\"language\":\"en\"}"
        );
        assert!(store.begin_value("one", "unknown", "hash").is_err());
        store
            .record_put("one", "test", "id", "{\"ok\":true}")
            .unwrap();
        drop(store);
        let c = Connection::open(&path).unwrap();
        c.execute_batch("DROP TABLE records").unwrap();
        drop(c);
        assert!(IntentStore::open(&path).is_err());
    }
    #[test]
    fn remote_intents_survive_restart_and_never_replay_unknown_outcomes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("actions.db");
        let store = IntentStore::open(&path).unwrap();
        assert!(matches!(
            store.begin("account-a", "one", "same").unwrap(),
            Intent::New
        ));
        drop(store);
        let store = IntentStore::open(&path).unwrap();
        assert_eq!(
            store.begin("account-a", "one", "same").err().unwrap().code,
            ErrorCode::Conflict
        );
        assert!(matches!(
            store.begin("account-b", "one", "same").unwrap(),
            Intent::New
        ));
        let result = OperationResult {
            op: MutateOp::Rename,
            succeeded: vec!["file-1".into()],
            failed: vec![],
            created: vec![],
            replayed: false,
        };
        store.finish("account-b", "one", &Ok(result)).unwrap();
        drop(store);
        let store = IntentStore::open(&path).unwrap();
        let Intent::Completed(result) = store.begin("account-b", "one", "same").unwrap() else {
            panic!("must replay confirmed result")
        };
        assert!(result.replayed);
        assert!(store.begin("account-b", "one", "changed").is_err());
    }
}
