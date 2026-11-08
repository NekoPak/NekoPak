//! Optional Windows filesystem adapters. No driver, installer or external CLI is bundled.
//!
//! Callbacks read an immutable snapshot and a caller-owned, version-checked range
//! source. Native mount success is reported only after the platform accepts it.
mod model;
pub use model::*;

#[cfg(windows)]
mod cloud_files;
#[cfg(windows)]
mod windows_util;
#[cfg(windows)]
mod winfsp;

#[cfg(windows)]
pub use cloud_files::CloudFilesMount;
#[cfg(windows)]
pub use winfsp::WinFspMount;

use serde::Serialize;

/// Required WinFsp FLOSS-exception attribution; expose with its link in the UI.
pub const WINFSP_NOTICE: &str =
    "WinFsp - Windows File System Proxy, Copyright (C) Bill Zissimopoulos";
pub const WINFSP_URL: &str = "https://github.com/winfsp/winfsp";

#[derive(Debug, Clone, Serialize)]
pub struct NativeCapabilities {
    pub cloud_files: PlatformCapability,
    pub winfsp: PlatformCapability,
}
#[derive(Debug, Clone, Serialize)]
pub struct PlatformCapability {
    pub available: bool,
    pub version: Option<String>,
    pub reason: Option<String>,
}
impl PlatformCapability {
    fn unavailable(reason: &str) -> Self {
        Self {
            available: false,
            version: None,
            reason: Some(reason.into()),
        }
    }
}

/// Read-only platform checks, without mounting, registration, installation or elevation.
/// Availability is prerequisite discovery, not proof of any remote account's range support.
pub fn native_capabilities() -> NativeCapabilities {
    #[cfg(windows)]
    {
        NativeCapabilities {
            cloud_files: cloud_files::detect(),
            winfsp: winfsp::detect(),
        }
    }
    #[cfg(not(windows))]
    {
        NativeCapabilities {
            cloud_files: PlatformCapability::unavailable("Windows Cloud Files API is required."),
            winfsp: PlatformCapability::unavailable(
                "Windows and user-installed WinFsp are required.",
            ),
        }
    }
}
