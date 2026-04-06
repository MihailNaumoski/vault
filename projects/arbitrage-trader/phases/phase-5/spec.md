# Phase 5 Specification — TUI + Paper Trading + Production Readiness

## Sub-Prompt 5-A: Paper Trading Connector

### Acceptance Criteria

**AC-1: PaperConnector compiles and passes all tests**
- `cargo test -p arb-engine` passes with all existing tests plus 5 new paper tests
- `cargo clippy -p arb-engine -- -D warnings` produces zero warnings
- **Pass:** Zero test failures, zero clippy warnings
- **Fail:** Any test failure or clippy warning

**AC-2: PaperConnector implements full PredictionMarketConnector trait**
- All 11 trait methods are implemented
- The struct is `Send + Sync + 'static` (required by trait bound)
- **Pass:** `cargo check -p arb-engine` succeeds, trait object can be created as `Arc<dyn PredictionMarketConnector>`
- **Fail:** Compile error on trait implementation

**AC-3: Market data methods delegate to inner connector**
- `list_markets`, `get_market`, `get_order_book`, `subscribe_prices` call through to `self.inner`
- `get_balance` and `get_positions` return local state (not delegated for trading safety)
- **Pass:** DummyConnector test shows market data methods work (no panic)
- **Fail:** Any market data method fails or returns incorrect data

**AC-4: Trading methods never touch the network**
- `place_limit_order` and `cancel_order` operate entirely on `PaperState`
- DummyConnector panics if real `place_limit_order`, `cancel_order`, or `get_order` are called
- **Pass:** `test_paper_never_calls_real_trading` passes (no panics)
- **Fail:** Any panic from DummyConnector during paper trading operations

**AC-5: Balance tracking is correct**
- `place_limit_order` deducts `price * quantity` from balance immediately
- `cancel_order` refunds `price * quantity` to balance
- Insufficient balance returns `ArbError::OrderRejected`
- **Pass:** `test_paper_cancel_refunds_balance` and `test_paper_rejects_insufficient_balance` pass
- **Fail:** Balance calculations incorrect or wrong error variant returned

**AC-6: Fill simulation works correctly**
- Orders fill based on `fill_probability` (0.0 = never, 1.0 = always)
- Fill occurs after `fill_delay_ms` has elapsed (checked on `get_order` call)
- Filled orders have `status = OrderStatus::Filled` and `filled_quantity = request.quantity`
- **Pass:** `test_paper_place_and_get_order` passes (100% fill, 0ms delay)
- **Fail:** Fill timing or status transitions incorrect

**AC-7: Order ID format is correct**
- Order IDs follow pattern `paper-{platform}-{sequence}` (e.g., `paper-polymarket-1`)
- Requires `Platform` to implement `Display` trait
- **Pass:** `assert!(resp.order_id.starts_with("paper-"))` in test passes
- **Fail:** Order ID format doesn't match or `Display` not implemented

**AC-8: list_open_orders returns only open orders**
- Filters to orders with `status == OrderStatus::Open`
- Cancelled and filled orders excluded
- **Pass:** `test_paper_list_open_orders` passes (2 open orders after 2 placements)
- **Fail:** Wrong count or includes non-open orders

### Edge Cases
- Place order with exactly the remaining balance (should succeed)
- Cancel an already-cancelled order (should be a no-op, not error)
- Get a non-existent order (should return `ArbError::Other`)
- Place order when balance is zero (should return `OrderRejected`)
- Multiple concurrent paper connectors for different platforms (each has independent state)

---

## Sub-Prompt 5-B: TUI Dashboard

### Acceptance Criteria

**AC-9: TUI module compiles**
- `cargo check -p arb-cli` succeeds after adding tui.rs and dependencies
- `cargo clippy -p arb-cli -- -D warnings` produces zero warnings
- **Pass:** Clean compilation
- **Fail:** Any compile error or clippy warning

**AC-10: TuiState snapshot struct is complete**
- Contains: mode, engine_running, started_at, open_orders, positions, daily_pnl, total_exposure, unhedged_exposure, daily_loss, unwind_rate, pair_count
- `TuiState::new(mode)` initializes all fields with sensible defaults (empty vecs, zero decimals)
- **Pass:** Struct compiles and `new()` returns valid state
- **Fail:** Missing fields or incorrect defaults

**AC-11: refresh_state queries DB and risk manager correctly**
- Calls `db.list_orders_by_status("open")` -> populates `open_orders`
- Calls `db.list_open_positions()` -> populates `positions`
- Calls `db.get_daily_pnl(today)` -> populates `daily_pnl`
- Reads `rm.exposure().total_exposure()`, `.unhedged_exposure()`, `.daily_loss()`, `.unwind_rate_pct()`
- DB errors are silently ignored (TUI should never crash on DB failure)
- **Pass:** All fields populated when DB and risk manager have data
- **Fail:** Missing query, wrong field mapping, or crash on DB error

