//! Requires explicit external tools. Run with --ignored; absence fails instead of silently skipping.
use nekopak_domain::ErrorCode;
use nekopak_media::{render_clip, ClipRange, FfmpegConfig};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

fn configuration() -> FfmpegConfig {
    FfmpegConfig {
        executable: PathBuf::from(
            std::env::var_os("NEKOPAK_TEST_FFMPEG").expect("set NEKOPAK_TEST_FFMPEG"),
        ),
        expected_sha256: std::env::var("NEKOPAK_TEST_FFMPEG_SHA256")
            .expect("set reviewed tool SHA-256"),
        timeout: Duration::from_secs(30),
    }
}
fn ffmpeg(config: &FfmpegConfig, args: &[&str], paths: &[&Path]) {
    let mut command = Command::new(&config.executable);
    command.args(args).args(paths).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().expect("launch test fixture generator");
    assert!(
        output.status.success(),
        "fixture command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn generate(config: &FfmpegConfig, destination: &Path) {
    config.verify().unwrap();
    ffmpeg(
        config,
        &[
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
            "-c:v",
            "libx264",
            "-g",
            "50",
            "-keyint_min",
            "50",
            "-sc_threshold",
            "0",
            "-bf",
            "0",
            "-c:a",
            "aac",
            "-shortest",
            "-y",
        ],
        &[destination],
    );
}
fn no_temps(directory: &Path) -> bool {
    fs::read_dir(directory).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".nekopak-clip-")
    })
}
async fn wait_cleanup(directory: &Path) {
    for _ in 0..100 {
        if no_temps(directory) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("temporary media file was not cleaned");
}

#[tokio::test]
#[ignore = "explicit external FFmpeg required; run scripts/test-media.ps1"]
async fn mkv_and_mp4_produce_decodable_keyframe_aligned_mp4() {
    let config = configuration();
    let directory = tempfile::tempdir().unwrap();
    for extension in ["mkv", "mp4"] {
        let source = directory.path().join(format!("source.{extension}"));
        generate(&config, &source);
        let destination = directory.path().join(format!("clip-{extension}.mp4"));
        let result = render_clip(
            &config,
            &source,
            &destination,
            ClipRange {
                start_ms: 5_000,
                end_ms: 8_500,
            },
            Some(12_000),
        )
        .await
        .unwrap();
        // Generated GOP = 2 seconds. Start 5s requires prior keyframe near 4s.
        assert!(
            (4_000..=5_000).contains(&result.output_duration_ms),
            "{extension}: {}ms",
            result.output_duration_ms
        );
        assert!(
            result.output_bytes > 0 && result.output_bytes < fs::metadata(&source).unwrap().len()
        );
        assert_eq!(result.output_sha256.len(), 64);
        let args = ["-hide_banner", "-loglevel", "error", "-xerror", "-i"];
        let output = Command::new(&config.executable)
            .args(args)
            .arg(&destination)
            .args(["-f", "null", "-"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(no_temps(directory.path()));
    }
}

#[tokio::test]
#[ignore = "explicit external FFmpeg required; run scripts/test-media.ps1"]
async fn ts_is_explicitly_unsupported_until_seek_parity_is_met() {
    let config = configuration();
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.ts");
    generate(&config, &source);
    let output = directory.path().join("clip.mp4");
    let error = render_clip(
        &config,
        &source,
        &output,
        ClipRange {
            start_ms: 5_000,
            end_ms: 8_500,
        },
        Some(12_000),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert!(!output.exists());
    assert!(no_temps(directory.path()));
}

#[tokio::test]
#[ignore = "explicit external FFmpeg required; run scripts/test-media.ps1"]
async fn failure_timeout_and_conflict_do_not_replace_user_data() {
    let mut config = configuration();
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("broken.mkv");
    fs::write(&source, [0x1a, 0x45, 0xdf, 0xa3, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
    let destination = directory.path().join("clip.mp4");
    assert!(render_clip(
        &config,
        &source,
        &destination,
        ClipRange {
            start_ms: 1_000,
            end_ms: 2_000
        },
        Some(12_000)
    )
    .await
    .is_err());
    assert!(
        no_temps(directory.path()),
        "ordinary failure must clean before returning"
    );
    assert!(!destination.exists());
    fs::write(&destination, b"existing file must stay").unwrap();
    assert_eq!(
        render_clip(
            &config,
            &source,
            &destination,
            ClipRange {
                start_ms: 1_000,
                end_ms: 2_000
            },
            Some(12_000)
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    assert_eq!(fs::read(&destination).unwrap(), b"existing file must stay");
    let good = directory.path().join("good.mkv");
    generate(&config, &good);
    config.timeout = Duration::ZERO;
    let timed = directory.path().join("timed.mp4");
    assert!(render_clip(
        &config,
        &good,
        &timed,
        ClipRange {
            start_ms: 1_000,
            end_ms: 2_000
        },
        Some(12_000)
    )
    .await
    .is_err());
    assert!(
        no_temps(directory.path()),
        "timeout must reap and clean before returning"
    );
    assert!(!timed.exists());
}

#[tokio::test]
#[ignore = "explicit external FFmpeg required; run scripts/test-media.ps1"]
async fn audio_only_and_local_playlist_inputs_are_rejected() {
    let config = configuration();
    let directory = tempfile::tempdir().unwrap();
    let audio = directory.path().join("audio.mp4");
    ffmpeg(
        &config,
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
            "-c:a",
            "aac",
            "-y",
        ],
        &[&audio],
    );
    let destination = directory.path().join("video.mp4");
    assert!(render_clip(
        &config,
        &audio,
        &destination,
        ClipRange {
            start_ms: 0,
            end_ms: 1_000
        },
        Some(3_000)
    )
    .await
    .is_err());
    wait_cleanup(directory.path()).await;
    let playlist = directory.path().join("input.m3u8");
    fs::write(
        &playlist,
        b"#EXTM3U\n#EXTINF:1\nhttps://example.invalid/private\n",
    )
    .unwrap();
    assert!(render_clip(
        &config,
        &playlist,
        &destination,
        ClipRange {
            start_ms: 0,
            end_ms: 1_000
        },
        Some(3_000)
    )
    .await
    .is_err());
    wait_cleanup(directory.path()).await;
    assert!(!destination.exists());
}

#[tokio::test]
#[ignore = "explicit external FFmpeg required; run scripts/test-media.ps1"]
async fn cancelling_active_render_reaps_process_and_removes_its_temp() {
    let config = configuration();
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mkv");
    generate(&config, &source);
    let destination = directory.path().join("cancelled.mp4");
    let worker_destination = destination.clone();
    let task = tokio::spawn(async move {
        render_clip(
            &config,
            &source,
            &worker_destination,
            ClipRange {
                start_ms: 0,
                end_ms: 12_000,
            },
            Some(12_000),
        )
        .await
    });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while no_temps(directory.path()) {
        assert!(
            !task.is_finished(),
            "render completed before cancellation could be observed"
        );
        assert!(
            tokio::time::Instant::now() < deadline,
            "render never started"
        );
        tokio::task::yield_now().await;
    }
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    wait_cleanup(directory.path()).await;
    assert!(!destination.exists());
}

#[tokio::test]
#[ignore = "explicit external FFmpeg required; run scripts/test-media.ps1"]
async fn unknown_duration_or_end_beyond_source_is_rejected_before_publish() {
    let config = configuration();
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.mkv");
    generate(&config, &source);
    let output = directory.path().join("clip.mp4");
    let range = ClipRange {
        start_ms: 11_000,
        end_ms: 13_000,
    };
    assert_eq!(
        render_clip(&config, &source, &output, range, None)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        render_clip(&config, &source, &output, range, Some(12_000))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert!(!output.exists());
    assert!(no_temps(directory.path()));
}
