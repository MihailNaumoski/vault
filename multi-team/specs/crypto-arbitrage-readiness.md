# Implementation Readiness Assessment — Crypto Arbitrage System

**Date**: 2026-04-05  
**Author**: Spec Writer Agent  
**Input**: `SPEC.md` v1.0.0 + architectural analysis `specs/crypto-arbitrage-analysis.md`  
**Purpose**: Decide whether to build this as-is, modify it, or scope it down before writing a line of code.

---

## Executive Summary

The SPEC.md is an unusually well-written engineering document. The crate structure, trait design, type system, risk management logic, and technology choices are all defensible. However, the spec has **five categories of problems** that make it unwise to start coding from today:

1. **Critical gaps** — real-money operations are impossible without rebalancing, crash recovery, and order reconciliation
2. **Ambiguous internals** — several key algorithms are named but not defined (VWAP, unwind logic, partial fill reconciliation)
3. **Missing schemas** — most API request/response bodies have no defined shape
4. **Overscoped MVP** — 7 crates, 17 endpoints, PostgreSQL, and a full WebSocket spec before a dashboard exists
5. **Silent assumptions** — static fee schedule, pre-positioned inventory model, no paper trading safety net

**Recommendation**: Two weeks of spec work before any code. Scope MVP down to 3 crates and 6 endpoints. Add the 11 missing operational concerns. Then build.

---

## 1. Implementation Readiness Scorecard

Rating: **1** = cannot implement without guessing; **5** = implement directly, no ambiguity.

### 1.1 Types / Data Models

| Rating | 4 / 5 |
|--------|-------|

**What you can code from today:**
- All enum definitions (`ExchangeId`, `EngineState`, `OrderSide`, `OrderStatus`)
- `TradingPair`, `OrderBook`, `OrderRequest`, `OrderResponse`, `TradeResult`, `Balance`
- `RiskConfig` struct with all fields named and typed
- `ArbitrageOpportunity` with UUID v7, timestamps, profit fields
- The invariants (Decimal for money, DateTime<Utc>, UUID v7) are clear

**What needs more detail before coding:**
- **`OrderBook` methods**: `simulated_fill_price()` and `spread()` are mentioned but not specified. What is the exact VWAP formula? Does `simulated_fill_price(side, quantity)` walk bids/asks level by level? Does it fail if the book doesn't have enough depth? This is used directly in profit calculation — ambiguity here means incorrect profit math.
- **`SystemEvent` variants**: The spec says "tagged enum for all system events" but never lists all variants with their payload types. The WebSocket spec mentions 10+ event types; the Rust enum must match them exactly.
- **`FeeSchedule`**: Used by `ExchangeConnector::fee_schedule()` but never defined. What fields? maker/taker per pair? Flat per exchange? This affects every profit calculation.
- **`SubscriptionHandle`**: Returned by `subscribe_order_book()` but never defined. Does it hold a channel? A task handle? A drop guard for cleanup?

**Verdict**: Define `simulated_fill_price()` algorithm, `FeeSchedule` struct, `SystemEvent` variant list, and `SubscriptionHandle` before implementing any crate that depends on `arb-types`.

---

### 1.2 Exchange Connector Trait

| Rating | 3 / 5 |
|--------|-------|

**What you can code from today:**
- The trait signature is fully written and correct
- HMAC-SHA256 auth flow is mentioned for Binance/Coinbase; SHA512 for Kraken
- Pair normalization table (canonical ↔ exchange-specific) exists for 2 pairs
- WebSocket reconnection backoff schedule (1→2→4→8→16→30s) is specified
- Rate limits per exchange are stated (1200/min Binance, 10/sec Coinbase, 15/sec Kraken)

**What needs more detail before coding:**
- **No `get_order_status()` method on the trait.** If the bot crashes after placing leg 1, there is no way to check whether that order filled. This is not a nice-to-have — it's required for safe restarts.
- **No `get_open_orders()` method.** Same problem. On restart, you cannot know your current position without this.
- **No `get_trade_history()` method.** Cannot reconcile the DB's view of trades against reality.
- **Pair normalization covers only 2 pairs.** What happens with ETH/USDT? The table is incomplete. The spec lists "ETH/USDT" in the trading config but the normalization table shows Kraken as `ETH/USDT` (same). What about BTC/USD vs BTC/USDT (Kraken has both)?
- **Per-exchange order ID format**: `cancel_order(id: &str)` accepts a string, but Binance uses `u64` numeric IDs, Coinbase uses UUID strings, Kraken uses alphanumeric. The implementation needs explicit conversion rules.
- **Rate limiter spec is global, not per-endpoint.** Binance has different limits for order placement (10/sec), WebSocket connections (5 per second), and market data (900/min on certain endpoints). The spec's single `rate_limiter.rs` isn't enough.
- **Coinbase Advanced Trade API** switched from the v2 Pro API in 2023. The spec lists `https://api.coinbase.com` — confirm this is the Advanced Trade API, not the legacy one, and which version.
- **Kraken WebSocket v2** changed message format significantly from v1. The spec says `wss://ws.kraken.com/v2` but doesn't specify the subscription message format, which differs from v1.

**Verdict**: Add `get_order_status()`, `get_open_orders()`, and `get_trade_history()` to the trait. Complete the normalization table for all planned pairs. Define per-endpoint rate limits for each exchange.

---

### 1.3 Arbitrage Engine

| Rating | 3 / 5 |
|--------|-------|

**What you can code from today:**
- Main loop structure (pseudocode in §7.1 is implementable)
- O(n²) exchange pair comparison logic
- Staleness check using `stale_book_threshold_ms`
- Concurrent leg execution via `tokio::join!()`
- Sequential pre-trade → execute → persist flow
- DB persistence via non-blocking mpsc channel

