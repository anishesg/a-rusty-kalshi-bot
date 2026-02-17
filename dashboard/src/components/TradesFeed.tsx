import type { TradeRow } from '../types';

interface TradesFeedProps {
  trades: TradeRow[];
  colors: Record<string, string>;
}

export function TradesFeed({ trades, colors }: TradesFeedProps) {
  return (
    <div
      className="rounded-lg border"
      style={{ background: 'var(--bg-card)', borderColor: 'var(--border)' }}
    >
      <div className="px-4 py-3 border-b" style={{ borderColor: 'var(--border)' }}>
        <h3 className="text-sm font-bold" style={{ color: 'var(--text-primary)' }}>
          Live Trade Feed
        </h3>
      </div>

      <div className="max-h-80 overflow-y-auto">
        {trades.length === 0 ? (
          <div className="p-4 text-center text-sm" style={{ color: 'var(--text-secondary)' }}>
            No trades yet. Engine will generate paper trades when conditions are met.
          </div>
        ) : (
          <table className="w-full text-xs">
            <thead>
              <tr style={{ color: 'var(--text-secondary)' }}>
                <th className="text-left px-4 py-2 font-medium">Time</th>
                <th className="text-left px-4 py-2 font-medium">Model</th>
                <th className="text-left px-4 py-2 font-medium">Side</th>
                <th className="text-right px-4 py-2 font-medium">Price</th>
                <th className="text-right px-4 py-2 font-medium">Contracts</th>
                <th className="text-right px-4 py-2 font-medium">EV</th>
                <th className="text-right px-4 py-2 font-medium">Outcome</th>
                <th className="text-right px-4 py-2 font-medium">P/L</th>
              </tr>
            </thead>
            <tbody>
              {trades.map((t) => (
                <tr
                  key={t.id}
                  className="border-t hover:opacity-80"
                  style={{ borderColor: 'var(--border)' }}
                >
                  <td className="px-4 py-2 tabular-nums" style={{ color: 'var(--text-secondary)' }}>
                    {formatTime(t.entry_time)}
                  </td>
                  <td className="px-4 py-2 font-bold whitespace-nowrap" style={{ color: colors[t.model_name] || 'var(--text-primary)' }}>
                    {t.model_name}
                  </td>
                  <td className="px-4 py-2">
                    <span
                      className="px-1.5 py-0.5 rounded text-xs font-bold uppercase"
                      style={{
                        background: t.side === 'yes' ? '#10b98120' : '#ef444420',
                        color: t.side === 'yes' ? '#10b981' : '#ef4444',
                      }}
                    >
                      {t.action} {t.side}
                    </span>
                  </td>
                  <td className="px-4 py-2 tabular-nums text-right" style={{ color: 'var(--text-primary)' }}>
                    {t.entry_price.toFixed(2)}
                  </td>
                  <td className="px-4 py-2 tabular-nums text-right" style={{ color: 'var(--text-primary)' }}>
                    {t.contracts.toFixed(1)}
                  </td>
                  <td className="px-4 py-2 tabular-nums text-right" style={{ color: t.ev > 0 ? '#10b981' : '#ef4444' }}>
                    {t.ev > 0 ? '+' : ''}{t.ev.toFixed(3)}
                  </td>
                  <td className="px-4 py-2 text-right">
                    {t.outcome ? (
                      <span
                        className="px-1.5 py-0.5 rounded text-xs font-bold"
                        style={{
                          background: t.outcome === 'win' || t.outcome.startsWith('exit:take_profit')
                            ? '#10b98120'
                            : t.outcome.startsWith('exit:')
                            ? '#f59e0b20'
                            : '#ef444420',
                          color: t.outcome === 'win' || t.outcome.startsWith('exit:take_profit')
                            ? '#10b981'
                            : t.outcome.startsWith('exit:')
                            ? '#f59e0b'
                            : '#ef4444',
                        }}
                      >
                        {t.outcome.startsWith('exit:') ? t.outcome.replace('exit:', '') : t.outcome}
                      </span>
                    ) : (
                      <span className="px-1.5 py-0.5 rounded text-xs" style={{ background: '#3b82f620', color: '#3b82f6' }}>open</span>
                    )}
                  </td>
                  <td
                    className="px-4 py-2 tabular-nums text-right font-bold"
                    style={{ color: (t.pnl ?? 0) >= 0 ? '#10b981' : '#ef4444' }}
                  >
                    {t.pnl != null ? `${t.pnl >= 0 ? '+' : ''}$${t.pnl.toFixed(2)}` : '--'}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

function formatTime(iso: string): string {
  if (!iso) return '--:--:--';
  try {
    return new Date(iso).toLocaleTimeString();
  } catch {
    return iso.slice(11, 19);
  }
}

