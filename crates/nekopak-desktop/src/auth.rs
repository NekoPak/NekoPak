//! Sign-in, human verification and saved accounts. The password only lives
//! in the form field and the single request that uses it.
use crate::{
    app::{error_text, request, App, Ctx},
    i18n::tr,
    overlay::{Field, FormEvent},
    settings,
};
use nekopak_domain::*;
use std::{cell::Cell, time::Duration};
use zeroize::Zeroizing;

thread_local! {
    static OPEN: Cell<bool> = const { Cell::new(false) };
}

fn auth_context() -> nekopak_provider::RequestContext {
    nekopak_provider::RequestContext::new(Duration::from_secs(30))
}

fn message_of(error: &ClientError) -> String {
    if error.code == ErrorCode::TransientNetwork {
        tr("无法连接 PikPak，请检查网络后重试")
    } else {
        error_text(error)
    }
}

pub fn open_sign_in(ctx: &Ctx) {
    if OPEN.with(|open| open.replace(true)) {
        return;
    }
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        sign_in_dialog(&ctx).await;
        OPEN.with(|open| open.set(false));
    });
}

async fn sign_in_dialog(ctx: &Ctx) {
    if ctx.mock {
        return ctx.notice(&tr("演示"), &tr("离线界面预览：未连接 PikPak，不使用真实账号或密码。")).await;
    }
    let available = ctx.feature("auth_sign_in");
    let preferences = ctx.call(|service| async move { service.auth_preferences_get(&request()).await }).await.ok();
    let remembered = preferences.as_ref().and_then(|value| value["username"].as_str()).unwrap_or_default().to_owned();
    let mut fields = Vec::new();
    if ctx.auth_expired() {
        fields.push(Field::note("expired", tr("登录已过期，请重新登录。"), "warn"));
    }
    if !available {
        fields.push(Field::note("unavailable", tr("当前版本尚不能连接账号，可以先浏览界面。"), "info"));
    }
    let mut user = Field::text("user", tr("账号"), remembered.clone()).placeholder(tr("邮箱")).disabled(!available);
    let mut pass = Field::password("pass", tr("密码")).disabled(!available);
    if remembered.is_empty() {
        user = user.focus();
    } else {
        pass = pass.focus();
    }
    fields.push(user);
    fields.push(pass);
    if let Some(value) = &preferences {
        fields.push(Field::toggle("remember", tr("记住账号"), value["remember_account"] == true));
        fields.push(Field::toggle("restore", tr("下次自动登录"), value["restore_session"] != false));
    }
    let mut form = ctx.form(&tr("登录 PikPak"), &tr("NekoPak 是社区开发的 PikPak 客户端，与 PikPak 官方无关。"), &tr("登录"), "", fields);
    form.set_confirm(&tr("登录"), available);
    loop {
        match form.next().await {
            FormEvent::Submit => {
                let user = form.value("user").trim().to_owned();
                let password = Zeroizing::new(form.value("pass"));
                if user.is_empty() {
                    form.set("user", |field| field.error = tr("请输入账号"));
                    continue;
                }
                if password.is_empty() {
                    form.set("pass", |field| field.error = tr("请输入密码"));
                    continue;
                }
                form.set_error("");
                form.set_busy(true);
                if preferences.is_some() {
                    let remember = form.checked("remember");
                    let mut value = serde_json::json!({ "remember_account": remember, "restore_session": form.checked("restore") });
                    if remember {
                        value["username"] = user.clone().into();
                    }
                    let _ = ctx.call(move |service| async move { service.auth_preferences_update(&value, &request()).await }).await;
                }
                let result = ctx
                    .call({
                        let user = user.clone();
                        move |service| async move { service.auth_sign_in(&user, &password, &auth_context()).await }
                    })
                    .await;
                // Never keep the password in the dialog after an attempt.
                form.set("pass", |field| field.value.clear());
                form.set_busy(false);
                match result {
                    Ok(SignInResult::SignedIn) => break,
                    Ok(SignInResult::Challenge { challenge }) => {
                        form.close();
                        if verify(ctx, challenge).await {
                            signed_in(ctx).await;
                        }
                        return;
                    }
                    Err(error) => form.set_error(message_of(&error)),
                }
            }
            FormEvent::Cancel => {
                form.close();
                return;
            }
            _ => {}
        }
    }
    form.close();
    signed_in(ctx).await;
}

