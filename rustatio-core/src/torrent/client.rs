use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClientType {
    #[serde(rename = "utorrent")]
    UTorrent,
    #[serde(rename = "qbittorrent")]
    QBittorrent,
    #[serde(rename = "transmission")]
    Transmission,
    #[serde(rename = "deluge")]
    Deluge,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpVersion {
    Http10,
    Http11,
}

impl ClientConfig {
    /// Get configuration for a specific client
    pub fn get(client_type: ClientType, version: Option<String>) -> Self {
        match client_type {
            ClientType::UTorrent => Self::utorrent(version),
            ClientType::QBittorrent => Self::qbittorrent(version),
            ClientType::Transmission => Self::transmission(version),
            ClientType::Deluge => Self::deluge(version),
        }
    }

    /// uTorrent client configuration
    fn utorrent(version: Option<String>) -> Self {
        let version = version.unwrap_or_else(|| "3.5.5".to_string());
        let version_code = version.replace('.', "");

        // Pad to exactly 4 characters
        let padded_version = version_code.pad_to_width_with_char(4, '0');

        ClientConfig {
            client_type: ClientType::UTorrent,
            version: version.clone(),
            peer_id_prefix: format!("-UT{}-", padded_version),
            user_agent: format!("uTorrent/{}", version),
            http_version: HttpVersion::Http11,
            num_want: 200,
            supports_compact: true,
            supports_crypto: true,
        }
    }

    /// qBittorrent client configuration
    fn qbittorrent(version: Option<String>) -> Self {
        let version = version.unwrap_or_else(|| "5.1.4".to_string());
        let parts: Vec<&str> = version.split('.').collect();
        let version_code = if parts.len() >= 3 {
            format!("{}{}{}", parts[0], parts[1], parts[2])
        } else {
            "514".to_string()
        };

        // Pad to exactly 4 characters
        let padded_version = version_code.pad_to_width_with_char(4, '0');

        ClientConfig {
            client_type: ClientType::QBittorrent,
            version: version.clone(),
            peer_id_prefix: format!("-qB{}-", padded_version),
            user_agent: format!("qBittorrent/{}", version),
            http_version: HttpVersion::Http11,
            num_want: 200,
            supports_compact: true,
            supports_crypto: true,
        }
    }

    /// Transmission client configuration
    fn transmission(version: Option<String>) -> Self {
        let version = version.unwrap_or_else(|| "4.0.5".to_string());
        let parts: Vec<&str> = version.split('.').collect();
        let version_code = if parts.len() >= 2 {
            format!("{}{}", parts[0], parts[1].pad_to_width_with_char(2, '0'))
        } else {
            "400".to_string()
        };

        // Pad to exactly 4 characters
        let padded_version = version_code.pad_to_width_with_char(4, '0');

        ClientConfig {
            client_type: ClientType::Transmission,
            version: version.clone(),
            peer_id_prefix: format!("-TR{}-", padded_version),
            user_agent: format!("Transmission/{}", version),
            http_version: HttpVersion::Http11,
            num_want: 80,
            supports_compact: true,
            supports_crypto: true,
        }
    }

    /// Deluge client configuration
    fn deluge(version: Option<String>) -> Self {
        let version = version.unwrap_or_else(|| "2.1.1".to_string());
        let parts: Vec<&str> = version.split('.').collect();
        let version_code = if parts.len() >= 3 {
            format!("{}{}{}", parts[0], parts[1], parts[2])
        } else {
            "211".to_string()
        };

        // Pad to exactly 4 characters
        let padded_version = version_code.pad_to_width_with_char(4, '0');

        ClientConfig {
            client_type: ClientType::Deluge,
            version: version.clone(),
            peer_id_prefix: format!("-DE{}-", padded_version),
            user_agent: format!("Deluge/{}", version),
            http_version: HttpVersion::Http11,
            num_want: 200,
            supports_compact: true,
            supports_crypto: true,
        }
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
        (0..8).map(|_| format!("{:X}", rng.random_range(0..16))).collect()
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
        let config = ClientConfig::get(ClientType::QBittorrent, Some("5.1.4".to_string()));
        let peer_id = config.generate_peer_id();
        assert!(peer_id.starts_with("-qB5140-"), "Peer ID should include version 5.1.4");
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
    }

    #[test]
    fn test_peer_id_generation_transmission() {
        let config = ClientConfig::get(ClientType::Transmission, None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20);
        assert!(peer_id.starts_with("-TR"), "Transmission peer ID should start with -TR");
    }

    #[test]
    fn test_peer_id_generation_deluge() {
        let config = ClientConfig::get(ClientType::Deluge, None);
        let peer_id = config.generate_peer_id();
        assert_eq!(peer_id.len(), 20);
        assert!(peer_id.starts_with("-DE"), "Deluge peer ID should start with -DE");
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
}