**What needs more detail before coding:**
- **VWAP fill price algorithm is not specified.** §6.2 says "calculate VWAP fill prices walking the book" but there is no formula. Walking a book for 1 BTC at $60,000 might look like: buy 0.3 BTC at ask[0], 0.3 BTC at ask[1], 0.4 BTC at ask[2]. What is the formula? What happens when the book doesn't have enough depth? Does it use partial quantity or reject?
- **Unwind logic is not specified.** §7.3 step 6 says "unwind imbalanced positions." How? Market order? IOC limit at a penalty price? What if the unwind itself fails? What if the unwind exchange is also down? This is a real-money safety mechanism with zero implementation detail.
- **Partial fill reconciliation is undefined.** If leg 1 fills 0.8 BTC (IOC) and leg 2 fills 0.6 BTC (IOC), the system has an inventory imbalance of 0.2 BTC. The spec says "reconcile results (full fill, partial fill, failure)" but doesn't define the reconciliation logic for the partial-partial case.
- **Sequential loop with blocking execution.** The main loop executes trades synchronously — while a trade is in-flight (up to 5 seconds with retries), no new opportunities are processed. The spec doesn't address this. Spawning execution as a background task would fix it, but then concurrent open positions must be tracked and limited to prevent over-exposure.
- **Channel buffer sizes are not defined.** `mpsc::channel(?)` for order book updates — what capacity? Too small and connectors block during execution; too large and stale books queue up.
- **No mechanism to skip a stale opportunity if another arrives while executing.** After spending 300ms on a trade, incoming book updates have been queued. The engine will attempt the next queued opportunity even if it's 2 seconds old.

**Verdict**: Write pseudocode for `simulated_fill_price()`, `unwind()`, and partial fill reconciliation before implementing. Define channel capacities. Decide: sequential execution loop vs. spawned tasks.

---

### 1.4 Risk Management

| Rating | 4 / 5 |
|--------|-------|

**What you can code from today:**
- All 8 pre-trade checks in correct fail-fast order
- Circuit breaker auto-trip conditions (3 consecutive failures, daily loss limit)
- Daily trade counter, daily PnL tracker, position tracker
- `max_slippage_pct` validation
- Config struct with all named limits

**What needs more detail before coding:**
- **No post-trade risk checks.** If both legs execute but at worse prices than expected (high slippage), there's no mechanism to escalate. The risk manager approves the trade in, but nothing audits the outcome.
- **Circuit breaker reset semantics.** `POST /api/v1/risk/circuit-breaker` resets it, but: Does it require a minimum cooldown? Does it require the operator to acknowledge the triggering condition? Can it be reset if the daily loss limit is still exceeded? The spec doesn't say.
- **"Consecutive failures" definition.** Does a partial fill count as a failure? A timeout? A network error? Only a full rejection from the exchange? The circuit breaker's accuracy depends on this definition.
- **Position tracking across restarts.** If the system restarts mid-trade, the position tracker starts at zero. Without `get_open_orders()` from the exchange connector, the risk manager will approve trades that would breach actual exposure limits.
- **Stale book threshold of 1000ms is too generous.** The architecture analysis flagged this: at sub-second opportunity windows, a 1-second-old book is functionally worthless. The spec claims <10ms detection but then accepts data up to 1000ms stale. This is internally inconsistent.

**Verdict**: Strongest section — but add post-trade outcome tracking, define circuit breaker reset conditions, clarify failure semantics, and reduce default `stale_book_threshold_ms` to 200–300ms.

---

### 1.5 Database Schema

| Rating | 2 / 5 |
|--------|-------|

**What you can code from today:**
- Table names and their purposes (6 tables)
- Key design decisions (UUID v7, NUMERIC(20,8), partial indexes, enum types)
- Migration strategy (SQLx migrations, no modifying applied migrations)
- Relationship between tables (opportunities → trades)

**What needs more detail before coding:**
- **No actual DDL is shown.** The spec says "UUID v7 primary keys" and "NUMERIC(20,8) for all money values" and "enum types for exchanges" but never shows a single `CREATE TABLE` statement. This is a significant gap — a developer implementing `arb-db` is guessing at column names, nullable/not-null constraints, foreign key relationships, and index definitions.
- **`configurations` table** stores "versioned config history (JSONB)" — but what is the schema? What triggers a new config version? How does the system load the latest? Is there a "current" flag?
- **`daily_summaries` table** has no field list. What aggregations are computed? Are they computed at query time or materialized? Who writes them?
- **`audit_logs` table** has no field list. What constitutes an "operator action"? What system events are logged here vs. just in the `tracing` log?
- **The `updated_at` trigger** is mentioned but not specified. What tables have it? What is the trigger function?
- **No foreign key relationships defined** between `trades` and `opportunities`. Are trades linked to the opportunity that triggered them? This matters for PnL analysis.
- **No mention of indexes beyond the partial index example.** What indexes exist on `trades`? On `opportunities`? The query patterns (paginated by date, filtered by exchange, filtered by status) require specific indexes that aren't defined.

**Verdict**: Write the actual SQL DDL for all 6 tables, including columns, types, constraints, indexes, and the `updated_at` trigger. This is the single most incomplete section for implementation.

---

### 1.6 API Specification

| Rating | 3 / 5 |
|--------|-------|

**What you can code from today:**
- Complete endpoint table (17 endpoints, methods, paths, descriptions)
- Error response envelope format
- Error code taxonomy (7 codes with HTTP status)
- Authentication mechanism (X-API-Key header)
- Global rate limit (1000 req/min per key)

