use arb_db::models::{DailyPnlRow, OrderRow, PositionRow};
use arb_db::Repository;
use arb_db::SqliteRepository;
use arb_engine::price_cache::{PriceCache, PricePair};
use arb_engine::types::PairInfo;
use arb_risk::RiskManager;
use chrono::Utc;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use parking_lot::RwLock;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Rectangle as CanvasRect};
use ratatui::widgets::{
    Axis, Block, Borders, Cell, Chart, Dataset, Gauge, GraphType, Paragraph, Row, Table,
};
use ratatui::Frame;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
//  Cyberpunk / Tailwind-inspired palette
// ═══════════════════════════════════════════════════════════════════════════

const POLY_COLOR: Color = Color::Rgb(34, 211, 238); // Cyan-400
const KALSHI_COLOR: Color = Color::Rgb(29, 78, 216); // Blue-700
const PROFIT_GREEN: Color = Color::Rgb(52, 211, 153); // Emerald-400
const LOSS_RED: Color = Color::Rgb(236, 72, 153); // Pink-500
const AMBER: Color = Color::Rgb(245, 158, 11); // Amber-500
const NEUTRAL_YELLOW: Color = Color::Rgb(250, 204, 21); // Yellow-400
const ACCENT_VIOLET: Color = Color::Rgb(139, 92, 246); // Violet-500
const GLOW_POLY: Color = Color::Rgb(21, 94, 117); // Cyan-800
const GLOW_KALSHI: Color = Color::Rgb(30, 58, 138); // Blue-900
const HEADER_BG: Color = Color::Rgb(32, 32, 32);
const DIM: Color = Color::Rgb(124, 124, 124);
const BORDER_DIM: Color = Color::Rgb(61, 61, 61);
const ROW_ALT_BG: Color = Color::Rgb(20, 20, 20);
const GAUGE_BG: Color = Color::Rgb(40, 40, 40);
const THRESHOLD_DIM: Color = Color::Rgb(60, 60, 60);

// ═══════════════════════════════════════════════════════════════════════════
//  Data structures
// ═══════════════════════════════════════════════════════════════════════════

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
    #[allow(dead_code)]
    pub volume: Decimal,
}

const CHART_MAX_TICKS: usize = 120;

pub struct PairChartData {
    pub poly_yes: VecDeque<f64>,
    pub kalshi_yes: VecDeque<f64>,
}

impl PairChartData {
    fn new() -> Self {
        Self {
            poly_yes: VecDeque::new(),
            kalshi_yes: VecDeque::new(),
        }
    }

