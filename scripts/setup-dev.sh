#!/usr/bin/env bash
set -euo pipefail

# FreeIX Development Environment Setup
# Supports: Linux (Debian/Ubuntu, Fedora, Arch) and macOS

GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

step() { echo -e "\n${CYAN}=> $1${NC}"; }
ok() { echo -e "  ${GREEN}$1${NC}"; }
warn() { echo -e "  ${YELLOW}$1${NC}"; }
err() { echo -e "  ${RED}$1${NC}"; exit 1; }

echo -e "${GREEN}============================================${NC}"
echo -e "${GREEN}  FreeIX Development Environment Setup${NC}"
echo -e "${GREEN}  Platform: $(uname -s)${NC}"
echo -e "${GREEN}============================================${NC}"

# Detect OS
OS="$(uname -s)"

# Install system dependencies
step "Installing system dependencies..."
case "$OS" in
    Linux)
        if command -v apt-get &>/dev/null; then
            sudo apt-get update
            sudo apt-get install -y \
                build-essential \
                curl \
                wget \
                libssl-dev \
                libgtk-3-dev \
                libwebkit2gtk-4.1-dev \
                libappindicator3-dev \
                libayatana-appindicator3-dev \
                librsvg2-dev \
                patchelf \
                pkg-config
        elif command -v dnf &>/dev/null; then
            sudo dnf install -y \
                gcc gcc-c++ \
                openssl-devel \
                gtk3-devel \
                webkit2gtk4.1-devel \
                libappindicator-gtk3-devel \
                librsvg2-devel \
                patchelf
        elif command -v pacman &>/dev/null; then
            sudo pacman -Syu --noconfirm \
                base-devel \
                openssl \
                gtk3 \
                webkit2gtk-4.1 \
                libappindicator-gtk3 \
                librsvg \
                patchelf
        else
            warn "Unknown package manager. Install dependencies manually."
        fi
        ;;
    Darwin)
        if ! command -v brew &>/dev/null; then
            warn "Homebrew not found. Installing..."
            /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
        fi
        xcode-select --install 2>/dev/null || true
        ;;
    *)
        err "Unsupported OS: $OS"
        ;;
esac

# Install Rust
step "Checking Rust installation..."
if command -v rustup &>/dev/null; then
    ok "Rust is installed: $(rustc --version)"
    rustup update stable
    rustup component add rustfmt clippy
else
    warn "Rust not found. Installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --component rustfmt clippy
    source "$HOME/.cargo/env"
fi

# Install Node.js
step "Checking Node.js installation..."
if command -v node &>/dev/null; then
    ok "Node.js is installed: $(node --version)"
else
    err "Node.js not found. Please install Node.js >= 18 from https://nodejs.org/"
fi

# Install pnpm
step "Checking pnpm installation..."
if command -v pnpm &>/dev/null; then
    ok "pnpm is installed: $(pnpm --version)"
else
    warn "Installing pnpm..."
    npm install -g pnpm
fi

# Install cargo tools
step "Installing cargo tools..."
cargo install cargo-make --locked 2>/dev/null || true
cargo install tauri-cli --locked 2>/dev/null || true
cargo install cargo-deny --locked 2>/dev/null || true

# Install frontend dependencies
step "Installing frontend dependencies..."
pnpm install

# Verify
step "Verifying setup..."
ok "rustc:  $(rustc --version)"
ok "cargo:  $(cargo --version)"
ok "node:   $(node --version)"
ok "pnpm:   $(pnpm --version)"

echo -e "\n${GREEN}============================================${NC}"
echo -e "${GREEN}  Setup complete! Run 'cargo make dev' to start developing.${NC}"
echo -e "${GREEN}============================================${NC}"
