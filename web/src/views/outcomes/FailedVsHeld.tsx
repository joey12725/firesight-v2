// What was different about the lives that fell short: one small panel per circumstance
// (returns, care costs, lifespan, time out of work ...), each showing where the median
// life that fell short and the median life that held sit within the spread of all lives.
// Ordered by how far apart the two groups are, so the first panels are the ones that matter.
import type { Summary } from '@/engine/output-types';
import { ChartFrame, ChartSyncProvider, Cursor, Legend, PlotClip, SvgLayer, TipRow, TipTitle, linearScale, textWidth, useChartTheme, withAlpha, type TableData } from '@/ui/charts';
import { XAxis } from '@/ui/charts/ext/outcomes-XAxis';
import { contrasts, type Contrast } from './data';
import { contrastSentence, contrastValue } from './text';

export function contrastTable(summary: Summary): TableData {
  const rows = contrasts(summary);
  return {
    caption: 'Lives that fell short of the plan against lives that held, on each circumstance (medians)',
    columns: [
      { key: 'what', label: 'Circumstance', align: 'left' },
      { key: 'failed', label: 'Median, fell short', align: 'left' },
      { key: 'held', label: 'Median, held', align: 'left' },
      { key: 'separation', label: 'Gap, in middle-halves of all lives', format: (v) => v.toFixed(2) },
      { key: 'tail_failed', label: 'Fell short: share in the tail quarter', format: (v) => `${(v * 100).toFixed(0)}%` },
      { key: 'tail_held', label: 'Held: share in the tail quarter', format: (v) => `${(v * 100).toFixed(0)}%` },
    ],
    rows: rows.map((c) => ({ what: c.label, failed: contrastValue(c, c.failed), held: contrastValue(c, c.succeeded), separation: c.separation, tail_failed: c.tail?.failed ?? null, tail_held: c.tail?.succeeded ?? null })),
  };
}

export function ContrastLegend() {
  const theme = useChartTheme();
  return (
    <div data-chart-legend>
      <Legend
        items={[
          { key: 'failed', label: 'Median life that fell short', color: theme.status.depleted, swatch: 'dot' },
          { key: 'held', label: 'Median life that held', color: theme.baseline.line, swatch: 'dot' },
          { key: 'all', label: 'All lives', color: withAlpha(theme.ink.tertiary, 0.45), swatch: 'box' },
        ]}
      />
    </div>
  );
}

