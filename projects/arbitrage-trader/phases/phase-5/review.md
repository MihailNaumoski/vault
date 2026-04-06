# Phase 5 Code Review

## Decision: APPROVE

## Summary

Phase 5 delivers a solid PaperConnector, a full TUI dashboard, and production-quality startup/shutdown/health wiring. All 154 workspace tests pass, clippy is clean, and every acceptance criterion is met. The implementation is faithful to the spec with only minor cosmetic deviations (em dashes vs double hyphens in log messages). Code quality is high — proper error handling, atomic file writes, async-safe patterns, and a clear safety boundary between paper and live trading.

## Verification Results

### `cargo clippy --workspace -- -D warnings`
```
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.15s
```
**Zero warnings.**

### `cargo test --workspace`
```
test result: ok. 154 passed; 0 failed; 0 ignored
```
Breakdown by crate:
| Crate | Tests |
|-------|-------|
| arb-cli | 0 |
| arb-db | 15 |
| arb-engine | 15 (including 5 new paper tests) |
| arb-kalshi | 48 |
| arb-matcher | 25 |
| arb-polymarket | 34 |
| arb-risk | 14 |
| arb-types | 3 |

All 5 new paper tests pass: `test_paper_place_and_get_order`, `test_paper_cancel_refunds_balance`, `test_paper_rejects_insufficient_balance`, `test_paper_never_calls_real_trading`, `test_paper_list_open_orders`.

## Acceptance Criteria Audit

| AC | Description | Status | Evidence |
|----|-------------|--------|----------|
| AC-1 | Paper tests pass | PASS | 15/15 arb-engine tests pass; zero clippy warnings |
| AC-2 | Full trait implementation | PASS | All 11 methods implemented (`platform`, `list_markets`, `get_market`, `get_order_book`, `subscribe_prices`, `place_limit_order`, `cancel_order`, `get_order`, `list_open_orders`, `get_balance`, `get_positions`); `inner: Arc<dyn PredictionMarketConnector>` confirms Send+Sync+'static |
| AC-3 | Market data delegation | PASS | `list_markets`, `get_market`, `get_order_book`, `subscribe_prices` all call `self.inner.method().await` (paper.rs:89-107); `get_balance`/`get_positions` return local state (paper.rs:199-206) |
| AC-4 | No network trading calls | PASS | `place_limit_order` and `cancel_order` operate on `PaperState` only; DummyConnector panics on real `place_limit_order`/`cancel_order`/`get_order`; `test_paper_never_calls_real_trading` passes without panic |
| AC-5 | Balance tracking | PASS | Deducts `price * quantity` on place (paper.rs:115,148); refunds on cancel (paper.rs:168-169); returns `ArbError::OrderRejected` on insufficient balance (paper.rs:117-120); both tests pass |
| AC-6 | Fill simulation | PASS | `rand::random::<f64>() < fill_probability` (paper.rs:127); fill after delay checked on `get_order` (paper.rs:182); status=Filled, filled_quantity=request.quantity (paper.rs:183-184); test with 100% fill / 0ms delay passes |
| AC-7 | Order ID format | PASS | `format!("paper-{}-{}", self.platform, state.next_order_id)` (paper.rs:123); `impl std::fmt::Display for Platform` in error.rs:57-64; test asserts `starts_with("paper-")` |
| AC-8 | Open order filtering | PASS | Filters `status == OrderStatus::Open` (paper.rs:193); `test_paper_list_open_orders` returns 2 orders after 2 placements with 0% fill |
| AC-9 | TUI compiles | PASS | `cargo clippy --workspace -- -D warnings` succeeds with zero warnings |
| AC-10 | TuiState complete | PASS | All 11 required fields present: `mode`, `engine_running`, `started_at`, `open_orders`, `positions`, `daily_pnl`, `total_exposure`, `unhedged_exposure`, `daily_loss`, `unwind_rate`, `pair_count`; `new()` initializes with empty vecs, zero decimals, None for pnl |
| AC-11 | Data refresh queries | PASS | `db.list_orders_by_status("open")` (tui.rs:58); `db.list_open_positions()` (tui.rs:61); `db.get_daily_pnl(today)` (tui.rs:64); `rm.exposure().total_exposure()` / `.unhedged_exposure()` / `.daily_loss()` / `.unwind_rate_pct()` (tui.rs:70-73); errors silently ignored via `if let Ok(...)` |
| AC-12 | 5-section layout | PASS | Status bar (tui.rs:99-111), Open orders table with 7 columns (tui.rs:114-136), Positions table with 6 columns (tui.rs:139-159), P&L summary (tui.rs:162-175), Key bindings (tui.rs:178-186); Layout uses `frame.area()` for dynamic sizing |
| AC-13 | Color coding | PASS | Status: `Color::Green` when running, `Color::Red` when paused (tui.rs:93); Mode: `Color::Yellow` for PAPER, `Color::Red` for LIVE (tui.rs:94) |
| AC-14 | 2-second refresh | PASS | `refresh_interval = Duration::from_secs(2)` (tui.rs:207); conditional `if last_refresh.elapsed() >= refresh_interval` (tui.rs:210); tick rate 250ms for input only |
| AC-15 | Keyboard input | PASS | `q` → break/exit (tui.rs:223); `p` → `engine_running = false` (tui.rs:225); `r` → `engine_running = true` (tui.rs:229); guard on `KeyEventKind::Press` only (tui.rs:221) |
| AC-16 | Terminal restoration | PASS | Normal exit: `disable_raw_mode()` + `LeaveAlternateScreen` (tui.rs:240-241); Panic: hook installed in main.rs:351-355 restores before calling original hook |
| AC-17 | Repository import | PASS | `use arb_db::Repository;` at tui.rs:2; compiles and resolves trait methods |
| AC-18 | Startup banner | PASS | Paper: `info!("PAPER TRADING — no real orders will be placed")` (main.rs:301); Live: `warn!("LIVE TRADING — real money at risk!")` (main.rs:303); banner includes system name and mode (main.rs:297-305) |
| AC-19 | Arc wrapping | PASS | `Arc::new(arb_db::SqliteRepository::new(...))` (main.rs:309); `Arc::new(parking_lot::RwLock::new(arb_risk::RiskManager::new(...)))` (main.rs:314); both before Ctrl+C handler and TUI |
| AC-20 | Ctrl+C handler | PASS | Spawned as tokio task (main.rs:328); sets `AtomicBool` with `SeqCst` ordering (main.rs:331); logs shutdown message (main.rs:330); health writer checks flag (main.rs:343); headless loop checks flag (main.rs:364) |
| AC-21 | Health file | PASS | Path: `data/health.json` (main.rs:396); every 30s (main.rs:340); contains status, mode, timestamp (RFC 3339), open_orders, open_positions, total_exposure, daily_loss (main.rs:385-393); atomic write via tmp+rename (main.rs:400-403); `create_dir_all` (main.rs:398) |
| AC-22 | TUI mode selection | PASS | Logic: `if args.tui \|\| (!args.headless && !args.paper)` (main.rs:349); `--tui` → TUI; `--headless` → no TUI; default → TUI; `--paper` alone → headless; `--tui --headless` → TUI wins |
| AC-23 | Headless mode | PASS | 1-second sleep loop (main.rs:363); checks shutdown flag (main.rs:364); no terminal manipulation; logs "Running headless — press Ctrl+C to stop" (main.rs:361) |
| AC-24 | Panic hook | PASS | `std::panic::set_hook` before TUI (main.rs:351); disables raw mode (main.rs:353); leaves alternate screen (main.rs:354); calls original hook (main.rs:355) |
| AC-25 | Imports present | PASS | `std::sync::Arc` (main.rs:8); `std::time::Duration` (main.rs:9); `tracing::warn` (main.rs:6); `mod tui;` (main.rs:14); compiles without errors |
| AC-26 | Workspace clean | PASS | `cargo clippy --workspace -- -D warnings`: zero warnings; `cargo test --workspace`: 154 passed, 0 failed |

