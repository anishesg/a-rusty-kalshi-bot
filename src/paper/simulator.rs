use crate::execution::ev::{self, EvParams};
use crate::models::calibration::Calibrator;
use crate::models::{PricingModel, VolContext};
use crate::risk::kelly::{self, KellyParams};
use crate::risk::limits;
use crate::state::*;
use crate::config::AppConfig;
use smallvec::SmallVec;

/// Output actions from the engine's decision loop.
#[derive(Debug)]
pub enum EngineAction {
    PlaceTrade {
        id: String,
        model_name: &'static str,
        market_ticker: String,
        side: &'static str,
        action: &'static str,
        price: f64,
        contracts: f64,
        probability: f64,
        ev: f64,
        kelly_fraction: f64,
    },
    ExitTrade {
        trade_id: String,
        model_name: &'static str,
        exit_price: f64,
        pnl: f64,
        reason: &'static str,
    },
    SettleTrade {
        trade_id: String,
        model_name: String,
        outcome: &'static str,
        pnl: f64,
    },
    BroadcastUpdate(WsMessage),
    DbWrite(DbCommand),
}

// ═══════════════════════════════════════════════════════════════════════════════
// ADAPTIVE BINARY OPTIONS STRATEGY
//
// Key insight: Binary contracts MUST resolve to $0 or $1.
// The only thing that matters is whether BTC is above or below the strike
// at expiry. This creates fundamentally different dynamics than equities.
//
// STRATEGY RULES:
// 1. STRIKE CROSSOVER EXIT: If BTC crosses the strike against our position,
//    exit immediately. This is THE most important rule.
// 2. SCALE INTO WINNERS: If we're holding and BTC moves further in our favor,
//    add another leg. The contract converges to $1 as certainty increases.
// 3. TRAILING STOP: Track peak unrealized P/L, exit if it drops by 50% from peak.
// 4. TIME-AWARE SIZING: As expiry approaches with us on the right side,
//    contracts become more valuable. Hold or add.
// 5. RESOLUTION HOLD: If within 2 min of close and BTC is strongly on our side
//    (>$200 from strike), hold to resolution for maximum $1 payout.
// 6. EARLY EXIT: If 2-4 min from close and not clearly winning, exit to avoid
//    the coin-flip zone.
// ═══════════════════════════════════════════════════════════════════════════════

/// BTC must cross strike by this $ amount against us to trigger hard exit
const STRIKE_CROSS_BUFFER: f64 = 25.0;
/// BTC must move this $ amount further in our favor to trigger a scale-in
const SCALE_IN_MOVE: f64 = 75.0;
/// Max legs per model (initial + scale-ins)
const MAX_LEGS: u32 = 3;
/// Trailing stop: exit if unrealized drops this fraction below peak
const TRAILING_STOP_PCT: f64 = 0.50;
/// Take partial profit: sell half when unrealized > this % of cost
const PARTIAL_TAKE_PROFIT_PCT: f64 = 0.40;
/// Hard take profit: sell everything when unrealized > this % of cost
const FULL_TAKE_PROFIT_PCT: f64 = 0.80;
/// Time exit: exit if not clearly winning within this many seconds of close
const UNCERTAIN_EXIT_SECONDS: f64 = 240.0;
/// BTC must be this far from strike to hold to resolution
const RESOLUTION_HOLD_DISTANCE: f64 = 200.0;
/// Don't exit if this close to expiry and clearly winning (let it resolve at $1)
const RESOLUTION_HOLD_SECONDS: f64 = 120.0;
/// Minimum hold time before any exit (ticks, ~1 tick/second)
const MIN_HOLD_TICKS: u64 = 5;
/// Don't enter with less than this many seconds to expiry
const MIN_ENTRY_TTL: f64 = 300.0;
/// Stop-loss: hard cut at this % of entry cost
const HARD_STOP_LOSS_PCT: f64 = 0.70;

