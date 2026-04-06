use crate::types::{MonitorAction, OrderConfig};
use arb_types::{ArbError, OrderStatus, Platform, PredictionMarketConnector};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

pub struct Monitor {
    poly: Arc<dyn PredictionMarketConnector>,
    kalshi: Arc<dyn PredictionMarketConnector>,
    config: OrderConfig,
}

impl Monitor {
    pub fn new(
        poly: Arc<dyn PredictionMarketConnector>,
        kalshi: Arc<dyn PredictionMarketConnector>,
        config: OrderConfig,
    ) -> Self {
        Self {
            poly,
            kalshi,
            config,
        }
    }

    pub async fn watch_order_pair(
        &self,
        poly_order_id: &str,
        kalshi_order_id: &str,
    ) -> Result<MonitorAction, ArbError> {
        let check_interval = Duration::from_millis(self.config.order_check_interval_ms);
        let max_order_age = Duration::from_secs(self.config.max_order_age_secs);
        let max_hedge_wait = Duration::from_secs(self.config.max_hedge_wait_secs);
        let start = Utc::now();
        let mut interval = tokio::time::interval(check_interval);

        loop {
            interval.tick().await;

            let poly_order = self.poly.get_order(poly_order_id).await?;
            let kalshi_order = self.kalshi.get_order(kalshi_order_id).await?;

            let poly_filled = poly_order.status == OrderStatus::Filled;
            let kalshi_filled = kalshi_order.status == OrderStatus::Filled;
            let poly_done =
                matches!(poly_order.status, OrderStatus::Cancelled | OrderStatus::Failed);
            let kalshi_done = matches!(
                kalshi_order.status,
                OrderStatus::Cancelled | OrderStatus::Failed
            );

            let elapsed = (Utc::now() - start).to_std().unwrap_or(Duration::ZERO);

            match (poly_filled, kalshi_filled) {
                (true, true) => {
                    info!(poly = poly_order_id, kalshi = kalshi_order_id, "both filled");
                    return Ok(MonitorAction::BothFilled {
                        poly_order,
                        kalshi_order,
                    });
                }
                (true, false) => {
                    if kalshi_done || elapsed > max_hedge_wait {
                        warn!("poly filled, kalshi timeout - need unwind");
                        return Ok(MonitorAction::NeedsUnwind {
                            filled_platform: Platform::Polymarket,
                            filled_order: poly_order,
                            unfilled_order_id: kalshi_order_id.to_string(),
                        });
                    }
                    debug!(
                        "poly filled, waiting for kalshi ({:.1}s)",
                        elapsed.as_secs_f64()
                    );
                }
                (false, true) => {
                    if poly_done || elapsed > max_hedge_wait {
                        warn!("kalshi filled, poly timeout - need unwind");
                        return Ok(MonitorAction::NeedsUnwind {
                            filled_platform: Platform::Kalshi,
                            filled_order: kalshi_order,
                            unfilled_order_id: poly_order_id.to_string(),
                        });
                    }
                    debug!(
                        "kalshi filled, waiting for poly ({:.1}s)",
                        elapsed.as_secs_f64()
                    );
                }
                (false, false) => {
                    if (poly_done && kalshi_done) || elapsed > max_order_age {
                        if !poly_done {
                            let _ = self.poly.cancel_order(poly_order_id).await;
                        }
                        if !kalshi_done {
                            let _ = self.kalshi.cancel_order(kalshi_order_id).await;
                        }
                        return Ok(MonitorAction::BothCancelled);
                    }
                    debug!("both open ({:.1}s)", elapsed.as_secs_f64());
                }
            }
        }
    }
}
