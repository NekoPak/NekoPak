//! Command palette (Ctrl+K / Ctrl+Shift+P): pages, pinned folders, commands
//! and settings sections ranked by a fuzzy match, plus folder hits from
//! search once something has been typed.
use crate::{
    app::{request, App, Ctx},
    files::{self, Mode},
    i18n::{tr, tx},
    nav::{Route, ROOT_ID},
    Palette, PaletteItem,
};
use nekopak_domain::*;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;

type Run = Rc<dyn Fn(&Ctx)>;

#[derive(Clone)]
struct Command {
    /// Group heading in the source language.
    group: &'static str,
    label: String,
    /// Extra match text: English words and pinyin, like the web palette.
    keys: &'static str,
    icon: &'static str,
    shortcut: &'static str,
    hint: String,
    run: Run,
}

#[derive(Default)]
pub struct PaletteState {
    commands: Vec<Command>,
    folders: Vec<RemoteEntry>,
    /// What each visible row does; `None` for headers.
    shown: Vec<Option<Run>>,
    token: u64,
}

/// Subsequence match with a bonus for word starts and contiguous runs; 0 = no match.
pub fn score(text: &str, query: &str) -> u32 {
    let needle: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if needle.is_empty() {
        return 1;
    }
    let hay: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    if let Some(at) = hay
        .windows(needle.len())
        .position(|window| window == needle.as_slice())
    {
        return 100u32.saturating_sub(at as u32).max(1) + 20;
    }
    let (mut from, mut points, mut run) = (0usize, 0u32, 0u32);
    for wanted in needle {
        let Some(offset) = hay[from..].iter().position(|c| *c == wanted) else {
            return 0;
        };
        let at = from + offset;
        run = if offset == 0 { run + 1 } else { 0 };
        let word_start =
            at == 0 || matches!(hay[at - 1], ' ' | '/' | '·' | '_' | '-' | '（' | '：');
        points += 1 + run * 2 + if word_start { 3 } else { 0 };
        from = at + 1;
    }
    points
}

fn command(
    group: &'static str,
    label: String,
    keys: &'static str,
    icon: &'static str,
    shortcut: &'static str,
    run: impl Fn(&Ctx) + 'static,
) -> Command {
    Command {
        group,
        label,
        keys,
        icon,
        shortcut,
        hint: String::new(),
        run: Rc::new(run),
    }
}

