//! Application updates: signed manifest check, verified download and a
//! user-confirmed installer launch. Nothing is installed silently.
use crate::{
    app::{error_text, App, Ctx},
    format,
    i18n::{tr, tx},
    nav::Route,
    panel::Row,
};
use nekopak_core::updates::UpdateStatus;
use std::time::Duration;

pub fn status(ctx: &Ctx) -> Option<UpdateStatus> {
    ctx.updater
        .as_ref()
        .and_then(|updater| updater.status().ok())
}

pub fn rows(ctx: &Ctx) -> Vec<Row> {
    let mut rows = vec![Row::header(tr("应用更新"))];
    let Some(status) = status(ctx).filter(|status| status.supported && ctx.feature("app_updates"))
    else {
        rows.push(Row::note(
            tr("请在支持的桌面或手机应用中检查更新。"),
            "info",
        ));
        return rows;
    };
    let auto = ctx.core.borrow().settings["updates"]["auto_check"] == true;
    rows.push(
        Row::toggle("update-auto", tr("启动时检查更新"), auto)
            .detail(tr("预览通道 · 下载和安装需手动确认")),
    );
    let checked = status
        .checked_at
        .as_deref()
        .map(|at| format!("{} {}", tr("上次检查"), format::relative(Some(at))))
        .unwrap_or_default();
    match (status.phase.as_str(), &status.update) {
        ("checking", _) => rows.push(Row::progress("", tr("检查更新"), -1.0)),
        ("downloading", Some(update)) => {
            let total = update.size_bytes.0;
            let value = if total > 0 {
                status.downloaded_bytes.0 as f32 / total as f32
            } else {
                -1.0
            };
            rows.push(
                Row::progress("update-cancel", tx("新版本 {0}", &[&update.version]), value)
                    .detail(format!(
                        "{} / {}",
                        format::bytes(status.downloaded_bytes.0),
                        format::bytes(total)
                    ))
                    .action(tr("取消下载")),
            );
        }
        ("ready", Some(update)) => rows.push(
            Row::button(
                "update-install",
                tx("新版本 {0}", &[&update.version]),
                tr("安装更新"),
            )
            .detail(tr("更新包已验证，可以安装。"))
            .icon("check-circle")
            .tone("ok"),
        ),
        (_, Some(update)) => {
            rows.push(
                Row::button(
                    "update-download",
                    tx("发现新版本 {0}", &[&update.version]),
                    tr("下载更新"),
                )
                .detail(format!(
                    "{} · {}",
                    format::bytes(update.size_bytes.0),
                    format::date_time(Some(&update.published_at))
                ))
                .icon("download")
                .tone("primary"),
            );
            for line in update
                .notes
                .lines()
                .filter(|line| !line.trim().is_empty())
                .take(12)
            {
                rows.push(Row::note(line.trim().to_owned(), "info"));
            }
        }
        (_, None) => rows.push(
            Row::button("update-check", tr("检查更新"), tr("检查更新")).detail(
                if status.checked_at.is_some() {
                    format!("{} · {}", tr("当前通道没有更高版本。"), checked)
                } else {
                    tx("当前版本 {0}", &[&status.current_version])
                },
            ),
        ),
    }
    if let Some(error) = &status.error {
        rows.push(Row::note(error_text(error), "error"));
    }
    rows
}

/// Handles the update rows of the about page. Returns true when it owned the id.
pub async fn handle(ctx: &Ctx, id: &str) -> bool {
    let Some(updater) = ctx.updater.clone() else {
        return false;
    };
    match id {
        "update-auto" => {
            let enabled = ctx.core.borrow().settings["updates"]["auto_check"] != true;
            crate::settings::update_settings(
                ctx,
                serde_json::json!({ "updates": { "auto_check": enabled } }),
            )
            .await;
        }
        "update-check" => check(ctx, false).await,
        "update-download" => {
            let Some(update) = status(ctx).and_then(|status| status.update) else {
                return true;
            };
            let settings = ctx.core.borrow().settings.clone();
            match updater.start_download(&update.update_id, settings) {
                Ok(()) => watch_download(ctx),
                Err(error) => ctx.toast_error(&error),
            }
        }
        "update-cancel" => {
            if let Err(error) = updater.cancel() {
                ctx.toast_error(&error);
            }
        }
        "update-install" => install(ctx).await,
        _ => return false,
    }
    true
}

