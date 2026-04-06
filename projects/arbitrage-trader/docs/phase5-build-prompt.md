# Phase 5 — TUI + Paper Trading + Production Readiness — Build Prompt

**Goal:** Terminal dashboard, paper trading mode for safe validation, graceful shutdown, health monitoring. After this phase: the system is ready for live paper trading against real price feeds.

**Project root:** `/Users/mihail/projects/vault/projects/arbitrage-trader`
**Depends on:** Phase 4 complete (engine compiles, 130+ tests)

---

## Context: What Exists

**CLI (`crates/arb-cli/src/main.rs`):**
- Clap args: `--paper`, `--match`, `--tui`, `--headless`
- Config loading from `config/default.toml`
- DB init, risk manager init
- `--match` mode fully working with demo data
- `--paper` and `--tui` print "not yet implemented"

**CLI deps already in Cargo.toml:** arb-types, arb-db, arb-risk, arb-engine, arb-matcher, clap, tokio, config, tracing
**Workspace deps available:** `ratatui = "0.29"`, `crossterm = "0.28"`

**Engine API (from Phase 4):**
```rust
// Engine::new(poly, kalshi, price_cache, executor, monitor, tracker, unwinder, config) -> Self
// Engine::run(self: Arc<Self>, price_rx: mpsc::Receiver<PriceUpdate>, pairs: Vec<PairInfo>) -> Result<()>
// Engine::shutdown(&self)  — cancels all open orders
// Engine.price_cache: Arc<PriceCache>
```

**DB queries available for TUI:**
```rust
list_active_market_pairs() -> Vec<MarketPairRow>
list_orders_by_status("open") -> Vec<OrderRow>
list_open_positions() -> Vec<PositionRow>
list_opportunities_by_status("detected") -> Vec<OpportunityRow>
get_daily_pnl(NaiveDate) -> Option<DailyPnlRow>
```

**ExposureTracker getters:**
```rust
total_exposure() -> Decimal
unhedged_exposure() -> Decimal
daily_loss() -> Decimal
unwind_rate_pct() -> Decimal
```

**ArbError variants (USE ONLY THESE):**
```rust
PlatformError { platform, message }, AuthError { platform, message },
RateLimited { platform, retry_after_ms }, OrderRejected { platform, reason },
InvalidPrice(String), MarketNotFound(String), PairNotVerified(String),
Database(String), Config(String), WebSocket(String), Other(String)
```

**PredictionMarketConnector trait:** 11 methods (list_markets, get_market, get_order_book, subscribe_prices, place_limit_order, cancel_order, get_order, list_open_orders, get_balance, get_positions, platform)

---

## Prompt 5-A: Paper Trading Connector

### File: `crates/arb-engine/src/paper.rs`

The PaperConnector wraps a REAL connector for market data but simulates all trading locally.

**Safety guarantee:** The PaperConnector delegates ONLY read methods (list_markets, get_market, get_order_book, subscribe_prices, get_balance/positions) to the inner connector. All write methods (place_limit_order, cancel_order) are handled locally with ZERO network calls.

