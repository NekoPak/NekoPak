//! Transfers page and the live activity summary in the sidebar. The core
//! snapshot is authoritative; events only ask for a (coalesced) re-read.
use crate::{
    app::{error_text, request, Ctx},
    format,
    i18n::{tr, tx},
    nav::Route,
    overlay::MenuItem,
    Choice, Shell, TransferRow, Transfers,
};
use nekopak_domain::*;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{collections::HashSet, rc::Rc, time::Duration};

#[derive(Default)]
pub struct TransfersState {
    tasks: Vec<TransferTask>,
    tab: String,
    collapsed: HashSet<String>,
    loading: bool,
    dirty: bool,
    loaded: bool,
    error: Option<ClientError>,
}

fn is_active(state: TaskState) -> bool {
    matches!(
        state,
        TaskState::Queued
            | TaskState::Running
            | TaskState::Paused
            | TaskState::Retrying
            | TaskState::Verifying
    )
}

fn is_local(kind: TaskKind) -> bool {
    kind != TaskKind::CloudDownload
}

fn tab_matches(tab: &str, kind: TaskKind) -> bool {
    match tab {
        "download" => matches!(kind, TaskKind::Download | TaskKind::SegmentDownload),
        "upload" => kind == TaskKind::Upload,
        "cloud" => kind == TaskKind::CloudDownload,
        _ => true,
    }
}

pub fn install(ctx: &Ctx) {
    let transfers = ctx.ui.global::<Transfers>();
    let weak = Rc::downgrade(ctx);
    macro_rules! handler {
        (|$ctx:ident $(, $arg:ident)*| $body:expr) => {{
            let weak = weak.clone();
            move |$($arg),*| {
                if let Some($ctx) = weak.upgrade() {
                    $body
                }
            }
        }};
    }
    macro_rules! task {
        (|$ctx:ident $(, $arg:ident)*| $body:expr) => {{
            let weak = weak.clone();
            move |$($arg),*| {
                if let Some($ctx) = weak.upgrade() {
                    let task = $ctx.clone();
                    task.spawn(async move { let $ctx = $ctx; $body })
                }
            }
        }};
    }
    transfers.on_tab(handler!(|ctx, id| ctx.replace_route(Route::Transfers {
        tab: id.to_string()
    })));
    transfers.on_refresh(handler!(|ctx| request_reload(&ctx)));
    transfers.on_add_link(handler!(|ctx| crate::actions::add_link(&ctx, None)));
    transfers.on_resume_all(task!(|ctx| control_all(&ctx, TransferAction::Resume).await));
    transfers.on_pause_all(task!(|ctx| control_all(&ctx, TransferAction::Pause).await));
    transfers.on_clear_finished(task!(|ctx| clear_finished(&ctx).await));
    transfers.on_limit_menu(task!(|ctx, x, y| limit_menu(&ctx, x, y).await));
    transfers.on_toggle_group(handler!(|ctx, id| {
        {
            let mut state = ctx.transfers.borrow_mut();
            let id = id.to_string();
            if !state.collapsed.remove(&id) {
                state.collapsed.insert(id);
            }
        }
        render(&ctx);
    }));
    transfers.on_primary(task!(|ctx, id| primary(&ctx, &id).await));
    transfers.on_menu(task!(
        |ctx, id, x, y| row_menu(&ctx, id.to_string(), x, y).await
    ));
    transfers.on_activated(task!(|ctx, id| open_directory(&ctx, id.to_string()).await));
}

pub fn enter(ctx: &Ctx, route: &Route) {
    if let Route::Transfers { tab } = route {
        ctx.transfers.borrow_mut().tab = tab.clone();
    }
    render(ctx);
    request_reload(ctx);
}

