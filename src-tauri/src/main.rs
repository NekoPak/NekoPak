#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use nekopak_core::AppService;
use nekopak_domain::*;
use nekopak_provider::{ProviderPort, RequestContext};
use nekopak_provider_cli::OfficialCliProvider;
use nekopak_provider_mock::{MockProvider, Scenario};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{Emitter, Manager, State};

type Service = Arc<AppService>;
#[derive(Default)]
struct Requests(Mutex<HashMap<String, RequestContext>>);
struct RequestGuard<'a> {
    registry: &'a Requests,
    id: Option<String>,
    context: RequestContext,
}
impl Requests {
    fn begin(&self, id: Option<String>) -> Result<RequestGuard<'_>, ClientError> {
        let context = RequestContext::default();
        if let Some(id) = &id {
            if id.is_empty() || id.len() > 128 {
                return Err(ClientError::new(ErrorCode::Conflict, "Invalid request ID."));
            }
            let mut requests = self.0.lock().map_err(|_| internal())?;
            if requests.contains_key(id) {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "Request ID is already active.",
                ));
            }
            requests.insert(id.clone(), context.clone());
        }
        Ok(RequestGuard {
            registry: self,
            id,
            context,
        })
    }
}
impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        if let Some(id) = &self.id {
            if let Ok(mut requests) = self.registry.0.lock() {
                requests.remove(id);
            }
        }
    }
}
fn internal() -> ClientError {
    ClientError::new(ErrorCode::Internal, "The command service is unavailable.")
}

#[tauri::command]
fn app_status(service: State<'_, Service>) -> AppStatus {
    service.app_status()
}
#[tauri::command(rename_all = "snake_case")]
fn request_cancel(request_id: String, requests: State<'_, Requests>) -> Result<(), ClientError> {
    if let Some(context) = requests.0.lock().map_err(|_| internal())?.get(&request_id) {
        context.cancel();
    }
    Ok(())
}
#[tauri::command(rename_all = "snake_case")]
async fn backend_discover(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    request_id: Option<String>,
) -> Result<Vec<CapabilityReport>, ClientError> {
    let request = requests.begin(request_id)?;
    service.backend_discover(&request.context).await
}
#[tauri::command(rename_all = "snake_case")]
async fn files_list(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    parent_id: String,
    cursor: Option<String>,
    limit: Option<u32>,
    request_id: Option<String>,
) -> Result<Page<RemoteEntry>, ClientError> {
    let request = requests.begin(request_id)?;
    service
        .files_list(&parent_id, cursor.as_deref(), limit, &request.context)
        .await
}
#[tauri::command(rename_all = "snake_case")]
async fn files_stat(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    id: String,
    request_id: Option<String>,
) -> Result<RemoteEntry, ClientError> {
    let request = requests.begin(request_id)?;
    service.files_stat(&id, &request.context).await
}

