import { useMemo } from 'react';
import type { AssistantChartProps } from '@/assistant/charts';
import { ChartSyncProvider } from '@/ui/charts';
import { Timeline, crowdFor } from '../Timeline';
import { buildLife } from '../life';

/** `life_story` in the assistant's conversation: one traced life's net worth with its main events marked. */
export default function LifeStoryCompact({ trace, profile, summary }: AssistantChartProps) {
  const life = useMemo(() => (trace ? buildLife(trace, profile) : null), [trace, profile]);
  const crowd = useMemo(() => (life ? crowdFor(life, summary) : null), [life, summary]);
  if (!life) return null;
  return (
    <ChartSyncProvider>
      <Timeline life={life} crowd={crowd} compact />
    </ChartSyncProvider>
  );
}
