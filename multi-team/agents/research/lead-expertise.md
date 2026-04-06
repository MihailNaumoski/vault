# Research Lead Expertise

*This file is maintained by the research lead agent. Do not edit manually.*

### Polymarket WS Protocol — Learned from Phase 6 — 2026-04-06
- **Context**: Official Rust SDK (`rs-clob-client`) was the authoritative source, not the docs
- **Insight**: The docs said `custom_feature_enabled: false` but the SDK uses `true`. The docs mentioned `initial_dump` and `level` fields that don't exist in the SDK struct. Always cross-reference docs with SDK source.
- **Action**: For any API research, always assign BOTH Doc Researcher (for docs) AND SDK Analyst (for code). If they disagree, the SDK wins.

### Kalshi API Protocol -- Learned from Kalshi Research -- 2026-04-06
- **Context**: Researched Kalshi REST + WebSocket API for arb-kalshi crate verification
- **Insight 1**: Kalshi uses RSA-PSS (not PKCS1v15) for request signing. The official Python starter code (`Kalshi/kalshi-starter-code-python/clients.py`) is the definitive source for auth details -- it explicitly uses `padding.PSS` with `MGF1(SHA256)`.
- **Insight 2**: Kalshi WebSocket auth is via HTTP upgrade headers, NOT via a JSON "auth" channel message post-connection. This differs from Polymarket.
- **Insight 3**: Kalshi is mid-migration from integer cents to fixed-point dollar strings. Legacy fields being removed March 2026. All new code should use `_dollars` and `_fp` suffixed fields.
- **Insight 4**: The base URL changed from `trading-api.kalshi.com` to `api.elections.kalshi.com`. Always verify base URLs from current docs, not cached assumptions.
- **Insight 5**: Rate limits are tier-dependent (Basic: 20 read/10 write, up to Prime: 400/400). Only order mutations count as "write". GET requests to /portfolio/* are read-limited.
- **Action**: For exchange API research, always check: (1) signing algorithm exact variant, (2) WebSocket auth mechanism (headers vs JSON message), (3) base URL currency, (4) price format (current migration state).
