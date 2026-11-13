//! In-app viewer: pictures (decoded here, with zoom and previous/next) and a
//! video sheet with a frame preview where the platform can decode one.
use crate::{
    app::{error_text, App, Ctx},
    files, format,
    frames::{self, Event as FrameEvent},
    i18n::{tr, tx},
    names, Viewer,
};
use nekopak_domain::*;
use nekopak_provider::RequestContext;
use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};
use std::{rc::Rc, time::Duration};

/// Largest picture file fetched for the viewer.
const VIEW_LIMIT: u64 = 64 * 1024 * 1024;
/// Largest picture file fetched just to draw a thumbnail.
pub const THUMB_LIMIT: u64 = 24 * 1024 * 1024;
/// Longest side kept in memory for the viewer.
const VIEW_SIDE: u32 = 4096;

pub struct Decoded {
    pub pixels: SharedPixelBuffer<Rgba8Pixel>,
    /// Size of the original picture, before any downscaling.
    pub width: u32,
    pub height: u32,
}

struct Video {
    session: Option<frames::Session>,
    /// Loopback media session to close with the viewer.
    stream: Option<String>,
    duration_ms: u64,
    playing: bool,
    /// A scrub in progress: frames from before it are not shown.
    dragging: bool,
    /// Media time of the frame on screen.
    at_ms: u64,
    /// Clip marks chosen on the preview.
    clip_from: Option<u64>,
    clip_to: Option<u64>,
}

#[derive(Default)]
pub struct ViewerState {
    items: Vec<RemoteEntry>,
    at: usize,
    token: u64,
    /// Recently shown pictures, so stepping back and forth is instant.
    cache: Vec<(String, slint::Image, (u32, u32))>,
    video: Option<Video>,
}

/// Formats this build can decode itself.
pub fn can_view_image(entry: &RemoteEntry) -> bool {
    if entry.kind != EntryKind::File {
        return false;
    }
    let extension = names::split_name(&entry.name)
        .1
        .trim_start_matches('.')
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
    ) || matches!(
        entry.mime_type.as_deref(),
        Some("image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/bmp")
    )
}

/// Decodes a picture off the UI thread, honouring EXIF orientation and
/// bounding both the decoder's memory and the size kept afterwards.
pub fn decode(bytes: &[u8], max_side: u32, thumbnail: bool) -> Result<Decoded, ClientError> {
    use image::ImageDecoder;
    let failed = || {
        ClientError::new(
            ErrorCode::Unsupported,
            "无法解码这张图片，文件可能已损坏或格式不受支持。",
        )
    };
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| failed())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(24_000);
    limits.max_image_height = Some(24_000);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| failed())?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = image::DynamicImage::from_decoder(decoder).map_err(|_| failed())?;
    picture.apply_orientation(orientation);
    let (width, height) = (picture.width(), picture.height());
    if width == 0 || height == 0 {
        return Err(failed());
    }
    if width.max(height) > max_side {
        picture = if thumbnail {
            picture.thumbnail(max_side, max_side)
        } else {
            picture.resize(max_side, max_side, image::imageops::FilterType::Triangle)
        };
    }
    let rgba = picture.into_rgba8();
    Ok(Decoded {
        pixels: SharedPixelBuffer::clone_from_slice(rgba.as_raw(), rgba.width(), rgba.height()),
        width,
        height,
    })
}

/// Fetches and decodes one picture on the runtime; nothing touches the UI thread.
pub async fn load_picture(
    ctx: &Ctx,
    id: String,
    limit: u64,
    max_side: u32,
    thumbnail: bool,
) -> Result<Decoded, ClientError> {
    ctx.call(move |service| async move {
        let bytes = service
            .media_bytes(&id, limit, &RequestContext::new(Duration::from_secs(90)))
            .await?;
        tokio::task::spawn_blocking(move || decode(&bytes, max_side, thumbnail))
            .await
            .map_err(|_| ClientError::new(ErrorCode::Internal, "图片解码意外中止。"))?
    })
    .await
}

fn subtitle(ctx: &Ctx, extra: &str) -> String {
    let state = ctx.viewer.borrow();
    let Some(entry) = state.items.get(state.at) else {
        return String::new();
    };
    let mut parts = Vec::new();
    if state.items.len() > 1 {
        parts.push(format!("{} / {}", state.at + 1, state.items.len()));
    }
    if !extra.is_empty() {
        parts.push(extra.to_owned());
    }
    parts.push(format::bytes(entry.size_bytes.0));
    parts.join(" · ")
}

