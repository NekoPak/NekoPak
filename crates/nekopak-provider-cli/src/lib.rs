//! Explicitly configured, checksum-pinned external CLI. Never installs, updates or authorizes itself.
mod process;
mod wire;

use async_trait::async_trait;
use nekopak_domain::*;
use nekopak_provider::{unsupported, ProviderPort};
use process::{error, CliRunner, PinnedProcess};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub const REVIEWED_VERSION: &str = "0.6.0";
pub const WINDOWS_X64_SHA256: &str =
    "b39a2bf29d69e561ab3a6f7ec2bfd899930008ed311eb3bd99c3697802e93b04";

pub struct OfficialCliProvider {
    runner: Option<Arc<dyn CliRunner>>,
    preview_enabled: bool,
    /// Help validation is reused only for the pinned executable; every invocation rechecks its hash.
    probe_cache: tokio::sync::Mutex<Option<CliProbeReport>>,
}

impl OfficialCliProvider {
    pub fn not_configured() -> Self {
        Self {
            runner: None,
            preview_enabled: false,
            probe_cache: tokio::sync::Mutex::new(None),
        }
    }

    pub fn configured(
        executable: &Path,
        profile: &Path,
        preview_enabled: bool,
    ) -> Result<Self, ClientError> {
        if !cfg!(all(windows, target_arch = "x86_64")) {
            return Err(error(
                ErrorCode::Unsupported,
                "Only the reviewed Windows x64 CLI binary is supported by this adapter.",
            ));
        }
        Ok(Self {
            runner: Some(Arc::new(PinnedProcess::new(
                executable,
                profile,
                WINDOWS_X64_SHA256,
            )?)),
            preview_enabled,
            probe_cache: tokio::sync::Mutex::new(None),
        })
    }

    pub async fn probe(&self) -> Result<CliProbeReport, ClientError> {
        let Some(runner) = &self.runner else {
            return Ok(CliProbeReport { backend: BackendId::OfficialCli, status: BackendStatus::NotInstalled, version: None, binary_sha256: None, verified_commands: Vec::new(), live_account_verified: false, limitations: vec!["No reviewed executable/profile is configured; PATH is not searched or executed.".into()], checked_at: now() });
        };
        let mut cache = self.probe_cache.lock().await;
        if let Some(report) = &*cache {
            return Ok(report.clone());
        }
        let version = runner.run(&args(&["version"])).await?;
        if !version.success || version.stdout != b"pikpak version v0.6.0\n" {
            return Err(error(
                ErrorCode::Unsupported,
                "The CLI version/output differs from the reviewed v0.6.0 protocol.",
            ));
        }
        let checks = [
            (
                vec!["ls", "--help"],
                vec!["--raw", "--parent", "--limit", "--token", "--format"],
            ),
            (vec!["get", "--help"], vec!["--raw", "--format"]),
            (
                vec!["auth", "status", "--help"],
                vec!["auth status", "--format"],
            ),
        ];
        for (arguments, required) in checks {
            let output = runner.run(&args(&arguments)).await?;
            let text = std::str::from_utf8(&output.stdout).map_err(|_| wire::invalid())?;
            if !output.success || !required.iter().all(|flag| text.contains(flag)) {
                return Err(error(
                    ErrorCode::Unsupported,
                    "The CLI help does not expose the required read-only protocol flags.",
                ));
            }
        }
        let report = CliProbeReport { backend: BackendId::OfficialCli, status: BackendStatus::Unavailable,
            version: Some(REVIEWED_VERSION.into()), binary_sha256: Some(WINDOWS_X64_SHA256.into()),
            verified_commands: vec!["version".into(), "ls --raw -F json".into(), "get --raw -F json".into(), "auth status -F json".into()],
            live_account_verified: false,
            limitations: vec!["Checksum, help flags and unauthenticated JSON errors were verified. Successful account listing is not yet live-tested.".into(), "Read-only preview needs an explicitly configured externally authorized profile. The CLI owns its credential storage; NekoPak does not import tokens.".into(), "Download, cloud writes, account quota and random reads are disabled. No automatic login, install, update or fallback.".into()],
            checked_at: now() };
        *cache = Some(report.clone());
        Ok(report)
    }

