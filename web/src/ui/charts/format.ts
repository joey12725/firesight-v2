// Number formatting for axes, labels and tooltips. One style per axis: `moneyTicks`
// returns a formatter chosen for the whole tick set, so an axis never mixes "$950k" with
// "$1.25M". Today's dollars is stated once per chart (the axis title), never per tick.
import { count as groupedCount, money, moneyCompact } from '../format';

export type ValueFormat = 'money' | 'percent' | 'share100' | 'age' | 'count' | 'ratio';

/** A formatter for a set of money ticks, compact and consistent across the axis. */
export function moneyTicks(ticks: number[]): (v: number) => string {
  const max = Math.max(...ticks.map(Math.abs), 0);
  const step = ticks.length > 1 ? Math.min(...ticks.slice(1).map((t, i) => Math.abs(t - (ticks[i] ?? 0))).filter((d) => d > 0)) : max;
  const [unit, suffix] = max >= 1e9 ? [1e9, 'B'] : max >= 1e6 ? [1e6, 'M'] : max >= 1e3 ? [1e3, 'k'] : [1, ''];
  // A symlog axis spans units (10k ... 10M): fall back to per-tick compact form.
  const mixed = ticks.some((t) => t !== 0 && Math.abs(t) < unit);
  if (mixed) return (v) => (v === 0 ? '$0' : moneyCompact(v));
  const digits = step >= unit ? 0 : step >= unit / 10 ? 1 : 2;
  return (v) => {
    if (v === 0) return '$0';
    const sign = v < 0 ? '−' : '';
    return `${sign}$${(Math.abs(v) / unit).toFixed(digits)}${suffix}`;
  };
}

/** 0.83 -> "83%". */
export function percentTick(v: number, digits = 0): string {
  return `${(v * 100).toFixed(digits)}%`;
}

/** Whole age for ticks ("65"), one decimal in prose only when it is not whole ("51.8"). */
export function age(v: number, digits = 1): string {
  const r = Math.round(v * 10 ** digits) / 10 ** digits;
  return Number.isInteger(r) ? String(r) : r.toFixed(digits);
}

/**
 * A share as a natural frequency: 0.8333 -> "83 of 100". Shares under 1 in 100 (or
 * over 99) switch to "of 1,000" so a rare outcome is not rounded to never.
 */
export function ofHundred(share: number): { n: string; of: string; text: string } {
  const s = Math.min(1, Math.max(0, share));
  const per100 = Math.round(s * 100);
  if (s === 0 || s === 1 || (per100 >= 1 && per100 <= 99)) return { n: String(per100), of: '100', text: `${per100} of 100` };
  const per1000 = Math.round(s * 1000);
  if (per1000 >= 1 && per1000 <= 999) return { n: String(per1000), of: '1,000', text: `${per1000} of 1,000` };
  return s < 0.5 ? { n: 'fewer than 1', of: '1,000', text: 'fewer than 1 of 1,000' } : { n: 'more than 999', of: '1,000', text: 'more than 999 of 1,000' };
}

export function formatValue(v: number | null | undefined, format: ValueFormat): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return 'n/a';
  switch (format) {
    case 'money':
      return Math.abs(v) >= 1e6 ? moneyCompact(v) : money(Math.round(v / 100) * 100);
    case 'percent':
      return percentTick(v, Math.abs(v) < 0.1 && v !== 0 ? 1 : 0);
    case 'share100':
      return ofHundred(v).text;
    case 'age':
      return age(v);
    case 'count':
      return groupedCount(v);
    case 'ratio':
      return v.toFixed(2);
  }
}

/** Plain value for a CSV cell (no symbols, no grouping). */
export function csvValue(v: unknown): string {
  if (v === null || v === undefined) return '';
  if (typeof v === 'number') return Number.isFinite(v) ? String(Math.round(v * 1e6) / 1e6) : '';
  const s = String(v as string);
  return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}
