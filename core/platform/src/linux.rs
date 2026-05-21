use async_trait::async_trait;
use crate::{run_command, DnsBackup, DnsManager, NetworkInterface, PlatformError};
use tracing::info;

pub struct LinuxDnsManager {
    method: LinuxDnsMethod,
}

#[derive(Debug, Clone)]
enum LinuxDnsMethod {
    SystemdResolved,
    NetworkManager,
    ResolvConf,
}

impl LinuxDnsManager {
    pub fn new() -> Self {
        Self {
            method: LinuxDnsMethod::SystemdResolved,
        }
    }

    pub async fn detect_method(&mut self) {
        if run_command("resolvectl", &["status"]).await.is_ok() {
            self.method = LinuxDnsMethod::SystemdResolved;
            return;
        }
        if run_command("nmcli", &["general", "status"]).await.is_ok() {
            self.method = LinuxDnsMethod::NetworkManager;
            return;
        }
        self.method = LinuxDnsMethod::ResolvConf;
    }

    async fn get_interfaces_impl(&self) -> Result<Vec<NetworkInterface>, PlatformError> {
        match &self.method {
            LinuxDnsMethod::SystemdResolved => self.get_interfaces_systemd().await,
            LinuxDnsMethod::NetworkManager => self.get_interfaces_nm().await,
            LinuxDnsMethod::ResolvConf => self.get_interfaces_resolv().await,
        }
    }

    async fn get_interfaces_systemd(&self) -> Result<Vec<NetworkInterface>, PlatformError> {
        let output = run_command("resolvectl", &["status"]).await?;
        let mut interfaces = Vec::new();
        let mut current_name: Option<String> = None;
        let mut current_dns: Vec<String> = Vec::new();

        for line in output.lines() {
            let line = line.trim();
            if line.starts_with("Link") {
                if let Some(name) = current_name.take() {
                    interfaces.push(NetworkInterface {
                        name,
                        dns_servers: std::mem::take(&mut current_dns),
                        is_active: true,
                    });
                }
                if let Some(paren_start) = line.find('(') {
                    if let Some(paren_end) = line.find(')') {
                        current_name = Some(line[paren_start + 1..paren_end].to_string());
                    }
                }
            } else if line.starts_with("DNS Servers:") || line.starts_with("Current DNS Server:") {
                if let Some(addr) = line.split(':').nth(1) {
                    let addr = addr.trim().to_string();
                    if !addr.is_empty() {
                        current_dns.push(addr);
                    }
                }
            }
        }
        if let Some(name) = current_name {
            interfaces.push(NetworkInterface {
                name,
                dns_servers: current_dns,
                is_active: true,
            });
        }

        Ok(interfaces)
    }

    async fn get_interfaces_nm(&self) -> Result<Vec<NetworkInterface>, PlatformError> {
        let output = run_command(
            "nmcli",
            &["-t", "-f", "NAME,DEVICE,TYPE,STATE", "connection", "show", "--active"],
        )
        .await?;

        let mut interfaces = Vec::new();
        for line in output.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() < 4 {
                continue;
            }
            let name = parts[0].to_string();
            let device = parts[1];

            let dns_output = run_command(
                "nmcli",
                &["-t", "-f", "IP4.DNS", "connection", "show", &name],
            )
            .await
            .unwrap_or_default();

            let dns_servers: Vec<String> = dns_output
                .lines()
                .filter_map(|l| l.split(':').nth(1))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            interfaces.push(NetworkInterface {
                name: device.to_string(),
                dns_servers,
                is_active: true,
            });
        }

