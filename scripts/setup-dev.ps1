#Requires -Version 5.1
<#
.SYNOPSIS
    FreeIX development environment setup for Windows.
.DESCRIPTION
    Installs all required dependencies for building FreeIX from source on Windows.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Step {
    param([string]$Message)
    Write-Host "`n=> $Message" -ForegroundColor Cyan
}

function Test-Command {
    param([string]$Command)
    $null -ne (Get-Command $Command -ErrorAction SilentlyContinue)
}

Write-Host "============================================" -ForegroundColor Green
Write-Host "  FreeIX Development Environment Setup"      -ForegroundColor Green
Write-Host "  Platform: Windows"                         -ForegroundColor Green
Write-Host "============================================" -ForegroundColor Green

# Check for Rust
Write-Step "Checking Rust installation..."
if (Test-Command "rustup") {
    Write-Host "  Rust is installed: $(rustc --version)"
    rustup update stable
    rustup component add rustfmt clippy
} else {
    Write-Host "  Rust not found. Installing via rustup..." -ForegroundColor Yellow
    Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile "$env:TEMP\rustup-init.exe"
    & "$env:TEMP\rustup-init.exe" -y --default-toolchain stable --component rustfmt clippy
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
}

# Check for Node.js
Write-Step "Checking Node.js installation..."
if (Test-Command "node") {
    Write-Host "  Node.js is installed: $(node --version)"
} else {
    Write-Host "  Node.js not found. Please install Node.js >= 18 from https://nodejs.org/" -ForegroundColor Red
    exit 1
}

# Check for pnpm
Write-Step "Checking pnpm installation..."
if (Test-Command "pnpm") {
    Write-Host "  pnpm is installed: $(pnpm --version)"
} else {
    Write-Host "  Installing pnpm..."
    npm install -g pnpm
}

# Install cargo tools
Write-Step "Installing cargo tools..."
cargo install cargo-make --locked 2>$null
cargo install tauri-cli --locked 2>$null
cargo install cargo-deny --locked 2>$null

# Install frontend dependencies
Write-Step "Installing frontend dependencies..."
pnpm install

# Verify setup
Write-Step "Verifying setup..."
Write-Host "  rustc:      $(rustc --version)"
Write-Host "  cargo:      $(cargo --version)"
Write-Host "  node:       $(node --version)"
Write-Host "  pnpm:       $(pnpm --version)"

Write-Host "`n============================================" -ForegroundColor Green
Write-Host "  Setup complete! Run 'cargo make dev' to start developing." -ForegroundColor Green
Write-Host "============================================" -ForegroundColor Green