/// Runs a selection command through the same dispatcher as the menus.
fn on_selection(id: &'static str) -> impl Fn(&Ctx) + 'static {
    move |ctx: &Ctx| {
        let entries = ctx.files.borrow().selected_entries();
        let task = ctx.clone();
        ctx.spawn(async move { crate::actions::run(&task, id, entries, 0.0, 0.0).await });
    }
}

fn build(ctx: &Ctx) -> Vec<Command> {
    let mut out = Vec::new();
    let go = |route: Route| move |ctx: &Ctx| ctx.navigate(route.clone());
    let pages: [(&str, &'static str, &'static str, &'static str, Route); 10] = [
        (
            "网盘",
            "files drive cloud wangpan",
            "cloud",
            "Ctrl+1",
            Route::root(),
        ),
        (
            "下载",
            "transfers tasks downloads xiazai renwu",
            "download",
            "Ctrl+2",
            Route::Transfers { tab: "all".into() },
        ),
        ("最近", "recent zuijin", "clock", "Ctrl+3", Route::Recent),
        (
            "星标",
            "starred favourites xingbiao",
            "star",
            "Ctrl+4",
            Route::Starred,
        ),
        (
            "我的分享",
            "shares fenxiang",
            "share",
            "Ctrl+5",
            Route::Shares,
        ),
        (
            "回收站",
            "trash bin huishouzhan",
            "trash",
            "Ctrl+6",
            Route::Trash,
        ),
        ("自动同步", "sync tongbu", "sync", "Ctrl+7", Route::Sync),
        (
            "挂载",
            "mount drive guazai",
            "drive",
            "Ctrl+8",
            Route::Mount,
        ),
        (
            "本地文件夹",
            "local folders bendi",
            "folder",
            "",
            Route::Local,
        ),
        (
            "引用归档",
            "references archive yinyong",
            "archive",
            "",
            Route::References,
        ),
    ];
    for (label, keys, icon, shortcut, route) in pages {
        out.push(command("跳转", tr(label), keys, icon, shortcut, go(route)));
    }
    for pin in &ctx.prefs.borrow().account.pins {
        let mut item = command(
            "跳转",
            pin.name.clone(),
            "pin",
            "folder",
            "",
            go(Route::Files {
                folder: pin.id.clone(),
            }),
        );
        item.hint = tr("快速访问");
        out.push(item);
    }

    let route = ctx.route();
    let (mode, folder, selected, listed) = {
        let state = ctx.files.borrow();
        (
            state.mode,
            state.folder.clone(),
            state.selected.len(),
            state.order.len(),
        )
    };
    let in_folder = matches!(route, Route::Files { .. }) && mode == Mode::Files;
    let on_files = route.page() == "files";
    if in_folder && ctx.can(Operation::Mkdir) {
        out.push(command(
            "操作",
            tr("新建文件夹"),
            "mkdir new folder xinjian",
            "folder-plus",
            "Ctrl+Shift+N",
            |ctx| {
                let task = ctx.clone();
                ctx.spawn(async move { crate::actions::new_folder(&task).await });
            },
        ));
    }
    if in_folder && ctx.can(Operation::Upload) {
        out.push(command(
            "操作",
            tr("上传文件"),
            "upload shangchuan",
            "upload",
            "",
            |ctx| {
                let task = ctx.clone();
                ctx.spawn(async move { crate::actions::upload(&task, false).await });
            },
        ));
        if ctx.feature("upload_directory") {
            out.push(command(
                "操作",
                tr("上传文件夹"),
                "upload folder shangchuan",
                "folder-move",
                "",
                |ctx| {
                    let task = ctx.clone();
                    ctx.spawn(async move { crate::actions::upload(&task, true).await });
                },
            ));
        }
    }
    if ctx.can(Operation::CloudTask) {
        out.push(command(
            "操作",
            tr("添加链接（云下载）"),
            "magnet link cloud torrent tianjia lianjie",
            "bolt",
            "",
            |ctx| crate::actions::add_link(ctx, None),
        ));
    }
    if ctx.can(Operation::Share) {
        out.push(command(
            "操作",
            tr("打开分享链接"),
            "open share link dakai fenxiang",
            "link",
            "",
            |ctx| {
                let task = ctx.clone();
                ctx.spawn(async move { crate::tools::open_share_link(&task).await });
            },
        ));
    }
    if in_folder && ctx.files.borrow().clip.is_some() {
        out.push(command(
            "操作",
            tr("粘贴"),
            "paste zhantie",
            "paste",
            "Ctrl+V",
            |ctx| {
                let task = ctx.clone();
                ctx.spawn(async move { crate::actions::paste(&task).await });
            },
        ));
    }
    if on_files && listed > 0 {
        out.push(command(
            "操作",
            tr("全选"),
            "select all quanxuan",
            "select-all",
            "Ctrl+A",
            files::select_all,
        ));
    }
    if on_files && selected > 0 {
        if files::source_of(&folder) != "shared" && ctx.can(Operation::Download) {
            out.push(command(
                "操作",
                tr("下载所选"),
                "download selection xiazai",
                "download",
                "",
                on_selection("download"),
            ));
        }
        if mode == Mode::Files && ctx.can(Operation::Rename) && selected > 1 {
            out.push(command(
                "操作",
                tr("批量重命名"),
                "rename batch chongmingming",
                "find-replace",
                "",
                on_selection("rename"),
            ));
        }
    }
    if in_folder && ctx.can(Operation::Duplicates) {
        out.push(command(
            "操作",
            tr("在当前文件夹查找重复"),
            "duplicates dedupe chongfu",
            "duplicates",
            "",
            go(Route::Duplicates {
                folder: folder.clone(),
            }),
        ));
    }
    if in_folder && ctx.can(Operation::SpaceSummary) {
        out.push(command(
            "操作",
            tr("当前文件夹的空间分布"),
            "space usage kongjian",
            "space",
            "",
            go(Route::Space {
                folder: folder.clone(),
            }),
        ));
    }
    if (in_folder || mode == Mode::Local) && on_files && ctx.feature("media_session") {
        out.push(command(
            "操作",
            tr("打开媒体库"),
            "gallery media library images videos meitiku tuku",
            "library",
            "",
            files::open_gallery,
        ));
    }
    out.push(command(
        "操作",
        tr("刷新"),
        "refresh reload shuaxin",
        "refresh",
        "F5",
        crate::shortcuts::refresh,
    ));
    if let Some((label, _)) = ctx.undo.borrow().as_ref() {
        out.push(command(
            "操作",
            tx("撤销{0}", &[label]),
            "undo chexiao",
            "undo",
            "Ctrl+Z",
            |ctx| ctx.undo_last(),
        ));
    }
    out.push(command(
        "操作",
        tr("新建标签页"),
        "new tab biaoqianye",
        "plus",
        "Ctrl+T",
        |ctx| {
            ctx.open_tab(Route::root(), None);
        },
    ));

    let dark = ctx.ui.global::<crate::Tk>().get_dark();
    out.push(command(
        "操作",
        if dark {
            tr("切换到浅色主题")
        } else {
            tr("切换到深色主题")
        },
        "theme dark light zhuti",
        if dark { "sun" } else { "moon" },
        "",
        move |ctx| {
            let task = ctx.clone();
            let theme = if dark { "light" } else { "dark" };
            ctx.spawn(async move {
                crate::settings::update_settings(&task, serde_json::json!({ "theme": theme }))
                    .await;
            });
        },
    ));
    let (list_view, compact) = {
        let prefs = ctx.prefs.borrow();
        (
            prefs.account.view == "list",
            prefs.account.density == "compact",
        )
    };
    if on_files {
        out.push(command(
            "操作",
            if list_view {
                tr("切换到网格视图")
            } else {
                tr("切换到列表视图")
            },
            "view grid list shitu",
            if list_view { "grid" } else { "rows" },
            "",
            move |ctx| files::set_view(ctx, if list_view { "grid" } else { "list" }),
        ));
    }
    out.push(command(
        "操作",
        if compact {
            tr("使用舒适行距")
        } else {
            tr("使用紧凑行距")
        },
        "density compact comfortable hangju",
        "rows",
        "",
        move |ctx| {
            let density = if compact { "comfortable" } else { "compact" };
            if crate::settings::update_account_prefs(ctx, |prefs| prefs.density = density.into()) {
                crate::settings::apply_theme(ctx);
            }
        },
    ));
    out.push(command(
        "操作",
        if ctx.prefs.borrow().device.sidebar_collapsed {
            tr("展开侧栏")
        } else {
            tr("收起侧栏")
        },
        "sidebar cebian",
        "menu-open",
        "Ctrl+B",
        |ctx| ctx.toggle_sidebar(),
    ));
    out.push(command(
        "操作",
        tr("查看全部快捷键"),
        "help shortcuts keyboard kuaijiejian",
        "keyboard",
        "F1",
        |ctx| {
            let task = ctx.clone();
            ctx.spawn(async move { crate::settings_page::shortcuts(&task).await });
        },
    ));
    if ctx.updater.is_some() {
        out.push(command(
            "操作",
            tr("检查更新"),
            "update upgrade gengxin",
            "refresh",
            "",
            go(Route::Settings {
                section: "about".into(),
            }),
        ));
    }
    if ctx.mock {
        out.push(command(
            "操作",
            tr("演示场景与故障注入"),
            "demo mock scenario yanshi",
            "flask",
            "",
            crate::demo::open,
        ));
    }

    let sections: [(&str, &'static str, &'static str, &'static str); 6] = [
        (
            "general",
            "外观",
            "palette",
            "settings shezhi appearance theme language waiguan",
        ),
        (
            "account",
            "账号",
            "user",
            "settings shezhi account login sign zhanghao",
        ),
        (
            "transfers",
            "下载与传输",
            "download",
            "settings shezhi download quality player xiazai",
        ),
        (
            "network",
            "网络",
            "network",
            "settings shezhi network proxy wangluo",
        ),
        ("quota", "配额", "quota", "settings shezhi quota peie"),
        (
            "about",
            "关于",
            "info",
            "settings shezhi about version guanyu",
        ),
    ];
    for (section, label, icon, keys) in sections {
        out.push(command(
            "设置",
            tx("设置：{0}", &[&tr(label)]),
            keys,
            icon,
            if section == "general" { "Ctrl+," } else { "" },
            go(Route::Settings {
                section: section.into(),
            }),
        ));
    }
    out
}

const GROUPS: [&str; 4] = ["跳转", "操作", "文件夹", "设置"];

/// Rebuilds the visible rows for the current query and folder hits.
fn refresh(ctx: &Ctx) {
    let palette = ctx.ui.global::<Palette>();
    let query = palette.get_query().trim().to_owned();
    let mut rows: Vec<PaletteItem> = Vec::new();
    let mut shown: Vec<Option<Run>> = Vec::new();
    {
        let state = ctx.palette.borrow();
        let row = |item: &Command, group_hint: bool| PaletteItem {
            label: item.label.as_str().into(),
            hint: if !item.hint.is_empty() {
                item.hint.as_str().into()
            } else if group_hint {
                tr(item.group).into()
            } else {
                Default::default()
            },
            shortcut: item.shortcut.into(),
            icon: ctx.icon(item.icon),
            header: false,
        };
        if query.is_empty() {
            for group in GROUPS {
                let mut first = true;
                for item in state.commands.iter().filter(|item| item.group == group) {
                    if first {
                        rows.push(PaletteItem {
                            label: tr(group).into(),
                            header: true,
                            ..PaletteItem::default()
                        });
                        shown.push(None);
                        first = false;
                    }
                    rows.push(row(item, false));
                    shown.push(Some(item.run.clone()));
                }
            }
        } else {
            let mut ranked: Vec<(u32, &Command)> = state
                .commands
                .iter()
                .filter_map(|item| {
                    let points = score(&item.label, &query).max(score(item.keys, &query) * 4 / 5);
                    (points > 0).then_some((points, item))
                })
                .collect();
            ranked.sort_by_key(|item| std::cmp::Reverse(item.0));
            for (_, item) in ranked {
                rows.push(row(item, true));
                shown.push(Some(item.run.clone()));
            }
            for folder in &state.folders {
                let id = folder.id.clone();
                rows.push(PaletteItem {
                    label: folder.name.as_str().into(),
                    hint: tr("文件夹").into(),
                    icon: ctx.icon("folder"),
                    ..PaletteItem::default()
                });
                shown.push(Some(Rc::new(move |ctx: &Ctx| {
                    ctx.navigate(Route::Files { folder: id.clone() })
                })));
            }
            if ctx.can(Operation::Search) {
                let text = query.clone();
                rows.push(PaletteItem {
                    label: tx("在云盘中搜索“{0}”", &[&query]).into(),
                    icon: ctx.icon("search"),
                    ..PaletteItem::default()
                });
                shown.push(Some(Rc::new(move |ctx: &Ctx| {
                    ctx.navigate(Route::Search {
                        query: text.clone(),
                    })
                })));
            }
        }
    }
    let first = shown.iter().position(Option::is_some);
    ctx.palette.borrow_mut().shown = shown;
    palette.set_items(ModelRc::new(VecModel::from(rows)));
    palette.set_current(first.map(|index| index as i32).unwrap_or(-1));
}

/// Looks up matching folders a moment after typing stops.
fn search_folders(ctx: &Ctx) {
    let query = ctx.ui.global::<Palette>().get_query().trim().to_owned();
    let token = {
        let mut state = ctx.palette.borrow_mut();
        state.token += 1;
        state.token
    };
    if query.is_empty() || !ctx.can(Operation::Search) {
        if !ctx.palette.borrow().folders.is_empty() {
            ctx.palette.borrow_mut().folders.clear();
        }
        return;
    }
    let task = ctx.clone();
    ctx.spawn(async move {
        let (done, wait) = tokio::sync::oneshot::channel::<()>();
        slint::Timer::single_shot(std::time::Duration::from_millis(180), move || {
            let _ = done.send(());
        });
        let _ = wait.await;
        if task.palette.borrow().token != token {
            return;
        }
        let session = task.session();
        let found = task
            .call(move |service| async move {
                service
                    .files_query(&FileQuery::Search(query), None, Some(50), &request())
                    .await
            })
            .await;
        if task.palette.borrow().token != token || session != task.session() || !task.palette_open()
        {
            return;
        }
        let folders: Vec<RemoteEntry> = found
            .map(|page| page.items)
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::Folder && entry.id != ROOT_ID)
            .take(6)
            .collect();
        let current = task.ui.global::<Palette>().get_current();
        task.palette.borrow_mut().folders = folders;
        refresh(&task);
        // Late folder hits must not move a highlight the user already chose.
        let count = task.palette.borrow().shown.len() as i32;
        if current >= 0 && current < count {
            task.ui.global::<Palette>().set_current(current);
        }
    });
}

fn step(ctx: &Ctx, delta: i32) {
    let palette = ctx.ui.global::<Palette>();
    let usable: Vec<i32> = ctx
        .palette
        .borrow()
        .shown
        .iter()
        .enumerate()
        .filter(|(_, run)| run.is_some())
        .map(|(index, _)| index as i32)
        .collect();
    if usable.is_empty() {
        return;
    }
    let at = usable
        .iter()
        .position(|index| *index == palette.get_current());
    let next = match at {
        None => 0,
        Some(at) if delta.abs() == 1 => (at as i32 + delta).rem_euclid(usable.len() as i32),
        Some(at) => (at as i32 + delta).clamp(0, usable.len() as i32 - 1),
    };
    palette.set_current(usable[next as usize]);
}

pub fn install(ctx: &Ctx) {
    let palette = ctx.ui.global::<Palette>();
    let weak = Rc::downgrade(ctx);
    palette.on_edited({
        let weak = weak.clone();
        move |_| {
            if let Some(ctx) = weak.upgrade() {
                refresh(&ctx);
                search_folders(&ctx);
            }
        }
    });
    palette.on_move({
        let weak = weak.clone();
        move |delta| {
            if let Some(ctx) = weak.upgrade() {
                step(&ctx, delta);
            }
        }
    });
    palette.on_hover({
        let weak = weak.clone();
        move |index| {
            if let Some(ctx) = weak.upgrade() {
                let usable = ctx
                    .palette
                    .borrow()
                    .shown
                    .get(index.max(0) as usize)
                    .is_some_and(Option::is_some);
                if usable {
                    ctx.ui.global::<Palette>().set_current(index);
                }
            }
        }
    });
    palette.on_activate({
        let weak = weak.clone();
        move |index| {
            if let Some(ctx) = weak.upgrade() {
                let run = ctx
                    .palette
                    .borrow()
                    .shown
                    .get(index.max(0) as usize)
                    .cloned()
                    .flatten();
                if let Some(run) = run.filter(|_| index >= 0) {
                    ctx.close_palette();
                    run(&ctx);
                }
            }
        }
    });
    palette.on_dismissed(move || {
        if let Some(ctx) = weak.upgrade() {
            ctx.close_palette();
        }
    });
}

impl App {
    pub fn palette_open(&self) -> bool {
        self.ui.global::<Palette>().get_open()
    }

    pub fn open_palette(self: &Rc<Self>) {
        if self.dialog_open() || self.ui.global::<crate::Shell>().get_boot() != "ready" {
            return;
        }
        self.finish_menu(None);
        {
            let commands = build(self);
            let mut state = self.palette.borrow_mut();
            state.commands = commands;
            state.folders.clear();
            state.token += 1;
        }
        let palette = self.ui.global::<Palette>();
        palette.set_query(Default::default());
        refresh(self);
        palette.set_open(true);
    }

    pub fn close_palette(&self) {
        let palette = self.ui.global::<Palette>();
        if !palette.get_open() {
            return;
        }
        palette.set_open(false);
        {
            let mut state = self.palette.borrow_mut();
            state.token += 1;
            state.commands.clear();
            state.shown.clear();
            state.folders.clear();
        }
        if !self.dialog_open() {
            self.ui.invoke_focus_root();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn fuzzy_score_prefers_substrings_and_word_starts() {
        assert_eq!(score("anything", ""), 1);
        assert_eq!(score("回收站", "下载"), 0);
        assert!(score("Settings: Network", "net") > score("Settings: Network", "stn"));
        assert!(score("new folder", "nf") > score("no effect", "nf"));
        assert!(score("回收站", "回收") > 0);
        assert!(score("Trash", "TRA") > 0);
        assert_eq!(score("abc", "abcd"), 0);
    }
}
