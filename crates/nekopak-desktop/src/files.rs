//! File browser controller: loading with pagination, sorting, filtering,
//! selection, keyboard handling and menus for every list-like route.
use crate::{
    actions,
    app::{error_text, error_title, request, App, Ctx},
    collate::SortKey,
    format,
    i18n::{tr, tx},
    names,
    nav::{Route, ROOT_ID},
    overlay::MenuItem,
    Crumb, FileRow, Files, GridRow,
};
use nekopak_domain::*;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    rc::Rc,
};

const PAGE: u32 = 500;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Mode {
    #[default]
    Files,
    Recent,
    Starred,
    Trash,
    Search,
    Local,
    Shared,
}

impl Mode {
    fn id(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Recent => "recent",
            Self::Starred => "starred",
            Self::Trash => "trash",
            Self::Search => "search",
            Self::Local => "local",
            Self::Shared => "shared",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Phase {
    #[default]
    Loading,
    Ready,
    Error,
    SignedOut,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipMode {
    Cut,
    Copy,
}

#[derive(Default)]
pub struct FilesState {
    pub mode: Mode,
    pub route: Option<Route>,
    pub folder: String,
    pub query: String,
    pub crumbs: Vec<(String, String)>,
    pub items: Vec<RemoteEntry>,
    /// Visible order: indices into `items` after filtering and sorting.
    pub order: Vec<usize>,
    index_of: HashMap<String, usize>,
    pub next: Option<String>,
    pub phase: Phase,
    pub error: Option<ClientError>,
    pub loading_more: bool,
    pub more_error: Option<ClientError>,
    pub selected: HashSet<String>,
    anchor: usize,
    focus: Option<usize>,
    pub kind_filter: Option<i32>,
    token: u64,
    rows: Option<Rc<VecModel<FileRow>>>,
    grid: Option<Rc<VecModel<GridRow>>>,
    columns: usize,
    crumb_cache: HashMap<String, Vec<(String, String)>>,
    recent_folders: Vec<(String, String)>,
    pub clip: Option<(ClipMode, Vec<RemoteEntry>)>,
    selection_memory: HashMap<String, HashSet<String>>,
    /// Per-route scroll offsets so going back restores the position.
    scroll_memory: HashMap<String, f32>,
    type_ahead: String,
    type_ahead_at: Option<std::time::Instant>,
}

impl FilesState {
    pub fn visible(&self, index: usize) -> Option<&RemoteEntry> {
        self.order.get(index).and_then(|at| self.items.get(*at))
    }

    pub fn selected_entries(&self) -> Vec<RemoteEntry> {
        self.order
            .iter()
            .filter_map(|at| self.items.get(*at))
            .filter(|entry| self.selected.contains(&entry.id))
            .cloned()
            .collect()
    }

    pub fn sibling_names(&self) -> Vec<String> {
        self.items.iter().map(|entry| entry.name.clone()).collect()
    }

    /// Cloud path of the listed folder, e.g. `/课程/第一章`.
    pub fn crumb_path(&self) -> String {
        format!(
            "/{}",
            self.crumbs
                .iter()
                .skip(1)
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>()
                .join("/")
        )
    }

    pub fn path_for(&self, entry: &RemoteEntry) -> String {
        format!("{}/{}", self.crumb_path().trim_end_matches('/'), entry.name)
    }

    pub fn crumbs(&self) -> Vec<(String, String)> {
        self.crumbs.clone()
    }

    pub fn clear_session(&mut self) {
        self.crumb_cache.clear();
        self.recent_folders.clear();
        self.selection_memory.clear();
        self.scroll_memory.clear();
        self.clip = None;
        self.items.clear();
        self.order.clear();
        self.index_of.clear();
        self.selected.clear();
        self.next = None;
    }
}

fn memory_key(tab: i32, route: &Route) -> String {
    format!("{tab}:{route:?}")
}

pub fn source_of(id: &str) -> &'static str {
    if id.starts_with("local:") {
        "local"
    } else if id.starts_with("shared:") || id.starts_with("share:") {
        "shared"
    } else {
        "drive"
    }
}

pub fn install(ctx: &Ctx) {
    let rows = Rc::new(VecModel::<FileRow>::default());
    let grid = Rc::new(VecModel::<GridRow>::default());
    let files = ctx.ui.global::<Files>();
    files.set_rows(ModelRc::from(rows.clone()));
    files.set_grid(ModelRc::from(grid.clone()));
    {
        let mut state = ctx.files.borrow_mut();
        state.rows = Some(rows);
        state.grid = Some(grid);
        state.columns = 4;
    }
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
    files.on_back(handler!(|ctx| ctx.navigate_history(-1)));
    files.on_forward(handler!(|ctx| ctx.navigate_history(1)));
    files.on_up(handler!(|ctx| go_up(&ctx)));
    files.on_home(handler!(|ctx| ctx.navigate(Route::root())));
    files.on_crumb(handler!(|ctx, id| ctx.navigate(Route::Files {
        folder: id.to_string()
    })));
    files.on_recent_menu(task!(|ctx, x, y| recent_menu(&ctx, x, y).await));
    files.on_open_search(handler!(|ctx| open_search(&ctx)));
    files.on_close_search(handler!(|ctx| close_search(&ctx)));
    files.on_submit_search(handler!(|ctx, text| {
        let query = text.trim().to_owned();
        if !query.is_empty() {
            ctx.navigate(Route::Search { query });
        }
    }));
    files.on_new_menu(task!(|ctx, x, y| new_menu(&ctx, x, y).await));
    files.on_sort_menu(task!(|ctx, x, y| sort_menu(&ctx, x, y).await));
    files.on_filter_menu(task!(|ctx, x, y| filter_menu(&ctx, x, y).await));
    files.on_organize_menu(task!(|ctx, x, y| organize_menu(&ctx, x, y).await));
    files.on_media_library(handler!(|ctx| actions::open_media_library(&ctx)));
    files.on_refresh(handler!(|ctx| reload(&ctx)));
    files.on_set_view(handler!(|ctx, view| set_view(&ctx, &view)));
    files.on_add_link(handler!(|ctx| actions::add_link(&ctx, None)));
    files.on_paste(task!(|ctx| actions::paste(&ctx).await));
    files.on_header_sort(handler!(|ctx, key| header_sort(&ctx, &key)));
    files.on_toggle_all(handler!(|ctx| toggle_all(&ctx)));
    files.on_clear_selection(handler!(|ctx| set_selection(&ctx, HashSet::new(), None)));
    files.on_selection_action(task!(|ctx, id, x, y| {
        let entries = ctx.files.borrow().selected_entries();
        actions::run(&ctx, &id, entries, x, y).await
    }));
    files.on_row_clicked(handler!(|ctx, index, ctrl, shift| row_clicked(
        &ctx,
        index.max(0) as usize,
        ctrl,
        shift
    )));
    files.on_row_toggled(handler!(|ctx, index| row_clicked(
        &ctx,
        index.max(0) as usize,
        true,
        false
    )));
    files.on_row_activated(handler!(|ctx, index| activate(&ctx, index.max(0) as usize)));
    files.on_row_context(task!(|ctx, index, x, y| row_context(
        &ctx,
        index.max(0) as usize,
        x,
        y
    )
    .await));
    files.on_background_context(task!(|ctx, x, y| background_context(&ctx, x, y).await));
    files.on_key({
        let weak = weak.clone();
        move |text, ctrl, shift, alt| {
            weak.upgrade()
                .is_some_and(|ctx| key(&ctx, &text, ctrl, shift, alt))
        }
    });
    files.on_near_end(task!(|ctx| load_more(&ctx, false).await));
    files.on_load_all(task!(|ctx| load_more(&ctx, true).await));
    files.on_retry_more(task!(|ctx| load_more(&ctx, false).await));
    files.on_state_action_clicked(handler!(|ctx| state_action(&ctx, false)));
    files.on_state_action2_clicked(handler!(|ctx| state_action(&ctx, true)));
    files.on_columns_changed(handler!(|ctx, columns| {
        let changed = {
            let mut state = ctx.files.borrow_mut();
            let columns = columns.max(1) as usize;
            let changed = state.columns != columns;
            state.columns = columns;
            changed
        };
        if changed && ctx.prefs.borrow().account.view == "grid" {
            push_rows(&ctx);
        }
    }));
}

/// Called whenever a files-like route becomes current.
pub fn enter(ctx: &Ctx, route: &Route) {
    let files = ctx.ui.global::<Files>();
    let tab = ctx.tab_id();
    let (same_listing, remembered_scroll) = {
        let mut state = ctx.files.borrow_mut();
        // Remember where the previous listing was before replacing it.
        if let Some(previous) = state.route.clone() {
            let key = memory_key(tab, &previous);
            let selection = state.selected.clone();
            if state.selection_memory.len() >= 256 && !state.selection_memory.contains_key(&key) {
                state.selection_memory.clear();
            }
            state.selection_memory.insert(key.clone(), selection);
            if state.scroll_memory.len() >= 256 {
                state.scroll_memory.clear();
            }
            state.scroll_memory.insert(key, files.get_viewport_y());
        }
        let same = state.route.as_ref() == Some(route);
        state.route = Some(route.clone());
        let (mode, folder, query) = match route {
            Route::Files { folder } if folder.starts_with("local:") => {
                (Mode::Local, folder.clone(), String::new())
            }
            Route::Files { folder } => (Mode::Files, folder.clone(), String::new()),
            Route::Search { query } => (Mode::Search, String::new(), query.clone()),
            Route::Recent => (Mode::Recent, String::new(), String::new()),
            Route::Starred => (Mode::Starred, String::new(), String::new()),
            Route::Trash => (Mode::Trash, String::new(), String::new()),
            Route::SharedFolder { parent, .. } => (Mode::Shared, parent.clone(), String::new()),
            _ => return,
        };
        state.mode = mode;
        state.folder = folder;
        state.query = query;
        state.kind_filter = None;
        let key = memory_key(tab, route);
        state.selected = state
            .selection_memory
            .get(&key)
            .cloned()
            .unwrap_or_default();
        state.anchor = 0;
        state.focus = None;
        state.crumbs = match mode {
            Mode::Files if state.folder == ROOT_ID => vec![(ROOT_ID.to_owned(), tr("网盘"))],
            Mode::Files | Mode::Local => state
                .crumb_cache
                .get(&state.folder)
                .cloned()
                .unwrap_or_else(|| {
                    vec![
                        (ROOT_ID.to_owned(), tr("网盘")),
                        (state.folder.clone(), "…".to_owned()),
                    ]
                }),
            Mode::Shared => match route {
                Route::SharedFolder { title, parent, .. } => vec![(parent.clone(), title.clone())],
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        (same, state.scroll_memory.get(&key).copied())
    };
    files.set_searching(matches!(route, Route::Search { .. }));
    if let Route::Search { query } = route {
        files.set_search_text(query.as_str().into());
    }
    push_chrome(ctx);
    let keep = if same_listing {
        ctx.files.borrow().items.len()
    } else {
        0
    };
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        load(&ctx, keep, remembered_scroll).await;
    });
}

/// Reloads the current listing in place, keeping as many rows as were loaded.
pub fn reload(ctx: &Ctx) {
    if !matches!(ctx.route().page(), "files") {
        return;
    }
    let keep = ctx.files.borrow().items.len();
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        load(&ctx, keep.max(1), None).await;
    });
}

