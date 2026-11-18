// The marks of the Assumptions view: interval rows for what each market model implies,
// range rows for the historical record, small series over age (survival, benefits,
// health costs) that share the view's crosshair, and the tax-bracket ladder.
import { useMemo } from 'react';
import type { Summary } from '@/engine/output-types';
import { moneyCompact } from '@/ui';
import {
  AgeAxis,
  CanvasLayer,
  ChartFrame,
  Cursor,
  EndLabels,
  SvgLayer,
  TipRow,
  TipTitle,
  ValueAxis,
  clipPlot,
  drawGrid,
  drawLine,
  inkOn,
  linearScale,
  niceDomain,
  ofHundred,
  summaryTime,
  textWidth,
  useAnimated,
  useChartSync,
  useTimeRange,
  type EndLabel,
} from '@/ui/charts';
import { XAxis } from '@/ui/charts/ext/outcomes-XAxis';
import { HISTORY, placeOnLadder, type Bracket, type ModelRange, type TaxPoint } from './data';

const pct = (v: number, digits = 1) => `${(v * 100).toFixed(digits).replace(/^-(0(\.0+)?)$/, '$1').replace('-', '−')}%`;

/** One row per market model: the 5th to 95th percentile as a line, the middle half as a bar, the median as a dot. */
export function ModelRangeStrip({ rows }: { rows: ModelRange[] }) {
  const ROW = 40;
  return (
    <ChartFrame height={rows.length * ROW + 46} margin={{ top: 4, right: 12, bottom: 42, left: 4 }} aria-label="Thirty-year real return of US stocks under each market model, and in the historical record">
      {({ plot, theme }) => {
        const x = linearScale(niceDomain(Math.min(0, ...rows.map((r) => r.p5)), Math.max(...rows.map((r) => r.p95)), 5), [plot.x, plot.right]);
        return (
          <SvgLayer>
            <XAxis x={x} format="percent" grid title="Real return a year over 30 years" />
            {rows.map((r, i) => {
              const top = plot.y + i * ROW;
              const cy = top + 27;
              const color = r.current ? theme.baseline.line : r.record ? theme.ink.primary : theme.ink.tertiary;
              return (
                <g key={r.kind}>
                  <text x={plot.x} y={top + 13} fontSize={11.5} fontWeight={r.current ? 600 : 400} fill={r.current ? theme.ink.primary : theme.ink.secondary}>
                    {r.label}
                    {r.current ? ' · this plan' : ''}
                  </text>
                  <line x1={x(r.p5)} x2={x(r.p95)} y1={cy} y2={cy} stroke={color} strokeWidth={2} strokeLinecap="round" opacity={r.current || r.record ? 1 : 0.8} />
                  <rect x={x(r.p25)} y={cy - 4} width={Math.max(1, x(r.p75) - x(r.p25))} height={8} rx={2} fill={color} opacity={r.record ? 0.55 : 1} />
                  <circle cx={x(r.p50)} cy={cy} r={4.5} fill={theme.surface} stroke={color} strokeWidth={2} />
                  <text x={Math.min(plot.right - textWidth(pct(r.p95), 10.5), x(r.p95) + 7)} y={cy + (x(r.p95) + 7 + textWidth(pct(r.p95), 10.5) > plot.right ? -8 : 3.5)} fontSize={10.5} fill={theme.label} style={{ fontVariantNumeric: 'tabular-nums' }}>
                    {pct(r.p95)}
                  </text>
                  <text x={Math.max(plot.x + textWidth(pct(r.p5), 10.5), x(r.p5) - 7)} y={cy + 3.5} textAnchor="end" fontSize={10.5} fill={theme.label} style={{ fontVariantNumeric: 'tabular-nums' }}>
                    {pct(r.p5)}
                  </text>
                </g>
              );
            })}
          </SvgLayer>
        );
      }}
    </ChartFrame>
  );
}

