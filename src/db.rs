use crate::errors::{EngineError, EngineResult};
use crate::state::DbCommand;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub type DbPool = Arc<Mutex<Connection>>;

pub fn init_db(data_dir: &Path) -> EngineResult<DbPool> {
    std::fs::create_dir_all(data_dir).map_err(|e| EngineError::Database(format!("create dir: {e}")))?;
    let db_path = data_dir.join("pretty_rusty.db");
    let conn = Connection::open(&db_path)?;

    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA cache_size=-64000;")?;

    let schema = include_str!("../migrations/001_init.sql");
    conn.execute_batch(schema)?;

    tracing::info!("database initialized at {}", db_path.display());
    Ok(Arc::new(Mutex::new(conn)))
}

/// Dedicated DB writer task. Reads commands from bounded channel, executes SQL.
/// This is the ONLY task that touches the database connection.
pub async fn run_db_writer(db: DbPool, mut rx: mpsc::Receiver<DbCommand>) {
    tracing::info!("db writer task started");

    while let Some(cmd) = rx.recv().await {
        let result = execute_command(&db, cmd);
        if let Err(e) = result {
            tracing::error!("db write error: {e}");
        }
    }

    tracing::info!("db writer task shutting down");
}

fn execute_command(db: &DbPool, cmd: DbCommand) -> EngineResult<()> {
    let conn = db.lock().map_err(|e| EngineError::Database(format!("lock poisoned: {e}")))?;

    match cmd {
        DbCommand::InsertBtcPrice { timestamp, price } => {
            conn.execute(
                "INSERT INTO btc_prices (timestamp, price) VALUES (?1, ?2)",
                rusqlite::params![timestamp, price],
            )?;
        }
        DbCommand::InsertMarket {
            ticker, event_ticker, series_ticker, strike_price,
            open_time, close_time, expiration_time,
        } => {
            conn.execute(
                "INSERT OR REPLACE INTO markets (ticker, event_ticker, series_ticker, strike_price, open_time, close_time, expiration_time)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![ticker, event_ticker, series_ticker, strike_price, open_time, close_time, expiration_time],
            )?;
        }
        DbCommand::InsertTrade {
            id, model_name, market_ticker, side, action, entry_price,
            contracts, model_probability, ev, kelly_fraction, fees_estimate, entry_time,
        } => {
            conn.execute(
                "INSERT INTO trades (id, model_name, market_ticker, side, action, entry_price, contracts, model_probability, ev, kelly_fraction, fees_estimate, entry_time)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                rusqlite::params![id, model_name, market_ticker, side, action, entry_price, contracts, model_probability, ev, kelly_fraction, fees_estimate, entry_time],
            )?;
        }
        DbCommand::SettleTrade { trade_id, outcome, pnl, settle_time } => {
            conn.execute(
                "UPDATE trades SET outcome = ?1, pnl = ?2, settle_time = ?3 WHERE id = ?4",
                rusqlite::params![outcome, pnl, settle_time, trade_id],
            )?;
        }
        DbCommand::ExitTrade { trade_id, exit_price, pnl, reason, exit_time } => {
            conn.execute(
                "UPDATE trades SET outcome = ?1, pnl = ?2, settle_time = ?3 WHERE id = ?4",
                rusqlite::params![format!("exit:{reason}"), pnl, exit_time, trade_id],
            )?;
            let _ = exit_price; // stored implicitly in pnl
        }
        DbCommand::InsertSnapshot {
            model_name, timestamp, btc_price, market_ticker,
            probability, ev, kelly_size, cumulative_pnl, volatility, regime,
        } => {
            conn.execute(
                "INSERT INTO model_snapshots (model_name, timestamp, btc_price, market_ticker, probability, ev, kelly_size, cumulative_pnl, volatility, regime)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![model_name, timestamp, btc_price, market_ticker, probability, ev, kelly_size, cumulative_pnl, volatility, regime],
            )?;
        }
        DbCommand::UpdateRiskState {
            model_name, exposure, daily_pnl, max_drawdown, peak_equity,
            total_trades, winning_trades,
        } => {
            conn.execute(
                "INSERT OR REPLACE INTO risk_state (model_name, current_exposure, daily_pnl, max_drawdown, peak_equity, total_trades, winning_trades, last_updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))",
                rusqlite::params![model_name, exposure, daily_pnl, max_drawdown, peak_equity, total_trades, winning_trades],
            )?;
        }
        DbCommand::UpdateMarketResult { ticker, result, settlement_value } => {
            conn.execute(
                "UPDATE markets SET result = ?1, settlement_value = ?2 WHERE ticker = ?3",
                rusqlite::params![result, settlement_value, ticker],
            )?;
        }
        DbCommand::GetPendingTrades { market_ticker, reply } => {
            let trades = get_pending_trades_inner(&conn, &market_ticker)?;
            let _ = reply.send(trades);
        }
    }
    Ok(())
}

