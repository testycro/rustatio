use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use std::sync::{OnceLock, RwLock};

/// Built-in client definitions, used as the seed when nothing has been saved
/// yet and as a fallback for any client missing from the saved definitions.
const BUILTIN_CLIENTS_JSON: &str = include_str!("clients.default.json");

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClientType {
    #[serde(rename = "utorrent")]
    UTorrent,
    #[serde(rename = "qbittorrent")]
    QBittorrent,
    #[serde(rename = "transmission")]
    Transmission,
    #[serde(rename = "deluge")]
    Deluge,
    #[serde(rename = "bittorrent")]
    BitTorrent,
    #[serde(rename = "rtorrent")]
    RTorrent,
}

/// Metadata about a torrent client for UI/API display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    /// Machine-readable ID (e.g., "qbittorrent")
    pub id: String,
    /// Human-readable display name (e.g., "qBittorrent")
    pub name: String,
    /// Default version to use
    pub default_version: String,
    /// Available versions (newest first)
    pub versions: Vec<String>,
    /// Default port
    pub default_port: u16,
}

/// How the 4-character version code of the peer ID is derived from the version string.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionScheme {
    /// Remove the dots ("3.5.5" -> "355")
    Digits,
    /// Concatenate major, minor and patch ("5.2.0" -> "520")
    #[default]
    MajorMinorPatch,
    /// Major followed by the minor zero-padded to 2 digits ("4.1.3" -> "410")
    MajorMinorPad2,
}

/// Full, data-driven description of an emulated client.
///
/// These definitions are persisted next to `default_config` in the state file,
/// so versions, ports, peer ID prefix, user agent, etc. can be changed without
/// recompiling. Any client missing from the saved list falls back to
/// `clients.default.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientDefinition {
    /// Machine-readable ID, must match a known `ClientType` (e.g. "qbittorrent")
    pub id: String,
    /// Human-readable display name
    pub name: String,
    /// Version used when none is requested
    pub default_version: String,
    /// Available versions (newest first)
    pub versions: Vec<String>,
    #[serde(default = "default_client_port")]
    pub default_port: u16,
    /// Two-character client code used in the peer ID (e.g. "qB" -> "-qB5300-...")
    pub peer_id_code: String,
    /// User-Agent template, `{version}` is replaced by the emulated version
    pub user_agent: String,
    #[serde(default)]
    pub version_scheme: VersionScheme,
    /// Version code used when the version has too few components
    #[serde(default)]
    pub fallback_version_code: Option<String>,
    #[serde(default = "default_client_num_want")]
    pub num_want: u32,
    #[serde(default = "default_true")]
    pub supports_compact: bool,
    #[serde(default = "default_true")]
    pub supports_crypto: bool,
    #[serde(default = "default_http_version")]
    pub http_version: HttpVersion,
}

const fn default_client_port() -> u16 {
    6881
}

const fn default_client_num_want() -> u32 {
    200
}

const fn default_true() -> bool {
    true
}

const fn default_http_version() -> HttpVersion {
    HttpVersion::Http11
}

