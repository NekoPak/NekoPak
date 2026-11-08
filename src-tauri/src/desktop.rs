use crate::{internal, Service};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, Runtime};

fn show<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn install<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "显示 NekoPak / Show", true, None::<&str>)?;
    let downloads =
        MenuItem::with_id(app, "downloads", "下载任务 / Downloads", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "隐藏窗口 / Hide", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 / Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show_item,
            &downloads,
            &hide,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id("nekopak")
        .tooltip("NekoPak")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show(app),
            "downloads" => {
                show(app);
                let _ = app.emit("nekopak://desktop/navigate", "transfers");
            }
            "hide" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        let app = window.app_handle();
        let retain = app
            .try_state::<Service>()
            .and_then(|service| service.settings_get().ok())
            .is_some_and(|settings| settings["close_to_tray"] == true);
        if retain && app.tray_by_id("nekopak").is_some() {
            api.prevent_close();
            let _ = window.hide();
        }
    }
}

#[tauri::command]
pub fn desktop_status<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<serde_json::Value, nekopak_domain::ClientError> {
    let window = app.get_webview_window("main").ok_or_else(internal)?;
    Ok(
        serde_json::json!({"tray_available":app.tray_by_id("nekopak").is_some(),"window_visible":window.is_visible().map_err(|_|internal())?}),
    )
}
