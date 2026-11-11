//! Library and tool pages rendered with the generic panel: shares, local
//! folders, reference archives, duplicates, space usage and archive browsing.
use crate::{
    actions,
    app::{error_text, error_title, new_key, request, Ctx},
    cloud, format,
    i18n::{tr, tx},
    names,
    nav::Route,
    overlay::{Field, FormEvent, MenuItem, PromptOptions},
    panel::{Event, Row, View},
};
use nekopak_domain::*;
use std::time::Duration;

pub fn enter(ctx: &Ctx, route: &Route) {
    match route {
        Route::Shares => shares(ctx),
        Route::Local => local(ctx),
        Route::References => references(ctx),
        Route::Duplicates { folder } => duplicates(ctx, folder.clone()),
        Route::Space { folder } => space(ctx, folder.clone()),
        Route::Archive { file, path } => archive(ctx, file.clone(), path.clone(), None),
        Route::Sync => crate::sync_mount::sync(ctx),
        Route::Mount => crate::sync_mount::mount(ctx),
        _ => {}
    }
}

pub(crate) fn show_error(ctx: &Ctx, error: &ClientError) {
    ctx.panel_state(
        true,
        "alert-circle",
        &error_title(error),
        &error_text(error),
        &tr("重试"),
    );
}

pub(crate) async fn pause(milliseconds: u64) {
    let (done, wait) = tokio::sync::oneshot::channel::<()>();
    slint::Timer::single_shot(Duration::from_millis(milliseconds), move || {
        let _ = done.send(());
    });
    let _ = wait.await;
}

fn reenter(ctx: &Ctx) {
    crate::pages::enter(ctx, &ctx.route());
}

/* -------------------------------- shares -------------------------------- */

fn shares(ctx: &Ctx) {
    let token = ctx.panel_begin(|ctx, event| {
        let ctx = ctx.clone();
        ctx.clone()
            .spawn(async move { shares_event(&ctx, event).await });
    });
    ctx.panel_loading(&tr("我的分享"), "share");
    if let Err(reason) = ctx.gate(Operation::Share) {
        return ctx.panel_state(false, "share", &reason, "", "");
    }
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let result = load_shares(&ctx).await;
        if !ctx.panel_owned(token) {
            return;
        }
        match result {
            Ok(list) => {
                let rows: Vec<Row> = list
                    .iter()
                    .map(|share| {
                        let mut detail = format!(
                            "{}{}",
                            tx("{0} 个项目", &[&share.item_count.to_string()]),
                            match &share.expires_at {
                                Some(expires) => format!(
                                    " · {} {}",
                                    tr("有效期"),
                                    format::date_time(Some(expires))
                                ),
                                None if share.expires_known => format!(" · {}", tr("永久有效")),
                                None => String::new(),
                            }
                        );
                        if let Some(views) = share.view_count {
                            detail.push_str(&tx(" · 浏览 {0} 次", &[&views.to_string()]));
                        }
                        if let Some(saves) = share.save_count {
                            detail.push_str(&tx(" · 转存 {0} 次", &[&saves.to_string()]));
                        }
                        Row::item(&share.share_id, share.title.clone())
                            .icon("share")
                            .detail(detail)
                            .value(format::relative(Some(&share.created_at)))
                            .action(tr("复制链接"))
                            .menu()
                    })
                    .collect();
                let view = View {
                    title: tr("我的分享"),
                    subtitle: tx("{0} 个分享", &[&list.len().to_string()]),
                    icon: "share",
                    actions: vec![
                        MenuItem::new("open-share", tr("打开分享链接")).icon("link"),
                        MenuItem::new("refresh", tr("刷新")).icon("refresh"),
                    ],
                    rows,
                    ..View::default()
                };
                let empty = list.is_empty();
                ctx.panel_show(view);
                if empty {
                    ctx.panel_state(
                        false,
                        "share",
                        &tr("还没有分享"),
                        &tr("在文件上使用右键菜单的“分享…”创建链接。"),
                        "",
                    );
                }
            }
            Err(error) => show_error(&ctx, &error),
        }
    });
}