async fn fetch(
    ctx: &Ctx,
    mode: Mode,
    folder: String,
    query: String,
    cursor: Option<String>,
) -> Result<Page<RemoteEntry>, ClientError> {
    let route = ctx.route();
    ctx.call(move |service| async move {
        let context = request();
        match mode {
            Mode::Files => {
                service
                    .files_list(&folder, cursor.as_deref(), Some(PAGE), &context)
                    .await
            }
            Mode::Local => service.local_list(&folder, cursor.as_deref(), Some(PAGE)),
            Mode::Recent => {
                service
                    .files_query(&FileQuery::Recent, cursor.as_deref(), Some(100), &context)
                    .await
            }
            Mode::Starred => {
                service
                    .files_query(&FileQuery::Starred, cursor.as_deref(), Some(200), &context)
                    .await
            }
            Mode::Trash => {
                service
                    .files_query(&FileQuery::Trash, cursor.as_deref(), Some(200), &context)
                    .await
            }
            Mode::Search => {
                service
                    .files_query(
                        &FileQuery::Search(query),
                        cursor.as_deref(),
                        Some(200),
                        &context,
                    )
                    .await
            }
            Mode::Shared => {
                let Route::SharedFolder { access, parent, .. } = route else {
                    return Err(ClientError::new(ErrorCode::Internal, "分享来源已失效。"));
                };
                let shared = service
                    .share_folder(&access, &parent, cursor.as_deref(), Some(200), &context)
                    .await?;
                Ok(Page {
                    items: shared.entries,
                    next_cursor: shared.next_cursor,
                })
            }
        }
    })
    .await
}

