// The difference the decision makes, life by life: a quantile dot plot of scenario
// minus plan for one per-life quantity. Under common random numbers each dot is the
// same simulated life lived twice, so the spread here is the decision meeting different
// futures, not market luck. Lives the decision leaves untouched are counted apart, at
// zero, as hollow dots.
import { useMemo } from 'react';
import type { ScalarComparison } from '@/engine/output-types';
import { moneyCompact } from '@/ui';
import { ChartFrame, Cursor, SvgLayer, TipNote, TipRow, TipTitle, textWidth, type Frame, type TableData } from '@/ui/charts';
import { ValueAxisX } from '@/ui/charts/ext/scenarios-marks';
import { binDeltas, deltaQuantile, deltaTolerance, type DeltaBin } from './model';
import { scalarWords } from './text';

const GLIDE = { transition: 'cx var(--duration-slow) var(--easing), cy var(--duration-slow) var(--easing)' };

/** A difference in the scalar's unit, signed, for ticks and tooltips. */
export function scalarDelta(v: number, s: Pick<ScalarComparison, 'unit'>, sign = true): string {
  const prefix = !sign || v === 0 ? '' : v > 0 ? '+' : '−';
  const a = Math.abs(v);
  switch (s.unit) {
    case 'dollars':
      return `${prefix}${moneyCompact(a)}`;
    case 'age':
    case 'years':
      return a === 0 ? '0' : a < 1 ? `${prefix}${Math.round(a * 12)} mo` : `${prefix}${Math.round(a * 10) / 10} yr`;
    case 'months':
      return `${prefix}${Math.round(a)} mo`;
    case 'rate':
    case 'ratio':
      return `${prefix}${Math.round(a * 1000) / 10}%`;
    default:
      return `${prefix}${a}`;
  }
}

export function deltaTable(s: ScalarComparison, quantileLevels: readonly number[]): TableData {
  const levels = quantileLevels.map((l) => Math.round(l * 100));
  const fmt = (v: number) => scalarDelta(v, s);
  return {
    caption: `Per-life difference in ${scalarWords(s.scalar).noun}: scenario minus plan, over lives with a value under both`,
    columns: [
      { key: 'what', label: 'Statistic', align: 'left' },
      { key: 'value', label: 'Scenario minus plan', format: fmt },
    ],
    rows: [
      { what: 'Lives paired', value: s.both },
      { what: 'Average difference', value: s.mean_delta.value },
      { what: '95% interval of the average, low', value: s.mean_delta.lo },
      { what: '95% interval of the average, high', value: s.mean_delta.hi },
      ...levels.map((l, i) => ({ what: `${l}th percentile of the difference`, value: s.quantiles[i] ?? null })),
      { what: 'Share of lives higher with the scenario', value: s.b_higher === null ? null : `${(s.b_higher * 100).toFixed(1)}%` },
      { what: 'Share of lives lower with the scenario', value: s.a_higher === null ? null : `${(s.a_higher * 100).toFixed(1)}%` },
      { what: 'Share of lives unchanged', value: s.equal === null ? null : `${(s.equal * 100).toFixed(1)}%` },
      { what: 'Lives with a value under the plan only', value: s.a_only },
      { what: 'Lives with a value under the scenario only', value: s.b_only },
    ],
  };
}

export function DeltaDots({ scalar, levels, compact }: { scalar: ScalarComparison; levels: readonly number[]; compact?: boolean }) {
  const words = scalarWords(scalar.scalar);
  const height = compact ? 150 : 236;
  const margin = { top: compact ? 8 : 20, right: 10, bottom: compact ? 40 : 76, left: 10 };
  return (
    <ChartFrame height={height} margin={margin} aria-label={`Per-life difference in ${words.noun}: one dot for each of 100 equally likely lives, scenario minus plan`}>
      {(frame) => <Plot frame={frame} scalar={scalar} levels={levels} compact={compact} />}
    </ChartFrame>
  );
}