## Findings

### Critical (blocks approval)

None.

### Major (should fix before merge)

None.

### Minor (nice to have)

1. **`Display for Platform` in `error.rs` instead of `lib.rs`**: The spec's pre-implementation fix #1 says to add `impl Display for Platform` to `lib.rs`, but it was placed in `error.rs`. This works due to Rust's orphan rules (same crate) but is unusual — `Display` is a general-purpose trait and `Platform` lives in `lib.rs`. Consider moving it to `lib.rs` for discoverability.

2. **`daily_pnl` not reset on `Ok(None)`**: In `refresh_state` (tui.rs:64), `if let Ok(Some(pnl)) = ...` means when the DB returns `Ok(None)` (e.g., new day with no trades yet), the previous value persists. Should consider adding an `else if let Ok(None)` branch to reset to `None`:
   ```rust
   match db.get_daily_pnl(Utc::now().date_naive()).await {
       Ok(pnl) => state.daily_pnl = pnl,
       Err(_) => {} // keep old value on error
   }
   ```

3. **Blocking `parking_lot::Mutex::lock()` in async context** (paper.rs): `self.state.lock()` is a synchronous lock held across no `.await` points, so it's safe in practice. However, `tokio::sync::Mutex` would be more idiomatic for async code. Acceptable here since critical sections are micro-short (hash map lookups/inserts).

4. **Log message formatting**: Spec uses `--` (double hyphen) for separators ("PAPER TRADING -- no real orders"), code uses `—` (em dash). Purely cosmetic, but deviates from spec literal text.

5. **`write_health_file` has its own `use arb_db::Repository;`** (main.rs:377): The import is scoped inside the function body. While this works, it's duplicated from tui.rs's module-level import. If more functions need it, consider moving to the top of main.rs.

6. **No `--tui` / `--headless` mutual exclusion via clap**: Both flags can be specified simultaneously. The code handles it gracefully (TUI wins), but `clap`'s `conflicts_with` attribute could prevent user confusion.

### Notes

- The PaperConnector safety boundary is well-designed: DummyConnector panics on real trading calls, and `test_paper_never_calls_real_trading` provides strong evidence that the boundary holds.
- Edge cases from the spec are handled correctly: exact-balance orders succeed (`<` not `<=`), cancel-already-cancelled is a no-op, non-existent order returns `ArbError::Other`, long IDs are truncated.
- The `PaperState::total_pnl()` method is defined but unused — it will be needed in future phases for P&L tracking.
- Five TODO comments remain in main.rs (connector init, engine init, pair loading, price cache, engine run) — these are expected for Phase 5 which wires the skeleton without full engine integration.
- Test count increased from ~135 (pre-phase-5) to 154, exceeding the spec's "140+" target.
