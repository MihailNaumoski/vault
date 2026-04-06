use arb_db::models::{DailyPnlRow, OrderRow, PositionRow};
use arb_db::Repository;
use arb_db::SqliteRepository;
use arb_engine::price_cache::PriceCache;
use arb_engine::types::PairInfo;
use arb_risk::RiskManager;
use chrono::Utc;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use parking_lot::RwLock;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

const CYAN: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const POLY_COLOR: Color = Color::Magenta;
const KALSHI_COLOR: Color = Color::Blue;
const PROFIT_COLOR: Color = Color::Green;
const LOSS_COLOR: Color = Color::Red;
const HEADER_BG: Color = Color::DarkGray;

#[derive(Clone)]
pub struct PriceRow {
    pub name: String,
    pub pair_id: Uuid,
    pub poly_yes: Decimal,
    pub poly_no: Decimal,
    pub kalshi_yes: Decimal,
    pub kalshi_no: Decimal,
    pub prev_poly_yes: Decimal,
    pub spread: Decimal,
    pub last_update_secs: i64,
    pub volume: Decimal,
}

#[allow(dead_code)]
pub struct TuiState {
    pub mode: String,
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
    pub market_names: HashMap<String, String>,
    pub order_scroll: u16,
    pub pos_scroll: u16,
    pub prices: Vec<PriceRow>,
    pub pairs: Vec<PairInfo>,
    pub price_cache: Option<Arc<PriceCache>>,
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
            market_names: HashMap::new(),
            order_scroll: 0,
            pos_scroll: 0,
            prices: Vec::new(),
            pairs: Vec::new(),
            price_cache: None,
        }
    }
}

pub async fn refresh_state(
    state: &mut TuiState,
    db: &SqliteRepository,
    risk_manager: &RwLock<RiskManager>,
) {
    if let Ok(orders) = db.list_orders_by_status("open").await {
        state.open_orders = orders;
    }
    if let Ok(positions) = db.list_open_positions().await {
        state.positions = positions;
    }
    if let Ok(Some(pnl)) = db.get_daily_pnl(Utc::now().date_naive()).await {
        state.daily_pnl = Some(pnl);
    }

    if state.market_names.is_empty() {
        if let Ok(pairs) = db.list_active_market_pairs().await {
            for p in &pairs {
                let q = &p.poly_question;
                let short = if q.len() > 35 { format!("{}...", &q[..35]) } else { q.clone() };
                state.market_names.insert(p.poly_condition_id.clone(), short.clone());
                state.market_names.insert(p.kalshi_ticker.clone(), short);
            }
        }
    }

    let rm = risk_manager.read();
    state.total_exposure = rm.exposure().total_exposure();
    state.unhedged_exposure = rm.exposure().unhedged_exposure();
    state.daily_loss = rm.exposure().daily_loss();
    state.unwind_rate = rm.exposure().unwind_rate_pct();

    // Refresh live prices from cache
    if let Some(ref cache) = state.price_cache {
        let now = Utc::now();
        // Build a map of old prices for change detection
        let old: HashMap<Uuid, Decimal> = state.prices.iter()
            .map(|p| (p.pair_id, p.poly_yes))
            .collect();

        state.prices = state.pairs.iter().filter_map(|pair| {
            let pp = cache.get(&pair.pair_id)?;
            let q = state.market_names.get(&pair.poly_market_id)
                .cloned()
                .unwrap_or_else(|| {
                    if pair.poly_market_id.len() > 20 {
                        format!("{}...", &pair.poly_market_id[..20])
                    } else {
                        pair.poly_market_id.clone()
                    }
                });
            let prev = old.get(&pair.pair_id).copied().unwrap_or(pp.poly_yes);
            let spread = if pp.poly_yes > Decimal::ZERO && pp.kalshi_yes > Decimal::ZERO {
                pp.poly_yes - pp.kalshi_yes
            } else {
                Decimal::ZERO
            };
            let age = (now - pp.poly_updated).num_seconds();
            Some(PriceRow {
                name: q,
                pair_id: pair.pair_id,
                poly_yes: pp.poly_yes,
                poly_no: pp.poly_no,
                kalshi_yes: pp.kalshi_yes,
                kalshi_no: pp.kalshi_no,
                prev_poly_yes: prev,
                spread,
                last_update_secs: age,
                volume: pair.volume,
            })
        }).collect();
    }
}

