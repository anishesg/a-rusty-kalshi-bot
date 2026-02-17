import { useState, useCallback, useRef } from 'react';
import { useWebSocket } from './hooks/useWebSocket';
import { Header } from './components/Header';
import { ModelGrid } from './components/ModelGrid';
import { TradesFeed } from './components/TradesFeed';
import { PnLChart } from './components/PnLChart';
import { RiskPanel } from './components/RiskPanel';
import { MarketState } from './components/MarketState';
import type { WsMessage, ModelState, TradeRow, EngineSnapshot } from './types';

const MODEL_COLORS: Record<string, string> = {
  'Black-Scholes': '#3b82f6',
  'Jump-Diffusion': '#10b981',
  'Student-t': '#f59e0b',
};

interface MarketInfo {
  ticker: string;
  strike: number | null;
  ttl_seconds: number;
  yes_bid: string | null;
  yes_ask: string | null;
  status: string;
}

interface PnlPoint {
  time: string;
  model: string;
  pnl: number;
}

export default function App() {
  const [btcPrice, setBtcPrice] = useState(0);
  const [btcTimestamp, setBtcTimestamp] = useState('');
  const [engineState, setEngineState] = useState('connecting');
  const [market, setMarket] = useState<MarketInfo | null>(null);
  const [models, setModels] = useState<Record<string, Partial<ModelState>>>({});
  const [trades, setTrades] = useState<TradeRow[]>([]);
  const [pnlData, setPnlData] = useState<PnlPoint[]>([]);

  const tradesRef = useRef(trades);
  tradesRef.current = trades;

  // Handle initial EngineSnapshot (no 'type' field, has 'engine_state')
  const handleSnapshot = useCallback((snap: EngineSnapshot) => {
    if (snap.engine_state) {
      setEngineState(snap.engine_state.toLowerCase?.() ?? snap.engine_state);
    }
    if (snap.btc_price > 0) {
      setBtcPrice(snap.btc_price);
      setBtcTimestamp(snap.btc_timestamp);
    }
    if (snap.active_market) {
      setMarket({
        ticker: snap.active_market.ticker,
        strike: snap.active_market.strike,
        ttl_seconds: 0,
        yes_bid: snap.active_market.yes_bid,
        yes_ask: snap.active_market.yes_ask,
        status: snap.active_market.status,
      });
    }
    if (snap.models && snap.models.length > 0) {
      const next: Record<string, Partial<ModelState>> = {};
      for (const m of snap.models) {
        next[m.name] = m;
      }
      setModels(next);
    }
  }, []);

  const handleMessage = useCallback((raw: WsMessage | EngineSnapshot) => {
    // Detect initial EngineSnapshot (has engine_state, no type field)
    if ('engine_state' in raw && !('type' in raw)) {
      handleSnapshot(raw as EngineSnapshot);
      return;
    }

    const msg = raw as WsMessage;
    switch (msg.type) {
      case 'btc_price':
        setBtcPrice(msg.price);
        setBtcTimestamp(msg.timestamp);
        break;

      case 'market_state':
        setMarket({
          ticker: msg.ticker,
          strike: msg.strike,
          ttl_seconds: msg.ttl_seconds,
          yes_bid: msg.yes_bid,
          yes_ask: msg.yes_ask,
          status: msg.status,
        });
        break;

      case 'model_update':
        setModels((prev) => ({
          ...prev,
          [msg.model]: {
            ...prev[msg.model],
            name: msg.model,
            probability: msg.probability,
            ev: msg.ev,
            kelly_size: msg.kelly_size,
            cumulative_pnl: msg.cumulative_pnl,
            unrealized_pnl: msg.unrealized_pnl,
            total_pnl: msg.total_pnl,
            total_trades: msg.total_trades,
            winning_trades: msg.winning_trades,
            sharpe: msg.sharpe,
            max_drawdown: msg.max_drawdown,
            brier_score: msg.brier_score,
            daily_pnl: msg.daily_pnl,
            current_exposure: msg.current_exposure,
            open_position_count: msg.open_position_count,
          },
        }));
        setPnlData((prev) => {
          const next = [...prev, { time: new Date().toISOString(), model: msg.model, pnl: msg.total_pnl }];
          return next.length > 3000 ? next.slice(-3000) : next;
        });
        break;

      case 'new_trade':
        setTrades((prev) => {
          const trade: TradeRow = {
            id: crypto.randomUUID(),
            model_name: msg.model,
            market_ticker: '',
            side: msg.side,
            action: msg.action,
            entry_price: msg.price,
            contracts: msg.contracts,
            model_probability: 0,
            ev: msg.ev,
            kelly_fraction: 0,
            outcome: null,
            pnl: null,
            fees_estimate: 0,
            entry_time: msg.timestamp,
            settle_time: null,
          };
          const next = [trade, ...prev];
          return next.length > 200 ? next.slice(0, 200) : next;
        });
        break;

      case 'trade_exited':
        setTrades((prev) => {
          // Mark the matching open trade with exit info
          const updated = prev.map((t) =>
            (t.model_name === msg.model && !t.outcome)
              ? { ...t, outcome: `exit:${msg.reason}`, pnl: msg.pnl, settle_time: msg.timestamp }
              : t
          );
          return updated;
        });
        break;

      case 'trade_settled':
        setTrades((prev) =>
          prev.map((t) =>
            t.id === msg.trade_id || (t.model_name === msg.model && !t.outcome)
              ? { ...t, outcome: msg.outcome, pnl: msg.pnl, settle_time: msg.timestamp }
              : t
          )
        );
        break;

      case 'metrics_update':
        setModels((prev) => ({
          ...prev,
          [msg.model]: {
            ...prev[msg.model],
            sharpe: msg.sharpe,
            max_drawdown: msg.max_drawdown,
            total_trades: msg.total_trades,
            winning_trades: Math.round(msg.win_rate * msg.total_trades),
            brier_score: msg.brier,
            daily_pnl: msg.daily_pnl,
          },
        }));
        break;

      case 'engine_state':
        setEngineState(msg.state);
        break;
    }
  }, [handleSnapshot]);

  const { connected } = useWebSocket(handleMessage);

  const modelList: ModelState[] = ['Black-Scholes', 'Jump-Diffusion', 'Student-t'].map(
    (name) => ({
      name,
      probability: 0,
      ev: 0,
      kelly_size: 0,
      cumulative_pnl: 0,
      unrealized_pnl: 0,
      total_pnl: 0,
      total_trades: 0,
      winning_trades: 0,
      sharpe: 0,
      max_drawdown: 0,
      brier_score: 0,
      daily_pnl: 0,
      current_exposure: 0,
      peak_equity: 0,
      beta_alpha: 1,
      beta_beta: 1,
      open_position_count: 0,
      ...models[name],
    })
  );

  return (
    <div className="min-h-screen" style={{ background: 'var(--bg-primary)' }}>
      <Header
        btcPrice={btcPrice}
        btcTimestamp={btcTimestamp}
        engineState={engineState}
        connected={connected}
      />

      <main className="max-w-[1600px] mx-auto px-4 py-4 space-y-4">
        <MarketState market={market} />
        <ModelGrid models={modelList} colors={MODEL_COLORS} />

        <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
          <div className="lg:col-span-2">
            <PnLChart data={pnlData} colors={MODEL_COLORS} />
          </div>
          <div>
            <RiskPanel models={modelList} />
          </div>
        </div>

        <TradesFeed trades={trades} colors={MODEL_COLORS} />
      </main>
    </div>
  );
}
