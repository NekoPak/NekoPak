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
