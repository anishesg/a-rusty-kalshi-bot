/// Performance metrics computation.
/// All functions are pure -- they take state and return computed values.

use crate::state::ModelState;

/// Aggregate metrics for all models. Used by REST endpoints.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AggregateMetrics {
    pub models: Vec<ModelMetrics>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelMetrics {
    pub name: String,
    pub cumulative_pnl: f64,
    pub daily_pnl: f64,
    pub total_trades: i64,
    pub winning_trades: i64,
    pub win_rate: f64,
    pub sharpe: f64,
    pub max_drawdown: f64,
    pub brier_score: f64,
    pub current_exposure: f64,
    pub kelly_size: f64,
    pub probability: f64,
    pub ev: f64,
}

/// Compute aggregate metrics from model states. Pure function.
pub fn compute_aggregate(models: &[ModelState]) -> AggregateMetrics {
    let metrics = models
        .iter()
        .map(|m| ModelMetrics {
            name: m.name.to_string(),
            cumulative_pnl: m.cumulative_pnl,
            daily_pnl: m.daily_pnl,
            total_trades: m.total_trades,
            winning_trades: m.winning_trades,
            win_rate: m.win_rate(),
            sharpe: m.sharpe,
            max_drawdown: m.max_drawdown,
            brier_score: m.brier_score,
            current_exposure: m.current_exposure,
            kelly_size: m.kelly_size,
            probability: m.probability,
            ev: m.ev,
        })
        .collect();

    AggregateMetrics { models: metrics }
}
