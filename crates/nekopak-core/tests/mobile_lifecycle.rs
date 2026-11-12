use nekopak_core::AppService;
use nekopak_domain::{TaskState, TransferAction};
use nekopak_provider::RequestContext;
use nekopak_provider_mock::Scenario;

#[tokio::test]
async fn background_pause_survives_restart_and_needs_explicit_resume() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let app = AppService::mock(&path, Scenario::Free);
    let task = app
        .transfer_start(
            "file-1",
            "C:/NekoPak-Mock/downloads",
            Some("mobile"),
            &RequestContext::default(),
        )
        .await
        .unwrap();
    app.tick().await.unwrap();
    app.tick().await.unwrap();
    let before = app.task(&task.task_id).unwrap();
    app.set_foreground(false);
    app.pause_local_transfers().unwrap();
    app.tick().await.unwrap();
    app.run_upload().await.unwrap();
    assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Paused);
    assert_eq!(
        app.task(&task.task_id).unwrap().transferred_bytes,
        before.transferred_bytes
    );
    app.set_foreground(true);
    app.tick().await.unwrap();
    assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Paused);
    drop(app);
    let app = AppService::mock(&path, Scenario::Free);
    assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Paused);
    app.transfer_control(&task.task_id, TransferAction::Resume)
        .unwrap();
    for _ in 0..16 {
        app.tick().await.unwrap();
    }
    assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Completed);
}

#[tokio::test]
async fn background_gate_blocks_new_tasks_before_the_pause_sweep() {
    let dir = tempfile::tempdir().unwrap();
    let app = AppService::mock(&dir.path().join("tasks.db"), Scenario::Free);
    app.set_foreground(false);
    let task = app
        .transfer_start(
            "file-1",
            "C:/NekoPak-Mock/downloads",
            Some("mobile"),
            &RequestContext::default(),
        )
        .await
        .unwrap();
    for _ in 0..3 {
        app.tick().await.unwrap();
    }
    assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Queued);
    assert_eq!(app.task(&task.task_id).unwrap().transferred_bytes.0, 0);
    app.pause_local_transfers().unwrap();
    assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Paused);
}

#[tokio::test]
async fn keep_alive_activity_counts_only_local_work_that_is_not_paused() {
    let dir = tempfile::tempdir().unwrap();
    let app = AppService::mock(&dir.path().join("tasks.db"), Scenario::Free);
    assert_eq!(app.local_transfer_activity().unwrap().active, 0);
    let task = app
        .transfer_start(
            "file-1",
            "C:/NekoPak-Mock/downloads",
            Some("activity"),
            &RequestContext::default(),
        )
        .await
        .unwrap();
    for _ in 0..3 {
        app.tick().await.unwrap();
    }
    let activity = app.local_transfer_activity().unwrap();
    assert_eq!(activity.active, 1);
    assert!(activity.percent.is_some_and(|percent| percent <= 100));
    app.transfer_control(&task.task_id, TransferAction::Pause)
        .unwrap();
    // A paused task must not keep a foreground service or wake lock alive.
    assert_eq!(app.local_transfer_activity().unwrap().active, 0);
    app.transfer_control(&task.task_id, TransferAction::Resume)
        .unwrap();
    for _ in 0..16 {
        app.tick().await.unwrap();
    }
    assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Completed);
    assert_eq!(app.local_transfer_activity().unwrap().active, 0);
}

#[test]
fn background_transfer_option_defaults_on_persists_and_rejects_non_booleans() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let app = AppService::mock(&path, Scenario::Free);
    assert!(app.background_transfers_enabled());
    assert!(app.app_status().features["background_transfers"]);
    assert!(app
        .settings_update(&serde_json::json!({"background_transfers":"no"}))
        .is_err());
    assert!(app.background_transfers_enabled());
    app.settings_update(&serde_json::json!({"background_transfers":false}))
        .unwrap();
    drop(app);
    let app = AppService::mock(&path, Scenario::Free);
    assert!(!app.background_transfers_enabled());
    assert_eq!(app.settings_get().unwrap()["background_transfers"], false);
}
