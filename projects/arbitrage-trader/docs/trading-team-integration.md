# Trading Team Integration

## Multi-Team Orchestration

```
                              +---------------+
                              | ORCHESTRATOR  | opus:xhigh
                              |  (you talk    |
                              |  to this)     |
                              +-------+-------+
                   +------------------+--------------------+
                   |                  |                     |
          +--------v--------+ +------v--------+ +----------v----------+
          |  TRADING        | | ENGINEERING   | | PLANNING            |
          |     LEAD        | |     LEAD      | |    LEAD             |
          +--+-+-+-+-+------+ +--+-+-+-+-+----+ +--+--------+---------+
             | | | | |          | | | | |         |          |
  +----------+ | | | +---+  +--+ | | | +---+  +--+     +----+
  |     +------+ | +--+  |  |  +-+ | +--+  |  |        |
  v     v        v     v  v  v  v   v     v  v  v        v
+----++----+  +----++--++--++--++----++--++--++----+  +----+
|Rust||Qnt |  |Exch||DA||BE||FE|    ||PW||CR||Arch|  |Spec|
|Eng ||Strt|  |Conn||  ||  ||  |    ||  ||  ||    |  |Wrtr|
|Dev ||    |  |Dev ||  ||  ||  |    ||  ||  ||    |  |    |
+----++----+  +----++--++--++--++----++--++--++----+  +----+

  + Research (Lead + Doc Researcher + SDK Analyst)
  + Validation (Lead + QA Engineer + Security Reviewer)

  21 agents total across 5 teams
```

## Arbitrage Engine Architecture

```
  Polymarket WebSocket                         Kalshi REST polling
  wss://ws-subscriptions                       api.elections.kalshi.com
  -clob.polymarket.com                         /markets/{ticker}/orderbook
         |                                              |
         |  PolymarketConnector              KalshiConnector
         |  (HMAC + EIP-712)                 (RSA-PSS-SHA256)
         |                                              |
         +----------------+       +---------------------+
                          v       v
                   +-----------------+
                   |   PriceCache    |  Arc<RwLock<HashMap>>
                   |  poly_yes/no    |
                   |  kalshi_yes/no  |
                   |  timestamps     |
                   +--------+--------+
                            | every 1s
                   +--------v--------+
                   |    Detector     |  spread = 1 - side_a - side_b
                   |  min_spread >=  |  filters: 2c abs, 3% rel,
                   |  $0.02 / 3.0%  |  30s staleness
                   +--------+--------+
                            | opportunity found
               +------------v------------+
               |       RiskManager       |  11 pre-trade checks
               |                         |
               |  1.  engine running     |
               |  2.  pair verified      |
               |  3.  spread >= min      |
               |  4.  time to close >=24h|
               |  5.  poly balance       |
               |  6.  kalshi balance     |
               |  7.  per-market <= $1K  |
               |  8.  total exp <= $10K  |
               |  9.  unhedged <= $500   |
               |  10. book depth >= 50   |
               |  11. unwind rate <= 20% | <-- NEW (was dead code)
               +------------+------------+
                            | all checks pass
               +------------v------------+
               |       Executor          |
               |                         |
               |  tokio::join!(          |
               |    poly.place_order(),  |  price_improve capped
               |    kalshi.place_order() |  at 25% of spread <-- NEW
               |  )                      |
               +------------+------------+
                            | both legs placed
               +------------v------------+
               |        Monitor          |  polls every 500ms
               |                         |
               |  BothFilled --------> Tracker (create position)
               |  NeedsUnwind -------> Unwinder
               |  BothCancelled -----> log + skip
               +------------------------+
                            |
                 +----------+----------+
                 v                     v
          +-------------+    +------------------+
          |   Tracker    |    |    Unwinder       |
          |              |    |                   |
          |  hedged qty  |    |  cancel unfilled  |
          |  guaranteed  |    |  exit at best bid |
          |  profit      |    |                   |
          |  update risk |    |  --> unwind_events| <-- NEW table
          +-------------+    |     (persisted!)   |
                             +------------------+
```

