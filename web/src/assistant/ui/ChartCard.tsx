import { createElement, Suspense, useEffect, useMemo, useState } from 'react';
import type { Comparison, PathTrace } from '@/engine/output-types';
import { Skeleton } from '@/ui';
import { assistantChart, CHART_SPECS, type AssistantChartProps } from '../charts';
import { getSession } from '../session';
import type { LeverRanking, SweepGrid } from '../tools/explore';
import type { ToolDisplay } from '../tools/types';

type ChartDisplay = Extract<ToolDisplay, { type: 'chart' }>;

/** A chart the assistant asked for, drawn by the app from results it already holds. */
export function ChartCard({ display }: { display: ChartDisplay }) {
  const spec = CHART_SPECS[display.kind];
  const registered = assistantChart(display.kind);
  // The paired statistics are computed off the main thread; the chart draws the two
  // summaries at once and gains them when they arrive.
  const [paired, setPaired] = useState<{ key: string; value: Comparison | null } | null>(null);
  const pairKey = display.runId && display.compareRunId ? `${display.runId}|${display.compareRunId}` : null;
  useEffect(() => {
    if (!pairKey || !display.runId || !display.compareRunId) return;
    let live = true;
    void getSession()
      .comparison(display.runId, display.compareRunId)
      .then((value) => live && setPaired({ key: pairKey, value }));
    return () => {
      live = false;
    };
  }, [pairKey, display.runId, display.compareRunId]);
  const comparison = paired && paired.key === pairKey ? paired.value : null;

  const props = useMemo((): AssistantChartProps | null => {
    const runs = getSession().runs;
    const run = display.runId ? runs.get(display.runId) : undefined;
    const other = display.compareRunId ? runs.get(display.compareRunId) : undefined;
    const data = display.dataId ? runs.getData(display.dataId) : undefined;
    if (spec.source !== 'data' && !run) return null;
    if (spec.source === 'data' && data === undefined) return null;
    return {
      kind: display.kind,
      compact: true,
      caption: display.caption,
      summary: run?.summary ?? null,
      profile: run?.profile ?? null,
      ...(run && other ? { comparison: { name: other.label, summary: other.summary, paired: comparison } } : {}),
      ...(spec.dataPrefix === 'ranking' ? { ranking: data as LeverRanking } : {}),
      ...(spec.dataPrefix === 'grid' ? { grid: data as SweepGrid } : {}),
      ...(spec.dataPrefix === 'trace' ? { trace: data as PathTrace } : {}),
    };
  }, [display, spec, comparison]);

  return (
    <figure className="mt-3 overflow-hidden rounded-md border border-line bg-surface-2 first:mt-0">
      <div className="border-b border-line px-3 py-2">
        <p className="eyebrow">{spec.label}</p>
      </div>
      <div className="px-3 py-3">
        {registered && props ? (
          <Suspense fallback={<Skeleton className="h-40 w-full" />}>{createElement(registered, props)}</Suspense>
        ) : (
          <div className="flex h-24 items-center justify-center rounded-sm border border-dashed border-line-strong px-4 text-center text-xs leading-snug text-ink-3">
            {props ? 'This chart is not drawn in this build yet. The figures behind it are in the answer.' : 'The run behind this chart is no longer in memory. Ask again to redraw it.'}
          </div>
        )}
      </div>
      <figcaption className="border-t border-line px-3 py-2 text-xs leading-snug text-ink-2">{display.caption}</figcaption>
    </figure>
  );
}
