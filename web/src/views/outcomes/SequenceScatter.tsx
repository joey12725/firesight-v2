// Sequence-of-returns risk, shown rather than asserted: every retired life in the sample
// as a dot, placed by the real return of its first ten retirement years and by how it
// ended. Lives that ran short are a different colour; the line is the median ending of
// lives with similar early returns; the strip above counts how many of them ran short.
// Dragging (or the buttons beneath) picks out a range of returns and lists those lives.
import { useMemo, useState } from 'react';
import type { Summary } from '@/engine/output-types';
import { navigateToLife } from '@/state/route';
import { Button, Icon, moneyCompact } from '@/ui';
import {
  CanvasLayer,
  ChartFrame,
  ChartSyncProvider,
  Cursor,
  EndLabels,
  Legend,
  SvgLayer,
  TipNote,
  TipRow,
  TipTitle,
  ValueAxis,
  clipPlot,
  drawGrid,
  linearScale,
  niceDomain,
  ofHundred,
  symlogScale,
  useChartTheme,
  withAlpha,
  type Scale,
  type TableData,
} from '@/ui/charts';
import { XAxis } from '@/ui/charts/ext/outcomes-XAxis';
import { luckSplit, quantileSorted, returnBins, scatterLives, selectLives, type ReturnBin, type ScatterLife } from './data';

export type ScatterMode = 'wealth' | 'age';

const pct = (v: number, digits = 1) => `${(v * 100).toFixed(digits).replace(/^-(0(\.0+)?)$/, '$1').replace('-', '−')}%`;
const STRIP = 46;
const NEVER_BAND = 34;

export function scatterTable(summary: Summary): TableData {
  const bins = returnBins(scatterLives(summary));
  return {
    caption: 'Retired lives grouped by the real return of their first ten retirement years (equal numbers of lives per group)',
    columns: [
      { key: 'lo', label: 'Return from', format: (v) => pct(v) },
      { key: 'hi', label: 'Return to', format: (v) => pct(v) },
      { key: 'lives', label: 'Lives in the sample' },
      { key: 'short', label: 'Ran short' },
      { key: 'share', label: 'Share that ran short', format: (v) => pct(v) },
      { key: 'p25', label: 'Wealth at the end, 25th percentile', format: moneyCompact },
      { key: 'median', label: 'Median', format: moneyCompact },
      { key: 'p75', label: '75th percentile', format: moneyCompact },
    ],
    rows: bins.map((b) => ({ lo: b.lo, hi: b.hi, lives: b.lives, short: b.short, share: b.shortShare, p25: b.p25Wealth, median: b.medianWealth, p75: b.p75Wealth })),
  };
}

export function ScatterLegend({ mode }: { mode: ScatterMode }) {
  const theme = useChartTheme();
  return (
    <div data-chart-legend>
      <Legend
        items={[
          { key: 'held', label: 'The money lasted', color: theme.baseline.line, swatch: 'dot' },
          { key: 'short', label: 'Ran short of essentials', color: theme.status.depleted, swatch: 'dot' },
          { key: 'trend', label: mode === 'wealth' ? 'Median ending, lives with similar early returns' : 'Median age it ran short, lives with similar early returns', color: theme.ink.primary, swatch: 'line' },
        ]}
      />
    </div>
  );
}

/** A stable pseudo-random number in 0..1 for a path, so jittered dots do not move between renders. */
const jitter = (path: number) => ((Math.imul(path + 1, 2654435761) >>> 0) % 1000) / 1000;

function describeLife(l: ScatterLife): string {
  return l.held ? `ended with ${moneyCompact(l.wealth)}` : `ran short at ${Math.floor(l.depletionAge ?? 0)}`;
}

