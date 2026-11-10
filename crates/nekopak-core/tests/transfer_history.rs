use nekopak_core::AppService;
use nekopak_domain::{ErrorCode, TransferAction};
use nekopak_provider::RequestContext;
use nekopak_provider_mock::Scenario;

#[tokio::test]
async fn cancelled_history_disappears_durably_without_deleting_files_or_idempotency() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let file = dir.path().join("kept.bin");
    std::fs::write(&file, b"keep").unwrap();
    let app = AppService::mock(&path, Scenario::Free);
    let ctx = RequestContext::default();
    let id = app
        .transfer_start("file-1", "C:/Mock/kept.bin", Some("history-intent"), &ctx)
        .await
        .unwrap()
        .task_id;
    assert_eq!(
        app.transfer_forget(&id, &ctx).await.unwrap_err().code,
        ErrorCode::Conflict
    );
    app.transfer_control(&id, TransferAction::Cancel).unwrap();
    app.transfer_forget(&id, &ctx).await.unwrap();
    assert!(app.transfers_list(None, None).unwrap().items.is_empty());
    assert_eq!(std::fs::read(&file).unwrap(), b"keep");
    drop(app);
    let app = AppService::mock(&path, Scenario::Free);
    assert!(app.transfers_list(None, None).unwrap().items.is_empty());
    assert_eq!(
        app.transfer_start("file-1", "C:/Mock/kept.bin", Some("history-intent"), &ctx)
            .await
            .unwrap()
            .task_id,
        id
    );
    assert!(app.transfers_list(None, None).unwrap().items.is_empty());
}

#[tokio::test]
async fn bulk_history_cleanup_leaves_active_tasks_visible() {
    let dir = tempfile::tempdir().unwrap();
    let app = AppService::mock(&dir.path().join("tasks.db"), Scenario::Free);
    let ctx = RequestContext::default();
    let first = app
        .transfer_start("file-1", "C:/Mock/a.bin", Some("a"), &ctx)
        .await
        .unwrap()
        .task_id;
    let second = app
        .transfer_start("file-1", "C:/Mock/b.bin", Some("b"), &ctx)
        .await
        .unwrap()
        .task_id;
    app.transfer_control(&first, TransferAction::Cancel)
        .unwrap();
    assert_eq!(app.transfer_clear_finished(&ctx).await.unwrap(), 1);
    assert_eq!(
        app.transfers_list(None, None)
            .unwrap()
            .items
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        vec![second]
    );
}
