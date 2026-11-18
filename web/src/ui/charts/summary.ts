// Selectors from the engine's Summary to what charts draw. Use these rather than
// indexing `summary.quantiles[0].values[4]` by hand: series are found by key and levels
// by value, so nothing depends on array order.
import type { HistSeries, Status, Summary, YearSeries } from '@/engine/output-types';
import type { Profile } from '@/engine/types';
import { interpolate } from './animate';
import type { DensityColumn } from './marks';
import { createTimeAxis, type TimeAxis } from './scales';

/** The quantile levels every `summary.quantiles` entry carries. */
export const QUANTILE_LEVELS = [0.01, 0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95, 0.99] as const;
export type QuantileLevel = (typeof QUANTILE_LEVELS)[number];

export function summaryTime(summary: Summary): TimeAxis {
  return createTimeAxis(summary.axis);
}

/** Quantile `level` of a histogram series by year (null where nobody is alive). */
export function quantileSeries(summary: Summary, series: HistSeries, level: QuantileLevel): (number | null)[] {
  const q = summary.quantiles.find((x) => x.series === series);
  const row = q?.values[q.levels.findIndex((l) => Math.abs(l - level) < 1e-9)];
  return row ?? [];
}

/** Mean per living household of one of the 43 year series (year-end level for balances, annualised for flows). */
export function compositionSeries(summary: Summary, series: YearSeries): (number | null)[] {
  return summary.composition.find((c) => c.series === series)?.values ?? [];
}

/** Share of all lives in a status, by year. */
export function statusShares(summary: Summary, status: Status): number[] {
  return summary.status.shares[summary.status.statuses.indexOf(status)] ?? [];
}

/** Share of households with someone alive, by year (1 at the start). */
export function aliveShare(summary: Summary): number[] {
  return summary.survival.household;
}

/** The first year index at which the first person is at least `age`. */
export function indexAtAge(summary: Summary, age: number): number {
  const i = summary.axis.ages.findIndex((a) => a >= age - 1e-9);
  return i < 0 ? summary.axis.ages.length - 1 : i;
}

// ---- the density fan ----------------------------------------------------------------

/**
 * Everything the density fan draws, in a shape that can be animated: the engine's
 * histogram grid for the series (`edges`), each year's shares of the living on that
 * grid (zero outside the year's occupied range), and the nine quantile curves.
 */
export interface FanModel {
  series: HistSeries;
  edges: number[];
  /** `cells[y][k]`: share of the living in year y between edges[k] and edges[k + 1]. */
  cells: Float32Array[];
  /** Share of the living at exactly zero, by year. */
  zero: number[];
  /** Share of households still alive, by year. */
  alive: number[];
  /** `quantiles[i][y]` for QUANTILE_LEVELS[i]. */
  quantiles: (number | null)[][];
}

export function fanModel(summary: Summary, series: HistSeries): FanModel | null {
  const d = summary.density.find((x) => x.series === series);
  const q = summary.quantiles.find((x) => x.series === series);
  if (!d || !q) return null;
  const width = Math.max(0, d.edges.length - 1);
  const cells = d.counts.map((counts, y) => {
    const out = new Float32Array(width);
    const alive = summary.axis.alive[y] ?? 0;
    const start = d.starts[y] ?? 0;
    if (alive > 0) for (let k = 0; k < counts.length; k++) out[start + k] = (counts[k] ?? 0) / alive;
    return out;
  });
  return {
    series,
    edges: d.edges,
    cells,
    zero: d.zeros.map((z, y) => ((summary.axis.alive[y] ?? 0) > 0 ? z / (summary.axis.alive[y] as number) : 0)),
    alive: summary.survival.household,
    quantiles: q.values,
  };
}

/**
 * Interpolates two fan models. The grids of two summaries are windows onto the same
 * lattice of cell edges, so cells are matched by edge value; where the lattices do not
 * line up (a different series) the target is taken at once.
 */
