use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use rust_decimal_macros::dec;
use serde::Deserialize;
use tracing::{error, info, warn};

use std::sync::Arc;
use std::time::Duration;

use arb_engine::engine::Engine;
use arb_engine::executor::Executor;
use arb_engine::fees::FeeConfig;
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
    /// Hybrid paper mode: real Kalshi (demo or prod) + simulated Polymarket orders.
    /// This is the recommended testing mode.
    #[arg(long)]
    paper: bool,

    /// Full paper mode: wrap BOTH connectors in PaperConnector (all orders simulated).
    #[arg(long)]
    paper_both: bool,

    /// Use Kalshi demo sandbox (demo-api.kalshi.co) instead of production.
    /// This is the DEFAULT — production requires --production flag.
    #[arg(long)]
    demo: bool,

    /// Use Kalshi PRODUCTION API. Requires explicit opt-in.
    /// WARNING: Real money at risk!
    #[arg(long)]
    production: bool,

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
    fees: FeesConfig,
    polymarket: PlatformConfig,
    kalshi: KalshiTomlConfig,
    matcher: MatcherTomlConfig,
    database: DatabaseConfig,
    logging: LoggingConfig,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MatcherTomlConfig {
    quality_gate: f64,
    text_similarity_floor: f64,
    auto_verified_threshold: f64,
    needs_review_floor: f64,
    max_discovered_pairs: usize,
    #[serde(default)]
    auto_verify_enabled: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FeesConfig {
    kalshi_taker_fee_pct: String,
    poly_taker_fee_pct: String,
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
struct KalshiTomlConfig {
    base_url: String,
    ws_url: String,
    demo_base_url: String,
    demo_ws_url: String,
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
    let tui_active = args.tui || (!args.headless && !args.paper && !args.paper_both);
    let _ = std::fs::create_dir_all("data");
    init_tracing(&app_config.logging, tui_active);

    info!("Starting arb system");
    info!(paper = args.paper, tui = args.tui, headless = args.headless, match_only = args.r#match);

    if args.r#match {
        info!("Match-only mode — fetching real markets from APIs");

        let matcher_cfg = &app_config.matcher;

        // --- Open DB for discovery data persistence ---
        let db_url = format!("sqlite://{}?mode=rwc", app_config.database.path);
        let db = arb_db::SqliteRepository::new(&db_url).await?;
        db.run_migrations().await?;
        let pool = db.pool();
        let run_id = uuid::Uuid::now_v7().to_string();
        let ran_at = Utc::now().to_rfc3339();
        info!(run_id = %run_id, "Discovery run started");

        // Track raw/filtered markets for DB persistence (including filtered-out ones)
        struct RawMarketRecord {
            platform: String,
            platform_id: String,
            question: String,
            yes_price: Option<f64>,
            no_price: Option<f64>,
            volume: Option<f64>,
            close_time: Option<String>,
            category: String,
            status: String,        // "filtered_in" or "filtered_out"
            filter_reason: Option<String>, // "price", "volume", "close_time", "short_title", "missing_data"
        }
        let mut all_market_records: Vec<RawMarketRecord> = Vec::new();

        // --- Fetch Polymarket markets (public API, no auth) with pagination ---
        println!("Fetching Polymarket markets from Gamma API (paginated)...");
        let gamma_url = &app_config.polymarket.gamma_url;
        let http = reqwest::Client::new();
        let mut raw_poly: Vec<serde_json::Value> = Vec::new();
        for page in 0u32..3 {
            let offset = page * 200;
            let url = format!(
                "{gamma_url}/markets?active=true&closed=false&limit=200&offset={offset}&order=volume24hr&ascending=false"
            );
            let poly_resp = http.get(&url).send().await;
            let page_markets: Vec<serde_json::Value> = match poly_resp {
                Ok(resp) => resp.json::<Vec<serde_json::Value>>().await.unwrap_or_default(),
                Err(e) => {
                    if page == 0 {
                        eprintln!("Failed to fetch Polymarket markets: {e}");
                        return Ok(());
                    }
                    eprintln!("Failed to fetch Polymarket page {}: {e}", page + 1);
                    break;
                }
            };
            let count = page_markets.len();
            println!("  Gamma page {}: {} markets (offset={})", page + 1, count, offset);
            raw_poly.extend(page_markets);
            if count < 200 { break; } // Last page — fewer results than limit
            if page < 2 {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        // Apply the same filters as the engine startup path
        let mut poly_markets: Vec<Market> = Vec::new();
        let mut filtered_by_price: usize = 0;
        let mut filtered_by_volume: usize = 0;
        let mut filtered_by_close_time: usize = 0;

        // Helper: classify a question for category label
        fn classify_category_label(question: &str) -> String {
            let ct = arb_matcher::normalize::classify_tokens(question);
            let (cat, _) = arb_matcher::category::classify(&ct.all_meaningful);
            match cat {
                arb_matcher::MarketCategory::Crypto => "Crypto",
                arb_matcher::MarketCategory::Politics => "Politics",
                arb_matcher::MarketCategory::Sports => "Sports",
                arb_matcher::MarketCategory::Weather => "Weather",
                arb_matcher::MarketCategory::Economics => "Economics",
                arb_matcher::MarketCategory::Entertainment => "Entertainment",
                arb_matcher::MarketCategory::Science => "Science",
                arb_matcher::MarketCategory::Other => "Other",
            }.to_string()
        }

        for m in &raw_poly {
            let condition_id = m["conditionId"].as_str().unwrap_or_default();
            let question = m["question"].as_str().unwrap_or_default();
            let outcome_prices_str = m["outcomePrices"].as_str().unwrap_or("[]");
            let end_date = m["endDateIso"].as_str().unwrap_or_default();
            let market_volume: rust_decimal::Decimal = m["volume24hr"]
                .as_f64().or_else(|| m["volume"].as_f64()).unwrap_or(0.0)
                .try_into().unwrap_or_default();

            let prices: Vec<String> = serde_json::from_str(outcome_prices_str).unwrap_or_default();
            if prices.len() < 2 {
                all_market_records.push(RawMarketRecord {
                    platform: "polymarket".into(),
                    platform_id: condition_id.to_string(),
                    question: question.to_string(),
                    yes_price: None,
                    no_price: None,
                    volume: Some(market_volume.to_string().parse().unwrap_or(0.0)),
                    close_time: Some(end_date.to_string()),
                    category: classify_category_label(question),
                    status: "filtered_out".into(),
                    filter_reason: Some("missing_data".into()),
                });
                continue;
            }
            let poly_yes: rust_decimal::Decimal = prices[0].parse().unwrap_or_default();
            let poly_no: rust_decimal::Decimal = prices[1].parse().unwrap_or_default();

            let cat_label = classify_category_label(question);

            // Price filter: reject near-decided markets (widened to capture crypto brackets)
            if poly_yes < dec!(0.05) || poly_yes > dec!(0.95) {
                filtered_by_price += 1;
                all_market_records.push(RawMarketRecord {
                    platform: "polymarket".into(),
                    platform_id: condition_id.to_string(),
                    question: question.to_string(),
                    yes_price: Some(poly_yes.to_string().parse().unwrap_or(0.0)),
                    no_price: Some(poly_no.to_string().parse().unwrap_or(0.0)),
                    volume: Some(market_volume.to_string().parse().unwrap_or(0.0)),
                    close_time: Some(end_date.to_string()),
                    category: cat_label,
                    status: "filtered_out".into(),
                    filter_reason: Some("price".into()),
                });
                continue;
            }
            if condition_id.is_empty() { continue; }

            // Minimum volume filter (lowered to capture crypto bracket sub-markets)
            if market_volume < dec!(1000) {
                filtered_by_volume += 1;
                all_market_records.push(RawMarketRecord {
                    platform: "polymarket".into(),
                    platform_id: condition_id.to_string(),
                    question: question.to_string(),
                    yes_price: Some(poly_yes.to_string().parse().unwrap_or(0.0)),
                    no_price: Some(poly_no.to_string().parse().unwrap_or(0.0)),
                    volume: Some(market_volume.to_string().parse().unwrap_or(0.0)),
                    close_time: Some(end_date.to_string()),
                    category: cat_label,
                    status: "filtered_out".into(),
                    filter_reason: Some("volume".into()),
                });
                continue;
            }

            let close_time = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d")
                .map(|d| d.and_hms_opt(0, 0, 0).unwrap_or_default())
                .map(|dt| chrono::DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
                .unwrap_or_else(|_| Utc::now() + chrono::Duration::days(30));

            // Close-time filter: skip events closing within 6 hours
            let hours_to_close = (close_time - Utc::now()).num_hours();
            if hours_to_close < 6 {
                filtered_by_close_time += 1;
                all_market_records.push(RawMarketRecord {
                    platform: "polymarket".into(),
                    platform_id: condition_id.to_string(),
                    question: question.to_string(),
                    yes_price: Some(poly_yes.to_string().parse().unwrap_or(0.0)),
                    no_price: Some(poly_no.to_string().parse().unwrap_or(0.0)),
                    volume: Some(market_volume.to_string().parse().unwrap_or(0.0)),
                    close_time: Some(close_time.to_rfc3339()),
                    category: cat_label,
                    status: "filtered_out".into(),
                    filter_reason: Some("close_time".into()),
                });
                continue;
            }

            // Passed all filters
            all_market_records.push(RawMarketRecord {
                platform: "polymarket".into(),
                platform_id: condition_id.to_string(),
                question: question.to_string(),
                yes_price: Some(poly_yes.to_string().parse().unwrap_or(0.0)),
                no_price: Some(poly_no.to_string().parse().unwrap_or(0.0)),
                volume: Some(market_volume.to_string().parse().unwrap_or(0.0)),
                close_time: Some(close_time.to_rfc3339()),
                category: cat_label,
                status: "filtered_in".into(),
                filter_reason: None,
            });

            poly_markets.push(Market {
                id: MarketId::new(),
                platform: Platform::Polymarket,
                platform_id: condition_id.to_string(),
                question: question.to_string(),
                yes_price: poly_yes,
                no_price: poly_no,
                volume: market_volume,
                liquidity: rust_decimal::Decimal::ZERO,
                status: MarketStatus::Open,
                close_time,
                updated_at: Utc::now(),
            });
        }
        println!("  Polymarket: {} markets after filtering ({} raw)", poly_markets.len(), raw_poly.len());
        println!("  Polymarket filter breakdown: {} price, {} volume, {} close-time",
            filtered_by_price, filtered_by_volume, filtered_by_close_time);

        // --- Fetch Kalshi markets (needs auth) — both /markets and /events ---
        let (kalshi_markets_raw, kalshi_from_markets, kalshi_from_events, kalshi_events_count): (Vec<Market>, usize, usize, usize) =
            match try_fetch_kalshi_markets(&app_config.kalshi).await {
                Ok(result) => {
                    println!(
                        "  Kalshi: {} markets total ({} from /markets + {} from /events, {} events fetched)",
                        result.markets.len(), result.from_markets_endpoint,
                        result.from_events_endpoint, result.events_fetched,
                    );
                    (result.markets, result.from_markets_endpoint, result.from_events_endpoint, result.events_fetched)
                }
                Err(e) => {
                    eprintln!("  Kalshi fetch failed ({e}) — showing Polymarket markets only");
                    (Vec::new(), 0, 0, 0)
                }
            };

        // Record all raw Kalshi markets, then filter
        let kalshi_raw_count = kalshi_markets_raw.len();
        let mut kalshi_markets: Vec<Market> = Vec::new();
        for m in kalshi_markets_raw {
            let cat_label = classify_category_label(&m.question);
            // Skip very short titles (novelty markets like "1+1 = 3")
            if m.question.len() < 15 {
                all_market_records.push(RawMarketRecord {
                    platform: "kalshi".into(),
                    platform_id: m.platform_id.clone(),
                    question: m.question.clone(),
                    yes_price: Some(m.yes_price.to_string().parse().unwrap_or(0.0)),
                    no_price: Some(m.no_price.to_string().parse().unwrap_or(0.0)),
                    volume: Some(m.volume.to_string().parse().unwrap_or(0.0)),
                    close_time: Some(m.close_time.to_rfc3339()),
                    category: cat_label,
                    status: "filtered_out".into(),
                    filter_reason: Some("short_title".into()),
                });
                continue;
            }
            all_market_records.push(RawMarketRecord {
                platform: "kalshi".into(),
                platform_id: m.platform_id.clone(),
                question: m.question.clone(),
                yes_price: Some(m.yes_price.to_string().parse().unwrap_or(0.0)),
                no_price: Some(m.no_price.to_string().parse().unwrap_or(0.0)),
                volume: Some(m.volume.to_string().parse().unwrap_or(0.0)),
                close_time: Some(m.close_time.to_rfc3339()),
                category: cat_label,
                status: "filtered_in".into(),
                filter_reason: None,
            });
            kalshi_markets.push(m);
        }
        println!("  Kalshi: {} markets after filtering ({} raw)", kalshi_markets.len(), kalshi_raw_count);

        // --- Run the matcher ---
        if kalshi_markets.is_empty() {
            println!();
            println!("=== Polymarket Markets (no Kalshi data for matching) ===");
            println!();
            println!(
                "{:<45} {:<8} {:<10} {:<12}",
                "Question", "Yes", "Volume", "Close"
            );
            println!("{}", "-".repeat(78));
            for m in poly_markets.iter().take(30) {
                let q = if m.question.len() > 42 {
                    format!("{}...", &m.question[..42])
                } else {
                    m.question.clone()
                };
                let close_days = (m.close_time - Utc::now()).num_days();
                println!(
                    "{:<45} {:<8} ${:<9} {}d",
                    q,
                    m.yes_price,
                    m.volume,
                    close_days
                );
            }
            println!();
            println!(
                "{} Polymarket markets available. Set KALSHI_API_KEY_ID and KALSHI_PRIVATE_KEY_PATH to enable cross-platform matching.",
                poly_markets.len()
            );
            return Ok(());
        }

        // Show sample markets from each platform for diagnostics
        println!("\n--- Polymarket sample (first 10) ---");
        for (i, m) in poly_markets.iter().take(10).enumerate() {
            let cat = arb_matcher::category::classify(
                &arb_matcher::normalize::classify_tokens(&m.question).all_meaningful,
            );
            println!("  {:>2}. [{}] {}", i + 1,
                match cat.0 {
                    arb_matcher::MarketCategory::Crypto => "CRYPTO",
                    arb_matcher::MarketCategory::Politics => "POLIT ",
                    arb_matcher::MarketCategory::Sports => "SPORT ",
                    arb_matcher::MarketCategory::Weather => "WEATH ",
                    arb_matcher::MarketCategory::Economics => "ECON  ",
                    arb_matcher::MarketCategory::Entertainment => "ENTER ",
                    arb_matcher::MarketCategory::Science => "SCI   ",
                    arb_matcher::MarketCategory::Other => "OTHER ",
                },
                if m.question.len() > 80 { format!("{}...", &m.question[..80]) } else { m.question.clone() },
            );
        }
        println!("\n--- Kalshi sample (first 10) ---");
        for (i, m) in kalshi_markets.iter().take(10).enumerate() {
            let cat = arb_matcher::category::classify(
                &arb_matcher::normalize::classify_tokens(&m.question).all_meaningful,
            );
            println!("  {:>2}. [{}] {}", i + 1,
                match cat.0 {
                    arb_matcher::MarketCategory::Crypto => "CRYPTO",
                    arb_matcher::MarketCategory::Politics => "POLIT ",
                    arb_matcher::MarketCategory::Sports => "SPORT ",
                    arb_matcher::MarketCategory::Weather => "WEATH ",
                    arb_matcher::MarketCategory::Economics => "ECON  ",
                    arb_matcher::MarketCategory::Entertainment => "ENTER ",
                    arb_matcher::MarketCategory::Science => "SCI   ",
                    arb_matcher::MarketCategory::Other => "OTHER ",
                },
                if m.question.len() > 80 { format!("{}...", &m.question[..80]) } else { m.question.clone() },
            );
        }

        // Show category distribution
        let mut poly_cats: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for m in &poly_markets {
            let cat = arb_matcher::category::classify(
                &arb_matcher::normalize::classify_tokens(&m.question).all_meaningful,
            );
            let label = match cat.0 {
                arb_matcher::MarketCategory::Crypto => "Crypto",
                arb_matcher::MarketCategory::Politics => "Politics",
                arb_matcher::MarketCategory::Sports => "Sports",
                arb_matcher::MarketCategory::Weather => "Weather",
                arb_matcher::MarketCategory::Economics => "Economics",
                arb_matcher::MarketCategory::Entertainment => "Entertainment",
                arb_matcher::MarketCategory::Science => "Science",
                arb_matcher::MarketCategory::Other => "Other",
            };
            *poly_cats.entry(label).or_default() += 1;
        }
        let mut kalshi_cats: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for m in &kalshi_markets {
            let cat = arb_matcher::category::classify(
                &arb_matcher::normalize::classify_tokens(&m.question).all_meaningful,
            );
            let label = match cat.0 {
                arb_matcher::MarketCategory::Crypto => "Crypto",
                arb_matcher::MarketCategory::Politics => "Politics",
                arb_matcher::MarketCategory::Sports => "Sports",
                arb_matcher::MarketCategory::Weather => "Weather",
                arb_matcher::MarketCategory::Economics => "Economics",
                arb_matcher::MarketCategory::Entertainment => "Entertainment",
                arb_matcher::MarketCategory::Science => "Science",
                arb_matcher::MarketCategory::Other => "Other",
            };
            *kalshi_cats.entry(label).or_default() += 1;
        }
        println!("\n--- Category distribution ---");
        println!("  {:>15}  {:>5}  {:>5}", "Category", "Poly", "Kalshi");
        let all_cats = ["Crypto", "Politics", "Sports", "Weather", "Economics", "Entertainment", "Science", "Other"];
        for cat in &all_cats {
            let pc = poly_cats.get(cat).unwrap_or(&0);
            let kc = kalshi_cats.get(cat).unwrap_or(&0);
            if *pc > 0 || *kc > 0 {
                let overlap = if *pc > 0 && *kc > 0 { " <-- overlap" } else { "" };
                println!("  {:>15}  {:>5}  {:>5}{}", cat, pc, kc, overlap);
            }
        }

        // Show overlap categories with samples for diagnostic purposes
        for diag_cat in &["Sports", "Crypto", "Other"] {
            let target_cat = match *diag_cat {
                "Sports" => arb_matcher::MarketCategory::Sports,
                "Crypto" => arb_matcher::MarketCategory::Crypto,
                _ => arb_matcher::MarketCategory::Other,
            };
            let poly_in_cat: Vec<&Market> = poly_markets.iter().filter(|m| {
                arb_matcher::category::classify(
                    &arb_matcher::normalize::classify_tokens(&m.question).all_meaningful,
                ).0 == target_cat
            }).collect();
            let kalshi_in_cat: Vec<&Market> = kalshi_markets.iter().filter(|m| {
                arb_matcher::category::classify(
                    &arb_matcher::normalize::classify_tokens(&m.question).all_meaningful,
                ).0 == target_cat
            }).collect();
            if !poly_in_cat.is_empty() && !kalshi_in_cat.is_empty() {
                println!("\n--- {} overlap: {} Poly x {} Kalshi (sample 5 each) ---", diag_cat, poly_in_cat.len(), kalshi_in_cat.len());
                for (i, m) in poly_in_cat.iter().take(5).enumerate() {
                    let q = if m.question.len() > 70 { format!("{}...", &m.question[..70]) } else { m.question.clone() };
                    println!("  P{}: {}", i + 1, q);
                }
                for (i, m) in kalshi_in_cat.iter().take(5).enumerate() {
                    let q = if m.question.len() > 70 { format!("{}...", &m.question[..70]) } else { m.question.clone() };
                    println!("  K{}: {}", i + 1, q);
                }
            }
        }

        let pipeline = MatchPipeline::default();
        let diag = pipeline.find_matches_diagnostic(&poly_markets, &kalshi_markets, 100);
        let candidates = &diag.candidates;

        // Print header
        println!();
        println!(
            "{:<7} {:<6} {:<6} {:<45} {:<45} {:<12}",
            "Score", "Text", "Time", "Polymarket Question", "Kalshi Question", "Decision"
        );
        println!("{}", "-".repeat(124));

        let mut auto_verified = 0;
        let mut needs_review = 0;
        let mut would_pass_gate = 0;
        for c in candidates {
            let decision = c.score.decision();
            let decision_str = match &decision {
                arb_matcher::MatchDecision::AutoVerified => "Auto-OK",
                arb_matcher::MatchDecision::NeedsReview => "Review",
                arb_matcher::MatchDecision::Rejected => "Rejected",
            };

            // Check if this candidate would pass the quality gate
            let passes_gate = c.score.composite >= matcher_cfg.quality_gate
                && c.score.text_similarity >= matcher_cfg.text_similarity_floor;
            if passes_gate {
                would_pass_gate += 1;
            }

            let gate_marker = if passes_gate { "*" } else { " " };

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
                "{:<7.3} {:<6.3} {:<6.3} {:<45} {:<45} {:<12}{}",
                c.score.composite, c.score.text_similarity, c.score.close_time_score,
                poly_q, kalshi_q, decision_str, gate_marker
            );

            match decision {
                arb_matcher::MatchDecision::AutoVerified => auto_verified += 1,
                arb_matcher::MatchDecision::NeedsReview => needs_review += 1,
                arb_matcher::MatchDecision::Rejected => {}
            }
        }

        println!();
        println!("Summary:");
        println!("  Markets: {} Polymarket x {} Kalshi", poly_markets.len(), kalshi_markets.len());
        if kalshi_from_events > 0 || kalshi_events_count > 0 {
            println!("  Kalshi sources: {} from /markets + {} unique from /events ({} events fetched)",
                kalshi_from_markets, kalshi_from_events, kalshi_events_count);
        }
        println!("  Candidates: {} total ({} auto-verified, {} needs-review)", candidates.len(), auto_verified, needs_review);
        println!("  Would pass quality gate (>={:.2} composite, >={:.2} text): {}",
            matcher_cfg.quality_gate, matcher_cfg.text_similarity_floor, would_pass_gate);
        println!("  (* = passes quality gate)");

        // --- Save discovery data to DB ---
        println!("\nSaving discovery data to database...");

        // 1. Insert discovery_runs row
        sqlx::query(
            "INSERT INTO discovery_runs (id, ran_at, poly_raw_count, poly_filtered_count, kalshi_raw_count, kalshi_filtered_count, candidates_found, matches_found)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
        )
        .bind(&run_id)
        .bind(&ran_at)
        .bind(raw_poly.len() as i64)
        .bind(poly_markets.len() as i64)
        .bind(kalshi_raw_count as i64)
        .bind(kalshi_markets.len() as i64)
        .bind(diag.comparisons.iter().filter(|c| c.blocked_by.is_none()).count() as i64)
        .bind(candidates.len() as i64)
        .execute(pool)
        .await
        .context("Failed to insert discovery_runs")?;

        // 2. Insert all market records in batches
        let discovered_at = Utc::now().to_rfc3339();
        let mut market_count = 0usize;
        for record in &all_market_records {
            let market_id = uuid::Uuid::now_v7().to_string();
            sqlx::query(
                "INSERT INTO discovery_markets (id, platform, platform_id, question, yes_price, no_price, volume, close_time, category, status, filter_reason, discovered_at, run_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"
            )
            .bind(&market_id)
            .bind(&record.platform)
            .bind(&record.platform_id)
            .bind(&record.question)
            .bind(record.yes_price)
            .bind(record.no_price)
            .bind(record.volume)
            .bind(&record.close_time)
            .bind(&record.category)
            .bind(&record.status)
            .bind(&record.filter_reason)
            .bind(&discovered_at)
            .bind(&run_id)
            .execute(pool)
            .await
            .context("Failed to insert discovery_markets")?;
            market_count += 1;
        }
        println!("  Saved {} market records", market_count);

        // 3. Insert comparison records
        let mut comp_count = 0usize;
        for comp in &diag.comparisons {
            let comp_id = uuid::Uuid::now_v7().to_string();
            sqlx::query(
                "INSERT INTO discovery_comparisons (id, run_id, poly_platform_id, kalshi_platform_id, poly_question, kalshi_question, poly_category, kalshi_category, blocked_by, composite_score, text_score, time_score, shared_entities, shared_tokens)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"
            )
            .bind(&comp_id)
            .bind(&run_id)
            .bind(&comp.poly_platform_id)
            .bind(&comp.kalshi_platform_id)
            .bind(&comp.poly_question)
            .bind(&comp.kalshi_question)
            .bind(&comp.poly_category)
            .bind(&comp.kalshi_category)
            .bind(&comp.blocked_by)
            .bind(comp.composite_score)
            .bind(comp.text_score)
            .bind(comp.time_score)
            .bind(comp.shared_entities.map(|v| v as i64))
            .bind(comp.shared_tokens.map(|v| v as i64))
            .execute(pool)
            .await
            .context("Failed to insert discovery_comparisons")?;
            comp_count += 1;
        }
        println!("  Saved {} comparison records", comp_count);
        println!("  Run ID: {}", run_id);
        println!("  DB path: {}", app_config.database.path);

        return Ok(());
    }

    // === Validate flags ===
    if args.production && args.demo {
        anyhow::bail!("Cannot use --production and --demo at the same time");
    }
    if args.paper && args.paper_both {
        anyhow::bail!("Cannot use --paper and --paper-both at the same time. Use --paper for hybrid mode or --paper-both for full simulation.");
    }

    // Default to production unless --demo is explicitly set
    let use_demo = args.demo;

    // === Derive trading mode for DB tagging ===
    let trading_mode: String = if args.paper_both || args.paper {
        "paper".into()
    } else if use_demo {
        "demo".into()
    } else {
        "production".into()
    };

    // === Startup banner ===
    let mode = if args.paper_both {
        "PAPER-BOTH"
    } else if args.paper {
        "PAPER-HYBRID"
    } else {
        "LIVE"
    };
    let kalshi_env = if use_demo { "DEMO" } else { "PRODUCTION" };
    info!("===========================================");
    info!("  Prediction Market Arbitrage System");
    info!("  Mode: {}", mode);
    info!("  Kalshi: {}", kalshi_env);
    if args.paper_both {
        info!("  FULL PAPER — all orders simulated on both platforms");
    } else if args.paper {
        info!("  HYBRID PAPER — real Kalshi ({}) + simulated Polymarket", kalshi_env);
    } else if use_demo {
        info!("  LIVE on Kalshi DEMO sandbox + real Polymarket");
    } else {
        warn!("  *** LIVE TRADING — REAL MONEY AT RISK ***");
    }
    info!("===========================================");

    // === Init DB ===
    let db_url = format!("sqlite://{}?mode=rwc", app_config.database.path);
    let db = Arc::new(arb_db::SqliteRepository::new(&db_url).await?);
    db.run_migrations().await?;
    info!("Database initialized");

    // === Load manually configured pairs from config/pairs.toml ===
    let pair_store = arb_matcher::PairStore::new(db.clone());
    match pair_store.load_from_toml("config/pairs.toml").await {
        Ok(n) => info!(inserted = n, "Loaded pairs from config/pairs.toml"),
        Err(e) => warn!(err = %e, "Could not load pairs.toml (file may not exist)"),
    }

    // === Init risk manager ===
    let rm = Arc::new(parking_lot::RwLock::new(arb_risk::RiskManager::new(app_config.risk)));
    rm.write().set_engine_running(true);

    // === Init Polymarket connector ===
    let poly_config = arb_polymarket::PolyConfig {
        api_key: std::env::var("POLY_API_KEY").context("POLY_API_KEY env var not set")?,
        secret: std::env::var("POLY_API_SECRET").context("POLY_API_SECRET env var not set")?,
        passphrase: std::env::var("POLY_PASSPHRASE").context("POLY_PASSPHRASE env var not set")?,
        private_key: std::env::var("POLY_PRIVATE_KEY").context("POLY_PRIVATE_KEY env var not set")?,
        clob_url: app_config.polymarket.clob_url.clone(),
        gamma_url: app_config.polymarket.gamma_url.clone(),
        ws_url: app_config.polymarket.ws_url.clone(),
        chain_id: 137,
    };
    let poly_real = Arc::new(
        arb_polymarket::PolymarketConnector::new(poly_config)
            .context("failed to create Polymarket connector")?,
    );
    info!("Polymarket connector initialized");

    // === Init Kalshi connector (real) ===
    let (kalshi_base_url, kalshi_ws_url) = if use_demo {
        info!("Using Kalshi DEMO sandbox");
        (
            app_config.kalshi.demo_base_url.clone(),
            app_config.kalshi.demo_ws_url.clone(),
        )
    } else {
        warn!("Using Kalshi PRODUCTION API — real money at risk!");
        (
            app_config.kalshi.base_url.clone(),
            app_config.kalshi.ws_url.clone(),
        )
    };

    // Read Kalshi credentials from env vars
    let kalshi_api_key_id = std::env::var("KALSHI_API_KEY_ID")
        .context("KALSHI_API_KEY_ID env var not set — required for Kalshi connector")?;
    let kalshi_private_key_pem = if let Ok(path) = std::env::var("KALSHI_PRIVATE_KEY_PATH") {
        std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read Kalshi private key from {path}"))?
    } else {
        let raw = std::env::var("KALSHI_PRIVATE_KEY_PEM")
            .context("Set KALSHI_PRIVATE_KEY_PATH (path to .pem file) or KALSHI_PRIVATE_KEY_PEM (raw base64/PEM string)")?;
        // Handle literal \n in .env files
        raw.replace("\\n", "\n")
    };

    let kalshi_config = arb_kalshi::KalshiConfig {
        api_key_id: kalshi_api_key_id,
        private_key_pem: kalshi_private_key_pem,
        base_url: kalshi_base_url.clone(),
        ws_url: kalshi_ws_url.clone(),
    };
    let kalshi_real = Arc::new(
        arb_kalshi::KalshiConnector::new(kalshi_config)
            .context("Failed to create Kalshi connector")?
    );
    info!(base_url = %kalshi_base_url, "Kalshi connector initialized");

    // === Seed market pairs ===
    use arb_db::Repository;
    use arb_db::models::MarketPairRow;

    // Step 1: Load existing DB pairs (manual from pairs.toml + previously discovered)
    let db_pairs: Vec<_> = db.list_active_market_pairs().await.unwrap_or_default()
        .into_iter()
        .filter(|p| !p.kalshi_ticker.starts_with("KMOCK-"))
        .collect();
    info!(pairs = db_pairs.len(), "Loaded existing pairs from DB");

    // Build a set of existing (poly_condition_id, kalshi_ticker) for dedup
    let existing_pair_keys: std::collections::HashSet<(String, String)> = db_pairs.iter()
        .map(|p| (p.poly_condition_id.clone(), p.kalshi_ticker.clone()))
        .collect();

    // Step 2: Always run auto-discovery
    info!("Running auto-discovery — fetching live markets from both platforms (paginated)...");
    let gamma_url = &app_config.polymarket.gamma_url;
    let http = reqwest::Client::new();

    // Fetch Kalshi markets from /markets endpoint
    let kalshi_flat_markets = match kalshi_real.list_markets(MarketStatus::Open).await {
        Ok(markets) => {
            info!(count = markets.len(), "Fetched open Kalshi markets from /markets (raw)");
            markets
        }
        Err(e) => {
            warn!(err = %e, "Failed to fetch Kalshi /markets — will use existing DB pairs only");
            Vec::new()
        }
    };

    // Fetch Kalshi events from /events endpoint (discovers crypto, politics, etc.)
    let kalshi_event_markets = match kalshi_real.list_events(5).await {
        Ok(events) => {
            let event_count = events.len();
            let event_markets = arb_kalshi::KalshiConnector::markets_from_events(&events);
            info!(events = event_count, markets = event_markets.len(), "Fetched Kalshi events and extracted markets");
            event_markets
        }
        Err(e) => {
            warn!(err = %e, "Failed to fetch Kalshi /events — continuing with /markets only");
            Vec::new()
        }
    };

    // Merge and dedup by platform_id
    let mut seen_tickers: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut kalshi_markets_raw: Vec<Market> = Vec::new();
    for m in kalshi_flat_markets {
        if seen_tickers.insert(m.platform_id.clone()) {
            kalshi_markets_raw.push(m);
        }
    }
    let from_flat = kalshi_markets_raw.len();
    for m in kalshi_event_markets {
        if seen_tickers.insert(m.platform_id.clone()) {
            kalshi_markets_raw.push(m);
        }
    }
    let from_events = kalshi_markets_raw.len() - from_flat;
    info!(total = kalshi_markets_raw.len(), from_flat, from_events, "Merged Kalshi markets (deduped)");

    // Filter out obviously malformed/novelty Kalshi markets
    let kalshi_markets: Vec<Market> = kalshi_markets_raw
        .into_iter()
        .filter(|m| m.question.len() >= 15)
        .collect();
    info!(count = kalshi_markets.len(), "Kalshi markets after filtering");

    let mut newly_discovered: usize = 0;
    // Fetch Gamma API markets with pagination (up to 3 pages of 200)
    let mut raw_markets: Vec<serde_json::Value> = Vec::new();
    for page in 0u32..3 {
        let offset = page * 200;
        let url = format!(
            "{gamma_url}/markets?active=true&closed=false&limit=200&offset={offset}&order=volume24hr&ascending=false"
        );
        match http.get(&url).send().await {
            Ok(resp) => {
                let page_markets: Vec<serde_json::Value> = resp.json().await.unwrap_or_default();
                let count = page_markets.len();
                info!(page = page + 1, count, offset, "Gamma API page fetched");
                raw_markets.extend(page_markets);
                if count < 200 { break; } // Last page
                if page < 2 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            Err(e) => {
                if page == 0 {
                    warn!(err = %e, "Failed to fetch Gamma API markets");
                } else {
                    warn!(err = %e, page = page + 1, "Failed to fetch Gamma API page");
                }
                break;
            }
        }
    }
    let raw_markets: Vec<serde_json::Value> = raw_markets;

    if !raw_markets.is_empty() {
        info!(total = raw_markets.len(), "Gamma API returned markets");

        let pipeline = MatchPipeline::default();

        let mut poly_markets_for_match: Vec<Market> = Vec::new();
        let mut poly_raw_by_cid: std::collections::HashMap<String, &serde_json::Value> = std::collections::HashMap::new();

        for m in &raw_markets {
            let condition_id = m["conditionId"].as_str().unwrap_or_default();
            let question = m["question"].as_str().unwrap_or_default();
            let outcome_prices_str = m["outcomePrices"].as_str().unwrap_or("[]");
            let end_date = m["endDateIso"].as_str().unwrap_or_default();
            let market_volume: rust_decimal::Decimal = m["volume24hr"]
                .as_f64().or_else(|| m["volume"].as_f64()).unwrap_or(0.0)
                .try_into().unwrap_or_default();

            let prices: Vec<String> = serde_json::from_str(outcome_prices_str).unwrap_or_default();
            if prices.len() < 2 { continue; }
            let poly_yes: rust_decimal::Decimal = prices[0].parse().unwrap_or_default();
            let poly_no: rust_decimal::Decimal = prices[1].parse().unwrap_or_default();

            // Price filter: reject near-decided markets (widened to capture crypto brackets)
            if poly_yes < dec!(0.05) || poly_yes > dec!(0.95) { continue; }
            if condition_id.is_empty() { continue; }

            // Minimum volume filter (lowered to capture crypto bracket sub-markets)
            if market_volume < dec!(1000) {
                continue;
            }

            let close_time = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d")
                .map(|d| d.and_hms_opt(0, 0, 0).unwrap_or_default())
                .map(|dt| chrono::DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
                .unwrap_or_else(|_| Utc::now() + chrono::Duration::days(30));

            // Close-time filter: skip events closing within 6 hours.
            let hours_to_close = (close_time - Utc::now()).num_hours();
            if hours_to_close < 6 {
                continue;
            }

            poly_raw_by_cid.insert(condition_id.to_string(), m);
            poly_markets_for_match.push(Market {
                id: MarketId::new(),
                platform: Platform::Polymarket,
                platform_id: condition_id.to_string(),
                question: question.to_string(),
                yes_price: poly_yes,
                no_price: poly_no,
                volume: market_volume,
                liquidity: rust_decimal::Decimal::ZERO,
                status: MarketStatus::Open,
                close_time,
                updated_at: Utc::now(),
            });
        }

        // Run the match pipeline if we have markets from both platforms
        if !kalshi_markets.is_empty() && !poly_markets_for_match.is_empty() {
            let candidates = pipeline.find_matches(&poly_markets_for_match, &kalshi_markets);
            info!(candidates = candidates.len(), "Match pipeline produced candidates");

            // Quality-gated pair selection with hard cap on DISCOVERED pairs only.
            // Manual pairs (from pairs.toml) don't count against the cap.
            let min_quality_score = app_config.matcher.quality_gate;
            let text_sim_floor = app_config.matcher.text_similarity_floor;
            let max_discovered = app_config.matcher.max_discovered_pairs;
            let mut discovered_count: usize = 0;
            for c in &candidates {
                if discovered_count >= max_discovered { break; }
                let decision = c.score.decision();
                if decision == arb_matcher::MatchDecision::Rejected { continue; }
                if c.score.text_similarity < text_sim_floor { continue; }
                if c.score.composite < min_quality_score { continue; }

                let condition_id = &c.poly_market.platform_id;
                let kalshi_ticker = &c.kalshi_market.platform_id;

                // Skip if this pair already exists in the DB
                if existing_pair_keys.contains(&(condition_id.to_string(), kalshi_ticker.to_string())) {
                    info!(poly = %condition_id, kalshi = %kalshi_ticker, "Pair already in DB, skipping insert");
                    continue;
                }

                // Extract token IDs from the raw Gamma data
                let (yes_token_id, no_token_id) = if let Some(m) = poly_raw_by_cid.get(condition_id) {
                    extract_token_ids(m)
                } else {
                    (String::new(), String::new())
                };

                if yes_token_id.is_empty() || no_token_id.is_empty() {
                    warn!(condition_id, "No token IDs found, skipping");
                    continue;
                }

                let pair_id = uuid::Uuid::now_v7();
                let row = MarketPairRow {
                    id: pair_id.to_string(),
                    poly_condition_id: condition_id.to_string(),
                    poly_yes_token_id: yes_token_id.clone(),
                    poly_no_token_id: no_token_id.clone(),
                    poly_question: c.poly_market.question.clone(),
                    kalshi_ticker: kalshi_ticker.to_string(),
                    kalshi_question: c.kalshi_market.question.clone(),
                    match_confidence: c.score.composite,
                    verified: decision == arb_matcher::MatchDecision::AutoVerified,
                    active: true,
                    close_time: c.poly_market.close_time,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                };
                if let Err(e) = db.insert_market_pair(&row).await {
                    warn!(err = %e, "Failed to insert discovered pair");
                    continue;
                }

                info!(
                    poly_question = %c.poly_market.question,
                    kalshi_question = %c.kalshi_market.question,
                    score = c.score.composite,
                    "Discovered and inserted new pair"
                );
                newly_discovered += 1;
                discovered_count += 1;
            }
        } else {
            warn!("No Kalshi markets available for matching — cannot discover new pairs");
        }
    }

    if newly_discovered > 0 {
        info!(count = newly_discovered, "Newly discovered pairs inserted into DB");
    } else if db_pairs.is_empty() {
        warn!("No pairs found — check internet connection and API credentials");
    }

    // Step 3: Reload all pairs from DB (existing + newly discovered)
    let all_db_pairs: Vec<_> = db.list_active_market_pairs().await.unwrap_or_default()
        .into_iter()
        .filter(|p| !p.kalshi_ticker.starts_with("KMOCK-"))
        .collect();
    info!(total = all_db_pairs.len(), existing = db_pairs.len(), discovered = newly_discovered, "Final pair set");

    // Build condition_id -> volume map from raw Gamma data for volume backfill
    let cid_to_volume: std::collections::HashMap<String, rust_decimal::Decimal> = raw_markets.iter()
        .filter_map(|m| {
            let cid = m["conditionId"].as_str()?;
            let vol: rust_decimal::Decimal = m["volume24hr"]
                .as_f64().or_else(|| m["volume"].as_f64()).unwrap_or(0.0)
                .try_into().unwrap_or_default();
            Some((cid.to_string(), vol))
        })
        .collect();

    let mut pairs: Vec<PairInfo> = all_db_pairs.iter().map(|p| {
        let volume = cid_to_volume.get(&p.poly_condition_id)
            .copied()
            .unwrap_or(rust_decimal::Decimal::ZERO);
        PairInfo {
            pair_id: uuid::Uuid::parse_str(&p.id).unwrap_or_else(|_| uuid::Uuid::now_v7()),
            poly_market_id: p.poly_condition_id.clone(),
            kalshi_market_id: p.kalshi_ticker.clone(),
            close_time: p.close_time,
            verified: p.verified,
            poly_yes_token_id: p.poly_yes_token_id.clone(),
            poly_no_token_id: p.poly_no_token_id.clone(),
            volume,
        }
    }).collect();

    // Step 4: Backfill empty token IDs from Gamma data we already fetched (or re-fetch)
    let needs_backfill = pairs.iter().any(|p| p.poly_yes_token_id.is_empty());
    if needs_backfill {
        // Build cid->tokens map from the raw_markets we already fetched for discovery
        let mut cid_to_tokens: std::collections::HashMap<String, (String, String)> = std::collections::HashMap::new();
        for m in &raw_markets {
            let cid = m["conditionId"].as_str().unwrap_or_default().to_string();
            let clob_str = m["clobTokenIds"].as_str().unwrap_or("[]");
            let clob_ids: Vec<String> = serde_json::from_str(clob_str).unwrap_or_default();
            if clob_ids.len() >= 2 {
                cid_to_tokens.insert(cid, (clob_ids[0].clone(), clob_ids[1].clone()));
            }
        }
        // If discovery fetch was empty, do a dedicated backfill fetch
        if cid_to_tokens.is_empty() {
            if let Ok(resp) = http.get(format!("{gamma_url}/markets?active=true&closed=false&limit=100"))
                .send().await
            {
                if let Ok(raw) = resp.json::<Vec<serde_json::Value>>().await {
                    for m in &raw {
                        let cid = m["conditionId"].as_str().unwrap_or_default().to_string();
                        let clob_str = m["clobTokenIds"].as_str().unwrap_or("[]");
                        let clob_ids: Vec<String> = serde_json::from_str(clob_str).unwrap_or_default();
                        if clob_ids.len() >= 2 {
                            cid_to_tokens.insert(cid, (clob_ids[0].clone(), clob_ids[1].clone()));
                        }
                    }
                }
            }
        }
        for pair in &mut pairs {
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
    let pairs: Vec<PairInfo> = pairs;

    // === Wrap connectors based on trading mode ===
    let kalshi_dyn: Arc<dyn PredictionMarketConnector> = kalshi_real.clone();
    let poly_dyn: Arc<dyn PredictionMarketConnector> = poly_real.clone();

    let (poly, kalshi): (Arc<dyn PredictionMarketConnector>, Arc<dyn PredictionMarketConnector>) =
        if args.paper_both {
            info!("Full paper mode — wrapping BOTH connectors in PaperConnector");
            (
                Arc::new(PaperConnector::new(poly_dyn, dec!(10000), 0.9, 500)),
                Arc::new(PaperConnector::new(kalshi_dyn, dec!(10000), 0.9, 500)),
            )
        } else if args.paper {
            info!("Hybrid paper mode — real Kalshi + simulated Polymarket");
            (
                Arc::new(PaperConnector::new(poly_dyn, dec!(10000), 0.9, 500)),
                kalshi_dyn,
            )
        } else {
            if use_demo {
                info!("Live mode with Kalshi DEMO sandbox");
            } else {
                warn!("LIVE MODE — using real connectors on both platforms");
            }
            (poly_dyn, kalshi_dyn)
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

    let fee_config = FeeConfig {
        kalshi_taker_fee_pct: app_config.fees.kalshi_taker_fee_pct.parse().unwrap_or(rust_decimal_macros::dec!(7.0)),
        poly_taker_fee_pct: app_config.fees.poly_taker_fee_pct.parse().unwrap_or(rust_decimal_macros::dec!(0.0)),
    };
    info!(
        kalshi_fee = %fee_config.kalshi_taker_fee_pct,
        poly_fee = %fee_config.poly_taker_fee_pct,
        "Fee configuration loaded"
    );

    let executor = Executor::new(poly.clone(), kalshi.clone(), rm.clone(), db.clone(), order_cfg.clone(), trading_mode.clone());
    let monitor = Monitor::new(poly.clone(), kalshi.clone(), order_cfg);
    let tracker = Tracker::new(db.clone(), rm.clone(), trading_mode.clone(), fee_config.clone());
    let unwinder = Unwinder::new(poly.clone(), kalshi.clone(), rm.clone(), db.clone());

    let engine = Arc::new(Engine::new(
        poly.clone(), kalshi.clone(), price_cache.clone(), db.clone(),
        executor, monitor, tracker, unwinder, engine_cfg, trading_mode.clone(), fee_config,
    ));
    info!("Engine initialized");

    // === Price feeds ===
    let (price_tx, price_rx) = tokio::sync::mpsc::channel(256);

    // Collect WS subscription handles so they are dropped cleanly on shutdown
    // instead of being leaked via std::mem::forget.
    let mut _ws_handles: Vec<arb_types::SubHandle> = Vec::new();

    // Polymarket WS subscription
    let poly_token_ids: Vec<String> = pairs.iter()
        .map(|p| p.poly_yes_token_id.clone())
        .filter(|id| !id.is_empty())
        .collect();

    if !poly_token_ids.is_empty() {
        match poly.subscribe_prices(&poly_token_ids, price_tx.clone()).await {
            Ok(handle) => {
                info!(count = poly_token_ids.len(), "Polymarket WS price feed started");
                _ws_handles.push(handle);
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

    // Kalshi WS price feed (real) + REST polling (always runs as reliable backup)
    let kalshi_ticker_ids: Vec<String> = pairs.iter()
        .map(|p| p.kalshi_market_id.clone())
        .collect();

    if !kalshi_ticker_ids.is_empty() {
        // Try WS subscription (may silently fail to deliver for low-volume markets)
        match kalshi.subscribe_prices(&kalshi_ticker_ids, price_tx.clone()).await {
            Ok(handle) => {
                info!(count = kalshi_ticker_ids.len(), "Kalshi WS price feed started");
                _ws_handles.push(handle);
            }
            Err(e) => {
                warn!(err = %e, "Failed to start Kalshi WS feed");
            }
        }

        // REST polling ALWAYS runs — WS may connect but never send updates for some markets
        {
            let kalshi_poll = kalshi.clone();
            let kalshi_poll_tickers: Vec<String> = kalshi_ticker_ids.clone();
            let ptx = price_tx.clone();
            tokio::spawn(async move {
                loop {
                    for ticker in &kalshi_poll_tickers {
                        match kalshi_poll.get_market(ticker).await {
                            Ok(market) => {
                                let _ = ptx.send(arb_types::PriceUpdate {
                                    platform: Platform::Kalshi,
                                    market_id: ticker.clone(),
                                    yes_price: market.yes_price,
                                    no_price: market.no_price,
                                    timestamp: Utc::now(),
                                }).await;
                            }
                            Err(e) => {
                                warn!(ticker, err = %e, "Kalshi REST poll failed");
                            }
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(8)).await;
                }
            });
            info!("Kalshi REST polling started (8s interval)");
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
    if args.tui || (!args.headless && !args.paper && !args.paper_both) {
        // Set panic hook to restore terminal on panic
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
            original_hook(panic_info);
        }));

        tui::run_tui(db.clone(), rm.clone(), mode, pair_count, newly_discovered, price_cache.clone(), pairs.clone()).await?;
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

/// Result of fetching Kalshi markets from both /markets and /events endpoints.
struct KalshiFetchResult {
    markets: Vec<Market>,
    from_markets_endpoint: usize,
    from_events_endpoint: usize,
    events_fetched: usize,
}

/// Try to create a Kalshi connector and fetch open markets from BOTH endpoints:
/// 1. `/markets` — flat list of sub-markets (dominated by short-term sports/weather)
/// 2. `/events` — event groups with nested sub-markets (better for crypto, politics)
///
/// Markets are deduped by ticker. Event-derived markets use the event title as the
/// question (more matchable than the sub-market bracket title).
async fn try_fetch_kalshi_markets(kalshi_cfg: &KalshiTomlConfig) -> Result<KalshiFetchResult> {
    let api_key_id = std::env::var("KALSHI_API_KEY_ID")
        .context("KALSHI_API_KEY_ID env var not set")?;
    let private_key_pem = if let Ok(path) = std::env::var("KALSHI_PRIVATE_KEY_PATH") {
        std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read Kalshi private key from {path}"))?
    } else {
        let raw = std::env::var("KALSHI_PRIVATE_KEY_PEM")
            .context("Set KALSHI_PRIVATE_KEY_PATH or KALSHI_PRIVATE_KEY_PEM")?;
        raw.replace("\\n", "\n")
    };

    // Default to demo for --match mode
    let config = arb_kalshi::KalshiConfig {
        api_key_id,
        private_key_pem,
        base_url: kalshi_cfg.demo_base_url.clone(),
        ws_url: kalshi_cfg.demo_ws_url.clone(),
    };
    let connector = arb_kalshi::KalshiConnector::new(config)
        .context("Failed to create Kalshi connector")?;

    // Fetch from /markets endpoint (existing behavior)
    let flat_markets = connector.list_markets(MarketStatus::Open).await
        .context("Failed to fetch Kalshi markets")?;
    let from_markets_endpoint = flat_markets.len();

    // Fetch from /events endpoint (new — discovers crypto, politics, etc.)
    let events = match connector.list_events(5).await {
        Ok(events) => events,
        Err(e) => {
            eprintln!("  Warning: Kalshi events fetch failed ({e}) — using /markets only");
            Vec::new()
        }
    };
    let events_fetched = events.len();
    let event_markets = arb_kalshi::KalshiConnector::markets_from_events(&events);
    let from_events_endpoint = event_markets.len();

    // Merge and dedup by platform_id (sub-market ticker)
    let mut seen_tickers: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut merged: Vec<Market> = Vec::new();

    // Add flat markets first (they have direct sub-market data)
    for m in flat_markets {
        if seen_tickers.insert(m.platform_id.clone()) {
            merged.push(m);
        }
    }

    // Add event-derived markets (only if ticker not already present)
    for m in event_markets {
        if seen_tickers.insert(m.platform_id.clone()) {
            merged.push(m);
        }
    }

    Ok(KalshiFetchResult {
        markets: merged,
        from_markets_endpoint,
        from_events_endpoint,
        events_fetched,
    })
}

/// Extract Polymarket token IDs from a raw Gamma API market JSON object.
fn extract_token_ids(m: &serde_json::Value) -> (String, String) {
    // Try clobTokenIds field (JSON-encoded string array)
    let clob_str = m["clobTokenIds"].as_str().unwrap_or("[]");
    let clob_ids: Vec<String> = serde_json::from_str(clob_str).unwrap_or_default();
    if clob_ids.len() >= 2 {
        return (clob_ids[0].clone(), clob_ids[1].clone());
    }
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
    (yes_id, no_id)
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
