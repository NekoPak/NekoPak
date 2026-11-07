//! Independently implemented local media clip engine. No provider-specific API or network input.
//! A remote clip path must later connect a verified random-access source and shared cache.
mod mp4;
use nekopak_domain::{ClientError, ErrorCode};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tempfile::NamedTempFile;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipRange {
    pub start_ms: u64,
    pub end_ms: u64,
}
impl ClipRange {
    pub fn validate(self, duration_ms: Option<u64>) -> Result<Self, ClientError> {
        if self.end_ms <= self.start_ms {
            return Err(error(
                ErrorCode::Conflict,
                "The clip end must be after its start.",
            ));
        }
        if self.end_ms > i64::MAX as u64 {
            return Err(error(
                ErrorCode::Conflict,
                "The clip timeline exceeds the media engine's supported range.",
            ));
        }
        if duration_ms.is_some_and(|duration| duration == 0 || self.end_ms > duration) {
            return Err(error(
                ErrorCode::Conflict,
                "The clip range exceeds the known media duration.",
            ));
        }
        Ok(self)
    }
    fn duration_ms(self) -> u64 {
        self.end_ms - self.start_ms
    }
}

#[derive(Debug, Clone)]
pub struct FfmpegConfig {
    pub executable: PathBuf,
    pub expected_sha256: String,
    pub timeout: Duration,
}
impl FfmpegConfig {
    pub fn verify(&self) -> Result<(), ClientError> {
        self.verified_file().map(|_| ())
    }
    fn verified_file(&self) -> Result<File, ClientError> {
        checked_path(&self.executable, false)?;
        if self.expected_sha256.len() != 64
            || !self
                .expected_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(error(
                ErrorCode::Conflict,
                "FFmpeg requires an explicit SHA-256 allowlist entry.",
            ));
        }
        let mut file = read_guard(&self.executable, false)?;
        if file.metadata().map_err(|_| integrity())?.len() > 256 * 1024 * 1024 {
            return Err(integrity());
        }
        let hash = hash_file(&mut file)?;
        if hash != self.expected_sha256.to_ascii_lowercase() {
            return Err(error(
                ErrorCode::IntegrityFailed,
                "FFmpeg does not match the configured checksum.",
            ));
        }
        Ok(file)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipResult {
    pub destination: PathBuf,
    pub requested_range: ClipRange,
    pub output_duration_ms: u64,
    pub output_bytes: u64,
    pub output_sha256: String,
    pub warnings: Vec<String>,
}

/// Explicit local export, independent of desktop IPC. Existing outputs are never overwritten.
/// Cancellation drops the request, terminates/reaps FFmpeg and removes only its unique temp file.
pub async fn render_clip(
    config: &FfmpegConfig,
    source: &Path,
    destination: &Path,
    range: ClipRange,
    known_duration_ms: Option<u64>,
) -> Result<ClipResult, ClientError> {
    let deadline = tokio::time::Instant::now() + config.timeout;
    if known_duration_ms.is_none() {
        return Err(error(ErrorCode::Unsupported, "A validated media duration is required before local clip export; automatic media probing is not implemented yet."));
    }
    range.validate(known_duration_ms)?;
    checked_path(source, false)?;
    checked_path(destination, true)?;
    if destination
        .extension()
        .and_then(|ext| ext.to_str())
        .is_none_or(|ext| !ext.eq_ignore_ascii_case("mp4"))
    {
        return Err(error(
            ErrorCode::Conflict,
            "The current stream-copy export produces an MP4 destination.",
        ));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| error(ErrorCode::Conflict, "A destination directory is required."))?;
    let tool_guard = config.verified_file()?;
    tokio::task::yield_now().await;
    check_deadline(deadline)?;
    let mut source_guard = read_guard(source, false)?;
    validate_container(&mut source_guard)?;
    let directory_guard = read_guard(parent, true)?;
    let source = source.canonicalize().map_err(|_| integrity())?;
    let destination = parent
        .canonicalize()
        .map_err(|_| integrity())?
        .join(destination.file_name().ok_or_else(integrity)?);
    if destination.try_exists().map_err(|_| integrity())? {
        return Err(error(
            ErrorCode::Conflict,
            "The destination already exists; choose a new filename.",
        ));
    }
    let temporary = tempfile::Builder::new()
        .prefix(".nekopak-clip-")
        .suffix(".part")
        .tempfile_in(parent)
        .map_err(|_| {
            error(
                ErrorCode::PermissionDenied,
                "A clip temporary file cannot be created.",
            )
        })?;
    let arguments = clip_arguments(&source, temporary.path(), range);
    let mut command = Command::new(&config.executable);
    command
        .args(arguments)
        .current_dir(parent)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_clear();
    for key in ["SystemRoot", "WINDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command
        .spawn()
        .map_err(|_| error(ErrorCode::PermissionDenied, "FFmpeg could not be started."))?;
    let stderr = child.stderr.take().ok_or_else(integrity)?;
    let mut session = Session {
        child: Some(child),
        temporary: Some(temporary),
        guards: Some((tool_guard, source_guard, directory_guard)),
    };
    let work = async {
        let child = session.child.as_mut().ok_or_else(integrity)?;
        let (_, status) = tokio::try_join!(discard_diagnostics(stderr), async {
            child
                .wait()
                .await
                .map_err(|_| error(ErrorCode::Internal, "FFmpeg could not be reaped."))
        })?;
        if !status.success() {
            return Err(error(
                ErrorCode::IntegrityFailed,
                "The source or its tracks could not be copied into MP4. No output was committed.",
            ));
        }
        Ok(())
    };
    let outcome = tokio::time::timeout_at(deadline, work)
        .await
        .unwrap_or_else(|_| {
            Err(error(
                ErrorCode::TransientNetwork,
                "Clip extraction timed out; temporary output is being cleaned.",
            ))
        });
    if let Err(error) = outcome {
        session.cleanup().await;
        return Err(error);
    }
    session.child.take();
    let temporary = session.temporary.as_mut().ok_or_else(integrity)?;
    temporary.as_file().sync_all().map_err(|_| {
        error(
            ErrorCode::DiskFull,
            "The clip could not be flushed; the destination was preserved.",
        )
    })?;
    let output_duration_ms = mp4::validate(temporary.as_file_mut())?;
    if output_duration_ms.saturating_add(500) < range.duration_ms()
        || output_duration_ms > range.end_ms.saturating_add(2_000)
    {
        return Err(error(
            ErrorCode::IntegrityFailed,
            "The clip duration is inconsistent with the requested interval.",
        ));
    }
    let output_bytes = temporary
        .as_file()
        .metadata()
        .map_err(|_| integrity())?
        .len();
    let output_sha256 = hash_output(temporary.as_file_mut(), deadline).await?;
    // Cancellation is observed before the small no-clobber publication section.
    // Once publication has started, cancellation does not undo the committed file.
    tokio::task::yield_now().await;
    check_deadline(deadline)?;
    let temporary = session.temporary.take().ok_or_else(integrity)?;
    let published = temporary.persist_noclobber(&destination).map_err(|_| {
        error(
            ErrorCode::Conflict,
            "The clip could not be published without replacing another file.",
        )
    })?;
    published.sync_all().map_err(|_| {
        error(
            ErrorCode::DiskFull,
            "The published file could not be flushed.",
        )
    })?;
    Ok(ClipResult { destination, requested_range: range, output_duration_ms, output_bytes, output_sha256,
        warnings: vec!["Stream copy may begin at an earlier keyframe; the requested start is not frame-exact.".into(), "Embedded subtitles/data/attachments are omitted. Unsupported audio/container combinations fail instead of silently transcoding or dropping audio.".into()] })
}

fn clip_arguments(source: &Path, temporary: &Path, range: ClipRange) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-protocol_whitelist",
        "file",
        "-format_whitelist",
        "mov,matroska,webm",
        "-ss",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    args.push(format_seconds(range.start_ms).into());
    args.push("-i".into());
    args.push(source.as_os_str().into());
    args.push("-t".into());
    args.push(format_seconds(range.duration_ms()).into());
    args.extend(
        [
            "-map",
            "0:v:0",
            "-map",
            "0:a?",
            "-sn",
            "-dn",
            "-c",
            "copy",
            "-map_metadata",
            "-1",
            "-map_chapters",
            "-1",
            "-avoid_negative_ts",
            "make_zero",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
            "-y",
        ]
        .into_iter()
        .map(Into::into),
    );
    args.push(temporary.as_os_str().into());
    args
}

struct Session {
    child: Option<Child>,
    temporary: Option<NamedTempFile>,
    guards: Option<(File, File, File)>,
}
impl Session {
    async fn cleanup(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        self.temporary.take();
        self.guards.take();
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            let temporary = self.temporary.take();
            let guards = self.guards.take();
            tokio::spawn(async move {
                let _ = child.wait().await;
                drop(temporary);
                drop(guards);
            });
        }
    }
}

