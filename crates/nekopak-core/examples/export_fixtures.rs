use nekopak_core::AppService;
use nekopak_provider::RequestContext;
use nekopak_provider_mock::Scenario;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    std::fs::create_dir_all(&root)?;
    for scenario in Scenario::ALL {
        let name = serde_json::to_value(scenario)?.as_str().unwrap().to_owned();
        let dir = tempfile::tempdir()?;
        let app = AppService::mock(&dir.path().join("fixtures.db"), scenario);
        let context = RequestContext::default();
        let value = json!({
            "contract_version":"0.1", "scenario":scenario,
            "app_status":app.app_status(),
            "backend_discover":app.backend_discover(&context).await?,
            "files_list":match app.files_list("root", None, Some(8), &context).await { Ok(page) => json!({"ok":page}), Err(error) => json!({"error":error}) },
            "quota_get":match app.quota_get(&context).await { Ok(quota) => json!({"ok":quota}), Err(error) => json!({"error":error}) },
        });
        std::fs::write(
            root.join(format!("{name}.json")),
            serde_json::to_string_pretty(&value)? + "\n",
        )?;
    }
    println!("Exported 10 offline fixture scenarios.");
    Ok(())
}
