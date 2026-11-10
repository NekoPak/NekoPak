use crate::Service;
use tauri::{Emitter, Manager, Runtime};

#[cfg(target_os = "android")]
pub fn system_locale() -> Option<String> {
    use jni::{
        objects::{JObject, JString},
        JavaVM,
    };
    let context = ndk_context::android_context();
    // SAFETY: the Android runtime owns this VM throughout the native app lifetime.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let result = env.with_local_frame(12, |env| -> jni::errors::Result<String> {
        // Resolve the configured application locale (Android13 per-app locale included).
        let raw = unsafe { JObject::from_raw(context.context().cast()) };
        let context = env.new_local_ref(&raw)?;
        let resources = env
            .call_method(
                context,
                "getResources",
                "()Landroid/content/res/Resources;",
                &[],
            )?
            .l()?;
        let config = env
            .call_method(
                resources,
                "getConfiguration",
                "()Landroid/content/res/Configuration;",
                &[],
            )?
            .l()?;
        let locales = env
            .call_method(config, "getLocales", "()Landroid/os/LocaleList;", &[])?
            .l()?;
        let locale = env
            .call_method(
                locales,
                "get",
                "(I)Ljava/util/Locale;",
                &[jni::objects::JValue::Int(0)],
            )?
            .l()?;
        let tag = JString::from(
            env.call_method(locale, "toLanguageTag", "()Ljava/lang/String;", &[])?
                .l()?,
        );
        let text: String = env.get_string(&tag)?.into();
        Ok(text)
    });
    if env.exception_check().unwrap_or(true) {
        let _ = env.exception_clear();
        return None;
    }
    result
        .ok()
        .filter(|s| s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}

pub fn install<R: Runtime>(_app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    Ok(())
}

pub fn window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    let foreground = match event {
        tauri::WindowEvent::Suspended => false,
        tauri::WindowEvent::Resumed => true,
        _ => return,
    };
    let app = window.app_handle();
    if let Some(service) = app.try_state::<Service>() {
        // Gate synchronously before scheduling the pause work.
        service.set_foreground(foreground);
        if !foreground {
            let service = service.inner().clone();
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = service.pause_local_transfers() {
                    let _ = handle.emit("nekopak://storage/error", error);
                }
            });
        }
    }
}

#[tauri::command]
pub fn desktop_status() -> Result<serde_json::Value, nekopak_domain::ClientError> {
    Ok(serde_json::json!({"tray_available":false,"window_visible":true}))
}
