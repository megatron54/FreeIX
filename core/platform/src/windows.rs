use async_trait::async_trait;
use crate::{run_command, DnsBackup, DnsManager, NetworkInterface, PlatformError};
use tracing::{debug, info};

pub struct WindowsDnsManager;

impl WindowsDnsManager {
    pub fn new() -> Self {
        Self
    }

    async fn get_interfaces_ps(&self) -> Result<Vec<NetworkInterface>, PlatformError> {
        let output = run_command(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Get-NetAdapter | Where-Object { $_.Status -eq 'Up' } | ForEach-Object { \
                    $dns = (Get-DnsClientServerAddress -InterfaceIndex $_.ifIndex -AddressFamily IPv4).ServerAddresses -join ','; \
                    \"$($_.Name)|$dns\" \
                }",
            ],
        )
        .await?;

        let mut interfaces = Vec::new();
        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.splitn(2, '|').collect();
            if parts.len() < 2 {
                continue;
            }
            let name = parts[0].to_string();
            let dns_servers: Vec<String> = parts[1]
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            interfaces.push(NetworkInterface {
                name,
                dns_servers,
                is_active: true,
            });
        }

        Ok(interfaces)
    }
}

#[async_trait]
impl DnsManager for WindowsDnsManager {
    async fn get_active_interfaces(&self) -> Result<Vec<NetworkInterface>, PlatformError> {
        self.get_interfaces_ps().await
    }

    async fn backup_dns(&self) -> Result<DnsBackup, PlatformError> {
        let interfaces = self.get_interfaces_ps().await?;
        info!(count = interfaces.len(), "backed up DNS settings");
        Ok(DnsBackup { interfaces })
    }

    async fn set_dns(&self, servers: &[String]) -> Result<(), PlatformError> {
        let interfaces = self.get_interfaces_ps().await?;
        if interfaces.is_empty() {
            return Err(PlatformError::NoInterface);
        }

        for iface in &interfaces {
            debug!(interface = %iface.name, "setting DNS");
            let cmd = format!(
                "Set-DnsClientServerAddress -InterfaceAlias '{}' -ServerAddresses ('{}')",
                iface.name,
                servers.join("','")
            );
            run_command("powershell", &["-NoProfile", "-Command", &cmd]).await?;
        }

        info!("DNS set to {:?} on all active interfaces", servers);
        Ok(())
    }

    async fn restore_dns(&self, backup: &DnsBackup) -> Result<(), PlatformError> {
        for iface in &backup.interfaces {
            if iface.dns_servers.is_empty() {
                let cmd = format!(
                    "Set-DnsClientServerAddress -InterfaceAlias '{}' -ResetServerAddresses",
                    iface.name
                );
                run_command("powershell", &["-NoProfile", "-Command", &cmd]).await?;
            } else {
                let cmd = format!(
                    "Set-DnsClientServerAddress -InterfaceAlias '{}' -ServerAddresses ('{}')",
                    iface.name,
                    iface.dns_servers.join("','")
                );
                run_command("powershell", &["-NoProfile", "-Command", &cmd]).await?;
            }
        }

        info!("DNS settings restored from backup");
        Ok(())
    }
}
