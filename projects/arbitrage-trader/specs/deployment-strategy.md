# Deployment Strategy

> Last updated: 2026-04-06
> Spec author: Spec Writer (Planning team)
> Decision authority: `specs/deployment-decisions.md` (ADR-1 through ADR-6)
> Project root: `/Users/mihail/projects/vault/projects/arbitrage-trader/`

This is the **actionable deployment guide** for the prediction market arbitrage bot. Follow it top-to-bottom for a first-time deployment. All architectural decisions have been pre-made — see `specs/deployment-decisions.md` for full rationale.

---

## Table of Contents

1. [Overview](#1-overview)
2. [Docker Configuration](#2-docker-configuration)
3. [Host Setup Guide](#3-host-setup-guide)
4. [Secret Management](#4-secret-management)
5. [Operations Runbook](#5-operations-runbook)
6. [Backup & Recovery](#6-backup--recovery)
7. [Monitoring](#7-monitoring)
8. [Migration to Cloud](#8-migration-to-cloud)
9. [Systemd Alternative](#9-systemd-alternative)

---

## 1. Overview

### 1.1 Strategy Summary

The arbitrage bot runs as a **single Docker container** on a Linux host — a home server initially, a Hetzner VPS when real capital warrants it (per ADR-1). Portability is built-in: the same `docker-compose.yml` runs identically on any Linux host. Migration is five commands: `docker buildx` → `save` → `scp` → `load` → `compose up`.

The binary (`arb --headless`) is a single ~10MB Rust executable with:

- SQLite for trade persistence (`data/arb.db`, relative to WORKDIR `/app`)
- Outbound WebSocket connections to Polymarket and Kalshi APIs
- RSA signing for Kalshi API authentication
- EIP-712 signing for Polymarket order submission

No inbound ports are exposed. The bot connects outbound only. There is no HTTP health endpoint — the process is either running or it isn't.

### 1.2 System Requirements

**Build machine (developer's macOS)**:
- Docker Desktop with `buildx` support (for `linux/amd64` cross-compilation from Apple Silicon)
- `ssh` and `scp` access to the deployment host
- ~4GB free disk for the build cache (Rust dependencies)

**Deployment host (Linux server or VPS)**:

| Requirement | Minimum | Recommended |
|-------------|---------|-------------|
| OS | Debian 12 or Ubuntu 22.04 LTS | Debian 12 (Bookworm) |
| RAM | 512MB free | 1GB free |
| Disk | 1GB free | 2GB free |
| Docker Engine | 24.0+ | Latest stable |
| Docker Compose | V2 (`docker compose`) | V2 |
| Internet | Outbound HTTPS + WSS | Stable broadband |
| `sqlite3` CLI | Required (for backups) | `apt install sqlite3` |

> **Docker Compose V2**: The compose plugin uses `docker compose` (space), not `docker-compose` (hyphen). All commands in this document use the V2 form.

### 1.3 Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│  Developer (macOS)                                                  │
│                                                                     │
│  ┌─────────────────────┐                                            │
│  │  Source Code        │  docker buildx build --platform linux/amd64│
│  │  Cargo.toml         │──────────────────────────┐                 │
│  │  crates/            │                          ▼                 │
│  │  config/default.toml│               arb-latest.tar.gz (~30MB)   │
│  └─────────────────────┘                          │                 │
│                                                   │  scp            │
└───────────────────────────────────────────────────┼─────────────────┘
                                                    │
┌───────────────────────────────────────────────────┼─────────────────┐
│  Deployment Host  (home server  ──or──  Hetzner VPS)                │
│                                                   ▼                 │
│  ~/arb-deploy/                          docker load                 │
│  ├── docker-compose.yml                           │                 │
│  ├── secrets/  (chmod 700)                        ▼                 │
│  │   ├── .env  (chmod 600)      ┌─────────────────────────────────┐ │
│  │   └── kalshi_private.pem     │  Container: arb                 │ │
│  │       (chmod 600, :ro mount) │                                 │ │
│  └── data/                      │  WORKDIR /app                   │ │
│      └── arb.db  (SQLite)       │  ./arb --headless               │ │
│          (volume mount)         │  config/default.toml  (baked)   │ │
│                                 │  data/arb.db          (volume)  │ │
│  ~/arb-backups/                 │  secrets/kalshi.pem   (ro bind) │ │
│  └── arb_YYYYMMDD.db            │                                 │ │
│      (daily cron backup)        └──────────────┬────────────────-─┘ │
└─────────────────────────────────────────────────┼───────────────────┘
                                                  │ outbound only
                           ┌──────────────────────┴──────────────────────┐
                           │                                             │
               ┌───────────▼────────────┐               ┌───────────────▼──────┐
               │  Polymarket API        │               │  Kalshi API          │
               │  clob.polymarket.com   │               │  trading-api.kalshi  │
               │  HTTPS + WSS           │               │  HTTPS + WSS         │
               └────────────────────────┘               └──────────────────────┘
```

---

## 2. Docker Configuration

> These files live in the **project root** and are committed to git. Secrets are never baked in — they are mounted at runtime from the host filesystem.

### 2.1 Dockerfile

Uses the **cargo-chef pattern** for dependency layer caching (per ADR-2). Without cargo-chef, every source code change triggers a full recompile of all ~50 dependencies (~10 minutes). With cargo-chef, only changed crates recompile (~30-90 seconds for small changes).

**How the caching works**:
- `cargo chef prepare` analyzes `Cargo.toml`/`Cargo.lock` and emits a `recipe.json`
- `cargo chef cook` compiles only the dependencies listed in `recipe.json`
- The `cook` layer is Docker-cached as long as `Cargo.toml`/`Cargo.lock` don't change
- Source code changes only invalidate the final `cargo build` layer

**Pre-build note**: Before building for production, set `format = "json"` in the `[logging]` section of `config/default.toml`. The default `"pretty"` format is for local development; headless Docker containers require JSON for structured log parsing.

```dockerfile
# syntax=docker/dockerfile:1
# =============================================================================
# Dockerfile — arb prediction market arbitrage bot
#
# Multi-stage build using cargo-chef for dependency layer caching.
# Build time: ~10min cold start (first build, downloads all deps)
#             ~1min warm      (source-only change, deps cached)
# Final image: ~90MB (debian:bookworm-slim + binary + config)
#
# Usage:
#   docker buildx build --platform linux/amd64 -t arb:latest --load .
# =============================================================================


# ── Stage 1: chef ─────────────────────────────────────────────────────────────
# Install cargo-chef once. This layer is cached as long as the Rust toolchain
# version doesn't change. cargo install cargo-chef runs on first build only.
FROM rust:1.85-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /build


# ── Stage 2: planner ──────────────────────────────────────────────────────────
# Analyzes Cargo.toml/Cargo.lock and produces recipe.json — a fingerprint of
# all dependency requirements. This layer only invalidates when dependencies
# change (Cargo.toml or Cargo.lock modified), NOT on source code changes.
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo chef prepare --recipe-path recipe.json


# ── Stage 3: builder ──────────────────────────────────────────────────────────
# Two-phase compilation:
#   3a. Cook deps  — slow (~8min first time), but CACHED across source changes
#   3b. Build src  — fast (~30s), only runs when our code changes
FROM chef AS builder

# 3a. Compile all dependencies (cached layer — only re-runs when recipe.json changes)
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# 3b. Compile our source code (runs on every code change, deps already compiled above)
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo build --release --bin arb


# ── Stage 4: runtime ──────────────────────────────────────────────────────────
# Minimal Debian bookworm-slim runtime image.
#
# Why Debian over Alpine (per ADR-2):
#   Alpine uses musl libc. While rustls eliminates the OpenSSL dependency,
#   libsqlite3-sys on musl has subtle threading/DNS issues in production.
#   Debian adds ~50MB vs Alpine but eliminates an entire class of crashes.
#   For a system handling real money, reliability > image size.
FROM debian:bookworm-slim AS runtime

# ca-certificates is required for TLS certificate verification.
# The binary uses rustls-tls-native-roots, which reads the system cert store.
# Without this package, all HTTPS/WSS connections will fail.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# All paths in the codebase are relative to CWD (per ADR-2):
#   config/default.toml  →  config::File::with_name("config/default")
#   data/arb.db          →  sqlite://data/arb.db?mode=rwc
#   secrets/             →  bind-mounted at runtime (never baked in)
#   .env                 →  dotenvy::dotenv() searches CWD (loaded via env_file)
WORKDIR /app

# Copy the release binary from the builder stage
COPY --from=builder /build/target/release/arb .

# Bake config into the image. The file is read at startup relative to WORKDIR.
# IMPORTANT: Set logging.format = "json" in config/default.toml before building.
COPY config/ config/

# Pre-create runtime directories.
# data/ will be a volume mount (persists across updates).
# secrets/ will be a bind mount (PEM key, read-only).
RUN mkdir -p data secrets

# --headless: disables the TUI dashboard, uses log-only output.
# Required for Docker — no terminal to render a UI in.
ENTRYPOINT ["./arb", "--headless"]
```

### 2.2 docker-compose.yml

Place this file in `~/arb-deploy/` on the deployment host. It is safe to commit to git — it contains no secrets.

```yaml
# =============================================================================
# docker-compose.yml — arb arbitrage bot deployment
# Place at: ~/arb-deploy/docker-compose.yml
# Safe to commit to git — no secrets embedded here.
# =============================================================================

services:
  arb:
    # Image built on the dev machine and loaded via docker load.
    # See Section 5.1 (Operations: Deploy New Version) for the full workflow.
    image: arb:latest

    # ── Secrets & environment ─────────────────────────────────────────────────
    # Load all API keys from secrets/.env (per ADR-3).
    # This file is NEVER committed to git — it exists only on the host.
    env_file: secrets/.env

    environment:
      # The .env file has KALSHI_PRIVATE_KEY_PATH=./kalshi_private.pem (relative).
      # Inside the container, the PEM is mounted at /app/secrets/kalshi_private.pem.
      # This environment override ensures the binary reads the correct mounted path.
      - KALSHI_PRIVATE_KEY_PATH=/app/secrets/kalshi_private.pem

    # ── Volumes ───────────────────────────────────────────────────────────────
    volumes:
      # SQLite database — persists across container restarts, updates, and rebuilds.
      # The binary opens: sqlite://data/arb.db?mode=rwc (relative to /app)
      - ./data:/app/data

      # Kalshi RSA private key — bind-mounted read-only for defense in depth.
      # The file must exist on the host before running docker compose up.
      # Host permissions: chmod 600 secrets/kalshi_private.pem
      - ./secrets/kalshi_private.pem:/app/secrets/kalshi_private.pem:ro

    # ── Restart policy ────────────────────────────────────────────────────────
    # unless-stopped: automatically restart on crash or panic.
    # Note: the release profile uses panic=abort, so any panic is a hard crash.
    # Docker will restart the container within a few seconds automatically.
    # Manual `docker compose stop` is respected — won't auto-restart after that.
    # Per ADR-4.
    restart: unless-stopped

    # ── Healthcheck ───────────────────────────────────────────────────────────
    # Verifies the arb process is alive inside the container (per ADR-4).
    # No HTTP endpoint is needed — the process either runs or it doesn't.
    # Docker marks the container "unhealthy" after 3 consecutive failures,
    # which surfaces in `docker ps` and triggers alerts if configured.
    healthcheck:
      test: ["CMD", "pgrep", "-f", "arb"]
      interval: 30s       # check every 30 seconds
      timeout: 5s         # fail check if no response within 5 seconds
      retries: 3          # mark unhealthy after 3 consecutive failures
      start_period: 10s   # grace period on startup before checks begin

    # ── Logging ───────────────────────────────────────────────────────────────
    # json-file driver with rotation prevents unbounded disk growth.
    # max-size: rotate log file at 50MB
    # max-file: keep 5 rotated files (250MB maximum total log storage)
    # Access logs with: docker logs arb
    logging:
      driver: json-file
      options:
        max-size: "50m"
        max-file: "5"
```

### 2.3 .dockerignore

Place this file in the **project root** alongside the Dockerfile. It prevents large build artifacts and sensitive files from being sent to the Docker build context.

```
# .dockerignore
# =============================================================================
# Excludes files from the Docker build context.
# CRITICAL: secrets, .env, and PEM files must NEVER reach the build context
# or the final image. Violation would bake secrets into a layer permanently.
# =============================================================================

# Rust build artifacts — largest exclusion (target/ can exceed 10GB)
target/

# Version control metadata
.git/
.gitignore
.gitattributes

# Secrets — NEVER include in build context (per ADR-3)
secrets/
*.pem
.env
.env.*
.env.example

# Runtime data — not part of the image
data/

# Documentation and specs — not needed in the runtime image
*.md
docs/
specs/

# Development tooling
.cargo/
.vscode/
.idea/
*.swp
*.swo
.DS_Store
```

---

## 3. Host Setup Guide

> Follow these steps **once** on the deployment host (home server or cloud VM). After this, deploys are a single `./scripts/deploy.sh` command.

### 3.1 Directory Structure

```bash
# SSH into your deployment host
ssh user@your-server

# Create the deployment root and required subdirectories
mkdir -p ~/arb-deploy/secrets
mkdir -p ~/arb-deploy/data

# Verify
ls -la ~/arb-deploy/
# Expected output:
# drwxr-xr-x  4 user user  ...  .
# drwxr-xr-x  ...              ..
# drwxr-xr-x  2 user user  ...  data/
# drwxr-xr-x  2 user user  ...  secrets/
```

The final directory tree (after secrets and config are placed):

```
~/arb-deploy/
├── docker-compose.yml          ← from git (safe to commit, no secrets)
├── secrets/   (chmod 700)      ← NOT in git, owner-only access
│   ├── .env   (chmod 600)      ← all API keys
│   └── kalshi_private.pem      ← RSA private key (chmod 600, mounted read-only)
│       (chmod 600)
└── data/                       ← SQLite database volume mount
    └── arb.db                  ← created automatically on first run
```

### 3.2 File Permissions

Security-critical permission setup (per ADR-3). Run this after placing secrets:

```bash
# Lock down the secrets directory: owner-only read/write/execute
chmod 700 ~/arb-deploy/secrets

# Lock down individual secret files: owner-only read/write
chmod 600 ~/arb-deploy/secrets/.env
chmod 600 ~/arb-deploy/secrets/kalshi_private.pem

# Verify
ls -la ~/arb-deploy/secrets/
# Expected output:
# drwx------  2 user user  ... secrets/
# -rw-------  1 user user  ... .env
# -rw-------  1 user user  ... kalshi_private.pem
```

### 3.3 Docker Installation

Install Docker Engine and Docker Compose V2 on Debian/Ubuntu:

```bash
# Official Docker install script (handles apt setup, GPG keys, etc.)
curl -fsSL https://get.docker.com | sh

# Add your user to the docker group so you can run docker without sudo
sudo usermod -aG docker $USER

# Apply group membership immediately (or log out and back in)
newgrp docker

# Verify Docker is working
docker --version        # Docker version 24.x or higher
docker compose version  # Docker Compose version v2.x or higher
```

Full documentation: <https://docs.docker.com/engine/install/debian/>

Install `sqlite3` CLI (required for the backup script in Section 6):

```bash
sudo apt-get update && sudo apt-get install -y sqlite3
sqlite3 --version   # verify
```

### 3.4 First Deploy

Run this sequence top-to-bottom on your **first deployment**:

```bash
# =============================================================================
# PART A — Developer machine (macOS, in project root)
# =============================================================================

# Step 0: Set logging format to JSON for production
# Edit config/default.toml, change the [logging] section:
#   [logging]
#   level = "info"
#   format = "json"     ← was "pretty"; change this before building
#
# The "pretty" format is for local development only.
# In a headless container, JSON is required for structured log parsing.
nano config/default.toml

# Step 1: Build linux/amd64 image (cross-compile via Docker buildx)
#   First run:  ~10 minutes  (downloads Rust deps, full compile)
#   Later runs: ~1 minute    (deps cached by cargo-chef)
docker buildx build --platform linux/amd64 -t arb:latest --load .

# Step 2: Export the image to a compressed tarball (~30MB)
docker save arb:latest | gzip > arb-latest.tar.gz

# Step 3: Transfer to deployment host
#   Replace SERVER with your server's SSH alias or user@hostname
scp arb-latest.tar.gz SERVER:~/

# =============================================================================
# PART B — Deployment host (SSH in after scp completes)
# =============================================================================
ssh SERVER

# Step 4: Load the image into Docker
docker load < ~/arb-latest.tar.gz
rm ~/arb-latest.tar.gz         # clean up the tarball

# Step 5: Place secrets (from your password manager — see Section 4)
cd ~/arb-deploy

# Create secrets/.env from the template in Section 4.1
nano secrets/.env              # paste and fill in your API keys

# Place Kalshi PEM key (see Section 4.2 for sourcing it)
nano secrets/kalshi_private.pem   # paste the PEM content

# Set permissions
chmod 700 secrets/
chmod 600 secrets/.env secrets/kalshi_private.pem

# Step 6: Place docker-compose.yml
# Option A: Copy from your project (recommended — keeps it version-controlled)
# scp user@macbook:~/projects/arbitrage-trader/docker-compose.yml ~/arb-deploy/
#
# Option B: Create it directly on the server (paste from Section 2.2)
# nano ~/arb-deploy/docker-compose.yml

# Step 7: Start the container
docker compose up -d

# Step 8: Verify the container started correctly
docker ps                          # STATUS should say "Up X seconds"
docker logs --tail 50 arb          # look for "Starting arb system"
sleep 15                           # wait for healthcheck start_period
docker inspect --format='{{.State.Health.Status}}' arb   # should say "healthy"

# Step 9: Watch live logs for a few minutes to confirm normal operation
docker logs -f arb
# Ctrl+C to stop following
```

---

## 4. Secret Management

### 4.1 secrets/.env Template

Create `~/arb-deploy/secrets/.env` on the deployment host. **Never commit this file to git.**

```bash
# =============================================================================
# ~/arb-deploy/secrets/.env
# Loaded by docker-compose.yml via: env_file: secrets/.env
# Permissions: chmod 600  |  Directory: chmod 700
# DO NOT commit to git. Verify: git check-ignore -v secrets/
# =============================================================================

# ── Polymarket (Polygon wallet) ───────────────────────────────────────────────
# Raw Ethereum private key — controls the Polymarket CLOB wallet.
# ⚠️  CRITICAL: If leaked, all funds in this wallet can be drained instantly
#     and irreversibly. Keep only operational capital in this wallet.
POLY_PRIVATE_KEY=0xabc123...your_64_hex_chars_here

# CLOB API credentials from https://docs.polymarket.com/developers/api-keys
# Generate via the Polymarket UI (Settings → API Keys)
POLY_API_KEY=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
POLY_API_SECRET=base64encodedSecretHere==
POLY_PASSPHRASE=yourPassphraseHere

# ── Kalshi (RSA key pair) ─────────────────────────────────────────────────────
# API Key ID from Kalshi dashboard: Settings → API Keys → Key ID field
KALSHI_API_KEY_ID=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx

# Path to the RSA private key inside the container.
# DO NOT change this value — docker-compose.yml overrides it via the
# environment: section to /app/secrets/kalshi_private.pem.
# This line documents the variable name for the binary's config loader.
KALSHI_PRIVATE_KEY_PATH=/app/secrets/kalshi_private.pem

# ── Logging ───────────────────────────────────────────────────────────────────
# Overrides the logging.level in config/default.toml at runtime.
# Format: global_level,module=level,module=level
# Available modules: arb_engine, arb_polymarket, arb_kalshi,
#                    arb_matcher, arb_risk, arb_db, arb_types
# Values: error | warn | info | debug | trace
RUST_LOG=info,arb_engine=debug,arb_polymarket=debug,arb_kalshi=debug
```

### 4.2 PEM Key Setup

The Kalshi API uses RSA authentication. The PEM file is the **private half** of the RSA key pair registered in your Kalshi dashboard.

```bash
# ── Step 1: Source the PEM key ────────────────────────────────────────────────
# Option A: You already generated a key pair and stored the PEM in your
#           password manager (Bitwarden / 1Password / KeePass).
#           Retrieve it and proceed to Step 2.
#
# Option B: Generate a new RSA key pair locally (if you haven't yet):
#   openssl genrsa -out kalshi_private.pem 2048
#   openssl rsa -in kalshi_private.pem -pubout -out kalshi_public.pem
#   # Upload kalshi_public.pem to Kalshi dashboard: Settings → API Keys → Add Key
#   # Save the returned Key ID as KALSHI_API_KEY_ID in secrets/.env

# ── Step 2: Place the PEM on the deployment host ──────────────────────────────
# Transfer from your dev machine:
scp kalshi_private.pem SERVER:~/arb-deploy/secrets/kalshi_private.pem

# OR paste directly on the server:
ssh SERVER
nano ~/arb-deploy/secrets/kalshi_private.pem
# Paste the full PEM content including -----BEGIN RSA PRIVATE KEY----- header

# ── Step 3: Set permissions ───────────────────────────────────────────────────
chmod 600 ~/arb-deploy/secrets/kalshi_private.pem

# ── Step 4: Verify the mount works ───────────────────────────────────────────
# The docker-compose.yml maps:
#   ./secrets/kalshi_private.pem  →  /app/secrets/kalshi_private.pem  (read-only)
# The KALSHI_PRIVATE_KEY_PATH env var is set to /app/secrets/kalshi_private.pem
# Verify after starting the container:
docker exec arb ls -la /app/secrets/
# Expected: -r--r--r-- 1 root root ... kalshi_private.pem  (read-only)

# ── Step 5: CRITICAL — back up the PEM key ────────────────────────────────────
# Store in your password manager as a file attachment NOW.
# If lost: regenerate via Kalshi dashboard (requires account access; not instant).
# Also consider an encrypted copy on offline media (USB drive, stored separately).
```

### 4.3 Security Checklist

Run this checklist before every deployment and before going live with real capital.

```bash
# =============================================================================
# Security checklist — run from the deployment host
# =============================================================================

# ── 1. File permission audit ──────────────────────────────────────────────────
echo "=== Checking file permissions ==="

stat -c "%a %n" ~/arb-deploy/secrets
# Must be: 700 /home/user/arb-deploy/secrets

stat -c "%a %n" ~/arb-deploy/secrets/.env
# Must be: 600 /home/user/arb-deploy/secrets/.env

stat -c "%a %n" ~/arb-deploy/secrets/kalshi_private.pem
# Must be: 600 /home/user/arb-deploy/secrets/kalshi_private.pem

# ── 2. Git verification (run from project root on dev machine) ────────────────
echo "=== Verifying .gitignore ==="

git check-ignore -v secrets/ .env .env.example kalshi_private.pem data/
# Each entry must show a matching rule. Empty output = NOT ignored (danger!).

git ls-files secrets/ .env *.pem data/
# Must produce NO output — if anything shows here, it's tracked in git.

# ── 3. Docker image audit — verify NO secrets baked in ───────────────────────
echo "=== Auditing Docker image layers ==="

docker history arb:latest --no-trunc
# Review each layer. You should only see:
#   apt-get install ca-certificates
#   COPY arb .
#   COPY config/ config/
#   RUN mkdir -p data secrets
#   ENTRYPOINT ["./arb", "--headless"]
# Any layer with secret-looking strings is a violation.

docker run --rm --entrypoint sh arb:latest -c \
  "ls /app/secrets/ && echo 'secrets/ contents:' && cat /app/secrets/* 2>/dev/null || echo '(empty — correct)'"
# Expected: "(empty — correct)"
# The secrets/ dir exists in the image but is empty — filled at runtime via mounts.

# ── 4. Operational checklist ──────────────────────────────────────────────────
echo ""
echo "Manual checklist:"
echo "[ ] PEM key backed up in password manager"
echo "[ ] POLY_PRIVATE_KEY backed up in password manager"
echo "[ ] Polymarket wallet holds only operational funds (not full portfolio)"
echo "[ ] Server SSH uses key authentication only (no password auth)"
echo "      → grep 'PasswordAuthentication' /etc/ssh/sshd_config"
echo "      → Expected: PasswordAuthentication no"
echo "[ ] secrets/ directory is NOT in the git repository"
echo "[ ] .env is NOT in the git repository"
```

---

## 5. Operations Runbook

### 5.1 Deploy a New Version

The `scripts/deploy.sh` script (below) automates the full build→save→scp→load workflow from ADR-6. Create it in the project root and make it executable.

```bash
# Create the scripts directory and save the script
mkdir -p scripts
# Paste the content below into scripts/deploy.sh
chmod +x scripts/deploy.sh
```

**`scripts/deploy.sh`**:

```bash
#!/usr/bin/env bash
# =============================================================================
# scripts/deploy.sh — build and deploy the arb container to a remote host
#
# Usage:
#   ./scripts/deploy.sh                          # uses default SERVER_HOST
#   ./scripts/deploy.sh user@192.168.1.100       # explicit host
#   ./scripts/deploy.sh arb-hetzner /opt/arb     # explicit host + deploy dir
#
# Prerequisites:
#   - Docker Desktop with buildx support
#   - SSH access to SERVER_HOST (key auth recommended)
#   - Recommend adding a Host alias to ~/.ssh/config (see usage note below)
#
# ~/.ssh/config alias (recommended):
#   Host arb-server
#     HostName 192.168.1.100
#     User youruser
#     IdentityFile ~/.ssh/id_ed25519
# =============================================================================

set -euo pipefail

# ── Configuration ─────────────────────────────────────────────────────────────
SERVER_HOST="${1:-arb-server}"        # SSH alias or user@hostname
DEPLOY_DIR="${2:-~/arb-deploy}"       # Directory on server containing docker-compose.yml
IMAGE_NAME="arb:latest"
TARBALL="arb-latest.tar.gz"

# ── Pre-flight checks ─────────────────────────────────────────────────────────
echo "==> Pre-flight checks..."

if ! docker buildx version &>/dev/null; then
    echo "ERROR: docker buildx is not available."
    echo "Install Docker Desktop or the buildx plugin: https://docs.docker.com/buildx/install/"
    exit 1
fi

if ! ssh -o ConnectTimeout=5 -o BatchMode=yes "$SERVER_HOST" true &>/dev/null; then
    echo "ERROR: Cannot connect to '$SERVER_HOST' via SSH."
    echo "Check your ~/.ssh/config or pass user@hostname as the first argument."
    exit 1
fi

echo "==> Server:     $SERVER_HOST"
echo "==> Deploy dir: $DEPLOY_DIR"
echo ""

# ── Step 1: Build image for linux/amd64 ──────────────────────────────────────
echo "==> [1/5] Building linux/amd64 image..."
echo "          (cargo-chef caches deps after first build — ~10min cold, ~1min warm)"
docker buildx build \
    --platform linux/amd64 \
    --tag "$IMAGE_NAME" \
    --load \
    .

IMAGE_SIZE=$(docker image inspect "$IMAGE_NAME" --format='{{.Size}}' | numfmt --to=iec 2>/dev/null || echo "unknown")
echo "==> Image size: $IMAGE_SIZE"

# ── Step 2: Save to compressed tarball ───────────────────────────────────────
echo "==> [2/5] Saving image to $TARBALL..."
docker save "$IMAGE_NAME" | gzip > "$TARBALL"
TARBALL_SIZE=$(du -sh "$TARBALL" | cut -f1)
echo "==> Tarball size: $TARBALL_SIZE"

# ── Step 3: Transfer to server ────────────────────────────────────────────────
echo "==> [3/5] Transferring $TARBALL to $SERVER_HOST..."
scp "$TARBALL" "$SERVER_HOST":~/
rm "$TARBALL"   # clean up local copy

# ── Step 4: Load image on server ──────────────────────────────────────────────
echo "==> [4/5] Loading image on server..."
ssh "$SERVER_HOST" "docker load < ~/arb-latest.tar.gz && rm ~/arb-latest.tar.gz"

# ── Step 5: Restart the container ─────────────────────────────────────────────
echo "==> [5/5] Restarting container..."
# `docker compose up -d` with a new image: stops old container, starts new one.
# Downtime: ~2-3 seconds. Acceptable for prediction markets (opps last minutes).
ssh "$SERVER_HOST" "cd $DEPLOY_DIR && docker compose up -d"

# ── Verify ────────────────────────────────────────────────────────────────────
echo ""
echo "==> Verifying deployment (waiting 5s for startup)..."
sleep 5

echo ""
ssh "$SERVER_HOST" "docker ps --filter name=arb --format 'table {{.Names}}\t{{.Status}}\t{{.Image}}'"

echo ""
echo "==> Last 20 log lines:"
ssh "$SERVER_HOST" "docker logs --tail 20 arb"

echo ""
echo "==> Health status: $(ssh "$SERVER_HOST" "docker inspect --format='{{.State.Health.Status}}' arb 2>/dev/null || echo 'unknown'")"
echo ""
echo "==> Deploy complete."
```

**Downtime per deploy**: ~2-3 seconds (old container stops, new one starts). Prediction market arbitrage opportunities last **minutes**, so this window is acceptable (per ADR-1).

### 5.2 View Logs

```bash
# ── Most common: recent logs ──────────────────────────────────────────────────
docker logs --tail 100 arb                      # last 100 lines
docker logs --tail 100 --timestamps arb         # with timestamps

# ── Follow live ───────────────────────────────────────────────────────────────
docker logs -f arb                              # follow from now
docker logs -f --tail 50 arb                   # start at last 50 lines, then follow

# ── Time-based filtering ──────────────────────────────────────────────────────
docker logs --since 1h arb                     # last hour
docker logs --since 30m arb                    # last 30 minutes
docker logs --since "2026-04-06T14:00:00" arb  # since specific timestamp

# ── Filter by log level (JSON format, after setting format = "json") ──────────
docker logs -f arb 2>&1 | grep '"level":"ERROR"'
docker logs -f arb 2>&1 | grep '"level":"WARN"'
docker logs --since 1h arb 2>&1 | grep '"level":"ERROR"' | wc -l  # count errors

# ── Check for unhedged positions (critical) ───────────────────────────────────
docker logs --since 24h arb 2>&1 | grep -i "unhedged"

# ── Check for WebSocket reconnects ───────────────────────────────────────────
docker logs --since 1h arb 2>&1 | grep -i "reconnect\|disconnect\|connected"

# ── Check bot startup (confirm a clean start) ─────────────────────────────────
docker logs arb 2>&1 | grep "Starting arb system"
```

### 5.3 Restart

```bash
# ── Option A: Graceful restart (routine use) ──────────────────────────────────
# Sends SIGTERM, waits up to 10 seconds for clean shutdown, then SIGKILL.
# Preserves the container — mounts and env are kept.
# Use for: config changes, routine maintenance.
cd ~/arb-deploy
docker compose restart arb

# ── Option B: Full down/up (after loading a new image) ───────────────────────
# Removes the old container and creates a fresh one from the current image.
# Required when you've run docker load with a new image version.
# Volumes (data/, secrets/) are NOT removed.
docker compose down
docker compose up -d

# ── After any restart: verify ─────────────────────────────────────────────────
docker ps                           # STATUS: "Up X seconds"
docker logs --tail 20 arb           # look for "Starting arb system"
sleep 15
docker inspect --format='{{.State.Health.Status}}' arb   # "healthy"
```

### 5.4 Check Health

```bash
# ── Container status overview ─────────────────────────────────────────────────
docker ps --filter name=arb
# STATUS column meanings:
#   "Up 3 hours (healthy)"   → ✅ normal operation
#   "Up 3 hours (unhealthy)" → ❌ healthcheck failing — check logs immediately
#   "Up 3 hours (starting)"  → ⏳ within start_period (first 10s after launch)
#   not shown / "Exited"     → ❌ container has stopped

# ── Detailed healthcheck history ──────────────────────────────────────────────
docker inspect arb --format='{{json .State.Health}}' | python3 -m json.tool
# Shows: Status, FailingStreak, and last 5 check results with timestamps

# ── Is the process alive? ─────────────────────────────────────────────────────
docker exec arb pgrep -f arb
# Returns a PID number if running, no output if dead

# ── Resource usage ────────────────────────────────────────────────────────────
docker stats arb --no-stream
# Expected at idle: <5% CPU, <100MB RAM, consistent small network I/O
# Spikes during active trading are normal

# ── Recent activity ───────────────────────────────────────────────────────────
docker logs --since 2m arb
# The bot logs every scan cycle (scan_interval_ms = 1000ms in config).
# If no logs for >2 minutes, the bot may be stuck or rate-limited.

# ── Database health ───────────────────────────────────────────────────────────
sqlite3 ~/arb-deploy/data/arb.db "PRAGMA integrity_check;"
# Expected: ok
# If not "ok": stop the container and restore from backup (see Section 6.4)
```

### 5.5 Emergency Stop

Use this when you need to **immediately halt all trading activity**.

```bash
# ── Stop the container (respects restart: unless-stopped) ────────────────────
cd ~/arb-deploy
docker compose stop arb

# Verify it stopped
docker ps --filter name=arb
# Should show nothing, or STATUS = "Exited"

# ── What happens to open positions? ──────────────────────────────────────────
# IMPORTANT: Stopping the container does NOT close open positions.
# Prediction market contracts are held by your accounts on Polymarket and Kalshi.
# They remain open until they resolve or you manually close them via:
#   - Polymarket UI: https://polymarket.com/portfolio
#   - Kalshi UI: https://kalshi.com/portfolio
#
# The bot enters positions only when they are hedged (both legs placed at entry).
# Your risk is bounded regardless of bot status. See ADR-1 for rationale.

# ── Container restarts automatically on crash — but not after manual stop ─────
# `restart: unless-stopped` means:
#   - Crash/panic → Docker restarts automatically ✅
#   - `docker compose stop` → Does NOT restart ✅ (manual stops are respected)

# ── For extended maintenance: remove the container entirely ───────────────────
docker compose down
# This removes the container but NOT the volumes (data/arb.db is safe).

# ── To resume trading ─────────────────────────────────────────────────────────
docker compose up -d
docker logs -f arb  # watch for clean startup
```

---

## 6. Backup & Recovery

### 6.1 backup.sh Script

This script uses `sqlite3 .backup` — safe even while the bot is actively writing (per ADR-5). A raw `cp` could produce a corrupted file if a write is in progress (SQLite WAL mode).

Save to `~/opt/backup.sh` on the deployment host:

```bash
#!/usr/bin/env bash
# =============================================================================
# ~/opt/backup.sh — daily SQLite backup for the arb bot
#
# Uses sqlite3 .backup (internal backup API) — safe under active writes.
# Retains last 30 days of backups. Optional rclone offsite (commented out).
#
# Per ADR-5. Set up via cron: see Section 6.2.
# Prerequisites: sqlite3 CLI (apt install sqlite3)
# =============================================================================

set -euo pipefail

# ── Configuration (edit if your deploy dir differs) ──────────────────────────
BACKUP_DIR="$HOME/arb-backups"
DB_PATH="$HOME/arb-deploy/data/arb.db"
RETENTION_DAYS=30
DATE=$(date +%Y%m%d_%H%M%S)
BACKUP_FILE="$BACKUP_DIR/arb_${DATE}.db"

# ── Validation ────────────────────────────────────────────────────────────────
if [ ! -f "$DB_PATH" ]; then
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] ERROR: Database not found at $DB_PATH" >&2
    exit 1
fi

if ! command -v sqlite3 &>/dev/null; then
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] ERROR: sqlite3 not installed. Run: sudo apt-get install sqlite3" >&2
    exit 1
fi

# ── Backup ────────────────────────────────────────────────────────────────────
mkdir -p "$BACKUP_DIR"
echo "[$(date '+%Y-%m-%d %H:%M:%S')] Starting backup: $DB_PATH → $BACKUP_FILE"

# sqlite3 .backup uses SQLite's built-in online backup API.
# It is safe to run while the container is running and writing to the DB.
# A raw `cp` during an active write would produce a corrupted backup.
sqlite3 "$DB_PATH" ".backup '$BACKUP_FILE'"

BACKUP_SIZE=$(du -sh "$BACKUP_FILE" | cut -f1)
echo "[$(date '+%Y-%m-%d %H:%M:%S')] Backup complete: $BACKUP_FILE ($BACKUP_SIZE)"

# ── Integrity check on the new backup ────────────────────────────────────────
INTEGRITY=$(sqlite3 "$BACKUP_FILE" "PRAGMA integrity_check;" 2>&1)
if [ "$INTEGRITY" != "ok" ]; then
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] WARNING: Backup integrity check failed: $INTEGRITY" >&2
fi

# ── Retention: remove backups older than RETENTION_DAYS ──────────────────────
DELETED=$(find "$BACKUP_DIR" -name "arb_*.db" -mtime +"$RETENTION_DAYS" -print -delete | wc -l)
echo "[$(date '+%Y-%m-%d %H:%M:%S')] Retention: removed $DELETED file(s) older than ${RETENTION_DAYS} days"

# ── Optional: offsite copy via rclone ────────────────────────────────────────
# Uncomment when rclone is configured with a remote (e.g., Backblaze B2).
# Cost: ~€0.005/GB/month. Free tier covers <100MB.
# Setup: https://rclone.org/backblaze/
#
# rclone copy "$BACKUP_FILE" remote:arb-backups/ --quiet
# echo "[$(date '+%Y-%m-%d %H:%M:%S')] Offsite copy to remote:arb-backups/ complete"

# ── Summary ───────────────────────────────────────────────────────────────────
BACKUP_COUNT=$(find "$BACKUP_DIR" -name "arb_*.db" | wc -l)
BACKUP_TOTAL=$(du -sh "$BACKUP_DIR" | cut -f1)
echo "[$(date '+%Y-%m-%d %H:%M:%S')] Backup store: $BACKUP_COUNT file(s), $BACKUP_TOTAL total on disk"
```

Setup:

```bash
# Save the script
mkdir -p ~/opt
nano ~/opt/backup.sh    # paste the script above
chmod +x ~/opt/backup.sh

# Test manually before enabling cron
~/opt/backup.sh
# Expected output:
# [2026-04-06 04:00:00] Starting backup: .../arb.db → .../arb_20260406_040000.db
# [2026-04-06 04:00:01] Backup complete: .../arb_20260406_040000.db (512K)
# [2026-04-06 04:00:01] Retention: removed 0 file(s) older than 30 days
# [2026-04-06 04:00:01] Backup store: 1 file(s), 512K total on disk

# Verify the backup is valid
sqlite3 ~/arb-backups/arb_*.db "PRAGMA integrity_check;"
# Expected: ok
```

### 6.2 Cron Setup

```bash
# Edit the crontab for the deployment user
crontab -e

# Add this line (runs daily at 04:00 AM local time):
0 4 * * * $HOME/opt/backup.sh >> /var/log/arb-backup.log 2>&1

# If /var/log/arb-backup.log is not writable by your user:
sudo touch /var/log/arb-backup.log
sudo chown $USER /var/log/arb-backup.log

# Verify the cron entry was saved
crontab -l

# Check logs after the first scheduled run
tail -20 /var/log/arb-backup.log
```

### 6.3 Recovery from Total Loss

Scenario: deployment host dies completely. Database, config, and container are gone.

```bash
# ── On new host: install prerequisites ───────────────────────────────────────
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER && newgrp docker
sudo apt-get install -y sqlite3

# ── Recreate directory structure ──────────────────────────────────────────────
mkdir -p ~/arb-deploy/{secrets,data}
chmod 700 ~/arb-deploy/secrets

# ── Restore secrets from password manager ────────────────────────────────────
nano ~/arb-deploy/secrets/.env             # paste from password manager
nano ~/arb-deploy/secrets/kalshi_private.pem   # paste from password manager
chmod 600 ~/arb-deploy/secrets/.env
chmod 600 ~/arb-deploy/secrets/kalshi_private.pem

# ── Restore database from most recent backup ─────────────────────────────────
# Transfer from backup storage (another machine, NAS, B2, USB, etc.)
# Replace source with your actual backup location:
scp backup-machine:~/arb-backups/$(ssh backup-machine "ls -t ~/arb-backups/ | head -1") \
    ~/arb-deploy/data/arb.db

# Verify restored database integrity
sqlite3 ~/arb-deploy/data/arb.db "PRAGMA integrity_check;"
# Must return: ok

# ── Build and deploy fresh image from source ─────────────────────────────────
# From developer machine (project root):
./scripts/deploy.sh user@new-server

# ── Place docker-compose.yml (from git) ───────────────────────────────────────
scp user@devmachine:~/projects/arbitrage-trader/docker-compose.yml ~/arb-deploy/

# ── Start and verify ─────────────────────────────────────────────────────────
cd ~/arb-deploy && docker compose up -d
docker logs --tail 30 arb

# ── Expected data loss ────────────────────────────────────────────────────────
# At most: ~24 hours of trade history (since last daily backup at 04:00)
# Open positions on Polymarket/Kalshi: NOT lost — they exist on-chain/in API
# The bot will reconnect and resume scanning for new opportunities.
```

### 6.4 Recovery from Partial Failure

**Container crash** (most common scenario):
```bash
# Docker's restart policy handles this automatically.
# restart: unless-stopped restarts within ~5 seconds of any crash.
# Verify recovery:
docker logs arb 2>&1 | grep "Starting arb system"
# Should show multiple timestamps if it has restarted before

# Check restart count
docker inspect arb --format='{{.RestartCount}}'
# If count is climbing rapidly (>5 in an hour), something is wrong.
# Diagnose: docker logs --tail 100 arb ← look for panic message above each restart
```

**Database corruption**:
```bash
# Step 1: Stop the container
cd ~/arb-deploy && docker compose stop arb

# Step 2: Check integrity
sqlite3 ~/arb-deploy/data/arb.db "PRAGMA integrity_check;"
# If output is NOT "ok", the DB is corrupted — proceed with restore

# Step 3: Preserve the corrupted file for forensics
cp ~/arb-deploy/data/arb.db ~/arb-deploy/data/arb.db.corrupt.$(date +%s)

# Step 4: Restore from latest backup
LATEST_BACKUP=$(ls -t ~/arb-backups/arb_*.db | head -1)
echo "Restoring from: $LATEST_BACKUP"
cp "$LATEST_BACKUP" ~/arb-deploy/data/arb.db

# Step 5: Verify restored file
sqlite3 ~/arb-deploy/data/arb.db "PRAGMA integrity_check;"
# Must return: ok

# Step 6: Resume
docker compose up -d
```

**Network outage (home internet goes down)**:
```bash
# Brief outages (seconds to a few minutes):
#   The bot's WebSocket connections disconnect and reconnect automatically.
#   No action required.

# Extended outages (hours):
#   The bot was live but not trading. No positions are entered during downtime.
#   Already-open positions remain hedged (entered only when both legs placed).
#
# After connectivity is restored, verify the bot has reconnected:
docker logs --since 10m arb | grep -i "connected\|reconnect\|websocket"

# If still stuck after connectivity returns:
docker compose restart arb
```

---

## 7. Monitoring

### 7.1 Day 1 Monitoring

No code changes required (per ADR-4). Docker's built-in mechanisms provide self-healing from day one:

| Mechanism | Configuration | What it handles |
|-----------|---------------|-----------------|
| `restart: unless-stopped` | docker-compose.yml | Crash/panic restarts automatically |
| Healthcheck (`pgrep -f arb`) | docker-compose.yml | Detects dead process, marks unhealthy |
| Log rotation (json-file) | docker-compose.yml | Prevents disk exhaustion |

**What you don't need** (per ADR-4):
- HTTP health endpoint (requires adding an HTTP server — overkill for one binary)
- Prometheus/Grafana (massive operational overhead for a solo bot)
- UptimeRobot/Pingdom (requires a public endpoint — the bot has no inbound connections)

```bash
# Quick Day 1 health check — run this anytime:
docker inspect --format='{{.State.Health.Status}}' arb
# "healthy"   = all good
# "unhealthy" = check logs immediately: docker logs --tail 50 arb
# "starting"  = recently started (within first 10s)

docker inspect --format='{{.RestartCount}}' arb
# 0 = never crashed since last `docker compose up -d`
# >0 = has crashed at least once; check logs for panic message
```

### 7.2 Log Monitoring Script

A simple cron script that detects bot silence (no log output in 5 minutes = bot may be stuck or dead):

```bash
#!/usr/bin/env bash
# =============================================================================
# ~/opt/check-arb-alive.sh
# Run every 5 minutes via cron. Alerts if the bot has been silent.
#
# Alert options (choose one):
#   A: Email (requires mailutils: apt install mailutils)
#   B: ntfy.sh push notification (no setup needed, free service)
#   C: Write to a log file (simplest — check manually)
# =============================================================================

set -euo pipefail

BOT_NAME="arb"
SILENCE_WINDOW="5m"
ALERT_EMAIL="your@email.com"          # ← update for Option A
NTFY_TOPIC="arb-bot-alerts-abc123"    # ← update for Option B (use a random suffix)

# Check for any log output in the last 5 minutes
if ! docker logs --since "$SILENCE_WINDOW" "$BOT_NAME" 2>&1 | grep -q .; then
    TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')
    HOST=$(hostname)
    MESSAGE="[$TIMESTAMP] ALERT on $HOST: arb bot silent for ${SILENCE_WINDOW} — may be dead or stuck"

    echo "$MESSAGE"

    # ── Option A: Email alert ──────────────────────────────────────────────────
    # echo "$MESSAGE" | mail -s "ARB BOT SILENT" "$ALERT_EMAIL"

    # ── Option B: ntfy.sh push notification (no account needed) ───────────────
    # Receive on phone via ntfy app: https://ntfy.sh
    # curl -s -H "Title: ARB Bot Alert" -d "$MESSAGE" "https://ntfy.sh/$NTFY_TOPIC"

    # ── Option C: Write to log file (always enabled as fallback) ──────────────
    echo "$MESSAGE" >> "$HOME/arb-monitor.log"
fi
```

Cron setup:

```bash
chmod +x ~/opt/check-arb-alive.sh

crontab -e
# Add (runs every 5 minutes):
*/5 * * * * $HOME/opt/check-arb-alive.sh >> /var/log/arb-monitor.log 2>&1
```

### 7.3 Future: Telegram Alerts (Engineering Flag)

> ⚠️ **This requires a code change in `crates/arb-cli/`.** It is not a deployment concern. Flag for the engineering team when the system is profitable and the developer wants real-time mobile notifications.

When ready to implement, add a `TelegramAlerter` to `arb-engine` or `arb-cli`:

```
Trigger events:
  - System startup / clean shutdown
  - Unhedged position detected (critical — immediate alert)
  - Daily P&L summary (informational)
  - Error rate spike (>5 errors/minute)
  - WebSocket disconnect/reconnect (informational)

Implementation sketch:
  - API: POST https://api.telegram.org/bot{TOKEN}/sendMessage
  - No library needed — one reqwest::Client call per notification
  - Add to secrets/.env:
      TELEGRAM_BOT_TOKEN=123456789:ABCdef...
      TELEGRAM_CHAT_ID=987654321
  - Add to config/default.toml:
      [alerts]
      telegram_enabled = false    # flip to true when configured

Estimated engineering effort: 2-4 hours
Crate to modify: crates/arb-cli/src/main.rs or a new crates/arb-alerts/
```

---

## 8. Migration to Cloud

### 8.1 When to Migrate

Migrate from home server to Hetzner VPS when **both** conditions are met (per ADR-1):

1. The bot is **profitable** and trading with **real capital** (personal pain threshold: >€1,000 simultaneously exposed)
2. Home internet has caused **at least one** missed opportunity or unhedged exposure event

**Do not migrate preemptively.** Home server is free. Hetzner costs ~€54/year. During paper trading and early real-money phases, prediction market arbitrage opportunities last **minutes, not milliseconds** — brief internet blips don't cost money; extended outages mean missed opportunities, not open risk (positions are hedged at entry).

### 8.2 Hetzner VPS Setup

**Recommended server spec** (per ADR-1):

| Parameter | Value |
|-----------|-------|
| Provider | Hetzner Cloud — [cloud.hetzner.com](https://cloud.hetzner.com) |
| Server type | CX22 (2 vCPU ARM64, 4GB RAM, 40GB NVMe SSD) |
| Location | Falkenstein DE (FSN1) or Helsinki FI (HEL1) — EU data residency |
| OS image | Debian 12 (Bookworm) |
| Cost | €4.51/month (~€54/year) |
| Snapshot backup | Enable (€0.01/GB/month) — optional but recommended |

Initial server provisioning:

```bash
# ── In Hetzner Cloud Console ──────────────────────────────────────────────────
# 1. Create project: "arb-bot"
# 2. Add Server: CX22, Debian 12, your region, add SSH public key
# 3. Note the IPv4 address: x.x.x.x

# ── From developer machine ────────────────────────────────────────────────────

# Add SSH alias
cat >> ~/.ssh/config << 'EOF'
Host arb-hetzner
  HostName x.x.x.x          # ← replace with Hetzner server IPv4
  User arb
  IdentityFile ~/.ssh/id_ed25519
EOF

# First connect as root to set up non-root user
ssh root@x.x.x.x

# Create dedicated non-root user
useradd -m -s /bin/bash -G sudo arb
mkdir -p /home/arb/.ssh
cp /root/.ssh/authorized_keys /home/arb/.ssh/
chown -R arb:arb /home/arb/.ssh
chmod 700 /home/arb/.ssh && chmod 600 /home/arb/.ssh/authorized_keys

# Disable root login and password authentication
sed -i 's/#PasswordAuthentication yes/PasswordAuthentication no/' /etc/ssh/sshd_config
sed -i 's/PermitRootLogin yes/PermitRootLogin no/' /etc/ssh/sshd_config
systemctl restart sshd

# Install Docker and sqlite3
curl -fsSL https://get.docker.com | sh
usermod -aG docker arb
apt-get install -y sqlite3

# Set up firewall (allow SSH only — bot has no inbound ports)
apt-get install -y ufw
ufw allow ssh
ufw --force enable

exit  # exit root session

# Verify non-root SSH works
ssh arb-hetzner whoami    # should return: arb
```

### 8.3 Migration Steps

The migration is straightforward because the same `docker-compose.yml` works on any Linux host. The image is identical.

```bash
# ── Step 1: Final backup on home server ───────────────────────────────────────
ssh user@home-server ~/opt/backup.sh
# Note the backup filename for restoration

# ── Step 2: Deploy image to Hetzner ──────────────────────────────────────────
# From developer machine (project root):
./scripts/deploy.sh arb-hetzner /home/arb/arb-deploy

# ── Step 3: Set up directories on Hetzner ────────────────────────────────────
ssh arb-hetzner
mkdir -p ~/arb-deploy/{secrets,data}
chmod 700 ~/arb-deploy/secrets

# ── Step 4: Restore secrets from password manager ─────────────────────────────
nano ~/arb-deploy/secrets/.env            # paste your API keys
nano ~/arb-deploy/secrets/kalshi_private.pem  # paste PEM content
chmod 600 ~/arb-deploy/secrets/.env ~/arb-deploy/secrets/kalshi_private.pem

# ── Step 5: Transfer database from home server ────────────────────────────────
# Get latest backup filename from home server
LATEST=$(ssh user@home-server "ls -t ~/arb-backups/ | head -1")
scp user@home-server:~/arb-backups/$LATEST ~/arb-deploy/data/arb.db

# Verify
sqlite3 ~/arb-deploy/data/arb.db "PRAGMA integrity_check;"  # must return: ok

# ── Step 6: Place docker-compose.yml ─────────────────────────────────────────
# scp user@devmachine:~/projects/arbitrage-trader/docker-compose.yml ~/arb-deploy/
exit  # back to dev machine

# ── Step 7: Cutover (do this during a quiet period) ──────────────────────────
# IMPORTANT: Stop home server FIRST to prevent two bots trading simultaneously
ssh user@home-server "cd ~/arb-deploy && docker compose stop arb"

# Start on Hetzner
ssh arb-hetzner "cd ~/arb-deploy && docker compose up -d"

# Verify Hetzner is running cleanly
ssh arb-hetzner "docker logs --tail 30 arb"

# ── Step 8: Set up backup cron on Hetzner ────────────────────────────────────
ssh arb-hetzner
sudo apt-get install -y sqlite3
mkdir -p ~/opt
# Copy backup.sh from Section 6.1 and set up cron per Section 6.2

# ── Step 9: Decommission home server (after 24 hours of stable Hetzner) ──────
ssh user@home-server "cd ~/arb-deploy && docker compose down"

# ── Update deploy.sh default target ──────────────────────────────────────────
# Edit scripts/deploy.sh line: SERVER_HOST="${1:-arb-hetzner}"
```

---

## 9. Systemd Alternative

> Use this approach only if Docker is unavailable or undesirable. Docker is the **primary** deployment method — this is a documented fallback.

**When to consider systemd instead of Docker**:
- Very minimal VPS (<512MB RAM) where Docker overhead matters
- Docker is unavailable (some shared/managed hosting)
- Stronger preference for bare-metal process management

**Tradeoff vs. Docker**:

| Concern | Docker | Systemd |
|---------|--------|---------|
| Portability | ✅ Same image anywhere | ❌ Must cross-compile or build on target |
| Restart policy | ✅ `restart: unless-stopped` | ✅ `Restart=on-failure` |
| Secret management | ✅ `env_file` + volume mount | ✅ `EnvironmentFile` + filesystem |
| Log management | ✅ json-file with rotation | ✅ journald (built into systemd) |
| Container isolation | ✅ Filesystem namespace | ❌ Process-level only |
| Image build (macOS→Linux) | ✅ `docker buildx` handles cross-compile | ❌ Need `cross` tool or native build |
| Update workflow | `docker load` + `compose up` | `cp binary` + `systemctl restart` |

### 9.1 arb.service

```ini
# /etc/systemd/system/arb.service
# =============================================================================
# Systemd unit for the arb prediction market arbitrage bot.
#
# Prerequisites before installing:
#   1. System user:  useradd -r -s /bin/false -d /opt/arb -m arb
#   2. Binary:       /opt/arb/arb  (chmod 755, owned by root)
#   3. Config:       /opt/arb/config/default.toml  (set format = "json")
#   4. Data dir:     /opt/arb/data/  (owned by arb user, writable)
#   5. Secrets dir:  /opt/arb/secrets/  (chmod 700, owned by arb user)
#   6. .env file:    /opt/arb/secrets/.env  (chmod 600)
#   7. PEM key:      /opt/arb/secrets/kalshi_private.pem  (chmod 600)
# =============================================================================

[Unit]
Description=Prediction Market Arbitrage Bot
Documentation=https://github.com/user/arbitrage-trader
# Wait for network to be fully up before starting (required for WebSocket connections)
After=network-online.target
Wants=network-online.target

[Service]
# Run as the dedicated non-root arb user
User=arb
Group=arb

# All config paths are relative to here (mirrors Docker WORKDIR /app)
WorkingDirectory=/opt/arb

# Load API keys and RUST_LOG from secrets file
EnvironmentFile=/opt/arb/secrets/.env

# Override PEM path to absolute (systemd requires absolute paths, not relative)
Environment=KALSHI_PRIVATE_KEY_PATH=/opt/arb/secrets/kalshi_private.pem

# The binary with headless flag (no TUI, log-only output)
ExecStart=/opt/arb/arb --headless

# Restart on crash/panic (panic=abort in release profile means SIGABRT on panic)
# Do not restart on clean exit (code 0) — that's intentional shutdown
Restart=on-failure
RestartSec=5s              # wait 5 seconds before restarting
StartLimitInterval=60s     # within any 60-second window...
StartLimitBurst=5          # ...allow at most 5 restart attempts before giving up

# Logging: all stdout/stderr captured by journald
StandardOutput=journal
StandardError=journal
SyslogIdentifier=arb

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict        # /usr, /boot read-only
ProtectHome=true            # block access to /home, /root
ReadWritePaths=/opt/arb/data    # allow SQLite writes
ReadOnlyPaths=/opt/arb/config /opt/arb/secrets  # config and secrets are read-only

[Install]
WantedBy=multi-user.target
```

### 9.2 Setup Commands

```bash
# ── 1. Create system user ─────────────────────────────────────────────────────
sudo useradd -r -s /bin/false -d /opt/arb -m arb

# ── 2. Create directory structure ────────────────────────────────────────────
sudo mkdir -p /opt/arb/{config,data,secrets}
sudo chown -R arb:arb /opt/arb
sudo chmod 700 /opt/arb/secrets
sudo chmod 755 /opt/arb/data

# ── 3. Deploy the binary ──────────────────────────────────────────────────────
# Option A: Build on a Linux machine natively
#   cargo build --release --bin arb
#   sudo cp target/release/arb /opt/arb/arb
#
# Option B: Extract binary from the Docker image (most compatible with this project)
#   docker buildx build --platform linux/amd64 -t arb:latest --load .
#   docker create --name tmp arb:latest
#   docker cp tmp:/app/arb ./arb-linux-amd64
#   docker rm tmp
#   scp arb-linux-amd64 user@SERVER:/opt/arb/arb

sudo chmod 755 /opt/arb/arb

# ── 4. Deploy config ─────────────────────────────────────────────────────────
# IMPORTANT: set format = "json" in [logging] section
sudo cp config/default.toml /opt/arb/config/default.toml
sudo chown arb:arb /opt/arb/config/default.toml

# ── 5. Place secrets ──────────────────────────────────────────────────────────
sudo nano /opt/arb/secrets/.env              # paste from Section 4.1
sudo nano /opt/arb/secrets/kalshi_private.pem # paste PEM content
sudo chown arb:arb /opt/arb/secrets/.env /opt/arb/secrets/kalshi_private.pem
sudo chmod 600 /opt/arb/secrets/.env /opt/arb/secrets/kalshi_private.pem

# ── 6. Install and enable the service ────────────────────────────────────────
sudo cp arb.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable arb       # start automatically on boot
sudo systemctl start arb        # start now

# ── 7. Verify ─────────────────────────────────────────────────────────────────
sudo systemctl status arb
# Expected: Active: active (running) since ...

# ── 8. View logs (via journald) ───────────────────────────────────────────────
sudo journalctl -u arb -f                       # follow live
sudo journalctl -u arb --since "10 minutes ago" # recent logs
sudo journalctl -u arb -n 50 --no-pager         # last 50 lines
sudo journalctl -u arb -p err                   # errors only

# ── 9. Update the binary ─────────────────────────────────────────────────────
sudo systemctl stop arb
sudo cp /path/to/new/arb /opt/arb/arb
sudo chmod 755 /opt/arb/arb
sudo systemctl start arb
sudo systemctl status arb

# ── 10. Stop/disable ──────────────────────────────────────────────────────────
sudo systemctl stop arb          # stop (will restart on reboot if enabled)
sudo systemctl disable arb       # prevent start on boot
sudo systemctl restart arb       # stop + start
```

### 9.3 When to Use

Use systemd (instead of Docker) when:

- The deployment target is a **very minimal VPS** (<512MB RAM) where Docker daemon overhead (~20-30MB RAM) matters
- Docker Engine cannot be installed (e.g., some container-based VPS hosts like Hetzner Shared Hosting)
- The developer strongly prefers bare-metal process management and systemd tooling (`journalctl`, `systemctl status`)
- You want to eliminate the Docker abstraction layer for debugging

For this project, **Docker is the recommended approach** because:
- The build toolchain (cargo-chef, `docker buildx` for macOS→linux/amd64 cross-compilation) is already Docker-native
- The systemd path requires solving cross-compilation separately (install `cross` tool or native Rust toolchain on server)
- `docker compose` is a familiar operational interface; `systemctl` is equally familiar but requires more manual setup

---

## Appendix: Quick Reference Card

```
# ── Deploy ────────────────────────────────────────────────────────
./scripts/deploy.sh                    # build + transfer + restart
./scripts/deploy.sh user@1.2.3.4       # explicit host

# ── Container lifecycle ───────────────────────────────────────────
docker compose up -d                   # start (or restart after new image)
docker compose stop arb                # graceful stop (won't auto-restart)
docker compose down                    # stop and remove container (data safe)
docker compose restart arb             # graceful restart (same image)

# ── Logs ─────────────────────────────────────────────────────────
docker logs -f arb                     # follow live
docker logs --since 1h arb             # last hour
docker logs --tail 50 arb              # last 50 lines

# ── Health ────────────────────────────────────────────────────────
docker ps --filter name=arb            # status overview
docker inspect --format='{{.State.Health.Status}}' arb  # healthy?
docker inspect --format='{{.RestartCount}}' arb         # crash count
docker stats arb --no-stream           # CPU/RAM usage

# ── Backup ────────────────────────────────────────────────────────
~/opt/backup.sh                        # manual backup now
tail -20 /var/log/arb-backup.log       # check last backup

# ── Database ─────────────────────────────────────────────────────
sqlite3 ~/arb-deploy/data/arb.db "PRAGMA integrity_check;"
```

---

*This specification was generated from `specs/deployment-decisions.md` (ADR-1 through ADR-6).*
*For architectural rationale behind any decision, refer to that document.*
