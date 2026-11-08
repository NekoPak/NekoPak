//! Explicit media fixture generator is only an oracle; product export is Rust.
use nekopak_core::{AppService, TransferOptions};
use nekopak_domain::{TaskState, TransferAction};
use nekopak_provider::RequestContext;
use nekopak_provider_mock::Scenario;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

#[tokio::test]
#[ignore = "explicit reviewed FFmpeg fixture generator required"]
async fn native_media_queue_persists_restarts_and_publishes_decodable_clip() {
    let ffmpeg = PathBuf::from(std::env::var_os("NEKOPAK_TEST_FFMPEG").expect("explicit oracle"));
    nekopak_media::FfmpegConfig {
        executable: ffmpeg.clone(),
        expected_sha256: std::env::var("NEKOPAK_TEST_FFMPEG_SHA256").unwrap(),
        timeout: Duration::from_secs(30),
    }
    .verify()
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.mp4");
    let output = dir.path().join("output");
    std::fs::create_dir(&output).unwrap();
    let mut generate = Command::new(&ffmpeg);
    generate
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=25:duration=12",
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
            "2",
            "-c:a",
            "aac",
            "-y",
        ])
        .arg(&source)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        generate.creation_flags(0x08000000);
    }
    let generated = generate.output().unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let database = dir.path().join("tasks.db");
    let app = Arc::new(AppService::mock(&database, Scenario::Free));
    let local = app.local_sources_add(dir.path().to_str().unwrap()).unwrap();
    let entry = app
        .local_list(&local.root_id, None, None)
        .unwrap()
        .items
        .into_iter()
        .find(|e| e.name == "source.mp4")
        .unwrap();
    let ctx = RequestContext::new(Duration::from_secs(45));
    let probe = app.media_probe(&entry.id, &ctx).await.unwrap();
    assert_eq!(probe["duration_ms"], 12000);
    let options:TransferOptions=serde_json::from_value(serde_json::json!({"quality_id":"original","clip":{"start_ms":5000,"end_ms":8500,"align":"previous_keyframe"}})).unwrap();
    let task = app
        .media_start(
            &entry.id,
            output.to_str().unwrap(),
            &options,
            Some("native-clip"),
            &ctx,
        )
        .await
        .unwrap();
    app.transfer_control(&task.task_id, TransferAction::Pause)
        .unwrap();
    drop(app);
    let app = Arc::new(AppService::mock(&database, Scenario::Free));
    assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Paused);
    app.transfer_control(&task.task_id, TransferAction::Resume)
        .unwrap();
    app.run_media().await.unwrap();
    let completed = app.task(&task.task_id).unwrap();
    assert_eq!(
        completed.state,
        TaskState::Completed,
        "{:?}",
        completed.error
    );
    assert_eq!(completed.total_bytes, Some(completed.transferred_bytes));
    let target = PathBuf::from(completed.destination.unwrap());
    assert!(
        target.exists(),
        "target {} missing; actual {:?}",
        target.display(),
        std::fs::read_dir(&output)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>()
    );
    let mut decode = Command::new(&ffmpeg);
    decode
        .args(["-hide_banner", "-loglevel", "error", "-xerror", "-i"])
        .arg(&target)
        .args(["-map", "0:v", "-map", "0:a", "-f", "null", "-"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        decode.creation_flags(0x08000000);
    }
    let decoded = decode.output().unwrap();
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    let replay = app
        .media_start(
            &entry.id,
            output.to_str().unwrap(),
            &options,
            Some("native-clip"),
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(replay.task_id, task.task_id);
    assert!(app
        .media_start(
            &entry.id,
            output.to_str().unwrap(),
            &options,
            Some("other-clip"),
            &ctx
        )
        .await
        .is_err());
}