export function interpolateFan(a: FanModel | null, b: FanModel | null, t: number): FanModel | null {
  if (!a || !b || t >= 1 || a.series !== b.series || a.cells.length !== b.cells.length) return b;
  // b.edges[k] === a.edges[k + shift]
  const first = b.edges[0] as number;
  let shift = a.edges.indexOf(first);
  if (shift < 0) {
    const back = b.edges.indexOf(a.edges[0] as number);
    if (back < 0) return b;
    shift = -back;
  }
  const cells = b.cells.map((target, y) => {
    const from = a.cells[y] as Float32Array;
    const out = new Float32Array(target.length);
    for (let k = 0; k < target.length; k++) {
      const j = k + shift;
      const v0 = j >= 0 && j < from.length ? (from[j] as number) : 0;
      out[k] = v0 + ((target[k] as number) - v0) * t;
    }
    return out;
  });
  return { series: b.series, edges: b.edges, cells, zero: interpolate(a.zero, b.zero, t), alive: interpolate(a.alive, b.alive, t), quantiles: interpolate(a.quantiles, b.quantiles, t) };
}

/** The columns `drawDensityFan` takes, for the visible years `first..last`. */
export function fanColumns(model: FanModel, first: number, last: number): DensityColumn[] {
  const out: DensityColumn[] = [];
  for (let y = first; y <= last; y++) out.push({ edges: model.edges, shares: model.cells[y] as Float32Array, zero: model.zero[y] ?? 0, weight: model.alive[y] ?? 0 });
  return out;
}

// ---- moments in the plan --------------------------------------------------------------

export interface PlanMoment {
  key: 'fi' | 'retirement' | 'social_security' | 'mortgage_paid';
  /** The first person's age. */
  age: number;
  label: string;
  detail?: string;
}

const yearFraction = (ym: string) => {
  const [y = 0, m = 1] = ym.split('-').map(Number);
  return y + (m - 1) / 12;
};

/**
 * The moments worth marking on any chart over time: when work becomes optional (median),
 * when retirement typically starts if that differs, when Social Security starts, and
 * when the mortgage is typically paid. Ages are the first person's.
 */
export function planMoments(summary: Summary, profile: Profile): PlanMoment[] {
  const out: PlanMoment[] = [];
  const h = summary.headline;
  const fi = h.median_fi_age;
  if (fi !== null) out.push({ key: 'fi', age: fi, label: 'Work optional', detail: `median ${fi.toFixed(1)}` });
  const retire = h.median_retirement_age;
  if (retire !== null && (fi === null || Math.abs(retire - fi) >= 1)) out.push({ key: 'retirement', age: retire, label: 'Retired', detail: `median ${retire.toFixed(1)}` });

  const people = profile.household.people;
  const firstBirth = people[0] ? yearFraction(people[0].birth) : null;
  if (firstBirth !== null) {
    const claims = people
      .filter((p) => p.social_security?.claim_age)
      .map((p) => ({ name: p.name, age: yearFraction(p.birth) + (p.social_security.claim_age as number) - firstBirth }))
      .sort((a, b) => a.age - b.age);
    const earliest = claims[0];
    if (earliest) {
      const together = claims.every((c) => Math.abs(c.age - earliest.age) < 1.5);
      out.push({ key: 'social_security', age: earliest.age, label: 'Social Security starts', detail: claims.length > 1 && !together && earliest.name ? `${earliest.name} first` : undefined });
    }
  }

  const mortgage = compositionSeries(summary, 'mortgage');
  const peak = Math.max(0, ...mortgage.map((v) => v ?? 0));
  if (peak > 1000) {
    const peakIndex = mortgage.findIndex((v) => (v ?? 0) >= peak);
    const paid = mortgage.findIndex((v, i) => i > peakIndex && (v ?? 0) < peak * 0.02);
    const age = summary.axis.ages[paid];
    if (paid > 0 && age !== undefined) out.push({ key: 'mortgage_paid', age, label: 'Mortgage paid', detail: 'typically' });
  }
  return out;
}
