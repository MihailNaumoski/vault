use arb_db::models::PositionRow;
use arb_db::{Repository, SqliteRepository};
use arb_risk::RiskManager;
use arb_types::{ArbError, Opportunity, OrderResponse, Position, PositionStatus};
use chrono::Utc;
use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

pub struct Tracker {
    db: Arc<SqliteRepository>,
    risk_manager: Arc<RwLock<RiskManager>>,
}

impl Tracker {
    pub fn new(db: Arc<SqliteRepository>, risk_manager: Arc<RwLock<RiskManager>>) -> Self {
        Self { db, risk_manager }
    }

    pub async fn create_position(
        &self,
        opp: &Opportunity,
        poly_order: &OrderResponse,
        kalshi_order: &OrderResponse,
    ) -> Result<Position, ArbError> {
        let hedged = poly_order.filled_quantity.min(kalshi_order.filled_quantity);
        let unhedged =
            (poly_order.filled_quantity as i32) - (kalshi_order.filled_quantity as i32);
        let profit = opp.spread * Decimal::from(hedged);

        let position = Position {
            id: Uuid::now_v7(),
            pair_id: opp.pair_id,
            poly_side: opp.poly_side,
            poly_quantity: poly_order.filled_quantity,
            poly_avg_price: poly_order.price,
            kalshi_side: opp.kalshi_side,
            kalshi_quantity: kalshi_order.filled_quantity,
            kalshi_avg_price: kalshi_order.price,
            hedged_quantity: hedged,
            unhedged_quantity: unhedged,
            guaranteed_profit: profit,
            status: PositionStatus::Open,
            opened_at: Utc::now(),
            settled_at: None,
        };

        let row = PositionRow {
            id: position.id.to_string(),
            pair_id: position.pair_id.to_string(),
            poly_side: format!("{:?}", position.poly_side).to_lowercase(),
            poly_quantity: position.poly_quantity as i64,
            poly_avg_price: position.poly_avg_price,
            kalshi_side: format!("{:?}", position.kalshi_side).to_lowercase(),
            kalshi_quantity: position.kalshi_quantity as i64,
            kalshi_avg_price: position.kalshi_avg_price,
            hedged_quantity: position.hedged_quantity as i64,
            unhedged_quantity: position.unhedged_quantity as i64,
            guaranteed_profit: position.guaranteed_profit,
            status: "open".into(),
            opened_at: position.opened_at,
            settled_at: None,
        };
        if let Err(e) = self.db.insert_position(&row).await {
            error!(pos = %position.id, err = %e, "persist position failed");
        }

        let capital = poly_order.price * Decimal::from(poly_order.filled_quantity)
            + kalshi_order.price * Decimal::from(kalshi_order.filled_quantity);
        self.risk_manager
            .write()
            .exposure_mut()
            .add_position(opp.pair_id, capital);

        info!(pos = %position.id, profit = %profit, hedged, "position created");
        Ok(position)
    }
}
