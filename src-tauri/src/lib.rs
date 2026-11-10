mod autostart;
mod catalog_commands;
#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;
#[cfg(mobile)]
use mobile as desktop;
use nekopak_core::AppService;
use nekopak_domain::*;
use nekopak_provider::{ProviderPort, RequestContext};
use nekopak_provider_mock::{MockProvider, Scenario};
use nekopak_provider_native::NativeProvider;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use zeroize::Zeroizing;

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
        self.begin_with_timeout(id, Duration::from_secs(45))
    }
    fn begin_auth(&self, id: Option<String>) -> Result<RequestGuard<'_>, ClientError> {
        self.begin_with_timeout(id, Duration::from_secs(30))
    }
    fn begin_with_timeout(
        &self,
        id: Option<String>,
        timeout: Duration,
    ) -> Result<RequestGuard<'_>, ClientError> {
        let context = RequestContext::new(timeout);
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

fn validate_host_destination(service: &AppService, destination: &str) -> Result<(), ClientError> {
    nekopak_core::validate_destination(destination)?;
    if cfg!(target_os = "android") {
        let settings = service.settings_get()?;
        if settings["download_dir"].as_str() != Some(destination) {
            return Err(ClientError::new(
                ErrorCode::PermissionDenied,
                "Android 下载只能写入应用下载目录。",
            ));
        }
    }
    Ok(())
}

