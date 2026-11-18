// The age work becomes optional, as a quantile dot plot: 100 dots, each an equally
// likely simulated life, stacked at the age that life reached financial independence
// (Kay et al. 2016). Lives that never get there are shown as their own group rather
// than dropped. The same data can be read as a cumulative "by this age" curve.
import { useMemo } from 'react';
import type { AgeCurve, Spread } from '@/engine/output-types';
import {
  AgeAxis,
  CanvasLayer,
  ChartFrame,
  Cursor,
  SvgLayer,
  TipNote,
  TipRow,
  TipTitle,
  ValueAxis,
  age as fmtAge,
  createTimeAxis,
  drawBand,
  drawLine,
  linearScale,
  ofHundred,
  useAnimated,
  type Frame,
  type TableData,
} from '@/ui/charts';

export type FiAgeMode = 'dots' | 'cumulative';

export interface FiAgeChartProps {
  curve: AgeCurve;
  spread: Spread;
  /** Calendar year in which the first person is `axisAge0` years old, to label years. */
  years: number[];
  ages: number[];
  mode: FiAgeMode;
  /** Lives simulated. Under 100, one dot is drawn per life instead of 100 quantile dots. */
  lives?: number;
  compact?: boolean;
  /** What the event is, for labels: "work optional". */
  noun?: string;
  height?: number;
}

interface Bin {
  /** Lower edge of the bin in years of age; NaN for the "never" group. */
  age: number;
  count: number;
}

/** Whole-age (or wider) bins of the 100 dots, with the "never" group last. */
export function binDots(dots: (number | null)[], size: number): { bins: Bin[]; never: number; min: number; max: number } {
  const counts = new Map<number, number>();
  let never = 0;
  let min = Infinity;
  let max = -Infinity;
  for (const d of dots) {
    if (d === null) {
      never += 1;
      continue;
    }
    const bin = Math.floor(d / size) * size;
    counts.set(bin, (counts.get(bin) ?? 0) + 1);
    min = Math.min(min, bin);
    max = Math.max(max, bin + size);
  }
  const bins = [...counts.entries()].sort((a, b) => a[0] - b[0]).map(([age, count]) => ({ age, count }));
  return { bins, never, min, max };
}

export function fiAgeTable(curve: AgeCurve): TableData {
  const rows = curve.ages
    .map((age, i) => ({ age, lives: curve.counts[i] ?? 0, by: curve.cumulative[i] ?? 0, lo: curve.lo[i] ?? 0, hi: curve.hi[i] ?? 0 }))
    .filter((r, i, all) => r.lives > 0 || (i > 0 && (all[i - 1]?.lives ?? 0) > 0));
  return {
    caption: 'Simulated lives by the age work becomes optional',
    columns: [
      { key: 'age', label: 'Age' },
      { key: 'lives', label: 'Lives reaching it at this age' },
      { key: 'by', label: 'Share by the end of this age', format: (v) => `${(v * 100).toFixed(1)}%` },
      { key: 'lo', label: '95% interval, low', format: (v) => `${(v * 100).toFixed(1)}%` },
      { key: 'hi', label: '95% interval, high', format: (v) => `${(v * 100).toFixed(1)}%` },
    ],
    rows: [...rows, { age: 'Never', lives: null, by: curve.never, lo: null, hi: null }],
  };
}

/** The dots to draw: the engine's 100 quantile dots, or one per life when fewer than 100 were simulated. */
export function dotsFor(dots: (number | null)[], lives: number | undefined): (number | null)[] {
  if (lives === undefined || lives >= dots.length || lives < 1) return dots;
  return Array.from({ length: lives }, (_, j) => dots[Math.min(dots.length - 1, Math.floor(((j + 0.5) / lives) * dots.length))] ?? null);
}