fn current(ctx: &Ctx) -> Option<RemoteEntry> {
    let state = ctx.viewer.borrow();
    state.items.get(state.at).cloned()
}

/// Shows the picture at the current index.
fn show_picture(ctx: &Ctx) {
    let viewer = ctx.ui.global::<Viewer>();
    let (entry, token, count, at, cached) = {
        let mut state = ctx.viewer.borrow_mut();
        state.token += 1;
        let Some(entry) = state.items.get(state.at).cloned() else {
            return;
        };
        let cached = state
            .cache
            .iter()
            .find(|(id, ..)| *id == entry.id)
            .map(|(_, image, size)| (image.clone(), *size));
        (entry, state.token, state.items.len(), state.at, cached)
    };
    viewer.set_kind("image".into());
    viewer.set_title(entry.name.as_str().into());
    viewer.set_can_prev(at > 0);
    viewer.set_can_next(at + 1 < count);
    viewer.set_can_frames(false);
    viewer.set_can_external(false);
    viewer.set_can_download(files::source_of(&entry.id) == "drive" && ctx.can(Operation::Download));
    viewer.set_zoom(1.0);
    viewer.set_note(Default::default());
    if let Some((image, (width, height))) = cached {
        viewer.set_picture(image);
        viewer.set_subtitle(subtitle(ctx, &format!("{width}×{height}")).into());
        viewer.set_phase("ready".into());
        return;
    }
    viewer.set_subtitle(subtitle(ctx, "").into());
    viewer.set_phase("loading".into());
    if entry.size_bytes.0 > VIEW_LIMIT {
        viewer.set_note(
            tx(
                "图片大于 {0}，请下载后查看。",
                &[&format::bytes(VIEW_LIMIT)],
            )
            .into(),
        );
        viewer.set_phase("error".into());
        return;
    }
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let session = ctx.session();
        let result = load_picture(&ctx, entry.id.clone(), VIEW_LIMIT, VIEW_SIDE, false).await;
        if ctx.viewer.borrow().token != token || session != ctx.session() || !ctx.viewer_open() {
            return;
        }
        let viewer = ctx.ui.global::<Viewer>();
        match result {
            Ok(decoded) => {
                let size = (decoded.width, decoded.height);
                let image = slint::Image::from_rgba8(decoded.pixels);
                {
                    let mut state = ctx.viewer.borrow_mut();
                    state.cache.retain(|(id, ..)| *id != entry.id);
                    state.cache.push((entry.id.clone(), image.clone(), size));
                    if state.cache.len() > 4 {
                        state.cache.remove(0);
                    }
                }
                viewer.set_picture(image);
                viewer.set_subtitle(subtitle(&ctx, &format!("{}×{}", size.0, size.1)).into());
                viewer.set_phase("ready".into());
            }
            Err(error) => {
                viewer.set_note(error_text(&error).into());
                viewer.set_phase("error".into());
            }
        }
    });
}

/// Opens the viewer on `entry`; `siblings` are the pictures it can step through.
pub fn open_image(ctx: &Ctx, entry: &RemoteEntry, siblings: Vec<RemoteEntry>) {
    if files::source_of(&entry.id) == "drive" {
        if let Err(reason) = ctx
            .gate(Operation::StreamUrl)
            .or_else(|_| ctx.gate(Operation::Download))
        {
            return ctx.toast(reason, "info");
        }
    }
    close_video(ctx);
    {
        let mut state = ctx.viewer.borrow_mut();
        let mut items: Vec<RemoteEntry> = siblings
            .into_iter()
            .filter(|item| can_view_image(item) || item.id == entry.id)
            .collect();
        if !items.iter().any(|item| item.id == entry.id) {
            items = vec![entry.clone()];
        }
        state.at = items
            .iter()
            .position(|item| item.id == entry.id)
            .unwrap_or(0);
        state.items = items;
    }
    ctx.finish_menu(None);
    let viewer = ctx.ui.global::<Viewer>();
    viewer.set_picture(Default::default());
    viewer.set_open(true);
    show_picture(ctx);
}

