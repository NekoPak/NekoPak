//! File actions shared by menus, the selection bar and keyboard shortcuts.
//! Every mutation uses one idempotency key per user intent, so a retry from
//! the error toast can never apply the same request twice.
use crate::{
    app::{error_text, error_title, new_key, request, App, Ctx},
    files::{self, ClipMode, Mode},
    format,
    i18n::{tr, tx},
    names,
    nav::{Route, ROOT_ID},
    overlay::{ConfirmOptions, MenuItem, PromptOptions},
    prefs::Pin,
};
use nekopak_domain::*;
use std::rc::Rc;

fn is_video(entry: &RemoteEntry) -> bool {
    names::file_kind(&entry.name, entry.kind, entry.mime_type.as_deref()) == names::KIND_VIDEO
}

fn is_playable(entry: &RemoteEntry) -> bool {
    matches!(
        names::file_kind(&entry.name, entry.kind, entry.mime_type.as_deref()),
        names::KIND_VIDEO | names::KIND_AUDIO
    )
}

fn is_archive(entry: &RemoteEntry) -> bool {
    entry.kind == EntryKind::File && names::is_archive_browsable(&entry.name)
}

fn count_suffix(count: usize) -> String {
    if count > 1 {
        tx(" {0} 项", &[&count.to_string()])
    } else {
        String::new()
    }
}

fn done_label(op: MutateOp) -> String {
    tr(match op {
        MutateOp::Mkdir => "已新建文件夹",
        MutateOp::Rename => "已重命名",
        MutateOp::Move => "已移动",
        MutateOp::Copy => "已复制",
        MutateOp::Trash => "已移至回收站",
        MutateOp::Restore => "已还原",
        MutateOp::DeleteForever => "已永久删除",
        MutateOp::Star => "已添加星标",
        MutateOp::Unstar => "已取消星标",
    })
}

fn noun(op: MutateOp) -> String {
    tr(match op {
        MutateOp::Rename => "重命名",
        MutateOp::Move => "移动",
        MutateOp::Copy => "复制",
        MutateOp::Trash => "移至回收站",
        MutateOp::Star => "星标",
        MutateOp::Unstar => "取消星标",
        _ => "",
    })
}

/// Steps that undo a successful mutation, when it is reversible.
fn inverse_of(
    op: MutateOp,
    result: &OperationResult,
    before: &[RemoteEntry],
) -> Option<Vec<FileMutation>> {
    let was: Vec<&RemoteEntry> = before
        .iter()
        .filter(|entry| result.succeeded.contains(&entry.id))
        .collect();
    let ids = || was.iter().map(|entry| entry.id.clone()).collect::<Vec<_>>();
    let step = |op, ids, target| FileMutation { op, ids, target };
    Some(match op {
        MutateOp::Rename => was
            .iter()
            .map(|entry| {
                step(
                    MutateOp::Rename,
                    vec![entry.id.clone()],
                    Some(MutateTarget {
                        parent_id: None,
                        name: Some(entry.name.clone()),
                    }),
                )
            })
            .collect(),
        MutateOp::Move => {
            let mut parents: Vec<(String, Vec<String>)> = Vec::new();
            for entry in &was {
                let parent = entry
                    .parent_id
                    .clone()
                    .unwrap_or_else(|| ROOT_ID.to_owned());
                match parents.iter_mut().find(|(id, _)| *id == parent) {
                    Some((_, ids)) => ids.push(entry.id.clone()),
                    None => parents.push((parent, vec![entry.id.clone()])),
                }
            }
            parents
                .into_iter()
                .map(|(parent, ids)| {
                    step(
                        MutateOp::Move,
                        ids,
                        Some(MutateTarget {
                            parent_id: Some(parent),
                            name: None,
                        }),
                    )
                })
                .collect()
        }
        MutateOp::Copy if !result.created.is_empty() => {
            vec![step(
                MutateOp::Trash,
                result
                    .created
                    .iter()
                    .map(|entry| entry.id.clone())
                    .collect(),
                None,
            )]
        }
        MutateOp::Trash => vec![step(MutateOp::Restore, ids(), None)],
        MutateOp::Star => vec![step(MutateOp::Unstar, ids(), None)],
        MutateOp::Unstar => vec![step(MutateOp::Star, ids(), None)],
        _ => return None,
    })
}