async fn load_shares(ctx: &Ctx) -> Result<Vec<ShareInfo>, ClientError> {
    ctx.call(|service| async move {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..50 {
            let page = service
                .shares_list(cursor.as_deref(), Some(200), &request())
                .await?;
            all.extend(page.items);
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        Ok(all)
    })
    .await
}

async fn shares_event(ctx: &Ctx, event: Event) {
    match event {
        Event::Action(id, ..) if id == "open-share" => open_share(ctx).await,
        Event::Action(..) | Event::StateAction => reenter(ctx),
        Event::Row(id, what, x, y) => {
            let Some(share) = load_shares(ctx)
                .await
                .ok()
                .and_then(|list| list.into_iter().find(|share| share.share_id == id))
            else {
                return;
            };
            let choice = match what.as_str() {
                "action" => Some("copy".to_owned()),
                "click" => Some("details".to_owned()),
                _ => {
                    ctx.menu(
                        x,
                        y,
                        &share.title,
                        vec![
                            MenuItem::new("copy", tr("复制链接")).icon("copy"),
                            MenuItem::new("browser", tr("在浏览器中打开")).icon("external"),
                            MenuItem::new("details", tr("属性")).icon("info"),
                            MenuItem::separator(),
                            MenuItem::new("cancel", tr("取消分享"))
                                .icon("link-off")
                                .danger(),
                        ],
                    )
                    .await
                }
            };
            match choice.as_deref() {
                Some("copy") => {
                    crate::platform::copy_text(ctx, &cloud::share_text(&share));
                    ctx.toast(tr("分享链接已复制"), "success");
                }
                Some("browser") => ctx.open_link(&share.url),
                Some("details") => cloud::show_share(ctx, &share).await,
                Some("cancel")
                    if ctx
                        .ask(
                            &tr("取消分享"),
                            &tr("取消后链接立即失效，已转存的内容不受影响。"),
                            &tr("取消分享"),
                            true,
                        )
                        .await =>
                {
                    let ids = vec![share.share_id.clone()];
                    match ctx
                        .call(move |service| async move {
                            service.shares_cancel(&ids, &new_key(), &request()).await
                        })
                        .await
                    {
                        Ok(_) => ctx.toast(tr("已取消分享"), "success"),
                        Err(error) => ctx.toast_error(&error),
                    }
                    reenter(ctx);
                }
                _ => {}
            }
        }
        _ => {}
    }
}

/// Extracts `(share_id, pass_code)` from a share link or a bare id.
fn parse_share(text: &str) -> Option<(String, String)> {
    let text = text.trim();
    let (head, query) = text.split_once('?').unwrap_or((text, ""));
    let id = head.trim_end_matches('/').rsplit('/').next()?.to_owned();
    let code = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("pwd="))
        .unwrap_or_default()
        .to_owned();
    (!id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    .then_some((id, code))
}

async fn open_share(ctx: &Ctx) {
    let fields = vec![
        Field::text("link", tr("分享链接"), "")
            .placeholder("https://mypikpak.com/s/…")
            .focus(),
        Field::text("code", tr("提取码"), "").placeholder(tr("没有可留空")),
    ];
    let mut form = ctx.form(&tr("打开分享链接"), "", &tr("打开"), "", fields);
    let opened = loop {
        match form.next().await {
            FormEvent::Submit => {
                let Some((id, embedded)) = parse_share(&form.value("link")) else {
                    form.set("link", |field| field.error = tr("不是有效的分享链接"));
                    continue;
                };
                let typed = form.value("code").trim().to_owned();
                let code = if typed.is_empty() { embedded } else { typed };
                form.set_busy(true);
                let result =
                    ctx.call(move |service| async move {
                        service.share_open(&id, &code, &request()).await
                    })
                    .await;
                form.set_busy(false);
                match result {
                    Ok(shared) => break Some(shared),
                    Err(error) => form.set_error(error_text(&error)),
                }
            }
            FormEvent::Cancel => break None,
            _ => {}
        }
    };
    form.close();
    if let Some(shared) = opened {
        ctx.navigate(Route::SharedFolder {
            access: shared.access_ref,
            parent: shared.parent_id,
            title: shared.title,
        });
    }
}

/// Saves entries of an opened share into the user's own drive.
pub async fn save_shared(ctx: &Ctx, entries: Vec<RemoteEntry>) {
    let Route::SharedFolder { access, .. } = ctx.route() else {
        return;
    };
    let Some((target, _)) = ctx
        .pick_folder(&tr("转存到我的云盘"), &tr("转存到这里"), Vec::new(), &[])
        .await
    else {
        return;
    };
    let ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
    match ctx
        .call(move |service| async move {
            service
                .share_restore(&access, &ids, &target, &new_key(), &request())
                .await
        })
        .await
    {
        Ok(result) if result.failed.is_empty() => ctx.toast(
            tx("已转存 {0} 项", &[&result.succeeded.len().to_string()]),
            "success",
        ),
        Ok(result) => ctx.toast(
            tx(
                "{0} 项成功，{1} 项失败",
                &[
                    &result.succeeded.len().to_string(),
                    &result.failed.len().to_string(),
                ],
            ),
            "warning",
        ),
        Err(error) => ctx.toast_error(&error),
    }
}

/* ----------------------------- local folders ---------------------------- */

fn local(ctx: &Ctx) {
    ctx.panel_begin(|ctx, event| {
        let ctx = ctx.clone();
        ctx.clone()
            .spawn(async move { local_event(&ctx, event).await });
    });
    match ctx.svc.local_sources_list() {
        Ok(sources) => {
            let mut rows = vec![Row::note(
                tr("只读浏览你选择的本地文件夹。不会上传、同步或修改原文件。"),
                "info",
            )];
            rows.extend(sources.iter().map(|source| {
                Row::item(
                    &format!("{}|{}", source.source_id, source.root_id),
                    source.name.clone(),
                )
                .file_kind(names::KIND_FOLDER)
                .detail(source.path.clone())
                .action2(tr("移除"))
            }));
            ctx.panel_show(View {
                title: tr("本地文件夹"),
                icon: "folder",
                actions: vec![MenuItem::new("add", tr("添加本地文件夹"))
                    .icon("folder-plus")
                    .checked(true)],
                rows,
                ..View::default()
            });
        }
        Err(error) => {
            ctx.panel_loading(&tr("本地文件夹"), "folder");
            show_error(ctx, &error);
        }
    }
}

async fn local_event(ctx: &Ctx, event: Event) {
    match event {
        Event::Action(id, ..) if id == "add" => {
            let Some(path) = crate::platform::pick_folder(ctx, &tr("添加本地文件夹"), "").await
            else {
                return;
            };
            match ctx
                .call(move |service| async move { service.local_sources_add(&path) })
                .await
            {
                Ok(_) => reenter(ctx),
                Err(error) => ctx.toast_error(&error),
            }
        }
        Event::Row(id, what, ..) => {
            let Some((source, root)) = id.split_once('|') else {
                return;
            };
            if what == "action2" {
                let source = source.to_owned();
                if let Err(error) = ctx
                    .call(move |service| async move { service.local_sources_remove(&source) })
                    .await
                {
                    ctx.toast_error(&error);
                }
                reenter(ctx);
            } else {
                ctx.navigate(Route::Files {
                    folder: root.to_owned(),
                });
            }
        }
        Event::StateAction => reenter(ctx),
        _ => {}
    }
}

/* --------------------------- reference archives ------------------------- */

fn reference_state(state: &str) -> (String, &'static str) {
    match state {
        "active" | "recorded" => (tr("已记录"), "primary"),
        "stored" => (tr("清单已保存到云盘"), "ok"),
        "archived" => (tr("已归档（原文件在回收站）"), "warn"),
        "restored" => (tr("已恢复"), "ok"),
        "partial" => (tr("部分完成"), "warn"),
        other => (other.to_owned(), "primary"),
    }
}

fn references(ctx: &Ctx) {
    let token = ctx.panel_begin(|ctx, event| {
        let ctx = ctx.clone();
        ctx.clone()
            .spawn(async move { references_event(&ctx, event).await });
    });
    ctx.panel_loading(&tr("引用归档"), "archive");
    if !ctx.feature("reference_archive") {
        return ctx.panel_state(false, "archive", &tr("当前版本尚未启用引用归档"), "", "");
    }
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let result = ctx.call(|service| async move { service.reference_list(&request()).await }).await;
        if !ctx.panel_owned(token) {
            return;
        }
        match result {
            Ok(list) if list.is_empty() => {
                ctx.panel_show(View { title: tr("引用归档"), icon: "archive", ..View::default() });
                ctx.panel_state(
                    false,
                    "archive",
                    &tr("还没有引用归档清单"),
                    &tr("在文件上使用右键菜单的“创建引用归档清单”，记录文件的内容指纹，之后可凭清单恢复。"),
                    "",
                );
            }
            Ok(list) => {
                let mut rows = vec![Row::note(tr("清单记录文件的内容指纹。归档会把原文件移入回收站；恢复时按指纹重新取回，结果取决于服务端是否仍保留内容。"), "info")];
                for manifest in &list {
                    let (label, tone) = reference_state(&manifest.state);
                    let bytes: u64 = manifest.entries.iter().map(|item| item.entry.size_bytes.0).sum();
                    rows.push(
                        Row::item(&manifest.manifest_id, manifest.roots.first().cloned().unwrap_or_else(|| manifest.manifest_id.clone()))
                            .icon("archive")
                            .tone(tone)
                            .detail(format!(
                                "{} · {} · {}",
                                tx("{0} 个文件", &[&manifest.entries.len().to_string()]),
                                format::bytes(bytes),
                                format::date_time(Some(&manifest.created_at))
                            ))
                            .value(label)
                            .menu(),
                    );
                }
                ctx.panel_show(View { title: tr("引用归档"), icon: "archive", rows, ..View::default() });
            }
            Err(error) => show_error(&ctx, &error),
        }
    });
}

