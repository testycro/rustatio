use crate::data_dir;
use rustatio_core::{
    AppConfig, ClientDefinition, FakerConfig, FakerState, ProxySettings, TorrentSummary,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedInstance {
    pub id: u32,
    pub torrent: TorrentSummary,
    pub config: FakerConfig,
    pub cumulative_uploaded: u64,
    pub cumulative_downloaded: u64,
    pub state: FakerState,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub from_watch_folder: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistedState {
    pub instances: HashMap<u32, PersistedInstance>,
    pub next_instance_id: u32,
    #[serde(default)]
    pub default_config: Option<FakerConfig>,
    /// Emulated `BitTorrent` client definitions, stored next to `default_config`.
    /// Add an entry to add a client, remove one to delete it; the built-in list is only
    /// used when this is absent or has no valid entry.
    #[serde(default)]
    pub clients: Option<Vec<ClientDefinition>>,
    #[serde(default)]
    pub watch_settings: Option<WatchSettings>,
    #[serde(default)]
    pub proxy_settings: Option<ProxySettings>,
    /// Options of the embedded admin daemon (see [`DaemonSettings`]).
    #[serde(default)]
    pub daemon_settings: Option<DaemonSettings>,
    pub version: u32,
}

/// Options of the admin daemon embedded in the desktop app, kept with the other desktop options
/// (like `watch_settings`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonSettings {
    /// Start the daemon together with the app (off by default).
    #[serde(default)]
    pub autostart: bool,
    /// Daemon variables saved from the Daemon view ("environment configuration"), applied at
    /// every start over the daemon defaults. Empty: the defaults.
    #[serde(default)]
    pub env: BTreeMap<String, serde_json::Value>,
}

impl PersistedState {
    pub fn new() -> Self {
        Self {
            instances: HashMap::new(),
            next_instance_id: 1,
            default_config: None,
            clients: None,
            watch_settings: None,
            proxy_settings: None,
            daemon_settings: None,
            version: 1,
        }
    }
}

const fn default_watch_max_depth() -> u32 {
    1
}

fn default_watch_auto_start() -> bool {
    std::env::var("WATCH_AUTO_START").is_ok_and(|v| v.eq_ignore_ascii_case("true") || v == "1")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WatchSettings {
    #[serde(default = "default_watch_max_depth")]
    pub max_depth: u32,
    #[serde(default = "default_watch_auto_start")]
    pub auto_start: bool,
    #[serde(default)]
    pub watch_dir: Option<String>,
}

impl Default for WatchSettings {
    fn default() -> Self {
        Self {
            max_depth: default_watch_max_depth(),
            auto_start: default_watch_auto_start(),
            watch_dir: None,
        }
    }
}

pub fn state_file_path() -> PathBuf {
    if let Some(override_path) = test_state_file_path() {
        return override_path;
    }

    data_dir::data_dir().join(data_dir::STATE_FILE)
}

/// File of the app configuration. On Windows it lives with the other app files, so that the
/// uninstaller removes it too.
pub fn config_file_path() -> PathBuf {
    if cfg!(windows) {
        data_dir::data_dir().join(data_dir::CONFIG_FILE)
    } else {
        AppConfig::default_path()
    }
}

/// Moves the files written by earlier versions to the current data folder. Call once at startup,
/// before anything is loaded.
pub fn migrate_legacy_data() {
    let data = data_dir::data_dir();

    for legacy in data_dir::legacy_dirs() {
        for moved in data_dir::migrate_from(&legacy, &data) {
            log::info!("Moved {} from the previous location", moved.display());
        }
    }

    let config = config_file_path();
    if config != AppConfig::default_path() {
        match data_dir::move_entry(&AppConfig::default_path(), &config) {
            Ok(true) => log::info!("Moved the configuration to {}", config.display()),
            Ok(false) => {}
            Err(e) => log::warn!("Could not move the configuration to {}: {e}", config.display()),
        }
    }
}

/// The app configuration, created with the defaults when there is none yet.
pub fn load_config() -> AppConfig {
    let path = config_file_path();
    if path == AppConfig::default_path() {
        return AppConfig::load_or_default();
    }

    if path.exists() {
        return AppConfig::load(&path).unwrap_or_else(|e| {
            log::warn!("Failed to load config from {}: {e}. Using defaults.", path.display());
            AppConfig::default()
        });
    }

    let config = AppConfig::default();
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = config.save(&path) {
        log::warn!("Failed to save default config: {e}");
    }
    config
}

#[allow(clippy::missing_const_for_fn)]
fn test_state_file_path() -> Option<PathBuf> {
    #[cfg(test)]
    {
        let guard = test_state_path_store().lock().ok()?;
        guard.clone()
    }

    #[cfg(not(test))]
    {
        None
    }
}

#[cfg(test)]
fn test_state_path_store() -> &'static Mutex<Option<PathBuf>> {
    static TEST_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    TEST_PATH.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
pub fn set_test_state_file_path(path: Option<PathBuf>) {
    if let Ok(mut guard) = test_state_path_store().lock() {
        *guard = path;
    }
}

pub fn load_state() -> PersistedState {
    let path = state_file_path();

    if !path.exists() {
        log::info!("No saved state found at {}, starting fresh", path.display());
        return PersistedState::new();
    }

    match std::fs::read_to_string(&path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(state) => {
                let state: PersistedState = state;
                log::info!(
                    "Loaded saved state from {} ({} instances)",
                    path.display(),
                    state.instances.len()
                );
                state
            }
            Err(e) => {
                log::error!("Failed to parse state file: {e}");
                let backup = path.with_extension("json.corrupted");
                let _ = std::fs::rename(&path, &backup);
                log::warn!("Backed up corrupted state to {}", backup.display());
                PersistedState::new()
            }
        },
        Err(e) => {
            log::error!("Failed to read state file: {e}");
            PersistedState::new()
        }
    }
}

pub fn save_state(state: &PersistedState) -> Result<(), String> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let path = state_file_path();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create data directory: {e}"))?;
    }

    let unique_id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_name = format!("desktop-state.json.{}.{}.tmp", std::process::id(), unique_id);
    let temp_path = path.with_file_name(temp_name);

    let json = serde_json::to_vec(state).map_err(|e| format!("Failed to serialize state: {e}"))?;

    std::fs::write(&temp_path, &json).map_err(|e| format!("Failed to write temp file: {e}"))?;

    // sync via opening the file and syncing
    if let Ok(file) = std::fs::File::open(&temp_path) {
        let _ = file.sync_all();
    }

    std::fs::rename(&temp_path, &path).map_err(|e| {
        // Clean up temp file on rename failure
        let _ = std::fs::remove_file(&temp_path);
        format!("Failed to rename state file: {e}")
    })?;

    log::debug!("State saved to {}", path.display());
    Ok(())
}

pub fn now_timestamp() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
}
