use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs;
use tracing::{debug, info, warn};

#[derive(Debug, Error)]
pub enum BlocklistError {
    #[error("download failed for {url}: {source}")]
    Download { url: String, source: reqwest::Error },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error on line {line}: {reason}")]
    Parse { line: usize, reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlocklistFormat {
    Hosts,
    Adblock,
    DomainList,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlocklistSource {
    pub name: String,
    pub url: String,
    pub format: BlocklistFormat,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlocklistMeta {
    pub source: BlocklistSource,
    pub domain_count: usize,
    pub last_updated: Option<SystemTime>,
    pub local_path: PathBuf,
}

/// Well-known blocklist presets.
impl BlocklistSource {
    pub fn steven_black_unified() -> Self {
        Self {
            name: "StevenBlack Unified".into(),
            url: "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts".into(),
            format: BlocklistFormat::Hosts,
            enabled: true,
        }
    }

    pub fn oisd_big() -> Self {
        Self {
            name: "OISD Big".into(),
            url: "https://big.oisd.nl/domainswild".into(),
            format: BlocklistFormat::DomainList,
            enabled: true,
        }
    }

    pub fn easyprivacy() -> Self {
        Self {
            name: "EasyPrivacy".into(),
            url: "https://v.firebog.net/hosts/Easyprivacy.txt".into(),
            format: BlocklistFormat::DomainList,
            enabled: true,
        }
    }

    pub fn adguard_dns() -> Self {
        Self {
            name: "AdGuard DNS Filter".into(),
            url: "https://adguardteam.github.io/HostlistsRegistry/assets/filter_1.txt".into(),
            format: BlocklistFormat::Adblock,
            enabled: true,
        }
    }
}

pub struct BlocklistManager {
    storage_dir: PathBuf,
    lists: Vec<BlocklistMeta>,
    client: reqwest::Client,
}

/// Returns default blocklist sources for ads, trackers, and malware.
pub fn default_blocklists() -> Vec<BlocklistSource> {
    vec![
        BlocklistSource::steven_black_unified(),
        BlocklistSource::oisd_big(),
        BlocklistSource::easyprivacy(),
        BlocklistSource::adguard_dns(),
    ]
}

impl BlocklistManager {
    pub fn new() -> Self {
        let storage_dir = directories::ProjectDirs::from("com", "freeix", "FreeIX")
            .map(|d| d.cache_dir().join("blocklists"))
            .unwrap_or_else(|| PathBuf::from("./blocklists"));
        Self {
            storage_dir,
            lists: Vec::new(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    pub fn with_storage_dir(storage_dir: PathBuf) -> Self {
        Self {
            storage_dir,
            lists: Vec::new(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    /// Fetch a blocklist URL and parse it into domain rules.
    pub async fn fetch_and_parse(&self, url: &str) -> Result<Vec<String>, BlocklistError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| BlocklistError::Download {
                url: url.to_string(),
                source: e,
            })?
            .text()
            .await
            .map_err(|e| BlocklistError::Download {
                url: url.to_string(),
                source: e,
            })?;

        // Auto-detect format from URL
        let format = if url.contains("hosts") {
            BlocklistFormat::Hosts
        } else if url.contains("adblock") || url.contains("filter") {
            BlocklistFormat::Adblock
        } else {
            BlocklistFormat::DomainList
        };

        Ok(parse_blocklist(&response, &format))
    }

    pub fn add_source(&mut self, source: BlocklistSource) {
        let local_path = self.storage_dir.join(sanitize_filename(&source.name));
        self.lists.push(BlocklistMeta {
            source,
            domain_count: 0,
            last_updated: None,
            local_path,
        });
    }

    pub async fn update_all(&mut self) -> Result<usize, BlocklistError> {
        fs::create_dir_all(&self.storage_dir).await?;

        let mut total = 0;
        for meta in &mut self.lists {
            if !meta.source.enabled {
                continue;
            }
            match download_and_store(&self.client, &meta.source, &meta.local_path).await {
                Ok(count) => {
                    meta.domain_count = count;
                    meta.last_updated = Some(SystemTime::now());
                    total += count;
                    info!(name = %meta.source.name, count, "blocklist updated");
                }
                Err(e) => {
                    warn!(name = %meta.source.name, error = %e, "failed to update blocklist");
                }
            }
        }
        Ok(total)
    }

    /// Parse all downloaded blocklists and return a flat list of domain rules.
    pub async fn get_all_rules(&self) -> Result<Vec<String>, BlocklistError> {
        let mut rules = Vec::new();
        for meta in &self.lists {
            if !meta.source.enabled || !meta.local_path.exists() {
                continue;
            }
            let content = fs::read_to_string(&meta.local_path).await?;
            let parsed = parse_blocklist(&content, &meta.source.format);
            rules.extend(parsed);
        }
        Ok(rules)
    }

    pub fn lists(&self) -> &[BlocklistMeta] {
        &self.lists
    }
}

async fn download_and_store(
    client: &reqwest::Client,
    source: &BlocklistSource,
    local_path: &Path,
) -> Result<usize, BlocklistError> {
    debug!(url = %source.url, "downloading blocklist");

    let response = client
        .get(&source.url)
        .send()
        .await
        .map_err(|e| BlocklistError::Download {
            url: source.url.clone(),
            source: e,
        })?
        .text()
        .await
        .map_err(|e| BlocklistError::Download {
            url: source.url.clone(),
            source: e,
        })?;

    let domains = parse_blocklist(&response, &source.format);
    let count = domains.len();

    let content = domains.join("\n");
    fs::write(local_path, &content).await?;

    Ok(count)
}

/// Parse blocklist content according to format, returning normalized domain rules.
pub fn parse_blocklist(content: &str, format: &BlocklistFormat) -> Vec<String> {
    let mut domains = Vec::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip comments and empty lines
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }

        match format {
            BlocklistFormat::Hosts => {
                // Format: 0.0.0.0 domain or 127.0.0.1 domain
                if let Some(domain) = parse_hosts_line(line) {
                    if is_valid_domain(&domain) {
                        domains.push(domain);
                    }
                }
            }
            BlocklistFormat::DomainList => {
                // One domain per line, optionally with wildcard prefix
                let domain = line.trim_start_matches("*.");
                if is_valid_domain(domain) {
                    if line.starts_with("*.") {
                        domains.push(format!("*.{}", domain));
                    } else {
                        domains.push(domain.to_string());
                    }
                }
            }
            BlocklistFormat::Adblock => {
                // Parse adblock-style: ||domain^ or ||domain^$options
                if let Some(domain) = parse_adblock_line(line) {
                    if is_valid_domain(&domain) {
                        domains.push(domain);
                    }
                }
            }
        }
    }

    domains
}

fn parse_hosts_line(line: &str) -> Option<String> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let ip = parts[0];
    if ip != "0.0.0.0" && ip != "127.0.0.1" {
        return None;
    }

    let domain = parts[1].to_lowercase();
    if domain == "localhost" || domain == "localhost.localdomain" || domain == "broadcasthost" {
        return None;
    }

    Some(domain)
}

fn parse_adblock_line(line: &str) -> Option<String> {
    // Allow rules start with @@
    if line.starts_with("@@") {
        return None; // Skip allows for now (handled separately)
    }

    // Domain rules: ||domain^
    if line.starts_with("||") {
        let rest = &line[2..];
        let domain = rest
            .split('^')
            .next()?
            .split('$')
            .next()?
            .split('/')
            .next()?
            .trim()
            .to_lowercase();
        return Some(domain);
    }

    None
}

fn is_valid_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 253 {
        return false;
    }
    if domain.contains(' ') || domain.starts_with('.') || domain.starts_with('-') {
        return false;
    }
    // Must have at least one dot
    domain.contains('.')
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>()
        + ".txt"
}
