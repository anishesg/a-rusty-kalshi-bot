mod config;
mod db;
mod errors;
mod execution;
mod feeds;
mod kalshi;
mod models;
mod paper;
mod risk;
mod server;
mod state;

use crate::models::black_scholes::BlackScholesDigital;
use crate::models::calibration::Calibrator;
use crate::models::jump_diffusion::JumpDiffusionDigital;
use crate::models::student_t::StudentTDigital;
use crate::models::volatility::VolatilityEngine;
use crate::models::PricingModel;
use crate::paper::simulator::{self, EngineAction};
use crate::state::*;
use portable_atomic::Ordering;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    // Early stdout so Railway captures something even if tracing fails
    eprintln!("[pretty_rusty] binary started, setting up logging...");

    // Structured logging (line-buffered for Railway)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("pretty_rusty engine starting");

    // Load config
    let cfg = match config::AppConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("config error: {e}");
            std::process::exit(1);
        }
    };

    // Init database
    let db_pool = match db::init_db(std::path::Path::new("data")) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("database init error: {e}");
            std::process::exit(1);
        }
    };

    // Create bounded channels
    let (engine_tx, engine_rx) = mpsc::channel::<EngineEvent>(512);
    let (db_tx, db_rx) = mpsc::channel::<DbCommand>(1024);

    // Create shared state
    let app_state = AppState::new(cfg.clone(), db_pool.clone(), engine_tx.clone(), db_tx.clone());

    // Init Kalshi auth
    let kalshi_auth = match kalshi::auth::KalshiAuth::new(
        &cfg.kalshi_api_key_id,
        &cfg.kalshi_private_key_path,
    ) {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("kalshi auth error: {e}");
            std::process::exit(1);
        }
    };

    let kalshi_client = kalshi::client::KalshiClient::new(&cfg.kalshi_base_url, kalshi_auth);

    // ── Spawn tasks ──

    // 1. DB writer task (dedicated, owns the DB connection for writes)
    let db_pool_writer = db_pool.clone();
    tokio::spawn(async move {
        db::run_db_writer(db_pool_writer, db_rx).await;
    });

    // 2. BTC price feed task
    let crypto_key = cfg.crypto_api_key.clone();
    let crypto_url = cfg.crypto_api_base_url.clone();
    let feed_tx = engine_tx.clone();
    tokio::spawn(async move {
        feeds::crypto_api::run_btc_feed(crypto_key, crypto_url, feed_tx).await;
    });

    // 3. Kalshi market scanner task
    let scanner_cfg = cfg.clone();
    let scanner_client = kalshi_client.clone();
    let scanner_tx = engine_tx.clone();
    tokio::spawn(async move {
        kalshi::scanner::run_market_scanner(scanner_cfg, scanner_client, scanner_tx).await;
    });

    // 4. Tick generator (1-second interval)
    let tick_tx = engine_tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            if tick_tx.send(EngineEvent::Tick).await.is_err() {
                break;
            }
        }
    });

    // 5. Engine task (core loop -- this is the hot path)
    let engine_state = app_state.clone();
    let engine_cfg = cfg.clone();
    tokio::spawn(async move {
        run_engine(engine_state, engine_cfg, engine_rx).await;
    });

    // 6. Axum HTTP + WS server
    let server_state = app_state.clone();
    let port = cfg.server_port;

    let app = axum::Router::new()
        .route("/api/state", axum::routing::get(server::routes::get_state))
        .route("/api/trades", axum::routing::get(server::routes::get_trades))
        .route("/api/pnl", axum::routing::get(server::routes::get_pnl))
        .route("/api/metrics", axum::routing::get(server::routes::get_metrics))
        .route("/api/risk", axum::routing::get(server::routes::get_risk))
        .route("/api/counters", axum::routing::get(server::routes::get_counters))
        .route("/ws", axum::routing::get(server::ws::ws_handler))
        .fallback_service(
            tower_http::services::ServeDir::new("dashboard/dist")
                .fallback(tower_http::services::ServeFile::new("dashboard/dist/index.html")),
        )
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .with_state(server_state);

    let addr = format!("0.0.0.0:{port}");
    tracing::info!("server listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("bind error: {e}");
            std::process::exit(1);
        });

    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!("server error: {e}");
    }
}

