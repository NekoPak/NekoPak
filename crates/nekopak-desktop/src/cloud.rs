//! Cloud download ("添加链接") with link preview and torrent import, plus sharing.
use crate::{
    app::{error_text, new_key, request, Ctx},
    files::Mode,
    format,
    i18n::{tr, tx},
    nav::{Route, ROOT_ID},
    overlay::{Field, FormEvent, MenuItem},
};
use nekopak_domain::*;

fn is_link(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    !text.chars().any(char::is_whitespace)
        && (lower.starts_with("magnet:?xt=urn:btih:")
            || lower.starts_with("http://")
            || lower.starts_with("https://"))
}

fn links_of(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Where a new cloud task is saved: the listed drive folder, else the root.
fn current_target(ctx: &Ctx) -> (String, String) {
    let state = ctx.files.borrow();
    if state.mode == Mode::Files && matches!(ctx.route(), Route::Files { .. }) {
        (state.folder.clone(), state.crumb_path())
    } else {
        (ROOT_ID.to_owned(), "/".to_owned())
    }
}

pub async fn add_link_dialog(ctx: &Ctx, prefill: Option<String>) {
    if let Err(reason) = ctx.gate(Operation::CloudTask) {
        return ctx.toast(reason, "info");
    }
    let (mut target, mut target_path) = current_target(ctx);
    let torrent_available = ctx.feature("torrent_import");
    let fields = vec![
        Field::multiline("links", tr("链接"), prefill.unwrap_or_default())
            .placeholder(tr("粘贴磁力链接或 http(s) 链接，每行一个"))
            .detail(tr(
                "提交后由 PikPak 服务端下载并保存到云盘，不占用本机带宽。",
            ))
            .focus(),
        Field::select("target", tr("保存到"), target_path.clone()),
    ];
    let alt = if torrent_available {
        tr("种子文件")
    } else {
        String::new()
    };
    let mut form = ctx.form(&tr("添加链接"), "", &tr("添加到云下载"), &alt, fields);
    let session = ctx.session();
    loop {
        match form.next().await {
            FormEvent::Select(_, _, _) => {
                if let Some((id, path)) = ctx
                    .pick_folder(&tr("保存到"), &tr("保存到这里"), Vec::new(), &[])
                    .await
                {
                    target = id;
                    target_path = path.clone();
                    form.set("target", |field| field.value = path);
                }
            }
            FormEvent::Alt => {
                form.close();
                return torrent_dialog(ctx, target, target_path, None).await;
            }
            FormEvent::Submit => {
                let links = links_of(&form.value("links"));
                if links.is_empty() {
                    form.set("links", |field| field.error = tr("请粘贴至少一个链接"));
                    continue;
                }
                if let Some(bad) = links.iter().find(|link| !is_link(link)) {
                    let bad = bad.chars().take(60).collect::<String>();
                    form.set("links", |field| {
                        field.error = tx("不是可识别的磁力或 http(s) 链接：{0}", &[&bad])
                    });
                    continue;
                }
                form.set_busy(true);
                let parent = target.clone();
                let result = ctx
                    .call(move |service| async move {
                        let (mut created, mut first_error) = (0usize, None);
                        for link in &links {
                            match service
                                .cloud_task_create(link, &parent, &new_key(), &request())
                                .await
                            {
                                Ok(_) => created += 1,
                                Err(error) => first_error = first_error.or(Some(error)),
                            }
                        }
                        (created, links.len(), first_error)
                    })
                    .await;
                form.set_busy(false);
                if session != ctx.session() {
                    break;
                }
                let (created, total, error) = result;
                if created > 0 {
                    crate::transfers::request_reload(ctx);
                    ctx.toast_action(
                        tx("已提交 {0} 个云下载任务", &[&created.to_string()]),
                        "success",
                        tr("查看"),
                        |ctx| {
                            ctx.navigate(Route::Transfers {
                                tab: "cloud".into(),
                            })
                        },
                    );
                }
                match error {
                    Some(error) if created == 0 => form.set_error(error_text(&error)),
                    Some(error) => {
                        ctx.toast(
                            tx(
                                "{0} 项成功，{1} 项失败",
                                &[&created.to_string(), &(total - created).to_string()],
                            ),
                            "warning",
                        );
                        ctx.toast_error(&error);
                        break;
                    }
                    None => break,
                }
            }
            FormEvent::Cancel => break,
            _ => {}
        }
    }
    form.close();
}

/// Offers a link found on the clipboard when the window regains focus.
/// Opt-in (off by default); the same link is only offered once.
pub fn clipboard_check(ctx: &Ctx) {
    if !ctx.prefs.borrow().device.clipboard_watch
        || ctx.dialog_open()
        || !ctx.can(Operation::CloudTask)
    {
        return;
    }
    ctx.ui.invoke_clipboard_paste();
    let text = ctx.ui.get_clipboard_text().trim().to_owned();
    ctx.ui.set_clipboard_text(Default::default());
    if text.len() > 4096 || !is_link(&text) {
        return;
    }
    {
        let mut last = ctx.last_clipboard_link.borrow_mut();
        if *last == text {
            return;
        }
        *last = text.clone();
    }
    ctx.toast_action(
        tr("剪贴板中有一个链接"),
        "info",
        tr("添加"),
        move |ctx| crate::actions::add_link(ctx, Some(text)),
    );
}

/// Imports a torrent that was dropped onto the window.
pub async fn import_torrent(ctx: &Ctx, path: std::path::PathBuf) {
    let (target, target_path) = current_target(ctx);
    torrent_dialog(ctx, target, target_path, Some(path)).await;
}

/// Reads a .torrent, shows what it contains and submits only after confirmation.
async fn torrent_dialog(
    ctx: &Ctx,
    target: String,
    target_path: String,
    chosen: Option<std::path::PathBuf>,
) {
    let session = ctx.session();
    let scope = match ctx
        .call(|service| async move { service.account_scope(&request()).await })
        .await
    {
        Ok(scope) => scope,
        Err(error) => return ctx.toast_error(&error),
    };
    let path = match chosen {
        Some(path) => path,
        None => {
            let Some(path) =
                crate::platform::pick_file(ctx, &tr("选择种子文件"), ("BitTorrent", &["torrent"]))
                    .await
            else {
                return;
            };
            path
        }
    };
    if session != ctx.session() {
        return;
    }
    let preview = ctx
        .call(move |service| async move {
            use std::io::Read;
            let file = std::fs::File::open(&path).map_err(|_| {
                ClientError::new(ErrorCode::PermissionDenied, "无法读取所选种子文件。")
            })?;
            let mut bytes = Vec::new();
            file.take(nekopak_core::MAX_TORRENT_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| {
                    ClientError::new(ErrorCode::PermissionDenied, "无法完整读取种子文件。")
                })?;
            service.torrent_import(&bytes, &scope, &request()).await
        })
        .await;
    let preview = match preview {
        Ok(preview) => preview,
        Err(error) => return ctx.toast_error(&error),
    };
    let mut rows = vec![
        (tr("名称"), preview.title.clone(), false),
        (tr("大小"), format::bytes(preview.total_bytes.0), false),
        (tr("文件数"), preview.file_count.to_string(), false),
        (tr("保存到"), target_path, false),
    ];
    if preview.private {
        rows.push((tr("注意"), tr("这是私有种子，服务端可能无法下载。"), false));
    }
    for file in preview.files.iter().take(200) {
        let value = serde_json::to_value(file).unwrap_or_default();
        rows.push((
            format::bytes(
                value["size_bytes"]
                    .as_str()
                    .and_then(|size| size.parse().ok())
                    .unwrap_or(0),
            ),
            value["path"].as_str().unwrap_or_default().to_owned(),
            false,
        ));
    }
    let import = preview.import_id.clone();
    if !ctx
        .details(&tr("种子文件"), rows, Some(tr("添加到云下载")))
        .await
    {
        let _ = ctx
            .call(move |service| async move { service.torrent_release(&import) })
            .await;
        return;
    }
    let result = ctx
        .call(move |service| async move {
            service
                .torrent_create(&import, &target, &new_key(), &request())
                .await
        })
        .await;
    match result {
        Ok(_) => {
            crate::transfers::request_reload(ctx);
            ctx.toast_action(
                tx("已提交 {0} 个云下载任务", &["1"]),
                "success",
                tr("查看"),
                |ctx| {
                    ctx.navigate(Route::Transfers {
                        tab: "cloud".into(),
                    })
                },
            );
        }
        Err(error) => ctx.toast_error(&error),
    }
}

pub async fn share_dialog(ctx: &Ctx, entries: Vec<RemoteEntry>) {
    if let Err(reason) = ctx.gate(Operation::Share) {
        return ctx.toast(reason, "info");
    }
    let title = if entries.len() == 1 {
        entries[0].name.clone()
    } else {
        tx("{0} 个项目", &[&entries.len().to_string()])
    };
    let expiry_label = |days: u32| match days {
        1 => tr("1 天"),
        7 => tr("7 天"),
        30 => tr("30 天"),
        _ => tr("永久有效"),
    };
    let mut days = 7u32;
    let fields = vec![
        Field::select("expiry", tr("有效期"), expiry_label(days)),
        Field::toggle("code", tr("需要提取码"), true)
            .detail(tr("访问者需要输入提取码才能打开分享。")),
    ];
    let mut form = ctx.form(&tr("分享"), &title, &tr("创建分享链接"), "", fields);
    let share = loop {
        match form.next().await {
            FormEvent::Select(_, x, y) => {
                let items = [1u32, 7, 30, 0]
                    .iter()
                    .map(|d| MenuItem::new(&d.to_string(), expiry_label(*d)).checked(*d == days))
                    .collect();
                if let Some(choice) = ctx.menu(x, y, "", items).await {
                    days = choice.parse().unwrap_or(7);
                    form.set("expiry", |field| field.value = expiry_label(days));
                }
            }
            FormEvent::Submit => {
                form.set_busy(true);
                let body = CreateShareRequest {
                    ids: entries.iter().map(|entry| entry.id.clone()).collect(),
                    with_code: form.checked("code"),
                    expires_in_days: days,
                };
                let result = ctx
                    .call(move |service| async move {
                        service.share_create(&body, &new_key(), &request()).await
                    })
                    .await;
                form.set_busy(false);
                match result {
                    Ok(share) => break Some(share),
                    Err(error) => form.set_error(error_text(&error)),
                }
            }
            FormEvent::Cancel => break None,
            _ => {}
        }
    };
    form.close();
    if let Some(share) = share {
        show_share(ctx, &share).await;
    }
}

pub fn share_text(share: &ShareInfo) -> String {
    match &share.code {
        Some(code) if !code.is_empty() => tx("{0}\n提取码：{1}", &[&share.url, code]),
        _ => share.url.clone(),
    }
}

pub async fn show_share(ctx: &Ctx, share: &ShareInfo) {
    let mut rows = vec![(tr("链接"), share.url.clone(), true)];
    if let Some(code) = share.code.as_ref().filter(|code| !code.is_empty()) {
        rows.push((tr("提取码"), code.clone(), true));
    }
    rows.push((
        tr("有效期"),
        match &share.expires_at {
            Some(expires) => format::date_time(Some(expires)),
            None if share.expires_known => tr("永久有效"),
            None => tr("未提供"),
        },
        false,
    ));
    if ctx
        .details(&tr("分享链接已创建"), rows, Some(tr("复制链接")))
        .await
    {
        crate::platform::copy_text(ctx, &share_text(share));
        ctx.toast(tr("分享链接已复制"), "success");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_single_token_magnet_or_http_links_are_accepted() {
        assert!(is_link("magnet:?xt=urn:btih:abcdef"));
        assert!(is_link("https://example.com/a.iso"));
        assert!(!is_link("ftp://example.com/a"));
        assert!(!is_link("https://example.com/a b"));
        assert!(!is_link("magnet:?dn=only-name"));
        assert_eq!(links_of(" a \n\n b\r\n"), vec!["a", "b"]);
    }
}
