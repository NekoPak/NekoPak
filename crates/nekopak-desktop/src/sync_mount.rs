//! Automatic sync (plan → review → apply) and read-only mounts.
use crate::{
    app::{error_text, request, Ctx},
    format,
    i18n::{tr, tx},
    nav::Route,
    overlay::{ConfirmOptions, Field, FormEvent, MenuItem},
    panel::{Event, Row, View},
    tools::{pause, show_error},
};
use nekopak_core::MountConfig;
use nekopak_domain::*;
use std::time::Duration;

fn long_request() -> nekopak_provider::RequestContext {
    nekopak_provider::RequestContext::new(Duration::from_secs(300))
}

fn mode_label(mode: SyncMode) -> String {
    tr(match mode {
        SyncMode::UploadOnly => "本地 → 云盘",
        SyncMode::DownloadOnly => "云盘 → 本地",
        SyncMode::Bidirectional => "双向同步",
    })
}

/* --------------------------------- sync --------------------------------- */

pub fn sync(ctx: &Ctx) {
    let token = ctx.panel_begin(|ctx, event| {
        let ctx = ctx.clone();
        ctx.clone()
            .spawn(async move { sync_event(&ctx, event).await });
    });
    ctx.panel_loading(&tr("自动同步"), "sync");
    if !ctx.feature("sync") {
        return ctx.panel_state(false, "sync", &tr("当前版本尚未支持自动同步"), "", "");
    }
    if !ctx.signed_in() {
        return ctx.panel_state(false, "login", &tr("需要先登录"), "", "");
    }
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let result = ctx.call(|service| async move { service.sync_configs_list(&request()).await }).await;
        if !ctx.panel_owned(token) {
            return;
        }
        match result {
            Ok(configs) => {
                let mut rows = vec![Row::note(
                    tr("每次同步先生成计划并预览。首次同步不删除任何内容；删除默认关闭，开启后仍需逐次确认。"),
                    "info",
                )];
                for config in &configs {
                    let last = match &config.last_applied_at {
                        Some(at) => format!("{} {}", tr("上次同步"), format::relative(Some(at))),
                        None => tr("尚未同步"),
                    };
                    rows.push(
                        Row::item(&config.config_id, config.name.clone())
                            .icon("sync")
                            .tone(if config.auto_enabled { "primary" } else { "" })
                            .detail(format!("{} ⇄ {} · {}", config.local_path, config.remote_path, last))
                            .value(if config.auto_enabled {
                                format!("{} · {}", mode_label(config.mode), tr("自动"))
                            } else {
                                mode_label(config.mode)
                            })
                            .action(tr("预览计划"))
                            .menu(),
                    );
                }
                let empty = configs.is_empty();
                ctx.panel_show(View {
                    title: tr("自动同步"),
                    icon: "sync",
                    actions: vec![MenuItem::new("add", tr("新建同步")).icon("plus").checked(true)],
                    rows,
                    ..View::default()
                });
                if empty {
                    ctx.panel_state(false, "sync", &tr("还没有同步任务"), &tr("选择一个本地文件夹和一个云盘文件夹，按方向保持一致。"), &tr("新建同步"));
                }
            }
            Err(error) => show_error(&ctx, &error),
        }
    });
}

async fn sync_event(ctx: &Ctx, event: Event) {
    match event {
        Event::Action(id, ..) if id == "add" => edit_config(ctx, None).await,
        Event::StateAction => {
            if ctx.signed_in() && ctx.feature("sync") {
                edit_config(ctx, None).await
            }
        }
        Event::Row(id, what, x, y) => {
            let configs = ctx
                .call(|service| async move { service.sync_configs_list(&request()).await })
                .await
                .unwrap_or_default();
            let Some(config) = configs.into_iter().find(|config| config.config_id == id) else {
                return;
            };
            let choice = match what.as_str() {
                "action" => Some("plan".to_owned()),
                "click" => Some("edit".to_owned()),
                _ => {
                    ctx.menu(
                        x,
                        y,
                        &config.name,
                        vec![
                            MenuItem::new("plan", tr("预览计划")).icon("manage-search"),
                            MenuItem::new("edit", tr("编辑")).icon("pencil"),
                            MenuItem::new("history", tr("同步记录")).icon("history"),
                            MenuItem::new("open", tr("打开本地文件夹")).icon("folder-open"),
                        ],
                    )
                    .await
                }
            };
            match choice.as_deref() {
                Some("plan") => plan_and_apply(ctx, config).await,
                Some("edit") => edit_config(ctx, Some(config)).await,
                Some("history") => history(ctx, config).await,
                Some("open")
                    if !crate::platform::open_path(std::path::Path::new(&config.local_path)) =>
                {
                    ctx.toast(tr("无法打开下载文件夹。"), "error");
                }
                _ => {}
            }
        }
        _ => {}
    }
}

