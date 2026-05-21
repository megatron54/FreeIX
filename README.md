# FreeIX

**Free your internet. Block ads, trackers, and malware system-wide.**

[![CI](https://github.com/AnomalyCo/FreeIX/actions/workflows/ci.yml/badge.svg)](https://github.com/AnomalyCo/FreeIX/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

FreeIX is an open-source, privacy-first DNS-based ad blocker for Windows, macOS, and Linux. It intercepts DNS queries at the system level and blocks ads, trackers, and malware domains before they ever reach your browser or apps.

![FreeIX Screenshot](docs/screenshots/placeholder.png)

## Features

- **System-wide blocking** - Works across all browsers and applications
- **DNS-level filtering** - Blocks requests before connections are established
- **Multiple blocklist support** - EasyList, Steven Black, OISD, and custom lists
- **Real-time query log** - See exactly what's being blocked
- **Allowlist/Denylist** - Fine-grained control over individual domains
- **Statistics dashboard** - Track blocked requests, top domains, and trends
- **Automatic updates** - Blocklists refresh on a configurable schedule
- **Minimal resource usage** - Written in Rust for speed and low memory footprint
- **No telemetry** - Zero data collection, fully local operation
- **Cross-platform** - Native experience on Windows, macOS, and Linux

## Installation

### Pre-built Binaries

Download the latest release for your platform from the [Releases](https://github.com/AnomalyCo/FreeIX/releases) page.

### Build from Source

**Prerequisites:**
- [Rust](https://rustup.rs/) (stable, see `rust-toolchain.toml`)
- [Node.js](https://nodejs.org/) >= 18
- [pnpm](https://pnpm.io/) >= 8
- Platform-specific dependencies (see below)

**Linux:**
```bash
sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
```

**macOS:**
```bash
xcode-select --install
```

**Build:**
```bash
# Clone the repository
git clone https://github.com/AnomalyCo/FreeIX.git
cd FreeIX

# Install frontend dependencies
pnpm install

# Build the application
cargo tauri build
```

The built application will be in `src-tauri/target/release/bundle/`.

## Architecture

FreeIX consists of several components:

| Component | Description |
|-----------|-------------|
| `freeix-dns` | Core DNS proxy server with filtering engine |
| `freeix-filter` | Blocklist parser, compiler, and domain matcher |
| `freeix-config` | Configuration management and persistence |
| `freeix-service` | System service for background DNS operation |
| `src-tauri` | Tauri application shell and IPC commands |
| `src/` (frontend) | SolidJS-based UI dashboard |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full system design.

## Tech Stack

- **Backend:** Rust, Tokio, Trust-DNS (Hickory DNS)
- **Frontend:** SolidJS, TypeScript, TailwindCSS
- **Desktop:** Tauri v2
- **Build:** cargo-make, pnpm, GitHub Actions
- **Testing:** Rust integration tests, Vitest (frontend)

## Contributing

Contributions are welcome! Please read the following before submitting a PR:

1. Fork the repository and create your branch from `main`.
2. Run `cargo fmt` and `cargo clippy` before committing.
3. Add tests for any new functionality.
4. Ensure all CI checks pass.
5. Write clear commit messages.

For major changes, please open an issue first to discuss the proposed change.

## Roadmap

- [x] Core DNS proxy with filtering
- [x] Tauri desktop UI with dashboard
- [x] Multi-platform CI/CD
- [ ] Encrypted DNS (DoH/DoT) upstream support
- [ ] Browser extension companion
- [ ] Network-wide mode (act as LAN DNS server)
- [ ] Auto-update mechanism
- [ ] Plugin system for custom filter rules
- [ ] Mobile companion app (status & stats)

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
