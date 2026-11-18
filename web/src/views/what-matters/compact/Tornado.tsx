import type { AssistantChartProps } from '@/assistant/charts';
import type { LeverRanking } from '@/assistant/tools/explore';
import { GROUP_LABEL, formatDelta, groupOf, type EffectState, type MetricDef } from '../model';
import { TornadoPlot, type TornadoBar, type TornadoGroup } from '../Tornado';

type Scale = Pick<MetricDef, 'axis' | 'unit' | 'better'>;

/** The assistant's metrics as the plot's scale: what the axis says, its unit, and which way is better. */
const SCALES: Record<LeverRanking['metric'], Scale> = {
  success_rate: { axis: 'Lives in 100 in which the plan holds', unit: 'share', better: 1 },
  depletion_rate: { axis: 'Lives in 100 in which money runs short', unit: 'share', better: -1 },
  median_fi_age: { axis: 'Age work becomes optional, middle life', unit: 'age', better: -1 },
  median_retirement_age: { axis: 'Retirement age, middle life', unit: 'age', better: -1 },
  median_terminal_net_worth: { axis: "Net worth at the end, middle life, today's dollars", unit: 'dollars', better: 1 },
  p10_terminal_net_worth: { axis: "Net worth at the end, 10th of 100 lives, today's dollars", unit: 'dollars', better: 1 },
  delivered_spending_ratio: { axis: 'Retirement spending delivered, % of plan', unit: 'ratio', better: 1 },
  median_lifetime_tax: { axis: "Lifetime tax, middle life, today's dollars", unit: 'dollars', better: -1 },
};

const MAX_LEVERS = 5;

/** A stored `rank_levers` result as the groups the tornado draws. */
export function rankingGroups(ranking: LeverRanking, scale: Scale): { groups: TornadoGroup[]; hidden: number } {
  const levers = [...ranking.levers].sort((a, b) => (b.swing ?? -1) - (a.swing ?? -1));
  const shown = levers.slice(0, MAX_LEVERS);
  const bar = (leverId: string, s: LeverRanking['levers'][number]['settings'][number], i: number): TornadoBar => {
    const state: EffectState = s.delta === null ? 'unknown' : s.within_noise ? 'noise' : s.delta === 0 ? 'none' : 'effect';
    const half = s.std_error === null ? null : 1.96 * s.std_error;
    return {
      key: `${leverId}:${i}`,
      label: s.label,
      state,
      delta: s.delta,
      lo: s.delta !== null && half !== null ? s.delta - half : null,
      hi: s.delta !== null && half !== null ? s.delta + half : null,
      value: s.value,
      statement: state === 'unknown' ? 'Not available.' : state === 'noise' ? 'Cannot be told from no effect at this number of lives.' : `Paired change ${formatDelta(scale.unit, s.delta)}.`,
    };
  };
  const groups = (['control', 'world'] as const)
    .map((g) => ({ id: g, title: GROUP_LABEL[g], levers: shown.filter((l) => groupOf(l.id) === g).map((l) => ({ id: l.id, label: l.label, bars: l.settings.map((s, i) => bar(l.id, s, i)) })) }))
    .filter((g) => g.levers.length > 0);
  return { groups, hidden: levers.length - shown.length };
}

/** `tornado` in the assistant's conversation: the largest lever effects from a stored ranking, with paired intervals. */
export default function TornadoCompact({ ranking }: AssistantChartProps) {
  if (!ranking || ranking.base === null) return null;
  const scale = SCALES[ranking.metric];
  const { groups, hidden } = rankingGroups(ranking, scale);
  if (groups.length === 0) return null;
  return (
    <div>
      <TornadoPlot groups={groups} metric={scale} base={ranking.base} compact />
      <p className="mt-1 text-2xs leading-4 text-ink-3">
        Paired against the plan on the same {ranking.paths.toLocaleString('en-US')} lives; whiskers are 95% intervals, hollow bars include zero.
        {hidden > 0 && ` ${hidden} more ${hidden === 1 ? 'lever has' : 'levers have'} smaller effects.`}
      </p>
    </div>
  );
}
