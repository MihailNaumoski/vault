use crate::types::{ExecutionResult, OrderConfig};
use arb_db::models::{OpportunityRow, OrderRow};
use arb_db::{Repository, SqliteRepository};
use arb_risk::RiskManager;
use arb_types::{
    ArbError, LimitOrderRequest, Opportunity, OpportunityStatus, OrderResponse, Platform,
    PredictionMarketConnector,
};
use chrono::Utc;
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

pub struct Executor {
    pub(crate) poly: Arc<dyn PredictionMarketConnector>,
    pub(crate) kalshi: Arc<dyn PredictionMarketConnector>,
    risk_manager: Arc<RwLock<RiskManager>>,
    db: Arc<SqliteRepository>,
    config: OrderConfig,
}

impl Executor {
    pub fn new(
        poly: Arc<dyn PredictionMarketConnector>,
        kalshi: Arc<dyn PredictionMarketConnector>,
        risk_manager: Arc<RwLock<RiskManager>>,
        db: Arc<SqliteRepository>,
        config: OrderConfig,
    ) -> Self {
        Self {
            poly,
            kalshi,
            risk_manager,
            db,
            config,
        }
    }

    pub async fn execute(&self, opp: &mut Opportunity) -> Result<ExecutionResult, ArbError> {
        let quantity = self.config.default_quantity;

        // 1. Pre-trade risk check — gather async data before locking
        let poly_balance = self.poly.get_balance().await?;
        let kalshi_balance = self.kalshi.get_balance().await?;
        let book = self
            .poly
            .get_order_book(&opp.poly_yes_token_id)
            .await
            .unwrap_or_default();
        let book_depth = book.asks.first().map(|l| l.quantity).unwrap_or(0);

        {
            let rm = self.risk_manager.read();
            rm.pre_trade_check(
                opp.pair_id,
                true,
                opp.spread,
                self.config.min_repost_spread,
                opp.close_time,
                quantity,
                opp.poly_price,
                opp.kalshi_price,
                poly_balance,
                kalshi_balance,
                book_depth,
            )
            .map_err(|e| ArbError::Other(format!("risk check failed: {e}")))?;
        }

        // 2. Build orders
        opp.max_quantity = quantity;
        opp.status = OpportunityStatus::Executing;

        let poly_req = LimitOrderRequest {
            market_id: opp.poly_yes_token_id.clone(),
            side: opp.poly_side,
            price: opp.poly_price,
            quantity,
        };
        let kalshi_req = LimitOrderRequest {
            market_id: opp.kalshi_market_id.clone(),
            side: opp.kalshi_side,
            price: opp.kalshi_price,
            quantity,
        };

        // 3. Place BOTH simultaneously
        info!(opp_id = %opp.id, spread = %opp.spread, "executing dual-leg order");
        let (poly_result, kalshi_result) = tokio::join!(
            self.poly.place_limit_order(&poly_req),
            self.kalshi.place_limit_order(&kalshi_req),
        );

        // 4. Handle all 4 outcomes
        match (poly_result, kalshi_result) {
            (Ok(poly_order), Ok(kalshi_order)) => {
                info!(
                    opp_id = %opp.id,
                    poly = %poly_order.order_id,
                    kalshi = %kalshi_order.order_id,
                    "both legs placed"
                );
                self.persist_opportunity(opp).await;
                self.persist_order(opp.id, Platform::Polymarket, &poly_order, &poly_req)
                    .await;
                self.persist_order(opp.id, Platform::Kalshi, &kalshi_order, &kalshi_req)
                    .await;
                Ok(ExecutionResult {
                    opportunity_id: opp.id,
                    poly_order,
                    kalshi_order,
                    executed_at: Utc::now(),
                })
            }
            (Ok(poly_order), Err(kalshi_err)) => {
                warn!(opp_id = %opp.id, err = %kalshi_err, "kalshi failed, cancelling poly");
                let _ = self.poly.cancel_order(&poly_order.order_id).await;
                opp.status = OpportunityStatus::Failed;
                self.persist_opportunity(opp).await;
                Err(ArbError::PlatformError {
                    platform: Platform::Kalshi,
                    message: format!("kalshi leg failed: {kalshi_err}"),
                })
            }
            (Err(poly_err), Ok(kalshi_order)) => {
                warn!(opp_id = %opp.id, err = %poly_err, "poly failed, cancelling kalshi");
                let _ = self.kalshi.cancel_order(&kalshi_order.order_id).await;
                opp.status = OpportunityStatus::Failed;
                self.persist_opportunity(opp).await;
                Err(ArbError::PlatformError {
                    platform: Platform::Polymarket,
                    message: format!("poly leg failed: {poly_err}"),
                })
            }
            (Err(poly_err), Err(kalshi_err)) => {
                error!(opp_id = %opp.id, "both legs failed");
                opp.status = OpportunityStatus::Failed;
                self.persist_opportunity(opp).await;
                Err(ArbError::Other(format!(
                    "both legs failed: poly={poly_err}, kalshi={kalshi_err}"
                )))
            }
        }
    }

    async fn persist_opportunity(&self, opp: &Opportunity) {
        let row = OpportunityRow {
            id: opp.id.to_string(),
            pair_id: opp.pair_id.to_string(),
            poly_side: format!("{:?}", opp.poly_side).to_lowercase(),
            poly_price: opp.poly_price,
            kalshi_side: format!("{:?}", opp.kalshi_side).to_lowercase(),
            kalshi_price: opp.kalshi_price,
            spread: opp.spread,
            spread_pct: opp.spread_pct,
            max_quantity: opp.max_quantity as i64,
            status: format!("{:?}", opp.status).to_lowercase(),
            detected_at: opp.detected_at,
            executed_at: Some(Utc::now()),
            resolved_at: None,
        };
        if let Err(e) = self.db.insert_opportunity(&row).await {
            error!(opp_id = %opp.id, err = %e, "failed to persist opportunity");
        }
    }

    async fn persist_order(
        &self,
        opp_id: Uuid,
        platform: Platform,
        resp: &OrderResponse,
        req: &LimitOrderRequest,
    ) {
        let row = OrderRow {
            id: Uuid::now_v7().to_string(),
            opportunity_id: opp_id.to_string(),
            platform: platform.to_string(),
            platform_order_id: Some(resp.order_id.clone()),
            market_id: req.market_id.clone(),
            side: format!("{:?}", req.side).to_lowercase(),
            price: req.price,
            quantity: req.quantity as i64,
            filled_quantity: resp.filled_quantity as i64,
            status: format!("{:?}", resp.status).to_lowercase(),
            placed_at: Utc::now(),
            filled_at: None,
            cancelled_at: None,
            cancel_reason: None,
        };
        if let Err(e) = self.db.insert_order(&row).await {
            error!(opp_id = %opp_id, err = %e, "failed to persist order");
        }
    }
}