    fn push(&mut self, poly: f64, kalshi: f64) {
        self.poly_yes.push_back(poly);
        self.kalshi_yes.push_back(kalshi);
        while self.poly_yes.len() > CHART_MAX_TICKS {
            self.poly_yes.pop_front();
        }
        while self.kalshi_yes.len() > CHART_MAX_TICKS {
            self.kalshi_yes.pop_front();
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum ChartView {
    PriceOverlay,
    SpreadHistory,
    Candlestick,
    EquityCurve,
}

impl ChartView {
    fn next(self) -> Self {
        match self {
            Self::PriceOverlay => Self::SpreadHistory,
            Self::SpreadHistory => Self::Candlestick,
            Self::Candlestick => Self::EquityCurve,
            Self::EquityCurve => Self::PriceOverlay,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::PriceOverlay => "Price",
            Self::SpreadHistory => "Spread",
            Self::Candlestick => "OHLC",
            Self::EquityCurve => "Equity",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum TabView {
    Orders,
    Positions,
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct OhlcBar {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub kalshi_close: f64,
    pub tick_start: usize,
}

pub struct RiskLimits {
    pub max_total_exposure: f64,
    pub max_daily_loss: f64,
    pub max_unhedged_exposure: f64,
    pub max_unwind_rate_pct: f64,
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_total_exposure: 10_000.0,
            max_daily_loss: 200.0,
            max_unhedged_exposure: 500.0,
            max_unwind_rate_pct: 20.0,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  TUI State
// ═══════════════════════════════════════════════════════════════════════════

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
    pub pair_charts: HashMap<Uuid, PairChartData>,
    pub selected_chart_pair: usize,
    pub spread_history: HashMap<Uuid, VecDeque<f64>>,
    pub ohlc_bars: HashMap<Uuid, Vec<OhlcBar>>,
    pub equity_curve: VecDeque<(f64, f64)>,
    pub chart_view: ChartView,
    pub tab_view: TabView,
    pub risk_limits: RiskLimits,
    pub tick_count: HashMap<Uuid, usize>,
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
            pair_charts: HashMap::new(),
            selected_chart_pair: 0,
            spread_history: HashMap::new(),
            ohlc_bars: HashMap::new(),
            equity_curve: VecDeque::new(),
            chart_view: ChartView::PriceOverlay,
            tab_view: TabView::Orders,
            risk_limits: RiskLimits::default(),
            tick_count: HashMap::new(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  State refresh
// ═══════════════════════════════════════════════════════════════════════════

pub async fn refresh_state(
    state: &mut TuiState,
    db: &SqliteRepository,
    risk_manager: &RwLock<RiskManager>,
) {
    // Orders
    let mut all_active = Vec::new();
    for status in &["open", "pending", "partial_fill"] {
        if let Ok(mut orders) = db.list_orders_by_status(status).await {
            all_active.append(&mut orders);
        }
    }
    state.open_orders = all_active;
    if state.order_scroll as usize >= state.open_orders.len() {
        state.order_scroll = state.open_orders.len().saturating_sub(1) as u16;
    }
    state.open_orders.sort_by(|a, b| {
        a.platform
            .cmp(&b.platform)
            .then(a.market_id.cmp(&b.market_id))
    });

    // Positions
    if let Ok(positions) = db.list_open_positions().await {
        state.positions = positions;
    }
    if let Ok(Some(pnl)) = db.get_daily_pnl(Utc::now().date_naive()).await {
        state.daily_pnl = Some(pnl);
    }

    // Market names
    if state.market_names.is_empty() {
        if let Ok(pairs) = db.list_active_market_pairs().await {
            for p in &pairs {
                let q = &p.poly_question;
                let short = if q.len() > 35 {
                    format!("{}...", &q[..35])
                } else {
                    q.clone()
                };
                state
                    .market_names
                    .insert(p.poly_condition_id.clone(), short.clone());
                state
                    .market_names
                    .insert(p.kalshi_ticker.clone(), short.clone());
                state.market_names.insert(p.id.clone(), short.clone());
                state
                    .market_names
                    .insert(p.poly_yes_token_id.clone(), short.clone());
                state
                    .market_names
                    .insert(p.poly_no_token_id.clone(), short);
            }
        }
    }

    // Risk metrics
    let rm = risk_manager.read();
    state.total_exposure = rm.exposure().total_exposure();
    state.unhedged_exposure = rm.exposure().unhedged_exposure();
    state.daily_loss = rm.exposure().daily_loss();
    state.unwind_rate = rm.exposure().unwind_rate_pct();

    // Live prices from cache
    if let Some(ref cache) = state.price_cache {
        let now = Utc::now();
        let old: HashMap<Uuid, Decimal> = state
            .prices
            .iter()
            .map(|p| (p.pair_id, p.poly_yes))
            .collect();

        state.prices = state
            .pairs
            .iter()
            .map(|pair| {
                let pp = cache.get(&pair.pair_id).unwrap_or_else(|| PricePair {
                    poly_yes: Decimal::ZERO,
                    poly_no: Decimal::ZERO,
                    kalshi_yes: Decimal::ZERO,
                    kalshi_no: Decimal::ZERO,
                    poly_updated: Utc::now(),
                    kalshi_updated: Utc::now(),
                });
                let q = state
                    .market_names
                    .get(&pair.poly_market_id)
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
                PriceRow {
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
                }
            })
            .collect();
    }

    // Update chart history buffers
    for p in &state.prices {
        let poly = p.poly_yes.to_f64().unwrap_or(0.0);
        let kalshi = p.kalshi_yes.to_f64().unwrap_or(0.0);
        state
            .pair_charts
            .entry(p.pair_id)
            .or_insert_with(PairChartData::new)
            .push(poly, kalshi);
    }

    // ──── Spread history ────
    for p in &state.prices {
        let spread = (p.poly_yes - p.kalshi_yes).to_f64().unwrap_or(0.0);
        let history = state.spread_history.entry(p.pair_id).or_default();
        history.push_back(spread);
        while history.len() > CHART_MAX_TICKS {
            history.pop_front();
        }
    }

    // ──── OHLC aggregation (10-tick) ────
    for p in &state.prices {
        let poly = p.poly_yes.to_f64().unwrap_or(0.0);
        if poly <= 0.0 {
            continue;
        }
        let count = state.tick_count.entry(p.pair_id).or_insert(0);
        *count += 1;
        if (*count).is_multiple_of(10) {
            if let Some(chart) = state.pair_charts.get(&p.pair_id) {
                let len = chart.poly_yes.len();
                let start = len.saturating_sub(10);
                let poly_slice: Vec<f64> =
                    chart.poly_yes.iter().skip(start).copied().collect();
                let kalshi_last = chart.kalshi_yes.iter().last().copied().unwrap_or(0.0);
                if !poly_slice.is_empty() {
                    let bar = OhlcBar {
                        open: poly_slice[0],
                        high: poly_slice.iter().cloned().fold(f64::MIN, f64::max),
                        low: poly_slice.iter().cloned().fold(f64::MAX, f64::min),
                        close: *poly_slice.last().unwrap(),
                        kalshi_close: kalshi_last,
                        tick_start: *count - 10,
                    };
                    let bars = state.ohlc_bars.entry(p.pair_id).or_default();
                    bars.push(bar);
                    if bars.len() > 24 {
                        bars.remove(0);
                    }
                }
            }
        }
    }

    // ──── Equity curve ────
    let cumulative_pnl: f64 = state
        .positions
        .iter()
        .map(|p| p.guaranteed_profit.to_f64().unwrap_or(0.0))
        .sum();
    let tick_idx = state.equity_curve.len() as f64;
    state.equity_curve.push_back((tick_idx, cumulative_pnl));
    while state.equity_curve.len() > 500 {
        state.equity_curve.pop_front();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn pnl_color(val: Decimal) -> Color {
    if val > dec!(0) {
        PROFIT_GREEN
    } else if val < dec!(0) {
        LOSS_RED
    } else {
        NEUTRAL_YELLOW
    }
}

fn spread_color(spread: Decimal) -> Color {
    if spread.abs() >= dec!(0.03) {
        PROFIT_GREEN
    } else if spread.abs() >= dec!(0.01) {
        NEUTRAL_YELLOW
    } else {
        DIM
    }
}

fn freshness_color(secs: i64) -> Color {
    if secs <= 10 {
        PROFIT_GREEN
    } else if secs <= 30 {
        NEUTRAL_YELLOW
    } else {
        LOSS_RED
    }
}

fn platform_color(platform: &str) -> Color {
    match platform {
        "polymarket" => POLY_COLOR,
        "kalshi" => KALSHI_COLOR,
        _ => Color::White,
    }
}

fn format_age(secs: i64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}

const BLOCKS: &[char] = &['\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}'];

fn sparkline_str(data: &VecDeque<f64>) -> String {
    if data.is_empty() {
        return "--------".to_string();
    }
    let n = data.len().min(8);
    let slice: Vec<f64> = data.iter().rev().take(n).copied().collect();
    let min = slice.iter().cloned().fold(f64::MAX, f64::min);
    let max = slice.iter().cloned().fold(f64::MIN, f64::max);
    let range = if (max - min).abs() < 1e-9 { 1.0 } else { max - min };
    slice
        .iter()
        .rev()
        .map(|&v| {
            let idx = (((v - min) / range) * 7.0) as usize;
            BLOCKS[idx.min(7)]
        })
        .collect()
}

fn trend_color(data: &VecDeque<f64>) -> Color {
    if data.len() < 2 {
        return DIM;
    }
    let first = *data.front().unwrap();
    let last = *data.back().unwrap();
    if last > first + 0.001 {
        PROFIT_GREEN
    } else if last < first - 0.001 {
        LOSS_RED
    } else {
        DIM
    }
}

fn gauge_color(ratio: f64) -> Color {
    if ratio < 0.6 {
        PROFIT_GREEN
    } else if ratio < 0.8 {
        NEUTRAL_YELLOW
    } else {
        LOSS_RED
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Drawing — main layout
// ═══════════════════════════════════════════════════════════════════════════

pub fn draw(frame: &mut Frame, state: &TuiState) {
    let area = frame.area();
    let show_chart = area.height >= 24;
    let show_gauges = area.width >= 80;

    let price_rows = state.prices.len() as u16;
    let price_height = (price_rows.max(1) + 3).max(4);

    if show_chart {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),       // Status
                Constraint::Percentage(45),  // Chart + Gauges
                Constraint::Length(price_height), // Live Prices
                Constraint::Percentage(25),  // Tabbed Orders/Positions
                Constraint::Length(2),       // Footer
            ])
            .split(area);

        draw_status(frame, state, chunks[0]);

        if show_gauges {
            let h_split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[1]);
            draw_chart_area(frame, state, h_split[0]);
            draw_risk_gauges(frame, state, h_split[1]);
        } else {
            draw_chart_area(frame, state, chunks[1]);
        }

        draw_prices(frame, state, chunks[2]);
        draw_tabbed_panel(frame, state, chunks[3]);
        draw_footer(frame, state, chunks[4]);
    } else {
        // Compact: hide chart area, show only status + prices + tabbed + footer
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(price_height),
                Constraint::Min(6),
                Constraint::Length(2),
            ])
            .split(area);

        draw_status(frame, state, chunks[0]);
        draw_prices(frame, state, chunks[1]);
        draw_tabbed_panel(frame, state, chunks[2]);
        draw_footer(frame, state, chunks[3]);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Drawing — status bar (3 rows — 1 inner line with borders)
// ═══════════════════════════════════════════════════════════════════════════

fn draw_status(frame: &mut Frame, state: &TuiState, area: Rect) {
    let uptime = state.started_at.elapsed().as_secs();
    let uptime_str = if uptime < 3600 {
        format!("{}m{}s", uptime / 60, uptime % 60)
    } else {
        format!("{}h{}m", uptime / 3600, (uptime % 3600) / 60)
    };

    let total_profit: Decimal = state.positions.iter().map(|p| p.guaranteed_profit).sum();

    let status_icon = if state.engine_running { ">>>" } else { "||" };
    let status_color = if state.engine_running {
        PROFIT_GREEN
    } else {
        NEUTRAL_YELLOW
    };
    let mode_color = if state.mode == "PAPER" {
        NEUTRAL_YELLOW
    } else {
        LOSS_RED
    };

    let any_stale = state.prices.iter().any(|p| p.last_update_secs > 30);
    let flash = state.started_at.elapsed().as_secs().is_multiple_of(2);

    let mut spans = vec![
        Span::raw(" "),
        Span::styled(status_icon, Style::default().fg(status_color).bold()),
        Span::raw(" "),
        Span::styled(
            if state.engine_running {
                "RUNNING"
            } else {
                "PAUSED"
            },
            Style::default().fg(status_color).bold(),
        ),
        Span::styled(" \u{2502} ", Style::default().fg(DIM)),
        Span::styled(state.mode.as_str(), Style::default().fg(mode_color).bold()),
        Span::styled(" \u{2502} ", Style::default().fg(DIM)),
        Span::styled(&uptime_str, Style::default().fg(POLY_COLOR)),
        Span::styled(" \u{2502} ", Style::default().fg(DIM)),
        Span::styled(
            format!("{} pairs", state.pair_count),
            Style::default().fg(POLY_COLOR),
        ),
        Span::styled(" \u{2502} ", Style::default().fg(DIM)),
        Span::raw("Exp:"),
        Span::styled(
            format!("${:.0}", state.total_exposure),
            Style::default().fg(NEUTRAL_YELLOW).bold(),
        ),
        Span::styled(" \u{2502} ", Style::default().fg(DIM)),
        Span::raw("P&L:"),
        Span::styled(
            format!("${:.2}", total_profit),
            Style::default().fg(pnl_color(total_profit)).bold(),
        ),
        Span::styled(" \u{2502} ", Style::default().fg(DIM)),
        Span::styled(
            format!("O:{} P:{}", state.open_orders.len(), state.positions.len()),
            Style::default().fg(POLY_COLOR),
        ),
    ];

    if any_stale && flash {
        spans.push(Span::styled(" \u{2502} ", Style::default().fg(DIM)));
        spans.push(Span::styled(
            "STALE",
            Style::default().fg(LOSS_RED).bold(),
        ));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_DIM))
        .title(Span::styled(
            " Prediction Market Arbitrage ",
            Style::default().fg(AMBER).bold(),
        ));

    frame.render_widget(Paragraph::new(Line::from(spans)).block(block), area);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Drawing — chart area dispatcher
// ═══════════════════════════════════════════════════════════════════════════

fn draw_chart_area(frame: &mut Frame, state: &TuiState, area: Rect) {
    if state.pair_count == 0 && state.chart_view != ChartView::EquityCurve {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_DIM))
            .title(Span::styled(
                format!(" {} ", state.chart_view.label()),
                Style::default().fg(AMBER).bold(),
            ));
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No market pairs configured",
                Style::default().fg(AMBER),
            )))
            .alignment(Alignment::Center)
            .block(block),
            area,
        );
        return;
    }

    match state.chart_view {
        ChartView::PriceOverlay => draw_price_overlay(frame, state, area),
        ChartView::SpreadHistory => draw_spread_history(frame, state, area),
        ChartView::Candlestick => draw_candlestick(frame, state, area),
        ChartView::EquityCurve => draw_equity_curve(frame, state, area),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  View 1 — Price Overlay (POLY vs KALSHI with glow)
// ═══════════════════════════════════════════════════════════════════════════

fn draw_price_overlay(frame: &mut Frame, state: &TuiState, area: Rect) {
    let pair_count = state.prices.len();
    let idx = if pair_count > 0 {
        state.selected_chart_pair % pair_count
    } else {
        0
    };

    let (market_name, poly_data, kalshi_data) = if let Some(price_row) = state.prices.get(idx) {
        let name = price_row.name.clone();
        if let Some(chart) = state.pair_charts.get(&price_row.pair_id) {
            let poly: Vec<(f64, f64)> = chart
                .poly_yes
                .iter()
                .enumerate()
                .map(|(i, &v)| (i as f64, v))
                .collect();
            let kalshi: Vec<(f64, f64)> = chart
                .kalshi_yes
                .iter()
                .enumerate()
                .map(|(i, &v)| (i as f64, v))
                .collect();
            (name, poly, kalshi)
        } else {
            (name, Vec::new(), Vec::new())
        }
    } else {
        ("No data".to_string(), Vec::new(), Vec::new())
    };

    // Glow datasets (shifted down 0.002)
    let poly_glow: Vec<(f64, f64)> = poly_data.iter().map(|&(x, y)| (x, y - 0.002)).collect();
    let kalshi_glow: Vec<(f64, f64)> =
        kalshi_data.iter().map(|&(x, y)| (x, y - 0.002)).collect();

    // Y-axis bounds
    let all_vals: Vec<f64> = poly_data
        .iter()
        .map(|d| d.1)
        .chain(kalshi_data.iter().map(|d| d.1))
        .filter(|v| *v > 0.0)
        .collect();
    let y_min = all_vals
        .iter()
        .cloned()
        .fold(f64::MAX, f64::min)
        .max(0.0);
    let y_max = all_vals
        .iter()
        .cloned()
        .fold(f64::MIN, f64::max)
        .min(1.0);
    let y_pad = ((y_max - y_min) * 0.1).max(0.02);
    let y_lo = (y_min - y_pad).max(0.0);
    let y_hi = (y_max + y_pad).min(1.0);
    let x_max = poly_data.len().max(kalshi_data.len()).max(1) as f64;

    // Spread threshold zone
    let mid = if !all_vals.is_empty() {
        (y_min + y_max) / 2.0
    } else {
        0.5
    };
    let threshold_upper: Vec<(f64, f64)> = (0..x_max as usize)
        .map(|i| (i as f64, mid + 0.01))
        .collect();
    let threshold_lower: Vec<(f64, f64)> = (0..x_max as usize)
        .map(|i| (i as f64, mid - 0.01))
        .collect();

    let pos_count = state.positions.len();
    let total_profit: Decimal = state.positions.iter().map(|p| p.guaranteed_profit).sum();
    let title = format!(
        " {} [{}/{}] | Pos: {} P&L: ${:.2} -- POLY vs KALSHI ",
        market_name,
        idx + 1,
        pair_count.max(1),
        pos_count,
        total_profit,
    );

    let datasets = vec![
        Dataset::default()
            .name("POLY")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(POLY_COLOR))
            .data(&poly_data),
        Dataset::default()
            .name("")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(GLOW_POLY))
            .data(&poly_glow),
        Dataset::default()
            .name("KALSHI")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(KALSHI_COLOR))
            .data(&kalshi_data),
        Dataset::default()
            .name("")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(GLOW_KALSHI))
            .data(&kalshi_glow),
        Dataset::default()
            .name("")
            .marker(Marker::Dot)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(THRESHOLD_DIM))
            .data(&threshold_upper),
        Dataset::default()
            .name("")
            .marker(Marker::Dot)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(THRESHOLD_DIM))
            .data(&threshold_lower),
    ];

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_DIM))
                .title(Span::styled(title, Style::default().fg(AMBER).bold())),
        )
        .x_axis(
            Axis::default()
                .style(Style::default().fg(DIM))
                .bounds([0.0, x_max]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(DIM))
                .labels(vec![
                    Span::styled(format!("{:.2}", y_lo), Style::default().fg(DIM)),
                    Span::styled(
                        format!("{:.2}", (y_lo + y_hi) / 2.0),
                        Style::default().fg(DIM),
                    ),
                    Span::styled(format!("{:.2}", y_hi), Style::default().fg(DIM)),
                ])
                .bounds([y_lo, y_hi]),
        );

    frame.render_widget(chart, area);
}

