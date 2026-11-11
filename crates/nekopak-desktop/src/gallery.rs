//! Media library: the pictures or videos of one folder as a thumbnail wall.
//! Thumbnails are decoded for the visible rows only, a few at a time, and
//! kept in a bounded cache.
use crate::{
    app::{error_text, request, Ctx},
    files,
    i18n::{tr, tx},
    names,
    nav::{Route, ROOT_ID},
    overlay::MenuItem,
    preview, Gallery, GalleryRow, GalleryTile,
};
use nekopak_domain::*;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
};

const PAGE: u32 = 500;
/// Thumbnails being fetched at once; each one downloads a whole picture.
const WORKERS: usize = 3;
const THUMB_SIDE: u32 = 256;
/// Decoded thumbnails kept in memory (about a quarter megabyte each).
const CACHE: usize = 160;
/// Tile height including caption and gap; must match `gallery.slint`.
const ROW_HEIGHT: f32 = 172.0 + 34.0 + 10.0;

#[derive(Default)]
pub struct GalleryState {
    folder: String,
    mode: String,
    token: u64,
    /// Everything listed so far in the folder (all kinds).
    entries: Vec<RemoteEntry>,
    next: Option<String>,
    loading_more: bool,
    /// Entries of the active kind, in display order.
    shown: Vec<RemoteEntry>,
    columns: usize,
    rows: Vec<Rc<VecModel<GalleryTile>>>,
    thumbs: HashMap<String, slint::Image>,
    thumb_order: VecDeque<String>,
    failed: HashSet<String>,
    pending: HashSet<String>,
    queue: VecDeque<String>,
    running: usize,
    scroll_scheduled: bool,
}

fn kind_of(entry: &RemoteEntry) -> i32 {
    names::file_kind(&entry.name, entry.kind, entry.mime_type.as_deref())
}

fn is_local(folder: &str) -> bool {
    folder.starts_with("local:")
}

async fn fetch(
    ctx: &Ctx,
    folder: String,
    cursor: Option<String>,
) -> Result<Page<RemoteEntry>, ClientError> {
    ctx.call(move |service| async move {
        if is_local(&folder) {
            service.local_list(&folder, cursor.as_deref(), Some(PAGE))
        } else {
            service
                .files_list(&folder, cursor.as_deref(), Some(PAGE), &request())
                .await
        }
    })
    .await
}

pub fn enter(ctx: &Ctx, route: &Route) {
    let Route::Gallery { folder, mode } = route else {
        return;
    };
    let mode = if mode == "videos" { "videos" } else { "images" };
    let same_folder = {
        let mut state = ctx.gallery.borrow_mut();
        let same = state.folder == *folder && !state.entries.is_empty();
        state.folder = folder.clone();
        state.mode = mode.to_owned();
        same
    };
    let gallery = ctx.ui.global::<Gallery>();
    gallery.set_mode(mode.into());
    gallery.set_viewport_y(0.0);
    if same_folder {
        // Switching between pictures and videos reuses the listing.
        rebuild(ctx);
        return;
    }
    reload(ctx);
}