```rust
use arb_types::*;
use arb_types::order::{OrderBookLevel, OrderStatus, Side};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Paper trading state — tracks simulated orders, positions, balance.
#[derive(Debug)]
pub struct PaperState {
    orders: HashMap<String, PaperOrder>,
    balance: Decimal,
    initial_balance: Decimal,
    next_order_id: u64,
    fill_probability: f64,
    fill_delay_ms: u64,
}

#[derive(Debug, Clone)]
struct PaperOrder {
    response: OrderResponse,
    request: LimitOrderRequest,
    placed_at: DateTime<Utc>,
    will_fill: bool,
    fill_after: DateTime<Utc>,
}

impl PaperState {
    pub fn new(initial_balance: Decimal, fill_probability: f64, fill_delay_ms: u64) -> Self {
        Self {
            orders: HashMap::new(),
            balance: initial_balance,
            initial_balance,
            next_order_id: 1,
            fill_probability,
            fill_delay_ms,
        }
    }

    pub fn total_pnl(&self) -> Decimal {
        self.balance - self.initial_balance
    }
}

/// Wraps a real connector for market data, simulates trading locally.
///
/// SAFETY: place_limit_order and cancel_order NEVER touch the network.
/// Only market data methods (list_markets, get_order_book, subscribe_prices)
/// delegate to the inner real connector.
pub struct PaperConnector {
    inner: Arc<dyn PredictionMarketConnector>,
    state: Arc<Mutex<PaperState>>,
    platform: Platform,
}

impl PaperConnector {
    pub fn new(
        inner: Arc<dyn PredictionMarketConnector>,
        initial_balance: Decimal,
        fill_probability: f64,
        fill_delay_ms: u64,
    ) -> Self {
        let platform = inner.platform();
        Self {
            inner,
            state: Arc::new(Mutex::new(PaperState::new(initial_balance, fill_probability, fill_delay_ms))),
            platform,
        }
    }

    pub fn state(&self) -> &Arc<Mutex<PaperState>> {
        &self.state
    }
}

#[async_trait]
impl PredictionMarketConnector for PaperConnector {
    fn platform(&self) -> Platform {
        self.platform
    }

    // === MARKET DATA: delegate to real connector ===

    async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>, ArbError> {
        self.inner.list_markets(status).await
    }

    async fn get_market(&self, id: &str) -> Result<Market, ArbError> {
        self.inner.get_market(id).await
    }

    async fn get_order_book(&self, id: &str) -> Result<OrderBook, ArbError> {
        self.inner.get_order_book(id).await
    }

    async fn subscribe_prices(
        &self,
        ids: &[String],
        tx: mpsc::Sender<PriceUpdate>,
    ) -> Result<SubHandle, ArbError> {
        self.inner.subscribe_prices(ids, tx).await
    }

    // === TRADING: simulated locally, ZERO network calls ===

    async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<OrderResponse, ArbError> {
        let mut state = self.state.lock();

        // Check simulated balance
        let cost = req.price * Decimal::from(req.quantity);
        if state.balance < cost {
            return Err(ArbError::OrderRejected {
                platform: self.platform,
                reason: format!("paper: insufficient balance ({} < {})", state.balance, cost),
            });
        }

        let order_id = format!("paper-{}-{}", self.platform, state.next_order_id);
        state.next_order_id += 1;

        // Determine if this order will fill (probability-based)
        let will_fill = rand::random::<f64>() < state.fill_probability;
        let fill_after = Utc::now() + chrono::Duration::milliseconds(state.fill_delay_ms as i64);

        let response = OrderResponse {
            order_id: order_id.clone(),
            status: OrderStatus::Open,
            filled_quantity: 0,
            price: req.price,
            side: req.side,
            market_id: req.market_id.clone(),
        };

        state.orders.insert(order_id.clone(), PaperOrder {
            response: response.clone(),
            request: req.clone(),
            placed_at: Utc::now(),
            will_fill,
            fill_after,
        });

        // Deduct balance immediately (reserved for this order)
        state.balance -= cost;

        info!(
            platform = %self.platform,
            order_id,
            will_fill,
            price = %req.price,
            qty = req.quantity,
            "paper order placed"
        );

        Ok(response)
    }

    async fn cancel_order(&self, order_id: &str) -> Result<(), ArbError> {
        let mut state = self.state.lock();
        if let Some(order) = state.orders.get_mut(order_id) {
            if order.response.status == OrderStatus::Open {
                order.response.status = OrderStatus::Cancelled;
                // Refund reserved balance
                let cost = order.request.price * Decimal::from(order.request.quantity);
                state.balance += cost;
                info!(order_id, "paper order cancelled, balance refunded");
            }
        }
        Ok(())
    }

    async fn get_order(&self, order_id: &str) -> Result<OrderResponse, ArbError> {
        let mut state = self.state.lock();
        let order = state.orders.get_mut(order_id)
            .ok_or_else(|| ArbError::Other(format!("paper order not found: {order_id}")))?;

        // Check if fill time has passed
        if order.will_fill && order.response.status == OrderStatus::Open && Utc::now() >= order.fill_after {
            order.response.status = OrderStatus::Filled;
            order.response.filled_quantity = order.request.quantity;
            info!(order_id, "paper order filled");
        }

        Ok(order.response.clone())
    }

    async fn list_open_orders(&self) -> Result<Vec<OrderResponse>, ArbError> {
        let state = self.state.lock();
        Ok(state.orders.values()
            .filter(|o| o.response.status == OrderStatus::Open)
            .map(|o| o.response.clone())
            .collect())
    }

    async fn get_balance(&self) -> Result<Decimal, ArbError> {
        Ok(self.state.lock().balance)
    }

    async fn get_positions(&self) -> Result<Vec<PlatformPosition>, ArbError> {
        // Paper positions tracked by the engine's Tracker, not here
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use arb_types::order::Side;

    // Use a simple mock for the inner connector in paper tests
    struct DummyConnector;

    #[async_trait]
    impl PredictionMarketConnector for DummyConnector {
        fn platform(&self) -> Platform { Platform::Polymarket }
        async fn list_markets(&self, _: MarketStatus) -> Result<Vec<Market>, ArbError> { Ok(vec![]) }
        async fn get_market(&self, _: &str) -> Result<Market, ArbError> { Err(ArbError::Other("dummy".into())) }
        async fn get_order_book(&self, _: &str) -> Result<OrderBook, ArbError> { Ok(OrderBook::default()) }
        async fn subscribe_prices(&self, _: &[String], _: mpsc::Sender<PriceUpdate>) -> Result<SubHandle, ArbError> {
            let (tx, _) = tokio::sync::oneshot::channel();
            Ok(SubHandle { cancel_tx: tx })
        }
        async fn place_limit_order(&self, _: &LimitOrderRequest) -> Result<OrderResponse, ArbError> {
            panic!("REAL place_limit_order called in paper mode — THIS MUST NEVER HAPPEN");
        }
        async fn cancel_order(&self, _: &str) -> Result<(), ArbError> {
            panic!("REAL cancel_order called in paper mode — THIS MUST NEVER HAPPEN");
        }
        async fn get_order(&self, _: &str) -> Result<OrderResponse, ArbError> {
            panic!("REAL get_order called in paper mode");
        }
        async fn list_open_orders(&self) -> Result<Vec<OrderResponse>, ArbError> { Ok(vec![]) }
        async fn get_balance(&self) -> Result<Decimal, ArbError> { Ok(dec!(10000)) }
        async fn get_positions(&self) -> Result<Vec<PlatformPosition>, ArbError> { Ok(vec![]) }
    }

    #[tokio::test]
    async fn test_paper_place_and_get_order() {
        let inner: Arc<dyn PredictionMarketConnector> = Arc::new(DummyConnector);
        let paper = PaperConnector::new(inner, dec!(10000), 1.0, 0); // 100% fill, instant

        let req = LimitOrderRequest {
            market_id: "tok-123".into(),
            side: Side::Yes,
            price: dec!(0.42),
            quantity: 50,
        };
        let resp = paper.place_limit_order(&req).await.unwrap();
        assert_eq!(resp.status, OrderStatus::Open);
        assert!(resp.order_id.starts_with("paper-"));

        // Balance should be deducted
        let bal = paper.get_balance().await.unwrap();
        assert_eq!(bal, dec!(10000) - dec!(21.00)); // 0.42 * 50

        // After fill delay (0ms), should be filled
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let filled = paper.get_order(&resp.order_id).await.unwrap();
        assert_eq!(filled.status, OrderStatus::Filled);
        assert_eq!(filled.filled_quantity, 50);
    }

    #[tokio::test]
    async fn test_paper_cancel_refunds_balance() {
        let inner: Arc<dyn PredictionMarketConnector> = Arc::new(DummyConnector);
        let paper = PaperConnector::new(inner, dec!(10000), 0.0, 5000); // 0% fill

        let req = LimitOrderRequest {
            market_id: "tok-123".into(),
            side: Side::Yes,
            price: dec!(0.50),
            quantity: 100,
        };
        let resp = paper.place_limit_order(&req).await.unwrap();
        assert_eq!(paper.get_balance().await.unwrap(), dec!(9950)); // 10000 - 50

        paper.cancel_order(&resp.order_id).await.unwrap();
        assert_eq!(paper.get_balance().await.unwrap(), dec!(10000)); // refunded
    }

    #[tokio::test]
    async fn test_paper_rejects_insufficient_balance() {
        let inner: Arc<dyn PredictionMarketConnector> = Arc::new(DummyConnector);
        let paper = PaperConnector::new(inner, dec!(10), 1.0, 0); // only $10

        let req = LimitOrderRequest {
            market_id: "tok-123".into(),
            side: Side::Yes,
            price: dec!(0.50),
            quantity: 100, // costs $50
        };
        let result = paper.place_limit_order(&req).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_paper_never_calls_real_trading() {
        // DummyConnector panics if real trading methods are called
        // PaperConnector should never trigger those panics
        let inner: Arc<dyn PredictionMarketConnector> = Arc::new(DummyConnector);
        let paper = PaperConnector::new(inner, dec!(10000), 1.0, 0);

        // These should all work without panic
        let req = LimitOrderRequest { market_id: "t".into(), side: Side::Yes, price: dec!(0.5), quantity: 10 };
        let resp = paper.place_limit_order(&req).await.unwrap();
        paper.cancel_order(&resp.order_id).await.unwrap();
        let _ = paper.get_order(&resp.order_id).await;
        let _ = paper.list_open_orders().await;
        // If we get here without panic, the safety boundary held
    }

    #[tokio::test]
    async fn test_paper_list_open_orders() {
        let inner: Arc<dyn PredictionMarketConnector> = Arc::new(DummyConnector);
        let paper = PaperConnector::new(inner, dec!(10000), 0.0, 99999); // never fills

        let req = LimitOrderRequest { market_id: "t".into(), side: Side::Yes, price: dec!(0.1), quantity: 10 };
        paper.place_limit_order(&req).await.unwrap();
        paper.place_limit_order(&req).await.unwrap();

        let open = paper.list_open_orders().await.unwrap();
        assert_eq!(open.len(), 2);
    }
}
```