// ═══════════════════════════════════════════════════════════════════════════
//  View 2 — Spread History
// ═══════════════════════════════════════════════════════════════════════════

fn draw_spread_history(frame: &mut Frame, state: &TuiState, area: Rect) {
    let pair_count = state.prices.len();
    let idx = if pair_count > 0 {
        state.selected_chart_pair % pair_count
    } else {
        0
    };

    let (market_name, spread_data, current_spread) =
        if let Some(price_row) = state.prices.get(idx) {
            let name = price_row.name.clone();
            let data = state
                .spread_history
                .get(&price_row.pair_id)
                .cloned()
                .unwrap_or_default();
            let current = data.back().copied().unwrap_or(0.0);
            (name, data, current)
        } else {
            ("No data".to_string(), VecDeque::new(), 0.0)
        };

    // Split into positive / negative datasets
    let positive: Vec<(f64, f64)> = spread_data
        .iter()
        .enumerate()
        .map(|(i, &v)| (i as f64, v.max(0.0)))
        .collect();
    let negative: Vec<(f64, f64)> = spread_data
        .iter()
        .enumerate()
        .map(|(i, &v)| (i as f64, v.min(0.0)))
        .collect();

    let x_max = spread_data.len().max(1) as f64;
    let y_min = spread_data.iter().cloned().fold(0.0_f64, f64::min);
    let y_max = spread_data.iter().cloned().fold(0.0_f64, f64::max);
    let y_pad = ((y_max - y_min).abs() * 0.15).max(0.01);
    let y_lo = y_min - y_pad;
    let y_hi = y_max + y_pad;

    let zero_line: Vec<(f64, f64)> = (0..x_max as usize).map(|i| (i as f64, 0.0)).collect();
    let thresh_upper: Vec<(f64, f64)> =
        (0..x_max as usize).map(|i| (i as f64, 0.02)).collect();
    let thresh_lower: Vec<(f64, f64)> =
        (0..x_max as usize).map(|i| (i as f64, -0.02)).collect();

    let title = format!(
        " Spread: {} [{}/{}] | Current: {:.1}c ",
        market_name,
        idx + 1,
        pair_count.max(1),
        current_spread * 100.0,
    );

    let datasets = vec![
        Dataset::default()
            .name("+spread")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(PROFIT_GREEN))
            .data(&positive),
        Dataset::default()
            .name("-spread")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(LOSS_RED))
            .data(&negative),
        Dataset::default()
            .name("")
            .marker(Marker::Dot)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(DIM))
            .data(&zero_line),
        Dataset::default()
            .name("")
            .marker(Marker::Dot)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(AMBER))
            .data(&thresh_upper),
        Dataset::default()
            .name("")
            .marker(Marker::Dot)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(AMBER))
            .data(&thresh_lower),
    ];

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_DIM))
                .title(Span::styled(title, Style::default().fg(AMBER).bold())),
        )
        .x_axis(
            Axis::default()
                .style(Style::default().fg(DIM))
                .bounds([0.0, x_max]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(DIM))
                .labels(vec![
                    Span::styled(format!("{:.3}", y_lo), Style::default().fg(DIM)),
                    Span::styled("0.000", Style::default().fg(DIM)),
                    Span::styled(format!("{:.3}", y_hi), Style::default().fg(DIM)),
                ])
                .bounds([y_lo, y_hi]),
        );

    frame.render_widget(chart, area);
}

