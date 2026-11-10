//! Account-independent updates from signed NekoPak releases. No arbitrary URLs from IPC.
use base64::{engine::general_purpose::STANDARD, Engine};
use nekopak_domain::{ClientError, DecimalU64, ErrorCode};
use nekopak_provider::RequestContext;
use reqwest::Client;
use ring::signature::{UnparsedPublicKey, ED25519};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

const RELEASES: &str = "https://api.github.com/repos/NekoPak/NikoPak/releases?per_page=30";
const PREFIX: &str = "https://github.com/NekoPak/NikoPak/releases/download/";
const MAX_MANIFEST: usize = 128 * 1024;
const MAX_PACKAGE: u64 = 256 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub update_id: String,
    pub version: String,
    pub channel: String,
    pub notes: String,
    pub published_at: String,
    pub size_bytes: DecimalU64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct UpdateStatus {
    pub supported: bool,
    pub phase: String,
    pub current_version: String,
    pub channel: String,
    pub checked_at: Option<String>,
    pub update: Option<UpdateInfo>,
    pub downloaded_bytes: DecimalU64,
    pub error: Option<ClientError>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    name: String,
    bytes: u64,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    product: String,
    version: String,
    channel: String,
    published_at: String,
    notes: String,
    platforms: BTreeMap<String, Artifact>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    payload_base64: String,
    signature: String,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}
#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}
#[derive(Clone)]
struct Candidate {
    public: UpdateInfo,
    artifact: Artifact,
    url: String,
}
struct State {
    public: UpdateStatus,
    candidate: Option<Candidate>,
    staged: Option<PathBuf>,
    cancel: Option<RequestContext>,
}

pub struct UpdateService {
    directory: PathBuf,
    platform: String,
    version: Version,
    channel: String,
    key: Vec<u8>,
    feed: String,
    asset_prefix: String,
    test_origin: Option<String>,
    state: Mutex<State>,
    operation: Arc<tokio::sync::Mutex<()>>,
}
pub struct VerifiedUpdate {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
    pub version: String,
    // Keep the exact file and parent directory pinned through process launch.
    _file: File,
    _parents: Vec<File>,
    _operation: tokio::sync::OwnedMutexGuard<()>,
}
fn failure(code: ErrorCode, message: &'static str) -> ClientError {
    ClientError::new(code, message)
}
fn invalid() -> ClientError {
    failure(
        ErrorCode::IntegrityFailed,
        "更新签名、版本或文件校验失败，已停止更新。",
    )
}
fn unavailable() -> ClientError {
    failure(ErrorCode::Internal, "更新服务暂不可用。")
}
fn network() -> ClientError {
    failure(
        ErrorCode::TransientNetwork,
        "无法连接更新服务器，请检查网络或代理后重试。",
    )
}
fn now() -> String {
    crate::now()
}
fn artifact_name(version: &str, platform: &str) -> Result<String, ClientError> {
    let suffix = match platform {
        "windows-x86_64" => "windows-x64-setup.exe",
        "android-aarch64" => "android-arm64-v8a.apk",
        _ => return Err(invalid()),
    };
    Ok(format!("NekoPak-{version}-{suffix}"))
}

