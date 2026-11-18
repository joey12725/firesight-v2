// Scales: plain functions from data to pixels, with the inverse and tick choices that
// suit money, shares and ages. Built on d3-scale for the arithmetic only.
import { scaleLinear, scaleSymlog } from 'd3-scale';

export interface Scale {
  (value: number): number;
  invert(px: number): number;
  domain: [number, number];
  range: [number, number];
  /** Tick values inside the domain, about `count` of them, at round numbers. */
  ticks(count?: number): number[];
  kind: 'linear' | 'symlog';
}

export function linearScale(domain: [number, number], range: [number, number]): Scale {
  const d: [number, number] = domain[0] === domain[1] ? [domain[0] - 1, domain[1] + 1] : domain;
  const s = scaleLinear().domain(d).range(range);
  const scale = ((v: number) => s(v)) as Scale;
  scale.invert = (px) => s.invert(px);
  scale.domain = d;
  scale.range = range;
  scale.ticks = (count = 5) => s.ticks(count);
  scale.kind = 'linear';
  return scale;
}

/**
 * Symmetric log: linear within `constant` of zero, logarithmic beyond, so wealth that
 * spans $10k to $10M and may cross zero fits one axis. `constant` is the dollar amount
 * around zero that gets about the same room as one later decade.
 */
export function symlogScale(domain: [number, number], range: [number, number], constant = 100_000): Scale {
  const s = scaleSymlog().domain(domain).range(range).constant(constant);
  const scale = ((v: number) => s(v)) as Scale;
  scale.invert = (px) => s.invert(px);
  scale.domain = domain;
  scale.range = range;
  scale.kind = 'symlog';
  scale.ticks = (count = 6) => {
    const [lo, hi] = domain;
    const out = new Set<number>();
    if (lo <= 0 && hi >= 0) out.add(0);
    const add = (sign: 1 | -1, limit: number) => {
      if (limit <= 0) return;
      for (let p = Math.floor(Math.log10(constant)); 10 ** p <= limit * 1.0001; p++) {
        for (const m of [1, 3]) {
          const v = m * 10 ** p;
          if (v >= constant && v <= limit) out.add(sign * v);
        }
      }
    };
    add(1, hi);
    add(-1, -lo);
    let ticks = [...out].sort((a, b) => a - b);
    // Too many: drop the 3x steps, then every other decade.
    if (ticks.length > count + 2) ticks = ticks.filter((v) => v === 0 || /^1/.test(String(Math.abs(v))));
    while (ticks.length > count + 2) ticks = ticks.filter((v, i) => v === 0 || i % 2 === ticks.length % 2);
    return ticks;
  };
  return scale;
}

/** Rounds a domain outwards to round numbers so the top gridline sits above the data. */
export function niceDomain(lo: number, hi: number, count = 5): [number, number] {
  if (!(hi > lo)) return [lo, lo + 1];
  const s = scaleLinear().domain([lo, hi]).nice(count);
  const [a, b] = s.domain();
  return [a ?? lo, b ?? hi];
}

export const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

// ---- the time axis ----------------------------------------------------------------

/**
 * The x axis every chart over time shares. Positions are the first person's age (the
 * primary reading); calendar years ride along as secondary ticks. Build it from
 * `summary.axis` (or a trace's years) and index everything by year index.
 */
export interface TimeAxis {
  /** Age of the first person at the end of each year index. */
  ages: number[];
  years: number[];
  length: number;
  /** Full extent in ages. */
  domain: [number, number];
  /** Nearest year index to an age, clamped. */
  indexAt(age: number): number;
  /** Age of the first person in a calendar year (may be fractional). */
  ageInYear(year: number): number;
  yearAtAge(age: number): number;
}

export function createTimeAxis(axis: { ages: number[]; years: number[] }): TimeAxis {
  const { ages, years } = axis;
  const first = ages[0] ?? 0;
  const last = ages[ages.length - 1] ?? first + 1;
  const offset = (years[0] ?? 0) - first;
  return {
    ages,
    years,
    length: ages.length,
    domain: [first, last],
    indexAt: (age) => clamp(Math.round(age - first), 0, ages.length - 1),
    ageInYear: (year) => year - offset,
    yearAtAge: (age) => Math.round(age + offset),
  };
}

/** Whole-age tick values for an age domain, at a step that suits the pixel width. */
export function ageTicks(domain: [number, number], width: number): number[] {
  const span = domain[1] - domain[0];
  const room = Math.max(2, Math.floor(width / 56));
  const step = [1, 2, 5, 10, 20].find((s) => span / s <= room) ?? 20;
  const out: number[] = [];
  for (let a = Math.ceil(domain[0] / step) * step; a <= domain[1] + 1e-9; a += step) out.push(a);
  return out;
}
