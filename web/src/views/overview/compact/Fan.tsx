import type { AssistantChartProps } from '@/assistant/charts';
import { ChartSyncProvider } from '@/ui/charts';
import { NetWorthFan } from '../NetWorthFan';

/** `fan` in the assistant's conversation: net worth by age, small and directly labelled. */
export default function FanCompact({ summary, profile, comparison }: AssistantChartProps) {
  if (!summary) return null;
  return (
    <ChartSyncProvider>
      <NetWorthFan summary={summary} profile={profile} series="net_worth" scale="linear" compact compare={comparison ? { name: comparison.name, summary: comparison.summary } : undefined} />
    </ChartSyncProvider>
  );
}
