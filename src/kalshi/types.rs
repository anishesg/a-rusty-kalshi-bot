use serde::{Deserialize, Serialize};

// ── Market ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    pub ticker: Option<String>,
    pub event_ticker: Option<String>,
    pub market_type: Option<String>,
    pub status: Option<String>,
    pub yes_bid: Option<i64>,
    pub yes_ask: Option<i64>,
    pub no_bid: Option<i64>,
    pub no_ask: Option<i64>,
    pub yes_bid_dollars: Option<String>,
    pub yes_ask_dollars: Option<String>,
    pub no_bid_dollars: Option<String>,
    pub no_ask_dollars: Option<String>,
    pub last_price: Option<i64>,
    pub last_price_dollars: Option<String>,
    pub volume: Option<i64>,
    pub volume_fp: Option<String>,
    pub open_interest: Option<i64>,
    pub open_interest_fp: Option<String>,
    pub open_time: Option<String>,
    pub close_time: Option<String>,
    pub expiration_time: Option<String>,
    pub latest_expiration_time: Option<String>,
    pub result: Option<String>,
    pub settlement_value: Option<String>,
    pub settlement_value_dollars: Option<String>,
    pub settlement_ts: Option<String>,
    pub floor_strike: Option<f64>,
    pub cap_strike: Option<f64>,
    pub strike_type: Option<String>,
    pub can_close_early: Option<bool>,
}

impl Market {
    #[inline]
    pub fn strike_price(&self) -> Option<f64> {
        self.floor_strike.or(self.cap_strike)
    }

    #[inline]
    pub fn yes_bid_f64(&self) -> Option<f64> {
        parse_fixed_point(self.yes_bid_dollars.as_deref()?)
    }

    #[inline]
    pub fn yes_ask_f64(&self) -> Option<f64> {
        parse_fixed_point(self.yes_ask_dollars.as_deref()?)
    }

    #[inline]
    pub fn no_bid_f64(&self) -> Option<f64> {
        parse_fixed_point(self.no_bid_dollars.as_deref()?)
    }

    #[inline]
    pub fn last_price_f64(&self) -> Option<f64> {
        parse_fixed_point(self.last_price_dollars.as_deref()?)
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(self.status.as_deref(), Some("active") | Some("open"))
    }

    #[inline]
    pub fn is_settled(&self) -> bool {
        // Kalshi uses "determined" for resolved, then "finalized"/"settled" later.
        // Also check if result field is non-empty as a secondary signal.
        let status_settled = matches!(
            self.status.as_deref(),
            Some("determined") | Some("finalized") | Some("settled") | Some("closed")
        );
        let has_result = self.result.as_deref().is_some_and(|r| !r.is_empty());
        status_settled || has_result
    }

    #[inline]
    pub fn ticker_str(&self) -> &str {
        self.ticker.as_deref().unwrap_or("")
    }
}

#[inline]
fn parse_fixed_point(s: &str) -> Option<f64> {
    s.parse::<f64>().ok()
}

// ── Responses ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetMarketsResponse {
    pub markets: Option<Vec<Market>>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetMarketResponse {
    pub market: Option<Market>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookResponse {
    pub orderbook: Option<Orderbook>,
    pub orderbook_fp: Option<OrderbookFp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Orderbook {
    pub yes: Option<Vec<Vec<serde_json::Value>>>,
    pub no: Option<Vec<Vec<serde_json::Value>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookFp {
    pub yes_dollars: Option<Vec<Vec<String>>>,
    pub no_dollars: Option<Vec<Vec<String>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub trade_id: Option<String>,
    pub ticker: Option<String>,
    pub yes_price_dollars: Option<String>,
    pub no_price_dollars: Option<String>,
    pub count_fp: Option<String>,
    pub taker_side: Option<String>,
    pub created_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTradesResponse {
    pub trades: Option<Vec<Trade>>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventData {
    pub event_ticker: Option<String>,
    pub series_ticker: Option<String>,
    pub title: Option<String>,
    pub sub_title: Option<String>,
    pub mutually_exclusive: Option<bool>,
    pub category: Option<String>,
    pub markets: Option<Vec<Market>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEventsResponse {
    pub events: Option<Vec<EventData>>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetEventResponse {
    pub event: Option<EventData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Series {
    pub ticker: Option<String>,
    pub title: Option<String>,
    pub frequency: Option<String>,
    pub category: Option<String>,
    pub fee_type: Option<String>,
    pub fee_multiplier: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSeriesResponse {
    pub series: Option<Vec<Series>>,
}
