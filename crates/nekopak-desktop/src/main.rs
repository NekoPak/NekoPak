//! NekoPak native desktop client. The UI is rendered directly by Slint
//! (GPU with a CPU fallback); there is no embedded browser or WebView2.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

slint::include_modules!();

mod actions;
mod app;
mod auth;
#[cfg(feature = "automation")]
mod automation;
mod cloud;
mod collate;
mod files;
mod format;
mod generated {
    pub mod icons;
    pub mod palettes;
}
mod host;
mod i18n;
mod media;
mod names;
mod nav;
mod overlay;
mod pages;
mod platform;
mod prefs;
mod settings;
mod settings_page;
mod shortcuts;
mod tools;
mod transfers;
mod updates;

use app::App;
use host::HostEvent;
use slint::ComponentHandle;
use std::rc::Rc;

fn fail(message: &str) -> ! {
    eprintln!("NekoPak could not start: {message}");
    let _ = rfd::MessageDialog::new()
        .set_title("NekoPak")
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .show();
    std::process::exit(1);
}

fn main() {
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => fail(&error.to_string()),
    };
    // Providers create timers and sockets, so the host starts inside the runtime.
    let guard = runtime.enter();
    let host = match host::start() {
        Ok(host) => host,
        Err(message) => fail(&message),
    };
    let ui = match AppWindow::new() {
        Ok(ui) => ui,
        Err(error) => fail(&format!("The window could not be created: {error}")),
    };
    let (events, mut inbox) = tokio::sync::mpsc::unbounded_channel::<HostEvent>();
    host::spawn_workers(&host.service, events.clone());
    drop(guard);

    let ctx = App::new(ui, host, runtime.handle().clone());
    let translations = ctx.ui.global::<Tr>();
    translations.on_translate(|source| i18n::tr(&source).into());
    translations.on_format1(|source, a| i18n::tx(&source, &[&a]).into());
    translations.on_format2(|source, a, b| i18n::tx(&source, &[&a, &b]).into());
    overlay::install(&ctx);
    nav::install(&ctx);
    files::install(&ctx);
    shortcuts::install(&ctx);
    ctx.ui.on_scheme_changed({
        let weak = Rc::downgrade(&ctx);
        move |_| {
            if let Some(ctx) = weak.upgrade() {
                settings::apply_theme(&ctx);
            }
        }
    });
    settings::apply_theme(&ctx);
    ctx.ui.global::<Shell>().on_retry_boot({
        let weak = Rc::downgrade(&ctx);
        move || {
            if let Some(ctx) = weak.upgrade() {
                let task = ctx.clone();
                task.spawn(async move { settings::boot(&ctx).await });
            }
        }
    });
    ctx.spawn({
        let ctx = ctx.clone();
        async move { settings::boot(&ctx).await }
    });
    ctx.spawn({
        let ctx = ctx.clone();
        async move {
            while let Some(event) = inbox.recv().await {
                match event {
                    HostEvent::TransfersChanged => transfers::request_reload(&ctx),
                    HostEvent::AuthChanged(event) => settings::on_auth_changed(&ctx, event),
                    HostEvent::StorageError(error) => ctx.toast_error(&error),
                    HostEvent::Quit => {
                        let _ = slint::quit_event_loop();
                    }
                    HostEvent::Activate(_) | HostEvent::ScanProgress(_) | HostEvent::SyncChanged(_) | HostEvent::SyncPlanReady(_) => {}
                }
            }
        }
    });
    #[cfg(feature = "automation")]
    automation::install(&ctx);
    let _events = events;
    if let Err(error) = ctx.ui.run() {
        fail(&error.to_string());
    }
    // Stop background workers without waiting on in-flight network calls.
    runtime.shutdown_background();
}