    fn runner(&self) -> Result<&Arc<dyn CliRunner>, ClientError> {
        if !self.preview_enabled {
            return Err(error(
                ErrorCode::Unsupported,
                "Explicit opt-in is required for the CLI read-only preview.",
            ));
        }
        self.runner.as_ref().ok_or_else(|| {
            error(
                ErrorCode::NotFound,
                "No CLI executable/profile is configured.",
            )
        })
    }
    async fn execute(&self, arguments: &[String]) -> Result<serde_json::Value, ClientError> {
        let runner = self.runner()?;
        self.probe().await?;
        wire::parse_output(runner.run(arguments).await?)
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}
fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("UTC timestamp")
}

#[async_trait]
impl ProviderPort for OfficialCliProvider {
    fn id(&self) -> BackendId {
        BackendId::OfficialCli
    }

    async fn discover(&self) -> Result<CapabilityReport, ClientError> {
        let probe = self.probe().await?;
        let status = if self.runner.is_none() {
            BackendStatus::NotInstalled
        } else if !self.preview_enabled {
            BackendStatus::Unavailable
        } else {
            match self.auth_status().await {
                Ok(auth) if auth.authenticated => BackendStatus::Ready,
                Ok(_) => BackendStatus::NeedsLogin,
                Err(error) if error.code == ErrorCode::AuthExpired => BackendStatus::NeedsLogin,
                Err(error) => return Err(error),
            }
        };
        let enabled = status == BackendStatus::Ready;
        Ok(CapabilityReport {
            backend: BackendId::OfficialCli,
            status,
            operations: BTreeMap::from([
                (Operation::List, enabled),
                (Operation::Stat, enabled),
                (Operation::ReadRange, false),
                (Operation::SegmentDownload, false),
                (Operation::Download, false),
                (Operation::CloudTask, false),
                (Operation::Upload, false),
                (Operation::Quota, false),
            ]),
            account_tier: Some(AccountTier::Unknown),
            version: probe.version,
            limitations: probe.limitations,
            checked_at: now(),
        })
    }

    async fn auth_status(&self) -> Result<AuthState, ClientError> {
        wire::auth(
            self.execute(&args(&["auth", "status", "-F", "json"]))
                .await?,
        )
    }

    async fn list(
        &self,
        parent: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<RemoteEntry>, ClientError> {
        wire::validate_id(parent, true)?;
        if !(10..=500).contains(&limit) {
            return Err(error(ErrorCode::Conflict, "The official CLI requires page limits from 10 to 500; smaller limits are rounded up by the service."));
        }
        let mut arguments = args(&["ls", "--raw", "-F", "json"]);
        // Flag=value prevents page tokens from being interpreted as new options. No shell is used.
        arguments.push(format!("--limit={limit}"));
        if parent != "root" {
            arguments.push(format!("--parent={parent}"));
        }
        if let Some(cursor) = cursor {
            wire::validate_cursor(cursor)?;
            arguments.push(format!("--token={cursor}"));
        }
        wire::list(self.execute(&arguments).await?, parent, cursor, limit)
    }

    async fn stat(&self, id: &str) -> Result<RemoteEntry, ClientError> {
        wire::validate_id(id, false)?;
        if id == "root" {
            return Err(unsupported(BackendId::OfficialCli));
        }
        let entry = wire::entry(
            &self
                .execute(&args(&["get", id, "--raw", "-F", "json"]))
                .await?,
        )?;
        if entry.id != id {
            return Err(wire::invalid());
        }
        Ok(entry)
    }

    async fn quota(&self) -> Result<QuotaReport, ClientError> {
        Err(error(ErrorCode::Unsupported, "CLI auth status exposes storage usage, not verified connected-app transfer quotas. No quota was inferred."))
    }
}

#[cfg(test)]
mod tests;