async fn edit_config(ctx: &Ctx, existing: Option<SyncConfig>) {
    let creating = existing.is_none();
    let mut config = existing.unwrap_or(SyncConfig {
        config_id: String::new(),
        name: String::new(),
        mode: SyncMode::UploadOnly,
        local_path: String::new(),
        remote_id: "root".into(),
        remote_path: "/".into(),
        exclude_globs: vec![],
        allow_deletes: false,
        first_run: true,
        auto_enabled: false,
        interval_seconds: 300,
        last_applied_at: None,
    });
    let bidirectional = ctx.feature("sync_bidirectional");
    let fields = vec![
        Field::text("name", tr("名称"), config.name.clone()).focus(),
        Field::select(
            "local",
            tr("本地文件夹"),
            if config.local_path.is_empty() {
                tr("选择文件夹")
            } else {
                config.local_path.clone()
            },
        ),
        Field::select("remote", tr("云盘文件夹"), config.remote_path.clone()),
        Field::select("mode", tr("方向"), mode_label(config.mode)),
        Field::text("exclude", tr("排除规则"), config.exclude_globs.join(", "))
            .placeholder("*.tmp, node_modules/**")
            .detail(tr("用逗号分隔的通配符，匹配的路径不参与同步。")),
        Field::toggle("auto", tr("自动同步"), config.auto_enabled)
            .detail(tr("按间隔检查变化；有删除或冲突时等待你确认。")),
        Field::text(
            "interval",
            tr("检查间隔（分钟）"),
            (config.interval_seconds / 60).max(1).to_string(),
        ),
        Field::toggle("deletes", tr("允许删除"), config.allow_deletes)
            .detail(tr("默认关闭。开启后计划中的删除仍需逐次确认。")),
    ];
    let title = if creating {
        tr("新建同步")
    } else {
        tr("编辑同步")
    };
    let mut form = ctx.form(&title, "", &tr("保存"), "", fields);
    let saved = loop {
        match form.next().await {
            FormEvent::Select(id, x, y) => match id.as_str() {
                "local" => {
                    if let Some(path) =
                        crate::platform::pick_folder(ctx, &tr("本地文件夹"), &config.local_path)
                            .await
                    {
                        config.local_path = path.clone();
                        form.set("local", |field| field.value = path);
                    }
                }
                "remote" => {
                    if let Some((id, path)) = ctx
                        .pick_folder(&tr("云盘文件夹"), &tr("选择这个文件夹"), Vec::new(), &[])
                        .await
                    {
                        config.remote_id = id;
                        config.remote_path = path.clone();
                        form.set("remote", |field| field.value = path);
                    }
                }
                _ => {
                    let mut modes = vec![SyncMode::UploadOnly, SyncMode::DownloadOnly];
                    if bidirectional {
                        modes.push(SyncMode::Bidirectional);
                    }
                    let items = modes.iter().enumerate().map(|(index, mode)| {
                        MenuItem::new(&index.to_string(), mode_label(*mode))
                            .checked(*mode == config.mode)
                    });
                    if let Some(index) = ctx
                        .menu(x, y, "", items.collect())
                        .await
                        .and_then(|choice| choice.parse::<usize>().ok())
                    {
                        config.mode = modes[index];
                        form.set("mode", |field| field.value = mode_label(modes[index]));
                    }
                }
            },
            FormEvent::Submit => {
                config.name = form.value("name").trim().to_owned();
                if config.name.is_empty() {
                    form.set("name", |field| field.error = tr("名称不能为空"));
                    continue;
                }
                if config.local_path.is_empty() {
                    form.set_error(tr("请选择本地文件夹"));
                    continue;
                }
                let Some(minutes) = form
                    .value("interval")
                    .trim()
                    .parse::<u64>()
                    .ok()
                    .filter(|minutes| (1..=1440).contains(minutes))
                else {
                    form.set("interval", |field| {
                        field.error = tx("请输入 {0}–{1} 之间的整数", &["1", "1440"])
                    });
                    continue;
                };
                config.interval_seconds = minutes * 60;
                config.exclude_globs = form
                    .value("exclude")
                    .split(',')
                    .map(str::trim)
                    .filter(|glob| !glob.is_empty())
                    .map(str::to_owned)
                    .collect();
                config.auto_enabled = form.checked("auto");
                config.allow_deletes = form.checked("deletes");
                form.set_busy(true);
                let candidate = config.clone();
                let result = ctx
                    .call(move |service| async move {
                        service.sync_config_save(&candidate, &request()).await
                    })
                    .await;
                form.set_busy(false);
                match result {
                    Ok(_) => break true,
                    Err(error) => form.set_error(error_text(&error)),
                }
            }
            FormEvent::Cancel => break false,
            _ => {}
        }
    };
    form.close();
    if saved {
        ctx.toast(tr("同步设置已保存"), "success");
        sync(ctx);
    }
}

