// Charts the assistant can place in the conversation.
//
// The model never supplies data: `show_chart` names a kind and the ids of results it
// already has, and the app draws from those results. Chart engineers register a
// component per kind; until a kind is registered the chat shows a labelled placeholder
// card with the caption, so answers read sensibly either way.
//
// To add a chart (anywhere that runs at startup, for example the view's own module):
//
//   import { registerAssistantChart } from '@/assistant/charts';
//   registerAssistantChart('fan', () => import('@/views/overview/FanChartCompact'));
//
// The module's default export receives `AssistantChartProps`. It is rendered inside a
// card about 320 px wide (the assistant column) and up to 640 px in the phone sheet, at
// whatever height it takes (aim for 160 to 220 px), with `compact: true`: no title (the
// card has the caption), no legend that needs hover, direct labels, tokens from
// src/ui/charts/theme.ts. It must tolerate `summary: null` (the conversation was
// restored from storage and the run is no longer in memory) by rendering nothing; the
// card then offers to re-run.
import { lazy, type ComponentType, type LazyExoticComponent } from 'react';
import type { Comparison, PathTrace, Summary } from '@/engine/output-types';
import type { Profile } from '@/engine/types';
import type { LeverRanking, SweepGrid } from './tools/explore';

export const CHART_KINDS = ['fan', 'fi_age', 'status_stack', 'tornado', 'compare_delta', 'heatmap', 'cash_flow', 'life_story'] as const;
export type ChartKind = (typeof CHART_KINDS)[number];

export interface ChartSpec {
  label: string;
  /** For the model: when to use it. */
  use: string;
  /** For the model: what it must point at. */
  needs: string;
  source: 'run' | 'comparison' | 'data';
  dataPrefix?: 'ranking' | 'grid' | 'trace';
}

export const CHART_SPECS: Record<ChartKind, ChartSpec> = {
  fan: { label: 'Net worth over time', use: 'the range of net worth by age across all lives', needs: 'a run; optional compare_run_id to overlay a scenario', source: 'run' },
  fi_age: { label: 'Age work becomes optional', use: 'the distribution of the age of financial independence, as 100 equally likely lives', needs: 'a run; optional compare_run_id', source: 'run' },
  status_stack: { label: 'Life stages by age', use: 'the share of lives working, retired, cutting spending, depleted or dead at each age', needs: 'a run', source: 'run' },
  tornado: { label: 'What moves the result', use: 'ranked lever effects', needs: 'the ranking_id from rank_levers as data_id', source: 'data', dataPrefix: 'ranking' },
  compare_delta: { label: 'Scenario against plan', use: 'lives rescued and harmed and the per-life change, for one scenario', needs: 'base_run_id as run_id and scenario_run_id as compare_run_id from try_scenario', source: 'comparison' },
  heatmap: { label: 'Trade-off grid', use: 'a metric across a grid of two quantities', needs: 'the grid_id from sweep as data_id', source: 'data', dataPrefix: 'grid' },
  cash_flow: { label: 'Cash flow by year', use: 'average income, tax, spending and saving by year', needs: 'a run', source: 'run' },
  life_story: { label: 'One simulated life', use: 'the net worth, income and events of a single traced life', needs: 'the trace_id from inspect_life as data_id', source: 'data', dataPrefix: 'trace' },
};

export interface AssistantChartProps {
  kind: ChartKind;
  /** Always true in the chat: small, direct-labelled, no hover-only content. */
  compact: true;
  caption: string;
  /** The run the chart refers to. Null when that run is no longer in memory. */
  summary: Summary | null;
  /** The plan that produced `summary`. */
  profile: Profile | null;
  /**
   * Present when the chart compares a scenario (B) with `summary` (A), both on the same seed and lives.
   * `paired` is core's comparison; it is computed off the main thread, so it is null on the first render.
   */
  comparison?: { name: string; summary: Summary; paired: Comparison | null };
  /** `tornado`: the ranking from rank_levers. */
  ranking?: LeverRanking;
  /** `heatmap`: the grid from sweep. */
  grid?: SweepGrid;
  /** `life_story`: the traced life from inspect_life. */
  trace?: PathTrace;
}

export type AssistantChart = ComponentType<AssistantChartProps>;

const registry = new Map<ChartKind, LazyExoticComponent<AssistantChart>>();

export function registerAssistantChart(kind: ChartKind, load: () => Promise<{ default: AssistantChart }>): void {
  registry.set(kind, lazy(load));
}

export function assistantChart(kind: ChartKind): LazyExoticComponent<AssistantChart> | undefined {
  return registry.get(kind);
}
