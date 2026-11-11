//! Start-up, re-synchronisation with the core, theme and preferences.
use crate::{
    app::{error_text, error_title, request, Ctx},
    generated::palettes,
    i18n,
    nav::Route,
    prefs::AccountPrefs,
    Pal, Shell, Tk, Tr,
};
use nekopak_domain::*;
use slint::ComponentHandle;

pub fn apply_theme(ctx: &Ctx) {
    let tokens = ctx.ui.global::<Tk>();
    let prefs = ctx.prefs.borrow();
    let accent = palettes::ACCENTS
        .iter()
        .find(|accent| accent.id == prefs.device.accent)
        .unwrap_or(&palettes::ACCENTS[0]);
    palettes::apply(
        &ctx.ui.global::<Pal>(),
        if tokens.get_dark() {
            &accent.dark
        } else {
            &accent.light
        },
    );
    tokens.set_reduce_motion(prefs.device.reduce_motion);
    tokens.set_compact(prefs.account.density == "compact");
}

/// Applies theme and language from the core settings document.
pub fn apply_settings(ctx: &Ctx) {
    let (theme, language) = {
        let core = ctx.core.borrow();
        (
            core.settings["theme"]
                .as_str()
                .unwrap_or("system")
                .to_owned(),
            core.settings["language"]
                .as_str()
                .unwrap_or("system")
                .to_owned(),
        )
    };
    i18n::apply_preference(&language);
    let translations = ctx.ui.global::<Tr>();
    translations.set_rev(translations.get_rev() + 1);
    ctx.ui.global::<Tk>().set_theme_pref(theme.into());
    apply_theme(ctx);
}

pub async fn boot(ctx: &Ctx) {
    let shell = ctx.ui.global::<Shell>();
    shell.set_boot("loading".into());
    let status = ctx.svc.app_status();
    let storage_error = status.storage.error.clone();
    ctx.core.borrow_mut().status = Some(status);
    if let Some(error) = storage_error {
        // A damaged journal is kept as-is; nothing is reset behind the user's back.
        shell.set_boot_error_title(error_title(&error).into());
        shell.set_boot_error_body(error_text(&error).into());
        shell.set_boot("fatal".into());
        return;
    }
    match ctx
        .call(|service| async move { service.settings_get() })
        .await
    {
        Ok(settings) => ctx.core.borrow_mut().settings = settings,
        Err(error) => {
            shell.set_boot_error_title(error_title(&error).into());
            shell.set_boot_error_body(error_text(&error).into());
            shell.set_boot("fatal".into());
            return;
        }
    }
    apply_settings(ctx);
    refresh_caps(ctx).await;
    refresh_auth(ctx).await;
    refresh_quota(ctx).await;
    shell.set_boot("ready".into());
    ctx.route_changed();
    crate::transfers::request_reload(ctx);
    if !ctx.signed_in() && !ctx.mock {
        crate::auth::open_sign_in(ctx);
    }
    let auto_check = ctx.core.borrow().settings["updates"]["auto_check"] == true;
    if auto_check && ctx.feature("app_updates") {
        crate::updates::check_at_startup(ctx);
    }
}

pub async fn refresh_caps(ctx: &Ctx) {
    match ctx
        .call(|service| async move { service.backend_discover(&request()).await })
        .await
    {
        Ok(caps) => ctx.core.borrow_mut().caps = caps,
        Err(error) => ctx.core.borrow_mut().auth_error = Some(error),
    }
}

pub async fn refresh_auth(ctx: &Ctx) {
    match ctx
        .call(|service| async move { service.auth_status(&request()).await })
        .await
    {
        Ok(auth) => {
            let previous = ctx
                .core
                .borrow()
                .auth
                .as_ref()
                .and_then(|auth| auth.account_id.clone());
            let changed =
                previous.is_some() && auth.account_id.is_some() && previous != auth.account_id;
            let signed_in = auth.authenticated;
            {
                let mut core = ctx.core.borrow_mut();
                core.auth = Some(auth);
                core.auth_error = None;
            }
            if changed {
                clear_account_data(ctx);
            }
            if signed_in {
                load_account_prefs(ctx).await;
            }
        }
        Err(error) => ctx.core.borrow_mut().auth_error = Some(error),
    }
}