fn op_label(op: SyncOp) -> String {
    tr(match op {
        SyncOp::Upload => "上传",
        SyncOp::Download => "下载",
        SyncOp::Mkdir => "新建文件夹",
        SyncOp::KeepBoth => "保留两份",
        SyncOp::Delete => "删除",
        SyncOp::Skip => "跳过",
    })
}

/// Scans both sides, shows the resulting plan and applies it after review.
async fn plan_and_apply(ctx: &Ctx, config: SyncConfig) {
    ctx.toast(tr("正在扫描并生成计划…"), "info");
    let id = config.config_id.clone();
    let plan = ctx
        .call(move |service| async move {
            service.sync_scan(&id, &long_request()).await?;
            service.sync_plan(&id, &long_request()).await
        })
        .await;
    let plan = match plan {
        Ok(plan) => plan,
        Err(error) => return ctx.toast_error(&error),
    };
    let effective: Vec<&SyncAction> = plan
        .actions
        .iter()
        .filter(|action| action.op != SyncOp::Skip)
        .collect();
    if effective.is_empty() && !plan.has_unresolved_conflicts {
        return ctx
            .notice(&config.name, &tr("两边已经一致，没有需要同步的内容。"))
            .await;
    }
    let mut rows: Vec<(String, String, bool)> = plan
        .warnings
        .iter()
        .map(|warning| (tr("注意"), tr(warning), false))
        .collect();
    let deletes = effective
        .iter()
        .filter(|action| action.op == SyncOp::Delete || action.destructive)
        .count();
    rows.push((
        tr("计划"),
        tx("{0} 项操作", &[&effective.len().to_string()]),
        false,
    ));
    if deletes > 0 {
        rows.push((
            tr("删除"),
            tx("{0} 项会被删除", &[&deletes.to_string()]),
            false,
        ));
    }
    for action in effective.iter().take(400) {
        rows.push((
            op_label(action.op),
            format!(
                "{}{}",
                action.path,
                if action.reason.is_empty() {
                    String::new()
                } else {
                    format!("（{}）", tr(&action.reason))
                }
            ),
            false,
        ));
    }
    if effective.len() > 400 {
        rows.push((
            "…".into(),
            tx("还有 {0} 项未显示", &[&(effective.len() - 400).to_string()]),
            false,
        ));
    }
    if plan.has_unresolved_conflicts {
        rows.insert(
            0,
            (
                tr("冲突"),
                tr("存在未解决的冲突，冲突文件会保留两份，不会覆盖。"),
                false,
            ),
        );
    }
    if !ctx
        .details(
            &tx("同步计划：{0}", &[&config.name]),
            rows,
            Some(tr("执行同步")),
        )
        .await
    {
        return;
    }
    let confirm_deletes = if plan.has_deletes {
        let answer = ctx
            .confirm(ConfirmOptions {
                title: tx("同步将删除 {0} 项", &[&deletes.to_string()]),
                body: tr("删除无法通过同步撤销。云盘侧的删除会进入回收站；本地删除直接生效。"),
                confirm: tr("执行同步"),
                danger: true,
                option: Some((tr("我已检查计划，确认执行其中的删除"), false)),
            })
            .await;
        match answer {
            Some(true) => true,
            Some(false) => {
                ctx.toast(tr("未确认删除，本次不会执行同步"), "info");
                return;
            }
            None => return,
        }
    } else {
        false
    };
    let plan_id = plan.plan_id.clone();
    let run = ctx
        .call(move |service| async move {
            service
                .sync_apply(&plan_id, confirm_deletes, &long_request())
                .await
        })
        .await;
    let run = match run {
        Ok(run) => run.task_id,
        Err(error) => return ctx.toast_error(&error),
    };
    ctx.toast(tr("同步已开始"), "info");
    // Follow the run until it settles so the outcome is always reported.
    for _ in 0..1800 {
        pause(1000).await;
        let id = run.clone();
        match ctx
            .call(move |service| async move { service.sync_run_status(&id, &request()).await })
            .await
        {
            Ok(status) => match status["state"].as_str() {
                Some("completed") => {
                    ctx.toast(tx("“{0}”同步完成", &[&config.name]), "success");
                    break;
                }
                Some("failed") | Some("cancelled") => {
                    let message = status["error"]["message"]
                        .as_str()
                        .map(tr)
                        .unwrap_or_else(|| tr("同步未完成"));
                    ctx.toast(format!("{}：{}", config.name, message), "error");
                    break;
                }
                _ => {}
            },
            Err(error) => {
                ctx.toast_error(&error);
                break;
            }
        }
    }
    ctx.tree_changed();
    if ctx.route() == Route::Sync {
        sync(ctx);
    }
}