#[tauri::command(rename_all = "snake_case")]
async fn files_path(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    id: String,
    request_id: Option<String>,
) -> Result<Vec<RemoteEntry>, ClientError> {
    let request = requests.begin(request_id)?;
    service.files_path(&id, &request.context).await
}
#[tauri::command(rename_all = "snake_case")]
fn transfers_list(
    service: State<'_, Service>,
    cursor: Option<String>,
    limit: Option<u32>,
) -> Result<Page<TransferTask>, ClientError> {
    service.transfers_list(cursor.as_deref(), limit)
}
#[tauri::command(rename_all = "snake_case")]
fn transfer_control(
    service: State<'_, Service>,
    task_id: String,
    action: TransferAction,
) -> Result<(), ClientError> {
    service.transfer_control(&task_id, action)
}
#[tauri::command(rename_all = "snake_case")]
async fn transfer_start(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    remote_id: String,
    destination: String,
    idempotency_key: Option<String>,
    request_id: Option<String>,
) -> Result<TaskRef, ClientError> {
    let request = requests.begin(request_id)?;
    service
        .transfer_start(
            &remote_id,
            &destination,
            idempotency_key.as_deref(),
            &request.context,
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
async fn cloud_task_create(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    source_url: String,
    parent_id: String,
    idempotency_key: String,
    request_id: Option<String>,
) -> Result<TaskRef, ClientError> {
    let request = requests.begin(request_id)?;
    service
        .cloud_task_create(&source_url, &parent_id, &idempotency_key, &request.context)
        .await
}
#[tauri::command(rename_all = "snake_case")]
async fn quota_get(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    request_id: Option<String>,
) -> Result<QuotaReport, ClientError> {
    let request = requests.begin(request_id)?;
    service.quota_get(&request.context).await
}

#[tauri::command(rename_all = "snake_case")]
async fn auth_status(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    request_id: Option<String>,
) -> Result<AuthState, ClientError> {
    let request = requests.begin(request_id)?;
    service.auth_status(&request.context).await
}

#[tauri::command(rename_all = "snake_case")]
async fn official_cli_probe(
    provider: State<'_, Arc<OfficialCliProvider>>,
    requests: State<'_, Requests>,
    request_id: Option<String>,
) -> Result<CliProbeReport, ClientError> {
    let request = requests.begin(request_id)?;
    request.context.run(provider.probe()).await
}

fn main() {
    let result = tauri::Builder::default()
        .manage(Requests::default())
        .setup(|app| {
            let directory = app.path().app_local_data_dir()?;
            #[cfg(debug_assertions)]
            let directory = std::env::var_os("NEKOPAK_TEST_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or(directory);
            std::fs::create_dir_all(&directory)?;
            let scenario = match std::env::var("NEKOPAK_MOCK_SCENARIO") {
                Ok(value) => serde_json::from_value::<Scenario>(serde_json::Value::String(value))?,
                Err(std::env::VarError::NotPresent) => Scenario::Free,
                Err(error) => return Err(error.into()),
            };
            let selected = std::env::var("NEKOPAK_BACKEND").unwrap_or_else(|_| "mock".into());
            if !matches!(selected.as_str(), "mock" | "official_cli") {
                return Err("NEKOPAK_BACKEND must be mock or official_cli".into());
            }
            if let Some(window) = app.get_webview_window("main") {
                window.set_title(if selected == "official_cli" {
                    "NekoPak — Official CLI Read-only Preview"
                } else {
                    "NekoPak — Offline Mock"
                })?;
            }
            let executable = std::env::var_os("NEKOPAK_CLI_PATH");
            let profile = std::env::var_os("NEKOPAK_CLI_PROFILE");
            let cli = Arc::new(match (executable, profile) {
                (Some(executable), Some(profile)) => OfficialCliProvider::configured(
                    std::path::Path::new(&executable),
                    std::path::Path::new(&profile),
                    selected == "official_cli",
                )?,
                (None, None) if selected == "mock" => OfficialCliProvider::not_configured(),
                _ => return Err(
                    "Set both NEKOPAK_CLI_PATH and NEKOPAK_CLI_PROFILE to use the reviewed CLI."
                        .into(),
                ),
            });
            let mock: Arc<dyn ProviderPort> = Arc::new(MockProvider::new(scenario));
            let (active, database): (Arc<dyn ProviderPort>, &str) = if selected == "official_cli" {
                (cli.clone(), "cli-preview.sqlite3")
            } else {
                (mock.clone(), "tasks.sqlite3")
            };
            let service = Arc::new(AppService::with_backends(
                &directory.join(database),
                active,
                vec![mock, cli.clone()],
            ));
            app.manage(cli);
            let mut receiver = service.subscribe();
            app.manage(service.clone());
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match receiver.recv().await {
                        Ok(event) => {
                            if handle.emit("nekopak://transfer/progress", &event).is_err() {
                                break;
                            }
                            if handle.emit("nekopak://transfer/changed", &event).is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            if handle
                                .emit("nekopak://transfer/resync-required", ())
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut timer = tokio::time::interval(Duration::from_millis(500));
                loop {
                    timer.tick().await;
                    if let Err(error) = service.tick().await {
                        // Fail closed; do not keep writing to an inaccessible or damaged store.
                        let _ = handle.emit("nekopak://storage/error", error);
                        break;
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_status,
            backend_discover,
            files_list,
            files_stat,
            files_path,
            transfers_list,
            transfer_start,
            transfer_control,
            cloud_task_create,
            quota_get,
            auth_status,
            official_cli_probe,
            request_cancel
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("NekoPak could not start: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn invoke(
        webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
        command: &str,
        body: Value,
    ) -> Result<Value, Value> {
        tauri::test::get_ipc_response(
            webview,
            tauri::webview::InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "http://tauri.localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.into(),
            },
        )
        .map(|body| body.deserialize::<Value>().unwrap())
    }

    #[test]
    fn tauri_command_wire_roundtrip_preserves_contract() {
        let dir = tempfile::tempdir().unwrap();
        let service = Arc::new(AppService::mock(&dir.path().join("ipc.db"), Scenario::Free));
        let app = tauri::test::mock_builder()
            .manage(service.clone())
            .manage(Arc::new(OfficialCliProvider::not_configured()))
            .manage(Requests::default())
            .invoke_handler(tauri::generate_handler![
                app_status,
                backend_discover,
                files_list,
                files_stat,
                files_path,
                transfers_list,
                transfer_start,
                transfer_control,
                cloud_task_create,
                quota_get,
                auth_status,
                official_cli_probe,
                request_cancel
            ])
            .build(tauri::generate_context!())
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        assert_eq!(
            invoke(&webview, "app_status", json!({})).unwrap()["storage"]["status"],
            "ready"
        );
        assert_eq!(
            invoke(&webview, "backend_discover", json!({})).unwrap()[0]["backend"],
            "mock"
        );
        let page = invoke(
            &webview,
            "files_list",
            json!({"parent_id":"root","limit":100}),
        )
        .unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 8);
        let path = invoke(&webview, "files_path", json!({"id":"nested-file"})).unwrap();
        assert_eq!(path[0]["id"], "documents");
        assert_eq!(path[1]["id"], "nested-file");
        assert_eq!(
            invoke(&webview, "files_stat", json!({"id":"file-7"})).unwrap()["size_bytes"],
            "18446744073709551615"
        );
        let task = invoke(
            &webview,
            "transfer_start",
            json!({"remote_id":"file-1","destination":"C:/Mock/ipc.bin","idempotency_key":"ipc"}),
        )
        .unwrap();
        invoke(
            &webview,
            "transfer_control",
            json!({"task_id":task["task_id"],"action":"pause"}),
        )
        .unwrap();
        assert_eq!(
            invoke(&webview, "auth_status", json!({})).unwrap()["credential_owner"],
            "none"
        );
        assert_eq!(
            invoke(&webview, "official_cli_probe", json!({})).unwrap()["status"],
            "not_installed"
        );
        assert_eq!(
            invoke(&webview, "transfers_list", json!({})).unwrap()["items"][0]["state"],
            "paused"
        );
        let error = invoke(
            &webview,
            "files_list",
            json!({"parent_id":"root","limit":0}),
        )
        .unwrap_err();
        assert_eq!(error["code"], "CONFLICT");
        assert_eq!(error["retryable"], false);
        assert_eq!(
            invoke(&webview, "quota_get", json!({})).unwrap()["source"],
            "mock"
        );
        let cloud = json!({"source_url":"https://example.com/file", "parent_id":"root", "idempotency_key":"ipc-cloud"});
        assert_eq!(
            invoke(&webview, "cloud_task_create", cloud.clone()),
            invoke(&webview, "cloud_task_create", cloud)
        );
        invoke(
            &webview,
            "request_cancel",
            json!({"request_id":"already-finished"}),
        )
        .unwrap();
    }

    #[test]
    fn request_ids_reject_duplicates_and_cleanup_on_drop() {
        let requests = Requests::default();
        let guard = requests.begin(Some("cancel-me".into())).unwrap();
        assert!(requests.begin(Some("cancel-me".into())).is_err());
        requests
            .0
            .lock()
            .unwrap()
            .get("cancel-me")
            .unwrap()
            .cancel();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert_eq!(
            runtime
                .block_on(guard.context.run(async { Ok(()) }))
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        drop(guard);
        assert!(requests.begin(Some("cancel-me".into())).is_ok());
    }
}
