# Solana DEX-CEX Arbitrage System — Technical Architecture

**Version**: 1.0.0
**Date**: 2026-04-05
**Status**: Ready for Implementation
**Author**: Architect Agent

---

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Workspace Structure](#2-workspace-structure)
3. [Complete Workspace Cargo.toml](#3-complete-workspace-cargotoml)
4. [Dependency Graph](#4-dependency-graph)
5. [Domain Types](#5-domain-types)
6. [Trait Definitions](#6-trait-definitions)
7. [Execution Flow](#7-execution-flow)
8. [Solana Integration Details](#8-solana-integration-details)
9. [Data Flow Architecture](#9-data-flow-architecture)
10. [Error Handling Strategy](#10-error-handling-strategy)
11. [Rebalancing Architecture](#11-rebalancing-architecture)
12. [Database Schema](#12-database-schema)
13. [Configuration](#13-configuration)
14. [HTTP API Specification](#14-http-api-specification)
15. [Implementation Roadmap](#15-implementation-roadmap)

---

## 1. System Overview

### 1.1 What It Does

Arbitrages price differences between Solana DEXs (Jupiter, Raydium) and Binance CEX on mid-cap Solana tokens. Capital is pre-positioned on both sides — no cross-chain bridging required.

**Target pairs**: SOL/USDC, JUP/USDC, BONK/USDC, WIF/USDC, PYTH/USDC

### 1.2 How It Works

```
1. Monitor Binance orderbook (WebSocket) and Solana DEX prices (RPC polling)
2. Detect when cross-venue spread exceeds all-in cost threshold
3. Execute BOTH legs concurrently: DEX swap + CEX IOC order
4. Record result, update balances, repeat
```

**Two directions of arbitrage:**

| Direction | When | DEX Action | CEX Action |
|-----------|------|------------|------------|
| **Buy DEX, Sell CEX** | DEX price < CEX bid − fees | Swap USDC → TOKEN on Jupiter/Raydium | Sell TOKEN on Binance (IOC) |
| **Buy CEX, Sell DEX** | CEX ask < DEX price − fees | Buy TOKEN on Binance (IOC) | Swap TOKEN → USDC on Jupiter/Raydium |

### 1.3 Fee Model

| Component | Cost | Notes |
|-----------|------|-------|
| Raydium swap fee | 0.25% | Constant-product AMM standard pool |
| Jupiter platform fee | 0% | Routes through underlying DEXs; their fees apply |
| Jupiter via low-fee pool | 0.01–0.04% | Whirlpool concentrated liquidity, if available |
| Binance taker fee | 0.10% | 0.075% with BNB discount |
| Solana base tx fee | ~0.000005 SOL | Fixed per signature |
| Priority fee | 0.0001–0.001 SOL | Compute unit price for faster inclusion |
| Jito tip | 0.001–0.01 SOL | MEV-protected bundle inclusion |
| **Minimum profitable spread** | **~0.35%** | Via Raydium direct |
| **Minimum profitable spread** | **~0.10%** | Via Jupiter (low-fee routes) |

### 1.4 Constraints

- **Single binary** — no Docker, no microservices
- **SQLite** — not Postgres; single-file database
- **Single developer** — ~140 hours total implementation budget
- **Pre-positioned capital** — no automated bridging between chains
- **Paper trading mode** — mandatory before live capital

---

## 2. Workspace Structure

```
sol-arb/
├── Cargo.toml                              # Workspace root
├── Cargo.lock
├── config/
│   ├── default.toml                        # Default configuration
│   └── pairs.toml                          # Trading pair definitions
├── migrations/
│   ├── 001_initial_schema.sql
│   └── 002_daily_pnl.sql
├── .env.example
├── README.md
└── crates/
    ├── sol-arb-types/                      # Shared domain types (ZERO internal deps)
    │   └── src/
    │       ├── lib.rs                      # Re-exports
    │       ├── pair.rs                     # TradingPair, TokenInfo, Venue
    │       ├── price.rs                    # PriceUpdate, DexPriceUpdate, CexPriceUpdate, Spread
    │       ├── order.rs                    # OrderRequest, OrderResponse, OrderSide, OrderStatus
    │       ├── swap.rs                     # SwapQuoteRequest, SwapQuote, SwapExecuteRequest, SwapResult
    │       ├── opportunity.rs              # ArbitrageOpportunity, OpportunityStatus, Direction
    │       ├── execution.rs                # TradeExecution, ExecutionLeg, LegStatus, ExecutionOutcome
    │       ├── balance.rs                  # Balance, BalanceSnapshot, VenueBalances
    │       ├── risk.rs                     # RiskCheck, RiskViolation, CircuitBreakerState
    │       ├── config.rs                   # All config structs (engine, risk, dex, cex, db)
    │       └── error.rs                    # Common error types, ArbitrageError enum
    │
    ├── sol-arb-dex/                        # Solana DEX connectors
    │   └── src/
    │       ├── lib.rs                      # DexConnector trait, re-exports
    │       ├── jupiter.rs                  # JupiterConnector — V6 API (quote + swap via reqwest)
    │       ├── raydium.rs                  # RaydiumConnector — on-chain pool reads + swap
    │       ├── pool.rs                     # PoolState, reserve parsing, price calculation
    │       ├── jito.rs                     # Jito bundle builder, tip management, gRPC submission
    │       ├── tx.rs                       # Transaction signing, priority fees, confirmation polling
    │       ├── mock.rs                     # MockDexConnector for testing + paper trading
    │       └── error.rs                    # DexError enum
    │
    ├── sol-arb-cex/                        # Binance connector
    │   └── src/
    │       ├── lib.rs                      # CexConnector trait, re-exports
    │       ├── binance.rs                  # BinanceConnector — REST client
    │       ├── ws.rs                       # Binance WebSocket feed (orderbook, trades)
    │       ├── auth.rs                     # HMAC-SHA256 request signing
    │       ├── rate_limit.rs              # Rate limiter (1200 req/min weight)
    │       ├── mock.rs                     # MockCexConnector for testing + paper trading
    │       └── error.rs                    # CexError enum
    │
    ├── sol-arb-engine/                     # Core arbitrage logic
    │   └── src/
    │       ├── lib.rs                      # ArbitrageEngine, re-exports
    │       ├── detector.rs                 # SpreadDetector — compares DEX vs CEX prices
    │       ├── executor.rs                 # TradeExecutor — concurrent dual-leg execution
    │       ├── reconciler.rs              # Reconciles execution results, handles partial fills
    │       ├── price_feed.rs              # Merges DEX + CEX price streams into unified feed
    │       ├── fee_model.rs               # All-in fee calculation per venue + direction
    │       └── error.rs                    # EngineError enum
    │
    ├── sol-arb-risk/                       # Risk management
    │   └── src/
    │       ├── lib.rs                      # RiskManager trait + DefaultRiskManager, re-exports
    │       ├── circuit_breaker.rs          # Auto-halt on consecutive losses or error bursts
    │       ├── limits.rs                   # Daily loss limit, position size limit, exposure limit
    │       ├── stale.rs                    # Stale price detection (max age thresholds)
    │       ├── balance_monitor.rs          # Cross-venue balance ratio tracking, rebalance alerts
    │       └── error.rs                    # RiskError enum (each variant = a specific violation)
    │
    ├── sol-arb-db/                         # SQLite persistence
    │   └── src/
    │       ├── lib.rs                      # Database init, migration runner, re-exports
    │       ├── repo.rs                     # Repository trait + SqliteRepository
    │       ├── models.rs                   # DB row structs (FromRow derives)
    │       ├── queries.rs                  # Named query constants
    │       └── writer.rs                   # Background writer task (batched inserts via mpsc)
    │
    ├── sol-arb-server/                     # Axum HTTP API
    │   └── src/
    │       ├── lib.rs                      # Router construction, AppState, re-exports
    │       ├── routes.rs                   # 6 REST endpoints + 1 WebSocket endpoint
    │       ├── ws.rs                       # WebSocket handler — live price/opportunity stream
    │       ├── dto.rs                      # Request/response DTOs (separate from domain types)
    │       └── error.rs                    # ApiError → HTTP status code mapping
    │
    └── sol-arb-cli/                        # Binary entry point
        └── src/
            ├── main.rs                     # Entrypoint, arg parsing, config loading, startup orchestration
            ├── shutdown.rs                 # Graceful shutdown (SIGINT/SIGTERM → CancellationToken)
            └── paper.rs                    # Paper trading coordinator (mock connectors + real prices)
```

**Total: 8 crates, 46 source files.**

---

## 3. Complete Workspace Cargo.toml

```toml
[workspace]
resolver = "2"
members = [
    "crates/sol-arb-types",
    "crates/sol-arb-dex",
    "crates/sol-arb-cex",
    "crates/sol-arb-engine",
    "crates/sol-arb-risk",
    "crates/sol-arb-db",
    "crates/sol-arb-server",
    "crates/sol-arb-cli",
]

[workspace.dependencies]
# ── Async Runtime ──
tokio = { version = "1.44", features = ["full"] }
futures = "0.3"
futures-util = "0.3"
async-trait = "0.1"

# ── Serialization ──
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# ── Precision Types ──
rust_decimal = { version = "1.36", features = ["serde-with-str"] }
rust_decimal_macros = "1.36"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1.11", features = ["v7", "serde"] }

# ── HTTP / WebSocket ──
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
tokio-tungstenite = { version = "0.26", features = ["rustls-tls-native-roots"] }
axum = { version = "0.8", features = ["ws"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["cors", "trace"] }

# ── Solana ──
solana-sdk = "2.2"
solana-client = "2.2"
solana-account-decoder = "2.2"
solana-transaction-status = "2.2"
spl-token = "7.0"
spl-associated-token-account = "5.0"
anchor-client = "0.30"
borsh = "1.5"

# ── Jito MEV ──
jito-protos = "2.1"
jito-searcher-client = "2.1"
tonic = { version = "0.12", features = ["tls"] }
prost = "0.13"

# ── Crypto / Auth ──
hmac = "0.12"
sha2 = "0.10"
hex = "0.4"
base64 = "0.22"
ed25519-dalek = "2.1"

# ── Database ──
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "chrono", "uuid", "json"] }

# ── Observability ──
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# ── Error Handling ──
thiserror = "2.0"
anyhow = "1.0"

# ── Concurrency ──
dashmap = "6.1"
parking_lot = "0.12"
tokio-util = { version = "0.7", features = ["rt"] }

# ── Configuration ──
config = { version = "0.15", features = ["toml"] }
dotenvy = "0.15"
clap = { version = "4.5", features = ["derive"] }

# ── Internal Crates ──
sol-arb-types  = { path = "crates/sol-arb-types" }
sol-arb-dex    = { path = "crates/sol-arb-dex" }
sol-arb-cex    = { path = "crates/sol-arb-cex" }
sol-arb-engine = { path = "crates/sol-arb-engine" }
sol-arb-risk   = { path = "crates/sol-arb-risk" }
sol-arb-db     = { path = "crates/sol-arb-db" }
sol-arb-server = { path = "crates/sol-arb-server" }

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "abort"
strip = true
```

### Per-Crate Dependencies

| Crate | Dependencies |
|-------|-------------|
| `sol-arb-types` | `serde`, `rust_decimal`, `chrono`, `uuid`, `thiserror`, `solana-sdk` (Pubkey only) |
| `sol-arb-dex` | `sol-arb-types`, `solana-sdk`, `solana-client`, `solana-account-decoder`, `solana-transaction-status`, `spl-token`, `spl-associated-token-account`, `anchor-client`, `borsh`, `jito-protos`, `jito-searcher-client`, `tonic`, `prost`, `reqwest`, `tokio`, `tracing`, `thiserror`, `async-trait`, `serde_json`, `base64` |
| `sol-arb-cex` | `sol-arb-types`, `reqwest`, `tokio-tungstenite`, `tokio`, `hmac`, `sha2`, `hex`, `tracing`, `thiserror`, `async-trait`, `serde_json`, `futures-util`, `dashmap`, `parking_lot` |
| `sol-arb-engine` | `sol-arb-types`, `sol-arb-dex`, `sol-arb-cex`, `sol-arb-risk`, `sol-arb-db`, `tokio`, `tracing`, `thiserror`, `async-trait`, `rust_decimal`, `chrono`, `uuid`, `dashmap` |
| `sol-arb-risk` | `sol-arb-types`, `tokio`, `tracing`, `thiserror`, `async-trait`, `rust_decimal`, `chrono`, `parking_lot` |
| `sol-arb-db` | `sol-arb-types`, `sqlx`, `tokio`, `tracing`, `thiserror`, `async-trait`, `chrono`, `uuid` |
| `sol-arb-server` | `sol-arb-types`, `sol-arb-engine`, `sol-arb-db`, `axum`, `tower`, `tower-http`, `tokio`, `tracing`, `serde`, `serde_json`, `uuid` |
| `sol-arb-cli` | `sol-arb-types`, `sol-arb-dex`, `sol-arb-cex`, `sol-arb-engine`, `sol-arb-risk`, `sol-arb-db`, `sol-arb-server`, `tokio`, `tracing`, `tracing-subscriber`, `anyhow`, `config`, `dotenvy`, `clap`, `tokio-util` |

---

## 4. Dependency Graph

```
sol-arb-cli (binary: sol-arb)
  ├── sol-arb-server
  │     ├── sol-arb-engine
  │     │     ├── sol-arb-dex ──────► sol-arb-types
  │     │     ├── sol-arb-cex ──────► sol-arb-types
  │     │     ├── sol-arb-risk ─────► sol-arb-types
  │     │     ├── sol-arb-db ───────► sol-arb-types
  │     │     └── sol-arb-types
  │     ├── sol-arb-db ─────────────► sol-arb-types
  │     └── sol-arb-types
  ├── sol-arb-engine (see above)
  ├── sol-arb-dex ──────────────────► sol-arb-types
  ├── sol-arb-cex ──────────────────► sol-arb-types
  ├── sol-arb-risk ─────────────────► sol-arb-types
  ├── sol-arb-db ───────────────────► sol-arb-types
  └── sol-arb-types (ZERO internal deps)
```

**Layered view:**

```
Layer 4 (Binary):     sol-arb-cli
                          │
Layer 3 (API):        sol-arb-server
                          │
Layer 2 (Logic):      sol-arb-engine ──── sol-arb-risk
                       │        │
Layer 1 (I/O):    sol-arb-dex  sol-arb-cex  sol-arb-db
                       │        │              │
Layer 0 (Types):      sol-arb-types ◄──────────┘
```

**No circular dependencies.** `sol-arb-types` has zero internal crate dependencies. Every crate depends on `sol-arb-types` and nothing depends on `sol-arb-cli`.

---

## 5. Domain Types

### 5.1 Core Enums (`sol-arb-types`)

```rust
// ── pair.rs ──

/// Which venue we're interacting with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Venue {
    Jupiter,
    Raydium,
    Binance,
}

/// Which DEX specifically (used when Jupiter routes through multiple).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DexVenue {
    Jupiter,
    Raydium,
}

/// Direction of the arbitrage trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArbDirection {
    /// Buy on DEX (cheap), sell on CEX (expensive)
    BuyDexSellCex,
    /// Buy on CEX (cheap), sell on DEX (expensive)
    BuyCexSellDex,
}

/// A tradeable token pair.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TradingPair {
    pub base_symbol: String,            // e.g., "SOL"
    pub quote_symbol: String,           // e.g., "USDC"
    pub base_mint: Pubkey,              // Solana mint address
    pub quote_mint: Pubkey,             // USDC mint address
    pub binance_symbol: String,         // e.g., "SOLUSDC"
    pub base_decimals: u8,              // e.g., 9 for SOL
    pub quote_decimals: u8,             // e.g., 6 for USDC
    pub min_trade_size_usd: Decimal,    // Minimum trade in USD
    pub max_trade_size_usd: Decimal,    // Maximum trade in USD
}

/// Token information for Solana-side operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub symbol: String,
    pub mint: Pubkey,
    pub decimals: u8,
}
```

### 5.2 Price Types

```rust
// ── price.rs ──

/// A price update from a DEX source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexPriceUpdate {
    pub pair: TradingPair,
    pub venue: DexVenue,
    pub bid_price: Decimal,             // Best price to sell into (we receive this)
    pub ask_price: Decimal,             // Best price to buy at (we pay this)
    pub mid_price: Decimal,
    pub pool_liquidity_usd: Decimal,    // Total pool TVL in USD
    pub timestamp: DateTime<Utc>,
    pub slot: u64,                      // Solana slot number
}

/// A price update from the CEX orderbook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CexPriceUpdate {
    pub symbol: String,                 // Binance symbol, e.g., "SOLUSDC"
    pub best_bid: Decimal,              // Highest buy order
    pub best_bid_qty: Decimal,          // Size at best bid
    pub best_ask: Decimal,              // Lowest sell order
    pub best_ask_qty: Decimal,          // Size at best ask
    pub timestamp: DateTime<Utc>,
    pub event_time_ms: u64,             // Binance server time
}

/// Unified spread calculation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpreadInfo {
    pub pair: TradingPair,
    pub direction: ArbDirection,
    pub dex_price: Decimal,             // The DEX price relevant for this direction
    pub cex_price: Decimal,             // The CEX price relevant for this direction
    pub gross_spread_pct: Decimal,      // Before fees
    pub total_fees_pct: Decimal,        // All-in fees as percentage
    pub net_spread_pct: Decimal,        // gross - fees (must be > 0 to trade)
    pub estimated_profit_usd: Decimal,  // At reference trade size
    pub dex_timestamp: DateTime<Utc>,
    pub cex_timestamp: DateTime<Utc>,
}
```

### 5.3 Swap & Order Types

```rust
// ── swap.rs ──

/// Request to get a DEX swap quote.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapQuoteRequest {
    pub input_mint: Pubkey,
    pub output_mint: Pubkey,
    pub amount: u64,                    // In smallest unit (lamports, etc.)
    pub slippage_bps: u16,              // Max slippage in basis points
    pub dex_venue: DexVenue,            // Which DEX to query
}

/// A DEX swap quote response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapQuote {
    pub id: Uuid,
    pub input_mint: Pubkey,
    pub output_mint: Pubkey,
    pub input_amount: u64,
    pub output_amount: u64,
    pub minimum_output_amount: u64,     // After slippage
    pub price_impact_pct: Decimal,
    pub route_description: String,      // Human-readable route, e.g., "SOL → USDC via Raydium"
    pub fees: SwapFees,
    pub quote_timestamp: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,      // Quotes go stale fast
}

/// Breakdown of fees for a DEX swap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapFees {
    pub lp_fee_pct: Decimal,            // AMM liquidity provider fee
    pub platform_fee_pct: Decimal,      // Jupiter/aggregator fee (usually 0)
    pub priority_fee_lamports: u64,     // Solana priority fee
    pub jito_tip_lamports: u64,         // Jito bundle tip
    pub total_fee_usd: Decimal,         // Total in USD terms
}

/// Request to execute a DEX swap.
#[derive(Debug, Clone)]
pub struct SwapExecuteRequest {
    pub quote: SwapQuote,
    pub user_pubkey: Pubkey,
    pub use_jito: bool,                 // Wrap in Jito bundle?
    pub jito_tip_lamports: u64,
    pub priority_fee_lamports: u64,
    pub compute_unit_limit: u32,        // Max compute units for the tx
}

/// Result of a DEX swap execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapResult {
    pub signature: String,              // Solana transaction signature
    pub input_amount: u64,
    pub output_amount: u64,
    pub effective_price: Decimal,       // output/input in human units
    pub fees_paid: SwapFees,
    pub slot: u64,                      // Slot the tx landed in
    pub confirmed: bool,
    pub confirmation_time_ms: u64,      // Time from submit to confirmation
}

// ── order.rs ──

/// Side of a CEX order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum OrderSide {
    Buy,
    Sell,
}

/// Type of CEX order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CexOrderType {
    /// Immediate-or-Cancel — fill what you can, cancel the rest
    Ioc,
    /// Standard limit order
    Limit,
    /// Fill-or-Kill — all or nothing
    Fok,
}

/// Time-in-force for CEX orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TimeInForce {
    Gtc,    // Good-Till-Cancelled
    Ioc,    // Immediate-or-Cancel
    Fok,    // Fill-or-Kill
}

/// Request to place a CEX order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub symbol: String,                 // e.g., "SOLUSDC"
    pub side: OrderSide,
    pub order_type: CexOrderType,
    pub quantity: Decimal,              // In base asset units
    pub price: Decimal,                 // Limit price
    pub time_in_force: TimeInForce,
}

/// Response from a CEX order placement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResponse {
    pub order_id: String,               // Binance order ID
    pub client_order_id: String,        // Our internal ID
    pub symbol: String,
    pub side: OrderSide,
    pub order_type: CexOrderType,
    pub price: Decimal,
    pub quantity: Decimal,
    pub filled_quantity: Decimal,
    pub filled_quote_quantity: Decimal,  // How much USDC spent/received
    pub avg_fill_price: Decimal,
    pub status: CexOrderStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Status of a CEX order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CexOrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    Expired,
}

/// CEX account balance for one asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    pub asset: String,
    pub free: Decimal,                  // Available for trading
    pub locked: Decimal,                // In open orders
    pub total: Decimal,                 // free + locked
}
```

### 5.4 Opportunity & Execution Types

```rust
// ── opportunity.rs ──

/// A detected arbitrage opportunity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    pub id: Uuid,                       // UUID v7 (time-ordered)
    pub pair: TradingPair,
    pub direction: ArbDirection,
    pub dex_venue: DexVenue,
    pub dex_price: Decimal,             // Price on DEX side
    pub cex_price: Decimal,             // Price on CEX side
    pub gross_spread_pct: Decimal,
    pub net_spread_pct: Decimal,        // After ALL fees
    pub trade_size_base: Decimal,       // Amount in base token
    pub trade_size_usd: Decimal,        // Amount in USD
    pub estimated_profit_usd: Decimal,
    pub fees: AllInFees,
    pub status: OpportunityStatus,
    pub detected_at: DateTime<Utc>,
    pub executed_at: Option<DateTime<Utc>>,
    pub expired_at: Option<DateTime<Utc>>,
}

/// All-in fee breakdown for an opportunity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllInFees {
    pub dex_fee_pct: Decimal,
    pub cex_fee_pct: Decimal,
    pub sol_tx_fee_usd: Decimal,        // Priority + Jito tip in USD
    pub total_pct: Decimal,             // Sum of all fees as % of trade
}

/// Status of an opportunity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityStatus {
    Detected,
    Executing,
    Completed,
    PartialExecution,                   // One leg succeeded, other failed
    Failed,
    Expired,                            // Spread closed before execution
    RiskRejected,                       // Blocked by risk manager
}

// ── execution.rs ──

/// Record of an executed trade (both legs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeExecution {
    pub id: Uuid,
    pub opportunity_id: Uuid,
    pub pair: TradingPair,
    pub direction: ArbDirection,
    pub dex_leg: ExecutionLeg,
    pub cex_leg: ExecutionLeg,
    pub outcome: ExecutionOutcome,
    pub gross_profit_usd: Decimal,
    pub fees_usd: Decimal,
    pub net_profit_usd: Decimal,
    pub execution_time_ms: u64,         // Total time from start to both legs confirmed
    pub created_at: DateTime<Utc>,
}

/// One leg of the trade execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLeg {
    pub venue: Venue,
    pub side: OrderSide,                // Buy or Sell
    pub intended_price: Decimal,
    pub actual_price: Decimal,
    pub quantity: Decimal,
    pub filled_quantity: Decimal,
    pub fee_paid_usd: Decimal,
    pub status: LegStatus,
    pub venue_id: String,               // TX signature (DEX) or order ID (CEX)
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

/// Status of a single execution leg.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegStatus {
    Pending,
    Submitted,
    Confirmed,
    Failed,
    TimedOut,
}

/// Outcome of the full trade execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOutcome {
    /// Both legs executed successfully
    BothSucceeded,
    /// DEX leg succeeded, CEX leg failed
    DexOnlySucceeded,
    /// CEX leg succeeded, DEX leg failed
    CexOnlySucceeded,
    /// Both legs failed
    BothFailed,
}
```

### 5.5 Balance & Risk Types

```rust
// ── balance.rs ──

/// Snapshot of balances across both venues.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueBalances {
    pub solana_wallet: WalletBalances,
    pub binance_account: BinanceBalances,
    pub total_usd: Decimal,
    pub solana_pct: Decimal,            // % of total on Solana
    pub binance_pct: Decimal,           // % of total on Binance
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletBalances {
    pub sol_balance: Decimal,
    pub usdc_balance: Decimal,
    pub token_balances: HashMap<String, Decimal>,  // symbol → amount
    pub total_usd: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinanceBalances {
    pub usdc_balance: Decimal,
    pub token_balances: HashMap<String, Decimal>,
    pub total_usd: Decimal,
}

// ── risk.rs ──

/// Result of a risk check on an opportunity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskCheck {
    Approved,
    Rejected(RiskViolation),
}

/// Specific risk violation that blocked a trade.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskViolation {
    CircuitBreakerOpen { reason: String },
    DailyLossLimitExceeded { current: Decimal, limit: Decimal },
    PositionLimitExceeded { pair: String, current: Decimal, limit: Decimal },
    InsufficientBalance { venue: Venue, asset: String, required: Decimal, available: Decimal },
    StaleDexPrice { age_ms: u64, max_age_ms: u64 },
    StaleCexPrice { age_ms: u64, max_age_ms: u64 },
    SpreadBelowMinimum { spread_pct: Decimal, min_pct: Decimal },
    TradesSizeBelowMinimum { size_usd: Decimal, min_usd: Decimal },
    ImbalancedVenues { solana_pct: Decimal, threshold_pct: Decimal },
    CooldownActive { remaining_ms: u64 },
}

/// Circuit breaker state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitBreakerState {
    Closed,                             // Normal operation
    Open { until: DateTime<Utc> },      // Halted until timestamp
    HalfOpen,                           // Testing with reduced size
}
```

---

## 6. Trait Definitions

### 6.1 DexConnector (`sol-arb-dex/src/lib.rs`)

```rust
use async_trait::async_trait;
use sol_arb_types::*;
use std::result::Result as StdResult;
use tokio::sync::mpsc;

pub type Result<T> = StdResult<T, DexError>;

/// Handle to cancel a subscription.
pub struct SubscriptionHandle {
    cancel_tx: tokio::sync::oneshot::Sender<()>,
}

impl SubscriptionHandle {
    pub fn cancel(self) {
        let _ = self.cancel_tx.send(());
    }
}

#[async_trait]
pub trait DexConnector: Send + Sync + 'static {
    /// Get a swap quote: how much output for given input amount.
    /// Does NOT execute — just prices the swap including all fees.
    async fn get_quote(&self, params: &SwapQuoteRequest) -> Result<SwapQuote>;

    /// Execute a swap on-chain.
    /// For Jupiter: calls /swap API, signs the returned transaction, submits via RPC or Jito.
    /// For Raydium: builds the swap instruction, signs, submits.
    /// Returns after confirmation (or timeout).
    async fn execute_swap(&self, params: &SwapExecuteRequest) -> Result<SwapResult>;

    /// Subscribe to real-time price updates for the given trading pairs.
    /// Polls pool state or account subscriptions and pushes DexPriceUpdate to the channel.
    /// Returns a handle to cancel the subscription.
    async fn subscribe_prices(
        &self,
        pairs: &[TradingPair],
        tx: mpsc::Sender<DexPriceUpdate>,
    ) -> Result<SubscriptionHandle>;

    /// Get current pool state: reserves, fee tier, liquidity.
    /// Used for one-off price checks and pre-trade validation.
    async fn get_pool_state(&self, pair: &TradingPair) -> Result<PoolState>;

    /// Get the SOL balance and all SPL token balances for the configured wallet.
    async fn get_wallet_balances(&self) -> Result<WalletBalances>;

    /// Which DEX this connector targets.
    fn venue(&self) -> DexVenue;
}

/// Raydium/Orca pool state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolState {
    pub pair: TradingPair,
    pub base_reserve: u64,              // In smallest units
    pub quote_reserve: u64,
    pub lp_fee_bps: u16,               // Fee in basis points (e.g., 25 = 0.25%)
    pub protocol_fee_bps: u16,
    pub pool_address: Pubkey,
    pub price: Decimal,                 // quote/base in human units
    pub liquidity_usd: Decimal,
    pub last_updated_slot: u64,
}
```

### 6.2 CexConnector (`sol-arb-cex/src/lib.rs`)

```rust
use async_trait::async_trait;
use sol_arb_types::*;
use std::collections::HashMap;
use std::result::Result as StdResult;
use tokio::sync::mpsc;

pub type Result<T> = StdResult<T, CexError>;

#[async_trait]
pub trait CexConnector: Send + Sync + 'static {
    /// Subscribe to real-time orderbook updates via WebSocket.
    /// Pushes CexPriceUpdate for every best bid/ask change.
    /// Binance: uses @bookTicker stream for lowest latency.
    async fn subscribe_orderbook(
        &self,
        symbols: &[String],
        tx: mpsc::Sender<CexPriceUpdate>,
    ) -> Result<SubscriptionHandle>;

    /// Place an order (IOC for arb execution, limit for other use cases).
    /// For arbitrage: always IOC to get immediate fill or nothing.
    async fn place_order(&self, req: &OrderRequest) -> Result<OrderResponse>;

    /// Cancel an open order by exchange order ID.
    async fn cancel_order(&self, symbol: &str, order_id: &str) -> Result<()>;

    /// Get current status of an order.
    async fn get_order_status(&self, symbol: &str, order_id: &str) -> Result<OrderResponse>;

    /// Get all asset balances on the exchange.
    async fn get_balances(&self) -> Result<HashMap<String, Balance>>;

    /// Get recent trades for a symbol (for price verification).
    async fn get_recent_trades(&self, symbol: &str, limit: u16) -> Result<Vec<CexTrade>>;

    /// Test connectivity (ping endpoint).
    async fn ping(&self) -> Result<()>;
}

/// A single trade on the CEX.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CexTrade {
    pub id: u64,
    pub price: Decimal,
    pub quantity: Decimal,
    pub time: DateTime<Utc>,
    pub is_buyer_maker: bool,
}
```

### 6.3 RiskManager (`sol-arb-risk/src/lib.rs`)

```rust
use async_trait::async_trait;
use sol_arb_types::*;
use std::result::Result as StdResult;

pub type Result<T> = StdResult<T, RiskError>;

#[async_trait]
pub trait RiskManager: Send + Sync + 'static {
    /// Validate an opportunity against all risk rules.
    /// Returns Approved or Rejected with the specific violation.
    async fn check_opportunity(&self, opp: &ArbitrageOpportunity) -> RiskCheck;

    /// Record a completed trade execution for risk tracking.
    /// Updates daily P&L, consecutive loss counters, etc.
    async fn record_execution(&self, execution: &TradeExecution) -> Result<()>;

    /// Get the current circuit breaker state.
    async fn circuit_breaker_state(&self) -> CircuitBreakerState;

    /// Manually trip the circuit breaker (emergency stop).
    async fn trip_circuit_breaker(&self, reason: &str) -> Result<()>;

    /// Manually reset the circuit breaker.
    async fn reset_circuit_breaker(&self) -> Result<()>;

    /// Get current risk metrics snapshot.
    async fn risk_snapshot(&self) -> Result<RiskSnapshot>;

    /// Check if venue balances are dangerously imbalanced.
    async fn check_balance_health(&self, balances: &VenueBalances) -> Vec<RiskViolation>;
}

/// Point-in-time snapshot of all risk metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskSnapshot {
    pub circuit_breaker: CircuitBreakerState,
    pub daily_pnl: Decimal,
    pub daily_loss_limit: Decimal,
    pub daily_trades: u32,
    pub consecutive_losses: u32,
    pub max_consecutive_losses: u32,
    pub open_exposure_usd: Decimal,
    pub max_exposure_usd: Decimal,
    pub solana_balance_pct: Decimal,
    pub binance_balance_pct: Decimal,
    pub last_trade_at: Option<DateTime<Utc>>,
    pub timestamp: DateTime<Utc>,
}
```

### 6.4 ArbitrageEngine (`sol-arb-engine/src/lib.rs`)

```rust
use async_trait::async_trait;
use sol_arb_types::*;
use tokio::sync::{broadcast, mpsc};

pub type Result<T> = std::result::Result<T, EngineError>;

/// Events emitted by the engine for UI/API consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EngineEvent {
    PriceUpdate(SpreadInfo),
    OpportunityDetected(ArbitrageOpportunity),
    OpportunityRejected { id: Uuid, violation: RiskViolation },
    ExecutionStarted { opportunity_id: Uuid },
    ExecutionCompleted(TradeExecution),
    BalanceUpdate(VenueBalances),
    CircuitBreakerTripped { reason: String },
    Error(String),
}

#[async_trait]
pub trait ArbitrageEngine: Send + Sync + 'static {
    /// Start the engine. Spawns price feed listeners, detection loop, and executor.
    /// Returns a channel receiver for engine events.
    async fn start(&self) -> Result<broadcast::Receiver<EngineEvent>>;

    /// Stop the engine gracefully. Waits for in-flight executions to complete.
    async fn stop(&self) -> Result<()>;

    /// Is the engine currently running?
    fn is_running(&self) -> bool;

    /// Pause trading (keep price feeds alive, stop executing).
    async fn pause(&self) -> Result<()>;

    /// Resume trading after pause.
    async fn resume(&self) -> Result<()>;

    /// Get all currently tracked spreads across all pairs.
    async fn current_spreads(&self) -> Result<Vec<SpreadInfo>>;

    /// Get recent trade executions (last N).
    async fn recent_executions(&self, limit: usize) -> Result<Vec<TradeExecution>>;

    /// Force a balance refresh on both venues.
    async fn refresh_balances(&self) -> Result<VenueBalances>;
}
```

### 6.5 Repository (`sol-arb-db/src/repo.rs`)

```rust
use async_trait::async_trait;
use sol_arb_types::*;

pub type Result<T> = std::result::Result<T, DbError>;

#[async_trait]
pub trait Repository: Send + Sync + 'static {
    // ── Opportunities ──
    async fn save_opportunity(&self, opp: &ArbitrageOpportunity) -> Result<()>;
    async fn update_opportunity_status(&self, id: Uuid, status: OpportunityStatus) -> Result<()>;
    async fn get_opportunities(
        &self,
        status: Option<OpportunityStatus>,
        limit: u32,
    ) -> Result<Vec<ArbitrageOpportunity>>;

    // ── Trade Executions ──
    async fn save_execution(&self, exec: &TradeExecution) -> Result<()>;
    async fn get_executions(
        &self,
        pair: Option<&TradingPair>,
        limit: u32,
    ) -> Result<Vec<TradeExecution>>;

    // ── Balance Snapshots ──
    async fn save_balance_snapshot(&self, snapshot: &VenueBalances) -> Result<()>;
    async fn get_latest_balance_snapshot(&self) -> Result<Option<VenueBalances>>;

    // ── Daily P&L ──
    async fn update_daily_pnl(&self, date: chrono::NaiveDate, pnl: &DailyPnl) -> Result<()>;
    async fn get_daily_pnl(&self, date: chrono::NaiveDate) -> Result<Option<DailyPnl>>;
    async fn get_pnl_range(
        &self,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
    ) -> Result<Vec<DailyPnl>>;

    // ── Price Snapshots (for analysis) ──
    async fn save_price_snapshot(&self, spread: &SpreadInfo) -> Result<()>;
}

/// Daily P&L summary record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyPnl {
    pub date: chrono::NaiveDate,
    pub trades_executed: u32,
    pub trades_succeeded: u32,
    pub trades_partial: u32,
    pub trades_failed: u32,
    pub gross_profit_usd: Decimal,
    pub total_fees_usd: Decimal,
    pub net_profit_usd: Decimal,
    pub best_trade_usd: Decimal,
    pub worst_trade_usd: Decimal,
}
```

---

## 7. Execution Flow

### 7.1 Main Loop — ASCII Diagram

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                           SOL-ARB ENGINE                                      │
│                                                                              │
│  ┌─────────────────┐         ┌─────────────────┐                            │
│  │ Binance WS Feed │         │ Solana RPC Poll  │                            │
│  │ (@bookTicker)   │         │ (every 400ms)    │                            │
│  └────────┬────────┘         └────────┬────────┘                            │
│           │ CexPriceUpdate            │ DexPriceUpdate                      │
│           │                           │                                      │
│           └───────────┬───────────────┘                                      │
│                       ▼                                                      │
│              ┌────────────────┐                                              │
│              │  Price Feed    │   Merges both streams into                    │
│              │  Aggregator    │   unified (pair, dex_price, cex_price)        │
│              └────────┬───────┘                                              │
│                       │ SpreadInfo (per pair, per tick)                       │
│                       ▼                                                      │
│              ┌────────────────┐                                              │
│              │  Spread        │   Computes gross spread, subtracts            │
│              │  Detector      │   all-in fees, checks both directions         │
│              └────────┬───────┘                                              │
│                       │ net_spread > threshold?                               │
│                       ▼                                                      │
│              ┌────────────────┐                                              │
│              │  Risk          │   Circuit breaker, daily loss limit,          │
│              │  Manager       │   position limits, stale price check,         │
│              │                │   balance ratio check                         │
│              └────────┬───────┘                                              │
│                       │ RiskCheck::Approved                                   │
│                       ▼                                                      │
│              ┌────────────────┐                                              │
│              │  Trade         │   Spawns concurrent execution:                │
│              │  Executor      │                                               │
│              │  ┌───────────┐ │                                               │
│              │  │ tokio::   │ │   ┌─────────────────┐  ┌──────────────────┐  │
│              │  │ join!     │◄├──►│ Jupiter Swap     │  │ Binance IOC      │  │
│              │  │           │ │   │ (sign + submit)  │  │ (REST API)       │  │
│              │  └───────────┘ │   └────────┬────────┘  └────────┬─────────┘  │
│              └────────┬───────┘            │                    │             │
│                       │                    └────────┬───────────┘             │
│                       ▼                             ▼                         │
│              ┌────────────────┐          ┌────────────────┐                   │
│              │  Reconciler    │          │  Result Pair    │                   │
│              │  Handles:      │◄─────────│  (SwapResult,  │                   │
│              │  • both OK     │          │   OrderResponse)│                   │
│              │  • one failed  │          └────────────────┘                   │
│              │  • both failed │                                               │
│              └────────┬───────┘                                              │
│                       │ TradeExecution                                        │
│                       ▼                                                      │
│              ┌────────────────┐          ┌────────────────┐                   │
│              │  DB Writer     │          │  Event Bus      │                   │
│              │  (mpsc batch)  │          │  (broadcast)    │                   │
│              └────────────────┘          └────────────────┘                   │
│                                                  │                            │
│                                    ┌─────────────┼──────────────┐            │
│                                    ▼             ▼              ▼            │
│                              ┌──────────┐ ┌──────────┐ ┌──────────┐         │
│                              │ HTTP API │ │ WS Feed  │ │ Logging  │         │
│                              └──────────┘ └──────────┘ └──────────┘         │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 7.2 Main Loop — Pseudocode

```rust
async fn run_engine(
    dex: Arc<dyn DexConnector>,
    cex: Arc<dyn CexConnector>,
    risk: Arc<dyn RiskManager>,
    db: Arc<dyn Repository>,
    config: EngineConfig,
    shutdown: CancellationToken,
) -> Result<()> {
    // 1. Start price feeds
    let (price_tx, mut price_rx) = mpsc::channel::<PriceEvent>(1024);
    let dex_sub = dex.subscribe_prices(&config.pairs, price_tx.clone()).await?;
    let cex_sub = cex.subscribe_orderbook(&config.binance_symbols(), price_tx.clone()).await?;

    // 2. State: latest prices per pair
    let prices: DashMap<String, LatestPrices> = DashMap::new();
    // LatestPrices { dex: Option<DexPriceUpdate>, cex: Option<CexPriceUpdate> }

    // 3. Event bus for UI/API consumers
    let (event_tx, _) = broadcast::channel::<EngineEvent>(256);

    // 4. Execution semaphore — max 2 concurrent arb executions
    let exec_semaphore = Arc::new(Semaphore::new(2));

    tracing::info!("Engine started. Monitoring {} pairs.", config.pairs.len());

    loop {
        tokio::select! {
            // ── Shutdown signal ──
            _ = shutdown.cancelled() => {
                tracing::info!("Shutdown requested. Waiting for in-flight executions...");
                // Wait for semaphore to be fully available (all executions done)
                let _ = exec_semaphore.acquire_many(2).await;
                break;
            }

            // ── Price update received ──
            Some(event) = price_rx.recv() => {
                match event {
                    PriceEvent::Dex(update) => {
                        let key = update.pair.base_symbol.clone();
                        prices.entry(key.clone()).or_default().dex = Some(update.clone());
                        let _ = event_tx.send(EngineEvent::PriceUpdate(/* ... */));
                    }
                    PriceEvent::Cex(update) => {
                        let key = symbol_to_base(&update.symbol);
                        prices.entry(key.clone()).or_default().cex = Some(update.clone());
                    }
                }

                // ── Check all pairs for arbitrage ──
                for entry in prices.iter() {
                    let latest = entry.value();
                    let (Some(dex_px), Some(cex_px)) = (&latest.dex, &latest.cex) else {
                        continue; // Need both prices
                    };

                    // 5. Compute spread both directions
                    let spread = compute_spread(dex_px, cex_px, &config.fee_model);

                    if spread.net_spread_pct <= config.min_spread_pct {
                        continue; // Not profitable
                    }

                    // 6. Build opportunity
                    let opportunity = ArbitrageOpportunity {
                        id: Uuid::now_v7(),
                        direction: spread.direction,
                        net_spread_pct: spread.net_spread_pct,
                        trade_size_usd: config.default_trade_size_usd,
                        // ... fill remaining fields
                        status: OpportunityStatus::Detected,
                        detected_at: Utc::now(),
                        ..
                    };

                    // 7. Risk check
                    match risk.check_opportunity(&opportunity).await {
                        RiskCheck::Approved => {
                            tracing::info!(
                                pair = %opportunity.pair.base_symbol,
                                spread = %opportunity.net_spread_pct,
                                "Opportunity approved. Executing..."
                            );
                        }
                        RiskCheck::Rejected(violation) => {
                            tracing::warn!(?violation, "Opportunity rejected by risk manager");
                            let _ = event_tx.send(EngineEvent::OpportunityRejected {
                                id: opportunity.id,
                                violation,
                            });
                            continue;
                        }
                    }

                    // 8. Execute concurrently (if slot available)
                    let permit = match exec_semaphore.clone().try_acquire_owned() {
                        Ok(p) => p,
                        Err(_) => {
                            tracing::debug!("Max concurrent executions reached, skipping");
                            continue;
                        }
                    };

                    let dex = dex.clone();
                    let cex = cex.clone();
                    let risk = risk.clone();
                    let db = db.clone();
                    let event_tx = event_tx.clone();

                    tokio::spawn(async move {
                        let _permit = permit; // Held until this block completes

                        let result = execute_arbitrage(
                            &*dex, &*cex, &opportunity, &config
                        ).await;

                        // 9. Reconcile and record
                        let execution = reconcile_execution(opportunity, result);
                        risk.record_execution(&execution).await.ok();
                        db.save_execution(&execution).await.ok();

                        let _ = event_tx.send(EngineEvent::ExecutionCompleted(execution));
                    });
                }
            }
        }
    }

    // Cleanup
    dex_sub.cancel();
    cex_sub.cancel();
    Ok(())
}

/// Execute both legs of the arb concurrently.
async fn execute_arbitrage(
    dex: &dyn DexConnector,
    cex: &dyn CexConnector,
    opp: &ArbitrageOpportunity,
    config: &EngineConfig,
) -> (Result<SwapResult>, Result<OrderResponse>) {
    let (dex_leg, cex_leg) = match opp.direction {
        ArbDirection::BuyDexSellCex => {
            // Buy token on DEX (USDC → TOKEN), sell token on CEX
            let swap_req = build_swap_request(opp, config);
            let order_req = OrderRequest {
                symbol: opp.pair.binance_symbol.clone(),
                side: OrderSide::Sell,
                order_type: CexOrderType::Ioc,
                quantity: opp.trade_size_base,
                price: opp.cex_price,
                time_in_force: TimeInForce::Ioc,
            };
            tokio::join!(
                dex.execute_swap(&swap_req),
                cex.place_order(&order_req)
            )
        }
        ArbDirection::BuyCexSellDex => {
            // Buy token on CEX, sell token on DEX (TOKEN → USDC)
            let order_req = OrderRequest {
                symbol: opp.pair.binance_symbol.clone(),
                side: OrderSide::Buy,
                order_type: CexOrderType::Ioc,
                quantity: opp.trade_size_base,
                price: opp.cex_price,
                time_in_force: TimeInForce::Ioc,
            };
            let swap_req = build_sell_swap_request(opp, config);
            tokio::join!(
                dex.execute_swap(&swap_req),
                cex.place_order(&order_req)
            )
        }
    };

    (dex_leg, cex_leg)
}

/// Reconcile the results of both legs into a TradeExecution record.
fn reconcile_execution(
    opp: ArbitrageOpportunity,
    results: (Result<SwapResult>, Result<OrderResponse>),
) -> TradeExecution {
    let (dex_result, cex_result) = results;

    let outcome = match (&dex_result, &cex_result) {
        (Ok(_), Ok(_))   => ExecutionOutcome::BothSucceeded,
        (Ok(_), Err(_))  => ExecutionOutcome::DexOnlySucceeded,
        (Err(_), Ok(_))  => ExecutionOutcome::CexOnlySucceeded,
        (Err(_), Err(_)) => ExecutionOutcome::BothFailed,
    };

    // Calculate actual P&L based on filled amounts
    let (gross_profit, fees, net_profit) = match outcome {
        ExecutionOutcome::BothSucceeded => {
            let dex = dex_result.unwrap();
            let cex = cex_result.unwrap();
            calculate_actual_pnl(&opp, &dex, &cex)
        }
        _ => (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO), // Partial executions need manual reconciliation
    };

    TradeExecution {
        id: Uuid::now_v7(),
        opportunity_id: opp.id,
        pair: opp.pair,
        direction: opp.direction,
        dex_leg: build_dex_leg(&dex_result, &opp),
        cex_leg: build_cex_leg(&cex_result, &opp),
        outcome,
        gross_profit_usd: gross_profit,
        fees_usd: fees,
        net_profit_usd: net_profit,
        execution_time_ms: /* measured */,
        created_at: Utc::now(),
    }
}
```

### 7.3 Spread Computation Logic

```rust
/// Compute the most profitable arbitrage direction and net spread.
fn compute_spread(
    dex: &DexPriceUpdate,
    cex: &CexPriceUpdate,
    fee_model: &FeeModel,
) -> SpreadInfo {
    // Direction 1: Buy on DEX (at ask), Sell on CEX (at bid)
    // Profit = cex_bid - dex_ask - fees
    let buy_dex_gross = (cex.best_bid - dex.ask_price) / dex.ask_price * dec!(100);
    let buy_dex_fees = fee_model.dex_fee_pct + fee_model.cex_taker_fee_pct
        + fee_model.sol_tx_fee_as_pct(dex.ask_price);
    let buy_dex_net = buy_dex_gross - buy_dex_fees;

    // Direction 2: Buy on CEX (at ask), Sell on DEX (at bid)
    // Profit = dex_bid - cex_ask - fees
    let buy_cex_gross = (dex.bid_price - cex.best_ask) / cex.best_ask * dec!(100);
    let buy_cex_fees = fee_model.dex_fee_pct + fee_model.cex_taker_fee_pct
        + fee_model.sol_tx_fee_as_pct(cex.best_ask);
    let buy_cex_net = buy_cex_gross - buy_cex_fees;

    // Pick the more profitable direction
    if buy_dex_net >= buy_cex_net {
        SpreadInfo {
            direction: ArbDirection::BuyDexSellCex,
            gross_spread_pct: buy_dex_gross,
            total_fees_pct: buy_dex_fees,
            net_spread_pct: buy_dex_net,
            dex_price: dex.ask_price,
            cex_price: cex.best_bid,
            // ...
        }
    } else {
        SpreadInfo {
            direction: ArbDirection::BuyCexSellDex,
            gross_spread_pct: buy_cex_gross,
            total_fees_pct: buy_cex_fees,
            net_spread_pct: buy_cex_net,
            dex_price: dex.bid_price,
            cex_price: cex.best_ask,
            // ...
        }
    }
}
```

---

## 8. Solana Integration Details

### 8.1 Jupiter V6 API

Jupiter is an HTTP-based aggregator — no Rust crate needed, just `reqwest`.

**Base URL**: `https://quote-api.jup.ag/v6`

#### Get Quote

```
GET /quote?inputMint={mint}&outputMint={mint}&amount={lamports}&slippageBps={bps}
```

| Param | Example | Notes |
|-------|---------|-------|
| `inputMint` | `So11111111111111111111111111111111111111112` | SOL mint |
| `outputMint` | `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v` | USDC mint |
| `amount` | `1000000000` | 1 SOL in lamports |
| `slippageBps` | `50` | 0.5% max slippage |
| `onlyDirectRoutes` | `false` | Allow multi-hop |
| `asLegacyTransaction` | `false` | Use versioned transactions |

**Response** (key fields):

```json
{
  "inputMint": "So111...",
  "outputMint": "EPjF...",
  "inAmount": "1000000000",
  "outAmount": "149550000",
  "otherAmountThreshold": "148802250",
  "priceImpactPct": "0.001",
  "routePlan": [
    {
      "swapInfo": {
        "ammKey": "...",
        "label": "Raydium",
        "inputMint": "So111...",
        "outputMint": "EPjF...",
        "inAmount": "1000000000",
        "outAmount": "149550000",
        "feeAmount": "374000",
        "feeMint": "So111..."
      },
      "percent": 100
    }
  ]
}
```

#### Get Swap Transaction

```
POST /swap
Content-Type: application/json

{
  "quoteResponse": { /* entire quote response from above */ },
  "userPublicKey": "YourWalletPubkeyBase58",
  "wrapAndUnwrapSol": true,
  "dynamicComputeUnitLimit": true,
  "prioritizationFeeLamports": "auto"
}
```

**Response**:

```json
{
  "swapTransaction": "base64-encoded-versioned-transaction",
  "lastValidBlockHeight": 123456789,
  "prioritizationFeeLamports": 50000
}
```

#### Sign and Submit

```rust
async fn execute_jupiter_swap(
    client: &RpcClient,
    keypair: &Keypair,
    swap_response: &JupiterSwapResponse,
    use_jito: bool,
) -> Result<String> {
    // 1. Decode the transaction
    let tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&swap_response.swap_transaction)?;
    let mut versioned_tx: VersionedTransaction = bincode::deserialize(&tx_bytes)?;

    // 2. Sign with our keypair
    let recent_blockhash = client.get_latest_blockhash().await?;
    versioned_tx.message.set_recent_blockhash(recent_blockhash);
    let signed_tx = VersionedTransaction::try_new(
        versioned_tx.message,
        &[keypair],
    )?;

    if use_jito {
        // 3a. Submit via Jito bundle (see §8.3)
        submit_jito_bundle(signed_tx).await
    } else {
        // 3b. Submit directly via RPC
        let sig = client.send_transaction_with_config(
            &signed_tx,
            RpcSendTransactionConfig {
                skip_preflight: true,  // We already validated
                max_retries: Some(3),
                ..Default::default()
            },
        ).await?;
        Ok(sig.to_string())
    }
}
```

### 8.2 Raydium On-Chain Price Reading

Raydium V4 AMM pools use a constant-product formula. Pool state is stored on-chain.

**Reading pool reserves:**

```rust
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use borsh::BorshDeserialize;

/// Raydium V4 AMM pool layout (simplified — key fields only).
#[derive(BorshDeserialize)]
pub struct RaydiumPoolState {
    pub status: u64,
    pub nonce: u64,
    pub order_num: u64,
    pub depth: u64,
    pub coin_decimals: u64,             // base token decimals
    pub pc_decimals: u64,               // quote token decimals
    pub state: u64,
    pub reset_flag: u64,
    pub min_size: u64,
    pub vol_max_cut_ratio: u64,
    pub amount_wave_ratio: u64,
    pub coin_lot_size: u64,
    pub pc_lot_size: u64,
    pub min_price_multiplier: u64,
    pub max_price_multiplier: u64,
    // ... other fields ...
    pub pool_coin_token_account: Pubkey, // Token account holding base reserves
    pub pool_pc_token_account: Pubkey,   // Token account holding quote reserves
    // ... more fields ...
}

async fn get_raydium_price(
    rpc: &RpcClient,
    pool_address: &Pubkey,
) -> Result<Decimal> {
    // 1. Read the pool account
    let pool_data = rpc.get_account_data(pool_address).await?;
    let pool: RaydiumPoolState = BorshDeserialize::deserialize(&mut &pool_data[..])?;

    // 2. Read token account balances (reserves)
    let coin_account = rpc.get_token_account_balance(&pool.pool_coin_token_account).await?;
    let pc_account = rpc.get_token_account_balance(&pool.pool_pc_token_account).await?;

    let coin_reserve: u64 = coin_account.amount.parse()?;
    let pc_reserve: u64 = pc_account.amount.parse()?;

    // 3. Compute price: quote_reserve / base_reserve (adjusted for decimals)
    let base_decimals = pool.coin_decimals as u32;
    let quote_decimals = pool.pc_decimals as u32;

    let price = Decimal::from(pc_reserve)
        / Decimal::new(10i64.pow(quote_decimals), 0)
        / (Decimal::from(coin_reserve) / Decimal::new(10i64.pow(base_decimals), 0));

    Ok(price)
}
```

**Computing effective price with fees (for a specific trade size):**

```rust
/// Constant-product AMM: x * y = k
/// To buy `amount_in` of base token:
///   amount_out = (reserve_quote * amount_in) / (reserve_base + amount_in)
///   Minus fee (0.25% for standard Raydium pools)
fn compute_swap_output(
    amount_in: u64,
    reserve_in: u64,
    reserve_out: u64,
    fee_bps: u64,  // 25 = 0.25%
) -> u64 {
    let amount_in_with_fee = amount_in as u128 * (10000 - fee_bps) as u128;
    let numerator = amount_in_with_fee * reserve_out as u128;
    let denominator = reserve_in as u128 * 10000 + amount_in_with_fee;
    (numerator / denominator) as u64
}
```

### 8.3 Jito Bundle Submission

Jito bundles provide MEV protection — your transaction is included atomically without being sandwiched.

**Jito Block Engine gRPC endpoints:**

| Region | Endpoint |
|--------|----------|
| Mainnet (Amsterdam) | `https://amsterdam.mainnet.block-engine.jito.wtf` |
| Mainnet (Frankfurt) | `https://frankfurt.mainnet.block-engine.jito.wtf` |
| Mainnet (NY) | `https://ny.mainnet.block-engine.jito.wtf` |
| Mainnet (Tokyo) | `https://tokyo.mainnet.block-engine.jito.wtf` |

**Jito tip accounts** (one of these randomly selected per bundle):

```rust
const JITO_TIP_ACCOUNTS: &[&str] = &[
    "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
    "HFqU5x63VTqvQss8hp11i4bVqkfRtQ7NmXwkiNPNYBZp",
    "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
    "ADaUMid9yfUytqMBgopwjb2DTLSQfPv7t3p4K47HcKKR",
    "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
    "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
    "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
    "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
];
```

**Bundle submission flow:**

```rust
use jito_protos::searcher::searcher_service_client::SearcherServiceClient;
use jito_protos::searcher::SendBundleRequest;
use jito_protos::bundle::Bundle;

async fn submit_jito_bundle(
    swap_tx: VersionedTransaction,
    tip_lamports: u64,
    keypair: &Keypair,
) -> Result<String> {
    // 1. Build tip transaction (separate from swap)
    let tip_account = JITO_TIP_ACCOUNTS[rand::random::<usize>() % JITO_TIP_ACCOUNTS.len()];
    let tip_ix = system_instruction::transfer(
        &keypair.pubkey(),
        &Pubkey::from_str(tip_account)?,
        tip_lamports,
    );
    let tip_tx = Transaction::new_signed_with_payer(
        &[tip_ix],
        Some(&keypair.pubkey()),
        &[keypair],
        recent_blockhash,
    );

    // 2. Create bundle: [swap_tx, tip_tx]
    let bundle = Bundle {
        transactions: vec![
            bincode::serialize(&swap_tx)?,
            bincode::serialize(&tip_tx)?,
        ],
    };

    // 3. Submit via gRPC
    let mut client = SearcherServiceClient::connect(
        "https://amsterdam.mainnet.block-engine.jito.wtf"
    ).await?;

    let response = client.send_bundle(SendBundleRequest {
        bundle: Some(bundle),
    }).await?;

    let bundle_id = response.into_inner().uuid;
    tracing::info!(%bundle_id, "Jito bundle submitted");

    // 4. Poll for bundle status (or wait for tx confirmation)
    // Bundle lands within ~2-4 slots if accepted
    Ok(bundle_id)
}
```

**Bundle lifecycle:**

```
submit_bundle() ──► Jito Block Engine ──► Validator (leader)
                                              │
                         ┌────────────────────┼────────────────┐
                         ▼                    ▼                ▼
                    ┌──────────┐       ┌──────────┐     ┌──────────┐
                    │ LANDED   │       │ DROPPED  │     │ EXPIRED  │
                    │ (in slot)│       │ (outbid) │     │ (5 slots)│
                    └──────────┘       └──────────┘     └──────────┘
```

**Tip sizing strategy:**

| Urgency | Tip Range | Use Case |
|---------|-----------|----------|
| Low | 0.001 SOL (~$0.15) | Non-urgent, willing to retry |
| Medium | 0.005 SOL (~$0.75) | Standard arb execution |
| High | 0.01 SOL (~$1.50) | Time-sensitive, large spread |
| Dynamic | Based on recent tips | Query `getTipAccounts` + `getRecentPrioritizationFees` |

### 8.4 Priority Fees

For non-Jito submissions, priority fees determine inclusion speed.

```rust
use solana_sdk::compute_budget::ComputeBudgetInstruction;

fn add_priority_fee(
    instructions: &mut Vec<Instruction>,
    compute_units: u32,
    priority_fee_lamports: u64,
) {
    // Compute unit price = fee / units (in micro-lamports)
    let micro_lamports_per_cu = (priority_fee_lamports * 1_000_000) / compute_units as u64;

    instructions.insert(0, ComputeBudgetInstruction::set_compute_unit_limit(compute_units));
    instructions.insert(1, ComputeBudgetInstruction::set_compute_unit_price(micro_lamports_per_cu));
}

// Example: 200,000 CU limit, 50,000 lamport fee ≈ $0.007
// micro_lamports_per_cu = 50000 * 1000000 / 200000 = 250,000
```

**Fee estimation** (query recent fees for the target program):

```rust
async fn estimate_priority_fee(
    rpc: &RpcClient,
    program_id: &Pubkey,
) -> Result<u64> {
    let recent_fees = rpc.get_recent_prioritization_fees(&[*program_id]).await?;

    // Use 75th percentile of recent fees as our bid
    let mut fees: Vec<u64> = recent_fees.iter().map(|f| f.prioritization_fee).collect();
    fees.sort();

    let p75_idx = (fees.len() as f64 * 0.75) as usize;
    Ok(fees.get(p75_idx).copied().unwrap_or(50_000))
}
```

### 8.5 Transaction Confirmation

**Option A: Polling (recommended for simplicity)**

```rust
async fn confirm_transaction(
    rpc: &RpcClient,
    signature: &Signature,
    timeout: Duration,
) -> Result<bool> {
    let start = Instant::now();

    loop {
        if start.elapsed() > timeout {
            return Err(DexError::TransactionTimeout {
                signature: signature.to_string(),
                timeout,
            });
        }

        let status = rpc.get_signature_statuses(&[*signature]).await?;

        if let Some(Some(status)) = status.value.first() {
            if status.err.is_some() {
                return Err(DexError::TransactionFailed {
                    signature: signature.to_string(),
                    error: format!("{:?}", status.err),
                });
            }
            if status.confirmation_status == Some(TransactionConfirmationStatus::Confirmed)
                || status.confirmation_status == Some(TransactionConfirmationStatus::Finalized)
            {
                return Ok(true);
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
```

**Option B: WebSocket subscription (lower latency)**

```rust
async fn confirm_via_websocket(
    ws_url: &str,
    signature: &Signature,
    timeout: Duration,
) -> Result<bool> {
    let pubsub_client = PubsubClient::new(ws_url).await?;

    let (mut notifications, unsub) = pubsub_client
        .signature_subscribe(signature, None)
        .await?;

    let result = tokio::time::timeout(timeout, notifications.next()).await;

    unsub().await;

    match result {
        Ok(Some(response)) => Ok(response.value.err.is_none()),
        Ok(None) => Err(DexError::WebSocketClosed),
        Err(_) => Err(DexError::TransactionTimeout { /* ... */ }),
    }
}
```

**Confirmation levels:**

| Level | Time | Guarantee | Use For |
|-------|------|-----------|---------|
| `Processed` | ~400ms | In current bank, may be rolled back | Nothing (unsafe) |
| `Confirmed` | ~6.4s | Supermajority voted | Arb execution ✓ |
| `Finalized` | ~12.8s | 31+ confirmed blocks | Settlement verification |

For arbitrage, **confirmed** is the right level — it's fast enough and safe enough. `Processed` is dangerous (can be rolled back). `Finalized` is too slow.

---

## 9. Data Flow Architecture

### 9.1 Complete Data Flow Diagram

```
┌─────────────────────────────────────────────────────────────────────────┐
│ DATA SOURCES                                                             │
│                                                                          │
│  Binance WebSocket                    Solana RPC                         │
│  wss://stream.binance.com             https://api.mainnet-beta.solana.com│
│  @bookTicker (best bid/ask)           getAccountInfo (pool reserves)     │
│  ~100ms update frequency              ~400ms polling interval            │
└────────┬──────────────────────────────────────────┬──────────────────────┘
         │                                          │
         ▼                                          ▼
┌────────────────┐                      ┌────────────────────┐
│ CexConnector   │                      │ DexConnector       │
│ • Parse WS msg │                      │ • Read pool accts  │
│ • Build        │                      │ • Compute price    │
│   CexPriceUpd  │                      │ • Build DexPriceUpd│
└────────┬───────┘                      └──────────┬─────────┘
         │                                          │
         │ mpsc::Sender<PriceEvent>                 │
         └──────────────────┬───────────────────────┘
                            ▼
                 ┌─────────────────────┐
                 │ Price Feed Aggregator│    Channel capacity: 1024
                 │ (mpsc receiver)      │    Drop policy: oldest (non-critical)
                 └──────────┬──────────┘
                            │
           ┌────────────────┼────────────────┐
           ▼                ▼                ▼
    ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
    │ DashMap:    │  │ Spread      │  │ broadcast   │
    │ Latest      │  │ Detector    │  │ → UI/WS     │
    │ Prices      │  │ (per pair)  │  │   consumers │
    └─────────────┘  └──────┬──────┘  └─────────────┘
                            │
                            │ ArbitrageOpportunity (if net_spread > threshold)
                            ▼
                 ┌─────────────────────┐
                 │ Risk Manager        │    Sync validation (no async I/O)
                 │ • Circuit breaker   │    Response: Approved / Rejected
                 │ • Daily loss check  │
                 │ • Position limits   │
                 │ • Stale price check │
                 │ • Balance check     │
                 └──────────┬──────────┘
                            │
                            │ RiskCheck::Approved
                            ▼
                 ┌─────────────────────┐
                 │ Trade Executor      │    Semaphore: max 2 concurrent
                 │                     │
                 │ tokio::join!(       │
                 │   dex.execute_swap, │    ← DEX: Jupiter/Raydium swap
                 │   cex.place_order   │    ← CEX: Binance IOC order
                 │ )                   │
                 └──────────┬──────────┘
                            │
                            │ (SwapResult, OrderResponse)
                            ▼
                 ┌─────────────────────┐
                 │ Reconciler          │    Determines outcome:
                 │                     │    BothSucceeded / DexOnly / CexOnly / BothFailed
                 │ → TradeExecution    │
                 └──────────┬──────────┘
                            │
              ┌─────────────┼─────────────┐
              ▼             ▼             ▼
       ┌────────────┐ ┌──────────┐ ┌──────────────┐
       │ DB Writer  │ │ Risk     │ │ Event Bus    │
       │ (mpsc,     │ │ Updater  │ │ (broadcast)  │
       │  batched)  │ │ (P&L,   │ │ → API/WS     │
       └──────┬─────┘ │  counts) │ └──────────────┘
              │        └──────────┘
              ▼
       ┌────────────┐
       │ SQLite     │
       │ (WAL mode) │
       └────────────┘
```

### 9.2 Channel Topology

| Channel | Type | Capacity | Drop Policy | Purpose |
|---------|------|----------|-------------|---------|
| Price feed | `mpsc` | 1024 | Lag = warn, oldest dropped | Hot-path price data |
| Event bus | `broadcast` | 256 | Slow receivers dropped | UI/API event stream |
| DB writer | `mpsc` | 512 | **Never drop** — backpressure | Critical trade records |
| Shutdown | `CancellationToken` | — | — | Graceful shutdown signal |

**Key principle**: The DB writer channel must NEVER drop messages. Trade execution records are the source of truth for reconciliation. If the DB channel is full, the engine must backpressure (slow down new executions) rather than lose data.

---

## 10. Error Handling Strategy

### 10.1 Error Type Hierarchy

```rust
// ── sol-arb-types/src/error.rs ──
// Common errors shared across crates

#[derive(Debug, thiserror::Error)]
pub enum ArbitrageError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Insufficient balance: need {required} {asset} on {venue}, have {available}")]
    InsufficientBalance {
        venue: String,
        asset: String,
        required: Decimal,
        available: Decimal,
    },

    #[error("Stale price data: {venue} price is {age_ms}ms old (max: {max_age_ms}ms)")]
    StalePrice {
        venue: String,
        age_ms: u64,
        max_age_ms: u64,
    },
}


// ── sol-arb-dex/src/error.rs ──

#[derive(Debug, thiserror::Error)]
pub enum DexError {
    // ── RPC Errors ──
    #[error("Solana RPC error: {0}")]
    RpcError(#[from] solana_client::client_error::ClientError),

    #[error("RPC timeout after {timeout_ms}ms to {endpoint}")]
    RpcTimeout { endpoint: String, timeout_ms: u64 },

    #[error("RPC rate limited. Retry after {retry_after_ms}ms")]
    RpcRateLimited { retry_after_ms: u64 },

    // ── Transaction Errors ──
    #[error("Transaction not confirmed within {timeout:?}: {signature}")]
    TransactionTimeout { signature: String, timeout: Duration },

    #[error("Transaction dropped (not included in any block): {signature}")]
    TransactionDropped { signature: String },

    #[error("Transaction failed on-chain: {signature} — {error}")]
    TransactionFailed { signature: String, error: String },

    #[error("Transaction simulation failed: {0}")]
    SimulationFailed(String),

    // ── Jupiter Errors ──
    #[error("Jupiter API error: HTTP {status} — {body}")]
    JupiterApiError { status: u16, body: String },

    #[error("Jupiter quote expired (age: {age_ms}ms)")]
    QuoteExpired { age_ms: u64 },

    #[error("Jupiter route not found for {input_mint} → {output_mint}")]
    NoRouteFound { input_mint: String, output_mint: String },

    // ── Pool Errors ──
    #[error("Pool account not found: {address}")]
    PoolNotFound { address: String },

    #[error("Failed to deserialize pool state: {0}")]
    PoolDeserializeError(String),

    // ── Jito Errors ──
    #[error("Jito bundle dropped (outbid or expired): {bundle_id}")]
    JitoBundleDropped { bundle_id: String },

    #[error("Jito gRPC connection error: {0}")]
    JitoConnectionError(String),

    // ── General ──
    #[error("DEX connector error: {0}")]
    Other(#[from] anyhow::Error),
}


// ── sol-arb-cex/src/error.rs ──

#[derive(Debug, thiserror::Error)]
pub enum CexError {
    // ── API Errors ──
    #[error("Binance API error: code={code}, msg={message}")]
    BinanceApiError { code: i64, message: String },

    #[error("Binance HTTP error: {status} — {body}")]
    HttpError { status: u16, body: String },

    #[error("Request timeout after {timeout_ms}ms")]
    RequestTimeout { timeout_ms: u64 },

    // ── Order Errors ──
    #[error("Order rejected: {reason} (symbol={symbol}, side={side:?})")]
    OrderRejected { symbol: String, side: OrderSide, reason: String },

    #[error("Order not found: {order_id}")]
    OrderNotFound { order_id: String },

    #[error("Insufficient balance for order: need {required} {asset}")]
    InsufficientBalance { asset: String, required: Decimal },

    // ── WebSocket Errors ──
    #[error("WebSocket disconnected: {reason}")]
    WebSocketDisconnected { reason: String },

    #[error("WebSocket reconnection failed after {attempts} attempts")]
    WebSocketReconnectFailed { attempts: u32 },

    // ── Auth Errors ──
    #[error("Authentication failed: {0}")]
    AuthError(String),

    #[error("Rate limited: retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    // ── General ──
    #[error("CEX connector error: {0}")]
    Other(#[from] anyhow::Error),
}


// ── sol-arb-engine/src/error.rs ──

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("DEX error: {0}")]
    Dex(#[from] DexError),

    #[error("CEX error: {0}")]
    Cex(#[from] CexError),

    #[error("Risk violation: {0}")]
    Risk(#[from] RiskError),

    #[error("Database error: {0}")]
    Db(#[from] DbError),

    #[error("Engine not running")]
    NotRunning,

    #[error("Engine already running")]
    AlreadyRunning,

    #[error("Partial execution: {0}")]
    PartialExecution(String),
}


// ── sol-arb-risk/src/error.rs ──

#[derive(Debug, thiserror::Error)]
pub enum RiskError {
    #[error("Circuit breaker is OPEN: {reason}")]
    CircuitBreakerOpen { reason: String },

    #[error("Daily loss limit exceeded: {current} / {limit}")]
    DailyLossExceeded { current: Decimal, limit: Decimal },

    #[error("Position limit exceeded for {pair}: {current} / {limit}")]
    PositionLimitExceeded { pair: String, current: Decimal, limit: Decimal },

    #[error("Internal risk error: {0}")]
    Internal(String),
}


// ── sol-arb-db/src/error.rs (wraps sqlx) ──

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("Database query failed: {0}")]
    Query(#[from] sqlx::Error),

    #[error("Migration failed: {0}")]
    Migration(String),

    #[error("Record not found: {entity} with id {id}")]
    NotFound { entity: String, id: String },

    #[error("Serialization error: {0}")]
    Serialization(String),
}
```

### 10.2 Critical Error Scenarios & Handling

#### Scenario 1: Solana RPC timeout/failure

```
Trigger: RPC node returns timeout or 5xx error
Impact: Cannot read prices or submit transactions
Handling:
  1. Retry with exponential backoff (3 attempts, 200ms/400ms/800ms)
  2. If primary RPC fails, failover to backup RPC endpoint
  3. If all RPCs fail, pause DEX price feed (emit EngineEvent::Error)
  4. Risk manager sees stale DEX prices → blocks new executions
  5. Alert via log: "DEX price feed offline"
Config:
  [dex]
  rpc_url = "https://api.mainnet-beta.solana.com"
  rpc_backup_url = "https://solana-mainnet.g.alchemy.com/v2/..."
  rpc_timeout_ms = 5000
  rpc_max_retries = 3
```

#### Scenario 2: Transaction dropped (not included in any block)

```
Trigger: Transaction submitted but never confirmed within timeout
Impact: DEX leg didn't execute; capital is safe (no state change)
Handling:
  1. confirm_transaction() returns TransactionTimeout after 30s
  2. Double-check via getSignatureStatuses one final time
  3. If truly not included: log as TransactionDropped
  4. If CEX leg was also submitted:
     - IOC order either filled (within ~100ms) or was already cancelled
     - If CEX filled: we have an exposed CEX position → alert operator
     - If CEX not filled: both legs clean, no harm done
  5. Increase priority fee for next attempt
```

#### Scenario 3: Binance order rejected

```
Trigger: Binance returns error code (e.g., -1013 LOT_SIZE, -2010 INSUFFICIENT_BALANCE)
Impact: CEX leg didn't execute
Handling:
  1. Map Binance error code to CexError variant
  2. If rejection is transient (rate limit): retry after backoff
  3. If rejection is permanent (insufficient balance, bad params):
     - Log the error
     - If DEX leg already submitted but not yet confirmed: can't cancel on-chain
     - If DEX leg succeeded: exposed position on DEX side → alert + track
  4. Trip circuit breaker if 3+ consecutive rejections
```

#### Scenario 4: Partial execution (one leg succeeds, other fails)

```
Trigger: tokio::join! returns (Ok, Err) or (Err, Ok)
Impact: Exposed position on one venue
Handling:
  1. Record as ExecutionOutcome::DexOnlySucceeded or CexOnlySucceeded
  2. Calculate the exposed position size
  3. Risk manager increments unhedged_exposure counter
  4. If unhedged exposure > max_unhedged_limit: trip circuit breaker
  5. Options (configurable):
     a. ALERT_ONLY: log warning, operator handles manually
     b. AUTO_UNWIND: attempt to close the position on the venue where it succeeded
        - If DEX succeeded (bought TOKEN): try to sell TOKEN on Binance
        - If CEX succeeded (bought TOKEN): try to sell TOKEN on DEX
     c. HOLD: keep the position, hope it's profitable directionally
  6. Default: ALERT_ONLY (safest for v1)

Config:
  [engine]
  partial_execution_policy = "alert_only"  # alert_only | auto_unwind | hold
```

#### Scenario 5: Stale prices

```
Trigger: Price update timestamp is older than configured threshold
Impact: Executing on stale data → potential loss
Handling:
  1. Every price update carries a timestamp
  2. Before spread calculation: check age of BOTH prices
     - DEX price age = now() - dex_price.timestamp
     - CEX price age = now() - cex_price.timestamp
  3. If either > max_price_age_ms (default: 2000ms):
     - Skip this spread check
     - Emit RiskViolation::StaleDexPrice or StaleCexPrice
  4. If stale for > 10 seconds: emit warning event
  5. If stale for > 30 seconds: pause engine

Config:
  [risk]
  max_dex_price_age_ms = 2000
  max_cex_price_age_ms = 1000   # CEX should be faster
  stale_warning_ms = 10000
  stale_pause_ms = 30000
```

---

## 11. Rebalancing Architecture

### 11.1 The Problem

Arbitrage is directional within each trade. After many "buy on DEX, sell on CEX" trades:
- Solana wallet: depleted USDC, accumulated TOKEN
- Binance account: accumulated USDC, depleted TOKEN

Eventually, one side runs out of capital for its leg → trading stops.

### 11.2 Balance Monitoring

```rust
/// Runs every 60 seconds (and after every trade execution).
async fn check_balance_health(
    dex: &dyn DexConnector,
    cex: &dyn CexConnector,
    config: &RebalanceConfig,
) -> Vec<RebalanceAlert> {
    let wallet = dex.get_wallet_balances().await?;
    let exchange = cex.get_balances().await?;

    let total_usd = wallet.total_usd + exchange_total_usd(&exchange);
    let solana_pct = wallet.total_usd / total_usd * dec!(100);
    let binance_pct = dec!(100) - solana_pct;

    let mut alerts = vec![];

    // Check overall venue balance ratio
    if solana_pct < config.min_venue_pct || solana_pct > config.max_venue_pct {
        alerts.push(RebalanceAlert::VenueImbalance {
            solana_pct,
            binance_pct,
            threshold_pct: config.min_venue_pct,
        });
    }

    // Check per-token balance ratio (e.g., SOL on both sides)
    for pair in &config.pairs {
        let sol_balance = wallet.token_balances.get(&pair.base_symbol).unwrap_or(&Decimal::ZERO);
        let bin_balance = exchange.get(&pair.base_symbol).map(|b| b.free).unwrap_or(Decimal::ZERO);
        let total_token = sol_balance + bin_balance;

        if total_token > Decimal::ZERO {
            let sol_token_pct = sol_balance / total_token * dec!(100);
            if sol_token_pct < config.min_token_pct || sol_token_pct > config.max_token_pct {
                alerts.push(RebalanceAlert::TokenImbalance {
                    token: pair.base_symbol.clone(),
                    solana_pct: sol_token_pct,
                    binance_pct: dec!(100) - sol_token_pct,
                });
            }
        }
    }

    alerts
}
```

### 11.3 Rebalance Thresholds

| Metric | Warning | Critical | Action |
|--------|---------|----------|--------|
| Venue balance ratio | 70/30 split | 85/15 split | Warning → log; Critical → pause trading that direction |
| Per-token ratio | 70/30 split | 80/20 split | Warning → log; Critical → reduce trade size for that pair |
| USDC on Solana | < 20% of allocation | < 10% | Pause buy-on-DEX trades |
| Token on Binance | < 20% of allocation | < 10% | Pause sell-on-CEX trades |

### 11.4 Rebalancing Options

**Option A: Manual rebalance (RECOMMENDED for v1)**

```
1. System detects imbalance, emits RebalanceAlert via event bus
2. API endpoint GET /api/rebalance/status shows current allocation
3. Operator manually:
   a. Withdraws TOKEN from Binance to Solana wallet address
   b. Or: sells TOKEN on DEX, transfers USDC to Binance via bridge
4. System detects new balances on next poll, resumes normal operation
```

**Option B: Semi-automated Binance withdrawal → Solana deposit**

```
1. System detects critical imbalance
2. Calculates rebalance amount (target 50/50 split)
3. Presents plan to operator via API: "Withdraw 100 SOL from Binance to {wallet}. Approve? Y/N"
4. If approved:
   a. Calls Binance withdrawal API (POST /sapi/v1/capital/withdraw/apply)
   b. Monitors Solana wallet for incoming transfer
   c. Confirms arrival, updates balance tracking
5. Safety limits:
   - Max 20% of total capital per day
   - Max 1 rebalance per 4 hours
   - Requires explicit operator approval
```

**Option C: Automated rebalancing (FUTURE — not for v1)**

Full automation with guardrails. Too risky for initial release.

### 11.5 Rebalancing Config

```toml
[rebalance]
enabled = true
check_interval_secs = 60
mode = "alert_only"                     # alert_only | semi_auto | auto

# Venue-level thresholds
min_venue_pct = 30                      # Warn if either side drops below 30%
critical_venue_pct = 15                 # Pause if either side drops below 15%

# Per-token thresholds
min_token_pct = 30
critical_token_pct = 20

# Safety limits (for semi_auto / auto modes)
max_rebalance_pct_per_day = 20          # Never move more than 20% of total capital per day
min_rebalance_interval_hours = 4
require_approval = true                 # For semi_auto: require operator approval
```

---

## 12. Database Schema

### 12.1 SQLite Schema (WAL Mode)

```sql
-- Enable WAL mode for concurrent reads + single writer
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;

-- ── Trading Pairs Configuration ──
CREATE TABLE trading_pairs (
    id TEXT PRIMARY KEY,                    -- UUID v7
    base_symbol TEXT NOT NULL,
    quote_symbol TEXT NOT NULL,
    base_mint TEXT NOT NULL,                -- Solana Pubkey base58
    quote_mint TEXT NOT NULL,
    binance_symbol TEXT NOT NULL,           -- e.g., "SOLUSDC"
    base_decimals INTEGER NOT NULL,
    quote_decimals INTEGER NOT NULL,
    min_trade_size_usd TEXT NOT NULL,       -- Decimal as string
    max_trade_size_usd TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL                -- ISO 8601
);

-- ── Detected Opportunities ──
CREATE TABLE opportunities (
    id TEXT PRIMARY KEY,                    -- UUID v7
    pair_id TEXT NOT NULL REFERENCES trading_pairs(id),
    direction TEXT NOT NULL,                -- 'buy_dex_sell_cex' or 'buy_cex_sell_dex'
    dex_venue TEXT NOT NULL,                -- 'jupiter' or 'raydium'
    dex_price TEXT NOT NULL,                -- Decimal as string
    cex_price TEXT NOT NULL,
    gross_spread_pct TEXT NOT NULL,
    net_spread_pct TEXT NOT NULL,
    trade_size_base TEXT NOT NULL,
    trade_size_usd TEXT NOT NULL,
    estimated_profit_usd TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'detected',
    detected_at TEXT NOT NULL,
    executed_at TEXT,
    expired_at TEXT
);

-- ── Trade Executions (both legs of each arb) ──
CREATE TABLE executions (
    id TEXT PRIMARY KEY,                    -- UUID v7
    opportunity_id TEXT NOT NULL REFERENCES opportunities(id),
    pair_id TEXT NOT NULL REFERENCES trading_pairs(id),
    direction TEXT NOT NULL,                -- 'buy_dex_sell_cex' or 'buy_cex_sell_dex'
    outcome TEXT NOT NULL,                  -- 'both_succeeded','dex_only','cex_only','both_failed'
    gross_profit_usd TEXT NOT NULL,
    fees_usd TEXT NOT NULL,
    net_profit_usd TEXT NOT NULL,
    execution_time_ms INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

-- ── Individual Execution Legs ──
CREATE TABLE execution_legs (
    id TEXT PRIMARY KEY,                    -- UUID v7
    execution_id TEXT NOT NULL REFERENCES executions(id),
    venue TEXT NOT NULL,                    -- 'solana_jupiter', 'solana_raydium', 'binance'
    side TEXT NOT NULL,                     -- 'buy' or 'sell'
    intended_price TEXT NOT NULL,
    actual_price TEXT,
    quantity TEXT NOT NULL,
    filled_quantity TEXT,
    fee_paid_usd TEXT,
    status TEXT NOT NULL,                   -- 'pending','submitted','confirmed','failed','timed_out'
    venue_id TEXT,                          -- TX signature (DEX) or order ID (CEX)
    started_at TEXT NOT NULL,
    completed_at TEXT,
    error_message TEXT
);

-- ── Balance Snapshots (periodic + after every trade) ──
CREATE TABLE balance_snapshots (
    id TEXT PRIMARY KEY,                    -- UUID v7
    solana_sol TEXT NOT NULL,
    solana_usdc TEXT NOT NULL,
    solana_tokens TEXT NOT NULL,            -- JSON: {"SOL":"123.45","JUP":"500.0",...}
    solana_total_usd TEXT NOT NULL,
    binance_usdc TEXT NOT NULL,
    binance_tokens TEXT NOT NULL,           -- JSON: {"SOL":"100.0","JUP":"300.0",...}
    binance_total_usd TEXT NOT NULL,
    total_usd TEXT NOT NULL,
    solana_pct TEXT NOT NULL,
    binance_pct TEXT NOT NULL,
    snapshot_at TEXT NOT NULL
);

-- ── Daily P&L Summaries ──
CREATE TABLE daily_pnl (
    date TEXT PRIMARY KEY,                  -- ISO 8601 date (YYYY-MM-DD)
    trades_executed INTEGER NOT NULL DEFAULT 0,
    trades_succeeded INTEGER NOT NULL DEFAULT 0,
    trades_partial INTEGER NOT NULL DEFAULT 0,
    trades_failed INTEGER NOT NULL DEFAULT 0,
    gross_profit_usd TEXT NOT NULL DEFAULT '0',
    total_fees_usd TEXT NOT NULL DEFAULT '0',
    net_profit_usd TEXT NOT NULL DEFAULT '0',
    best_trade_usd TEXT NOT NULL DEFAULT '0',
    worst_trade_usd TEXT NOT NULL DEFAULT '0',
    updated_at TEXT NOT NULL
);

-- ── Price Snapshots (sampled for historical analysis) ──
CREATE TABLE price_snapshots (
    id TEXT PRIMARY KEY,                    -- UUID v7
    pair_id TEXT NOT NULL REFERENCES trading_pairs(id),
    dex_venue TEXT NOT NULL,
    dex_price TEXT NOT NULL,
    cex_bid TEXT NOT NULL,
    cex_ask TEXT NOT NULL,
    gross_spread_pct TEXT NOT NULL,
    net_spread_pct TEXT NOT NULL,
    recorded_at TEXT NOT NULL
);

-- ── Risk Events (circuit breaker trips, violations) ──
CREATE TABLE risk_events (
    id TEXT PRIMARY KEY,                    -- UUID v7
    event_type TEXT NOT NULL,               -- 'circuit_breaker_tripped','daily_loss_exceeded', etc.
    details TEXT NOT NULL,                  -- JSON blob with event-specific data
    created_at TEXT NOT NULL
);

-- ── Indexes for query performance ──
CREATE INDEX idx_opportunities_status ON opportunities(status);
CREATE INDEX idx_opportunities_detected_at ON opportunities(detected_at);
CREATE INDEX idx_executions_created_at ON executions(created_at);
CREATE INDEX idx_executions_pair_id ON executions(pair_id);
CREATE INDEX idx_execution_legs_execution_id ON execution_legs(execution_id);
CREATE INDEX idx_balance_snapshots_snapshot_at ON balance_snapshots(snapshot_at);
CREATE INDEX idx_price_snapshots_pair_recorded ON price_snapshots(pair_id, recorded_at);
CREATE INDEX idx_risk_events_created_at ON risk_events(created_at);
```

### 12.2 Migration Strategy

Migrations live in `migrations/` as numbered SQL files. The application runs them on startup.

```rust
// sol-arb-db/src/lib.rs

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::path::Path;

pub async fn init_database(db_path: &str) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)           // 1 writer + 4 readers (WAL allows this)
        .connect_with(options)
        .await?;

    // Run migrations
    sqlx::migrate!("../migrations").run(&pool).await?;

    Ok(pool)
}
```

**Migration files:**

```
migrations/
├── 001_initial_schema.sql        # All tables above
└── 002_daily_pnl.sql             # daily_pnl table (added separately for clarity)
```

### 12.3 Background Writer

High-frequency data (price snapshots, balance updates) is written via a batched background task to avoid blocking the main engine loop.

```rust
use tokio::sync::mpsc;
use std::time::Duration;

enum WriteCommand {
    SaveOpportunity(ArbitrageOpportunity),
    SaveExecution(TradeExecution),
    SaveBalanceSnapshot(VenueBalances),
    SavePriceSnapshot(SpreadInfo),
    UpdateDailyPnl(chrono::NaiveDate, DailyPnl),
}

async fn background_writer(
    pool: SqlitePool,
    mut rx: mpsc::Receiver<WriteCommand>,
) {
    let mut batch: Vec<WriteCommand> = Vec::with_capacity(64);
    let flush_interval = Duration::from_millis(500);

    loop {
        // Collect commands for up to 500ms or 64 items
        let deadline = tokio::time::sleep(flush_interval);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                cmd = rx.recv() => {
                    match cmd {
                        Some(cmd) => {
                            batch.push(cmd);
                            if batch.len() >= 64 { break; }
                        }
                        None => {
                            // Channel closed — flush remaining and exit
                            flush_batch(&pool, &mut batch).await;
                            return;
                        }
                    }
                }
                _ = &mut deadline => { break; }
            }
        }

        if !batch.is_empty() {
            flush_batch(&pool, &mut batch).await;
        }
    }
}

async fn flush_batch(pool: &SqlitePool, batch: &mut Vec<WriteCommand>) {
    let mut tx = pool.begin().await.expect("begin transaction");

    for cmd in batch.drain(..) {
        match cmd {
            WriteCommand::SaveOpportunity(opp) => {
                sqlx::query(
                    "INSERT INTO opportunities (id, pair_id, direction, dex_venue, dex_price, \
                     cex_price, gross_spread_pct, net_spread_pct, trade_size_base, trade_size_usd, \
                     estimated_profit_usd, status, detected_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                )
                .bind(opp.id.to_string())
                .bind(opp.pair.id.to_string())
                .bind(opp.direction.to_string())
                .bind(opp.dex_venue.to_string())
                .bind(opp.dex_price.to_string())
                .bind(opp.cex_price.to_string())
                .bind(opp.gross_spread_pct.to_string())
                .bind(opp.net_spread_pct.to_string())
                .bind(opp.trade_size_base.to_string())
                .bind(opp.trade_size_usd.to_string())
                .bind(opp.estimated_profit_usd.to_string())
                .bind(opp.status.to_string())
                .bind(opp.detected_at.to_rfc3339())
                .execute(&mut *tx)
                .await
                .ok();
            }
            // ... similar for other variants
            _ => { /* implement per variant */ }
        }
    }

    tx.commit().await.expect("commit batch");
}
```

### 12.4 Data Retention

| Data Type | Retention | Strategy |
|-----------|-----------|----------|
| Trading pairs | Permanent | Configuration data |
| Opportunities | 90 days | Prune with `DELETE WHERE detected_at < date('now', '-90 days')` |
| Executions + legs | 1 year | Critical financial records |
| Balance snapshots | 30 days | High volume; keep only daily summaries after 30d |
| Price snapshots | 7 days | Very high volume; subsample before archiving |
| Daily P&L | Permanent | Aggregated, tiny footprint |
| Risk events | 90 days | Prune with executions |

**Cleanup job** (runs daily at 03:00 UTC):

```rust
async fn run_data_retention(pool: &SqlitePool) -> Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM price_snapshots WHERE recorded_at < datetime('now', '-7 days')")
        .execute(&mut *tx).await?;

    sqlx::query("DELETE FROM balance_snapshots WHERE snapshot_at < datetime('now', '-30 days')")
        .execute(&mut *tx).await?;

    sqlx::query("DELETE FROM opportunities WHERE detected_at < datetime('now', '-90 days')")
        .execute(&mut *tx).await?;

    sqlx::query("DELETE FROM risk_events WHERE created_at < datetime('now', '-90 days')")
        .execute(&mut *tx).await?;

    tx.commit().await?;

    // Reclaim space
    sqlx::query("VACUUM").execute(pool).await?;

    Ok(())
}
```

---

## 13. Configuration

### 13.1 Configuration File Structure

The system uses TOML configuration with layered loading: `config/default.toml` → `config/{ENV}.toml` → environment variables → `.env` file. The `config` crate handles merging.

**`config/default.toml`** — full reference configuration:

```toml
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# SOL-ARB — Default Configuration
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# ── Engine Core ──
[engine]
mode = "paper"                              # paper | live
min_spread_pct = "0.35"                     # Minimum net spread to trigger execution
default_trade_size_usd = "100"              # Default trade size in USD
max_trade_size_usd = "500"                  # Maximum single trade size
trade_cooldown_ms = 2000                    # Minimum ms between trades on same pair
max_concurrent_executions = 2               # Semaphore permits
partial_execution_policy = "alert_only"     # alert_only | auto_unwind | hold

# ── DEX Configuration ──
[dex]
preferred_venue = "jupiter"                 # jupiter | raydium
rpc_url = "https://api.mainnet-beta.solana.com"
rpc_backup_url = ""                         # Fallback RPC (e.g., Alchemy, Helius)
rpc_timeout_ms = 5000
rpc_max_retries = 3
wallet_path = ""                            # Path to Solana keypair JSON (set via env var)
slippage_bps = 50                           # Max slippage: 0.50%
compute_unit_limit = 200000

# Jupiter-specific
[dex.jupiter]
api_url = "https://quote-api.jup.ag/v6"
swap_api_url = "https://quote-api.jup.ag/v6/swap"
max_accounts = 64                           # Max accounts in a swap transaction
only_direct_routes = false                  # false = allow multi-hop
restrict_intermediate_tokens = true         # Only allow known intermediates

# Raydium-specific
[dex.raydium]
pool_program_id = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"
amm_authority = "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1"

# ── Jito MEV Protection ──
[jito]
enabled = true
block_engine_url = "https://amsterdam.mainnet.block-engine.jito.wtf"
tip_lamports = 5000                         # Default tip: 0.000005 SOL (~$0.00075)
min_tip_lamports = 1000
max_tip_lamports = 10_000_000               # 0.01 SOL cap
dynamic_tips = false                        # If true, query recent tips and use p75

# ── CEX Configuration ──
[cex]
exchange = "binance"
rest_url = "https://api.binance.com"
ws_url = "wss://stream.binance.com:9443/ws"
api_key = ""                                # Set via BINANCE_API_KEY env var
api_secret = ""                             # Set via BINANCE_API_SECRET env var
recv_window_ms = 5000                       # Binance recvWindow parameter
use_bnb_fee_discount = true                 # 25% fee discount with BNB

# Rate limiting (Binance limits: 1200 weight/min, 10 orders/sec)
[cex.rate_limit]
max_weight_per_minute = 1100                # Stay under 1200 limit
max_orders_per_second = 8                   # Stay under 10 limit
max_orders_per_day = 160000                 # Stay under 200,000 limit

# ── Risk Management ──
[risk]
daily_loss_limit_usd = "50"                 # Halt if daily losses exceed this
max_position_per_pair_usd = "500"           # Max open position per trading pair
max_unhedged_exposure_usd = "200"           # Max exposure from partial executions
max_consecutive_losses = 5                  # Trip circuit breaker after N consecutive losses
circuit_breaker_cooldown_secs = 300         # 5-minute cooldown after breaker trips
min_trade_size_usd = "10"                   # Don't trade below this (fees eat profit)
max_dex_price_age_ms = 2000                 # Reject if DEX price older than 2s
max_cex_price_age_ms = 1000                 # Reject if CEX price older than 1s
stale_warning_ms = 10000                    # Warn if any price > 10s old
stale_pause_ms = 30000                      # Pause engine if price > 30s old

# ── Rebalancing ──
[rebalance]
enabled = true
check_interval_secs = 60
mode = "alert_only"                         # alert_only | semi_auto | auto
min_venue_pct = 30                          # Warn if either side < 30%
critical_venue_pct = 15                     # Pause if either side < 15%
min_token_pct = 30
critical_token_pct = 20
max_rebalance_pct_per_day = 20
min_rebalance_interval_hours = 4
require_approval = true

# ── Database ──
[database]
path = "data/sol-arb.db"                    # SQLite file path
max_connections = 5                         # 1 writer + 4 readers
busy_timeout_secs = 5
wal_mode = true
retention_enabled = true
retention_run_hour_utc = 3                  # Run cleanup at 03:00 UTC

# ── HTTP Server ──
[server]
host = "127.0.0.1"
port = 3000
cors_origin = "http://localhost:5173"       # Vite dev server default
ws_heartbeat_secs = 30                      # WebSocket ping interval
request_timeout_secs = 30

# ── Logging ──
[logging]
level = "info"                              # trace | debug | info | warn | error
format = "pretty"                           # pretty | json | compact
log_file = ""                               # Empty = stdout only; set path for file logging
```

### 13.2 Trading Pair Configuration

Pairs are configured separately in `config/pairs.toml` for easy addition/removal without touching the main config.

**`config/pairs.toml`:**

```toml
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Trading Pair Definitions
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[pairs]]
base_symbol = "SOL"
quote_symbol = "USDC"
base_mint = "So11111111111111111111111111111111111111112"
quote_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
binance_symbol = "SOLUSDC"
base_decimals = 9
quote_decimals = 6
min_trade_size_usd = "20"
max_trade_size_usd = "500"
active = true

[[pairs]]
base_symbol = "JUP"
quote_symbol = "USDC"
base_mint = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN"
quote_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
binance_symbol = "JUPUSDC"
base_decimals = 6
quote_decimals = 6
min_trade_size_usd = "20"
max_trade_size_usd = "300"
active = true

[[pairs]]
base_symbol = "WIF"
quote_symbol = "USDC"
base_mint = "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm"
quote_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
binance_symbol = "WIFUSDC"
base_decimals = 6
quote_decimals = 6
min_trade_size_usd = "15"
max_trade_size_usd = "200"
active = true

[[pairs]]
base_symbol = "BONK"
quote_symbol = "USDC"
base_mint = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263"
quote_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
binance_symbol = "BONKUSDC"
base_decimals = 5
quote_decimals = 6
min_trade_size_usd = "10"
max_trade_size_usd = "150"
active = true

[[pairs]]
base_symbol = "PYTH"
quote_symbol = "USDC"
base_mint = "HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3"
quote_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
binance_symbol = "PYTHUSDC"
base_decimals = 6
quote_decimals = 6
min_trade_size_usd = "15"
max_trade_size_usd = "200"
active = true
```

### 13.3 Environment Variables

Secrets and host-specific settings are loaded from environment variables (or `.env` file via `dotenvy`).

**`.env.example`:**

```bash
# ── Solana ──
SOLANA_WALLET_PATH=/path/to/keypair.json
SOLANA_RPC_URL=https://api.mainnet-beta.solana.com
SOLANA_RPC_BACKUP_URL=https://solana-mainnet.g.alchemy.com/v2/YOUR_KEY

# ── Binance ──
BINANCE_API_KEY=your_api_key_here
BINANCE_API_SECRET=your_api_secret_here

# ── Database ──
DATABASE_PATH=data/sol-arb.db

# ── Server ──
SERVER_HOST=127.0.0.1
SERVER_PORT=3000

# ── Logging ──
RUST_LOG=sol_arb=info,sqlx=warn
```

### 13.4 Configuration Loading

```rust
// sol-arb-cli/src/main.rs

use config::{Config, Environment, File};
use sol_arb_types::AppConfig;

fn load_config() -> Result<AppConfig> {
    let env = std::env::var("SOL_ARB_ENV").unwrap_or_else(|_| "default".to_string());

    let config = Config::builder()
        // 1. Default config
        .add_source(File::with_name("config/default"))
        // 2. Environment-specific override (e.g., config/production.toml)
        .add_source(File::with_name(&format!("config/{}", env)).required(false))
        // 3. Trading pairs (always loaded)
        .add_source(File::with_name("config/pairs"))
        // 4. Environment variables (prefix SOL_ARB_, separator __)
        //    e.g., SOL_ARB_ENGINE__MODE=live → engine.mode = "live"
        .add_source(
            Environment::with_prefix("SOL_ARB")
                .separator("__")
                .try_parsing(true),
        )
        .build()?;

    let app_config: AppConfig = config.try_deserialize()?;

    // Validate critical settings
    validate_config(&app_config)?;

    Ok(app_config)
}

fn validate_config(config: &AppConfig) -> Result<()> {
    // Must have at least one active pair
    if config.pairs.iter().filter(|p| p.active).count() == 0 {
        anyhow::bail!("No active trading pairs configured");
    }

    // In live mode, secrets must be present
    if config.engine.mode == EngineMode::Live {
        if config.cex.api_key.is_empty() {
            anyhow::bail!("BINANCE_API_KEY required in live mode");
        }
        if config.dex.wallet_path.is_empty() {
            anyhow::bail!("SOLANA_WALLET_PATH required in live mode");
        }
    }

    // Risk limits must be positive
    if config.risk.daily_loss_limit_usd <= Decimal::ZERO {
        anyhow::bail!("daily_loss_limit_usd must be > 0");
    }

    Ok(())
}
```

### 13.5 Config Structs

```rust
// sol-arb-types/src/config.rs

use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub engine: EngineConfig,
    pub dex: DexConfig,
    pub jito: JitoConfig,
    pub cex: CexConfig,
    pub risk: RiskConfig,
    pub rebalance: RebalanceConfig,
    pub database: DatabaseConfig,
    pub server: ServerConfig,
    pub logging: LoggingConfig,
    #[serde(default)]
    pub pairs: Vec<PairConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineMode {
    Paper,
    Live,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EngineConfig {
    pub mode: EngineMode,
    pub min_spread_pct: Decimal,
    pub default_trade_size_usd: Decimal,
    pub max_trade_size_usd: Decimal,
    pub trade_cooldown_ms: u64,
    pub max_concurrent_executions: usize,
    pub partial_execution_policy: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DexConfig {
    pub preferred_venue: String,
    pub rpc_url: String,
    pub rpc_backup_url: String,
    pub rpc_timeout_ms: u64,
    pub rpc_max_retries: u32,
    pub wallet_path: String,
    pub slippage_bps: u16,
    pub compute_unit_limit: u32,
    pub jupiter: JupiterConfig,
    pub raydium: RaydiumConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JupiterConfig {
    pub api_url: String,
    pub swap_api_url: String,
    pub max_accounts: u8,
    pub only_direct_routes: bool,
    pub restrict_intermediate_tokens: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RaydiumConfig {
    pub pool_program_id: String,
    pub amm_authority: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JitoConfig {
    pub enabled: bool,
    pub block_engine_url: String,
    pub tip_lamports: u64,
    pub min_tip_lamports: u64,
    pub max_tip_lamports: u64,
    pub dynamic_tips: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CexConfig {
    pub exchange: String,
    pub rest_url: String,
    pub ws_url: String,
    pub api_key: String,
    pub api_secret: String,
    pub recv_window_ms: u64,
    pub use_bnb_fee_discount: bool,
    pub rate_limit: RateLimitConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    pub max_weight_per_minute: u32,
    pub max_orders_per_second: u32,
    pub max_orders_per_day: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RiskConfig {
    pub daily_loss_limit_usd: Decimal,
    pub max_position_per_pair_usd: Decimal,
    pub max_unhedged_exposure_usd: Decimal,
    pub max_consecutive_losses: u32,
    pub circuit_breaker_cooldown_secs: u64,
    pub min_trade_size_usd: Decimal,
    pub max_dex_price_age_ms: u64,
    pub max_cex_price_age_ms: u64,
    pub stale_warning_ms: u64,
    pub stale_pause_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RebalanceConfig {
    pub enabled: bool,
    pub check_interval_secs: u64,
    pub mode: String,
    pub min_venue_pct: Decimal,
    pub critical_venue_pct: Decimal,
    pub min_token_pct: Decimal,
    pub critical_token_pct: Decimal,
    pub max_rebalance_pct_per_day: Decimal,
    pub min_rebalance_interval_hours: u64,
    pub require_approval: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub path: String,
    pub max_connections: u32,
    pub busy_timeout_secs: u64,
    pub wal_mode: bool,
    pub retention_enabled: bool,
    pub retention_run_hour_utc: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub cors_origin: String,
    pub ws_heartbeat_secs: u64,
    pub request_timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
    pub log_file: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PairConfig {
    pub base_symbol: String,
    pub quote_symbol: String,
    pub base_mint: String,
    pub quote_mint: String,
    pub binance_symbol: String,
    pub base_decimals: u8,
    pub quote_decimals: u8,
    pub min_trade_size_usd: Decimal,
    pub max_trade_size_usd: Decimal,
    pub active: bool,
}
```

---

## 14. HTTP API Specification

### 14.1 Overview

The HTTP API is built with Axum and provides 6 REST endpoints + 1 WebSocket endpoint. It serves both monitoring (human operators) and programmatic access (future UI dashboard).

**Base URL:** `http://127.0.0.1:3000/api`

### 14.2 REST Endpoints

#### `GET /api/health`

Health check — confirms the system is running and all subsystems are reachable.

**Response `200 OK`:**

```json
{
  "status": "ok",
  "engine_running": true,
  "engine_mode": "paper",
  "uptime_secs": 3600,
  "dex_connected": true,
  "cex_connected": true,
  "db_connected": true,
  "active_pairs": 5,
  "version": "0.1.0"
}
```

**Response `503 Service Unavailable`** (if any critical subsystem is down):

```json
{
  "status": "degraded",
  "engine_running": false,
  "dex_connected": false,
  "cex_connected": true,
  "db_connected": true,
  "errors": ["DEX RPC connection failed: timeout after 5000ms"]
}
```

---

#### `GET /api/spreads`

Returns current live spreads for all active trading pairs. Updated every tick.

**Response `200 OK`:**

```json
{
  "spreads": [
    {
      "pair": "SOL/USDC",
      "direction": "buy_dex_sell_cex",
      "dex_venue": "jupiter",
      "dex_price": "148.2350",
      "cex_bid": "148.9200",
      "cex_ask": "148.9400",
      "gross_spread_pct": "0.4614",
      "total_fees_pct": "0.1750",
      "net_spread_pct": "0.2864",
      "profitable": false,
      "dex_price_age_ms": 340,
      "cex_price_age_ms": 52,
      "timestamp": "2026-04-05T21:30:00.123Z"
    },
    {
      "pair": "JUP/USDC",
      "direction": "buy_dex_sell_cex",
      "dex_venue": "jupiter",
      "dex_price": "0.8920",
      "cex_bid": "0.8975",
      "cex_ask": "0.8978",
      "gross_spread_pct": "0.6166",
      "total_fees_pct": "0.1750",
      "net_spread_pct": "0.4416",
      "profitable": true,
      "dex_price_age_ms": 520,
      "cex_price_age_ms": 30,
      "timestamp": "2026-04-05T21:30:00.123Z"
    }
  ],
  "updated_at": "2026-04-05T21:30:00.123Z"
}
```

---

#### `GET /api/executions?limit={n}&pair={symbol}`

Returns recent trade executions. Defaults to last 50.

**Query parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `limit` | u32 | 50 | Max results to return (1–500) |
| `pair` | string | — | Filter by base symbol (e.g., `SOL`) |

**Response `200 OK`:**

```json
{
  "executions": [
    {
      "id": "019513a8-7c3b-7def-8a1f-000000000001",
      "opportunity_id": "019513a8-7c3a-7def-8a1f-000000000001",
      "pair": "SOL/USDC",
      "direction": "buy_dex_sell_cex",
      "outcome": "both_succeeded",
      "dex_leg": {
        "venue": "jupiter",
        "side": "buy",
        "intended_price": "148.2350",
        "actual_price": "148.2510",
        "quantity": "0.674",
        "filled_quantity": "0.674",
        "fee_paid_usd": "0.025",
        "status": "confirmed",
        "venue_id": "5xGf...abc",
        "execution_time_ms": 1850
      },
      "cex_leg": {
        "venue": "binance",
        "side": "sell",
        "intended_price": "148.9200",
        "actual_price": "148.9100",
        "quantity": "0.674",
        "filled_quantity": "0.674",
        "fee_paid_usd": "0.100",
        "status": "confirmed",
        "venue_id": "123456789",
        "execution_time_ms": 85
      },
      "gross_profit_usd": "0.444",
      "fees_usd": "0.125",
      "net_profit_usd": "0.319",
      "execution_time_ms": 1850,
      "created_at": "2026-04-05T21:29:58.456Z"
    }
  ],
  "total_count": 142,
  "returned_count": 1
}
```

---

#### `GET /api/pnl?from={date}&to={date}`

Returns daily P&L summaries for the given date range.

**Query parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `from` | date | today | Start date (YYYY-MM-DD) |
| `to` | date | today | End date (YYYY-MM-DD) |

**Response `200 OK`:**

```json
{
  "daily": [
    {
      "date": "2026-04-05",
      "trades_executed": 47,
      "trades_succeeded": 42,
      "trades_partial": 3,
      "trades_failed": 2,
      "gross_profit_usd": "18.450",
      "total_fees_usd": "5.230",
      "net_profit_usd": "13.220",
      "best_trade_usd": "1.240",
      "worst_trade_usd": "-0.350"
    }
  ],
  "summary": {
    "total_net_profit_usd": "13.220",
    "total_trades": 47,
    "win_rate_pct": "89.36",
    "avg_profit_per_trade_usd": "0.281",
    "days": 1
  }
}
```

---

#### `GET /api/risk`

Returns current risk manager state: circuit breaker, daily P&L tracking, balance health.

**Response `200 OK`:**

```json
{
  "circuit_breaker": "closed",
  "daily_pnl_usd": "13.220",
  "daily_loss_limit_usd": "50.000",
  "daily_loss_remaining_usd": "36.780",
  "daily_trades": 47,
  "consecutive_losses": 0,
  "max_consecutive_losses": 5,
  "open_exposure_usd": "0.000",
  "max_exposure_usd": "200.000",
  "balances": {
    "solana_total_usd": "4850.00",
    "binance_total_usd": "5150.00",
    "solana_pct": "48.50",
    "binance_pct": "51.50",
    "venue_status": "healthy"
  },
  "last_trade_at": "2026-04-05T21:29:58.456Z",
  "timestamp": "2026-04-05T21:30:01.000Z"
}
```

---

#### `POST /api/engine/{action}`

Control the engine state. Actions: `pause`, `resume`, `stop`.

**Request:** No body required. Action is in the URL path.

**Response `200 OK`:**

```json
{
  "action": "pause",
  "previous_state": "running",
  "new_state": "paused",
  "message": "Engine paused. Price feeds still active. No new executions."
}
```

**Response `409 Conflict`** (if action is invalid for current state):

```json
{
  "error": "Cannot resume: engine is not paused (current state: running)"
}
```

---

#### `POST /api/circuit-breaker/{action}`

Manually control the circuit breaker. Actions: `trip`, `reset`.

**Request for `trip`:**

```json
{
  "reason": "Manual stop: investigating unexpected P&L"
}
```

**Response `200 OK`:**

```json
{
  "action": "trip",
  "circuit_breaker": "open",
  "cooldown_until": "2026-04-05T21:35:01.000Z",
  "reason": "Manual stop: investigating unexpected P&L"
}
```

### 14.3 WebSocket Endpoint

#### `GET /api/ws` (Upgrade to WebSocket)

Streams live events from the engine in real-time. The client receives a JSON message for every price update, opportunity detection, execution completion, and system event.

**Connection:**

```
ws://127.0.0.1:3000/api/ws
```

**Server → Client messages:**

Each message is a JSON object with a `type` field to distinguish event kinds.

```json
// Price update (most frequent — throttled to 1 per pair per 500ms)
{
  "type": "price_update",
  "data": {
    "pair": "SOL/USDC",
    "dex_price": "148.2350",
    "cex_bid": "148.9200",
    "cex_ask": "148.9400",
    "net_spread_pct": "0.2864",
    "profitable": false
  }
}

// Opportunity detected
{
  "type": "opportunity_detected",
  "data": {
    "id": "019513a8-7c3a-7def-8a1f-000000000001",
    "pair": "JUP/USDC",
    "direction": "buy_dex_sell_cex",
    "net_spread_pct": "0.4416",
    "estimated_profit_usd": "0.442",
    "status": "executing"
  }
}

// Execution completed
{
  "type": "execution_completed",
  "data": {
    "id": "019513a8-7c3b-7def-8a1f-000000000001",
    "pair": "JUP/USDC",
    "outcome": "both_succeeded",
    "net_profit_usd": "0.319",
    "execution_time_ms": 1850
  }
}

// Risk event
{
  "type": "risk_event",
  "data": {
    "event": "circuit_breaker_tripped",
    "reason": "5 consecutive losses",
    "cooldown_until": "2026-04-05T21:35:01.000Z"
  }
}

// Balance update (every 60 seconds)
{
  "type": "balance_update",
  "data": {
    "solana_total_usd": "4850.00",
    "binance_total_usd": "5150.00",
    "solana_pct": "48.50",
    "binance_pct": "51.50"
  }
}
```

**Client → Server messages:**

```json
// Subscribe to specific pairs only (optional filter)
{
  "action": "subscribe",
  "pairs": ["SOL/USDC", "JUP/USDC"]
}

// Unsubscribe from a pair
{
  "action": "unsubscribe",
  "pairs": ["BONK/USDC"]
}

// Ping (keepalive)
{
  "action": "ping"
}
```

**Server sends `pong` response:**

```json
{
  "type": "pong",
  "timestamp": "2026-04-05T21:30:01.000Z"
}
```

### 14.4 Router Construction

```rust
// sol-arb-server/src/lib.rs

use axum::{
    Router,
    routing::{get, post},
    extract::State,
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

pub struct AppState {
    pub engine: Arc<dyn ArbitrageEngine>,
    pub risk: Arc<dyn RiskManager>,
    pub db: Arc<dyn Repository>,
    pub event_rx: broadcast::Receiver<EngineEvent>,
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(state.config.server.cors_origin.parse().unwrap())
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    Router::new()
        // Health & monitoring
        .route("/api/health", get(routes::health))
        .route("/api/spreads", get(routes::get_spreads))
        .route("/api/executions", get(routes::get_executions))
        .route("/api/pnl", get(routes::get_pnl))
        .route("/api/risk", get(routes::get_risk))
        // Engine control
        .route("/api/engine/:action", post(routes::engine_control))
        .route("/api/circuit-breaker/:action", post(routes::circuit_breaker_control))
        // WebSocket
        .route("/api/ws", get(ws::ws_handler))
        .layer(cors)
        .with_state(state)
}
```

### 14.5 Error Response Format

All error responses follow a consistent format:

```json
{
  "error": "Human-readable error message",
  "code": "MACHINE_READABLE_CODE",
  "details": {}
}
```

**Error codes:**

| HTTP Status | Code | Meaning |
|-------------|------|---------|
| 400 | `BAD_REQUEST` | Invalid query parameter or request body |
| 404 | `NOT_FOUND` | Resource does not exist |
| 409 | `CONFLICT` | Action invalid for current state |
| 429 | `RATE_LIMITED` | Too many requests (unlikely for single-operator) |
| 500 | `INTERNAL_ERROR` | Unexpected server error |
| 503 | `SERVICE_UNAVAILABLE` | Engine or subsystem is down |

```rust
// sol-arb-server/src/error.rs

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

pub enum ApiError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Internal(anyhow::Error),
    ServiceUnavailable(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", msg),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, "NOT_FOUND", msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, "CONFLICT", msg),
            ApiError::Internal(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                format!("Internal error: {err}"),
            ),
            ApiError::ServiceUnavailable(msg) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "SERVICE_UNAVAILABLE",
                msg,
            ),
        };

        let body = json!({ "error": message, "code": code });
        (status, axum::Json(body)).into_response()
    }
}
```

---

## 15. Implementation Roadmap

### 15.1 Phase Overview

The implementation is budgeted at **~140 hours** for a single developer. Phases are sequential — each builds on the previous. Paper trading mode is mandatory before live capital.

| Phase | Duration | Hours | Deliverable |
|-------|----------|-------|-------------|
| **Phase 1: Foundation** | Week 1–2 | 30h | Types, config, DB, project scaffold |
| **Phase 2: Connectors** | Week 3–4 | 40h | DEX + CEX connectors with paper-trade mocks |
| **Phase 3: Engine** | Week 5–6 | 35h | Detection loop, executor, risk manager |
| **Phase 4: API + Polish** | Week 7 | 15h | HTTP API, WebSocket, logging |
| **Phase 5: Paper Trading** | Week 8 | 10h | End-to-end paper trading, tuning |
| **Phase 6: Live** | Week 9 | 10h | Small capital live run, monitoring |
| **Total** | | **140h** | |

### 15.2 Phase 1 — Foundation (30 hours)

**Goal:** Project skeleton compiles, database works, config loads, all types defined.

| # | Task | Hours | Dependencies | Verification |
|---|------|-------|--------------|--------------|
| 1.1 | `cargo init` workspace + 8 crate scaffolds | 2h | — | `cargo check` passes |
| 1.2 | Define all types in `sol-arb-types` (sections 5.1–5.5 of this doc) | 6h | 1.1 | All types compile with serde derives |
| 1.3 | Configuration structs + TOML loading (section 13) | 4h | 1.1 | `load_config()` returns valid AppConfig from `default.toml` |
| 1.4 | SQLite schema + migrations (section 12.1) | 4h | 1.1 | `sqlx migrate run` succeeds; all tables exist |
| 1.5 | Repository trait + SQLite implementation (section 6.5) | 8h | 1.2, 1.4 | Integration tests: save + read opportunity, execution, pnl |
| 1.6 | Background writer task (section 12.3) | 3h | 1.5 | Unit test: batch 100 writes, verify all persisted |
| 1.7 | Error types for all crates | 3h | 1.2 | All error enums compile with thiserror derives |

**Exit criteria:**
- `cargo build --workspace` succeeds
- `cargo test --workspace` passes all unit tests
- Config loads from file + env vars
- Database creates tables, runs CRUD operations

### 15.3 Phase 2 — Connectors (40 hours)

**Goal:** Can read live DEX + CEX prices in paper mode. Mock connectors work for testing.

| # | Task | Hours | Dependencies | Verification |
|---|------|-------|--------------|--------------|
| 2.1 | `DexConnector` trait definition (section 6.1) | 2h | Phase 1 | Trait compiles |
| 2.2 | Jupiter connector — quote API + swap API (section 8.1) | 10h | 2.1 | Integration test: get real quote for SOL/USDC |
| 2.3 | Raydium connector — pool state reading (section 8.2) | 8h | 2.1 | Integration test: read real Raydium SOL/USDC pool price |
| 2.4 | Jito bundle submission (section 8.3) | 5h | 2.2 | Unit test with mock; devnet integration test |
| 2.5 | Transaction signing + priority fees (section 8.4) | 3h | 2.2 | Signs a transaction, serializes correctly |
| 2.6 | `CexConnector` trait + Binance REST client | 4h | Phase 1 | Integration test: ping, get balances (testnet) |
| 2.7 | Binance WebSocket feed (@bookTicker) | 4h | 2.6 | Integration test: connect, receive 10 updates, disconnect |
| 2.8 | Mock connectors (DEX + CEX) for paper trading | 4h | 2.1, 2.6 | Unit tests: mocks return configurable prices/results |

**Exit criteria:**
- Jupiter returns real quotes on mainnet (read-only)
- Raydium pool price matches on-chain data
- Binance WebSocket streams live book ticker data
- Mock connectors work for paper trading mode

### 15.4 Phase 3 — Engine (35 hours)

**Goal:** Full detection → execution loop runs in paper mode with mock connectors.

| # | Task | Hours | Dependencies | Verification |
|---|------|-------|--------------|--------------|
| 3.1 | Price feed aggregator (merges DEX + CEX streams) | 4h | Phase 2 | Unit test: feeds 10 updates, produces merged state |
| 3.2 | Spread detector (computes both directions, subtracts fees) | 4h | 3.1 | Unit test: known prices → expected spread |
| 3.3 | Fee model (all-in cost calculation per venue + direction) | 3h | 3.2 | Unit test: matches hand-calculated fees from section 1.3 |
| 3.4 | Risk manager — circuit breaker + daily loss limit | 5h | Phase 1 | Unit test: trips after 5 losses, resets after cooldown |
| 3.5 | Risk manager — stale price, balance, position limits | 4h | 3.4 | Unit test: rejects stale prices, oversized positions |
| 3.6 | Trade executor (concurrent dual-leg with tokio::join!) | 6h | Phase 2 | Unit test with mocks: both succeed, one fails, both fail |
| 3.7 | Reconciler (maps results to TradeExecution, handles partial) | 4h | 3.6 | Unit test: all 4 outcome variants |
| 3.8 | Main engine loop (section 7.2 pseudocode → real code) | 5h | 3.1–3.7 | Integration test: feed mock prices, detect opportunity, execute, record |

**Exit criteria:**
- Engine runs in paper mode with mock connectors
- Detects spreads, checks risk, executes, records to DB
- Circuit breaker trips and resets correctly
- All 4 execution outcomes handled (both ok, dex only, cex only, both fail)

### 15.5 Phase 4 — API + Polish (15 hours)

**Goal:** HTTP API works, WebSocket streams events, logging is production-grade.

| # | Task | Hours | Dependencies | Verification |
|---|------|-------|--------------|--------------|
| 4.1 | Axum router + health endpoint | 2h | Phase 3 | `curl /api/health` returns JSON |
| 4.2 | Spreads, executions, PnL, risk endpoints | 4h | 4.1 | `curl` each endpoint returns expected JSON |
| 4.3 | Engine control + circuit breaker endpoints | 2h | 4.1 | Pause/resume/stop via API works |
| 4.4 | WebSocket handler — event stream | 4h | 4.1 | `wscat` connects, receives price updates |
| 4.5 | Structured logging (tracing-subscriber, JSON format) | 2h | Phase 3 | Logs in JSON format with span context |
| 4.6 | Graceful shutdown (SIGINT/SIGTERM → CancellationToken) | 1h | 4.1 | Ctrl+C → waits for in-flight, exits cleanly |

**Exit criteria:**
- All 6 REST endpoints return correct JSON
- WebSocket streams live events
- Ctrl+C → graceful shutdown in < 5 seconds

### 15.6 Phase 5 — Paper Trading (10 hours)

**Goal:** System runs 48+ hours in paper mode against real market data without crashes.

| # | Task | Hours | Dependencies | Verification |
|---|------|-------|--------------|--------------|
| 5.1 | Paper trading coordinator (real prices + mock execution) | 3h | Phase 4 | Runs for 1 hour without panic |
| 5.2 | Run 48-hour paper trading session | 4h | 5.1 | No crashes; P&L tracking matches manual calculation |
| 5.3 | Tune parameters (min spread, trade size, cooldown) | 2h | 5.2 | Optimized based on paper trading data |
| 5.4 | Validate: at least 5 paper trades per day with positive net P&L | 1h | 5.3 | Dashboard shows ≥5 trades/day, net P&L > 0 |

**Exit criteria:**
- 48-hour paper run completes without crashes or hangs
- Paper P&L is net positive
- Parameters tuned based on real spread observations
- Confidence that the system will not lose money from bugs

### 15.7 Phase 6 — Live Trading (10 hours)

**Goal:** Small capital live run on mainnet. Capital at risk: $100–$500.

| # | Task | Hours | Dependencies | Verification |
|---|------|-------|--------------|--------------|
| 6.1 | Fund Solana wallet + Binance account ($500 total, split 50/50) | 1h | Phase 5 | Balances visible in both venues |
| 6.2 | Deploy with `mode = "live"`, conservative risk limits | 1h | 6.1 | Engine starts, reads live balances |
| 6.3 | Monitor first 10 live trades (manual observation) | 4h | 6.2 | All trades execute both legs; no stuck positions |
| 6.4 | Run 24-hour unattended live session | 3h | 6.3 | No circuit breaker trips; net P&L > 0 |
| 6.5 | Post-mortem: analyze all trades, fees, slippage | 1h | 6.4 | Actual fees match model; no unexpected costs |

**Exit criteria:**
- 10+ live trades executed with both legs confirmed
- Actual slippage within expected range
- No exposed (unhedged) positions
- Daily P&L is net positive after all fees
- System runs 24 hours without manual intervention

### 15.8 Risk Milestones

Each phase has a risk gate — do NOT proceed to the next phase until the gate is satisfied.

| Gate | Requirement | Consequence of Failure |
|------|-------------|----------------------|
| **G1: Types compile** | All types from section 5 compile with serde | Re-examine type design |
| **G2: Connectors work** | Can read real prices from both venues | Investigate API changes, rate limits |
| **G3: Engine stable** | Paper mode runs 1 hour without panic | Fix crashes before continuing |
| **G4: Paper profitable** | 48-hour paper run is net positive | Re-tune parameters; reconsider viability |
| **G5: Live safe** | First 10 live trades: no stuck positions | Stop live trading; fix executor/reconciler |
| **G6: Live profitable** | 24-hour live run is net positive | Analyze: is it fees? slippage? spread frequency? |

### 15.9 Post-Launch Priorities

After successful Phase 6, these are the highest-value improvements in priority order:

| Priority | Improvement | Estimated Hours | Impact |
|----------|-------------|-----------------|--------|
| P0 | **Dynamic trade sizing** — scale size with spread magnitude | 8h | 2–3× profit per opportunity |
| P1 | **Multi-DEX routing** — check Jupiter AND Raydium, pick best | 6h | More opportunities; better prices |
| P2 | **Jito dynamic tipping** — query recent tips, bid at p75 | 4h | Lower fees when network is calm |
| P3 | **Telegram alerts** — notify on circuit breaker, large trades | 4h | Peace of mind; faster incident response |
| P4 | **Historical spread analysis** — dashboard showing spread frequency by pair/hour | 8h | Data-driven pair selection |
| P5 | **Auto-unwind for partial executions** — configurable policy | 6h | Reduce manual intervention |
| P6 | **Additional pairs** — add RNDR, RAY, ORCA when liquidity is sufficient | 4h each | More opportunities |