async fn references_event(ctx: &Ctx, event: Event) {
    let Event::Row(id, _, x, y) = event else {
        if matches!(event, Event::StateAction) {
            reenter(ctx);
        }
        return;
    };
    let choice = ctx
        .menu(
            x,
            y,
            "",
            vec![
                MenuItem::new("status", tr("检查状态")).icon("refresh"),
                MenuItem::new("details", tr("查看清单内容")).icon("list"),
                MenuItem::new("store", tr("把清单保存到云盘")).icon("upload"),
                MenuItem::separator(),
                MenuItem::new("archive", tr("归档（原文件移入回收站）"))
                    .icon("trash")
                    .danger(),
                MenuItem::new("restore", tr("按清单恢复")).icon("restore"),
            ],
        )
        .await;
    let manifest = id.clone();
    let result: Result<Option<ReferenceManifest>, ClientError> = match choice.as_deref() {
        Some("status") | Some("details") => {
            ctx.call(move |service| async move {
                service
                    .reference_status(&manifest, &request())
                    .await
                    .map(Some)
            })
            .await
        }
        Some("store") => {
            ctx.call(move |service| async move {
                service
                    .reference_store_manifest(&manifest, &new_key(), &request())
                    .await
                    .map(|_| None)
            })
            .await
        }
        Some("archive") => {
            let body = tr(
                "清单中的原文件会移入云盘回收站。回收站清空后只能按内容指纹恢复，且不保证成功。",
            );
            if !ctx
                .ask(&tr("归档这些文件？"), &body, &tr("归档"), true)
                .await
            {
                return;
            }
            ctx.call(move |service| async move {
                service
                    .reference_archive(&manifest, true, &new_key(), &request())
                    .await
                    .map(Some)
            })
            .await
        }
        Some("restore") => {
            let Some(recreate) = ctx
                .confirm(crate::overlay::ConfirmOptions {
                    title: tr("按清单恢复？"),
                    body: tr("优先从回收站还原；找不到时可按内容指纹重新创建。"),
                    confirm: tr("恢复"),
                    danger: false,
                    option: Some((tr("回收站中找不到时，按指纹重新创建"), false)),
                })
                .await
            else {
                return;
            };
            ctx.call(move |service| async move {
                service
                    .reference_restore(&manifest, recreate, &new_key(), &request())
                    .await
                    .map(Some)
            })
            .await
        }
        _ => return,
    };
    match result {
        Ok(Some(manifest)) if choice.as_deref() == Some("details") => {
            let rows = manifest
                .entries
                .iter()
                .map(|item| {
                    (
                        reference_state(&item.state).0,
                        format!(
                            "{}（{}）",
                            item.path,
                            format::bytes(item.entry.size_bytes.0)
                        ),
                        false,
                    )
                })
                .collect();
            ctx.details(&tr("清单内容"), rows, None).await;
        }
        Ok(Some(manifest)) => {
            let failed = manifest
                .entries
                .iter()
                .filter(|item| item.error.is_some())
                .count();
            if failed > 0 {
                ctx.toast(tx("{0} 项未完成", &[&failed.to_string()]), "warning");
            } else {
                ctx.toast(reference_state(&manifest.state).0, "success");
            }
            ctx.tree_changed();
        }
        Ok(None) => {
            ctx.toast(tr("已开始上传清单"), "success");
            crate::transfers::request_reload(ctx);
        }
        Err(error) => ctx.toast_error(&error),
    }
    reenter(ctx);
}