async fn run_inverse(ctx: &Ctx, label: String, steps: Vec<FileMutation>) {
    let session = ctx.session();
    let mut failed = 0usize;
    let mut first = String::new();
    for step in steps {
        if session != ctx.session() {
            return;
        }
        if step.ids.is_empty() {
            continue;
        }
        let count = step.ids.len();
        let result = ctx
            .call(move |service| async move {
                service.files_mutate(&step, &new_key(), &request()).await
            })
            .await;
        match result {
            Ok(result) => {
                failed += result.failed.len();
                if first.is_empty() {
                    if let Some(failure) = result.failed.first() {
                        first = error_text(&failure.error);
                    }
                }
            }
            Err(error) => {
                failed += count;
                if first.is_empty() {
                    first = error_text(&error);
                }
            }
        }
    }
    if session != ctx.session() {
        return;
    }
    ctx.tree_changed();
    if failed > 0 {
        ctx.toast(
            tx(
                "撤销{0}时 {1} 项失败：{2}",
                &[&label, &failed.to_string(), &first],
            ),
            "error",
        );
    } else {
        ctx.toast(tx("已撤销{0}", &[&label]), "success");
    }
}

/// Runs one mutation and reports it; reversible operations offer "撤销".
pub async fn mutate(
    ctx: &Ctx,
    op: MutateOp,
    ids: Vec<String>,
    target: Option<MutateTarget>,
    before: Vec<RemoteEntry>,
) -> Option<OperationResult> {
    mutate_keyed(ctx, op, ids, target, before, new_key()).await
}

fn mutate_keyed(
    ctx: &Ctx,
    op: MutateOp,
    ids: Vec<String>,
    target: Option<MutateTarget>,
    before: Vec<RemoteEntry>,
    key: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<OperationResult>> + '_>> {
    Box::pin(async move {
        let session = ctx.session();
        let mutation = FileMutation {
            op,
            ids: ids.clone(),
            target: target.clone(),
        };
        let result = ctx
            .call({
                let key = key.clone();
                move |service| async move { service.files_mutate(&mutation, &key, &request()).await }
            })
            .await;
        if session != ctx.session() {
            return None;
        }
        match result {
            Ok(result) => {
                let ok = if op == MutateOp::Mkdir {
                    1
                } else {
                    result.succeeded.len()
                };
                if result.failed.is_empty() {
                    let text = format!(
                        "{}{}{}",
                        done_label(op),
                        count_suffix(ok),
                        if result.replayed {
                            tr("（重复请求已忽略）")
                        } else {
                            String::new()
                        }
                    );
                    match inverse_of(op, &result, &before).filter(|_| ok > 0 && !result.replayed) {
                        Some(steps) => {
                            let label = format!("{}{}", noun(op), count_suffix(ok));
                            ctx.set_undo(label.clone(), steps.clone());
                            ctx.toast_action(text, "success", tr("撤销"), move |ctx| {
                                ctx.take_undo();
                                let ctx = ctx.clone();
                                ctx.clone()
                                    .spawn(async move { run_inverse(&ctx, label, steps).await });
                            });
                        }
                        None => ctx.toast(text, "success"),
                    }
                } else {
                    let first = error_text(&result.failed[0].error);
                    let prefix = if ok > 0 {
                        format!("{} {} 项，", done_label(op), ok)
                    } else {
                        String::new()
                    };
                    ctx.toast(
                        tx(
                            "{0}{1} 项失败：{2}",
                            &[&prefix, &result.failed.len().to_string(), &first],
                        ),
                        "error",
                    );
                }
                ctx.tree_changed();
                Some(result)
            }
            Err(error) if error.retryable => {
                let text = format!("{}：{}", error_title(&error), error_text(&error));
                ctx.toast_action(text, "error", tr("重试"), move |ctx| {
                    let ctx = ctx.clone();
                    ctx.clone().spawn(async move {
                        mutate_keyed(&ctx, op, ids, target, before, key).await;
                    });
                });
                None
            }
            Err(error) => {
                ctx.toast(
                    format!("{}：{}", error_title(&error), error_text(&error)),
                    "error",
                );
                None
            }
        }
    })
}

impl App {
    fn set_undo(&self, label: String, steps: Vec<FileMutation>) {
        *self.undo.borrow_mut() = Some((label, steps));
    }

    fn take_undo(&self) -> Option<(String, Vec<FileMutation>)> {
        self.undo.borrow_mut().take()
    }

    /// Ctrl+Z: reverts the most recent reversible file operation.
    pub fn undo_last(self: &Rc<Self>) {
        match self.take_undo() {
            Some((label, steps)) => {
                let ctx = self.clone();
                self.spawn(async move { run_inverse(&ctx, label, steps).await });
            }
            None => self.toast(tr("没有可以撤销的操作"), "info"),
        }
    }
}

fn name_prompt<'a>(
    title: String,
    initial: String,
    siblings: &'a [String],
    current: Option<&'a str>,
    confirm: String,
) -> PromptOptions<'a> {
    let select_to = names::split_name(&initial).0.chars().count();
    PromptOptions {
        title,
        label: tr("名称"),
        select_to: if current.is_some() { select_to } else { 0 },
        initial,
        placeholder: String::new(),
        confirm,
        password: false,
        validate: Box::new(move |text| {
            let check = names::validate_name(text, siblings, current);
            (check.error, check.warnings.join("\n"))
        }),
    }
}