## Data Pipeline

```
  Engine loop
     |
     +--- every 1s ----> Detector scan
     |
     +--- every 30s ---> capture_price_snapshots() ---> SQLite  <-- NEW
     |                    poly_yes, kalshi_yes,
     |                    spread per pair
     |
     +--- every 60s ---> aggregate_daily_pnl() -------> SQLite  <-- NEW
     |                    trades, fills, profit,
     |                    fees, capital
     |
     +--- on fill -----> update_order_in_db() ---------> SQLite  <-- NEW
                          status: open -> filled/cancelled
                          (was broken: orders stuck "open" forever)


  +--------------------------- SQLite ----------------------------+
  |                                                               |
  |  market_pairs        orders              positions            |
  |  opportunities       price_snapshots <-- NOW POPULATED       |
  |  daily_pnl <-------- NOW POPULATED      unwind_events <-- NEW|
  |                                                               |
  +---------------------------------------------------------------+
                            |
                   +--------v--------+
                   |  Data Analyst   |  queries via sqlite3
                   |  health checks  |
                   |  anomaly detect |
                   |  metrics/P&L    |
                   +-----------------+
```

## What Was Built

### Trading Team (5 agents)

| Agent | Role | Domain |
|-------|------|--------|
| Trading Lead | Sequencing rules, risk tiers, 6 named workflows | expertise only |
| Rust Engine Dev | Engine, TUI, backtesting infra, DB, migrations | arb-engine, arb-cli, arb-db, arb-types, arb-risk, arb-matcher, config |
| Quant Strategist | Strategy, risk tuning, spread algos, backtesting | detector.rs, paper.rs, arb-risk, config |
| Exchange Connector Dev | Polymarket/Kalshi APIs, auth, WebSocket | arb-polymarket, arb-kalshi, arb-types |
| Data Analyst | SQL metrics, anomaly detection, trade forensics | docs/analysis |

Code review flows through Engineering's Code Reviewer (cross-team).

### Safety Fixes Applied

- `max_unwind_rate_pct` enforced in pre-trade check (was dead code)
- `min_book_depth` = 50 (was 0, liquidity check was disabled)
- `min_time_to_close` = 24h (was 1h in TOML)
- `price_improve_amount` capped at 25% of spread (was unconditional, could eat thin margins)
- `unwind_events` table persists losses to SQLite (was in-memory only, lost on restart)
- `position_id` nullable in unwind events (unwinds happen before position creation)

### Pipeline Fixes

- Engine now writes order status changes to DB (orders were stuck "open" forever)
- Price snapshots captured every 30 seconds
- Daily P&L aggregated every 60 seconds

### Research Findings

- Kalshi public API works at `api.elections.kalshi.com` (no auth for market data)
- Our codebase uses the old URL `trading-api.kalshi.com` which requires auth for everything
- Public endpoints: `/markets`, `/markets/{ticker}/orderbook`, `/markets/trades`, historical data
- WebSocket still requires auth
- Demo sandbox available at `demo-api.kalshi.co`

### Known Config/Code Discrepancies (Resolved)

| Parameter | Was | Now | Impact |
|-----------|-----|-----|--------|
| `min_book_depth` | TOML=0, code=50 | Both=50 | Liquidity check re-enabled |
| `min_time_to_close_hours` | TOML=1, code=24 | Both=24 | Safer expiry guard |
| `max_unwind_rate_pct` | Configured but never checked | Enforced at 20% | Cascade protection |
| `price_improve_amount` | Configured but unused | Wired in, capped | Better fills |

## Next Steps

- [ ] Switch Kalshi base URL to `api.elections.kalshi.com` (unlocks free price data)
- [ ] Build backtest replay engine (read price_snapshots, feed through detector)
- [ ] Start recording real cross-market spreads
- [ ] Backtest strategies on historical data