impl UpdateService {
    pub fn production(directory: PathBuf) -> Result<Arc<Self>, ClientError> {
        let platform = if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            "windows-x86_64"
        } else if cfg!(all(target_os = "android", target_arch = "aarch64")) {
            "android-aarch64"
        } else {
            "unsupported"
        };
        Self::create(
            directory,
            env!("CARGO_PKG_VERSION"),
            platform,
            "preview",
            STANDARD
                .decode(include_str!("../../../release/release-signing.pub").trim())
                .map_err(|_| invalid())?,
            RELEASES.into(),
            PREFIX.into(),
            None,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn create(
        directory: PathBuf,
        version: &str,
        platform: &str,
        channel: &str,
        key: Vec<u8>,
        feed: String,
        asset_prefix: String,
        test_origin: Option<String>,
    ) -> Result<Arc<Self>, ClientError> {
        let version = Version::parse(version).map_err(|_| invalid())?;
        if key.len() != 32 || !directory.is_absolute() {
            return Err(invalid());
        }
        Ok(Arc::new(Self {
            directory,
            platform: platform.into(),
            version: version.clone(),
            channel: channel.into(),
            key,
            feed,
            asset_prefix,
            test_origin,
            operation: Arc::new(tokio::sync::Mutex::new(())),
            state: Mutex::new(State {
                public: UpdateStatus {
                    supported: platform != "unsupported",
                    phase: "idle".into(),
                    current_version: version.to_string(),
                    channel: channel.into(),
                    checked_at: None,
                    update: None,
                    downloaded_bytes: DecimalU64(0),
                    error: None,
                },
                candidate: None,
                staged: None,
                cancel: None,
            }),
        }))
    }
    /// Debug-only isolated acceptance: loopback server + a synthetic test key.
    #[cfg(debug_assertions)]
    pub fn synthetic(
        directory: PathBuf,
        origin: &str,
        public_key: &str,
        current: &str,
        platform: &str,
    ) -> Result<Arc<Self>, ClientError> {
        let url = url::Url::parse(origin).map_err(|_| invalid())?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        let origin = origin.trim_end_matches('/');
        Self::create(
            directory,
            current,
            platform,
            "preview",
            STANDARD.decode(public_key).map_err(|_| invalid())?,
            format!("{origin}/releases"),
            format!("{origin}/download/"),
            Some(origin.into()),
        )
    }
    pub fn status(&self) -> Result<UpdateStatus, ClientError> {
        Ok(self.state.lock().map_err(|_| unavailable())?.public.clone())
    }
    fn client(&self, settings: &Value) -> Result<Client, ClientError> {
        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(600))
            .user_agent(concat!("NekoPak/", env!("CARGO_PKG_VERSION")))
            .min_tls_version(reqwest::tls::Version::TLS_1_2);
        if self.test_origin.is_some() {
            builder = builder.no_proxy();
        } else {
            let network = &settings["experimental_network"];
            if network["enabled"] == true {
                match network["proxy_mode"].as_str() {
                    Some("none") => builder = builder.no_proxy(),
                    Some("user_provided") => {
                        let proxy = network["proxy_url"].as_str().ok_or_else(invalid)?;
                        builder = builder
                            .no_proxy()
                            .proxy(reqwest::Proxy::all(proxy).map_err(|_| network_error())?);
                    }
                    Some("system") => {}
                    _ => return Err(invalid()),
                }
            }
        }
        builder.build().map_err(|_| network())
    }
    fn allowed(&self, url: &url::Url) -> bool {
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return false;
        }
        if let Some(origin) = &self.test_origin {
            return url.origin().ascii_serialization() == *origin;
        }
        url.scheme() == "https"
            && url.port_or_known_default() == Some(443)
            && matches!(
                url.host_str(),
                Some(
                    "api.github.com"
                        | "github.com"
                        | "release-assets.githubusercontent.com"
                        | "objects.githubusercontent.com"
                )
            )
    }
    async fn response(&self, client: &Client, url: &str) -> Result<reqwest::Response, ClientError> {
        let mut url = url::Url::parse(url).map_err(|_| invalid())?;
        for _ in 0..5 {
            if !self.allowed(&url) {
                return Err(invalid());
            }
            let accept = if url.host_str() == Some("api.github.com") {
                "application/vnd.github+json"
            } else {
                "application/octet-stream"
            };
            let response = client
                .get(url.clone())
                .header("Accept", accept)
                .send()
                .await
                .map_err(|_| network())?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(invalid)?;
                url = url.join(location).map_err(|_| invalid())?;
                continue;
            }
            if !response.status().is_success() {
                return Err(network());
            }
            return Ok(response);
        }
        Err(network())
    }
    async fn read_bounded(
        &self,
        client: &Client,
        url: &str,
        max: usize,
    ) -> Result<Vec<u8>, ClientError> {
        let mut response = self.response(client, url).await?;
        if response
            .content_length()
            .is_some_and(|size| size > max as u64)
        {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| network())? {
            if bytes.len() + chunk.len() > max {
                return Err(invalid());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    fn verify(
        &self,
        bytes: &[u8],
        tag: &str,
        prerelease: bool,
    ) -> Result<Option<Candidate>, ClientError> {
        let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        let payload = STANDARD
            .decode(envelope.payload_base64)
            .map_err(|_| invalid())?;
        if payload.len() > 64 * 1024 {
            return Err(invalid());
        }
        let signature = STANDARD.decode(envelope.signature).map_err(|_| invalid())?;
        UnparsedPublicKey::new(&ED25519, &self.key)
            .verify(&payload, &signature)
            .map_err(|_| invalid())?;
        let manifest: Manifest = serde_json::from_slice(&payload).map_err(|_| invalid())?;
        let version = Version::parse(&manifest.version).map_err(|_| invalid())?;
        if manifest.schema_version != 1
            || manifest.product != "NekoPak"
            || tag != format!("v{}", manifest.version)
            || !matches!(manifest.channel.as_str(), "preview" | "stable")
            || prerelease != (manifest.channel == "preview")
            || manifest.notes.len() > 16000
            || manifest
                .notes
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            || time::OffsetDateTime::parse(
                &manifest.published_at,
                &time::format_description::well_known::Rfc3339,
            )
            .is_err()
        {
            return Err(invalid());
        }
        if version <= self.version || (self.channel == "stable" && manifest.channel != "stable") {
            return Ok(None);
        }
        let Some(artifact) = manifest.platforms.get(&self.platform).cloned() else {
            return Ok(None);
        };
        if artifact.name != artifact_name(&manifest.version, &self.platform)?
            || !(1..=MAX_PACKAGE).contains(&artifact.bytes)
            || artifact.sha256.len() != 64
            || !artifact
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid());
        }
        Ok(Some(Candidate {
            url: format!("{}{tag}/{}", self.asset_prefix, artifact.name),
            public: UpdateInfo {
                update_id: uuid::Uuid::new_v4().to_string(),
                version: manifest.version,
                channel: manifest.channel,
                notes: manifest.notes,
                published_at: manifest.published_at,
                size_bytes: DecimalU64(artifact.bytes),
            },
            artifact,
        }))
    }
    async fn find(&self, settings: &Value) -> Result<Option<Candidate>, ClientError> {
        let client = self.client(settings)?;
        let releases: Vec<Release> = serde_json::from_slice(
            &self
                .read_bounded(&client, &self.feed, 2 * 1024 * 1024)
                .await?,
        )
        .map_err(|_| invalid())?;
        let mut candidates: Vec<_> = releases
            .into_iter()
            .filter(|release| !release.draft)
            .filter_map(|release| {
                Version::parse(release.tag_name.strip_prefix('v')?)
                    .ok()
                    .filter(|v| v > &self.version)
                    .map(|v| (v, release))
            })
            .collect();
        candidates.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, release) in candidates.into_iter().take(10) {
            if self.channel == "stable" && release.prerelease {
                continue;
            }
            let manifests: Vec<_> = release
                .assets
                .iter()
                .filter(|a| a.name == "update-manifest.json")
                .collect();
            if manifests.is_empty() {
                continue;
            }
            if manifests.len() != 1 {
                return Err(invalid());
            }
            let manifest_url = format!(
                "{}{}/update-manifest.json",
                self.asset_prefix, release.tag_name
            );
            if manifests[0].browser_download_url != manifest_url {
                return Err(invalid());
            }
            let bytes = self
                .read_bounded(&client, &manifest_url, MAX_MANIFEST)
                .await?;
            if let Some(candidate) = self.verify(&bytes, &release.tag_name, release.prerelease)? {
                if release
                    .assets
                    .iter()
                    .filter(|asset| {
                        asset.name == candidate.artifact.name
                            && asset.browser_download_url == candidate.url
                    })
                    .count()
                    != 1
                {
                    return Err(invalid());
                }
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }
    pub async fn check(&self, settings: &Value) -> Result<UpdateStatus, ClientError> {
        let _gate = self
            .operation
            .try_lock()
            .map_err(|_| failure(ErrorCode::Conflict, "更新操作正在进行，请稍后再试。"))?;
        {
            let mut state = self.state.lock().map_err(|_| unavailable())?;
            if !state.public.supported {
                return Err(failure(
                    ErrorCode::Unsupported,
                    "当前平台尚未提供应用内更新。",
                ));
            }
            if state.cancel.is_some() {
                return Err(failure(ErrorCode::Conflict, "更新包正在下载。"));
            }
            state.public.phase = "checking".into();
            state.public.error = None;
            state.public.update = None;
            state.public.downloaded_bytes = DecimalU64(0);
            state.candidate = None;
            state.staged = None;
        }
        let result = RequestContext::new(Duration::from_secs(45))
            .run(async {
                self.cleanup_staging()?;
                self.find(settings).await
            })
            .await;
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        state.public.checked_at = Some(now());
        match result {
            Ok(candidate) => {
                state.public.update = candidate.as_ref().map(|c| c.public.clone());
                state.public.phase = if candidate.is_some() {
                    "available"
                } else {
                    "up_to_date"
                }
                .into();
                state.candidate = candidate;
                state.staged = None;
                state.public.downloaded_bytes = DecimalU64(0);
            }
            Err(error) => {
                state.public.phase = "error".into();
                state.public.error = Some(error);
            }
        }
        Ok(state.public.clone())
    }
    fn cleanup_staging(&self) -> Result<(), ClientError> {
        if !self.directory.try_exists().map_err(|_| unavailable())? {
            return Ok(());
        }
        let _parents = crate::downloads::lock_parents(&self.directory)?;
        for entry in std::fs::read_dir(&self.directory).map_err(|_| unavailable())? {
            let entry = entry.map_err(|_| unavailable())?;
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("apk" | "exe")
            ) || !uuid::Uuid::parse_str(stem).is_ok_and(|id| id.to_string() == stem)
            {
                continue;
            }
            let metadata = std::fs::symlink_metadata(&path).map_err(|_| unavailable())?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    continue;
                }
            }
            // Only updater-owned UUID files directly inside the verified staging
            // directory. Never recurse, follow links or remove arbitrary names.
            std::fs::remove_file(path).map_err(|_| {
                failure(
                    ErrorCode::Conflict,
                    "无法清理旧更新包，请关闭仍在运行的安装器后重试。",
                )
            })?;
        }
        Ok(())
    }
    fn stage_path(&self, id: &str) -> Result<PathBuf, ClientError> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(invalid());
        }
        Ok(self.directory.join(format!(
            "{id}.{}",
            if self.platform.starts_with("android") {
                "apk"
            } else {
                "exe"
            }
        )))
    }
    pub fn start_download(self: &Arc<Self>, id: &str, settings: Value) -> Result<(), ClientError> {
        let _gate = self
            .operation
            .try_lock()
            .map_err(|_| failure(ErrorCode::Conflict, "正在检查更新，请稍后重试。"))?;
        let context = RequestContext::new(Duration::from_secs(900));
        let candidate = {
            let mut state = self.state.lock().map_err(|_| unavailable())?;
            if state.cancel.is_some() {
                return Err(failure(ErrorCode::Conflict, "更新包正在下载。"));
            }
            let candidate = state
                .candidate
                .clone()
                .filter(|c| c.public.update_id == id)
                .ok_or_else(|| failure(ErrorCode::Conflict, "更新信息已失效，请重新检查。"))?;
            state.public.phase = "downloading".into();
            state.public.downloaded_bytes = DecimalU64(0);
            state.public.error = None;
            state.staged = None;
            state.cancel = Some(context.clone());
            candidate
        };
        let service = self.clone();
        tokio::spawn(async move {
            let _gate = service.operation.lock().await;
            let path = service.stage_path(&candidate.public.update_id);
            let result = match path.as_ref() {
                Ok(path) => {
                    context
                        .run(service.download_to(&candidate, &settings, path))
                        .await
                }
                Err(error) => Err(error.clone()),
            };
            if result.is_err() {
                if let Ok(path) = &path {
                    let _ = std::fs::remove_file(path);
                }
            }
            if let Ok(mut state) = service.state.lock() {
                state.cancel = None;
                match result {
                    Ok(()) => {
                        state.public.phase = "ready".into();
                        state.staged = path.ok();
                    }
                    Err(error) => {
                        state.public.phase = if error.code == ErrorCode::Cancelled {
                            "available"
                        } else {
                            "error"
                        }
                        .into();
                        state.public.downloaded_bytes = DecimalU64(0);
                        state.public.error = if error.code == ErrorCode::Cancelled {
                            None
                        } else {
                            Some(error)
                        };
                    }
                }
            }
        });
        Ok(())
    }
    async fn download_to(
        &self,
        candidate: &Candidate,
        settings: &Value,
        path: &Path,
    ) -> Result<(), ClientError> {
        std::fs::create_dir_all(&self.directory).map_err(|_| unavailable())?;
        let _parents = crate::downloads::lock_parents(&self.directory)?;
        if path.exists() {
            std::fs::remove_file(path).map_err(|_| unavailable())?;
        }
        let mut output = crate::downloads::open_part(path, true)?;
        let client = self.client(settings)?;
        let mut response = self.response(&client, &candidate.url).await?;
        if response
            .content_length()
            .is_some_and(|size| size != candidate.artifact.bytes)
        {
            return Err(invalid());
        }
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(|_| network())? {
            received = received
                .checked_add(chunk.len() as u64)
                .filter(|n| *n <= candidate.artifact.bytes)
                .ok_or_else(invalid)?;
            hasher.update(&chunk);
            output
                .write_all(&chunk)
                .map_err(|_| failure(ErrorCode::DiskFull, "无法保存更新包，请检查可用空间。"))?;
            self.state
                .lock()
                .map_err(|_| unavailable())?
                .public
                .downloaded_bytes = DecimalU64(received);
        }
        if received != candidate.artifact.bytes
            || format!("{:x}", hasher.finalize()) != candidate.artifact.sha256
        {
            return Err(invalid());
        }
        output.sync_all().map_err(|_| unavailable())?;
        Ok(())
    }
    pub fn cancel(&self) -> Result<(), ClientError> {
        if let Some(context) = &self.state.lock().map_err(|_| unavailable())?.cancel {
            context.cancel();
        }
        Ok(())
    }
    pub fn verified_file(&self, id: &str) -> Result<VerifiedUpdate, ClientError> {
        let operation = self
            .operation
            .clone()
            .try_lock_owned()
            .map_err(|_| failure(ErrorCode::Conflict, "更新操作正在进行，请稍后再试。"))?;
        let state = self.state.lock().map_err(|_| unavailable())?;
        if state.public.phase != "ready" {
            return Err(failure(ErrorCode::Conflict, "请先下载并验证更新包。"));
        }
        let candidate = state
            .candidate
            .as_ref()
            .filter(|c| c.public.update_id == id)
            .ok_or_else(invalid)?
            .clone();
        let path = state
            .staged
            .as_ref()
            .filter(|path| Some(*path) == self.stage_path(id).ok().as_ref())
            .ok_or_else(invalid)?
            .clone();
        drop(state);
        let parents = crate::downloads::lock_parents(&self.directory)?;
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| invalid())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(invalid());
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(invalid());
            }
            options.share_mode(1).custom_flags(0x00200000);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(&path).map_err(|_| invalid())?;
        if file.metadata().map_err(|_| invalid())?.len() != candidate.artifact.bytes {
            return Err(invalid());
        }
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let n = file.read(&mut buffer).map_err(|_| invalid())?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        if format!("{:x}", hash.finalize()) != candidate.artifact.sha256 {
            return Err(invalid());
        }
        Ok(VerifiedUpdate {
            path,
            sha256: candidate.artifact.sha256.clone(),
            bytes: candidate.artifact.bytes,
            version: candidate.public.version.clone(),
            _file: file,
            _parents: parents,
            _operation: operation,
        })
    }
}
fn network_error() -> ClientError {
    failure(ErrorCode::Conflict, "更新代理配置无效。")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    fn signing() -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&[42; 32]).unwrap()
    }
    fn payload(bytes: &[u8]) -> Value {
        json!({"schema_version":1,"product":"NekoPak","version":"0.1.3","channel":"preview","published_at":"2026-11-10T00:00:00Z","notes":"Synthetic update","platforms":{"windows-x86_64":{"name":"NekoPak-0.1.3-windows-x64-setup.exe","bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes))}}})
    }
    fn signed(value: &Value) -> Vec<u8> {
        let raw = serde_json::to_vec(value).unwrap();
        serde_json::to_vec(&json!({"payload_base64":STANDARD.encode(&raw),"signature":STANDARD.encode(signing().sign(&raw).as_ref())})).unwrap()
    }
    fn service(path: PathBuf, origin: &str) -> Arc<UpdateService> {
        UpdateService::synthetic(
            path,
            origin,
            &STANDARD.encode(signing().public_key().as_ref()),
            "0.1.2",
            "windows-x86_64",
        )
        .unwrap()
    }
    #[test]
    fn signed_manifest_rejects_tampering_wrong_platform_product_names_and_downgrades() {
        let directory = tempfile::tempdir().unwrap();
        let app = service(directory.path().join("updates"), "http://127.0.0.1:12345");
        let value = payload(b"package");
        assert!(app
            .verify(&signed(&value), "v0.1.3", true)
            .unwrap()
            .is_some());
        let mut wrong = value.clone();
        wrong["notes"] = "tampered".into();
        let mut envelope: Value = serde_json::from_slice(&signed(&value)).unwrap();
        envelope["payload_base64"] = STANDARD.encode(serde_json::to_vec(&wrong).unwrap()).into();
        assert!(app
            .verify(&serde_json::to_vec(&envelope).unwrap(), "v0.1.3", true)
            .is_err());
        for (field, new_value) in [
            ("product", json!("other")),
            ("schema_version", json!(2)),
            ("published_at", json!("bad")),
        ] {
            let mut wrong = value.clone();
            wrong[field] = new_value;
            assert!(app.verify(&signed(&wrong), "v0.1.3", true).is_err());
        }
        let mut wrong = value.clone();
        wrong["platforms"]["windows-x86_64"]["name"] = "../payload.exe".into();
        assert!(app.verify(&signed(&wrong), "v0.1.3", true).is_err());
        let mut wrong = value.clone();
        wrong["version"] = "0.1.1".into();
        assert!(app
            .verify(&signed(&wrong), "v0.1.1", true)
            .unwrap()
            .is_none());
        let mut other = value.clone();
        other["platforms"] = json!({});
        assert!(app
            .verify(&signed(&other), "v0.1.3", true)
            .unwrap()
            .is_none());
        assert!(app.verify(&signed(&value), "v0.1.4", true).is_err());
        assert!(app.verify(&signed(&value), "v0.1.3", false).is_err());
        assert!(!app.allowed(&url::Url::parse("https://attacker.example/file.exe").unwrap()));
    }
    async fn fixture(corrupt: bool, slow: bool) -> (String, tokio::task::JoinHandle<()>, Vec<u8>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let package = vec![13; 128 * 1024];
        let bytes = package.clone();
        let base = origin.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let bytes = bytes.clone();
                let base = base.clone();
                tokio::spawn(async move {
                    let mut input = Vec::new();
                    let mut buffer = [0; 4096];
                    while !input.windows(4).any(|window| window == b"\r\n\r\n") {
                        let n = stream.read(&mut buffer).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        input.extend_from_slice(&buffer[..n]);
                        if input.len() > 16000 {
                            return;
                        }
                    }
                    let text = String::from_utf8_lossy(&input);
                    assert!(!text.to_lowercase().contains("authorization:"));
                    let path = text.split_whitespace().nth(1).unwrap();
                    let (response, download) = if path == "/releases" {
                        (serde_json::to_vec(&json!([{"tag_name":"v0.1.3","draft":false,"prerelease":true,"assets":[{"name":"update-manifest.json","browser_download_url":format!("{base}/download/v0.1.3/update-manifest.json")},{"name":"NekoPak-0.1.3-windows-x64-setup.exe","browser_download_url":format!("{base}/download/v0.1.3/NekoPak-0.1.3-windows-x64-setup.exe")}]}])).unwrap(),false)
                    } else if path.ends_with("update-manifest.json") {
                        (signed(&payload(&bytes)), false)
                    } else {
                        let mut content = bytes.clone();
                        if corrupt {
                            content[0] ^= 1;
                        }
                        (content, true)
                    };
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        response.len()
                    );
                    if stream.write_all(header.as_bytes()).await.is_err() {
                        return;
                    }
                    for chunk in response.chunks(4096) {
                        if stream.write_all(chunk).await.is_err() {
                            break;
                        }
                        if slow && download {
                            tokio::time::sleep(Duration::from_millis(30)).await;
                        }
                    }
                });
            }
        });
        (origin, task, package)
    }
    async fn finished(app: &UpdateService) -> UpdateStatus {
        for _ in 0..200 {
            let s = app.status().unwrap();
            if s.phase != "downloading" {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("download did not finish")
    }
    #[tokio::test]
    async fn actual_http_update_download_verifies_bytes_and_rechecks_before_install() {
        let directory = tempfile::tempdir().unwrap();
        let (origin, server, bytes) = fixture(false, false).await;
        let app = service(directory.path().join("updates"), &origin);
        let status = app.check(&json!({})).await.unwrap();
        assert_eq!(status.phase, "available");
        let id = status.update.unwrap().update_id;
        app.start_download(&id, json!({})).unwrap();
        let status = finished(&app).await;
        assert_eq!(status.phase, "ready");
        let verified = app.verified_file(&id).unwrap();
        assert_eq!(std::fs::read(&verified.path).unwrap(), bytes);
        assert!(app.verified_file(&id).is_err());
        assert!(app.start_download(&id, json!({})).is_err());
        assert!(app.check(&json!({})).await.is_err());
        let path = verified.path.clone();
        drop(verified);
        std::fs::write(&path, b"tampered").unwrap();
        assert!(app.verified_file(&id).is_err());
        server.abort();
    }
    #[tokio::test]
    async fn recheck_clears_stale_candidates_and_only_removes_owned_packages() {
        let directory = tempfile::tempdir().unwrap();
        let updates = directory.path().join("updates");
        std::fs::create_dir(&updates).unwrap();
        let orphan = updates.join(format!("{}.exe", uuid::Uuid::new_v4()));
        let unrelated = updates.join("keep.txt");
        let subdirectory = updates.join(format!("{}.exe", uuid::Uuid::new_v4()));
        std::fs::write(&orphan, b"abandoned download").unwrap();
        std::fs::write(&unrelated, b"not an update").unwrap();
        std::fs::create_dir(&subdirectory).unwrap();
        let (origin, server, _) = fixture(false, false).await;
        let app = service(updates, &origin);
        let id = app
            .check(&json!({}))
            .await
            .unwrap()
            .update
            .unwrap()
            .update_id;
        assert!(!orphan.exists());
        assert!(unrelated.exists() && subdirectory.is_dir());
        app.start_download(&id, json!({})).unwrap();
        assert_eq!(finished(&app).await.phase, "ready");
        let path = app.stage_path(&id).unwrap();
        assert!(path.exists());
        server.abort();
        let _ = server.await;
        let status = app.check(&json!({})).await.unwrap();
        assert_eq!(status.phase, "error");
        assert!(status.update.is_none());
        assert!(app.start_download(&id, json!({})).is_err());
        assert!(!path.exists());
        assert!(unrelated.exists() && subdirectory.is_dir());
    }
    #[tokio::test]
    async fn corrupt_and_cancelled_downloads_never_become_installable() {
        for cancel in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let (origin, server, _) = fixture(!cancel, cancel).await;
            let app = service(directory.path().join("updates"), &origin);
            let id = app
                .check(&json!({}))
                .await
                .unwrap()
                .update
                .unwrap()
                .update_id;
            app.start_download(&id, json!({})).unwrap();
            if cancel {
                tokio::time::sleep(Duration::from_millis(60)).await;
                app.cancel().unwrap();
            }
            let status = finished(&app).await;
            assert_eq!(status.phase, if cancel { "available" } else { "error" });
            assert!(app.verified_file(&id).is_err());
            assert!(!app.stage_path(&id).unwrap().exists());
            server.abort();
        }
    }
    #[test]
    fn production_roots_reject_untrusted_hosts_and_plain_http() {
        let dir = tempfile::tempdir().unwrap();
        let app = UpdateService::production(dir.path().join("updates")).unwrap();
        for url in [
            "http://github.com/NekoPak/NikoPak/file",
            "https://github.com.evil.test/file",
            "https://user:pass@github.com/file",
            "https://github.com:444/file",
        ] {
            assert!(!app.allowed(&url::Url::parse(url).unwrap()));
        }
        assert!(app.allowed(
            &url::Url::parse("https://release-assets.githubusercontent.com/asset").unwrap()
        ));
    }
    #[tokio::test]
    #[ignore = "Reads the public release feed and downloads a signed package; run after publication"]
    async fn public_release_feed_and_package_verify_with_production_trust() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = UpdateService::production(directory.path().join("updates")).unwrap();
        // A test-only older client exercises discovery of the current published
        // release without a runtime override of the production source or key.
        let inner = Arc::get_mut(&mut app).unwrap();
        inner.version = Version::parse("0.1.2").unwrap();
        inner.platform = "windows-x86_64".into();
        inner.state.get_mut().unwrap().public.supported = true;
        let status = app.check(&json!({})).await.unwrap();
        assert_eq!(status.phase, "available");
        let update = status.update.unwrap();
        assert!(
            Version::parse(&update.version).unwrap()
                >= Version::parse(env!("CARGO_PKG_VERSION")).unwrap()
        );
        app.start_download(&update.update_id, json!({})).unwrap();
        let status = tokio::time::timeout(Duration::from_secs(600), async {
            loop {
                let status = app.status().unwrap();
                if status.phase != "downloading" {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(status.phase, "ready");
        let verified = app.verified_file(&update.update_id).unwrap();
        println!(
            "Verified public NekoPak {} ({} bytes; SHA256 {}). Installer was not launched.",
            verified.version, verified.bytes, verified.sha256
        );
    }
}