/// Creates a folder in `parent`; used by the page and by the folder picker.
pub async fn new_folder_in(ctx: &Ctx, parent: &str, siblings: Vec<String>) -> bool {
    if let Err(reason) = ctx.gate(Operation::Mkdir) {
        ctx.toast(reason, "info");
        return false;
    }
    let Some(name) = ctx
        .prompt(name_prompt(
            tr("新建文件夹"),
            tr("新建文件夹"),
            &siblings,
            None,
            tr("新建"),
        ))
        .await
    else {
        return false;
    };
    let target = MutateTarget {
        parent_id: Some(parent.to_owned()),
        name: Some(name),
    };
    mutate(ctx, MutateOp::Mkdir, Vec::new(), Some(target), Vec::new())
        .await
        .is_some()
}

pub async fn new_folder(ctx: &Ctx) {
    let (folder, siblings, mode) = {
        let state = ctx.files.borrow();
        (state.folder.clone(), state.sibling_names(), state.mode)
    };
    if mode != Mode::Files {
        return;
    }
    new_folder_in(ctx, &folder, siblings).await;
}

async fn rename(ctx: &Ctx, entry: RemoteEntry) {
    if let Err(reason) = ctx.gate(Operation::Rename) {
        return ctx.toast(reason, "info");
    }
    let siblings = ctx.files.borrow().sibling_names();
    let prompt = name_prompt(
        tr("重命名"),
        entry.name.clone(),
        &siblings,
        Some(&entry.name),
        tr("重命名"),
    );
    if let Some(name) = ctx.prompt(prompt).await {
        let target = MutateTarget {
            parent_id: None,
            name: Some(name),
        };
        mutate(
            ctx,
            MutateOp::Rename,
            vec![entry.id.clone()],
            Some(target),
            vec![entry],
        )
        .await;
    }
}

async fn trash(ctx: &Ctx, entries: Vec<RemoteEntry>) {
    if let Err(reason) = ctx.gate(Operation::Trash) {
        return ctx.toast(reason, "info");
    }
    let (title, body) = if entries.len() == 1 {
        (
            tr("移至回收站？"),
            format!(
                "“{}{}",
                entries[0].name,
                tr("”将移至云盘回收站，可在回收站中还原，也可以立即撤销（Ctrl+Z）。")
            ),
        )
    } else {
        (
            tx("将 {0} 项移至回收站？", &[&entries.len().to_string()]),
            format!(
                "{}{} {}",
                tr("所选"),
                entries.len(),
                tr("项将移至云盘回收站，可在回收站中还原，也可以立即撤销（Ctrl+Z）。")
            ),
        )
    };
    if ctx.ask(&title, &body, &tr("移至回收站"), true).await {
        let ids = entries.iter().map(|entry| entry.id.clone()).collect();
        mutate(ctx, MutateOp::Trash, ids, None, entries).await;
    }
}

async fn delete_forever(ctx: &Ctx, entries: Vec<RemoteEntry>) {
    if let Err(reason) = ctx.gate(Operation::DeleteForever) {
        return ctx.toast(reason, "info");
    }
    let confirmed = ctx
        .confirm(ConfirmOptions {
            title: tx("永久删除 {0} 项？", &[&entries.len().to_string()]),
            body: tr("永久删除后无法还原，也无法撤销。"),
            confirm: tr("永久删除"),
            danger: true,
            option: None,
        })
        .await
        .is_some();
    if confirmed {
        let ids = entries.iter().map(|entry| entry.id.clone()).collect();
        mutate(ctx, MutateOp::DeleteForever, ids, None, Vec::new()).await;
    }
}

/// Move or copy to a known folder (picker or paste).
async fn send_to(
    ctx: &Ctx,
    op: MutateOp,
    entries: Vec<RemoteEntry>,
    destination: String,
) -> Option<OperationResult> {
    let gate = ctx.gate(if op == MutateOp::Move {
        Operation::Move
    } else {
        Operation::Copy
    });
    if let Err(reason) = gate {
        ctx.toast(reason, "info");
        return None;
    }
    if files::source_of(&destination) != "drive"
        || entries
            .iter()
            .any(|entry| files::source_of(&entry.id) != "drive")
    {
        ctx.toast(tr("本地和分享来源不能作为云盘写操作的目标"), "info");
        return None;
    }
    let list: Vec<RemoteEntry> = if op == MutateOp::Move {
        entries
            .into_iter()
            .filter(|entry| entry.parent_id.as_deref().unwrap_or(ROOT_ID) != destination)
            .collect()
    } else {
        entries
    };
    if list.is_empty() {
        ctx.toast(tr("已在目标文件夹中"), "info");
        return None;
    }
    if list.iter().any(|entry| entry.id == destination) {
        ctx.toast(tr("不能移动或复制到自身"), "info");
        return None;
    }
    let ids = list.iter().map(|entry| entry.id.clone()).collect();
    mutate(
        ctx,
        op,
        ids,
        Some(MutateTarget {
            parent_id: Some(destination),
            name: None,
        }),
        list,
    )
    .await
}