export function SequenceScatter({ summary, mode }: { summary: Summary; mode: ScatterMode }) {
  const lives = useMemo(() => scatterLives(summary), [summary]);
  const bins = useMemo(() => returnBins(lives), [lives]);
  const split = useMemo(() => luckSplit(lives), [lives]);
  const [hovered, setHovered] = useState<ScatterLife | null>(null);
  const [picked, setPicked] = useState<[number, number] | null>(null);
  const selection = useMemo(() => (picked ? selectLives(lives, picked[0], picked[1]) : null), [lives, picked]);
  const theme = useChartTheme();
  const binAt = (i: number) => bins[i] as ReturnBin;

  if (lives.length < 20 || bins.length === 0) {
    return (
      <p className="flex items-start gap-2 py-6 text-sm text-ink-2">
        <Icon name="info" className="mt-0.5 shrink-0 text-ink-3" />
        <span>{lives.length === 0 ? 'No simulated life has retired, so there are no early retirement returns to plot.' : `Only ${lives.length} retired lives so far: too few to draw a pattern from.`}</span>
      </p>
    );
  }

  // The axis covers all but the most extreme two lives in a thousand at each end, which would otherwise squeeze everyone else.
  const rets = lives.map((l) => l.ret).sort((a, b) => a - b);
  const xDomain = niceDomain(quantileSorted(rets, 0.002) as number, quantileSorted(rets, 0.998) as number, 6);
  const beyond = rets.filter((r) => r < xDomain[0] || r > xDomain[1]).length;
  const failedAges = lives.filter((l) => !l.held).map((l) => l.depletionAge as number);
  const maxShare = Math.max(0.05, ...bins.map((b) => b.shortShare));
  const n = lives.length;
  const alpha = Math.min(0.85, Math.max(0.28, 1.5 / Math.sqrt(n / 250)));
  const radius = n > 2500 ? 1.6 : n > 800 ? 2 : 2.6;

  return (
    <div>
      <ChartSyncProvider>
        <ChartFrame
          height={(w) => Math.round(Math.min(430, Math.max(280, w * 0.5))) + STRIP + 28}
          margin={{ top: STRIP + 40, right: 64, bottom: 44, left: 52 }}
          aria-label={`${n.toLocaleString('en-US')} retired lives by the return of their first ten retirement years and how they ended`}
        >
          {(frame) => {
            const { plot } = frame;
            const x = linearScale(xDomain, [plot.x, plot.right]);
            const wealthTop = Math.max(1, ...lives.map((l) => l.wealth));
            const wealthBottom = Math.min(0, ...lives.map((l) => l.wealth));
            const never = mode === 'age' ? NEVER_BAND : 0;
            const y: Scale =
              mode === 'wealth'
                ? symlogScale([wealthBottom * 1.1, wealthTop * 1.2], [plot.bottom, plot.y], Math.max(10_000, 10 ** (Math.floor(Math.log10(Math.max(wealthTop, 1e5))) - 2)))
                : linearScale(failedAges.length ? [Math.floor(Math.min(...failedAges)) - 1, Math.ceil(Math.max(...failedAges)) + 1] : [60, 100], [plot.bottom, plot.y + never + 10]);
            const ticks = y.ticks(Math.max(3, Math.round(plot.height / 60)));
            const yOf = (l: ScatterLife) => (mode === 'wealth' ? y(l.wealth) : l.held ? plot.y + 4 + jitter(l.path) * (never - 8) : y(l.depletionAge as number));
            const stripTop = 14;
            const yShare = linearScale([0, maxShare], [stripTop + STRIP, stripTop]);
            const trend = bins.map((b) => {
              if (mode === 'wealth') return { x: x(b.mid), y: y(b.medianWealth) };
              const ages = lives.filter((l) => !l.held && l.ret >= b.lo && l.ret <= b.hi).map((l) => l.depletionAge as number).sort((p, q) => p - q);
              return ages.length >= 3 ? { x: x(b.mid), y: y(ages[Math.floor(ages.length / 2)] as number) } : null;
            });
            const trendPoints = trend.filter((p): p is { x: number; y: number } => p !== null);
            const last = trendPoints[trendPoints.length - 1];

            return (
              <>
                <CanvasLayer
                  deps={[lives, mode, xDomain[0], xDomain[1], alpha]}
                  draw={(ctx) => {
                    drawGrid(ctx, plot, y, ticks, theme.grid, theme.axis);
                    // Zero real return: to its left, ten years of retirement left the portfolio poorer before any spending.
                    ctx.strokeStyle = theme.axis;
                    ctx.lineWidth = 1;
                    ctx.beginPath();
                    ctx.moveTo(Math.round(x(0)) + 0.5, plot.y);
                    ctx.lineTo(Math.round(x(0)) + 0.5, plot.bottom);
                    ctx.stroke();
                    const done = clipPlot(ctx, plot, 3);
                    for (const pass of [true, false]) {
                      ctx.fillStyle = pass ? withAlpha(theme.baseline.line, alpha) : withAlpha(theme.status.depleted, Math.min(0.9, alpha + 0.3));
                      const r = pass ? radius : radius + 0.6;
                      ctx.beginPath();
                      for (const l of lives) {
                        if (l.held !== pass) continue;
                        const px = x(l.ret);
                        const py = yOf(l);
                        ctx.moveTo(px + r, py);
                        ctx.arc(px, py, r, 0, Math.PI * 2);
                      }
                      ctx.fill();
                    }
                    // The trend, with a surface-coloured casing so it reads over the dots.
                    for (const [width, color] of [
                      [5, theme.surface],
                      [2, theme.ink.primary],
                    ] as const) {
                      ctx.strokeStyle = color;
                      ctx.lineWidth = width;
                      ctx.lineJoin = 'round';
                      ctx.lineCap = 'round';
                      ctx.beginPath();
                      trendPoints.forEach((p, i) => (i === 0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y)));
                      ctx.stroke();
                    }
                    done();
                    // The strip: of the lives in each group, how many ran short.
                    ctx.fillStyle = theme.status.depleted;
                    for (const b of bins) {
                      const top = yShare(b.shortShare);
                      const h = stripTop + STRIP - top;
                      if (h <= 0) continue;
                      const w = Math.max(6, Math.min(16, plot.width / bins.length / 2.2));
                      ctx.beginPath();
                      ctx.roundRect(x(b.mid) - w / 2, top, w, h, [Math.min(4, h), Math.min(4, h), 0, 0]);
                      ctx.fill();
                    }
                  }}
                />
                <SvgLayer>
                  <ValueAxis y={y} format={mode === 'wealth' ? 'money' : 'age'} ticks={ticks} grid={false} title={mode === 'wealth' ? 'Net worth at the end, today’s dollars' : 'Age the money ran short'} />
                  <XAxis x={x} format="percent" title="Real return a year over the first ten years of retirement" />
                  <g fontSize={10}>
                    <line x1={plot.x} x2={plot.right} y1={stripTop + STRIP + 0.5} y2={stripTop + STRIP + 0.5} stroke={theme.axis} strokeWidth={1} />
                    <text x={0} y={stripTop - 4} fill={theme.label} fontSize={11}>
                      Ran short, of 100 lives with these returns
                    </text>
                    <text x={plot.x - 8} y={yShare(maxShare) + 7} textAnchor="end" fill={theme.label} fontSize={11}>
                      {Math.round(maxShare * 100)}
                    </text>
                    <text x={plot.x - 8} y={stripTop + STRIP + 3} textAnchor="end" fill={theme.label} fontSize={11}>
                      0
                    </text>
                  </g>
                  {x(0) > plot.x + 40 && x(0) < plot.right - 60 && (
                    <text x={x(0) + 5} y={plot.y + never + 11} fontSize={10.5} fill={theme.ink.tertiary} stroke={theme.surface} strokeWidth={3} paintOrder="stroke">
                      no growth after inflation
                    </text>
                  )}
                  {mode === 'age' && (
                    <g>
                      <line x1={plot.x} x2={plot.right} y1={plot.y + never + 0.5} y2={plot.y + never + 0.5} stroke={theme.grid} strokeWidth={1} />
                      <text x={plot.x - 8} y={plot.y + never / 2 + 4} textAnchor="end" fontSize={11} fill={theme.label}>
                        Never
                      </text>
                    </g>
                  )}
                  {last && <EndLabels labels={[{ key: 'trend', y: last.y, text: 'Median', color: theme.ink.primary, strong: true }]} />}
                  {selection && (
                    <rect x={Math.max(plot.x, x(selection.lo))} y={plot.y} width={Math.max(2, Math.min(plot.right, x(selection.hi)) - Math.max(plot.x, x(selection.lo)))} height={plot.height} fill={theme.ink.primary} fillOpacity={0.06} stroke={theme.ink.tertiary} strokeWidth={1} />
                  )}
                  {hovered && (
                    <circle cx={x(hovered.ret)} cy={yOf(hovered)} r={5} fill={hovered.held ? theme.baseline.line : theme.status.depleted} stroke={theme.ink.primary} strokeWidth={1.5} />
                  )}
                </SvgLayer>
                <Cursor
                  count={bins.length}
                  xOf={(i) => x(binAt(i).mid)}
                  label="Early retirement returns and how lives ended"
                  describe={(i) => {
                    const b = binAt(i);
                    return `First-decade returns from ${pct(b.lo)} to ${pct(b.hi)}: ${b.lives} lives, ${ofHundred(b.shortShare).text} ran short, median ending ${moneyCompact(b.medianWealth)}. Press Enter to list these lives.`;
                  }}
                  clickable={hovered !== null}
                  onPointer={(i, px, py) => {
                    if (i === null) return setHovered(null);
                    let best: ScatterLife | null = null;
                    let distance = 12 * 12;
                    for (const l of lives) {
                      const dx = x(l.ret) - px;
                      if (dx > 12 || dx < -12) continue;
                      const dy = yOf(l) - py;
                      const d = dx * dx + dy * dy;
                      if (d < distance) {
                        distance = d;
                        best = l;
                      }
                    }
                    setHovered(best);
                  }}
                  onSelect={(i, py) => {
                    if (py !== null && hovered) return navigateToLife(hovered.path, 'scatter');
                    if (py === null) setPicked([binAt(i).lo, binAt(i).hi]);
                  }}
                  onBrush={(from, to) => setPicked([binAt(from).lo, binAt(to).hi])}
                  marks={(i) => {
                    const p = trend[i];
                    return p ? <circle cx={p.x} cy={p.y} r={4} fill={theme.ink.primary} stroke={theme.surface} strokeWidth={2} /> : null;
                  }}
                  tooltip={(i) => {
                    const b = binAt(i);
                    return (
                      <>
                        <TipTitle detail={`${b.lives} lives`}>
                          {pct(b.lo)} to {pct(b.hi)} a year
                        </TipTitle>
                        <TipRow color={theme.status.depleted} swatch="box" label="Ran short" value={ofHundred(b.shortShare).text} strong />
                        <TipRow color={theme.ink.primary} label="Median ending" value={moneyCompact(b.medianWealth)} />
                        <TipRow label="Middle half" value={`${moneyCompact(b.p25Wealth)} to ${moneyCompact(b.p75Wealth)}`} />
                        {hovered ? (
                          <TipNote>
                            Life #{hovered.path}: {pct(hovered.ret)} a year, {describeLife(hovered)}. Click to open it.
                          </TipNote>
                        ) : (
                          <TipNote>Drag across to list the lives in a range of returns.</TipNote>
                        )}
                      </>
                    );
                  }}
                />
              </>
            );
          }}
        </ChartFrame>
      </ChartSyncProvider>

      <div className="mt-4 rounded-md border border-line bg-surface-1 px-4 py-3" data-testid="scatter-selection">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <p className="text-xs font-medium text-ink-2">Pick out lives by their early returns</p>
          <div className="ml-auto flex flex-wrap gap-1.5">
            {split && (
              <Button size="sm" variant={picked && picked[1] === split.threshold ? 'primary' : 'secondary'} onClick={() => setPicked([rets[0] as number, split.threshold])}>
                Unluckiest fifth
              </Button>
            )}
            {bins.length > 2 && (
              <Button size="sm" onClick={() => setPicked([binAt(Math.floor(bins.length / 2)).lo, binAt(Math.floor(bins.length / 2)).hi])}>
                Middling
              </Button>
            )}
            {bins.length > 2 && (
              <Button size="sm" onClick={() => setPicked([binAt(bins.length - 1).lo, binAt(bins.length - 1).hi])}>
                Luckiest
              </Button>
            )}
            {picked && (
              <Button size="sm" variant="ghost" icon="x" onClick={() => setPicked(null)}>
                Clear
              </Button>
            )}
          </div>
        </div>
        {selection ? (
          selection.lives.length === 0 ? (
            <p className="mt-2 text-sm text-ink-2">No life in the sample has returns in that range.</p>
          ) : (
            <>
              <p className="mt-2 text-sm text-pretty text-ink-1">
                <b className="font-semibold">{selection.lives.length.toLocaleString('en-US')}</b> lives returned between <b className="font-semibold">{pct(Math.max(selection.lo, Math.min(...selection.lives.map((l) => l.ret))))}</b> and{' '}
                <b className="font-semibold">{pct(Math.min(selection.hi, Math.max(...selection.lives.map((l) => l.ret))))}</b> a year in their first retirement decade. <b className="font-semibold">{ofHundred(selection.shortShare).text}</b> of them ran short
                {selection.medianWealth !== null && (
                  <>
                    ; the middle one ended with <b className="font-semibold">{moneyCompact(selection.medianWealth)}</b>
                  </>
                )}
                .
              </p>
              <ul className="mt-2 flex flex-wrap gap-1.5" aria-label="Lives in this range, worst endings first">
                {spread(selection.lives, 8).map((l) => (
                  <li key={l.path}>
                    <button
                      type="button"
                      className="inline-flex h-7 items-center gap-1.5 rounded-sm border border-line bg-surface-0 px-2 text-xs text-ink-2 hover:border-line-strong hover:text-ink-1"
                      onClick={() => navigateToLife(l.path, 'scatter')}
                    >
                      <span aria-hidden className="inline-block size-2 rounded-full" style={{ background: l.held ? theme.baseline.line : theme.status.depleted }} />
                      <span className="num">#{l.path}</span>
                      <span>{describeLife(l)}</span>
                      <Icon name="arrowRight" size={11} />
                    </button>
                  </li>
                ))}
              </ul>
              <p className="mt-2 text-2xs text-ink-3">Eight of these lives, spread from the worst ending to the best. Each opens in Life stories.</p>
            </>
          )
        ) : (
          <p className="mt-1.5 text-xs leading-relaxed text-ink-3">
            Drag across the chart, or use a button, to list the lives in a range of returns and open any of them. Click a single dot to open that life.
            {beyond > 0 ? ` ${beyond} ${beyond === 1 ? 'life lies' : 'lives lie'} beyond the ends of the axis: counted in the end groups, not drawn.` : ''}
          </p>
        )}
      </div>
    </div>
  );
}

/** `count` lives spread evenly through a ranked list, ends included. */
function spread(lives: ScatterLife[], count: number): ScatterLife[] {
  if (lives.length <= count) return lives;
  return Array.from({ length: count }, (_, i) => lives[Math.round((i / (count - 1)) * (lives.length - 1))] as ScatterLife);
}