**What needs more detail before coding:**
- **No request body schemas for any mutating endpoint.** `PUT /api/v1/config` accepts a partial config update but the body format is not specified. What fields are allowed? What happens with unknown fields? What validation runs?
- **No response body schemas for any endpoint.** `GET /api/v1/status` returns "full system status" — but what JSON shape? Which fields are present? What does `GET /api/v1/trades/summary` return? What fields in the summary?
- **Pagination format is not specified.** `GET /api/v1/opportunities` is "paginated, filterable" — what query parameters? `?page=1&per_page=50`? Cursor-based? What does the response envelope look like (`data`, `total`, `next_cursor`)?
- **Filter parameters are not specified.** "Filterable" for `/api/v1/opportunities` — by what? Exchange? Pair? Date range? Status? These are query parameter names a developer needs to implement.
- **`POST /api/v1/risk/circuit-breaker`** — no body spec. Should this require a reason or confirmation token?
- **`GET /api/v1/exchanges/:id/balances`** — what is `:id`? The `ExchangeId` enum value? Lowercase string? Case-sensitive?
- **No spec for the 422 VALIDATION_ERROR details field.** The example shows `"details": {}` — but what structure do validation errors use in practice? Array of field errors?

**Verdict**: Add JSON schemas (even informal ones) for all request and response bodies. Define pagination/filter query parameters. This is the minimum for implementing `arb-server` correctly.

---

### 1.7 WebSocket Specification

| Rating | 3 / 5 |
|--------|-------|

**What you can code from today:**
- Channel names and event type taxonomy
- Heartbeat protocol (30s ping, 10s timeout)
- Order book throttling (100ms)
- Connection URL format

**What needs more detail before coding:**
- **No JSON message schemas for any event type.** The spec lists `opportunity`, `opportunity_expired`, `trade`, `trade_failed`, `orderbook`, `risk_alert`, `exchange_status`, `engine_state` — but what fields does each contain? The WebSocket client (future dashboard, operator tools) cannot be built without knowing the message shape.
- **`subscribe` message format is not specified.** What JSON does the client send? `{"type": "subscribe", "channels": ["opportunities", "trades"]}`? What happens if an unknown channel is requested?
- **`welcome` message format is not specified.** What does the server include? Session ID? Server time? Supported channels?
- **No spec for what happens when the WS connection is unauthenticated.** Is authentication done via query param (`?key=<api_key>`) or via the first message? What HTTP status or close code on auth failure?
- **Broadcast drop behavior on slow subscribers is not documented.** `broadcast::Sender` in Tokio drops messages when the buffer is full and a subscriber is slow. This means a slow monitoring client misses events silently. Operators should know this.

**Verdict**: Write JSON schemas for at least the top 5 most important message types (`opportunity`, `trade`, `risk_alert`, `engine_state`, `subscribe`). Document broadcast drop behavior.

---

### 1.8 Configuration

| Rating | 5 / 5 |
|--------|-------|

**What you can code from today:**
- Complete `default.toml` with all keys, types, and example values
- All environment variable names
- Config section structure (`[server]`, `[database]`, `[trading]`, `[risk]`)
- Hot-reload via `PUT /api/v1/config` (mentioned)

**No critical gaps.** This is the most complete section in the spec. The `config` crate with TOML + `.env` via `dotenvy` is a standard pattern with clear implementation.

**Minor things to add:**
- Which fields are hot-reloadable vs. require restart? (e.g., `server.bind_addr` requires restart; `trading.min_profit_threshold_pct` can be hot-reloaded)
- Validation rules for each field (e.g., `min_profit_threshold_pct` must be > 0.001, `max_daily_loss_usd` must be > `max_loss_per_trade_usd`)

**Verdict**: Implement as-is. Clarify hot-reload scope and add field validation rules.

---

### 1.9 Testing Strategy

| Rating | 2 / 5 |
|--------|-------|

**What you can code from today:**
- The three test command aliases
- That a dedicated test DB exists with transaction rollback
- That a mock exchange connector exists

**What needs more detail before coding:**
- **No test cases are listed.** "Detector tested with synthetic order books" — which cases? Same pair, different prices? One exchange stale? Below threshold? Above threshold? Partial depth? These are the test cases a developer needs to write. Without them, "100% line coverage" means nothing about correctness.
- **"Replay mode" is not specified.** What format are the recorded order book snapshots? JSON files? Binary? A database table? How do you record them? How do you replay them deterministically?
- **100% line coverage for `arb-types`** — `arb-types` contains mostly data structures with trivial derives. Line coverage here is nearly free. This metric gives a false sense of quality.
- **No integration test scenarios.** What is the happy-path integration test? What does the "mock exchange returns stale data" test verify? What does "circuit breaker trips on 3 failures" test look like end-to-end?
- **No API endpoint tests.** 17 endpoints with no test specifications. What HTTP client is used in tests? `axum::test`? `reqwest` against a live test server?
- **Load testing** mentioned as 4 hours but with no target metrics. What req/sec? What latency targets? Against which endpoints?
- **No chaos/failure tests.** What happens when one exchange's WebSocket disconnects? When the DB is unavailable? When a trade partially fills? These are the failures that cause real-money losses.

**Verdict**: The testing section needs to be rewritten as actual test cases with inputs, expected outputs, and failure scenarios. This is the weakest section relative to the complexity being tested.

---

### 1.10 Deployment

| Rating | 4 / 5 |
|--------|-------|

**What you can code from today:**
- Multi-stage Dockerfile
- `docker-compose.yml` with postgres and arb services
- Basic observability: structured JSON logging in production, Prometheus endpoint stub

