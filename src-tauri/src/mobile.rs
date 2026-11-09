use crate::Service;
use tauri::{Emitter, Manager, Runtime};

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