**Also add to `crates/arb-engine/src/lib.rs`:**
```rust
pub mod paper;
```

**Add to `crates/arb-engine/Cargo.toml` `[dependencies]`:**
```toml
rand = { workspace = true }
```

Check if `rand` is in workspace deps. If not, add `rand = "0.8"` to root `Cargo.toml` `[workspace.dependencies]`.

### Verification
```bash
cargo test -p arb-engine  # paper tests should pass
cargo clippy -p arb-engine -- -D warnings
```

---

## Prompt 5-B: TUI Dashboard

### Add deps to CLI

**File:** `crates/arb-cli/Cargo.toml` — add to `[dependencies]`:
```toml
ratatui = { workspace = true }
crossterm = { workspace = true }
```

### File: `crates/arb-cli/src/tui.rs`

```rust
use arb_db::models::{DailyPnlRow, OrderRow, PositionRow};
use arb_db::SqliteRepository;
use arb_risk::RiskManager;
use chrono::Utc;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use parking_lot::RwLock;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;
use rust_decimal::Decimal;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Shared state snapshot for TUI rendering.
/// Updated periodically from the DB + risk manager.
pub struct TuiState {
    pub mode: String,           // "PAPER" or "LIVE"
    pub engine_running: bool,
    pub started_at: Instant,
    pub open_orders: Vec<OrderRow>,
    pub positions: Vec<PositionRow>,
    pub daily_pnl: Option<DailyPnlRow>,
    pub total_exposure: Decimal,
    pub unhedged_exposure: Decimal,
    pub daily_loss: Decimal,
    pub unwind_rate: Decimal,
    pub pair_count: usize,
}

impl TuiState {
    pub fn new(mode: &str) -> Self {
        Self {
            mode: mode.to_string(),
            engine_running: true,
            started_at: Instant::now(),
            open_orders: Vec::new(),
            positions: Vec::new(),
            daily_pnl: None,
            total_exposure: Decimal::ZERO,
            unhedged_exposure: Decimal::ZERO,
            daily_loss: Decimal::ZERO,
            unwind_rate: Decimal::ZERO,
            pair_count: 0,
        }
    }
}

/// Refresh TUI state from DB + risk manager.
pub async fn refresh_state(
    state: &mut TuiState,
    db: &SqliteRepository,
    risk_manager: &RwLock<RiskManager>,
) {
    // DB queries — ignore errors (TUI should never crash)
    if let Ok(orders) = db.list_orders_by_status("open").await {
        state.open_orders = orders;
    }
    if let Ok(positions) = db.list_open_positions().await {
        state.positions = positions;
    }
    if let Ok(Some(pnl)) = db.get_daily_pnl(Utc::now().date_naive()).await {
        state.daily_pnl = Some(pnl);
    }

    // Risk manager
    let rm = risk_manager.read();
    state.total_exposure = rm.exposure().total_exposure();
    state.unhedged_exposure = rm.exposure().unhedged_exposure();
    state.daily_loss = rm.exposure().daily_loss();
    state.unwind_rate = rm.exposure().unwind_rate_pct();
}

/// Draw the full dashboard.
pub fn draw(frame: &mut Frame, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),   // Status bar
            Constraint::Min(6),      // Open orders
            Constraint::Min(6),      // Positions
            Constraint::Length(3),   // P&L summary
            Constraint::Length(1),   // Key bindings
        ])
        .split(frame.area());

    // === Status Bar ===
    let uptime = state.started_at.elapsed().as_secs();
    let uptime_str = format!("{}h {}m", uptime / 3600, (uptime % 3600) / 60);

    let status_color = if state.engine_running { Color::Green } else { Color::Red };
    let mode_color = if state.mode == "PAPER" { Color::Yellow } else { Color::Red };

    let status = Paragraph::new(Line::from(vec![
        Span::raw(" Status: "),
        Span::styled(
            if state.engine_running { "RUNNING" } else { "PAUSED" },
            Style::default().fg(status_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  │  Mode: "),
        Span::styled(&state.mode, Style::default().fg(mode_color).add_modifier(Modifier::BOLD)),
        Span::raw(&format!("  │  Uptime: {}  │  Exposure: ${}", uptime_str, state.total_exposure)),
        Span::raw(&format!("  │  Pairs: {}", state.pair_count)),
    ]))
    .block(Block::default().borders(Borders::ALL).title(" Prediction Market Arbitrage "));
    frame.render_widget(status, chunks[0]);

    // === Open Orders Table ===
    let order_header = Row::new(vec!["Platform", "Market", "Side", "Price", "Qty", "Status", "Age"])
        .style(Style::default().add_modifier(Modifier::BOLD));

    let order_rows: Vec<Row> = state.open_orders.iter().map(|o| {
        let age = (Utc::now() - o.placed_at).num_seconds();
        Row::new(vec![
            Cell::from(o.platform.clone()),
            Cell::from(if o.market_id.len() > 20 { format!("{}...", &o.market_id[..20]) } else { o.market_id.clone() }),
            Cell::from(o.side.clone()),
            Cell::from(o.price.to_string()),
            Cell::from(o.quantity.to_string()),
            Cell::from(o.status.clone()),
            Cell::from(format!("{}s", age)),
        ])
    }).collect();

    let orders_table = Table::new(order_rows, [
        Constraint::Length(12), Constraint::Min(22), Constraint::Length(6),
        Constraint::Length(8), Constraint::Length(6), Constraint::Length(10), Constraint::Length(6),
    ])
    .header(order_header)
    .block(Block::default().borders(Borders::ALL).title(format!(" Open Orders ({}) ", state.open_orders.len())));
    frame.render_widget(orders_table, chunks[1]);

    // === Positions Table ===
    let pos_header = Row::new(vec!["Pair", "Poly", "Kalshi", "Hedged", "Profit", "Status"])
        .style(Style::default().add_modifier(Modifier::BOLD));

    let pos_rows: Vec<Row> = state.positions.iter().map(|p| {
        let pair_short = if p.pair_id.len() > 8 { &p.pair_id[..8] } else { &p.pair_id };
        Row::new(vec![
            Cell::from(pair_short.to_string()),
            Cell::from(format!("{} {} @{}", p.poly_quantity, p.poly_side, p.poly_avg_price)),
            Cell::from(format!("{} {} @{}", p.kalshi_quantity, p.kalshi_side, p.kalshi_avg_price)),
            Cell::from(p.hedged_quantity.to_string()),
            Cell::from(format!("${}", p.guaranteed_profit)),
            Cell::from(p.status.clone()),
        ])
    }).collect();

    let pos_table = Table::new(pos_rows, [
        Constraint::Length(10), Constraint::Min(16), Constraint::Min(16),
        Constraint::Length(8), Constraint::Length(10), Constraint::Length(10),
    ])
    .header(pos_header)
    .block(Block::default().borders(Borders::ALL).title(format!(" Positions ({}) ", state.positions.len())));
    frame.render_widget(pos_table, chunks[2]);

    // === P&L Summary ===
    let pnl = state.daily_pnl.as_ref();
    let net = pnl.map(|p| p.net_profit.to_string()).unwrap_or("0".into());
    let trades = pnl.map(|p| p.trades_executed.to_string()).unwrap_or("0".into());

    let pnl_text = Paragraph::new(Line::from(vec![
        Span::raw(&format!(" Today P&L: ${net}  │  Trades: {trades}  │  Daily Loss: ${}", state.daily_loss)),
        Span::raw(&format!("  │  Unhedged: ${}  │  Unwind Rate: {}%", state.unhedged_exposure, state.unwind_rate)),
    ]))
    .block(Block::default().borders(Borders::ALL).title(" P&L Summary "));
    frame.render_widget(pnl_text, chunks[3]);

    // === Key Bindings ===
    let keys = Paragraph::new(Line::from(vec![
        Span::styled(" q", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(":quit  "),
        Span::styled("p", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(":pause  "),
        Span::styled("r", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(":resume"),
    ]));
    frame.render_widget(keys, chunks[4]);
}

/// Run the TUI event loop. Returns when user presses 'q' or shutdown is triggered.
pub async fn run_tui(
    db: Arc<SqliteRepository>,
    risk_manager: Arc<RwLock<RiskManager>>,
    mode: &str,
    pair_count: usize,
) -> anyhow::Result<()> {
    // Init terminal
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;

    let mut state = TuiState::new(mode);
    state.pair_count = pair_count;

    let tick_rate = Duration::from_millis(250);
    let mut last_refresh = Instant::now();
    let refresh_interval = Duration::from_secs(2);

    let result = loop {
        // Refresh data every 2 seconds
        if last_refresh.elapsed() >= refresh_interval {
            refresh_state(&mut state, &db, &risk_manager).await;
            last_refresh = Instant::now();
        }

        // Draw
        terminal.draw(|frame| draw(frame, &state))?;

        // Handle input with timeout
        if crossterm::event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => break Ok(()),
                        KeyCode::Char('p') => {
                            state.engine_running = false;
                            // TODO: send pause signal to engine
                        }
                        KeyCode::Char('r') => {
                            state.engine_running = true;
                            // TODO: send resume signal to engine
                        }
                        _ => {}
                    }
                }
            }
        }
    };

    // Restore terminal (ALWAYS — even on error)
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;

    result
}
```

