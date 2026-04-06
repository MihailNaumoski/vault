# Phase 5 Architecture Plan — TUI + Paper Trading + Production Readiness

## Overview

Phase 5 adds three capabilities to the arbitrage-trader system:
1. **Paper Trading Connector (5-A)** — wraps real connectors for market data, simulates all trading locally
2. **TUI Dashboard (5-B)** — ratatui-based terminal UI showing orders, positions, P&L, risk metrics
3. **Startup Wiring + Shutdown + Health (5-C)** — ties everything together with Ctrl+C handling, health file, mode selection

After Phase 5, the system is ready for live paper trading against real price feeds.

---

## Component Architecture

### 5-A: Paper Trading Connector

**New file:** `crates/arb-engine/src/paper.rs`
**Modified files:**
- `crates/arb-engine/src/lib.rs` — add `pub mod paper;`
- `crates/arb-engine/Cargo.toml` — add `rand = { workspace = true }`

**Architecture:**
- `PaperState` — internal mutable state (orders hashmap, balance, fill settings)
- `PaperConnector` — implements `PredictionMarketConnector` trait
  - Holds an `Arc<dyn PredictionMarketConnector>` (the real connector) + `Arc<Mutex<PaperState>>`
  - **Read methods** (list_markets, get_market, get_order_book, subscribe_prices) — delegate to inner real connector
  - **Write methods** (place_limit_order, cancel_order) — handled entirely locally, ZERO network calls
  - **Query methods** (get_order, list_open_orders, get_balance, get_positions) — served from local state
- `PaperOrder` — tracks each simulated order with fill probability and delay

**Safety boundary:** The DummyConnector test struct panics if any real trading method is called, proving the paper connector never leaks trading calls to the network.

**Data flow:**
```
Real market data  ─→  PaperConnector.inner  ─→  Engine reads prices
                      PaperConnector.state  ←─  Engine places simulated orders
```

### 5-B: TUI Dashboard

**New file:** `crates/arb-cli/src/tui.rs`
**Modified files:**
- `crates/arb-cli/Cargo.toml` — add `ratatui`, `crossterm`, `parking_lot` deps

**Architecture:**
- `TuiState` — snapshot struct holding all data the UI renders (decoupled from DB/risk manager)
- `refresh_state()` — async function that queries DB + risk manager and populates TuiState
- `draw()` — pure rendering function, takes a `Frame` and `TuiState`, draws 5 layout sections:
  1. Status bar (mode, running status, uptime, exposure, pair count)
  2. Open orders table (platform, market, side, price, qty, status, age)
  3. Positions table (pair, poly leg, kalshi leg, hedged qty, profit, status)
  4. P&L summary (daily net, trades, daily loss, unhedged exposure, unwind rate)
  5. Key bindings footer
- `run_tui()` — async event loop:
  - 250ms tick rate for UI responsiveness
  - 2-second refresh interval for DB/risk data
  - Handles keyboard input (q=quit, p=pause, r=resume)
  - Restores terminal on exit (raw mode off, leave alternate screen)

**Data flow:**
```
SqliteRepository  ──┐
                    ├──→ refresh_state() ──→ TuiState ──→ draw() ──→ terminal
RiskManager (RwLock)┘
```

### 5-C: Startup Wiring + Shutdown + Health

**Modified file:** `crates/arb-cli/src/main.rs`
**Modified files:**
- `crates/arb-cli/Cargo.toml` — add `serde_json`, `parking_lot`, `rand` deps

**Architecture:**
- Replaces the bottom half of `main()` (after match mode), keeps config structs and init_tracing
- Adds `mod tui;` declaration
- Startup sequence:
  1. Config load + tracing init (unchanged)
  2. Match-only mode check (unchanged)
  3. Mode banner (PAPER vs LIVE)
  4. DB init with `Arc` wrapping
  5. Risk manager init with `Arc<RwLock<_>>` wrapping
  6. TODO placeholders for connector and engine init
  7. Ctrl+C handler (spawned tokio task, sets AtomicBool flag)
  8. Health file writer (spawned tokio task, 30-second interval)
  9. TUI or headless mode selection
- `write_health_file()` — writes `data/health.json` atomically (write to .tmp, rename)
- Panic hook — restores terminal raw mode before panic output

---

## Dependency Order

```
5-A (Paper Connector)  →  5-B (TUI Dashboard)  →  5-C (Startup Wiring)
     standalone              needs TUI deps          ties everything together
     arb-engine only         arb-cli new file        arb-cli main.rs rewrite
```

