//! Native host: data directory, provider selection and the background
//! workers that keep transfers, uploads, media jobs and sync running. This
//! mirrors the Android/Tauri host so both share one on-disk state layout.
use nekopak_core::{updates::UpdateService, AppService};
use nekopak_domain::{AuthChangedEvent, AuthStatus, BackendId, ClientError};
use nekopak_provider::{ProviderPort, RequestContext};
use nekopak_provider_mock::{MockProvider, Scenario};
use nekopak_provider_native::NativeProvider;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{broadcast::error::RecvError, mpsc::UnboundedSender};

/// Same identifier the previous desktop builds used, so sessions, tasks and
/// settings carry over when upgrading to the native client.
const APP_IDENTIFIER: &str = "io.nekopak.desktop";

#[derive(Debug)]
pub enum HostEvent {
    TransfersChanged,
    AuthChanged(AuthChangedEvent),
    ScanProgress(serde_json::Value),
    SyncChanged(serde_json::Value),
    SyncPlanReady(serde_json::Value),
    StorageError(ClientError),
    /// Tray menu or a second launch asked for the window (optionally a page).
    Activate(Option<&'static str>),
    Quit,
}

pub struct Host {
    pub service: Arc<AppService>,
    pub updater: Option<Arc<UpdateService>>,
    pub mock: bool,
    pub directory: PathBuf,
}

fn test_override(name: &str) -> Option<std::ffi::OsString> {
    if cfg!(any(debug_assertions, feature = "automation")) {
        std::env::var_os(name)
    } else {
        None
    }
}

pub fn data_directory() -> Result<PathBuf, String> {
    if let Some(directory) = test_override("NEKOPAK_TEST_DATA_DIR") {
        return Ok(PathBuf::from(directory));
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
    };
    base.map(|base| base.join(APP_IDENTIFIER))
        .ok_or_else(|| "The user data directory is unavailable.".to_owned())
}

pub fn start() -> Result<Host, String> {
    let directory = data_directory()?;
    std::fs::create_dir_all(&directory).map_err(|error| format!("Cannot create the data directory: {error}"))?;
    #[cfg(not(windows))]
    let directory = std::fs::canonicalize(directory).map_err(|error| error.to_string())?;
    let mock = match std::env::var("NEKOPAK_BACKEND").as_deref() {
        Ok("mock") => true,
        Ok("rust_compat") | Err(std::env::VarError::NotPresent) => false,
        _ => return Err("NEKOPAK_BACKEND must be rust_compat or mock.".into()),
    };
    let (provider, database): (Arc<dyn ProviderPort>, &str) = if mock {
        let scenario = match std::env::var("NEKOPAK_MOCK_SCENARIO") {
            Ok(value) => serde_json::from_value::<Scenario>(serde_json::Value::String(value))
                .map_err(|_| "Unknown mock scenario.".to_owned())?,
            Err(_) => Scenario::Free,
        };
        (Arc::new(MockProvider::new(scenario)), "tasks.sqlite3")
    } else {
        let synthetic = test_override("NEKOPAK_TEST_API_ORIGIN")
            .filter(|_| test_override("NEKOPAK_TEST_DATA_DIR").is_some())
            .and_then(|origin| origin.into_string().ok());
        let provider = match synthetic {
            // Loopback-only fixture origin used by the end-to-end harness.
            Some(origin) => NativeProvider::synthetic_loopback(&origin),
            None => NativeProvider::from_environment(&directory.to_string_lossy()),
        }
        .map_err(|error| error.message)?;
        (Arc::new(provider), "api-preview.sqlite3")
    };
    let service = Arc::new(AppService::with_provider(&directory.join(database), provider));
    // Apply persisted proxy/TLS settings before any request can be made. A
    // damaged journal keeps its recovery screen instead of failing start-up.
    if service.app_status().storage.status == "ready" {
        service.settings_get().map_err(|error| error.message)?;
    }
    let updater = UpdateService::production(directory.join("updates")).ok();
    Ok(Host { service, updater, mock, directory })
}

/// Starts the workers. Must be called from inside the Tokio runtime.
pub fn spawn_workers(service: &Arc<AppService>, events: UnboundedSender<HostEvent>) {
    if let Some(mut auth) = service.subscribe_auth() {
        let events = events.clone();
        tokio::spawn(async move {
            loop {
                let event = match auth.recv().await {
                    Ok(event) => event,
                    // The UI re-reads authoritative state for every auth event.
                    Err(RecvError::Lagged(_)) => AuthChangedEvent { state: AuthStatus::Pending, error: None },
                    Err(RecvError::Closed) => break,
                };
                if events.send(HostEvent::AuthChanged(event)).is_err() {
                    break;
                }
            }
        });
    }
    {
        let mut scans = service.subscribe_scans();
        let events = events.clone();
        tokio::spawn(async move {
            loop {
                match scans.recv().await {
                    Ok(event) => {
                        if events.send(HostEvent::ScanProgress(event)).is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }
    {
        // Snapshots are authoritative and filtered by the current account, so
        // the UI only needs to know that something changed.
        let mut transfers = service.subscribe();
        let events = events.clone();
        tokio::spawn(async move {
            loop {
                match transfers.recv().await {
                    Ok(_) | Err(RecvError::Lagged(_)) => {
                        if events.send(HostEvent::TransfersChanged).is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }
    {
        let service = service.clone();
        let events = events.clone();
        tokio::spawn(async move {
            if let Err(error) = service.run_download_queue().await {
                let _ = events.send(HostEvent::StorageError(error));
            }
        });
    }
    for media in [true, false] {
        let service = service.clone();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_millis(500));
            loop {
                timer.tick().await;
                let result = if media { service.run_media().await } else { service.run_upload().await };
                if result.is_err() {
                    break;
                }
            }
        });
    }
    {
        let service = service.clone();
        let events = events.clone();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_secs(2));
            loop {
                timer.tick().await;
                if !service.is_foreground() {
                    continue;
                }
                let context = || RequestContext::new(Duration::from_secs(45));
                if let Ok(changes) = service.sync_reconcile(&context()).await {
                    for change in changes {
                        let _ = events.send(HostEvent::SyncChanged(change));
                    }
                }
                if let Ok(changes) = service.sync_automatic(&context()).await {
                    for change in changes {
                        let _ = events.send(if change["state"] == "needs_review" {
                            HostEvent::SyncPlanReady(change)
                        } else {
                            HostEvent::SyncChanged(change)
                        });
                    }
                }
                let _ = service
                    .cleanup_shared_sources(&RequestContext::new(Duration::from_secs(15)))
                    .await;
            }
        });
    }
    {
        let service = service.clone();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_millis(500));
            loop {
                timer.tick().await;
                if let Err(error) = service.tick_periodic().await {
                    // Fail closed: stop writing to an inaccessible or damaged store.
                    let _ = events.send(HostEvent::StorageError(error));
                    break;
                }
            }
        });
    }
}

pub fn backend_id(mock: bool) -> BackendId {
    if mock {
        BackendId::Mock
    } else {
        BackendId::RustCompat
    }
}