fn pnl_color(val: Decimal) -> Color {
    if val > dec!(0) { PROFIT_COLOR } else if val < dec!(0) { LOSS_COLOR } else { Color::White }
}

fn platform_color(platform: &str) -> Color {
    match platform {
        "polymarket" => POLY_COLOR,
        "kalshi" => KALSHI_COLOR,
        _ => Color::White,
    }
}

fn format_age(secs: i64) -> String {
    if secs < 60 { format!("{secs}s") }
    else if secs < 3600 { format!("{}m", secs / 60) }
    else { format!("{}h{}m", secs / 3600, (secs % 3600) / 60) }
}

pub fn draw(frame: &mut Frame, state: &TuiState) {
    let price_rows = state.prices.len() as u16;
    let price_height = price_rows.max(1) + 3; // header + border + rows

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),            // Status
            Constraint::Length(price_height), // Live Prices
            Constraint::Percentage(30),      // Orders
            Constraint::Percentage(25),      // Positions
            Constraint::Length(3),            // P&L
            Constraint::Length(1),            // Keys
        ])
        .split(frame.area());

    draw_status(frame, state, chunks[0]);
    draw_prices(frame, state, chunks[1]);
    draw_orders(frame, state, chunks[2]);
    draw_positions(frame, state, chunks[3]);
    draw_pnl(frame, state, chunks[4]);
    draw_keys(frame, state, chunks[5]);
}

fn draw_prices(frame: &mut Frame, state: &TuiState, area: Rect) {
    let header = Row::new(vec![
        Cell::from("Market").style(Style::default().bold()),
        Cell::from("P.Yes").style(Style::default().bold()),
        Cell::from("P.No").style(Style::default().bold()),
        Cell::from("Chg").style(Style::default().bold()),
        Cell::from("K.Yes").style(Style::default().bold()),
        Cell::from("K.No").style(Style::default().bold()),
        Cell::from("Spread").style(Style::default().bold()),
        Cell::from("Vol 24h").style(Style::default().bold()),
        Cell::from("Upd").style(Style::default().bold()),
    ]).style(Style::default().bg(HEADER_BG));

    let rows: Vec<Row> = state.prices.iter().map(|p| {
        let change = if p.prev_poly_yes > Decimal::ZERO {
            ((p.poly_yes - p.prev_poly_yes) / p.prev_poly_yes) * dec!(100)
        } else {
            Decimal::ZERO
        };
        let change_str = if change == dec!(0) {
            " --".to_string()
        } else {
            format!("{:+.1}%", change)
        };
        let change_color = if change > dec!(0) { PROFIT_COLOR } else if change < dec!(0) { LOSS_COLOR } else { DIM };

        let spread_str = format!("{:.1}c", p.spread * dec!(100));
        let spread_color = if p.spread.abs() >= dec!(0.03) { PROFIT_COLOR } else { Color::Yellow };

        let age_str = if p.last_update_secs < 0 { "now".into() }
            else if p.last_update_secs < 60 { format!("{}s ago", p.last_update_secs) }
            else { format!("{}m ago", p.last_update_secs / 60) };
        let age_color = if p.last_update_secs > 30 { LOSS_COLOR } else { DIM };

        let vol_str = if p.volume >= dec!(1_000_000) {
            format!("${:.1}M", p.volume / dec!(1_000_000))
        } else if p.volume >= dec!(1_000) {
            format!("${:.0}K", p.volume / dec!(1_000))
        } else if p.volume > Decimal::ZERO {
            format!("${:.0}", p.volume)
        } else {
            "--".into()
        };

        Row::new(vec![
            Cell::from(p.name.clone()),
            Cell::from(format!("{:.2}", p.poly_yes)).style(Style::default().fg(POLY_COLOR).bold()),
            Cell::from(format!("{:.2}", p.poly_no)).style(Style::default().fg(POLY_COLOR)),
            Cell::from(change_str).style(Style::default().fg(change_color)),
            Cell::from(format!("{:.2}", p.kalshi_yes)).style(Style::default().fg(KALSHI_COLOR).bold()),
            Cell::from(format!("{:.2}", p.kalshi_no)).style(Style::default().fg(KALSHI_COLOR)),
            Cell::from(spread_str).style(Style::default().fg(spread_color)),
            Cell::from(vol_str).style(Style::default().fg(CYAN)),
            Cell::from(age_str).style(Style::default().fg(age_color)),
        ])
    }).collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            format!(" Live Prices ({}) ", state.prices.len()),
            Style::default().fg(Color::White).bold(),
        ));

    let table = Table::new(rows, [
        Constraint::Min(18),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(7),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(7),
        Constraint::Length(8),
        Constraint::Length(7),
    ])
    .header(header)
    .block(block);

    frame.render_widget(table, area);
}