async fn history(ctx: &Ctx, config: SyncConfig) {
    let id = config.config_id.clone();
    match ctx
        .call(move |service| async move { service.sync_history(&id, None, &request()).await })
        .await
    {
        Ok(page) if page.items.is_empty() => {
            ctx.notice(&config.name, &tr("还没有同步记录。")).await
        }
        Ok(page) => {
            let rows = page
                .items
                .iter()
                .map(|event| {
                    let text = match &event.error {
                        Some(error) => format!("{}：{}", tr(&event.summary), error_text(error)),
                        None => tr(&event.summary),
                    };
                    (format::date_time(Some(&event.at)), text, false)
                })
                .collect();
            ctx.details(&tx("同步记录：{0}", &[&config.name]), rows, None)
                .await;
        }
        Err(error) => ctx.toast_error(&error),
    }
}

pub fn on_sync_event(ctx: &Ctx, event: serde_json::Value, review: bool) {
    if review {
        let name = event["name"].as_str().unwrap_or_default().to_owned();
        ctx.toast_action(
            tx("“{0}”有变化需要确认", &[&name]),
            "info",
            tr("查看"),
            |ctx| ctx.navigate(Route::Sync),
        );
    } else if ctx.route() == Route::Sync && !ctx.dialog_open() {
        sync(ctx);
    }
}

/* --------------------------------- mount -------------------------------- */

fn mount_mode_label(mode: &str) -> String {
    tr(match mode {
        "cloud_files_sync_root" => "同步文件夹（Windows 云文件）",
        "winfsp_drive" => "虚拟盘符（WinFsp）",
        other => other,
    })
}

