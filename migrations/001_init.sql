-- Markets we've tracked
CREATE TABLE IF NOT EXISTS markets (
    ticker TEXT PRIMARY KEY,
    event_ticker TEXT NOT NULL,
    series_ticker TEXT NOT NULL,
    strike_price REAL,
    open_time TEXT NOT NULL,
    close_time TEXT NOT NULL,
    expiration_time TEXT NOT NULL,
    result TEXT,  -- 'yes', 'no', NULL if not yet settled
    settlement_value REAL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Paper trades per model
CREATE TABLE IF NOT EXISTS trades (
    id TEXT PRIMARY KEY,
    model_name TEXT NOT NULL,
    market_ticker TEXT NOT NULL,
    side TEXT NOT NULL,       -- 'yes' or 'no'
    action TEXT NOT NULL,     -- 'buy' or 'sell'
    entry_price REAL NOT NULL,
    contracts REAL NOT NULL,
    model_probability REAL NOT NULL,
    ev REAL NOT NULL,
    kelly_fraction REAL NOT NULL,
    outcome TEXT,             -- 'win', 'loss', NULL if pending
    pnl REAL,
    fees_estimate REAL NOT NULL DEFAULT 0.0,
    entry_time TEXT NOT NULL,
    settle_time TEXT,
    FOREIGN KEY (market_ticker) REFERENCES markets(ticker)
);

-- Time-series snapshots of model state (every tick)
CREATE TABLE IF NOT EXISTS model_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model_name TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    btc_price REAL NOT NULL,
    market_ticker TEXT,
    probability REAL,
    ev REAL,
    kelly_size REAL,
    cumulative_pnl REAL NOT NULL DEFAULT 0.0,
    volatility REAL,
    regime TEXT
);

-- Calibration buckets per model
CREATE TABLE IF NOT EXISTS calibration_buckets (
    model_name TEXT NOT NULL,
    bucket_lower REAL NOT NULL,
    bucket_upper REAL NOT NULL,
    predicted_count INTEGER NOT NULL DEFAULT 0,
    realized_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (model_name, bucket_lower)
);

-- BTC price history (for volatility computation on restart)
CREATE TABLE IF NOT EXISTS btc_prices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    price REAL NOT NULL,
    source TEXT NOT NULL DEFAULT 'freecryptoapi'
);

-- Risk state per model
CREATE TABLE IF NOT EXISTS risk_state (
    model_name TEXT PRIMARY KEY,
    current_exposure REAL NOT NULL DEFAULT 0.0,
    daily_pnl REAL NOT NULL DEFAULT 0.0,
    max_drawdown REAL NOT NULL DEFAULT 0.0,
    peak_equity REAL NOT NULL DEFAULT 0.0,
    total_trades INTEGER NOT NULL DEFAULT 0,
    winning_trades INTEGER NOT NULL DEFAULT 0,
    last_updated TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Create indexes for common queries
CREATE INDEX IF NOT EXISTS idx_trades_model ON trades(model_name, entry_time);
CREATE INDEX IF NOT EXISTS idx_trades_market ON trades(market_ticker);
CREATE INDEX IF NOT EXISTS idx_snapshots_model_time ON model_snapshots(model_name, timestamp);
CREATE INDEX IF NOT EXISTS idx_btc_prices_time ON btc_prices(timestamp);