fn draw_status(frame: &mut Frame, state: &TuiState, area: Rect) {
    let uptime = state.started_at.elapsed().as_secs();
    let uptime_str = if uptime < 3600 {
        format!("{}m {}s", uptime / 60, uptime % 60)
    } else {
        format!("{}h {}m", uptime / 3600, (uptime % 3600) / 60)
    };

    let total_profit: Decimal = state.positions.iter()
        .map(|p| p.guaranteed_profit)
        .sum();

    let status_icon = if state.engine_running { ">>>" } else { "||" };
    let status_color = if state.engine_running { Color::Green } else { Color::Yellow };
    let mode_color = if state.mode == "PAPER" { Color::Yellow } else { Color::Red };

    let line1 = Line::from(vec![
        Span::raw("  "),
        Span::styled(status_icon, Style::default().fg(status_color).bold()),
        Span::raw(" "),
        Span::styled(
            if state.engine_running { "RUNNING" } else { "PAUSED" },
            Style::default().fg(status_color).bold(),
        ),
        Span::styled("  |  ", Style::default().fg(DIM)),
        Span::raw("Mode: "),
        Span::styled(&state.mode, Style::default().fg(mode_color).bold()),
        Span::styled("  |  ", Style::default().fg(DIM)),
        Span::raw("Uptime: "),
        Span::styled(uptime_str, Style::default().fg(CYAN)),
        Span::styled("  |  ", Style::default().fg(DIM)),
        Span::raw("Pairs: "),
        Span::styled(state.pair_count.to_string(), Style::default().fg(CYAN).bold()),
    ]);

    let line2 = Line::from(vec![
        Span::raw("  Exposure: "),
        Span::styled(format!("${}", state.total_exposure), Style::default().fg(Color::Yellow).bold()),
        Span::styled("  |  ", Style::default().fg(DIM)),
        Span::raw("Orders: "),
        Span::styled(state.open_orders.len().to_string(), Style::default().fg(CYAN)),
        Span::styled("  |  ", Style::default().fg(DIM)),
        Span::raw("Positions: "),
        Span::styled(state.positions.len().to_string(), Style::default().fg(CYAN)),
        Span::styled("  |  ", Style::default().fg(DIM)),
        Span::raw("Total Profit: "),
        Span::styled(format!("${total_profit}"), Style::default().fg(pnl_color(total_profit)).bold()),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CYAN))
        .title(Span::styled(
            " Prediction Market Arbitrage ",
            Style::default().fg(Color::White).bold(),
        ));

    let text = Paragraph::new(vec![Line::default(), line1, line2])
        .block(block);
    frame.render_widget(text, area);
}