**AC-12: Dashboard renders 5 layout sections**
- Section 1: Status bar with mode (PAPER/LIVE), running status, uptime, exposure, pair count
- Section 2: Open orders table with 7 columns (Platform, Market, Side, Price, Qty, Status, Age)
- Section 3: Positions table with 6 columns (Pair, Poly, Kalshi, Hedged, Profit, Status)
- Section 4: P&L summary with daily net, trades count, daily loss, unhedged exposure, unwind rate
- Section 5: Key bindings (q:quit, p:pause, r:resume)
- **Pass:** All 5 sections render without panic; layout fills terminal area
- **Fail:** Missing section, panic during render, or layout overflow

**AC-13: Color coding is correct**
- Status: Green when running, Red when paused
- Mode: Yellow for PAPER, Red for LIVE
- **Pass:** Visual inspection shows correct colors
- **Fail:** Wrong color assignments

**AC-14: Data refresh interval is 2 seconds**
- Data is not refreshed on every tick (250ms) — only every 2 seconds
- This prevents DB query overhead
- **Pass:** `refresh_interval = Duration::from_secs(2)` and conditional refresh based on elapsed time
- **Fail:** Refreshes too frequently or not at all

**AC-15: Keyboard input handling**
- `q` key: exits the TUI loop cleanly
- `p` key: sets `engine_running = false`
- `r` key: sets `engine_running = true`
- Only responds to `KeyEventKind::Press` (not release/repeat)
- **Pass:** Keys trigger correct state changes
- **Fail:** Wrong key mapping or responds to non-press events

**AC-16: Terminal restoration on exit**
- On normal exit: disables raw mode, leaves alternate screen
- On panic: panic hook restores terminal before printing panic info
- **Pass:** Terminal is usable after TUI exit (both normal and panic paths)
- **Fail:** Terminal left in raw mode or alternate screen after exit

**AC-17: Repository trait import is present**
- `use arb_db::Repository;` must be in scope for trait method access on `SqliteRepository`
- **Pass:** TUI compiles and DB queries resolve
- **Fail:** "method not found" compile error

### Edge Cases
- Terminal resize during rendering (ratatui handles this via `frame.area()`)
- DB returns empty results (empty tables should render correctly)
- Very long market IDs (truncated to 20 chars with "...")
- Very long pair IDs (truncated to 8 chars)
- No daily P&L row for today (shows "0" defaults)
- Negative P&L values (should display with minus sign)

---

## Sub-Prompt 5-C: Startup Wiring + Shutdown + Health

### Acceptance Criteria

**AC-18: Startup banner displays correctly**
- Paper mode shows "PAPER TRADING -- no real orders will be placed"
- Live mode shows "LIVE TRADING -- real money at risk!" with `warn!` level
- Banner includes system name and mode
- **Pass:** `cargo run -- --paper --headless` shows paper banner; default shows live warning
- **Fail:** Wrong banner for mode or missing mode indicator

**AC-19: DB and risk manager initialized with Arc wrapping**
- DB is `Arc<SqliteRepository>` (shareable across TUI and health writer)
- Risk manager is `Arc<RwLock<RiskManager>>` (shared mutable access)
- Both initialized before Ctrl+C handler and TUI
- **Pass:** Compiles and runtime initialization succeeds
- **Fail:** Type mismatch or initialization order wrong

**AC-20: Ctrl+C handler works**
- Spawned as tokio task
- Sets `AtomicBool` flag on Ctrl+C
- Logs "Ctrl+C received -- initiating shutdown"
- Health writer and headless loop check this flag to exit
- **Pass:** `--paper --headless` responds to Ctrl+C with clean shutdown
- **Fail:** Process doesn't respond to Ctrl+C or hangs

**AC-21: Health file written correctly**
- Path: `data/health.json`
- Written every 30 seconds
- Contains: status, mode, timestamp (RFC 3339), open_orders count, open_positions count, total_exposure, daily_loss
- Written atomically (write to `data/health.tmp`, rename to `data/health.json`)
- Parent directory created if missing
- **Pass:** After 30 seconds, `data/health.json` exists with valid JSON containing all fields
- **Fail:** File missing, invalid JSON, missing fields, or non-atomic write

**AC-22: TUI mode selection logic**
- `--tui` flag: always shows TUI
- `--headless` flag: never shows TUI
- Default (no flag, not paper): shows TUI
- `--paper` without `--tui`: runs headless (paper mode defaults to headless for scripting)
- **Pass:** Mode selection matches: `if args.tui || (!args.headless && !args.paper)`
- **Fail:** Wrong mode for given flag combination

**AC-23: Headless mode works**
- Loops with 1-second sleep, checking shutdown flag
- No terminal manipulation
- Logs "Running headless -- press Ctrl+C to stop"
- **Pass:** `--paper --headless` runs without terminal issues, responds to Ctrl+C
- **Fail:** Headless mode manipulates terminal or doesn't respond to shutdown