pub async fn create_reference(ctx: &Ctx, entries: Vec<RemoteEntry>) {
    let ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
    match ctx.call(move |service| async move { service.reference_create(&ids, &new_key(), &request()).await }).await {
        Ok(manifest) => ctx.toast_action(tx("已创建清单，记录 {0} 个文件", &[&manifest.entries.len().to_string()]), "success", tr("查看"), |ctx| {
            ctx.navigate(Route::References)
        }),
        Err(error) => ctx.toast_error(&error),
    }
}

/* ------------------------------- duplicates ----------------------------- */

fn duplicates(ctx: &Ctx, folder: String) {
    let token = ctx.panel_begin({
        let folder = folder.clone();
        move |ctx, event| {
            let ctx = ctx.clone();
            let folder = folder.clone();
            ctx.clone()
                .spawn(async move { duplicates_event(&ctx, folder, event).await });
        }
    });
    ctx.panel_loading(&tr("查找重复"), "duplicates");
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let scan = ctx
            .call(move |service| async move { service.duplicates_scan(&folder, &request()).await })
            .await;
        let scan = match scan {
            Ok(scan) => scan.scan_id,
            Err(error) => {
                if ctx.panel_owned(token) {
                    show_error(&ctx, &error);
                }
                return;
            }
        };
        *ctx.scan.borrow_mut() = Some(scan.clone());
        loop {
            let id = scan.clone();
            let report = ctx
                .call(
                    move |service| async move { service.duplicates_report(&id, &request()).await },
                )
                .await;
            if !ctx.panel_owned(token) {
                // Leaving the page stops the walk instead of letting it run on.
                let id = scan.clone();
                let _ =
                    ctx.call(move |service| async move {
                        service.duplicates_cancel(&id, &request()).await
                    })
                    .await;
                return;
            }
            match report {
                Ok(report) => {
                    let done = report.state != "running" && report.state != "queued";
                    show_duplicates(&ctx, &report, done);
                    if done {
                        return;
                    }
                }
                Err(error) => return show_error(&ctx, &error),
            }
            pause(700).await;
        }
    });
}

