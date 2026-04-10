-- Discovery run metadata
CREATE TABLE IF NOT EXISTS discovery_runs (
    id TEXT PRIMARY KEY,
    ran_at TEXT NOT NULL,
    poly_raw_count INTEGER,
    poly_filtered_count INTEGER,
    kalshi_raw_count INTEGER,
    kalshi_filtered_count INTEGER,
    candidates_found INTEGER,
    matches_found INTEGER
);

-- All markets fetched during discovery
CREATE TABLE IF NOT EXISTS discovery_markets (
    id TEXT PRIMARY KEY,
    platform TEXT NOT NULL,
    platform_id TEXT NOT NULL,
    question TEXT NOT NULL,
    yes_price REAL,
    no_price REAL,
    volume REAL,
    close_time TEXT,
    category TEXT,
    status TEXT NOT NULL,
    filter_reason TEXT,
    discovered_at TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES discovery_runs(id)
);

-- Sample of comparisons the matcher evaluated
CREATE TABLE IF NOT EXISTS discovery_comparisons (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES discovery_runs(id),
    poly_platform_id TEXT NOT NULL,
    kalshi_platform_id TEXT NOT NULL,
    poly_question TEXT,
    kalshi_question TEXT,
    poly_category TEXT,
    kalshi_category TEXT,
    blocked_by TEXT,
    composite_score REAL,
    text_score REAL,
    time_score REAL,
    shared_entities INTEGER,
    shared_tokens INTEGER
);

CREATE INDEX IF NOT EXISTS idx_discovery_markets_run ON discovery_markets(run_id);
CREATE INDEX IF NOT EXISTS idx_discovery_markets_platform ON discovery_markets(platform, status);
CREATE INDEX IF NOT EXISTS idx_discovery_comparisons_run ON discovery_comparisons(run_id);
CREATE INDEX IF NOT EXISTS idx_discovery_comparisons_score ON discovery_comparisons(composite_score);