export function FiAgeChart({ curve, spread, years, ages, mode, lives, compact, noun = 'work optional', height }: FiAgeChartProps) {
  const time = useMemo(() => createTimeAxis({ ages, years }), [ages, years]);
  const dots = useMemo(() => dotsFor(curve.dots, lives), [curve.dots, lives]);
  const animatedCurve = useAnimated(useMemo(() => ({ cumulative: curve.cumulative, lo: curve.lo, hi: curve.hi }), [curve]), { duration: 380 });
  const h = height ?? (compact ? 176 : 248);
  const margin = { top: compact ? 8 : 14, right: 8, bottom: compact ? 40 : 64, left: mode === 'cumulative' ? 44 : compact ? 8 : 16 };

  return (
    <ChartFrame height={h} margin={margin} aria-label={`Age at which ${noun}: ${mode === 'dots' ? 'one dot for each of 100 equally likely lives' : 'share of lives by each age'}`}>
      {(frame) => (mode === 'dots' ? <Dots frame={frame} dots={dots} spread={spread} time={time} compact={compact} noun={noun} /> : <Cumulative frame={frame} curve={curve} animated={animatedCurve} spread={spread} time={time} compact={compact} noun={noun} />)}
    </ChartFrame>
  );
}

type Time = ReturnType<typeof createTimeAxis>;

/** Dots keep their rank as the result refines, and glide to their new place (no motion when reduced motion is on). */
const GLIDE = { transition: 'cx var(--duration-slow) var(--easing), cy var(--duration-slow) var(--easing)' };

function domainOf(dots: (number | null)[], time: Time): [number, number] {
  const real = dots.filter((d): d is number => d !== null);
  if (real.length === 0) return [Math.floor(time.domain[0]), Math.floor(time.domain[0]) + 10];
  return [Math.floor(Math.min(...real)) - 1, Math.ceil(Math.max(...real)) + 1];
}

/** The middle 80 of 100 lives as a bar under the axis, with the median marked. */
function IntervalBar({ frame, x, spread, y, compact }: { frame: Frame; x: (age: number) => number; spread: Spread; y: number; compact?: boolean }) {
  const { theme, plot } = frame;
  const { p10, p50, p90 } = spread;
  if (p10 === null) return null;
  const x0 = x(p10);
  const x1 = p90 === null ? plot.right : x(p90);
  const text = p50 === null ? `1 in 10 by ${fmtAge(p10)}; fewer than half ever` : p90 === null ? `Half by ${fmtAge(p50)}; 1 in 10 by ${fmtAge(p10)}; fewer than 9 in 10 ever` : `Half by ${fmtAge(p50)} · middle 80 of 100 lives: ${fmtAge(p10)} to ${fmtAge(p90)}`;
  const mid = (x0 + x1) / 2;
  const anchor = mid < plot.x + 110 ? 'start' : mid > plot.right - 110 ? 'end' : 'middle';
  return (
    <g>
      <line x1={x0} x2={x1} y1={y} y2={y} stroke={theme.ink.secondary} strokeWidth={2} strokeLinecap="round" strokeDasharray={p90 === null ? '2 4' : undefined} />
      <line x1={x0} x2={x0} y1={y - 3} y2={y + 3} stroke={theme.ink.secondary} strokeWidth={2} strokeLinecap="round" />
      {p90 !== null && <line x1={x1} x2={x1} y1={y - 3} y2={y + 3} stroke={theme.ink.secondary} strokeWidth={2} strokeLinecap="round" />}
      {p50 !== null && <circle cx={x(p50)} cy={y} r={4} fill={theme.ink.primary} stroke={theme.surface} strokeWidth={2} />}
      {!compact && (
        <text x={anchor === 'start' ? plot.x : anchor === 'end' ? plot.right : mid} y={y + 16} textAnchor={anchor} fontSize={11} fill={theme.ink.secondary}>
          {text}
        </text>
      )}
    </g>
  );
}