async fn move_or_copy(ctx: &Ctx, op: MutateOp, entries: Vec<RemoteEntry>) {
    let count = entries.len().to_string();
    let (title, confirm) = if op == MutateOp::Move {
        (tx("移动 {0} 项到…", &[&count]), tr("移动到这里"))
    } else {
        (tx("复制 {0} 项到…", &[&count]), tr("复制到这里"))
    };
    let blocked: Vec<String> = entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Folder)
        .map(|entry| entry.id.clone())
        .collect();
    let start = {
        let state = ctx.files.borrow();
        if state.mode == Mode::Files {
            state.crumbs()
        } else {
            Vec::new()
        }
    };
    if let Some((destination, _)) = ctx.pick_folder(&title, &confirm, start, &blocked).await {
        send_to(ctx, op, entries, destination).await;
    }
}

fn cut_or_copy(ctx: &Ctx, mode: ClipMode, entries: Vec<RemoteEntry>) {
    let gate = ctx.gate(if mode == ClipMode::Cut {
        Operation::Move
    } else {
        Operation::Copy
    });
    if let Err(reason) = gate {
        return ctx.toast(reason, "info");
    }
    if entries
        .iter()
        .any(|entry| files::source_of(&entry.id) != "drive")
    {
        return ctx.toast(tr("本地映射为只读，不执行云端写操作"), "info");
    }
    let verb = if mode == ClipMode::Cut {
        tr("剪切")
    } else {
        tr("复制")
    };
    ctx.toast(
        tx(
            "已{0} {1} 项，到目标文件夹按 Ctrl+V 粘贴",
            &[&verb, &entries.len().to_string()],
        ),
        "info",
    );
    ctx.files.borrow_mut().clip = Some((mode, entries));
    files::rebuild(ctx);
}

pub async fn paste(ctx: &Ctx) {
    let (clip, folder, mode) = {
        let state = ctx.files.borrow();
        (state.clip.clone(), state.folder.clone(), state.mode)
    };
    if mode != Mode::Files {
        return;
    }
    let Some((clip_mode, entries)) = clip else {
        return ctx.toast(tr("剪贴板中没有云盘项目"), "info");
    };
    let op = if clip_mode == ClipMode::Cut {
        MutateOp::Move
    } else {
        MutateOp::Copy
    };
    if let Some(result) = send_to(ctx, op, entries, folder).await {
        if clip_mode == ClipMode::Cut && result.failed.is_empty() {
            ctx.files.borrow_mut().clip = None;
            files::rebuild(ctx);
        }
    }
}

async fn set_star(ctx: &Ctx, entries: Vec<RemoteEntry>, on: bool) {
    if let Err(reason) = ctx.gate(Operation::Star) {
        return ctx.toast(reason, "info");
    }
    let list: Vec<RemoteEntry> = entries
        .into_iter()
        .filter(|entry| (entry.starred == Some(true)) != on)
        .collect();
    if list.is_empty() {
        return;
    }
    let ids = list.iter().map(|entry| entry.id.clone()).collect();
    mutate(
        ctx,
        if on { MutateOp::Star } else { MutateOp::Unstar },
        ids,
        None,
        list,
    )
    .await;
}

pub fn toggle_pin(ctx: &Ctx, id: &str, name: &str) {
    let pinned = ctx.prefs.borrow().is_pinned(id);
    let (id_owned, name_owned) = (id.to_owned(), name.to_owned());
    let changed = crate::settings::update_account_prefs(ctx, move |prefs| {
        if pinned {
            prefs.pins.retain(|pin| pin.id != id_owned);
        } else {
            prefs.pins.push(Pin {
                id: id_owned,
                name: name_owned,
            });
        }
    });
    if changed {
        ctx.refresh_shell();
        ctx.toast(
            if pinned {
                tx("已取消固定“{0}”", &[name])
            } else {
                tx("已固定“{0}”到快速访问", &[name])
            },
            "success",
        );
    }
}

/// Starts ordinary downloads into the configured (or chosen) local folder.
pub async fn download(ctx: &Ctx, entries: Vec<RemoteEntry>) {
    if let Err(reason) = ctx.gate(Operation::Download) {
        return ctx.toast(reason, "info");
    }
    let Some(destination) = download_directory(ctx).await else {
        return;
    };
    let ids: Vec<String> = entries.iter().map(|entry| entry.id.clone()).collect();
    let session = ctx.session();
    let result = ctx
        .call(move |service| async move {
            nekopak_core::validate_destination(&destination)?;
            service
                .transfer_start_batch(&ids, &destination, &new_key(), &request())
                .await
        })
        .await;
    if session != ctx.session() {
        return;
    }
    match result {
        Ok(batch) => {
            if !batch.task_ids.is_empty() {
                ctx.toast_action(
                    tx(
                        "已加入下载队列 {0} 项",
                        &[&batch.task_ids.len().to_string()],
                    ),
                    "success",
                    tr("查看"),
                    |ctx| ctx.navigate(Route::Transfers { tab: "all".into() }),
                );
            }
            if let Some(skipped) = batch.skipped.first() {
                ctx.toast(tr(&skipped.reason), "warning");
            }
            crate::transfers::request_reload(ctx);
        }
        Err(error) => ctx.toast_error(&error),
    }
}