async fn load(ctx: &Ctx, keep: usize, restore_scroll: Option<f32>) {
    let (token, mode, folder, query) = {
        let mut state = ctx.files.borrow_mut();
        state.token += 1;
        state.error = None;
        state.more_error = None;
        state.loading_more = false;
        if keep == 0 {
            state.items.clear();
            state.order.clear();
            state.index_of.clear();
            state.next = None;
            state.phase = Phase::Loading;
        }
        (
            state.token,
            state.mode,
            state.folder.clone(),
            state.query.clone(),
        )
    };
    let files = ctx.ui.global::<Files>();
    if mode != Mode::Local && !ctx.signed_in() {
        {
            let mut state = ctx.files.borrow_mut();
            state.phase = Phase::SignedOut;
            state.items.clear();
            state.order.clear();
            state.index_of.clear();
        }
        push_rows(ctx);
        push_chrome(ctx);
        return;
    }
    if keep == 0 {
        push_rows(ctx);
        push_chrome(ctx);
    } else {
        files.set_refreshing(true);
    }
    if matches!(mode, Mode::Files | Mode::Local) && folder != ROOT_ID {
        let ctx = ctx.clone();
        let folder = folder.clone();
        ctx.clone()
            .spawn(async move { load_crumbs(&ctx, token, mode, folder).await });
    }
    let session = ctx.session();
    let mut items: Vec<RemoteEntry> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut seen = HashSet::new();
    let result = loop {
        match fetch(ctx, mode, folder.clone(), query.clone(), cursor.clone()).await {
            Ok(page) => {
                items.extend(page.items);
                cursor = page.next_cursor;
                match &cursor {
                    Some(next) if items.len() < keep => {
                        if !seen.insert(next.clone()) {
                            break Err(ClientError::new(
                                ErrorCode::Conflict,
                                "服务返回重复分页，请刷新目录。",
                            ));
                        }
                    }
                    _ => break Ok(()),
                }
            }
            Err(error) => break Err(error),
        }
    };
    if session != ctx.session() || ctx.files.borrow().token != token {
        return;
    }
    files.set_refreshing(false);
    {
        let mut state = ctx.files.borrow_mut();
        match result {
            Ok(()) => {
                state.items = items;
                state.next = cursor;
                state.phase = Phase::Ready;
            }
            Err(error) => {
                state.phase = Phase::Error;
                state.error = Some(error);
                state.items.clear();
                state.next = None;
            }
        }
    }
    rebuild(ctx);
    if let Some(offset) = restore_scroll {
        files.set_viewport_y(offset);
    } else if keep == 0 {
        files.set_viewport_y(0.0);
    }
}

async fn load_crumbs(ctx: &Ctx, token: u64, mode: Mode, folder: String) {
    let chain = ctx
        .call({
            let folder = folder.clone();
            move |service| async move {
                match mode {
                    Mode::Local => service.local_path(&folder),
                    _ => service.files_path(&folder, &request()).await,
                }
            }
        })
        .await;
    let Ok(chain) = chain else { return };
    let title = {
        let mut state = ctx.files.borrow_mut();
        if state.token != token {
            return;
        }
        let root = match mode {
            Mode::Local => (ROOT_ID.to_owned(), tr("本地文件夹")),
            _ => (ROOT_ID.to_owned(), tr("网盘")),
        };
        let crumbs: Vec<(String, String)> = std::iter::once(root)
            .chain(chain.into_iter().map(|entry| (entry.id, entry.name)))
            .collect();
        if state.crumb_cache.len() > 2000 {
            state.crumb_cache.clear();
        }
        state.crumb_cache.insert(folder.clone(), crumbs.clone());
        state.crumbs = crumbs;
        let current = state.crumbs.last().cloned();
        if let Some(current) = &current {
            state.recent_folders.retain(|(id, _)| *id != current.0);
            state.recent_folders.insert(0, current.clone());
            state.recent_folders.truncate(12);
        }
        current.map(|(_, name)| name)
    };
    if let Some(title) = title {
        ctx.set_tab_title(&Route::Files { folder }, &title);
    }
    push_chrome(ctx);
}

async fn load_more(ctx: &Ctx, all: bool) {
    let (token, mode, folder, query, mut cursor) = {
        let mut state = ctx.files.borrow_mut();
        if state.loading_more || state.phase != Phase::Ready {
            return;
        }
        let Some(cursor) = state.next.clone() else {
            return;
        };
        state.loading_more = true;
        state.more_error = None;
        (
            state.token,
            state.mode,
            state.folder.clone(),
            state.query.clone(),
            Some(cursor),
        )
    };
    push_chrome(ctx);
    let session = ctx.session();
    let mut seen = HashSet::new();
    let mut failure = None;
    while let Some(current) = cursor.clone() {
        if !seen.insert(current.clone()) {
            failure = Some(ClientError::new(
                ErrorCode::Conflict,
                "服务返回重复分页，请刷新目录。",
            ));
            break;
        }
        match fetch(ctx, mode, folder.clone(), query.clone(), Some(current)).await {
            Ok(page) => {
                if session != ctx.session() || ctx.files.borrow().token != token {
                    return;
                }
                let mut state = ctx.files.borrow_mut();
                state.items.extend(page.items);
                state.next = page.next_cursor.clone();
                cursor = page.next_cursor;
            }
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
        if !all {
            break;
        }
    }
    if session != ctx.session() || ctx.files.borrow().token != token {
        return;
    }
    {
        let mut state = ctx.files.borrow_mut();
        state.loading_more = false;
        state.more_error = failure;
    }
    rebuild(ctx);
}

fn compare(
    a: &(SortKey, &RemoteEntry, i32, usize),
    b: &(SortKey, &RemoteEntry, i32, usize),
    key: &str,
) -> Ordering {
    let by_name = || a.0.compare(&b.0);
    let order = match key {
        "size" => {
            let size = |entry: &RemoteEntry| {
                if entry.kind == EntryKind::Folder {
                    None
                } else {
                    Some(entry.size_bytes.0)
                }
            };
            size(a.1).cmp(&size(b.1))
        }
        "modified" => {
            a.1.modified_at
                .as_deref()
                .unwrap_or("")
                .cmp(b.1.modified_at.as_deref().unwrap_or(""))
        }
        "kind" => a.2.cmp(&b.2),
        _ => by_name(),
    };
    if order == Ordering::Equal && key != "name" {
        by_name()
    } else {
        order
    }
}

/// Recomputes the visible order and pushes rows, selection and chrome.
pub fn rebuild(ctx: &Ctx) {
    let (sort_key, descending, folders_first) = {
        let prefs = ctx.prefs.borrow();
        (
            prefs.account.sort.key.clone(),
            prefs.account.sort.dir == "desc",
            prefs.account.folders_first,
        )
    };
    {
        let mut state = ctx.files.borrow_mut();
        let filter = state.kind_filter;
        // Recent keeps the service order (most recent first) unless a column is chosen.
        let server_order = state.mode == Mode::Recent;
        let mut keyed: Vec<(SortKey, &RemoteEntry, i32, usize)> = state
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                let kind = names::file_kind(&entry.name, entry.kind, entry.mime_type.as_deref());
                (filter.is_none() || filter == Some(kind))
                    .then(|| (SortKey::new(&entry.name), entry, kind, index))
            })
            .collect();
        if !server_order {
            keyed.sort_by(|a, b| {
                if folders_first && a.1.kind != b.1.kind {
                    return if a.1.kind == EntryKind::Folder {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    };
                }
                let order = compare(a, b, &sort_key);
                let order = if descending { order.reverse() } else { order };
                order.then(a.3.cmp(&b.3))
            });
        }
        let order: Vec<usize> = keyed.iter().map(|item| item.3).collect();
        drop(keyed);
        state.index_of = order
            .iter()
            .enumerate()
            .map(|(visible, at)| (state.items[*at].id.clone(), visible))
            .collect();
        state.order = order;
        // Drop ids that disappeared after a reload; keep focus in range.
        let present: HashSet<String> = state.index_of.keys().cloned().collect();
        state.selected.retain(|id| present.contains(id));
        if let Some(focus) = state.focus {
            state.focus = if state.order.is_empty() {
                None
            } else {
                Some(focus.min(state.order.len() - 1))
            };
        }
        state.anchor = state.anchor.min(state.order.len().saturating_sub(1));
    }
    push_rows(ctx);
    push_chrome(ctx);
}

