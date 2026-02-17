use crate::db::DbPool;
use crate::config::AppConfig;
use smallvec::SmallVec;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};
use portable_atomic::{AtomicU64, Ordering};

// ── Engine State Machine ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineState {
    Connecting,
    Syncing,
    Trading,
    Halted,
}

impl std::fmt::Display for EngineState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connecting => write!(f, "connecting"),
            Self::Syncing => write!(f, "syncing"),
            Self::Trading => write!(f, "trading"),
            Self::Halted => write!(f, "halted"),
        }
    }
}

// ── Deterministic Decision Types ──

#[derive(Debug, Clone, serde::Serialize)]
pub struct PaperOrder {
    pub model: &'static str,
    pub market_ticker: String,
    pub side: &'static str,
    pub action: &'static str,
    pub price: f64,
    pub contracts: f64,
    pub probability: f64,
    pub ev: f64,
    pub kelly_fraction: f64,
}

#[derive(Debug, Clone)]
pub enum Decision {
    PlacePaperTrade(PaperOrder),
    NoAction { reason: &'static str },
}

// ── Messages INTO the engine (bounded channels) ──

#[derive(Debug, Clone)]
pub enum EngineEvent {
    BtcPrice { price: f64, timestamp_ms: i64 },
    MarketUpdate(Box<ActiveMarket>),
    MarketSettled { ticker: String, result: String },
    Tick,
    Shutdown,
}

// ── Messages OUT of the engine ──

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    #[serde(rename = "btc_price")]
    BtcPrice { price: f64, timestamp: String },

    #[serde(rename = "market_state")]
    MarketState {
        ticker: String,
        strike: Option<f64>,
        ttl_seconds: f64,
        yes_bid: Option<String>,
        yes_ask: Option<String>,
        status: String,
    },

    #[serde(rename = "model_update")]
    ModelUpdate {
        model: String,
        probability: f64,
        ev: f64,
        kelly_size: f64,
        cumulative_pnl: f64,
        unrealized_pnl: f64,
        total_pnl: f64,
        total_trades: i64,
        winning_trades: i64,
        sharpe: f64,
        max_drawdown: f64,
        brier_score: f64,
        daily_pnl: f64,
        current_exposure: f64,
        open_position_count: usize,
    },

    #[serde(rename = "new_trade")]
    NewTrade {
        model: String,
        side: String,
        action: String,
        price: f64,
        contracts: f64,
        ev: f64,
        timestamp: String,
    },

    #[serde(rename = "trade_exited")]
    TradeExited {
        model: String,
        trade_id: String,
        side: String,
        entry_price: f64,
        exit_price: f64,
        contracts: f64,
        pnl: f64,
        reason: String,
        timestamp: String,
    },

    #[serde(rename = "trade_settled")]
    TradeSettled {
        model: String,
        trade_id: String,
        outcome: String,
        pnl: f64,
        timestamp: String,
    },

    #[serde(rename = "metrics_update")]
    MetricsUpdate {
        model: String,
        sharpe: f64,
        max_drawdown: f64,
        win_rate: f64,
        brier: f64,
        total_trades: i64,
        daily_pnl: f64,
    },

    #[serde(rename = "engine_state")]
    EngineStateMsg {
        state: String,
        reason: String,
    },
}

// ── DB Commands (sent to writer task via bounded channel) ──

#[derive(Debug)]
pub enum DbCommand {
    InsertBtcPrice { timestamp: String, price: f64 },
    InsertMarket {
        ticker: String,
        event_ticker: String,
        series_ticker: String,
        strike_price: Option<f64>,
        open_time: String,
        close_time: String,
        expiration_time: String,
    },
    InsertTrade {
        id: String,
        model_name: String,
        market_ticker: String,
        side: String,
        action: String,
        entry_price: f64,
        contracts: f64,
        model_probability: f64,
        ev: f64,
        kelly_fraction: f64,
        fees_estimate: f64,
        entry_time: String,
    },
    SettleTrade {
        trade_id: String,
        outcome: String,
        pnl: f64,
        settle_time: String,
    },
    ExitTrade {
        trade_id: String,
        exit_price: f64,
        pnl: f64,
        reason: String,
        exit_time: String,
    },
    InsertSnapshot {
        model_name: String,
        timestamp: String,
        btc_price: f64,
        market_ticker: Option<String>,
        probability: Option<f64>,
        ev: Option<f64>,
        kelly_size: Option<f64>,
        cumulative_pnl: f64,
        volatility: Option<f64>,
        regime: Option<String>,
    },
    UpdateRiskState {
        model_name: String,
        exposure: f64,
        daily_pnl: f64,
        max_drawdown: f64,
        peak_equity: f64,
        total_trades: i64,
        winning_trades: i64,
    },
    UpdateMarketResult {
        ticker: String,
        result: String,
        settlement_value: Option<f64>,
    },
    GetPendingTrades {
        market_ticker: String,
        reply: tokio::sync::oneshot::Sender<Vec<crate::db::TradeRow>>,
    },
}

