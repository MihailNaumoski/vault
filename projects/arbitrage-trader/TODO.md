# TODO

## Research (use `/team` with Research team)

- [ ] **Research Kalshi API & WebSocket protocol** — Use Research team (Doc Researcher + SDK Analyst) to fully map out Kalshi's trading API: REST endpoints, WebSocket format, auth flow, order placement, market data. Check their official docs, any SDKs (Rust/Python/JS), and GitHub repos. Produce structured findings doc before Engineering starts.
- [ ] **Research Polymarket order placement flow** — Verify the full order lifecycle: EIP-712 signing → POST /order → fill detection → settlement. Check if POLY_ADDRESS header is the only missing piece or if there are other gaps.

## High Priority (needed for live trading)

- [ ] **Implement real Kalshi connector** — Replace mock with real API integration. Need API approval first, then REST + WebSocket client matching Kalshi trading API. **Blocked by: Kalshi API research.**
- [ ] **Wire real cross-platform market matching** — Connect arb-matcher pipeline to live market discovery. Currently faking Kalshi prices with offsets. Depends on Kalshi connector.
- [ ] **Add POLY_ADDRESS header to auth.rs** — Polymarket CLOB authenticated endpoints require a POLY_ADDRESS header (wallet address). Missing — needed for live order placement.
- [ ] **Set production risk config values** — `min_book_depth=0` and `min_time_to_close_hours=1` are dev workarounds. Set proper values before live trading.

## Medium Priority

- [ ] **Add continuous market scanning** — Currently seeds markets once on startup. Add periodic scanning (every 5 min) to discover new markets, remove closed ones, update token IDs.
- [ ] **Implement multi-page Gamma API fetching** — Only fetches first 100 markets. Implement cursor-based pagination to get all active markets.
- [ ] **Persist backfilled token IDs to DB** — When DB-loaded pairs have empty token IDs and get backfilled on startup, resolved IDs aren't saved back. Re-resolves every restart.
- [ ] **Test and verify position settlement flow** — Positions are created by the tracker but settlement/P&L booking needs end-to-end testing. Verify unwinder correctly exits positions.

## Low Priority

- [ ] **Add Docker and deployment config** — Dockerfile, docker-compose, environment config for production deployment. Include health check endpoint.
- [ ] **Add monitoring and alerting** — Beyond health.json. Metrics export, alerts on risk limit breaches, connection failures, stale prices.
- [ ] **Populate historical price snapshots** — The `price_snapshots` DB table exists but is never written to. Periodically snapshot prices for backtesting and analysis.