/// Coalesces bursts of change events into at most one snapshot read at a time.
pub fn request_reload(ctx: &Ctx) {
    {
        let mut state = ctx.transfers.borrow_mut();
        if state.loading {
            state.dirty = true;
            return;
        }
        state.loading = true;
        state.dirty = false;
    }
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        loop {
            let session = ctx.session();
            let result = ctx
                .call(|service| async move {
                    let mut tasks = Vec::new();
                    let mut cursor: Option<String> = None;
                    let mut seen = HashSet::new();
                    loop {
                        let page = service
                            .visible_transfers(cursor.as_deref(), Some(500), &request())
                            .await?;
                        tasks.extend(page.items);
                        match page.next_cursor {
                            // A repeated cursor would loop forever; refuse the incomplete snapshot.
                            Some(next) if !seen.insert(next.clone()) || seen.len() > 50 => {
                                return Err(ClientError::new(
                                    ErrorCode::Conflict,
                                    "服务返回重复分页，未接受不完整快照。",
                                ));
                            }
                            Some(next) => cursor = Some(next),
                            None => return Ok(tasks),
                        }
                    }
                })
                .await;
            if session == ctx.session() {
                let mut state = ctx.transfers.borrow_mut();
                state.loaded = true;
                match result {
                    Ok(tasks) => {
                        state.tasks = tasks;
                        state.error = None;
                    }
                    Err(error) => state.error = Some(error),
                }
            }
            render(&ctx);
            // Keep the UI responsive under a stream of progress events.
            let (done, wait) = tokio::sync::oneshot::channel::<()>();
            slint::Timer::single_shot(Duration::from_millis(350), move || {
                let _ = done.send(());
            });
            let _ = wait.await;
            let mut state = ctx.transfers.borrow_mut();
            if state.dirty {
                state.dirty = false;
            } else {
                state.loading = false;
                break;
            }
        }
    });
}

fn state_label(task: &TransferTask) -> (String, &'static str) {
    let text = match task.state {
        TaskState::Queued => "排队中",
        TaskState::Paused => "已暂停",
        TaskState::Retrying => "重试等待",
        TaskState::Verifying => "校验中",
        TaskState::Completed => "已完成",
        TaskState::Failed => "失败",
        TaskState::Cancelled => "已取消",
        TaskState::Running => match task.kind {
            TaskKind::Download => "下载中",
            TaskKind::Upload => "上传中",
            TaskKind::SegmentDownload => "正在转封装",
            TaskKind::CloudDownload => "云端下载中",
        },
    };
    let tone = match task.state {
        TaskState::Failed => "error",
        TaskState::Completed => "ok",
        TaskState::Paused | TaskState::Cancelled | TaskState::Retrying => "warn",
        _ => "primary",
    };
    (tr(text), tone)
}

fn task_name(task: &TransferTask) -> String {
    task.name
        .clone()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| task.remote_id.clone())
}

fn done_bytes(task: &TransferTask) -> u64 {
    task.received_bytes
        .map(|bytes| bytes.0)
        .unwrap_or(0)
        .max(task.transferred_bytes.0)
}