fn row_for(entry: &RemoteEntry, selected: bool, cut: bool) -> FileRow {
    let kind = names::file_kind(&entry.name, entry.kind, entry.mime_type.as_deref());
    let folder = entry.kind == EntryKind::Folder;
    let issues = names::windows_issues(&entry.name);
    FileRow {
        id: entry.id.as_str().into(),
        name: entry.name.as_str().into(),
        kind,
        folder,
        size: if folder {
            "—".into()
        } else {
            format::bytes(entry.size_bytes.0).into()
        },
        modified: format::date_time(entry.modified_at.as_deref()).into(),
        type_label: names::kind_label(kind).into(),
        location: Default::default(),
        starred: entry.starred == Some(true),
        selected,
        cut,
        note: if issues.is_empty() {
            Default::default()
        } else {
            issues.join("；").into()
        },
    }
}

fn push_rows(ctx: &Ctx) {
    let state = ctx.files.borrow();
    let cut: HashSet<&str> = match &state.clip {
        Some((ClipMode::Cut, entries)) => entries.iter().map(|entry| entry.id.as_str()).collect(),
        _ => HashSet::new(),
    };
    let rows: Vec<FileRow> = state
        .order
        .iter()
        .map(|at| {
            let entry = &state.items[*at];
            row_for(
                entry,
                state.selected.contains(&entry.id),
                cut.contains(entry.id.as_str()),
            )
        })
        .collect();
    let grid_view = ctx.prefs.borrow().account.view == "grid";
    if let Some(grid) = &state.grid {
        if grid_view {
            let columns = state.columns.max(1);
            let lines: Vec<GridRow> = rows
                .chunks(columns)
                .enumerate()
                .map(|(line, cells)| GridRow {
                    cells: ModelRc::new(VecModel::from(cells.to_vec())),
                    first: (line * columns) as i32,
                })
                .collect();
            grid.set_vec(lines);
        } else if grid.row_count() > 0 {
            grid.set_vec(Vec::new());
        }
    }
    if let Some(model) = &state.rows {
        model.set_vec(rows);
    }
}

/// Pushes everything around the list: crumbs, labels, capability flags and states.
pub fn push_chrome(ctx: &Ctx) {
    let files = ctx.ui.global::<Files>();
    let state = ctx.files.borrow();
    let prefs = ctx.prefs.borrow();
    files.set_mode(state.mode.id().into());
    let (title, icon) = match state.mode {
        Mode::Files => (
            state
                .crumbs
                .last()
                .map(|(_, name)| name.clone())
                .unwrap_or_default(),
            "cloud",
        ),
        Mode::Local => (
            state
                .crumbs
                .last()
                .map(|(_, name)| name.clone())
                .unwrap_or_default(),
            "folder",
        ),
        Mode::Shared => (
            state
                .crumbs
                .last()
                .map(|(_, name)| name.clone())
                .unwrap_or_default(),
            "share",
        ),
        Mode::Recent => (tr("最近"), "clock"),
        Mode::Starred => (tr("星标"), "star"),
        Mode::Trash => (tr("回收站"), "trash"),
        Mode::Search => (tx("“{0}”的搜索结果", &[&state.query]), "search"),
    };
    files.set_title(title.into());
    files.set_title_icon(ctx.icon(icon));
    files.set_crumbs(ModelRc::new(VecModel::from(
        state
            .crumbs
            .iter()
            .map(|(id, name)| Crumb {
                id: id.into(),
                name: name.into(),
            })
            .collect::<Vec<_>>(),
    )));
    files.set_can_back(ctx.can_go(-1));
    files.set_can_forward(ctx.can_go(1));
    files.set_can_up(matches!(state.mode, Mode::Files | Mode::Local) && state.crumbs.len() > 1);
    files.set_view(prefs.account.view.as_str().into());
    files.set_grid_size(match prefs.account.grid_size.as_str() {
        "s" => 0,
        "l" => 2,
        _ => 1,
    });
    files.set_sort_key(if state.mode == Mode::Recent {
        "".into()
    } else {
        prefs.account.sort.key.as_str().into()
    });
    files.set_sort_desc(prefs.account.sort.dir == "desc");
    files.set_sort_label(tx("按{0}", &[&sort_label(&prefs.account.sort.key)]).into());
    files.set_filter_label(
        state
            .kind_filter
            .map(names::kind_label)
            .unwrap_or_else(|| tr("全部类型"))
            .into(),
    );
    files.set_filter_active(state.kind_filter.is_some());
    let count = state.order.len();
    files.set_count_label(
        if state.phase != Phase::Ready {
            String::new()
        } else if state.next.is_some() {
            tx(
                "已加载 {0} 项，还有更多",
                &[&format::group_digits(count as u64)],
            )
        } else {
            tx("{0} 项", &[&format::group_digits(count as u64)])
        }
        .into(),
    );
    files.set_has_more(state.next.is_some());
    files.set_loading_more(state.loading_more);
    files.set_more_error(
        state
            .more_error
            .as_ref()
            .map(error_text)
            .unwrap_or_default()
            .into(),
    );
    files.set_show_location(false);
    let drive = state.mode == Mode::Files;
    files.set_can_write(drive && ctx.can(Operation::Mkdir));
    files.set_can_upload(drive && ctx.can(Operation::Upload) && ctx.feature("upload"));
    files.set_can_add_link(matches!(
        state.mode,
        Mode::Files | Mode::Recent | Mode::Starred | Mode::Search
    ));
    files.set_can_organize(
        drive
            && (ctx.can(Operation::Duplicates)
                || ctx.can(Operation::SpaceSummary)
                || ctx.can(Operation::Rename)),
    );
    files.set_can_media(ctx.feature("media_session"));
    files.set_paste_label(match (&state.clip, drive) {
        (Some((_, entries)), true) => tx("粘贴 {0} 项", &[&entries.len().to_string()]).into(),
        _ => Default::default(),
    });
    files.set_phase(
        match state.phase {
            Phase::Loading => "loading",
            Phase::Ready => "ready",
            Phase::Error => "error",
            Phase::SignedOut => "signed-out",
        }
        .into(),
    );
    // Empty, error and signed-out states share one panel.
    let (state_title, state_body, action, action2, state_icon, is_error) = match state.phase {
        Phase::SignedOut => (
            if ctx.auth_expired() {
                tr("登录已过期")
            } else {
                tr("登录后浏览云盘")
            },
            String::new(),
            if ctx.auth_expired() {
                tr("重新登录")
            } else {
                tr("登录")
            },
            String::new(),
            "login",
            false,
        ),
        Phase::Error => {
            let error = state.error.as_ref();
            (
                error.map(error_title).unwrap_or_default(),
                error.map(error_text).unwrap_or_default(),
                tr("重试"),
                if error.is_some_and(|error| error.code == ErrorCode::AuthExpired) {
                    tr("重新登录")
                } else {
                    String::new()
                },
                "alert-circle",
                true,
            )
        }
        _ if state.kind_filter.is_some() && !state.items.is_empty() => (
            tr("没有这种类型的项目"),
            String::new(),
            tr("显示全部类型"),
            String::new(),
            "filter",
            false,
        ),
        _ => match state.mode {
            Mode::Files => (
                tr("这个文件夹是空的"),
                tr("新建文件夹，或粘贴磁力链接、http(s) 链接让服务端保存到这里。"),
                if ctx.can(Operation::Mkdir) {
                    tr("新建文件夹")
                } else {
                    String::new()
                },
                tr("添加链接"),
                "folder-open",
                false,
            ),
            Mode::Local => (
                tr("这个文件夹是空的"),
                tr("只读浏览你选择的本地文件夹。不会上传、同步或修改原文件。"),
                String::new(),
                String::new(),
                "folder-open",
                false,
            ),
            Mode::Recent => (
                tr("还没有最近使用的文件"),
                String::new(),
                String::new(),
                String::new(),
                "clock",
                false,
            ),
            Mode::Starred => (
                tr("还没有星标项目"),
                tr("在文件上按 Ctrl+D 或使用右键菜单添加星标。"),
                String::new(),
                String::new(),
                "star",
                false,
            ),
            Mode::Trash => (
                tr("回收站是空的"),
                String::new(),
                String::new(),
                String::new(),
                "trash",
                false,
            ),
            Mode::Search => (
                tx("没有找到“{0}”", &[&state.query]),
                tr("试试更短的关键词，或检查拼写。"),
                String::new(),
                String::new(),
                "manage-search",
                false,
            ),
            Mode::Shared => (
                tr("这个文件夹是空的"),
                String::new(),
                String::new(),
                String::new(),
                "folder-open",
                false,
            ),
        },
    };
    files.set_state_title(state_title.into());
    files.set_state_body(state_body.into());
    files.set_state_action(action.into());
    files.set_state_action2(action2.into());
    files.set_state_icon(ctx.icon(state_icon));
    files.set_state_error(is_error);
    drop(prefs);
    drop(state);
    push_selection(ctx);
}

