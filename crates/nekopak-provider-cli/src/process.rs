use async_trait::async_trait;
use nekopak_domain::{ClientError, ErrorCode};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::Semaphore,
};

pub(crate) struct Output {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[async_trait]
pub(crate) trait CliRunner: Send + Sync {
    async fn run(&self, arguments: &[String]) -> Result<Output, ClientError>;
}

pub(crate) struct PinnedProcess {
    executable: PathBuf,
    profile: PathBuf,
    expected_sha256: String,
    semaphore: Semaphore,
    timeout: Duration,
    max_bytes: usize,
}

impl PinnedProcess {
    pub fn new(
        executable: &Path,
        profile: &Path,
        expected_sha256: &str,
    ) -> Result<Self, ClientError> {
        if !executable.is_absolute() || !profile.is_absolute() {
            return Err(error(
                ErrorCode::Conflict,
                "CLI executable and profile must use absolute paths.",
            ));
        }
        if !profile.is_dir() {
            return Err(error(
                ErrorCode::NotFound,
                "The explicitly configured CLI profile directory does not exist.",
            ));
        }
        let executable = executable.canonicalize().map_err(|_| {
            error(
                ErrorCode::NotFound,
                "The configured CLI executable does not exist.",
            )
        })?;
        let profile = profile.canonicalize().map_err(|_| {
            error(
                ErrorCode::NotFound,
                "The configured CLI profile is inaccessible.",
            )
        })?;
        let process = Self {
            executable,
            profile,
            expected_sha256: expected_sha256.into(),
            semaphore: Semaphore::new(1),
            timeout: Duration::from_secs(10),
            max_bytes: 4 * 1024 * 1024,
        };
        process.open_verified()?;
        Ok(process)
    }

    /// On Windows, holding this handle disallows replacing or writing the file during execution.
    fn open_verified(&self) -> Result<File, ClientError> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let mut file = options.open(&self.executable).map_err(|_| {
            error(
                ErrorCode::PermissionDenied,
                "The pinned CLI executable cannot be opened safely.",
            )
        })?;
        let metadata = file.metadata().map_err(|_| {
            error(
                ErrorCode::IntegrityFailed,
                "CLI executable metadata is unavailable.",
            )
        })?;
        if !metadata.is_file() || metadata.len() > 128 * 1024 * 1024 {
            return Err(error(
                ErrorCode::IntegrityFailed,
                "CLI executable is not a supported binary.",
            ));
        }
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(|_| {
                error(
                    ErrorCode::IntegrityFailed,
                    "Cannot verify the CLI executable.",
                )
            })?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
        }
        if format!("{:x}", hash.finalize()) != self.expected_sha256 {
            return Err(error(ErrorCode::IntegrityFailed, "CLI checksum differs from the reviewed release. Review the new binary before enabling it."));
        }
        Ok(file)
    }
}

#[async_trait]
impl CliRunner for PinnedProcess {
    async fn run(&self, arguments: &[String]) -> Result<Output, ClientError> {
        let work = async {
            let _permit = self
                .semaphore
                .acquire()
                .await
                .map_err(|_| error(ErrorCode::Internal, "CLI process gate is unavailable."))?;
            let _executable_guard = self.open_verified()?;
            let mut command = Command::new(&self.executable);
            command
                .args(arguments)
                .current_dir(&self.profile)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .env_clear();
            // No inherited PAT/credential overrides, referral codes or agent configuration.
            for key in [
                "SystemRoot",
                "WINDIR",
                "TEMP",
                "TMP",
                "USERPROFILE",
                "HOME",
                "LOCALAPPDATA",
                "APPDATA",
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "NO_PROXY",
                "SSL_CERT_FILE",
                "SSL_CERT_DIR",
            ] {
                if let Some(value) = std::env::var_os(key) {
                    command.env(key, value);
                }
            }
            command
                .env("PIKPAK_DIR", &self.profile)
                .env("PIKPAK_NO_UPDATE_NOTIFIER", "1")
                .env("PIKPAK_NO_LOG", "1")
                .env("PIKPAK_NO_SKILL_SYNC", "1");
            #[cfg(windows)]
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW; never launch a terminal.
            let mut child = command.spawn().map_err(|_| {
                error(
                    ErrorCode::PermissionDenied,
                    "The verified CLI could not be started.",
                )
            })?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| error(ErrorCode::Internal, "CLI stdout pipe is unavailable."))?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| error(ErrorCode::Internal, "CLI stderr pipe is unavailable."))?;
            let (stdout, stderr, status) = tokio::try_join!(
                bounded_read(stdout, self.max_bytes),
                bounded_read(stderr, self.max_bytes),
                async {
                    child.wait().await.map_err(|_| {
                        error(ErrorCode::Internal, "The CLI process could not be reaped.")
                    })
                }
            )?;
            Ok(Output {
                success: status.success(),
                stdout,
                stderr,
            })
        };
        tokio::time::timeout(self.timeout, work)
            .await
            .unwrap_or_else(|_| {
                Err(error(
                    ErrorCode::TransientNetwork,
                    "The CLI request timed out; its process was terminated.",
                ))
            })
    }
}

