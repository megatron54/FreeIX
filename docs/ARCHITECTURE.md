# FreeIX Architecture

## System Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        FreeIX Desktop App                       │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌───────────────┐     IPC      ┌────────────────────────────┐ │
│  │   Frontend    │◄────────────►│       Tauri Backend        │ │
│  │  (SolidJS)   │              │       (src-tauri)           │ │
│  └───────────────┘              └─────────────┬──────────────┘ │
│                                               │                 │
└───────────────────────────────────────────────┼─────────────────┘
                                                │
                                                ▼
┌───────────────────────────────────────────────────────────────────┐
│                     FreeIX Service (Background)                    │
├───────────────────────────────────────────────────────────────────┤
│                                                                   │
│  ┌──────────────┐  ┌──────────────────┐  ┌────────────────────┐ │
│  │  DNS Proxy   │  │  Filter Engine   │  │  Config Manager    │ │
│  │ (freeix-dns) │  │ (freeix-filter)  │  │ (freeix-config)    │ │
│  └──────┬───────┘  └────────┬─────────┘  └────────────────────┘ │
│         │                   │                                     │
│         ▼                   ▼                                     │
│  ┌──────────────┐  ┌──────────────────┐                         │
│  │  Upstream    │  │   Blocklists     │                         │
│  │  Resolvers   │  │   (compiled)     │                         │
│  └──────────────┘  └──────────────────┘                         │
│                                                                   │
└───────────────────────────────────────────────────────────────────┘
```

## Crate Responsibilities

### `freeix-dns`
- Listens on `127.0.0.1:53` (or configured port)
- Receives DNS queries from the OS
- Checks domains against the filter engine
- Forwards allowed queries to upstream resolvers
- Returns `0.0.0.0` / `::` for blocked domains
- Maintains query log with timestamps and decisions

### `freeix-filter`
- Parses blocklist formats: hosts files, adblock-style, domain lists
- Compiles blocklists into an optimized in-memory data structure (radix trie)
- Supports allowlist overrides and custom deny rules
- Handles wildcard matching and subdomain rules
- Hot-reloads blocklists without restarting the DNS server

### `freeix-config`
- Manages application configuration (TOML-based)
- Stores user preferences, blocklist URLs, upstream DNS servers
- Handles first-run setup and migration between versions
- Provides typed configuration access to other crates

### `freeix-service`
- Manages the background DNS service lifecycle
- Handles privilege escalation for binding port 53
- Configures system DNS settings (platform-specific)
- Provides health checks and status reporting

### `src-tauri` (Tauri shell)
- Defines IPC commands for the frontend
- Bridges UI actions to service control
- Manages tray icon and notifications
- Handles auto-start and minimize-to-tray

### `src/` (Frontend)
- Dashboard with real-time statistics
- Query log viewer with filtering
- Blocklist management UI
- Settings panel
- Domain allowlist/denylist editor

## Data Flow

### DNS Query Lifecycle

```
1. Application makes DNS request
2. OS resolves via system DNS (127.0.0.1:53 → FreeIX)
3. freeix-dns receives UDP/TCP query
4. Domain extracted from query
5. freeix-filter checks domain against compiled blocklist
   ├─ BLOCKED: Return 0.0.0.0 / :: (NXDOMAIN or null IP)
   └─ ALLOWED: Forward to upstream resolver
6. Response returned to application
7. Query logged (domain, decision, latency, timestamp)
```

### Blocklist Update Flow

```
1. Scheduler triggers update (or user initiates)
2. Download blocklist sources (HTTP/HTTPS)
3. Parse each source into domain rules
4. Merge all rules, deduplicate
5. Compile into optimized lookup structure
6. Hot-swap the active filter (atomic pointer swap)
7. Log update stats (added/removed domains, errors)
```

## Security Model

See [SECURITY.md](SECURITY.md) for the full security model.

Key principles:
- Minimal privilege: only escalate for port 53 binding
- No network communication except DNS forwarding and blocklist downloads
- All data stored locally, no telemetry
- Blocklist sources validated via HTTPS

## Directory Structure

```
FreeIX/
├── src-tauri/           # Tauri application
│   ├── src/
│   │   ├── main.rs      # Entry point
│   │   └── commands/    # IPC command handlers
│   └── Cargo.toml
├── crates/
│   ├── freeix-dns/      # DNS proxy server
│   ├── freeix-filter/   # Filtering engine
│   ├── freeix-config/   # Configuration
│   └── freeix-service/  # System service
├── src/                 # Frontend (SolidJS)
│   ├── components/
│   ├── pages/
│   └── lib/
├── tests/
│   └── integration/     # Integration tests
├── scripts/             # Dev setup scripts
├── docs/                # Documentation
└── Makefile.toml        # Build tasks
```
