import type { ModelState } from '../types';

interface RiskPanelProps {
  models: ModelState[];
}

const modelColors: Record<string, string> = {
  'Black-Scholes': '#3b82f6',
  'Jump-Diffusion': '#10b981',
  'Student-t': '#f59e0b',
};

export function RiskPanel({ models }: RiskPanelProps) {
  const totalExposure = models.reduce((s, m) => s + m.current_exposure, 0);
  const totalDailyPnl = models.reduce((s, m) => s + m.daily_pnl, 0);
  const totalTrades = models.reduce((s, m) => s + m.total_trades, 0);
  const worstDrawdown = Math.max(...models.map((m) => m.max_drawdown));

  return (
    <div
      className="rounded-lg border p-4"
      style={{ background: 'var(--bg-card)', borderColor: 'var(--border)' }}
    >
      <h3 className="text-sm font-bold mb-4" style={{ color: 'var(--text-primary)' }}>
        Risk Dashboard
      </h3>

      {/* Aggregate stats */}
      <div className="grid grid-cols-2 gap-3 mb-4">
        <RiskMetric label="Total Exposure" value={`$${totalExposure.toFixed(2)}`} />
        <RiskMetric
          label="Daily P/L"
          value={`${totalDailyPnl >= 0 ? '+' : ''}$${totalDailyPnl.toFixed(2)}`}
          color={totalDailyPnl >= 0 ? '#10b981' : '#ef4444'}
        />
        <RiskMetric label="Total Trades" value={totalTrades.toString()} />
        <RiskMetric label="Worst DD" value={`-$${worstDrawdown.toFixed(2)}`} color="#ef4444" />
      </div>

      {/* Per-model risk bars */}
      <div className="space-y-3">
        {models.map((m) => {
          const maxExp = 50;
          const pct = Math.min((m.current_exposure / maxExp) * 100, 100);
          return (
            <div key={m.name}>
              <div className="flex items-center justify-between text-xs mb-1">
                <span style={{ color: modelColors[m.name] || 'var(--text-secondary)' }}>
                  {m.name}
                </span>
                <span className="tabular-nums" style={{ color: 'var(--text-secondary)' }}>
                  ${m.current_exposure.toFixed(2)}
                </span>
              </div>
              <div className="h-1.5 rounded-full" style={{ background: 'var(--bg-secondary)' }}>
                <div
                  className="h-1.5 rounded-full transition-all"
                  style={{
                    width: `${pct}%`,
                    background: modelColors[m.name] || '#6b7280',
                  }}
                />
              </div>
            </div>
          );
        })}
      </div>

      {/* Vol regime */}
      <div className="mt-4 pt-3 border-t text-xs" style={{ borderColor: 'var(--border)' }}>
        <div className="flex items-center justify-between">
          <span style={{ color: 'var(--text-secondary)' }}>Volatility Regime</span>
          <span className="font-bold uppercase" style={{ color: '#f59e0b' }}>--</span>
        </div>
      </div>
    </div>
  );
}

function RiskMetric({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <div>
      <div className="text-xs uppercase tracking-wider" style={{ color: 'var(--text-secondary)', fontSize: '0.6rem' }}>
        {label}
      </div>
      <div className="text-sm font-bold tabular-nums" style={{ color: color || 'var(--text-primary)' }}>
        {value}
      </div>
    </div>
  );
}
