use crate::*;

#[tauri::command(rename_all = "snake_case")]
pub async fn transfer_forget(
    service: State<'_, Service>,
    task_id: String,
) -> Result<(), ClientError> {
    service
        .transfer_forget(&task_id, &RequestContext::default())
        .await
}
#[tauri::command]
pub async fn transfer_clear_finished(
    service: State<'_, Service>,
) -> Result<serde_json::Value, ClientError> {
    let removed = service
        .transfer_clear_finished(&RequestContext::new(Duration::from_secs(60)))
        .await?;
    Ok(serde_json::json!({"removed":removed}))
}
#[tauri::command]
pub async fn auth_preferences_get(
    service: State<'_, Service>,
) -> Result<serde_json::Value, ClientError> {
    service
        .auth_preferences_get(&RequestContext::default())
        .await
}
#[tauri::command]
pub async fn auth_preferences_update(
    service: State<'_, Service>,
    preferences: serde_json::Value,
) -> Result<serde_json::Value, ClientError> {
    service
        .auth_preferences_update(&preferences, &RequestContext::default())
        .await
}
#[tauri::command]
pub async fn network_domains_probe(
    service: State<'_, Service>,
) -> Result<serde_json::Value, ClientError> {
    service
        .network_domains_probe(&RequestContext::new(Duration::from_secs(60)))
        .await
}
#[tauri::command]
pub fn network_reconnect(service: State<'_, Service>) -> Result<(), ClientError> {
    service.network_reconnect()
}