/// The configured download folder, asking first when the user prefers that.
pub async fn download_directory(ctx: &Ctx) -> Option<String> {
    let (ask, configured) = {
        let core = ctx.core.borrow();
        (
            core.settings["ask_download_directory"] == true,
            core.settings["download_dir"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        )
    };
    if !ask && !configured.is_empty() {
        return Some(configured);
    }
    crate::platform::pick_folder(ctx, &tr("选择下载文件夹"), &configured).await
}

pub async fn upload(ctx: &Ctx, directory: bool) {
    if let Err(reason) = ctx.gate(Operation::Upload) {
        return ctx.toast(reason, "info");
    }
    let (folder, mode) = {
        let state = ctx.files.borrow();
        (state.folder.clone(), state.mode)
    };
    if mode != Mode::Files {
        return;
    }
    let session = ctx.session();
    // The account may change while the system dialog is open; bind the upload
    // to the account that was active when the user asked for it.
    let scope = match ctx
        .call(|service| async move { service.account_scope(&request()).await })
        .await
    {
        Ok(scope) => scope,
        Err(error) => return ctx.toast_error(&error),
    };
    let paths: Vec<String> = if directory {
        crate::platform::pick_folder(ctx, &tr("上传文件夹"), "")
            .await
            .into_iter()
            .collect()
    } else {
        crate::platform::pick_files(ctx, &tr("上传文件")).await
    };
    if paths.is_empty() || session != ctx.session() {
        return;
    }
    let result = ctx
        .call(move |service| async move {
            let mut batch = BatchRef {
                task_ids: Vec::new(),
                skipped: Vec::new(),
                expanded_files: 0,
                group_id: None,
            };
            for path in paths {
                if directory {
                    return service
                        .upload_directory_start_at(&path, &folder, &new_key(), &scope, &request())
                        .await;
                }
                match service
                    .upload_start_at(&path, &folder, &new_key(), &scope, &request())
                    .await
                {
                    Ok(task) => batch.task_ids.push(task.task_id),
                    Err(error) => batch.skipped.push(SkippedEntry {
                        id: String::new(),
                        name: std::path::Path::new(&path)
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned()),
                        reason: error.message,
                    }),
                }
            }
            Ok(batch)
        })
        .await;
    if session != ctx.session() {
        return;
    }
    match result {
        Ok(batch) => {
            if !batch.task_ids.is_empty() {
                ctx.toast(
                    tx(
                        "已加入上传队列 {0} 项",
                        &[&batch.task_ids.len().to_string()],
                    ),
                    "success",
                );
                crate::transfers::request_reload(ctx);
            }
            if let Some(skipped) = batch.skipped.first() {
                ctx.toast(tr(&skipped.reason), "warning");
            }
            if directory {
                files::reload(ctx);
            }
        }
        Err(error) => ctx.toast_error(&error),
    }
}

async fn properties(ctx: &Ctx, entry: RemoteEntry) {
    let path = ctx.files.borrow().path_for(&entry);
    // Read fresh metadata; fall back to the listed entry when the lookup fails.
    let fresh = ctx
        .call({
            let id = entry.id.clone();
            move |service| async move {
                if id.starts_with("local:") {
                    service.local_stat(&id)
                } else {
                    service.files_stat(&id, &request()).await
                }
            }
        })
        .await
        .unwrap_or_else(|_| entry.clone());
    let kind = names::file_kind(&fresh.name, fresh.kind, fresh.mime_type.as_deref());
    let mut rows = vec![
        (tr("名称"), fresh.name.clone(), false),
        (tr("类型"), names::kind_label(kind), false),
        (tr("位置"), path, false),
    ];
    if fresh.kind == EntryKind::File {
        rows.push((
            tr("大小"),
            format!(
                "{}（{}）",
                format::bytes(fresh.size_bytes.0),
                format::bytes_exact(fresh.size_bytes.0)
            ),
            false,
        ));
    }
    rows.push((
        tr("修改时间"),
        format::date_time(fresh.modified_at.as_deref()),
        false,
    ));
    if let Some(mime) = &fresh.mime_type {
        rows.push(("MIME".into(), mime.clone(), true));
    }
    if let Some(hash) = &fresh.hash {
        rows.push((
            format!("{:?}", hash.algorithm).to_uppercase(),
            hash.value.clone(),
            true,
        ));
    }
    for issue in names::windows_issues(&fresh.name) {
        rows.push((
            tr("注意"),
            tx("下载到 Windows 时需转义：{0}", &[&issue]),
            false,
        ));
    }
    rows.push(("ID".into(), fresh.id.clone(), true));
    let can_download = fresh.kind == EntryKind::File
        && files::source_of(&fresh.id) == "drive"
        && ctx.can(Operation::Download);
    if ctx
        .details(&tr("属性"), rows, can_download.then(|| tr("下载")))
        .await
    {
        download(ctx, vec![fresh]).await;
    }
}

