# Trading Lead Expertise

*This file is maintained by the trading lead agent. Do not edit manually.*

<!-- 
This file will grow over time as the agent works.
The agent tracks: patterns, decisions, risks, mistakes, and insights.
Max lines: 5000
-->

## Patterns

### Real Connector Wiring (2026-04-08)
- KalshiConfig requires `api_key_id`, `private_key_pem`, `base_url`, `ws_url` fields
- Kalshi demo sandbox: `demo-api.kalshi.co` (not `trading-api.kalshi.com`)
- Demo should ALWAYS be the default to prevent accidental production trading
- The `--production` flag must require explicit opt-in
- Config TOML field was `api_url` but KalshiConfig uses `base_url` — renamed in TOML to match

### Safety Architecture Decisions
- Hybrid paper mode (real Kalshi demo + paper Polymarket) is the most useful testing setup
- Full paper mode (--paper-both) kept for pure simulation testing
- Flag validation: --production and --demo cannot coexist, --paper and --paper-both cannot coexist
- Kalshi WS subscription has REST polling fallback if WS fails (8s interval)

### arb-cli Mock Removal
- Removed `features = ["mock"]` from arb-cli Cargo.toml dep on arb-kalshi
- Mock module still exists in arb-kalshi for unit tests but no longer used at runtime
- All mock state initialization, mock market seeding, and mock price feed removed from main.rs
