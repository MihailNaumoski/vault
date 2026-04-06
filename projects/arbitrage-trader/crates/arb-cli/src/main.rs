use anyhow::Result;
use chrono::Utc;
use clap::Parser;
use rust_decimal_macros::dec;
use serde::Deserialize;
use tracing::{error, info, warn};

use std::sync::Arc;
use std::time::Duration;

use arb_engine::engine::Engine;
use arb_engine::executor::Executor;
use arb_engine::monitor::Monitor;
use arb_engine::paper::PaperConnector;
use arb_engine::price_cache::PriceCache;
use arb_engine::tracker::Tracker;
use arb_engine::types::{EngineConfig as EngineTypesConfig, OrderConfig, PairInfo};
use arb_engine::unwinder::Unwinder;
use arb_matcher::MatchPipeline;
use arb_types::{Market, MarketId, MarketStatus, Platform, PredictionMarketConnector};

mod tui;

/// Prediction market arbitrage system.
#[derive(Parser, Debug)]
#[command(name = "arb", about = "Cross-platform prediction market arbitrage")]
struct Args {
    /// Run in paper trading mode (connect to real APIs, simulate orders).
    #[arg(long)]
    paper: bool,

    /// Run market matcher only — show proposed pairs and exit.
    #[arg(long, rename_all = "kebab-case")]
    r#match: bool,

    /// Enable terminal UI dashboard.
    #[arg(long)]
    tui: bool,

    /// Headless mode — no TUI, log-only.
    #[arg(long)]
    headless: bool,
}

