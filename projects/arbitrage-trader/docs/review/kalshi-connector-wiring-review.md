# Kalshi Connector Wiring — Code Review

**Reviewer:** Code Reviewer (Engineering Team)  
**Date:** 2026-04-08  
**Scope:** Cross-team review of Trading team output (Rust Engine Dev)  
**Rework Cycle:** 1  

## Decision: PASS

> Both blocking findings from Rework Cycle 0 have been resolved. Polymarket credential loading now uses `.context()?` matching the Kalshi pattern, and `std::mem::forget` has been replaced with proper handle storage. No new issues introduced. Three MINOR and two NIT findings remain from the initial review — all non-blocking.

## Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 1 | RESOLVED in Rework 1 |
| MAJOR    | 1 | RESOLVED in Rework 1 |
| MINOR    | 3 | Open (non-blocking) |
| NIT      | 2 | Open (non-blocking) |
| **Total** | **7** | **0 blocking** |

**Blocking findings:** 0

## Resolved Findings (Rework Cycle 1)

### [CRITICAL] RESOLVED — Polymarket env vars use `.context()?` — `main.rs:380-383,391`

**Before (Rework 0):**
```rust
api_key: std::env::var("POLY_API_KEY").expect("POLY_API_KEY not set"),
```

**After (Rework 1):**
```rust
api_key: std::env::var("POLY_API_KEY").context("POLY_API_KEY env var not set")?,
secret: std::env::var("POLY_API_SECRET").context("POLY_API_SECRET env var not set")?,
passphrase: std::env::var("POLY_PASSPHRASE").context("POLY_PASSPHRASE env var not set")?,
private_key: std::env::var("POLY_PRIVATE_KEY").context("POLY_PRIVATE_KEY env var not set")?,
```

And the connector creation at line 391:
```rust
arb_polymarket::PolymarketConnector::new(poly_config)
    .context("failed to create Polymarket connector")?,
```

**Verification:** All 5 `.expect()` calls replaced with `.context()?`. Grep confirms zero `.expect()` on env var reads. The two remaining `.expect()` calls in the file are:
- Line 129: `File::create("data/arb.log").expect(...)` — pre-trading init, acceptable
- Line 821: `ctrl_c().await.expect(...)` — signal handler setup, acceptable

Polymarket credential handling now matches the Kalshi pattern exactly (lines 411-414).

---

### [MAJOR] RESOLVED — `std::mem::forget(handle)` replaced with Vec storage — `main.rs:693,705,771`

**Before (Rework 0):**
```rust
std::mem::forget(handle);  // Polymarket WS
std::mem::forget(handle);  // Kalshi WS
```

**After (Rework 1):**
```rust
let mut _ws_handles: Vec<arb_types::SubHandle> = Vec::new();  // line 693
// ...
_ws_handles.push(handle);  // line 705 (Polymarket WS)
// ...
_ws_handles.push(handle);  // line 771 (Kalshi WS)
```

**Verification:** Grep confirms zero `std::mem::forget` calls remain (only a comment referencing the old pattern at line 692). The `_ws_handles` Vec lives for the full duration of `main()` — handles are dropped when `main()` returns after the shutdown sequence completes at line 861. Destructors will fire on clean exit.

---

## Open Findings (Non-Blocking)

### [MINOR] TUI activation logic may unintentionally start TUI — `main.rs:169,841`

```rust
let tui_active = args.tui || (!args.headless && !args.paper);  // line 169
if args.tui || (!args.headless && !args.paper) {               // line 841
```

When running `--paper-both`, TUI activates because `args.paper` is false. Inconsistent with `--paper` which runs headless. Consider adding `&& !args.paper_both` to both conditions.

**Status:** Open — non-blocking, cosmetic UX issue.

---

### [MINOR] `--match` mode still uses hardcoded sample markets — `main.rs:176-330`

The `--match` demo mode constructs fake market data rather than fetching real markets. Acceptable since `--match` is explicitly a preview mode, but worth updating now that real connectors are available.

**Status:** Open — non-blocking, future improvement.

---

### [MINOR] Polymarket connector `.expect()` at line 391

Originally listed separately for tracking — **now resolved** as part of the CRITICAL fix. Reclassified as resolved.

**Status:** RESOLVED (merged into CRITICAL fix).

---

### [NIT] Inconsistent use of `rust_decimal_macros::dec!` vs imported `dec!` — `main.rs:748`

