# Deployment Architecture Decisions

> Date: 2026-04-06
> Status: Accepted
> Context: Solo developer deploying a prediction market arbitrage bot that handles real money (Polymarket crypto wallet + Kalshi USD). System is a single Rust binary (~10MB) with SQLite, running 24/7 in headless mode.

---

## ADR-1: Deployment Target

**Decision**: Docker on home server (PRIMARY), with documented path to migrate the same image to a €5/month Hetzner VPS (FALLBACK).

**Rationale**:
- The home server is free and already available. For a solo developer learning the system, this eliminates cost friction during the initial paper-trading and low-stakes phase.
- Docker provides the portability layer: the exact same `docker-compose.yml` works on a home server or a cloud VPS. Migration is `docker save` + `docker load` (or push to a private registry), then `docker compose up`.
- A trading bot that misses opportunities loses money, but arbitrage opportunities on prediction markets last **minutes, not milliseconds**. Brief home internet blips (seconds) won't matter; WebSocket reconnection logic handles this. Extended outages (hours) mean missed opportunities, not lost positions — positions are already hedged at entry.
- When the bot is trading with real capital above the user's pain threshold (say >€1,000 exposed), migrating to Hetzner (Falkenstein or Helsinki, EU data residency) for €4.51/month (CX22: 2 vCPU, 4GB RAM) provides professional-grade uptime.

**Alternatives considered**:
| Option | Pros | Cons |
|--------|------|------|
| Home server bare metal (no Docker) | Simplest, no Docker overhead | Not portable, messy upgrades, no isolation |
| Home server + Docker (CHOSEN) | Free, portable, isolated | Depends on home power/internet |
| Hetzner VPS + Docker | Reliable, EU, cheap | €50-60/year cost before earning anything |
| AWS/GCP | Managed services | Overkill, expensive, complex |

**Migration trigger**: Move to cloud when (a) bot is profitable and running with real capital, AND (b) home internet has caused a missed opportunity or an unhedged exposure event.

---

## ADR-2: Container Strategy

**Decision**: Multi-stage Dockerfile with Debian `bookworm-slim` runtime base. `WORKDIR /app` with config baked in and data directory as a named volume.

**Rationale**:

### Multi-stage build (mandatory)
- Rust build image: `rust:1.85-bookworm` (~1.5GB) — only used for compilation
- Runtime image: `debian:bookworm-slim` (~80MB) — only the binary + minimal libs
- Final image size: ~90MB (vs ~1.5GB without multi-stage)

### Debian over Alpine
- Alpine uses musl libc. While `rustls-tls` eliminates the OpenSSL dependency (the most common musl pain point), `sqlx` with SQLite compiles `libsqlite3-sys` from source — this works with musl but has historically caused subtle issues with DNS resolution and threading on musl.
- Debian `bookworm-slim` adds ~50MB vs Alpine but eliminates an entire class of "works on my machine, segfaults in production" bugs.
- For a system handling real money, reliability over image size is the correct trade-off.
- If image size becomes a concern later, Alpine can be tested — the switch is a one-line change.

### WORKDIR strategy
All paths in the codebase are **relative to CWD**:
- Config: `config::File::with_name("config/default")` → expects `config/default.toml` relative to CWD
- Database: `sqlite://data/arb.db?mode=rwc` → expects `data/` directory relative to CWD
- PEM key: `KALSHI_PRIVATE_KEY_PATH=./kalshi_private.pem` → expects file relative to CWD
- `.env`: `dotenvy::dotenv()` → looks in CWD

```
WORKDIR /app
├── config/default.toml   (COPY — baked into image, overridable via volume)
├── data/                  (VOLUME — persistent SQLite database)
├── secrets/               (VOLUME — .env + kalshi_private.pem, read-only)
└── arb                    (COPY — the binary)
```

### Dockerfile outline

```dockerfile
# === Build stage ===
FROM rust:1.85-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo build --release --bin arb

# === Runtime stage ===
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/arb .
COPY config/ config/
RUN mkdir -p data
EXPOSE 8080
ENTRYPOINT ["./arb", "--headless"]
```