async fn bounded_read(
    reader: impl AsyncRead + Unpin,
    maximum: usize,
) -> Result<Vec<u8>, ClientError> {
    let mut bytes = Vec::new();
    reader
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| error(ErrorCode::Internal, "Cannot read the CLI response."))?;
    if bytes.len() > maximum {
        return Err(error(
            ErrorCode::IntegrityFailed,
            "CLI output exceeded the safety limit; the operation was stopped.",
        ));
    }
    Ok(bytes)
}
pub(crate) fn error(code: ErrorCode, message: &'static str) -> ClientError {
    ClientError::new(code, message).for_backend(nekopak_domain::BackendId::OfficialCli)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn own_process(profile: &Path) -> PinnedProcess {
        let executable = std::env::current_exe().unwrap();
        let hash = format!("{:x}", Sha256::digest(std::fs::read(&executable).unwrap()));
        PinnedProcess::new(&executable, profile, &hash).unwrap()
    }
    #[test]
    fn child_flood() {
        if !std::env::args().any(|arg| arg == "--exact") {
            return;
        }
        std::io::stdout()
            .write_all(&vec![b'x'; 1024 * 1024])
            .unwrap();
    }
    #[test]
    fn child_wait() {
        if !std::env::args().any(|arg| arg == "--exact") {
            return;
        }
        std::fs::write("started", b"yes").unwrap();
        std::thread::sleep(Duration::from_millis(700));
        std::fs::write("survived", b"bad").unwrap();
    }
    fn helper(name: &str) -> Vec<String> {
        vec![
            "--exact".into(),
            format!("process::tests::{name}"),
            "--nocapture".into(),
        ]
    }
    #[tokio::test]
    async fn process_output_is_bounded_and_hash_is_checked() {
        let dir = tempfile::tempdir().unwrap();
        let mut process = own_process(dir.path());
        process.max_bytes = 2048;
        assert_eq!(
            process
                .run(&helper("child_flood"))
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::IntegrityFailed
        );
        process.expected_sha256 = "0".repeat(64);
        assert_eq!(
            process.run(&helper("child_wait")).await.err().unwrap().code,
            ErrorCode::IntegrityFailed
        );
    }
    #[tokio::test]
    async fn timeout_and_cancellation_kill_the_child() {
        for cancel in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut process = own_process(dir.path());
            process.timeout = Duration::from_millis(450);
            if cancel {
                let context = nekopak_provider::RequestContext::default();
                let cancellation = context.clone();
                let marker = dir.path().join("started");
                let cancel_when_started = async move {
                    for _ in 0..100 {
                        if marker.exists() {
                            cancellation.cancel();
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                    panic!("child never started before cancellation");
                };
                let arguments = helper("child_wait");
                let (result, ()) =
                    tokio::join!(context.run(process.run(&arguments)), cancel_when_started);
                assert_eq!(result.err().unwrap().code, ErrorCode::Cancelled);
            } else {
                assert!(process.run(&helper("child_wait")).await.is_err());
            }
            assert!(dir.path().join("started").exists());
            tokio::time::sleep(Duration::from_millis(800)).await;
            assert!(!dir.path().join("survived").exists());
        }
    }
}
