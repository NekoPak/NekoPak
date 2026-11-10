use crate::*;
use nekopak_core::updates::{UpdateService, UpdateStatus};
type Updater = Arc<UpdateService>;

pub fn initialize(app: &tauri::AppHandle, directory: &std::path::Path) -> Result<(), ClientError> {
    let updates = if cfg!(target_os = "android") {
        directory.join("files/updates")
    } else {
        directory.join("updates")
    };
    let updater = UpdateService::production(updates.clone())?;
    #[cfg(debug_assertions)]
    let updater = {
        let marker = directory.join("update-offline-smoke.json");
        if marker.exists() {
            let bytes = std::fs::read(marker).map_err(|_| internal())?;
            if bytes.len() > 4096 {
                return Err(internal());
            }
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| internal())?;
            UpdateService::synthetic(
                updates,
                value["origin"].as_str().ok_or_else(internal)?,
                value["public_key"].as_str().ok_or_else(internal)?,
                value["current_version"]
                    .as_str()
                    .unwrap_or(env!("CARGO_PKG_VERSION")),
                if cfg!(target_os = "android") {
                    "android-aarch64"
                } else {
                    "windows-x86_64"
                },
            )?
        } else {
            updater
        }
    };
    app.manage(updater);
    Ok(())
}

#[tauri::command]
pub fn update_status(updater: State<'_, Updater>) -> Result<UpdateStatus, ClientError> {
    updater.status()
}
#[tauri::command]
pub async fn update_check(
    updater: State<'_, Updater>,
    service: State<'_, Service>,
) -> Result<UpdateStatus, ClientError> {
    updater.check(&service.settings_get()?).await
}
#[tauri::command(rename_all = "snake_case")]
pub async fn update_download(
    updater: State<'_, Updater>,
    service: State<'_, Service>,
    update_id: String,
) -> Result<(), ClientError> {
    updater
        .inner()
        .start_download(&update_id, service.settings_get()?)
}
#[tauri::command]
pub fn update_cancel(updater: State<'_, Updater>) -> Result<(), ClientError> {
    updater.cancel()
}

#[tauri::command(rename_all = "snake_case")]
pub async fn update_install(
    app: tauri::AppHandle,
    updater: State<'_, Updater>,
    service: State<'_, Service>,
    update_id: String,
) -> Result<serde_json::Value, ClientError> {
    let updater = updater.inner().clone();
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let file = updater.verified_file(&update_id)?;
        #[cfg(target_os = "android")]
        {
            let _ = (app, service);
            let outcome = android_install(&file)?;
            Ok(serde_json::json!({
                "state": if outcome == 1 { "permission_required" } else { "installer_opened" }
            }))
        }
        #[cfg(windows)]
        {
            service.set_foreground(false);
            if let Err(error) = service.pause_local_transfers() {
                service.set_foreground(true);
                return Err(error);
            }
            let mut process = std::process::Command::new(&file.path);
            // No shell, elevation request or silent-install switch. The user
            // confirms installation in the normal NSIS wizard.
            if process.spawn().is_err() {
                service.set_foreground(true);
                return Err(ClientError::new(
                    ErrorCode::Internal,
                    "无法启动更新安装器，已保留更新包。",
                ));
            }
            app.exit(0);
            Ok(serde_json::json!({"state":"installer_opened"}))
        }
        #[cfg(not(any(windows, target_os = "android")))]
        {
            let _ = (app, service, file);
            Err(ClientError::new(
                ErrorCode::Unsupported,
                "当前平台尚未支持安装更新。",
            ))
        }
    })
    .await
    .map_err(|_| internal())?
}

#[cfg(target_os = "android")]
fn android_install(file: &nekopak_core::updates::VerifiedUpdate) -> Result<i32, ClientError> {
    use jni::{
        objects::{JClass, JObject, JValue},
        JavaVM,
    };
    let context = ndk_context::android_context();
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.map_err(|_| internal())?;
    let mut env = vm.attach_current_thread().map_err(|_| internal())?;
    let result = env.with_local_frame(16, |env| -> jni::errors::Result<i32> {
        // Android owns this global Context. The local copy is scoped to this JNI frame.
        let raw = unsafe { JObject::from_raw(context.context().cast()) };
        let context = env.new_local_ref(&raw)?;
        let loader = env
            .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
            .l()?;
        let name = env.new_string("io.nekopak.desktop.UpdateInstaller")?;
        let class = JClass::from(
            env.call_method(
                loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name)],
            )?
            .l()?,
        );
        let path = env.new_string(file.path.to_string_lossy())?;
        let sha = env.new_string(&file.sha256)?;
        let version = env.new_string(&file.version)?;
        env.call_static_method(
            class,
            "install",
            "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;J)I",
            &[
                JValue::Object(&context),
                JValue::Object(&path),
                JValue::Object(&sha),
                JValue::Object(&version),
                JValue::Long(file.bytes as i64),
            ],
        )?
        .i()
    });
    if env.exception_check().unwrap_or(true) {
        let _ = env.exception_clear();
        return Err(ClientError::new(
            ErrorCode::IntegrityFailed,
            "Android拒绝更新包，请检查版本、签名或系统安装权限。",
        ));
    }
    result.map_err(|_| ClientError::new(ErrorCode::Internal, "无法打开Android安装界面。"))
}
