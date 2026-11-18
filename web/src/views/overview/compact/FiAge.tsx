import type { AssistantChartProps } from '@/assistant/charts';
import { ChartSyncProvider } from '@/ui/charts';
import { FiAgeChart } from '../FiAgeChart';

/** `fi_age` in the assistant's conversation: 100 dots by the age work becomes optional. */
export default function FiAgeCompact({ summary }: AssistantChartProps) {
  if (!summary) return null;
  return (
    <ChartSyncProvider>
      <FiAgeChart curve={summary.fi} spread={summary.headline.fi_age} ages={summary.axis.ages} years={summary.axis.years} mode="dots" lives={summary.paths} compact />
    </ChartSyncProvider>
  );
}
