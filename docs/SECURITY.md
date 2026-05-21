# FreeIX Security Model

## Overview

FreeIX is designed with a security-first, local-first philosophy. This document describes the security considerations, threat model, and mitigations built into the application.

## Core Principles

1. **Minimal privilege** - Request only the permissions absolutely required
2. **Local-first** - All data stays on your machine
3. **No telemetry** - Zero data collection, no analytics, no phone-home
4. **Transparent** - Open source, auditable, reproducible builds

## Privilege Escalation

### Why Elevated Privileges Are Needed

FreeIX needs to:
- Bind to UDP/TCP port 53 (privileged port on most OSes)
- Modify system DNS resolver configuration

### Platform-Specific Approach

#### Windows
- FreeIX installs a Windows Service (`FreeIXService`) running as `LOCAL SERVICE`
- The service binds port 53 and runs the DNS proxy
- The desktop UI communicates with the service via named pipes
- UAC prompt is shown only during initial setup/installation

#### macOS
- A privileged helper tool is installed via `SMJobBless`
- The helper binds port 53 and manages DNS configuration
- Signed with the application's code signature
- User authorizes via system authentication dialog once

#### Linux
- Systemd service with `AmbientCapabilities=CAP_NET_BIND_SERVICE`
- No full root access required after initial setup
- Alternatively: `setcap cap_net_bind_service=+ep` on the binary
- DNS configuration via `systemd-resolved` or `/etc/resolv.conf`

### Privilege Separation

```
┌─────────────────────┐     Named Pipe / Socket     ┌──────────────────┐
│   Desktop UI        │◄───────────────────────────►│  DNS Service     │
│   (user privilege)  │     (restricted IPC)        │  (minimal priv)  │
└─────────────────────┘                             └──────────────────┘
```

The desktop UI never runs with elevated privileges. All privileged operations are isolated in the service component.

## No Telemetry Guarantee

FreeIX makes **zero** network connections except:
1. DNS query forwarding to user-configured upstream resolvers
2. Blocklist downloads from user-configured URLs (HTTPS only)
3. Optional: update checks to GitHub releases API (opt-in, disabled by default)

There is no analytics, crash reporting, or usage tracking of any kind. This is verifiable by auditing the source code and monitoring network traffic.

## Network Security

### Upstream DNS
- Supports DNS-over-HTTPS (DoH) and DNS-over-TLS (DoT) for upstream queries
- Validates TLS certificates
- Configurable upstream servers (defaults to privacy-respecting resolvers)

### Blocklist Downloads
- HTTPS only (no plaintext HTTP sources)
- TLS certificate validation enforced
- Integrity verification via ETags and content hashing
- Timeout and size limits to prevent resource exhaustion

## Local Data Security

### Configuration
- Stored in platform-appropriate config directory
- File permissions restricted to the owning user (0600 on Unix)
- No sensitive data stored (no passwords, no API keys)

### Query Logs
- Stored locally only, never transmitted
- Configurable retention period (default: 24 hours)
- Can be completely disabled
- Cleared on demand via UI or CLI

### DNS Backup/Restore Safety
- Original DNS settings backed up before modification
- Backup stored with timestamp in config directory
- Automatic restore on:
  - Clean uninstall
  - Service crash (watchdog-based)
  - Manual restore command
- Fail-safe: if FreeIX service is unreachable, OS falls back to backup DNS

### Backup Format
```toml
# ~/.config/freeix/dns-backup.toml
[backup]
timestamp = "2025-01-15T10:30:00Z"
platform = "linux"

[[resolvers]]
address = "192.168.1.1"
interface = "eth0"

[[resolvers]]
address = "8.8.8.8"
interface = "eth0"
```

## Threat Model

### In Scope
| Threat | Mitigation |
|--------|-----------|
| Malicious blocklist source | HTTPS validation, size limits, sandboxed parsing |
| DNS spoofing (upstream) | DoH/DoT support, DNSSEC validation |
| Local privilege escalation | Minimal capabilities, privilege separation |
| Query log exposure | Local-only storage, user-only permissions |
| Service crash leaving DNS broken | Watchdog auto-restore, backup DNS config |
| Supply chain attacks | `cargo-deny` for dependency auditing, reproducible builds |

### Out of Scope
- Physical access to the machine
- Compromised operating system
- Attacks requiring kernel-level access
- VPN or encrypted tunnel traffic inspection

## Dependency Auditing

FreeIX uses `cargo-deny` to continuously audit dependencies for:
- Known vulnerabilities (RustSec advisory database)
- Prohibited licenses
- Unmaintained crates
- Duplicate dependencies

See `deny.toml` for the full configuration.

## Reporting Security Issues

If you discover a security vulnerability, please report it responsibly:
- **Do NOT** open a public GitHub issue
- Email: security@freeix.dev
- We aim to respond within 48 hours
- We will coordinate disclosure after a fix is available