pub fn mount(ctx: &Ctx) {
    let token = ctx.panel_begin(|ctx, event| {
        let ctx = ctx.clone();
        ctx.clone()
            .spawn(async move { mount_event(&ctx, event).await });
    });
    ctx.panel_loading(&tr("挂载"), "drive");
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let capabilities = ctx
            .call(|service| async move { service.mount_capabilities().await })
            .await;
        let configs = if ctx.signed_in() && ctx.feature("mount") {
            ctx.call(|service| async move { service.mount_configs_list(&request()).await })
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut states = Vec::new();
        for config in &configs {
            let id = config.config_id.clone();
            states.push(
                ctx.call(move |service| async move { service.mount_status(&id, &request()).await })
                    .await
                    .ok(),
            );
        }
        if !ctx.panel_owned(token) {
            return;
        }
        let capabilities = match capabilities {
            Ok(capabilities) => capabilities,
            Err(error) => return show_error(&ctx, &error),
        };
        let mut rows = vec![
            Row::note(
                tr("挂载为只读：可以浏览和按需读取，不会修改云盘内容。需要账号支持随机读取。"),
                "info",
            ),
            Row::header(tr("环境检查")),
        ];
        let mut any_mode = false;
        for mode in capabilities["modes"].as_array().into_iter().flatten() {
            let available = mode["available"] == true;
            any_mode |= available;
            let detail = mode["checks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|check| {
                    check["detail"]
                        .as_str()
                        .filter(|detail| !detail.is_empty())
                        .map(tr)
                })
                .collect::<Vec<_>>()
                .join("；");
            rows.push(
                Row::info(
                    mount_mode_label(mode["mode"].as_str().unwrap_or_default()),
                    if available {
                        tr("可用")
                    } else {
                        tr("不可用")
                    },
                )
                .icon(if available {
                    "check-circle"
                } else {
                    "minus-circle"
                })
                .tone(if available { "ok" } else { "warn" })
                .detail(detail),
            );
        }
        if !configs.is_empty() {
            rows.push(Row::header(tr("挂载点")));
        }
        for (config, state) in configs.iter().zip(&states) {
            let mounted = state
                .as_ref()
                .is_some_and(|state| state["state"] == "mounted");
            let target = config
                .local_path
                .clone()
                .or(config
                    .drive_letter
                    .as_ref()
                    .map(|letter| format!("{letter}:")))
                .unwrap_or_default();
            let mut row = Row::item(&config.config_id, target)
                .icon("drive")
                .tone(if mounted { "ok" } else { "" })
                .detail(mount_mode_label(&config.mode))
                .value(if mounted {
                    tr("已挂载")
                } else {
                    tr("未挂载")
                })
                .action(if mounted { tr("卸载") } else { tr("挂载") });
            if mounted {
                row = row.action2(tr("打开"));
            }
            rows.push(row);
        }
        ctx.panel_show(View {
            title: tr("挂载"),
            icon: "drive",
            actions: vec![MenuItem::new("add", tr("新建挂载"))
                .icon("plus")
                .checked(true)
                .gate(if !ctx.feature("mount") {
                    Err(tr("当前平台尚未支持挂载"))
                } else if !any_mode {
                    Err(tr("没有可用的挂载方式"))
                } else {
                    ctx.gate(Operation::ReadRange)
                })],
            rows,
            ..View::default()
        });
    });
}

async fn mount_event(ctx: &Ctx, event: Event) {
    match event {
        Event::Action(id, ..) if id == "add" => add_mount(ctx).await,
        Event::Row(id, what, ..) if what == "action" || what == "action2" || what == "click" => {
            let configs = ctx
                .call(|service| async move { service.mount_configs_list(&request()).await })
                .await
                .unwrap_or_default();
            let Some(config) = configs.into_iter().find(|config| config.config_id == id) else {
                return;
            };
            let status = {
                let id = id.clone();
                ctx.call(move |service| async move { service.mount_status(&id, &request()).await })
                    .await
                    .ok()
            };
            let mounted = status
                .as_ref()
                .is_some_and(|state| state["state"] == "mounted");
            if what == "action2" || (what == "click" && mounted) {
                let target = config
                    .local_path
                    .clone()
                    .or(config.drive_letter.map(|letter| format!("{letter}:\\")))
                    .unwrap_or_default();
                if !crate::platform::open_path(std::path::Path::new(&target)) {
                    ctx.toast(tr("无法打开挂载位置"), "error");
                }
                return;
            }
            let result = if mounted {
                ctx.call(
                    move |service| async move { service.mount_stop(&id, &long_request()).await },
                )
                .await
            } else {
                ctx.call(move |service| async move {
                    service
                        .mount_start(&id, true, &long_request())
                        .await
                        .map(|_| ())
                })
                .await
            };
            match result {
                Ok(()) => ctx.toast(
                    if mounted {
                        tr("已卸载")
                    } else {
                        tr("已挂载（只读）")
                    },
                    "success",
                ),
                Err(error) => ctx.toast_error(&error),
            }
            mount(ctx);
        }
        Event::StateAction => mount(ctx),
        _ => {}
    }
}