fn show_duplicates(ctx: &Ctx, report: &DuplicateReport, done: bool) {
    let mut rows = Vec::new();
    if !done {
        rows.push(
            Row::progress("", tr("正在扫描"), -1.0)
                .detail(tx(
                    "已扫描 {0} 个文件、{1} 个文件夹",
                    &[
                        &report.files_scanned.to_string(),
                        &report.folders_scanned.to_string(),
                    ],
                ))
                .action(tr("取消")),
        );
    }
    if let Some(error) = &report.error {
        rows.push(Row::note(error_text(error), "error"));
    }
    if report.unhashed_files > 0 {
        rows.push(Row::note(
            tx(
                "{0} 个文件没有内容指纹，未参与比较。",
                &[&report.unhashed_files.to_string()],
            ),
            "warn",
        ));
    }
    for group in &report.groups {
        rows.push(Row::header(format!(
            "{} · {} × {}",
            names::split_name(
                &group
                    .entries
                    .first()
                    .map(|item| item.entry.name.clone())
                    .unwrap_or_default()
            )
            .0,
            group.entries.len(),
            format::bytes(group.size_bytes.0)
        )));
        for (index, item) in group.entries.iter().enumerate() {
            let kind = names::file_kind(
                &item.entry.name,
                item.entry.kind,
                item.entry.mime_type.as_deref(),
            );
            let mut row = Row::item(&item.entry.id, item.entry.name.clone())
                .file_kind(kind)
                .detail(item.path.clone())
                .value(format::date_time(item.entry.modified_at.as_deref()));
            if index > 0 {
                row = row.action(tr("移至回收站")).danger();
            } else {
                row = row.value(tr("保留"));
            }
            rows.push(row);
        }
    }
    let extra = report
        .groups
        .iter()
        .map(|group| group.entries.len().saturating_sub(1))
        .sum::<usize>();
    ctx.panel_show(View {
        title: tr("查找重复"),
        subtitle: if done {
            tx(
                "{0} 组重复，可释放 {1}",
                &[
                    &report.groups.len().to_string(),
                    &format::bytes(report.reclaimable_bytes.0),
                ],
            )
        } else {
            report.folder_path.clone()
        },
        icon: "duplicates",
        back: true,
        actions: if done && extra > 0 {
            vec![MenuItem::new(
                "trash-all",
                tx("每组保留一份，其余 {0} 项移至回收站", &[&extra.to_string()]),
            )
            .icon("trash-sweep")
            .danger()]
        } else {
            Vec::new()
        },
        rows,
        ..View::default()
    });
    if done && report.groups.is_empty() && report.error.is_none() {
        ctx.panel_state(
            false,
            "check-circle",
            &tr("没有发现重复文件"),
            &tx("已扫描 {0} 个文件", &[&report.files_scanned.to_string()]),
            "",
        );
    }
}

async fn duplicates_event(ctx: &Ctx, folder: String, event: Event) {
    let Some(scan) = ctx.scan.borrow().clone() else {
        return;
    };
    let report = {
        let id = scan.clone();
        ctx.call(move |service| async move { service.duplicates_report(&id, &request()).await })
            .await
    };
    let ids: Vec<String> = match (&event, report) {
        (Event::Row(_, what, ..), Ok(report)) if what == "action" && report.state == "running" => {
            let _ =
                ctx.call(move |service| async move {
                    service.duplicates_cancel(&scan, &request()).await
                })
                .await;
            return;
        }
        (Event::Row(id, what, ..), _) if what == "action" => vec![id.clone()],
        (Event::Action(id, ..), Ok(report)) if id == "trash-all" => report
            .groups
            .iter()
            .flat_map(|group| {
                group
                    .entries
                    .iter()
                    .skip(1)
                    .map(|item| item.entry.id.clone())
            })
            .collect(),
        (Event::StateAction, _) => return duplicates(ctx, folder),
        _ => return,
    };
    if ids.is_empty() {
        return;
    }
    let title = tx("将 {0} 项移至回收站？", &[&ids.len().to_string()]);
    if !ctx
        .ask(
            &title,
            &tr("每组会保留列在最前面的一份。移入回收站的文件可以还原。"),
            &tr("移至回收站"),
            true,
        )
        .await
    {
        return;
    }
    // Large selections are sent in bounded batches.
    for chunk in ids.chunks(200) {
        if actions::mutate(ctx, MutateOp::Trash, chunk.to_vec(), None, Vec::new())
            .await
            .is_none()
        {
            break;
        }
    }
    duplicates(ctx, folder);
}

