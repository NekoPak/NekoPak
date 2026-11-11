//! Media Foundation frame decoder for the video preview (Windows).
//!
//! Media Foundation is an optional Windows component (absent on the N
//! editions without the Media Feature Pack and on servers without the
//! feature), so its libraries are loaded on demand and never imported
//! statically: the application must start and run without them, and then
//! simply has no frame preview.
use crate::frames::{Decoder, Frame, Info};
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::ffi::c_void;
use windows::{
    core::{Interface, GUID, HRESULT, HSTRING, PCSTR, PCWSTR},
    Win32::{
        Foundation::HMODULE,
        Media::MediaFoundation::{
            IMFAttributes, IMFMediaType, IMFSample, IMFSourceReader, MFMediaType_Video,
            MFVideoFormat_RGB32, MFSTARTUP_LITE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE,
            MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION,
            MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, MF_VERSION,
        },
        System::{
            Com::{
                CoInitializeEx, CoUninitialize, StructuredStorage::PROPVARIANT,
                COINIT_MULTITHREADED,
            },
            LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32},
            Variant::{VT_I8, VT_UI8},
        },
    },
};

const FIRST_VIDEO: u32 = 0xFFFF_FFFC;
const ALL_STREAMS: u32 = 0xFFFF_FFFE;
const MEDIA_SOURCE: u32 = 0xFFFF_FFFF;
const FLAG_ERROR: u32 = 0x1;
const FLAG_END_OF_STREAM: u32 = 0x2;
const FLAG_TYPE_CHANGED: u32 = 0x20;
/// Preview frames are scaled to fit this box; decoding stays cheap and one
/// frame stays small enough to hand to the UI thread dozens of times a second.
const MAX_WIDTH: u32 = 1280;
const MAX_HEIGHT: u32 = 720;
/// Frames read forward after a seek to land on the requested time.
const MAX_FORWARD_READS: u32 = 900;

type Startup = unsafe extern "system" fn(u32, u32) -> HRESULT;
type Shutdown = unsafe extern "system" fn() -> HRESULT;
type CreateAttributes = unsafe extern "system" fn(*mut *mut c_void, u32) -> HRESULT;
type CreateMediaType = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;
type CreateReader = unsafe extern "system" fn(PCWSTR, *mut c_void, *mut *mut c_void) -> HRESULT;

/// Entry points resolved from the system's Media Foundation libraries.
struct Library {
    shutdown: Shutdown,
    create_attributes: CreateAttributes,
    create_media_type: CreateMediaType,
    create_reader: CreateReader,
}

unsafe fn symbol(module: HMODULE, name: &'static [u8]) -> Result<*const c_void, String> {
    // SAFETY: `name` is a NUL-terminated ASCII literal and `module` is loaded.
    unsafe { GetProcAddress(module, PCSTR(name.as_ptr())) }
        .map(|function| function as *const c_void)
        .ok_or_else(|| "Media Foundation is incomplete on this system".to_owned())
}