// ── Active Market (stack-friendly) ──

#[derive(Debug, Clone, serde::Serialize)]
pub struct ActiveMarket {
    pub ticker: String,
    pub event_ticker: String,
    pub series_ticker: String,
    pub strike: Option<f64>,
    pub yes_bid: Option<String>,
    pub yes_ask: Option<String>,
    pub no_bid: Option<String>,
    pub no_ask: Option<String>,
    pub last_price: Option<String>,
    pub close_time: String,
    pub expiration_time: String,
    pub status: String,
    pub result: Option<String>,
}

// ── Per-Model State ──

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelState {
    pub name: &'static str,
    pub probability: f64,
    pub ev: f64,
    pub kelly_size: f64,
    pub cumulative_pnl: f64,
    pub total_trades: i64,
    pub winning_trades: i64,
    pub sharpe: f64,
    pub max_drawdown: f64,
    pub brier_score: f64,
    pub daily_pnl: f64,
    pub current_exposure: f64,
    pub peak_equity: f64,
    /// Rolling returns for Sharpe (capped at 500)
    #[serde(skip)]
    pub trade_returns: VecDeque<f64>,
    /// Beta distribution params for Bayesian Kelly
    pub beta_alpha: f64,
    pub beta_beta: f64,
    /// Brier score accumulators
    #[serde(skip)]
    pub brier_sum: f64,
    #[serde(skip)]
    pub brier_count: i64,
    /// Live unrealized P/L from open positions (mark-to-market)
    pub unrealized_pnl: f64,
    /// Open positions for this model (replaces simple trade ID list)
    pub open_positions: SmallVec<[OpenPosition; 4]>,
}

/// A live open paper trade position with full details for MTM + adaptive management.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OpenPosition {
    pub trade_id: String,
    pub market_ticker: String,
    pub side: String,
    pub entry_price: f64,
    pub contracts: f64,
    pub model_probability: f64,
    /// Tick counter at entry (for hold-time tracking)
    pub entry_tick: u64,
    /// BTC price at time of entry (for strike-relative tracking)
    pub entry_btc_price: f64,
    /// Highest unrealized P/L seen (for trailing stop)
    pub peak_unrealized: f64,
    /// Which "leg" this is (0 = initial, 1+ = scale-ins)
    pub leg: u32,
}

impl ModelState {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            probability: 0.0,
            ev: 0.0,
            kelly_size: 0.0,
            cumulative_pnl: 0.0,
            total_trades: 0,
            winning_trades: 0,
            sharpe: 0.0,
            max_drawdown: 0.0,
            brier_score: 0.0,
            daily_pnl: 0.0,
            current_exposure: 0.0,
            peak_equity: 0.0,
            trade_returns: VecDeque::with_capacity(500),
            beta_alpha: 20.0,
            beta_beta: 20.0,
            brier_sum: 0.0,
            brier_count: 0,
            unrealized_pnl: 0.0,
            open_positions: SmallVec::new(),
        }
    }

    #[inline]
    pub fn win_rate(&self) -> f64 {
        if self.total_trades == 0 {
            return 0.0;
        }
        self.winning_trades as f64 / self.total_trades as f64
    }

    pub fn compute_sharpe(&mut self) {
        let n = self.trade_returns.len();
        if n < 2 {
            self.sharpe = 0.0;
            return;
        }
        let nf = n as f64;
        let mean = self.trade_returns.iter().sum::<f64>() / nf;
        let var = self.trade_returns.iter().map(|r| (r - mean) * (r - mean)).sum::<f64>() / (nf - 1.0);
        let std = var.sqrt();
        if std < 1e-12 {
            self.sharpe = 0.0;
            return;
        }
        // ~96 trades/day (every 15 min), annualize
        let annualization = (96.0_f64 * 365.0).sqrt();
        self.sharpe = (mean / std) * annualization;
    }

    #[inline]
    pub fn compute_brier(&mut self) {
        self.brier_score = if self.brier_count == 0 {
            0.0
        } else {
            self.brier_sum / self.brier_count as f64
        };
    }

    pub fn record_return(&mut self, ret: f64) {
        if self.trade_returns.len() >= 500 {
            self.trade_returns.pop_front();
        }
        self.trade_returns.push_back(ret);
    }

    pub fn update_drawdown(&mut self) {
        if self.cumulative_pnl > self.peak_equity {
            self.peak_equity = self.cumulative_pnl;
        }
        let dd = self.peak_equity - self.cumulative_pnl;
        if dd > self.max_drawdown {
            self.max_drawdown = dd;
        }
    }
}

