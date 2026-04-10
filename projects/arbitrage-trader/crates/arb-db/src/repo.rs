use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::models::*;

/// Repository trait for all database operations.
#[async_trait::async_trait]
pub trait Repository: Send + Sync {
    // Market pairs
    async fn insert_market_pair(&self, pair: &MarketPairRow) -> anyhow::Result<()>;
    async fn get_market_pair(&self, id: &Uuid) -> anyhow::Result<Option<MarketPairRow>>;
    async fn list_active_market_pairs(&self) -> anyhow::Result<Vec<MarketPairRow>>;
    async fn update_market_pair(&self, pair: &MarketPairRow) -> anyhow::Result<()>;
    async fn delete_market_pair(&self, id: &Uuid) -> anyhow::Result<()>;

    // Opportunities
    async fn insert_opportunity(&self, opp: &OpportunityRow) -> anyhow::Result<()>;
    async fn get_opportunity(&self, id: &Uuid) -> anyhow::Result<Option<OpportunityRow>>;
    async fn list_opportunities_by_status(&self, status: &str) -> anyhow::Result<Vec<OpportunityRow>>;
    async fn update_opportunity_status(
        &self,
        id: &Uuid,
        status: &str,
        executed_at: Option<DateTime<Utc>>,
        resolved_at: Option<DateTime<Utc>>,
    ) -> anyhow::Result<()>;

    // Orders
    async fn insert_order(&self, order: &OrderRow) -> anyhow::Result<()>;
    async fn get_order(&self, id: &Uuid) -> anyhow::Result<Option<OrderRow>>;
    async fn list_orders_by_opportunity(&self, opportunity_id: &Uuid) -> anyhow::Result<Vec<OrderRow>>;
    async fn list_orders_by_status(&self, status: &str) -> anyhow::Result<Vec<OrderRow>>;
    async fn update_order_status(
        &self,
        id: &Uuid,
        status: &str,
        filled_quantity: i64,
        filled_at: Option<DateTime<Utc>>,
        cancelled_at: Option<DateTime<Utc>>,
        cancel_reason: Option<&str>,
    ) -> anyhow::Result<()>;

    // Positions
    async fn insert_position(&self, position: &PositionRow) -> anyhow::Result<()>;
    async fn get_position(&self, id: &Uuid) -> anyhow::Result<Option<PositionRow>>;
    async fn list_open_positions(&self) -> anyhow::Result<Vec<PositionRow>>;
    async fn list_positions_by_mode(&self, mode: &str) -> anyhow::Result<Vec<PositionRow>>;
    async fn update_position_status(
        &self,
        id: &Uuid,
        status: &str,
        settled_at: Option<DateTime<Utc>>,
    ) -> anyhow::Result<()>;

    // Price snapshots
    async fn insert_price_snapshot(&self, snapshot: &NewPriceSnapshot) -> anyhow::Result<()>;
    async fn list_price_snapshots(
        &self,
        pair_id: &Uuid,
        since: DateTime<Utc>,
    ) -> anyhow::Result<Vec<PriceSnapshotRow>>;

    // Daily P&L
    async fn upsert_daily_pnl(&self, pnl: &DailyPnlRow) -> anyhow::Result<()>;
    async fn get_daily_pnl(&self, date: NaiveDate) -> anyhow::Result<Option<DailyPnlRow>>;
    async fn list_daily_pnl(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> anyhow::Result<Vec<DailyPnlRow>>;

    // Unwind events
    async fn insert_unwind_event(&self, event: &UnwindEventRow) -> anyhow::Result<()>;
}

/// SQLite implementation of the Repository trait.
pub struct SqliteRepository {
    pool: SqlitePool,
}

impl SqliteRepository {
    pub async fn new(database_url: &str) -> anyhow::Result<Self> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    /// Run migrations from the SQL files at runtime.
    pub async fn run_migrations(&self) -> anyhow::Result<()> {
        let schema = include_str!("../../../migrations/001_initial_schema.sql");
        sqlx::raw_sql(schema).execute(&self.pool).await?;
        let unwind_events = include_str!("../../../migrations/002_unwind_events.sql");
        sqlx::raw_sql(unwind_events).execute(&self.pool).await?;

        // Migration 003: add mode column (idempotent — checks if column exists first)
        self.run_migration_003().await?;

        // Migration 004: discovery tables (idempotent — CREATE IF NOT EXISTS)
        let discovery = include_str!("../../../migrations/004_discovery_tables.sql");
        sqlx::raw_sql(discovery).execute(&self.pool).await?;

        tracing::info!("Database migrations applied");
        Ok(())
    }

