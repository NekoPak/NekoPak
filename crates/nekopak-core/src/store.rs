use crate::now;
use nekopak_domain::*;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use std::{
    fs::{File, OpenOptions},
    path::Path,
    sync::Mutex,
    time::Duration,
};

pub const SCHEMA_VERSION: u32 = 1;
pub struct Store {
    connection: Mutex<Connection>,
    // OS ownership lock releases on process death; stale sidecar files are harmless.
    _ownership: File,
}

pub fn storage_error(error: rusqlite::Error) -> ClientError {
    use rusqlite::ErrorCode as SqlCode;
    let (code, message) = match error.sqlite_error_code() {
        Some(SqlCode::DiskFull) => (
            ErrorCode::DiskFull,
            "The task database disk is full; existing data was preserved.",
        ),
        Some(SqlCode::DatabaseCorrupt | SqlCode::NotADatabase) => (
            ErrorCode::IntegrityFailed,
            "The task database is damaged; preserve it for recovery. It was not reset.",
        ),
        Some(SqlCode::DatabaseBusy | SqlCode::DatabaseLocked) => (
            ErrorCode::Conflict,
            "The task database is busy. Retry after the other operation finishes.",
        ),
        Some(SqlCode::PermissionDenied | SqlCode::ReadOnly | SqlCode::CannotOpen) => (
            ErrorCode::PermissionDenied,
            "The task database is not accessible.",
        ),
        _ => (
            ErrorCode::Internal,
            "The task database operation failed; no automatic reset was attempted.",
        ),
    };
    ClientError::new(code, message)
}
fn damaged() -> ClientError {
    ClientError::new(
        ErrorCode::IntegrityFailed,
        "Task storage failed validation; preserve the original database for recovery.",
    )
}
fn conflict() -> ClientError {
    ClientError::new(
        ErrorCode::Conflict,
        "The task changed; refresh its snapshot before applying another update.",
    )
}
fn decode(payload: String) -> Result<TransferTask, ClientError> {
    let task: TransferTask = serde_json::from_str(&payload).map_err(|_| damaged())?;
    if task
        .total_bytes
        .is_some_and(|total| task.transferred_bytes > total)
        || (task.state == TaskState::Completed
            && task.kind != TaskKind::CloudDownload
            && task.total_bytes != Some(task.transferred_bytes))
        || task.progress_percent.is_some_and(|p| p > 100)
        || task.id.is_empty()
        || task.revision.0 == 0
        || task
            .speed_bytes_per_sec
            .is_some_and(|speed| !speed.is_finite() || speed < 0.0)
    {
        return Err(damaged());
    }
    Ok(task)
}
fn encode(task: &TransferTask) -> Result<String, ClientError> {
    serde_json::to_string(task).map_err(|_| damaged())
}
fn locked() -> ClientError {
    ClientError::new(
        ErrorCode::Internal,
        "Task storage is unavailable; restart without deleting its database.",
    )
}