fn require_desktop_files() -> Result<(), ClientError> {
    if cfg!(mobile) {
        Err(ClientError::new(
            ErrorCode::Unsupported,
            "此功能需要桌面文件系统；Android 适配尚未完成。",
        ))
    } else {
        Ok(())
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn transfer_export<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    service: State<'_, Service>,
    task_id: String,
) -> Result<bool, ClientError> {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_fs::{FsExt, OpenOptions};
        let context = RequestContext::new(Duration::from_secs(45));
        let scope = service.account_scope(&context).await?;
        let mut auth_events = service.subscribe_auth();
        let (name, mut source, length) =
            service.completed_transfer_file(&task_id, &context).await?;
        let (send, receive) = tokio::sync::oneshot::channel();
        app.dialog()
            .file()
            .set_file_name(&name)
            .save_file(move |path| {
                let _ = send.send(path);
            });
        let Some(target) = receive.await.map_err(|_| internal())? else {
            return Ok(false);
        };
        // Account may change while the Android document picker is visible.
        if service.account_scope(&RequestContext::default()).await? != scope {
            return Err(ClientError::new(
                ErrorCode::AuthExpired,
                "账号已改变，请重新导出。",
            ));
        }
        if !matches!(&target, tauri_plugin_dialog::FilePath::Url(url) if url.scheme() == "content")
        {
            return Err(ClientError::new(
                ErrorCode::PermissionDenied,
                "请选择系统文档保存位置。",
            ));
        }
        tauri::async_runtime::spawn_blocking(move || {
            let check_account = |events: &mut Option<
                tokio::sync::broadcast::Receiver<AuthChangedEvent>,
            >|
             -> Result<(), ClientError> {
                if let Some(events) = events {
                    if !matches!(
                        events.try_recv(),
                        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
                    ) {
                        return Err(ClientError::new(
                            ErrorCode::AuthExpired,
                            "账号状态已改变，导出已停止；原文件仍保留。",
                        ));
                    }
                }
                Ok(())
            };
            check_account(&mut auth_events)?;
            let mut options = OpenOptions::new();
            options.write(true).truncate(true);
            let mut output = app
                .fs()
                .open(target, options)
                .map_err(|_| ClientError::new(ErrorCode::PermissionDenied, "无法写入所选文件。"))?;
            use std::io::{Read, Write};
            let mut copied = 0u64;
            let mut buffer = vec![0; 256 * 1024];
            loop {
                check_account(&mut auth_events)?;
                let count = source.read(&mut buffer).map_err(|_| internal())?;
                if count == 0 {
                    break;
                }
                output.write_all(&buffer[..count]).map_err(|_| {
                    ClientError::new(
                        ErrorCode::Internal,
                        "导出失败，目标可能不完整；原下载文件仍保留。",
                    )
                })?;
                copied += count as u64;
                if copied > length {
                    break;
                }
            }
            if copied != length {
                return Err(ClientError::new(
                    ErrorCode::IntegrityFailed,
                    "导出长度不符，原下载文件仍保留。",
                ));
            }
            check_account(&mut auth_events)?;
            output.flush().map_err(|_| internal())?;
            // Some SAF providers expose a stream rather than a regular file.
            // Sync real files; successful streaming export is not advertised as
            // atomic or power-loss safe on arbitrary document providers.
            if output.metadata().map_err(|_| internal())?.is_file() {
                output.sync_all().map_err(|_| internal())?;
            }
            Ok(true)
        })
        .await
        .map_err(|_| internal())?
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, service, task_id);
        Err(ClientError::new(
            ErrorCode::Unsupported,
            "系统文档导出仅用于 Android。",
        ))
    }
}
#[tauri::command(rename_all = "snake_case")]
pub async fn reference_create(
    service: State<'_, Service>,
    ids: Vec<String>,
    idempotency_key: String,
) -> Result<ReferenceManifest, ClientError> {
    service
        .reference_create(
            &ids,
            &idempotency_key,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command]
pub async fn reference_list(
    service: State<'_, Service>,
) -> Result<Vec<ReferenceManifest>, ClientError> {
    service.reference_list(&RequestContext::default()).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn reference_status(
    service: State<'_, Service>,
    manifest_id: String,
) -> Result<ReferenceManifest, ClientError> {
    service
        .reference_status(&manifest_id, &RequestContext::new(Duration::from_secs(45)))
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn reference_store_manifest(
    service: State<'_, Service>,
    manifest_id: String,
    idempotency_key: String,
) -> Result<TaskRef, ClientError> {
    service
        .reference_store_manifest(
            &manifest_id,
            &idempotency_key,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn reference_archive(
    service: State<'_, Service>,
    manifest_id: String,
    confirm_trash: bool,
    idempotency_key: String,
) -> Result<ReferenceManifest, ClientError> {
    service
        .reference_archive(
            &manifest_id,
            confirm_trash,
            &idempotency_key,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn reference_restore(
    service: State<'_, Service>,
    manifest_id: String,
    allow_recreate: bool,
    idempotency_key: String,
) -> Result<ReferenceManifest, ClientError> {
    service
        .reference_restore(
            &manifest_id,
            allow_recreate,
            &idempotency_key,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn share_preview_entry(
    service: State<'_, Service>,
    access_ref: String,
    file_id: String,
) -> Result<RemoteEntry, ClientError> {
    service
        .share_preview_entry(
            &access_ref,
            &file_id,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn upload_directory_pick_and_start<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    service: State<'_, Service>,
    parent_id: String,
) -> Result<BatchRef, ClientError> {
    require_desktop_files()?;
    let scope = service.account_scope(&RequestContext::default()).await?;
    let path = crate::download_directory_pick(app, window, service.clone()).await?;
    let Some(path) = path else {
        return Ok(BatchRef {
            task_ids: vec![],
            skipped: vec![],
            expanded_files: 0,
            group_id: None,
        });
    };
    service
        .upload_directory_start_at(
            &path,
            &parent_id,
            &uuid_key(),
            &scope,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command]
pub async fn mount_capabilities(
    service: State<'_, Service>,
) -> Result<serde_json::Value, ClientError> {
    service.mount_capabilities().await
}
#[tauri::command]
pub async fn mount_configs_list(
    service: State<'_, Service>,
) -> Result<Vec<nekopak_core::MountConfig>, ClientError> {
    service.mount_configs_list(&RequestContext::default()).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn mount_config_save(
    service: State<'_, Service>,
    config: nekopak_core::MountConfig,
) -> Result<nekopak_core::MountConfig, ClientError> {
    service
        .mount_config_save(&config, &RequestContext::default())
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn mount_start(
    service: State<'_, Service>,
    config_id: String,
    read_only: bool,
) -> Result<serde_json::Value, ClientError> {
    service
        .inner()
        .mount_start(
            &config_id,
            read_only,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn mount_status(
    service: State<'_, Service>,
    id: String,
) -> Result<serde_json::Value, ClientError> {
    service.mount_status(&id, &RequestContext::default()).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn mount_stop(service: State<'_, Service>, id: String) -> Result<(), ClientError> {
    service.mount_stop(&id, &RequestContext::default()).await
}
#[tauri::command]
pub async fn accounts_list(service: State<'_, Service>) -> Result<Vec<SavedAccount>, ClientError> {
    service.accounts_list(&RequestContext::default()).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn account_switch(
    service: State<'_, Service>,
    id: String,
) -> Result<AuthState, ClientError> {
    service
        .account_switch(&id, &RequestContext::new(Duration::from_secs(45)))
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn account_remove(service: State<'_, Service>, id: String) -> Result<(), ClientError> {
    service
        .account_remove(&id, &RequestContext::new(Duration::from_secs(45)))
        .await
}
#[tauri::command]
pub async fn ui_prefs_get(service: State<'_, Service>) -> Result<serde_json::Value, ClientError> {
    service.ui_prefs_get(&RequestContext::default()).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn ui_prefs_update(
    service: State<'_, Service>,
    prefs: serde_json::Value,
    expected_account_id: String,
) -> Result<serde_json::Value, ClientError> {
    service
        .ui_prefs_update(&prefs, &expected_account_id, &RequestContext::default())
        .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn transfer_start_batch(
    service: State<'_, Service>,
    remote_ids: Vec<String>,
    destination: String,
    idempotency_key: String,
    options: Option<serde_json::Value>,
) -> Result<BatchRef, ClientError> {
    validate_host_destination(&service, &destination)?;
    let options = options
        .map(serde_json::from_value::<nekopak_core::TransferOptions>)
        .transpose()
        .map_err(|_| ClientError::new(ErrorCode::Conflict, "媒体选项无效。"))?;
    service
        .transfer_start_batch_options(
            &remote_ids,
            &destination,
            &idempotency_key,
            options.as_ref(),
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn duplicates_scan(
    service: State<'_, Service>,
    folder_id: String,
) -> Result<ScanRef, ClientError> {
    service
        .inner()
        .duplicates_scan(&folder_id, &RequestContext::default())
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn duplicates_report(
    service: State<'_, Service>,
    scan_id: String,
) -> Result<DuplicateReport, ClientError> {
    service
        .duplicates_report(&scan_id, &RequestContext::default())
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn duplicates_cancel(
    service: State<'_, Service>,
    scan_id: String,
) -> Result<(), ClientError> {
    service
        .duplicates_cancel(&scan_id, &RequestContext::default())
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn files_space_summary(
    service: State<'_, Service>,
    folder_id: String,
) -> Result<SpaceSummary, ClientError> {
    service
        .files_space_summary(&folder_id, &RequestContext::new(Duration::from_secs(45)))
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn cloud_task_preview(
    service: State<'_, Service>,
    source_url: String,
) -> Result<serde_json::Value, ClientError> {
    service
        .cloud_preview(&source_url, &RequestContext::new(Duration::from_secs(45)))
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn share_create(
    service: State<'_, Service>,
    ids: Vec<String>,
    expires_in_days: Option<u32>,
    with_code: bool,
    idempotency_key: String,
) -> Result<ShareInfo, ClientError> {
    service
        .share_create(
            &CreateShareRequest {
                ids,
                expires_in_days: expires_in_days.unwrap_or(0),
                with_code,
            },
            &idempotency_key,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn shares_cancel(
    service: State<'_, Service>,
    share_ids: Vec<String>,
    idempotency_key: String,
) -> Result<BulkResult, ClientError> {
    service
        .shares_cancel(
            &share_ids,
            &idempotency_key,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn media_probe(
    service: State<'_, Service>,
    remote_id: String,
) -> Result<serde_json::Value, ClientError> {
    service
        .media_probe(&remote_id, &RequestContext::new(Duration::from_secs(45)))
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn media_align(
    service: State<'_, Service>,
    remote_id: String,
    quality_id: String,
    start_ms: u64,
    end_ms: u64,
) -> Result<serde_json::Value, ClientError> {
    service
        .media_align(
            &remote_id,
            &quality_id,
            start_ms,
            end_ms,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn media_session_open(
    service: State<'_, Service>,
    remote_id: String,
    quality_id: Option<String>,
) -> Result<serde_json::Value, ClientError> {
    service
        .inner()
        .media_session_open(&remote_id, quality_id.as_deref())
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub fn media_session_close(
    service: State<'_, Service>,
    session_id: String,
) -> Result<(), ClientError> {
    service.media_session_close(&session_id)
}
#[tauri::command]
pub fn local_sources_list(service: State<'_, Service>) -> Result<serde_json::Value, ClientError> {
    serde_json::to_value(service.local_sources_list()?).map_err(|_| internal())
}
#[tauri::command(rename_all = "snake_case")]
pub async fn local_sources_add<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    service: State<'_, Service>,
    path: Option<String>,
) -> Result<Option<serde_json::Value>, ClientError> {
    require_desktop_files()?;
    let path = if let Some(path) = path {
        path
    } else {
        let Some(path) = crate::download_directory_pick(app, window, service.clone()).await? else {
            return Ok(None);
        };
        path
    };
    serde_json::to_value(service.local_sources_add(&path)?)
        .map(Some)
        .map_err(|_| internal())
}
#[tauri::command(rename_all = "snake_case")]
pub fn local_sources_remove(
    service: State<'_, Service>,
    source_id: String,
) -> Result<(), ClientError> {
    service.local_sources_remove(&source_id)
}
#[tauri::command(rename_all = "snake_case")]
pub async fn upload_pick_and_start<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
    service: State<'_, Service>,
    parent_id: String,
) -> Result<BatchRef, ClientError> {
    require_desktop_files()?;
    let scope = service.account_scope(&RequestContext::default()).await?;
    let (send, receive) = tokio::sync::oneshot::channel();
    let dialog = app.dialog().file().set_title("选择上传文件 / Upload files");
    #[cfg(desktop)]
    let dialog = dialog.set_parent(&window);
    #[cfg(mobile)]
    let _ = window;
    dialog.pick_files(move |files| {
        let _ = send.send(files);
    });
    let mut result = BatchRef {
        task_ids: vec![],
        skipped: vec![],
        expanded_files: 0,
        group_id: None,
    };
    for file in receive.await.map_err(|_| internal())?.unwrap_or_default() {
        let path = file.into_path().map_err(|_| internal())?;
        let text = path.to_str().ok_or_else(internal)?;
        match service
            .upload_start_at(
                text,
                &parent_id,
                &uuid_key(),
                &scope,
                &RequestContext::new(Duration::from_secs(45)),
            )
            .await
        {
            Ok(task) => result.task_ids.push(task.task_id),
            Err(error) => result.skipped.push(SkippedEntry {
                id: String::new(),
                name: path.file_name().map(|s| s.to_string_lossy().into_owned()),
                reason: error.message,
            }),
        }
    }
    Ok(result)
}
fn uuid_key() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}
#[tauri::command(rename_all = "snake_case")]
pub async fn archive_list(
    service: State<'_, Service>,
    file_id: String,
    path: Option<String>,
    password_ref: Option<String>,
) -> Result<ArchiveListing, ClientError> {
    service
        .archive_list(
            &file_id,
            path.as_deref().unwrap_or(""),
            password_ref.as_deref(),
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn archive_unlock(
    service: State<'_, Service>,
    file_id: String,
    password: String,
) -> Result<serde_json::Value, ClientError> {
    let password = Zeroizing::new(password);
    service
        .archive_unlock(
            &file_id,
            &password,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn archive_extract(
    service: State<'_, Service>,
    file_id: String,
    paths: serde_json::Value,
    target_parent_id: String,
    password_ref: Option<String>,
    idempotency_key: String,
) -> Result<serde_json::Value, ClientError> {
    service
        .archive_extract(
            &file_id,
            &paths,
            &target_parent_id,
            password_ref.as_deref(),
            &idempotency_key,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn jobs_list(
    service: State<'_, Service>,
    cursor: Option<String>,
    limit: Option<u32>,
) -> Result<Page<serde_json::Value>, ClientError> {
    service
        .jobs_list(
            cursor.as_deref(),
            limit,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command]
pub async fn sync_configs_list(
    service: State<'_, Service>,
) -> Result<Vec<SyncConfig>, ClientError> {
    service.sync_configs_list(&RequestContext::default()).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn sync_config_save(
    service: State<'_, Service>,
    config: SyncConfig,
) -> Result<SyncConfig, ClientError> {
    require_desktop_files()?;
    service
        .sync_config_save(&config, &RequestContext::new(Duration::from_secs(45)))
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn sync_scan<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    service: State<'_, Service>,
    config_id: String,
) -> Result<ScanRef, ClientError> {
    let scan = service
        .sync_scan(&config_id, &RequestContext::new(Duration::from_secs(45)))
        .await?;
    let _ = app.emit(
        "nekopak://sync/plan-ready",
        serde_json::json!({"scan_id":scan.scan_id,"config_id":config_id}),
    );
    Ok(scan)
}
#[tauri::command(rename_all = "snake_case")]
pub async fn sync_plan(
    service: State<'_, Service>,
    config_id: String,
) -> Result<SyncPlan, ClientError> {
    service
        .sync_plan(&config_id, &RequestContext::default())
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn sync_apply(
    service: State<'_, Service>,
    plan_id: String,
    confirm_deletes: bool,
) -> Result<TaskRef, ClientError> {
    require_desktop_files()?;
    service
        .sync_apply(
            &plan_id,
            confirm_deletes,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn sync_run_status(
    service: State<'_, Service>,
    id: String,
) -> Result<serde_json::Value, ClientError> {
    service
        .sync_run_status(&id, &RequestContext::default())
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn sync_history(
    service: State<'_, Service>,
    config_id: String,
    cursor: Option<String>,
) -> Result<Page<SyncEvent>, ClientError> {
    service
        .sync_history(&config_id, cursor.as_deref(), &RequestContext::default())
        .await
}
#[tauri::command]
pub async fn player_config_pick<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
) -> Result<Option<String>, ClientError> {
    require_desktop_files()?;
    let (send, receive) = tokio::sync::oneshot::channel();
    let dialog = app
        .dialog()
        .file()
        .set_title("选择播放器 / Select player")
        .add_filter("Windows application", &["exe"]);
    #[cfg(desktop)]
    let dialog = dialog.set_parent(&window);
    #[cfg(mobile)]
    let _ = window;
    dialog.pick_file(move |file| {
        let _ = send.send(file);
    });
    receive
        .await
        .map_err(|_| internal())?
        .map(|p| {
            p.into_path()
                .map(|p| p.to_string_lossy().into_owned())
                .map_err(|_| internal())
        })
        .transpose()
}
#[tauri::command(rename_all = "snake_case")]
pub async fn media_open_external(
    service: State<'_, Service>,
    remote_id: String,
    quality_id: Option<String>,
) -> Result<serde_json::Value, ClientError> {
    require_desktop_files()?;
    let settings = service.settings_get()?;
    let player = settings["media"]["player_path"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ClientError::new(ErrorCode::Conflict, "请先在设置中选择已安装的外部播放器。")
        })?;
    let session = service
        .inner()
        .media_session_open(&remote_id, quality_id.as_deref())
        .await?;
    let id = session["session_id"].as_str().ok_or_else(internal)?;
    let mut command = std::process::Command::new(player);
    command
        .arg(session["url"].as_str().ok_or_else(internal)?)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    match command.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(
                serde_json::json!({"launched":true,"player":std::path::Path::new(player).file_name().map(|s|s.to_string_lossy().into_owned())}),
            )
        }
        Err(_) => {
            service.media_session_close(id)?;
            Err(ClientError::new(
                ErrorCode::Internal,
                "无法启动所选播放器，请检查应用是否可用。",
            ))
        }
    }
}
#[tauri::command(rename_all = "snake_case")]
pub async fn share_open(
    service: State<'_, Service>,
    share_id: String,
    pass_code: Option<String>,
) -> Result<SharedFolder, ClientError> {
    let code = Zeroizing::new(pass_code.unwrap_or_default());
    service
        .share_open(
            &share_id,
            &code,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn share_folder(
    service: State<'_, Service>,
    access_ref: String,
    parent_id: String,
    cursor: Option<String>,
    limit: Option<u32>,
) -> Result<SharedFolder, ClientError> {
    service
        .share_folder(
            &access_ref,
            &parent_id,
            cursor.as_deref(),
            limit,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn share_restore(
    service: State<'_, Service>,
    access_ref: String,
    ids: Vec<String>,
    target_parent_id: String,
    idempotency_key: String,
) -> Result<BulkResult, ClientError> {
    service
        .share_restore(
            &access_ref,
            &ids,
            &target_parent_id,
            &idempotency_key,
            &RequestContext::new(Duration::from_secs(45)),
        )
        .await
}
