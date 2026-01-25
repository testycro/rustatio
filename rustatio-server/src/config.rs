use rustatio_core::FakerConfig;
use rustatio_core::torrent::ClientType;
use std::env;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    // [client]
    pub client_default_type: String,
    pub client_default_port: u16,
    pub client_default_num_want: u32,

    // [faker]
    pub faker_default_upload_rate: f64,
    pub faker_default_download_rate: f64,
    pub faker_default_announce_interval: u64,
    pub faker_update_interval: u64,
    pub faker_default_announce_max_retries: u32,
    pub faker_default_announce_retry_ms: u64,

    // [ui]
    pub ui_window_width: u32,
    pub ui_window_height: u32,
    pub ui_dark_mode: bool,
}

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    match env::var(key) {
        Ok(val) => val.parse().unwrap_or(default),
        Err(_) => default,
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(v) => matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}

impl ServerConfig {
    pub fn from_env() -> Self {
        Self {
            // [client]
            client_default_type: env::var("CLIENT_DEFAULT_TYPE")
                .unwrap_or_else(|_| "transmission".into()),

            client_default_port: env_or("CLIENT_DEFAULT_PORT", 59859),
            client_default_num_want: env_or("CLIENT_DEFAULT_NUM_WANT", 50),

            // [faker]
            faker_default_upload_rate: env_or("FAKER_DEFAULT_UPLOAD_RATE", 700.0),
            faker_default_download_rate: env_or("FAKER_DEFAULT_DOWNLOAD_RATE", 0.0),
            faker_default_announce_interval: env_or("FAKER_DEFAULT_ANNOUNCE_INTERVAL", 1800),
            faker_update_interval: env_or("FAKER_UPDATE_INTERVAL", 5),
            faker_default_announce_max_retries: env_or("FAKER_DEFAULT_ANNOUNCE_MAX_RETRIES", 10),
            faker_default_announce_retry_ms: env_or("FAKER_DEFAULT_ANNOUNCE_RETRY_MS", 5000),

            // [ui]
            ui_window_width: env_or("UI_WINDOW_WIDTH", 1200),
            ui_window_height: env_or("UI_WINDOW_HEIGHT", 800),
            ui_dark_mode: env_bool("UI_DARK_MODE", true),
        }
    }

    pub fn to_faker_defaults(&self) -> FakerConfig {
        FakerConfig {
            upload_rate: self.faker_default_upload_rate,
            download_rate: self.faker_default_download_rate,
            port: self.client_default_port,
            client_type: self.client_default_type
                .parse::<ClientType>()
                .unwrap_or(ClientType::Transmission),
            client_version: None,
            initial_uploaded: 0,
            initial_downloaded: 0,
            completion_percent: 100.0,
            num_want: self.client_default_num_want,
            randomize_rates: true,
            random_range_percent: 50.0,
            stop_at_ratio: None,
            stop_at_uploaded: None,
            stop_at_downloaded: None,
            stop_at_seed_time: Some(2678400),
            stop_when_no_leechers: false,
            progressive_rates: false,
            target_upload_rate: None,
            target_download_rate: None,
            progressive_duration: 3600,
            announce_max_retries: self.faker_default_announce_max_retries,
            announce_retry_delay_ms: self.faker_default_announce_retry_ms,
            announce_interval: self.faker_default_announce_interval,
            update_interval: self.faker_update_interval,
        }
    }
}