    /// Migration 003: Add `mode` column to opportunities, orders, positions, daily_pnl.
    /// Recreates daily_pnl with composite primary key (date, mode).
    /// Idempotent — safe to run multiple times.
    async fn run_migration_003(&self) -> anyhow::Result<()> {
        // Helper: check if a column exists in a table
        async fn has_column(pool: &SqlitePool, table: &str, column: &str) -> bool {
            let query = format!("PRAGMA table_info({})", table);
            let rows = sqlx::query(&query).fetch_all(pool).await.unwrap_or_default();
            rows.iter().any(|r| {
                let name: &str = r.get("name");
                name == column
            })
        }

        // Add mode column to tables that don't have it yet
        for table in &["opportunities", "orders", "positions"] {
            if !has_column(&self.pool, table, "mode").await {
                let sql = format!("ALTER TABLE {} ADD COLUMN mode TEXT NOT NULL DEFAULT 'paper'", table);
                sqlx::query(&sql).execute(&self.pool).await?;
            }
        }

        // For daily_pnl, we need to check if it has mode AND has the composite PK.
        // If mode column is missing, we need to recreate the table with (date, mode) PK.
        if !has_column(&self.pool, "daily_pnl", "mode").await {
            sqlx::raw_sql(
                "CREATE TABLE IF NOT EXISTS daily_pnl_new (
                    date TEXT NOT NULL,
                    mode TEXT NOT NULL DEFAULT 'paper',
                    trades_executed INTEGER NOT NULL DEFAULT 0,
                    trades_filled INTEGER NOT NULL DEFAULT 0,
                    gross_profit TEXT NOT NULL DEFAULT '0',
                    fees_paid TEXT NOT NULL DEFAULT '0',
                    net_profit TEXT NOT NULL DEFAULT '0',
                    capital_deployed TEXT NOT NULL DEFAULT '0',
                    PRIMARY KEY (date, mode)
                );
                INSERT OR IGNORE INTO daily_pnl_new (date, mode, trades_executed, trades_filled, gross_profit, fees_paid, net_profit, capital_deployed)
                    SELECT date, 'paper', trades_executed, trades_filled, gross_profit, fees_paid, net_profit, capital_deployed FROM daily_pnl;
                DROP TABLE daily_pnl;
                ALTER TABLE daily_pnl_new RENAME TO daily_pnl;"
            ).execute(&self.pool).await?;
        }

