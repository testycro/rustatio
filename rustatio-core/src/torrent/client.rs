use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Write};
use std::sync::{OnceLock, RwLock};

/// Built-in client definitions, used as the seed when nothing has been saved
/// yet (or when every saved definition is invalid).
const BUILTIN_CLIENTS_JSON: &str = include_str!("clients.default.json");

/// Identifier of an emulated client (e.g. `"qbittorrent"`).
///
/// Clients are no longer a closed list: any [`ClientDefinition`] present in the
/// active definitions can be selected. The identifier serializes as a plain
/// string, so existing configs and APIs are unchanged. An identifier that no
/// longer exists (client deleted) is not an error: it resolves to the default
/// client when a [`ClientConfig`] is built.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClientType(String);

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
/// The list of definitions is persisted next to `default_config` in the state
/// file: add an entry to add a client, remove one to delete it. When nothing is
/// saved (or no saved entry is valid) `clients.default.json` is used.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientDefinition {
    /// Machine-readable ID: lowercase letters, digits, '-' or '_' (e.g. "qbittorrent")
    pub id: String,
    /// Human-readable display name
    pub name: String,
    /// Version used when none is requested
    pub default_version: String,
    /// Available versions (newest first)
    pub versions: Vec<String>,
    #[serde(default = "default_client_port")]
    pub default_port: u16,
    /// Client used when none is selected or when the selected one no longer exists.
    /// If several (or none) are flagged, the first flagged one (or the first client) wins.
    #[serde(default)]
    pub is_default: bool,
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
    /// A definition is usable when it has everything needed to build a valid
    /// 20-character peer ID and a user agent.
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(format!(
                "invalid client id '{}': use lowercase letters, digits, '-' or '_'",
                self.id
            ));
        }
        if self.name.trim().is_empty() {
            return Err(format!("client '{}': name is empty", self.id));
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
            is_default: true,
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
    #[allow(clippy::literal_string_with_formatting_args)]
    pub fn to_config(&self, version: Option<String>) -> ClientConfig {
        let version = version.unwrap_or_else(|| self.default_version.clone());
        let padded_version = self.version_code(&version);

        ClientConfig {
            client_type: ClientType::new(self.id.clone()),
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
/// `None` means "use the built-in definitions".
static SAVED_DEFINITIONS: RwLock<Option<Vec<ClientDefinition>>> = RwLock::new(None);

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
/// Invalid entries are skipped with a warning (a later entry replaces an
/// earlier one with the same id). If nothing valid remains, the built-in
/// definitions are used. Returns the number of definitions accepted.
pub fn set_client_definitions(definitions: Vec<ClientDefinition>) -> usize {
    let mut accepted: Vec<ClientDefinition> = Vec::new();

    for definition in definitions {
        match definition.validate() {
            Ok(()) => {
                accepted.retain(|d| d.id != definition.id);
                accepted.push(definition);
            }
            Err(reason) => log::warn!("Ignoring invalid client definition: {reason}"),
        }
    }

    let count = accepted.len();
    if count == 0 {
        log::warn!("No valid client definition found, using the built-in ones");
    }

    if let Ok(mut saved) = SAVED_DEFINITIONS.write() {
        *saved = (count > 0).then_some(accepted);
    }
    count
}

/// Go back to the built-in definitions.
pub fn reset_client_definitions() {
    if let Ok(mut saved) = SAVED_DEFINITIONS.write() {
        *saved = None;
    }
}

/// Add a client, or replace the one with the same id.
pub fn upsert_client_definition(definition: ClientDefinition) -> Result<(), String> {
    update_definitions(|list| with_upserted(list, definition))
}

/// Delete a client. The last remaining client cannot be deleted.
pub fn remove_client_definition(id: &str) -> Result<(), String> {
    update_definitions(|list| without_client(list, id))
}

fn update_definitions<F>(change: F) -> Result<(), String>
where
    F: FnOnce(Vec<ClientDefinition>) -> Result<Vec<ClientDefinition>, String>,
{
    let mut saved = SAVED_DEFINITIONS.write().map_err(|_| "client registry unavailable")?;
    let current = saved.clone().unwrap_or_else(|| builtin_client_definitions().to_vec());
    *saved = Some(change(current)?);
    Ok(())
}

fn with_upserted(
    mut list: Vec<ClientDefinition>,
    definition: ClientDefinition,
) -> Result<Vec<ClientDefinition>, String> {
    definition.validate()?;

    if definition.is_default {
        for existing in &mut list {
            existing.is_default = false;
        }
    }

    match list.iter_mut().find(|d| d.id == definition.id) {
        Some(existing) => *existing = definition,
        None => list.push(definition),
    }
    Ok(list)
}

fn without_client(
    mut list: Vec<ClientDefinition>,
    id: &str,
) -> Result<Vec<ClientDefinition>, String> {
    let before = list.len();
    list.retain(|d| d.id != id);

    if list.len() == before {
        return Err(format!("client '{id}' not found"));
    }
    if list.is_empty() {
        return Err("the last client cannot be deleted".to_string());
    }
    Ok(list)
}

/// All definitions currently in effect (saved ones, else built-in), in display order.
/// This is what gets persisted next to `default_config`.
pub fn client_definitions() -> Vec<ClientDefinition> {
    if let Ok(saved) = SAVED_DEFINITIONS.read() {
        if let Some(definitions) = saved.as_ref() {
            return definitions.clone();
        }
    }
    builtin_client_definitions().to_vec()
}

/// Definition of the client with this id, if it exists.
pub fn client_definition(id: &str) -> Option<ClientDefinition> {
    client_definitions().into_iter().find(|d| d.id == id)
}

/// Client used when none is selected: the one flagged `is_default`, else the first.
pub fn default_client_definition() -> ClientDefinition {
    let mut definitions = client_definitions();
    let index = definitions.iter().position(|d| d.is_default).unwrap_or(0);

    if index < definitions.len() {
        definitions.swap_remove(index)
    } else {
        ClientDefinition::placeholder("qbittorrent")
    }
}

impl ClientType {
    /// Wrap an identifier. It is not checked against the definitions.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Machine-readable ID of this client
    pub fn id(&self) -> &str {
        &self.0
    }

    /// Get all available client types
    pub fn all() -> Vec<Self> {
        client_definitions().into_iter().map(|d| Self(d.id)).collect()
    }

    /// Get all client type IDs as strings
    pub fn all_ids() -> Vec<String> {
        client_definitions().into_iter().map(|d| d.id).collect()
    }

    /// Get all client infos
    pub fn all_infos() -> Vec<ClientInfo> {
        client_definitions().iter().map(ClientDefinition::info).collect()
    }

    /// Parse a client type from its string ID (only if such a client exists)
    pub fn from_id(id: &str) -> Option<Self> {
        client_definition(id).map(|d| Self(d.id))
    }

    /// Definition of this client; an unknown id resolves to the default client
    pub fn definition(&self) -> ClientDefinition {
        client_definition(&self.0).unwrap_or_else(|| {
            log::warn!("Unknown client '{}', falling back to the default client", self.0);
            default_client_definition()
        })
    }

    /// Get full metadata for this client type
    pub fn info(&self) -> ClientInfo {
        self.definition().info()
    }
}

impl Default for ClientType {
    fn default() -> Self {
        Self(default_client_definition().id)
    }
}

impl fmt::Display for ClientType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
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
    pub fn get(client_type: &ClientType, version: Option<String>) -> Self {
        client_type.definition().to_config(version)
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
        let config = ClientConfig::get(&ClientType::new("qbittorrent"), None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20, "Peer ID must be exactly 20 characters");
        assert!(peer_id.starts_with("-qB"), "qBittorrent peer ID should start with -qB");

        // Test with specific version
        let config = ClientConfig::get(&ClientType::new("qbittorrent"), Some("5.2.0".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-qB5200-"), "Peer ID should include version 5.2.0");

        let config = ClientConfig::get(&ClientType::new("qbittorrent"), Some("5.3.0".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-qB5300-"), "Peer ID should include version 5.3.0");
    }

    #[test]
    fn test_qbittorrent_info() {
        let info = ClientType::new("qbittorrent").info();
        assert_eq!(info.id, "qbittorrent");
        assert_eq!(info.default_version, "5.3.0");
        assert_eq!(info.versions.first(), Some(&info.default_version));
        assert!(info.versions.contains(&"5.2.4".to_string()));
    }

    #[test]
    fn test_peer_id_generation_utorrent() {
        let config = ClientConfig::get(&ClientType::new("utorrent"), None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20);
        assert!(peer_id.starts_with("-UT"), "µTorrent peer ID should start with -UT");

        // Test with specific version
        let config = ClientConfig::get(&ClientType::new("utorrent"), Some("3.5.5".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-UT355"), "Peer ID should include version 3.5.5");

        let config = ClientConfig::get(&ClientType::new("utorrent"), Some("3.6.0".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-UT3600-"), "Peer ID should include version 3.6.0");
    }

    #[test]
    fn test_utorrent_info() {
        let info = ClientType::new("utorrent").info();
        assert_eq!(info.id, "utorrent");
        assert_eq!(info.default_version, "3.6.0");
        assert_eq!(info.versions.first(), Some(&info.default_version));
        assert!(info.versions.contains(&"3.5.5".to_string()));
    }

    #[test]
    fn test_peer_id_generation_transmission() {
        let config = ClientConfig::get(&ClientType::new("transmission"), None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20);
        assert!(peer_id.starts_with("-TR"), "Transmission peer ID should start with -TR");

        let config = ClientConfig::get(&ClientType::new("transmission"), Some("4.1.3".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-TR4100-"), "Peer ID should include version 4.1.3");
    }

    #[test]
    fn test_transmission_info() {
        let info = ClientType::new("transmission").info();
        assert_eq!(info.id, "transmission");
        assert_eq!(info.default_version, "4.1.4");
        assert_eq!(info.versions.first(), Some(&info.default_version));
        assert!(info.versions.contains(&"4.1.3".to_string()));
    }

    #[test]
    fn test_deluge_info() {
        let info = ClientType::new("deluge").info();
        assert_eq!(info.id, "deluge");
        assert_eq!(info.default_version, "2.2.0");
        assert_eq!(info.versions.first(), Some(&info.default_version));
    }

    #[test]
    fn test_peer_id_generation_deluge() {
        let config = ClientConfig::get(&ClientType::new("deluge"), None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20);
        assert!(peer_id.starts_with("-DE"), "Deluge peer ID should start with -DE");
    }

    #[test]
    fn test_peer_id_generation_bittorrent() {
        let config = ClientConfig::get(&ClientType::new("bittorrent"), None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20);
        assert!(peer_id.starts_with("-BT"), "BitTorrent peer ID should start with -BT");

        // Test with specific version
        let config = ClientConfig::get(&ClientType::new("bittorrent"), Some("7.11.0".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-BT7110-"), "Peer ID should include version 7.11.0");
    }

    #[test]
    fn test_peer_id_generation_rtorrent() {
        let config = ClientConfig::get(&ClientType::new("rtorrent"), None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20);
        assert!(peer_id.starts_with("-RT"), "rTorrent peer ID should start with -RT");

        let config = ClientConfig::get(&ClientType::new("rtorrent"), Some("0.16.12".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-RT0161-"), "Peer ID should include version 0.16.12");
    }

    #[test]
    fn test_peer_id_uniqueness() {
        let config = ClientConfig::get(&ClientType::new("qbittorrent"), None);
        let peer_id1 = config.generate_peer_id();
        let peer_id2 = config.generate_peer_id();

        // Peer IDs should be different (random suffixes)
        assert_ne!(peer_id1, peer_id2, "Generated peer IDs should be unique");
    }

    #[test]
    fn test_peer_id_valid_characters() {
        let config = ClientConfig::get(&ClientType::new("qbittorrent"), None);
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
        let config = ClientConfig::get(&ClientType::new("qbittorrent"), None);
        assert_eq!(config.client_type, ClientType::new("qbittorrent"));
        assert!(config.user_agent.contains("qBittorrent"));
        assert_eq!(config.http_version, HttpVersion::Http11);
        assert!(config.supports_compact);
        assert!(config.supports_crypto);
    }

    #[test]
    fn test_client_config_utorrent() {
        let config = ClientConfig::get(&ClientType::new("utorrent"), None);
        assert_eq!(config.client_type, ClientType::new("utorrent"));
        assert!(config.user_agent.contains("uTorrent") || config.user_agent.contains("µTorrent"));
        assert_eq!(config.http_version, HttpVersion::Http11);
    }

    #[test]
    fn test_client_config_transmission() {
        let config = ClientConfig::get(&ClientType::new("transmission"), None);
        assert_eq!(config.client_type, ClientType::new("transmission"));
        assert!(config.user_agent.contains("Transmission"));
    }

    #[test]
    fn test_client_config_deluge() {
        let config = ClientConfig::get(&ClientType::new("deluge"), None);
        assert_eq!(config.client_type, ClientType::new("deluge"));
        assert!(config.user_agent.contains("Deluge"));
    }

    #[test]
    fn test_client_config_bittorrent() {
        let config = ClientConfig::get(&ClientType::new("bittorrent"), None);
        assert_eq!(config.client_type, ClientType::new("bittorrent"));
        assert!(config.user_agent.contains("BitTorrent"));
        assert_eq!(config.http_version, HttpVersion::Http11);
        assert!(config.supports_compact);
        assert!(config.supports_crypto);
    }

    #[test]
    fn test_client_config_rtorrent() {
        let config = ClientConfig::get(&ClientType::new("rtorrent"), None);
        assert_eq!(config.client_type, ClientType::new("rtorrent"));
        assert!(config.user_agent.contains("rTorrent"));
        assert_eq!(config.http_version, HttpVersion::Http11);
        assert!(config.supports_compact);
        assert!(config.supports_crypto);
    }

    #[test]
    fn test_client_config_with_version() {
        let config = ClientConfig::get(&ClientType::new("qbittorrent"), Some("4.5.0".to_string()));
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
        let info = ClientType::new("rtorrent").info();
        assert_eq!(info.id, "rtorrent");
        assert_eq!(info.name, "rTorrent");
        assert_eq!(info.default_version, "0.16.26");
        assert_eq!(info.versions.first(), Some(&info.default_version));
        assert_eq!(info.default_port, 6881);
    }

    #[test]
    fn test_rtorrent_in_all_and_from_id() {
        let all = ClientType::all();
        assert!(all.contains(&ClientType::new("rtorrent")));
        assert_eq!(ClientType::from_id("rtorrent"), Some(ClientType::new("rtorrent")));
    }

    fn def(id: &str) -> ClientDefinition {
        client_definition(id).unwrap_or_else(|| unreachable!("missing client {id}"))
    }

    fn custom(id: &str) -> ClientDefinition {
        ClientDefinition {
            id: id.to_string(),
            name: "Custom".to_string(),
            default_version: "1.2.3".to_string(),
            versions: vec!["1.2.3".to_string()],
            default_port: 7000,
            is_default: false,
            peer_id_code: "CU".to_string(),
            user_agent: "Custom/{version}".to_string(),
            version_scheme: VersionScheme::MajorMinorPatch,
            fallback_version_code: None,
            num_want: 10,
            supports_compact: true,
            supports_crypto: false,
            http_version: HttpVersion::Http11,
        }
    }

    #[test]
    fn test_builtin_definitions_are_valid_and_have_one_default() {
        let builtin = builtin_client_definitions();
        assert_eq!(builtin.len(), 6);
        for definition in builtin {
            assert!(
                definition.validate().is_ok(),
                "invalid built-in definition: {}",
                definition.id
            );
            assert_eq!(definition.versions.first(), Some(&definition.default_version));
        }
        let defaults: Vec<_> = builtin.iter().filter(|d| d.is_default).collect();
        assert_eq!(defaults.len(), 1);
        assert_eq!(defaults[0].id, "qbittorrent");
    }

    #[test]
    fn test_definition_drives_config() {
        let mut definition = def("qbittorrent");
        definition.peer_id_code = "zZ".to_string();
        definition.user_agent = "Custom/{version} (test)".to_string();
        definition.num_want = 42;

        let config = definition.to_config(Some("5.2.0".to_string()));
        assert_eq!(config.client_type, ClientType::new("qbittorrent"));
        assert_eq!(config.peer_id_prefix, "-zZ5200-");
        assert_eq!(config.user_agent, "Custom/5.2.0 (test)");
        assert_eq!(config.num_want, 42);
        assert_eq!(config.generate_peer_id().len(), 20);
    }

    #[test]
    fn test_version_scheme_fallbacks() {
        let config = def("transmission").to_config(Some("4".to_string()));
        assert!(config.peer_id_prefix.starts_with("-TR4000-"));

        let config = def("rtorrent").to_config(Some("1".to_string()));
        assert!(config.peer_id_prefix.starts_with("-RT0161-"));
    }

    #[test]
    fn test_validate_rejects_bad_definitions() {
        assert!(def("deluge").validate().is_ok());

        let mut definition = def("deluge");
        definition.peer_id_code = "DEL".to_string();
        assert!(definition.validate().is_err());

        let mut definition = def("deluge");
        definition.id = "Not Valid".to_string();
        assert!(definition.validate().is_err());

        let mut definition = def("deluge");
        definition.id = String::new();
        assert!(definition.validate().is_err());

        let mut definition = def("deluge");
        definition.versions.clear();
        assert!(definition.validate().is_err());
    }

    #[test]
    fn test_with_upserted_adds_and_replaces() {
        let list = builtin_client_definitions().to_vec();
        let count = list.len();

        let list = with_upserted(list, custom("my-client")).unwrap_or_else(|_| unreachable!());
        assert_eq!(list.len(), count + 1);

        let mut changed = custom("my-client");
        changed.name = "Renamed".to_string();
        let list = with_upserted(list, changed).unwrap_or_else(|_| unreachable!());
        assert_eq!(list.len(), count + 1);
        assert_eq!(list.iter().filter(|d| d.id == "my-client").count(), 1);
        assert!(list.iter().any(|d| d.name == "Renamed"));

        assert!(with_upserted(list, custom("Bad Id")).is_err());
    }

    #[test]
    fn test_with_upserted_default_is_exclusive() {
        let mut new_default = custom("my-client");
        new_default.is_default = true;

        let list = with_upserted(builtin_client_definitions().to_vec(), new_default)
            .unwrap_or_else(|_| unreachable!());
        let defaults: Vec<_> =
            list.iter().filter(|d| d.is_default).map(|d| d.id.as_str()).collect();
        assert_eq!(defaults, vec!["my-client"]);
    }

    #[test]
    fn test_without_client() {
        let list = builtin_client_definitions().to_vec();
        let count = list.len();

        let list = without_client(list, "deluge").unwrap_or_else(|_| unreachable!());
        assert_eq!(list.len(), count - 1);
        assert!(list.iter().all(|d| d.id != "deluge"));

        assert!(without_client(list, "deluge").is_err());

        let single = vec![custom("only-one")];
        assert!(without_client(single, "only-one").is_err());
    }

    #[test]
    fn test_client_type_serializes_as_plain_string() {
        let client = ClientType::new("qbittorrent");
        assert_eq!(serde_json::to_string(&client).unwrap_or_default(), "\"qbittorrent\"");

        // Unknown ids must not break deserialization of saved configs
        let parsed: ClientType =
            serde_json::from_str("\"deleted-client\"").unwrap_or_else(|_| unreachable!());
        assert_eq!(parsed.id(), "deleted-client");
    }

    #[test]
    fn test_unknown_client_falls_back_to_default() {
        let config = ClientConfig::get(&ClientType::new("deleted-client"), None);
        assert_eq!(config.client_type, ClientType::default());
        assert_eq!(ClientType::default(), ClientType::new("qbittorrent"));
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
        assert!(!definition.is_default);
        assert_eq!(definition.http_version, HttpVersion::Http11);
        assert_eq!(definition.version_scheme, VersionScheme::MajorMinorPatch);
    }
}