/* ------------------------------ space usage ----------------------------- */

fn space(ctx: &Ctx, folder: String) {
    let token = ctx.panel_begin({
        let folder = folder.clone();
        move |ctx, event| match event {
            Event::Row(id, ..) if !id.is_empty() => ctx.navigate(Route::Space { folder: id }),
            Event::StateAction => space(ctx, folder.clone()),
            Event::Action(id, ..) if id == "open" => ctx.navigate(Route::Files {
                folder: folder.clone(),
            }),
            _ => {}
        }
    });
    ctx.panel_loading(&tr("空间分布"), "space");
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let result = ctx
            .call(move |service| async move {
                service
                    .files_space_summary(
                        &folder,
                        &nekopak_provider::RequestContext::new(Duration::from_secs(180)),
                    )
                    .await
            })
            .await;
        if !ctx.panel_owned(token) {
            return;
        }
        match result {
            Ok(summary) => {
                let mut children = summary.children.clone();
                children.sort_by_key(|node| std::cmp::Reverse(node.size_bytes));
                let total = summary.total_bytes.0.max(1);
                let mut rows = Vec::new();
                if let Some(note) = &summary.note {
                    rows.push(Row::note(tr(note), "info"));
                }
                rows.extend(children.iter().map(|node| {
                    let folder = node.kind == EntryKind::Folder;
                    let share = node.size_bytes.0 as f64 / total as f64;
                    let mut row = Row::progress(
                        if folder { &node.id } else { "" },
                        node.name.clone(),
                        share as f32,
                    )
                    .file_kind(names::file_kind(
                        &node.name,
                        node.kind,
                        node.mime_type.as_deref(),
                    ))
                    .detail(if folder {
                        tx("{0} 个文件", &[&node.file_count.to_string()])
                    } else {
                        String::new()
                    })
                    .value(format!(
                        "{} · {:.1}%",
                        format::bytes(node.size_bytes.0),
                        share * 100.0
                    ));
                    row.kind = if folder { "item" } else { "progress" };
                    row
                }));
                let empty = children.is_empty();
                ctx.panel_show(View {
                    title: tr("空间分布"),
                    subtitle: format!(
                        "{} · {} · {}",
                        format::bytes(summary.total_bytes.0),
                        tx("{0} 个文件", &[&summary.file_count.to_string()]),
                        tx("{0} 个文件夹", &[&summary.folder_count.to_string()])
                    ),
                    icon: "space",
                    back: true,
                    actions: vec![MenuItem::new("open", tr("在网盘中查看")).icon("folder-open")],
                    rows,
                    ..View::default()
                });
                if empty {
                    ctx.panel_state(false, "folder-open", &tr("这个文件夹是空的"), "", "");
                }
            }
            Err(error) => show_error(&ctx, &error),
        }
    });
}

/* ---------------------------- archive browsing -------------------------- */

fn archive(ctx: &Ctx, file: String, path: String, password: Option<String>) {
    let token = ctx.panel_begin({
        let (file, path, password) = (file.clone(), path.clone(), password.clone());
        move |ctx, event| {
            let ctx = ctx.clone();
            let (file, path, password) = (file.clone(), path.clone(), password.clone());
            ctx.clone()
                .spawn(async move { archive_event(&ctx, file, path, password, event).await });
        }
    });
    ctx.panel_loading(&tr("压缩包"), "archive");
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let listing = ctx
            .call({
                let (file, path, password) = (file.clone(), path.clone(), password.clone());
                move |service| async move {
                    service
                        .archive_list(&file, &path, password.as_deref(), &request())
                        .await
                }
            })
            .await;
        if !ctx.panel_owned(token) {
            return;
        }
        let listing = match listing {
            Ok(listing) => listing,
            Err(error) => return show_error(&ctx, &error),
        };
        if listing.needs_password && password.is_none() {
            let entered = ctx
                .prompt(PromptOptions {
                    title: tr("压缩包已加密"),
                    label: tr("密码"),
                    initial: String::new(),
                    placeholder: String::new(),
                    confirm: tr("解锁"),
                    password: true,
                    select_to: 0,
                    validate: Box::new(|text| {
                        (
                            if text.is_empty() {
                                Some(tr("请输入密码"))
                            } else {
                                None
                            },
                            String::new(),
                        )
                    }),
                })
                .await;
            let Some(entered) = entered else {
                return ctx.navigate_history(-1);
            };
            let id = file.clone();
            let unlocked = ctx
                .call(move |service| async move {
                    service.archive_unlock(&id, &entered, &request()).await
                })
                .await;
            return match unlocked {
                Ok(value) => archive(
                    &ctx,
                    file,
                    path,
                    value["password_ref"].as_str().map(str::to_owned),
                ),
                Err(error) => show_error(&ctx, &error),
            };
        }
        if !listing.supported {
            ctx.panel_show(View {
                title: tr("压缩包"),
                icon: "archive",
                back: true,
                ..View::default()
            });
            return ctx.panel_state(
                false,
                "archive",
                &tr("暂不支持浏览这个压缩包"),
                &listing.reason.map(|r| tr(&r)).unwrap_or_default(),
                "",
            );
        }
        let mut entries = listing.entries.clone();
        entries.sort_by(|a, b| {
            (a.kind != EntryKind::Folder)
                .cmp(&(b.kind != EntryKind::Folder))
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        let rows = entries
            .iter()
            .map(|entry| {
                let folder = entry.kind == EntryKind::Folder;
                Row::item(
                    &format!("{}|{}", if folder { "d" } else { "f" }, entry.path),
                    entry.name.clone(),
                )
                .file_kind(names::file_kind(&entry.name, entry.kind, None))
                .value(if folder {
                    String::new()
                } else {
                    format::bytes(entry.size_bytes.0)
                })
                .action2(tr("解压到…"))
            })
            .collect();
        ctx.panel_show(View {
            title: tr("压缩包"),
            subtitle: if path.is_empty() {
                listing.format.to_uppercase()
            } else {
                format!("/{path}")
            },
            icon: "archive",
            back: true,
            actions: vec![MenuItem::new("extract-all", tr("全部解压到…"))
                .icon("unarchive")
                .checked(true)],
            rows,
            ..View::default()
        });
        if entries.is_empty() {
            ctx.panel_state(false, "archive", &tr("这个文件夹是空的"), "", "");
        }
    });
}