impl ClientDefinition {
    /// A definition is usable when it targets a known client and has everything
    /// needed to build a valid 20-character peer ID and a user agent.
    pub fn validate(&self) -> Result<(), String> {
        if ClientType::from_known_id(&self.id).is_none() {
            return Err(format!("unknown client id '{}'", self.id));
        }
        if self.default_version.trim().is_empty() {
            return Err(format!("client '{}': default_version is empty", self.id));
        }
        if self.versions.is_empty() {
            return Err(format!("client '{}': versions is empty", self.id));
        }
        if self.peer_id_code.chars().count() != 2
            || !self.peer_id_code.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Err(format!(
                "client '{}': peer_id_code must be exactly 2 ASCII alphanumeric characters",
                self.id
            ));
        }
        if self.user_agent.trim().is_empty() {
            return Err(format!("client '{}': user_agent is empty", self.id));
        }
        Ok(())
    }

    pub fn info(&self) -> ClientInfo {
        ClientInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            default_version: self.default_version.clone(),
            versions: self.versions.clone(),
            default_port: self.default_port,
        }
    }

    /// Minimal definition used only if the embedded JSON is unreadable.
    fn placeholder(id: &str) -> Self {
        Self {
            id: id.to_string(),
            name: id.to_string(),
            default_version: "1.0.0".to_string(),
            versions: vec!["1.0.0".to_string()],
            default_port: default_client_port(),
            peer_id_code: "XX".to_string(),
            user_agent: format!("{id}/{{version}}"),
            version_scheme: VersionScheme::default(),
            fallback_version_code: None,
            num_want: default_client_num_want(),
            supports_compact: true,
            supports_crypto: true,
            http_version: default_http_version(),
        }
    }

    fn version_code(&self, version: &str) -> String {
        let parts: Vec<&str> = version.split('.').collect();
        let fallback = || self.fallback_version_code.clone().unwrap_or_else(|| "0000".to_string());

        let code = match self.version_scheme {
            VersionScheme::Digits => version.replace('.', ""),
            VersionScheme::MajorMinorPatch => {
                if parts.len() >= 3 {
                    format!("{}{}{}", parts[0], parts[1], parts[2])
                } else {
                    fallback()
                }
            }
            VersionScheme::MajorMinorPad2 => {
                if parts.len() >= 2 {
                    format!("{}{}", parts[0], parts[1].pad_to_width_with_char(2, '0'))
                } else {
                    fallback()
                }
            }
        };

        // Pad to exactly 4 characters
        code.pad_to_width_with_char(4, '0')
    }

    /// Build the runtime configuration for this client at the given version.
    pub fn to_config(&self, client_type: ClientType, version: Option<String>) -> ClientConfig {
        let version = version.unwrap_or_else(|| self.default_version.clone());
        let padded_version = self.version_code(&version);

        ClientConfig {
            client_type,
            peer_id_prefix: format!("-{}{padded_version}-", self.peer_id_code),
            user_agent: self.user_agent.replace("{version}", &version),
            version,
            http_version: self.http_version.clone(),
            num_want: self.num_want,
            supports_compact: self.supports_compact,
            supports_crypto: self.supports_crypto,
        }
    }
}

static BUILTIN_DEFINITIONS: OnceLock<Vec<ClientDefinition>> = OnceLock::new();
static SAVED_DEFINITIONS: RwLock<Vec<ClientDefinition>> = RwLock::new(Vec::new());

/// Definitions shipped with the application (`clients.default.json`).
pub fn builtin_client_definitions() -> &'static [ClientDefinition] {
    BUILTIN_DEFINITIONS.get_or_init(|| {
        serde_json::from_str(BUILTIN_CLIENTS_JSON).unwrap_or_else(|e| {
            log::error!("Failed to parse built-in client definitions: {e}");
            Vec::new()
        })
    })
}

/// Replace the active client definitions (e.g. loaded from the state file).
///
/// Invalid entries are skipped with a warning and the built-in definition is
/// used for those clients. Returns the number of definitions accepted.
pub fn set_client_definitions(definitions: Vec<ClientDefinition>) -> usize {
    let mut accepted: Vec<ClientDefinition> = Vec::new();

    for definition in definitions {
        match definition.validate() {
            Ok(()) => {
                // Last entry wins if an id is listed twice
                accepted.retain(|d| d.id != definition.id);
                accepted.push(definition);
            }
            Err(reason) => log::warn!("Ignoring invalid client definition: {reason}"),
        }
    }

    let count = accepted.len();
    if let Ok(mut saved) = SAVED_DEFINITIONS.write() {
        *saved = accepted;
    }
    count
}

/// Definition currently in effect for a client (saved one, else built-in).
pub fn client_definition(client: ClientType) -> ClientDefinition {
    let id = client.id();

    if let Ok(saved) = SAVED_DEFINITIONS.read() {
        if let Some(definition) = saved.iter().find(|d| d.id == id) {
            return definition.clone();
        }
    }

    builtin_client_definitions()
        .iter()
        .find(|d| d.id == id)
        .cloned()
        .unwrap_or_else(|| ClientDefinition::placeholder(id))
}

/// All definitions currently in effect, in `ClientType::all()` order.
/// This is what gets persisted next to `default_config`.
pub fn client_definitions() -> Vec<ClientDefinition> {
    ClientType::all().into_iter().map(client_definition).collect()
}