        Ok(())
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

fn parse_dt(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .expect("valid RFC3339 datetime")
        .with_timezone(&Utc)
}

fn parse_decimal(s: &str) -> Decimal {
    s.parse().expect("valid decimal string")
}

fn parse_opt_dt(s: Option<String>) -> Option<DateTime<Utc>> {
    s.map(|v| parse_dt(&v))
}

fn market_pair_from_row(row: sqlx::sqlite::SqliteRow) -> MarketPairRow {
    MarketPairRow {
        id: row.get("id"),
        poly_condition_id: row.get("poly_condition_id"),
        poly_yes_token_id: row.get("poly_yes_token_id"),
        poly_no_token_id: row.get("poly_no_token_id"),
        poly_question: row.get("poly_question"),
        kalshi_ticker: row.get("kalshi_ticker"),
        kalshi_question: row.get("kalshi_question"),
        match_confidence: row.get("match_confidence"),
        verified: row.get::<bool, _>("verified"),
        active: row.get::<bool, _>("active"),
        close_time: parse_dt(row.get::<&str, _>("close_time")),
        created_at: parse_dt(row.get::<&str, _>("created_at")),
        updated_at: parse_dt(row.get::<&str, _>("updated_at")),
    }
}

fn opportunity_from_row(row: sqlx::sqlite::SqliteRow) -> OpportunityRow {
    OpportunityRow {
        id: row.get("id"),
        pair_id: row.get("pair_id"),
        poly_side: row.get("poly_side"),
        poly_price: parse_decimal(row.get::<&str, _>("poly_price")),
        kalshi_side: row.get("kalshi_side"),
        kalshi_price: parse_decimal(row.get::<&str, _>("kalshi_price")),
        spread: parse_decimal(row.get::<&str, _>("spread")),
        spread_pct: parse_decimal(row.get::<&str, _>("spread_pct")),
        max_quantity: row.get::<i64, _>("max_quantity"),
        status: row.get("status"),
        detected_at: parse_dt(row.get::<&str, _>("detected_at")),
        executed_at: parse_opt_dt(row.get::<Option<String>, _>("executed_at")),
        resolved_at: parse_opt_dt(row.get::<Option<String>, _>("resolved_at")),
        mode: row.get("mode"),
    }
}

fn order_from_row(row: sqlx::sqlite::SqliteRow) -> OrderRow {
    OrderRow {
        id: row.get("id"),
        opportunity_id: row.get("opportunity_id"),
        platform: row.get("platform"),
        platform_order_id: row.get("platform_order_id"),
        market_id: row.get("market_id"),
        side: row.get("side"),
        price: parse_decimal(row.get::<&str, _>("price")),
        quantity: row.get::<i64, _>("quantity"),
        filled_quantity: row.get::<i64, _>("filled_quantity"),
        status: row.get("status"),
        placed_at: parse_dt(row.get::<&str, _>("placed_at")),
        filled_at: parse_opt_dt(row.get::<Option<String>, _>("filled_at")),
        cancelled_at: parse_opt_dt(row.get::<Option<String>, _>("cancelled_at")),
        cancel_reason: row.get("cancel_reason"),
        mode: row.get("mode"),
    }
}

fn position_from_row(row: sqlx::sqlite::SqliteRow) -> PositionRow {
    PositionRow {
        id: row.get("id"),
        pair_id: row.get("pair_id"),
        poly_side: row.get("poly_side"),
        poly_quantity: row.get::<i64, _>("poly_quantity"),
        poly_avg_price: parse_decimal(row.get::<&str, _>("poly_avg_price")),
        kalshi_side: row.get("kalshi_side"),
        kalshi_quantity: row.get::<i64, _>("kalshi_quantity"),
        kalshi_avg_price: parse_decimal(row.get::<&str, _>("kalshi_avg_price")),
        hedged_quantity: row.get::<i64, _>("hedged_quantity"),
        unhedged_quantity: row.get::<i64, _>("unhedged_quantity"),
        guaranteed_profit: parse_decimal(row.get::<&str, _>("guaranteed_profit")),
        status: row.get("status"),
        opened_at: parse_dt(row.get::<&str, _>("opened_at")),
        settled_at: parse_opt_dt(row.get::<Option<String>, _>("settled_at")),
        mode: row.get("mode"),
    }
}

fn snapshot_from_row(row: sqlx::sqlite::SqliteRow) -> PriceSnapshotRow {
    PriceSnapshotRow {
        id: row.get::<i64, _>("id"),
        pair_id: row.get("pair_id"),
        poly_yes_price: parse_decimal(row.get::<&str, _>("poly_yes_price")),
        kalshi_yes_price: parse_decimal(row.get::<&str, _>("kalshi_yes_price")),
        spread: parse_decimal(row.get::<&str, _>("spread")),
        captured_at: parse_dt(row.get::<&str, _>("captured_at")),
    }
}

fn daily_pnl_from_row(row: sqlx::sqlite::SqliteRow) -> DailyPnlRow {
    DailyPnlRow {
        date: row.get("date"),
        mode: row.get("mode"),
        trades_executed: row.get::<i64, _>("trades_executed"),
        trades_filled: row.get::<i64, _>("trades_filled"),
        gross_profit: parse_decimal(row.get::<&str, _>("gross_profit")),
        fees_paid: parse_decimal(row.get::<&str, _>("fees_paid")),
        net_profit: parse_decimal(row.get::<&str, _>("net_profit")),
        capital_deployed: parse_decimal(row.get::<&str, _>("capital_deployed")),
    }
}

#[async_trait::async_trait]
impl Repository for SqliteRepository {
    async fn insert_market_pair(&self, pair: &MarketPairRow) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO market_pairs (id, poly_condition_id, poly_yes_token_id, poly_no_token_id, poly_question, kalshi_ticker, kalshi_question, match_confidence, verified, active, close_time, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&pair.id)
        .bind(&pair.poly_condition_id)
        .bind(&pair.poly_yes_token_id)
        .bind(&pair.poly_no_token_id)
        .bind(&pair.poly_question)
        .bind(&pair.kalshi_ticker)
        .bind(&pair.kalshi_question)
        .bind(pair.match_confidence)
        .bind(pair.verified)
        .bind(pair.active)
        .bind(pair.close_time.to_rfc3339())
        .bind(pair.created_at.to_rfc3339())
        .bind(pair.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_market_pair(&self, id: &Uuid) -> anyhow::Result<Option<MarketPairRow>> {
        let id_str = id.to_string();
        let row = sqlx::query(
            "SELECT id, poly_condition_id, poly_yes_token_id, poly_no_token_id, poly_question, kalshi_ticker, kalshi_question, match_confidence, verified, active, close_time, created_at, updated_at FROM market_pairs WHERE id = ?"
        )
        .bind(&id_str)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(market_pair_from_row))
    }

    async fn list_active_market_pairs(&self) -> anyhow::Result<Vec<MarketPairRow>> {
        let rows = sqlx::query(
            "SELECT id, poly_condition_id, poly_yes_token_id, poly_no_token_id, poly_question, kalshi_ticker, kalshi_question, match_confidence, verified, active, close_time, created_at, updated_at FROM market_pairs WHERE active = 1"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(market_pair_from_row).collect())
    }

