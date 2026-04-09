# TUI Chart Architecture Design

## 1. Design Philosophy

**Goal**: Transform the current single-chart TUI into a multi-view premium trading terminal inspired by Bloomberg Terminal, OpenBB, and professional HFT dashboards.

**Principles**:
- Information density without clutter -- every pixel earns its place
- Glanceable risk status -- danger states must be visible in <0.5s
- Spread-first hierarchy -- this is an arbitrage engine, spread is king
- Zero-config defaults -- beautiful out of the box, power users can customize

## 2. Layout Architecture

### 2.1 Current Layout (for reference)

```
+----------------------------------------------+
| Status Bar (5 lines)                         |
+----------------------------------------------+
| Live Prices Table (dynamic height)           |
+----------------------------------------------+
| Open Orders (30%)                            |
+----------------------------------------------+
| Positions / Chart toggle (25%)               |
+----------------------------------------------+
| Daily P&L Bar (3 lines)                      |
+----------------------------------------------+
| Key Hints (1 line)                           |
+----------------------------------------------+
```

### 2.2 New Layout: Three-Column Hybrid

```
+====================================================+
| STATUS BAR: Engine | Mode | Uptime | Pairs | Profit | (3 lines)
+==================+================+=================+
|                  |                |  RISK GAUGES    |
|  LIVE PRICES     |  CHART AREA   |  - Exposure %   |
|  TABLE           |  (cycles via  |  - Daily Loss % |
|  (left column,   |   1-5 keys)   |  - Unhedged %   |
|   40% width)     |               |  - Unwind Rate  |
|                  |               |                 |
+------------------+               +-----------------+
|  OPEN ORDERS     |               |  TRADE FEED     |
|  (scrollable)    |               |  (recent fills) |
+==================+================+=================+
| P&L SUMMARY | KEY HINTS                             | (3 lines)
+====================================================+
```

**Rationale**: Professional trading terminals use side-by-side layouts. The left column is data tables (scannable), the center is the main chart (focal point), and the right column is risk/activity (glanceable alerts).

### 2.3 Responsive Breakpoints

- **Wide (>= 160 cols)**: Full three-column layout as above
- **Medium (120-159 cols)**: Two columns -- left tables stacked, right chart+risk combined
- **Narrow (< 120 cols)**: Current single-column stacked layout (graceful fallback)

Column proportions for wide layout:
- Left: `Constraint::Percentage(35)` -- prices + orders
- Center: `Constraint::Percentage(40)` -- chart area
- Right: `Constraint::Percentage(25)` -- risk gauges + trade feed

### 2.4 Layout Split Hierarchy (ratatui)

```
Vertical split:
  [0] Status bar -- Length(3)
  [1] Main area  -- Min(0)  (fills remaining)
  [2] Footer     -- Length(3)

Main area horizontal split:
  [0] Left column  -- Percentage(35)
  [1] Center column -- Percentage(40)
  [2] Right column  -- Percentage(25)

Left column vertical split:
  [0] Live Prices -- Percentage(55)
  [1] Open Orders -- Percentage(45)

Right column vertical split:
  [0] Risk Gauges  -- Length(12)
  [1] Trade Feed   -- Min(0)
```

## 3. Chart Views (Center Column)

Five chart views, cycled with number keys `1`-`5`:

### View 1: Spread Convergence Chart (default)

**Purpose**: The core arbitrage view. Shows whether spreads are converging or diverging.

**Data**:
- Primary line: `spread = poly_yes - kalshi_yes` over time (from `PairChartData`)
- Horizontal reference: spread threshold line at `min_spread_absolute` (0.02)
- Horizontal reference: zero line
- Shaded region: between spread line and threshold (green when above = opportunity, red when below = no opportunity)

**Computed fields (new)**:
- `spread_history: VecDeque<f64>` -- add to `PairChartData`
- `spread_ma_20: f64` -- 20-tick simple moving average of spread
- `spread_volatility: f64` -- rolling std dev of last 30 spread values