/// Core engine loop. Receives events, updates state, runs models, emits actions.
/// This is the hot path. No locks, no IO in the decision logic.
async fn run_engine(
    state: Arc<AppState>,
    config: config::AppConfig,
    mut rx: mpsc::Receiver<EngineEvent>,
) {
    tracing::info!("engine task started");

    // ── Local engine state (owned, no locks needed) ──
    let mut engine_state = EngineState::Connecting;
    let mut btc_price: f64 = 0.0;
    let mut btc_prices: VecDeque<(i64, f64)> = VecDeque::with_capacity(2000);
    let mut active_market: Option<ActiveMarket> = None;
    let mut vol_engine = VolatilityEngine::new();

    let mut model_states = vec![
        ModelState::new("Black-Scholes"),
        ModelState::new("Jump-Diffusion"),
        ModelState::new("Student-t"),
    ];

    let mut calibrators = vec![
        Calibrator::new(),
        Calibrator::new(),
        Calibrator::new(),
    ];

    // Pricing model instances (created once, reused)
    let bs = BlackScholesDigital::new();
    let jd = JumpDiffusionDigital::new();
    let st = StudentTDigital::new();
    let pricing_models: Vec<&dyn PricingModel> = vec![&bs, &jd, &st];

    let mut tick_counter: u64 = 0;

    while let Some(event) = rx.recv().await {
        let result = process_event(
            event,
            &mut engine_state,
            &mut btc_price,
            &mut btc_prices,
            &mut active_market,
            &mut vol_engine,
            &mut model_states,
            &mut calibrators,
            &pricing_models,
            &config,
            &state,
            &mut tick_counter,
        )
        .await;

        if let Err(e) = result {
            tracing::error!(error = %e, "engine error");
            state.counters.errors_recovered.fetch_add(1, Ordering::Relaxed);

            // On unrecoverable state corruption, halt
            if matches!(e, errors::EngineError::StateCorruption(_)) {
                engine_state = EngineState::Halted;
                state.broadcast(WsMessage::EngineStateMsg {
                    state: "halted".into(),
                    reason: e.to_string(),
                });
                tracing::error!("ENGINE HALTED: {e}");
            }
        }
    }

    tracing::info!("engine task shutting down");
}