impl ClientType {
    /// Machine-readable ID of this client (matches the serde name)
    pub const fn id(self) -> &'static str {
        match self {
            Self::UTorrent => "utorrent",
            Self::QBittorrent => "qbittorrent",
            Self::Transmission => "transmission",
            Self::Deluge => "deluge",
            Self::BitTorrent => "bittorrent",
            Self::RTorrent => "rtorrent",
        }
    }

    fn from_known_id(id: &str) -> Option<Self> {
        Self::all().into_iter().find(|c| c.id() == id)
    }

    /// Get all available client types
    pub fn all() -> Vec<Self> {
        vec![
            Self::UTorrent,
            Self::QBittorrent,
            Self::Transmission,
            Self::Deluge,
            Self::BitTorrent,
            Self::RTorrent,
        ]
    }

    /// Get all client type IDs as strings
    pub fn all_ids() -> Vec<String> {
        Self::all().iter().map(|c| c.info().id).collect()
    }

    /// Get all client infos
    pub fn all_infos() -> Vec<ClientInfo> {
        Self::all().iter().map(Self::info).collect()
    }

    /// Parse a client type from its string ID
    pub fn from_id(id: &str) -> Option<Self> {
        Self::from_known_id(id)
    }

    /// Get full metadata for this client type
    pub fn info(&self) -> ClientInfo {
        client_definition(*self).info()
    }
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub client_type: ClientType,
    pub version: String,
    pub peer_id_prefix: String,
    pub user_agent: String,
    pub http_version: HttpVersion,
    pub num_want: u32,
    pub supports_compact: bool,
    pub supports_crypto: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HttpVersion {
    Http10,
    Http11,
}

impl ClientConfig {
    /// Get configuration for a specific client, built from its active definition
    pub fn get(client_type: ClientType, version: Option<String>) -> Self {
        client_definition(client_type).to_config(client_type, version)
    }

    /// Generate a random peer ID based on this client config
    pub fn generate_peer_id(&self) -> String {
        let mut rng = rand::rng();
        let random_suffix: String = (0..12)
            .map(|_| {
                let chars = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
                chars[rng.random_range(0..chars.len())] as char
            })
            .collect();

        format!("{}{}", self.peer_id_prefix, random_suffix)
    }

    /// Generate a random key (8 hex characters)
    pub fn generate_key() -> String {
        let mut rng = rand::rng();
        (0..8).fold(String::new(), |mut acc, _| {
            let _ = write!(acc, "{:X}", rng.random_range(0..16));
            acc
        })
    }
}

trait PadString {
    fn pad_to_width_with_char(&self, width: usize, ch: char) -> String;
}