fn video_label(probe: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    if let Some(duration) = probe["duration_ms"].as_u64().filter(|ms| *ms > 0) {
        parts.push(format::media_time(duration));
    }
    if let (Some(width), Some(height)) = (probe["width"].as_u64(), probe["height"].as_u64()) {
        parts.push(format!("{width}×{height}"));
    }
    if let Some(codec) = probe["video_codec"]
        .as_str()
        .filter(|codec| !codec.is_empty())
    {
        parts.push(codec.to_uppercase());
    }
    parts.join(" · ")
}

fn push_time(ctx: &Ctx, at_ms: u64) {
    let viewer = ctx.ui.global::<Viewer>();
    let duration = ctx
        .viewer
        .borrow()
        .video
        .as_ref()
        .map(|video| video.duration_ms)
        .unwrap_or(0);
    viewer.set_time_label(format::media_time(at_ms).into());
    viewer.set_position(if duration == 0 {
        0.0
    } else {
        (at_ms as f32 / duration as f32).clamp(0.0, 1.0)
    });
}

/// Stops any frame preview and closes its loopback session.
fn close_video(ctx: &Ctx) {
    let video = ctx.viewer.borrow_mut().video.take();
    if let Some(video) = video {
        drop(video.session);
        if let Some(stream) = video.stream {
            let service = ctx.svc.clone();
            let _ = service.media_session_close(&stream);
        }
    }
    ctx.ui.global::<Viewer>().set_playing(false);
}