fn sort_label(key: &str) -> String {
    tr(match key {
        "size" => "大小",
        "modified" => "修改时间",
        "kind" => "类型",
        _ => "名称",
    })
}

/// Selection count, byte summary, focus marker and the contextual action bar.
fn push_selection(ctx: &Ctx) {
    let files = ctx.ui.global::<Files>();
    let (count, bytes, any_file, focus, mode, entries) = {
        let state = ctx.files.borrow();
        let entries = state.selected_entries();
        let bytes: u64 = entries
            .iter()
            .filter(|entry| entry.kind == EntryKind::File)
            .map(|entry| entry.size_bytes.0)
            .fold(0u64, u64::saturating_add);
        let any_file = entries.iter().any(|entry| entry.kind == EntryKind::File);
        (
            entries.len(),
            bytes,
            any_file,
            state.focus,
            state.mode,
            entries,
        )
    };
    files.set_selected_count(count as i32);
    files.set_selection_summary(if any_file {
        tx("所选文件共 {0}", &[&format::bytes(bytes)]).into()
    } else {
        Default::default()
    });
    files.set_focus_index(focus.map(|index| index as i32).unwrap_or(-1));
    let actions = if count == 0 {
        Vec::new()
    } else {
        actions::selection_actions(ctx, mode, &entries)
    };
    files.set_selection_actions(ModelRc::new(VecModel::from(ctx.menu_entries(&actions))));
}

/// Applies a new selection, touching only the rows whose state changed.
pub fn set_selection(ctx: &Ctx, selection: HashSet<String>, focus: Option<usize>) {
    let grid_view = ctx.prefs.borrow().account.view == "grid";
    {
        let mut state = ctx.files.borrow_mut();
        let changed: Vec<usize> = state
            .selected
            .symmetric_difference(&selection)
            .filter_map(|id| state.index_of.get(id).copied())
            .collect();
        state.selected = selection;
        if focus.is_some() {
            state.focus = focus;
        }
        if !grid_view {
            if let Some(model) = state.rows.clone() {
                for index in changed {
                    if let Some(mut row) = model.row_data(index) {
                        row.selected = state.selected.contains(row.id.as_str());
                        model.set_row_data(index, row);
                    }
                }
            }
        }
    }
    if grid_view {
        push_rows(ctx);
    }
    push_selection(ctx);
}

fn row_clicked(ctx: &Ctx, index: usize, ctrl: bool, shift: bool) {
    let selection = {
        let mut state = ctx.files.borrow_mut();
        let Some(id) = state.visible(index).map(|entry| entry.id.clone()) else {
            return;
        };
        let mut selection = state.selected.clone();
        if shift {
            let (low, high) = if state.anchor <= index {
                (state.anchor, index)
            } else {
                (index, state.anchor)
            };
            if !ctrl {
                selection.clear();
            }
            for visible in low..=high {
                if let Some(entry) = state.visible(visible) {
                    selection.insert(entry.id.clone());
                }
            }
        } else if ctrl {
            if !selection.remove(&id) {
                selection.insert(id);
            }
            state.anchor = index;
        } else {
            selection.clear();
            selection.insert(id);
            state.anchor = index;
        }
        selection
    };
    set_selection(ctx, selection, Some(index));
}