function Plot({ frame, scalar, levels, compact }: { frame: Frame; scalar: ScalarComparison; levels: readonly number[]; compact?: boolean }) {
  const { plot, theme } = frame;
  const words = scalarWords(scalar.scalar);
  const tolerance = deltaTolerance(scalar);
  const columns = Math.max(8, Math.min(26, Math.floor(plot.width / 15)));
  const model = useMemo(() => binDeltas(scalar.dots, columns, tolerance, scalar.equal), [scalar.dots, columns, tolerance, scalar.equal]);
  const { bins, unchanged, total, domain, step } = model;

  if (total === 0) {
    return (
      <SvgLayer>
        <text x={plot.x + plot.width / 2} y={plot.y + plot.height / 2} textAnchor="middle" fontSize={12} fill={theme.ink.secondary}>
          No life has a value under both plans.
        </text>
      </SvgLayer>
    );
  }

  // Layout: the unchanged lives stand in a block at zero; changed lives are binned on a
  // linear axis either side of it. Two passes settle the dot size against the block width.
  const tallest = Math.max(1, ...bins.map((b) => b.count));
  const span = domain[1] - domain[0];
  const binCount = Math.round(span / step);
  let diameter = compact ? 9 : 12;
  let blockColumns = 0;
  let blockWidth = 0;
  let rows = 1;
  for (let pass = 0; pass < 3; pass++) {
    rows = Math.max(1, Math.floor(plot.height / diameter));
    blockColumns = unchanged > 0 ? Math.ceil(unchanged / rows) : 0;
    blockWidth = blockColumns > 0 ? blockColumns * (diameter + 1.5) + 16 : 0;
    const perBin = (plot.width - blockWidth) / Math.max(1, binCount);
    diameter = Math.max(3.5, Math.min(perBin - 1.5, plot.height / tallest, compact ? 10 : 14));
  }
  const usable = plot.width - blockWidth;
  const leftWidth = usable * (-domain[0] / span);
  const zeroLeft = plot.x + leftWidth;
  const zeroRight = zeroLeft + blockWidth;
  const zeroCentre = (zeroLeft + zeroRight) / 2;
  const x = (v: number) => {
    const c = Math.min(domain[1], Math.max(domain[0], v));
    if (Math.abs(v) <= tolerance) return zeroCentre;
        return c < 0 ? plot.x + ((c - domain[0]) / -domain[0]) * leftWidth : zeroRight + (c / domain[1]) * (usable - leftWidth);
  };
  const r = diameter / 2 - 0.75;
  const base = plot.bottom - 1;

  const circles: { key: string; cx: number; cy: number; bin: number }[] = [];
  bins.forEach((b, i) => {
    for (let k = 0; k < b.count; k++) circles.push({ key: `${b.lo}:${k}`, cx: x(b.mid), cy: base - diameter / 2 - k * diameter, bin: i });
  });
  const blockX0 = zeroCentre - (blockColumns * (diameter + 1.5)) / 2;
  const unchangedCircles = Array.from({ length: unchanged }, (_, k) => ({ key: `u${k}`, cx: blockX0 + Math.floor(k / rows) * (diameter + 1.5) + (diameter + 1.5) / 2, cy: base - diameter / 2 - (k % rows) * diameter }));

  const fmt = (v: number) => scalarDelta(v, scalar);
  // Ticks at bin edges, thinned to what fits; zero is the block.
  const every = Math.max(1, Math.ceil((textWidth(fmt(domain[1]), 11) + 14) / Math.max(1, usable / binCount)));
  const ticks: number[] = [];
  for (let k = Math.round(domain[0] / step); k <= Math.round(domain[1] / step); k++) if (k !== 0 && k % every === 0) ticks.push(k * step);
  const tickX = (t: number) => (t < 0 ? plot.x + ((t - domain[0]) / -domain[0]) * leftWidth : zeroRight + (t / domain[1]) * (usable - leftWidth));

  const positions = [...bins.map((b) => ({ x: x(b.mid), bin: b as DeltaBin | null })), ...(unchanged > 0 ? [{ x: zeroCentre, bin: null }] : [])].sort((a, b) => a.x - b.x);
  const rangeText = (b: DeltaBin) => (b.open === 'below' ? `${fmt(b.hi)} or lower` : b.open === 'above' ? `${fmt(b.lo)} or higher` : `${fmt(b.lo)} to ${fmt(b.hi)}`);
  const lives = (n: number) => `${n} of ${total}`;

  // Under the axis: the middle 80 of 100 lives and the median, like the Overview's FI chart.
  const [p10, p50, p90] = [deltaQuantile(levels, scalar, 0.1), deltaQuantile(levels, scalar, 0.5), deltaQuantile(levels, scalar, 0.9)];
  const barY = plot.bottom + (compact ? 30 : 46);
  const summaryText =
    p50 === null || p10 === null || p90 === null
      ? ''
      : `Middle life: ${Math.abs(p50) <= tolerance ? 'no change' : fmt(p50)} · middle 80 of 100: ${Math.abs(p10) <= tolerance ? 'no change' : fmt(p10)} to ${Math.abs(p90) <= tolerance ? 'no change' : fmt(p90)}`;

  return (
    <>
      <SvgLayer>
        <ValueAxisX x={tickX} ticks={ticks} format={fmt} grid={false} />
        <g fontSize={11}>
          {unchanged > 0 ? (
            <>
              <text x={zeroCentre} y={plot.bottom + 16} textAnchor="middle" fill={theme.ink.secondary}>
                No change
              </text>
              <text x={zeroCentre} y={Math.max(plot.y - 6, base - Math.min(rows, unchanged) * diameter - 6)} textAnchor="middle" fill={theme.ink.primary} fontWeight={500}>
                {unchanged}
              </text>
            </>
          ) : (
            <>
              <line x1={Math.round(zeroCentre) + 0.5} x2={Math.round(zeroCentre) + 0.5} y1={plot.y} y2={plot.bottom + 4} stroke={theme.annotation} strokeWidth={1} />
              <text x={zeroCentre} y={plot.bottom + 16} textAnchor="middle" fill={theme.ink.secondary}>
                No change
              </text>
            </>
          )}
          {!compact && (
            <>
              <text x={plot.x} y={plot.y - 6} fill={theme.ink.secondary}>
                ← {upper(words.less)} with the change
              </text>
              <text x={plot.right} y={plot.y - 6} textAnchor="end" fill={theme.ink.secondary}>
                {upper(words.more)} with the change →
              </text>
            </>
          )}
        </g>
        {unchangedCircles.map((c) => (
          <circle key={c.key} cx={c.cx} cy={c.cy} r={Math.max(1, r - 0.5)} fill="none" stroke={theme.ink.tertiary} strokeWidth={1.5} />
        ))}
        {circles.map((c) => (
          <circle key={c.key} cx={c.cx} cy={c.cy} r={r} fill={theme.scenario.line} style={GLIDE} />
        ))}
        {p10 !== null && p50 !== null && p90 !== null && (
          <g>
            <line x1={x(p10)} x2={x(p90)} y1={barY} y2={barY} stroke={theme.ink.secondary} strokeWidth={2} strokeLinecap="round" />
            <line x1={x(p10)} x2={x(p10)} y1={barY - 3} y2={barY + 3} stroke={theme.ink.secondary} strokeWidth={2} strokeLinecap="round" />
            <line x1={x(p90)} x2={x(p90)} y1={barY - 3} y2={barY + 3} stroke={theme.ink.secondary} strokeWidth={2} strokeLinecap="round" />
            <circle cx={x(p50)} cy={barY} r={4} fill={theme.ink.primary} stroke={theme.surface} strokeWidth={2} />
            {!compact && (
              <text x={(plot.x + plot.right) / 2} y={barY + 18} textAnchor="middle" fontSize={11} fill={theme.ink.secondary}>
                {summaryText}
              </text>
            )}
          </g>
        )}
      </SvgLayer>
      <Cursor
        count={positions.length}
        xOf={(i) => positions[i]?.x ?? 0}
        label={`Per-life difference in ${words.noun}, ${total} equally likely lives`}
        crosshair={false}
        describe={(i) => {
          const p = positions[i];
          if (!p) return '';
          return p.bin ? `${rangeText(p.bin)}: ${lives(p.bin.count)} lives.` : `No change: ${lives(unchanged)} lives.`;
        }}
        marks={(i) => {
          const p = positions[i];
          if (!p) return null;
          const set = p.bin ? circles.filter((c) => bins[c.bin] === p.bin) : unchangedCircles;
          return (
            <g>
              {set.map((c) => (
                <circle key={c.key} cx={c.cx} cy={c.cy} r={r + 1.5} fill="none" stroke={theme.ink.primary} strokeWidth={1.5} />
              ))}
            </g>
          );
        }}
        tooltip={(i) => {
          const p = positions[i];
          if (!p) return null;
          if (!p.bin)
            return (
              <>
                <TipTitle>No change</TipTitle>
                <TipRow label="Lives" value={lives(unchanged)} strong />
                <TipNote>In these lives the decision leaves {words.noun} where it was.</TipNote>
              </>
            );
          return (
            <>
              <TipTitle>{rangeText(p.bin)}</TipTitle>
              <TipRow color={theme.scenario.line} swatch="box" label="Lives" value={lives(p.bin.count)} strong />
              <TipNote>
                {p.bin.mid > 0 ? upper(words.more) : upper(words.less)} with the change{p.bin.open ? '; this column gathers every life beyond the axis' : ''}.
              </TipNote>
            </>
          );
        }}
      />
    </>
  );
}

const upper = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
