use nekopak_core::{store::Store, validate_destination, validate_source, AppService};
use nekopak_domain::*;
use nekopak_provider::RequestContext;
use nekopak_provider_mock::Scenario;
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn context() -> RequestContext {
    RequestContext::default()
}
async fn start(app: &AppService) -> String {
    app.transfer_start(
        "file-1",
        "C:/NekoPak-Mock/file.bin",
        Some("test-download"),
        &context(),
    )
    .await
    .unwrap()
    .task_id
}

#[tokio::test]
async fn download_controls_and_event_revisions_are_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let app = AppService::mock(&path, Scenario::Free);
    let mut events = app.subscribe();
    let id = start(&app).await;
    app.tick().await.unwrap();
    app.tick().await.unwrap();
    app.transfer_control(&id, TransferAction::Pause).unwrap();
    let paused = app.task(&id).unwrap();
    app.tick().await.unwrap();
    assert_eq!(app.task(&id).unwrap(), paused);
    app.transfer_control(&id, TransferAction::Resume).unwrap();
    for _ in 0..16 {
        app.tick().await.unwrap();
    }
    let completed = app.task(&id).unwrap();
    assert_eq!(completed.state, TaskState::Completed);
    assert_eq!(completed.total_bytes, Some(completed.transferred_bytes));
    assert!(app.transfer_control(&id, TransferAction::Retry).is_err());
    let mut last = 0;
    while let Ok(event) = events.try_recv() {
        assert!(event.revision.0 > last);
        last = event.revision.0;
    }
    drop(app);
    let restored = AppService::mock(&path, Scenario::Free);
    assert_eq!(restored.task(&id).unwrap(), completed);
}

#[tokio::test]
async fn in_flight_restart_pauses_and_keeps_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let app = AppService::mock(&path, Scenario::Free);
    let id = start(&app).await;
    app.tick().await.unwrap();
    app.tick().await.unwrap();
    let before = app.task(&id).unwrap();
    drop(app);
    let restored = AppService::mock(&path, Scenario::Free);
    let after = restored.task(&id).unwrap();
    assert_eq!(after.state, TaskState::Paused);
    assert_eq!(after.transferred_bytes, before.transferred_bytes);
    assert!(after.revision > before.revision);
}

#[tokio::test]
async fn restart_above_disconnect_threshold_does_not_rewind_or_stop_worker() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let app = AppService::mock(&path, Scenario::DisconnectAt37);
    let id = start(&app).await;
    for _ in 0..6 {
        app.tick().await.unwrap();
    }
    assert_eq!(app.task(&id).unwrap().transferred_bytes.0, 37_000_000);
    app.transfer_control(&id, TransferAction::Retry).unwrap();
    for _ in 0..3 {
        app.tick().await.unwrap();
    }
    assert!(app.task(&id).unwrap().transferred_bytes.0 > 37_000_000);
    drop(app);
    let restored = AppService::mock(&path, Scenario::DisconnectAt37);
    restored
        .transfer_control(&id, TransferAction::Resume)
        .unwrap();
    for _ in 0..16 {
        restored.tick().await.unwrap();
    }
    assert_eq!(restored.task(&id).unwrap().state, TaskState::Completed);
}

#[tokio::test]
async fn faults_never_look_like_success_and_37_percent_retry_resumes() {
    for (scenario, code) in [
        (Scenario::QuotaExhausted, ErrorCode::QuotaExceeded),
        (Scenario::DiskFull, ErrorCode::DiskFull),
        (Scenario::IntegrityFailure, ErrorCode::IntegrityFailed),
        (Scenario::DisconnectAt37, ErrorCode::TransientNetwork),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::mock(&dir.path().join("tasks.db"), scenario);
        let id = start(&app).await;
        for _ in 0..16 {
            app.tick().await.unwrap();
        }
        let task = app.task(&id).unwrap();
        assert_eq!(task.state, TaskState::Failed);
        assert_eq!(task.error.unwrap().code, code);
        if scenario == Scenario::DisconnectAt37 {
            assert_eq!(task.transferred_bytes.0, 37_000_000);
            app.transfer_control(&id, TransferAction::Retry).unwrap();
            for _ in 0..16 {
                app.tick().await.unwrap();
            }
            assert_eq!(app.task(&id).unwrap().state, TaskState::Completed);
        }
    }
}

#[tokio::test]
async fn cloud_idempotency_survives_restart_and_never_persists_urls() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let app = AppService::mock(&path, Scenario::Free);
    let source = "https://example.com/file?token=PRIVATE_TOKEN_SENTINEL";
    let first = app
        .cloud_task_create(source, "root", "cloud-key", &context())
        .await
        .unwrap();
    let second = app
        .cloud_task_create(source, "root", "cloud-key", &context())
        .await
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(
        app.cloud_task_create("https://example.com/other", "root", "cloud-key", &context())
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    drop(app);
    let app = AppService::mock(&path, Scenario::Free);
    assert_eq!(
        app.cloud_task_create(source, "root", "cloud-key", &context())
            .await
            .unwrap(),
        first
    );
    assert_eq!(app.transfers_list(None, None).unwrap().items.len(), 1);
    drop(app);
    for file in fs::read_dir(dir.path()).unwrap() {
        let bytes = fs::read(file.unwrap().path()).unwrap();
        assert!(!bytes
            .windows(b"PRIVATE_TOKEN_SENTINEL".len())
            .any(|value| value == b"PRIVATE_TOKEN_SENTINEL"));
    }
}

