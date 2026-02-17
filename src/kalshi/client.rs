use super::auth::KalshiAuth;
use super::types::*;
use crate::errors::{EngineError, EngineResult};
use reqwest::Client;

/// Kalshi REST API client. All methods return Result, never panic.
#[derive(Clone)]
pub struct KalshiClient {
    client: Client,
    base_url: String,
    auth: KalshiAuth,
}

impl KalshiClient {
    pub fn new(base_url: &str, auth: KalshiAuth) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .pool_max_idle_per_host(4)
                .build()
                .unwrap_or_default(),
            base_url: base_url.trim_end_matches('/').to_string(),
            auth,
        }
    }

    async fn auth_get<T: serde::de::DeserializeOwned>(&self, path: &str) -> EngineResult<T> {
        let url = format!("{}{}", self.base_url, path);
        let (key_id, timestamp, signature) = self.auth.sign_request("GET", path, "")?;

        let resp = self
            .client
            .get(&url)
            .header("KALSHI-ACCESS-KEY", &key_id)
            .header("KALSHI-ACCESS-TIMESTAMP", &timestamp)
            .header("KALSHI-ACCESS-SIGNATURE", &signature)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(EngineError::KalshiApi {
                status: status.as_u16(),
                body,
            });
        }

        resp.json::<T>().await.map_err(|e| EngineError::Parse(format!("GET {path}: {e}")))
    }

    async fn public_get<T: serde::de::DeserializeOwned>(&self, path: &str) -> EngineResult<T> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.client.get(&url).send().await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(EngineError::KalshiApi {
                status: status.as_u16(),
                body,
            });
        }

        resp.json::<T>().await.map_err(|e| EngineError::Parse(format!("GET {path}: {e}")))
    }

    // ── Public endpoints ──

    pub async fn get_markets(
        &self,
        series_ticker: Option<&str>,
        status: Option<&str>,
        limit: Option<u32>,
        cursor: Option<&str>,
    ) -> EngineResult<GetMarketsResponse> {
        let mut parts: smallvec::SmallVec<[String; 4]> = smallvec::SmallVec::new();
        if let Some(s) = series_ticker { parts.push(format!("series_ticker={s}")); }
        if let Some(s) = status { parts.push(format!("status={s}")); }
        if let Some(l) = limit { parts.push(format!("limit={l}")); }
        if let Some(c) = cursor { parts.push(format!("cursor={c}")); }
        let query = if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) };
        self.public_get(&format!("/markets{query}")).await
    }

    pub async fn get_market(&self, ticker: &str) -> EngineResult<GetMarketResponse> {
        self.public_get(&format!("/markets/{ticker}")).await
    }

    pub async fn get_market_trades(&self, ticker: Option<&str>, limit: Option<u32>) -> EngineResult<GetTradesResponse> {
        let mut parts: smallvec::SmallVec<[String; 2]> = smallvec::SmallVec::new();
        if let Some(t) = ticker { parts.push(format!("ticker={t}")); }
        if let Some(l) = limit { parts.push(format!("limit={l}")); }
        let query = if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) };
        self.public_get(&format!("/markets/trades{query}")).await
    }

    pub async fn get_events(
        &self,
        series_ticker: Option<&str>,
        status: Option<&str>,
        limit: Option<u32>,
    ) -> EngineResult<GetEventsResponse> {
        let mut parts: smallvec::SmallVec<[String; 3]> = smallvec::SmallVec::new();
        if let Some(s) = series_ticker { parts.push(format!("series_ticker={s}")); }
        if let Some(s) = status { parts.push(format!("status={s}")); }
        if let Some(l) = limit { parts.push(format!("limit={l}")); }
        let query = if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) };
        self.public_get(&format!("/events{query}")).await
    }

    pub async fn get_event(&self, event_ticker: &str) -> EngineResult<GetEventResponse> {
        self.public_get(&format!("/events/{event_ticker}")).await
    }

    pub async fn get_series(&self) -> EngineResult<GetSeriesResponse> {
        self.public_get("/series").await
    }

    // ── Authenticated endpoints ──

    pub async fn get_orderbook(&self, ticker: &str, depth: Option<u32>) -> EngineResult<OrderbookResponse> {
        let depth_param = depth.map(|d| format!("?depth={d}")).unwrap_or_default();
        self.auth_get(&format!("/markets/{ticker}/orderbook{depth_param}")).await
    }
}