async fn signed_in(ctx: &Ctx) {
    settings::clear_account_data(ctx);
    settings::resync(ctx).await;
    ctx.toast(tr("已登录"), "success");
}

/// Human verification in the system browser, then completion in the app.
async fn verify(ctx: &Ctx, mut challenge: SignInChallenge) -> bool {
    loop {
        let mut fields = vec![Field::note("note", tr("PikPak 要求先完成一次安全验证。"), "info")];
        if challenge.requires_password {
            fields.push(Field::password("pass", tr("重新输入密码")).focus());
        }
        let mut form = ctx.form(&tr("需要安全验证"), "", &tr("已完成验证，继续登录"), &tr("打开验证页面"), fields);
        let next = loop {
            match form.next().await {
                FormEvent::Alt => ctx.open_link(&challenge.url),
                FormEvent::Submit => {
                    let password = challenge.requires_password.then(|| Zeroizing::new(form.value("pass")));
                    if password.as_ref().is_some_and(|password| password.is_empty()) {
                        form.set("pass", |field| field.error = tr("请重新输入密码以完成登录"));
                        continue;
                    }
                    form.set_busy(true);
                    let id = challenge.challenge_id.clone();
                    let result = ctx
                        .call(move |service| async move {
                            service.auth_verify(&id, "", password.as_deref().map(|password| password.as_str()), &auth_context()).await
                        })
                        .await;
                    form.set_busy(false);
                    if challenge.requires_password {
                        form.set("pass", |field| field.value.clear());
                    }
                    match result {
                        Ok(SignInResult::SignedIn) => break Some(None),
                        Ok(SignInResult::Challenge { challenge }) => break Some(Some(challenge)),
                        Err(error) => form.set_error(message_of(&error)),
                    }
                }
                FormEvent::Cancel => break None,
                _ => {}
            }
        };
        form.close();
        match next {
            Some(None) => return true,
            Some(Some(following)) => challenge = following,
            None => return false,
        }
    }
}

pub async fn sign_out(ctx: &Ctx) {
    if !ctx.ask(&tr("退出登录"), &tr("退出后需要重新登录才能访问云盘，本地任务记录会保留。"), &tr("退出登录"), false).await {
        return;
    }
    let backend = crate::host::backend_id(ctx.mock);
    match ctx.call(move |service| async move { service.auth_logout(backend, &request()).await }).await {
        Ok(()) => {
            settings::clear_account_data(ctx);
            settings::resync(ctx).await;
            ctx.toast(tr("已退出登录"), "success");
        }
        Err(error) => ctx.toast_error(&error),
    }
}

pub async fn switch_account(ctx: &Ctx, id: String) {
    match ctx.call(move |service| async move { service.account_switch(&id, &request()).await }).await {
        Ok(_) => {
            settings::clear_account_data(ctx);
            settings::resync(ctx).await;
        }
        Err(error) => ctx.toast_error(&error),
    }
}

pub async fn remove_account(ctx: &Ctx, account: SavedAccount) {
    let body = tr("将从这台设备移除该账号的登录信息，云盘内容不受影响。");
    if !ctx.ask(&format!("{}：{}", tr("移除账号"), account.display_name), &body, &tr("移除账号"), true).await {
        return;
    }
    let id = account.id.clone();
    match ctx.call(move |service| async move { service.account_remove(&id, &request()).await }).await {
        Ok(()) => {
            if account.active {
                settings::clear_account_data(ctx);
            }
            settings::resync(ctx).await;
        }
        Err(error) => ctx.toast_error(&error),
    }
}

impl App {
    /// Re-establishes the connection after a network change.
    pub async fn reconnect(self: &std::rc::Rc<Self>) {
        let _ = self.call(|service| async move { service.network_reconnect() }).await;
        settings::resync(self).await;
    }
}