// ═══════════════════════════════════════════════════════════════════════════
//  View 3 — Candlestick (Canvas-based OHLC)
// ═══════════════════════════════════════════════════════════════════════════

fn draw_candlestick(frame: &mut Frame, state: &TuiState, area: Rect) {
    let pair_count = state.prices.len();
    let idx = if pair_count > 0 {
        state.selected_chart_pair % pair_count
    } else {
        0
    };

    let empty_bars = Vec::new();
    let (market_name, bars) = if let Some(price_row) = state.prices.get(idx) {
        let name = price_row.name.clone();
        let b = state
            .ohlc_bars
            .get(&price_row.pair_id)
            .unwrap_or(&empty_bars);
        (name, b)
    } else {
        ("No data".to_string(), &empty_bars as &Vec<OhlcBar>)
    };

    let kalshi_points: Vec<(f64, f64)> = bars
        .iter()
        .enumerate()
        .map(|(i, bar)| (i as f64 + 0.5, bar.kalshi_close))
        .collect();

    let title = format!(
        " Candles: {} [{}/{}] | OHLC(10-tick) ",
        market_name,
        idx + 1,
        pair_count.max(1),
    );

    if bars.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_DIM))
            .title(Span::styled(title, Style::default().fg(AMBER).bold()));
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Collecting data...",
                Style::default().fg(DIM),
            )))
            .alignment(Alignment::Center)
            .block(block),
            area,
        );
        return;
    }

    let y_min = bars
        .iter()
        .map(|b| b.low)
        .fold(f64::MAX, f64::min);
    let y_max = bars
        .iter()
        .map(|b| b.high)
        .fold(f64::MIN, f64::max);
    let y_pad = ((y_max - y_min) * 0.1).max(0.005);
    let y_lo = y_min - y_pad;
    let y_hi = y_max + y_pad;
    let x_max = bars.len() as f64 + 1.0;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_DIM))
        .title(Span::styled(title, Style::default().fg(AMBER).bold()));

    let canvas = Canvas::default()
        .block(block)
        .x_bounds([0.0, x_max])
        .y_bounds([y_lo, y_hi])
        .marker(Marker::HalfBlock)
        .paint(|ctx| {
            // Draw candles
            for (i, bar) in bars.iter().enumerate() {
                let x = i as f64 + 0.5;
                let color = if bar.close >= bar.open {
                    PROFIT_GREEN
                } else {
                    LOSS_RED
                };

                // Wick
                ctx.draw(&CanvasLine {
                    x1: x,
                    y1: bar.low,
                    x2: x,
                    y2: bar.high,
                    color,
                });

                // Body
                let body_lo = bar.open.min(bar.close);
                let body_hi = bar.open.max(bar.close);
                ctx.draw(&CanvasRect {
                    x: x - 0.3,
                    y: body_lo,
                    width: 0.6,
                    height: (body_hi - body_lo).max(0.001),
                    color,
                });
            }

            // Kalshi overlay line
            for pair in kalshi_points.windows(2) {
                ctx.draw(&CanvasLine {
                    x1: pair[0].0,
                    y1: pair[0].1,
                    x2: pair[1].0,
                    y2: pair[1].1,
                    color: KALSHI_COLOR,
                });
            }
        });

    frame.render_widget(canvas, area);
}