/** The record itself: for each asset, its worst and best calendar year, the usual range and the average, after inflation. */
export function RecordRanges() {
  const assets = HISTORY.assets;
  const ROW = 44;
  return (
    <ChartFrame height={assets.length * ROW + 46} margin={{ top: 4, right: 12, bottom: 42, left: 4 }} aria-label="Calendar-year returns after inflation in the historical record, 1927 to 2025, by asset">
      {({ plot, theme }) => {
        const x = linearScale(niceDomain(Math.min(...assets.map((a) => a.worst[1] as number)) - (plot.width < 520 ? 0.02 : 0.2), Math.max(...assets.map((a) => a.best[1] as number)) + (plot.width < 520 ? 0.02 : 0.22), 5), [plot.x, plot.right]);
        return (
          <SvgLayer>
            <XAxis x={x} format="percent" grid title="Return in a calendar year, after inflation" />
            <line x1={x(0)} x2={x(0)} y1={plot.y} y2={plot.bottom} stroke={theme.axis} strokeWidth={1} />
            {assets.map((a, i) => {
              const top = plot.y + i * ROW;
              const cy = top + 29;
              const worst = a.worst[1] as number;
              const best = a.best[1] as number;
              const lo = Math.max(worst, a.mean_arith - a.sd);
              const hi = Math.min(best, a.mean_arith + a.sd);
              const bestText = `${pct(best, 0)} in ${a.best[0]}`;
              const worstText = `${pct(worst, 0)} in ${a.worst[0]}`;
              return (
                <g key={a.key}>
                  <text x={plot.x} y={top + 13} fontSize={11.5} fill={theme.ink.secondary}>
                    <tspan fontWeight={500} fill={theme.ink.primary}>
                      {a.label}
                    </tspan>
                    {plot.width >= 470 ? `  ${pct(a.mean_geo)} a year compounded · lost ground in ${Math.round(a.share_negative_years * 100)} years of 100` : `  ${pct(a.mean_geo)} a year compounded`}
                  </text>
                  <line x1={x(worst)} x2={x(best)} y1={cy} y2={cy} stroke={theme.ink.tertiary} strokeWidth={2} strokeLinecap="round" />
                  <rect x={x(lo)} y={cy - 4} width={Math.max(1, x(hi) - x(lo))} height={8} rx={2} fill={theme.ink.secondary} />
                  <circle cx={x(a.mean_arith)} cy={cy} r={4.5} fill={theme.surface} stroke={theme.ink.primary} strokeWidth={2} />
                  {x(best) + 7 + textWidth(bestText, 10.5) <= plot.right && (
                    <text x={x(best) + 7} y={cy + 3.5} fontSize={10.5} fill={theme.label}>
                      {bestText}
                    </text>
                  )}
                  {x(worst) - 7 - textWidth(worstText, 10.5) >= plot.x && (
                    <text x={x(worst) - 7} y={cy + 3.5} textAnchor="end" fontSize={10.5} fill={theme.label}>
                      {worstText}
                    </text>
                  )}
                </g>
              );
            })}
          </SvgLayer>
        );
      }}
    </ChartFrame>
  );
}

export interface AgeSeries {
  key: string;
  label: string;
  /** Shorter form for the direct label at the line's end. */
  short?: string;
  color: string;
  values: (number | null)[];
  strong?: boolean;
}

