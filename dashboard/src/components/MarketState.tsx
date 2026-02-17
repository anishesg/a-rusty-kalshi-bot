interface MarketInfo {
  ticker: string;
  strike: number | null;
  ttl_seconds: number;
  yes_bid: string | null;
  yes_ask: string | null;
  status: string;
}

interface MarketStateProps {
  market: MarketInfo | null;
}

function formatTTL(seconds: number): string {
  if (seconds <= 0) return '00:00';
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
}

export function MarketState({ market }: MarketStateProps) {
  if (!market) {
    return (
      <div
        className="rounded-lg border p-4 text-center"
        style={{ background: 'var(--bg-card)', borderColor: 'var(--border)' }}
      >
        <span style={{ color: 'var(--text-secondary)' }}>
          Scanning for active BTC 15-min market...
        </span>
      </div>
    );
  }

  const ttlColor = market.ttl_seconds < 60 ? '#ef4444' : market.ttl_seconds < 300 ? '#f59e0b' : '#10b981';

  return (
    <div
      className="rounded-lg border p-4 flex items-center justify-between"
      style={{ background: 'var(--bg-card)', borderColor: 'var(--border)' }}
    >
      <div className="flex items-center gap-6">
        <div>
          <div className="text-xs uppercase tracking-wider" style={{ color: 'var(--text-secondary)' }}>
            Market
          </div>
          <div className="text-sm font-bold" style={{ color: 'var(--text-primary)' }}>
            {market.ticker}
          </div>
        </div>

        {market.strike && (
          <div>
            <div className="text-xs uppercase tracking-wider" style={{ color: 'var(--text-secondary)' }}>
              Strike
            </div>
            <div className="text-sm font-bold tabular-nums" style={{ color: 'var(--text-primary)' }}>
              ${market.strike.toLocaleString()}
            </div>
          </div>
        )}

        <div>
          <div className="text-xs uppercase tracking-wider" style={{ color: 'var(--text-secondary)' }}>
            Status
          </div>
          <div className="text-sm font-bold uppercase" style={{ color: '#10b981' }}>
            {market.status}
          </div>
        </div>
      </div>

      <div className="flex items-center gap-6">
        {market.yes_bid && (
          <div>
            <div className="text-xs uppercase tracking-wider" style={{ color: 'var(--text-secondary)' }}>
              Yes Bid / Ask
            </div>
            <div className="text-sm font-bold tabular-nums" style={{ color: 'var(--text-primary)' }}>
              {market.yes_bid} / {market.yes_ask || '--'}
            </div>
          </div>
        )}

        <div className="text-right">
          <div className="text-xs uppercase tracking-wider" style={{ color: 'var(--text-secondary)' }}>
            Time Left
          </div>
          <div className="text-2xl font-bold tabular-nums" style={{ color: ttlColor }}>
            {formatTTL(market.ttl_seconds)}
          </div>
        </div>
      </div>
    </div>
  );
}
