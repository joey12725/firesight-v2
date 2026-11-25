// Outcome odds as counts out of 100 futures. The profile stores probabilities; people
// read and type "27 of 100". A custom table always has one "still private" row that
// holds whatever the dated rows leave, so an edit changes one row and that remainder
// and nothing else.
import type { OutcomeRow } from '@/engine/types';
import { uniqueId } from '@/state/pointer';

/** Decimals for a set of counts: tenths when any row is under 5 of 100 (a seed IPO is 1.5, not 1 or 2). */
export function futuresDecimals(probabilities: number[]): 0 | 1 {
  return probabilities.some((p) => p > 0 && p < 0.05) ? 1 : 0;
}

/**
 * Counts out of 100 for a set of probabilities, rounded by largest remainder so that
 * the counts add up to 100 (or to what the probabilities add up to, when that is not 1).
 */
export function toFutures(probabilities: number[], decimals: 0 | 1 = 0): number[] {
  const scale = decimals === 1 ? 10 : 1;
  const exact = probabilities.map((p) => Math.max(0, p) * 100 * scale);
  const floors = exact.map((v) => Math.floor(v + 1e-9));
  let left = Math.round(exact.reduce((a, b) => a + b, 0)) - floors.reduce((a, b) => a + b, 0);
  const order = exact.map((v, i) => ({ i, rest: v - (floors[i] ?? 0) })).sort((a, b) => b.rest - a.rest || a.i - b.i);
  for (const { i } of order) {
    if (left <= 0) break;
    floors[i] = (floors[i] ?? 0) + 1;
    left -= 1;
  }
  return floors.map((v) => v / scale);
}

/** "27" or "1.5": a count with the table's decimals. */
export function futuresText(n: number, decimals: 0 | 1 = 0): string {
  return decimals === 1 ? (Math.round(n * 10) / 10).toFixed(1).replace(/\.0$/, '') : String(Math.round(n));
}

const clean = (p: number) => Math.round(p * 1e9) / 1e9;

/** The dated rows of a table (everything except "still private"), in order. */
export const datedRows = (rows: OutcomeRow[]): OutcomeRow[] => rows.filter((r) => r.kind !== 'still_private');

/** Futures out of 100 that the dated rows leave: the "still private" count. Not below 0. */
export function remainder(rows: OutcomeRow[]): number {
  return Math.max(0, clean(100 - datedRows(rows).reduce((n, r) => n + r.probability * 100, 0)));
}

/**
 * The table in the form the editor writes: the dated rows in their order, then exactly
 * one "still private" row holding the remainder. A table that arrives without that row,
 * or with several, comes out with one.
 */
export function normaliseTable(rows: OutcomeRow[]): OutcomeRow[] {
  const dated = datedRows(rows);
  const existing = rows.find((r) => r.kind === 'still_private');
  const id = existing?.id || uniqueId('still_private', dated.map((r) => r.id));
  const last: OutcomeRow = { id, kind: 'still_private', probability: clean(remainder(rows) / 100), years_from: existing?.years_from ?? 10, years_to: existing?.years_to ?? 10, multiple_low: 1, multiple_high: 1 };
  return [...dated, last];
}

/** True when the table already has the editor's form (one "still private" row, last). */
export function isNormal(rows: OutcomeRow[]): boolean {
  return rows.filter((r) => r.kind === 'still_private').length === 1 && rows.at(-1)?.kind === 'still_private';
}

/** The most futures row `id` may take: what it has now plus the remainder. */
export function roomFor(rows: OutcomeRow[], id: string): number {
  const row = datedRows(rows).find((r) => r.id === id);
  return row ? clean(row.probability * 100 + remainder(rows)) : 0;
}

/**
 * Sets one dated row to `n` futures of 100. Only that row and the remainder change. An
 * entry the remainder cannot cover (or a negative one, or an unknown row) returns the
 * very same `rows`. That alone does not protect a value while it is typed: "7" on the
 * way to "70" fits and would be written. The editor therefore hands this only finished
 * entries (`countEntry`, from a field that commits when it is left).
 */
export function setFutures(rows: OutcomeRow[], id: string, n: number): OutcomeRow[] {
  if (!Number.isFinite(n) || n < 0 || n > roomFor(rows, id) + 1e-9) return rows;
  if (!datedRows(rows).some((r) => r.id === id)) return rows;
  return normaliseTable(rows.map((r) => (r.id === id && r.kind !== 'still_private' ? { ...r, probability: clean(n / 100) } : r)));
}

const tenth = (v: number) => Math.round(v * 10) / 10;

export interface CountEntry {
  /** The table after the entry: the very same `rows` when nothing was written. */
  rows: OutcomeRow[];
  /** Why the entry was refused, said to the person who typed it. Null when it was taken (or changed nothing). */
  refused: string | null;
}

/**
 * A finished entry in a row's "futures of 100" field. One that fits is written; one the
 * remainder cannot cover is refused whole, with a message that says what was typed, how
 * much fits and what the row keeps. It is never cut down to a part of what was typed.
 */
export function countEntry(rows: OutcomeRow[], id: string, typed: number): CountEntry {
  const row = datedRows(rows).find((r) => r.id === id);
  if (!row || !Number.isFinite(typed)) return { rows, refused: null };
  const n = tenth(typed);
  const has = tenth(row.probability * 100);
  if (n === has) return { rows, refused: null };
  const next = setFutures(rows, id, n);
  if (next !== rows) return { rows: next, refused: null };
  const stays = `so this stays at ${futuresText(has, 1)}`;
  if (n < 0) return { rows, refused: `A count cannot be below 0, ${stays}.` };
  return { rows, refused: `You typed ${futuresText(n, 1)}; only ${futuresText(tenth(roomFor(rows, id)), 1)} fit, ${stays}. Lower another row first.` };
}

/** Removes a dated row; its futures go back to the remainder. */
export function removeOutcome(rows: OutcomeRow[], id: string): OutcomeRow[] {
  return normaliseTable(rows.filter((r) => r.id !== id || r.kind === 'still_private'));
}

/** Adds a dated row before the remainder row. */
export function addOutcome(rows: OutcomeRow[], row: OutcomeRow): OutcomeRow[] {
  return normaliseTable([...datedRows(rows), row, ...rows.filter((r) => r.kind === 'still_private')]);
}

/** Futures of 100 per outcome kind, summed over the rows of that kind. */
export function futuresByKind(rows: OutcomeRow[]): Record<OutcomeRow['kind'], number> {
  const out = { fails: 0, acquired: 0, ipo: 0, still_private: 0 };
  for (const r of rows) out[r.kind] += r.probability * 100;
  return out;
}
