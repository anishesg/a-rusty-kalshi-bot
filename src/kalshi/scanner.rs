use super::client::KalshiClient;
use super::types::Market;
use crate::config::AppConfig;
use crate::state::{ActiveMarket, EngineEvent};
use chrono::Utc;
use tokio::sync::mpsc;

/// Polls Kalshi for active BTC binary markets.
/// Sends MarketUpdate / MarketSettled events to the engine via bounded channel.
///
/// Market selection strategy:
///   1. Get all open/active binary markets in the BTC series.
///   2. Group by close_time, pick the soonest-closing group.
///   3. Among those, pick the market with yes_ask closest to $0.50 (near ATM).
///   4. Track previously active markets for settlement checking.
pub async fn run_market_scanner(
    config: AppConfig,
    client: KalshiClient,
    engine_tx: mpsc::Sender<EngineEvent>,
) {
    tracing::info!("market scanner started, series={}", config.btc_series_ticker);

    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
    let mut current_ticker: Option<String> = None;
    // Track old market tickers that need settlement checking
    let mut pending_settlement: Vec<String> = Vec::new();

    loop {
        interval.tick().await;

        // ── 1. Check settlement of ALL previously-tracked markets ──
        let mut settled: Vec<usize> = Vec::new();
        for (idx, ticker) in pending_settlement.iter().enumerate() {
            match client.get_market(ticker).await {
                Ok(resp) => {
                    if let Some(market) = resp.market {
                        if market.is_settled() {
                            let result = market.result.clone().unwrap_or_default();
                            tracing::info!(ticker = %ticker, result = %result, "market settled");

                            let _ = engine_tx
                                .send(EngineEvent::MarketSettled {
                                    ticker: ticker.clone(),
                                    result,
                                })
                                .await;

                            settled.push(idx);
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!(ticker = %ticker, error = %e, "settlement check failed");
                }
            }
        }
        // Remove settled ones (reverse order to preserve indices)
        for idx in settled.into_iter().rev() {
            pending_settlement.remove(idx);
        }

        // ── 2. Scan for the best active market ──
        match scan_for_market(&config, &client).await {
            Ok(Some(market)) => {
                let ticker = market.ticker.clone().unwrap_or_default();
                let is_new = current_ticker.as_ref() != Some(&ticker);

                let am = market_to_active(&config, &market);

                if is_new {
                    // If we were tracking a different market, move it to settlement tracking
                    if let Some(old_ticker) = current_ticker.take() {
                        if !pending_settlement.contains(&old_ticker) {
                            tracing::info!(old = %old_ticker, new = %ticker, "switching market, tracking old for settlement");
                            pending_settlement.push(old_ticker);
                        }
                    }

                    tracing::info!(
                        ticker = %ticker,
                        strike = ?market.strike_price(),
                        yes_ask = ?market.yes_ask_dollars,
                        "tracking new market"
                    );
                    current_ticker = Some(ticker.clone());
                }

                if engine_tx.send(EngineEvent::MarketUpdate(Box::new(am))).await.is_err() {
                    tracing::error!("engine channel closed, scanner shutting down");
                    return;
                }
            }
            Ok(None) => {
                // No active market; if we had one, move it to settlement tracking
                if let Some(old_ticker) = current_ticker.take() {
                    if !pending_settlement.contains(&old_ticker) {
                        pending_settlement.push(old_ticker);
                    }
                }
                tracing::debug!("no active BTC market found");
            }
            Err(e) => {
                tracing::warn!(error = %e, "market scanner error");
            }
        }

        // Cap pending settlement list to avoid unbounded growth
        if pending_settlement.len() > 20 {
            pending_settlement.drain(0..pending_settlement.len() - 20);
        }
    }
}

async fn scan_for_market(
    config: &AppConfig,
    client: &KalshiClient,
) -> Result<Option<Market>, crate::errors::EngineError> {
    let series = &config.btc_series_ticker;

    let resp = client.get_markets(Some(series), Some("open"), Some(100), None).await?;
    let mut markets = resp.markets.unwrap_or_default();

    if markets.is_empty() {
        let resp2 = client.get_markets(Some(series), Some("active"), Some(100), None).await?;
        markets = resp2.markets.unwrap_or_default();
    }

    Ok(find_best_market(markets))
}

fn find_best_market(markets: Vec<Market>) -> Option<Market> {
    let now = Utc::now();

    let candidates: Vec<_> = markets
        .into_iter()
        .filter(|m| m.is_active() && m.market_type.as_deref() == Some("binary"))
        .filter(|m| {
            m.close_time.as_ref().is_some_and(|ct| {
                parse_datetime(ct).is_some_and(|close| close > now)
            })
        })
        .collect();

    if candidates.is_empty() {
        return None;
    }

    // Find the earliest close time
    let earliest_ts = candidates
        .iter()
        .filter_map(|m| m.close_time.as_ref().and_then(|ct| parse_datetime(ct)))
        .min()?
        .timestamp();

    // Among markets with the soonest close time (within 60s tolerance),
    // pick the one with yes_ask closest to $0.50 (nearest to ATM).
    candidates
        .into_iter()
        .filter(|m| {
            m.close_time
                .as_ref()
                .and_then(|ct| parse_datetime(ct))
                .map(|dt| (dt.timestamp() - earliest_ts).abs() < 60)
                .unwrap_or(false)
        })
        .min_by_key(|m| {
            let yes_ask = m
                .yes_ask_dollars
                .as_ref()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            // Distance from 0.50 -- lower = closer to ATM
            ((yes_ask - 0.50).abs() * 10000.0) as i64
        })
}

fn parse_datetime(s: &str) -> Option<chrono::DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ")
                .ok()
                .map(|dt| dt.and_utc())
        })
}

fn market_to_active(config: &AppConfig, m: &Market) -> ActiveMarket {
    ActiveMarket {
        ticker: m.ticker.clone().unwrap_or_default(),
        event_ticker: m.event_ticker.clone().unwrap_or_default(),
        series_ticker: config.btc_series_ticker.clone(),
        strike: m.strike_price(),
        yes_bid: m.yes_bid_dollars.clone(),
        yes_ask: m.yes_ask_dollars.clone(),
        no_bid: m.no_bid_dollars.clone(),
        no_ask: m.no_ask_dollars.clone(),
        last_price: m.last_price_dollars.clone(),
        close_time: m.close_time.clone().unwrap_or_default(),
        expiration_time: m.expiration_time.clone().unwrap_or_default(),
        status: m.status.clone().unwrap_or_default(),
        result: m.result.clone(),
    }
}