**AC-24: Panic hook restores terminal**
- `std::panic::set_hook` installed before TUI starts
- Hook disables raw mode and leaves alternate screen before calling original hook
- **Pass:** A panic during TUI rendering leaves terminal in usable state
- **Fail:** Panic leaves terminal in raw mode

**AC-25: All new imports present in main.rs**
- `std::sync::Arc` for `Arc::new()`
- `std::time::Duration` for sleep/interval durations
- `tracing::warn` for live mode warning
- `mod tui;` declaration
- **Pass:** Compiles without "unresolved import" errors
- **Fail:** Missing import causes compile error

**AC-26: Workspace builds cleanly**
- `cargo build --workspace` succeeds
- `cargo test --workspace` passes (all existing 130+ tests plus new paper tests)
- `cargo clippy --workspace -- -D warnings` is clean
- **Pass:** All three commands succeed with zero failures/warnings
- **Fail:** Any build failure, test failure, or clippy warning

### Edge Cases
- `data/` directory doesn't exist (health writer creates it)
- Health file write fails (silently ignored, should not crash)
- Ctrl+C pressed multiple times rapidly (AtomicBool is idempotent)
- Ctrl+C during startup before TUI is initialized (shutdown flag prevents TUI from starting)
- TUI and health writer accessing DB concurrently (both use Arc, no conflict)
- `--tui --headless` both specified (TUI wins per the conditional logic)

---

## Pre-Implementation Fixes Required

Before implementing any sub-prompt, these fixes must be applied:

| # | Fix | File | Blocking |
|---|-----|------|----------|
| 1 | Add `impl Display for Platform` | `crates/arb-types/src/lib.rs` | 5-A |
| 2 | Add `use arb_db::Repository;` to TUI imports | `crates/arb-cli/src/tui.rs` | 5-B |
| 3 | Add `rand` to arb-engine deps | `crates/arb-engine/Cargo.toml` | 5-A |
| 4 | Add `ratatui`, `crossterm`, `serde_json`, `parking_lot`, `rand` to arb-cli deps | `crates/arb-cli/Cargo.toml` | 5-B, 5-C |
| 5 | Add missing std imports to main.rs | `crates/arb-cli/src/main.rs` | 5-C |

---

## Test Verification Commands

```bash
# After 5-A:
cargo test -p arb-engine -- paper       # Run only paper tests
cargo test -p arb-engine                # All engine tests still pass
cargo clippy -p arb-engine -- -D warnings

# After 5-B:
cargo check -p arb-cli                  # TUI compiles
cargo clippy -p arb-cli -- -D warnings

# After 5-C:
cargo build --workspace                 # Full workspace builds
cargo run -- --help                     # CLI shows all flags
cargo run -- --paper --headless &       # Starts, prints banner, writes health
sleep 35 && cat data/health.json        # Health file exists with valid JSON
kill %1                                 # Clean shutdown

# Full verification:
cargo test --workspace                  # All 140+ tests pass
cargo clippy --workspace -- -D warnings # Zero warnings
```

---

## Acceptance Criteria Summary

| AC | Sub-Prompt | Description | Verification |
|----|-----------|-------------|-------------|
| AC-1 | 5-A | Paper tests pass | `cargo test -p arb-engine` |
| AC-2 | 5-A | Full trait implementation | `cargo check -p arb-engine` |
| AC-3 | 5-A | Market data delegation | DummyConnector test |
| AC-4 | 5-A | No network trading calls | Panic safety test |
| AC-5 | 5-A | Balance tracking | Cancel refund + insufficient balance tests |
| AC-6 | 5-A | Fill simulation | Place and get order test |
| AC-7 | 5-A | Order ID format | String prefix assertion |
| AC-8 | 5-A | Open order filtering | List open orders test |
| AC-9 | 5-B | TUI compiles | `cargo check -p arb-cli` |
| AC-10 | 5-B | TuiState complete | Struct compilation |
| AC-11 | 5-B | Data refresh | Code review of refresh_state |
| AC-12 | 5-B | 5-section layout | Code review of draw() |
| AC-13 | 5-B | Color coding | Visual inspection |
| AC-14 | 5-B | 2-second refresh | Code review |
| AC-15 | 5-B | Keyboard input | Code review + manual test |
| AC-16 | 5-B | Terminal restoration | Manual test |
| AC-17 | 5-B | Repository import | `cargo check` |
| AC-18 | 5-C | Startup banner | `cargo run -- --paper --headless` |
| AC-19 | 5-C | Arc wrapping | `cargo check` |
| AC-20 | 5-C | Ctrl+C handler | Manual test |
| AC-21 | 5-C | Health file | Check `data/health.json` after 30s |
| AC-22 | 5-C | TUI mode selection | Flag combination testing |
| AC-23 | 5-C | Headless mode | `--paper --headless` test |
| AC-24 | 5-C | Panic hook | Deliberate panic test |
| AC-25 | 5-C | Imports present | `cargo check` |
| AC-26 | 5-C | Workspace clean | `cargo test --workspace && cargo clippy --workspace` |
