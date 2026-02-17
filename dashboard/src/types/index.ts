export interface ModelState {
  name: string;
  probability: number;
  ev: number;
  kelly_size: number;
  cumulative_pnl: number;
  unrealized_pnl: number;
  total_pnl: number;
  total_trades: number;
  winning_trades: number;
  sharpe: number;
  max_drawdown: number;
  brier_score: number;
  daily_pnl: number;
  current_exposure: number;
  peak_equity: number;
  beta_alpha: number;
  beta_beta: number;
  open_position_count: number;
}

export interface ActiveMarket {
  ticker: string;
  event_ticker: string;
  series_ticker: string;
  strike: number | null;
  yes_bid: string | null;
  yes_ask: string | null;
  no_bid: string | null;
  no_ask: string | null;
  last_price: string | null;
  close_time: string;
  expiration_time: string;
  status: string;
  result: string | null;
}

export interface VolatilityState {
  ewma_vol: number;
  jump_intensity: number;
  jump_mean: number;
  jump_var: number;
  student_t_nu: number;
  regime: string;
  sample_count: number;
}

export interface EngineSnapshot {
  engine_state: string;
  btc_price: number;
  btc_timestamp: string;
  active_market: ActiveMarket | null;
  volatility: VolatilityState;
  models: ModelState[];
}

export interface TradeRow {
  id: string;
  model_name: string;
  market_ticker: string;
  side: string;
  action: string;
  entry_price: number;
  contracts: number;
  model_probability: number;
  ev: number;
  kelly_fraction: number;
  outcome: string | null;
  pnl: number | null;
  fees_estimate: number;
  entry_time: string;
  settle_time: string | null;
}

export type WsMessage =
  | { type: 'btc_price'; price: number; timestamp: string }
  | { type: 'market_state'; ticker: string; strike: number | null; ttl_seconds: number; yes_bid: string | null; yes_ask: string | null; status: string }
  | { type: 'model_update'; model: string; probability: number; ev: number; kelly_size: number; cumulative_pnl: number; unrealized_pnl: number; total_pnl: number; total_trades: number; winning_trades: number; sharpe: number; max_drawdown: number; brier_score: number; daily_pnl: number; current_exposure: number; open_position_count: number }
  | { type: 'new_trade'; model: string; side: string; action: string; price: number; contracts: number; ev: number; timestamp: string }
  | { type: 'trade_exited'; model: string; trade_id: string; side: string; entry_price: number; exit_price: number; contracts: number; pnl: number; reason: string; timestamp: string }
  | { type: 'trade_settled'; model: string; trade_id: string; outcome: string; pnl: number; timestamp: string }
  | { type: 'metrics_update'; model: string; sharpe: number; max_drawdown: number; win_rate: number; brier: number; total_trades: number; daily_pnl: number }
  | { type: 'engine_state'; state: string; reason: string };