/// Default action for double-click / Enter.
pub fn open_entry(ctx: &Ctx, entry: RemoteEntry) {
    if entry.kind == EntryKind::Folder {
        if ctx.files.borrow().mode == Mode::Trash {
            return ctx.toast(tr("请先还原，再打开这个文件夹"), "info");
        }
        return ctx.navigate(Route::Files { folder: entry.id });
    }
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        if is_playable(&entry)
            && ctx.feature("media_external")
            && files::source_of(&entry.id) != "shared"
        {
            crate::media::play(&ctx, entry).await;
        } else if is_archive(&entry) && ctx.can(Operation::Archive) {
            ctx.navigate(Route::Archive {
                file: entry.id,
                path: String::new(),
            });
        } else {
            properties(&ctx, entry).await;
        }
    });
}

pub fn add_link(ctx: &Ctx, prefill: Option<String>) {
    let ctx = ctx.clone();
    ctx.clone()
        .spawn(async move { crate::cloud::add_link_dialog(&ctx, prefill).await });
}

/// Shows only the videos of the current folder, sorted like the list.
pub fn open_media_library(ctx: &Ctx) {
    let mut state = ctx.files.borrow_mut();
    state.kind_filter = if state.kind_filter == Some(names::KIND_VIDEO) {
        None
    } else {
        Some(names::KIND_VIDEO)
    };
    drop(state);
    files::rebuild(ctx);
}

pub async fn batch_rename(ctx: &Ctx, entries: Vec<RemoteEntry>) {
    crate::tools::batch_rename(ctx, entries).await;
}

/// Buttons of the selection bar; ids are shared with `run`.
pub fn selection_actions(ctx: &Ctx, mode: Mode, entries: &[RemoteEntry]) -> Vec<MenuItem> {
    let count = entries.len().to_string();
    match mode {
        Mode::Trash => vec![
            MenuItem::new("restore", tr("还原"))
                .icon("restore")
                .gate(ctx.gate(Operation::Restore)),
            MenuItem::new("delete-forever", tr("永久删除"))
                .icon("trash")
                .danger()
                .gate(ctx.gate(Operation::DeleteForever)),
        ],
        Mode::Shared => vec![
            MenuItem::new("save-shared", tr("转存到我的云盘"))
                .icon("folder-copy")
                .gate(ctx.gate(Operation::Share)),
            MenuItem::new("props", tr("属性"))
                .icon("info")
                .enabled(entries.len() == 1),
        ],
        Mode::Local => vec![MenuItem::new("props", tr("属性"))
            .icon("info")
            .enabled(entries.len() == 1)],
        _ => {
            let all_starred = entries.iter().all(|entry| entry.starred == Some(true));
            vec![
                MenuItem::new("download", tx("下载 {0} 项", &[&count]))
                    .icon("download")
                    .gate(ctx.gate(Operation::Download)),
                MenuItem::new("move", tr("移动到…"))
                    .icon("folder-move")
                    .gate(ctx.gate(Operation::Move)),
                MenuItem::new("copy-to", tr("复制到…"))
                    .icon("folder-copy")
                    .gate(ctx.gate(Operation::Copy)),
                MenuItem::new(
                    "rename",
                    if entries.len() > 1 {
                        tr("批量重命名")
                    } else {
                        tr("重命名")
                    },
                )
                .icon("pencil")
                .hint("F2")
                .gate(ctx.gate(Operation::Rename)),
                MenuItem::new(
                    "star-toggle",
                    if all_starred {
                        tr("取消星标")
                    } else {
                        tr("添加星标")
                    },
                )
                .icon("star")
                .hint("Ctrl+D")
                .gate(ctx.gate(Operation::Star)),
                MenuItem::new("share", tr("分享…"))
                    .icon("share")
                    .gate(ctx.gate(Operation::Share)),
                MenuItem::new("trash", tr("移至回收站"))
                    .icon("trash")
                    .hint("Delete")
                    .danger()
                    .gate(ctx.gate(Operation::Trash)),
            ]
        }
    }
}