fn draw_orders(frame: &mut Frame, state: &TuiState, area: Rect) {
    let header = Row::new(vec![
        Cell::from("Platform").style(Style::default().bold()),
        Cell::from("Market").style(Style::default().bold()),
        Cell::from("Side").style(Style::default().bold()),
        Cell::from("Price").style(Style::default().bold()),
        Cell::from("Qty").style(Style::default().bold()),
        Cell::from("Age").style(Style::default().bold()),
    ]).style(Style::default().bg(HEADER_BG));

    let rows: Vec<Row> = state.open_orders.iter().map(|o| {
        let age = (Utc::now() - o.placed_at).num_seconds();
        let plat = if o.platform == "polymarket" { "POLY" } else { "KALSHI" };
        let market_name = state.market_names.get(&o.market_id).cloned()
            .unwrap_or_else(|| if o.market_id.len() > 25 { format!("{}...", &o.market_id[..25]) } else { o.market_id.clone() });

        let side_color = match o.side.as_str() {
            "yes" => Color::Green,
            "no" => Color::Red,
            _ => Color::White,
        };

        let price_str = if o.price == dec!(0) { "-".to_string() } else { format!("{:.2}", o.price) };

        Row::new(vec![
            Cell::from(plat).style(Style::default().fg(platform_color(&o.platform))),
            Cell::from(market_name),
            Cell::from(o.side.to_uppercase()).style(Style::default().fg(side_color).bold()),
            Cell::from(price_str).style(Style::default().fg(Color::Yellow)),
            Cell::from(o.quantity.to_string()),
            Cell::from(format_age(age)).style(Style::default().fg(DIM)),
        ])
    }).collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            format!(" Open Orders ({}) ", state.open_orders.len()),
            Style::default().fg(Color::White).bold(),
        ));

    let table = Table::new(rows, [
        Constraint::Length(8),
        Constraint::Min(30),
        Constraint::Length(5),
        Constraint::Length(8),
        Constraint::Length(5),
        Constraint::Length(6),
    ])
    .header(header)
    .block(block)
    .row_highlight_style(Style::default().bg(Color::DarkGray));

    frame.render_widget(table, area);
}

fn draw_positions(frame: &mut Frame, state: &TuiState, area: Rect) {
    let header = Row::new(vec![
        Cell::from("Market").style(Style::default().bold()),
        Cell::from("Poly Side").style(Style::default().bold()),
        Cell::from("Kalshi Side").style(Style::default().bold()),
        Cell::from("Qty").style(Style::default().bold()),
        Cell::from("Profit").style(Style::default().bold()),
        Cell::from("Status").style(Style::default().bold()),
    ]).style(Style::default().bg(HEADER_BG));

    let rows: Vec<Row> = state.positions.iter().map(|p| {
        let market_name = state.market_names.values().next()
            .cloned().unwrap_or_else(|| p.pair_id[..8].to_string());

        let profit_color = pnl_color(p.guaranteed_profit);
        let status_color = match p.status.as_str() {
            "open" => Color::Green,
            "settled" => Color::DarkGray,
            _ => Color::Yellow,
        };

        Row::new(vec![
            Cell::from(if market_name.len() > 25 { format!("{}...", &market_name[..25]) } else { market_name }),
            Cell::from(format!("{} {} @{:.2}", p.poly_quantity, p.poly_side.to_uppercase(), p.poly_avg_price))
                .style(Style::default().fg(POLY_COLOR)),
            Cell::from(format!("{} {} @{:.2}", p.kalshi_quantity, p.kalshi_side.to_uppercase(), p.kalshi_avg_price))
                .style(Style::default().fg(KALSHI_COLOR)),
            Cell::from(p.hedged_quantity.to_string()),
            Cell::from(format!("${:.2}", p.guaranteed_profit))
                .style(Style::default().fg(profit_color).bold()),
            Cell::from(p.status.to_uppercase()).style(Style::default().fg(status_color)),
        ])
    }).collect();

    let total_profit: Decimal = state.positions.iter().map(|p| p.guaranteed_profit).sum();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            format!(" Positions ({}) | Total: ${:.2} ", state.positions.len(), total_profit),
            Style::default().fg(pnl_color(total_profit)).bold(),
        ));

    let table = Table::new(rows, [
        Constraint::Min(20),
        Constraint::Length(16),
        Constraint::Length(16),
        Constraint::Length(5),
        Constraint::Length(10),
        Constraint::Length(8),
    ])
    .header(header)
    .block(block);

    frame.render_widget(table, area);
}