**Note**: `ca-certificates` is needed for TLS certificate verification even with `rustls` (it uses the system cert store for root CAs via `rustls-tls-native-roots`).

---

## ADR-3: Secret Management

**Decision**: Docker Compose `env_file` for API keys + bind-mounted secrets directory (read-only) for the PEM key. Host-side file permissions enforced to `600`. Secrets NEVER baked into images or committed to git.

**Rationale**:

### Threat model (solo developer)
The primary threats are:
1. **Accidental git commit** of secrets → immediate wallet drain
2. **Image pushed to public registry** with baked-in secrets
3. **Unauthorized access to host** → reads .env or PEM file

For a solo developer, Docker Swarm secrets or HashiCorp Vault are overkill. File permissions + separation + `.gitignore` discipline is the right level.

### Secret layout on host

```
~/arb-deploy/
├── docker-compose.yml       (in git — no secrets)
├── config/default.toml      (in git — no secrets)
├── secrets/                  (NOT in git, 700 permissions)
│   ├── .env                  (600 permissions)
│   └── kalshi_private.pem    (600 permissions)
└── data/                     (SQLite volume mount)
```

### Environment variable handling

```yaml
# docker-compose.yml
services:
  arb:
    env_file: secrets/.env
    volumes:
      - ./secrets/kalshi_private.pem:/app/secrets/kalshi_private.pem:ro
      - ./data:/app/data
    environment:
      - KALSHI_PRIVATE_KEY_PATH=/app/secrets/kalshi_private.pem
```

### Security rules
1. `secrets/` directory: `chmod 700`, owned by deploying user only
2. `.env` and PEM files: `chmod 600`
3. `.gitignore` MUST contain: `secrets/`, `*.pem`, `.env`
4. Docker image NEVER contains secrets — verified by `docker history`
5. The PEM key override (`KALSHI_PRIVATE_KEY_PATH=/app/secrets/kalshi_private.pem`) ensures the container reads from the mounted path, not the default `./kalshi_private.pem`
6. **Backup the PEM key** — see ADR-5. If lost, Kalshi API access must be regenerated.

### POLY_PRIVATE_KEY special concern
This is a **raw Ethereum private key** that controls the Polymarket wallet. If compromised, all funds in that wallet can be drained instantly and irreversibly. Mitigation:
- Keep only operational funds in the Polymarket wallet (not life savings)
- The `.env` file with this key should exist ONLY on the deployment host
- Consider a hardware wallet or smart contract wallet for larger amounts (out of scope for v1)

---

## ADR-4: Monitoring & Alerting

**Decision**: Three-layer approach — (1) Docker restart policy + healthcheck, (2) log monitoring, (3) future Telegram alerts. No code changes needed for layer 1.

**Rationale**:

### Layer 1: Docker self-healing (Day 1, zero code changes)

```yaml
services:
  arb:
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "pgrep", "-f", "arb"]
      interval: 30s
      timeout: 5s
      retries: 3
      start_period: 10s
```

- `restart: unless-stopped` — if the process panics (which it will, `panic=abort` in release), Docker restarts it automatically.
- The healthcheck verifies the process is running. If it crashes in a loop, Docker marks it unhealthy after 3 failures.
- This covers the "bot crashed at 3 AM" case — it just restarts.

### Layer 2: Log monitoring (Day 1, zero code changes)

The binary already outputs structured logs via `tracing`. With `format = "json"` in production config:

```bash
# Check if bot is running and recent
docker logs --since 5m arb 2>&1 | tail -5

# Watch for errors
docker logs -f arb 2>&1 | grep -i "error\|panic\|unhedged"
```

A simple cron job can alert on silence (no logs in last 5 minutes = bot is dead):

```bash
# /etc/cron.d/arb-monitor (runs every 5 min)
*/5 * * * * root docker logs --since 5m arb 2>&1 | grep -q "." || echo "ARB BOT SILENT" | mail -s "Alert" user@example.com
```

