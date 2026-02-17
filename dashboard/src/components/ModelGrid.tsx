import type { ModelState } from '../types';
import { ModelCard } from './ModelCard';

interface ModelGridProps {
  models: ModelState[];
  colors: Record<string, string>;
}

export function ModelGrid({ models, colors }: ModelGridProps) {
  return (
    <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
      {models.map((m) => (
        <ModelCard key={m.name} model={m} color={colors[m.name] || '#6b7280'} />
      ))}
    </div>
  );
}
