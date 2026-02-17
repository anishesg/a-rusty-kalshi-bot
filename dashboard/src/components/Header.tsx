interface HeaderProps {
  btcPrice: number;
  btcTimestamp: string;
  engineState: string;
  connected: boolean;
}

const stateColors: Record<string, string> = {
  connecting: '#f59e0b',
  syncing: '#3b82f6',
  trading: '#10b981',
  halted: '#ef4444',
};

export function Header({ btcPrice, btcTimestamp, engineState, connected }: HeaderProps) {
  const time = btcTimestamp ? new Date(btcTimestamp).toLocaleTimeString() : '--:--:--';

  return (
    <header
      className="sticky top-0 z-50 border-b px-4 py-3 flex items-center justify-between"
      style={{ background: 'var(--bg-secondary)', borderColor: 'var(--border)' }}
    >
      <div className="flex items-center gap-6">
        <h1 className="text-lg font-bold tracking-tight" style={{ color: 'var(--text-primary)' }}>
          pretty_rusty
        </h1>
        <div className="flex items-center gap-2">
          <span className="text-xs uppercase tracking-wider" style={{ color: 'var(--text-secondary)' }}>
            BTC
          </span>
          <span className="text-xl font-bold tabular-nums" style={{ color: btcPrice > 0 ? '#10b981' : 'var(--text-secondary)' }}>
            ${btcPrice > 0 ? btcPrice.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 }) : '---'}
          </span>
        </div>
      </div>

      <div className="flex items-center gap-6">
        <span className="text-sm tabular-nums" style={{ color: 'var(--text-secondary)' }}>
          {time}
        </span>

        <div className="flex items-center gap-2">
          <div
            className="w-2 h-2 rounded-full"
            style={{ background: stateColors[engineState] || '#6b7280' }}
          />
          <span className="text-xs uppercase tracking-wider" style={{ color: 'var(--text-secondary)' }}>
            {engineState}
          </span>
        </div>

        <div className="flex items-center gap-2">
          <div
            className="w-2 h-2 rounded-full"
            style={{ background: connected ? '#10b981' : '#ef4444' }}
          />
          <span className="text-xs" style={{ color: 'var(--text-secondary)' }}>
            {connected ? 'LIVE' : 'OFFLINE'}
          </span>
        </div>
      </div>
    </header>
  );
}