5-A is fully standalone (only touches arb-engine). 5-B adds a new module to arb-cli. 5-C modifies main.rs to wire them together.

---

## File Change List

### New Files
| File | Purpose |
|------|---------|
| `crates/arb-engine/src/paper.rs` | PaperConnector + PaperState + tests |
| `crates/arb-cli/src/tui.rs` | TUI dashboard module |

### Modified Files
| File | Changes Needed |
|------|---------------|
| `crates/arb-types/src/lib.rs` | Add `impl Display for Platform` (see Deviation #1) |
| `crates/arb-engine/src/lib.rs` | Add `pub mod paper;` |
| `crates/arb-engine/Cargo.toml` | Add `rand = { workspace = true }` |
| `crates/arb-cli/Cargo.toml` | Add `ratatui`, `crossterm`, `serde_json`, `parking_lot`, `rand` (all workspace) |
| `crates/arb-cli/src/main.rs` | Add `mod tui;`, rewrite `main()` bottom half, add `write_health_file()`, add imports |

---

## Confirmed Deviations from Build Prompt

### Deviation #1: `Platform` enum lacks `Display` impl — MUST FIX
- **Location:** `crates/arb-types/src/lib.rs` lines 24-29
- **Impact:** `format!("paper-{}-{}", self.platform, ...)` in paper.rs line 190 and `%self.platform` in tracing macros (lines 217-221) will fail to compile
- **Fix:** Add to `crates/arb-types/src/lib.rs`:
```rust
impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Polymarket => write!(f, "polymarket"),
            Platform::Kalshi => write!(f, "kalshi"),
        }
    }
}
```

### Deviation #2: `OrderBook::default()` — NOT A PROBLEM
- **Status:** Already has manual `Default` impl at `order.rs` lines 100-109
- **No fix needed**

### Deviation #3: `LimitOrderRequest` and `OrderResponse` lack `Clone` — NOT A PROBLEM
- **Status:** Both already derive `Clone` (order.rs lines 57 and 65)
- **No fix needed**

### Deviation #4: TUI needs `Repository` trait in scope — MUST FIX
- **Location:** `crates/arb-cli/src/tui.rs`
- **Impact:** The TUI code calls `db.list_orders_by_status(...)`, `db.list_open_positions()`, `db.get_daily_pnl(...)` on `&SqliteRepository`. These methods are defined on the `Repository` trait, not inherent methods on `SqliteRepository`.
- **Fix:** Add `use arb_db::Repository;` to the TUI module imports (or change the import to `use arb_db::{Repository, SqliteRepository};`)

### Deviation #5: `rand` not in arb-engine Cargo.toml — MUST FIX
- **Impact:** `rand::random::<f64>()` in paper.rs line 194 won't compile
- **Fix:** Add `rand = { workspace = true }` to `crates/arb-engine/Cargo.toml` `[dependencies]`
- **Verified:** `rand = "0.8"` exists in workspace deps (root Cargo.toml line 51)

### Deviation #6: Missing deps in arb-cli Cargo.toml — MUST FIX
- **Current deps:** arb-types, arb-db, arb-risk, arb-engine, arb-matcher, chrono, rust_decimal_macros, tokio, config, dotenvy, tracing, tracing-subscriber, clap, anyhow, serde, rust_decimal
- **Missing:** `ratatui`, `crossterm`, `serde_json`, `parking_lot`, `rand`
- **Fix:** Add all five as `{ workspace = true }` entries
- **Verified:** All five exist in workspace deps (root Cargo.toml)

### Deviation #7: Additional missing imports in 5-C main.rs — MUST FIX
- `std::sync::Arc` — needed for `Arc::new(db)` and `Arc::new(RwLock::new(...))`
- `std::time::Duration` — needed for `Duration::from_secs(30)` and `Duration::from_secs(1)`
- `crossterm` — needed for panic hook terminal restore (lines 764-765)
- `tracing::warn` — needed for LIVE mode warning (line 713)
- The build prompt code snippet doesn't show full imports since it says "keep everything above `#[tokio::main]`" — the implementer must add these imports to the existing import block

---

## Cargo.toml Changes Summary

### `crates/arb-engine/Cargo.toml`
```toml
# Add to [dependencies]:
rand = { workspace = true }
```

### `crates/arb-cli/Cargo.toml`
```toml
# Add to [dependencies]:
ratatui = { workspace = true }
crossterm = { workspace = true }
serde_json = { workspace = true }
parking_lot = { workspace = true }
rand = { workspace = true }
```

### Root `Cargo.toml`
No changes needed — all workspace dependencies already declared.

---

## API Contract Verification

### PredictionMarketConnector trait (11 methods)
All 11 methods match between the trait definition in `arb-types/src/lib.rs` and the PaperConnector implementation in the build prompt. Verified:
- `platform()` -> `Platform`
- `list_markets(MarketStatus)` -> `Result<Vec<Market>, ArbError>`
- `get_market(&str)` -> `Result<Market, ArbError>`
- `get_order_book(&str)` -> `Result<OrderBook, ArbError>`
- `subscribe_prices(&[String], mpsc::Sender<PriceUpdate>)` -> `Result<SubHandle, ArbError>`
- `place_limit_order(&LimitOrderRequest)` -> `Result<OrderResponse, ArbError>`
- `cancel_order(&str)` -> `Result<(), ArbError>`
- `get_order(&str)` -> `Result<OrderResponse, ArbError>`
- `list_open_orders()` -> `Result<Vec<OrderResponse>, ArbError>`
- `get_balance()` -> `Result<Decimal, ArbError>`
- `get_positions()` -> `Result<Vec<PlatformPosition>, ArbError>`

### Repository trait DB methods (used by TUI)
All three methods exist on the trait and are implemented for SqliteRepository:
- `list_orders_by_status(&str)` -> `anyhow::Result<Vec<OrderRow>>` (line 34/395 of repo.rs)
- `list_open_positions()` -> `anyhow::Result<Vec<PositionRow>>` (line 48/463)
- `get_daily_pnl(NaiveDate)` -> `anyhow::Result<Option<DailyPnlRow>>` (line 66/543)

### ExposureTracker getters (used by TUI + health file)
All four methods confirmed on `ExposureTracker`:
- `total_exposure()` -> `Decimal`
- `unhedged_exposure()` -> `Decimal`
- `daily_loss()` -> `Decimal`
- `unwind_rate_pct()` -> `Decimal`

### RiskManager access pattern
- `exposure() -> &ExposureTracker` (line 71 of manager.rs) — read access via `parking_lot::RwLock::read()`
- `set_engine_running(&mut self, bool)` (line 67) — write access via `parking_lot::RwLock::write()`

### DB model types (used by TUI rendering)
All fields referenced in TUI draw code verified against `arb-db/src/models.rs`:
- `OrderRow`: `platform`, `market_id`, `side`, `price`, `quantity`, `status`, `placed_at` — all present
- `PositionRow`: `pair_id`, `poly_side`, `poly_quantity`, `poly_avg_price`, `kalshi_side`, `kalshi_quantity`, `kalshi_avg_price`, `hedged_quantity`, `guaranteed_profit`, `status` — all present
- `DailyPnlRow`: `net_profit`, `trades_executed` — all present

### ArbError variants (used by paper connector)
- `OrderRejected { platform: Platform, reason: String }` — used for insufficient balance rejection
- `Other(String)` — used for "paper order not found"
Both confirmed in `arb-types/src/error.rs`

---

## Risk Assessment

### Low Risk
- 5-A paper connector is straightforward: wraps trait, simulates locally, has comprehensive tests
- 5-B TUI is a read-only dashboard, cannot affect engine behavior
- All workspace deps already declared in root Cargo.toml

### Medium Risk
- 5-C startup wiring has TODO placeholders for connector/engine init — this is intentional (Phase 5 sets up the scaffold; actual connector init comes when platform connectors are production-ready)
- The TUI pause/resume buttons are placeholders (`// TODO: send pause signal to engine`) — functional but incomplete
- Health file writes to `data/health.json` relative to CWD — must ensure working directory is project root

### Blocking Issues (must fix before implementation)
1. **Platform Display impl** — without this, paper.rs won't compile (tracing `%self.platform` requires Display)
2. **Repository trait import in TUI** — without this, DB method calls won't resolve
3. **Missing Cargo.toml deps** — without these, nothing compiles

---

## Implementation Sequence Recommendation

1. **Fix Platform Display** in arb-types first (unblocks 5-A compilation)
2. **Build 5-A** (paper.rs + engine lib.rs + engine Cargo.toml) — run `cargo test -p arb-engine`
3. **Build 5-B** (tui.rs + cli Cargo.toml deps) — run `cargo check -p arb-cli`
4. **Build 5-C** (main.rs rewrite + remaining cli Cargo.toml deps) — run `cargo build --workspace`
5. **Full verification** — `cargo test --workspace && cargo clippy --workspace -- -D warnings`
