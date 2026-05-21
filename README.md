# FreeIX

**Free your internet. Block ads, trackers, and malware system-wide.**

[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

FreeIX is an open-source, privacy-first DNS-based ad blocker for Windows, macOS, and Linux. It intercepts DNS queries at the system level and blocks ads, trackers, and malware domains before they ever reach your browser or apps.

## Features

- **System-wide blocking** - Works across all browsers and applications
- **DNS-level filtering** - Blocks requests before connections are established
- **Multiple blocklist support** - StevenBlack, OISD Big, EasyPrivacy, AdGuard DNS Filter
- **DNS-over-HTTPS (DoH)** - Encrypted upstream queries to Cloudflare, AdGuard, Quad9
- **Real-time query log** - See exactly what's being blocked (highlighted in red)
- **Allowlist/Denylist** - Fine-grained control over individual domains
- **Statistics dashboard** - Top blocked domains, block rate, category breakdown
- **Persistent config** - Settings saved to TOML, survive restarts
- **Auto-start on boot** - Optional Windows registry integration
- **Minimal resource usage** - Written in Rust for speed and low memory footprint
- **No telemetry** - Zero data collection, fully local operation
- **Cross-platform** - Native experience on Windows, macOS, and Linux

## Installation

### Download (Windows)

**[Download FreeIX v0.1.0](https://github.com/megatron54/FreeIX/releases/latest/download/FreeIX-Windows-x64.exe)**

Place the `.exe` anywhere on your system and run it. No installer needed.

### Build from Source

**Prerequisites:**
- [Rust](https://rustup.rs/) (stable 1.95+)
- [Node.js](https://nodejs.org/) >= 18
- npm (included with Node.js)
- Windows: Visual Studio Build Tools 2022 (MSVC + Windows SDK)
- Linux: `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`
- macOS: `xcode-select --install`

**Build:**
```bash
# Clone the repository
git clone https://github.com/megatron54/FreeIX.git
cd FreeIX

# Install frontend dependencies
cd apps/desktop
npm install

# Build the application (embeds frontend into binary)
npx tauri build --no-bundle
```

The built binary will be at `target/release/freeix-desktop.exe` (or set `CARGO_TARGET_DIR`).

> **Important:** Always use `npx tauri build` instead of `cargo build` — only the Tauri CLI embeds the frontend assets into the binary.

## Architecture

FreeIX is a Rust workspace with 7 core crates:

| Crate | Description |
|-------|-------------|
| `freeix-dns-engine` | DNS proxy server (UDP+TCP), upstream resolution, caching, DoH support |
| `freeix-filtering-engine` | Domain filter with exact, wildcard, and regex rules |
| `freeix-blocklists` | Blocklist fetching, parsing (hosts format, adblock syntax) |
| `freeix-config` | Configuration management and TOML persistence |
| `freeix-platform` | OS-specific DNS management (Windows PowerShell, macOS, Linux) |
| `freeix-stats` | Query statistics and analytics (SQLite) |
| `freeix-desktop` | Tauri 2 app shell, commands, and state management |

Frontend: `apps/desktop/src/` — React 18 + TypeScript + TailwindCSS

## Tech Stack

- **Backend:** Rust, Tokio, Hickory DNS, SQLite
- **Frontend:** React 18, TypeScript, TailwindCSS, Vite
- **Desktop:** Tauri v2
- **DNS:** Plain DNS, DNS-over-HTTPS (reqwest-based)
- **Upstream providers:** AdGuard, Cloudflare Family, Mullvad, Quad9

## How It Works

1. **Enable Protection** starts a local DNS server on `127.0.0.1:53`
2. System DNS is redirected to localhost (requires admin/elevated privileges)
3. All DNS queries flow through FreeIX's filtering engine
4. Blocked domains get `0.0.0.0` (NXDOMAIN), allowed queries go upstream
5. Upstream queries support plain DNS and DoH for privacy
6. Results are cached locally for performance

## DNS Providers

| Provider | Type | Addresses |
|----------|------|-----------|
| AdGuard DNS | Plain + DoH | 94.140.14.14, 94.140.15.15 |
| Cloudflare Family | Plain + DoH | 1.1.1.3, 1.0.0.3 |
| Mullvad | Plain | 194.242.2.2, 194.242.2.3 |
| Quad9 | Plain + DoH | 9.9.9.9, 149.112.112.112 |
| Cloudflare | Plain | 1.1.1.1, 1.0.0.1 |

## Blocklists

| List | Coverage |
|------|----------|
| StevenBlack Unified | Ads, malware, fakenews |
| OISD Big | Comprehensive ads/trackers |
| EasyPrivacy | Trackers, analytics, telemetry |
| AdGuard DNS Filter | AdGuard's curated filter |

## Contributing

Contributions are welcome! Please:

1. Fork the repository and create your branch from `master`.
2. Run `cargo fmt` and `cargo clippy` before committing.
3. Add tests for any new functionality.
4. Write clear commit messages.

## Roadmap

- [x] Core DNS proxy with filtering
- [x] Tauri desktop UI with dashboard
- [x] Real-time query log with blocked highlighting
- [x] Statistics page with top blocked domains
- [x] DNS-over-HTTPS (DoH) upstream support
- [x] Persistent configuration (TOML)
- [x] Auto-start on boot (Windows)
- [x] Multiple DNS provider selection
- [ ] System tray with minimize-to-tray
- [ ] Admin elevation for DNS changes
- [ ] Network-wide mode (act as LAN DNS server)
- [ ] Auto-update mechanism
- [ ] Browser extension companion
- [ ] Mobile companion app

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
