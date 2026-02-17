use crate::errors::{EngineError, EngineResult};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub kalshi_api_key_id: String,
    pub kalshi_private_key_path: PathBuf,
    pub kalshi_base_url: String,
    pub crypto_api_key: String,
    pub crypto_api_base_url: String,
    pub btc_series_ticker: String,
    pub fractional_kelly: f64,
    pub max_position_size: f64,
    pub ev_threshold: f64,
    pub max_daily_drawdown: f64,
    pub server_port: u16,
}

impl AppConfig {
    pub fn from_env() -> EngineResult<Self> {
        dotenvy::dotenv().ok();

        let fractional_kelly = env_var_or("FRACTIONAL_KELLY", "0.2")
            .parse::<f64>()
            .map_err(|e| EngineError::Config(format!("FRACTIONAL_KELLY: {e}")))?;

        let max_position_size = env_var_or("MAX_POSITION_SIZE", "50")
            .parse::<f64>()
            .map_err(|e| EngineError::Config(format!("MAX_POSITION_SIZE: {e}")))?;

        let ev_threshold = env_var_or("EV_THRESHOLD", "0.02")
            .parse::<f64>()
            .map_err(|e| EngineError::Config(format!("EV_THRESHOLD: {e}")))?;

        let max_daily_drawdown = env_var_or("MAX_DAILY_DRAWDOWN", "100.0")
            .parse::<f64>()
            .map_err(|e| EngineError::Config(format!("MAX_DAILY_DRAWDOWN: {e}")))?;

        let server_port = env_var_or("SERVER_PORT", "3001")
            .parse::<u16>()
            .map_err(|e| EngineError::Config(format!("SERVER_PORT: {e}")))?;

        Ok(Self {
            kalshi_api_key_id: env_var("KALSHI_API_KEY_ID")?,
            kalshi_private_key_path: PathBuf::from(env_var("KALSHI_PRIVATE_KEY_PATH")?),
            kalshi_base_url: env_var_or(
                "KALSHI_BASE_URL",
                "https://api.elections.kalshi.com/trade-api/v2",
            ),
            crypto_api_key: env_var("CRYPTO_API_KEY")?,
            crypto_api_base_url: env_var_or(
                "CRYPTO_API_BASE_URL",
                "https://api.freecryptoapi.com/v1",
            ),
            btc_series_ticker: env_var_or("BTC_SERIES_TICKER", "KXBTCD"),
            fractional_kelly,
            max_position_size,
            ev_threshold,
            max_daily_drawdown,
            server_port,
        })
    }
}

fn env_var(key: &str) -> EngineResult<String> {
    std::env::var(key).map_err(|_| EngineError::Config(format!("missing env var: {key}")))
}

fn env_var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
