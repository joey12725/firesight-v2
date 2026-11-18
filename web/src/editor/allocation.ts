// Arithmetic for the allocation editor. Weights are kept as whole percents that always
// sum to exactly 100.
import type { Allocation, AssetClass } from '@/engine/types';
import { ASSETS } from './labels';

export type Weights = { key: AssetClass; pct: number }[];

/** Rounds to whole percents summing to 100 (largest remainder), in the canonical asset order. */
export function toPercents(allocation: Allocation): Weights {
  const entries = ASSETS.map((a) => ({ key: a.value, raw: Math.max(0, allocation[a.value] ?? 0) })).filter((e) => e.raw > 0);
  const total = entries.reduce((n, e) => n + e.raw, 0);
  if (total <= 0) return [];
  return apportion(entries, 100);
}

export function toAllocation(weights: Weights): Allocation {
  const out: Allocation = {};
  for (const w of weights) if (w.pct > 0) out[w.key] = w.pct;
  return out;
}

/** Splits `total` whole units across `raw` weights in proportion (largest remainder). */
function apportion(raw: { key: AssetClass; raw: number }[], total: number): Weights {
  const sum = raw.reduce((n, e) => n + e.raw, 0);
  if (sum <= 0 || total <= 0) return raw.map((e) => ({ key: e.key, pct: 0 }));
  const parts = raw.map((e) => {
    const x = (e.raw / sum) * total;
    return { key: e.key, pct: Math.floor(x), frac: x - Math.floor(x) };
  });
  let left = total - parts.reduce((n, e) => n + e.pct, 0);
  for (const e of [...parts].sort((x, y) => y.frac - x.frac)) {
    if (left <= 0) break;
    e.pct += 1;
    left -= 1;
  }
  return parts.map(({ key, pct }) => ({ key, pct }));
}

/**
 * Sets one asset to `pct` and scales the others proportionally so the total stays 100.
 * If the others are all zero, the remainder goes to `fallback`.
 */
export function setWeight(weights: Weights, key: AssetClass, pct: number, fallback: AssetClass = 'bonds'): Weights {
  const target = Math.max(0, Math.min(100, Math.round(pct)));
  let others = weights.filter((w) => w.key !== key && w.pct > 0).map((w) => ({ key: w.key, raw: w.pct }));
  if (others.length === 0 && target < 100) others = [{ key: fallback === key ? 'us_stocks' : fallback, raw: 1 }];
  const result = [...apportion(others, 100 - target).filter((w) => w.pct > 0), { key, pct: target }];
  return ASSETS.flatMap((a) => result.filter((w) => w.key === a.value));
}

/** Moves `delta` percent from the asset after a boundary to the one before it (or back). */
export function shiftBoundary(weights: Weights, index: number, delta: number): Weights {
  const a = weights[index];
  const b = weights[index + 1];
  if (!a || !b) return weights;
  const d = Math.max(-a.pct, Math.min(b.pct, Math.round(delta)));
  return weights.map((w, i) => (i === index ? { ...w, pct: w.pct + d } : i === index + 1 ? { ...w, pct: w.pct - d } : w));
}

/** Share of growth assets (everything but bonds and cash). */
export function equityShare(weights: Weights): number {
  return weights.filter((w) => w.key !== 'bonds' && w.key !== 'cash').reduce((n, w) => n + w.pct, 0);
}