unsafe fn load(name: &str) -> Result<HMODULE, String> {
    // Only the system directory is searched: a library planted next to the
    // executable or in the working directory is never picked up.
    unsafe { LoadLibraryExW(&HSTRING::from(name), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
        .map_err(|_| "Media Foundation is not installed on this system".to_owned())
}

fn check(result: HRESULT, what: &str) -> Result<(), String> {
    if result.is_ok() {
        Ok(())
    } else {
        Err(format!("{what} failed ({:#010x})", result.0))
    }
}

fn failed(what: &str) -> impl Fn(windows::core::Error) -> String + '_ {
    move |error| format!("{what} failed ({:#010x})", error.code().0)
}

/// COM and Media Foundation initialised for the decoder thread; released in
/// reverse order when the decoder goes away.
struct Runtime {
    library: Library,
}

impl Runtime {
    fn start() -> Result<Self, String> {
        // SAFETY: plain FFI; every pointer passed below outlives its call and
        // each resolved symbol is transmuted to its documented signature.
        unsafe {
            let platform = load("mfplat.dll")?;
            let readwrite = load("mfreadwrite.dll")?;
            let startup: Startup = std::mem::transmute(symbol(platform, b"MFStartup\0")?);
            let library =
                Library {
                    shutdown: std::mem::transmute::<*const c_void, Shutdown>(symbol(
                        platform,
                        b"MFShutdown\0",
                    )?),
                    create_attributes: std::mem::transmute::<*const c_void, CreateAttributes>(
                        symbol(platform, b"MFCreateAttributes\0")?,
                    ),
                    create_media_type: std::mem::transmute::<*const c_void, CreateMediaType>(
                        symbol(platform, b"MFCreateMediaType\0")?,
                    ),
                    create_reader: std::mem::transmute::<*const c_void, CreateReader>(symbol(
                        readwrite,
                        b"MFCreateSourceReaderFromURL\0",
                    )?),
                };
            let com = CoInitializeEx(None, COINIT_MULTITHREADED);
            if com.is_err() {
                return Err(format!("COM initialisation failed ({:#010x})", com.0));
            }
            if let Err(message) = check(startup(MF_VERSION, MFSTARTUP_LITE), "MFStartup") {
                CoUninitialize();
                return Err(message);
            }
            Ok(Self { library })
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // SAFETY: balances the successful MFStartup and CoInitializeEx above.
        unsafe {
            let _ = (self.library.shutdown)();
            CoUninitialize();
        }
    }
}

struct Layout {
    width: u32,
    height: u32,
    /// Bytes per row; negative when the rows are stored bottom-up.
    stride: i32,
}

pub struct MediaFoundation {
    // Field order matters: the reader is released before the runtime shuts down.
    reader: IMFSourceReader,
    layout: Layout,
    duration_ms: u64,
    /// Media time of the most recent frame, to estimate frame spacing.
    last_ms: u64,
    spacing_ms: u64,
    // Held only to keep Media Foundation and COM alive for `reader`.
    _runtime: Runtime,
}

fn scaled(width: u32, height: u32) -> (u32, u32) {
    if width <= MAX_WIDTH && height <= MAX_HEIGHT {
        return (width, height);
    }
    let ratio =
        (f64::from(MAX_WIDTH) / f64::from(width)).min(f64::from(MAX_HEIGHT) / f64::from(height));
    // Even sizes keep every colour converter happy.
    let even = |value: f64| ((value.round() as u32).max(2) / 2) * 2;
    (
        even(f64::from(width) * ratio),
        even(f64::from(height) * ratio),
    )
}

unsafe fn output_type(library: &Library, size: Option<(u32, u32)>) -> Result<IMFMediaType, String> {
    unsafe {
        let mut raw = std::ptr::null_mut();
        check((library.create_media_type)(&mut raw), "MFCreateMediaType")?;
        let kind = IMFMediaType::from_raw(raw);
        kind.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(failed("SetGUID"))?;
        kind.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
            .map_err(failed("SetGUID"))?;
        if let Some((width, height)) = size {
            kind.SetUINT64(
                &MF_MT_FRAME_SIZE,
                (u64::from(width) << 32) | u64::from(height),
            )
            .map_err(failed("SetUINT64"))?;
        }
        Ok(kind)
    }
}

unsafe fn current_layout(reader: &IMFSourceReader) -> Result<Layout, String> {
    unsafe {
        let kind = reader
            .GetCurrentMediaType(FIRST_VIDEO)
            .map_err(failed("GetCurrentMediaType"))?;
        let size = kind
            .GetUINT64(&MF_MT_FRAME_SIZE)
            .map_err(failed("frame size"))?;
        let (width, height) = ((size >> 32) as u32, size as u32);
        if width == 0 || height == 0 || width > 16_384 || height > 16_384 {
            return Err("the video reports an unusable frame size".into());
        }
        // Without an explicit stride RGB32 is stored bottom-up.
        let stride = kind
            .GetUINT32(&MF_MT_DEFAULT_STRIDE)
            .map(|value| value as i32)
            .unwrap_or(-((width * 4) as i32));
        Ok(Layout {
            width,
            height,
            stride,
        })
    }
}

pub fn open(url: &str) -> Result<Box<dyn Decoder>, String> {
    let runtime = Runtime::start()?;
    // SAFETY: COM objects are used on this thread only and released before
    // `runtime` shuts Media Foundation down (see the field order).
    unsafe {
        let mut raw = std::ptr::null_mut();
        check(
            (runtime.library.create_attributes)(&mut raw, 2),
            "MFCreateAttributes",
        )?;
        let attributes = IMFAttributes::from_raw(raw);
        // Lets the reader convert to RGB and scale the frames itself.
        attributes
            .SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)
            .map_err(failed("SetUINT32"))?;
        let mut raw = std::ptr::null_mut();
        check(
            (runtime.library.create_reader)(
                PCWSTR(HSTRING::from(url).as_ptr()),
                attributes.as_raw(),
                &mut raw,
            ),
            "opening the video",
        )?;
        let reader = IMFSourceReader::from_raw(raw);
        reader
            .SetStreamSelection(ALL_STREAMS, false)
            .map_err(failed("SetStreamSelection"))?;
        reader
            .SetStreamSelection(FIRST_VIDEO, true)
            .map_err(|_| "the file has no video stream this system can read".to_owned())?;
        let native = reader
            .GetNativeMediaType(FIRST_VIDEO, 0)
            .map_err(failed("GetNativeMediaType"))?;
        let size = native.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
        let wanted = match ((size >> 32) as u32, size as u32) {
            (0, _) | (_, 0) => None,
            (width, height) => Some(scaled(width, height)),
        };
        // Preferred: RGB at preview size. Some decoders refuse to scale, so
        // fall back to RGB at the native size before giving up.
        let scaled_type = output_type(&runtime.library, wanted)?;
        if reader
            .SetCurrentMediaType(FIRST_VIDEO, None, &scaled_type)
            .is_err()
        {
            let plain = output_type(&runtime.library, None)?;
            reader
                .SetCurrentMediaType(FIRST_VIDEO, None, &plain)
                .map_err(|_| "no installed decoder can read this video".to_owned())?;
        }
        let layout = current_layout(&reader)?;
        let duration_ms = reader
            .GetPresentationAttribute(MEDIA_SOURCE, &MF_PD_DURATION)
            .ok()
            .and_then(|value| {
                let inner = &value.Anonymous.Anonymous;
                (inner.vt == VT_UI8).then(|| inner.Anonymous.uhVal / 10_000)
            })
            .unwrap_or(0);
        Ok(Box::new(MediaFoundation {
            reader,
            layout,
            duration_ms,
            last_ms: 0,
            spacing_ms: 40,
            _runtime: runtime,
        }))
    }
}

impl MediaFoundation {
    /// Reads the next decoded frame; `None` at the end of the stream.
    fn read(&mut self) -> Result<Option<Frame>, String> {
        // SAFETY: out-pointers are valid for the call; the locked buffer is
        // only read within its reported length and unlocked before returning.
        unsafe {
            loop {
                let (mut flags, mut timestamp) = (0u32, 0i64);
                let mut sample: Option<IMFSample> = None;
                self.reader
                    .ReadSample(
                        FIRST_VIDEO,
                        0,
                        None,
                        Some(&mut flags),
                        Some(&mut timestamp),
                        Some(&mut sample),
                    )
                    .map_err(failed("reading a frame"))?;
                if flags & FLAG_ERROR != 0 {
                    return Err("the decoder reported an error".into());
                }
                if flags & FLAG_TYPE_CHANGED != 0 {
                    self.layout = current_layout(&self.reader)?;
                }
                if flags & FLAG_END_OF_STREAM != 0 {
                    return Ok(None);
                }
                // Stream ticks and gaps arrive without a sample.
                let Some(sample) = sample else { continue };
                let buffer = sample
                    .ConvertToContiguousBuffer()
                    .map_err(failed("ConvertToContiguousBuffer"))?;
                let mut data = std::ptr::null_mut();
                let mut length = 0u32;
                buffer
                    .Lock(&mut data, None, Some(&mut length))
                    .map_err(failed("locking the frame"))?;
                let (width, height) = (self.layout.width as usize, self.layout.height as usize);
                let row = self.layout.stride.unsigned_abs() as usize;
                let frame = if data.is_null() || row < width * 4 || (length as usize) < row * height
                {
                    Err("the decoder returned a short frame".to_owned())
                } else {
                    let source = std::slice::from_raw_parts(data, row * height);
                    let mut pixels =
                        SharedPixelBuffer::<Rgba8Pixel>::new(width as u32, height as u32);
                    let target = pixels.make_mut_slice();
                    for y in 0..height {
                        let from = if self.layout.stride < 0 {
                            height - 1 - y
                        } else {
                            y
                        };
                        let line = &source[from * row..from * row + width * 4];
                        for (pixel, bgrx) in target[y * width..(y + 1) * width]
                            .iter_mut()
                            .zip(line.as_chunks::<4>().0)
                        {
                            *pixel = Rgba8Pixel {
                                r: bgrx[2],
                                g: bgrx[1],
                                b: bgrx[0],
                                a: 255,
                            };
                        }
                    }
                    Ok(pixels)
                };
                let _ = buffer.Unlock();
                let at_ms = (timestamp.max(0) / 10_000) as u64;
                if at_ms > self.last_ms && at_ms - self.last_ms < 1000 {
                    self.spacing_ms = at_ms - self.last_ms;
                }
                self.last_ms = at_ms;
                return frame.map(|pixels| Some(Frame { pixels, at_ms }));
            }
        }
    }
}

impl Decoder for MediaFoundation {
    fn info(&self) -> Info {
        Info {
            duration_ms: self.duration_ms,
        }
    }

    fn seek(&mut self, ms: u64, exact: bool) -> Result<Frame, String> {
        let target = if self.duration_ms > 0 {
            ms.min(self.duration_ms.saturating_sub(1))
        } else {
            ms
        };
        // SAFETY: the PROPVARIANT holds a plain 64-bit integer (no ownership).
        unsafe {
            let mut position = PROPVARIANT::default();
            let inner = &mut *position.Anonymous.Anonymous;
            inner.vt = VT_I8;
            inner.Anonymous.hVal = (target as i64).saturating_mul(10_000);
            self.reader
                .SetCurrentPosition(&GUID::zeroed(), &position)
                .map_err(failed("seeking"))?;
        }
        // The reader resumes at the preceding keyframe. While dragging that
        // frame is shown at once; a finished seek decodes on to the target.
        let mut frame = match self.read()? {
            Some(frame) => frame,
            None => return Err("no frame at this position".into()),
        };
        if exact {
            let mut reads = 0;
            while frame.at_ms + self.spacing_ms <= target && reads < MAX_FORWARD_READS {
                match self.read()? {
                    Some(next) => frame = next,
                    None => break,
                }
                reads += 1;
            }
        }
        Ok(frame)
    }

    fn next(&mut self) -> Result<Option<Frame>, String> {
        self.read()
    }
}

#[cfg(test)]
mod tests {
    use super::{open, scaled};
    use nekopak_core::AppService;
    use nekopak_provider_mock::Scenario;
    use std::{collections::HashSet, sync::Arc};

    /// End to end on a real Windows: a local video is served through the
    /// loopback media session and decoded by Media Foundation, exactly the
    /// path the viewer uses. Skipped only where Media Foundation is not
    /// installed, unless `NEKOPAK_REQUIRE_MEDIA_FOUNDATION` insists.
    #[test]
    fn decodes_the_loopback_session_of_a_local_video() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();
        let work = tempfile::tempdir().unwrap();
        let media = work.path().join("media");
        std::fs::create_dir(&media).unwrap();
        std::fs::write(
            media.join("preview.mp4"),
            include_bytes!("../tests/data/preview.mp4"),
        )
        .unwrap();
        let service = Arc::new(AppService::mock(
            &work.path().join("tasks.sqlite3"),
            Scenario::Free,
        ));
        let source = service.local_sources_add(media.to_str().unwrap()).unwrap();
        let listing = service.local_list(&source.root_id, None, None).unwrap();
        assert_eq!(listing.items.len(), 1);
        let session = runtime
            .block_on(service.media_session_open(&listing.items[0].id, None))
            .unwrap();
        let url = session["url"].as_str().unwrap().to_owned();
        let mut decoder = match open(&url) {
            Ok(decoder) => decoder,
            Err(message)
                if message.contains("not installed")
                    && std::env::var_os("NEKOPAK_REQUIRE_MEDIA_FOUNDATION").is_none() =>
            {
                eprintln!("skipped: {message}");
                return;
            }
            Err(message) => panic!("the preview decoder did not open: {message}"),
        };
        let duration = decoder.info().duration_ms;
        assert!((2900..=3100).contains(&duration), "duration {duration}");
        let first = decoder.seek(0, false).unwrap();
        assert_eq!((first.pixels.width(), first.pixels.height()), (320, 180));
        // A test pattern, not a blank or single-colour frame.
        let colours: HashSet<[u8; 3]> = first
            .pixels
            .as_slice()
            .iter()
            .step_by(97)
            .map(|pixel| [pixel.r, pixel.g, pixel.b])
            .collect();
        assert!(colours.len() > 8, "{} colours", colours.len());
        assert!(first.pixels.as_slice().iter().all(|pixel| pixel.a == 255));
        let exact = decoder.seek(2000, true).unwrap();
        assert!((1900..=2100).contains(&exact.at_ms), "at {}", exact.at_ms);
        let following = decoder.next().unwrap().unwrap();
        assert!(following.at_ms > exact.at_ms);
        let mut remaining = 0;
        while decoder.next().unwrap().is_some() {
            remaining += 1;
            assert!(remaining < 200, "the stream never ended");
        }
        assert!(remaining > 5, "only {remaining} frames after 2 s");
        // Seeking back after the end still works (scrubbing after playback).
        let again = decoder.seek(500, true).unwrap();
        assert!((400..=600).contains(&again.at_ms), "at {}", again.at_ms);
        drop(decoder);
        service
            .media_session_close(session["session_id"].as_str().unwrap())
            .unwrap();
    }

    #[test]
    fn preview_size_keeps_the_aspect_ratio() {
        assert_eq!(scaled(640, 360), (640, 360));
        assert_eq!(scaled(1920, 1080), (1280, 720));
        assert_eq!(scaled(3840, 1600), (1280, 534));
        assert_eq!(scaled(1080, 1920), (406, 720));
    }
}
