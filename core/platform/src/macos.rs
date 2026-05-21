use async_trait::async_trait;
use crate::{run_command, DnsBackup, DnsManager, NetworkInterface, PlatformError};
use tracing::info;

pub struct MacOsDnsManager;

impl MacOsDnsManager {
    pub fn new() -> Self {
        Self
    }

    async fn get_interfaces_impl(&self) -> Result<Vec<NetworkInterface>, PlatformError> {
        let output = run_command("networksetup", &["-listallnetworkservices"]).await?;
        let mut interfaces = Vec::new();

        for line in output.lines().skip(1) {
            let name = line.trim();
            if name.is_empty() || name.starts_with('*') {
                continue;
            }

            let dns_output = run_command("networksetup", &["-getdnsservers", name]).await?;
            let dns_servers: Vec<String> = if dns_output.contains("There aren't any DNS Servers") {
                Vec::new()
            } else {
                dns_output
                    .lines()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            };

            let info_output =
                run_command("networksetup", &["-getinfo", name]).await.unwrap_or_default();
            let is_active = info_output.contains("IP address:")
                && !info_output.contains("IP address: none");

            if is_active {
                interfaces.push(NetworkInterface {
                    name: name.to_string(),
                    dns_servers,
                    is_active: true,
                });
            }
        }

        Ok(interfaces)
    }
}

#[async_trait]
impl DnsManager for MacOsDnsManager {
    async fn get_active_interfaces(&self) -> Result<Vec<NetworkInterface>, PlatformError> {
        self.get_interfaces_impl().await
    }

    async fn backup_dns(&self) -> Result<DnsBackup, PlatformError> {
        let interfaces = self.get_interfaces_impl().await?;
        info!(count = interfaces.len(), "backed up DNS settings");
        Ok(DnsBackup { interfaces })
    }

    async fn set_dns(&self, servers: &[String]) -> Result<(), PlatformError> {
        let interfaces = self.get_interfaces_impl().await?;
        if interfaces.is_empty() {
            return Err(PlatformError::NoInterface);
        }

        for iface in &interfaces {
            let mut args: Vec<&str> = vec!["-setdnsservers", &iface.name];
            let server_refs: Vec<&str> = servers.iter().map(|s| s.as_str()).collect();
            args.extend(server_refs);
            run_command("networksetup", &args).await?;
        }

        info!("DNS set to {:?} on all active interfaces", servers);
        Ok(())
    }

    async fn restore_dns(&self, backup: &DnsBackup) -> Result<(), PlatformError> {
        for iface in &backup.interfaces {
            if iface.dns_servers.is_empty() {
                run_command("networksetup", &["-setdnsservers", &iface.name, "empty"]).await?;
            } else {
                let mut args: Vec<&str> = vec!["-setdnsservers", &iface.name];
                let refs: Vec<&str> = iface.dns_servers.iter().map(|s| s.as_str()).collect();
                args.extend(refs);
                run_command("networksetup", &args).await?;
            }
        }

        info!("DNS settings restored from backup");
        Ok(())
    }
}
