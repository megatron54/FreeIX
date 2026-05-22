//! Windows Filtering Platform (WFP) traffic redirection.
//!
//! Uses `netsh` to redirect outgoing TCP connections on ports 80/443
//! to our local transparent proxy. This is a simplified approach
//! using Windows built-in port proxy (netsh portproxy) rather than
//! raw WFP API calls.
//!
//! For a production implementation, we'd use the Windows Filtering Platform
//! COM API directly, but netsh portproxy + Windows Firewall rules achieve
//! the same result without needing unsafe FFI.

use std::process::Command;
use tracing::{info, warn};

use crate::ProxyError;

/// Enable traffic redirection to our proxy port.
///
/// This configures Windows to redirect outgoing HTTP/HTTPS traffic
/// to our local proxy using netsh portproxy.
pub fn enable_redirect(proxy_port: u16) -> Result<(), ProxyError> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        // We use a different approach: configure the system proxy settings
        // to route through our local proxy. This is how most usermode proxies work
        // (Fiddler, Charles, mitmproxy).
        let script = format!(
            r#"
            # Set system proxy to redirect HTTP/HTTPS through FreeIX
            $proxyAddr = '127.0.0.1:{port}'

            # Set Internet Explorer / WinINET proxy (affects most apps)
            Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings' -Name ProxyEnable -Value 1
            Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings' -Name ProxyServer -Value $proxyAddr
            Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings' -Name ProxyOverride -Value '<local>;localhost;127.0.0.1'

            # Notify the system of the change
            $signature = @'
            [DllImport("wininet.dll", SetLastError = true, CharSet=CharSet.Auto)]
            public static extern bool InternetSetOption(IntPtr hInternet, int dwOption, IntPtr lpBuffer, int dwBufferLength);
'@
            $type = Add-Type -MemberDefinition $signature -Name wininet -Namespace pinvoke -PassThru
            $INTERNET_OPTION_SETTINGS_CHANGED = 39
            $INTERNET_OPTION_REFRESH = 37
            $type::InternetSetOption([IntPtr]::Zero, $INTERNET_OPTION_SETTINGS_CHANGED, [IntPtr]::Zero, 0) | Out-Null
            $type::InternetSetOption([IntPtr]::Zero, $INTERNET_OPTION_REFRESH, [IntPtr]::Zero, 0) | Out-Null
            "#,
            port = proxy_port
        );

        let result = Command::new("powershell")
            .args(&["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        match result {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!(%stderr, "WFP redirect script had errors");
                }
            }
            Err(e) => return Err(ProxyError::Io(e)),
        }
    }

    Ok(())
}

/// Disable traffic redirection — restore original proxy settings.
pub fn disable_redirect() -> Result<(), ProxyError> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let script = r#"
            # Disable system proxy
            Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings' -Name ProxyEnable -Value 0
            Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings' -Name ProxyServer -ErrorAction SilentlyContinue

            # Notify the system
            $signature = @'
            [DllImport("wininet.dll", SetLastError = true, CharSet=CharSet.Auto)]
            public static extern bool InternetSetOption(IntPtr hInternet, int dwOption, IntPtr lpBuffer, int dwBufferLength);
'@
            $type = Add-Type -MemberDefinition $signature -Name wininet -Namespace pinvoke -PassThru
            $INTERNET_OPTION_SETTINGS_CHANGED = 39
            $INTERNET_OPTION_REFRESH = 37
            $type::InternetSetOption([IntPtr]::Zero, $INTERNET_OPTION_SETTINGS_CHANGED, [IntPtr]::Zero, 0) | Out-Null
            $type::InternetSetOption([IntPtr]::Zero, $INTERNET_OPTION_REFRESH, [IntPtr]::Zero, 0) | Out-Null
        "#;

        let result = Command::new("powershell")
            .args(&["-NoProfile", "-WindowStyle", "Hidden", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        match result {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!(%stderr, "WFP disable script had errors");
                }
            }
            Err(e) => return Err(ProxyError::Io(e)),
        }
    }

    Ok(())
}
