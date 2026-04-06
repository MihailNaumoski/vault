-- Verified market pairs across platforms
CREATE TABLE IF NOT EXISTS market_pairs (
    id TEXT PRIMARY KEY,                    -- UUID v7
    poly_condition_id TEXT NOT NULL,
    poly_yes_token_id TEXT NOT NULL,
    poly_no_token_id TEXT NOT NULL,
    poly_question TEXT NOT NULL,
    kalshi_ticker TEXT NOT NULL,
    kalshi_question TEXT NOT NULL,
    match_confidence REAL NOT NULL,
    verified INTEGER NOT NULL DEFAULT 0,    -- boolean
    active INTEGER NOT NULL DEFAULT 1,
    close_time TEXT NOT NULL,               -- ISO 8601
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Detected arbitrage opportunities
CREATE TABLE IF NOT EXISTS opportunities (
    id TEXT PRIMARY KEY,
    pair_id TEXT NOT NULL REFERENCES market_pairs(id),
    poly_side TEXT NOT NULL,                -- 'yes' or 'no'
    poly_price TEXT NOT NULL,               -- decimal as string
    kalshi_side TEXT NOT NULL,
    kalshi_price TEXT NOT NULL,
    spread TEXT NOT NULL,
    spread_pct TEXT NOT NULL,
    max_quantity INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'detected',
    detected_at TEXT NOT NULL,
    executed_at TEXT,
    resolved_at TEXT
);

-- Orders placed on platforms
CREATE TABLE IF NOT EXISTS orders (
    id TEXT PRIMARY KEY,
    opportunity_id TEXT NOT NULL REFERENCES opportunities(id),
    platform TEXT NOT NULL,                 -- 'polymarket' or 'kalshi'
    platform_order_id TEXT,
    market_id TEXT NOT NULL,
    side TEXT NOT NULL,
    price TEXT NOT NULL,
    quantity INTEGER NOT NULL,
    filled_quantity INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    placed_at TEXT NOT NULL,
    filled_at TEXT,
    cancelled_at TEXT,
    cancel_reason TEXT
);

-- Hedged positions
CREATE TABLE IF NOT EXISTS positions (
    id TEXT PRIMARY KEY,
    pair_id TEXT NOT NULL REFERENCES market_pairs(id),
    poly_side TEXT NOT NULL,
    poly_quantity INTEGER NOT NULL,
    poly_avg_price TEXT NOT NULL,
    kalshi_side TEXT NOT NULL,
    kalshi_quantity INTEGER NOT NULL,
    kalshi_avg_price TEXT NOT NULL,
    hedged_quantity INTEGER NOT NULL,
    unhedged_quantity INTEGER NOT NULL DEFAULT 0,
    guaranteed_profit TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    opened_at TEXT NOT NULL,
    settled_at TEXT
);

-- Price snapshots for analysis
CREATE TABLE IF NOT EXISTS price_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pair_id TEXT NOT NULL REFERENCES market_pairs(id),
    poly_yes_price TEXT NOT NULL,
    kalshi_yes_price TEXT NOT NULL,
    spread TEXT NOT NULL,
    captured_at TEXT NOT NULL
);

-- Daily P&L summary
CREATE TABLE IF NOT EXISTS daily_pnl (
    date TEXT PRIMARY KEY,                  -- YYYY-MM-DD
    trades_executed INTEGER NOT NULL DEFAULT 0,
    trades_filled INTEGER NOT NULL DEFAULT 0,
    gross_profit TEXT NOT NULL DEFAULT '0',
    fees_paid TEXT NOT NULL DEFAULT '0',
    net_profit TEXT NOT NULL DEFAULT '0',
    capital_deployed TEXT NOT NULL DEFAULT '0'
);

CREATE INDEX IF NOT EXISTS idx_opportunities_pair ON opportunities(pair_id, status);
CREATE INDEX IF NOT EXISTS idx_orders_opportunity ON orders(opportunity_id);
CREATE INDEX IF NOT EXISTS idx_orders_status ON orders(status) WHERE status IN ('pending', 'open');
CREATE INDEX IF NOT EXISTS idx_positions_status ON positions(status) WHERE status = 'open';
CREATE INDEX IF NOT EXISTS idx_snapshots_pair_time ON price_snapshots(pair_id, captured_at);