async fn add_mount(ctx: &Ctx) {
    let capabilities = ctx
        .call(|service| async move { service.mount_capabilities().await })
        .await
        .unwrap_or_default();
    let modes: Vec<String> = capabilities["modes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|mode| mode["available"] == true)
        .filter_map(|mode| mode["mode"].as_str().map(str::to_owned))
        .collect();
    let Some(mut mode) = modes.first().cloned() else {
        return ctx.toast(tr("没有可用的挂载方式"), "info");
    };
    let (mut remote, mut local) = ("root".to_owned(), String::new());
    let fields = vec![
        Field::select("mode", tr("挂载方式"), mount_mode_label(&mode)),
        Field::select("remote", tr("云盘文件夹"), "/"),
        Field::select("local", tr("本地位置"), tr("选择文件夹")),
        Field::text("letter", tr("盘符"), "P")
            .detail(tr("仅虚拟盘符方式使用，A–Z 中未占用的字母。")),
    ];
    let mut form = ctx.form(
        &tr("新建挂载"),
        &tr("挂载为只读，不会修改云盘内容。"),
        &tr("保存"),
        "",
        fields,
    );
    let saved = loop {
        match form.next().await {
            FormEvent::Select(id, x, y) => match id.as_str() {
                "mode" => {
                    let items = modes.iter().map(|candidate| {
                        MenuItem::new(candidate, mount_mode_label(candidate))
                            .checked(*candidate == mode)
                    });
                    if let Some(choice) = ctx.menu(x, y, "", items.collect()).await {
                        mode = choice;
                        let label = mount_mode_label(&mode);
                        form.set("mode", |field| field.value = label);
                    }
                }
                "remote" => {
                    if let Some((id, path)) = ctx
                        .pick_folder(&tr("云盘文件夹"), &tr("选择这个文件夹"), Vec::new(), &[])
                        .await
                    {
                        remote = id;
                        form.set("remote", |field| field.value = path);
                    }
                }
                _ => {
                    if let Some(path) = crate::platform::pick_folder(ctx, &tr("本地位置"), "").await
                    {
                        local = path.clone();
                        form.set("local", |field| field.value = path);
                    }
                }
            },
            FormEvent::Submit => {
                let drive = mode == "winfsp_drive";
                let letter = form.value("letter").trim().to_ascii_uppercase();
                if drive && !(letter.len() == 1 && letter.chars().all(|c| c.is_ascii_uppercase())) {
                    form.set("letter", |field| field.error = tr("请输入一个 A–Z 的盘符"));
                    continue;
                }
                if !drive && local.is_empty() {
                    form.set_error(tr("请选择本地位置"));
                    continue;
                }
                let config = MountConfig {
                    config_id: String::new(),
                    mode: mode.clone(),
                    remote_id: remote.clone(),
                    local_path: (!drive).then(|| local.clone()),
                    drive_letter: drive.then_some(letter),
                };
                form.set_busy(true);
                let result = ctx
                    .call(move |service| async move {
                        service.mount_config_save(&config, &request()).await
                    })
                    .await;
                form.set_busy(false);
                match result {
                    Ok(_) => break true,
                    Err(error) => form.set_error(error_text(&error)),
                }
            }
            FormEvent::Cancel => break false,
            _ => {}
        }
    };
    form.close();
    if saved {
        mount(ctx);
    }
}
