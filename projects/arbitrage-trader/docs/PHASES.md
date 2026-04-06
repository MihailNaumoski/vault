# Prediction Market Arbitrage — Complete Phase Scroll

**Generated:** 2026-04-06
**Input:** SPEC.md, architecture-plan.md, phase2-build-prompts.md, full codebase review, team analysis
**Team:** Architect + Researcher + Security Reviewer + QA Engineer
**Purpose:** One-pass implementation guide — no debugging, no iteration, build it right the first time.

---

## Table of Contents

1. [Current State Snapshot](#1-current-state-snapshot)
2. [Phase 2.5: Hardening & Test Fix](#2-phase-25-hardening--test-fix)
3. [Phase 3: Market Matching Engine](#3-phase-3-market-matching-engine)
4. [Phase 4: Arbitrage Engine — Detection + Execution](#4-phase-4-arbitrage-engine--detection--execution)
5. [Phase 5: TUI + Paper Trading + Production Readiness](#5-phase-5-tui--paper-trading--production-readiness)
6. [Cross-Phase Patterns](#6-cross-phase-patterns)
7. [Risk & Security Guardrails](#7-risk--security-guardrails)

---

## 1. Current State Snapshot

### What's Built & Compiling

| Crate | Status | Lines | Tests | Notes |
|-------|--------|-------|-------|-------|
| `arb-types` | ✅ Complete | 448 | 0 | All domain types, trait, enums |
| `arb-db` | ✅ Complete | 684 | 0 | SQLite + sqlx, repo pattern, migrations |
| `arb-risk` | ✅ Complete | 323 | 0 | RiskManager with 10-point pre-trade check |
| `arb-polymarket` | ✅ Complete | 3,152 | 27 | Auth, REST, WS, signing, mock, rate_limit |
| `arb-kalshi` | ✅ Complete | 3,349 | 38 | Auth, REST, WS, mock, dual rate_limit |
| `arb-engine` | 🔲 Stub | 2 | 0 | Empty — this is Phase 4 |
| `arb-matcher` | 🔲 Stub | 2 | 0 | Empty — this is Phase 3 |
| `arb-cli` | 🔧 Skeleton | 147 | 0 | Config loading, startup — needs TUI |
| **Total** | | **6,501** | **65** | `cargo check --workspace` ✅ passes |

### Known Defects (from Phase 2 code review)

| ID | Severity | Issue | Fix Phase |
|----|----------|-------|-----------|
| H1 | 🔴 HIGH | `PolyConfig` derives `Debug` → leaks API key + private key to logs | 2.5 |
| H2 | 🔴 HIGH | `unwrap()` on HTTP response header values → panic on malformed response | 2.5 |
| H3 | 🔴 HIGH | `f64` arithmetic in EIP-712 signing → rounding errors on order amounts | 2.5 |
| M1 | 🟡 MED | No price bounds validation on API responses | 2.5 |
| M2 | 🟡 MED | Silent zero `token_id` accepted without error | 2.5 |
| M3 | 🟡 MED | No heartbeat/ping on WebSocket connections — silent disconnect possible | 2.5 |
| T1 | 🔴 HIGH | `cargo test --workspace` finds 0 tests — per-crate works fine | 2.5 |

---

## 2. Phase 2.5: Hardening & Test Fix

**Goal:** Fix all HIGH/MEDIUM defects from Phase 2 code review. Fix workspace test discovery. Add missing unit tests to foundation crates. After this phase, every crate has tests and `cargo test --workspace` reports all 65+ tests.

**Crates affected:** `arb-polymarket`, `arb-kalshi`, `arb-types`, `arb-db`, `arb-risk`

### 2.5.1 Fix Workspace Test Discovery (T1)

**Root cause:** The workspace `cargo test` compiles lib targets but the `#[cfg(test)]` modules inside each crate's source files are only compiled when testing that specific crate. This happens when the workspace root has `default-members` excluding some crates, or when there's a feature gate issue.

**Investigation steps:**
```bash
# Verify the issue
cargo test --workspace 2>&1 | grep "running"
# Check if crates are in workspace members
grep -A 20 "members" Cargo.toml
```

**Most likely fix:** Ensure ALL 8 crates are listed in `[workspace] members` (not commented out from Phase 1 scaffolding). Check that no crate has a `required-features` gate that prevents test compilation.

**Files to check/modify:**
- `Cargo.toml` (workspace root) — verify all members listed
- Each crate's `Cargo.toml` — check for `required-features` or `default-run` that might skip test targets

**Acceptance criteria:**
- `cargo test --workspace` reports `65 passed` (27 poly + 38 kalshi + any new tests)
- `cargo test --workspace 2>&1 | grep "running" | grep -v "0 tests"` shows all crate test counts

### 2.5.2 Fix H1: Secret Leakage in Debug

**File:** `crates/arb-polymarket/src/client.rs` (and check kalshi equivalent)

**Current problem:** `#[derive(Debug)]` on config structs that hold `api_key`, `api_secret`, `private_key`.

**Fix:**
```rust
// REMOVE: #[derive(Debug)]
// ADD: Manual Debug impl

use std::fmt;

impl fmt::Debug for PolyConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PolyConfig")
            .field("clob_url", &self.clob_url)
            .field("gamma_url", &self.gamma_url)
            .field("ws_url", &self.ws_url)
            .field("api_key", &"[REDACTED]")
            .field("api_secret", &"[REDACTED]")
            .field("private_key", &"[REDACTED]")
            .finish()
    }
}
```

**Apply same pattern to:**
- `KalshiConfig` in `crates/arb-kalshi/src/client.rs` — redact `api_key_id`, `private_key_pem`
- Any struct containing `String` fields named `*key*`, `*secret*`, `*token*`, `*password*`

**Test:**
```rust
#[test]
fn test_config_debug_redacts_secrets() {
    let config = PolyConfig {
        api_key: "super_secret_key".into(),
        api_secret: "super_secret".into(),
        private_key: "0xdeadbeef".into(),
        clob_url: "https://clob.polymarket.com".into(),
        gamma_url: "https://gamma-api.polymarket.com".into(),
        ws_url: "wss://ws.polymarket.com".into(),
    };
    let debug_output = format!("{:?}", config);
    assert!(!debug_output.contains("super_secret"));
    assert!(!debug_output.contains("deadbeef"));
    assert!(debug_output.contains("[REDACTED]"));
}
```

### 2.5.3 Fix H2: Unwrap on Headers

**Files:** Search all `*.rs` for `.unwrap()` on header parsing:
```bash
grep -rn "\.unwrap()" crates/arb-polymarket/src/*.rs crates/arb-kalshi/src/*.rs | grep -i "header\|HeaderValue"
```

**Fix pattern:** Replace every `.unwrap()` on header operations with:
```rust
// BEFORE (panics)
let header_value = HeaderValue::from_str(&timestamp).unwrap();

// AFTER (returns error)
let header_value = HeaderValue::from_str(&timestamp)
    .map_err(|e| PolymarketError::Auth(format!("invalid header value for timestamp: {e}")))?;
```

### 2.5.4 Fix H3: f64 Financial Math in Signing

**File:** `crates/arb-polymarket/src/signing.rs`

**Current problem:** EIP-712 order signing uses `f64` for amount calculations, which has floating-point rounding issues. Example: `0.1 + 0.2 != 0.3` in f64.

**Fix approach:** The EIP-712 typed data `tokenAmount` field is a `U256` (unsigned 256-bit integer) representing the amount in the token's smallest unit. The calculation should go:

```rust
// BEFORE (wrong):
let amount_f64 = price_f64 * quantity_f64 * 1_000_000.0;
let token_amount = U256::from(amount_f64 as u64);

// AFTER (correct):
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

let price = Decimal::from_str("0.42")?;  // already Decimal from arb-types
let quantity = Decimal::from(50u32);
let scale = Decimal::from(1_000_000u64); // USDC has 6 decimals

let token_amount_decimal = price * quantity * scale;
let token_amount_u64 = token_amount_decimal
    .to_u64()
    .ok_or(PolymarketError::Signing("amount overflow".into()))?;
let token_amount = U256::from(token_amount_u64);
```

**Critical:** The `LimitOrderRequest` in `arb-types` already uses `Decimal` for `price`. Make sure the signing code receives `Decimal` and never converts to `f64`.

**Test:**
```rust
#[test]
fn test_sign_order_amount_precision() {
    // This is the classic floating point trap
    let price = dec!(0.1) + dec!(0.2); // Should be exactly 0.3
    let quantity = dec!(100);
    let scale = dec!(1_000_000);
    let result = (price * quantity * scale).to_u64().unwrap();
    assert_eq!(result, 30_000_000); // Exactly 30M, no rounding error
}
```

### 2.5.5 Fix M1: Price Bounds Validation

**File:** Add to connector `types.rs` (both poly and kalshi)

When deserializing API responses, validate that prices are in `[0.00, 1.00]`:

```rust
fn validate_price(price: Decimal) -> Result<Decimal, ConnectorError> {
    if price < dec!(0) || price > dec!(1) {
        return Err(ConnectorError::InvalidData(
            format!("price {} out of range [0.00, 1.00]", price)
        ));
    }
    Ok(price)
}
```

Apply in `client.rs` wherever an API response includes a price field.

### 2.5.6 Fix M2: Zero Token ID Validation

**File:** `crates/arb-polymarket/src/connector.rs`

In `place_limit_order()`, before sending the request:

```rust
if req.market_id.is_empty() {
    return Err(ArbError::Connector {
        platform: Platform::Polymarket,
        message: "token_id is empty — cannot place order without a valid token".into(),
        source: None,
    });
}
```

### 2.5.7 Fix M3: WebSocket Heartbeat

**Files:** `crates/arb-polymarket/src/ws.rs`, `crates/arb-kalshi/src/ws.rs`

Add a ping interval to detect silent disconnections:

```rust
use tokio::time::{interval, Duration};

// Inside the WebSocket read loop:
let mut ping_interval = interval(Duration::from_secs(30));
let mut last_pong = Instant::now();

loop {
    tokio::select! {
        msg = ws_stream.next() => {
            match msg {
                Some(Ok(Message::Pong(_))) => {
                    last_pong = Instant::now();
                }
                Some(Ok(msg)) => { /* handle normally */ }
                Some(Err(e)) => { /* reconnect */ }
                None => { /* stream ended, reconnect */ }
            }
        }
        _ = ping_interval.tick() => {
            if last_pong.elapsed() > Duration::from_secs(90) {
                tracing::warn!("no pong received in 90s, reconnecting");
                break; // trigger reconnection
            }
            let _ = ws_stream.send(Message::Ping(vec![])).await;
        }
    }
}
```

### 2.5.8 Add Missing Unit Tests

**`arb-types` (currently 0 tests):**
```
tests to add:
  - Price validation: reject < 0, reject > 1, accept 0.00 and 1.00
  - MarketPair serialization round-trip
  - Opportunity spread calculation
  - OrderBook best_ask / best_bid
  - PriceUpdate construction
  - ArbError display messages
```

**`arb-db` (currently 0 tests):**
```
tests to add:
  - Create in-memory DB, run migrations
  - CRUD market_pairs (insert, get, list_active)
  - CRUD opportunities (insert, update_status, list_recent)
  - CRUD orders (insert, update, list_open)
  - CRUD positions (insert, update, list_open)
  - Foreign key constraint (order with bad opportunity_id → error)
  - WAL mode is active after init
```

**`arb-risk` (currently 0 tests):**
```
tests to add:
  - pre_trade_check passes with valid params
  - pre_trade_check rejects: engine not running
  - pre_trade_check rejects: pair not verified
  - pre_trade_check rejects: spread too low
  - pre_trade_check rejects: too close to expiry
  - pre_trade_check rejects: insufficient balance (poly)
  - pre_trade_check rejects: insufficient balance (kalshi)
  - pre_trade_check rejects: position limit exceeded
  - pre_trade_check rejects: total exposure exceeded
  - pre_trade_check rejects: daily loss exceeded
  - pre_trade_check rejects: insufficient book depth
  - ExposureTracker add/remove/reset
  - RiskConfig deserializes from TOML
```

### Phase 2.5 Acceptance Criteria

- [ ] `cargo test --workspace` finds and runs ALL tests (>= 90 total)
- [ ] Zero `unwrap()` on header values or API response parsing in connector crates
- [ ] `format!("{:?}", config)` never contains API keys or private keys
- [ ] EIP-712 signing uses `Decimal` → `U256` path, no `f64` in financial math
- [ ] Price validation rejects out-of-range values in API responses
- [ ] Empty `token_id` / `market_id` returns error, not silent success
- [ ] WebSocket connections send ping every 30s, reconnect after 90s silence
- [ ] `cargo clippy --workspace -- -D warnings` passes

### Build Prompts for Phase 2.5

**Prompt 2.5-A: Fix workspace tests + add arb-types tests**
```
Read: Cargo.toml (workspace root), all crate Cargo.toml files
Fix: Ensure all 8 crates are in workspace members list
Add: Unit tests to crates/arb-types/src/ (price validation, serialization, spread calc)
Verify: cargo test --workspace shows tests from all crates
```

**Prompt 2.5-B: Fix H1 + H2 + H3 (security fixes)**
```
Read: crates/arb-polymarket/src/client.rs, signing.rs, auth.rs
Read: crates/arb-kalshi/src/client.rs, auth.rs
Fix: Manual Debug impl for configs (H1), .map_err on headers (H2), Decimal in signing (H3)
Add: Tests for each fix (secret redaction, header error handling, amount precision)
Verify: cargo test -p arb-polymarket && cargo test -p arb-kalshi
```

**Prompt 2.5-C: Fix M1-M3 + add arb-db and arb-risk tests**
```
Read: connector types, ws.rs files, arb-db repo.rs, arb-risk manager.rs
Fix: Price validation (M1), token_id validation (M2), WS heartbeat (M3)
Add: Full test suites for arb-db (CRUD) and arb-risk (all 10 checks + exposure)
Verify: cargo test --workspace reports >= 90 tests passing
```

**Execution order:** 2.5-A first (fixes test discovery), then 2.5-B and 2.5-C can run in parallel.

---

## 3. Phase 3: Market Matching Engine

**Goal:** Given market lists from both platforms, find equivalent markets across Polymarket and Kalshi. Output verified `MarketPair` entries stored in DB and loaded from `config/pairs.toml`.

**Crates affected:** `arb-matcher` (main), `arb-types` (minor additions), `arb-db` (pair persistence), `arb-cli` (new `--match` command)

### 3.1 Architecture

```
                                  ┌─────────────────┐
                                  │  CLI --match     │
                                  │  (human review)  │
                                  └────────┬────────┘
                                           │
                                           ▼
┌──────────────┐    ┌──────────────┐   ┌──────────┐
│ Polymarket   │───►│ MarketFetcher│──►│ Matcher  │──► Candidates
│ list_markets │    │ (normalizes) │   │ Pipeline │    (scored)
└──────────────┘    └──────────────┘   └──────────┘
                                           ▲          ┌──────────┐
┌──────────────┐    ┌──────────────┐       │          │ PairStore│
│ Kalshi       │───►│ MarketFetcher│───────┘          │ (DB+TOML)│
│ list_markets │    │ (normalizes) │                  └──────────┘
└──────────────┘    └──────────────┘
```

### 3.2 Files to Create/Modify

```
crates/arb-matcher/
  src/
    lib.rs          — module declarations + re-exports
    normalize.rs    — text normalization (lowercase, strip punctuation, stop words)
    scorer.rs       — multi-signal similarity scoring
    pipeline.rs     — end-to-end matching pipeline
    store.rs        — pair persistence (DB + TOML)
    types.rs        — matcher-specific types (MatchCandidate, MatchScore)
```

### 3.3 Matching Pipeline (SPEC §7.2)

The matcher uses a **multi-stage pipeline** to avoid O(N²) full comparison:

**Stage 1 — Normalize:**
```rust
pub fn normalize(text: &str) -> String {
    text.to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && !c.is_whitespace(), "")
        .split_whitespace()
        .filter(|w| !STOP_WORDS.contains(w))
        .collect::<Vec<_>>()
        .join(" ")
}

const STOP_WORDS: &[&str] = &[
    "will", "the", "a", "an", "of", "in", "on", "by", "to", "for",
    "be", "is", "at", "or", "and", "before", "after",
];
```

**Stage 2 — Candidate generation (bloom filter on tokens):**
- Tokenize both normalized question texts
- For each Polymarket market, find Kalshi markets sharing ≥ 2 significant tokens
- This reduces N×M comparisons to ~N×K where K << M

**Stage 3 — Scoring (multi-signal):**
```rust
pub struct MatchScore {
    pub text_similarity: f64,     // Jaro-Winkler on normalized questions
    pub close_time_delta: f64,    // 1.0 if within 24h, decays linearly
    pub category_match: f64,      // 1.0 if same category, 0.0 otherwise
    pub composite: f64,           // weighted average
}

impl MatchScore {
    pub fn compute(poly: &Market, kalshi: &Market) -> Self {
        let text_sim = strsim::jaro_winkler(
            &normalize(&poly.question),
            &normalize(&kalshi.question),
        );

        let time_delta_hours = (poly.close_time - kalshi.close_time)
            .num_hours()
            .abs() as f64;
        let close_time_score = (1.0 - time_delta_hours / 168.0).max(0.0); // 1 week decay

        let category_score = if poly.category == kalshi.category { 1.0 } else { 0.0 };

        let composite = text_sim * 0.60 + close_time_score * 0.30 + category_score * 0.10;

        Self {
            text_similarity: text_sim,
            close_time_delta: close_time_score,
            category_match: category_score,
            composite,
        }
    }
}
```

**Why Jaro-Winkler (not Levenshtein):**
- Jaro-Winkler gives higher scores to strings that match from the beginning — good for prediction markets where the question often starts similarly ("Will X", "Will Y")
- Levenshtein gives raw edit distance which penalizes length differences too heavily
- For very short titles, also compute token overlap as a fallback

**Stage 4 — Threshold + Human review:**
```rust
// Confidence tiers
const AUTO_VERIFY_THRESHOLD: f64 = 0.95;   // Auto-verify if score > 0.95
const SUGGEST_THRESHOLD: f64 = 0.70;        // Suggest for human review if > 0.70
const REJECT_THRESHOLD: f64 = 0.70;         // Below this, don't even show

pub enum MatchDecision {
    AutoVerified(MatchCandidate),
    NeedsReview(MatchCandidate),
    Rejected,
}
```

### 3.4 Pair Store (DB + TOML)

**TOML file** (`config/pairs.toml`) for manually verified pairs:
```toml
[[pairs]]
poly_condition_id = "0x1234..."
poly_yes_token_id = "0xABCD..."
poly_no_token_id = "0xEF01..."
kalshi_ticker = "PRES-2028-DEM"
verified = true
notes = "2028 US Presidential Election - Democratic nominee"

[[pairs]]
# ... more pairs
```

**Store logic:**
```rust
pub struct PairStore {
    db: Arc<Database>,
}

impl PairStore {
    /// Load pairs from TOML file, upsert into DB
    pub async fn load_from_toml(&self, path: &Path) -> Result<Vec<MarketPair>>;

    /// Save a new match candidate to DB (unverified)
    pub async fn save_candidate(&self, candidate: &MatchCandidate) -> Result<()>;

    /// Mark a pair as verified (human approved)
    pub async fn verify_pair(&self, pair_id: Uuid) -> Result<()>;

    /// Get all active, verified pairs
    pub async fn active_pairs(&self) -> Result<Vec<MarketPair>>;

    /// Deactivate pairs whose markets have closed
    pub async fn deactivate_expired(&self) -> Result<u32>;
}
```

### 3.5 CLI `--match` Command

Add to `arb-cli/src/main.rs`:
```rust
match cli.command {
    Command::Match => {
        // 1. Init connectors
        // 2. Fetch markets from both platforms
        let poly_markets = poly_connector.list_markets(MarketStatus::Active).await?;
        let kalshi_markets = kalshi_connector.list_markets(MarketStatus::Active).await?;

        // 3. Run matching pipeline
        let candidates = matcher.find_matches(&poly_markets, &kalshi_markets);

        // 4. Display results in terminal table
        for candidate in &candidates {
            println!(
                "{:.2} | {} ↔ {} | closes: {} vs {}",
                candidate.score.composite,
                candidate.poly_market.question,
                candidate.kalshi_market.question,
                candidate.poly_market.close_time,
                candidate.kalshi_market.close_time,
            );
        }

        // 5. Interactive: prompt user to verify each candidate
        for candidate in candidates.iter().filter(|c| c.score.composite >= SUGGEST_THRESHOLD) {
            print!("Verify pair? [y/N/s(skip)]: ");
            // Read stdin, save verified pairs to DB
        }
    }
}
```

### 3.6 Security Considerations

**🔴 CRITICAL — False positive matching is catastrophic:**
If the matcher pairs "Will Biden win 2028?" (Polymarket) with "Will Trump win 2028?" (Kalshi), you'd buy YES on both sides thinking they're the same market. When one resolves YES and the other NO, you lose both positions.

**Guardrails:**
1. **NEVER auto-trade on unverified pairs.** The `verified` flag in DB must be `true` before the engine uses a pair. The engine MUST check this.
2. **Human review is mandatory** for any pair with composite score < 0.95
3. **Resolution equivalence check** (SPEC §7.4): After matching, verify that the markets have the same resolution source or similar resolution criteria. This is a human judgment call.
4. **Deactivation on close_time divergence:** If matched markets suddenly have close_times > 48h apart, auto-deactivate and alert.

### 3.7 Test Strategy

```
Unit tests (in arb-matcher):
  - normalize(): "Will the U.S. GDP grow by 3%?" → "us gdp grow 3"
  - scorer: identical texts → 1.0 composite
  - scorer: completely different texts → < 0.3
  - scorer: similar markets (real examples) → > 0.8
  - scorer: close_time within 24h → 1.0 time score
  - scorer: close_time 2 weeks apart → 0.0 time score
  - pipeline: 100 poly + 100 kalshi markets → finds known pairs in < 1s
  - store: load from TOML, upsert to DB, read back

Integration tests:
  - Full pipeline with mock connector data
  - Verify that unverified pairs are NOT returned by active_pairs()
  - Verify deactivation of expired pairs
```

### 3.8 Acceptance Criteria

- [ ] `arb --match` fetches markets from both platforms and displays scored candidates
- [ ] Jaro-Winkler + close_time + category scoring produces composite > 0.85 for known equivalent markets
- [ ] TOML pairs load into DB correctly
- [ ] `active_pairs()` only returns pairs where `verified = true AND active = true`
- [ ] Expired market pairs are auto-deactivated
- [ ] Pipeline processes 500 Poly × 500 Kalshi markets in < 5 seconds
- [ ] All new code has unit tests, `cargo test -p arb-matcher` passes

### 3.9 Build Prompts

**Prompt 3-A: Core matching engine**
```
Crate: arb-matcher
Create: lib.rs, normalize.rs, scorer.rs, types.rs
Read: arb-types Market struct, SPEC §7.1-7.4
Implement: normalize(), MatchScore::compute(), MatchCandidate struct
Tests: normalization, scoring with known market pairs, threshold classification
Verify: cargo test -p arb-matcher
```

**Prompt 3-B: Pipeline + Store**
```
Crate: arb-matcher
Create: pipeline.rs, store.rs
Read: arb-db repo.rs (for DB pattern), SPEC §7.2-7.3
Implement: MatchPipeline::find_matches(), PairStore (TOML + DB)
Dependencies: arb-types, arb-db
Tests: pipeline end-to-end with mock data, store CRUD
Verify: cargo test -p arb-matcher
```

**Prompt 3-C: CLI --match command**
```
Crate: arb-cli
Modify: main.rs (add Command::Match)
Read: arb-matcher pipeline.rs, store.rs, arb-polymarket connector, arb-kalshi connector
Implement: --match subcommand with interactive pair verification
Add: clap dependency if not present
Verify: cargo build && cargo run -- --match --help
```

**Execution order:** 3-A first, then 3-B (depends on types), then 3-C (depends on pipeline + store).

---

## 4. Phase 4: Arbitrage Engine — Detection + Execution

**Goal:** The brain of the system. Detect arbitrage opportunities from live price feeds, execute dual-leg orders, monitor fills, manage positions, handle unwinds. This is the most complex phase.

**Crates affected:** `arb-engine` (main), `arb-risk` (integration), `arb-db` (persistence), `arb-types` (minor additions)

### 4.1 Architecture

```
                     ┌──────────────────────────────────────────┐
                     │              arb-engine                  │
                     │                                          │
 [Poly WS] ──► mpsc ─┤  ┌───────────┐   ┌───────────┐         │
                      │  │ Detector  │──►│ Executor  │         │
 [Kalshi WS] ─► mpsc ─┤  │ (price    │   │ (places   │         │
                      │  │  compare) │   │  orders)  │         │
                      │  └───────────┘   └─────┬─────┘         │
                      │                        │               │
                      │  ┌───────────┐   ┌─────▼─────┐         │
                      │  │ Tracker   │◄──│ Monitor   │         │
                      │  │ (P&L,     │   │ (polls    │         │
                      │  │  positions)│   │  fills)   │         │
                      │  └───────────┘   └───────────┘         │
                      │                                        │
                      │  ┌───────────┐                         │
                      │  │ Unwinder  │ ← triggered by Monitor  │
                      │  │ (exit     │    when one leg fails   │
                      │  │  bad pos) │                         │
                      │  └───────────┘                         │
                      └────────────────────────────────────────┘
```

### 4.2 Files to Create

```
crates/arb-engine/
  src/
    lib.rs            — Engine struct, start(), shutdown()
    detector.rs       — Opportunity detection from price updates
    executor.rs       — Dual-leg order placement
    monitor.rs        — Order fill monitoring + state machine
    tracker.rs        — Position tracking + P&L
    unwinder.rs       — Unwind strategy for one-legged positions
    price_cache.rs    — In-memory latest prices per market pair
    types.rs          — Engine-internal types (ExecutionResult, MonitorAction)
```

### 4.3 Module Specifications

#### 4.3.1 `price_cache.rs` — Real-Time Price State

```rust
use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::collections::HashMap;
use uuid::Uuid;

pub struct PriceCache {
    /// pair_id -> (poly_yes_price, kalshi_yes_price, last_updated)
    prices: RwLock<HashMap<Uuid, PricePair>>,
}

pub struct PricePair {
    pub poly_yes: Decimal,
    pub poly_no: Decimal,
    pub kalshi_yes: Decimal,
    pub kalshi_no: Decimal,
    pub poly_updated: DateTime<Utc>,
    pub kalshi_updated: DateTime<Utc>,
}

impl PriceCache {
    pub fn new() -> Self;
    pub fn update_poly(&self, pair_id: Uuid, yes: Decimal, no: Decimal);
    pub fn update_kalshi(&self, pair_id: Uuid, yes: Decimal, no: Decimal);
    pub fn get(&self, pair_id: &Uuid) -> Option<PricePair>;

    /// Returns true if both sides have been updated within `max_age`
    pub fn is_fresh(&self, pair_id: &Uuid, max_age: Duration) -> bool;
}
```

**Why `parking_lot::RwLock` not `tokio::sync::RwLock`:** The price cache is read-heavy (detector reads every scan cycle) and write-light (updates on each WS message). `parking_lot::RwLock` is faster for short critical sections and doesn't require `.await`.

#### 4.3.2 `detector.rs` — Opportunity Detection

```rust
pub struct Detector {
    price_cache: Arc<PriceCache>,
    risk_manager: Arc<RwLock<RiskManager>>,
    min_spread: Decimal,
    min_spread_absolute: Decimal,
    max_staleness: Duration,
}

impl Detector {
    /// Scan all active pairs for arbitrage opportunities.
    /// Called on every price update or on a timer.
    pub fn scan(&self, pairs: &[MarketPair]) -> Vec<Opportunity> {
        pairs.iter()
            .filter_map(|pair| self.check_pair(pair))
            .collect()
    }

    fn check_pair(&self, pair: &MarketPair) -> Option<Opportunity> {
        let prices = self.price_cache.get(&pair.id)?;

        // SPEC §8.3 — Spread calculation:
        // Combined cost = poly_yes + kalshi_no (or poly_no + kalshi_yes)
        // Guaranteed payout = $1.00 (if markets are equivalent)
        // Spread = 1.00 - combined_cost

        // Check both directions:
        let spread_a = dec!(1) - prices.poly_yes - prices.kalshi_no;
        let spread_b = dec!(1) - prices.poly_no - prices.kalshi_yes;

        // Pick the better direction
        let (spread, poly_side, kalshi_side) = if spread_a > spread_b {
            (spread_a, Side::Yes, Side::No)
        } else {
            (spread_b, Side::No, Side::Yes)
        };

        // Must exceed minimums (SPEC §8.1)
        if spread < self.min_spread_absolute {
            return None;
        }

        let combined_cost = if poly_side == Side::Yes {
            prices.poly_yes + prices.kalshi_no
        } else {
            prices.poly_no + prices.kalshi_yes
        };
        let spread_pct = spread / combined_cost * dec!(100);
        if spread_pct < self.min_spread {
            return None;
        }

        // Staleness check — prices must be fresh
        if !self.price_cache.is_fresh(&pair.id, self.max_staleness) {
            tracing::debug!(pair_id = %pair.id, "skipping stale prices");
            return None;
        }

        Some(Opportunity {
            id: Uuid::now_v7(),
            pair_id: pair.id,
            poly_side,
            poly_price: if poly_side == Side::Yes { prices.poly_yes } else { prices.poly_no },
            kalshi_side,
            kalshi_price: if kalshi_side == Side::No { prices.kalshi_no } else { prices.kalshi_yes },
            spread,
            spread_pct,
            max_quantity: 0, // calculated by executor based on book depth
            status: OpportunityStatus::Detected,
            detected_at: Utc::now(),
            executed_at: None,
            resolved_at: None,
        })
    }
}
```

#### 4.3.3 `executor.rs` — Dual-Leg Order Placement

```rust
pub struct Executor {
    poly: Arc<dyn PredictionMarketConnector>,
    kalshi: Arc<dyn PredictionMarketConnector>,
    risk_manager: Arc<RwLock<RiskManager>>,
    db: Arc<Database>,
    order_config: OrderConfig,
}

pub struct OrderConfig {
    pub default_quantity: u32,
    pub price_improve_amount: Decimal,
    pub max_order_age_secs: u64,
}

pub struct ExecutionResult {
    pub opportunity_id: Uuid,
    pub poly_order: OrderResponse,
    pub kalshi_order: OrderResponse,
    pub executed_at: DateTime<Utc>,
}

impl Executor {
    /// Execute an opportunity by placing orders on both platforms simultaneously.
    pub async fn execute(&self, opp: &mut Opportunity) -> Result<ExecutionResult, ArbError> {
        // 1. Run risk pre-trade check
        {
            let rm = self.risk_manager.read();
            let poly_balance = self.poly.get_balance().await?;
            let kalshi_balance = self.kalshi.get_balance().await?;
            let poly_book = self.poly.get_order_book(&opp.poly_market_id()).await?;

            rm.pre_trade_check(
                opp.pair_id,
                true, // pair already verified if we got here
                opp.spread,
                self.order_config.price_improve_amount, // min spread from config
                // ... all other params from SPEC §10.1
            )?;
        }

        // 2. Calculate quantity from book depth
        let quantity = self.calculate_quantity(opp).await?;

        // 3. Place both legs SIMULTANEOUSLY
        let poly_req = LimitOrderRequest {
            market_id: opp.poly_market_id().to_string(),
            side: opp.poly_side.clone(),
            price: opp.poly_price,
            quantity,
            ..Default::default()
        };
        let kalshi_req = LimitOrderRequest {
            market_id: opp.kalshi_market_id().to_string(),
            side: opp.kalshi_side.clone(),
            price: opp.kalshi_price,
            quantity,
            ..Default::default()
        };

        let (poly_result, kalshi_result) = tokio::join!(
            self.poly.place_limit_order(&poly_req),
            self.kalshi.place_limit_order(&kalshi_req),
        );

        // 4. Handle results — both must succeed, or cancel the successful one
        match (poly_result, kalshi_result) {
            (Ok(poly_order), Ok(kalshi_order)) => {
                // Both placed successfully
                opp.status = OpportunityStatus::Executed;
                opp.executed_at = Some(Utc::now());

                // Persist orders to DB
                self.persist_orders(opp, &poly_order, &kalshi_order).await?;

                Ok(ExecutionResult {
                    opportunity_id: opp.id,
                    poly_order,
                    kalshi_order,
                    executed_at: Utc::now(),
                })
            }
            (Ok(poly_order), Err(kalshi_err)) => {
                // Poly succeeded, Kalshi failed → cancel Poly order
                tracing::warn!(
                    opp_id = %opp.id,
                    "kalshi leg failed, cancelling poly order: {}",
                    kalshi_err
                );
                let _ = self.poly.cancel_order(&poly_order.platform_order_id).await;
                opp.status = OpportunityStatus::Failed;
                Err(ArbError::Connector {
                    platform: Platform::Kalshi,
                    message: format!("kalshi leg failed: {}", kalshi_err),
                    source: None,
                })
            }
            (Err(poly_err), Ok(kalshi_order)) => {
                // Kalshi succeeded, Poly failed → cancel Kalshi order
                tracing::warn!(
                    opp_id = %opp.id,
                    "poly leg failed, cancelling kalshi order: {}",
                    poly_err
                );
                let _ = self.kalshi.cancel_order(&kalshi_order.platform_order_id).await;
                opp.status = OpportunityStatus::Failed;
                Err(ArbError::Connector {
                    platform: Platform::Polymarket,
                    message: format!("poly leg failed: {}", poly_err),
                    source: None,
                })
            }
            (Err(poly_err), Err(kalshi_err)) => {
                // Both failed — nothing to clean up
                opp.status = OpportunityStatus::Failed;
                Err(ArbError::Order(format!(
                    "both legs failed: poly={}, kalshi={}",
                    poly_err, kalshi_err
                )))
            }
        }
    }
}
```

**🔴 CRITICAL CONCURRENCY NOTE:**
`tokio::join!` places both orders concurrently. But there's a race: if Poly fills instantly before Kalshi even acknowledges, you have a one-legged position. The Monitor (§4.3.4) handles this — it runs immediately after execution and watches for asymmetric fills.

#### 4.3.4 `monitor.rs` — Order Fill Monitoring

```rust
pub struct Monitor {
    poly: Arc<dyn PredictionMarketConnector>,
    kalshi: Arc<dyn PredictionMarketConnector>,
    db: Arc<Database>,
    config: MonitorConfig,
}

pub struct MonitorConfig {
    pub check_interval: Duration,          // 500ms from SPEC §9.3
    pub max_order_age: Duration,           // 30s from SPEC §9.4
    pub max_hedge_wait: Duration,          // 60s from SPEC §9.4
}

pub enum MonitorAction {
    /// Both orders fully filled — create position
    BothFilled {
        poly_order: OrderResponse,
        kalshi_order: OrderResponse,
    },
    /// Both still open — wait
    Waiting,
    /// One filled, other still open — keep watching but start countdown
    PartialHedge {
        filled_platform: Platform,
        filled_order: OrderResponse,
        open_order: OrderResponse,
    },
    /// Timeout reached — cancel unfilled, unwind filled
    NeedsUnwind {
        filled_platform: Platform,
        filled_order: OrderResponse,
        unfilled_order_id: String,
    },
    /// Both expired/cancelled — clean up
    BothCancelled,
}

impl Monitor {
    /// Monitor a pair of orders until resolution.
    /// This is a BLOCKING loop — run in a spawned tokio task.
    pub async fn watch_order_pair(
        &self,
        poly_order_id: &str,
        kalshi_order_id: &str,
        placed_at: DateTime<Utc>,
    ) -> Result<MonitorAction, ArbError> {
        let mut interval = tokio::time::interval(self.config.check_interval);

        loop {
            interval.tick().await;

            // Batch poll: use list_open_orders() instead of per-order get_order()
            // This is critical for Kalshi's 10 req/s rate limit (SPEC gap G5)
            let poly_order = self.poly.get_order(poly_order_id).await?;
            let kalshi_order = self.kalshi.get_order(kalshi_order_id).await?;

            let poly_filled = poly_order.status == OrderStatus::Filled;
            let kalshi_filled = kalshi_order.status == OrderStatus::Filled;

            match (poly_filled, kalshi_filled) {
                (true, true) => {
                    return Ok(MonitorAction::BothFilled {
                        poly_order,
                        kalshi_order,
                    });
                }
                (true, false) | (false, true) => {
                    let elapsed = Utc::now() - placed_at;
                    if elapsed > chrono::Duration::from_std(self.config.max_hedge_wait)
                        .unwrap_or(chrono::Duration::seconds(60))
                    {
                        let (filled_platform, filled_order, unfilled_id) = if poly_filled {
                            (Platform::Polymarket, poly_order, kalshi_order_id.to_string())
                        } else {
                            (Platform::Kalshi, kalshi_order, poly_order_id.to_string())
                        };
                        return Ok(MonitorAction::NeedsUnwind {
                            filled_platform,
                            filled_order,
                            unfilled_order_id: unfilled_id,
                        });
                    }
                    // Still within hedge wait — continue polling
                }
                (false, false) => {
                    let elapsed = Utc::now() - placed_at;
                    if elapsed > chrono::Duration::from_std(self.config.max_order_age)
                        .unwrap_or(chrono::Duration::seconds(30))
                    {
                        // Both timed out — cancel both
                        let _ = self.poly.cancel_order(poly_order_id).await;
                        let _ = self.kalshi.cancel_order(kalshi_order_id).await;
                        return Ok(MonitorAction::BothCancelled);
                    }
                }
            }
        }
    }
}
```

**Rate limit note for Kalshi:** In production, replace per-order `get_order()` with `list_open_orders()` batching:
```rust
// Better: one call returns all open orders
let all_kalshi_orders = self.kalshi.list_open_orders().await?;
let kalshi_order = all_kalshi_orders.iter()
    .find(|o| o.platform_order_id == kalshi_order_id)
    .cloned();
```

#### 4.3.5 `unwinder.rs` — Exit Strategy for Failed Hedges

```rust
pub struct Unwinder {
    poly: Arc<dyn PredictionMarketConnector>,
    kalshi: Arc<dyn PredictionMarketConnector>,
    risk_manager: Arc<RwLock<RiskManager>>,
    db: Arc<Database>,
}

impl Unwinder {
    /// Unwind a one-legged position by selling the filled leg.
    /// This is a loss-taking operation.
    pub async fn unwind(
        &self,
        filled_platform: Platform,
        filled_order: &OrderResponse,
        unfilled_order_id: &str,
    ) -> Result<Decimal, ArbError> {
        // 1. Cancel the unfilled order (may already be cancelled)
        let cancel_connector = match filled_platform {
            Platform::Polymarket => self.kalshi.as_ref(),
            Platform::Kalshi => self.poly.as_ref(),
        };
        let _ = cancel_connector.cancel_order(unfilled_order_id).await;

        // 2. Sell the filled position at market
        // Place a limit order at the current best bid (taker)
        let connector = match filled_platform {
            Platform::Polymarket => self.poly.as_ref(),
            Platform::Kalshi => self.kalshi.as_ref(),
        };

        let book = connector.get_order_book(&filled_order.market_id).await?;
        let best_bid = book.bids.first()
            .ok_or(ArbError::Order("no bids available for unwind".into()))?;

        let unwind_req = LimitOrderRequest {
            market_id: filled_order.market_id.clone(),
            side: filled_order.side.opposite(), // Sell what we bought
            price: best_bid.price,
            quantity: filled_order.filled_quantity,
            ..Default::default()
        };

        let unwind_order = connector.place_limit_order(&unwind_req).await?;

        // 3. Calculate and record loss
        let entry_cost = filled_order.price * Decimal::from(filled_order.filled_quantity);
        let exit_value = best_bid.price * Decimal::from(filled_order.filled_quantity);
        let loss = entry_cost - exit_value; // positive = loss

        // 4. Update risk manager
        self.risk_manager.write().exposure_mut().record_unwind_loss(loss);

        tracing::warn!(
            platform = %filled_platform,
            loss = %loss,
            "unwind completed — loss recorded"
        );

        Ok(loss)
    }
}
```

#### 4.3.6 `tracker.rs` — Position Management

```rust
pub struct Tracker {
    db: Arc<Database>,
    risk_manager: Arc<RwLock<RiskManager>>,
}

impl Tracker {
    /// Create a new hedged position from two filled orders.
    pub async fn create_position(
        &self,
        opp: &Opportunity,
        poly_order: &OrderResponse,
        kalshi_order: &OrderResponse,
    ) -> Result<Position, ArbError> {
        let hedged_qty = poly_order.filled_quantity.min(kalshi_order.filled_quantity);
        let guaranteed_profit = opp.spread * Decimal::from(hedged_qty);

        let position = Position {
            id: Uuid::now_v7(),
            pair_id: opp.pair_id,
            poly_side: opp.poly_side.clone(),
            poly_quantity: poly_order.filled_quantity,
            poly_avg_price: poly_order.price,
            kalshi_side: opp.kalshi_side.clone(),
            kalshi_quantity: kalshi_order.filled_quantity,
            kalshi_avg_price: kalshi_order.price,
            hedged_quantity: hedged_qty,
            unhedged_quantity: (poly_order.filled_quantity as i32 - kalshi_order.filled_quantity as i32).unsigned_abs(),
            guaranteed_profit,
            status: PositionStatus::Open,
            opened_at: Utc::now(),
            settled_at: None,
        };

        // Persist
        self.db.repo().insert_position(&position).await?;

        // Update exposure
        let capital = poly_order.price * Decimal::from(poly_order.filled_quantity)
            + kalshi_order.price * Decimal::from(kalshi_order.filled_quantity);
        self.risk_manager.write().exposure_mut().add_position(opp.pair_id, capital);

        tracing::info!(
            position_id = %position.id,
            profit = %guaranteed_profit,
            hedged = hedged_qty,
            "new position created"
        );

        Ok(position)
    }

    /// Settle a position when the market resolves.
    pub async fn settle_position(
        &self,
        position_id: Uuid,
        resolution: Side, // which side won
    ) -> Result<Decimal, ArbError> {
        // ... calculate actual P&L based on resolution
        // ... update DB status to Settled
        // ... update risk manager exposure
        todo!()
    }
}
```

#### 4.3.7 `lib.rs` — Engine Orchestration

```rust
pub struct Engine {
    detector: Detector,
    executor: Executor,
    monitor: Monitor,
    tracker: Tracker,
    unwinder: Unwinder,
    price_cache: Arc<PriceCache>,
    pair_store: Arc<PairStore>,
    config: EngineConfig,
    shutdown_tx: broadcast::Sender<()>,
}

pub struct EngineConfig {
    pub scan_interval: Duration,
    pub enabled: bool,
}

impl Engine {
    /// Start the engine. This spawns background tasks and returns a JoinHandle.
    pub async fn start(self: Arc<Self>) -> Result<(), ArbError> {
        let (shutdown_tx, _) = broadcast::channel(1);

        // Task 1: Price feed consumer
        // Reads from mpsc channel, updates PriceCache
        let engine = self.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(update) = price_rx.recv() => {
                        engine.price_cache.update(&update);
                    }
                    _ = shutdown_rx.recv() => break,
                }
            }
        });

        // Task 2: Opportunity scanner
        // Runs on interval, calls detector.scan(), queues for execution
        let engine = self.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(engine.config.scan_interval);
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let pairs = engine.pair_store.active_pairs().await.unwrap_or_default();
                        let opportunities = engine.detector.scan(&pairs);
                        for mut opp in opportunities {
                            // Spawn execution as separate task (don't block scanner)
                            let engine = engine.clone();
                            tokio::spawn(async move {
                                engine.handle_opportunity(&mut opp).await;
                            });
                        }
                    }
                    _ = shutdown_rx.recv() => break,
                }
            }
        });

        Ok(())
    }

    async fn handle_opportunity(self: &Arc<Self>, opp: &mut Opportunity) {
        // 1. Execute
        match self.executor.execute(opp).await {
            Ok(result) => {
                // 2. Monitor fills
                match self.monitor.watch_order_pair(
                    &result.poly_order.platform_order_id,
                    &result.kalshi_order.platform_order_id,
                    result.executed_at,
                ).await {
                    Ok(MonitorAction::BothFilled { poly_order, kalshi_order }) => {
                        // 3a. Both filled — create position
                        let _ = self.tracker.create_position(opp, &poly_order, &kalshi_order).await;
                    }
                    Ok(MonitorAction::NeedsUnwind { filled_platform, filled_order, unfilled_order_id }) => {
                        // 3b. One-legged — unwind
                        let _ = self.unwinder.unwind(filled_platform, &filled_order, &unfilled_order_id).await;
                    }
                    Ok(MonitorAction::BothCancelled) => {
                        tracing::info!(opp_id = %opp.id, "both orders expired, no action needed");
                    }
                    Ok(MonitorAction::Waiting) => unreachable!(),
                    Ok(MonitorAction::PartialHedge { .. }) => unreachable!(),
                    Err(e) => {
                        tracing::error!(opp_id = %opp.id, "monitor error: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::error!(opp_id = %opp.id, "execution failed: {}", e);
            }
        }
    }

    /// Graceful shutdown: cancel all open orders, save state.
    pub async fn shutdown(&self) {
        tracing::info!("engine shutting down — cancelling open orders");
        let _ = self.shutdown_tx.send(());

        // Cancel all open orders on both platforms
        if let Ok(poly_orders) = self.executor.poly.list_open_orders().await {
            for order in poly_orders {
                let _ = self.executor.poly.cancel_order(&order.platform_order_id).await;
            }
        }
        if let Ok(kalshi_orders) = self.executor.kalshi.list_open_orders().await {
            for order in kalshi_orders {
                let _ = self.executor.kalshi.cancel_order(&order.platform_order_id).await;
            }
        }
        tracing::info!("engine shutdown complete");
    }
}
```

### 4.4 Concurrency Model Summary

```
Main thread (tokio runtime):
  ├── Task: price_feed_consumer (reads WS → updates PriceCache)
  ├── Task: opportunity_scanner (timer → scan → spawn execution)
  │     └── Task: handle_opportunity (per-opportunity, spawned)
  │           ├── executor.execute() [places orders]
  │           ├── monitor.watch_order_pair() [polls until resolved]
  │           └── tracker.create_position() OR unwinder.unwind()
  └── Task: (Phase 5) TUI renderer
```

**Shared state:**
- `PriceCache` — `parking_lot::RwLock` (read-heavy, fast)
- `RiskManager` — `parking_lot::RwLock` (read-heavy: pre-trade checks; write: exposure updates)
- `Database` — `sqlx::SqlitePool` (connection pool handles concurrency)
- Connectors — `Arc<dyn PredictionMarketConnector>` (internally handle rate limiting)

### 4.5 Test Strategy

**Unit tests (per module):**
```
detector:
  - Detects opportunity when spread > threshold
  - Skips when spread < threshold
  - Skips stale prices (> max_staleness)
  - Picks correct direction (poly_yes+kalshi_no vs poly_no+kalshi_yes)
  - Returns empty vec when no active pairs

executor:
  - Both legs succeed → returns ExecutionResult
  - Poly fails, Kalshi succeeds → cancels Kalshi, returns error
  - Kalshi fails, Poly succeeds → cancels Poly, returns error
  - Both fail → returns error, no cleanup needed
  - Risk check blocks trade → returns RiskError (not placed)

monitor:
  - Both fill within 5s → BothFilled
  - One fills, other times out at max_hedge_wait → NeedsUnwind
  - Neither fills within max_order_age → BothCancelled

unwinder:
  - Successfully sells position at best bid
  - Records loss in exposure tracker
  - Handles "no bids" gracefully

tracker:
  - Creates position with correct hedged/unhedged quantities
  - Calculates guaranteed profit correctly
  - Updates exposure tracker
```

**Integration tests (with mock connectors):**
```
full_cycle:
  - Set up mock connectors with pre-loaded prices
  - Inject price update that creates a spread > threshold
  - Verify: detector fires, executor places both orders, monitor sees fills
  - Verify: position created in DB with correct profit
  - Verify: exposure tracker updated

unwind_cycle:
  - Set up mock where Kalshi order never fills
  - Verify: monitor triggers unwind after max_hedge_wait
  - Verify: Poly position unwound at best bid
  - Verify: loss recorded in exposure tracker

risk_block:
  - Set up mock with exposure at 95% of limit
  - Inject opportunity
  - Verify: pre-trade check blocks, no orders placed
```

### 4.6 Acceptance Criteria

- [ ] Engine starts, subscribes to WS feeds, updates PriceCache
- [ ] Detector scans on interval, finds opportunities above threshold
- [ ] Executor places both legs simultaneously via `tokio::join!`
- [ ] Monitor polls fills and resolves to correct action within timeouts
- [ ] Unwinder exits one-legged positions and records loss
- [ ] Tracker creates positions with correct P&L math
- [ ] Graceful shutdown cancels all open orders
- [ ] All unit + integration tests pass (>= 30 new tests)
- [ ] No `unwrap()` on any Result in engine code
- [ ] Engine handles connector errors without panicking

### 4.7 Build Prompts

**Prompt 4-A: PriceCache + Detector**
```
Crate: arb-engine
Create: lib.rs (module declarations), price_cache.rs, detector.rs, types.rs
Read: arb-types (all types), SPEC §8.1-8.3
Implement: PriceCache (RwLock<HashMap>), Detector::scan(), spread calculation
Tests: all detector unit tests (5 tests minimum)
Dependencies: arb-types, parking_lot, rust_decimal, chrono
Verify: cargo test -p arb-engine
```

**Prompt 4-B: Executor + Error Handling**
```
Crate: arb-engine
Create: executor.rs
Read: arb-types (PredictionMarketConnector trait, Order types), arb-risk (pre_trade_check), SPEC §9.2
Implement: Executor with dual-leg placement, all 4 error cases, risk check integration
Tests: all executor unit tests (5 tests minimum, using mock connectors)
Verify: cargo test -p arb-engine
```

**Prompt 4-C: Monitor + Unwinder**
```
Crate: arb-engine
Create: monitor.rs, unwinder.rs
Read: SPEC §9.1-9.5, executor.rs (for types), arb-risk exposure tracker
Implement: Monitor state machine (poll loop), Unwinder (cancel + sell + record loss)
Tests: monitor state transitions, unwind loss calculation
Verify: cargo test -p arb-engine
```

**Prompt 4-D: Tracker + Engine Orchestration**
```
Crate: arb-engine
Create: tracker.rs, update lib.rs with Engine struct
Read: arb-db (repo), arb-risk (exposure), all engine modules
Implement: Tracker (create_position, settle_position), Engine::start() with spawned tasks, Engine::shutdown()
Tests: position creation, full integration with mocks
Verify: cargo test -p arb-engine && cargo test --workspace
```

**Execution order:** 4-A first (detector needs PriceCache), 4-B second (executor needs detector types), 4-C third (monitor/unwinder need executor types), 4-D last (orchestration needs everything).

---

## 5. Phase 5: TUI + Paper Trading + Production Readiness

**Goal:** Terminal dashboard showing live state, paper trading mode for safe validation, graceful shutdown, and production deployment readiness.

**Crates affected:** `arb-cli` (main), `arb-engine` (paper mode wrapper)

### 5.1 Paper Trading Mode

**Create:** `crates/arb-engine/src/paper.rs`

```rust
/// Wraps real connectors but intercepts trading calls.
/// Uses real price feeds, simulates order execution.
pub struct PaperConnector {
    inner: Arc<dyn PredictionMarketConnector>,
    state: Mutex<PaperState>,
    fill_probability: f64,     // 0.0 - 1.0, default 0.85
    fill_delay: Duration,      // simulated fill delay, default 2s
}

struct PaperState {
    next_order_id: u64,
    orders: HashMap<String, PaperOrder>,
    positions: Vec<PlatformPosition>,
    balance: Decimal,
}

#[async_trait]
impl PredictionMarketConnector for PaperConnector {
    // Market data methods: delegate to inner (real prices)
    async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>, ArbError> {
        self.inner.list_markets(status).await
    }
    async fn get_order_book(&self, id: &str) -> Result<OrderBook, ArbError> {
        self.inner.get_order_book(id).await
    }
    async fn subscribe_prices(&self, ids: &[String], tx: mpsc::Sender<PriceUpdate>) -> Result<SubHandle, ArbError> {
        self.inner.subscribe_prices(ids, tx).await
    }

    // Trading methods: simulate locally
    async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<OrderResponse, ArbError> {
        let mut state = self.state.lock();

        // Check balance
        let cost = req.price * Decimal::from(req.quantity);
        if state.balance < cost {
            return Err(ArbError::Order("paper: insufficient balance".into()));
        }

        let order_id = format!("paper-{}", state.next_order_id);
        state.next_order_id += 1;

        // Simulate fill based on probability
        let will_fill = rand::random::<f64>() < self.fill_probability;
        let paper_order = PaperOrder {
            id: order_id.clone(),
            req: req.clone(),
            will_fill,
            fill_at: if will_fill {
                Some(Utc::now() + chrono::Duration::from_std(self.fill_delay).unwrap())
            } else {
                None
            },
        };
        state.orders.insert(order_id.clone(), paper_order);

        if will_fill {
            state.balance -= cost;
        }

        Ok(OrderResponse {
            platform_order_id: order_id,
            status: OrderStatus::Open,
            price: req.price,
            quantity: req.quantity,
            filled_quantity: 0,
            ..Default::default()
        })
    }

    async fn get_order(&self, order_id: &str) -> Result<OrderResponse, ArbError> {
        let state = self.state.lock();
        let paper = state.orders.get(order_id)
            .ok_or(ArbError::Order(format!("paper order {} not found", order_id)))?;

        let status = if paper.will_fill {
            if Utc::now() >= paper.fill_at.unwrap() {
                OrderStatus::Filled
            } else {
                OrderStatus::Open
            }
        } else {
            OrderStatus::Open // will eventually be cancelled by monitor timeout
        };

        let filled_qty = if status == OrderStatus::Filled {
            paper.req.quantity
        } else {
            0
        };

        Ok(OrderResponse {
            platform_order_id: order_id.to_string(),
            status,
            price: paper.req.price,
            quantity: paper.req.quantity,
            filled_quantity: filled_qty,
            market_id: paper.req.market_id.clone(),
            side: paper.req.side.clone(),
            ..Default::default()
        })
    }

    // ... implement remaining trait methods similarly
}
```

**🔴 CRITICAL SAFETY: Paper vs Live separation:**
```rust
// In arb-cli main.rs:
let (poly_connector, kalshi_connector): (Arc<dyn PredictionMarketConnector>, Arc<dyn PredictionMarketConnector>) =
    if cli.paper {
        tracing::warn!("🧪 PAPER TRADING MODE — no real orders will be placed");
        (
            Arc::new(PaperConnector::new(real_poly, dec!(10000), 0.85, Duration::from_secs(2))),
            Arc::new(PaperConnector::new(real_kalshi, dec!(10000), 0.85, Duration::from_secs(2))),
        )
    } else {
        tracing::warn!("💰 LIVE TRADING MODE — real money at risk");
        (Arc::new(real_poly), Arc::new(real_kalshi))
    };
```

### 5.2 TUI Dashboard

**Create:** `crates/arb-cli/src/tui.rs`

**Dependencies to add:** `ratatui = "0.29"`, `crossterm = "0.28"`

**Layout (SPEC §3.1):**
```
┌─────────────────── Prediction Market Arbitrage ───────────────────┐
│ Status: RUNNING │ Mode: PAPER │ Uptime: 2h 15m │ Exposure: $4,250│
├───────────── Active Pairs ─────────────┬──── Open Orders ────────┤
│ Pair               Spread  Status      │ ID    Platform  Status  │
│ Biden 2028    3.2%  WATCHING           │ P-42  Poly      OPEN    │
│ Fed Rate Cut  4.1%  EXECUTING          │ K-17  Kalshi    FILLED  │
│ ...                                    │ ...                     │
├────────── Positions ──────────────────┬──── Recent Trades ───────┤
│ Pair            Hedged  Profit  Age   │ Time   Pair  Spread P&L  │
│ Biden 2028     50 cts  $1.60   3d    │ 14:32  Fed   4.1%  $2.05│
│ ...                                   │ ...                      │
├──────────── P&L Summary ──────────────┴──────────────────────────┤
│ Today: +$3.20 │ This Week: +$18.50 │ Total: +$142.30            │
│ Trades: 5     │ Fill Rate: 82%     │ Unwind Rate: 8%            │
├──────────────────────────────────────────────────────────────────┤
│ q:quit  p:pause  r:resume  m:markets  o:orders  /:filter        │
└──────────────────────────────────────────────────────────────────┘
```

**TUI event loop pattern (best practice for async + ratatui):**
```rust
pub async fn run_tui(engine: Arc<Engine>, db: Arc<Database>) -> Result<()> {
    let mut terminal = ratatui::init();
    let tick_rate = Duration::from_millis(250);

    loop {
        // Draw
        terminal.draw(|frame| {
            draw_dashboard(frame, &engine, &db);
        })?;

        // Handle input with timeout
        if crossterm::event::poll(tick_rate)? {
            if let Event::Key(key) = crossterm::event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('p') => engine.pause(),
                    KeyCode::Char('r') => engine.resume(),
                    _ => {}
                }
            }
        }
    }

    ratatui::restore();
    Ok(())
}
```

**Important:** The TUI runs in the main thread. The engine runs in spawned tokio tasks. The TUI reads state from shared `Arc<Engine>` and `Arc<Database>` — it never blocks the engine.

### 5.3 Graceful Shutdown

**In `arb-cli/src/main.rs`:**
```rust
// Set up Ctrl+C handler
let engine_for_shutdown = engine.clone();
tokio::spawn(async move {
    tokio::signal::ctrl_c().await.expect("failed to listen for ctrl+c");
    tracing::info!("ctrl+c received, initiating graceful shutdown");
    engine_for_shutdown.shutdown().await;
});
```

### 5.4 Health File

**Create `data/health.json` updated every 30s:**
```json
{
    "status": "running",
    "mode": "paper",
    "uptime_secs": 8100,
    "last_price_update": "2026-04-06T12:00:00Z",
    "open_orders": 2,
    "open_positions": 3,
    "total_exposure": "4250.00",
    "daily_pnl": "3.20",
    "errors_last_hour": 0
}
```

### 5.5 Acceptance Criteria

- [ ] `arb --paper` starts with paper trading connectors, places no real orders
- [ ] `arb --paper` logs simulated P&L to database
- [ ] TUI displays all panels from SPEC §3.1
- [ ] TUI responds to keyboard input (q, p, r)
- [ ] TUI does not block or lag the engine
- [ ] Ctrl+C triggers graceful shutdown: all open orders cancelled
- [ ] Health file updates every 30s
- [ ] 1 week of stable paper trading against live price feeds

### 5.6 Build Prompts

**Prompt 5-A: Paper trading connector**
```
Crate: arb-engine
Create: paper.rs
Read: arb-types PredictionMarketConnector trait, connector implementations
Implement: PaperConnector wrapping real connectors, simulated fills
Tests: paper connector fill/no-fill, balance tracking, order status transitions
Verify: cargo test -p arb-engine
```

**Prompt 5-B: TUI dashboard**
```
Crate: arb-cli
Create: tui.rs, update main.rs
Add deps: ratatui, crossterm to workspace + arb-cli Cargo.toml
Read: SPEC §3.1, engine lib.rs (for state access)
Implement: full TUI with 6 panels, key bindings, 250ms refresh
Tests: compile + manual visual verification
Verify: cargo build && cargo run -- --paper --tui
```

**Prompt 5-C: Shutdown + health + production polish**
```
Crate: arb-cli
Modify: main.rs
Implement: Ctrl+C handler, health file writer, startup banner with mode/config info
Add: --headless flag (no TUI, just engine + logging)
Tests: verify shutdown cancels mock orders
Verify: cargo build && cargo run -- --paper --headless
```

**Execution order:** 5-A first (paper connector needed for safe testing), then 5-B and 5-C in parallel.

---

## 6. Cross-Phase Patterns

### 6.1 Error Handling Convention

Every crate follows this hierarchy:
```
Application crate (arb-cli):  anyhow::Result for top-level
Library crates:               typed errors via thiserror
Connectors:                   ConnectorError → ArbError::Connector
Risk:                         RiskError → ArbError::Risk
Database:                     sqlx::Error → ArbError::Database
```

**NEVER use `.unwrap()` on:**
- API responses
- Header parsing
- Decimal conversions
- Database queries
- Channel sends/receives

**Acceptable `.unwrap()` only on:**
- `Uuid::now_v7()` (infallible)
- Static regex compilation in `lazy_static!` or `once_cell`
- Test code

### 6.2 Logging Convention

```rust
// Module-level spans
#[tracing::instrument(skip(self))]
async fn execute(&self, opp: &mut Opportunity) -> Result<ExecutionResult> { ... }

// Use structured fields, not format strings
tracing::info!(
    pair_id = %opp.pair_id,
    spread = %opp.spread,
    poly_price = %opp.poly_price,
    "opportunity detected"
);

// Error logging always includes context
tracing::error!(
    opp_id = %opp.id,
    platform = %platform,
    error = %e,
    "order placement failed"
);
```

### 6.3 Testing Convention

Every module file has a `#[cfg(test)] mod tests` at the bottom:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_thing() { ... }

    #[tokio::test]
    async fn test_async_thing() { ... }
}
```

For integration tests requiring mock connectors, use the feature-gated mocks:
```rust
#[cfg(test)]
mod integration_tests {
    use arb_polymarket::MockConnector as PolyMock;
    use arb_kalshi::MockConnector as KalshiMock;
    // ...
}
```

### 6.4 Shared Constants

Add to `arb-types/src/lib.rs`:
```rust
pub mod constants {
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    pub const MIN_PRICE: Decimal = dec!(0.01);
    pub const MAX_PRICE: Decimal = dec!(0.99);
    pub const PAYOUT: Decimal = dec!(1.00);
    pub const USDC_DECIMALS: u32 = 6;
    pub const USDC_SCALE: u64 = 1_000_000;
}
```

---

## 7. Risk & Security Guardrails

### 7.1 Non-Negotiable Safety Rules (enforce in code)

| Rule | Where | How |
|------|-------|-----|
| Never trade unverified pairs | `executor.rs` | Check `pair.verified == true` before placing orders |
| Never auto-deploy on depeg | N/A (future) | Human approval required |
| Cancel both legs if one fails | `executor.rs` | Done in error handling branches |
| Graceful shutdown cancels orders | `engine.rs` | `Engine::shutdown()` |
| Paper mode NEVER calls real trade endpoints | `paper.rs` | Only delegates market data, not trading |
| Secrets never in logs | All configs | Manual `Debug` impl with redaction |
| Price always in [0.01, 0.99] | Connectors + detector | Validate on receive + before trade |
| Max exposure enforced | `risk/manager.rs` | Pre-trade check #7 |
| Daily loss circuit breaker | `risk/manager.rs` | Pre-trade check #9 |
| Max 2 concurrent executions | `engine.rs` | Semaphore on `handle_opportunity()` |

### 7.2 Execution Concurrency Limit

The engine should limit concurrent order executions to prevent overwhelming rate limits and risk controls:

```rust
// In Engine
let execution_semaphore = Arc::new(Semaphore::new(2)); // max 2 concurrent executions

// In opportunity_scanner task
let permit = execution_semaphore.clone().acquire_owned().await?;
tokio::spawn(async move {
    engine.handle_opportunity(&mut opp).await;
    drop(permit); // release when done
});
```

### 7.3 Phase Implementation Order & Gates

```
Phase 2.5: Hardening     → Gate: cargo test --workspace >= 90 tests, zero HIGH findings
     ↓
Phase 3: Matcher          → Gate: --match command works, known pairs score > 0.85
     ↓
Phase 4: Engine           → Gate: full cycle works with mock connectors, all tests pass
     ↓
Phase 5: TUI + Paper      → Gate: 1 week stable paper trading, TUI renders correctly
     ↓
PRODUCTION                → Gate: 2 weeks paper trading profitable, all safety rules verified
```

---

*End of Phase Scroll. This document is the single source of truth for all remaining implementation work. Each phase's build prompts are designed for one-pass implementation by a Backend Dev with no iteration needed.*
