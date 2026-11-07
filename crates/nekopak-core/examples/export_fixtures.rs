use nekopak_core::AppService;
use nekopak_domain::TransferAction;
use nekopak_provider::RequestContext;
use nekopak_provider_mock::{Scenario, FIXTURE_TIME};
use serde_json::{json, Value};

fn normalize(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields.iter_mut() {
                if matches!(key.as_str(), "id" | "task_id")
                    && value
                        .as_str()
                        .is_some_and(|value| value.len() == 36 && value.as_bytes()[8] == b'-')
                {
                    *value = json!("fixture-transfer-1");
                } else if matches!(key.as_str(), "created_at" | "updated_at") {
                    *value = json!(FIXTURE_TIME);
                } else {
                    normalize(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize(value);
            }
        }
        _ => {}
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    std::fs::create_dir_all(&root)?;
    for scenario in Scenario::ALL {
        let name = serde_json::to_value(scenario)?.as_str().unwrap().to_owned();
        let dir = tempfile::tempdir()?;
        let app = AppService::mock(&dir.path().join("fixtures.db"), scenario);
        let context = RequestContext::default();
        let files = app.files_list("root", None, Some(8), &context).await;
        let mut pages = Vec::new();
        if let Ok(first) = &files {
            pages.push(json!(first));
            if let Some(cursor) = &first.next_cursor {
                pages.push(json!(
                    app.files_list("root", Some(cursor), Some(8), &context)
                        .await?
                ));
            }
        }
        let mut events = app.subscribe();
        let created = app
            .transfer_start("file-1", "C:/Mock/fixture.bin", Some("fixture"), &context)
            .await;
        let mut controls = Vec::new();
        if let Ok(task) = &created {
            app.transfer_control(&task.task_id, TransferAction::Pause)?;
            controls.push(json!({"action":"pause", "snapshot":app.task(&task.task_id)?}));
            app.transfer_control(&task.task_id, TransferAction::Resume)?;
            controls.push(json!({"action":"resume", "snapshot":app.task(&task.task_id)?}));
            for _ in 0..16 {
                app.tick().await?;
            }
        }
        let mut ordered_events = Vec::new();
        while let Ok(event) = events.try_recv() {
            ordered_events.push(json!(event));
        }
        let out_of_order = ordered_events
            .last()
            .cloned()
            .into_iter()
            .chain(ordered_events.iter().rev().cloned())
            .collect::<Vec<_>>();
        let mut value = json!({
            "contract_version":"0.1", "scenario":scenario,
            "app_status":app.app_status(),
            "backend_discover":app.backend_discover(&context).await?,
            "files_list":match files { Ok(page) => json!({"ok":page}), Err(error) => json!({"error":error}) },
            "file_pages":pages,
            "transfer_start": match created { Ok(task) => json!({"ok":task}), Err(error) => json!({"error":error}) },
            "transfer_controls":controls,
            "transfers_list":app.transfers_list(None, None)?,
            "ordered_events":ordered_events,
            "out_of_order_events":out_of_order,
            "quota_get":match app.quota_get(&context).await { Ok(quota) => json!({"ok":quota}), Err(error) => json!({"error":error}) },
        });
        normalize(&mut value);
        std::fs::write(
            root.join(format!("{name}.json")),
            serde_json::to_string_pretty(&value)? + "\n",
        )?;
    }
    println!("Exported 10 offline fixture scenarios.");
    Ok(())
}
