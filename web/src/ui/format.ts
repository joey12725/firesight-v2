// Parsing and formatting of the figures people type and read. Profile values are plain
// numbers: dollars, decimal fractions for rates, years.
import { format } from 'd3-format';

export type NumberKind = 'money' | 'percent' | 'years' | 'months' | 'number' | 'integer';

const SUFFIX: Record<string, number> = { k: 1e3, m: 1e6, mm: 1e6, b: 1e9, bn: 1e9 };

/**
 * Reads what a person typed. Returns null when it is not a number.
 *   money:   "85k" -> 85000, "$1.2m" -> 1200000, "1,250.50" -> 1250.5
 *   percent: "4%" or "4" -> 0.04 (always entered in percent points)
 *   years:   "30", "30y", "18mo" -> 1.5
 *   months:  "18", "2y" -> 24
 */
export function parseNumber(text: string, kind: NumberKind): number | null {
  const t = text.trim().toLowerCase().replace(/[\s,_$]/g, '');
  if (t === '') return null;
  const m = /^([+-]?(?:\d+\.?\d*|\.\d+))([a-z%]*)$/.exec(t.replace(/^\((.*)\)$/, '-$1'));
  if (!m) return null;
  const n = Number(m[1]);
  const unit = m[2] ?? '';
  if (!Number.isFinite(n)) return null;
  switch (kind) {
    case 'money':
    case 'number': {
      if (unit === '') return n;
      const scale = SUFFIX[unit];
      return scale === undefined ? null : n * scale;
    }
    case 'integer':
      return unit === '' ? Math.round(n) : SUFFIX[unit] === undefined ? null : Math.round(n * (SUFFIX[unit] as number));
    case 'percent':
      return unit === '' || unit === '%' || unit === 'pct' ? round(n / 100, 8) : null;
    case 'years':
      if (unit === '' || /^(y|yr|yrs|year|years)$/.test(unit)) return n;
      if (/^(mo|mos|month|months)$/.test(unit)) return round(n / 12, 4);
      return null;
    case 'months':
      if (unit === '' || /^(mo|mos|month|months)$/.test(unit)) return Math.round(n);
      if (/^(y|yr|yrs|year|years)$/.test(unit)) return Math.round(n * 12);
      return null;
  }
}

function round(n: number, digits: number): number {
  const f = 10 ** digits;
  return Math.round(n * f) / f;
}

const grouped = format(',.0f');
const grouped2 = format(',.2f');

/** The text shown inside an input (no currency sign or unit; adornments carry those). */
export function formatForInput(value: number, kind: NumberKind): string {
  switch (kind) {
    case 'money':
      return Number.isInteger(value) || Math.abs(value) >= 1000 ? grouped(value) : grouped2(value);
    case 'percent':
      return String(round(value * 100, 4));
    case 'integer':
    case 'months':
      return String(Math.round(value));
    case 'years':
    case 'number':
      return String(round(value, 4));
  }
}

/** "$85,000". */
export function money(value: number): string {
  const sign = value < 0 ? '−' : '';
  return `${sign}$${grouped(Math.abs(value))}`;
}

/** "$85k", "$1.2M", "$950": for summaries and axis labels. */
export function moneyCompact(value: number): string {
  const sign = value < 0 ? '−' : '';
  const a = Math.abs(value);
  if (a >= 1e9) return `${sign}$${trim(a / 1e9, a >= 1e10 ? 0 : 1)}B`;
  if (a >= 1e6) return `${sign}$${trim(a / 1e6, a >= 1e7 ? 1 : 2)}M`;
  if (a >= 1e4) return `${sign}$${trim(a / 1e3, 0)}k`;
  if (a >= 1e3) return `${sign}$${trim(a / 1e3, 1)}k`;
  return `${sign}$${grouped(a)}`;
}

function trim(n: number, digits: number): string {
  return String(round(n, digits));
}

/** 0.0425 -> "4.25%". */
export function percent(value: number, digits = 2): string {
  return `${round(value * 100, digits)}%`;
}

/** 12345 -> "12,345". */
export function count(value: number): string {
  return grouped(value);
}

/** 250000 -> "250k". */
export function countCompact(value: number): string {
  if (value >= 1e6) return `${trim(value / 1e6, 1)}M`;
  if (value >= 1e3) return `${trim(value / 1e3, value >= 1e4 ? 0 : 1)}k`;
  return String(Math.round(value));
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** "2031-09" -> "Sep 2031". */
export function yearMonth(ym: string): string {
  const [y, m] = ym.split('-');
  return `${MONTHS[Number(m) - 1] ?? ''} ${y}`.trim();
}

/** Whole years between two "YYYY-MM" strings. */
export function yearsBetween(from: string, to: string): number {
  const [fy = 0, fm = 1] = from.split('-').map(Number);
  const [ty = 0, tm = 1] = to.split('-').map(Number);
  return Math.floor(((ty - fy) * 12 + (tm - fm)) / 12);
}

export function currentYearMonth(now = new Date()): string {
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}`;
}