// ═══════════════════════════════════════════════════════════════════════════
//  View 4 — P&L Equity Curve
// ═══════════════════════════════════════════════════════════════════════════

fn draw_equity_curve(frame: &mut Frame, state: &TuiState, area: Rect) {
    let curve: Vec<(f64, f64)> = state
        .equity_curve
        .iter()
        .enumerate()
        .map(|(i, &(_, pnl))| (i as f64, pnl))
        .collect();

    if curve.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_DIM))
            .title(Span::styled(
                " Equity Curve ",
                Style::default().fg(AMBER).bold(),
            ));
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Collecting data...",
                Style::default().fg(DIM),
            )))
            .alignment(Alignment::Center)
            .block(block),
            area,
        );
        return;
    }

    let current = curve.last().map(|&(_, y)| y).unwrap_or(0.0);
    let peak = curve.iter().map(|&(_, y)| y).fold(0.0_f64, f64::max);
    let drawdown = (peak - current).max(0.0);

    let above: Vec<(f64, f64)> = curve.iter().map(|&(x, y)| (x, y.max(0.0))).collect();
    let below: Vec<(f64, f64)> = curve.iter().map(|&(x, y)| (x, y.min(0.0))).collect();

    let x_max = curve.len().max(1) as f64;
    let y_min = curve.iter().map(|&(_, y)| y).fold(0.0_f64, f64::min);
    let y_max_val = curve.iter().map(|&(_, y)| y).fold(0.0_f64, f64::max);
    let y_pad = ((y_max_val - y_min).abs() * 0.15).max(0.5);
    let y_lo = y_min - y_pad;
    let y_hi = y_max_val + y_pad;

    let zero_line: Vec<(f64, f64)> = (0..x_max as usize).map(|i| (i as f64, 0.0)).collect();

    let title = format!(
        " Equity Curve | Net: ${:.2} | Peak: ${:.2} | Drawdown: ${:.2} ",
        current, peak, drawdown,
    );

    let datasets = vec![
        Dataset::default()
            .name("P&L+")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(PROFIT_GREEN))
            .data(&above),
        Dataset::default()
            .name("P&L-")
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(LOSS_RED))
            .data(&below),
        Dataset::default()
            .name("")
            .marker(Marker::Dot)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(DIM))
            .data(&zero_line),
    ];

    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_DIM))
                .title(Span::styled(title, Style::default().fg(AMBER).bold())),
        )
        .x_axis(
            Axis::default()
                .style(Style::default().fg(DIM))
                .bounds([0.0, x_max]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(DIM))
                .labels(vec![
                    Span::styled(format!("${:.2}", y_lo), Style::default().fg(DIM)),
                    Span::styled("$0.00", Style::default().fg(DIM)),
                    Span::styled(format!("${:.2}", y_hi), Style::default().fg(DIM)),
                ])
                .bounds([y_lo, y_hi]),
        );

    frame.render_widget(chart, area);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Drawing — Risk Gauges sidebar
// ═══════════════════════════════════════════════════════════════════════════