fn toggle_all(ctx: &Ctx) {
    let selection = {
        let state = ctx.files.borrow();
        if state.selected.len() == state.order.len() && !state.order.is_empty() {
            HashSet::new()
        } else {
            state
                .order
                .iter()
                .map(|at| state.items[*at].id.clone())
                .collect()
        }
    };
    set_selection(ctx, selection, None);
}

fn go_up(ctx: &Ctx) {
    let parent = {
        let state = ctx.files.borrow();
        if !matches!(state.mode, Mode::Files | Mode::Local) || state.crumbs.len() < 2 {
            return;
        }
        state.crumbs[state.crumbs.len() - 2].0.clone()
    };
    if parent == ROOT_ID && ctx.files.borrow().mode == Mode::Local {
        ctx.navigate(Route::Local);
    } else {
        ctx.navigate(Route::Files { folder: parent });
    }
}

pub fn open_search(ctx: &Ctx) {
    if !matches!(ctx.route(), Route::Files { .. } | Route::Search { .. }) {
        ctx.navigate(Route::root());
    }
    let files = ctx.ui.global::<Files>();
    files.set_searching(true);
    files.set_search_focus_rev(files.get_search_focus_rev() + 1);
}

fn close_search(ctx: &Ctx) {
    ctx.ui.global::<Files>().set_searching(false);
    if matches!(ctx.route(), Route::Search { .. }) {
        if ctx.can_go(-1) {
            ctx.navigate_history(-1);
        } else {
            ctx.navigate(Route::root());
        }
    }
}

fn set_view(ctx: &Ctx, view: &str) {
    if ctx.prefs.borrow().account.view == view {
        return;
    }
    if !crate::settings::update_account_prefs(ctx, |prefs| prefs.view = view.to_owned()) {
        return;
    }
    ctx.ui.global::<Files>().set_viewport_y(0.0);
    push_rows(ctx);
    push_chrome(ctx);
}

/// One zoom step: list → small → medium → large cards (Ctrl + wheel, Ctrl +/-).
pub fn zoom(ctx: &Ctx, delta: i32) {
    let (view, size) = {
        let prefs = ctx.prefs.borrow();
        (prefs.account.view.clone(), prefs.account.grid_size.clone())
    };
    let steps = [
        ("list", size.as_str()),
        ("grid", "s"),
        ("grid", "m"),
        ("grid", "l"),
    ];
    let at = if view == "list" {
        0
    } else {
        1 + ["s", "m", "l"].iter().position(|s| *s == size).unwrap_or(1)
    };
    let next = steps[(at as i32 + delta).clamp(0, 3) as usize];
    if next.0 == view && next.1 == size {
        return;
    }
    let (next_view, next_size) = (next.0.to_owned(), next.1.to_owned());
    if crate::settings::update_account_prefs(ctx, |prefs| {
        prefs.view = next_view;
        prefs.grid_size = next_size;
    }) {
        push_rows(ctx);
        push_chrome(ctx);
    }
}

fn header_sort(ctx: &Ctx, key: &str) {
    let (current, direction) = {
        let prefs = ctx.prefs.borrow();
        (
            prefs.account.sort.key.clone(),
            prefs.account.sort.dir.clone(),
        )
    };
    let direction = if current == key && direction == "asc" {
        "desc"
    } else {
        "asc"
    };
    set_sort(ctx, key, direction);
}

fn set_sort(ctx: &Ctx, key: &str, direction: &str) {
    let (key, direction) = (key.to_owned(), direction.to_owned());
    if crate::settings::update_account_prefs(ctx, |prefs| {
        prefs.sort.key = key;
        prefs.sort.dir = direction;
    }) {
        rebuild(ctx);
    }
}

async fn sort_menu(ctx: &Ctx, x: f32, y: f32) {
    let (key, descending, folders_first) = {
        let prefs = ctx.prefs.borrow();
        (
            prefs.account.sort.key.clone(),
            prefs.account.sort.dir == "desc",
            prefs.account.folders_first,
        )
    };
    let mut items: Vec<MenuItem> = ["name", "size", "modified", "kind"]
        .iter()
        .map(|candidate| MenuItem::new(candidate, sort_label(candidate)).checked(*candidate == key))
        .collect();
    items.push(MenuItem::separator());
    items.push(
        MenuItem::new("asc", tr("升序"))
            .icon("arrow-up")
            .checked(!descending),
    );
    items.push(MenuItem::new("desc", tr("降序")).checked(descending));
    items.push(MenuItem::separator());
    items.push(MenuItem::new("folders-first", tr("文件夹优先")).checked(folders_first));
    match ctx.menu(x, y, &tr("排序方式"), items).await.as_deref() {
        Some("asc") => set_sort(ctx, &key, "asc"),
        Some("desc") => set_sort(ctx, &key, "desc"),
        Some("folders-first") => {
            if crate::settings::update_account_prefs(ctx, |prefs| {
                prefs.folders_first = !folders_first
            }) {
                rebuild(ctx);
            }
        }
        Some(choice) => set_sort(ctx, choice, if descending { "desc" } else { "asc" }),
        None => {}
    }
}

async fn filter_menu(ctx: &Ctx, x: f32, y: f32) {
    let current = ctx.files.borrow().kind_filter;
    let mut items = vec![
        MenuItem::new("all", tr("全部类型")).checked(current.is_none()),
        MenuItem::separator(),
    ];
    for kind in names::FILTER_KINDS {
        items.push(
            MenuItem::new(&kind.to_string(), names::kind_label(kind))
                .checked(current == Some(kind)),
        );
    }
    let Some(choice) = ctx.menu(x, y, &tr("按类型筛选"), items).await else {
        return;
    };
    ctx.files.borrow_mut().kind_filter = choice.parse().ok();
    ctx.ui.global::<Files>().set_viewport_y(0.0);
    rebuild(ctx);
}

async fn organize_menu(ctx: &Ctx, x: f32, y: f32) {
    let (folder, count) = {
        let state = ctx.files.borrow();
        (state.folder.clone(), state.order.len())
    };
    let items = vec![
        MenuItem::new("duplicates", tr("在此文件夹中查找重复"))
            .icon("duplicates")
            .gate(ctx.gate(Operation::Duplicates)),
        MenuItem::new("space", tr("查看空间分布"))
            .icon("space")
            .gate(ctx.gate(Operation::SpaceSummary)),
        MenuItem::separator(),
        MenuItem::new("batch-rename", tr("批量重命名本文件夹…"))
            .icon("find-replace")
            .gate(ctx.gate(Operation::Rename).and_then(|_| {
                if count >= 2 {
                    Ok(())
                } else {
                    Err(tr("至少需要两个项目"))
                }
            })),
    ];
    match ctx.menu(x, y, &tr("整理"), items).await.as_deref() {
        Some("duplicates") => ctx.navigate(Route::Duplicates { folder }),
        Some("space") => ctx.navigate(Route::Space { folder }),
        Some("batch-rename") => {
            let entries: Vec<RemoteEntry> = {
                let state = ctx.files.borrow();
                state
                    .order
                    .iter()
                    .map(|at| state.items[*at].clone())
                    .collect()
            };
            actions::batch_rename(ctx, entries).await;
        }
        _ => {}
    }
}

