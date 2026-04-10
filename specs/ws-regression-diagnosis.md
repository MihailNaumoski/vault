# WebSocket Price Feed Regression Diagnosis

## Executive Summary

The TUI showing no prices is **NOT a WebSocket code regression**. The WebSocket code (both Polymarket and Kalshi) is **unchanged** between commit `99d8323` and the current working tree. The root cause is that **the pairs list is likely empty or the auto-discovery quality gate is too strict**, meaning there are no pairs to subscribe to, and therefore no price feeds start.

## Root Cause Analysis

### What Changed Between `99d8323` and Current Code

There are two commits and uncommitted changes:

1. **`5bfba21`** — Added session logs, docs, TUI check modules (no runtime code changes)
2. **`2e9c633`** — "improve market selection with quality gates and expiry filters"
3. **Uncommitted changes** — Major refactoring of `main.rs` and `tui.rs`

### The Breaking Change: Pair Discovery Overhaul

The old code (`99d8323`) had a two-branch pair loading strategy:

```
if db_pairs.is_empty() {
    // Fetch from APIs, match, seed into DB, return pairs directly
    seeded = [...];
} else {
    // Load existing pairs from DB
    pairs_vec = db_pairs.map(...)
}
```

The new code (uncommitted) replaced this with an always-run auto-discovery pipeline:

```
Step 1: Load existing DB pairs
Step 2: Always run auto-discovery (fetch both APIs, match, insert new pairs)
Step 3: Reload all pairs from DB
```

### Three Interacting Problems

#### Problem 1: pairs.toml Was Emptied (HIGH IMPACT)
**File:** `config/pairs.toml`

The manually configured Hormuz pair was removed and replaced with:
```
# Leave empty to rely entirely on auto-discovery.
```

At commit `99d8323`, this file had a known-good pair (Strait of Hormuz) with valid token IDs. Removing it means the system now relies entirely on auto-discovery to populate the DB.

#### Problem 2: Quality Gate May Reject All Candidates (HIGH IMPACT)
**File:** `config/default.toml` (new `[matcher]` section)

```toml
[matcher]
quality_gate = 0.65
text_similarity_floor = 0.55
```

**File:** `main.rs` lines 1043-1052

```rust
let min_quality_score = app_config.matcher.quality_gate;   // 0.65
let text_sim_floor = app_config.matcher.text_similarity_floor; // 0.55
...
if decision == arb_matcher::MatchDecision::Rejected { continue; }
if c.score.text_similarity < text_sim_floor { continue; }
if c.score.composite < min_quality_score { continue; }
```

If the matcher doesn't find pairs exceeding these thresholds (which is likely given that Polymarket and Kalshi often have different phrasing for similar markets), **zero pairs get inserted** into the DB, leading to:
- Empty pairs list
- No price cache registrations
- No WS subscriptions attempted
- No REST polling started
- TUI shows 0 prices

#### Problem 3: `MatcherTomlConfig` is Required But May Cause Deserialization Failure (MEDIUM IMPACT)
**File:** `main.rs` line 71

```rust
struct AppConfig {
    ...
    matcher: MatcherTomlConfig,  // NEW - non-optional field
    ...
}
```

If the user's `config/default.toml` doesn't have the `[matcher]` section (e.g., running from an older config), the TOML deserialization will fail at startup. However, the `2e9c633` commit did add this section, so this is only an issue if the user has a stale config.

## What Did NOT Change

### WebSocket Code (Both Platforms)
- `arb-polymarket/src/ws.rs` — **No changes** between `99d8323` and HEAD
- `arb-polymarket/src/connector.rs` — **No changes**
- `arb-kalshi/src/ws.rs` — **No changes** since `99d8323` (already had the delta/snapshot/ticker fixes from the `12-46-39` session)
- `arb-kalshi/src/connector.rs` — Only added `list_events()` and `markets_from_events()` methods; `subscribe_prices` delegation is unchanged

### PredictionMarketConnector Trait
- `arb-types/src/lib.rs` — **No changes** to trait signature

### PaperConnector
- `arb-engine/src/paper.rs` — **No changes**; still delegates `subscribe_prices` to inner connector

### Price Cache
- `arb-engine/src/price_cache.rs` — **No changes**

### Subscription Code
- `main.rs` lines 1264-1378 — The subscription setup (price_tx/price_rx channel, WS subscribe calls, REST polling fallback) is **structurally identical** to the old code

### Price Cache Registration
- `main.rs` lines 1236-1240 — Same logic: register with `poly_yes_token_id` if available, else `poly_market_id`

## Recommended Fix

### Quick Fix (Restore Manual Pair)
Restore the Hormuz pair in `config/pairs.toml` to verify the subscription pipeline still works:

```toml
[[pairs]]
poly_condition_id = "0x924a2942747dd75703321a7c8d809c68f6a514c3b0f2a2e64274e02310634669"
poly_yes_token_id = "77893140510362582253172593084218413010407941075415081594586195705930819989216"
poly_no_token_id  = "56231474151770057648855426299021396541474371449092934400587810748856711049761"
poly_question     = "Strait of Hormuz traffic returns to normal by end of April?"
kalshi_ticker     = "KXHORMUZNORM-26MAR17-B260501"
kalshi_question   = "Will Strait of Hormuz transit calls be above 60 before May 1, 2026?"
```

If prices appear after restoring this pair, the diagnosis is confirmed: the auto-discovery pipeline fails to produce valid pairs.

### Proper Fix (Lower Quality Gates + Add Logging)
1. **Lower quality gate temporarily** to see what the matcher produces:
   ```toml
   quality_gate = 0.40
   text_similarity_floor = 0.30
   ```

2. **Add pair count logging** after the discovery pipeline to make the failure visible:
   ```rust
   // After Step 3: Reload
   if pairs.is_empty() {
       error!("FATAL: No pairs available — WS feeds will NOT start. Check matcher quality gates or add manual pairs in config/pairs.toml");
   }
   ```

3. **Add subscription count logging** to distinguish "no pairs" from "pairs exist but subscription fails":
   ```rust
   info!(poly_token_ids = poly_token_ids.len(), "Polymarket WS: tokens to subscribe");
   info!(kalshi_ticker_ids = kalshi_ticker_ids.len(), "Kalshi WS: tickers to subscribe");
   ```

### Diagnosis Verification Steps
Run the app with `RUST_LOG=arb_cli=debug` and check the logs for:
1. `"Loaded existing pairs from DB"` — how many pairs were loaded?
2. `"Newly discovered pairs inserted into DB"` — any discovered?
3. `"Final pair set"` — is `total` > 0?
4. `"Polymarket WS price feed started"` — does this appear?
5. `"Kalshi WS price feed started"` — does this appear?

If none of the last two lines appear, the pairs list is empty.

## Files and Lines Involved

| File | Lines | Change | Impact |
|------|-------|--------|--------|
| `config/pairs.toml` | all | Removed Hormuz pair | No manual pairs available |
| `config/default.toml` | 39-48 | Added `[matcher]` section | Quality gates filter candidates |
| `crates/arb-cli/src/main.rs` | 68-90 | Added `MatcherTomlConfig` | Config deserialization change |
| `crates/arb-cli/src/main.rs` | 876-1192 | Replaced pair loading | Auto-discovery may produce 0 pairs |
| `crates/arb-cli/src/main.rs` | 1043-1052 | Quality gate filtering | May reject all candidates |
| `crates/arb-cli/src/tui.rs` | 1900 | Added `total_discovered` param | Signature change (must match caller) |
