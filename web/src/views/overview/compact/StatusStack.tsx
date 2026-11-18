import type { AssistantChartProps } from '@/assistant/charts';
import { ChartSyncProvider } from '@/ui/charts';
import { StatusLegend, StatusStack } from '../StatusStack';

/** `status_stack` in the assistant's conversation: life stages by age, with its legend. */
export default function StatusStackCompact({ summary }: AssistantChartProps) {
  if (!summary) return null;
  return (
    <ChartSyncProvider>
      <StatusLegend summary={summary} className="mb-1.5 !gap-x-3 !text-2xs" />
      <StatusStack summary={summary} compact />
    </ChartSyncProvider>
  );
}
