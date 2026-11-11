//! Window events Slint does not surface itself: focus (clipboard link
//! reminder), files dragged in from the system, and the back/forward mouse
//! buttons.
use crate::{
    app::Ctx,
    files::Mode,
    i18n::{tr, tx},
    nav::Route,
    Shell,
};
use nekopak_domain::Operation;
use slint::{
    winit_030::{
        winit::event::{ElementState, MouseButton, WindowEvent},
        EventResult, WinitWindowAccessor,
    },
    ComponentHandle,
};
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

/// Folder a system drop would upload into, or why it cannot be accepted.
fn drop_target(ctx: &Ctx) -> Result<(String, String), String> {
    if ctx.dialog_open() {
        return Err(tr("请先关闭当前对话框"));
    }
    let state = ctx.files.borrow();
    if !matches!(ctx.route(), Route::Files { .. }) || state.mode != Mode::Files {
        return Err(tr("打开一个网盘文件夹后，可以把文件拖进来上传"));
    }
    ctx.gate(Operation::Upload)?;
    let name = state
        .crumbs()
        .last()
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| tr("网盘"));
    Ok((state.folder.clone(), name))
}

fn show_hint(ctx: &Ctx) {
    let shell = ctx.ui.global::<Shell>();
    match drop_target(ctx) {
        Ok((_, name)) => {
            shell.set_drop_ok(true);
            shell.set_drop_hint(tx("松开即可上传到“{0}”", &[&name]).into());
        }
        Err(reason) => {
            shell.set_drop_ok(false);
            shell.set_drop_hint(reason.into());
        }
    }
}

fn dropped(ctx: &Ctx, paths: Vec<PathBuf>) {
    let paths: Vec<String> = paths
        .iter()
        .filter_map(|path| path.to_str().map(str::to_owned))
        .collect();
    if paths.is_empty() {
        return;
    }
    // A single torrent is far more likely meant for cloud download than upload.
    if paths.len() == 1
        && paths[0].to_ascii_lowercase().ends_with(".torrent")
        && ctx.feature("torrent_import")
        && ctx.can(Operation::CloudTask)
        && !ctx.dialog_open()
    {
        let (ctx, path) = (ctx.clone(), PathBuf::from(&paths[0]));
        ctx.clone()
            .spawn(async move { crate::cloud::import_torrent(&ctx, path).await });
        return;
    }
    match drop_target(ctx) {
        Ok((folder, _)) => {
            let ctx = ctx.clone();
            ctx.clone()
                .spawn(async move { crate::actions::upload_paths(&ctx, paths, folder).await });
        }
        Err(reason) => ctx.toast(reason, "info"),
    }
}

pub fn install(ctx: &Ctx) {
    let weak = Rc::downgrade(ctx);
    let pending: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    ctx.ui.window().on_winit_window_event(move |_, event| {
        let Some(ctx) = weak.upgrade() else {
            return EventResult::Propagate;
        };
        match event {
            WindowEvent::Focused(true) => crate::cloud::clipboard_check(&ctx),
            WindowEvent::HoveredFile(_) => show_hint(&ctx),
            WindowEvent::HoveredFileCancelled => {
                ctx.ui.global::<Shell>().set_drop_hint(Default::default());
            }
            WindowEvent::DroppedFile(path) => {
                ctx.ui.global::<Shell>().set_drop_hint(Default::default());
                let first = pending.borrow().is_empty();
                pending.borrow_mut().push(path.clone());
                if first {
                    // One event arrives per file; act once the whole drop is in.
                    let (ctx, pending) = (ctx.clone(), pending.clone());
                    slint::Timer::single_shot(Duration::from_millis(80), move || {
                        let paths = std::mem::take(&mut *pending.borrow_mut());
                        dropped(&ctx, paths);
                    });
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } if !ctx.dialog_open() && !ctx.menu_open() => match button {
                MouseButton::Back => ctx.navigate_history(-1),
                MouseButton::Forward => ctx.navigate_history(1),
                _ => {}
            },
            _ => {}
        }
        EventResult::Propagate
    });
}
