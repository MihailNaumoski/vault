# Spec Writer Expertise

*This file is maintained by the spec writer agent. Do not edit manually.*

<!-- 
This file will grow over time as the agent works.
The agent tracks: patterns, decisions, risks, mistakes, and insights.
Max lines: 5000
-->

---

## Session: 2026-04-06 — Arbitrage Bot Deployment Strategy

### Task
Wrote `specs/deployment-strategy.md` — a 1556-line, 62KB comprehensive deployment specification from 6 ADRs.

### What Worked Well

**Reading strategy**: Parallelized all 4 source file reads in one call (deployment-decisions.md, Cargo.toml, .env.example, config/default.toml + main.rs). Saved multiple round-trips. Always batch independent reads.

**Spec completeness pattern**: When producing an actionable spec from ADRs, include:
1. Rationale pointer (why this decision, per ADR-X)
2. Complete, copy-paste-ready code block
3. Verification command(s) to confirm it worked
4. What NOT to do (alternatives rejected, and why)

**cargo-chef Dockerfile pattern** (for Rust multi-stage builds):
```dockerfile
FROM rust:1.85-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /build

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json   # deps layer — cached
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo build --release --bin arb                       # src layer — fast

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/arb .
COPY config/ config/
RUN mkdir -p data secrets
ENTRYPOINT ["./arb", "--headless"]
```

**docker-compose.yml volume mount for secrets**: The PEM key override pattern is critical:
```yaml
env_file: secrets/.env          # loads KALSHI_PRIVATE_KEY_PATH=./kalshi_private.pem
environment:
  - KALSHI_PRIVATE_KEY_PATH=/app/secrets/kalshi_private.pem  # overrides to container path
volumes:
  - ./secrets/kalshi_private.pem:/app/secrets/kalshi_private.pem:ro
```
The env_file sets a relative default; the environment: section overrides to the container-absolute path.

**deploy.sh pattern**: Pre-flight SSH check before slow operations (avoids 10-minute build then failed scp):
```bash
if ! ssh -o ConnectTimeout=5 -o BatchMode=yes "$SERVER_HOST" true &>/dev/null; then
    echo "ERROR: Cannot connect to '$SERVER_HOST' via SSH."
    exit 1
fi
```

### Domain Knowledge: Rust + Docker Specific

- **`panic = "abort"` in release profile**: No stack unwinding; process terminates immediately on panic. Docker's `restart: unless-stopped` handles this correctly — the container exits and restarts.
- **ca-certificates on Debian bookworm-slim**: Required even with `rustls` because `rustls-tls-native-roots` reads the system certificate store. Without it, all HTTPS/WSS connections fail silently.
- **SQLite hot backup**: Use `sqlite3 "$DB_PATH" ".backup '$BACKUP_FILE'"` — NOT `cp`. Raw copy during WAL write produces corrupt backup.
- **config/default.toml format**: The binary checks `config.format` at startup. `"pretty"` = human-readable (dev), `"json"` = structured (production). Must be set before building the Docker image — it's baked in.
- **WORKDIR /app**: The binary uses relative paths for everything (config, DB, .env). WORKDIR is load-bearing — all paths break if changed.

### Gotchas Found During This Task

1. **Volume mount + non-root user**: If adding a non-root user to the Dockerfile (`useradd arb`), the `./data:/app/data` volume mount creates `/app/data` owned by `root` on the host. The arb user can't write to it. Solution: pre-create with `chown 1001:1001 ~/arb-deploy/data` OR skip non-root user for simplicity. I opted to skip non-root in the Dockerfile to avoid this complexity for a solo developer — mentioned it as optional hardening.

2. **`scripts/` directory doesn't exist in the project**: Verified with `ls`. The spec instructs creating it with `mkdir -p scripts`. Document this in the spec, not silently.

3. **`docker-compose` vs `docker compose`**: V2 uses a space (plugin form). All commands in specs should use V2 form.

4. **`crontab -e` vs `/etc/cron.d/`**: Using `crontab -e` (user crontab) is simpler for solo developers than system-level cron files. Avoids permission issues.

### Spec Structure Pattern for Operational Guides

For ops-heavy specs, the right structure is:
- Section 1: Overview (ASCII diagram, requirements table)
- Section 2: All config files (complete, copy-paste-ready)
- Section 3: One-time setup (follow top to bottom)
- Section 4: Secrets (separate section — security-critical)
- Section 5: Runbook (day-to-day ops)
- Section 6: Backup/recovery (disaster scenarios)
- Section 7: Monitoring (observability)
- Section 8+: Advanced topics (migration, alternatives)
- Appendix: Quick reference card (most-used commands)

### Template: Acceptance Criteria Verification

After writing a spec, verify:
```bash
# Check all required sections present
grep "^## " spec-file.md

# Check all required subsections present  
grep "^### " spec-file.md

# Check ADR cross-references
grep "ADR-" spec-file.md | wc -l  # should be > 0 for every major decision

# Check no placeholder values remain
grep -i "your_.*_here\|<TODO>\|PLACEHOLDER\|example\.com" spec-file.md
```

### File Size Reference
- Simple spec (3-5 sections): 200-400 lines
- Medium spec (5-8 sections): 400-800 lines  
- Comprehensive ops guide (9+ sections): 1000-1600 lines
- `deployment-strategy.md`: 1556 lines, 62KB — at the upper end of appropriate

