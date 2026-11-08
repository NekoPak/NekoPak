//! Native, bounded container indexing and stream-copy exports over authenticated ranges.
//! No codec, executable, URL, credential or network client belongs to this module.
mod codecs;
mod isobmff;
mod matroska;
mod transport_stream;

use nekopak_domain::{ClientError, ErrorCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    future::Future,
    io::{Seek, SeekFrom, Write},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

pub type RangeFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<u8>, ClientError>> + Send + 'a>>;

/// One immutable account + remote ID + representation + version. Implementors must
/// verify the current response identity/length and the exact HTTP range on every read.
pub trait RangeSource: Send + Sync {
    fn len(&self) -> u64;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn identity(&self) -> &str;
    fn read_exact(&self, offset: u64, length: usize) -> RangeFuture<'_>;
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_metadata_bytes: usize,
    pub max_samples: usize,
    pub max_tracks: usize,
    pub max_read_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_metadata_bytes: 32 * 1024 * 1024,
            max_samples: 2_000_000,
            max_tracks: 32,
            max_read_bytes: 1024 * 1024,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<Self, ClientError> {
        if self.max_metadata_bytes == 0
            || self.max_metadata_bytes > 64 * 1024 * 1024
            || self.max_samples == 0
            || self.max_samples > 4_000_000
            || self.max_tracks == 0
            || self.max_tracks > 64
            || self.max_read_bytes == 0
            || self.max_read_bytes > 4 * 1024 * 1024
        {
            return Err(invalid());
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackInfo {
    pub id: u32,
    pub kind: TrackKind,
    pub codec: String,
    pub language: String,
    pub duration_ms: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaInfo {
    pub container: String,
    pub duration_ms: u64,
    pub tracks: Vec<TrackInfo>,
    pub keyframes: String,
    pub warnings: Vec<String>,
}

pub struct MediaIndex {
    pub info: MediaInfo,
    index: ContainerIndex,
}
enum ContainerIndex {
    Mp4(isobmff::Index),
    Matroska(matroska::Index),
    TransportStream(transport_stream::Index),
}

/// Persists only local immutable source identity, container metadata and byte spans.
/// This is an internal Rust plan, never a UI DTO. Public IPC should expose its summary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClipPlan {
    pub source_identity: String,
    pub source_bytes: u64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub aligned_start_ms: u64,
    pub output_duration_ms: u64,
    pub output_format: String,
    pub output_bytes: u64,
    pub warnings: Vec<String>,
    header: Vec<u8>,
    spans: Vec<ReadSpan>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReadSpan {
    offset: u64,
    length: u64,
    #[serde(default)]
    prefix: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct RenderControl {
    cancelled: Arc<AtomicBool>,
    progress: Option<Arc<dyn Fn(u64, u64) + Send + Sync>>,
}
impl RenderControl {
    pub fn with_progress(callback: impl Fn(u64, u64) + Send + Sync + 'static) -> Self {
        Self {
            progress: Some(Arc::new(callback)),
            ..Self::default()
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    fn check(&self) -> Result<(), ClientError> {
        if self.is_cancelled() {
            Err(ClientError::new(
                ErrorCode::Cancelled,
                "Media export cancelled.",
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderResult {
    pub output_bytes: u64,
    pub output_sha256: String,
    pub output_duration_ms: u64,
    pub aligned_start_ms: u64,
    pub warnings: Vec<String>,
}

pub async fn probe(source: &dyn RangeSource, limits: Limits) -> Result<MediaIndex, ClientError> {
    let limits = limits.validate()?;
    if source.identity().is_empty() || source.identity().len() > 4096 || source.len() < 16 {
        return Err(invalid());
    }
    let signature = read(source, 0, 8, limits).await?;
    if signature[..4] == [0x1a, 0x45, 0xdf, 0xa3] {
        let index = matroska::probe(source, limits).await?;
        Ok(MediaIndex {
            info: index.info(),
            index: ContainerIndex::Matroska(index),
        })
    } else if signature[0] == 0x47 {
        let index = transport_stream::probe(source, limits).await?;
        Ok(MediaIndex {
            info: index.info(),
            index: ContainerIndex::TransportStream(index),
        })
    } else {
        let index = isobmff::probe(source, limits).await?;
        Ok(MediaIndex {
            info: index.info(),
            index: ContainerIndex::Mp4(index),
        })
    }
}

impl MediaIndex {
    pub fn plan(&self, range: crate::ClipRange) -> Result<ClipPlan, ClientError> {
        range.validate(Some(self.info.duration_ms))?;
        match &self.index {
            ContainerIndex::Mp4(index) => index.plan(range),
            ContainerIndex::Matroska(_) => Err(unsupported(
                "Matroska clipping requires asynchronous cluster inspection; use prepare().",
            )),
            ContainerIndex::TransportStream(_) => Err(unsupported(
                "MPEG-TS clipping requires asynchronous timestamp inspection; use prepare().",
            )),
        }
    }
}

/// Resolves container indexes into a durable clip plan. MP4 uses its sample tables;
/// Matroska reads bounded cluster headers around cue points, never the whole film.
pub async fn prepare(
    source: &dyn RangeSource,
    index: &MediaIndex,
    range: crate::ClipRange,
) -> Result<ClipPlan, ClientError> {
    range.validate(Some(index.info.duration_ms))?;
    match &index.index {
        ContainerIndex::Mp4(index) => index.plan(range),
        ContainerIndex::Matroska(index) => index.plan(source, range).await,
        ContainerIndex::TransportStream(index) => index.plan(source, range).await,
    }
}

/// Explicit container choice. Remuxing is stream copy; unsupported track formats
/// fail rather than silently dropping audio or re-encoding video.
pub async fn prepare_format(
    source: &dyn RangeSource,
    index: &MediaIndex,
    range: crate::ClipRange,
    output_format: &str,
) -> Result<ClipPlan, ClientError> {
    range.validate(Some(index.info.duration_ms))?;
    if output_format == index.info.container {
        return prepare(source, index, range).await;
    }
    if output_format != "mp4" {
        return Err(unsupported(
            "The requested output container is not supported.",
        ));
    }
    match &index.index {
        ContainerIndex::Mp4(index) => index.plan(range),
        ContainerIndex::TransportStream(index) => index.mp4_plan(source, range).await,
        ContainerIndex::Matroska(index) => index.mp4_plan(source, range).await,
    }
}

struct MuxSample {
    dts: i64,
    pts: i64,
    duration: u32,
    sync: bool,
    spans: Vec<ReadSpan>,
}
struct MuxTrack {
    info: TrackInfo,
    scale: u32,
    sample_entry: Vec<u8>,
    samples: Vec<MuxSample>,
}

/// Writes a complete new container to a caller-owned private temporary file.
/// Pause/restart re-renders from the beginning; the caller may reuse verified range
/// cache blocks. This deliberately does not claim incremental mux resume.
pub async fn render(
    source: &dyn RangeSource,
    plan: &ClipPlan,
    output: &mut File,
    control: &RenderControl,
) -> Result<RenderResult, ClientError> {
    validate_plan(source, plan)?;
    control.check()?;
    output.set_len(0).map_err(io_error)?;
    output.seek(SeekFrom::Start(0)).map_err(io_error)?;
    output.write_all(&plan.header).map_err(io_error)?;
    let mut digest = Sha256::new();
    digest.update(&plan.header);
    let mut done = plan.header.len() as u64;
    if let Some(progress) = &control.progress {
        progress(done, plan.output_bytes);
    }
    for span in &plan.spans {
        control.check()?;
        output.write_all(&span.prefix).map_err(io_error)?;
        digest.update(&span.prefix);
        done = done
            .checked_add(span.prefix.len() as u64)
            .ok_or_else(invalid)?;
        let mut offset = span.offset;
        let mut remaining = span.length;
        while remaining > 0 {
            tokio::task::yield_now().await;
            control.check()?;
            let length = remaining.min(1024 * 1024) as usize;
            let bytes = source.read_exact(offset, length).await?;
            control.check()?;
            if bytes.len() != length
                || source.identity() != plan.source_identity
                || source.len() != plan.source_bytes
            {
                return Err(invalid());
            }
            output.write_all(&bytes).map_err(io_error)?;
            digest.update(&bytes);
            done = done.checked_add(length as u64).ok_or_else(invalid)?;
            offset += length as u64;
            remaining -= length as u64;
            if let Some(progress) = &control.progress {
                progress(done, plan.output_bytes);
            }
        }
    }
    control.check()?;
    if done != plan.output_bytes {
        return Err(invalid());
    }
    Ok(RenderResult {
        output_bytes: done,
        output_sha256: format!("{:x}", digest.finalize()),
        output_duration_ms: plan.output_duration_ms,
        aligned_start_ms: plan.aligned_start_ms,
        warnings: plan.warnings.clone(),
    })
}

fn validate_plan(source: &dyn RangeSource, plan: &ClipPlan) -> Result<(), ClientError> {
    if source.identity() != plan.source_identity
        || source.len() != plan.source_bytes
        || !matches!(plan.output_format.as_str(), "mp4" | "mkv" | "ts")
        || plan.header.len() > 64 * 1024 * 1024
        || plan.header.len() < 32
        || plan.spans.is_empty()
        || plan.spans.len() > 4_000_000
        || plan.start_ms >= plan.end_ms
        || plan.aligned_start_ms > plan.start_ms
        || plan.output_duration_ms == 0
        || plan.source_identity.len() > 4096
    {
        return Err(invalid());
    }
    let mut bytes = plan.header.len() as u64;
    let mut media_bytes = 0u64;
    let mut metadata = plan.header.len();
    for span in &plan.spans {
        if span.length == 0
            || span.prefix.len() > 64 * 1024
            || span
                .offset
                .checked_add(span.length)
                .is_none_or(|end| end > source.len())
        {
            return Err(invalid());
        }
        metadata = metadata
            .checked_add(span.prefix.len())
            .ok_or_else(invalid)?;
        if metadata > 64 * 1024 * 1024 {
            return Err(invalid());
        }
        bytes = bytes
            .checked_add(span.length)
            .and_then(|n| n.checked_add(span.prefix.len() as u64))
            .ok_or_else(invalid)?;
        media_bytes = media_bytes.checked_add(span.length).ok_or_else(invalid)?;
        if media_bytes > source.len() {
            return Err(invalid());
        }
    }
    if bytes != plan.output_bytes {
        return Err(invalid());
    }
    match plan.output_format.as_str() {
        "mp4" => isobmff::validate_header(&plan.header, plan.output_bytes),
        "mkv" => matroska::validate_header(&plan.header),
        "ts" => transport_stream::validate_header(&plan.header),
        _ => Err(invalid()),
    }
}

fn invalid() -> ClientError {
    ClientError::new(
        ErrorCode::IntegrityFailed,
        "The media container, timeline or source identity failed validation.",
    )
}
fn unsupported(message: &'static str) -> ClientError {
    ClientError::new(ErrorCode::Unsupported, message)
}
fn io_error(error: std::io::Error) -> ClientError {
    ClientError::new(
        if error.kind() == std::io::ErrorKind::StorageFull {
            ErrorCode::DiskFull
        } else {
            ErrorCode::PermissionDenied
        },
        "The private media output could not be written; no final file was published.",
    )
}

async fn read(
    source: &dyn RangeSource,
    offset: u64,
    length: usize,
    limits: Limits,
) -> Result<Vec<u8>, ClientError> {
    if length > limits.max_metadata_bytes
        || offset
            .checked_add(length as u64)
            .is_none_or(|end| end > source.len())
    {
        return Err(invalid());
    }
    let mut bytes = Vec::with_capacity(length);
    while bytes.len() < length {
        let size = (length - bytes.len()).min(limits.max_read_bytes);
        let chunk = source.read_exact(offset + bytes.len() as u64, size).await?;
        if chunk.len() != size {
            return Err(invalid());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