// ── Volatility State (stack-allocated, no heap) ──

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[repr(C)]
pub struct VolatilityState {
    pub ewma_vol: f64,
    pub jump_intensity: f64,
    pub jump_mean: f64,
    pub jump_var: f64,
    pub student_t_nu: f64,
    pub regime: VolRegime,
    pub sample_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VolRegime {
    Low,
    High,
}

impl std::fmt::Display for VolRegime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::High => write!(f, "high"),
        }
    }
}

impl Default for VolatilityState {
    fn default() -> Self {
        Self {
            ewma_vol: 0.01,
            jump_intensity: 0.5,
            jump_mean: 0.0,
            jump_var: 0.0001,
            student_t_nu: 5.0,
            regime: VolRegime::Low,
            sample_count: 0,
        }
    }
}

// ── Precomputed model parameters (stack, no alloc) ──

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ModelParams {
    pub spot: f64,
    pub strike: f64,
    pub ttl_years: f64,
    pub sigma: f64,
    // Precomputed
    pub ln_s_k: f64,
    pub sqrt_t: f64,
    pub sigma_sqrt_t: f64,
    pub half_sigma_sq: f64,
}

impl ModelParams {
    #[inline]
    pub fn new(spot: f64, strike: f64, ttl_seconds: f64, sigma: f64) -> Self {
        let ttl_years = ttl_seconds / (365.25 * 24.0 * 3600.0);
        let ln_s_k = (spot / strike).ln();
        let sqrt_t = ttl_years.sqrt();
        let sigma_sqrt_t = sigma * sqrt_t;
        let half_sigma_sq = 0.5 * sigma * sigma;
        Self {
            spot,
            strike,
            ttl_years,
            sigma,
            ln_s_k,
            sqrt_t,
            sigma_sqrt_t,
            half_sigma_sq,
        }
    }
}

// ── Engine snapshot for dashboard (sent via watch channel) ──

#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineSnapshot {
    pub engine_state: EngineState,
    pub btc_price: f64,
    pub btc_timestamp: String,
    pub active_market: Option<ActiveMarket>,
    pub volatility: VolatilityState,
    pub models: Vec<ModelState>,
}

impl Default for EngineSnapshot {
    fn default() -> Self {
        Self {
            engine_state: EngineState::Connecting,
            btc_price: 0.0,
            btc_timestamp: String::new(),
            active_market: None,
            volatility: VolatilityState::default(),
            models: vec![
                ModelState::new("Black-Scholes"),
                ModelState::new("Jump-Diffusion"),
                ModelState::new("Student-t"),
            ],
        }
    }
}

// ── Performance Counters (lock-free) ──

pub struct PerfCounters {
    pub ticks_processed: AtomicU64,
    pub prices_received: AtomicU64,
    pub decisions_made: AtomicU64,
    pub trades_placed: AtomicU64,
    pub errors_recovered: AtomicU64,
    pub ws_messages_sent: AtomicU64,
}

impl PerfCounters {
    pub fn new() -> Self {
        Self {
            ticks_processed: AtomicU64::new(0),
            prices_received: AtomicU64::new(0),
            decisions_made: AtomicU64::new(0),
            trades_placed: AtomicU64::new(0),
            errors_recovered: AtomicU64::new(0),
            ws_messages_sent: AtomicU64::new(0),
        }
    }
}

// ── Application shared state (channels, not locks) ──

pub struct AppState {
    pub config: AppConfig,
    pub db: DbPool,

    // Engine -> Dashboard: latest snapshot (watch = single producer, multi consumer)
    pub snapshot_tx: watch::Sender<EngineSnapshot>,
    pub snapshot_rx: watch::Receiver<EngineSnapshot>,

    // Engine -> Dashboard: event stream (broadcast for WS clients)
    pub ws_tx: broadcast::Sender<WsMessage>,

    // Feed/Scanner -> Engine: bounded event channel
    pub engine_tx: mpsc::Sender<EngineEvent>,

    // Engine -> DB Writer: bounded command channel
    pub db_tx: mpsc::Sender<DbCommand>,

    // Lock-free performance counters
    pub counters: PerfCounters,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        db: DbPool,
        engine_tx: mpsc::Sender<EngineEvent>,
        db_tx: mpsc::Sender<DbCommand>,
    ) -> Arc<Self> {
        let (ws_tx, _) = broadcast::channel(2048);
        let (snapshot_tx, snapshot_rx) = watch::channel(EngineSnapshot::default());

        Arc::new(Self {
            config,
            db,
            snapshot_tx,
            snapshot_rx,
            ws_tx,
            engine_tx,
            db_tx,
            counters: PerfCounters::new(),
        })
    }

    #[inline]
    pub fn broadcast(&self, msg: WsMessage) {
        self.counters.ws_messages_sent.fetch_add(1, Ordering::Relaxed);
        let _ = self.ws_tx.send(msg);
    }
}