pub fn reload(ctx: &Ctx) {
    let (token, folder) = {
        let mut state = ctx.gallery.borrow_mut();
        state.token += 1;
        state.entries.clear();
        state.shown.clear();
        state.rows.clear();
        state.next = None;
        state.loading_more = false;
        state.queue.clear();
        state.pending.clear();
        state.failed.clear();
        (state.token, state.folder.clone())
    };
    let gallery = ctx.ui.global::<Gallery>();
    gallery.set_phase("loading".into());
    gallery.set_rows(Default::default());
    gallery.set_count_label(Default::default());
    gallery.set_images_count(Default::default());
    gallery.set_videos_count(Default::default());
    gallery.set_has_more(false);
    gallery.set_loading_more(false);
    gallery.set_title(Default::default());
    if !is_local(&folder) && !ctx.signed_in() {
        gallery.set_state_title(tr("需要先登录").into());
        gallery.set_state_body(Default::default());
        gallery.set_state_error(false);
        gallery.set_phase("empty".into());
        return;
    }
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let session = ctx.session();
        // The folder name for the heading; the root has a fixed one.
        if folder == ROOT_ID {
            ctx.ui.global::<Gallery>().set_title(tr("网盘").into());
        } else {
            let ctx = ctx.clone();
            let folder = folder.clone();
            ctx.clone().spawn(async move {
                let name = ctx
                    .call({
                        let folder = folder.clone();
                        move |service| async move {
                            if is_local(&folder) {
                                service.local_stat(&folder)
                            } else {
                                service.files_stat(&folder, &request()).await
                            }
                        }
                    })
                    .await
                    .map(|entry| entry.name)
                    .unwrap_or_default();
                if ctx.gallery.borrow().token == token {
                    ctx.ui.global::<Gallery>().set_title(name.as_str().into());
                    if !name.is_empty() {
                        let route = ctx.route();
                        if matches!(route, Route::Gallery { .. }) {
                            ctx.set_tab_title(&route, &tx("媒体库 · {0}", &[&name]));
                        }
                    }
                }
            });
        }
        let result = fetch(&ctx, folder, None).await;
        if ctx.gallery.borrow().token != token || session != ctx.session() {
            return;
        }
        let gallery = ctx.ui.global::<Gallery>();
        match result {
            Ok(page) => {
                {
                    let mut state = ctx.gallery.borrow_mut();
                    state.entries = page.items;
                    state.next = page.next_cursor;
                }
                rebuild(&ctx);
            }
            Err(error) => {
                gallery.set_state_title(crate::app::error_title(&error).into());
                gallery.set_state_body(error_text(&error).into());
                gallery.set_state_error(true);
                gallery.set_phase("error".into());
            }
        }
    });
}

fn load_more(ctx: &Ctx) {
    let (token, folder, cursor) = {
        let mut state = ctx.gallery.borrow_mut();
        if state.loading_more {
            return;
        }
        let Some(cursor) = state.next.clone() else {
            return;
        };
        state.loading_more = true;
        (state.token, state.folder.clone(), cursor)
    };
    ctx.ui.global::<Gallery>().set_loading_more(true);
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let session = ctx.session();
        let result = fetch(&ctx, folder, Some(cursor)).await;
        if ctx.gallery.borrow().token != token || session != ctx.session() {
            return;
        }
        {
            let mut state = ctx.gallery.borrow_mut();
            state.loading_more = false;
            match result {
                Ok(page) => {
                    state.entries.extend(page.items);
                    state.next = page.next_cursor;
                }
                Err(_) => state.next = None,
            }
        }
        ctx.ui.global::<Gallery>().set_loading_more(false);
        let offset = ctx.ui.global::<Gallery>().get_viewport_y();
        rebuild(&ctx);
        ctx.ui.global::<Gallery>().set_viewport_y(offset);
    });
}

fn tile(state: &GalleryState, entry: &RemoteEntry) -> GalleryTile {
    let thumb = state.thumbs.get(&entry.id);
    GalleryTile {
        id: entry.id.as_str().into(),
        name: entry.name.as_str().into(),
        kind: kind_of(entry),
        detail: crate::format::bytes(entry.size_bytes.0).into(),
        thumb: thumb.cloned().unwrap_or_default(),
        loaded: thumb.is_some(),
        failed: state.failed.contains(&entry.id),
    }
}

/// Rebuilds the wall for the active kind and the current column count.
fn rebuild(ctx: &Ctx) {
    let gallery = ctx.ui.global::<Gallery>();
    let (rows, images, videos, shown, more) = {
        let mut state = ctx.gallery.borrow_mut();
        let wanted = if state.mode == "videos" {
            names::KIND_VIDEO
        } else {
            names::KIND_IMAGE
        };
        let (mut images, mut videos) = (0usize, 0usize);
        let mut shown = Vec::new();
        for entry in &state.entries {
            let kind = kind_of(entry);
            if kind == names::KIND_IMAGE {
                images += 1;
            } else if kind == names::KIND_VIDEO {
                videos += 1;
            }
            if kind == wanted {
                shown.push(entry.clone());
            }
        }
        state.shown = shown;
        let columns = state.columns.max(1);
        let models: Vec<Rc<VecModel<GalleryTile>>> = state
            .shown
            .chunks(columns)
            .map(|line| {
                Rc::new(VecModel::from(
                    line.iter()
                        .map(|entry| tile(&state, entry))
                        .collect::<Vec<_>>(),
                ))
            })
            .collect();
        let rows: Vec<GalleryRow> = models
            .iter()
            .enumerate()
            .map(|(index, model)| GalleryRow {
                tiles: ModelRc::from(model.clone()),
                first: (index * columns) as i32,
            })
            .collect();
        state.rows = models;
        state.queue.clear();
        (
            rows,
            images,
            videos,
            state.shown.len(),
            state.next.is_some(),
        )
    };
    gallery.set_images_count(images.to_string().into());
    gallery.set_videos_count(videos.to_string().into());
    gallery.set_has_more(more);
    gallery.set_count_label(if more {
        tx("已加载 {0} 项，还有更多", &[&shown.to_string()]).into()
    } else {
        tx("{0} 项", &[&shown.to_string()]).into()
    });
    if shown == 0 {
        gallery.set_rows(Default::default());
        gallery.set_state_error(false);
        gallery.set_state_title(tr("此文件夹中没有这种媒体").into());
        gallery.set_state_body(
            if more {
                tr("文件夹还没有读取完，继续读取可能会找到更多。")
            } else {
                String::new()
            }
            .into(),
        );
        gallery.set_phase("empty".into());
        if more {
            load_more(ctx);
        }
        return;
    }
    gallery.set_rows(ModelRc::new(VecModel::from(rows)));
    gallery.set_phase("ready".into());
    request_thumbs(ctx);
}