async fn archive_event(
    ctx: &Ctx,
    file: String,
    path: String,
    password: Option<String>,
    event: Event,
) {
    let paths = match event {
        Event::Action(id, ..) if id == "extract-all" => serde_json::json!("all"),
        Event::Row(id, what, ..) => {
            let Some((kind, entry)) = id.split_once('|') else {
                return;
            };
            if what == "action2" {
                serde_json::json!([entry])
            } else if kind == "d" {
                return ctx.navigate(Route::Archive {
                    file,
                    path: entry.to_owned(),
                });
            } else {
                return;
            }
        }
        Event::StateAction => return archive(ctx, file, path, password),
        _ => return,
    };
    let Some((target, _)) = ctx
        .pick_folder(&tr("解压到…"), &tr("解压到这里"), Vec::new(), &[])
        .await
    else {
        return;
    };
    let result = ctx
        .call(move |service| async move {
            service
                .archive_extract(
                    &file,
                    &paths,
                    &target,
                    password.as_deref(),
                    &new_key(),
                    &request(),
                )
                .await
        })
        .await;
    match result {
        Ok(_) => {
            ctx.toast(tr("已提交解压任务，完成后会出现在目标文件夹"), "success");
            crate::transfers::request_reload(ctx);
        }
        Err(error) => ctx.toast_error(&error),
    }
}

/* ------------------------------ batch rename ---------------------------- */

/// Computes new names; entries whose name would not change are omitted.
fn rename_plan(
    entries: &[RemoteEntry],
    find: &str,
    replace: &str,
    prefix: &str,
    suffix: &str,
    number: bool,
) -> Vec<(RemoteEntry, String)> {
    let width = entries.len().to_string().len().max(2);
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let (stem, extension) = if entry.kind == EntryKind::Folder {
                (entry.name.as_str(), "")
            } else {
                names::split_name(&entry.name)
            };
            let mut stem = if find.is_empty() {
                stem.to_owned()
            } else {
                stem.replace(find, replace)
            };
            if number {
                stem = format!("{stem} {:0width$}", index + 1);
            }
            let name = format!("{prefix}{stem}{suffix}{extension}");
            (name != entry.name).then(|| (entry.clone(), name))
        })
        .collect()
}