**Minor gaps:**
- **No health check in Dockerfile** (`HEALTHCHECK CMD curl /health`). The `GET /health` endpoint exists but isn't wired into Docker's health system.
- **No resource limits in docker-compose.** A runaway goroutine shouldn't consume all host memory. Add `mem_limit` and `cpus`.
- **No volume for the `config/` directory.** The Dockerfile copies `config/` to `/etc/arb/config/` at build time. If you want to modify config without rebuilding, you need a bind mount or config volume.
- **No restart policy** on the `arb` service. After a crash, it should restart. `restart: unless-stopped` is the standard.
- **Production config** (`config/production.toml`) is listed in the directory structure but its contents are never shown.

**Verdict**: Solid foundation. Add health check, restart policy, and resource limits before production use.

---

### 1.11 Implementation Roadmap

| Rating | 3 / 5 |
|--------|-------|

**What you can code from today:**
- Phase ordering (types → engine → connectors → server → testing) is correct
- Task groupings are logical
- Phase 1–2 are specific enough to start

**What needs more detail:**
- **Phase 3 tasks are too vague.** "Binance REST + WS: 12h" — this is three subtasks: WebSocket subscription + reconnection (4h), order placement with HMAC signing (4h), and order book parsing/normalization (4h). Each has different complexity and failure modes. The aggregated 12h estimate hides the real work.
- **No acceptance criteria per phase.** How do you know Phase 1 is done? "Risk config + RiskManager skeleton: 6h" — what does done mean? Does the risk manager need to pass tests? Which tests? Which checks must be implemented vs. stubbed?
- **Phase 5 testing is an afterthought.** 16 hours of unit tests in the final phase, on code written 8 weeks earlier, is a recipe for discovering architectural problems too late to fix. Tests should be written alongside each phase.
- **No explicit milestone gates.** There's no checkpoint between phases that says "if you reach this phase and the mock exchange integration doesn't work end-to-end, stop and reassess."
- **No parallel tasks identified.** Connectors for Binance, Coinbase, and Kraken are listed sequentially — but they're independent and could be developed in parallel by separate developers (or in separate sprints).

**Verdict**: Add acceptance criteria to each phase. Move testing to each phase (not just Phase 5). Identify the end-of-Phase-2 integration checkpoint as a mandatory go/no-go gate.

---

## 2. Critical Gaps — What's Missing

These are not optional features. Without them, the system either loses money silently, cannot recover from crashes, or is unsafe to run with real capital.

### 2.1 Balance Rebalancing / Fund Transfer

**Gap severity: CRITICAL**

The spec has no mechanism to handle capital imbalance across exchanges. If BTC is consistently cheaper on Binance, the bot buys on Binance and sells on Coinbase/Kraken. After N trades:
- Binance: accumulated BTC, depleted USDT → can no longer fund buy legs
- Coinbase/Kraken: accumulated USDT, depleted BTC → fine for sell legs but capital is stranded

The bot halts without triggering a circuit breaker, because no individual trade fails — it simply can't fund the next trade.

**What's needed:**
- A balance threshold alert: when any exchange's USDT balance < `max_trade_quantity_usd × 2`, warn the operator
- A rebalancing guide in the spec: what manual steps does the operator take? (Withdraw from exchange A, deposit to exchange B — which takes 10–30 min for crypto, 1–3 days for fiat)
- A halt mechanism: if no exchange has sufficient balance for any configured pair, pause the engine automatically rather than attempting failed trades

The spec's `GET /api/v1/exchanges/:id/balances` helps operators see balances, but there's no alert or auto-pause when they run low.

---

### 2.2 Paper Trading / Simulation Mode

**Gap severity: HIGH**

The spec has a Mock exchange connector and a "replay mode" for testing — but neither is a full paper trading mode. Paper trading means: **real market data, simulated order execution, real PnL tracking**.

Without this:
- The operator cannot validate the system's behavior before deploying real money
- There is no way to tune `min_profit_threshold_pct` without losing money on live trades
- The detection logic may have bugs that only manifest with real exchange data (not synthetic books)

**What's needed:**
- A `paper_trading: true` config flag that routes all `place_order()` calls to a simulated executor
- The simulated executor should:
  - Record the order as "filled" at the current best ask/bid (plus a configurable slippage simulation)
  - Update a virtual balance tracked in memory
  - Write results to the same `trades` table with a `paper_trade: true` column
- All risk checks still run against paper positions (so limits are validated)

---

### 2.3 Crash Recovery and State Reconciliation

**Gap severity: CRITICAL**

The system can crash at any point during trade execution:
- After placing leg 1, before placing leg 2
- After both legs are placed, before receiving confirmations
- During the unwind of a failed leg
- During DB write of a completed trade

On restart, the system starts fresh with empty position tracker and risk counters. If there are open orders or filled-but-unrecorded trades on the exchanges, the system is operating with an incorrect view of reality.

**What's needed:**
- A startup reconciliation routine that:
  1. Calls `get_open_orders()` on all configured exchanges
  2. Calls `get_trade_history(since: last_known_timestamp)` on all exchanges
  3. Compares exchange state against the `trades` table in the DB
  4. For any open orders: either cancel them (safe, conservative) or record the position
  5. For any unrecorded fills: insert them into the `trades` table and update risk counters
  6. If reconciliation fails or finds irreconcilable state: halt and alert operator
- `ExchangeConnector` trait must add `get_open_orders()` and `get_trade_history()` methods
- A `ReconciliationReport` type in `arb-types` for logging startup findings

---

### 2.4 Order Status Polling / Confirmation

**Gap severity: HIGH**

The spec uses IOC (Immediate-or-Cancel) limit orders and assumes they either fill immediately or not at all. In practice:
- IOC orders on Kraken have up to 5 second fill windows before returning
- Exchange REST APIs can return HTTP 200 with status "NEW" before transitioning to "FILLED"
- Network issues can cause the `place_order()` call to timeout without knowing whether the order was accepted