impl Store {
    /// Bind the auxiliary journal lifecycle to this already-validated task store.
    pub fn actions_initialized(&self) -> Result<bool, ClientError> {
        self.connection
            .lock()
            .map_err(|_| locked())?
            .query_row(
                "SELECT value FROM metadata WHERE key='actions_initialized'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map(|v| v == Some(1))
            .map_err(storage_error)
    }
    pub fn mark_actions_initialized(&self) -> Result<(), ClientError> {
        self.connection.lock().map_err(|_|locked())?.execute("INSERT INTO metadata(key,value) VALUES('actions_initialized',1) ON CONFLICT(key) DO UPDATE SET value=1",[]).map_err(storage_error)?;
        Ok(())
    }
    pub fn open(path: &Path) -> Result<Self, ClientError> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = parent.canonicalize().map_err(|_| {
            ClientError::new(
                ErrorCode::PermissionDenied,
                "The database directory is inaccessible.",
            )
        })?;
        let path = parent.join(path.file_name().ok_or_else(damaged)?);
        let mut lock_name = path.as_os_str().to_os_string();
        lock_name.push(".lock");
        let ownership = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_name)
            .map_err(|_| {
                ClientError::new(
                    ErrorCode::PermissionDenied,
                    "Cannot open the task database ownership lock.",
                )
            })?;
        ownership.try_lock().map_err(|_| ClientError::new(ErrorCode::Conflict, "Another NekoPak process owns this task database. Close that process before reopening."))?;
        let existing = path.try_exists().map_err(|_| {
            ClientError::new(
                ErrorCode::PermissionDenied,
                "Cannot inspect the task database.",
            )
        })?;
        if existing {
            // Read-only preflight BEFORE WAL, migrations or any task updates. Corruption must never become an empty queue.
            let check = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(storage_error)?;
            check
                .busy_timeout(Duration::from_secs(2))
                .map_err(storage_error)?;
            let integrity: String = check
                .query_row("PRAGMA integrity_check", [], |row| row.get(0))
                .map_err(storage_error)?;
            if integrity != "ok" {
                return Err(damaged());
            }
            let version: u32 = check
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .map_err(storage_error)?;
            if version > SCHEMA_VERSION {
                return Err(ClientError::new(
                    ErrorCode::Unsupported,
                    "The database was created by a newer NekoPak version.",
                ));
            }
            if version == 0 {
                // A pre-existing empty/truncated file is not a trusted first installation.
                return Err(damaged());
            } else {
                validate_schema(&check)?;
                let mut statement = check
                    .prepare("SELECT id, revision, payload FROM tasks")
                    .map_err(|_| damaged())?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })
                    .map_err(storage_error)?;
                let mut max_revision = 0;
                for row in rows {
                    let (id, revision, payload) = row.map_err(storage_error)?;
                    let task = decode(payload)?;
                    if task.id != id || revision <= 0 || task.revision.0 != revision as u64 {
                        return Err(damaged());
                    }
                    max_revision = max_revision.max(revision);
                }
                let revision: i64 = check
                    .query_row(
                        "SELECT value FROM metadata WHERE key='revision'",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|_| damaged())?;
                if revision < max_revision {
                    return Err(damaged());
                }
                let foreign_keys: u32 = check
                    .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
                        row.get(0)
                    })
                    .map_err(|_| damaged())?;
                if foreign_keys != 0 {
                    return Err(damaged());
                }
            }
        }
        let mut connection = Connection::open(&path).map_err(storage_error)?;
        connection
            .busy_timeout(Duration::from_secs(2))
            .map_err(storage_error)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
            )
            .map_err(storage_error)?;
        if !existing {
            let tx = connection.transaction().map_err(storage_error)?;
            tx.execute_batch(include_str!("../migrations/001_initial.sql"))
                .map_err(storage_error)?;
            tx.commit().map_err(storage_error)?;
        }
        let store = Self {
            connection: Mutex::new(connection),
            _ownership: ownership,
        };
        store.recover()?;
        Ok(store)
    }

    pub fn get(&self, id: &str) -> Result<TransferTask, ClientError> {
        let connection = self.connection.lock().map_err(|_| locked())?;
        get(&connection, id)
    }
    pub fn list(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<TransferTask>, ClientError> {
        let after = match cursor {
            None => 0,
            Some(value) => value
                .strip_prefix("t1:")
                .and_then(|value| value.parse::<i64>().ok())
                .filter(|value| *value >= 0)
                .ok_or_else(conflict)?,
        };
        let connection = self.connection.lock().map_err(|_| locked())?;
        let mut statement = connection
            .prepare("SELECT seq,payload FROM tasks WHERE seq > ?1 ORDER BY seq LIMIT ?2")
            .map_err(storage_error)?;
        let rows = statement
            .query_map(params![after, limit as i64 + 1], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage_error)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(storage_error)?);
        }
        let more = records.len() > limit;
        records.truncate(limit);
        let next_cursor = if more {
            records.last().map(|(seq, _)| format!("t1:{seq}"))
        } else {
            None
        };
        let items = records
            .into_iter()
            .map(|(_, payload)| decode(payload))
            .collect::<Result<_, _>>()?;
        Ok(Page { items, next_cursor })
    }

    /// Key+fingerprint and task creation commit together. Same key with different input is a conflict.
    pub fn insert(
        &self,
        mut task: TransferTask,
        idempotency: Option<(&str, &str)>,
    ) -> Result<TransferTask, ClientError> {
        let mut connection = self.connection.lock().map_err(|_| locked())?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        if let Some((key, fingerprint)) = idempotency {
            let existing: Option<(String, String)> = tx
                .query_row(
                    "SELECT task_id,fingerprint FROM idempotency WHERE key=?1",
                    [key],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(storage_error)?;
            if let Some((id, old_fingerprint)) = existing {
                if old_fingerprint != fingerprint {
                    return Err(ClientError::new(
                        ErrorCode::Conflict,
                        "This idempotency key was already used for a different request.",
                    ));
                }
                return get(&tx, &id);
            }
        }
        task.revision = next_revision(&tx)?;
        tx.execute(
            "INSERT INTO tasks(id,revision,payload) VALUES(?1,?2,?3)",
            params![task.id, task.revision.0 as i64, encode(&task)?],
        )
        .map_err(storage_error)?;
        if let Some((key, fingerprint)) = idempotency {
            tx.execute(
                "INSERT INTO idempotency(key,fingerprint,task_id) VALUES(?1,?2,?3)",
                params![key, fingerprint, task.id],
            )
            .map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)?;
        Ok(task)
    }

    pub fn lookup_key(
        &self,
        key: &str,
        fingerprint: &str,
    ) -> Result<Option<TransferTask>, ClientError> {
        let connection = self.connection.lock().map_err(|_| locked())?;
        let existing: Option<(String, String)> = connection
            .query_row(
                "SELECT task_id,fingerprint FROM idempotency WHERE key=?1",
                [key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        match existing {
            Some((id, old)) if old == fingerprint => get(&connection, &id).map(Some),
            Some(_) => Err(ClientError::new(
                ErrorCode::Conflict,
                "This idempotency key was already used for a different request.",
            )),
            None => Ok(None),
        }
    }

    pub fn control(&self, id: &str, action: TransferAction) -> Result<TransferTask, ClientError> {
        let task = self.get(id)?;
        let next = task.state.control(action)?;
        if next == task.state {
            return Ok(task);
        }
        self.update(&task, next, task.transferred_bytes.0, None)
    }

    pub fn update(
        &self,
        expected: &TransferTask,
        state: TaskState,
        bytes: u64,
        error: Option<ClientError>,
    ) -> Result<TransferTask, ClientError> {
        if state != expected.state && !expected.state.can_transition_to(state) {
            return Err(conflict());
        }
        if bytes < expected.transferred_bytes.0
            || expected.total_bytes.is_some_and(|total| bytes > total.0)
            || (state == TaskState::Completed && expected.total_bytes != Some(DecimalU64(bytes)))
        {
            return Err(ClientError::new(
                ErrorCode::IntegrityFailed,
                "Provider progress failed length or transition validation.",
            ));
        }
        let mut connection = self.connection.lock().map_err(|_| locked())?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let current = get(&tx, &expected.id)?;
        if current.revision != expected.revision {
            return Err(conflict());
        }
        let mut task = current;
        task.state = state;
        task.transferred_bytes = DecimalU64(bytes);
        task.error = error;
        task.speed_bytes_per_sec = None;
        task.updated_at = now();
        task.revision = next_revision(&tx)?;
        save(&tx, &task)?;
        tx.commit().map_err(storage_error)?;
        Ok(task)
    }

    /// Remote cloud progress is authoritative and may have no byte total.
    /// This never relaxes local-file completion checks.
    pub fn update_cloud(
        &self,
        expected: &TransferTask,
        snapshot: &CloudTaskSnapshot,
    ) -> Result<TransferTask, ClientError> {
        if expected.kind != TaskKind::CloudDownload
            || snapshot.id != expected.remote_id
            || snapshot.progress.is_some_and(|p| p > 100)
        {
            return Err(damaged());
        }
        let mut connection = self.connection.lock().map_err(|_| locked())?;
        let tx = connection.transaction().map_err(storage_error)?;
        let mut task = get(&tx, &expected.id)?;
        if task.revision != expected.revision {
            return Err(conflict());
        }
        task.state = snapshot.state;
        task.name = Some(snapshot.name.clone());
        task.error = snapshot.error.clone();
        task.total_bytes = snapshot.total_bytes;
        task.progress_percent = snapshot.progress;
        task.transferred_bytes = DecimalU64(match (snapshot.total_bytes, snapshot.progress) {
            (Some(total), _) if snapshot.state == TaskState::Completed => total.0,
            (Some(total), Some(progress)) => {
                (u128::from(total.0) * u128::from(progress) / 100) as u64
            }
            _ => 0,
        });
        task.updated_at = now();
        task.revision = next_revision(&tx)?;
        task.speed_bytes_per_sec = None;
        save(&tx, &task)?;
        tx.commit().map_err(storage_error)?;
        Ok(task)
    }

    pub fn active(&self) -> Result<Vec<TransferTask>, ClientError> {
        let connection = self.connection.lock().map_err(|_| locked())?;
        let mut statement = connection.prepare("SELECT payload FROM tasks WHERE json_extract(payload,'$.state') IN ('queued','running','retrying','verifying') ORDER BY revision").map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage_error)?;
        rows.map(|row| decode(row.map_err(storage_error)?))
            .collect()
    }

    fn recover(&self) -> Result<(), ClientError> {
        let mut connection = self.connection.lock().map_err(|_| locked())?;
        let tx = connection.transaction().map_err(storage_error)?;
        let mut tasks = {
            let mut statement = tx
                .prepare("SELECT payload FROM tasks")
                .map_err(storage_error)?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(storage_error)?;
            rows.map(|row| decode(row.map_err(storage_error)?))
                .collect::<Result<Vec<_>, _>>()?
        };
        for task in tasks.iter_mut().filter(|task| {
            matches!(
                task.state,
                TaskState::Running | TaskState::Retrying | TaskState::Verifying
            )
        }) {
            // Explicit recovery transition, not an ordinary user transition. Resume revalidates before completion.
            task.state = TaskState::Paused;
            task.error = Some(ClientError::new(ErrorCode::Conflict, "The previous process stopped during this task; progress was retained. Resume explicitly."));
            task.speed_bytes_per_sec = None;
            task.updated_at = now();
            task.revision = next_revision(&tx)?;
            save(&tx, task)?;
        }
        tx.commit().map_err(storage_error)
    }
}

fn validate_schema(check: &Connection) -> Result<(), ClientError> {
    // Validate all schema-v1 definitions, including uniqueness/FKs. Never reconstruct missing dedup state.
    let reference = Connection::open_in_memory().map_err(storage_error)?;
    reference
        .execute_batch(include_str!("../migrations/001_initial.sql"))
        .map_err(storage_error)?;
    fn definitions(connection: &Connection) -> Result<Vec<(String, String)>, ClientError> {
        let mut statement = connection.prepare("SELECT name,sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").map_err(|_| damaged())?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|_| damaged())?;
        rows.map(|row| {
            let (name, sql) = row.map_err(|_| damaged())?;
            Ok((
                name,
                sql.split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase(),
            ))
        })
        .collect()
    }
    if definitions(check)? != definitions(&reference)? {
        return Err(damaged());
    }
    Ok(())
}

fn get(connection: &Connection, id: &str) -> Result<TransferTask, ClientError> {
    let payload: Option<String> = connection
        .query_row("SELECT payload FROM tasks WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(storage_error)?;
    decode(payload.ok_or_else(|| {
        ClientError::new(ErrorCode::NotFound, "The transfer task does not exist.")
    })?)
}
fn next_revision(tx: &Transaction<'_>) -> Result<DecimalU64, ClientError> {
    let current: i64 = tx
        .query_row(
            "SELECT value FROM metadata WHERE key='revision'",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let revision = current.checked_add(1).ok_or_else(damaged)?;
    tx.execute(
        "UPDATE metadata SET value=?1 WHERE key='revision'",
        [revision],
    )
    .map_err(storage_error)?;
    Ok(DecimalU64(revision as u64))
}
fn save(tx: &Transaction<'_>, task: &TransferTask) -> Result<(), ClientError> {
    tx.execute(
        "UPDATE tasks SET revision=?1,payload=?2 WHERE id=?3",
        params![task.revision.0 as i64, encode(task)?, task.id],
    )
    .map_err(storage_error)?;
    Ok(())
}
