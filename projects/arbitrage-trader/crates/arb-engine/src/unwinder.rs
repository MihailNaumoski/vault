use arb_risk::RiskManager;
use arb_types::{ArbError, LimitOrderRequest, OrderResponse, Platform, PredictionMarketConnector};
use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::{info, warn};

pub struct Unwinder {
    poly: Arc<dyn PredictionMarketConnector>,
    kalshi: Arc<dyn PredictionMarketConnector>,
    risk_manager: Arc<RwLock<RiskManager>>,
}

impl Unwinder {
    pub fn new(
        poly: Arc<dyn PredictionMarketConnector>,
        kalshi: Arc<dyn PredictionMarketConnector>,
        risk_manager: Arc<RwLock<RiskManager>>,
    ) -> Self {
        Self {
            poly,
            kalshi,
            risk_manager,
        }
    }

    pub async fn unwind(
        &self,
        filled_platform: Platform,
        filled_order: &OrderResponse,
        unfilled_order_id: &str,
    ) -> Result<Decimal, ArbError> {
        let (filled_conn, unfilled_conn): (
            &dyn PredictionMarketConnector,
            &dyn PredictionMarketConnector,
        ) = match filled_platform {
            Platform::Polymarket => (self.poly.as_ref(), self.kalshi.as_ref()),
            Platform::Kalshi => (self.kalshi.as_ref(), self.poly.as_ref()),
        };

        // 1. Cancel unfilled leg
        if let Err(e) = unfilled_conn.cancel_order(unfilled_order_id).await {
            warn!(
                order = unfilled_order_id,
                err = %e,
                "cancel failed (may already be cancelled)"
            );
        }

        // 2. Exit filled position at best bid
        let book = filled_conn
            .get_order_book(&filled_order.market_id)
            .await?;
        let best_bid = book
            .bids
            .first()
            .ok_or_else(|| ArbError::Other("no bids available for unwind".into()))?;

        let unwind_req = LimitOrderRequest {
            market_id: filled_order.market_id.clone(),
            side: filled_order.side.opposite(),
            price: best_bid.price,
            quantity: filled_order.filled_quantity,
        };

        info!(
            platform = %filled_platform,
            entry = %filled_order.price,
            exit = %best_bid.price,
            qty = filled_order.filled_quantity,
            "unwinding"
        );
        let _ = filled_conn.place_limit_order(&unwind_req).await?;

        // 3. Calculate + record loss
        let entry_cost = filled_order.price * Decimal::from(filled_order.filled_quantity);
        let exit_value = best_bid.price * Decimal::from(filled_order.filled_quantity);
        let loss = (entry_cost - exit_value).max(Decimal::ZERO);

        self.risk_manager
            .write()
            .exposure_mut()
            .record_unwind_loss(loss);
        warn!(platform = %filled_platform, loss = %loss, "unwind complete");
        Ok(loss)
    }
}