### Layer 3: Telegram alerts (future enhancement, requires code change)

When the system matures, add a lightweight Telegram notification module:
- Send alert on: startup, clean shutdown, unhedged position, daily P&L summary, error rate spike
- Use the Telegram Bot API (single HTTP POST, no library needed)
- This is a **code change** in `arb-cli` — not a deployment concern. Flag for engineering.

### What we explicitly skip
- **HTTP health endpoint**: Would require adding an HTTP server (axum/warp), a new port, firewall rules. Overkill for a solo bot — the process either runs or it doesn't.
- **Prometheus/Grafana**: Beautiful but massive operational overhead for one binary.
- **UptimeRobot/Pingdom**: Requires a public endpoint. The bot has no inbound connections.

---

## ADR-5: Backup Strategy

**Decision**: Daily SQLite backup via cron + `sqlite3 .backup`, PEM key stored in password manager, config in git.

**Rationale**:

### What to back up

| Asset | Size | Importance | Replaceable? | Strategy |
|-------|------|------------|--------------|----------|
| `data/arb.db` | ≤100MB | High (trade history, P&L) | No — data is unique | Daily automated backup |
| `config/default.toml` | <1KB | Medium | Yes — in git | Git repository |
| `secrets/.env` | <1KB | Critical | Partially — API keys can be regenerated | Password manager |
| `secrets/kalshi_private.pem` | ~2KB | Critical | No — must regenerate via Kalshi dashboard | Password manager + encrypted backup |

### SQLite backup script

```bash
#!/bin/bash
# /opt/arb/backup.sh — run daily via cron
set -euo pipefail

BACKUP_DIR="$HOME/arb-backups"
DB_PATH="$HOME/arb-deploy/data/arb.db"
DATE=$(date +%Y%m%d_%H%M%S)

mkdir -p "$BACKUP_DIR"

# Use sqlite3 .backup for a consistent snapshot (safe even while bot is running)
sqlite3 "$DB_PATH" ".backup '$BACKUP_DIR/arb_${DATE}.db'"

# Keep only last 30 days
find "$BACKUP_DIR" -name "arb_*.db" -mtime +30 -delete

# Optional: copy to offsite (uncomment when ready)
# rclone copy "$BACKUP_DIR/arb_${DATE}.db" remote:arb-backups/
```

```cron
# Daily at 04:00
0 4 * * * /opt/arb/backup.sh >> /var/log/arb-backup.log 2>&1
```

### Why `sqlite3 .backup` over file copy
- SQLite can be in the middle of a write (WAL mode). A raw `cp` may produce a corrupted backup.
- `.backup` uses SQLite's internal backup API — guaranteed consistent even under load.
- Alternative: stop the container, copy, restart. But this means downtime = missed opportunities.

### Offsite backup (recommended but not mandatory Day 1)
- `rclone` to Backblaze B2 or a second machine: ~€0.005/GB/month, essentially free for <100MB
- Or simpler: `rsync` to a second drive / NAS on the home network

### PEM key backup
- Store the Kalshi PEM key in a password manager (Bitwarden, 1Password, KeePass)
- Also keep an encrypted copy on a USB drive stored separately
- If lost: regenerate via Kalshi dashboard (account access required, not instant)

---

## ADR-6: Update/Deploy Workflow

**Decision**: Build Docker image locally on macOS using `docker buildx` for `linux/amd64`, then transfer to server via `docker save`/`docker load` (Day 1) or a private registry (later).

**Rationale**:

### Why not cross-compile bare binary?
- Rust cross-compilation from macOS (ARM) to Linux (x86_64) requires a cross-linker and target sysroot.
- The `cross` tool (uses Docker internally) works but adds complexity.
- Since we're already using Docker for deployment, `docker buildx` handles cross-compilation transparently — the build happens inside a Linux container.

### Day 1 workflow (no registry needed)