async fn new_menu(ctx: &Ctx, x: f32, y: f32) {
    let upload = ctx.gate(Operation::Upload);
    let items = vec![
        MenuItem::new("folder", tr("新建文件夹"))
            .icon("folder-plus")
            .hint("Ctrl+Shift+N")
            .gate(ctx.gate(Operation::Mkdir)),
        MenuItem::separator(),
        MenuItem::new("upload", tr("上传文件"))
            .icon("upload")
            .gate(upload.clone()),
        MenuItem::new("upload-folder", tr("上传文件夹"))
            .icon("folder-move")
            .gate(upload.and_then(|_| {
                if ctx.feature("upload_directory") {
                    Ok(())
                } else {
                    Err(tr("当前版本尚未支持目录上传"))
                }
            })),
    ];
    match ctx.menu(x, y, "", items).await.as_deref() {
        Some("folder") => actions::new_folder(ctx).await,
        Some("upload") => actions::upload(ctx, false).await,
        Some("upload-folder") => actions::upload(ctx, true).await,
        _ => {}
    }
}

async fn recent_menu(ctx: &Ctx, x: f32, y: f32) {
    let list: Vec<(String, String)> = {
        let state = ctx.files.borrow();
        state
            .recent_folders
            .iter()
            .filter(|(id, _)| *id != state.folder)
            .take(8)
            .cloned()
            .collect()
    };
    let items = if list.is_empty() {
        vec![MenuItem::new("none", tr("还没有其他最近访问的位置")).enabled(false)]
    } else {
        list.iter()
            .map(|(id, name)| {
                MenuItem::new(id, name.clone()).icon(if id == ROOT_ID { "cloud" } else { "folder" })
            })
            .collect()
    };
    if let Some(folder) = ctx.menu(x, y, &tr("最近访问"), items).await {
        ctx.navigate(Route::Files { folder });
    }
}

async fn row_context(ctx: &Ctx, index: usize, x: f32, y: f32) {
    // Right-clicking outside the selection selects just that row, like Explorer.
    let needs_select = {
        let state = ctx.files.borrow();
        state
            .visible(index)
            .is_some_and(|entry| !state.selected.contains(&entry.id))
    };
    if needs_select {
        row_clicked(ctx, index, false, false);
    }
    let (entries, mode) = {
        let state = ctx.files.borrow();
        (state.selected_entries(), state.mode)
    };
    if entries.is_empty() {
        return;
    }
    let title = if entries.len() == 1 {
        entries[0].name.clone()
    } else {
        tx("{0} 个项目", &[&entries.len().to_string()])
    };
    let items = actions::entry_menu(ctx, mode, &entries);
    if let Some(choice) = ctx.menu(x, y, &title, items).await {
        actions::run(ctx, &choice, entries, x, y).await;
    }
}

async fn background_context(ctx: &Ctx, x: f32, y: f32) {
    let (mode, has_clip, title) = {
        let state = ctx.files.borrow();
        (
            state.mode,
            state.clip.is_some(),
            state
                .crumbs
                .last()
                .map(|(_, name)| name.clone())
                .unwrap_or_default(),
        )
    };
    let mut items = Vec::new();
    if mode == Mode::Files {
        items.push(
            MenuItem::new("new-folder", tr("新建文件夹"))
                .icon("folder-plus")
                .hint("Ctrl+Shift+N")
                .gate(ctx.gate(Operation::Mkdir)),
        );
        items.push(
            MenuItem::new("add-link", tr("添加链接"))
                .icon("bolt")
                .gate(ctx.gate(Operation::CloudTask)),
        );
        items.push(
            MenuItem::new("upload", tr("上传文件"))
                .icon("upload")
                .gate(ctx.gate(Operation::Upload)),
        );
        items.push(
            MenuItem::new("paste", tr("粘贴"))
                .icon("paste")
                .hint("Ctrl+V")
                .gate(if has_clip {
                    Ok(())
                } else {
                    Err(tr("先在其他位置剪切或复制项目"))
                }),
        );
        items.push(MenuItem::separator());
    }
    items.push(
        MenuItem::new("select-all", tr("全选"))
            .icon("select-all")
            .hint("Ctrl+A"),
    );
    items.push(
        MenuItem::new("refresh", tr("刷新"))
            .icon("refresh")
            .hint("F5"),
    );
    match ctx
        .menu(x, y, &tx("{0} 中的文件", &[&title]), items)
        .await
        .as_deref()
    {
        Some("new-folder") => actions::new_folder(ctx).await,
        Some("add-link") => actions::add_link(ctx, None),
        Some("upload") => actions::upload(ctx, false).await,
        Some("paste") => actions::paste(ctx).await,
        Some("select-all") => toggle_all(ctx),
        Some("refresh") => reload(ctx),
        _ => {}
    }
}

fn activate(ctx: &Ctx, index: usize) {
    let Some(entry) = ctx.files.borrow().visible(index).cloned() else {
        return;
    };
    actions::open_entry(ctx, entry);
}

fn state_action(ctx: &Ctx, secondary: bool) {
    let (phase, filtered, mode, auth_error) = {
        let state = ctx.files.borrow();
        (
            state.phase,
            state.kind_filter.is_some() && !state.items.is_empty(),
            state.mode,
            state
                .error
                .as_ref()
                .is_some_and(|error| error.code == ErrorCode::AuthExpired),
        )
    };
    match (phase, secondary) {
        (Phase::SignedOut, _) => crate::auth::open_sign_in(ctx),
        (Phase::Error, true) if auth_error => crate::auth::open_sign_in(ctx),
        (Phase::Error, _) => {
            let ctx = ctx.clone();
            ctx.clone().spawn(async move { load(&ctx, 0, None).await });
        }
        (_, false) if filtered => {
            ctx.files.borrow_mut().kind_filter = None;
            rebuild(ctx);
        }
        (_, false) if mode == Mode::Files => {
            let ctx = ctx.clone();
            ctx.clone()
                .spawn(async move { actions::new_folder(&ctx).await });
        }
        (_, true) if mode == Mode::Files => actions::add_link(ctx, None),
        _ => {}
    }
}