/// Context menu for one or more entries.
pub fn entry_menu(ctx: &Ctx, mode: Mode, entries: &[RemoteEntry]) -> Vec<MenuItem> {
    let one = if entries.len() == 1 {
        Some(&entries[0])
    } else {
        None
    };
    let single = |item: MenuItem| {
        item.gate(if one.is_some() {
            Ok(())
        } else {
            Err(tr("请选择一个项目"))
        })
    };
    if mode == Mode::Trash {
        return vec![
            MenuItem::new("restore", tr("还原"))
                .icon("restore")
                .gate(ctx.gate(Operation::Restore)),
            single(MenuItem::new("props", tr("属性")).icon("info")),
            MenuItem::separator(),
            MenuItem::new("delete-forever", tr("永久删除"))
                .icon("trash")
                .danger()
                .gate(ctx.gate(Operation::DeleteForever)),
        ];
    }
    if entries
        .iter()
        .any(|entry| files::source_of(&entry.id) != "drive")
    {
        let mut items = Vec::new();
        if let Some(entry) = one {
            if entry.kind == EntryKind::Folder {
                items.push(MenuItem::new("open", tr("打开")).icon("folder-open"));
            } else if is_playable(entry) {
                items.push(
                    MenuItem::new("play", tr("用外部播放器打开"))
                        .icon("play-circle")
                        .gate(ctx.gate(Operation::ExternalPlayer)),
                );
            }
            if is_video(entry) {
                items.push(MenuItem::new("download-clip", tr("下载指定段落…")).icon("clip"));
            }
        }
        if mode == Mode::Shared {
            items.push(
                MenuItem::new("save-shared", tr("转存到我的云盘"))
                    .icon("folder-copy")
                    .gate(ctx.gate(Operation::Share)),
            );
        }
        items.push(single(MenuItem::new("props", tr("属性")).icon("info")));
        return items;
    }
    let files_count = entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::File)
        .count();
    let folders = entries.len() - files_count;
    let folder = one.filter(|entry| entry.kind == EntryKind::Folder);
    let video = one.filter(|entry| is_video(entry));
    let archive = one.filter(|entry| is_archive(entry));
    let all_starred = entries.iter().all(|entry| entry.starred == Some(true));
    let download = ctx.gate(Operation::Download).and_then(|_| {
        if files_count == 0 && folders > 0 {
            ctx.gate(Operation::FolderDownload)
        } else {
            Ok(())
        }
    });
    let rename = ctx.gate(Operation::Rename);
    let mut items = Vec::new();
    if let Some(entry) = folder {
        let _ = entry;
        items.push(
            MenuItem::new("open", tr("打开"))
                .icon("folder-open")
                .hint("Enter"),
        );
        items.push(MenuItem::new("open-tab", tr("在新标签页打开")).icon("plus"));
    } else if archive.is_some() {
        items.push(
            MenuItem::new("open", tr("浏览压缩包内容"))
                .icon("archive")
                .hint("Enter")
                .gate(ctx.gate(Operation::Archive)),
        );
    }
    if one.is_some_and(is_playable) {
        items.push(
            MenuItem::new("play", tr("用外部播放器打开"))
                .icon("play-circle")
                .gate(ctx.gate(Operation::ExternalPlayer)),
        );
    }
    if !items.is_empty() {
        items.push(MenuItem::separator());
    }
    items.push(
        MenuItem::new(
            "download",
            if entries.len() > 1 {
                tx("下载 {0} 项", &[&entries.len().to_string()])
            } else {
                tr("下载")
            },
        )
        .icon("download")
        .gate(download.clone()),
    );
    if entries.iter().any(is_video) || folders > 0 {
        items.push(
            MenuItem::new("download-quality", tr("下载指定画质…"))
                .icon("quality")
                .gate(ctx.gate(Operation::QualityDownload).and(download)),
        );
    }
    if video.is_some() {
        items.push(
            MenuItem::new("download-clip", tr("下载指定段落…"))
                .icon("clip")
                .gate(ctx.gate(Operation::ClipDownload)),
        );
    }
    items.push(MenuItem::separator());
    items.push(
        MenuItem::new("clip-cut", tr("剪切"))
            .icon("cut")
            .hint("Ctrl+X")
            .gate(ctx.gate(Operation::Move)),
    );
    items.push(
        MenuItem::new("clip-copy", tr("复制"))
            .icon("copy")
            .hint("Ctrl+C")
            .gate(ctx.gate(Operation::Copy)),
    );
    items.push(
        MenuItem::new("move", tr("移动到…"))
            .icon("folder-move")
            .gate(ctx.gate(Operation::Move)),
    );
    items.push(
        MenuItem::new("copy-to", tr("复制到…"))
            .icon("folder-copy")
            .gate(ctx.gate(Operation::Copy)),
    );
    items.push(
        MenuItem::new(
            "rename",
            if entries.len() > 1 {
                tr("批量重命名…")
            } else {
                tr("重命名")
            },
        )
        .icon("pencil")
        .hint("F2")
        .gate(rename),
    );
    items.push(MenuItem::separator());
    items.push(
        MenuItem::new(
            "star-toggle",
            if all_starred {
                tr("取消星标")
            } else {
                tr("添加星标")
            },
        )
        .icon("star")
        .hint("Ctrl+D")
        .gate(ctx.gate(Operation::Star)),
    );
    items.push(
        MenuItem::new("share", tr("分享…"))
            .icon("share")
            .gate(ctx.gate(Operation::Share)),
    );
    items.push(
        MenuItem::new(
            "copy-path",
            if entries.len() > 1 {
                tx("复制 {0} 个路径", &[&entries.len().to_string()])
            } else {
                tr("复制路径")
            },
        )
        .icon("link")
        .hint("Ctrl+Shift+C"),
    );
    if let Some(entry) = folder {
        let pinned = ctx.prefs.borrow().is_pinned(&entry.id);
        items.push(MenuItem::separator());
        items.push(
            MenuItem::new("dups", tr("在此文件夹中查找重复…"))
                .icon("duplicates")
                .gate(ctx.gate(Operation::Duplicates)),
        );
        items.push(
            MenuItem::new("space", tr("查看空间分布"))
                .icon("space")
                .gate(ctx.gate(Operation::SpaceSummary)),
        );
        items.push(
            MenuItem::new(
                "pin",
                if pinned {
                    tr("取消固定")
                } else {
                    tr("固定到快速访问")
                },
            )
            .icon(if pinned { "unpin" } else { "pin" }),
        );
    }
    items.push(MenuItem::separator());
    items.push(
        MenuItem::new("reference", tr("创建引用归档清单"))
            .icon("archive")
            .gate(if ctx.feature("reference_archive") {
                ctx.gate(Operation::Stat)
            } else {
                Err(tr("当前版本尚未启用引用归档"))
            }),
    );
    items.push(single(
        MenuItem::new("props", tr("属性"))
            .icon("info")
            .hint("Alt+Enter"),
    ));
    items.push(MenuItem::separator());
    items.push(
        MenuItem::new("trash", tr("移至回收站"))
            .icon("trash")
            .hint("Delete")
            .danger()
            .gate(ctx.gate(Operation::Trash)),
    );
    items
}

