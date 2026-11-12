use crate::{
    keepalive::{Backend, KeepAlive, PauseRequired},
    Service,
};
use std::{sync::Arc, time::Duration};
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

#[cfg(not(target_os = "android"))]
struct Unavailable;
#[cfg(not(target_os = "android"))]
impl Backend for Unavailable {
    fn start(&self, _: nekopak_core::LocalTransferActivity) -> bool {
        false
    }
    fn update(&self, _: nekopak_core::LocalTransferActivity) {}
    fn stop(&self) {}
}

fn pause<R: Runtime>(app: &tauri::AppHandle<R>, service: Service) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = service.pause_local_transfers() {
            let _ = handle.emit("nekopak://storage/error", error);
        }
        let _ = handle.emit("nekopak://transfer/resync-required", ());
    });
}

pub fn install<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let service = app.state::<Service>().inner().clone();
    #[cfg(target_os = "android")]
    let backend: Box<dyn Backend> = Box::new(crate::keepalive::android::Service);
    #[cfg(not(target_os = "android"))]
    let backend: Box<dyn Backend> = Box::new(Unavailable);
    let keep = Arc::new(KeepAlive::new(backend));
    app.manage(keep.clone());
    #[cfg(target_os = "android")]
    {
        let (keep, service, handle) = (keep.clone(), service.clone(), app.clone());
        crate::keepalive::android::on_ended(move |reason| {
            if keep.ended(&service, reason).0 {
                pause(&handle, service.clone());
            }
        });
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut changed = service.subscribe();
        loop {
            // Storage failures are reported by the transfer workers; keep-alive
            // then has nothing to protect and simply tries again later.
            if let Ok(PauseRequired(true)) = keep.reconcile(&service) {
                pause(&handle, service.clone());
            }
            tokio::select! {
                _ = changed.recv() => {},
                _ = tokio::time::sleep(Duration::from_secs(1)) => {},
            }
            // Coalesce event bursts and a closed channel into a bounded rate.
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    Ok(())
}

pub fn window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    let foreground = match event {
        tauri::WindowEvent::Suspended => false,
        tauri::WindowEvent::Resumed => true,
        _ => return,
    };
    let app = window.app_handle();
    let (Some(service), Some(keep)) = (
        app.try_state::<Service>(),
        app.try_state::<Arc<KeepAlive>>(),
    ) else {
        return;
    };
    if foreground {
        keep.resumed(&service);
        // The WebView was not painting while hidden; reload the snapshot.
        let _ = app.emit("nekopak://transfer/resync-required", ());
    } else if keep.suspended(&service).0 {
        // Gated synchronously above; the durable pause follows off-thread.
        pause(app, service.inner().clone());
    }
}

#[tauri::command]
pub fn desktop_status() -> Result<serde_json::Value, nekopak_domain::ClientError> {
    Ok(serde_json::json!({"tray_available":false,"window_visible":true}))
}