/// Queues thumbnails for the rows on screen (plus one row either side).
fn request_thumbs(ctx: &Ctx) {
    let gallery = ctx.ui.global::<Gallery>();
    let top = -gallery.get_viewport_y();
    let height = gallery.get_visible_height().max(ROW_HEIGHT);
    {
        let mut state = ctx.gallery.borrow_mut();
        if state.mode != "images" {
            return;
        }
        let columns = state.columns.max(1);
        let first_row = ((top / ROW_HEIGHT).floor() as usize).saturating_sub(1);
        let last_row = ((top + height) / ROW_HEIGHT).ceil() as usize + 1;
        let from = first_row * columns;
        let to = (last_row * columns).min(state.shown.len());
        let wanted: Vec<String> = state
            .shown
            .get(from..to)
            .unwrap_or_default()
            .iter()
            .filter(|entry| {
                preview::can_view_image(entry)
                    && entry.size_bytes.0 <= preview::THUMB_LIMIT
                    && !state.thumbs.contains_key(&entry.id)
                    && !state.failed.contains(&entry.id)
                    && !state.pending.contains(&entry.id)
            })
            .map(|entry| entry.id.clone())
            .collect();
        // Only what is visible now is worth fetching; drop stale requests.
        state.queue = wanted.into();
    }
    pump(ctx);
}

fn pump(ctx: &Ctx) {
    loop {
        let (id, token) = {
            let mut state = ctx.gallery.borrow_mut();
            if state.running >= WORKERS {
                return;
            }
            let Some(id) = state.queue.pop_front() else {
                return;
            };
            state.running += 1;
            state.pending.insert(id.clone());
            (id, state.token)
        };
        let ctx = ctx.clone();
        ctx.clone().spawn(async move {
            let session = ctx.session();
            let result =
                preview::load_picture(&ctx, id.clone(), preview::THUMB_LIMIT, THUMB_SIDE, true)
                    .await;
            {
                let mut state = ctx.gallery.borrow_mut();
                state.running = state.running.saturating_sub(1);
                state.pending.remove(&id);
                if state.token == token && session == ctx.session() {
                    match result {
                        Ok(decoded) => {
                            let image = slint::Image::from_rgba8(decoded.pixels);
                            state.thumbs.insert(id.clone(), image);
                            state.thumb_order.push_back(id.clone());
                            while state.thumb_order.len() > CACHE {
                                if let Some(old) = state.thumb_order.pop_front() {
                                    state.thumbs.remove(&old);
                                }
                            }
                        }
                        Err(_) => {
                            state.failed.insert(id.clone());
                        }
                    }
                    // Update just this tile.
                    let columns = state.columns.max(1);
                    if let Some(at) = state.shown.iter().position(|entry| entry.id == id) {
                        let fresh = tile(&state, &state.shown[at]);
                        if let Some(model) = state.rows.get(at / columns) {
                            if at % columns < model.row_count() {
                                model.set_row_data(at % columns, fresh);
                            }
                        }
                    }
                }
            }
            pump(&ctx);
        });
    }
}

fn open(ctx: &Ctx, index: usize) {
    let (entry, siblings) = {
        let state = ctx.gallery.borrow();
        let Some(entry) = state.shown.get(index).cloned() else {
            return;
        };
        (entry, state.shown.clone())
    };
    activate(ctx, entry, siblings);
}