/// Executes an action id from a menu, the selection bar or a shortcut.
pub async fn run(ctx: &Ctx, id: &str, entries: Vec<RemoteEntry>, _x: f32, _y: f32) {
    if entries.is_empty() {
        return;
    }
    let first = entries[0].clone();
    match id {
        "open" => open_entry(ctx, first),
        "open-tab" => {
            ctx.open_tab(Route::Files { folder: first.id }, Some(first.name));
        }
        "play" => crate::media::play(ctx, first).await,
        "download" => download(ctx, entries).await,
        "download-quality" => crate::media::download_quality(ctx, entries).await,
        "download-clip" => crate::media::clip_dialog(ctx, first).await,
        "clip-cut" => cut_or_copy(ctx, ClipMode::Cut, entries),
        "clip-copy" => cut_or_copy(ctx, ClipMode::Copy, entries),
        "move" => move_or_copy(ctx, MutateOp::Move, entries).await,
        "copy-to" => move_or_copy(ctx, MutateOp::Copy, entries).await,
        "rename" if entries.len() == 1 => rename(ctx, first).await,
        "rename" => batch_rename(ctx, entries).await,
        "star-toggle" => {
            let all = entries.iter().all(|entry| entry.starred == Some(true));
            set_star(ctx, entries, !all).await;
        }
        "share" => crate::cloud::share_dialog(ctx, entries).await,
        "copy-path" => {
            let text = {
                let state = ctx.files.borrow();
                entries
                    .iter()
                    .map(|entry| state.path_for(entry))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let what = if entries.len() > 1 {
                tx(" {0} 个路径", &[&entries.len().to_string()])
            } else {
                tr("路径")
            };
            crate::platform::copy_text(ctx, &text);
            ctx.toast(tx("已复制{0}", &[&what]), "success");
        }
        "dups" => ctx.navigate(Route::Duplicates { folder: first.id }),
        "space" => ctx.navigate(Route::Space { folder: first.id }),
        "pin" => toggle_pin(ctx, &first.id, &first.name),
        "reference" => crate::tools::create_reference(ctx, entries).await,
        "props" => properties(ctx, first).await,
        "trash" => trash(ctx, entries).await,
        "restore" => {
            if let Err(reason) = ctx.gate(Operation::Restore) {
                return ctx.toast(reason, "info");
            }
            let ids = entries.iter().map(|entry| entry.id.clone()).collect();
            mutate(ctx, MutateOp::Restore, ids, None, Vec::new()).await;
        }
        "delete-forever" => delete_forever(ctx, entries).await,
        "save-shared" => crate::tools::save_shared(ctx, entries).await,
        _ => {}
    }
}
