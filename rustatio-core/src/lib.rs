pub mod config;
pub mod faker;
pub mod grid;
pub mod logger;
#[cfg(not(target_arch = "wasm32"))]
pub mod peer_listener;
pub mod protocol;
pub mod proxy;
pub mod torrent;
pub mod validation;

// Re-export main types explicitly to avoid ambiguous Result types
pub use config::{AppConfig, ClientSettings, ConfigError, FakerSettings, UiSettings};
#[cfg(not(target_arch = "wasm32"))]
pub use faker::RatioFakerHandle;
pub use faker::{
    FakerConfig, FakerError, FakerState, FakerStats, PostStopAction, PresetSettings, RatioFaker,
};
pub use grid::{primary_tracker_host, GridImportSettings, GridMode, InstanceSummary};
#[cfg(not(target_arch = "wasm32"))]
pub use peer_listener::{PeerCatalog, PeerListenerService, PeerListenerStatus, PeerLookup};
pub use proxy::ProxySettings;
pub use torrent::{
    builtin_client_definitions, client_definition, client_definitions, set_client_definitions,
    ClientConfig, ClientDefinition, ClientInfo, ClientType, HttpVersion, TorrentError,
    TorrentFile, TorrentInfo, TorrentSummary, VersionScheme,
};
pub use validation::*;

// Re-export reqwest::Client for downstream crates that need shared HTTP clients
#[cfg(any(feature = "native", feature = "wasm"))]
pub use reqwest;