function Dots({ frame, dots, spread, time, compact, noun }: { frame: Frame; dots: (number | null)[]; spread: Spread; time: Time; compact?: boolean; noun: string }) {
  const { plot, theme } = frame;
  const total = dots.length;
  const [lo, hi] = domainOf(dots, time);
  const neverCount = dots.filter((d) => d === null).length;
  // The "never" group takes a slice of the width proportional to its need, after a gap.
  const neverColumns = neverCount === 0 ? 0 : Math.max(2, Math.ceil(neverCount / 10));
  const span = hi - lo;
  const perYear = (plot.width - (neverCount ? 28 : 0)) / (span + neverColumns * 1.2);
  const size = [1, 2, 5, 10].find((s) => perYear * s >= 7.5) ?? 10;
  const { bins } = binDots(dots, size);
  const tallest = Math.max(1, ...bins.map((b) => b.count), Math.min(10, neverCount));
  const diameter = Math.max(4, Math.min(perYear * size - 1.5, plot.height / tallest, compact ? 11 : 15));
  const agesWidth = plot.width - (neverCount ? 28 + neverColumns * (diameter + 1.5) : 0);
  const x = linearScale([lo, hi], [plot.x, plot.x + agesWidth]);
  const r = diameter / 2 - 0.75;
  const base = plot.bottom - 1;
  const neverX0 = plot.x + agesWidth + 28;

  const positions = [...bins.map((b) => x(b.age + size / 2)), ...(neverCount ? [neverX0 + (neverColumns * (diameter + 1.5)) / 2] : [])];
  const cumulativeBefore = (i: number) => bins.slice(0, i + 1).reduce((n, b) => n + b.count, 0);
  const label = (i: number) => {
    const b = bins[i];
    if (!b) return `Never: in ${neverCount} of ${total} lives ${noun} never happens.`;
    const range = size === 1 ? `age ${b.age}` : `ages ${b.age} to ${b.age + size - 1}`;
    return `At ${range}: ${b.count} of ${total} lives. By then, ${cumulativeBefore(i)} of ${total}.`;
  };

  // Each dot keeps its identity (its quantile rank) so it glides when the result refines.
  const circles: { key: number; cx: number; cy: number; never: boolean; bin: number }[] = [];
  const stack = new Map<number, number>();
  let neverSeen = 0;
  dots.forEach((d, i) => {
    if (d === null) {
      const col = Math.floor(neverSeen / 10);
      const row = neverSeen % 10;
      neverSeen += 1;
      circles.push({ key: i, cx: neverX0 + col * (diameter + 1.5) + diameter / 2, cy: base - diameter / 2 - row * diameter, never: true, bin: bins.length });
      return;
    }
    const bin = Math.floor(d / size) * size;
    const k = stack.get(bin) ?? 0;
    stack.set(bin, k + 1);
    circles.push({ key: i, cx: x(bin + size / 2), cy: base - diameter / 2 - k * diameter, never: false, bin: bins.findIndex((b) => b.age === bin) });
  });

  return (
    <>
      <SvgLayer>
        <AgeAxis x={x} time={time} years={!compact} />
        {neverCount > 0 && (
          <g fontSize={11}>
            <line x1={neverX0 - 4} x2={neverX0 + neverColumns * (diameter + 1.5) + 2} y1={plot.bottom + 0.5} y2={plot.bottom + 0.5} stroke={theme.axis} strokeWidth={1} />
            <text x={neverX0 + (neverColumns * (diameter + 1.5)) / 2} y={plot.bottom + 16} textAnchor="middle" fill={theme.ink.secondary}>
              Never
            </text>
          </g>
        )}
        {circles.map((c) =>
          c.never ? (
            <circle key={c.key} cx={c.cx} cy={c.cy} r={r - 0.5} fill="none" stroke={theme.ink.tertiary} strokeWidth={1.5} style={GLIDE} />
          ) : (
            <circle key={c.key} cx={c.cx} cy={c.cy} r={r} fill={theme.baseline.line} style={GLIDE} />
          ),
        )}
        <IntervalBar frame={frame} x={x} spread={spread} y={plot.bottom + (compact ? 28 : 42)} compact={compact} />
      </SvgLayer>
      <Cursor
        count={positions.length}
        xOf={(i) => positions[i] ?? 0}
        label={`Age at which ${noun}, ${total} simulated lives`}
        describe={label}
        crosshair={false}
        marks={(i) => (
          <g>
            {circles
              .filter((c) => c.bin === i)
              .map((c) => (
                <circle key={c.key} cx={c.cx} cy={c.cy} r={r + 1.5} fill="none" stroke={theme.ink.primary} strokeWidth={1.5} />
              ))}
          </g>
        )}
        tooltip={(i) => {
          const b = bins[i];
          if (!b)
            return (
              <>
                <TipTitle>Never</TipTitle>
                <TipRow label="Lives" value={`${neverCount} of ${total}`} />
                <TipNote>These lives never meet the funded test while the plan would still have them working.</TipNote>
              </>
            );
          return (
            <>
              <TipTitle detail={size === 1 ? String(time.yearAtAge(b.age)) : undefined}>{size === 1 ? `Age ${b.age}` : `Ages ${b.age}–${b.age + size - 1}`}</TipTitle>
              <TipRow color={theme.baseline.line} swatch="box" label="At this age" value={`${b.count} of ${total}`} />
              <TipRow label="By the end of it" value={`${cumulativeBefore(i)} of ${total}`} strong />
            </>
          );
        }}
      />
    </>
  );
}