fn row_for(ctx: &Ctx, task: &TransferTask) -> TransferRow {
    let (state_text, state_tone) = state_label(task);
    let total = task.total_bytes.map(|bytes| bytes.0);
    let done = done_bytes(task);
    let percent = match (task.state, task.kind) {
        (TaskState::Completed, _) => Some(100.0),
        (_, TaskKind::CloudDownload) => task
            .progress_percent
            .map(|value| value.min(100) as f32)
            .or_else(|| format::percent(done, total)),
        _ => format::percent(done, total)
            .or(task.progress_percent.map(|value| value.min(100) as f32)),
    };
    let running = task.state == TaskState::Running;
    let (icon, tone) = match task.kind {
        TaskKind::Download => ("download", "download"),
        TaskKind::Upload => ("upload", "upload"),
        TaskKind::CloudDownload => ("cloud-down", "cloud"),
        TaskKind::SegmentDownload => ("clip", "clip"),
    };
    let (primary_icon, primary_tip, primary_action) = match task.state {
        TaskState::Running | TaskState::Queued | TaskState::Retrying if is_local(task.kind) => {
            ("pause", tr("暂停"), "pause")
        }
        TaskState::Paused if is_local(task.kind) => ("play", tr("继续"), "resume"),
        TaskState::Failed => ("retry", tr("重试"), "retry"),
        _ => ("", String::new(), ""),
    };
    let detail = match (&task.error, task.state) {
        (Some(error), TaskState::Failed) => error_text(error),
        _ => format::relative(Some(&task.updated_at)),
    };
    let mut name = task_name(task);
    if let Some(label) = &task.quality_label {
        name = format!("{name} · {label}");
    }
    if let Some(clip) = &task.clip {
        name = format!(
            "{name} · {}–{}",
            format::media_time(clip.start_ms),
            format::media_time(clip.end_ms)
        );
    }
    TransferRow {
        id: task.id.as_str().into(),
        header: false,
        name: name.into(),
        kind_icon: ctx.icon(icon),
        kind_tone: tone.into(),
        state_text: state_text.into(),
        state_tone: state_tone.into(),
        detail: detail.into(),
        progress: match percent {
            Some(value) => value / 100.0,
            None if running => -1.0,
            None => 0.0,
        },
        progress_tone: match task.state {
            TaskState::Failed => "error",
            TaskState::Completed => "ok",
            TaskState::Paused | TaskState::Cancelled => "muted",
            _ => "primary",
        }
        .into(),
        percent: percent
            .map(|value| format!("{value:.1}%"))
            .unwrap_or_else(|| "—".into())
            .into(),
        size: match total {
            Some(total) if task.state == TaskState::Completed => format::bytes(total),
            Some(total) => format!("{} / {}", format::bytes(done), format::bytes(total)),
            None if done > 0 => format::bytes(done),
            None => tr("大小未知"),
        }
        .into(),
        speed: if running && task.speed_bytes_per_sec.is_some() {
            format::speed(task.speed_bytes_per_sec)
        } else {
            String::new()
        }
        .into(),
        eta: if running {
            format::eta(done, total, task.speed_bytes_per_sec)
                .map(|eta| format!("{} {eta}", tr("剩余")))
                .unwrap_or_default()
        } else {
            String::new()
        }
        .into(),
        destination: task.destination.clone().unwrap_or_default().into(),
        primary_icon: if primary_icon.is_empty() {
            slint::Image::default()
        } else {
            ctx.icon(primary_icon)
        },
        primary_tip: primary_tip.into(),
        primary_action: primary_action.into(),
        ..TransferRow::default()
    }
}