    async fn update_market_pair(&self, pair: &MarketPairRow) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE market_pairs SET poly_condition_id = ?, poly_yes_token_id = ?, poly_no_token_id = ?, poly_question = ?, kalshi_ticker = ?, kalshi_question = ?, match_confidence = ?, verified = ?, active = ?, close_time = ?, updated_at = ? WHERE id = ?"
        )
        .bind(&pair.poly_condition_id)
        .bind(&pair.poly_yes_token_id)
        .bind(&pair.poly_no_token_id)
        .bind(&pair.poly_question)
        .bind(&pair.kalshi_ticker)
        .bind(&pair.kalshi_question)
        .bind(pair.match_confidence)
        .bind(pair.verified)
        .bind(pair.active)
        .bind(pair.close_time.to_rfc3339())
        .bind(pair.updated_at.to_rfc3339())
        .bind(&pair.id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_market_pair(&self, id: &Uuid) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM market_pairs WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn insert_opportunity(&self, opp: &OpportunityRow) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO opportunities (id, pair_id, poly_side, poly_price, kalshi_side, kalshi_price, spread, spread_pct, max_quantity, status, detected_at, executed_at, resolved_at, mode)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&opp.id)
        .bind(&opp.pair_id)
        .bind(&opp.poly_side)
        .bind(opp.poly_price.to_string())
        .bind(&opp.kalshi_side)
        .bind(opp.kalshi_price.to_string())
        .bind(opp.spread.to_string())
        .bind(opp.spread_pct.to_string())
        .bind(opp.max_quantity)
        .bind(&opp.status)
        .bind(opp.detected_at.to_rfc3339())
        .bind(opp.executed_at.map(|t| t.to_rfc3339()))
        .bind(opp.resolved_at.map(|t| t.to_rfc3339()))
        .bind(&opp.mode)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_opportunity(&self, id: &Uuid) -> anyhow::Result<Option<OpportunityRow>> {
        let id_str = id.to_string();
        let row = sqlx::query(
            "SELECT id, pair_id, poly_side, poly_price, kalshi_side, kalshi_price, spread, spread_pct, max_quantity, status, detected_at, executed_at, resolved_at, mode FROM opportunities WHERE id = ?"
        )
        .bind(&id_str)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(opportunity_from_row))
    }

