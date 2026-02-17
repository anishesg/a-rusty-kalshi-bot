use crate::db;
use crate::paper::tracker;
use crate::state::{AppState, EngineSnapshot};
use axum::extract::{Query, State};
use axum::response::Json;
use std::sync::Arc;

#[derive(serde::Deserialize)]
pub struct TradesQuery {
    pub model: Option<String>,
    pub limit: Option<usize>,
}

#[derive(serde::Deserialize)]
pub struct PnlQuery {
    pub model: String,
    pub limit: Option<usize>,
}

/// GET /api/state -- current engine snapshot (from watch channel, no lock)
pub async fn get_state(
    State(state): State<Arc<AppState>>,
) -> Json<EngineSnapshot> {
    let snapshot = state.snapshot_rx.borrow().clone();
    Json(snapshot)
}

/// GET /api/trades -- recent trades from DB (cold path)
pub async fn get_trades(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TradesQuery>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(50).min(200);
    match db::get_recent_trades(&state.db, params.model.as_deref(), limit) {
        Ok(trades) => Json(serde_json::json!({ "trades": trades })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// GET /api/pnl -- P/L time series from DB (cold path)
pub async fn get_pnl(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PnlQuery>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(500).min(5000);
    match db::get_model_pnl_series(&state.db, &params.model, limit) {
        Ok(series) => Json(serde_json::json!({
            "model": params.model,
            "series": series.iter().map(|(t, v)| serde_json::json!({"t": t, "pnl": v})).collect::<Vec<_>>()
        })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// GET /api/metrics -- aggregate metrics (from watch channel snapshot)
pub async fn get_metrics(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let snapshot = state.snapshot_rx.borrow().clone();
    let metrics = tracker::compute_aggregate(&snapshot.models);
    Json(serde_json::json!(metrics))
}

/// GET /api/risk -- risk states from DB
pub async fn get_risk(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    match db::get_risk_states(&state.db) {
        Ok(states) => Json(serde_json::json!({ "risk": states })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// GET /api/counters -- performance counters (lock-free reads)
pub async fn get_counters(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    use portable_atomic::Ordering::Relaxed;
    Json(serde_json::json!({
        "ticks_processed": state.counters.ticks_processed.load(Relaxed),
        "prices_received": state.counters.prices_received.load(Relaxed),
        "decisions_made": state.counters.decisions_made.load(Relaxed),
        "trades_placed": state.counters.trades_placed.load(Relaxed),
        "errors_recovered": state.counters.errors_recovered.load(Relaxed),
        "ws_messages_sent": state.counters.ws_messages_sent.load(Relaxed),
    }))
}
