//! Window-wide keyboard shortcuts.
use crate::{app::Ctx, files, nav::Route, Shell};
use slint::{platform::Key, ComponentHandle, SharedString};
use std::rc::Rc;

pub fn install(ctx: &Ctx) {
    let weak = Rc::downgrade(ctx);
    ctx.ui
        .global::<Shell>()
        .on_shortcut(move |text, ctrl, shift, alt| {
            weak.upgrade()
                .is_some_and(|ctx| handle(&ctx, &text, ctrl, shift, alt))
        });
}

fn handle(ctx: &Ctx, text: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
    let is = |key: Key| text == SharedString::from(key).as_str();
    if ctx.menu_open() {
        return false;
    }
    if ctx.dialog_open() {
        return is(Key::Escape) && ctx.cancel_top_dialog();
    }
    let lower = text.to_lowercase();
    if ctrl && !alt {
        match lower.as_str() {
            "t" => {
                ctx.open_tab(Route::root(), None);
            }
            "w" => ctx.close_tab(None),
            "\t" => ctx.cycle_tab(if shift { -1 } else { 1 }),
            "z" if !shift => ctx.undo_last(),
            "f" if !shift => files::open_search(ctx),
            "b" => ctx.toggle_sidebar(),
            "," => ctx.navigate(Route::Settings {
                section: "general".into(),
            }),
            "r" => refresh(ctx),
            "v" if !shift && matches!(ctx.route(), Route::Files { .. }) => {
                let task = ctx.clone();
                ctx.spawn(async move { crate::actions::paste(&task).await });
            }
            "1" => ctx.navigate(Route::root()),
            "2" => ctx.navigate(Route::Transfers { tab: "all".into() }),
            "3" => ctx.navigate(Route::Recent),
            "4" => ctx.navigate(Route::Starred),
            "5" => ctx.navigate(Route::Shares),
            "6" => ctx.navigate(Route::Trash),
            "7" => ctx.navigate(Route::Sync),
            "8" => ctx.navigate(Route::Mount),
            _ => return ctx.route().page() == "files" && files::key(ctx, text, ctrl, shift, alt),
        }
        return true;
    }
    if alt && is(Key::LeftArrow) {
        ctx.navigate_history(-1);
    } else if alt && is(Key::RightArrow) {
        ctx.navigate_history(1);
    } else if is(Key::F5) {
        refresh(ctx);
    } else {
        // Keys the focused control did not use still drive the file list.
        return ctx.route().page() == "files" && files::key(ctx, text, ctrl, shift, alt);
    }
    true
}

fn refresh(ctx: &Ctx) {
    match ctx.route().page() {
        "files" => files::reload(ctx),
        "transfers" => crate::transfers::request_reload(ctx),
        _ => crate::pages::enter(ctx, &ctx.route()),
    }
}