function Panel({ c }: { c: Contrast }) {
  return (
    <div className="min-w-0">
      <h4 className="text-sm font-medium text-ink-1">{c.label}</h4>
      <ChartSyncProvider>
        <ChartFrame height={112} margin={{ top: 36, right: 10, bottom: 24, left: 10 }} aria-label={`${c.label}: median ${contrastValue(c, c.failed)} for lives that fell short, ${contrastValue(c, c.succeeded)} for lives that held`}>
          {({ plot, theme }) => {
            const x = linearScale(c.domain, [plot.x, plot.right]);
            const tallest = Math.max(1, ...c.counts);
            const y = linearScale([0, tallest], [plot.bottom, plot.y + 2]);
            const bars = c.counts.map((n, i) => ({ n, lo: c.edges[i] as number, hi: c.edges[i + 1] as number })).filter((b) => b.n > 0 && Number.isFinite(b.lo) && Number.isFinite(b.hi));
            const total = c.counts.reduce((s, n) => s + n, 0);
            // Each label sits on the side of its rule away from the other group's rule, so
            // two close medians never print over each other; a surface halo keeps a label
            // legible where the plot edge forces it across a rule.
            const label = (value: number, other: number, text: string, row: number, color: string) => {
              const px = x(value);
              const w = textWidth(text, 11) + 12;
              const wantLeft = value < other || (value === other && row > 20);
              const fitsLeft = px - 6 - w >= plot.x;
              const fitsRight = px + 6 + w <= plot.right;
              const onLeft = wantLeft ? fitsLeft || !fitsRight : !fitsRight && fitsLeft;
              const left = Math.min(plot.right - w, Math.max(plot.x, onLeft ? px - 6 - w : px + 6));
              return (
                <g>
                  <line x1={px} x2={px} y1={row - 8} y2={plot.bottom} stroke={color} strokeWidth={1.5} />
                  <circle cx={left + 4} cy={row - 3.5} r={3.5} fill={color} stroke={theme.surface} strokeWidth={1.5} />
                  <text x={left + 11} y={row} fontSize={11} fill={theme.ink.primary} fontWeight={500} stroke={theme.surface} strokeWidth={3} paintOrder="stroke" strokeLinejoin="round" style={{ fontVariantNumeric: 'tabular-nums' }}>
                    {text}
                  </text>
                  <circle cx={px} cy={plot.bottom} r={4} fill={color} stroke={theme.surface} strokeWidth={2} />
                </g>
              );
            };
            return (
              <>
                <SvgLayer>
                  <PlotClip>
                    {bars.map((b, i) => {
                      const x0 = x(b.lo);
                      const x1 = x(b.hi);
                      return <rect key={i} x={x0 + 0.5} y={y(b.n)} width={Math.max(1, x1 - x0 - 1)} height={Math.max(0, plot.bottom - y(b.n))} fill={withAlpha(theme.ink.tertiary, 0.32)} />;
                    })}
                  </PlotClip>
                  <XAxis x={x} format={(v) => contrastValue(c, v)} count={3} />
                  {label(c.succeeded, c.failed, contrastValue(c, c.succeeded), 26, theme.baseline.line)}
                  {label(c.failed, c.succeeded, contrastValue(c, c.failed), 11, theme.status.depleted)}
                </SvgLayer>
                <Cursor
                  count={bars.length}
                  xOf={(i) => x(((bars[i]?.lo ?? 0) + (bars[i]?.hi ?? 0)) / 2)}
                  label={`${c.label}, all lives`}
                  crosshair={false}
                  describe={(i) => `${contrastValue(c, bars[i]?.lo ?? 0)} to ${contrastValue(c, bars[i]?.hi ?? 0)}: ${Math.round(((bars[i]?.n ?? 0) / total) * 1000) / 10}% of all lives.`}
                  marks={(i) => {
                    const b = bars[i];
                    return b ? <rect x={x(b.lo) + 0.5} y={y(b.n)} width={Math.max(1, x(b.hi) - x(b.lo) - 1)} height={Math.max(0, plot.bottom - y(b.n))} fill={withAlpha(theme.ink.secondary, 0.55)} /> : null;
                  }}
                  tooltip={(i) => (
                    <>
                      <TipTitle>
                        {contrastValue(c, bars[i]?.lo ?? 0)} to {contrastValue(c, bars[i]?.hi ?? 0)}
                      </TipTitle>
                      <TipRow color={withAlpha(theme.ink.tertiary, 0.6)} swatch="box" label="All lives" value={`${(((bars[i]?.n ?? 0) / total) * 100).toFixed(1)}%`} />
                      <TipRow color={theme.status.depleted} swatch="box" label="Median, fell short" value={contrastValue(c, c.failed)} />
                      <TipRow color={theme.baseline.line} swatch="box" label="Median, held" value={contrastValue(c, c.succeeded)} />
                    </>
                  )}
                />
              </>
            );
          }}
        </ChartFrame>
      </ChartSyncProvider>
      <p className="mt-1 text-xs leading-relaxed text-pretty text-ink-3">{contrastSentence(c)}</p>
    </div>
  );
}

export function FailedVsHeld({ summary }: { summary: Summary }) {
  const rows = contrasts(summary);
  if (rows.length === 0) return <p className="py-4 text-sm text-ink-2">{summary.failure.failed === 0 ? 'Every simulated life holds, so there are no lives that fell short to compare.' : `Only ${summary.failure.failed} simulated ${summary.failure.failed === 1 ? 'life falls' : 'lives fall'} short: too few to compare with the rest.`}</p>;
  return (
    <div className="grid grid-cols-1 gap-x-10 gap-y-7 @xl:grid-cols-2 @5xl:grid-cols-3">
      {rows.map((c) => (
        <Panel key={c.scalar} c={c} />
      ))}
    </div>
  );
}