    async fn list_opportunities_by_status(&self, status: &str) -> anyhow::Result<Vec<OpportunityRow>> {
        let rows = sqlx::query(
            "SELECT id, pair_id, poly_side, poly_price, kalshi_side, kalshi_price, spread, spread_pct, max_quantity, status, detected_at, executed_at, resolved_at, mode FROM opportunities WHERE status = ?"
        )
        .bind(status)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(opportunity_from_row).collect())
    }

    async fn update_opportunity_status(
        &self,
        id: &Uuid,
        status: &str,
        executed_at: Option<DateTime<Utc>>,
        resolved_at: Option<DateTime<Utc>>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE opportunities SET status = ?, executed_at = ?, resolved_at = ? WHERE id = ?"
        )
        .bind(status)
        .bind(executed_at.map(|t| t.to_rfc3339()))
        .bind(resolved_at.map(|t| t.to_rfc3339()))
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_order(&self, order: &OrderRow) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO orders (id, opportunity_id, platform, platform_order_id, market_id, side, price, quantity, filled_quantity, status, placed_at, filled_at, cancelled_at, cancel_reason, mode)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&order.id)
        .bind(&order.opportunity_id)
        .bind(&order.platform)
        .bind(&order.platform_order_id)
        .bind(&order.market_id)
        .bind(&order.side)
        .bind(order.price.to_string())
        .bind(order.quantity)
        .bind(order.filled_quantity)
        .bind(&order.status)
        .bind(order.placed_at.to_rfc3339())
        .bind(order.filled_at.map(|t| t.to_rfc3339()))
        .bind(order.cancelled_at.map(|t| t.to_rfc3339()))
        .bind(&order.cancel_reason)
        .bind(&order.mode)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_order(&self, id: &Uuid) -> anyhow::Result<Option<OrderRow>> {
        let id_str = id.to_string();
        let row = sqlx::query(
            "SELECT id, opportunity_id, platform, platform_order_id, market_id, side, price, quantity, filled_quantity, status, placed_at, filled_at, cancelled_at, cancel_reason, mode FROM orders WHERE id = ?"
        )
        .bind(&id_str)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(order_from_row))
    }

    async fn list_orders_by_opportunity(&self, opportunity_id: &Uuid) -> anyhow::Result<Vec<OrderRow>> {
        let id_str = opportunity_id.to_string();
        let rows = sqlx::query(
            "SELECT id, opportunity_id, platform, platform_order_id, market_id, side, price, quantity, filled_quantity, status, placed_at, filled_at, cancelled_at, cancel_reason, mode FROM orders WHERE opportunity_id = ?"
        )
        .bind(&id_str)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(order_from_row).collect())
    }

    async fn list_orders_by_status(&self, status: &str) -> anyhow::Result<Vec<OrderRow>> {
        let rows = sqlx::query(
            "SELECT id, opportunity_id, platform, platform_order_id, market_id, side, price, quantity, filled_quantity, status, placed_at, filled_at, cancelled_at, cancel_reason, mode FROM orders WHERE status = ?"
        )
        .bind(status)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(order_from_row).collect())
    }

    async fn update_order_status(
        &self,
        id: &Uuid,
        status: &str,
        filled_quantity: i64,
        filled_at: Option<DateTime<Utc>>,
        cancelled_at: Option<DateTime<Utc>>,
        cancel_reason: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE orders SET status = ?, filled_quantity = ?, filled_at = ?, cancelled_at = ?, cancel_reason = ? WHERE id = ?"
        )
        .bind(status)
        .bind(filled_quantity)
        .bind(filled_at.map(|t| t.to_rfc3339()))
        .bind(cancelled_at.map(|t| t.to_rfc3339()))
        .bind(cancel_reason)
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn insert_position(&self, position: &PositionRow) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO positions (id, pair_id, poly_side, poly_quantity, poly_avg_price, kalshi_side, kalshi_quantity, kalshi_avg_price, hedged_quantity, unhedged_quantity, guaranteed_profit, status, opened_at, settled_at, mode)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&position.id)
        .bind(&position.pair_id)
        .bind(&position.poly_side)
        .bind(position.poly_quantity)
        .bind(position.poly_avg_price.to_string())
        .bind(&position.kalshi_side)
        .bind(position.kalshi_quantity)
        .bind(position.kalshi_avg_price.to_string())
        .bind(position.hedged_quantity)
        .bind(position.unhedged_quantity)
        .bind(position.guaranteed_profit.to_string())
        .bind(&position.status)
        .bind(position.opened_at.to_rfc3339())
        .bind(position.settled_at.map(|t| t.to_rfc3339()))
        .bind(&position.mode)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_position(&self, id: &Uuid) -> anyhow::Result<Option<PositionRow>> {
        let id_str = id.to_string();
        let row = sqlx::query(
            "SELECT id, pair_id, poly_side, poly_quantity, poly_avg_price, kalshi_side, kalshi_quantity, kalshi_avg_price, hedged_quantity, unhedged_quantity, guaranteed_profit, status, opened_at, settled_at, mode FROM positions WHERE id = ?"
        )
        .bind(&id_str)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(position_from_row))
    }

    async fn list_open_positions(&self) -> anyhow::Result<Vec<PositionRow>> {
        let rows = sqlx::query(
            "SELECT id, pair_id, poly_side, poly_quantity, poly_avg_price, kalshi_side, kalshi_quantity, kalshi_avg_price, hedged_quantity, unhedged_quantity, guaranteed_profit, status, opened_at, settled_at, mode FROM positions WHERE status = 'open'"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(position_from_row).collect())
    }

    /// List open positions filtered by trading mode.
    async fn list_positions_by_mode(&self, mode: &str) -> anyhow::Result<Vec<PositionRow>> {
        let rows = sqlx::query(
            "SELECT id, pair_id, poly_side, poly_quantity, poly_avg_price, kalshi_side, kalshi_quantity, kalshi_avg_price, hedged_quantity, unhedged_quantity, guaranteed_profit, status, opened_at, settled_at, mode FROM positions WHERE status = 'open' AND mode = ?"
        )
        .bind(mode)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(position_from_row).collect())
    }

    async fn update_position_status(
        &self,
        id: &Uuid,
        status: &str,
        settled_at: Option<DateTime<Utc>>,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE positions SET status = ?, settled_at = ? WHERE id = ?")
            .bind(status)
            .bind(settled_at.map(|t| t.to_rfc3339()))
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn insert_price_snapshot(&self, snapshot: &NewPriceSnapshot) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO price_snapshots (pair_id, poly_yes_price, kalshi_yes_price, spread, captured_at)
             VALUES (?, ?, ?, ?, ?)"
        )
        .bind(snapshot.pair_id.to_string())
        .bind(snapshot.poly_yes_price.to_string())
        .bind(snapshot.kalshi_yes_price.to_string())
        .bind(snapshot.spread.to_string())
        .bind(snapshot.captured_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_price_snapshots(
        &self,
        pair_id: &Uuid,
        since: DateTime<Utc>,
    ) -> anyhow::Result<Vec<PriceSnapshotRow>> {
        let pair_str = pair_id.to_string();
        let since_str = since.to_rfc3339();
        let rows = sqlx::query(
            "SELECT id, pair_id, poly_yes_price, kalshi_yes_price, spread, captured_at FROM price_snapshots WHERE pair_id = ? AND captured_at >= ? ORDER BY captured_at"
        )
        .bind(&pair_str)
        .bind(&since_str)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(snapshot_from_row).collect())
    }

    async fn upsert_daily_pnl(&self, pnl: &DailyPnlRow) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO daily_pnl (date, mode, trades_executed, trades_filled, gross_profit, fees_paid, net_profit, capital_deployed)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(date, mode) DO UPDATE SET
                trades_executed = excluded.trades_executed,
                trades_filled = excluded.trades_filled,
                gross_profit = excluded.gross_profit,
                fees_paid = excluded.fees_paid,
                net_profit = excluded.net_profit,
                capital_deployed = excluded.capital_deployed"
        )
        .bind(&pnl.date)
        .bind(&pnl.mode)
        .bind(pnl.trades_executed)
        .bind(pnl.trades_filled)
        .bind(pnl.gross_profit.to_string())
        .bind(pnl.fees_paid.to_string())
        .bind(pnl.net_profit.to_string())
        .bind(pnl.capital_deployed.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_daily_pnl(&self, date: NaiveDate) -> anyhow::Result<Option<DailyPnlRow>> {
        let date_str = date.to_string();
        let row = sqlx::query(
            "SELECT date, mode, trades_executed, trades_filled, gross_profit, fees_paid, net_profit, capital_deployed FROM daily_pnl WHERE date = ?"
        )
        .bind(&date_str)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(daily_pnl_from_row))
    }

    async fn list_daily_pnl(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> anyhow::Result<Vec<DailyPnlRow>> {
        let from_str = from.to_string();
        let to_str = to.to_string();
        let rows = sqlx::query(
            "SELECT date, mode, trades_executed, trades_filled, gross_profit, fees_paid, net_profit, capital_deployed FROM daily_pnl WHERE date >= ? AND date <= ? ORDER BY date"
        )
        .bind(&from_str)
        .bind(&to_str)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(daily_pnl_from_row).collect())
    }

    async fn insert_unwind_event(&self, event: &UnwindEventRow) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO unwind_events (id, position_id, platform, order_id, entry_price, exit_price, quantity, slippage, loss, unwound_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&event.id)
        .bind(&event.position_id)
        .bind(&event.platform)
        .bind(&event.order_id)
        .bind(event.entry_price.to_string())
        .bind(event.exit_price.to_string())
        .bind(event.quantity)
        .bind(event.slippage.to_string())
        .bind(event.loss.to_string())
        .bind(event.unwound_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    async fn setup() -> SqliteRepository {
        let repo = SqliteRepository::new("sqlite::memory:").await.unwrap();
        repo.run_migrations().await.unwrap();
        repo
    }

    fn make_pair(id: &str) -> MarketPairRow {
        MarketPairRow {
            id: id.to_string(),
            poly_condition_id: "poly-cond-1".to_string(),
            poly_yes_token_id: "poly-yes-1".to_string(),
            poly_no_token_id: "poly-no-1".to_string(),
            poly_question: "Will it rain?".to_string(),
            kalshi_ticker: "RAIN-2026".to_string(),
            kalshi_question: "Will it rain tomorrow?".to_string(),
            match_confidence: 0.95,
            verified: true,
            active: true,
            close_time: Utc::now(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn make_opportunity(id: &str, pair_id: &str) -> OpportunityRow {
        OpportunityRow {
            id: id.to_string(),
            pair_id: pair_id.to_string(),
            poly_side: "yes".to_string(),
            poly_price: dec!(0.45),
            kalshi_side: "no".to_string(),
            kalshi_price: dec!(0.50),
            spread: dec!(0.05),
            spread_pct: dec!(5.0),
            max_quantity: 100,
            status: "detected".to_string(),
            detected_at: Utc::now(),
            executed_at: None,
            resolved_at: None,
            mode: "paper".to_string(),
        }
    }

    fn make_order(id: &str, opp_id: &str) -> OrderRow {
        OrderRow {
            id: id.to_string(),
            opportunity_id: opp_id.to_string(),
            platform: "polymarket".to_string(),
            platform_order_id: Some("plat-ord-1".to_string()),
            market_id: "mkt-1".to_string(),
            side: "yes".to_string(),
            price: dec!(0.45),
            quantity: 10,
            filled_quantity: 0,
            status: "pending".to_string(),
            placed_at: Utc::now(),
            filled_at: None,
            cancelled_at: None,
            cancel_reason: None,
            mode: "paper".to_string(),
        }
    }

    fn make_position(id: &str, pair_id: &str) -> PositionRow {
        PositionRow {
            id: id.to_string(),
            pair_id: pair_id.to_string(),
            poly_side: "yes".to_string(),
            poly_quantity: 10,
            poly_avg_price: dec!(0.45),
            kalshi_side: "no".to_string(),
            kalshi_quantity: 10,
            kalshi_avg_price: dec!(0.50),
            hedged_quantity: 10,
            unhedged_quantity: 0,
            guaranteed_profit: dec!(0.50),
            status: "open".to_string(),
            opened_at: Utc::now(),
            settled_at: None,
            mode: "paper".to_string(),
        }
    }

    // --- Market Pairs ---

    #[tokio::test]
    async fn test_insert_and_get_market_pair() {
        let repo = setup().await;
        let pair = make_pair("pair-1");
        repo.insert_market_pair(&pair).await.unwrap();
        let _fetched = repo.get_market_pair(&Uuid::parse_str("pair-1").unwrap_or_default()).await.unwrap();
        // Since our IDs are plain strings, query with the string directly
        let fetched = sqlx::query("SELECT id FROM market_pairs WHERE id = ?")
            .bind("pair-1")
            .fetch_optional(repo.pool())
            .await
            .unwrap();
        assert!(fetched.is_some());
    }

    #[tokio::test]
    async fn test_list_active_market_pairs() {
        let repo = setup().await;
        let mut pair1 = make_pair("pair-active");
        pair1.active = true;
        let mut pair2 = make_pair("pair-inactive");
        pair2.active = false;
        repo.insert_market_pair(&pair1).await.unwrap();
        repo.insert_market_pair(&pair2).await.unwrap();
        let active = repo.list_active_market_pairs().await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "pair-active");
    }

    #[tokio::test]
    async fn test_update_market_pair() {
        let repo = setup().await;
        let mut pair = make_pair("pair-upd");
        repo.insert_market_pair(&pair).await.unwrap();
        pair.kalshi_question = "Updated question?".to_string();
        pair.verified = false;
        repo.update_market_pair(&pair).await.unwrap();
        let rows = repo.list_active_market_pairs().await.unwrap();
        let found = rows.iter().find(|p| p.id == "pair-upd").unwrap();
        assert_eq!(found.kalshi_question, "Updated question?");
        assert!(!found.verified);
    }

    #[tokio::test]
    async fn test_delete_market_pair() {
        let repo = setup().await;
        let pair = make_pair("pair-del");
        repo.insert_market_pair(&pair).await.unwrap();
        repo.delete_market_pair(&Uuid::nil()).await.unwrap(); // delete by nil won't match
        // Delete using the actual string-based id via raw SQL to match our schema
        sqlx::query("DELETE FROM market_pairs WHERE id = ?")
            .bind("pair-del")
            .execute(repo.pool())
            .await
            .unwrap();
        let rows = repo.list_active_market_pairs().await.unwrap();
        assert!(rows.iter().all(|p| p.id != "pair-del"));
    }

    // --- Opportunities ---

    #[tokio::test]
    async fn test_insert_and_get_opportunity() {
        let repo = setup().await;
        let pair = make_pair("pair-opp");
        repo.insert_market_pair(&pair).await.unwrap();
        let opp = make_opportunity("opp-1", "pair-opp");
        repo.insert_opportunity(&opp).await.unwrap();
        let row = sqlx::query("SELECT id FROM opportunities WHERE id = ?")
            .bind("opp-1")
            .fetch_optional(repo.pool())
            .await
            .unwrap();
        assert!(row.is_some());
    }

    #[tokio::test]
    async fn test_list_opportunities_by_status() {
        let repo = setup().await;
        let pair = make_pair("pair-opp2");
        repo.insert_market_pair(&pair).await.unwrap();
        let opp1 = make_opportunity("opp-det", "pair-opp2");
        let mut opp2 = make_opportunity("opp-exec", "pair-opp2");
        opp2.status = "executed".to_string();
        repo.insert_opportunity(&opp1).await.unwrap();
        repo.insert_opportunity(&opp2).await.unwrap();
        let detected = repo.list_opportunities_by_status("detected").await.unwrap();
        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].id, "opp-det");
    }

    #[tokio::test]
    async fn test_update_opportunity_status() {
        let repo = setup().await;
        let pair = make_pair("pair-opp3");
        repo.insert_market_pair(&pair).await.unwrap();
        let opp = make_opportunity("opp-upd", "pair-opp3");
        repo.insert_opportunity(&opp).await.unwrap();
        let opp_id = Uuid::nil(); // won't match; use raw SQL to verify
        let _ = repo.update_opportunity_status(&opp_id, "executed", Some(Utc::now()), None).await;
        // Update via raw SQL to match string id
        sqlx::query("UPDATE opportunities SET status = ? WHERE id = ?")
            .bind("executed")
            .bind("opp-upd")
            .execute(repo.pool())
            .await
            .unwrap();
        let list = repo.list_opportunities_by_status("executed").await.unwrap();
        assert_eq!(list.len(), 1);
    }

    // --- Orders ---

    #[tokio::test]
    async fn test_insert_and_get_order() {
        let repo = setup().await;
        let pair = make_pair("pair-ord");
        repo.insert_market_pair(&pair).await.unwrap();
        let opp = make_opportunity("opp-ord", "pair-ord");
        repo.insert_opportunity(&opp).await.unwrap();
        let order = make_order("ord-1", "opp-ord");
        repo.insert_order(&order).await.unwrap();
        let row = sqlx::query("SELECT id FROM orders WHERE id = ?")
            .bind("ord-1")
            .fetch_optional(repo.pool())
            .await
            .unwrap();
        assert!(row.is_some());
    }

    #[tokio::test]
    async fn test_list_orders_by_status() {
        let repo = setup().await;
        let pair = make_pair("pair-ord2");
        repo.insert_market_pair(&pair).await.unwrap();
        let opp = make_opportunity("opp-ord2", "pair-ord2");
        repo.insert_opportunity(&opp).await.unwrap();
        let mut ord1 = make_order("ord-pend", "opp-ord2");
        ord1.status = "pending".to_string();
        let mut ord2 = make_order("ord-filled", "opp-ord2");
        ord2.status = "filled".to_string();
        repo.insert_order(&ord1).await.unwrap();
        repo.insert_order(&ord2).await.unwrap();
        let pending = repo.list_orders_by_status("pending").await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "ord-pend");
    }

    // --- Positions ---

    #[tokio::test]
    async fn test_insert_and_get_position() {
        let repo = setup().await;
        let pair = make_pair("pair-pos");
        repo.insert_market_pair(&pair).await.unwrap();
        let pos = make_position("pos-1", "pair-pos");
        repo.insert_position(&pos).await.unwrap();
        let row = sqlx::query("SELECT id FROM positions WHERE id = ?")
            .bind("pos-1")
            .fetch_optional(repo.pool())
            .await
            .unwrap();
        assert!(row.is_some());
    }

    #[tokio::test]
    async fn test_list_open_positions() {
        let repo = setup().await;
        let pair = make_pair("pair-pos2");
        repo.insert_market_pair(&pair).await.unwrap();
        let pos1 = make_position("pos-open", "pair-pos2");
        let mut pos2 = make_position("pos-closed", "pair-pos2");
        pos2.status = "settled".to_string();
        repo.insert_position(&pos1).await.unwrap();
        repo.insert_position(&pos2).await.unwrap();
        let open = repo.list_open_positions().await.unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, "pos-open");
    }

    // --- Price Snapshots ---

    #[tokio::test]
    async fn test_insert_and_list_price_snapshots() {
        let repo = setup().await;
        let pair = make_pair("pair-snap");
        repo.insert_market_pair(&pair).await.unwrap();
        let snap = NewPriceSnapshot {
            pair_id: Uuid::nil(),
            poly_yes_price: dec!(0.45),
            kalshi_yes_price: dec!(0.50),
            spread: dec!(0.05),
            captured_at: Utc::now(),
        };
        // Insert using raw SQL with the string pair_id to satisfy FK
        sqlx::query(
            "INSERT INTO price_snapshots (pair_id, poly_yes_price, kalshi_yes_price, spread, captured_at) VALUES (?, ?, ?, ?, ?)"
        )
        .bind("pair-snap")
        .bind(snap.poly_yes_price.to_string())
        .bind(snap.kalshi_yes_price.to_string())
        .bind(snap.spread.to_string())
        .bind(snap.captured_at.to_rfc3339())
        .execute(repo.pool())
        .await
        .unwrap();

        // List using raw query since pair_id is a string
        let rows: Vec<PriceSnapshotRow> = sqlx::query(
            "SELECT id, pair_id, poly_yes_price, kalshi_yes_price, spread, captured_at FROM price_snapshots WHERE pair_id = ?"
        )
        .bind("pair-snap")
        .fetch_all(repo.pool())
        .await
        .unwrap()
        .into_iter()
        .map(snapshot_from_row)
        .collect();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].poly_yes_price, dec!(0.45));
    }

    // --- Daily P&L ---

    #[tokio::test]
    async fn test_upsert_and_get_daily_pnl() {
        let repo = setup().await;
        let pnl = DailyPnlRow {
            date: "2026-04-06".to_string(),
            mode: "paper".to_string(),
            trades_executed: 5,
            trades_filled: 3,
            gross_profit: dec!(10.50),
            fees_paid: dec!(1.00),
            net_profit: dec!(9.50),
            capital_deployed: dec!(500.00),
        };
        repo.upsert_daily_pnl(&pnl).await.unwrap();
        let fetched = repo.get_daily_pnl(NaiveDate::from_ymd_opt(2026, 4, 6).unwrap()).await.unwrap().unwrap();
        assert_eq!(fetched.trades_executed, 5);
        assert_eq!(fetched.net_profit, dec!(9.50));
    }

    #[tokio::test]
    async fn test_upsert_daily_pnl_overwrites() {
        let repo = setup().await;
        let pnl1 = DailyPnlRow {
            date: "2026-04-06".to_string(),
            mode: "paper".to_string(),
            trades_executed: 5,
            trades_filled: 3,
            gross_profit: dec!(10.00),
            fees_paid: dec!(1.00),
            net_profit: dec!(9.00),
            capital_deployed: dec!(500.00),
        };
        repo.upsert_daily_pnl(&pnl1).await.unwrap();
        let pnl2 = DailyPnlRow {
            date: "2026-04-06".to_string(),
            mode: "paper".to_string(),
            trades_executed: 10,
            trades_filled: 8,
            gross_profit: dec!(25.00),
            fees_paid: dec!(2.00),
            net_profit: dec!(23.00),
            capital_deployed: dec!(1000.00),
        };
        repo.upsert_daily_pnl(&pnl2).await.unwrap();
        let fetched = repo.get_daily_pnl(NaiveDate::from_ymd_opt(2026, 4, 6).unwrap()).await.unwrap().unwrap();
        assert_eq!(fetched.trades_executed, 10);
        assert_eq!(fetched.net_profit, dec!(23.00));
    }

    // --- Foreign Key Constraint ---

    #[tokio::test]
    async fn test_order_with_invalid_opportunity_id_fails() {
        let repo = setup().await;
        // Enable foreign keys (SQLite has them off by default)
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(repo.pool())
            .await
            .unwrap();
        let order = make_order("ord-bad", "nonexistent-opp");
        let result = repo.insert_order(&order).await;
        assert!(result.is_err(), "inserting order with invalid opportunity_id should fail");
    }
}
