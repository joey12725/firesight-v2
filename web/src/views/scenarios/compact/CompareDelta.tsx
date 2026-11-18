import type { AssistantChartProps } from '@/assistant/charts';
import { count } from '@/ui';
import { ofHundred } from '@/ui/charts';
import { DeltaDots } from '../DeltaDots';
import { FlipBar } from '../FlipBar';
import { flipsOf, scalarOf } from '../model';
import { deltaTakeaway } from '../text';

/**
 * `compare_delta` in the assistant's conversation: the lives the change rescues and
 * harms as one bar, and the per-life change in wealth at the end as 100 dots. Before
 * the paired statistics arrive it states the two headline rates and nothing more.
 */
export default function CompareDeltaCompact({ summary, comparison }: AssistantChartProps) {
  if (!summary || !comparison) return null;
  const paired = comparison.paired;
  const flips = paired ? flipsOf(paired) : null;
  if (!paired || !flips) {
    const a = ofHundred(summary.headline.success_rate.value);
    const b = ofHundred(comparison.summary.headline.success_rate.value);
    return (
      <p className="text-xs leading-relaxed text-ink-2" aria-busy="true">
        The plan holds in <b className="font-semibold text-ink-1">{b.text}</b> lives with “{comparison.name}” and <b className="font-semibold text-ink-1">{a.text}</b> without. Matching the lives one to one…
      </p>
    );
  }
  const scalar = scalarOf(paired, 'terminal_net_worth');
  return (
    <div className="space-y-2">
      <p className="text-xs leading-relaxed text-pretty text-ink-2">
        Of {count(flips.paths)} lives lived twice, the change rescues <b className="font-semibold text-ink-1">{count(flips.rescued)}</b>, harms <b className="font-semibold text-ink-1">{count(flips.harmed)}</b> and leaves{' '}
        <b className="font-semibold text-ink-1">{count(flips.both + flips.neither)}</b> as they were{flips.withinNoise && flips.rescued + flips.harmed > 0 ? ' (a net effect within sampling noise)' : ''}.
      </p>
      <FlipBar flips={flips} compact />
      {scalar && scalar.dots.length > 0 && (
        <>
          <p className="pt-1 text-xs leading-relaxed text-pretty text-ink-2">{deltaTakeaway(scalar)}</p>
          <DeltaDots scalar={scalar} compact />
        </>
      )}
    </div>
  );
}
