import { useMemo } from 'react';
import { LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, Legend } from 'recharts';

interface PnlPoint {
  time: string;
  model: string;
  pnl: number;
}

interface PnLChartProps {
  data: PnlPoint[];
  colors: Record<string, string>;
}

interface ChartRow {
  time: string;
  'Black-Scholes': number;
  'Jump-Diffusion': number;
  'Student-t': number;
}

export function PnLChart({ data, colors }: PnLChartProps) {
  const chartData = useMemo(() => {
    const byTime = new Map<string, ChartRow>();

    for (const d of data) {
      const key = d.time.slice(0, 19);
      if (!byTime.has(key)) {
        byTime.set(key, { time: key, 'Black-Scholes': 0, 'Jump-Diffusion': 0, 'Student-t': 0 });
      }
      const row = byTime.get(key)!;
      if (d.model === 'Black-Scholes') row['Black-Scholes'] = d.pnl;
      else if (d.model === 'Jump-Diffusion') row['Jump-Diffusion'] = d.pnl;
      else if (d.model === 'Student-t') row['Student-t'] = d.pnl;
    }

    const arr = Array.from(byTime.values());

    // Forward-fill
    let lastBs = 0, lastJd = 0, lastSt = 0;
    for (const row of arr) {
      if (row['Black-Scholes'] !== 0) lastBs = row['Black-Scholes'];
      else row['Black-Scholes'] = lastBs;
      if (row['Jump-Diffusion'] !== 0) lastJd = row['Jump-Diffusion'];
      else row['Jump-Diffusion'] = lastJd;
      if (row['Student-t'] !== 0) lastSt = row['Student-t'];
      else row['Student-t'] = lastSt;
    }

    if (arr.length > 300) {
      const step = Math.ceil(arr.length / 300);
      return arr.filter((_, i) => i % step === 0);
    }
    return arr;
  }, [data]);

  return (
    <div
      className="rounded-lg border p-4"
      style={{ background: 'var(--bg-card)', borderColor: 'var(--border)' }}
    >
      <h3 className="text-sm font-bold mb-4" style={{ color: 'var(--text-primary)' }}>
        Cumulative P/L
      </h3>

      {chartData.length < 2 ? (
        <div className="h-64 flex items-center justify-center" style={{ color: 'var(--text-secondary)' }}>
          Waiting for data...
        </div>
      ) : (
        <ResponsiveContainer width="100%" height={280}>
          <LineChart data={chartData}>
            <CartesianGrid stroke="#2d3748" strokeDasharray="3 3" />
            <XAxis
              dataKey="time"
              tick={{ fill: '#94a3b8', fontSize: 10 }}
              tickFormatter={(v: string) => v.slice(11, 19)}
            />
            <YAxis
              tick={{ fill: '#94a3b8', fontSize: 10 }}
              tickFormatter={(v: number) => `$${v.toFixed(0)}`}
            />
            <Tooltip
              contentStyle={{ background: '#1a1f2e', border: '1px solid #2d3748', borderRadius: 8, fontSize: 12 }}
              labelStyle={{ color: '#94a3b8' }}
              // eslint-disable-next-line @typescript-eslint/no-explicit-any
              formatter={(value: any, name: any) => [`$${Number(value).toFixed(2)}`, String(name)]}
              // eslint-disable-next-line @typescript-eslint/no-explicit-any
              labelFormatter={(label: any) => String(label).slice(11, 19)}
            />
            <Legend wrapperStyle={{ fontSize: 12 }} />
            <Line type="monotone" dataKey="Black-Scholes" stroke={colors['Black-Scholes']} strokeWidth={2} dot={false} isAnimationActive={false} />
            <Line type="monotone" dataKey="Jump-Diffusion" stroke={colors['Jump-Diffusion']} strokeWidth={2} dot={false} isAnimationActive={false} />
            <Line type="monotone" dataKey="Student-t" stroke={colors['Student-t']} strokeWidth={2} dot={false} isAnimationActive={false} />
          </LineChart>
        </ResponsiveContainer>
      )}
    </div>
  );
}