pub async fn batch_rename(ctx: &Ctx, entries: Vec<RemoteEntry>) {
    if let Err(reason) = ctx.gate(Operation::Rename) {
        return ctx.toast(reason, "info");
    }
    if entries.len() < 2 {
        return ctx.toast(tr("至少需要两个项目"), "info");
    }
    let siblings = ctx.files.borrow().sibling_names();
    let fields = vec![
        Field::text("find", tr("查找"), "").focus(),
        Field::text("replace", tr("替换为"), ""),
        Field::text("prefix", tr("添加前缀"), ""),
        Field::text("suffix", tr("添加后缀"), "").detail(tr("后缀加在扩展名之前。")),
        Field::toggle("number", tr("追加序号"), false),
    ];
    let mut form = ctx.form(
        &tr("批量重命名"),
        &tx("{0} 个项目", &[&entries.len().to_string()]),
        &tr("重命名"),
        "",
        fields,
    );
    form.set_confirm(&tr("重命名"), false);
    let plan = loop {
        let event = form.next().await;
        let plan = rename_plan(
            &entries,
            &form.value("find"),
            &form.value("replace"),
            &form.value("prefix"),
            &form.value("suffix"),
            form.checked("number"),
        );
        // Validate against the folder as it will look after all renames.
        let mut taken: Vec<String> = siblings
            .iter()
            .filter(|name| !plan.iter().any(|(entry, _)| entry.name == **name))
            .cloned()
            .collect();
        let mut problem = None;
        for (entry, name) in &plan {
            if let Some(error) = names::validate_name(name, &taken, None).error {
                problem = Some(format!("{} → {}：{}", entry.name, name, error));
                break;
            }
            taken.push(name.clone());
        }
        let example = plan
            .first()
            .map(|(entry, name)| format!("{} → {}", entry.name, name))
            .unwrap_or_default();
        form.set_confirm(&tr("重命名"), problem.is_none() && !plan.is_empty());
        // Problems use the error slot; the neutral preview goes in the body.
        form.set_error(problem.clone().unwrap_or_default());
        form.set_text(
            &tr("批量重命名"),
            &if plan.is_empty() {
                tx("{0} 个项目", &[&entries.len().to_string()])
            } else {
                format!(
                    "{} · {}",
                    tx("将重命名 {0} 项", &[&plan.len().to_string()]),
                    example
                )
            },
        );
        match event {
            FormEvent::Submit if problem.is_none() && !plan.is_empty() => break Some(plan),
            FormEvent::Cancel => break None,
            _ => {}
        }
    };
    form.close();
    let Some(plan) = plan else { return };
    let session = ctx.session();
    let total = plan.len();
    let mut failed = 0usize;
    let mut first = String::new();
    for (entry, name) in plan {
        if session != ctx.session() {
            return;
        }
        let mutation = FileMutation {
            op: MutateOp::Rename,
            ids: vec![entry.id],
            target: Some(MutateTarget {
                parent_id: None,
                name: Some(name),
            }),
        };
        match ctx
            .call(move |service| async move {
                service
                    .files_mutate(&mutation, &new_key(), &request())
                    .await
            })
            .await
        {
            Ok(result) if result.failed.is_empty() => {}
            Ok(result) => {
                failed += 1;
                if first.is_empty() {
                    first = error_text(&result.failed[0].error);
                }
            }
            Err(error) => {
                failed += 1;
                if first.is_empty() {
                    first = error_text(&error);
                }
            }
        }
    }
    if failed > 0 {
        ctx.toast(
            tx(
                "{0}{1} 项失败：{2}",
                &[
                    &format!("{} {} 项，", tr("已重命名"), total - failed),
                    &failed.to_string(),
                    &first,
                ],
            ),
            "error",
        );
    } else {
        ctx.toast(
            format!("{}{}", tr("已重命名"), tx(" {0} 项", &[&total.to_string()])),
            "success",
        );
    }
    ctx.tree_changed();
}

pub fn on_scan_progress(_ctx: &Ctx, _event: serde_json::Value) {
    // The duplicates page polls the authoritative report; progress events only
    // exist so other hosts can avoid polling.
}

pub fn on_sync_changed(ctx: &Ctx, event: serde_json::Value, review: bool) {
    crate::sync_mount::on_sync_event(ctx, event, review);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: EntryKind) -> RemoteEntry {
        RemoteEntry {
            id: name.into(),
            parent_id: None,
            name: name.into(),
            kind,
            size_bytes: DecimalU64(0),
            modified_at: None,
            etag: None,
            hash: None,
            mime_type: None,
            starred: None,
        }
    }

    #[test]
    fn batch_rename_keeps_extensions_and_skips_unchanged_names() {
        let entries = vec![
            entry("课程 a.mp4", EntryKind::File),
            entry("课程 b.mkv", EntryKind::File),
            entry("备注.d", EntryKind::Folder),
        ];
        let plan = rename_plan(&entries, "课程", "Lecture", "", "", false);
        assert_eq!(
            plan.iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>(),
            ["Lecture a.mp4", "Lecture b.mkv"]
        );
        let plan = rename_plan(&entries, "", "", "[x] ", "-v2", true);
        assert_eq!(plan[0].1, "[x] 课程 a 01-v2.mp4");
        assert_eq!(plan[2].1, "[x] 备注.d 03-v2");
        assert!(rename_plan(&entries, "zzz", "y", "", "", false).is_empty());
    }

    #[test]
    fn share_links_and_ids_are_parsed() {
        assert_eq!(
            parse_share("https://mypikpak.com/s/VNabc_1-2?pwd=x9k2"),
            Some(("VNabc_1-2".into(), "x9k2".into()))
        );
        assert_eq!(
            parse_share(" VNabc "),
            Some(("VNabc".into(), String::new()))
        );
        assert_eq!(parse_share("not a link"), None);
    }
}