/// Top-level configuration deserialized from config/default.toml.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AppConfig {
    engine: EngineConfig,
    orders: OrdersConfig,
    risk: arb_risk::RiskConfig,
    polymarket: PlatformConfig,
    kalshi: KalshiConfig,
    database: DatabaseConfig,
    logging: LoggingConfig,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EngineConfig {
    enabled: bool,
    scan_interval_ms: u64,
    min_spread_pct: String,
    min_spread_absolute: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OrdersConfig {
    max_order_age_secs: u64,
    max_hedge_wait_secs: u64,
    order_check_interval_ms: u64,
    min_repost_spread: String,
    price_improve_amount: String,
    default_quantity: u32,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PlatformConfig {
    clob_url: String,
    gamma_url: String,
    ws_url: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct KalshiConfig {
    api_url: String,
    ws_url: String,
}

#[derive(Debug, Deserialize)]
struct DatabaseConfig {
    path: String,
}

#[derive(Debug, Deserialize)]
struct LoggingConfig {
    level: String,
    format: String,
}

fn init_tracing(config: &LoggingConfig, tui_mode: bool) {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.level));

    if tui_mode {
        // When TUI is active, write logs to file to avoid polluting the terminal
        let log_file = std::fs::File::create("data/arb.log").expect("failed to create log file");
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(log_file)
            .with_ansi(false)
            .init();
        return;
    }

    match config.format.as_str() {
        "json" => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .json()
                .init();
        }
        _ => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .pretty()
                .init();
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file (optional — won't fail if missing)
    let _ = dotenvy::dotenv();

    let args = Args::parse();

    // Load config from config/default.toml
    let settings = config::Config::builder()
        .add_source(config::File::with_name("config/default"))
        .build()?;

    let app_config: AppConfig = settings.try_deserialize()?;

    // Initialize tracing (logs to file when TUI is active)
    let tui_active = args.tui || (!args.headless && !args.paper);
    let _ = std::fs::create_dir_all("data");
    init_tracing(&app_config.logging, tui_active);

    info!("Starting arb system");
    info!(paper = args.paper, tui = args.tui, headless = args.headless, match_only = args.r#match);

    if args.r#match {
        info!("Match-only mode — running market matcher demo");

        // Sample Polymarket markets
        let poly_markets = vec![
            Market {
                id: MarketId::new(),
                platform: Platform::Polymarket,
                platform_id: "poly-btc-100k".into(),
                question: "Will Bitcoin hit $100k by December 2025?".into(),
                yes_price: dec!(0.62),
                no_price: dec!(0.38),
                volume: dec!(500000),
                liquidity: dec!(120000),
                status: MarketStatus::Open,
                close_time: Utc::now() + chrono::Duration::days(30),
                updated_at: Utc::now(),
            },
            Market {
                id: MarketId::new(),
                platform: Platform::Polymarket,
                platform_id: "poly-eth-5k".into(),
                question: "Will Ethereum reach $5,000 before 2026?".into(),
                yes_price: dec!(0.35),
                no_price: dec!(0.65),
                volume: dec!(200000),
                liquidity: dec!(50000),
                status: MarketStatus::Open,
                close_time: Utc::now() + chrono::Duration::days(60),
                updated_at: Utc::now(),
            },
            Market {
                id: MarketId::new(),
                platform: Platform::Polymarket,
                platform_id: "poly-fed-rate".into(),
                question: "Will the Fed cut rates in June 2025?".into(),
                yes_price: dec!(0.45),
                no_price: dec!(0.55),
                volume: dec!(800000),
                liquidity: dec!(300000),
                status: MarketStatus::Open,
                close_time: Utc::now() + chrono::Duration::days(14),
                updated_at: Utc::now(),
            },
        ];

        // Sample Kalshi markets
        let kalshi_markets = vec![
            Market {
                id: MarketId::new(),
                platform: Platform::Kalshi,
                platform_id: "KBTC-100K-DEC25".into(),
                question: "Bitcoin above $100,000 on December 31, 2025?".into(),
                yes_price: dec!(0.60),
                no_price: dec!(0.40),
                volume: dec!(350000),
                liquidity: dec!(80000),
                status: MarketStatus::Open,
                close_time: Utc::now() + chrono::Duration::days(31),
                updated_at: Utc::now(),
            },
            Market {
                id: MarketId::new(),
                platform: Platform::Kalshi,
                platform_id: "KETH-5000-DEC25".into(),
                question: "Ethereum price above $5,000 by end of 2025?".into(),
                yes_price: dec!(0.33),
                no_price: dec!(0.67),
                volume: dec!(150000),
                liquidity: dec!(40000),
                status: MarketStatus::Open,
                close_time: Utc::now() + chrono::Duration::days(58),
                updated_at: Utc::now(),
            },
            Market {
                id: MarketId::new(),
                platform: Platform::Kalshi,
                platform_id: "KFED-CUT-JUN25".into(),
                question: "Federal Reserve to cut interest rates at June meeting?".into(),
                yes_price: dec!(0.48),
                no_price: dec!(0.52),
                volume: dec!(600000),
                liquidity: dec!(250000),
                status: MarketStatus::Open,
                close_time: Utc::now() + chrono::Duration::days(15),
                updated_at: Utc::now(),
            },
            Market {
                id: MarketId::new(),
                platform: Platform::Kalshi,
                platform_id: "KTSLA-500-DEC25".into(),
                question: "Tesla stock above $500 by December 2025?".into(),
                yes_price: dec!(0.20),
                no_price: dec!(0.80),
                volume: dec!(100000),
                liquidity: dec!(30000),
                status: MarketStatus::Open,
                close_time: Utc::now() + chrono::Duration::days(45),
                updated_at: Utc::now(),
            },
        ];

        let pipeline = MatchPipeline::default();
        let candidates = pipeline.find_matches(&poly_markets, &kalshi_markets);

        // Print header
        println!();
        println!(
            "{:<7} {:<45} {:<45} {:<12}",
            "Score", "Polymarket Question", "Kalshi Question", "Decision"
        );
        println!("{}", "-".repeat(112));

        let mut reviewable = 0;
        for c in &candidates {
            let decision = c.score.decision();
            let decision_str = match &decision {
                arb_matcher::MatchDecision::AutoVerified => "Auto-OK",
                arb_matcher::MatchDecision::NeedsReview => "Review",
                arb_matcher::MatchDecision::Rejected => "Rejected",
            };

            let poly_q = if c.poly_market.question.len() > 42 {
                format!("{}...", &c.poly_market.question[..42])
            } else {
                c.poly_market.question.clone()
            };
            let kalshi_q = if c.kalshi_market.question.len() > 42 {
                format!("{}...", &c.kalshi_market.question[..42])
            } else {
                c.kalshi_market.question.clone()
            };

            println!(
                "{:<7.3} {:<45} {:<45} {:<12}",
                c.score.composite, poly_q, kalshi_q, decision_str
            );

            if decision == arb_matcher::MatchDecision::NeedsReview
                || decision == arb_matcher::MatchDecision::AutoVerified
            {
                reviewable += 1;
            }
        }

        println!();
        println!(
            "Found {} candidates ({} reviewable) from {} x {} markets",
            candidates.len(),
            reviewable,
            poly_markets.len(),
            kalshi_markets.len()
        );

        return Ok(());
    }

    // === Startup banner ===
    let mode = if args.paper { "PAPER" } else { "LIVE" };
    info!("===========================================");
    info!("  Prediction Market Arbitrage System");
    info!("  Mode: {}", mode);
    if args.paper {
        info!("  PAPER TRADING — no real orders will be placed");
    } else {
        warn!("  LIVE TRADING — real money at risk!");
    }
    info!("===========================================");

    // === Init DB ===
    let db_url = format!("sqlite://{}?mode=rwc", app_config.database.path);
    let db = Arc::new(arb_db::SqliteRepository::new(&db_url).await?);
    db.run_migrations().await?;
    info!("Database initialized");

    // === Init risk manager ===
    let rm = Arc::new(parking_lot::RwLock::new(arb_risk::RiskManager::new(app_config.risk)));
    rm.write().set_engine_running(true);

    // === Init Polymarket connector ===
    let poly_config = arb_polymarket::PolyConfig {
        api_key: std::env::var("POLY_API_KEY").expect("POLY_API_KEY not set"),
        secret: std::env::var("POLY_API_SECRET").expect("POLY_API_SECRET not set"),
        passphrase: std::env::var("POLY_PASSPHRASE").expect("POLY_PASSPHRASE not set"),
        private_key: std::env::var("POLY_PRIVATE_KEY").expect("POLY_PRIVATE_KEY not set"),
        clob_url: app_config.polymarket.clob_url.clone(),
        gamma_url: app_config.polymarket.gamma_url.clone(),
        ws_url: app_config.polymarket.ws_url.clone(),
        chain_id: 137,
    };
    let poly_real = Arc::new(
        arb_polymarket::PolymarketConnector::new(poly_config)
            .expect("failed to create Polymarket connector"),
    );
    info!("Polymarket connector initialized");

    // === Seed market pairs ===
    use arb_db::Repository;
    use arb_db::models::MarketPairRow;
    use arb_types::order::OrderBookLevel;

    // Kalshi mock state
    let kalshi_state = Arc::new(parking_lot::Mutex::new(arb_kalshi::mock::MockState::default()));
    kalshi_state.lock().balance = rust_decimal_macros::dec!(10000);

    let db_pairs = db.list_active_market_pairs().await.unwrap_or_default();
    let pairs: Vec<PairInfo> = if !db_pairs.is_empty() {
        info!(pairs = db_pairs.len(), "Loaded existing pairs from DB");
        for p in &db_pairs {
            let mut ks = kalshi_state.lock();
            ks.order_books.insert(
                p.kalshi_ticker.clone(),
                arb_types::OrderBook {
                    market_id: p.kalshi_ticker.clone(),
                    bids: vec![OrderBookLevel { price: rust_decimal_macros::dec!(0.40), quantity: 200 }],
                    asks: vec![OrderBookLevel { price: rust_decimal_macros::dec!(0.45), quantity: 200 }],
                    timestamp: Utc::now(),
                },
            );
        }
        let mut pairs_vec: Vec<PairInfo> = db_pairs.iter().map(|p| PairInfo {
            pair_id: uuid::Uuid::parse_str(&p.id).unwrap_or_else(|_| uuid::Uuid::now_v7()),
            poly_market_id: p.poly_condition_id.clone(),
            kalshi_market_id: p.kalshi_ticker.clone(),
            close_time: p.close_time,
            verified: p.verified,
            poly_yes_token_id: p.poly_yes_token_id.clone(),
            poly_no_token_id: p.poly_no_token_id.clone(),
            volume: rust_decimal::Decimal::ZERO,
        }).collect();
        // Backfill empty token IDs from bulk Gamma API
        let needs_backfill = pairs_vec.iter().any(|p| p.poly_yes_token_id.is_empty());
        if needs_backfill {
            let gamma_url = &app_config.polymarket.gamma_url;
            let http = reqwest::Client::new();
            if let Ok(resp) = http.get(format!("{gamma_url}/markets?active=true&closed=false&limit=100"))
                .send().await
            {
                if let Ok(raw) = resp.json::<Vec<serde_json::Value>>().await {
                    let mut cid_to_tokens: std::collections::HashMap<String, (String, String)> = std::collections::HashMap::new();
                    for m in &raw {
                        let cid = m["conditionId"].as_str().unwrap_or_default().to_string();
                        let clob_str = m["clobTokenIds"].as_str().unwrap_or("[]");
                        let clob_ids: Vec<String> = serde_json::from_str(clob_str).unwrap_or_default();
                        if clob_ids.len() >= 2 {
                            cid_to_tokens.insert(cid, (clob_ids[0].clone(), clob_ids[1].clone()));
                        }
                    }
                    for pair in &mut pairs_vec {
                        if pair.poly_yes_token_id.is_empty() {
                            if let Some((yes_id, no_id)) = cid_to_tokens.get(&pair.poly_market_id) {
                                pair.poly_yes_token_id = yes_id.clone();
                                pair.poly_no_token_id = no_id.clone();
                                info!(condition_id = %pair.poly_market_id, "Backfilled token IDs");
                            } else {
                                warn!(condition_id = %pair.poly_market_id, "Could not find token IDs for backfill");
                            }
                        }
                    }
                }
            }
        }
        pairs_vec
    } else {
        info!("No pairs in DB — fetching live Polymarket markets...");

        // Fetch real markets directly via HTTP (bypasses connector parsing issues)
        let gamma_url = &app_config.polymarket.gamma_url;
        let http = reqwest::Client::new();
        let resp = http.get(format!("{gamma_url}/markets?active=true&closed=false&limit=100&order=volume24hr&ascending=false"))
            .send().await;

        let mut seeded = Vec::new();
        if let Ok(resp) = resp {
            if let Ok(raw_markets) = resp.json::<Vec<serde_json::Value>>().await {
                let offsets = [
                    rust_decimal_macros::dec!(0.03), rust_decimal_macros::dec!(0.04),
                    rust_decimal_macros::dec!(0.05), rust_decimal_macros::dec!(0.03),
                    rust_decimal_macros::dec!(0.04),
                ];

                info!(total = raw_markets.len(), "Gamma API returned markets");

                let mut count = 0usize;
                for m in &raw_markets {
                    if count >= 12 { break; }

                    let condition_id = m["conditionId"].as_str().unwrap_or_default();
                    let question = m["question"].as_str().unwrap_or_default();
                    let outcome_prices_str = m["outcomePrices"].as_str().unwrap_or("[]");
                    let end_date = m["endDateIso"].as_str().unwrap_or_default();
                    let market_volume: rust_decimal::Decimal = m["volume24hr"]
                        .as_f64().or_else(|| m["volume"].as_f64()).unwrap_or(0.0)
                        .try_into().unwrap_or_default();

                    // Parse prices
                    let prices: Vec<String> = serde_json::from_str(outcome_prices_str).unwrap_or_default();
                    if prices.len() < 2 {
                        info!(question, "Skipped: no outcomePrices");
                        continue;
                    }
                    let poly_yes: rust_decimal::Decimal = prices[0].parse().unwrap_or_default();
                    let _poly_no: rust_decimal::Decimal = prices[1].parse().unwrap_or_default();

                    // Skip very extreme prices (< 5% or > 95%) — allow wider range
                    if poly_yes < rust_decimal_macros::dec!(0.05) || poly_yes > rust_decimal_macros::dec!(0.95) {
                        info!(question, yes = %poly_yes, "Skipped: price too extreme");
                        continue;
                    }
                    if condition_id.is_empty() { continue; }

                    // Extract token IDs directly from the bulk response (avoids per-market API call)
                    let (yes_token_id, no_token_id) = {
                        // Try clobTokenIds field (JSON-encoded string array)
                        let clob_str = m["clobTokenIds"].as_str().unwrap_or("[]");
                        let clob_ids: Vec<String> = serde_json::from_str(clob_str).unwrap_or_default();
                        if clob_ids.len() >= 2 {
                            (clob_ids[0].clone(), clob_ids[1].clone())
                        } else {
                            // Try tokens array fallback
                            let tokens = m["tokens"].as_array();
                            let mut yes_id = String::new();
                            let mut no_id = String::new();
                            if let Some(tokens) = tokens {
                                for t in tokens {
                                    let outcome = t["outcome"].as_str().unwrap_or_default();
                                    let tid = t["token_id"].as_str().unwrap_or_default();
                                    match outcome.to_lowercase().as_str() {
                                        "yes" => yes_id = tid.to_string(),
                                        "no" => no_id = tid.to_string(),
                                        _ => {}
                                    }
                                }
                            }
                            if yes_id.is_empty() || no_id.is_empty() {
                                warn!(condition_id, "No token IDs found in bulk response, skipping");
                                continue;
                            }
                            (yes_id, no_id)
                        }
                    };

                    let pair_id = uuid::Uuid::now_v7();
                    let kalshi_ticker = format!("KMOCK-{}", count + 1);
                    let offset = offsets[count % offsets.len()];
                    let kalshi_yes = (poly_yes - offset).max(rust_decimal_macros::dec!(0.05));
                    let kalshi_no = rust_decimal_macros::dec!(1) - kalshi_yes;
                    let close_time = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d")
                        .map(|d| d.and_hms_opt(0, 0, 0).unwrap_or_default())
                        .map(|dt| chrono::DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
                        .unwrap_or_else(|_| Utc::now() + chrono::Duration::days(30));

                    // Seed Kalshi mock
                    {
                        let mut ks = kalshi_state.lock();
                        ks.markets.push(Market {
                            id: MarketId::new(), platform: Platform::Kalshi,
                            platform_id: kalshi_ticker.clone(),
                            question: format!("[Mock] {question}"),
                            yes_price: kalshi_yes, no_price: kalshi_no,
                            volume: rust_decimal_macros::dec!(100000),
                            liquidity: rust_decimal_macros::dec!(50000),
                            status: MarketStatus::Open,
                            close_time, updated_at: Utc::now(),
                        });
                        ks.order_books.insert(kalshi_ticker.clone(), arb_types::OrderBook {
                            market_id: kalshi_ticker.clone(),
                            bids: vec![OrderBookLevel { price: kalshi_yes - rust_decimal_macros::dec!(0.02), quantity: 200 }],
                            asks: vec![OrderBookLevel { price: kalshi_yes, quantity: 200 }],
                            timestamp: Utc::now(),
                        });
                    }

                    let row = MarketPairRow {
                        id: pair_id.to_string(),
                        poly_condition_id: condition_id.to_string(),
                        poly_yes_token_id: yes_token_id.clone(), poly_no_token_id: no_token_id.clone(),
                        poly_question: question.to_string(),
                        kalshi_ticker: kalshi_ticker.clone(),
                        kalshi_question: format!("[Mock] {question}"),
                        match_confidence: 0.95, verified: true, active: true,
                        close_time, created_at: Utc::now(), updated_at: Utc::now(),
                    };
                    if let Err(e) = db.insert_market_pair(&row).await {
                        warn!(err = %e, "Failed to insert pair");
                        continue;
                    }

                    info!(
                        question,
                        poly_yes = %poly_yes,
                        kalshi_yes = %kalshi_yes,
                        spread = %offset,
                        "Seeded pair with real Polymarket market"
                    );

                    seeded.push(PairInfo {
                        pair_id, poly_market_id: condition_id.to_string(),
                        kalshi_market_id: kalshi_ticker, close_time, verified: true,
                        poly_yes_token_id: yes_token_id, poly_no_token_id: no_token_id,
                        volume: market_volume,
                    });
                    count += 1;
                }
            }
        }
        if seeded.is_empty() {
            warn!("Could not fetch Polymarket markets — check internet connection");
        } else {
            info!(pairs = seeded.len(), "Seeded pairs from live Polymarket data");
        }
        seeded
    };

    // === Create Kalshi mock connector and wrap both in PaperConnector ===
    let kalshi_state_for_feed = kalshi_state.clone();
    let kalshi_mock: Arc<dyn PredictionMarketConnector> = Arc::new(
        arb_kalshi::MockKalshiConnector::with_state(kalshi_state),
    );

    let poly_dyn: Arc<dyn PredictionMarketConnector> = poly_real.clone();
    let (poly, kalshi): (Arc<dyn PredictionMarketConnector>, Arc<dyn PredictionMarketConnector>) =
        if args.paper {
            info!("Wrapping connectors in PaperConnector (simulated trading)");
            (
                Arc::new(PaperConnector::new(poly_dyn, rust_decimal_macros::dec!(10000), 0.9, 500)),
                Arc::new(PaperConnector::new(kalshi_mock, rust_decimal_macros::dec!(10000), 0.9, 500)),
            )
        } else {
            warn!("LIVE MODE — using real connectors");
            (poly_dyn, kalshi_mock)
        };
    info!("Connectors initialized");

    // === Init engine components ===
    let engine_cfg = EngineTypesConfig {
        scan_interval_ms: app_config.engine.scan_interval_ms,
        min_spread_pct: app_config.engine.min_spread_pct.parse().unwrap_or(rust_decimal_macros::dec!(3.0)),
        min_spread_absolute: app_config.engine.min_spread_absolute.parse().unwrap_or(rust_decimal_macros::dec!(0.02)),
    };
    let order_cfg = OrderConfig {
        max_order_age_secs: app_config.orders.max_order_age_secs,
        max_hedge_wait_secs: app_config.orders.max_hedge_wait_secs,
        order_check_interval_ms: app_config.orders.order_check_interval_ms,
        min_repost_spread: app_config.orders.min_repost_spread.parse().unwrap_or(rust_decimal_macros::dec!(0.02)),
        price_improve_amount: app_config.orders.price_improve_amount.parse().unwrap_or(rust_decimal_macros::dec!(0.01)),
        default_quantity: app_config.orders.default_quantity,
    };

    let price_cache = Arc::new(PriceCache::new());
    for p in &pairs {
        let poly_cache_id = if !p.poly_yes_token_id.is_empty() { &p.poly_yes_token_id } else { &p.poly_market_id };
        price_cache.register_pair(p.pair_id, poly_cache_id, &p.kalshi_market_id);
    }

    let executor = Executor::new(poly.clone(), kalshi.clone(), rm.clone(), db.clone(), order_cfg.clone());
    let monitor = Monitor::new(poly.clone(), kalshi.clone(), order_cfg);
    let tracker = Tracker::new(db.clone(), rm.clone());
    let unwinder = Unwinder::new(poly.clone(), kalshi.clone(), rm.clone());

    let engine = Arc::new(Engine::new(
        poly.clone(), kalshi.clone(), price_cache.clone(),
        executor, monitor, tracker, unwinder, engine_cfg,
    ));
    info!("Engine initialized");

    // === Price feeds ===
    let (price_tx, price_rx) = tokio::sync::mpsc::channel(256);

    // Polymarket WS subscription
    let poly_token_ids: Vec<String> = pairs.iter()
        .map(|p| p.poly_yes_token_id.clone())
        .filter(|id| !id.is_empty())
        .collect();

    if !poly_token_ids.is_empty() {
        match poly.subscribe_prices(&poly_token_ids, price_tx.clone()).await {
            Ok(handle) => {
                info!(count = poly_token_ids.len(), "Polymarket WS price feed started");
                std::mem::forget(handle);
            }
            Err(e) => {
                warn!(err = %e, "Failed to start Polymarket WS feed");
            }
        }
    }

    // Polymarket REST polling — always runs as reliable price source
    {
        let poly_poll_pairs: Vec<(String, String)> = pairs.iter()
            .filter(|p| !p.poly_yes_token_id.is_empty())
            .map(|p| (p.poly_yes_token_id.clone(), p.poly_market_id.clone()))
            .collect();
        if !poly_poll_pairs.is_empty() {
            let gamma_url = app_config.polymarket.gamma_url.clone();
            let ptx = price_tx.clone();
            tokio::spawn(async move {
                let http = reqwest::Client::new();
                loop {
                    let url = format!("{}/markets?active=true&closed=false&limit=100&order=volume24hr&ascending=false", gamma_url);
                    if let Ok(resp) = http.get(&url).send().await {
                        if let Ok(raw) = resp.json::<Vec<serde_json::Value>>().await {
                            // Build condition_id → yes_price map from Gamma API
                            let mut cid_prices: std::collections::HashMap<String, rust_decimal::Decimal> = std::collections::HashMap::new();
                            for m in &raw {
                                let cid = m["conditionId"].as_str().unwrap_or_default();
                                let op = m["outcomePrices"].as_str().unwrap_or("[]");
                                if let Ok(prices) = serde_json::from_str::<Vec<String>>(op) {
                                    if prices.len() >= 2 {
                                        if let Ok(yes) = prices[0].parse::<rust_decimal::Decimal>() {
                                            cid_prices.insert(cid.to_string(), yes);
                                        }
                                    }
                                }
                            }
                            // Send updates keyed by token ID (matches price cache registration)
                            for (token_id, condition_id) in &poly_poll_pairs {
                                if let Some(&py) = cid_prices.get(condition_id) {
                                    let _ = ptx.send(arb_types::PriceUpdate {
                                        platform: Platform::Polymarket,
                                        market_id: token_id.clone(),
                                        yes_price: py,
                                        no_price: rust_decimal_macros::dec!(1) - py,
                                        timestamp: Utc::now(),
                                    }).await;
                                }
                            }
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(8)).await;
                }
            });
            info!("Polymarket REST polling fallback started (8s interval)");
        }
    }

    // Kalshi mock price feed
    {
        let kalshi_feed_pairs: Vec<(String, rust_decimal::Decimal)> = {
            let ks = kalshi_state_for_feed.lock();
            pairs.iter().filter_map(|p| {
                let km = ks.markets.iter().find(|m| m.platform_id == p.kalshi_market_id)?;
                Some((p.kalshi_market_id.clone(), km.yes_price))
            }).collect()
        };
        if !kalshi_feed_pairs.is_empty() {
            let ptx = price_tx.clone();
            tokio::spawn(async move {
                loop {
                    for (kalshi_id, kalshi_base) in &kalshi_feed_pairs {
                        // Static price — no jitter. Kalshi mock stays at the seeded price.
                        let _ = ptx.send(arb_types::PriceUpdate {
                            platform: Platform::Kalshi,
                            market_id: kalshi_id.clone(),
                            yes_price: *kalshi_base,
                            no_price: rust_decimal_macros::dec!(1) - *kalshi_base,
                            timestamp: Utc::now(),
                        }).await;
                    }
                    tokio::time::sleep(Duration::from_secs(8)).await;
                }
            });
            info!("Kalshi mock price feed started");
        }
    }

    // === Start engine ===
    let engine_handle = engine.clone();
    let engine_pairs = pairs.clone();
    let pair_count = pairs.len();
    tokio::spawn(async move {
        if let Err(e) = engine_handle.run(price_rx, engine_pairs).await {
            error!("Engine error: {e}");
        }
    });
    info!(pairs = pair_count, "Engine running");

    // === Setup Ctrl+C handler ===
    let shutdown_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sf = shutdown_flag.clone();
    let shutdown_engine = engine.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("failed to listen for ctrl+c");
        info!("Ctrl+C received — initiating shutdown");
        sf.store(true, std::sync::atomic::Ordering::SeqCst);
        shutdown_engine.shutdown().await;
    });

    // === Health file writer ===
    let health_db = db.clone();
    let health_rm = rm.clone();
    let health_flag = shutdown_flag.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            if health_flag.load(std::sync::atomic::Ordering::SeqCst) { break; }
            write_health_file(&health_db, &health_rm, mode).await;
        }
    });

    // === TUI or headless ===
    if args.tui || (!args.headless && !args.paper) {
        // Set panic hook to restore terminal on panic
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
            original_hook(panic_info);
        }));

        tui::run_tui(db.clone(), rm.clone(), mode, pair_count, price_cache.clone(), pairs.clone()).await?;
    } else {
        // Headless: just wait for Ctrl+C
        info!("Running headless — press Ctrl+C to stop");
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if shutdown_flag.load(std::sync::atomic::Ordering::SeqCst) { break; }
        }
    }

    info!("Shutdown complete");
    Ok(())
}

async fn write_health_file(
    db: &arb_db::SqliteRepository,
    rm: &parking_lot::RwLock<arb_risk::RiskManager>,
    mode: &str,
) {
    use arb_db::Repository;
    use std::io::Write;

    let exposure = rm.read().exposure().total_exposure();
    let daily_loss = rm.read().exposure().daily_loss();
    let open_orders = db.list_orders_by_status("open").await.map(|o| o.len()).unwrap_or(0);
    let positions = db.list_open_positions().await.map(|p| p.len()).unwrap_or(0);

    let health = serde_json::json!({
        "status": "running",
        "mode": mode,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "open_orders": open_orders,
        "open_positions": positions,
        "total_exposure": exposure.to_string(),
        "daily_loss": daily_loss.to_string(),
    });

    // Write atomically (write to tmp, rename)
    let path = std::path::Path::new("data/health.json");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp_path = path.with_extension("tmp");
    if let Ok(mut f) = std::fs::File::create(&tmp_path) {
        if f.write_all(serde_json::to_string_pretty(&health).unwrap_or_default().as_bytes()).is_ok() {
            let _ = std::fs::rename(&tmp_path, path);
        }
    }
}