/// Pushes rows, tab counts, footer totals and the sidebar summary.
pub fn render(ctx: &Ctx) {
    let state = ctx.transfers.borrow();
    let transfers = ctx.ui.global::<Transfers>();
    let tab = if state.tab.is_empty() {
        "all"
    } else {
        state.tab.as_str()
    };
    let count = |wanted: &str| {
        state
            .tasks
            .iter()
            .filter(|task| tab_matches(wanted, task.kind))
            .count()
    };
    let tabs = [
        ("all", "全部"),
        ("download", "下载"),
        ("upload", "上传"),
        ("cloud", "云下载"),
    ];
    transfers.set_tabs(ModelRc::new(VecModel::from(
        tabs.iter()
            .map(|(id, label)| Choice {
                id: (*id).into(),
                label: tr(label).into(),
                detail: count(id).to_string().into(),
                selected: *id == tab,
                enabled: true,
            })
            .collect::<Vec<_>>(),
    )));
    let visible: Vec<&TransferTask> = state
        .tasks
        .iter()
        .filter(|task| tab_matches(tab, task.kind))
        .collect();
    type Belongs = fn(&TransferTask) -> bool;
    let groups: [(&str, &str, Belongs); 3] = [
        ("attention", "需要处理", |task| {
            task.state == TaskState::Failed
        }),
        ("active", "进行中", |task| is_active(task.state)),
        ("finished", "已完成", |task| {
            matches!(task.state, TaskState::Completed | TaskState::Cancelled)
        }),
    ];
    let mut rows = Vec::new();
    for (id, label, belongs) in &groups {
        let members: Vec<&&TransferTask> = visible.iter().filter(|task| belongs(task)).collect();
        if members.is_empty() {
            continue;
        }
        let collapsed = state.collapsed.contains(*id);
        rows.push(TransferRow {
            id: (*id).into(),
            header: true,
            title: tr(label).into(),
            count: members.len().to_string().into(),
            collapsed,
            ..TransferRow::default()
        });
        if !collapsed {
            rows.extend(members.iter().map(|task| row_for(ctx, task)));
        }
    }
    transfers.set_rows(ModelRc::new(VecModel::from(rows)));
    transfers.set_phase(
        if !state.loaded {
            "loading"
        } else if state.error.is_some() && state.tasks.is_empty() {
            "error"
        } else {
            "ready"
        }
        .into(),
    );
    let (title, body) = match (&state.error, tab) {
        (Some(error), _) if state.tasks.is_empty() => {
            (crate::app::error_title(error), error_text(error))
        }
        (_, "all") => (
            tr("还没有任务"),
            tr("在网盘中选择文件下载，或点“添加链接”让服务端保存磁力或 http(s) 资源。"),
        ),
        (_, other) => {
            let label = tabs
                .iter()
                .find(|(id, _)| *id == other)
                .map(|(_, label)| tr(label))
                .unwrap_or_default();
            (tx("没有{0}任务", &[&label]), String::new())
        }
    };
    transfers.set_state_title(title.into());
    transfers.set_state_body(body.into());
    transfers.set_can_resume_all(
        state
            .tasks
            .iter()
            .any(|task| task.state == TaskState::Paused && is_local(task.kind)),
    );
    transfers.set_can_pause_all(state.tasks.iter().any(|task| {
        matches!(
            task.state,
            TaskState::Running | TaskState::Queued | TaskState::Retrying
        ) && is_local(task.kind)
    }));
    transfers.set_can_clear(state.tasks.iter().any(|task| task.state.is_terminal()));
    transfers.set_can_add_link(ctx.can(Operation::CloudTask));

    let mut active = 0usize;
    let mut failed = 0usize;
    let (mut down, mut up) = (0.0f64, 0.0f64);
    let (mut remaining, mut eta_known) = (0u64, true);
    for task in &state.tasks {
        if task.state == TaskState::Failed {
            failed += 1;
        }
        if !is_active(task.state) {
            continue;
        }
        active += 1;
        if task.state == TaskState::Running {
            let rate = task.speed_bytes_per_sec.unwrap_or(0.0);
            match task.kind {
                TaskKind::Upload => up += rate,
                TaskKind::Download | TaskKind::SegmentDownload => {
                    down += rate;
                    match task.total_bytes {
                        Some(total) => remaining += total.0.saturating_sub(done_bytes(task)),
                        None => eta_known = false,
                    }
                }
                TaskKind::CloudDownload => {}
            }
        }
    }
    transfers.set_down_speed(format::speed(Some(down)).into());
    transfers.set_up_speed(format::speed(Some(up)).into());
    transfers.set_eta_label(
        if down > 0.0 && eta_known && remaining > 0 {
            format!(
                "{}{}",
                tr("下载剩余约"),
                format::duration(remaining as f64 / down)
            )
        } else {
            String::new()
        }
        .into(),
    );
    let core = ctx.core.borrow();
    let limit = core.settings["bandwidth_limit_kib_s"].as_u64().unwrap_or(0);
    transfers.set_limit_label(
        if limit == 0 {
            tr("不限速")
        } else {
            tx("限速 {0} MB/s", &[&format!("{:.1}", limit as f64 / 1024.0)])
        }
        .into(),
    );
    let quota = core.quota.as_ref().and_then(|quota| {
        ["download_daily", "download"].iter().find_map(|wanted| {
            let dimension = quota
                .dimensions
                .iter()
                .find(|dimension| dimension.dimension == *wanted)?;
            let limit = dimension.limit_bytes?.0;
            let used = dimension.used_bytes.map(|bytes| bytes.0).or(dimension
                .remaining_bytes
                .map(|left| limit.saturating_sub(left.0)))?;
            Some((*wanted, used, limit))
        })
    });
    match quota {
        Some((dimension, used, limit)) if limit > 0 => {
            transfers.set_quota_label(
                tr(if dimension == "download_daily" {
                    "今日下行"
                } else {
                    "本月下行"
                })
                .into(),
            );
            transfers.set_quota_value((used as f64 / limit as f64).min(1.0) as f32);
            transfers.set_quota_text(
                format!("{} / {}", format::bytes(used), format::bytes(limit)).into(),
            );
        }
        _ => {
            transfers.set_quota_label(Default::default());
            transfers.set_quota_value(-1.0);
            transfers.set_quota_text(Default::default());
        }
    }
    drop(core);
    drop(state);

    let shell = ctx.ui.global::<Shell>();
    shell.set_transfers_count(
        if active == 0 {
            String::new()
        } else if active > 99 {
            "99+".into()
        } else {
            active.to_string()
        }
        .into(),
    );
    shell.set_transfers_alert(failed > 0 && down <= 0.0);
    shell.set_transfers_sub(
        if down > 0.0 {
            format!("↓ {}", format::speed(Some(down)).replace(' ', ""))
        } else if failed > 0 {
            tx("{0} 项需处理", &[&failed.to_string()])
        } else if active > 0 {
            tx("{0} 项进行中", &[&active.to_string()])
        } else if limit > 0 {
            tr("已限速")
        } else {
            String::new()
        }
        .into(),
    );
}

