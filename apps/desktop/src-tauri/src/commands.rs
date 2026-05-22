use crate::state::{AppConfig, AppState, DnsProvider, QueryEvent, QueryStatus};
use std::process::Command;
use freeix_blocklists::BlocklistManager;
use freeix_ca::RootCa;
use freeix_dns_engine::upstream::{UpstreamConfig, UpstreamEntry, UpstreamProtocol, UpstreamProvider};
use freeix_dns_engine::{DnsServer, DnsServerConfig, QueryCallback, QueryInfo};
use freeix_filtering_engine::{FilterEngine, Rule, RuleType2};
use freeix_platform::{get_dns_manager, DnsBackup};
use freeix_proxy_engine::ProxyEngine;
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Serialize)]
pub struct ProtectionStatus {
    pub enabled: bool,
    pub dns_provider: String,
    pub uptime_seconds: u64,
    pub total_rules: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsResponse {
    pub total_queries: u64,
    pub blocked_queries: u64,
    pub cache_hits: u64,
    pub block_percentage: f64,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopBlocked {
    pub domain: String,
    pub count: u64,
}

/// Check if port 53 is available for binding
fn check_port_available(addr: &str, port: u16) -> Result<(), String> {
    let bind_addr: std::net::SocketAddr = format!("{}:{}", addr, port).parse()
        .map_err(|e| format!("invalid address: {}", e))?;

    match std::net::UdpSocket::bind(bind_addr) {
        Ok(_) => Ok(()), // Socket bound successfully, port is free
        Err(e) => Err(format!("Port {} is already in use: {}. Another DNS service (ICS, Docker, or another DNS proxy) may be running.", port, e)),
    }
}

#[tauri::command]
pub async fn is_setup_complete(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    Ok(state.config.read().setup_complete)
}

#[tauri::command]
pub async fn complete_setup(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut config = state.config.write();
    config.setup_complete = true;
    AppState::save_config(&config);
    Ok(())
}

#[tauri::command]
pub async fn set_system_dns_to_local() -> Result<(), String> {
    set_system_dns_elevated("127.0.0.1");
    Ok(())
}

#[tauri::command]
pub async fn install_ca_certificate(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.root_ca.install_to_system().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn is_ca_installed(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    Ok(state.root_ca.is_installed())
}

/// The proxy listens on this port. System proxy is configured to use it.
const PROXY_PORT: u16 = 8853;

/// Shared blocklist update logic used by both the command and auto-update task.
pub async fn update_blocklists_inner(state: &AppState) -> Result<usize, String> {
    let manager = BlocklistManager::new();
    let default_lists = freeix_blocklists::default_blocklists();

    let mut total_rules = 0;
    for list in &default_lists {
        match manager.fetch_and_parse(&list.url).await {
            Ok(domains) => {
                let count = domains.len();
                for domain in domains {
                    state.filter_engine.add_rule(Rule {
                        pattern: domain,
                        rule_type: RuleType2::Block,
                    });
                }
                total_rules += count;
                tracing::info!(list = %list.name, count, "loaded blocklist");
            }
            Err(e) => {
                tracing::warn!(list = %list.name, error = %e, "failed to load blocklist");
            }
        }
    }

    tracing::info!("Loaded {} blocking rules from {} lists", total_rules, default_lists.len());
    Ok(total_rules)
}

type AppStateHandle = Arc<AppState>;

#[tauri::command]
pub async fn toggle_protection(
    enable: bool,
    state: State<'_, AppStateHandle>,
    app: AppHandle,
) -> Result<bool, String> {
    if enable {
        // Start DNS server and redirect system DNS
        let config = state.config.read().clone();
        let provider = state
            .dns_providers
            .iter()
            .find(|p| p.id == config.dns_provider_id)
            .cloned()
            .unwrap_or_else(|| state.dns_providers[0].clone());

        // Build upstream config from selected provider
        let upstream_entries = vec![
            UpstreamEntry {
                address: format!("{}:53", provider.primary).parse().map_err(|e| format!("{}", e))?,
                protocol: UpstreamProtocol::Plain,
                hostname: None,
                doh_url: None,
            },
            UpstreamEntry {
                address: format!("{}:53", provider.secondary).parse().map_err(|e| format!("{}", e))?,
                protocol: UpstreamProtocol::Plain,
                hostname: None,
                doh_url: None,
            },
        ];

        let upstream_config = UpstreamConfig {
            providers: vec![UpstreamProvider::Custom(upstream_entries)],
            timeout: std::time::Duration::from_secs(5),
            retries: 2,
        };

        let server_config = DnsServerConfig {
            listen_addr: format!("{}:{}", config.listen_address, config.port)
                .parse()
                .map_err(|e| format!("invalid listen address: {}", e))?,
            upstream: upstream_config,
            cache_size: config.cache_size as usize,
            ..Default::default()
        };

        // Build query callback for real-time events
        let state_ref = state.inner().clone();
        let app_handle = app.clone();
        let callback: QueryCallback = Arc::new(move |info: QueryInfo| {
            // Increment stats
            state_ref.stats.total_queries.fetch_add(1, Ordering::Relaxed);
            if info.blocked {
                state_ref.stats.blocked_queries.fetch_add(1, Ordering::Relaxed);
            }
            if info.cached {
                state_ref.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
            }

            // Push to query log
            let status = if info.blocked {
                QueryStatus::Blocked
            } else if info.cached {
                QueryStatus::Cached
            } else {
                QueryStatus::Allowed
            };
            let event = QueryEvent {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                domain: info.domain.clone(),
                query_type: info.query_type.clone(),
                status,
                response_time_ms: info.response_time_ms,
                upstream: String::new(),
                rule: info.rule.clone(),
            };
            state_ref.push_log(event.clone());

            // Emit Tauri event
            let _ = app_handle.emit("query-event", &event);
        });

        let dns_server = Arc::new(DnsServer::new(server_config, state.filter_engine.clone(), Some(callback)));

        // Check if port is available (warn but still try — on Windows, 127.0.0.1:53 can coexist with 0.0.0.0:53 from ICS)
        if let Err(e) = check_port_available(&config.listen_address, config.port) {
            tracing::warn!("{}", e);
        }

        dns_server.start().await.map_err(|e| format!("Failed to start DNS server: {}", e))?;

        // Backup current DNS and redirect to our local proxy (elevated)
        match get_dns_manager() {
            Ok(manager) => {
                match manager.backup_dns().await {
                    Ok(backup) => {
                        *state.dns_backup.write() = Some(backup);
                    }
                    Err(e) => tracing::warn!("Failed to backup DNS: {}", e),
                }
            }
            Err(e) => tracing::warn!("Platform DNS manager unavailable: {}", e),
        }
        set_system_dns_elevated(&config.listen_address);

        *state.dns_server.write() = Some(dns_server);
        *state.stats.started_at.write() = Some(Instant::now());
        *state.protection_enabled.write() = true;

        // Load blocklists immediately
        let state_for_bl = state.inner().clone();
        tokio::spawn(async move {
            match update_blocklists_inner(&state_for_bl).await {
                Ok(count) => tracing::info!(count, "Blocklists loaded"),
                Err(e) => tracing::warn!("Failed to load blocklists: {}", e),
            }
        });

        tracing::info!(provider = %provider.name, "Protection enabled");
    } else {
        // Stop DNS server and restore system DNS
        if let Some(server) = state.dns_server.write().take() {
            let _ = server.stop();
        }

        // Restore DNS
        let backup_clone = state.dns_backup.read().clone();
        if let Some(backup) = backup_clone {
            if let Ok(manager) = get_dns_manager() {
                if let Err(e) = manager.restore_dns(&backup).await {
                    tracing::warn!("Failed to restore DNS: {}", e);
                }
            }
        }

        *state.protection_enabled.write() = false;
        tracing::info!("Protection disabled");
    }

    Ok(enable)
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppStateHandle>) -> Result<ProtectionStatus, String> {
    let enabled = *state.protection_enabled.read();
    let config = state.config.read().clone();
    Ok(ProtectionStatus {
        enabled,
        dns_provider: config.dns_provider_id,
        uptime_seconds: state.stats.uptime_seconds(),
        total_rules: state.filter_engine.rule_count(),
    })
}

#[tauri::command]
pub async fn get_stats(state: State<'_, AppStateHandle>) -> Result<StatsResponse, String> {
    let total = state.stats.total_queries.load(std::sync::atomic::Ordering::Relaxed);
    let blocked = state.stats.blocked_queries.load(std::sync::atomic::Ordering::Relaxed);
    let cache_hits = state.stats.cache_hits.load(std::sync::atomic::Ordering::Relaxed);
    let block_percentage = if total > 0 {
        (blocked as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    Ok(StatsResponse {
        total_queries: total,
        blocked_queries: blocked,
        cache_hits,
        block_percentage,
        uptime_seconds: state.stats.uptime_seconds(),
    })
}

#[tauri::command]
pub async fn get_config(state: State<'_, AppStateHandle>) -> Result<AppConfig, String> {
    Ok(state.config.read().clone())
}

#[tauri::command]
pub async fn update_config(
    config: AppConfig,
    state: State<'_, AppStateHandle>,
) -> Result<(), String> {
    *state.config.write() = config.clone();
    AppState::save_config(&config);
    tracing::info!("Configuration updated");
    Ok(())
}

#[tauri::command]
pub async fn add_whitelist(
    domain: String,
    state: State<'_, AppStateHandle>,
) -> Result<(), String> {
    let domain = domain.to_lowercase();
    state.whitelist.write().insert(domain.clone());
    state.filter_engine.add_rule(Rule {
        pattern: domain.clone(),
        rule_type: RuleType2::Allow,
    });
    tracing::info!("Added to whitelist: {}", domain);
    Ok(())
}

#[tauri::command]
pub async fn remove_whitelist(
    domain: String,
    state: State<'_, AppStateHandle>,
) -> Result<(), String> {
    let domain = domain.to_lowercase();
    state.whitelist.write().remove(&domain);
    state.filter_engine.remove_rule(&domain);
    tracing::info!("Removed from whitelist: {}", domain);
    Ok(())
}

#[tauri::command]
pub async fn add_blacklist(
    domain: String,
    state: State<'_, AppStateHandle>,
) -> Result<(), String> {
    let domain = domain.to_lowercase();
    state.blacklist.write().insert(domain.clone());
    state.filter_engine.add_rule(Rule {
        pattern: domain.clone(),
        rule_type: RuleType2::Block,
    });
    tracing::info!("Added to blacklist: {}", domain);
    Ok(())
}

#[tauri::command]
pub async fn remove_blacklist(
    domain: String,
    state: State<'_, AppStateHandle>,
) -> Result<(), String> {
    let domain = domain.to_lowercase();
    state.blacklist.write().remove(&domain);
    state.filter_engine.remove_rule(&domain);
    tracing::info!("Removed from blacklist: {}", domain);
    Ok(())
}

#[tauri::command]
pub async fn get_whitelist(state: State<'_, AppStateHandle>) -> Result<Vec<String>, String> {
    Ok(state.whitelist.read().iter().cloned().collect())
}

#[tauri::command]
pub async fn get_blacklist(state: State<'_, AppStateHandle>) -> Result<Vec<String>, String> {
    Ok(state.blacklist.read().iter().cloned().collect())
}

#[tauri::command]
pub async fn get_dns_providers(
    state: State<'_, AppStateHandle>,
) -> Result<Vec<DnsProvider>, String> {
    Ok(state.dns_providers.clone())
}

#[tauri::command]
pub async fn set_dns_provider(
    id: String,
    state: State<'_, AppStateHandle>,
) -> Result<(), String> {
    let exists = state.dns_providers.iter().any(|p| p.id == id);
    if !exists {
        return Err(format!("DNS provider '{}' not found", id));
    }
    state.config.write().dns_provider_id = id.clone();
    tracing::info!("DNS provider set to: {}", id);
    Ok(())
}

#[tauri::command]
pub async fn get_logs(
    limit: Option<usize>,
    state: State<'_, AppStateHandle>,
) -> Result<Vec<QueryEvent>, String> {
    let logs = state.query_log.read();
    let limit = limit.unwrap_or(200);
    let result: Vec<QueryEvent> = logs.iter().rev().take(limit).cloned().collect();
    Ok(result)
}

#[tauri::command]
pub async fn get_top_blocked(
    state: State<'_, AppStateHandle>,
) -> Result<Vec<TopBlocked>, String> {
    let logs = state.query_log.read();
    let mut counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for log in logs.iter() {
        if log.status == QueryStatus::Blocked {
            *counts.entry(log.domain.clone()).or_default() += 1;
        }
    }
    let mut top: Vec<TopBlocked> = counts
        .into_iter()
        .map(|(domain, count)| TopBlocked { domain, count })
        .collect();
    top.sort_by(|a, b| b.count.cmp(&a.count));
    top.truncate(20);
    Ok(top)
}

#[tauri::command]
pub async fn update_blocklists(state: State<'_, AppStateHandle>) -> Result<String, String> {
    tracing::info!("Updating blocklists...");
    let count = update_blocklists_inner(&state).await?;
    let default_lists = freeix_blocklists::default_blocklists();
    let msg = format!("Loaded {} blocking rules from {} lists", count, default_lists.len());
    Ok(msg)
}

#[tauri::command]
pub async fn set_autostart(enable: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_str = exe_path.to_string_lossy().to_string();
        if enable {
            Command::new("reg")
                .args(&[
                    "add",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v", "FreeIX",
                    "/t", "REG_SZ",
                    "/d", &exe_str,
                    "/f",
                ])
                .output()
                .map_err(|e| e.to_string())?;
        } else {
            Command::new("reg")
                .args(&[
                    "delete",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v", "FreeIX",
                    "/f",
                ])
                .output()
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn get_autostart() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("reg")
            .args(&[
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v", "FreeIX",
            ])
            .output()
            .map_err(|e| e.to_string())?;
        return Ok(output.status.success());
    }
    #[cfg(not(target_os = "windows"))]
    Ok(false)
}

/// Auto-start protection on app launch (called from setup, not a Tauri command)
pub async fn auto_start_protection(state: Arc<AppState>, app: tauri::AppHandle) -> Result<(), String> {
    let config = state.config.read().clone();
    let provider = state
        .dns_providers
        .iter()
        .find(|p| p.id == config.dns_provider_id)
        .cloned()
        .unwrap_or_else(|| state.dns_providers[0].clone());

    let upstream_entries = vec![
        UpstreamEntry {
            address: format!("{}:53", provider.primary).parse().map_err(|e| format!("{}", e))?,
            protocol: UpstreamProtocol::Plain,
            hostname: None,
            doh_url: None,
        },
        UpstreamEntry {
            address: format!("{}:53", provider.secondary).parse().map_err(|e| format!("{}", e))?,
            protocol: UpstreamProtocol::Plain,
            hostname: None,
            doh_url: None,
        },
    ];

    let upstream_config = UpstreamConfig {
        providers: vec![UpstreamProvider::Custom(upstream_entries)],
        timeout: std::time::Duration::from_secs(5),
        retries: 2,
    };

    let server_config = DnsServerConfig {
        listen_addr: format!("{}:{}", config.listen_address, config.port)
            .parse()
            .map_err(|e| format!("invalid listen address: {}", e))?,
        upstream: upstream_config,
        cache_size: config.cache_size as usize,
        ..Default::default()
    };

    // Build query callback
    let state_ref = state.clone();
    let app_handle = app.clone();
    let callback: QueryCallback = Arc::new(move |info: QueryInfo| {
        state_ref.stats.total_queries.fetch_add(1, Ordering::Relaxed);
        if info.blocked {
            state_ref.stats.blocked_queries.fetch_add(1, Ordering::Relaxed);
        }
        if info.cached {
            state_ref.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
        }
        let status = if info.blocked {
            QueryStatus::Blocked
        } else if info.cached {
            QueryStatus::Cached
        } else {
            QueryStatus::Allowed
        };
        let event = QueryEvent {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            domain: info.domain.clone(),
            query_type: info.query_type.clone(),
            status,
            response_time_ms: info.response_time_ms,
            upstream: String::new(),
            rule: info.rule.clone(),
        };
        state_ref.push_log(event.clone());
        let _ = app_handle.emit("query-event", &event);
    });

    let dns_server = Arc::new(DnsServer::new(server_config, state.filter_engine.clone(), Some(callback)));

    // Check if port is available (warn but still try — on Windows, 127.0.0.1:53 can coexist with 0.0.0.0:53 from ICS)
    if let Err(e) = check_port_available(&config.listen_address, config.port) {
        tracing::warn!("{}", e);
    }

    dns_server.start().await.map_err(|e| format!("Failed to start DNS server: {}", e))?;

    // Set system DNS with elevation
    set_system_dns_elevated(&config.listen_address);

    *state.dns_server.write() = Some(dns_server);
    *state.stats.started_at.write() = Some(Instant::now());
    *state.protection_enabled.write() = true;

    // Load blocklists immediately so blocking works from the start
    let state_for_blocklists = state.clone();
    tokio::spawn(async move {
        match update_blocklists_inner(&state_for_blocklists).await {
            Ok(count) => tracing::info!(count, "Blocklists loaded on startup"),
            Err(e) => tracing::warn!("Failed to load blocklists on startup: {}", e),
        }
    });

    // HTTPS proxy disabled — MITM doesn't work for cert-pinned domains (Google, Facebook).
    // DNS blocking handles domain-level ads. YouTube video ads need browser extension approach.
    // TODO: Re-enable proxy when we have a working non-MITM URL filtering strategy.
    //
    // let proxy = state.proxy_engine.clone();
    // tokio::spawn(async move {
    //     if let Err(e) = proxy.start(PROXY_PORT).await {
    //         tracing::warn!("Proxy engine failed to start: {}", e);
    //     } else {
    //         if let Err(e) = proxy.enable_wfp_redirect(PROXY_PORT) {
    //             tracing::warn!("Failed to enable proxy redirect: {}", e);
    //         }
    //     }
    // });

    tracing::info!(provider = %provider.name, "Auto-start protection enabled");
    Ok(())
}

/// Set system DNS using elevated PowerShell (triggers UAC if needed)
fn set_system_dns_elevated(address: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        // Build a script that sets DNS on all active adapters
        let script = format!(
            "Get-NetAdapter | Where-Object {{ $_.Status -eq 'Up' }} | ForEach-Object {{ Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ServerAddresses ('{}') }}",
            address
        );
        let result = Command::new("powershell")
            .args(&["-NoProfile", "-WindowStyle", "Hidden", "-Command", &format!(
                "Start-Process powershell -ArgumentList '-NoProfile -WindowStyle Hidden -Command {}' -Verb RunAs -Wait -WindowStyle Hidden",
                script.replace("'", "''")
            )])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        match result {
            Ok(output) => {
                if output.status.success() {
                    tracing::info!("System DNS set to {} via elevated PowerShell", address);
                } else {
                    tracing::warn!("Elevated DNS change may have been cancelled by user");
                }
            }
            Err(e) => tracing::warn!("Failed to launch elevated DNS change: {}", e),
        }
    }
}

