# Architect Expertise

*This file is maintained by the architect agent. Do not edit manually.*

## Project: arbitrage-trader

### Architecture Patterns Observed
- Single Rust workspace with 8 crates (types, polymarket, kalshi, matcher, engine, risk, db, cli)
- All file paths are RELATIVE to CWD: config/default.toml, data/arb.db, ./kalshi_private.pem
- dotenvy loads .env gracefully (won't fail if missing)
- rustls-tls throughout — no OpenSSL dependency (Alpine viable but Debian preferred for reliability)
- SQLite via sqlx with runtime-tokio feature
- Release profile: LTO=thin, codegen-units=1, panic=abort
- Logging: tracing + tracing-subscriber with env-filter and JSON support

### Decisions Made (2026-04-06)
- **Deployment**: Docker on home server (primary), Hetzner VPS (fallback)
- **Container**: Multi-stage Dockerfile, Debian bookworm-slim, WORKDIR /app
- **Secrets**: env_file + bind-mounted PEM, chmod 600, never in image
- **Monitoring**: Docker restart:unless-stopped + cron log checks (day 1), Telegram alerts (future)
- **Backup**: Daily sqlite3 .backup via cron, PEM in password manager
- **Deploy**: docker buildx cross-compile → save → scp → load → compose up

### Key Insights
- ca-certificates needed in runtime image even with rustls (uses system cert store via native-roots)
- sqlite3 .backup is the only safe way to backup SQLite while bot is running (WAL mode)
- KALSHI_PRIVATE_KEY_PATH must be overridden in Docker to point to /app/secrets/kalshi_private.pem
- POLY_PRIVATE_KEY is a raw Ethereum private key — highest sensitivity secret in the system

### Risks Flagged
- Home server reliability for a system handling real money (mitigated by cloud fallback path)
- No HTTP health endpoint means external monitoring tools can't probe the bot
- Telegram alerting requires code changes in arb-cli (flagged for engineering)