async fn control(ctx: &Ctx, id: String, action: TransferAction) -> Result<(), ClientError> {
    ctx.call(move |service| async move {
        service
            .control_visible_transfer(&id, action, &request())
            .await
    })
    .await
}

async fn primary(ctx: &Ctx, id: &str) {
    let Some(task) = ctx
        .transfers
        .borrow()
        .tasks
        .iter()
        .find(|task| task.id == id)
        .cloned()
    else {
        return;
    };
    let action = match task.state {
        TaskState::Paused => TransferAction::Resume,
        TaskState::Failed => {
            if task
                .error
                .as_ref()
                .is_some_and(|error| error.code == ErrorCode::QuotaExceeded)
            {
                return ctx.toast(
                    tr("配额已用尽，重试不会成功，等待配额重置后再试"),
                    "warning",
                );
            }
            TransferAction::Retry
        }
        _ => TransferAction::Pause,
    };
    if let Err(error) = control(ctx, task.id, action).await {
        ctx.toast_error(&error);
    }
    request_reload(ctx);
}

async fn control_all(ctx: &Ctx, action: TransferAction) {
    let ids: Vec<String> = ctx
        .transfers
        .borrow()
        .tasks
        .iter()
        .filter(|task| {
            is_local(task.kind)
                && match action {
                    TransferAction::Resume => task.state == TaskState::Paused,
                    _ => matches!(
                        task.state,
                        TaskState::Running | TaskState::Queued | TaskState::Retrying
                    ),
                }
        })
        .map(|task| task.id.clone())
        .collect();
    if ids.is_empty() {
        return ctx.toast(
            tr(if action == TransferAction::Resume {
                "没有已暂停的任务"
            } else {
                "没有可暂停的本地任务"
            }),
            "info",
        );
    }
    let (mut ok, mut failed) = (0, 0);
    for id in ids {
        match control(ctx, id, action).await {
            Ok(()) => ok += 1,
            Err(_) => failed += 1,
        }
    }
    if failed > 0 {
        ctx.toast(
            tx(
                "{0} 项成功，{1} 项失败",
                &[&ok.to_string(), &failed.to_string()],
            ),
            "warning",
        );
    } else {
        let verb = tr(if action == TransferAction::Resume {
            "继续"
        } else {
            "暂停"
        });
        ctx.toast(tx("已{0} {1} 项", &[&verb, &ok.to_string()]), "success");
    }
    request_reload(ctx);
}

async fn clear_finished(ctx: &Ctx) {
    if !ctx
        .ask(
            &tr("清理已结束"),
            &tr("保留已下载文件与云端文件"),
            &tr("清理已结束"),
            false,
        )
        .await
    {
        return;
    }
    match ctx
        .call(|service| async move { service.transfer_clear_finished(&request()).await })
        .await
    {
        Ok(removed) => ctx.toast(tx("已移除 {0} 条记录", &[&removed.to_string()]), "success"),
        Err(error) => ctx.toast_error(&error),
    }
    request_reload(ctx);
}

async fn open_directory(ctx: &Ctx, id: String) {
    let path = ctx
        .call(move |service| async move { service.transfer_directory(&id, &request()).await })
        .await;
    match path {
        Ok(path) => {
            if !crate::platform::open_path(&path) {
                ctx.toast(tr("无法打开下载文件夹。"), "error");
            }
        }
        Err(error) => ctx.toast_error(&error),
    }
}