**Visual**:
- Braille line for spread, colored green when >= threshold, red when below
- Dotted line for 20-tick MA (amber)
- Dashed horizontal at threshold value (dim gray)
- Title shows: market name, current spread in cents, MA trend arrow

### View 2: Price Overlay Chart (current chart, enhanced)

**Purpose**: Compare POLY vs KALSHI YES prices to see divergence visually.

**Data** (existing):
- `pair_charts[id].poly_yes` -- POLY YES price series
- `pair_charts[id].kalshi_yes` -- KALSHI YES price series
- Threshold bands (existing)

**Enhancements**:
- Add fill markers where orders were executed (from `open_orders` timestamps)
- Add position entry markers (triangle up for buy, triangle down for sell)
- Show current live spread value as a right-margin annotation

### View 3: P&L Waterfall / Equity Curve

**Purpose**: Track cumulative profit over the session.

**Data** (new, computed in `refresh_state`):
- `equity_curve: VecDeque<f64>` -- running sum of guaranteed_profit snapshots
- `trade_markers: Vec<(f64, f64)>` -- x,y points where fills happened

**Computed**:
- Each refresh, snapshot current total `guaranteed_profit` from positions
- Build cumulative curve: `[0, p1, p1+p2, ...]`
- Positive region shaded green, negative region shaded red

**Visual**:
- Braille line for equity curve
- Horizontal zero line (dim)
- Green/red coloring above/below zero
- Markers at trade execution points
- Title: "Equity Curve | $X.XX total | Y trades"

### View 4: Opportunity Scanner / Heatmap

**Purpose**: At-a-glance view of all pairs and their current spread attractiveness.

**Data**:
- All `prices` rows -- one bar per pair
- Each bar height = `spread` value
- Color = spread_color function (green >= 3c, yellow >= 1c, dim < 1c)

**Visual**:
- Horizontal bar chart, one row per market pair
- Bar length proportional to spread (in cents)
- Color coding by attractiveness
- Right-aligned spread value in cents
- Pairs sorted by spread descending (best opportunities at top)
- Title: "Opportunity Scanner | X pairs | Best: Y.Yc"

**Implementation**: Use ratatui `BarChart` widget or manual `Paragraph` with block characters.

### View 5: Risk Dashboard

**Purpose**: Comprehensive risk state with visual gauges.

**Data** (all from `RiskManager` via `TuiState`):
- `total_exposure` vs `max_total_exposure` (10000)
- `unhedged_exposure` vs `max_unhedged_exposure` (500)
- `daily_loss` vs `max_daily_loss` (200)
- `unwind_rate` vs `max_unwind_rate_pct` (20%)
- Per-market exposure breakdown

**Visual**:
- Four horizontal gauge bars showing utilization %
- Color transitions: green (0-60%), yellow (60-80%), red (80-100%)
- Per-market exposure as a mini stacked bar
- Title: "Risk Dashboard | System: OK/WARNING/DANGER"

## 4. Right Column: Risk Gauges Widget

Always visible (not view-dependent). Compact version of View 5.

### 4.1 Mini Risk Gauges

Four single-line gauges:

```
EXP [=========-------] 62%  $6,200
UNH [====------------] 30%  $150
LOS [===--------------] 15%  $30
UNW [=----------------] 5%   5.0%
```

Each gauge:
- Label (3 chars)
- Bar (16 chars wide, using block characters)
- Percentage
- Absolute value

Color rules:
- 0-60%: PROFIT_GREEN
- 60-80%: NEUTRAL_YELLOW
- 80-100%: LOSS_RED
- >100% (breached): LOSS_RED + bold + blinking border

### 4.2 Trade Feed

Below the gauges, a scrolling feed of recent events:

```
14:32:01 FILL POLY YES 50@0.42 BTC>$50k
14:31:58 FILL KLSH NO  50@0.53 BTC>$50k
14:31:45 OPP  +3.2c  BTC>$50k
14:30:12 UNWD POLY YES 20@0.40 (-$0.40)
```

