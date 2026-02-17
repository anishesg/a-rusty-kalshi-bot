import type { ModelState } from '../types';

interface ModelCardProps {
  model: ModelState;
  color: string;
}

function fmt(n: number, decimals = 2): string {
  return n.toFixed(decimals);
}

function fmtPnl(n: number): string {
  const sign = n >= 0 ? '+' : '';
  return `${sign}$${n.toFixed(2)}`;
}

export function ModelCard({ model, color }: ModelCardProps) {
  const winRate = model.total_trades > 0
    ? ((model.winning_trades / model.total_trades) * 100).toFixed(1)
    : '0.0';

  const totalPnl = model.total_pnl ?? (model.cumulative_pnl + model.unrealized_pnl);
  const hasOpenPositions = (model.open_position_count ?? 0) > 0;

  return (
    <div
      className="rounded-lg border p-4 relative overflow-hidden"
      style={{ background: 'var(--bg-card)', borderColor: 'var(--border)' }}
    >
      {/* Color accent bar */}
      <div
        className="absolute top-0 left-0 right-0 h-1"
        style={{ background: color }}
      />

      {/* Header */}
      <div className="flex items-center justify-between mb-2 mt-1">
        <div>
          <h3 className="text-sm font-bold" style={{ color }}>
            {model.name}
          </h3>
          {hasOpenPositions && (
            <span className="text-xs px-1.5 py-0.5 rounded" style={{ background: '#3b82f620', color: '#3b82f6' }}>
              {model.open_position_count} open
            </span>
          )}
        </div>
        <div className="text-right">
          <span
            className="text-lg font-bold tabular-nums block"
            style={{ color: totalPnl >= 0 ? '#10b981' : '#ef4444' }}
          >
            {fmtPnl(totalPnl)}
          </span>
          {model.unrealized_pnl !== 0 && (
            <span
              className="text-xs tabular-nums block"
              style={{ color: model.unrealized_pnl >= 0 ? '#10b98180' : '#ef444480' }}
            >
              unrealized: {fmtPnl(model.unrealized_pnl)}
            </span>
          )}
        </div>
      </div>

      {/* P/L breakdown */}
      {(model.cumulative_pnl !== 0 || model.unrealized_pnl !== 0) && (
        <div className="flex gap-3 mb-3 text-xs">
          <div className="flex items-center gap-1">
            <span style={{ color: 'var(--text-secondary)' }}>Realized:</span>
            <span
              className="font-bold tabular-nums"
              style={{ color: model.cumulative_pnl >= 0 ? '#10b981' : '#ef4444' }}
            >
              {fmtPnl(model.cumulative_pnl)}
            </span>
          </div>
          <div className="flex items-center gap-1">
            <span style={{ color: 'var(--text-secondary)' }}>Unrealized:</span>
            <span
              className="font-bold tabular-nums"
              style={{ color: model.unrealized_pnl >= 0 ? '#10b981' : '#ef4444' }}
            >
              {fmtPnl(model.unrealized_pnl)}
            </span>
          </div>
        </div>
      )}

      {/* Metrics grid */}
      <div className="grid grid-cols-2 gap-y-3 gap-x-4 text-xs">
        <Metric label="Probability" value={`${(model.probability * 100).toFixed(1)}%`} />
        <Metric label="EV" value={fmtPnl(model.ev)} valueColor={model.ev > 0 ? '#10b981' : '#ef4444'} />
        <Metric label="Kelly Size" value={`${fmt(model.kelly_size, 1)} cts`} />
        <Metric label="Sharpe" value={fmt(model.sharpe, 2)} />
        <Metric label="Win Rate" value={`${winRate}%`} />
        <Metric label="Max DD" value={`-$${fmt(model.max_drawdown, 2)}`} valueColor="#ef4444" />
        <Metric label="Trades" value={model.total_trades.toString()} />
        <Metric label="Brier" value={fmt(model.brier_score, 4)} />
        <Metric label="Daily P/L" value={fmtPnl(model.daily_pnl)} valueColor={model.daily_pnl >= 0 ? '#10b981' : '#ef4444'} />
        <Metric label="Exposure" value={`$${fmt(model.current_exposure, 2)}`} />
      </div>
    </div>
  );
}

function Metric({ label, value, valueColor }: { label: string; value: string; valueColor?: string }) {
  return (
    <div>
      <div className="uppercase tracking-wider mb-0.5" style={{ color: 'var(--text-secondary)', fontSize: '0.65rem' }}>
        {label}
      </div>
      <div className="font-bold tabular-nums" style={{ color: valueColor || 'var(--text-primary)' }}>
        {value}
      </div>
    </div>
  );
}