fn draw_pnl(frame: &mut Frame, state: &TuiState, area: Rect) {
    let pnl = state.daily_pnl.as_ref();
    let net: Decimal = pnl.map(|p| p.net_profit).unwrap_or(dec!(0));
    let trades = pnl.map(|p| p.trades_executed).unwrap_or(0);

    let line = Line::from(vec![
        Span::raw("  P&L: "),
        Span::styled(format!("${net}"), Style::default().fg(pnl_color(net)).bold()),
        Span::styled("  |  ", Style::default().fg(DIM)),
        Span::raw("Trades: "),
        Span::styled(trades.to_string(), Style::default().fg(CYAN)),
        Span::styled("  |  ", Style::default().fg(DIM)),
        Span::raw("Daily Loss: "),
        Span::styled(format!("${}", state.daily_loss), Style::default().fg(if state.daily_loss > dec!(0) { LOSS_COLOR } else { Color::White })),
        Span::styled("  |  ", Style::default().fg(DIM)),
        Span::raw("Unhedged: "),
        Span::styled(format!("${}", state.unhedged_exposure), Style::default().fg(if state.unhedged_exposure > dec!(0) { Color::Yellow } else { Color::White })),
        Span::styled("  |  ", Style::default().fg(DIM)),
        Span::raw("Unwind: "),
        Span::styled(format!("{}%", state.unwind_rate), Style::default().fg(Color::White)),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(" Daily Summary ", Style::default().fg(Color::White).bold()));

    frame.render_widget(Paragraph::new(line).block(block), area);
}

fn draw_keys(frame: &mut Frame, _state: &TuiState, area: Rect) {
    let keys = Line::from(vec![
        Span::styled(" q", Style::default().fg(CYAN).bold()),
        Span::styled(" quit  ", Style::default().fg(DIM)),
        Span::styled("p", Style::default().fg(CYAN).bold()),
        Span::styled(" pause  ", Style::default().fg(DIM)),
        Span::styled("r", Style::default().fg(CYAN).bold()),
        Span::styled(" resume  ", Style::default().fg(DIM)),
        Span::styled("j/k", Style::default().fg(CYAN).bold()),
        Span::styled(" scroll", Style::default().fg(DIM)),
    ]);
    frame.render_widget(Paragraph::new(keys), area);
}

pub async fn run_tui(
    db: Arc<SqliteRepository>,
    risk_manager: Arc<RwLock<RiskManager>>,
    mode: &str,
    pair_count: usize,
    price_cache: Arc<PriceCache>,
    pairs: Vec<PairInfo>,
) -> anyhow::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;

    let mut state = TuiState::new(mode);
    state.pair_count = pair_count;
    state.price_cache = Some(price_cache);
    state.pairs = pairs;

    let tick_rate = Duration::from_millis(250);
    let mut last_refresh = Instant::now();
    let refresh_interval = Duration::from_secs(2);

    let result = loop {
        if last_refresh.elapsed() >= refresh_interval {
            refresh_state(&mut state, &db, &risk_manager).await;
            last_refresh = Instant::now();
        }

        terminal.draw(|frame| draw(frame, &state))?;

        if crossterm::event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => break Ok(()),
                        KeyCode::Char('p') => state.engine_running = false,
                        KeyCode::Char('r') => state.engine_running = true,
                        _ => {}
                    }
                }
            }
        }
    };

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;

    result
}