impl PadString for str {
    fn pad_to_width_with_char(&self, width: usize, ch: char) -> String {
        if self.len() >= width {
            self[..width].to_string()
        } else {
            format!("{}{}", self, ch.to_string().repeat(width - self.len()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_id_generation_qbittorrent() {
        let config = ClientConfig::get(ClientType::QBittorrent, None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20, "Peer ID must be exactly 20 characters");
        assert!(peer_id.starts_with("-qB"), "qBittorrent peer ID should start with -qB");

        // Test with specific version
        let config = ClientConfig::get(ClientType::QBittorrent, Some("5.2.0".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-qB5200-"), "Peer ID should include version 5.2.0");

        let config = ClientConfig::get(ClientType::QBittorrent, Some("5.3.0".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-qB5300-"), "Peer ID should include version 5.3.0");
    }

    #[test]
    fn test_qbittorrent_info() {
        let info = ClientType::QBittorrent.info();
        assert_eq!(info.id, "qbittorrent");
        assert_eq!(info.default_version, "5.3.0");
        assert_eq!(info.versions.first(), Some(&info.default_version));
        assert!(info.versions.contains(&"5.2.4".to_string()));
    }

    #[test]
    fn test_peer_id_generation_utorrent() {
        let config = ClientConfig::get(ClientType::UTorrent, None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20);
        assert!(peer_id.starts_with("-UT"), "µTorrent peer ID should start with -UT");

        // Test with specific version
        let config = ClientConfig::get(ClientType::UTorrent, Some("3.5.5".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-UT355"), "Peer ID should include version 3.5.5");

        let config = ClientConfig::get(ClientType::UTorrent, Some("3.6.0".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-UT3600-"), "Peer ID should include version 3.6.0");
    }

    #[test]
    fn test_utorrent_info() {
        let info = ClientType::UTorrent.info();
        assert_eq!(info.id, "utorrent");
        assert_eq!(info.default_version, "3.6.0");
        assert_eq!(info.versions.first(), Some(&info.default_version));
        assert!(info.versions.contains(&"3.5.5".to_string()));
    }

    #[test]
    fn test_peer_id_generation_transmission() {
        let config = ClientConfig::get(ClientType::Transmission, None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20);
        assert!(peer_id.starts_with("-TR"), "Transmission peer ID should start with -TR");

        let config = ClientConfig::get(ClientType::Transmission, Some("4.1.3".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-TR4100-"), "Peer ID should include version 4.1.3");
    }

    #[test]
    fn test_transmission_info() {
        let info = ClientType::Transmission.info();
        assert_eq!(info.id, "transmission");
        assert_eq!(info.default_version, "4.1.4");
        assert_eq!(info.versions.first(), Some(&info.default_version));
        assert!(info.versions.contains(&"4.1.3".to_string()));
    }

    #[test]
    fn test_deluge_info() {
        let info = ClientType::Deluge.info();
        assert_eq!(info.id, "deluge");
        assert_eq!(info.default_version, "2.2.0");
        assert_eq!(info.versions.first(), Some(&info.default_version));
    }

    #[test]
    fn test_peer_id_generation_deluge() {
        let config = ClientConfig::get(ClientType::Deluge, None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20);
        assert!(peer_id.starts_with("-DE"), "Deluge peer ID should start with -DE");
    }

    #[test]
    fn test_peer_id_generation_bittorrent() {
        let config = ClientConfig::get(ClientType::BitTorrent, None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20);
        assert!(peer_id.starts_with("-BT"), "BitTorrent peer ID should start with -BT");

        // Test with specific version
        let config = ClientConfig::get(ClientType::BitTorrent, Some("7.11.0".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-BT7110-"), "Peer ID should include version 7.11.0");
    }

    #[test]
    fn test_peer_id_generation_rtorrent() {
        let config = ClientConfig::get(ClientType::RTorrent, None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20);
        assert!(peer_id.starts_with("-RT"), "rTorrent peer ID should start with -RT");

        let config = ClientConfig::get(ClientType::RTorrent, Some("0.16.12".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-RT0161-"), "Peer ID should include version 0.16.12");
    }

    #[test]
    fn test_peer_id_uniqueness() {
        let config = ClientConfig::get(ClientType::QBittorrent, None);
        let peer_id1 = config.generate_peer_id();
        let peer_id2 = config.generate_peer_id();

        // Peer IDs should be different (random suffixes)
        assert_ne!(peer_id1, peer_id2, "Generated peer IDs should be unique");
    }

    #[test]
    fn test_peer_id_valid_characters() {
        let config = ClientConfig::get(ClientType::QBittorrent, None);
        let peer_id = config.generate_peer_id();

        // All characters should be valid (alphanumeric or -)
        assert!(peer_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[test]
    fn test_key_generation() {
        let key = ClientConfig::generate_key();
        assert_eq!(key.len(), 8, "Key must be exactly 8 characters");
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()), "Key must be hexadecimal");
    }

    #[test]
    fn test_key_uniqueness() {
        let key1 = ClientConfig::generate_key();
        let key2 = ClientConfig::generate_key();

        // Keys should be different (random)
        assert_ne!(key1, key2, "Generated keys should be unique");
    }

    #[test]
    fn test_key_uppercase() {
        let key = ClientConfig::generate_key();
        // All hex digits should be uppercase
        assert!(key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
    }

    #[test]
    fn test_client_config_qbittorrent() {
        let config = ClientConfig::get(ClientType::QBittorrent, None);
        assert_eq!(config.client_type, ClientType::QBittorrent);
        assert!(config.user_agent.contains("qBittorrent"));
        assert_eq!(config.http_version, HttpVersion::Http11);
        assert!(config.supports_compact);
        assert!(config.supports_crypto);
    }

    #[test]
    fn test_client_config_utorrent() {
        let config = ClientConfig::get(ClientType::UTorrent, None);
        assert_eq!(config.client_type, ClientType::UTorrent);
        assert!(config.user_agent.contains("uTorrent") || config.user_agent.contains("µTorrent"));
        assert_eq!(config.http_version, HttpVersion::Http11);
    }

    #[test]
    fn test_client_config_transmission() {
        let config = ClientConfig::get(ClientType::Transmission, None);
        assert_eq!(config.client_type, ClientType::Transmission);
        assert!(config.user_agent.contains("Transmission"));
    }

    #[test]
    fn test_client_config_deluge() {
        let config = ClientConfig::get(ClientType::Deluge, None);
        assert_eq!(config.client_type, ClientType::Deluge);
        assert!(config.user_agent.contains("Deluge"));
    }

    #[test]
    fn test_client_config_bittorrent() {
        let config = ClientConfig::get(ClientType::BitTorrent, None);
        assert_eq!(config.client_type, ClientType::BitTorrent);
        assert!(config.user_agent.contains("BitTorrent"));
        assert_eq!(config.http_version, HttpVersion::Http11);
        assert!(config.supports_compact);
        assert!(config.supports_crypto);
    }

    #[test]
    fn test_client_config_rtorrent() {
        let config = ClientConfig::get(ClientType::RTorrent, None);
        assert_eq!(config.client_type, ClientType::RTorrent);
        assert!(config.user_agent.contains("rTorrent"));
        assert_eq!(config.http_version, HttpVersion::Http11);
        assert!(config.supports_compact);
        assert!(config.supports_crypto);
    }

    #[test]
    fn test_client_config_with_version() {
        let config = ClientConfig::get(ClientType::QBittorrent, Some("4.5.0".to_string()));
        assert_eq!(config.version, "4.5.0");
        assert!(config.user_agent.contains("4.5.0"));
    }

    #[test]
    fn test_pad_string_trait() {
        assert_eq!("12".pad_to_width_with_char(4, '0'), "1200");
        assert_eq!("1234".pad_to_width_with_char(4, '0'), "1234");
        assert_eq!("12345".pad_to_width_with_char(4, '0'), "1234");
        assert_eq!("1".pad_to_width_with_char(3, 'x'), "1xx");
    }

    #[test]
    fn test_rtorrent_info() {
        let info = ClientType::RTorrent.info();
        assert_eq!(info.id, "rtorrent");
        assert_eq!(info.name, "rTorrent");
        assert_eq!(info.default_version, "0.16.26");
        assert_eq!(info.versions.first(), Some(&info.default_version));
        assert_eq!(info.default_port, 6881);
    }

    #[test]
    fn test_rtorrent_in_all_and_from_id() {
        let all = ClientType::all();
        assert!(all.contains(&ClientType::RTorrent));
        assert_eq!(ClientType::from_id("rtorrent"), Some(ClientType::RTorrent));
    }

    #[test]
    fn test_builtin_definitions_cover_all_clients_and_are_valid() {
        let builtin = builtin_client_definitions();
        for client in ClientType::all() {
            let definition = builtin
                .iter()
                .find(|d| d.id == client.id())
                .unwrap_or_else(|| unreachable!("missing built-in definition for {}", client.id()));
            assert!(definition.validate().is_ok(), "invalid built-in definition: {}", client.id());
            assert_eq!(definition.versions.first(), Some(&definition.default_version));
        }
    }

    #[test]
    fn test_definition_drives_config() {
        let mut definition = client_definition(ClientType::QBittorrent);
        definition.peer_id_code = "zZ".to_string();
        definition.user_agent = "Custom/{version} (test)".to_string();
        definition.num_want = 42;

        let config = definition.to_config(ClientType::QBittorrent, Some("5.2.0".to_string()));
        assert_eq!(config.peer_id_prefix, "-zZ5200-");
        assert_eq!(config.user_agent, "Custom/5.2.0 (test)");
        assert_eq!(config.num_want, 42);
        assert_eq!(config.generate_peer_id().len(), 20);
    }

    #[test]
    fn test_version_scheme_fallbacks() {
        let transmission = client_definition(ClientType::Transmission);
        let config = transmission.to_config(ClientType::Transmission, Some("4".to_string()));
        assert!(config.peer_id_prefix.starts_with("-TR4000-"));

        let rtorrent = client_definition(ClientType::RTorrent);
        let config = rtorrent.to_config(ClientType::RTorrent, Some("1".to_string()));
        assert!(config.peer_id_prefix.starts_with("-RT0161-"));
    }

    #[test]
    fn test_validate_rejects_bad_definitions() {
        let mut definition = client_definition(ClientType::Deluge);
        assert!(definition.validate().is_ok());

        definition.peer_id_code = "DEL".to_string();
        assert!(definition.validate().is_err());

        let mut definition = client_definition(ClientType::Deluge);
        definition.id = "unknown".to_string();
        assert!(definition.validate().is_err());

        let mut definition = client_definition(ClientType::Deluge);
        definition.versions.clear();
        assert!(definition.validate().is_err());
    }

    #[test]
    fn test_definitions_json_round_trip_with_defaults() {
        let json = r#"{
            "id": "deluge", "name": "Deluge", "default_version": "2.2.0",
            "versions": ["2.2.0"], "peer_id_code": "DE", "user_agent": "Deluge/{version}"
        }"#;
        let definition: ClientDefinition =
            serde_json::from_str(json).unwrap_or_else(|_| unreachable!());
        assert_eq!(definition.default_port, 6881);
        assert_eq!(definition.num_want, 200);
        assert_eq!(definition.http_version, HttpVersion::Http11);
        assert_eq!(definition.version_scheme, VersionScheme::MajorMinorPatch);
    }
}