function Cumulative({
  frame,
  curve,
  animated,
  spread,
  time,
  compact,
  noun,
}: {
  frame: Frame;
  curve: AgeCurve;
  animated: { cumulative: number[]; lo: number[]; hi: number[] };
  spread: Spread;
  time: Time;
  compact?: boolean;
  noun: string;
}) {
  const { plot, theme } = frame;
  const [lo, hi] = domainOf(curve.dots, time);
  const x = linearScale([lo, hi], [plot.x, plot.right]);
  const y = linearScale([0, 1], [plot.bottom, plot.y]);
  // Entry i is the share by the END of age ages[i]: plot it at ages[i] + 1.
  const first = Math.max(0, curve.ages.findIndex((a) => a + 1 >= lo));
  const lastFound = curve.ages.findIndex((a) => a + 1 > hi);
  const last = lastFound < 0 ? curve.ages.length - 1 : lastFound - 1;
  const index = Array.from({ length: Math.max(0, last - first + 1) }, (_, k) => first + k);
  const xs = index.map((i) => x((curve.ages[i] as number) + 1));
  const pick = (values: number[]) => index.map((i) => values[i] ?? null);
  const total = 1 - curve.never;

  return (
    <>
      <CanvasLayer
        deps={[animated, lo, hi]}
        draw={(ctx) => {
          drawBand(ctx, xs, pick(animated.lo), pick(animated.hi), y, theme.baseline.bands[1]);
          drawLine(ctx, xs, pick(animated.cumulative), y, { color: theme.baseline.line, width: 2 });
        }}
      />
      <SvgLayer>
        <ValueAxis y={y} format="share100" ticks={[0, 0.25, 0.5, 0.75, 1]} title={compact ? undefined : 'Lives, of 100'} />
        <AgeAxis x={x} time={time} years={!compact} />
        {curve.never > 0.005 && (
          <text x={plot.right} y={y(total) - 6} textAnchor="end" fontSize={11} fill={theme.ink.secondary}>
            {ofHundred(curve.never).text} never
          </text>
        )}
        <IntervalBar frame={frame} x={x} spread={spread} y={plot.bottom + (compact ? 28 : 42)} compact={compact} />
      </SvgLayer>
      <Cursor
        count={index.length}
        xOf={(k) => xs[k] ?? 0}
        label={`Share of lives in which ${noun} by each age`}
        describe={(k) => {
          const i = index[k] as number;
          return `By age ${(curve.ages[i] as number) + 1}: ${ofHundred(curve.cumulative[i] ?? 0).text} lives.`;
        }}
        marks={(k) => <circle cx={xs[k]} cy={y(curve.cumulative[index[k] as number] ?? 0)} r={4} fill={theme.baseline.line} stroke={theme.surface} strokeWidth={2} />}
        tooltip={(k) => {
          const i = index[k] as number;
          const a = (curve.ages[i] as number) + 1;
          return (
            <>
              <TipTitle detail={String(time.yearAtAge(a))}>By age {a}</TipTitle>
              <TipRow color={theme.baseline.line} label="Lives" value={ofHundred(curve.cumulative[i] ?? 0).text} strong />
              <TipRow label="95% interval" value={`${((curve.lo[i] ?? 0) * 100).toFixed(1)}–${((curve.hi[i] ?? 0) * 100).toFixed(1)}%`} />
            </>
          );
        }}
      />
    </>
  );
}