#[tokio::test]
async fn concurrent_duplicate_requests_make_one_durable_task() {
    let dir = tempfile::tempdir().unwrap();
    let app = AppService::mock(&dir.path().join("tasks.db"), Scenario::Free);
    let first_context = context();
    let second_context = context();
    let (first, second) = tokio::join!(
        app.cloud_task_create(
            "https://example.com/file",
            "root",
            "same-key",
            &first_context
        ),
        app.cloud_task_create(
            "https://example.com/file",
            "root",
            "same-key",
            &second_context
        ),
    );
    assert_eq!(first.unwrap(), second.unwrap());
    assert_eq!(app.transfers_list(None, None).unwrap().items.len(), 1);
}

#[test]
fn corrupt_database_is_not_modified_or_silently_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let bytes = b"this is a damaged task database -- preserve it";
    fs::write(&path, bytes).unwrap();
    let app = AppService::mock(&path, Scenario::Free);
    assert_eq!(app.app_status().storage.status, "recovery_required");
    assert_eq!(
        app.transfers_list(None, None).unwrap_err().code,
        ErrorCode::IntegrityFailed
    );
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn zero_byte_database_is_preserved_for_explicit_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.db");
    fs::write(&path, b"").unwrap();
    let app = AppService::mock(&path, Scenario::Free);
    assert_eq!(
        app.app_status().storage.error.unwrap().code,
        ErrorCode::IntegrityFailed
    );
    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
}

#[tokio::test]
async fn second_instance_cannot_recover_tasks_owned_by_live_worker() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let first = AppService::mock(&path, Scenario::Free);
    let id = start(&first).await;
    first.tick().await.unwrap();
    let second = AppService::mock(&path, Scenario::Free);
    assert_eq!(
        second.app_status().storage.error.unwrap().code,
        ErrorCode::Conflict
    );
    assert_eq!(first.task(&id).unwrap().state, TaskState::Running);
    drop(first);
    let third = AppService::mock(&path, Scenario::Free);
    assert_eq!(third.task(&id).unwrap().state, TaskState::Paused);
}

#[tokio::test]
async fn missing_idempotency_schema_is_not_silently_recreated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let app = AppService::mock(&path, Scenario::Free);
    start(&app).await;
    drop(app);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute("DROP TABLE idempotency", []).unwrap();
    drop(connection);
    let before = fs::read(&path).unwrap();
    let app = AppService::mock(&path, Scenario::Free);
    assert_eq!(
        app.app_status().storage.error.unwrap().code,
        ErrorCode::IntegrityFailed
    );
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[tokio::test]
async fn invalid_task_record_is_preserved_and_settings_damage_is_independent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let app = AppService::mock(&path, Scenario::Free);
    let id = start(&app).await;
    drop(app);
    fs::write(dir.path().join("settings.json"), b"{broken").unwrap();
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get(&id).unwrap().id, id);
    drop(store);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("UPDATE tasks SET payload='{}'", []).unwrap();
    drop(db);
    let before = fs::read(&path).unwrap();
    assert!(Store::open(&path).is_err());
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn future_schema_is_rejected_without_downgrade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.pragma_update(None, "user_version", 999).unwrap();
    drop(db);
    let before = fs::read(&path).unwrap();
    assert!(Store::open(&path).is_err());
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn dangerous_paths_and_credential_urls_are_rejected() {
    for path in [
        "../x",
        "C:relative",
        "C:/../x",
        "C:/CON.txt",
        "C:/LPT1",
        "C:/file:stream",
        "C:/file. ",
        "\\\\?\\C:\\secret",
        "\\\\server\\share\\x",
    ] {
        assert!(validate_destination(path).is_err(), "{path}");
    }
    assert!(validate_destination("C:/downloads/中文 file.bin").is_ok());
    assert!(validate_destination("/tmp/nekopak/file.bin").is_ok());
    for url in [
        "file:///C:/secret",
        "https://user:password@example.com/file",
        "magnet:?xt=urn:btih:garbage",
        "javascript:alert(1)",
    ] {
        assert!(validate_source(url).is_err());
    }
    assert!(
        validate_source("magnet:?xt=urn:btih:0123456789012345678901234567890123456789").is_ok()
    );
}

#[tokio::test]
async fn stale_worker_update_cannot_undo_user_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let app = AppService::mock(&path, Scenario::Free);
    let id = start(&app).await;
    drop(app);
    let store = Store::open(&path).unwrap();
    let stale = store.get(&id).unwrap();
    store.control(&id, TransferAction::Cancel).unwrap();
    assert_eq!(
        store
            .update(&stale, TaskState::Running, 0, None)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(store.get(&id).unwrap().state, TaskState::Cancelled);
}

// Spawned only by the crash test; the parent kills this process while a transaction is open.
#[test]
fn crash_writer_child() {
    let Ok(path) = std::env::var("NEKOPAK_TEST_CRASH_DB") else {
        return;
    };
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("BEGIN IMMEDIATE; UPDATE tasks SET payload='{}';")
        .unwrap();
    fs::write(format!("{path}.ready"), b"transaction-open").unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[tokio::test]
async fn killed_process_rolls_back_uncommitted_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crash.db");
    let app = AppService::mock(&path, Scenario::Free);
    let id = start(&app).await;
    drop(app);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_writer_child", "--nocapture"])
        .env("NEKOPAK_TEST_CRASH_DB", &path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let ready = path.with_extension("db.ready");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let observed = ready.exists();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(observed, "crash writer did not reach open transaction");
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get(&id).unwrap().state, TaskState::Queued);
}
