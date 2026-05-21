pub mod linux;
pub mod macos;
pub mod windows;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("command failed: {command} - {reason}")]
    CommandFailed { command: String, reason: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("no active network interface found")]
    NoInterface,
    #[error("unsupported platform")]
    Unsupported,
    #[error("permission denied: {0}")]
    PermissionDenied(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub dns_servers: Vec<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsBackup {
    pub interfaces: Vec<NetworkInterface>,
}

/// Platform-agnostic trait for managing system DNS settings.
#[async_trait]
pub trait DnsManager: Send + Sync {
    async fn get_active_interfaces(&self) -> Result<Vec<NetworkInterface>, PlatformError>;
    async fn backup_dns(&self) -> Result<DnsBackup, PlatformError>;
    async fn set_dns(&self, servers: &[String]) -> Result<(), PlatformError>;
    async fn restore_dns(&self, backup: &DnsBackup) -> Result<(), PlatformError>;
}

/// Factory function: returns the platform-appropriate DnsManager.
pub fn get_dns_manager() -> Result<Box<dyn DnsManager>, PlatformError> {
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WindowsDnsManager::new()))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOsDnsManager::new()))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::LinuxDnsManager::new()))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Err(PlatformError::Unsupported)
    }
}

/// Helper to run a shell command and return stdout.
pub(crate) async fn run_command(program: &str, args: &[&str]) -> Result<String, PlatformError> {
    use tokio::process::Command;

    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| PlatformError::CommandFailed {
            command: format!("{} {}", program, args.join(" ")),
            reason: e.to_string(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(PlatformError::CommandFailed {
            command: format!("{} {}", program, args.join(" ")),
            reason: stderr,
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