**Data sources**:
- Order fills (from order status changes in `refresh_state`)
- Opportunity detections (new field needed: `recent_events: VecDeque<TradeEvent>`)
- Unwind events (from `UnwindEventRow` queries)

## 5. New Data Structures

### 5.1 Extended PairChartData

```rust
pub struct PairChartData {
    pub poly_yes: VecDeque<f64>,
    pub kalshi_yes: VecDeque<f64>,
    // NEW:
    pub spread: VecDeque<f64>,
    pub spread_ma: f64,         // 20-tick MA
    pub spread_std: f64,        // 30-tick std dev
}
```

### 5.2 New TuiState Fields

```rust
pub struct TuiState {
    // ... existing fields ...

    // NEW: Chart view management
    pub active_chart_view: ChartView,  // enum: Spread, PriceOverlay, Equity, Scanner, Risk

    // NEW: Equity curve
    pub equity_curve: VecDeque<f64>,

    // NEW: Trade feed
    pub trade_feed: VecDeque<TradeEvent>,

    // NEW: Risk limits (from config, for gauge rendering)
    pub risk_config: RiskConfig,

    // NEW: Computed metrics
    pub fill_rate: f64,             // filled / total orders
    pub opportunity_frequency: f64, // opportunities per minute
    pub best_spread: Decimal,       // max spread across all pairs
    pub avg_spread: Decimal,        // mean spread across all pairs
}
```

### 5.3 ChartView Enum

```rust
pub enum ChartView {
    Spread,        // 1
    PriceOverlay,  // 2
    Equity,        // 3
    Scanner,       // 4
    Risk,          // 5
}
```

### 5.4 TradeEvent

```rust
pub struct TradeEvent {
    pub timestamp: DateTime<Utc>,
    pub kind: TradeEventKind,  // Fill, Opportunity, Unwind, Cancel
    pub platform: String,
    pub market_short: String,
    pub detail: String,        // "YES 50@0.42" or "+3.2c" or "-$0.40"
}
```

## 6. Keyboard Interaction Design

### New Key Bindings

| Key | Action |
|-----|--------|
| `1` | Switch to Spread Convergence chart |
| `2` | Switch to Price Overlay chart |
| `3` | Switch to Equity Curve chart |
| `4` | Switch to Opportunity Scanner |
| `5` | Switch to Risk Dashboard |
| `t` | Toggle chart panel (same as current, shows/hides center column) |
| `n/N` | Next/prev pair (for Views 1, 2) |
| `j/k` | Scroll orders (left column) |
| `J/K` | Scroll trade feed (right column) |
| `q` | Quit |
| `p` | Pause engine |
| `r` | Resume engine |
| `?` | Toggle full key help overlay |

### Updated Key Hints Bar

```
q quit  p pause  r resume  1-5 views  n/N pair  j/k scroll  ? help
```

## 7. Color System

### 7.1 Existing Palette (preserved)

| Constant | RGB | Usage |
|----------|-----|-------|
| AMBER | (251,139,30) | Section titles, key hints |
| PROFIT_GREEN | (74,246,195) | Gains, positive values, healthy gauges |
| LOSS_RED | (255,67,61) | Losses, negative values, danger gauges |
| NEUTRAL_YELLOW | (247,206,70) | Zero/neutral, warning gauges |
| POLY_COLOR | (102,203,228) | Polymarket data |
| KALSHI_COLOR | (50,115,185) | Kalshi data |
| HEADER_BG | (32,32,32) | Table headers |
| DIM | (124,124,124) | Auxiliary text |
| BORDER_DIM | (61,61,61) | Subtle borders |
| ROW_ALT_BG | (20,20,20) | Alternating rows |

### 7.2 New Colors