/// Opens the viewer on a video: details first, then a frame preview when the
/// platform has a decoder. Playback with sound stays with the external player.
pub fn open_video(ctx: &Ctx, entry: &RemoteEntry) {
    if !entry.id.starts_with("local:") {
        if let Err(reason) = ctx
            .gate(Operation::StreamUrl)
            .or_else(|_| ctx.gate(Operation::ExternalPlayer))
        {
            return ctx.toast(reason, "info");
        }
    }
    close_video(ctx);
    let token = {
        let mut state = ctx.viewer.borrow_mut();
        state.items = vec![entry.clone()];
        state.at = 0;
        state.token += 1;
        state.token
    };
    ctx.finish_menu(None);
    let viewer = ctx.ui.global::<Viewer>();
    viewer.set_kind("video".into());
    viewer.set_title(entry.name.as_str().into());
    viewer.set_subtitle(subtitle(ctx, "").into());
    viewer.set_picture(Default::default());
    viewer.set_can_prev(false);
    viewer.set_can_next(false);
    viewer.set_can_frames(false);
    viewer.set_playing(false);
    viewer.set_position(0.0);
    viewer.set_time_label("00:00".into());
    viewer.set_duration_label(Default::default());
    viewer.set_zoom(1.0);
    viewer.set_clip_from(-1.0);
    viewer.set_clip_to(-1.0);
    viewer.set_clip_from_label(Default::default());
    viewer.set_clip_to_label(Default::default());
    // Headless UI tests exercise the marks with synthetic frames.
    let scripted =
        cfg!(feature = "automation") && std::env::var_os("NEKOPAK_FAKE_FRAMES").is_some();
    viewer.set_can_clip(
        scripted || entry.id.starts_with("local:") || ctx.can(Operation::ClipDownload),
    );
    viewer
        .set_can_external(ctx.feature("media_external") && files::source_of(&entry.id) != "shared");
    viewer.set_can_download(files::source_of(&entry.id) == "drive" && ctx.can(Operation::Download));
    viewer.set_note(tr("正在读取视频信息…").into());
    viewer.set_phase("loading".into());
    viewer.set_open(true);
    let entry = entry.clone();
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let session = ctx.session();
        let stale = |ctx: &Ctx| {
            ctx.viewer.borrow().token != token || session != ctx.session() || !ctx.viewer_open()
        };
        let viewer = ctx.ui.global::<Viewer>();
        // Container details come from the built-in parser and need no decoder.
        let probe = ctx
            .call({
                let id = entry.id.clone();
                move |service| async move {
                    service
                        .media_probe(&id, &RequestContext::new(Duration::from_secs(90)))
                        .await
                }
            })
            .await;
        if stale(&ctx) {
            return;
        }
        let details = probe.as_ref().map(video_label).unwrap_or_default();
        viewer.set_subtitle(subtitle(&ctx, &details).into());
        if !frames::available() {
            viewer.set_note(
                tr("这个平台没有内置的视频解码器。可以用播放器打开，或下载后观看。").into(),
            );
            viewer.set_phase("info".into());
            return;
        }
        viewer.set_note(tr("正在准备画面预览…").into());
        let opened = ctx
            .call({
                let id = entry.id.clone();
                move |service| async move { service.media_session_open(&id, None).await }
            })
            .await;
        if stale(&ctx) {
            if let Ok(value) = &opened {
                if let Some(id) = value["session_id"].as_str() {
                    let _ = ctx.svc.media_session_close(id);
                }
            }
            return;
        }
        let (url, stream) = match &opened {
            Ok(value) => match (value["url"].as_str(), value["session_id"].as_str()) {
                (Some(url), Some(stream)) if url.starts_with("http://127.0.0.1:") => {
                    (url.to_owned(), stream.to_owned())
                }
                _ => {
                    viewer.set_note(tr("媒体会话暂不可用。").into());
                    viewer.set_phase("error".into());
                    return;
                }
            },
            Err(error) => {
                viewer.set_note(error_text(error).into());
                viewer.set_phase("error".into());
                return;
            }
        };
        let (events, mut inbox) = tokio::sync::mpsc::unbounded_channel();
        let preview = frames::open(url, events);
        ctx.viewer.borrow_mut().video = Some(Video {
            session: Some(preview),
            stream: Some(stream),
            duration_ms: probe
                .as_ref()
                .ok()
                .and_then(|value| value["duration_ms"].as_u64())
                .unwrap_or(0),
            playing: false,
            dragging: false,
            at_ms: 0,
            clip_from: None,
            clip_to: None,
        });
        while let Some(event) = inbox.recv().await {
            if stale(&ctx) {
                return;
            }
            match event {
                FrameEvent::Opened(info) => {
                    let duration = {
                        let mut state = ctx.viewer.borrow_mut();
                        let Some(video) = state.video.as_mut() else {
                            return;
                        };
                        if info.duration_ms > 0 {
                            video.duration_ms = info.duration_ms;
                        }
                        video.duration_ms
                    };
                    viewer.set_duration_label(format::media_time(duration).into());
                }
                FrameEvent::Frame(frame) => {
                    let at = frame.at_ms;
                    viewer.set_picture(slint::Image::from_rgba8(frame.pixels));
                    if viewer.get_phase() != "ready" {
                        viewer.set_can_frames(true);
                        viewer.set_phase("ready".into());
                    }
                    let dragging = {
                        let mut state = ctx.viewer.borrow_mut();
                        state.video.as_mut().is_some_and(|video| {
                            video.at_ms = at;
                            video.dragging
                        })
                    };
                    if !dragging {
                        push_time(&ctx, at);
                    }
                }
                FrameEvent::Ended => {
                    if let Some(video) = ctx.viewer.borrow_mut().video.as_mut() {
                        video.playing = false;
                    }
                    viewer.set_playing(false);
                }
                FrameEvent::Failed(reason) => {
                    eprintln!("NekoPak video preview unavailable: {reason}");
                    // The decoder cannot handle this stream (codec, container
                    // or connection): keep the details and offer the player.
                    close_video(&ctx);
                    viewer.set_can_frames(false);
                    viewer.set_note(
                        tr("系统解码器无法预览这个视频。可以用播放器打开，或下载后观看。").into(),
                    );
                    viewer.set_phase("info".into());
                    return;
                }
            }
        }
    });
}

fn step(ctx: &Ctx, delta: i32) {
    let moved = {
        let mut state = ctx.viewer.borrow_mut();
        let next = state.at as i32 + delta;
        if state.video.is_none() && next >= 0 && (next as usize) < state.items.len() {
            state.at = next as usize;
            true
        } else {
            false
        }
    };
    if moved {
        show_picture(ctx);
    }
}

fn toggle_play(ctx: &Ctx) {
    let playing = {
        let mut state = ctx.viewer.borrow_mut();
        let Some(video) = state.video.as_mut() else {
            return;
        };
        let Some(session) = video.session.as_ref() else {
            return;
        };
        video.playing = !video.playing;
        if video.playing {
            session.play();
        } else {
            session.pause();
        }
        video.playing
    };
    ctx.ui.global::<Viewer>().set_playing(playing);
}