/** A few series over the first person's age: lines, or stacked areas when `stacked`. Shares the view's crosshair and zoom. */
export function AgeSeriesChart({ summary, series, format, title, stacked, max, label, height = 210 }: { summary: Summary; series: AgeSeries[]; format: 'money' | 'share100'; title: string; stacked?: boolean; max?: number; label: string; height?: number }) {
  const sync = useChartSync();
  const time = useMemo(() => summaryTime(summary), [summary]);
  const range = useTimeRange(time, sync);
  const target = useMemo(() => series.map((s) => s.values.map((v) => v ?? 0)), [series]);
  const values = useAnimated(target, { resetKey: `${summary.axis.start}:${summary.axis.years.length}:${series.length}`, duration: 360 });
  const fmt = (v: number) => (format === 'money' ? moneyCompact(v) : ofHundred(v).text);

  return (
    <ChartFrame height={height} margin={{ top: 22, right: stacked ? 12 : 84, bottom: 36, left: 52 }} aria-label={label}>
      {({ plot, theme }) => {
        const x = linearScale(range.domain, [plot.x, plot.right]);
        const index = Array.from({ length: range.last - range.first + 1 }, (_, k) => range.first + k);
        const xs = index.map((i) => x(time.ages[i] as number));
        const tops = stacked ? values.reduce<number[][]>((acc, row) => [...acc, row.map((v, i) => v + (acc[acc.length - 1]?.[i] ?? 0))], []) : values;
        const peak = max ?? Math.max(1, ...index.flatMap((i) => tops.map((row) => row[i] ?? 0)));
        const y = linearScale(format === 'share100' ? [0, 1] : niceDomain(0, peak * 1.04, 4), [plot.bottom, plot.y]);
        const ticks = format === 'share100' ? [0, 0.25, 0.5, 0.75, 1] : y.ticks(4);
        const ends: EndLabel[] = stacked
          ? []
          : series.map((s, k) => ({ key: s.key, y: y(values[k]?.[index[index.length - 1] as number] ?? 0), text: s.short ?? s.label, color: s.color, strong: s.strong }));
        return (
          <>
            <CanvasLayer
              deps={[values, range.first, range.last, stacked, peak]}
              draw={(ctx) => {
                drawGrid(ctx, plot, y, ticks, theme.grid);
                const done = clipPlot(ctx, plot, 1);
                if (stacked) {
                  for (let k = series.length - 1; k >= 0; k--) {
                    const top = tops[k] as number[];
                    const below = k > 0 ? (tops[k - 1] as number[]) : null;
                    ctx.fillStyle = (series[k] as AgeSeries).color;
                    ctx.beginPath();
                    index.forEach((i, n) => (n === 0 ? ctx.moveTo(xs[n] as number, y(top[i] ?? 0)) : ctx.lineTo(xs[n] as number, y(top[i] ?? 0))));
                    for (let n = index.length - 1; n >= 0; n--) ctx.lineTo(xs[n] as number, y(below ? (below[index[n] as number] ?? 0) : 0));
                    ctx.closePath();
                    ctx.fill();
                    // The 2 px gap in the surface colour between touching fills.
                    if (below) drawLine(ctx, xs, index.map((i) => below[i] ?? 0), y, { color: theme.surface, width: 2 });
                  }
                } else {
                  series.forEach((s, k) => drawLine(ctx, xs, index.map((i) => values[k]?.[i] ?? null), y, { color: s.color, width: 2 }));
                }
                done();
              }}
            />
            <SvgLayer>
              <ValueAxis y={y} format={format} ticks={ticks} grid={false} title={title} />
              <AgeAxis x={x} time={time} />
              {ends.length > 0 && <EndLabels labels={ends} />}
            </SvgLayer>
            <Cursor
              count={index.length}
              xOf={(k) => xs[k] ?? 0}
              label={label}
              describe={(k) => {
                const i = index[k] as number;
                return `Age ${Math.floor(time.ages[i] as number)}: ${series.map((s, n) => `${s.label} ${fmt(target[n]?.[i] ?? 0)}`).join('; ')}.`;
              }}
              onBrush={(a, b) => range.set([time.ages[index[a] as number] as number, time.ages[index[b] as number] as number])}
              marks={(k) => {
                const i = index[k] as number;
                return (
                  <g>
                    {series.map((s, n) => (
                      <circle key={s.key} cx={xs[k]} cy={y((stacked ? tops[n]?.[i] : values[n]?.[i]) ?? 0)} r={3.5} fill={s.color} stroke={theme.surface} strokeWidth={2} />
                    ))}
                  </g>
                );
              }}
              tooltip={(k) => {
                const i = index[k] as number;
                const total = series.reduce((sum, _s, n) => sum + (target[n]?.[i] ?? 0), 0);
                return (
                  <>
                    <TipTitle detail={String(time.years[i])}>Age {Math.floor(time.ages[i] as number)}</TipTitle>
                    {[...series.keys()].reverse().map((n) => (
                      <TipRow key={(series[n] as AgeSeries).key} color={(series[n] as AgeSeries).color} swatch={stacked ? 'box' : 'line'} label={(series[n] as AgeSeries).label} value={fmt(target[n]?.[i] ?? 0)} strong={(series[n] as AgeSeries).strong} />
                    ))}
                    {stacked && series.length > 1 && <TipRow label="Together" value={fmt(total)} strong />}
                  </>
                );
              }}
            />
          </>
        );
      }}
    </ChartFrame>
  );
}