Line 748 uses `rust_decimal_macros::dec!(1)` while the rest of the file uses the imported `dec!` macro. Cosmetic inconsistency.

**Status:** Open — non-blocking.

---

### [NIT] Log file path hardcoded without config — `main.rs:129`

```rust
let log_file = std::fs::File::create("data/arb.log").expect("failed to create log file");
```

Path is hardcoded while database path comes from config. Also uses `.expect()` but this is pre-trading init. Consider making configurable.

**Status:** Open — non-blocking.

---

## Spec Compliance Audit

| # | Acceptance Criterion | Status | Notes |
|---|---------------------|--------|-------|
| 1 | No way to accidentally hit production Kalshi without `--production` flag | PASS | `let use_demo = !args.production;` on line 342 — demo is truly the default. Flag conflict validation on line 334. |
| 2 | No hardcoded API keys, secrets, or credentials | PASS | All credentials loaded from env vars. No secrets in source. `base_url` logged but not keys. |
| 3 | Demo is truly the default | PASS | `use_demo` defaults to `true` unless `--production` is explicitly passed. Config has separate `demo_base_url`/`demo_ws_url`. |
| 4 | PaperConnector wrapping logic correct | PASS | `--paper` wraps only Polymarket (hybrid). `--paper-both` wraps both. `--paper` does NOT paper-wrap Kalshi. |
| 5 | Credential loading uses proper error handling | PASS | Both Kalshi and Polymarket now use `.context()?`. Fixed in Rework 1. |
| 6 | Price feed uses real Kalshi WS + REST fallback | PASS | WS attempted first (line 768), REST polling fallback on WS failure (line 775). 8-second interval. |
| 7 | Market matching uses real Kalshi + Poly via MatchPipeline | PASS | `kalshi_real.list_markets(Open)` fetched, Gamma API for Poly, `MatchPipeline::default()` pairs them. |
| 8 | No sensitive data in logs | PASS | Only `base_url` logged (line 426). No API keys, private keys, or secrets in any `info!`/`warn!` call. |
| 9 | Risk parameters not weakened or bypassed | PASS | `min_time_to_close_hours` increased 1→24, `min_book_depth` increased 0→50. Risk is tighter, not weaker. |

## Safety-Critical Concern Audit

| Concern | Status | Details |
|---------|--------|---------|
| Can production Kalshi be reached without `--production`? | **SAFE** | `use_demo = !args.production` — impossible to reach prod without the flag. `--production && --demo` bails. |
| Code paths where mock removal could cause panic? | **SAFE** | All credential loading uses `.context()?`. No panics on missing env vars. Fixed in Rework 1. |
| PaperConnector wrapping correct in hybrid mode? | **SAFE** | `--paper` leaves `kalshi_dyn` unwrapped. Only Poly is paper-wrapped. Correct. |
| WS handles leak on shutdown? | **SAFE** | Handles stored in `_ws_handles` Vec. Dropped cleanly when `main()` returns. Fixed in Rework 1. |
| Env var names consistent and documented? | **PARTIAL** | All env vars have descriptive `.context()` messages. No central env var documentation file yet. |

## Linter / Type-Check Results

Not executed — `cargo clippy` requires the full workspace to be buildable, and this is a cross-team review without build environment access. The reviewer recommends the Trading team run `cargo clippy --all-targets -- -D warnings` before merging.

## Config Changes Assessment

`config/default.toml` changes are correct:
- `api_url` → `base_url` rename aligns with `KalshiTomlConfig` struct
- `demo_base_url` and `demo_ws_url` added with correct Kalshi demo sandbox URLs
- Risk parameter tightening (`min_time_to_close_hours: 1→24`, `min_book_depth: 0→50`) is a safety improvement

## Cargo.toml Assessment

`arb-kalshi = { workspace = true }` — removed `features = ["mock"]`. This is correct since the mock connector is no longer used. No other dependency changes.

## Rework History

| Cycle | Decision | Blocking | Summary |
|-------|----------|----------|---------|
| 0 | REWORK | 2 | CRITICAL: Polymarket `.expect()` panics. MAJOR: `std::mem::forget` leaks WS handles. |
| 1 | PASS | 0 | Both blocking findings resolved. No new issues introduced. 2 MINOR + 2 NIT remain (non-blocking). |