        Ok(interfaces)
    }

    async fn get_interfaces_resolv(&self) -> Result<Vec<NetworkInterface>, PlatformError> {
        let content = tokio::fs::read_to_string("/etc/resolv.conf")
            .await
            .map_err(PlatformError::Io)?;

        let dns_servers: Vec<String> = content
            .lines()
            .filter(|l| l.starts_with("nameserver"))
            .filter_map(|l| l.split_whitespace().nth(1))
            .map(|s| s.to_string())
            .collect();

        Ok(vec![NetworkInterface {
            name: "resolv.conf".to_string(),
            dns_servers,
            is_active: true,
        }])
    }
}

#[async_trait]
impl DnsManager for LinuxDnsManager {
    async fn get_active_interfaces(&self) -> Result<Vec<NetworkInterface>, PlatformError> {
        self.get_interfaces_impl().await
    }

    async fn backup_dns(&self) -> Result<DnsBackup, PlatformError> {
        let interfaces = self.get_interfaces_impl().await?;
        info!(count = interfaces.len(), "backed up DNS settings");
        Ok(DnsBackup { interfaces })
    }

    async fn set_dns(&self, servers: &[String]) -> Result<(), PlatformError> {
        match &self.method {
            LinuxDnsMethod::SystemdResolved => {
                let interfaces = self.get_interfaces_systemd().await?;
                for iface in &interfaces {
                    let mut args = vec!["dns".to_string(), iface.name.clone()];
                    args.extend(servers.iter().cloned());
                    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                    run_command("resolvectl", &refs).await?;
                }
            }
            LinuxDnsMethod::NetworkManager => {
                let interfaces = self.get_interfaces_nm().await?;
                let dns_str = servers.join(" ");
                for iface in &interfaces {
                    run_command(
                        "nmcli",
                        &[
                            "connection", "modify", &iface.name,
                            "ipv4.dns", &dns_str,
                            "ipv4.ignore-auto-dns", "yes",
                        ],
                    )
                    .await?;
                    run_command("nmcli", &["connection", "up", &iface.name]).await?;
                }
            }
            LinuxDnsMethod::ResolvConf => {
                let content: String = servers
                    .iter()
                    .map(|s| format!("nameserver {}", s))
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n";
                tokio::fs::write("/etc/resolv.conf", content)
                    .await
                    .map_err(PlatformError::Io)?;
            }
        }

        info!("DNS set to {:?}", servers);
        Ok(())
    }

    async fn restore_dns(&self, backup: &DnsBackup) -> Result<(), PlatformError> {
        match &self.method {
            LinuxDnsMethod::SystemdResolved => {
                for iface in &backup.interfaces {
                    if iface.dns_servers.is_empty() {
                        run_command("resolvectl", &["revert", &iface.name]).await?;
                    } else {
                        let mut args = vec!["dns".to_string(), iface.name.clone()];
                        args.extend(iface.dns_servers.clone());
                        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                        run_command("resolvectl", &refs).await?;
                    }
                }
            }
            LinuxDnsMethod::NetworkManager => {
                for iface in &backup.interfaces {
                    if iface.dns_servers.is_empty() {
                        run_command(
                            "nmcli",
                            &[
                                "connection", "modify", &iface.name,
                                "ipv4.dns", "",
                                "ipv4.ignore-auto-dns", "no",
                            ],
                        )
                        .await?;
                    } else {
                        let dns_str = iface.dns_servers.join(" ");
                        run_command(
                            "nmcli",
                            &["connection", "modify", &iface.name, "ipv4.dns", &dns_str],
                        )
                        .await?;
                    }
                    run_command("nmcli", &["connection", "up", &iface.name]).await?;
                }
            }
            LinuxDnsMethod::ResolvConf => {
                let servers = backup.interfaces.first()
                    .map(|i| &i.dns_servers)
                    .cloned()
                    .unwrap_or_default();
                let content: String = servers
                    .iter()
                    .map(|s| format!("nameserver {}", s))
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n";
                tokio::fs::write("/etc/resolv.conf", content)
                    .await
                    .map_err(PlatformError::Io)?;
            }
        }

        info!("DNS settings restored from backup");
        Ok(())
    }
}