/** The federal bracket ladder for the household's filing status, with the traced life's taxable income marked on it. */
export function TaxLadder({ ladder, points }: { ladder: Bracket[]; points: { key: string; label: string; point: TaxPoint }[] }) {
  const placed = points.filter((p) => !p.point.foreign).map((p) => ({ ...p, place: placeOnLadder(p.point, ladder) }));
  const highest = Math.max(0, ...placed.map((p) => (p.place.bracket ? ladder.indexOf(p.place.bracket) : 0)));
  const shown = ladder.slice(0, Math.min(ladder.length, Math.max(3, highest + 2)));
  const lastShown = shown[shown.length - 1] as Bracket;
  const end = lastShown.to ?? lastShown.from * 1.3;
  const cut = end * (lastShown.to === null || shown.length === ladder.length ? 1 : 0.86);
  return (
    <ChartFrame height={46 + placed.length * 16 + 48} margin={{ top: 14 + placed.length * 16, right: 14, bottom: 40, left: 4 }} aria-label="Federal income tax brackets, with the plan's taxable income marked">
      {({ plot, theme }) => {
        const x = linearScale([0, cut], [plot.x, plot.right]);
        const barTop = plot.y + 4;
        const barH = plot.height - 4;
        const ramp = theme.sequentialCool;
        return (
          <SvgLayer>
            {shown.map((b, i) => {
              const x0 = x(b.from) + (i ? 1 : 0);
              const x1 = Math.min(plot.right, x(b.to ?? cut)) - 1;
              const fill = ramp[Math.min(ramp.length - 1, i + 2)] ?? theme.ink.tertiary;
              const text = `${Math.round(b.rate * 100)}%`;
              const open = (b.to ?? Infinity) > cut;
              return (
                <g key={b.rate}>
                  <rect x={x0} y={barTop} width={Math.max(1, x1 - x0)} height={barH} fill={fill} rx={i === 0 ? 2 : 0} />
                  {x1 - x0 >= textWidth(text, 11) + 10 && (
                    <text x={(x0 + x1) / 2} y={barTop + barH / 2 + 4} textAnchor="middle" fontSize={11} fontWeight={500} fill={inkOn(fill)}>
                      {text}
                    </text>
                  )}
                  {open && (
                    <text x={plot.right + 3} y={barTop + barH / 2 + 4} fontSize={11} fill={theme.label}>
                      →
                    </text>
                  )}
                </g>
              );
            })}
            <XAxis x={x} format="money" ticks={[0, ...shown.map((b) => b.to).filter((t): t is number => t !== null && t <= cut)]} title="Taxable income after deductions, today’s dollars (2026 thresholds)" />
            {placed.map((p, row) => {
              const px = x(p.place.x);
              const text = p.place.bracket ? `${p.label}: ${moneyCompact(p.point.taxable)} taxable, next dollar at ${Math.round(p.point.marginal * 100)}%` : `${p.label}: income is below the deductions, no federal income tax`;
              const w = textWidth(text, 11);
              const ty = 11 + row * 16;
              const left = Math.min(plot.right - w, Math.max(plot.x, px + 7));
              return (
                <g key={p.key}>
                  <line x1={px} x2={px} y1={ty - 9} y2={plot.bottom} stroke={theme.ink.primary} strokeWidth={1.5} />
                  <circle cx={px} cy={barTop + barH / 2} r={4} fill={theme.ink.primary} stroke={theme.surface} strokeWidth={2} />
                  <text x={left} y={ty} fontSize={11} fill={theme.ink.primary} stroke={theme.surface} strokeWidth={3} paintOrder="stroke">
                    {text}
                  </text>
                </g>
              );
            })}
          </SvgLayer>
        );
      }}
    </ChartFrame>
  );
}