async fn check(ctx: &Ctx, quiet: bool) {
    let Some(updater) = ctx.updater.clone() else {
        return;
    };
    let settings = ctx.core.borrow().settings.clone();
    let runtime_updater = updater.clone();
    let result = ctx
        .call(move |_| async move { runtime_updater.check(&settings).await })
        .await;
    match result {
        Ok(status) => match status.update {
            Some(update) => ctx.toast_action(
                tx("发现新版本 {0}", &[&update.version]),
                "info",
                tr("查看更新"),
                |ctx| {
                    ctx.navigate(Route::Settings {
                        section: "about".into(),
                    })
                },
            ),
            None if !quiet => ctx.toast(tr("当前通道没有更高版本。"), "info"),
            None => {}
        },
        Err(error) if !quiet => ctx.toast_error(&error),
        Err(_) => {}
    }
    refresh_page(ctx);
}

pub fn check_at_startup(ctx: &Ctx) {
    let ctx = ctx.clone();
    ctx.clone().spawn(async move { check(&ctx, true).await });
}

fn refresh_page(ctx: &Ctx) {
    if matches!(ctx.route(), Route::Settings { ref section } if section == "about") {
        crate::pages::enter(ctx, &ctx.route());
    }
}

/// Repaints the progress row until the download leaves the `downloading` phase.
fn watch_download(ctx: &Ctx) {
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        loop {
            refresh_page(&ctx);
            if status(&ctx).is_none_or(|status| status.phase != "downloading") {
                break;
            }
            let (done, wait) = tokio::sync::oneshot::channel::<()>();
            slint::Timer::single_shot(Duration::from_millis(400), move || {
                let _ = done.send(());
            });
            let _ = wait.await;
        }
        if status(&ctx).is_some_and(|status| status.phase == "ready") {
            ctx.toast(tr("更新包已验证，可以安装。"), "success");
        }
    });
}

async fn install(ctx: &Ctx) {
    let Some(updater) = ctx.updater.clone() else {
        return;
    };
    let Some(update) = status(ctx).and_then(|status| status.update) else {
        return;
    };
    let body = tr("Windows 更新使用安装器；便携版的原目录会保留。");
    if !ctx
        .ask(
            &tx("新版本 {0}", &[&update.version]),
            &body,
            &tr("安装更新"),
            false,
        )
        .await
    {
        return;
    }
    let service = ctx.svc.clone();
    let result = ctx
        .blocking(move || {
            let file = updater.verified_file(&update.update_id)?;
            if !cfg!(windows) {
                return Err(nekopak_domain::ClientError::new(
                    nekopak_domain::ErrorCode::Unsupported,
                    "当前平台尚未支持安装更新。",
                ));
            }
            service.set_foreground(false);
            if let Err(error) = service.pause_local_transfers() {
                service.set_foreground(true);
                return Err(error);
            }
            // No shell, elevation request or silent switch: the user confirms
            // installation in the normal installer wizard.
            if std::process::Command::new(&file.path).spawn().is_err() {
                service.set_foreground(true);
                return Err(nekopak_domain::ClientError::new(
                    nekopak_domain::ErrorCode::Internal,
                    "无法启动更新安装器，已保留更新包。",
                ));
            }
            Ok(())
        })
        .await;
    match result {
        Ok(()) => {
            let _ = slint::quit_event_loop();
        }
        Err(error) => ctx.toast_error(&error),
    }
}

impl App {
    pub fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
}