fn draw_risk_gauges(frame: &mut Frame, state: &TuiState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(22),
            Constraint::Percentage(22),
            Constraint::Percentage(22),
            Constraint::Percentage(22),
            Constraint::Percentage(12),
        ])
        .split(area);

    let limits = &state.risk_limits;

    // Exposure
    let exp_val = state.total_exposure.to_f64().unwrap_or(0.0);
    let exp_ratio = (exp_val / limits.max_total_exposure).clamp(0.0, 1.0);
    frame.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BORDER_DIM))
                    .title(Span::styled(
                        " Exposure ",
                        Style::default().fg(ACCENT_VIOLET).bold(),
                    )),
            )
            .gauge_style(Style::default().fg(gauge_color(exp_ratio)).bg(GAUGE_BG))
            .ratio(exp_ratio)
            .label(Span::styled(
                format!(
                    "${:.0}/{:.0} ({:.0}%)",
                    exp_val,
                    limits.max_total_exposure,
                    exp_ratio * 100.0
                ),
                Style::default().fg(Color::White).bold(),
            )),
        chunks[0],
    );

    // Daily Loss
    let loss_val = state.daily_loss.to_f64().unwrap_or(0.0);
    let loss_ratio = (loss_val / limits.max_daily_loss).clamp(0.0, 1.0);
    frame.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BORDER_DIM))
                    .title(Span::styled(
                        " Daily Loss ",
                        Style::default().fg(ACCENT_VIOLET).bold(),
                    )),
            )
            .gauge_style(Style::default().fg(gauge_color(loss_ratio)).bg(GAUGE_BG))
            .ratio(loss_ratio)
            .label(Span::styled(
                format!(
                    "${:.0}/{:.0} ({:.0}%)",
                    loss_val,
                    limits.max_daily_loss,
                    loss_ratio * 100.0
                ),
                Style::default().fg(Color::White).bold(),
            )),
        chunks[1],
    );

    // Unhedged
    let uh_val = state.unhedged_exposure.to_f64().unwrap_or(0.0);
    let uh_ratio = (uh_val / limits.max_unhedged_exposure).clamp(0.0, 1.0);
    frame.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BORDER_DIM))
                    .title(Span::styled(
                        " Unhedged ",
                        Style::default().fg(ACCENT_VIOLET).bold(),
                    )),
            )
            .gauge_style(Style::default().fg(gauge_color(uh_ratio)).bg(GAUGE_BG))
            .ratio(uh_ratio)
            .label(Span::styled(
                format!(
                    "${:.0}/{:.0} ({:.0}%)",
                    uh_val,
                    limits.max_unhedged_exposure,
                    uh_ratio * 100.0
                ),
                Style::default().fg(Color::White).bold(),
            )),
        chunks[2],
    );

    // Unwind Rate
    let uw_val = state.unwind_rate.to_f64().unwrap_or(0.0);
    let uw_ratio = (uw_val / limits.max_unwind_rate_pct).clamp(0.0, 1.0);
    frame.render_widget(
        Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BORDER_DIM))
                    .title(Span::styled(
                        " Unwind Rate ",
                        Style::default().fg(ACCENT_VIOLET).bold(),
                    )),
            )
            .gauge_style(Style::default().fg(gauge_color(uw_ratio)).bg(GAUGE_BG))
            .ratio(uw_ratio)
            .label(Span::styled(
                format!(
                    "{:.1}%/{:.0}% ({:.0}%)",
                    uw_val,
                    limits.max_unwind_rate_pct,
                    uw_ratio * 100.0
                ),
                Style::default().fg(Color::White).bold(),
            )),
        chunks[3],
    );

    // Position summary
    let total_profit: Decimal = state.positions.iter().map(|p| p.guaranteed_profit).sum();
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} pos", state.positions.len()),
                Style::default().fg(POLY_COLOR),
            ),
            Span::styled(" \u{2502} ", Style::default().fg(DIM)),
            Span::styled(
                format!("${:.2}", total_profit),
                Style::default().fg(pnl_color(total_profit)).bold(),
            ),
        ])),
        chunks[4],
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Drawing — Live prices table
// ═══════════════════════════════════════════════════════════════════════════

fn draw_prices(frame: &mut Frame, state: &TuiState, area: Rect) {
    let header = Row::new(vec![
        Cell::from("Market").style(Style::default().bold()),
        Cell::from("P.Yes").style(Style::default().bold()),
        Cell::from("P.No").style(Style::default().bold()),
        Cell::from("Chg").style(Style::default().bold()),
        Cell::from("K.Yes").style(Style::default().bold()),
        Cell::from("K.No").style(Style::default().bold()),
        Cell::from("Spread").style(Style::default().bold()),
        Cell::from("Trend").style(Style::default().bold()),
        Cell::from("Age").style(Style::default().bold()),
    ])
    .style(Style::default().bg(HEADER_BG));

    let rows: Vec<Row> = state
        .prices
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let row_bg = if i % 2 == 1 { ROW_ALT_BG } else { Color::Reset };

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
            let change_color = if change > dec!(0) {
                PROFIT_GREEN
            } else if change < dec!(0) {
                LOSS_RED
            } else {
                DIM
            };

            let spread_str = format!("{:.1}c", p.spread * dec!(100));

            let age_str = if p.last_update_secs < 0 {
                "now".into()
            } else if p.last_update_secs < 60 {
                format!("{}s ago", p.last_update_secs)
            } else {
                format!("{}m ago", p.last_update_secs / 60)
            };

            let trend_str = state
                .pair_charts
                .get(&p.pair_id)
                .map(|c| sparkline_str(&c.poly_yes))
                .unwrap_or_else(|| "--------".to_string());
            let t_color = state
                .pair_charts
                .get(&p.pair_id)
                .map(|c| trend_color(&c.poly_yes))
                .unwrap_or(DIM);

            let age_cell = Cell::from(Line::from(vec![
                Span::styled(
                    "\u{25cf}",
                    Style::default().fg(freshness_color(p.last_update_secs)),
                ),
                Span::styled(
                    format!(" {}", age_str),
                    Style::default().fg(freshness_color(p.last_update_secs)),
                ),
            ]));

            Row::new(vec![
                Cell::from(p.name.clone()),
                Cell::from(format!("{:.2}", p.poly_yes))
                    .style(Style::default().fg(POLY_COLOR).bold()),
                Cell::from(format!("{:.2}", p.poly_no)).style(Style::default().fg(POLY_COLOR)),
                Cell::from(change_str).style(Style::default().fg(change_color)),
                Cell::from(format!("{:.2}", p.kalshi_yes))
                    .style(Style::default().fg(KALSHI_COLOR).bold()),
                Cell::from(format!("{:.2}", p.kalshi_no))
                    .style(Style::default().fg(KALSHI_COLOR)),
                Cell::from(spread_str).style(Style::default().fg(spread_color(p.spread))),
                Cell::from(trend_str).style(Style::default().fg(t_color)),
                age_cell,
            ])
            .style(Style::default().bg(row_bg))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_DIM))
        .title(Span::styled(
            format!(" Live Prices ({}) ", state.prices.len()),
            Style::default().fg(AMBER).bold(),
        ));

    let table = Table::new(
        rows,
        [
            Constraint::Min(18),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(9),
        ],
    )
    .header(header)
    .block(block);

    frame.render_widget(table, area);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Drawing — Tabbed panel (Orders / Positions)