/// Default action for a media file, shared with the file list.
pub fn activate(ctx: &Ctx, entry: RemoteEntry, siblings: Vec<RemoteEntry>) {
    if preview::can_view_image(&entry) {
        preview::open_image(ctx, &entry, siblings);
    } else if kind_of(&entry) == names::KIND_VIDEO {
        preview::open_video(ctx, &entry);
    } else {
        let ctx = ctx.clone();
        ctx.clone()
            .spawn(async move { crate::actions::properties(&ctx, entry).await });
    }
}

async fn context(ctx: &Ctx, index: usize, x: f32, y: f32) {
    let Some(entry) = ctx.gallery.borrow().shown.get(index).cloned() else {
        return;
    };
    let drive = files::source_of(&entry.id) == "drive";
    let video = kind_of(&entry) == names::KIND_VIDEO;
    let mut items = vec![MenuItem::new("view", tr("预览")).icon("visibility")];
    if video && ctx.feature("media_external") {
        items.push(MenuItem::new("play", tr("用外部播放器打开")).icon("play-circle"));
    }
    if drive {
        items.push(
            MenuItem::new("download", tr("下载"))
                .icon("download")
                .gate(ctx.gate(Operation::Download)),
        );
    }
    items.push(MenuItem::new("locate", tr("在文件夹中显示")).icon("folder-open"));
    items.push(MenuItem::separator());
    items.push(MenuItem::new("props", tr("属性")).icon("info"));
    match ctx.menu(x, y, &entry.name, items).await.as_deref() {
        Some("view") => open(ctx, index),
        Some("play") => crate::media::play(ctx, entry).await,
        Some("download") => crate::actions::download(ctx, vec![entry]).await,
        Some("locate") => {
            let folder = ctx.gallery.borrow().folder.clone();
            ctx.navigate(Route::Files { folder });
        }
        Some("props") => crate::actions::properties(ctx, entry).await,
        _ => {}
    }
}

pub fn install(ctx: &Ctx) {
    let gallery = ctx.ui.global::<Gallery>();
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
    gallery.on_back(handler!(|ctx| {
        let folder = ctx.gallery.borrow().folder.clone();
        if ctx.can_go(-1) {
            ctx.navigate_history(-1);
        } else {
            ctx.navigate(Route::Files { folder });
        }
    }));
    gallery.on_set_mode(handler!(|ctx, mode| {
        let folder = ctx.gallery.borrow().folder.clone();
        ctx.replace_route(Route::Gallery {
            folder,
            mode: mode.to_string(),
        });
    }));
    gallery.on_open(handler!(|ctx, index| open(&ctx, index.max(0) as usize)));
    gallery.on_context(handler!(|ctx, index, x, y| {
        let task = ctx.clone();
        ctx.spawn(async move { context(&task, index.max(0) as usize, x, y).await });
    }));
    gallery.on_columns_changed(handler!(|ctx, columns| {
        let columns = columns.max(1) as usize;
        let changed = {
            let mut state = ctx.gallery.borrow_mut();
            let changed = state.columns != columns;
            state.columns = columns;
            changed && !state.shown.is_empty()
        };
        if changed {
            rebuild(&ctx);
        }
    }));
    gallery.on_scrolled(handler!(|ctx| {
        // Scrolling fires continuously; look at what is visible once it settles.
        if std::mem::replace(&mut ctx.gallery.borrow_mut().scroll_scheduled, true) {
            return;
        }
        let ctx = ctx.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(90), move || {
            ctx.gallery.borrow_mut().scroll_scheduled = false;
            request_thumbs(&ctx);
        });
    }));
    gallery.on_near_end(handler!(|ctx| load_more(&ctx)));
    gallery.on_retry(handler!(|ctx| reload(&ctx)));
    gallery.on_refresh(handler!(|ctx| reload(&ctx)));
}

/// Drops everything tied to the previous account.
pub fn clear_session(ctx: &Ctx) {
    let mut state = ctx.gallery.borrow_mut();
    state.token += 1;
    state.entries.clear();
    state.shown.clear();
    state.rows.clear();
    state.thumbs.clear();
    state.thumb_order.clear();
    state.failed.clear();
    state.queue.clear();
    state.pending.clear();
    state.folder.clear();
}