fn get_pending_trades_inner(conn: &Connection, market_ticker: &str) -> EngineResult<Vec<TradeRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, model_name, market_ticker, side, action, entry_price, contracts, model_probability, ev, kelly_fraction, outcome, pnl, fees_estimate, entry_time, settle_time FROM trades WHERE market_ticker = ?1 AND outcome IS NULL"
    )?;
    let rows = stmt.query_map(rusqlite::params![market_ticker], |row| {
        Ok(TradeRow {
            id: row.get(0)?,
            model_name: row.get(1)?,
            market_ticker: row.get(2)?,
            side: row.get(3)?,
            action: row.get(4)?,
            entry_price: row.get(5)?,
            contracts: row.get(6)?,
            model_probability: row.get(7)?,
            ev: row.get(8)?,
            kelly_fraction: row.get(9)?,
            outcome: row.get(10)?,
            pnl: row.get(11)?,
            fees_estimate: row.get(12)?,
            entry_time: row.get(13)?,
            settle_time: row.get(14)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

// ── Query helpers (for server REST reads -- these DO lock, but only from cold path) ──

pub fn get_recent_trades(db: &DbPool, model_name: Option<&str>, limit: usize) -> EngineResult<Vec<TradeRow>> {
    let conn = db.lock().map_err(|e| EngineError::Database(format!("lock: {e}")))?;
    let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match model_name {
        Some(name) => (
            "SELECT id, model_name, market_ticker, side, action, entry_price, contracts, model_probability, ev, kelly_fraction, outcome, pnl, fees_estimate, entry_time, settle_time FROM trades WHERE model_name = ?1 ORDER BY entry_time DESC LIMIT ?2".into(),
            vec![Box::new(name.to_string()), Box::new(limit as i64)],
        ),
        None => (
            "SELECT id, model_name, market_ticker, side, action, entry_price, contracts, model_probability, ev, kelly_fraction, outcome, pnl, fees_estimate, entry_time, settle_time FROM trades ORDER BY entry_time DESC LIMIT ?1".into(),
            vec![Box::new(limit as i64)],
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
        Ok(TradeRow {
            id: row.get(0)?,
            model_name: row.get(1)?,
            market_ticker: row.get(2)?,
            side: row.get(3)?,
            action: row.get(4)?,
            entry_price: row.get(5)?,
            contracts: row.get(6)?,
            model_probability: row.get(7)?,
            ev: row.get(8)?,
            kelly_fraction: row.get(9)?,
            outcome: row.get(10)?,
            pnl: row.get(11)?,
            fees_estimate: row.get(12)?,
            entry_time: row.get(13)?,
            settle_time: row.get(14)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn get_model_pnl_series(db: &DbPool, model_name: &str, limit: usize) -> EngineResult<Vec<(String, f64)>> {
    let conn = db.lock().map_err(|e| EngineError::Database(format!("lock: {e}")))?;
    let mut stmt = conn.prepare(
        "SELECT timestamp, cumulative_pnl FROM model_snapshots WHERE model_name = ?1 ORDER BY id DESC LIMIT ?2"
    )?;
    let rows = stmt.query_map(rusqlite::params![model_name, limit], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
    })?;
    let mut series: Vec<_> = rows.filter_map(|r| r.ok()).collect();
    series.reverse();
    Ok(series)
}

pub fn get_risk_states(db: &DbPool) -> EngineResult<Vec<RiskStateRow>> {
    let conn = db.lock().map_err(|e| EngineError::Database(format!("lock: {e}")))?;
    let mut stmt = conn.prepare(
        "SELECT model_name, current_exposure, daily_pnl, max_drawdown, peak_equity, total_trades, winning_trades, last_updated FROM risk_state"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(RiskStateRow {
            model_name: row.get(0)?,
            current_exposure: row.get(1)?,
            daily_pnl: row.get(2)?,
            max_drawdown: row.get(3)?,
            peak_equity: row.get(4)?,
            total_trades: row.get(5)?,
            winning_trades: row.get(6)?,
            last_updated: row.get(7)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

// ── Row types ──

#[derive(Debug, Clone, serde::Serialize)]
pub struct TradeRow {
    pub id: String,
    pub model_name: String,
    pub market_ticker: String,
    pub side: String,
    pub action: String,
    pub entry_price: f64,
    pub contracts: f64,
    pub model_probability: f64,
    pub ev: f64,
    pub kelly_fraction: f64,
    pub outcome: Option<String>,
    pub pnl: Option<f64>,
    pub fees_estimate: f64,
    pub entry_time: String,
    pub settle_time: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RiskStateRow {
    pub model_name: String,
    pub current_exposure: f64,
    pub daily_pnl: f64,
    pub max_drawdown: f64,
    pub peak_equity: f64,
    pub total_trades: i64,
    pub winning_trades: i64,
    pub last_updated: String,
}