// ═══════════════════════════════════════════════════════════════════════════

fn draw_tabbed_panel(frame: &mut Frame, state: &TuiState, area: Rect) {
    match state.tab_view {
        TabView::Orders => draw_orders(frame, state, area),
        TabView::Positions => draw_positions(frame, state, area),
    }
}

fn draw_orders(frame: &mut Frame, state: &TuiState, area: Rect) {
    let tab_title = Line::from(vec![
        Span::styled(
            format!(" [Orders ({})]", state.open_orders.len()),
            Style::default().fg(AMBER).bold(),
        ),
        Span::styled(
            format!(" Positions ({}) ", state.positions.len()),
            Style::default().fg(DIM),
        ),
    ]);

    if state.open_orders.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_DIM))
            .title(tab_title);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No active orders",
                Style::default().fg(DIM),
            )))
            .alignment(Alignment::Center)
            .block(block),
            area,
        );
        return;
    }

    let header = Row::new(vec![
        Cell::from("Platform").style(Style::default().bold()),
        Cell::from("Market").style(Style::default().bold()),
        Cell::from("Side").style(Style::default().bold()),
        Cell::from("Price").style(Style::default().bold()),
        Cell::from("Qty").style(Style::default().bold()),
        Cell::from("Age").style(Style::default().bold()),
        Cell::from("Status").style(Style::default().bold()),
    ])
    .style(Style::default().bg(HEADER_BG));

    let rows: Vec<Row> = state
        .open_orders
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let row_bg = if i % 2 == 1 { ROW_ALT_BG } else { Color::Reset };
            let age = (Utc::now() - o.placed_at).num_seconds();
            let plat = if o.platform == "polymarket" {
                "POLY"
            } else {
                "KALSHI"
            };
            let market_name = state
                .market_names
                .get(&o.market_id)
                .cloned()
                .unwrap_or_else(|| {
                    if o.market_id.len() > 25 {
                        format!("{}...", &o.market_id[..25])
                    } else {
                        o.market_id.clone()
                    }
                });

            let side_color = match o.side.as_str() {
                "yes" => PROFIT_GREEN,
                "no" => LOSS_RED,
                _ => Color::White,
            };

            let price_str = if o.price == dec!(0) {
                "-".to_string()
            } else {
                format!("{:.2}", o.price)
            };

            let status_color = match o.status.as_str() {
                "open" => PROFIT_GREEN,
                "pending" => NEUTRAL_YELLOW,
                "partial_fill" => POLY_COLOR,
                _ => Color::White,
            };

            Row::new(vec![
                Cell::from(plat).style(Style::default().fg(platform_color(&o.platform))),
                Cell::from(market_name),
                Cell::from(o.side.to_uppercase())
                    .style(Style::default().fg(side_color).bold()),
                Cell::from(price_str).style(Style::default().fg(NEUTRAL_YELLOW)),
                Cell::from(o.quantity.to_string()),
                Cell::from(format_age(age)).style(Style::default().fg(DIM)),
                Cell::from(o.status.to_uppercase())
                    .style(Style::default().fg(status_color)),
            ])
            .style(Style::default().bg(row_bg))
        })
        .collect();

    // Scrolling
    let visible_height = area.height.saturating_sub(4) as usize;
    let scroll = (state.order_scroll as usize).min(rows.len().saturating_sub(1));
    let end = (scroll + visible_height).min(rows.len());
    let visible_rows: Vec<Row> = rows.into_iter().skip(scroll).take(visible_height).collect();

    let scroll_info = if state.open_orders.len() > visible_height {
        format!(
            " [Orders ({}) {}-{}]",
            state.open_orders.len(),
            scroll + 1,
            end,
        )
    } else {
        format!(" [Orders ({})]", state.open_orders.len())
    };
    let tab_title_scroll = Line::from(vec![
        Span::styled(scroll_info, Style::default().fg(AMBER).bold()),
        Span::styled(
            format!(" Positions ({}) ", state.positions.len()),
            Style::default().fg(DIM),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_DIM))
        .title(tab_title_scroll);

    let table = Table::new(
        visible_rows,
        [
            Constraint::Length(8),
            Constraint::Min(30),
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(5),
            Constraint::Length(6),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(block)
    .row_highlight_style(Style::default().bg(HEADER_BG));

    frame.render_widget(table, area);
}

fn draw_positions(frame: &mut Frame, state: &TuiState, area: Rect) {
    let total_profit: Decimal = state.positions.iter().map(|p| p.guaranteed_profit).sum();

    let tab_title = Line::from(vec![
        Span::styled(
            format!(" Orders ({}) ", state.open_orders.len()),
            Style::default().fg(DIM),
        ),
        Span::styled(
            format!("[Positions ({}) ${:.2}]", state.positions.len(), total_profit),
            Style::default().fg(AMBER).bold(),
        ),
    ]);

    if state.positions.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_DIM))
            .title(tab_title);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No open positions",
                Style::default().fg(DIM),
            )))
            .alignment(Alignment::Center)
            .block(block),
            area,
        );
        return;
    }

    let header = Row::new(vec![
        Cell::from("Market").style(Style::default().bold()),
        Cell::from("Poly Side").style(Style::default().bold()),
        Cell::from("Kalshi Side").style(Style::default().bold()),
        Cell::from("Qty").style(Style::default().bold()),
        Cell::from("Profit").style(Style::default().bold()),
        Cell::from("Status").style(Style::default().bold()),
    ])
    .style(Style::default().bg(HEADER_BG));

    let rows: Vec<Row> = state
        .positions
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let row_bg = if i % 2 == 1 { ROW_ALT_BG } else { Color::Reset };
            let market_name = state
                .market_names
                .get(&p.pair_id)
                .cloned()
                .unwrap_or_else(|| {
                    if p.pair_id.len() > 25 {
                        format!("{}...", &p.pair_id[..25])
                    } else {
                        p.pair_id.clone()
                    }
                });

            let profit_color = pnl_color(p.guaranteed_profit);
            let status_color = match p.status.as_str() {
                "open" => PROFIT_GREEN,
                "settled" => DIM,
                _ => NEUTRAL_YELLOW,
            };

            Row::new(vec![
                Cell::from(if market_name.len() > 25 {
                    format!("{}...", &market_name[..25])
                } else {
                    market_name
                }),
                Cell::from(format!(
                    "{} {} @{:.2}",
                    p.poly_quantity,
                    p.poly_side.to_uppercase(),
                    p.poly_avg_price
                ))
                .style(Style::default().fg(POLY_COLOR)),
                Cell::from(format!(
                    "{} {} @{:.2}",
                    p.kalshi_quantity,
                    p.kalshi_side.to_uppercase(),
                    p.kalshi_avg_price
                ))
                .style(Style::default().fg(KALSHI_COLOR)),
                Cell::from(p.hedged_quantity.to_string()),
                Cell::from(format!("${:.2}", p.guaranteed_profit))
                    .style(Style::default().fg(profit_color).bold()),
                Cell::from(p.status.to_uppercase())
                    .style(Style::default().fg(status_color)),
            ])
            .style(Style::default().bg(row_bg))
        })
        .collect();

    // Scrolling
    let visible_height = area.height.saturating_sub(4) as usize;
    let scroll = (state.pos_scroll as usize).min(rows.len().saturating_sub(1));
    let visible_rows: Vec<Row> = rows.into_iter().skip(scroll).take(visible_height).collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_DIM))
        .title(tab_title);

    let table = Table::new(
        visible_rows,
        [
            Constraint::Min(20),
            Constraint::Length(16),
            Constraint::Length(16),
            Constraint::Length(5),
            Constraint::Length(10),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(block);

    frame.render_widget(table, area);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Drawing — Footer (P&L bar + key hints, 2 rows no border)
// ═══════════════════════════════════════════════════════════════════════════

fn draw_footer(frame: &mut Frame, state: &TuiState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    // P&L bar
    let pnl = state.daily_pnl.as_ref();
    let net: Decimal = pnl.map(|p| p.net_profit).unwrap_or(dec!(0));
    let trades = pnl.map(|p| p.trades_executed).unwrap_or(0);

    let pnl_line = Line::from(vec![
        Span::styled(" P&L:", Style::default().fg(DIM)),
        Span::styled(
            format!("${:.2}", net),
            Style::default().fg(pnl_color(net)).bold(),
        ),
        Span::styled(" \u{2502} ", Style::default().fg(BORDER_DIM)),
        Span::styled(
            format!("Trades:{}", trades),
            Style::default().fg(POLY_COLOR),
        ),
        Span::styled(" \u{2502} ", Style::default().fg(BORDER_DIM)),
        Span::styled(
            format!("Loss:${:.2}", state.daily_loss),
            Style::default().fg(if state.daily_loss > dec!(0) {
                LOSS_RED
            } else {
                DIM
            }),
        ),
        Span::styled(" \u{2502} ", Style::default().fg(BORDER_DIM)),
        Span::styled(
            format!("Unhedged:${:.2}", state.unhedged_exposure),
            Style::default().fg(if state.unhedged_exposure > dec!(0) {
                NEUTRAL_YELLOW
            } else {
                DIM
            }),
        ),
        Span::styled(" \u{2502} ", Style::default().fg(BORDER_DIM)),
        Span::styled(
            format!("Unwind:{:.1}%", state.unwind_rate),
            Style::default().fg(DIM),
        ),
    ]);
    frame.render_widget(Paragraph::new(pnl_line), chunks[0]);

    // Key hints
    let view_label = state.chart_view.label();
    let keys = Line::from(vec![
        Span::styled(" q", Style::default().fg(AMBER).bold()),
        Span::styled(" quit  ", Style::default().fg(DIM)),
        Span::styled("p", Style::default().fg(AMBER).bold()),
        Span::styled(" pause  ", Style::default().fg(DIM)),
        Span::styled("r", Style::default().fg(AMBER).bold()),
        Span::styled(" resume  ", Style::default().fg(DIM)),
        Span::styled("v", Style::default().fg(AMBER).bold()),
        Span::styled(format!(" view({})  ", view_label), Style::default().fg(DIM)),
        Span::styled("1-4", Style::default().fg(AMBER).bold()),
        Span::styled(" jump  ", Style::default().fg(DIM)),
        Span::styled("Tab", Style::default().fg(AMBER).bold()),
        Span::styled(" tab  ", Style::default().fg(DIM)),
        Span::styled("j/k", Style::default().fg(AMBER).bold()),
        Span::styled(" scroll  ", Style::default().fg(DIM)),
        Span::styled("n/N", Style::default().fg(AMBER).bold()),
        Span::styled(" pair", Style::default().fg(DIM)),
    ]);
    frame.render_widget(Paragraph::new(keys), chunks[1]);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Main TUI loop
// ═══════════════════════════════════════════════════════════════════════════

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
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;

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
                        KeyCode::Char('v') => {
                            state.chart_view = state.chart_view.next();
                        }
                        KeyCode::Char('1') => state.chart_view = ChartView::PriceOverlay,
                        KeyCode::Char('2') => state.chart_view = ChartView::SpreadHistory,
                        KeyCode::Char('3') => state.chart_view = ChartView::Candlestick,
                        KeyCode::Char('4') => state.chart_view = ChartView::EquityCurve,
                        KeyCode::Tab => {
                            state.tab_view = match state.tab_view {
                                TabView::Orders => TabView::Positions,
                                TabView::Positions => TabView::Orders,
                            };
                        }
                        KeyCode::Char('j') => match state.tab_view {
                            TabView::Orders => {
                                state.order_scroll = (state.order_scroll + 1)
                                    .min(state.open_orders.len().saturating_sub(1) as u16);
                            }
                            TabView::Positions => {
                                state.pos_scroll = (state.pos_scroll + 1)
                                    .min(state.positions.len().saturating_sub(1) as u16);
                            }
                        },
                        KeyCode::Char('k') => match state.tab_view {
                            TabView::Orders => {
                                state.order_scroll = state.order_scroll.saturating_sub(1);
                            }
                            TabView::Positions => {
                                state.pos_scroll = state.pos_scroll.saturating_sub(1);
                            }
                        },
                        KeyCode::Char('n') => {
                            if !state.prices.is_empty() {
                                state.selected_chart_pair =
                                    (state.selected_chart_pair + 1) % state.prices.len();
                            }
                        }
                        KeyCode::Char('N') => {
                            if !state.prices.is_empty() {
                                state.selected_chart_pair = if state.selected_chart_pair == 0 {
                                    state.prices.len() - 1
                                } else {
                                    state.selected_chart_pair - 1
                                };
                            }
                        }
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