**What's needed:**
- Either: confirm IOC semantics are synchronous and final for all three exchanges (verify per-exchange documentation)
- Or: add `get_order_status(id: &str, pair: &TradingPair)` to the `ExchangeConnector` trait with polling logic for ambiguous responses
- Define: what is the maximum polling window before declaring an order "unknown" and triggering a circuit breaker?

---

### 2.5 Exchange Maintenance Window Handling

**Gap severity: MEDIUM**

All three exchanges perform scheduled maintenance (Binance: typically Sunday 00:00–02:00 UTC, Kraken: quarterly). During maintenance:
- WebSocket connections drop
- REST API returns 503
- The current reconnection logic treats this as a transient error with exponential backoff up to 30s

After 30s, the reconnection loop stalls at max backoff and keeps retrying forever. If maintenance lasts 2 hours, the system is stuck in a retry loop with no operator alert.

**What's needed:**
- Detect repeated 503 responses as a maintenance signal (after N retries)
- Emit a `ExchangeStatus::Maintenance` event via the system event channel
- Surface this in `GET /api/v1/exchanges` response
- Automatically suspend trading on affected pairs (not a full circuit break)
- Resume automatically when the exchange reconnects (without operator intervention)

---

### 2.6 Withdrawal Fee Tracking

**Gap severity: MEDIUM**

When the operator rebalances by withdrawing crypto from one exchange and depositing to another, there's a withdrawal fee (e.g., Binance charges 0.0002 BTC to withdraw BTC, ~$12 at current prices). These fees:
- Are not currently tracked anywhere in the spec
- Affect the true cost basis of trades
- Should be included in PnL calculations to get accurate return figures

**What's needed:**
- A `withdrawal_fees` table: exchange, asset, fee_amount, timestamp
- `GET /api/v1/trades/summary` should optionally include accumulated withdrawal fees
- Operator can log withdrawals via a `POST /api/v1/balances/withdrawal` endpoint or manually via DB

---

### 2.7 Tax Event Logging

**Gap severity: MEDIUM (jurisdiction-dependent)**

In most jurisdictions, each executed trade leg is a taxable event. The current `trades` table records all the necessary data (exchange, asset, quantity, price, timestamp), but there's no:
- Cost basis tracking per asset lot
- Gain/loss calculation per trade
- Export format (CSV for accountants, 8949 for US filers)

**What's needed:**
- A tax-aware PnL view (FIFO or HIFO cost basis method, configurable)
- A `GET /api/v1/tax/events?year=2026` endpoint returning realized gains/losses
- This is explicitly a Phase 2 concern but should be mentioned as a non-goal in the spec to set expectations

---

### 2.8 Alerting (Email / Telegram / Discord)

**Gap severity: HIGH**

The spec has a WebSocket feed for monitoring. But the WebSocket requires an active client connection — if no one is watching, critical events are silently lost.

When should an operator be alerted without being actively connected?
- Circuit breaker trips (the most important)
- Daily loss limit > 50% of max (warning)
- Any exchange goes offline for > 5 minutes
- Available balance on any exchange drops below threshold
- System error / panic
- Unexpected restart

**What's needed:**
- An `[alerting]` config section: enable/disable, webhook URL (Discord/Slack), Telegram bot token, email SMTP config
- An alert router in `arb-server` (or a separate `arb-alert` module) that subscribes to `SystemEvent` and sends notifications for high-priority events
- Alert deduplication: don't send 1000 "circuit breaker tripped" alerts if it keeps retrying

This is not complex to add but its absence means the operator must stare at logs 24/7 to catch problems.

---

### 2.9 Secrets Management

**Gap severity: MEDIUM**

The spec stores API keys in environment variables or `.env` file. This is minimally acceptable for a single-person deployment but has known risks:
- `.env` files are often accidentally committed to git
- Process-level environment variable exposure (any process on the machine can read them via `/proc`)
- No rotation workflow: changing an API key requires a restart

**What's needed:**
- At minimum: document the `.env` file in `.gitignore` and the README
- For production: add optional integration with HashiCorp Vault or AWS Secrets Manager via environment variable injection at container startup (no code change required if using Docker secrets)
- Consider: a secrets validation step at startup that verifies all required env vars are present before connecting to any exchange

---

### 2.10 Monitoring / Observability Beyond Logging

**Gap severity: MEDIUM**

The spec defers Prometheus metrics to Phase 2. This is a mistake for a financial system. Without metrics, you cannot:
- Know when detection latency exceeds the 10ms p99 target
- Correlate trade failures with exchange latency spikes
- Alert when the opportunity queue is growing (engine falling behind)
- Track order book update rate per exchange

**What's needed in Phase 1 (not Phase 2):**
- Counter: `opportunities_detected_total{pair, buy_exchange, sell_exchange}`
- Counter: `trades_executed_total{outcome}` (outcome: filled, partial, failed)
- Histogram: `detection_latency_ms` (p50, p95, p99)
- Histogram: `execution_latency_ms{exchange}` (per-exchange order placement time)
- Gauge: `available_balance_usd{exchange}`
- Gauge: `engine_state` (0=stopped, 1=running, 2=paused)
- The Prometheus `/metrics` endpoint should be in Phase 1, not Phase 2

Adding metrics to the hot path after the fact in Rust requires touching every component again. Instrument from the start.

---

### 2.11 Per-Endpoint Rate Limit Handling

**Gap severity: HIGH**

The spec's `rate_limiter.rs` is described as a single global rate limiter per exchange. Real exchange rate limits are per-endpoint and per-second, not just per-minute globally:

| Exchange | Endpoint | Limit |
|----------|----------|-------|
| Binance | `POST /api/v3/order` | 10/sec (weight 1) |
| Binance | `GET /api/v3/depth` | 100/min (weight 5–50 depending on depth) |
| Binance | `GET /api/v3/account` | 10/sec (weight 10) |
| Coinbase | `POST /api/v3/brokerage/orders` | 5/sec |
| Coinbase | `GET /api/v3/brokerage/market/product_book` | 10/sec |
| Kraken | `POST /0/private/AddOrder` | 15/sec (max) |
| Kraken | `GET /0/public/Depth` | 1/sec burst, 0.33/sec sustained |

Treating these as a single bucket means you'll either under-utilize (too conservative) or get IP-banned (too aggressive). Binance's weight system is particularly important — a single order book query at depth 500 costs 10 weight units, not 1.

**What's needed:**
- A `RateLimiter` that accepts per-endpoint configuration
- Binance's request weight system needs explicit modeling: each endpoint call has a weight, and the global limit is weight/minute, not requests/minute
- `ExchangeError::RateLimited` should include a `retry_after` duration if the exchange provides one (Binance includes `Retry-After` header on 429)

---

## 3. What's Overengineered for MVP

An MVP serves one purpose: **prove the detection and execution logic works with real exchange data before investing in infrastructure.** Here's what can be deferred or simplified.

### 3.1 7 Crates vs. 3–4 Crates

**Current**: 7 crates (`arb-types`, `arb-exchange`, `arb-engine`, `arb-risk`, `arb-db`, `arb-server`, `arb-cli`)

**For a solo operator MVP, this is fine architecture but expensive Rust build times.** A 7-crate workspace means:
- `cargo build` from scratch: 8–12 minutes on typical hardware
- Incremental builds are fast, but cold builds (CI, new dev machine) are slow
- Each crate needs its own `Cargo.toml`, `src/lib.rs`, module declarations

**Simpler alternative** that preserves the important boundaries:
- `arb-core` = types + exchange trait + engine + risk (merged)
- `arb-connectors` = Binance + Coinbase + Kraken implementations
- `arb-server` = API + WebSocket
- `arb-cli` = binary

4 crates, same logical separation, half the boilerplate. The risk-safety argument for separating `arb-risk` is real in production — but for MVP with a solo developer, the mock exchange test is the critical path, not the crate boundary.

**Recommendation**: Keep the 7-crate structure if you plan to grow the team or open-source. Collapse to 4 crates if it's a solo project.

---

### 3.2 PostgreSQL vs. SQLite for MVP

**Current spec**: PostgreSQL with SQLx, migrations, JSONB, enum types, partial indexes, UUID extension, `pg_trgm` for text search.

**For MVP**, the write volume is trivial:
- 10 trades/day × 365 = 3,650 trade records/year
- 1,000 opportunities/day × 365 = 365,000 opportunity records/year
- Balance snapshots: maybe 10/hour × 24 = 240/day

SQLite handles 100,000 writes/second. The above workload is a rounding error.

**SQLite advantages for MVP**:
- Zero infrastructure: no Postgres container, no Docker dependency, no `DATABASE_URL` to configure
- Single file: backup is `cp arb.db arb.db.bak`
- `sqlx` supports SQLite with the same API — it's a one-line change in `Cargo.toml`
- Eliminates the `docker-compose.yml` requirement for development
- Eliminates the `arb-db` complexity (no connection pool tuning, no extension setup)

**SQLite disadvantages**:
- JSONB → not available; use `TEXT` and parse in Rust
- Some PostgreSQL-specific types (`UUID`, `NUMERIC`) need mapping
- `sqlx prepare` offline verification works for SQLite too
- Concurrent writes are serialized — fine given the single-threaded engine loop

**Recommendation**: Start with SQLite. Migrate to PostgreSQL when the system proves profitable and write volume justifies it. This removes ~40 hours of infrastructure work from Phase 1.

---

### 3.3 Full WebSocket Spec Before Dashboard Exists

**Current spec**: Full channel-based subscription model with 10+ message types, throttled order book updates, heartbeat, subscribe/unsubscribe, broadcast channels.

**For MVP**, there is no dashboard. The operator monitors via:
- REST API calls (polling)
- `tracing` logs in the terminal

The WebSocket spec is real engineering work (8 hours in the roadmap). Building a WebSocket broadcaster that no client will consume for months is premature. 

**Recommendation**: Defer WebSocket to Phase 4 (after at least one exchange connector works in production). For MVP, a simple Server-Sent Events endpoint for `risk_alert` and `engine_state` events is sufficient and takes 30 minutes to implement.

---

### 3.4 17 REST Endpoints vs. 6 Endpoints for MVP

| Priority | Endpoint | Needed for MVP? |
|----------|----------|-----------------|
| ✅ Essential | `GET /health` | Yes — ops |
| ✅ Essential | `GET /api/v1/status` | Yes — is it working? |
| ✅ Essential | `POST /api/v1/engine/pause` | Yes — emergency stop |
| ✅ Essential | `POST /api/v1/engine/start` | Yes |
| ✅ Essential | `GET /api/v1/risk/exposure` | Yes — are we overexposed? |
| ✅ Essential | `POST /api/v1/risk/circuit-breaker` | Yes — reset after inspection |
| 🟡 Useful | `GET /api/v1/trades` | Nice to have — can read DB directly for MVP |
| 🟡 Useful | `GET /api/v1/config` + `PUT` | Nice to have — can restart with new config |
| 🟡 Useful | `GET /api/v1/exchanges/:id/balances` | Nice to have |
| ❌ Defer | `GET /api/v1/opportunities` | Defer — query DB directly |
| ❌ Defer | `GET /api/v1/opportunities/live` | Defer |
| ❌ Defer | `GET /api/v1/trades/:id` | Defer |
| ❌ Defer | `GET /api/v1/trades/summary` | Defer |
| ❌ Defer | `GET /api/v1/exchanges` | Defer |
| ❌ Defer | `GET /api/v1/risk/positions` | Defer |
| ❌ Defer | `POST /api/v1/engine/stop` | Defer — same as pause + Ctrl-C |
| ❌ Defer | `/ws` WebSocket | Defer |

