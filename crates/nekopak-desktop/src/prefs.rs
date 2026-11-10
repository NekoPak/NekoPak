//! UI preferences. Device-level choices live in a small JSON file next to the
//! task database; account-scoped ones (pins, view, sort) are stored by the
//! core per account using the same schema as the web UI.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct DevicePrefs {
    pub accent: String,
    pub sidebar_collapsed: bool,
    pub reduce_motion: bool,
    pub clipboard_watch: bool,
    pub disclaimer_accepted: bool,
    pub window_width: f32,
    pub window_height: f32,
}

impl Default for DevicePrefs {
    fn default() -> Self {
        Self {
            accent: "mist".into(),
            sidebar_collapsed: false,
            reduce_motion: false,
            clipboard_watch: false,
            disclaimer_accepted: false,
            window_width: 0.0,
            window_height: 0.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Pin {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SortSpec {
    pub key: String,
    pub dir: String,
}

/// Mirrors the web `UiPreferences` wire shape (camelCase keys).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AccountPrefs {
    pub pins: Vec<Pin>,
    pub view: String,
    pub sort: SortSpec,
    pub density: String,
    pub grid_size: String,
    pub folders_first: bool,
}

impl Default for AccountPrefs {
    fn default() -> Self {
        Self {
            pins: Vec::new(),
            view: "list".into(),
            sort: SortSpec { key: "name".into(), dir: "asc".into() },
            density: "comfortable".into(),
            grid_size: "m".into(),
            folders_first: true,
        }
    }
}

impl AccountPrefs {
    /// Accepts whatever a previous version stored and falls back per field.
    pub fn normalized(value: &serde_json::Value) -> Self {
        let mut prefs: Self = serde_json::from_value(value.clone()).unwrap_or_default();
        let defaults = Self::default();
        if !matches!(prefs.view.as_str(), "list" | "grid") {
            prefs.view = defaults.view;
        }
        if !matches!(prefs.density.as_str(), "comfortable" | "compact") {
            prefs.density = defaults.density;
        }
        if !matches!(prefs.grid_size.as_str(), "s" | "m" | "l") {
            prefs.grid_size = defaults.grid_size;
        }
        if !matches!(prefs.sort.key.as_str(), "name" | "size" | "modified" | "kind")
            || !matches!(prefs.sort.dir.as_str(), "asc" | "desc")
        {
            prefs.sort = defaults.sort;
        }
        prefs.pins.retain(|pin| !pin.id.is_empty() && !pin.name.is_empty());
        prefs
    }
}

#[derive(Default)]
pub struct Prefs {
    pub device: DevicePrefs,
    pub account: AccountPrefs,
    /// Account the scoped preferences were read for; `None` until loaded.
    pub account_id: Option<String>,
    path: PathBuf,
}

impl Prefs {
    pub fn load(directory: &Path) -> Self {
        let path = directory.join("desktop-ui.json");
        let device = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { device, account: AccountPrefs::default(), account_id: None, path }
    }

    /// Best effort: these are conveniences, never authoritative state.
    pub fn save_device(&self) {
        let Ok(bytes) = serde_json::to_vec_pretty(&self.device) else {
            return;
        };
        let temporary = self.path.with_extension("json.tmp");
        if std::fs::write(&temporary, bytes).is_ok() {
            let _ = std::fs::rename(&temporary, &self.path);
        }
    }

    pub fn is_pinned(&self, id: &str) -> bool {
        self.account.pins.iter().any(|pin| pin.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_preferences_survive_unknown_or_bad_values() {
        let prefs = AccountPrefs::normalized(&serde_json::json!({
            "pins": [{"id": "a", "name": "A"}, {"id": "", "name": "x"}],
            "view": "tiles", "gridSize": "l", "foldersFirst": false,
            "sort": {"key": "size", "dir": "sideways"}
        }));
        assert_eq!(prefs.pins.len(), 1);
        assert_eq!(prefs.view, "list");
        assert_eq!(prefs.grid_size, "l");
        assert!(!prefs.folders_first);
        assert_eq!(prefs.sort.key, "name");
        assert_eq!(AccountPrefs::normalized(&serde_json::json!({"pins": []})), AccountPrefs::default());
        let wire = serde_json::to_value(AccountPrefs::default()).unwrap();
        assert!(wire.get("gridSize").is_some() && wire.get("foldersFirst").is_some());
    }

    #[test]
    fn device_preferences_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let mut prefs = Prefs::load(directory.path());
        assert_eq!(prefs.device, DevicePrefs::default());
        prefs.device.accent = "rose".into();
        prefs.device.sidebar_collapsed = true;
        prefs.save_device();
        assert_eq!(Prefs::load(directory.path()).device, prefs.device);
        std::fs::write(directory.path().join("desktop-ui.json"), b"{broken").unwrap();
        assert_eq!(Prefs::load(directory.path()).device, DevicePrefs::default());
    }
}