async fn discard_diagnostics(reader: impl AsyncRead + Unpin) -> Result<(), ClientError> {
    let mut reader = reader.take(64 * 1024 + 1);
    let mut buffer = Vec::new();
    reader
        .read_to_end(&mut buffer)
        .await
        .map_err(|_| integrity())?;
    if buffer.len() > 64 * 1024 {
        return Err(error(
            ErrorCode::IntegrityFailed,
            "Media diagnostics exceeded the safety bound; output was discarded.",
        ));
    }
    Ok(())
}

fn checked_path(path: &Path, destination: bool) -> Result<(), ClientError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(error(
            ErrorCode::Conflict,
            "Media paths must be absolute without parent traversal.",
        ));
    }
    let mut built = PathBuf::new();
    for component in path.components() {
        if let Component::Normal(name) = component {
            let name = name
                .to_str()
                .ok_or_else(|| error(ErrorCode::Conflict, "Media paths must be valid Unicode."))?;
            let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
            if name.is_empty()
                || name.chars().any(char::is_control)
                || name.ends_with([' ', '.'])
                || name.contains([':', '?', '*', '<', '>', '|', '"'])
                || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.len() == 4
                    && stem.as_bytes()[3].is_ascii_digit())
            {
                return Err(error(
                    ErrorCode::Conflict,
                    "Media paths contain unsupported special components.",
                ));
            }
        }
        #[cfg(windows)]
        if let Component::Prefix(prefix) = component {
            if !matches!(
                prefix.kind(),
                std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
            ) {
                return Err(error(
                    ErrorCode::Conflict,
                    "Network and device paths are not supported for local media.",
                ));
            }
        }
        built.push(component);
        match fs::symlink_metadata(&built) {
            Ok(metadata) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(error(
                            ErrorCode::Conflict,
                            "Media paths cannot traverse reparse points.",
                        ));
                    }
                }
                if metadata.file_type().is_symlink() {
                    return Err(error(
                        ErrorCode::Conflict,
                        "Media paths cannot traverse symbolic links.",
                    ));
                }
            }
            Err(err)
                if destination && built == path && err.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(error(
                    ErrorCode::NotFound,
                    "The media source or destination directory does not exist.",
                ))
            }
        }
    }
    if !destination && !path.is_file() {
        return Err(error(
            ErrorCode::Conflict,
            "The media source must be a regular file.",
        ));
    }
    if destination && path.try_exists().map_err(|_| integrity())? {
        return Err(error(
            ErrorCode::Conflict,
            "The destination already exists; choose a new filename.",
        ));
    }
    Ok(())
}
fn read_guard(path: &Path, directory: bool) -> Result<File, ClientError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1 | if directory { 2 } else { 0 });
        if directory {
            options.custom_flags(0x02000000);
        }
    }
    #[cfg(not(windows))]
    let _ = directory;
    options.open(path).map_err(|_| {
        error(
            ErrorCode::PermissionDenied,
            "Media file or directory cannot be held safely.",
        )
    })
}
fn validate_container(file: &mut File) -> Result<(), ClientError> {
    let mut prefix = [0u8; 12];
    let read = file.read(&mut prefix).map_err(|_| integrity())?;
    file.seek(SeekFrom::Start(0)).map_err(|_| integrity())?;
    if read >= 8 && (&prefix[4..8] == b"ftyp" || prefix[..4] == [0x1a, 0x45, 0xdf, 0xa3]) {
        return Ok(());
    }
    Err(error(ErrorCode::Unsupported, "This local preview supports indexed MP4/MKV sources. MPEG-TS keyframe seeking remains a required, unimplemented parity item."))
}
fn hash_file(file: &mut File) -> Result<String, ClientError> {
    file.seek(SeekFrom::Start(0)).map_err(|_| integrity())?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| integrity())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