| Constant | RGB | Usage |
|----------|-----|-------|
| SPREAD_POSITIVE | (74,246,195) | Spread above threshold (= PROFIT_GREEN) |
| SPREAD_NEGATIVE | (255,67,61) | Spread below threshold (= LOSS_RED) |
| MA_LINE | (251,139,30) | Moving average line (= AMBER) |
| GAUGE_OK | (74,246,195) | 0-60% utilization |
| GAUGE_WARN | (247,206,70) | 60-80% utilization |
| GAUGE_DANGER | (255,67,61) | 80-100% utilization |
| FEED_TIME | (124,124,124) | Timestamp in trade feed |
| FEED_FILL | (74,246,195) | Fill events |
| FEED_OPP | (251,139,30) | Opportunity events |
| FEED_UNWIND | (255,67,61) | Unwind events |

## 8. Data Flow

### 8.1 Refresh Cycle (every 2s, existing)

```
refresh_state()
  -> query DB for orders, positions, P&L
  -> read RiskManager for exposure data
  -> update price cache -> PriceRow[]
  -> push to PairChartData (poly_yes, kalshi_yes)
  
  NEW additions:
  -> compute spread and push to PairChartData.spread
  -> recompute spread_ma and spread_std
  -> snapshot equity_curve point
  -> diff orders for new fills -> push TradeEvents
  -> compute fill_rate, opportunity_frequency, best_spread, avg_spread
```

### 8.2 New DB Queries Needed

1. `list_recent_unwind_events(limit: i64)` -- for trade feed (unwind events)
2. `count_opportunities_since(since: DateTime<Utc>)` -- for opportunity frequency metric

### 8.3 Computed Metrics (in refresh_state)

```rust
// Spread MA (20-tick simple moving average)
let spread_vals: Vec<f64> = chart_data.spread.iter().copied().collect();
let ma_window = spread_vals.len().min(20);
let ma = spread_vals[spread_vals.len()-ma_window..].iter().sum::<f64>() / ma_window as f64;

// Spread volatility (30-tick std dev)
let vol_window = spread_vals.len().min(30);
let slice = &spread_vals[spread_vals.len()-vol_window..];
let mean = slice.iter().sum::<f64>() / vol_window as f64;
let variance = slice.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vol_window as f64;
let std_dev = variance.sqrt();

// Fill rate
let filled = orders.iter().filter(|o| o.status == "filled").count();
let fill_rate = if orders.is_empty() { 0.0 } else { filled as f64 / orders.len() as f64 };

// Best/avg spread
let best_spread = prices.iter().map(|p| p.spread).max().unwrap_or(Decimal::ZERO);
let avg_spread = if prices.is_empty() { Decimal::ZERO }
    else { prices.iter().map(|p| p.spread).sum::<Decimal>() / Decimal::from(prices.len()) };
```

## 9. Trade-offs and Constraints

### 9.1 ratatui Limitations
- `Chart` widget only supports `Dataset` with `(f64, f64)` points -- no native bar chart in Chart
- `BarChart` widget exists but is separate from `Chart` -- use for View 4 (Scanner)
- No native gauge with color transitions -- build from `Paragraph` + block characters
- Braille markers give highest resolution but require Unicode support in terminal

### 9.2 Performance Considerations
- 120-tick rolling window per pair is fine for 10-20 pairs
- Equity curve should also be capped at 120 points (5-minute window at 2.5s refresh)
- Trade feed capped at 50 most recent events
- Spread MA/std dev computed inline during refresh -- O(n) where n=120, negligible

### 9.3 Terminal Size Edge Cases
- Minimum usable size: 80x24 (fallback to single-column, abbreviated tables)
- Three-column layout needs >= 160 columns and >= 40 rows
- If terminal is too small for three columns, gracefully degrade to two-column, then single

### 9.4 Implementation Phasing

**Phase 1**: Add ChartView enum + keyboard switching, implement Spread Convergence chart, keep current layout
**Phase 2**: Implement risk gauges widget, trade feed, right column
**Phase 3**: Implement three-column responsive layout
**Phase 4**: Add remaining chart views (Equity, Scanner, Risk Dashboard)
**Phase 5**: Polish, edge cases, responsive breakpoints