### Verification
```bash
cargo check -p arb-cli
cargo clippy -p arb-cli -- -D warnings
```

---

## Prompt 5-C: Startup Wiring + Shutdown + Health File

### File: `crates/arb-cli/src/main.rs` — rewrite the bottom half

Keep everything above the `#[tokio::main]` function. Replace `main()` with:

```rust
mod tui;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let args = Args::parse();

    let settings = config::Config::builder()
        .add_source(config::File::with_name("config/default"))
        .build()?;
    let app_config: AppConfig = settings.try_deserialize()?;

    init_tracing(&app_config.logging);

    // === Match-only mode (unchanged) ===
    if args.r#match {
        // ... existing match code stays the same ...
        return Ok(());
    }

    // === Startup banner ===
    let mode = if args.paper { "PAPER" } else { "LIVE" };
    info!("===========================================");
    info!("  Prediction Market Arbitrage System");
    info!("  Mode: {}", mode);
    if args.paper {
        info!("  🧪 PAPER TRADING — no real orders will be placed");
    } else {
        warn!("  💰 LIVE TRADING — real money at risk!");
    }
    info!("===========================================");

    // === Init DB ===
    let db_url = format!("sqlite://{}?mode=rwc", app_config.database.path);
    let db = Arc::new(arb_db::SqliteRepository::new(&db_url).await?);
    db.run_migrations().await?;
    info!("Database initialized");

    // === Init risk manager ===
    let rm = Arc::new(parking_lot::RwLock::new(arb_risk::RiskManager::new(app_config.risk)));
    rm.write().set_engine_running(true);

    // TODO: Init connectors (real or paper based on mode)
    // TODO: Init engine with Engine::new(...)
    // TODO: Load active pairs from DB
    // TODO: Register pairs in PriceCache
    // TODO: Start engine with Engine::run(price_rx, pairs)

    let pair_count = 0; // TODO: from active pairs

    // === Setup Ctrl+C handler ===
    let shutdown_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sf = shutdown_flag.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("failed to listen for ctrl+c");
        info!("Ctrl+C received — initiating shutdown");
        sf.store(true, std::sync::atomic::Ordering::SeqCst);
        // TODO: engine.shutdown().await when engine is wired
    });

    // === Health file writer ===
    let health_db = db.clone();
    let health_rm = rm.clone();
    let health_flag = shutdown_flag.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            if health_flag.load(std::sync::atomic::Ordering::SeqCst) { break; }
            write_health_file(&health_db, &health_rm, mode).await;
        }
    });

    // === TUI or headless ===
    if args.tui || (!args.headless && !args.paper) {
        // Set panic hook to restore terminal on panic
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
            original_hook(panic_info);
        }));

        tui::run_tui(db.clone(), rm.clone(), mode, pair_count).await?;
    } else {
        // Headless: just wait for Ctrl+C
        info!("Running headless — press Ctrl+C to stop");
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if shutdown_flag.load(std::sync::atomic::Ordering::SeqCst) { break; }
        }
    }

    info!("Shutdown complete");
    Ok(())
}

async fn write_health_file(
    db: &arb_db::SqliteRepository,
    rm: &parking_lot::RwLock<arb_risk::RiskManager>,
    mode: &str,
) {
    use std::io::Write;

    let exposure = rm.read().exposure().total_exposure();
    let daily_loss = rm.read().exposure().daily_loss();
    let open_orders = db.list_orders_by_status("open").await.map(|o| o.len()).unwrap_or(0);
    let positions = db.list_open_positions().await.map(|p| p.len()).unwrap_or(0);

    let health = serde_json::json!({
        "status": "running",
        "mode": mode,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "open_orders": open_orders,
        "open_positions": positions,
        "total_exposure": exposure.to_string(),
        "daily_loss": daily_loss.to_string(),
    });

    // Write atomically (write to tmp, rename)
    let path = std::path::Path::new("data/health.json");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp_path = path.with_extension("tmp");
    if let Ok(mut f) = std::fs::File::create(&tmp_path) {
        if f.write_all(serde_json::to_string_pretty(&health).unwrap_or_default().as_bytes()).is_ok() {
            let _ = std::fs::rename(&tmp_path, path);
        }
    }
}
```

