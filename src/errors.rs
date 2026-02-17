/// Domain-specific error types for the trading engine.
/// All external failures must be handled. The engine must:
/// - Continue running on recoverable errors
/// - Halt safely on unrecoverable state corruption
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("network error: {0}")]
    Network(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("kalshi API error: {status} {body}")]
    KalshiApi { status: u16, body: String },

    #[error("crypto feed error: {0}")]
    CryptoFeed(String),

    #[error("model computation error: {0}")]
    Model(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("auth error: {0}")]
    Auth(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("channel closed: {0}")]
    ChannelClosed(String),

    #[error("risk limit breached: {0}")]
    RiskLimit(String),

    #[error("state corruption: {0}")]
    StateCorruption(String),
}

impl From<reqwest::Error> for EngineError {
    fn from(e: reqwest::Error) -> Self {
        EngineError::Network(e.to_string())
    }
}

impl From<serde_json::Error> for EngineError {
    fn from(e: serde_json::Error) -> Self {
        EngineError::Parse(e.to_string())
    }
}

impl From<rusqlite::Error> for EngineError {
    fn from(e: rusqlite::Error) -> Self {
        EngineError::Database(e.to_string())
    }
}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        EngineError::Network(e.to_string())
    }
}

pub type EngineResult<T> = Result<T, EngineError>;