#[allow(clippy::too_many_arguments)]
async fn process_event(
    event: EngineEvent,
    engine_state: &mut EngineState,
    btc_price: &mut f64,
    btc_prices: &mut VecDeque<(i64, f64)>,
    active_market: &mut Option<ActiveMarket>,
    vol_engine: &mut VolatilityEngine,
    model_states: &mut [ModelState],
    calibrators: &mut [Calibrator],
    pricing_models: &[&dyn PricingModel],
    config: &config::AppConfig,
    state: &Arc<AppState>,
    tick_counter: &mut u64,
) -> Result<(), errors::EngineError> {
    match event {
        EngineEvent::BtcPrice { price, timestamp_ms } => {
            *btc_price = price;
            state.counters.prices_received.fetch_add(1, Ordering::Relaxed);

            // Store in ring buffer
            if btc_prices.len() >= 2000 {
                btc_prices.pop_front();
            }
            btc_prices.push_back((timestamp_ms, price));

            // Update volatility
            vol_engine.update(price);

            // State transitions
            match engine_state {
                EngineState::Connecting => {
                    *engine_state = EngineState::Syncing;
                    tracing::info!(price = price, "first BTC price received, entering Syncing");
                    state.broadcast(WsMessage::EngineStateMsg {
                        state: "syncing".into(),
                        reason: "first price received".into(),
                    });
                }
                EngineState::Syncing => {
                    if vol_engine.is_ready() && active_market.is_some() {
                        *engine_state = EngineState::Trading;
                        tracing::info!("volatility ready + market found, entering Trading");
                        state.broadcast(WsMessage::EngineStateMsg {
                            state: "trading".into(),
                            reason: "vol ready, market active".into(),
                        });
                    }
                }
                _ => {}
            }

            // Broadcast price
            let ts = chrono::DateTime::from_timestamp_millis(timestamp_ms)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default();

            state.broadcast(WsMessage::BtcPrice {
                price,
                timestamp: ts.clone(),
            });

            // DB write (throttled: every 5th price)
            if state.counters.prices_received.load(Ordering::Relaxed) % 5 == 0 {
                let _ = state.db_tx.send(DbCommand::InsertBtcPrice {
                    timestamp: ts,
                    price,
                }).await;
            }
        }

        EngineEvent::MarketUpdate(market) => {
            // Broadcast market state
            let ttl = compute_ttl_secs(&market.close_time);

            state.broadcast(WsMessage::MarketState {
                ticker: market.ticker.clone(),
                strike: market.strike,
                ttl_seconds: ttl,
                yes_bid: market.yes_bid.clone(),
                yes_ask: market.yes_ask.clone(),
                status: market.status.clone(),
            });

            // Insert to DB if new; reset model positions when switching markets
            if active_market.as_ref().map(|m| &m.ticker) != Some(&market.ticker) {
                tracing::info!(
                    ticker = %market.ticker,
                    strike = ?market.strike,
                    yes_ask = ?market.yes_ask,
                    "switching to new market"
                );

                // Clear open positions so each model can trade the new market
                for ms in model_states.iter_mut() {
                    ms.open_positions.clear();
                    ms.unrealized_pnl = 0.0;
                }

                let _ = state.db_tx.send(DbCommand::InsertMarket {
                    ticker: market.ticker.clone(),
                    event_ticker: market.event_ticker.clone(),
                    series_ticker: market.series_ticker.clone(),
                    strike_price: market.strike,
                    open_time: String::new(),
                    close_time: market.close_time.clone(),
                    expiration_time: market.expiration_time.clone(),
                }).await;
            }

            *active_market = Some(*market);

            // Check if we should transition to Trading
            if *engine_state == EngineState::Syncing && vol_engine.is_ready() {
                *engine_state = EngineState::Trading;
                tracing::info!("entering Trading state");
                state.broadcast(WsMessage::EngineStateMsg {
                    state: "trading".into(),
                    reason: "market + vol ready".into(),
                });
            }
        }

        EngineEvent::MarketSettled { ticker, result } => {
            tracing::info!(ticker = %ticker, result = %result, "processing market settlement");

            // Get pending trades from DB
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            let _ = state.db_tx.send(DbCommand::GetPendingTrades {
                market_ticker: ticker.clone(),
                reply: reply_tx,
            }).await;

            if let Ok(pending) = reply_rx.await {
                tracing::info!(
                    ticker = %ticker,
                    pending_count = pending.len(),
                    "settling trades"
                );

                let now = chrono::Utc::now().to_rfc3339();
                let actions = simulator::settle_trades(
                    model_states,
                    calibrators,
                    &ticker,
                    &result,
                    &pending,
                    &now,
                );

                execute_actions(actions, state).await;

                // Immediately update snapshot so dashboard sees P/L change
                let snapshot = EngineSnapshot {
                    engine_state: *engine_state,
                    btc_price: *btc_price,
                    btc_timestamp: now,
                    active_market: active_market.clone(),
                    volatility: vol_engine.state,
                    models: model_states.to_vec(),
                };
                let _ = state.snapshot_tx.send(snapshot);

                // Log post-settlement P/L
                for ms in model_states.iter() {
                    tracing::info!(
                        model = ms.name,
                        pnl = ms.cumulative_pnl,
                        trades = ms.total_trades,
                        wins = ms.winning_trades,
                        "post-settlement state"
                    );
                }
            } else {
                tracing::warn!(ticker = %ticker, "failed to get pending trades for settlement");
            }

            // Update market result in DB
            let _ = state.db_tx.send(DbCommand::UpdateMarketResult {
                ticker,
                result,
                settlement_value: None,
            }).await;

            // Clear the active market -- scanner will find the next one
            *active_market = None;
        }

        EngineEvent::Tick => {
            *tick_counter += 1;
            state.counters.ticks_processed.fetch_add(1, Ordering::Relaxed);

            // Only run models in Trading state
            if *engine_state != EngineState::Trading {
                return Ok(());
            }

            if *btc_price <= 0.0 {
                return Ok(());
            }

            let now = chrono::Utc::now().to_rfc3339();

            // Run the decision loop (hot path, pure computation)
            let actions = simulator::run_tick(
                pricing_models,
                model_states,
                calibrators,
                &vol_engine.state,
                active_market,
                *btc_price,
                config,
                &now,
                *tick_counter,
            );

            state.counters.decisions_made.fetch_add(1, Ordering::Relaxed);

            // Execute actions (DB writes + WS broadcasts)
            execute_actions(actions, state).await;

            // Update snapshot for dashboard (watch channel -- cheap, no lock)
            if *tick_counter % 2 == 0 {
                let snapshot = EngineSnapshot {
                    engine_state: *engine_state,
                    btc_price: *btc_price,
                    btc_timestamp: now,
                    active_market: active_market.clone(),
                    volatility: vol_engine.state,
                    models: model_states.to_vec(),
                };
                let _ = state.snapshot_tx.send(snapshot);
            }
        }

        EngineEvent::Shutdown => {
            tracing::info!("shutdown event received");
            *engine_state = EngineState::Halted;
            return Ok(());
        }
    }

    Ok(())
}

/// Execute engine actions (cold path -- involves channel sends)
async fn execute_actions(
    actions: smallvec::SmallVec<[EngineAction; 16]>,
    state: &Arc<AppState>,
) {
    for action in actions {
        match action {
            EngineAction::PlaceTrade { .. } => {
                state.counters.trades_placed.fetch_add(1, Ordering::Relaxed);
            }
            EngineAction::ExitTrade { model_name, pnl, reason, .. } => {
                tracing::info!(model = model_name, pnl = pnl, reason = reason, "trade exited");
            }
            EngineAction::BroadcastUpdate(msg) => {
                state.broadcast(msg);
            }
            EngineAction::DbWrite(cmd) => {
                let _ = state.db_tx.send(cmd).await;
            }
            EngineAction::SettleTrade { .. } => {
                // Logging handled in simulator
            }
        }
    }
}

fn compute_ttl_secs(close_time: &str) -> f64 {
    let now = chrono::Utc::now();
    chrono::DateTime::parse_from_rfc3339(close_time)
        .ok()
        .map(|dt| (dt.with_timezone(&chrono::Utc) - now).num_seconds() as f64)
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(close_time, "%Y-%m-%dT%H:%M:%SZ")
                .ok()
                .map(|dt| (dt.and_utc() - now).num_seconds() as f64)
        })
        .unwrap_or(0.0)
        .max(0.0)
}
