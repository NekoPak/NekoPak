//! FFmpeg is only an explicit fixture generator and decode oracle; export itself is pure Rust.
use nekopak_domain::{ClientError, ErrorCode};
use nekopak_media::{
    native::{self, Limits, RangeFuture, RangeSource, RenderControl},
    ClipRange, FfmpegConfig,
};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::Duration,
};

struct FileSource {
    path: PathBuf,
    length: u64,
    reads: Mutex<Vec<(u64, usize)>>,
}
impl FileSource {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
            length: fs::metadata(path).unwrap().len(),
            reads: Mutex::new(Vec::new()),
        }
    }
    fn received(&self) -> u64 {
        self.reads
            .lock()
            .unwrap()
            .iter()
            .map(|(_, n)| *n as u64)
            .sum()
    }
}
impl RangeSource for FileSource {
    fn len(&self) -> u64 {
        self.length
    }
    fn identity(&self) -> &str {
        "immutable-synthetic-file-v1"
    }
    fn read_exact(&self, offset: u64, length: usize) -> RangeFuture<'_> {
        Box::pin(async move {
            assert!(length <= 1024 * 1024);
            assert!(offset + length as u64 <= self.length);
            self.reads.lock().unwrap().push((offset, length));
            let mut file = fs::File::open(&self.path).unwrap();
            file.seek(SeekFrom::Start(offset)).unwrap();
            let mut out = vec![0; length];
            file.read_exact(&mut out).unwrap();
            Ok(out)
        })
    }
}
fn config() -> FfmpegConfig {
    let c = FfmpegConfig {
        executable: PathBuf::from(
            std::env::var_os("NEKOPAK_TEST_FFMPEG").expect("explicit NEKOPAK_TEST_FFMPEG"),
        ),
        expected_sha256: std::env::var("NEKOPAK_TEST_FFMPEG_SHA256")
            .expect("explicit reviewed SHA256"),
        timeout: Duration::from_secs(30),
    };
    c.verify().unwrap();
    c
}
fn command(config: &FfmpegConfig, args: &[&str], path: Option<&Path>) -> std::process::Output {
    let mut c = Command::new(&config.executable);
    c.args(args);
    if let Some(path) = path {
        c.arg(path);
    }
    c.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    let output = c.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn generate(c: &FfmpegConfig, path: &Path, hevc: bool, b_frames: bool, negative_ctts: bool) {
    let mut args = vec![
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=160x90:rate=25:duration=12",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=12",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=880:sample_rate=48000:duration=12",
        "-map",
        "0:v",
        "-map",
        "1:a",
        "-map",
        "2:a",
        "-metadata:s:a:0",
        "language=eng",
        "-metadata:s:a:1",
        "language=jpn",
        "-c:v",
        if hevc { "libx265" } else { "libx264" },
        "-g",
        "50",
        "-keyint_min",
        "50",
        "-sc_threshold",
        "0",
        "-bf",
        if b_frames { "2" } else { "0" },
        "-c:a",
        "aac",
        "-y",
    ];
    if hevc {
        args.extend(["-tag:v", "hvc1", "-x265-params", "log-level=error:pools=1"]);
    }
    if negative_ctts {
        args.extend(["-movflags", "+negative_cts_offsets"]);
    }
    command(c, &args, Some(path));
}
fn decode(c: &FfmpegConfig, path: &Path) -> Vec<String> {
    let output = Command::new(&c.executable)
        .args(["-hide_banner", "-loglevel", "error", "-xerror", "-i"])
        .arg(path)
        .args(["-map", "0:v:0", "-f", "framemd5", "-"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|s| !s.starts_with('#') && !s.trim().is_empty())
        .map(|s| s.rsplit(',').next().unwrap().trim().to_owned())
        .collect()
}

#[tokio::test]
#[ignore = "explicit FFmpeg fixture generator/decode oracle required"]
async fn native_mp4_clips_bframes_multiaudio_and_hevc_using_sparse_ranges() {
    let c = config();
    let dir = tempfile::tempdir().unwrap();
    for (hevc, bframes, negative) in [
        (false, false, false),
        (false, true, false),
        (false, true, true),
        (true, true, false),
    ] {
        let input = dir
            .path()
            .join(format!("source-{hevc}-{bframes}-{negative}.mp4"));
        generate(&c, &input, hevc, bframes, negative);
        let source = FileSource::new(&input);
        let index = native::probe(&source, Limits::default()).await.unwrap();
        assert!((12000..12100).contains(&index.info.duration_ms));
        assert_eq!(index.info.tracks.len(), 3);
        assert_eq!(
            index
                .info
                .tracks
                .iter()
                .filter(|t| t.language == "eng" || t.language == "jpn")
                .count(),
            2
        );
        let metadata_bytes = source.received();
        assert!(
            metadata_bytes < source.length / 3,
            "probe downloaded too much: {metadata_bytes}/{}",
            source.length
        );
        let plan = index
            .plan(ClipRange {
                start_ms: 5000,
                end_ms: 8500,
            })
            .unwrap();
        assert!(
            (3900..=4100).contains(&plan.aligned_start_ms),
            "{}",
            plan.aligned_start_ms
        );
        assert!((4400..=4600).contains(&plan.output_duration_ms));
        // Round trip exactly what the durable queue will store; metadata is not a signed URL.
        let serialized = serde_json::to_string(&plan).unwrap();
        assert!(!serialized.contains("http"));
        let plan = serde_json::from_str(&serialized).unwrap();
        let output = dir
            .path()
            .join(format!("clip-{hevc}-{bframes}-{negative}.mp4"));
        let mut file = fs::File::create(&output).unwrap();
        let result = native::render(&source, &plan, &mut file, &RenderControl::default())
            .await
            .unwrap();
        drop(file);
        assert_eq!(fs::metadata(&output).unwrap().len(), result.output_bytes);
        assert!(
            source.received() < source.length,
            "native clip fetched full file: {} / {}",
            source.received(),
            source.length
        );
        let original_frames = decode(&c, &input);
        let clipped_frames = decode(&c, &output);
        assert!(
            (111..=114).contains(&clipped_frames.len()),
            "{} frames",
            clipped_frames.len()
        );
        assert_eq!(
            &clipped_frames[..50],
            &original_frames[100..150],
            "keyframe decode content mismatch"
        );
        let output_source = FileSource::new(&output);
        let reparsed = native::probe(&output_source, Limits::default())
            .await
            .unwrap();
        assert_eq!(reparsed.info.tracks.len(), 3);
        assert_eq!(reparsed.info.duration_ms, result.output_duration_ms);
        // Decode every retained audio track as well; no unexamined extra track is silently dropped.
        let output = Command::new(&c.executable)
            .args(["-hide_banner", "-loglevel", "error", "-xerror", "-i"])
            .arg(&output)
            .args(["-map", "0", "-f", "null", "-"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[tokio::test]
#[ignore = "explicit FFmpeg fixture generator/decode oracle required"]
async fn native_plan_rejects_changed_identity_bad_bounds_and_observes_cancel() {
    let c = config();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.mp4");
    generate(&c, &input, false, true, false);
    let source = FileSource::new(&input);
    let index = native::probe(&source, Limits::default()).await.unwrap();
    let plan = index
        .plan(ClipRange {
            start_ms: 5000,
            end_ms: 8500,
        })
        .unwrap();
    let mut file = tempfile::tempfile().unwrap();
    let control = RenderControl::default();
    control.cancel();
    assert_eq!(
        native::render(&source, &plan, &mut file, &control)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert_eq!(file.metadata().unwrap().len(), 0);
    let mut tampered = serde_json::to_value(&plan).unwrap();
    tampered["spans"][0]["offset"] = serde_json::Value::from(u64::MAX);
    let tampered = serde_json::from_value(tampered).unwrap();
    assert_eq!(
        native::render(&source, &tampered, &mut file, &RenderControl::default())
            .await
            .unwrap_err()
            .code,
        ErrorCode::IntegrityFailed
    );
    let mut tampered = plan.clone();
    tampered.source_identity = "changed-source".into();
    assert!(
        native::render(&source, &tampered, &mut file, &RenderControl::default())
            .await
            .is_err()
    );
    let fail = FailSource {
        length: source.length,
    };
    assert!(native::probe(&fail, Limits::default()).await.is_err());
    // Cancellation after header delivery must stop before the next source read.
    let slot = std::sync::Arc::new(Mutex::new(None::<RenderControl>));
    let listener = slot.clone();
    let control = RenderControl::with_progress(move |_, _| {
        listener.lock().unwrap().as_ref().unwrap().cancel()
    });
    *slot.lock().unwrap() = Some(control.clone());
    let before = source.received();
    assert_eq!(
        native::render(&source, &plan, &mut file, &control)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert_eq!(source.received(), before);
    assert!(file.metadata().unwrap().len() < plan.output_bytes);
    slot.lock().unwrap().take();
}

#[tokio::test]
#[ignore = "explicit FFmpeg fixture generator/decode oracle required"]
async fn native_matroska_clips_cues_without_fetching_the_whole_source() {
    let c = config();
    let dir = tempfile::tempdir().unwrap();
    for bframes in [false, true] {
        let input = dir.path().join(format!("source-{bframes}.mkv"));
        generate(&c, &input, false, bframes, false);
        let source = FileSource::new(&input);
        let index = native::probe(&source, Limits::default()).await.unwrap();
        assert_eq!(index.info.tracks.len(), 3);
        assert!(source.received() < source.length / 3);
        let plan = native::prepare(
            &source,
            &index,
            ClipRange {
                start_ms: 5000,
                end_ms: 8500,
            },
        )
        .await
        .unwrap();
        assert_eq!(plan.output_format, "mkv");
        assert!((3900..=4100).contains(&plan.aligned_start_ms));
        let output = dir.path().join(format!("clip-{bframes}.mkv"));
        let mut f = fs::File::create(&output).unwrap();
        native::render(&source, &plan, &mut f, &RenderControl::default())
            .await
            .unwrap();
        drop(f);
        assert!(
            source.received() < source.length,
            "{} / {}",
            source.received(),
            source.length
        );
        let original_frames = decode(&c, &input);
        let clipped_frames = decode(&c, &output);
        assert!(
            (110..=114).contains(&clipped_frames.len()),
            "{} frames",
            clipped_frames.len()
        );
        assert_eq!(&clipped_frames[..50], &original_frames[100..150]);
        let out = Command::new(&c.executable)
            .args(["-hide_banner", "-loglevel", "error", "-xerror", "-i"])
            .arg(&output)
            .args(["-map", "0", "-f", "null", "-"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[tokio::test]
#[ignore = "explicit FFmpeg fixture generator/decode oracle required"]
async fn native_ts_uses_real_pts_and_keyframes_for_h264_and_hevc() {
    let c = config();
    let dir = tempfile::tempdir().unwrap();
    for (hevc, bframes) in [(false, false), (true, false), (false, true), (true, true)] {
        let input = dir.path().join(format!("source-{hevc}-{bframes}.ts"));
        generate(&c, &input, hevc, bframes, false);
        let source = FileSource::new(&input);
        let index = native::probe(&source, Limits::default()).await.unwrap();
        assert_eq!(index.info.tracks.len(), 3);
        assert!(source.received() < source.length / 2);
        assert!(
            (11900..=12100).contains(&index.info.duration_ms),
            "{}",
            index.info.duration_ms
        );
        let plan = native::prepare(
            &source,
            &index,
            ClipRange {
                start_ms: 5000,
                end_ms: 8500,
            },
        )
        .await
        .unwrap();
        assert_eq!(plan.output_format, "ts");
        assert!(
            (3900..=4100).contains(&plan.aligned_start_ms),
            "{}",
            plan.aligned_start_ms
        );
        let output = dir.path().join(format!("clip-{hevc}-{bframes}.ts"));
        let mut f = fs::File::create(&output).unwrap();
        native::render(&source, &plan, &mut f, &RenderControl::default())
            .await
            .unwrap();
        drop(f);
        assert!(
            source.received() < source.length,
            "{} / {}",
            source.received(),
            source.length
        );
        let original_frames = decode(&c, &input);
        let clipped_frames = decode(&c, &output);
        assert!(
            (110..=114).contains(&clipped_frames.len()),
            "{} frames",
            clipped_frames.len()
        );
        assert_eq!(&clipped_frames[..50], &original_frames[100..150]);
        let out = Command::new(&c.executable)
            .args(["-hide_banner", "-loglevel", "error", "-xerror", "-i"])
            .arg(&output)
            .args(["-map", "0", "-f", "null", "-"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[tokio::test]
#[ignore = "explicit FFmpeg fixture generator/decode oracle required"]
async fn native_ts_remuxes_h264_hevc_and_all_aac_tracks_to_faststart_mp4() {
    let c = config();
    let dir = tempfile::tempdir().unwrap();
    for (hevc, bframes) in [(false, false), (false, true), (true, false), (true, true)] {
        let input = dir.path().join(format!("source-{hevc}-{bframes}.ts"));
        generate(&c, &input, hevc, bframes, false);
        let source = FileSource::new(&input);
        let index = native::probe(&source, Limits::default()).await.unwrap();
        let plan = native::prepare_format(
            &source,
            &index,
            ClipRange {
                start_ms: 5000,
                end_ms: 8500,
            },
            "mp4",
        )
        .await
        .unwrap();
        let output = dir.path().join(format!("clip-{hevc}-{bframes}.mp4"));
        let mut f = fs::File::create(&output).unwrap();
        native::render(&source, &plan, &mut f, &RenderControl::default())
            .await
            .unwrap();
        drop(f);
        let original = decode(&c, &input);
        let clipped = decode(&c, &output);
        assert_eq!(&clipped[..50], &original[100..150]);
        assert!((110..=114).contains(&clipped.len()), "{}", clipped.len());
        let index = native::probe(&FileSource::new(&output), Limits::default())
            .await
            .unwrap();
        assert_eq!(index.info.tracks.len(), 3);
        assert_eq!(index.info.tracks[0].width, Some(160));
        assert_eq!(
            index
                .info
                .tracks
                .iter()
                .filter(|t| t.language == "eng" || t.language == "jpn")
                .count(),
            2
        );
        let out = Command::new(&c.executable)
            .args(["-hide_banner", "-loglevel", "error", "-xerror", "-i"])
            .arg(&output)
            .args(["-map", "0", "-f", "null", "-"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[tokio::test]
#[ignore = "explicit FFmpeg fixture generator/decode oracle required"]
async fn native_whole_ts_remux_preserves_every_video_frame() {
    let c = config();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("whole.ts");
    generate(&c, &input, true, true, false);
    let source = FileSource::new(&input);
    let index = native::probe(&source, Limits::default()).await.unwrap();
    let plan = native::prepare_format(
        &source,
        &index,
        ClipRange {
            start_ms: 0,
            end_ms: index.info.duration_ms,
        },
        "mp4",
    )
    .await
    .unwrap();
    let output = dir.path().join("whole.mp4");
    let mut file = fs::File::create(&output).unwrap();
    native::render(&source, &plan, &mut file, &RenderControl::default())
        .await
        .unwrap();
    drop(file);
    assert_eq!(decode(&c, &output), decode(&c, &input));
}

#[tokio::test]
#[ignore = "explicit FFmpeg fixture generator/decode oracle required"]
async fn native_matroska_remuxes_h264_hevc_multiaudio_to_mp4() {
    let c = config();
    let dir = tempfile::tempdir().unwrap();
    for (hevc, bframes) in [(false, false), (false, true), (true, true)] {
        let input = dir.path().join(format!("source-{hevc}-{bframes}.mkv"));
        generate(&c, &input, hevc, bframes, false);
        let source = FileSource::new(&input);
        let index = native::probe(&source, Limits::default()).await.unwrap();
        let plan = native::prepare_format(
            &source,
            &index,
            ClipRange {
                start_ms: 5000,
                end_ms: 8500,
            },
            "mp4",
        )
        .await
        .unwrap();
        let output = dir.path().join(format!("clip-{hevc}-{bframes}.mp4"));
        let mut f = fs::File::create(&output).unwrap();
        native::render(&source, &plan, &mut f, &RenderControl::default())
            .await
            .unwrap();
        drop(f);
        let original = decode(&c, &input);
        let clipped = decode(&c, &output);
        assert_eq!(&clipped[..50], &original[100..150]);
        assert!((110..=114).contains(&clipped.len()), "{}", clipped.len());
        let index = native::probe(&FileSource::new(&output), Limits::default())
            .await
            .unwrap();
        assert_eq!(index.info.tracks.len(), 3);
        let out = Command::new(&c.executable)
            .args(["-hide_banner", "-loglevel", "error", "-xerror", "-i"])
            .arg(&output)
            .args(["-map", "0", "-f", "null", "-"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
struct FailSource {
    length: u64,
}
impl RangeSource for FailSource {
    fn len(&self) -> u64 {
        self.length
    }
    fn identity(&self) -> &str {
        "immutable-synthetic-file-v1"
    }
    fn read_exact(&self, _: u64, length: usize) -> RangeFuture<'_> {
        Box::pin(async move { Ok(vec![0; length.saturating_sub(1)]) })
    }
}

#[tokio::test]
async fn invalid_media_and_limits_are_rejected_without_panicking() {
    let source = FailSource { length: 4096 };
    assert_eq!(
        native::probe(
            &source,
            Limits {
                max_metadata_bytes: usize::MAX,
                ..Limits::default()
            }
        )
        .await
        .err()
        .unwrap()
        .code,
        ErrorCode::IntegrityFailed
    );
    assert!(native::probe(&source, Limits::default()).await.is_err());
    let _unused: Option<ClientError> = None;
}
