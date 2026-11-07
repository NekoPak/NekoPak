use nekopak_core::AppService;
use nekopak_domain::TaskState;
use nekopak_provider::RequestContext;
use nekopak_provider_mock::Scenario;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let app = AppService::mock(&dir.path().join("smoke.db"), Scenario::Free);
    let context = RequestContext::default();
    assert_eq!(app.app_status().storage.status, "ready");
    let files = app.files_list("root", None, Some(100), &context).await?;
    let task = app
        .transfer_start("file-1", "C:/Mock/smoke.bin", Some("smoke"), &context)
        .await?;
    for _ in 0..14 {
        app.tick().await?;
    }
    assert_eq!(app.task(&task.task_id)?.state, TaskState::Completed);
    println!(
        "{}",
        serde_json::json!({"mode":"mock", "files":files.items.len(), "task":app.task(&task.task_id)?, "status":"passed"})
    );
    Ok(())
}
