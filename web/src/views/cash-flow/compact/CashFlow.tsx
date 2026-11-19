import { useMemo } from 'react';
import type { AssistantChartProps } from '@/assistant/charts';
import { ChartSyncProvider, useChartTheme } from '@/ui/charts';
import { StackLegend } from '@/ui/charts/ext/cashflow-StackedAreas';
import { MoneyFlows } from '../charts';
import { cashFlowModel, livesAbroad } from '../model';

/** `cash_flow` in the assistant's conversation: average money in (above the line) and money out (below) by age. */
export default function CashFlowCompact({ summary, profile }: AssistantChartProps) {
  const theme = useChartTheme();
  const model = useMemo(() => (summary ? cashFlowModel(summary, { abroad: profile ? livesAbroad(profile) : false }) : null), [summary, profile]);
  if (!model) return null;
  const legend = (groups: typeof model.moneyIn) => groups.map((g) => ({ key: g.key, label: g.short, color: theme.categorical[g.color] as string, values: g.values }));
  return (
    <ChartSyncProvider>
      <div className="mb-1.5 space-y-0.5">
        <div className="flex gap-1.5">
          <span className="w-5 shrink-0 text-2xs text-ink-3">In</span>
          <StackLegend series={legend(model.moneyIn)} className="!gap-x-2.5 !text-2xs" />
        </div>
        <div className="flex gap-1.5">
          <span className="w-5 shrink-0 text-2xs text-ink-3">Out</span>
          <StackLegend series={legend(model.moneyOut).map((s) => ({ ...s, side: 'down' as const }))} className="!gap-x-2.5 !text-2xs" />
        </div>
      </div>
      <MoneyFlows model={model} compact />
    </ChartSyncProvider>
  );
}