#[tauri::command]
fn app_status(service: State<'_, Service>) -> AppStatus {
    let status = service.app_status();
    #[cfg(target_os = "android")]
    let status = {
        let mut status = status;
        status.system_locale = mobile::system_locale();
        status
    };
    status
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
    let reports = service.backend_discover(&request.context).await?;
    #[cfg(target_os = "android")]
    let reports = reports
        .into_iter()
        .map(|mut report| {
            report.operations.insert(Operation::Upload, false);
            report
        })
        .collect();
    Ok(reports)
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

macro_rules! file_query_command {
    ($name:ident,$query:expr) => {
        #[tauri::command(rename_all = "snake_case")]
        async fn $name(
            service: State<'_, Service>,
            requests: State<'_, Requests>,
            cursor: Option<String>,
            limit: Option<u32>,
            request_id: Option<String>,
        ) -> Result<Page<RemoteEntry>, ClientError> {
            let request = requests.begin(request_id)?;
            service
                .files_query(&$query, cursor.as_deref(), limit, &request.context)
                .await
        }
    };
}
file_query_command!(files_starred, FileQuery::Starred);
file_query_command!(trash_list, FileQuery::Trash);
file_query_command!(files_recent, FileQuery::Recent);

#[tauri::command(rename_all = "snake_case")]
async fn files_search(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    query: String,
    cursor: Option<String>,
    limit: Option<u32>,
    request_id: Option<String>,
) -> Result<Page<RemoteEntry>, ClientError> {
    let request = requests.begin(request_id)?;
    service
        .files_query(
            &FileQuery::Search(query),
            cursor.as_deref(),
            limit,
            &request.context,
        )
        .await
}

#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)]
async fn files_mutate(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    op: MutateOp,
    ids: Vec<String>,
    target: Option<MutateTarget>,
    idempotency_key: String,
    request_id: Option<String>,
) -> Result<OperationResult, ClientError> {
    let request = requests.begin(request_id)?;
    service
        .files_mutate(
            &FileMutation { op, ids, target },
            &idempotency_key,
            &request.context,
        )
        .await
}

#[tauri::command(rename_all = "snake_case")]
async fn shares_list(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    cursor: Option<String>,
    limit: Option<u32>,
    request_id: Option<String>,
) -> Result<Page<ShareInfo>, ClientError> {
    let request = requests.begin(request_id)?;
    service
        .shares_list(cursor.as_deref(), limit, &request.context)
        .await
}

#[tauri::command(rename_all = "snake_case")]
async fn verification_status(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    verification_id: Option<String>,
    request_id: Option<String>,
) -> Result<Option<VerificationState>, ClientError> {
    let request = requests.begin(request_id)?;
    service
        .verification_status(verification_id.as_deref(), &request.context)
        .await
}
#[tauri::command(rename_all = "snake_case")]
async fn verification_open<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    verification_id: String,
    request_id: Option<String>,
) -> Result<(), ClientError> {
    let request = requests.begin_auth(request_id)?;
    let url = service
        .verification_url(&verification_id, &request.context)
        .await?;
    #[cfg(debug_assertions)]
    if std::env::var_os("NEKOPAK_TEST_API_ORIGIN").is_some()
        && std::env::var_os("NEKOPAK_TEST_HIDE_WINDOW").is_some()
    {
        if let Some(directory) = std::env::var_os("NEKOPAK_TEST_DATA_DIR") {
            // Synthetic fixture link only; the loopback harness below replaces the entire provider/vault.
            std::fs::write(
                std::path::PathBuf::from(directory).join("fixture-verification.json"),
                serde_json::json!({"url":url}).to_string(),
            )
            .map_err(|_| internal())?;
            return Ok(());
        }
    }
    if !url.is_empty() {
        app.opener().open_url(url, None::<&str>).map_err(|_| {
            ClientError::new(
                ErrorCode::Internal,
                "无法打开系统浏览器，请检查默认浏览器设置。",
            )
        })?;
    }
    Ok(())
}
#[tauri::command(rename_all = "snake_case")]
async fn verification_complete(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    verification_id: String,
    request_id: Option<String>,
) -> Result<(), ClientError> {
    let request = requests.begin(request_id)?;
    service
        .verification_complete(&verification_id, &request.context)
        .await
}
#[tauri::command(rename_all = "snake_case")]
async fn verification_cancel(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    verification_id: String,
    request_id: Option<String>,
) -> Result<(), ClientError> {
    let request = requests.begin(request_id)?;
    service
        .verification_cancel(&verification_id, &request.context)
        .await
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
async fn transfers_list(
    service: State<'_, Service>,
    cursor: Option<String>,
    limit: Option<u32>,
) -> Result<Page<TransferTask>, ClientError> {
    service
        .visible_transfers(
            cursor.as_deref(),
            limit,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
async fn transfer_control(
    service: State<'_, Service>,
    task_id: String,
    action: TransferAction,
) -> Result<(), ClientError> {
    service
        .control_visible_transfer(
            &task_id,
            action,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}

#[tauri::command]
fn settings_get(service: State<'_, Service>) -> Result<serde_json::Value, ClientError> {
    service.settings_get()
}
#[tauri::command]
fn settings_update(
    service: State<'_, Service>,
    patch: serde_json::Value,
) -> Result<serde_json::Value, ClientError> {
    service.settings_update(&patch)
}

#[tauri::command]
#[cfg(desktop)]
async fn download_directory_pick<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    service: State<'_, Service>,
) -> Result<Option<String>, ClientError> {
    let settings = service.settings_get()?;
    let mut dialog = app
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("选择下载文件夹 / Choose download folder");
    if let Some(path) = settings["download_dir"]
        .as_str()
        .filter(|p| std::path::Path::new(p).is_dir())
    {
        dialog = dialog.set_directory(path);
    }
    let (send, receive) = tokio::sync::oneshot::channel();
    dialog.pick_folder(move |selection| {
        let _ = send.send(selection);
    });
    let selection = receive.await.map_err(|_| internal())?;
    selection
        .map(|selection| {
            let path = selection
                .into_path()
                .map_err(|_| ClientError::new(ErrorCode::Conflict, "请选择本地文件夹。"))?;
            let text = path
                .to_str()
                .ok_or_else(|| ClientError::new(ErrorCode::Conflict, "目录路径无法表示。"))?
                .to_owned();
            nekopak_core::validate_destination(&text)?;
            if !path.is_dir() {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "请选择已存在的文件夹。",
                ));
            }
            Ok(text)
        })
        .transpose()
}

#[tauri::command]
#[cfg(mobile)]
async fn download_directory_pick<R: tauri::Runtime>(
    _app: tauri::AppHandle<R>,
    _window: tauri::WebviewWindow<R>,
    _service: State<'_, Service>,
) -> Result<Option<String>, ClientError> {
    Err(ClientError::new(
        ErrorCode::Unsupported,
        "Android 下载保存在应用内，请在完成后使用导出。",
    ))
}

#[tauri::command(rename_all = "snake_case")]
async fn transfer_open_directory<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    service: State<'_, Service>,
    task_id: String,
) -> Result<(), ClientError> {
    if cfg!(mobile) {
        return Err(ClientError::new(
            ErrorCode::Unsupported,
            "Android 请使用导出保存已完成的文件。",
        ));
    }
    let path = service
        .transfer_directory(&task_id, &RequestContext::new(Duration::from_secs(45)))
        .await?;
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|_| ClientError::new(ErrorCode::Internal, "无法打开下载文件夹。"))
}
#[tauri::command(rename_all = "snake_case")]
async fn transfer_start(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    remote_id: String,
    destination: String,
    idempotency_key: Option<String>,
    options: Option<serde_json::Value>,
    request_id: Option<String>,
) -> Result<TaskRef, ClientError> {
    validate_host_destination(&service, &destination)?;
    let request = requests.begin(request_id)?;
    if let Some(options) = options {
        let options: nekopak_core::TransferOptions = serde_json::from_value(options)
            .map_err(|_| ClientError::new(ErrorCode::Conflict, "媒体下载选项无效。"))?;
        return service
            .media_start(
                &remote_id,
                &destination,
                &options,
                idempotency_key.as_deref(),
                &request.context,
            )
            .await;
    }
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
// Keep the frozen flat IPC keys while explicitly rejecting v3 selection parameters.
#[allow(clippy::too_many_arguments)]
async fn cloud_task_create(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    source_url: String,
    parent_id: String,
    idempotency_key: String,
    preview_id: Option<String>,
    file_indices: Option<Vec<u32>>,
    request_id: Option<String>,
) -> Result<TaskRef, ClientError> {
    if file_indices.as_ref().is_some_and(|v| !v.is_empty()) {
        return Err(ClientError::new(
            ErrorCode::Unsupported,
            "按预览选择文件的云下载尚未实现。",
        ));
    }
    let request = requests.begin(request_id)?;
    if let Some(id) = preview_id {
        service
            .validate_preview(&source_url, &id, &request.context)
            .await?;
    }
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
async fn auth_sign_in(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    username: String,
    password: String,
    request_id: Option<String>,
) -> Result<SignInResult, ClientError> {
    let password = Zeroizing::new(password);
    let request = requests.begin_auth(request_id)?;
    service
        .auth_sign_in(&username, &password, &request.context)
        .await
}

#[tauri::command(rename_all = "snake_case")]
async fn auth_verify(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    challenge_id: String,
    code: String,
    password: Option<String>,
    request_id: Option<String>,
) -> Result<SignInResult, ClientError> {
    let password = password.map(Zeroizing::new);
    let request = requests.begin_auth(request_id)?;
    service
        .auth_verify(
            &challenge_id,
            &code,
            password.as_deref().map(|s| s.as_str()),
            &request.context,
        )
        .await
}

#[tauri::command(rename_all = "snake_case")]
async fn auth_logout(
    service: State<'_, Service>,
    requests: State<'_, Requests>,
    backend: BackendId,
    request_id: Option<String>,
) -> Result<(), ClientError> {
    let request = requests.begin(request_id)?;
    service.auth_logout(backend, &request.context).await
}

fn selected_backend(value: &str) -> Result<BackendId, &'static str> {
    match value {
        "mock" => Ok(BackendId::Mock),
        "rust_compat" => Ok(BackendId::RustCompat),
        _ => Err("NEKOPAK_BACKEND must be rust_compat or mock. The former external adapter has been removed."),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let context = tauri::generate_context!();
    #[cfg(debug_assertions)]
    let context = {
        let mut context = context;
        if std::env::var_os("NEKOPAK_TEST_HIDE_WINDOW").is_some() {
            for window in &mut context.config_mut().app.windows {
                window.visible = false;
            }
        }
        context
    };
    let builder = tauri::Builder::default();
    #[cfg(target_os = "android")]
    let builder = builder.plugin(tauri_plugin_fs::init());
    let result = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(desktop::window_event)
        .manage(Requests::default())
        .setup(|app| {
            let directory = app.path().app_local_data_dir()?;
            #[cfg(debug_assertions)]
            let directory = std::env::var_os("NEKOPAK_TEST_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or(directory);
            std::fs::create_dir_all(&directory)?;
            #[cfg(not(windows))]
            let directory = std::fs::canonicalize(directory)?;
            #[cfg(all(target_os = "android", debug_assertions))]
            let android_test_origin = {
                // Opt-in isolated emulator harness, never compiled into release.
                // The marker is app-private and only a loopback origin is
                // accepted by NativeProvider::synthetic_loopback below.
                let marker = directory.join("android-offline-smoke.json");
                if marker.exists() {
                    NativeProvider::android_vault_self_test()?;
                    let data = std::fs::read(&marker)?;
                    if data.len() > 1024 {
                        return Err("Invalid offline smoke marker".into());
                    }
                    let value: serde_json::Value = serde_json::from_slice(&data)?;
                    let origin = value["origin"]
                        .as_str()
                        .ok_or("Invalid offline smoke origin")?
                        .to_owned();
                    std::fs::write(
                        directory.join("android-vault-selftest.json"),
                        b"{\"rust_jni_keystore\":\"passed\"}\n",
                    )?;
                    Some(origin)
                } else {
                    None
                }
            };
            let selected = selected_backend(
                &std::env::var("NEKOPAK_BACKEND").unwrap_or_else(|_| "rust_compat".into()),
            )?;
            #[cfg(desktop)]
            if let Some(window) = app.get_webview_window("main") {
                window.set_title(if selected == BackendId::Mock {
                    "NekoPak — Offline Mock"
                } else {
                    "NekoPak"
                })?;
            }
            let (active, database): (Arc<dyn ProviderPort>, &str) = if selected == BackendId::Mock {
                let scenario = match std::env::var("NEKOPAK_MOCK_SCENARIO") {
                    Ok(value) => {
                        serde_json::from_value::<Scenario>(serde_json::Value::String(value))?
                    }
                    Err(std::env::VarError::NotPresent) => Scenario::Free,
                    Err(_) => return Err("Invalid mock scenario encoding".into()),
                };
                (Arc::new(MockProvider::new(scenario)), "tasks.sqlite3")
            } else {
                #[cfg(debug_assertions)]
                let test_origin = std::env::var("NEKOPAK_TEST_API_ORIGIN").ok().filter(|_| {
                    std::env::var_os("NEKOPAK_TEST_DATA_DIR").is_some()
                        && std::env::var_os("NEKOPAK_TEST_HIDE_WINDOW").is_some()
                });
                #[cfg(all(target_os = "android", debug_assertions))]
                let test_origin = android_test_origin.or(test_origin);
                #[cfg(debug_assertions)]
                let provider = if let Some(origin) = test_origin {
                    NativeProvider::synthetic_loopback(&origin)?
                } else {
                    NativeProvider::from_environment(&directory.to_string_lossy())?
                };
                #[cfg(not(debug_assertions))]
                let provider = NativeProvider::from_environment(&directory.to_string_lossy())?;
                (Arc::new(provider), "api-preview.sqlite3")
            };
            let service = Arc::new(AppService::with_provider(&directory.join(database), active));
            // Apply persisted proxy/TLS settings before any auth/discovery IPC
            // or background transfer can initiate a connection. Preserve the
            // existing storage recovery UI when the task journal is damaged.
            if service.app_status().storage.status == "ready" {
                service.settings_get()?;
            }
            if let Some(mut events) = service.subscribe_auth() {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match events.recv().await {
                            Ok(event) => {
                                if handle.emit("nekopak://auth/changed", event).is_err() {
                                    break;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                // UI always re-reads authoritative state for any auth event.
                                let _ = handle.emit(
                                    "nekopak://auth/changed",
                                    serde_json::json!({"state":"pending"}),
                                );
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
            }
            let mut receiver = service.subscribe();
            let mut scans = service.subscribe_scans();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match scans.recv().await {
                        Ok(event) => {
                            let _ = handle.emit("nekopak://duplicates/progress", event);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            let native_transfer_events = service.uses_native_transfers();
            app.manage(service.clone());
            #[cfg(target_os = "android")]
            tauri::WebviewWindowBuilder::from_config(app.handle(), &app.config().app.windows[0])?
                .build()?;
            desktop::install(app.handle())?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match receiver.recv().await {
                        Ok(event) => {
                            if native_transfer_events {
                                // The authoritative snapshot filters by the current account. No previous account task metadata crosses this event channel.
                                if handle
                                    .emit("nekopak://transfer/resync-required", ())
                                    .is_err()
                                {
                                    break;
                                }
                                continue;
                            }
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
            let download_service = service.clone();
            let download_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = download_service.run_download_queue().await {
                    let _ = download_handle.emit("nekopak://storage/error", error);
                }
            });
            let media_service = service.clone();
            tauri::async_runtime::spawn(async move {
                let mut timer = tokio::time::interval(Duration::from_millis(500));
                loop {
                    timer.tick().await;
                    if media_service.run_media().await.is_err() {
                        break;
                    }
                }
            });
            let upload_service = service.clone();
            tauri::async_runtime::spawn(async move {
                let mut timer = tokio::time::interval(Duration::from_millis(500));
                loop {
                    timer.tick().await;
                    if upload_service.run_upload().await.is_err() {
                        break;
                    }
                }
            });
            let sync_service = service.clone();
            let sync_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut timer = tokio::time::interval(Duration::from_secs(2));
                loop {
                    timer.tick().await;
                    if !sync_service.is_foreground() || cfg!(mobile) {
                        continue;
                    }
                    if let Ok(events) = sync_service
                        .sync_reconcile(&RequestContext::new(Duration::from_secs(45)))
                        .await
                    {
                        for event in events {
                            let _ = sync_handle.emit("nekopak://sync/changed", event);
                        }
                    }
                    if let Ok(events) = sync_service
                        .sync_automatic(&RequestContext::new(Duration::from_secs(45)))
                        .await
                    {
                        for event in events {
                            let name = if event["state"] == "needs_review" {
                                "nekopak://sync/plan-ready"
                            } else {
                                "nekopak://sync/changed"
                            };
                            let _ = sync_handle.emit(name, event);
                        }
                    }
                    let _ = sync_service
                        .cleanup_shared_sources(&RequestContext::new(Duration::from_secs(15)))
                        .await;
                }
            });
            tauri::async_runtime::spawn(async move {
                let mut timer = tokio::time::interval(Duration::from_millis(500));
                loop {
                    timer.tick().await;
                    if let Err(error) = service.tick_periodic().await {
                        // Fail closed; do not keep writing to an inaccessible or damaged store.
                        let _ = handle.emit("nekopak://storage/error", error);
                        break;
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            catalog_commands::cloud_torrent_pick,
            catalog_commands::cloud_torrent_create,
            catalog_commands::cloud_torrent_release,
            catalog_commands::transfer_forget,
            catalog_commands::transfer_clear_finished,
            catalog_commands::auth_preferences_get,
            catalog_commands::auth_preferences_update,
            catalog_commands::network_reconnect,
            catalog_commands::network_domains_probe,
            app_status,
            autostart::autostart_get,
            autostart::autostart_set,
            catalog_commands::transfer_start_batch,
            catalog_commands::transfer_export,
            catalog_commands::duplicates_scan,
            catalog_commands::duplicates_report,
            catalog_commands::duplicates_cancel,
            catalog_commands::files_space_summary,
            catalog_commands::cloud_task_preview,
            catalog_commands::share_create,
            catalog_commands::shares_cancel,
            catalog_commands::media_probe,
            catalog_commands::media_align,
            catalog_commands::media_session_open,
            catalog_commands::media_session_close,
            catalog_commands::local_sources_list,
            catalog_commands::local_sources_add,
            catalog_commands::local_sources_remove,
            catalog_commands::upload_pick_and_start,
            catalog_commands::archive_list,
            catalog_commands::archive_unlock,
            catalog_commands::archive_extract,
            catalog_commands::jobs_list,
            catalog_commands::sync_configs_list,
            catalog_commands::sync_config_save,
            catalog_commands::sync_scan,
            catalog_commands::sync_plan,
            catalog_commands::sync_apply,
            catalog_commands::sync_run_status,
            catalog_commands::sync_history,
            catalog_commands::player_config_pick,
            catalog_commands::media_open_external,
            catalog_commands::share_open,
            catalog_commands::share_folder,
            catalog_commands::share_restore,
            catalog_commands::ui_prefs_get,
            catalog_commands::ui_prefs_update,
            catalog_commands::reference_create,
            catalog_commands::reference_list,
            catalog_commands::reference_status,
            catalog_commands::reference_store_manifest,
            catalog_commands::reference_archive,
            catalog_commands::reference_restore,
            catalog_commands::accounts_list,
            catalog_commands::account_switch,
            catalog_commands::account_remove,
            catalog_commands::share_preview_entry,
            catalog_commands::upload_directory_pick_and_start,
            catalog_commands::mount_capabilities,
            catalog_commands::mount_configs_list,
            catalog_commands::mount_config_save,
            catalog_commands::mount_start,
            catalog_commands::mount_status,
            catalog_commands::mount_stop,
            desktop::desktop_status,
            settings_get,
            settings_update,
            download_directory_pick,
            transfer_open_directory,
            backend_discover,
            files_list,
            files_starred,
            files_recent,
            files_search,
            trash_list,
            files_mutate,
            shares_list,
            verification_status,
            verification_open,
            verification_complete,
            verification_cancel,
            files_stat,
            files_path,
            transfers_list,
            transfer_start,
            transfer_control,
            cloud_task_create,
            quota_get,
            auth_status,
            auth_sign_in,
            auth_verify,
            auth_logout,
            request_cancel
        ])
        .run(context);
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
            .manage(Requests::default())
            .invoke_handler(tauri::generate_handler![
                catalog_commands::transfer_forget,
                catalog_commands::transfer_clear_finished,
                app_status,
                backend_discover,
                files_list,
                files_starred,
                files_recent,
                files_search,
                trash_list,
                files_mutate,
                shares_list,
                verification_status,
                verification_open,
                verification_complete,
                verification_cancel,
                files_stat,
                files_path,
                transfers_list,
                transfer_start,
                transfer_control,
                cloud_task_create,
                quota_get,
                auth_status,
                auth_sign_in,
                auth_verify,
                auth_logout,
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
            invoke(&webview, "transfers_list", json!({})).unwrap()["items"][0]["state"],
            "paused"
        );
        assert_eq!(
            invoke(
                &webview,
                "transfer_forget",
                json!({"task_id":task["task_id"]})
            )
            .unwrap_err()["code"],
            "CONFLICT"
        );
        invoke(
            &webview,
            "transfer_control",
            json!({"task_id":task["task_id"],"action":"cancel"}),
        )
        .unwrap();
        invoke(
            &webview,
            "transfer_forget",
            json!({"task_id":task["task_id"]}),
        )
        .unwrap();
        assert!(
            invoke(&webview, "transfers_list", json!({})).unwrap()["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            invoke(&webview, "auth_status", json!({})).unwrap()["credential_owner"],
            "none"
        );
        assert!(invoke(&webview, "official_cli_probe", json!({})).is_err());
        let rejected = invoke(
            &webview,
            "auth_sign_in",
            json!({"username":"example@example.test","password":"synthetic-password"}),
        )
        .unwrap_err();
        assert_eq!(rejected["code"], "UNSUPPORTED");
        assert!(!rejected.to_string().contains("synthetic-password"));
        assert_eq!(
            invoke(
                &webview,
                "auth_verify",
                json!({"challenge_id":"none","code":""})
            )
            .unwrap_err()["code"],
            "UNSUPPORTED"
        );
        assert_eq!(
            invoke(&webview, "auth_logout", json!({"backend":"rust_compat"})).unwrap_err()["code"],
            "UNSUPPORTED"
        );
        assert_eq!(invoke(&webview, "transfer_start", json!({"remote_id":"file-1","destination":"C:/Mock/clip.mp4", "options":{"clip":{"start_ms":1000,"end_ms":2000,"align":"previous_keyframe"}}})).unwrap_err()["code"], "UNSUPPORTED");
        assert_eq!(invoke(&webview, "cloud_task_create", json!({"source_url":"https://example.com/file","parent_id":"root","idempotency_key":"selected","file_indices":[1]})).unwrap_err()["code"], "UNSUPPORTED");
        assert!(
            invoke(&webview, "transfers_list", json!({})).unwrap()["items"]
                .as_array()
                .unwrap()
                .is_empty(),
            "unsupported options must not create an unintended whole-file task"
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

    #[test]
    fn removed_backend_cannot_be_selected() {
        assert!(selected_backend("official_cli").is_err());
        assert_eq!(
            selected_backend("rust_compat").unwrap(),
            BackendId::RustCompat
        );
        assert_eq!(selected_backend("mock").unwrap(), BackendId::Mock);
    }

    #[cfg(windows)]
    #[test]
    fn native_default_ipc_accepts_login_input_without_developer_setup() {
        let directory = tempfile::tempdir().unwrap();
        let native = Arc::new(
            NativeProvider::public_web(&format!("ipc-default-{}", directory.path().display()))
                .unwrap(),
        );
        let service = Arc::new(AppService::with_provider(
            &directory.path().join("native.db"),
            native,
        ));
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_opener::init())
            .manage(service)
            .manage(Requests::default())
            .invoke_handler(tauri::generate_handler![
                app_status,
                backend_discover,
                auth_status,
                auth_sign_in
            ])
            .build(tauri::generate_context!())
            .unwrap();
        let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        assert_eq!(
            invoke(&window, "app_status", json!({})).unwrap()["features"]["auth_sign_in"],
            true
        );
        assert_eq!(
            invoke(&window, "backend_discover", json!({})).unwrap()[0]["status"],
            "needs_login"
        );
        assert_eq!(
            invoke(&window, "auth_status", json!({})).unwrap()["state"],
            "signed_out"
        );
        // Invalid empty input reaches the native validator, not an unavailable provider.
        // No real email or password is sent over the network by this test.
        let error = invoke(
            &window,
            "auth_sign_in",
            json!({"username":"","password":"synthetic-not-sent"}),
        )
        .unwrap_err();
        assert_eq!(error["code"], "CONFLICT");
        assert!(!error.to_string().contains("synthetic-not-sent"));
        assert!(invoke(
            &window,
            "plugin:opener|open_url",
            json!({"url":"file:///C:/forbidden-test","with":null})
        )
        .is_err());
        let blocked = invoke(
            &window,
            "plugin:opener|open_url",
            json!({"url":"https://example.com","with":"nekopak-nonexistent-test-application"}),
        )
        .unwrap_err();
        assert!(blocked.to_string().contains("Not allowed"));
    }
}