async fn row_menu(ctx: &Ctx, id: String, x: f32, y: f32) {
    let Some(task) = ctx
        .transfers
        .borrow()
        .tasks
        .iter()
        .find(|task| task.id == id)
        .cloned()
    else {
        return;
    };
    let terminal = task.state.is_terminal();
    let has_directory = task.kind != TaskKind::CloudDownload
        && task.kind != TaskKind::Upload
        && task.destination.is_some();
    let items = vec![
        MenuItem::new("open-dir", tr("打开所在文件夹"))
            .icon("folder-open")
            .gate(if has_directory {
                Ok(())
            } else {
                Err(tr("此任务没有本地下载目录"))
            }),
        MenuItem::new("view", tr("在网盘中查看"))
            .icon("cloud")
            .gate(
                if task.kind == TaskKind::CloudDownload && task.state != TaskState::Completed {
                    Err(tr("完成后可查看"))
                } else {
                    Ok(())
                },
            ),
        MenuItem::new("diagnostics", tr("复制诊断信息"))
            .icon("copy")
            .gate(if task.error.is_some() {
                Ok(())
            } else {
                Err(tr("任务没有错误"))
            }),
        MenuItem::separator(),
        MenuItem::new("cancel", tr("取消任务"))
            .icon("x")
            .danger()
            .gate(if terminal {
                Err(tr("任务已结束"))
            } else {
                Ok(())
            }),
        MenuItem::new("forget", tr("移除记录"))
            .icon("trash")
            .gate(if terminal {
                Ok(())
            } else {
                Err(tr("任务已结束"))
            })
            .enabled(terminal),
    ];
    match ctx.menu(x, y, &task_name(&task), items).await.as_deref() {
        Some("open-dir") => open_directory(ctx, id).await,
        Some("view") => {
            let remote = task.remote_id.clone();
            let parent = ctx
                .call(move |service| async move { service.files_stat(&remote, &request()).await })
                .await
                .ok()
                .map(|entry| {
                    if entry.kind == EntryKind::Folder {
                        entry.id
                    } else {
                        entry.parent_id.unwrap_or_else(|| "root".into())
                    }
                });
            match parent {
                Some(folder) => ctx.navigate(Route::Files { folder }),
                None => ctx.toast(tr("找不到项目：资源已失效或没有可用来源"), "warning"),
            }
        }
        Some("diagnostics") => {
            if let Some(error) = &task.error {
                let text = format!(
                    "NekoPak {}\ntask: {:?} {:?}\ncode: {:?}\nmessage: {}\nretryable: {}",
                    env!("CARGO_PKG_VERSION"),
                    task.kind,
                    task.state,
                    error.code,
                    error.message,
                    error.retryable
                );
                crate::platform::copy_text(ctx, &text);
                ctx.toast(tr("诊断信息已复制"), "info");
            }
        }
        Some("cancel") => {
            if let Err(error) = control(ctx, id, TransferAction::Cancel).await {
                ctx.toast_error(&error);
            }
            request_reload(ctx);
        }
        Some("forget") => {
            if let Err(error) = ctx
                .call(move |service| async move { service.transfer_forget(&id, &request()).await })
                .await
            {
                ctx.toast_error(&error);
            }
            request_reload(ctx);
        }
        _ => {}
    }
}

async fn limit_menu(ctx: &Ctx, x: f32, y: f32) {
    let current = ctx.core.borrow().settings["bandwidth_limit_kib_s"]
        .as_u64()
        .unwrap_or(0);
    let options: [u64; 5] = [0, 1024, 2048, 5120, 10240];
    let items = options
        .iter()
        .map(|limit| {
            let label = if *limit == 0 {
                tr("不限速")
            } else {
                format!("{} MB/s", limit / 1024)
            };
            MenuItem::new(&limit.to_string(), label).checked(*limit == current)
        })
        .collect();
    let Some(choice) = ctx
        .menu(x, y, &tr("限制 NekoPak 自身的传输带宽"), items)
        .await
    else {
        return;
    };
    let Ok(limit) = choice.parse::<u64>() else {
        return;
    };
    if crate::settings::update_settings(ctx, serde_json::json!({ "bandwidth_limit_kib_s": limit }))
        .await
    {
        ctx.toast(
            if limit == 0 {
                tr("已取消限速")
            } else {
                tx("已限速 {0} MB/s", &[&(limit / 1024).to_string()])
            },
            "success",
        );
        render(ctx);
    }
}
