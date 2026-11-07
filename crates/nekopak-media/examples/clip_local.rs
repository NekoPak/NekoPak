use nekopak_media::{render_clip, ClipRange, FfmpegConfig};
use std::{path::PathBuf, time::Duration};

#[tokio::main]
async fn main() {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.len() != 6 {
        eprintln!("Usage: clip_local <ffmpeg.exe> <sha256> <source.mp4|mkv> <new-output.mp4> <start_ms> <end_ms>");
        std::process::exit(2);
    }
    let (Ok(start_ms), Ok(end_ms)) = (arguments[4].parse::<u64>(), arguments[5].parse::<u64>())
    else {
        eprintln!("Times must be nonnegative integer milliseconds.");
        std::process::exit(2);
    };
    let config = FfmpegConfig {
        executable: PathBuf::from(&arguments[0]),
        expected_sha256: arguments[1].clone(),
        timeout: Duration::from_secs(1800),
    };
    match render_clip(
        &config,
        &PathBuf::from(&arguments[2]),
        &PathBuf::from(&arguments[3]),
        ClipRange { start_ms, end_ms },
        None,
    )
    .await
    {
        Ok(result) => {
            println!(
                "Output bytes: {}\nOutput duration ms: {}\nSHA256: {}",
                result.output_bytes, result.output_duration_ms, result.output_sha256
            );
            for warning in result.warnings {
                println!("{warning}");
            }
        }
        Err(error) => {
            eprintln!("{:?}: {}", error.code, error.message);
            std::process::exit(1);
        }
    }
}
