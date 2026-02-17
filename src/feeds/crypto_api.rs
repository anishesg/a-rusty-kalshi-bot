use crate::errors::{EngineError, EngineResult};
use crate::state::EngineEvent;
use reqwest::Client;
use tokio::sync::mpsc;

/// FreeCryptoAPI REST client. Polls BTC price at configurable interval.
/// Sends BtcPrice events to engine via bounded channel.
pub async fn run_btc_feed(
    api_key: String,
    base_url: String,
    engine_tx: mpsc::Sender<EngineEvent>,
) {
    tracing::info!("BTC price feed started (FreeCryptoAPI)");

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    // Poll every 2 seconds for near-real-time price updates
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
    let mut consecutive_errors: u32 = 0;

    loop {
        interval.tick().await;

        match fetch_btc_price(&client, &api_key, &base_url).await {
            Ok(price) => {
                consecutive_errors = 0;
                let timestamp_ms = chrono::Utc::now().timestamp_millis();

                if engine_tx
                    .send(EngineEvent::BtcPrice {
                        price,
                        timestamp_ms,
                    })
                    .await
                    .is_err()
                {
                    tracing::error!("engine channel closed, btc feed shutting down");
                    return;
                }
            }
            Err(e) => {
                consecutive_errors += 1;
                tracing::warn!(
                    error = %e,
                    consecutive = consecutive_errors,
                    "btc price fetch failed"
                );

                // Exponential backoff on repeated failures (cap at 30s)
                if consecutive_errors > 3 {
                    let backoff = std::cmp::min(consecutive_errors * 2, 30);
                    tokio::time::sleep(tokio::time::Duration::from_secs(backoff as u64)).await;
                }
            }
        }
    }
}

// Actual FreeCryptoAPI response format:
// {
//   "status": "success",
//   "symbols": [
//     {
//       "symbol": "BTC",
//       "last": "68078",
//       "last_btc": "1",
//       "lowest": "67960.52",
//       "highest": "69194.3",
//       "date": "2026-02-17 08:27:54",
//       "daily_change_percentage": "-0.65",
//       "source_exchange": "binance"
//     }
//   ]
// }

#[derive(serde::Deserialize)]
struct CryptoDataResponse {
    #[allow(dead_code)]
    status: Option<String>,
    symbols: Option<Vec<SymbolData>>,
}

#[derive(serde::Deserialize)]
struct SymbolData {
    #[allow(dead_code)]
    symbol: Option<String>,
    last: Option<String>,
    #[allow(dead_code)]
    lowest: Option<String>,
    #[allow(dead_code)]
    highest: Option<String>,
}

async fn fetch_btc_price(client: &Client, api_key: &str, base_url: &str) -> EngineResult<f64> {
    let url = format!("{}/getData?symbol=BTC", base_url.trim_end_matches('/'));

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .map_err(|e| EngineError::CryptoFeed(format!("request failed: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(EngineError::CryptoFeed(format!("HTTP {status}: {body}")));
    }

    let data: CryptoDataResponse = resp
        .json()
        .await
        .map_err(|e| EngineError::CryptoFeed(format!("parse: {e}")))?;

    // Extract price from symbols[0].last (it's a string like "68078")
    let price_str = data
        .symbols
        .as_ref()
        .and_then(|syms| syms.first())
        .and_then(|s| s.last.as_deref())
        .ok_or_else(|| EngineError::CryptoFeed("no BTC symbol in response".into()))?;

    let price: f64 = price_str
        .parse()
        .map_err(|_| EngineError::CryptoFeed(format!("invalid price string: {price_str}")))?;

    if price <= 0.0 || !price.is_finite() {
        return Err(EngineError::CryptoFeed(format!("invalid price: {price}")));
    }

    Ok(price)
}