async fn hash_output(
    file: &mut File,
    deadline: tokio::time::Instant,
) -> Result<String, ClientError> {
    file.seek(SeekFrom::Start(0)).map_err(|_| integrity())?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        tokio::task::yield_now().await;
        check_deadline(deadline)?;
        let read = file.read(&mut buffer).map_err(|_| integrity())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn check_deadline(deadline: tokio::time::Instant) -> Result<(), ClientError> {
    if tokio::time::Instant::now() >= deadline {
        return Err(error(
            ErrorCode::TransientNetwork,
            "Clip verification exceeded its time budget; no output was committed.",
        ));
    }
    Ok(())
}
fn format_seconds(milliseconds: u64) -> String {
    format!("{}.{:03}", milliseconds / 1000, milliseconds % 1000)
}
fn error(code: ErrorCode, message: &'static str) -> ClientError {
    ClientError::new(code, message)
}
fn integrity() -> ClientError {
    error(
        ErrorCode::IntegrityFailed,
        "Media output failed validation; no output was committed.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges_and_timeline_are_validated_without_rounding() {
        assert!(ClipRange {
            start_ms: 5_000,
            end_ms: 8_500
        }
        .validate(Some(10_000))
        .is_ok());
        for range in [
            ClipRange {
                start_ms: 8_500,
                end_ms: 5_000,
            },
            ClipRange {
                start_ms: 0,
                end_ms: 10_001,
            },
            ClipRange {
                start_ms: 0,
                end_ms: 0,
            },
        ] {
            assert!(range.validate(Some(10_000)).is_err());
        }
        assert!(ClipRange {
            start_ms: 0,
            end_ms: u64::MAX
        }
        .validate(None)
        .is_err());
        assert_eq!(format_seconds(8_501), "8.501");
    }
    #[test]
    fn arguments_are_stream_copy_local_only_and_require_video() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("input.mp4");
        let part = dir.path().join("output.part");
        let args = clip_arguments(
            &source,
            &part,
            ClipRange {
                start_ms: 5000,
                end_ms: 8500,
            },
        );
        let rendered: Vec<_> = args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        assert!(rendered.windows(2).any(|pair| pair == ["-c", "copy"]));
        assert!(rendered
            .windows(2)
            .any(|pair| pair == ["-protocol_whitelist", "file"]));
        assert!(rendered.windows(2).any(|pair| pair == ["-map", "0:v:0"]));
        assert_eq!(args.last().unwrap(), part.as_os_str());
    }
    #[test]
    fn existing_paths_and_traversal_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.mp4");
        fs::write(&source, b"source").unwrap();
        assert!(checked_path(&source, false).is_ok());
        assert!(checked_path(&source, true).is_err());
        assert!(checked_path(&dir.path().join(".."), true).is_err());
        assert!(checked_path(&dir.path().join("CON.mp4"), true).is_err());
    }
    #[test]
    fn unknown_tool_checksum_is_rejected() {
        let config = FfmpegConfig {
            executable: std::env::current_exe().unwrap(),
            expected_sha256: "0".repeat(64),
            timeout: Duration::from_secs(1),
        };
        assert_eq!(
            config.verify().unwrap_err().code,
            ErrorCode::IntegrityFailed
        );
    }
    #[tokio::test]
    async fn final_hash_has_timeout_and_cancellation_points() {
        use std::io::Write;
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&vec![7u8; 2 * 1024 * 1024]).unwrap();
        assert!(hash_output(&mut file, tokio::time::Instant::now())
            .await
            .is_err());
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        let (started, mut seen) = tokio::sync::mpsc::channel(1);
        let task = tokio::spawn(async move {
            started.send(()).await.unwrap();
            hash_output(&mut file, deadline).await
        });
        seen.recv().await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}