fn seek(ctx: &Ctx, position: f32, released: bool) {
    let target = {
        let mut state = ctx.viewer.borrow_mut();
        let Some(video) = state.video.as_mut() else {
            return;
        };
        let Some(session) = video.session.as_ref() else {
            return;
        };
        let target = (f64::from(position.clamp(0.0, 1.0)) * video.duration_ms as f64) as u64;
        session.seek(target, released);
        video.dragging = !released;
        target
    };
    let viewer = ctx.ui.global::<Viewer>();
    viewer.set_time_label(format::media_time(target).into());
    viewer.set_position(position.clamp(0.0, 1.0));
}

/// Marks the frame on screen as the start or the end of a clip.
fn mark(ctx: &Ctx, start: bool) {
    let viewer = ctx.ui.global::<Viewer>();
    let mut state = ctx.viewer.borrow_mut();
    let Some(video) = state.video.as_mut() else {
        return;
    };
    if video.duration_ms == 0 {
        return;
    }
    let at = video.at_ms.min(video.duration_ms);
    if start {
        video.clip_from = Some(at);
        // A start after the end invalidates the end.
        if video.clip_to.is_some_and(|end| end <= at) {
            video.clip_to = None;
        }
    } else {
        video.clip_to = Some(at);
        if video.clip_from.is_some_and(|from| from >= at) {
            video.clip_from = None;
        }
    }
    let duration = video.duration_ms as f32;
    let fraction = |value: Option<u64>| value.map(|ms| ms as f32 / duration).unwrap_or(-1.0);
    let label = |value: Option<u64>| value.map(format::media_time).unwrap_or_default();
    viewer.set_clip_from(fraction(video.clip_from));
    viewer.set_clip_to(fraction(video.clip_to));
    viewer.set_clip_from_label(label(video.clip_from).into());
    viewer.set_clip_to_label(label(video.clip_to).into());
}

/// Hands the marked range to the clip download dialog.
fn clip(ctx: &Ctx) {
    let range = {
        let state = ctx.viewer.borrow();
        state
            .video
            .as_ref()
            .and_then(|video| Some((video.clip_from?, video.clip_to?)))
    };
    let (Some(range), Some(entry)) = (range, current(ctx)) else {
        return;
    };
    ctx.close_viewer();
    let task = ctx.clone();
    ctx.spawn(async move { crate::media::clip_dialog_with(&task, entry, Some(range)).await });
}

/// Moves the video by a number of seconds (arrow keys).
fn nudge(ctx: &Ctx, seconds: i64) {
    let (position, duration) = {
        let state = ctx.viewer.borrow();
        let Some(video) = state.video.as_ref() else {
            return;
        };
        (ctx.ui.global::<Viewer>().get_position(), video.duration_ms)
    };
    if duration == 0 {
        return;
    }
    let at = (f64::from(position) * duration as f64) as i64 + seconds * 1000;
    seek(
        ctx,
        (at.clamp(0, duration as i64) as f64 / duration as f64) as f32,
        true,
    );
}

pub fn key(ctx: &Ctx, text: &str, ctrl: bool) -> bool {
    use slint::platform::Key;
    let is = |key: Key| text == slint::SharedString::from(key).as_str();
    let viewer = ctx.ui.global::<Viewer>();
    let video = ctx.viewer.borrow().video.is_some();
    if is(Key::Escape) {
        ctx.close_viewer();
    } else if is(Key::LeftArrow) || is(Key::PageUp) {
        if video {
            nudge(ctx, -5);
        } else {
            step(ctx, -1);
        }
    } else if is(Key::RightArrow) || is(Key::PageDown) {
        if video {
            nudge(ctx, 5);
        } else {
            step(ctx, 1);
        }
    } else if text == " " {
        if video {
            toggle_play(ctx);
        } else {
            step(ctx, 1);
        }
    } else if is(Key::Home) && !video {
        let at = ctx.viewer.borrow().at as i32;
        step(ctx, -at);
    } else if is(Key::End) && !video {
        let (at, count) = {
            let state = ctx.viewer.borrow();
            (state.at as i32, state.items.len() as i32)
        };
        step(ctx, count - 1 - at);
    } else if video && text.eq_ignore_ascii_case("i") {
        mark(ctx, true);
    } else if video && text.eq_ignore_ascii_case("o") {
        mark(ctx, false);
    } else if (text == "+" || text == "=") && !video {
        viewer.set_zoom((viewer.get_zoom() * 1.5).min(8.0));
    } else if text == "-" && !video {
        viewer.set_zoom((viewer.get_zoom() / 1.5).max(1.0));
    } else if text == "0" && !video {
        viewer.set_zoom(1.0);
    } else {
        // Everything else is swallowed: the page behind must not react.
        let _ = ctrl;
    }
    true
}