**Add to `crates/arb-cli/Cargo.toml` `[dependencies]`:**
```toml
serde_json = { workspace = true }
parking_lot = { workspace = true }
rand = { workspace = true }
```

### Verification
```bash
cargo build --workspace
cargo run -- --help
cargo run -- --paper --headless  # should start, print banner, write health file, wait for Ctrl+C
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

---

## Phase 5 Acceptance Criteria

- [ ] `cargo test --workspace` passes (all previous + paper connector tests = 140+)
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] **Paper connector:**
  - [ ] Delegates ONLY market data methods to inner connector
  - [ ] place_limit_order never makes network calls (proven by DummyConnector panic test)
  - [ ] Simulates fills based on probability + delay
  - [ ] Tracks balance, deducts on place, refunds on cancel
  - [ ] Rejects orders when balance insufficient
- [ ] **TUI:**
  - [ ] Renders status bar, orders table, positions table, P&L summary, key bindings
  - [ ] Refreshes data every 2 seconds from DB + risk manager
  - [ ] 'q' quits cleanly, restoring terminal
  - [ ] Terminal restored on panic (panic hook)
  - [ ] Does not block engine (async with crossterm::poll timeout)
- [ ] **Startup:**
  - [ ] `--paper` shows paper trading banner
  - [ ] `--headless` runs without TUI
  - [ ] Ctrl+C initiates graceful shutdown
- [ ] **Health file:**
  - [ ] `data/health.json` written every 30s
  - [ ] Contains: status, mode, timestamp, open orders/positions, exposure, loss
  - [ ] Written atomically (tmp + rename)
- [ ] **Graceful shutdown:**
  - [ ] Ctrl+C caught → engine.shutdown() called → all open orders cancelled → exit

## Execution Order

```
5-A first  → Paper connector (standalone, no CLI changes needed)
5-B second → TUI module (new file, needs ratatui deps)
5-C last   → Startup wiring, shutdown, health file (ties everything together)
```
