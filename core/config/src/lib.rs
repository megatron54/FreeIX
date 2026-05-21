use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, warn};

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("toml serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("config directory not found")]
    NoDirs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub dns: DnsConfig,
    pub filter: FilterConfig,
    pub ui: UiConfig,
    pub stats: StatsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsConfig {
    pub listen_addr: SocketAddr,
    pub upstream_servers: Vec<String>,
    pub cache_size: usize,
    pub max_ttl_secs: u64,
    pub min_ttl_secs: u64,
    pub enable_doh: bool,
    pub enable_dot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConfig {
    pub enabled: bool,
    pub blocklists: Vec<BlocklistEntry>,
    pub custom_rules: Vec<String>,
    pub whitelist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlocklistEntry {
    pub name: String,
    pub url: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub theme: String,
    pub start_minimized: bool,
    pub show_notifications: bool,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsConfig {
    pub enabled: bool,
    pub retention_days: u32,
    pub db_path: Option<PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            dns: DnsConfig::default(),
            filter: FilterConfig::default(),
            ui: UiConfig::default(),
            stats: StatsConfig::default(),
        }
    }
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:53".parse().unwrap(),
            upstream_servers: vec![
                "1.1.1.1".into(),
                "1.0.0.1".into(),
                "8.8.8.8".into(),
            ],
            cache_size: 10_000,
            max_ttl_secs: 86400,
            min_ttl_secs: 60,
            enable_doh: true,
            enable_dot: false,
        }
    }
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            blocklists: vec![
                BlocklistEntry {
                    name: "StevenBlack Unified".into(),
                    url: "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts".into(),
                    enabled: true,
                },
                BlocklistEntry {
                    name: "OISD Big".into(),
                    url: "https://big.oisd.nl/domainswild".into(),
                    enabled: true,
                },
            ],
            custom_rules: Vec::new(),
            whitelist: Vec::new(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            start_minimized: false,
            show_notifications: true,
            language: "en".into(),
        }
    }
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 30,
            db_path: None,
        }
    }
}

impl AppConfig {
    /// Get the default config file path.
    pub fn default_path() -> Result<PathBuf, ConfigError> {
        let dirs = ProjectDirs::from("com", "freeix", "FreeIX").ok_or(ConfigError::NoDirs)?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    /// Get the default data directory.
    pub fn data_dir() -> Result<PathBuf, ConfigError> {
        let dirs = ProjectDirs::from("com", "freeix", "FreeIX").ok_or(ConfigError::NoDirs)?;
        Ok(dirs.data_dir().to_path_buf())
    }

    /// Load config from a file, or return default if file doesn't exist.
    pub async fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            info!("config file not found, using defaults");
            return Ok(Self::default());
        }

        let content = tokio::fs::read_to_string(path).await?;
        let config: Self = toml::from_str(&content)?;
        info!(path = %path.display(), "loaded config");
        Ok(config)
    }

    /// Load from the default path.
    pub async fn load_default() -> Result<Self, ConfigError> {
        let path = Self::default_path()?;
        Self::load(&path).await
    }

    /// Save config to a file.
    pub async fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let content = toml::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, content).await?;
        info!(path = %path.display(), "saved config");
        Ok(())
    }

    /// Save to the default path.
    pub async fn save_default(&self) -> Result<(), ConfigError> {
        let path = Self::default_path()?;
        self.save(&path).await
    }
}