pub fn install(ctx: &Ctx) {
    let viewer = ctx.ui.global::<Viewer>();
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
    viewer.on_close(handler!(|ctx| ctx.close_viewer()));
    viewer.on_step(handler!(|ctx, delta| step(&ctx, delta)));
    viewer.on_toggle_play(handler!(|ctx| toggle_play(&ctx)));
    viewer.on_mark(handler!(|ctx, start| mark(&ctx, start)));
    viewer.on_clip(handler!(|ctx| clip(&ctx)));
    viewer.on_seek(handler!(|ctx, position, released| seek(
        &ctx, position, released
    )));
    viewer.on_download(handler!(|ctx| {
        if let Some(entry) = current(&ctx) {
            let task = ctx.clone();
            ctx.spawn(async move { crate::actions::download(&task, vec![entry]).await });
        }
    }));
    viewer.on_external(handler!(|ctx| {
        if let Some(entry) = current(&ctx) {
            ctx.close_viewer();
            let task = ctx.clone();
            ctx.spawn(async move { crate::media::play(&task, entry).await });
        }
    }));
    viewer.on_properties(handler!(|ctx| {
        if let Some(entry) = current(&ctx) {
            let task = ctx.clone();
            ctx.spawn(async move { crate::actions::properties(&task, entry).await });
        }
    }));
    viewer.on_key({
        let weak = weak.clone();
        move |text, ctrl| weak.upgrade().is_some_and(|ctx| key(&ctx, &text, ctrl))
    });
}

impl App {
    pub fn viewer_open(&self) -> bool {
        self.ui.global::<Viewer>().get_open()
    }

    pub fn close_viewer(self: &Rc<Self>) {
        let viewer = self.ui.global::<Viewer>();
        if !viewer.get_open() {
            return;
        }
        close_video(self);
        {
            let mut state = self.viewer.borrow_mut();
            state.token += 1;
            state.items.clear();
            state.cache.clear();
        }
        viewer.set_open(false);
        viewer.set_picture(Default::default());
        if !self.dialog_open() {
            self.ui.invoke_focus_root();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![120u8; (width * height * 3) as usize])
            .unwrap();
        drop(writer);
        bytes
    }

    #[test]
    fn pictures_are_decoded_and_bounded() {
        let decoded = decode(&png(64, 32), 4096, false).unwrap();
        assert_eq!((decoded.width, decoded.height), (64, 32));
        assert_eq!((decoded.pixels.width(), decoded.pixels.height()), (64, 32));
        // Larger than the bound: scaled down, original size still reported.
        let small = decode(&png(400, 100), 100, true).unwrap();
        assert_eq!((small.width, small.height), (400, 100));
        assert_eq!((small.pixels.width(), small.pixels.height()), (100, 25));
        assert!(matches!(
            decode(b"not a picture", 100, true),
            Err(error) if error.code == ErrorCode::Unsupported
        ));
    }

    #[test]
    fn only_decodable_formats_open_in_the_viewer() {
        let entry = |name: &str, mime: Option<&str>| RemoteEntry {
            id: "x".into(),
            parent_id: None,
            name: name.into(),
            kind: EntryKind::File,
            size_bytes: DecimalU64(1),
            modified_at: None,
            etag: None,
            hash: None,
            mime_type: mime.map(str::to_owned),
            starred: None,
        };
        assert!(can_view_image(&entry("a.JPG", None)));
        assert!(can_view_image(&entry("photo", Some("image/webp"))));
        assert!(!can_view_image(&entry("a.heic", None)));
        assert!(!can_view_image(&entry("a.svg", Some("image/svg+xml"))));
    }
}