/// Run the engine decision loop for a single tick.
///
/// Four phases per tick:
///   1. Mark-to-market: update unrealized P/L + peak tracking
///   2. Exit check: strike crossover, trailing stop, time-based, hard stop
///   3. Scale-in check: add to winners when BTC moves further in our favor
///   4. Entry check: new position when model detects edge
#[allow(clippy::too_many_arguments)]
pub fn run_tick(
    pricing_models: &[&dyn PricingModel],
    model_states: &mut [ModelState],
    calibrators: &mut [Calibrator],
    vol_state: &VolatilityState,
    active_market: &Option<ActiveMarket>,
    btc_price: f64,
    config: &AppConfig,
    timestamp: &str,
    tick_counter: u64,
) -> SmallVec<[EngineAction; 16]> {
    let mut actions: SmallVec<[EngineAction; 16]> = SmallVec::new();

    let Some(market) = active_market else {
        for state in model_states.iter_mut() {
            state.unrealized_pnl = 0.0;
        }
        return actions;
    };

    let strike = match market.strike {
        Some(s) if s > 0.0 => s,
        _ => return actions,
    };

    let yes_ask = market
        .yes_ask
        .as_ref()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    let yes_bid = market
        .yes_bid
        .as_ref()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    if yes_ask <= 0.0 || yes_ask >= 1.0 {
        return actions;
    }

    let ttl_seconds = compute_ttl(&market.close_time);
    if ttl_seconds <= 0.0 {
        return actions;
    }

    let annualized_sigma = vol_state.ewma_vol * (365.25_f64 * 24.0 * 3600.0 / 2.0).sqrt();
    let params = ModelParams::new(btc_price, strike, ttl_seconds, annualized_sigma);

    let vol_ctx = VolContext {
        jump_intensity: vol_state.jump_intensity,
        jump_mean: vol_state.jump_mean,
        jump_var: vol_state.jump_var,
        student_t_nu: vol_state.student_t_nu,
    };

    // BTC's relationship to the strike -- this is the core signal
    let btc_above_strike = btc_price > strike;
    let btc_distance = btc_price - strike; // positive = above, negative = below

    for (i, model) in pricing_models.iter().enumerate() {
        let state = &mut model_states[i];
        let cal = &mut calibrators[i];

        let raw_prob = model.probability(&params, &vol_ctx);
        let prob = cal.calibrate(raw_prob);

        let ev_params = EvParams {
            probability: prob,
            contract_price: yes_ask,
            fee_rate: 0.02,
            slippage: 0.005,
            fill_probability: 0.9,
        };
        let ev_result = ev::compute_ev(&ev_params, config.ev_threshold);

        let win_prob = if ev_result.buy_yes { prob } else { 1.0 - prob };
        let kelly_result = kelly::compute_kelly(&KellyParams {
            model_probability: win_prob,
            alpha: state.beta_alpha,
            beta: state.beta_beta,
            contract_price: if ev_result.buy_yes { yes_ask } else { 1.0 - yes_ask },
            fractional_gamma: config.fractional_kelly,
            lambda: 0.5,
            max_position: config.max_position_size,
        });

        let paper_contracts = if kelly_result.contracts > 0.0 {
            kelly_result.contracts.max(1.0)
        } else {
            kelly_result.contracts
        };

        state.probability = prob;
        state.ev = ev_result.ev;
        state.kelly_size = paper_contracts;

        // ── PHASE 1: Mark-to-Market + Peak Tracking ──
        let mut total_unrealized = 0.0_f64;
        for pos in state.open_positions.iter_mut() {
            let current_bid = if pos.side == "yes" {
                yes_bid
            } else {
                1.0 - yes_ask
            };
            let unrealized = (current_bid - pos.entry_price) * pos.contracts;
            total_unrealized += unrealized;

            // Update peak unrealized for trailing stop
            if unrealized > pos.peak_unrealized {
                pos.peak_unrealized = unrealized;
            }
        }
        state.unrealized_pnl = total_unrealized;

        // ── PHASE 2: Exit Checks (ordered by priority) ──
        let mut positions_to_exit: SmallVec<[usize; 4]> = SmallVec::new();
        let mut exit_reasons: SmallVec<[&'static str; 4]> = SmallVec::new();
        let mut partial_exit_indices: SmallVec<[usize; 4]> = SmallVec::new();

        for (pos_idx, pos) in state.open_positions.iter().enumerate() {
            let current_bid = if pos.side == "yes" {
                yes_bid
            } else {
                1.0 - yes_ask
            };

            let entry_cost = pos.entry_price * pos.contracts;
            let unrealized = (current_bid - pos.entry_price) * pos.contracts;
            let hold_ticks = tick_counter.saturating_sub(pos.entry_tick);

            // Skip exits for very new positions (unless strike crossover)
            let is_new = hold_ticks < MIN_HOLD_TICKS;

            // ─── RULE 1: Strike Crossover Exit (highest priority, ignores hold time) ───
            // If BTC has crossed the strike against our position, the contract value
            // is collapsing. Cut immediately, don't wait.
            let position_is_yes = pos.side == "yes";
            let btc_against_us = if position_is_yes {
                // We hold YES (bet BTC > strike), but BTC has dropped below strike
                btc_price < strike - STRIKE_CROSS_BUFFER
            } else {
                // We hold NO (bet BTC < strike), but BTC has risen above strike
                btc_price > strike + STRIKE_CROSS_BUFFER
            };

            if btc_against_us {
                positions_to_exit.push(pos_idx);
                exit_reasons.push("strike_cross");
                continue;
            }

            if is_new {
                continue;
            }

            // ─── RULE 2: Hard Stop-Loss ───
            if entry_cost > 0.0 && unrealized < -(entry_cost * HARD_STOP_LOSS_PCT) {
                positions_to_exit.push(pos_idx);
                exit_reasons.push("stop_loss");
                continue;
            }

            // ─── RULE 3: Trailing Stop ───
            // Once we've had significant gains, don't let them evaporate.
            // Exit if unrealized drops 50% from peak.
            if pos.peak_unrealized > entry_cost * 0.10 {
                let trailing_threshold = pos.peak_unrealized * (1.0 - TRAILING_STOP_PCT);
                if unrealized < trailing_threshold {
                    positions_to_exit.push(pos_idx);
                    exit_reasons.push("trailing_stop");
                    continue;
                }
            }

            // ─── RULE 4: Full Take-Profit ───
            if entry_cost > 0.0 && unrealized > entry_cost * FULL_TAKE_PROFIT_PCT {
                positions_to_exit.push(pos_idx);
                exit_reasons.push("take_profit");
                continue;
            }

            // ─── RULE 5: Partial Take-Profit ───
            // Sell ~half when at significant gain (only for multi-contract positions)
            if entry_cost > 0.0
                && unrealized > entry_cost * PARTIAL_TAKE_PROFIT_PCT
                && pos.contracts > 1.5
                && pos.leg == 0
            {
                partial_exit_indices.push(pos_idx);
                continue;
            }

            // ─── RULE 6: Time-Based Exit ───
            if ttl_seconds < UNCERTAIN_EXIT_SECONDS {
                // Near expiry: should we hold or exit?
                let on_right_side = if position_is_yes {
                    btc_price > strike
                } else {
                    btc_price < strike
                };

                let strongly_winning = btc_distance.abs() > RESOLUTION_HOLD_DISTANCE;

                if ttl_seconds < RESOLUTION_HOLD_SECONDS && on_right_side && strongly_winning {
                    // HOLD: We're strongly winning with < 2 min left.
                    // Contract is converging to $1, let it resolve.
                    continue;
                }

                if !on_right_side || !strongly_winning {
                    // EXIT: Near expiry and not clearly winning = coin flip zone.
                    positions_to_exit.push(pos_idx);
                    exit_reasons.push("time_exit");
                    continue;
                }
            }
        }

        // Execute partial exits: collect data first to avoid borrow conflicts
        struct PartialExitData {
            pos_idx: usize,
            exit_contracts: f64,
            exit_price: f64,
            entry_price: f64,
            fee: f64,
            pnl: f64,
            trade_id: String,
            side: String,
        }

        let partial_exits: SmallVec<[PartialExitData; 4]> = partial_exit_indices
            .iter()
            .rev()
            .filter_map(|&pos_idx| {
                if pos_idx >= state.open_positions.len() {
                    return None;
                }
                let pos = &state.open_positions[pos_idx];
                let exit_contracts = (pos.contracts * 0.5).floor().max(1.0);
                if exit_contracts >= pos.contracts {
                    return None;
                }
                let exit_price = if pos.side == "yes" {
                    yes_bid.max(0.01)
                } else {
                    (1.0 - yes_ask).max(0.01)
                };
                let fee = exit_price * exit_contracts * 0.02;
                let pnl = (exit_price - pos.entry_price) * exit_contracts - fee;
                Some(PartialExitData {
                    pos_idx,
                    exit_contracts,
                    exit_price,
                    entry_price: pos.entry_price,
                    fee,
                    pnl,
                    trade_id: pos.trade_id.clone(),
                    side: pos.side.clone(),
                })
            })
            .collect();

        for pe in partial_exits {
            tracing::info!(
                model = model.name(),
                side = %pe.side,
                contracts_sold = pe.exit_contracts,
                pnl = pe.pnl,
                "partial take-profit"
            );

            state.open_positions[pe.pos_idx].contracts -= pe.exit_contracts;
            state.cumulative_pnl += pe.pnl;
            state.daily_pnl += pe.pnl;
            state.current_exposure -= pe.entry_price * pe.exit_contracts;
            state.current_exposure = state.current_exposure.max(0.0);

            if pe.pnl > 0.0 {
                state.winning_trades += 1;
                state.beta_alpha += 1.0;
            }
            let ret = pe.pnl / (pe.entry_price * pe.exit_contracts).max(0.01);
            state.record_return(ret);
            state.update_drawdown();
            state.compute_sharpe();

            actions.push(EngineAction::BroadcastUpdate(WsMessage::NewTrade {
                model: model.name().to_string(),
                side: pe.side.clone(),
                action: "partial sell".to_string(),
                price: pe.exit_price,
                contracts: pe.exit_contracts,
                ev: pe.pnl,
                timestamp: timestamp.to_string(),
            }));

            actions.push(EngineAction::DbWrite(DbCommand::InsertTrade {
                id: format!("{}-partial", pe.trade_id),
                model_name: model.name().to_string(),
                market_ticker: market.ticker.clone(),
                side: pe.side,
                action: "sell".to_string(),
                entry_price: pe.exit_price,
                contracts: pe.exit_contracts,
                model_probability: prob,
                ev: pe.pnl,
                kelly_fraction: 0.0,
                fees_estimate: pe.fee,
                entry_time: timestamp.to_string(),
            }));
        }

        // Execute full exits (in reverse to preserve indices)
        for j in (0..positions_to_exit.len()).rev() {
            let pos_idx = positions_to_exit[j];
            let reason = exit_reasons[j];

            if pos_idx >= state.open_positions.len() {
                continue;
            }
            let pos = state.open_positions.remove(pos_idx);

            let exit_price = if pos.side == "yes" {
                yes_bid.max(0.01)
            } else {
                (1.0 - yes_ask).max(0.01)
            };

            let fee = exit_price * pos.contracts * 0.02;
            let pnl = (exit_price - pos.entry_price) * pos.contracts - fee;

            tracing::info!(
                model = model.name(),
                side = %pos.side,
                entry = pos.entry_price,
                exit = exit_price,
                contracts = pos.contracts,
                pnl = pnl,
                reason = reason,
                btc = btc_price,
                strike = strike,
                "exiting position"
            );

            state.cumulative_pnl += pnl;
            state.daily_pnl += pnl;
            state.current_exposure -= pos.entry_price * pos.contracts;
            state.current_exposure = state.current_exposure.max(0.0);

            if pnl > 0.0 {
                state.winning_trades += 1;
                state.beta_alpha += 1.0;
            } else {
                state.beta_beta += 1.0;
            }

            let ret = pnl / (pos.entry_price * pos.contracts).max(0.01);
            state.record_return(ret);
            state.update_drawdown();
            state.compute_sharpe();

            actions.push(EngineAction::ExitTrade {
                trade_id: pos.trade_id.clone(),
                model_name: model.name(),
                exit_price,
                pnl,
                reason,
            });

            actions.push(EngineAction::DbWrite(DbCommand::ExitTrade {
                trade_id: pos.trade_id.clone(),
                exit_price,
                pnl,
                reason: reason.to_string(),
                exit_time: timestamp.to_string(),
            }));

            actions.push(EngineAction::BroadcastUpdate(WsMessage::TradeExited {
                model: model.name().to_string(),
                trade_id: pos.trade_id.clone(),
                side: pos.side.clone(),
                entry_price: pos.entry_price,
                exit_price,
                contracts: pos.contracts,
                pnl,
                reason: reason.to_string(),
                timestamp: timestamp.to_string(),
            }));

            actions.push(EngineAction::BroadcastUpdate(WsMessage::NewTrade {
                model: model.name().to_string(),
                side: pos.side.clone(),
                action: format!("sell ({reason})"),
                price: exit_price,
                contracts: pos.contracts,
                ev: pnl,
                timestamp: timestamp.to_string(),
            }));
        }

        // Recompute unrealized after exits
        let mut post_exit_unrealized = 0.0_f64;
        for pos in state.open_positions.iter() {
            let bid = if pos.side == "yes" { yes_bid } else { 1.0 - yes_ask };
            post_exit_unrealized += (bid - pos.entry_price) * pos.contracts;
        }
        state.unrealized_pnl = post_exit_unrealized;

        // ── PHASE 3: Scale-In Check (add to winners) ──
        // Only scale if we have existing positions AND BTC has moved further in our favor
        if !state.open_positions.is_empty() && ttl_seconds > MIN_ENTRY_TTL {
            let current_leg_count = state.open_positions.iter().map(|p| p.leg).max().unwrap_or(0);

            if current_leg_count < MAX_LEGS - 1 {
                // Check if BTC has moved significantly in our favor since entry
                let first_pos = &state.open_positions[0];
                let btc_move_since_entry = btc_price - first_pos.entry_btc_price;

                let btc_moved_in_favor = if first_pos.side == "yes" {
                    btc_move_since_entry > SCALE_IN_MOVE
                } else {
                    btc_move_since_entry < -SCALE_IN_MOVE
                };

                // Also require positive unrealized to scale in
                if btc_moved_in_favor && state.unrealized_pnl > 0.0 && ev_result.is_signal {
                    let scale_side = first_pos.side.clone();
                    let scale_price = if scale_side == "yes" { yes_ask } else { 1.0 - yes_ask };

                    // Scale-in with 1 contract
                    let scale_contracts = 1.0_f64;

                    let risk = limits::check_risk_limits(
                        state,
                        vol_state,
                        scale_contracts,
                        scale_price,
                        config.max_daily_drawdown,
                        config.max_position_size,
                    );

                    if risk.is_allowed() {
                        let trade_id = uuid::Uuid::new_v4().to_string();
                        let side_str: &'static str = if scale_side == "yes" { "yes" } else { "no" };

                        tracing::info!(
                            model = model.name(),
                            side = side_str,
                            price = scale_price,
                            leg = current_leg_count + 1,
                            btc = btc_price,
                            btc_move = btc_move_since_entry,
                            "scaling into winner"
                        );

                        state.open_positions.push(OpenPosition {
                            trade_id: trade_id.clone(),
                            market_ticker: market.ticker.clone(),
                            side: scale_side,
                            entry_price: scale_price,
                            contracts: scale_contracts,
                            model_probability: prob,
                            entry_tick: tick_counter,
                            entry_btc_price: btc_price,
                            peak_unrealized: 0.0,
                            leg: current_leg_count + 1,
                        });

                        state.current_exposure += scale_contracts * scale_price;
                        state.total_trades += 1;

                        actions.push(EngineAction::PlaceTrade {
                            id: trade_id.clone(),
                            model_name: model.name(),
                            market_ticker: market.ticker.clone(),
                            side: side_str,
                            action: "scale_in",
                            price: scale_price,
                            contracts: scale_contracts,
                            probability: prob,
                            ev: ev_result.ev,
                            kelly_fraction: kelly_result.robust_fraction,
                        });

                        actions.push(EngineAction::DbWrite(DbCommand::InsertTrade {
                            id: trade_id,
                            model_name: model.name().to_string(),
                            market_ticker: market.ticker.clone(),
                            side: side_str.to_string(),
                            action: "scale_in".to_string(),
                            entry_price: scale_price,
                            contracts: scale_contracts,
                            model_probability: prob,
                            ev: ev_result.ev,
                            kelly_fraction: kelly_result.robust_fraction,
                            fees_estimate: scale_price * scale_contracts * 0.02,
                            entry_time: timestamp.to_string(),
                        }));

                        actions.push(EngineAction::BroadcastUpdate(WsMessage::NewTrade {
                            model: model.name().to_string(),
                            side: side_str.to_string(),
                            action: "scale in".to_string(),
                            price: scale_price,
                            contracts: scale_contracts,
                            ev: ev_result.ev,
                            timestamp: timestamp.to_string(),
                        }));
                    }
                }
            }
        }

        // ── PHASE 4: New Entry Check ──
        let price = if ev_result.buy_yes { yes_ask } else { 1.0 - yes_ask };
        let has_position = !state.open_positions.is_empty();

        // Only enter if: signal, no existing position, enough time, and not too close to strike
        if ev_result.is_signal && paper_contracts > 0.0 && !has_position && ttl_seconds > MIN_ENTRY_TTL {
            let risk = limits::check_risk_limits(
                state,
                vol_state,
                paper_contracts,
                price,
                config.max_daily_drawdown,
                config.max_position_size,
            );

            if risk.is_allowed() {
                let trade_id = uuid::Uuid::new_v4().to_string();
                let side: &'static str = if ev_result.buy_yes { "yes" } else { "no" };

                tracing::info!(
                    model = model.name(),
                    side = side,
                    price = price,
                    contracts = paper_contracts,
                    prob = prob,
                    ev = ev_result.ev,
                    btc = btc_price,
                    strike = strike,
                    ttl = ttl_seconds,
                    "new position"
                );

                state.open_positions.push(OpenPosition {
                    trade_id: trade_id.clone(),
                    market_ticker: market.ticker.clone(),
                    side: side.to_string(),
                    entry_price: price,
                    contracts: paper_contracts,
                    model_probability: prob,
                    entry_tick: tick_counter,
                    entry_btc_price: btc_price,
                    peak_unrealized: 0.0,
                    leg: 0,
                });

                state.current_exposure += paper_contracts * price;
                state.total_trades += 1;

                actions.push(EngineAction::PlaceTrade {
                    id: trade_id.clone(),
                    model_name: model.name(),
                    market_ticker: market.ticker.clone(),
                    side,
                    action: "buy",
                    price,
                    contracts: paper_contracts,
                    probability: prob,
                    ev: ev_result.ev,
                    kelly_fraction: kelly_result.robust_fraction,
                });

                actions.push(EngineAction::DbWrite(DbCommand::InsertTrade {
                    id: trade_id,
                    model_name: model.name().to_string(),
                    market_ticker: market.ticker.clone(),
                    side: side.to_string(),
                    action: "buy".to_string(),
                    entry_price: price,
                    contracts: paper_contracts,
                    model_probability: prob,
                    ev: ev_result.ev,
                    kelly_fraction: kelly_result.robust_fraction,
                    fees_estimate: price * paper_contracts * 0.02,
                    entry_time: timestamp.to_string(),
                }));

                actions.push(EngineAction::BroadcastUpdate(WsMessage::NewTrade {
                    model: model.name().to_string(),
                    side: side.to_string(),
                    action: "buy".to_string(),
                    price,
                    contracts: paper_contracts,
                    ev: ev_result.ev,
                    timestamp: timestamp.to_string(),
                }));
            }
        }

        // Re-compute unrealized after all modifications
        let mut final_unrealized = 0.0_f64;
        for pos in state.open_positions.iter() {
            let bid = if pos.side == "yes" { yes_bid } else { 1.0 - yes_ask };
            final_unrealized += (bid - pos.entry_price) * pos.contracts;
        }
        state.unrealized_pnl = final_unrealized;

        // Broadcast model update
        let total_pnl = state.cumulative_pnl + state.unrealized_pnl;
        actions.push(EngineAction::BroadcastUpdate(WsMessage::ModelUpdate {
            model: model.name().to_string(),
            probability: prob,
            ev: ev_result.ev,
            kelly_size: paper_contracts,
            cumulative_pnl: state.cumulative_pnl,
            unrealized_pnl: state.unrealized_pnl,
            total_pnl,
            total_trades: state.total_trades,
            winning_trades: state.winning_trades,
            sharpe: state.sharpe,
            max_drawdown: state.max_drawdown,
            brier_score: state.brier_score,
            daily_pnl: state.daily_pnl,
            current_exposure: state.current_exposure,
            open_position_count: state.open_positions.len(),
        }));

        actions.push(EngineAction::DbWrite(DbCommand::InsertSnapshot {
            model_name: model.name().to_string(),
            timestamp: timestamp.to_string(),
            btc_price,
            market_ticker: Some(market.ticker.clone()),
            probability: Some(prob),
            ev: Some(ev_result.ev),
            kelly_size: Some(kelly_result.contracts),
            cumulative_pnl: state.cumulative_pnl + state.unrealized_pnl,
            volatility: Some(vol_state.ewma_vol),
            regime: Some(vol_state.regime.to_string()),
        }));
    }

    actions
}

/// Settle all pending trades for a market that has resolved.
pub fn settle_trades(
    model_states: &mut [ModelState],
    calibrators: &mut [Calibrator],
    _market_ticker: &str,
    result: &str,
    pending_trades: &[crate::db::TradeRow],
    timestamp: &str,
) -> SmallVec<[EngineAction; 16]> {
    let mut actions: SmallVec<[EngineAction; 16]> = SmallVec::new();

    for trade in pending_trades {
        let won = (trade.side == "yes" && result == "yes")
            || (trade.side == "no" && result == "no");

        let pnl = if won {
            (1.0 - trade.entry_price) * trade.contracts - trade.fees_estimate
        } else {
            -trade.entry_price * trade.contracts - trade.fees_estimate
        };

        let outcome: &'static str = if won { "win" } else { "loss" };

        if let Some(state) = model_states.iter_mut().find(|s| s.name == trade.model_name) {
            state.cumulative_pnl += pnl;
            state.daily_pnl += pnl;
            if won {
                state.winning_trades += 1;
                state.beta_alpha += 1.0;
            } else {
                state.beta_beta += 1.0;
            }
            state.current_exposure -= trade.entry_price * trade.contracts;
            state.current_exposure = state.current_exposure.max(0.0);

            let ret = pnl / (trade.entry_price * trade.contracts).max(0.01);
            state.record_return(ret);
            state.update_drawdown();
            state.compute_sharpe();

            let outcome_val = if result == "yes" { 1.0 } else { 0.0 };
            let brier_diff = trade.model_probability - outcome_val;
            state.brier_sum += brier_diff * brier_diff;
            state.brier_count += 1;
            state.compute_brier();

            state.open_positions.retain(|p| p.trade_id != trade.id);
            state.unrealized_pnl = 0.0;
        }

        let cal_idx = model_states.iter().position(|s| s.name == trade.model_name);
        if let Some(i) = cal_idx {
            let outcome_bool = (result == "yes" && trade.side == "yes")
                || (result == "no" && trade.side == "no");
            calibrators[i].record(trade.model_probability, outcome_bool);
        }

        actions.push(EngineAction::SettleTrade {
            trade_id: trade.id.clone(),
            model_name: trade.model_name.clone(),
            outcome,
            pnl,
        });

        actions.push(EngineAction::DbWrite(DbCommand::SettleTrade {
            trade_id: trade.id.clone(),
            outcome: outcome.to_string(),
            pnl,
            settle_time: timestamp.to_string(),
        }));

        actions.push(EngineAction::BroadcastUpdate(WsMessage::TradeSettled {
            model: trade.model_name.clone(),
            trade_id: trade.id.clone(),
            outcome: outcome.to_string(),
            pnl,
            timestamp: timestamp.to_string(),
        }));
    }

    for state in model_states.iter() {
        actions.push(EngineAction::BroadcastUpdate(WsMessage::MetricsUpdate {
            model: state.name.to_string(),
            sharpe: state.sharpe,
            max_drawdown: state.max_drawdown,
            win_rate: state.win_rate(),
            brier: state.brier_score,
            total_trades: state.total_trades,
            daily_pnl: state.daily_pnl,
        }));

        actions.push(EngineAction::DbWrite(DbCommand::UpdateRiskState {
            model_name: state.name.to_string(),
            exposure: state.current_exposure,
            daily_pnl: state.daily_pnl,
            max_drawdown: state.max_drawdown,
            peak_equity: state.peak_equity,
            total_trades: state.total_trades,
            winning_trades: state.winning_trades,
        }));
    }

    actions
}

fn compute_ttl(close_time: &str) -> f64 {
    let now = chrono::Utc::now();
    let close = chrono::DateTime::parse_from_rfc3339(close_time)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(close_time, "%Y-%m-%dT%H:%M:%SZ")
                .ok()
                .map(|dt| dt.and_utc())
        });

    match close {
        Some(c) => (c - now).num_seconds() as f64,
        None => -1.0,
    }
}