pub async fn refresh_quota(ctx: &Ctx) {
    if !ctx.signed_in() {
        ctx.core.borrow_mut().quota = None;
        return;
    }
    let session = ctx.session();
    let quota = ctx
        .call(|service| async move { service.quota_get(&request()).await })
        .await;
    if session == ctx.session() {
        ctx.core.borrow_mut().quota = quota.ok();
        ctx.refresh_shell();
    }
}

async fn load_account_prefs(ctx: &Ctx) {
    let session = ctx.session();
    match ctx
        .call(|service| async move { service.ui_prefs_get(&request()).await })
        .await
    {
        Ok(value) if session == ctx.session() => {
            {
                let mut prefs = ctx.prefs.borrow_mut();
                prefs.account = AccountPrefs::normalized(&value["prefs"]);
                prefs.account_id = value["account_id"].as_str().map(str::to_owned);
            }
            apply_theme(ctx);
        }
        Ok(_) => {}
        Err(error) => {
            if session == ctx.session() && ctx.feature("ui_prefs") {
                ctx.toast_error(&error);
            }
        }
    }
}

/// Changes account-scoped preferences and saves them for the account they were read for.
pub fn update_account_prefs(ctx: &Ctx, change: impl FnOnce(&mut AccountPrefs)) -> bool {
    let (snapshot, account) = {
        let mut prefs = ctx.prefs.borrow_mut();
        if prefs.account_id.is_none() && !ctx.mock && ctx.signed_in() {
            drop(prefs);
            ctx.toast(i18n::tr("账号偏好尚未读取，请连接账号后重试"), "info");
            return false;
        }
        change(&mut prefs.account);
        (prefs.account.clone(), prefs.account_id.clone())
    };
    apply_theme(ctx);
    if let Some(account) = account {
        let session = ctx.session();
        let ctx = ctx.clone();
        ctx.clone().spawn(async move {
            let Ok(value) = serde_json::to_value(&snapshot) else {
                return;
            };
            let result = ctx
                .call(move |service| async move {
                    service.ui_prefs_update(&value, &account, &request()).await
                })
                .await;
            if let Err(error) = result {
                if session == ctx.session() {
                    ctx.toast_error(&error);
                }
            }
        });
    }
    true
}

pub fn update_device_prefs(ctx: &Ctx, change: impl FnOnce(&mut crate::prefs::DevicePrefs)) {
    {
        let mut prefs = ctx.prefs.borrow_mut();
        change(&mut prefs.device);
        prefs.save_device();
    }
    apply_theme(ctx);
    ctx.refresh_shell();
}

/// Drops everything that belongs to the previous account.
pub fn clear_account_data(ctx: &Ctx) {
    {
        let mut core = ctx.core.borrow_mut();
        core.quota = None;
        core.session += 1;
        core.epoch += 1;
    }
    {
        let mut prefs = ctx.prefs.borrow_mut();
        prefs.account = AccountPrefs::default();
        prefs.account_id = None;
    }
    ctx.files.borrow_mut().clear_session();
    *ctx.undo.borrow_mut() = None;
    ctx.cancel_top_dialog();
    ctx.finish_menu(None);
}

/// Re-reads everything after sign-in, sign-out, account switch or reconnect.
pub async fn resync(ctx: &Ctx) {
    refresh_caps(ctx).await;
    refresh_auth(ctx).await;
    refresh_quota(ctx).await;
    ctx.core.borrow_mut().epoch += 1;
    crate::transfers::request_reload(ctx);
    ctx.route_changed();
}

pub async fn update_settings(ctx: &Ctx, patch: serde_json::Value) -> bool {
    match ctx
        .call(move |service| async move { service.settings_update(&patch) })
        .await
    {
        Ok(settings) => {
            ctx.core.borrow_mut().settings = settings;
            apply_settings(ctx);
            ctx.refresh_shell();
            true
        }
        Err(error) => {
            ctx.toast_error(&error);
            false
        }
    }
}

pub fn on_auth_changed(ctx: &Ctx, event: AuthChangedEvent) {
    if matches!(event.state, AuthStatus::SignedOut | AuthStatus::Expired) {
        clear_account_data(ctx);
    }
    if let Some(error) = &event.error {
        ctx.toast_error(error);
    }
    let ctx = ctx.clone();
    ctx.clone().spawn(async move { resync(&ctx).await });
}

pub fn enter(ctx: &Ctx, route: &Route) {
    crate::settings_page::enter(ctx, route);
}
