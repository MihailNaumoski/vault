-- Unwind events: persists every unwind so loss history survives restarts
CREATE TABLE IF NOT EXISTS unwind_events (
    id TEXT PRIMARY KEY,
    position_id TEXT,
    platform TEXT NOT NULL,
    order_id TEXT,
    entry_price TEXT NOT NULL,
    exit_price TEXT NOT NULL,
    quantity INTEGER NOT NULL,
    slippage TEXT NOT NULL,
    loss TEXT NOT NULL,
    unwound_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_unwind_events_position ON unwind_events(position_id);
CREATE INDEX IF NOT EXISTS idx_unwind_events_time ON unwind_events(unwound_at);