```bash
# On macOS development machine:

# 1. Build for linux/amd64 (cross-compile via buildx)
docker buildx build --platform linux/amd64 -t arb:latest --load .

# 2. Save image to tarball
docker save arb:latest | gzip > arb-latest.tar.gz

# 3. Transfer to server
scp arb-latest.tar.gz server:~/

# --- On server: ---

# 4. Load image
docker load < arb-latest.tar.gz

# 5. Restart with new image
cd ~/arb-deploy && docker compose up -d
```

Total commands: 5. Total time: ~5 minutes (most is Rust compile). No registry, no CI, no complexity.

### Upgrade workflow (when bot is live)

```bash
# On server:
cd ~/arb-deploy

# Pull new image (or docker load as above)
docker compose pull  # if using a registry
# OR
docker load < arb-latest.tar.gz

# Graceful restart
docker compose up -d

# Verify
docker logs -f arb  # watch for startup logs
```

**Downtime**: ~2-3 seconds between container stop and start. The bot will miss any opportunities during this window. Acceptable for prediction markets (opportunities last minutes).

### Future: Private Docker registry

When deploys become frequent, set up a private registry:
- **GitHub Container Registry** (ghcr.io): free for private repos, integrates with GitHub Actions
- **Self-hosted**: `registry:2` Docker image on the same server, but adds operational burden
- Trigger: when the `scp` workflow feels tedious (probably after 10+ deploys)

### Future: CI/CD via GitHub Actions

```yaml
# .github/workflows/deploy.yml (future)
on:
  push:
    tags: ['v*']
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: docker/build-push-action@v5
        with:
          platforms: linux/amd64
          push: true
          tags: ghcr.io/user/arb:${{ github.ref_name }}
```

This is a **future optimization**. Day 1 should be manual and simple.

### What we explicitly skip
- **Building on the server**: Rust compilation needs ~4GB RAM and takes 5-10 minutes. The home server may not have the headroom, and it's wasteful to install the Rust toolchain on a production machine.
- **Nix/Guix reproducible builds**: Interesting but massive learning curve for marginal benefit here.
- **Blue-green deployment**: One bot, one instance. Just restart it.

---

## Summary Table

| Concern | Decision | Day 1 | Future |
|---------|----------|-------|--------|
| **Target** | Docker on home server | Home server | Hetzner VPS when real capital at risk |
| **Container** | Multi-stage, Debian bookworm-slim, WORKDIR /app | Dockerfile + docker-compose.yml | — |
| **Secrets** | env_file + bind-mounted PEM (read-only, 600 perms) | Host filesystem | Same (no change needed) |
| **Monitoring** | Docker restart + log checks | `restart: unless-stopped` + cron | Telegram bot alerts |
| **Backup** | Daily sqlite3 .backup + password manager for PEM | Cron script + local backups | rclone to B2 offsite |
| **Deploys** | docker buildx → save → scp → load → compose up | Manual 5-command flow | GitHub Actions + GHCR |

---

## Artifacts Needed (for Spec Writer)

Based on these decisions, the following concrete files should be produced:

1. **`Dockerfile`** — Multi-stage build as outlined in ADR-2
2. **`docker-compose.yml`** — Service definition with volumes, env_file, restart policy, healthcheck
3. **`.dockerignore`** — Exclude target/, .git/, secrets/, .env, *.pem
4. **`scripts/backup.sh`** — SQLite backup script from ADR-5
5. **`scripts/deploy.sh`** — Wrapper for the build→save→scp→load workflow from ADR-6
6. **`config/default.toml` updates** — Set `format = "json"` for production logging
7. **`.gitignore` updates** — Ensure secrets/, *.pem, .env are excluded

---

## Security Checklist

- [ ] `.env` file has `chmod 600` — only owner can read
- [ ] `kalshi_private.pem` has `chmod 600`
- [ ] `secrets/` directory has `chmod 700`
- [ ] `.gitignore` contains: `secrets/`, `*.pem`, `.env`, `data/`
- [ ] Docker image verified to contain NO secrets: `docker history arb:latest`
- [ ] PEM key backed up in password manager
- [ ] Polymarket wallet funded with operational amount only, not full portfolio
- [ ] Server SSH uses key auth only (no password)