**6 endpoints cover 90% of MVP operational needs.** Saves ~10 hours of implementation.

---

### 3.5 Testing Strategy: Is 200 Hours Realistic With This Testing Scope?

The spec in Phase 5 allocates:
- 16h: unit tests (all crates)  
- 8h: integration tests with mock exchange
- 6h: API endpoint tests  
- 4h: load testing  
- **Total: 34 hours of testing in the final phase**

Problems with this:
- "100% line coverage" for `arb-types` is a vanity metric and nearly free (mostly derives)
- Integration tests written after all code is done miss the design feedback loop that tests provide
- 6 hours for 17 API endpoints tests = 21 minutes per endpoint including setup, which is tight
- Load testing in 4 hours with no defined targets or tooling chosen is not credible
- There are zero chaos/failure test cases despite the spec having extensive failure handling logic

**Recommendation**: Remove the testing phase from the roadmap. Replace with "write tests alongside each task." Add specific test cases to each section of the spec. The 34 hours is not wasted — it's relocated to Phase 1–4 where it provides actual value.

---

## 4. Time Estimate Reality Check

### 4.1 The Spec Says: 200 Hours / 10 Weeks

Let's audit each phase against realistic estimates for an **experienced Rust developer** who knows tokio and async Rust but has not worked with these specific exchange APIs before.

| Phase | Spec Hours | Realistic Hours | Reason for Gap |
|-------|-----------|-----------------|----------------|
| Phase 1: Foundation | 34h | 40–50h | DB schema needs to be written from scratch; SQLx compile-time verification is slow in CI; `arb-types` has more edge cases than expected |
| Phase 2: Core Engine | 37h | 50–70h | VWAP algorithm has edge cases; unwind logic is genuinely hard; partial fill reconciliation adds 10–15h that aren't in the spec; async Rust borrow checker friction |
| Phase 3: Connectors | 44h | 70–100h | **This is where estimates die.** Each exchange API has undocumented quirks. Binance's WebSocket has 24h connection resets. Coinbase Advanced Trade API has changed significantly. Kraken's auth signature is notoriously tricky. Rate limiting edge cases. Production debugging takes 2× estimated time. |
| Phase 4: API Server | 33h | 30–45h | Axum is well-documented; this phase is closest to estimate. WebSocket broadcaster is the complexity spike. |
| Phase 5: Testing | 38h | 50–60h | Writing good tests takes longer than writing the code. Load testing requires tooling setup (k6, Gatling). |
| **Total** | **186h** | **240–325h** | — |

**Adjusted estimate: 240–325 hours.** This is 12–16 weeks at 20 hours/week, or 6–8 weeks full-time.

**Where the spec's 200-hour estimate is optimistic:**
1. **Exchange connector debugging** is the single biggest time sink. Real exchange APIs have:
   - Undocumented rate limit headers
   - Responses that differ from official documentation
   - WebSocket message formats that changed after the docs were last updated
   - Staging/sandbox environments that don't match production
   - Auth signature implementations that are correct but the exchange rejects due to clock skew

2. **The spec assumes zero rework.** In Rust with async, the first working version of anything takes 2–3× longer than expected. The borrow checker and lifetime system impose a non-trivial overhead on architectural decisions — you may discover that the channel topology you designed doesn't compose with how Axum's state is shared, and restructure a day's work.

3. **Testing takes longer than coding.** A test for "partial fill reconciliation" requires building a mock exchange that returns partial fills in a specific order, setting up DB state, running the engine, and verifying DB state afterward. These tests are 3–5× longer to write than the feature.

### 4.2 Minimum Viable Version

The true MVP is: **detect and execute a trade using one exchange pair with two real exchanges, persist to SQLite, and have an emergency pause button.**

| MVP Component | Hours |
|--------------|-------|
| `arb-types` (core types only) | 6h |
| Binance connector only (WS + order placement) | 20h |
| Coinbase connector only (WS + order placement) | 20h |
| Detection algorithm (VWAP + threshold check) | 8h |
| Executor (concurrent legs, no unwind) | 10h |
| Risk manager (circuit breaker + daily loss limit only) | 8h |
| SQLite persistence (trades + opportunities tables only) | 6h |
| 4 REST endpoints (health, status, pause, circuit-breaker reset) | 6h |
| Config loading + graceful shutdown | 4h |
| **Total True MVP** | **~88 hours** |

88 hours is 4–5 weeks part-time. This version would prove whether detection and execution work with real exchanges, without building a full REST API, WebSocket server, third exchange, or database infrastructure.

**The spec as written is not MVP. It's the full product.**

---

## 5. Recommendations

### 5.1 Fix Before Starting: Prioritized Spec Gaps

These must be resolved before writing any code that touches them. Fix in this order:

**Priority 1 — Blocking (fix immediately, <1 week of spec work):**

1. **Write the SQL DDL.** All 6 tables with column definitions, types, constraints, foreign keys, and indexes. Without this, `arb-db` cannot be implemented.

2. **Define `simulated_fill_price()` algorithm.** Exact formula for VWAP walk. What happens at depth exhaustion? This is used in every profit calculation.

