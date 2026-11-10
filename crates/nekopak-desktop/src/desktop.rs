//! Desktop shell integration: single instance, system tray and close-to-tray.
use crate::{app::Ctx, host::HostEvent};
use slint::ComponentHandle;
use tokio::sync::mpsc::UnboundedSender;

pub fn tray_available() -> bool {
    cfg!(windows)
}

/// Returns false when another instance already runs (it has been asked to show itself).
pub fn claim_single_instance(events: &UnboundedSender<HostEvent>) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{GetLastError, ERROR_ALREADY_EXISTS, WAIT_OBJECT_0},
            System::Threading::{CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE},
        };
        fn wide(text: &str) -> Vec<u16> {
            text.encode_utf16().chain(Some(0)).collect()
        }
        // Per-session names: each signed-in Windows user gets an own instance.
        let mutex = wide("Local\\io.nekopak.desktop.instance");
        let signal = wide("Local\\io.nekopak.desktop.activate");
        unsafe {
            // The handles are intentionally kept for the lifetime of the process.
            let handle = CreateMutexW(std::ptr::null(), 0, mutex.as_ptr());
            let already = !handle.is_null() && GetLastError() == ERROR_ALREADY_EXISTS;
            let event = CreateEventW(std::ptr::null(), 0, 0, signal.as_ptr());
            if already {
                if !event.is_null() {
                    SetEvent(event);
                }
                return false;
            }
            if !event.is_null() {
                let events = events.clone();
                let event = event as usize;
                std::thread::spawn(move || loop {
                    if WaitForSingleObject(event as _, INFINITE) != WAIT_OBJECT_0 || events.send(HostEvent::Activate(None)).is_err() {
                        break;
                    }
                });
            }
        }
        true
    }
    #[cfg(not(windows))]
    {
        let _ = events;
        true
    }
}

#[cfg(windows)]
pub struct Tray(#[allow(dead_code)] tray_icon::TrayIcon);
#[cfg(not(windows))]
pub struct Tray;

/// Creates the tray icon. Menu and click events are forwarded to the UI loop.
pub fn install_tray(events: &UnboundedSender<HostEvent>) -> Option<Tray> {
    #[cfg(windows)]
    {
        use tray_icon::{
            menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
            Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
        };
        let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!("../assets/icon.png").as_slice()));
        let mut reader = decoder.read_info().ok()?;
        let mut pixels = vec![0; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut pixels).ok()?;
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return None;
        }
        pixels.truncate(info.buffer_size());
        let icon = Icon::from_rgba(pixels, info.width, info.height).ok()?;
        let menu = Menu::new();
        let show = MenuItem::with_id("show", "显示 NekoPak / Show", true, None);
        let downloads = MenuItem::with_id("downloads", "下载任务 / Downloads", true, None);
        let hide = MenuItem::with_id("hide", "隐藏窗口 / Hide", true, None);
        let quit = MenuItem::with_id("quit", "退出 / Quit", true, None);
        menu.append_items(&[&show, &downloads, &hide, &PredefinedMenuItem::separator(), &quit]).ok()?;
        let tray = TrayIconBuilder::new()
            .with_tooltip("NekoPak")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .ok()?;
        let sender = events.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let _ = sender.send(match event.id.as_ref() {
                "show" => HostEvent::Activate(None),
                "downloads" => HostEvent::Activate(Some("transfers")),
                "hide" => HostEvent::Activate(Some("hide")),
                "quit" => HostEvent::Quit,
                _ => return,
            });
        }));
        let sender = events.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                let _ = sender.send(HostEvent::Activate(None));
            }
        }));
        Some(Tray(tray))
    }
    #[cfg(not(windows))]
    {
        let _ = events;
        None
    }
}

pub fn show_window(ctx: &Ctx) {
    let window = ctx.ui.window();
    let _ = ctx.ui.show();
    window.set_minimized(false);
    ctx.svc.set_foreground(true);
}

pub fn activate(ctx: &Ctx, target: Option<&'static str>) {
    match target {
        Some("hide") => {
            let _ = ctx.ui.hide();
        }
        Some("transfers") => {
            show_window(ctx);
            ctx.navigate(crate::nav::Route::Transfers { tab: "all".into() });
        }
        _ => show_window(ctx),
    }
}

/// Closing hides to the tray when the user enabled it and a tray exists.
pub fn install_close_handler(ctx: &Ctx, has_tray: bool) {
    let weak = std::rc::Rc::downgrade(ctx);
    ctx.ui.window().on_close_requested(move || {
        let Some(ctx) = weak.upgrade() else { return slint::CloseRequestResponse::HideWindow };
        let to_tray = has_tray && ctx.core.borrow().settings["close_to_tray"] == true;
        if !to_tray {
            let _ = slint::quit_event_loop();
        }
        slint::CloseRequestResponse::HideWindow
    });
}