/// Keeps the focused row inside the viewport.
fn scroll_to(ctx: &Ctx, index: usize) {
    let files = ctx.ui.global::<Files>();
    let (grid_view, compact, grid_size) = {
        let prefs = ctx.prefs.borrow();
        (
            prefs.account.view == "grid",
            prefs.account.density == "compact",
            prefs.account.grid_size.clone(),
        )
    };
    let columns = ctx.files.borrow().columns.max(1);
    let (row, height) = if grid_view {
        let cell = match grid_size.as_str() {
            "s" => 132.0,
            "l" => 196.0,
            _ => 160.0,
        };
        ((index / columns) as f32, cell + 4.0)
    } else {
        (index as f32, if compact { 36.0 } else { 44.0 })
    };
    let top = row * height;
    let visible = files.get_visible_height().max(height);
    let offset = -files.get_viewport_y();
    if top < offset {
        files.set_viewport_y(-top);
    } else if top + height > offset + visible {
        files.set_viewport_y(-(top + height - visible));
    }
}

fn move_focus(ctx: &Ctx, target: usize, shift: bool, ctrl: bool) {
    let selection = {
        let mut state = ctx.files.borrow_mut();
        if state.order.is_empty() {
            return;
        }
        let target = target.min(state.order.len() - 1);
        let mut selection = state.selected.clone();
        if shift {
            let (low, high) = if state.anchor <= target {
                (state.anchor, target)
            } else {
                (target, state.anchor)
            };
            selection.clear();
            for visible in low..=high {
                if let Some(entry) = state.visible(visible) {
                    selection.insert(entry.id.clone());
                }
            }
        } else if !ctrl {
            selection.clear();
            if let Some(entry) = state.visible(target) {
                selection.insert(entry.id.clone());
            }
            state.anchor = target;
        }
        state.focus = Some(target);
        selection
    };
    let target = ctx.files.borrow().focus.unwrap_or(0);
    set_selection(ctx, selection, Some(target));
    scroll_to(ctx, target);
}

/// Keyboard handling for the focused list. Returns whether the key was used.
pub fn key(ctx: &Ctx, text: &str, ctrl: bool, shift: bool, alt: bool) -> bool {
    use slint::platform::Key;
    let is = |key: Key| text == slint::SharedString::from(key).as_str();
    let (count, focus, columns, mode) = {
        let state = ctx.files.borrow();
        (
            state.order.len(),
            state.focus,
            state.columns.max(1),
            state.mode,
        )
    };
    let grid_view = ctx.prefs.borrow().account.view == "grid";
    let step = if grid_view { columns } else { 1 };
    let page = {
        let files = ctx.ui.global::<Files>();
        let row = if grid_view {
            164.0
        } else if ctx.prefs.borrow().account.density == "compact" {
            36.0
        } else {
            44.0
        };
        ((files.get_visible_height() / row).floor().max(1.0) as usize) * step
    };
    let current = focus.unwrap_or(0);
    let selected = || ctx.files.borrow().selected_entries();
    let spawn = |id: &'static str| {
        let entries = selected();
        if entries.is_empty() {
            return;
        }
        let ctx = ctx.clone();
        ctx.clone()
            .spawn(async move { actions::run(&ctx, id, entries, 0.0, 0.0).await });
    };
    if is(Key::DownArrow) {
        move_focus(
            ctx,
            if focus.is_none() { 0 } else { current + step },
            shift,
            ctrl,
        );
    } else if is(Key::UpArrow) {
        move_focus(ctx, current.saturating_sub(step), shift, ctrl);
    } else if is(Key::RightArrow) && grid_view {
        move_focus(ctx, current + 1, shift, ctrl);
    } else if is(Key::LeftArrow) && grid_view {
        move_focus(ctx, current.saturating_sub(1), shift, ctrl);
    } else if is(Key::Home) {
        move_focus(ctx, 0, shift, ctrl);
    } else if is(Key::End) {
        move_focus(ctx, count.saturating_sub(1), shift, ctrl);
    } else if is(Key::PageDown) {
        move_focus(ctx, current + page, shift, ctrl);
    } else if is(Key::PageUp) {
        move_focus(ctx, current.saturating_sub(page), shift, ctrl);
    } else if is(Key::Return) && alt {
        spawn("props");
    } else if is(Key::Return) {
        if let Some(index) = focus {
            activate(ctx, index);
        }
    } else if is(Key::Backspace) {
        go_up(ctx);
    } else if is(Key::Escape) {
        if ctx.files.borrow().selected.is_empty() {
            return false;
        }
        set_selection(ctx, HashSet::new(), None);
    } else if text == " " && ctrl {
        if let Some(index) = focus {
            row_clicked(ctx, index, true, false);
        }
    } else if is(Key::Delete) {
        spawn(if mode == Mode::Trash {
            "delete-forever"
        } else {
            "trash"
        });
    } else if is(Key::F2) {
        spawn("rename");
    } else if is(Key::Menu) || (is(Key::F10) && shift) {
        if let Some(index) = focus {
            let ctx = ctx.clone();
            ctx.clone()
                .spawn(async move { row_context(&ctx, index, 320.0, 220.0).await });
        }
    } else if ctrl && !alt {
        match text.to_lowercase().as_str() {
            "a" => toggle_all(ctx),
            "c" if shift => spawn("copy-path"),
            "c" => spawn("clip-copy"),
            "x" => spawn("clip-cut"),
            "d" => spawn("star-toggle"),
            "n" if shift => {
                let ctx = ctx.clone();
                ctx.clone()
                    .spawn(async move { actions::new_folder(&ctx).await });
            }
            "=" | "+" => zoom(ctx, 1),
            "-" => zoom(ctx, -1),
            _ => return false,
        }
    } else if !ctrl && !alt && text.chars().count() == 1 && text.chars().all(|c| !c.is_control()) {
        type_ahead(ctx, text);
    } else {
        return false;
    }
    true
}

/// Typing a few letters jumps to the next entry whose name starts with them.
fn type_ahead(ctx: &Ctx, text: &str) {
    let target = {
        let mut state = ctx.files.borrow_mut();
        let now = std::time::Instant::now();
        if state
            .type_ahead_at
            .is_none_or(|at| now.duration_since(at).as_millis() > 900)
        {
            state.type_ahead.clear();
        }
        state.type_ahead_at = Some(now);
        state.type_ahead.push_str(&text.to_lowercase());
        let needle = state.type_ahead.clone();
        let count = state.order.len();
        let start = state
            .focus
            .map(|focus| {
                if needle.chars().count() == 1 {
                    focus + 1
                } else {
                    focus
                }
            })
            .unwrap_or(0);
        (0..count)
            .map(|offset| (start + offset) % count.max(1))
            .find(|index| {
                state
                    .visible(*index)
                    .is_some_and(|entry| entry.name.to_lowercase().starts_with(&needle))
            })
    };
    if let Some(index) = target {
        move_focus(ctx, index, false, false);
    }
}

impl App {
    /// Marks the remote tree as changed and refreshes the visible listing.
    pub fn tree_changed(self: &Rc<Self>) {
        self.core.borrow_mut().epoch += 1;
        reload(self);
    }
}