3. **Add `get_order_status()` and `get_open_orders()` to `ExchangeConnector` trait.** Crash recovery is impossible without these. Define them now before implementing any connector.

4. **Specify `FeeSchedule` struct.** What fields does it have? What is the unit of each fee? Does it vary per pair? This is needed by `arb-types` and affects profit math.

5. **Write JSON schemas for API request/response bodies.** Especially `PUT /api/v1/config` (request), `GET /api/v1/status` (response), and `GET /api/v1/trades/summary` (response). An implementer cannot write these endpoints without knowing the data shape.

**Priority 2 — Important (fix before Phase 3):**

6. **Define the unwind algorithm.** What type of order? What price? What happens if unwind fails? Where is this logic — in the executor or a separate `unwind.rs`?

7. **Specify partial fill reconciliation.** Both-partial, leg1-full/leg2-partial, leg1-partial/leg2-full, leg1-full/leg2-failed. Four cases, each with distinct handling.

8. **Add `[alerting]` config section.** At minimum, a single webhook URL for critical alerts. Operators need to know when the circuit breaker trips without actively monitoring.

9. **Write the startup reconciliation routine.** Specify what happens on restart when open orders exist on exchanges.

10. **Define `SystemEvent` enum variants** with all payloads. This is the backbone of the event system.

**Priority 3 — Clarify before implementation:**

11. **Decide: SQLite or PostgreSQL for MVP.** This changes `arb-db` significantly.
12. **Decide: 7 crates or 4 crates for MVP.** This changes the workspace structure.
13. **Clarify which config fields are hot-reloadable.** Add to §9.1.
14. **Reduce `stale_book_threshold_ms` default to 250ms** (current 1000ms is inconsistent with latency goals).
15. **Add paper trading mode** to the configuration spec.

---

### 5.2 Suggested Scope Cuts for True MVP

If the goal is **prove it works before investing 300 hours**, cut the following from v1:

| Cut | Hours Saved | Risk |
|-----|------------|------|
| Use SQLite instead of PostgreSQL | ~15h | None for MVP; migrate later |
| Kraken connector (add in v1.1) | ~15h | Start with Binance + Coinbase only |
| Full WebSocket server (use SSE for alerts) | ~8h | No dashboard anyway |
| 11 deferred REST endpoints | ~8h | Query DB directly for now |
| `arb-server` as separate crate (merge into CLI) | ~4h | Solo operator doesn't need the boundary |
| Phase 5 testing as a separate phase | Redistributed | Write tests alongside each component |
| `daily_summaries` + `audit_logs` DB tables | ~4h | Add in v1.1 |
| Prometheus metrics (move to v1.1) | ~4h | Use logging for now |
| **Total savings** | **~58h** | — |

Cutting 58 hours from 200–325 reduces MVP to **142–267 hours**. Combined with the true MVP scope (88 hours), the fastest path to "does this actually work?" is 88 hours.

---

### 5.3 What to Add That's Missing

These don't exist in the spec at all and should be added before building a production system:

| Addition | Section to Add | Complexity |
|----------|---------------|------------|
| Paper trading mode | §9 (Configuration) + §6.3 (Executor) | Low — config flag + stub executor |
| Startup reconciliation routine | New §6.6 | Medium — requires exchange trait additions |
| Per-endpoint rate limits | §8 (Exchange Connector) | Medium — affects rate_limiter.rs design |
| Balance threshold alerts | §6.4 (Risk Manager) | Low — add to pre-trade checks |
| Alerting webhook | §9 (Configuration) | Low — webhook POST on critical events |
| Withdrawal fee logging | §5 (Database Schema) | Low — one table, one endpoint |
| `SystemEvent` variant table | §4.1 (Data Models) | Low — documentation only |
| `FeeSchedule` struct definition | §4.1 (Data Models) | Low — 3–5 fields |
| SQL DDL for all tables | §5 (Database Schema) | Medium — already designed, just write it |
| `simulated_fill_price()` algorithm | §4.1 or §7.2 | Low — write the formula |
| Circuit breaker reset conditions | §6.4 (Risk Manager) | Low — add cooldown + confirmation rules |

---

## Start Here: Recommended First Actions

**If you are deciding whether to build this today:**

> The spec is 80% ready. The architecture is sound. The technology choices are correct. The two-week investment to close the spec gaps is worth it — building from an incomplete spec for financial software is how you discover the reconciliation logic is undefined at 2am when real money is stuck in a partial fill.

**If you are about to start coding:**

1. **Stop. Write the SQL DDL first.** It will clarify every other data model question.
2. **Define `simulated_fill_price()` in pseudocode.** One page. This validates the profit math before any code exists.
3. **Add `get_order_status()` and `get_open_orders()` to the connector trait.** This changes the mock implementation and all three exchange implementations. Add it now.
4. **Build the mock exchange end-to-end first.** Before touching Binance: mock exchange → detection → execution → SQLite persistence → `GET /health` returning running state. If this 88-hour path doesn't work cleanly, stop and find the bug before adding real exchange complexity.
5. **Treat Phase 2 completion as a go/no-go gate.** If the end-to-end test with the mock exchange has more than 2 unexpected design problems, the spec has gaps. Fix the spec before proceeding to real exchange connectors.

**If you are evaluating business viability:**

Read the architecture analysis first (`specs/crypto-arbitrage-analysis.md`, §4 Profitability). The system as specced targets a market dominated by HFT firms with co-location. At retail fee tiers on BTC/USDT, it is not profitable. If the goal is education or a portfolio project: **excellent spec, build it**. If the goal is profit: **target DEX-CEX arb or altcoin pairs with wider spreads instead**.

---

*End of readiness assessment.*
