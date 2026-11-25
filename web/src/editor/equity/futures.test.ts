import { describe, expect, it } from 'vitest';
import type { OutcomeRow } from '@/engine/types';
import { finishedEntry } from '@/ui/inputs';
import { addOutcome, countEntry, futuresByKind, futuresDecimals, futuresText, isNormal, normaliseTable, remainder, removeOutcome, roomFor, setFutures, toFutures } from './futures';

const row = (id: string, kind: OutcomeRow['kind'], probability: number): OutcomeRow => ({ id, kind, probability, years_from: 1, years_to: 5, multiple_low: 1, multiple_high: 2 });
const sum = (xs: number[]) => Math.round(xs.reduce((a, b) => a + b, 0) * 1e6) / 1e6;

// The seed preset: an IPO is 1.5 of 100.
const SEED = [0.6, 0.22, 0.015, 0.165];

describe('toFutures', () => {
  it('rounds whole counts by largest remainder so they add to 100', () => {
    expect(toFutures([1 / 3, 1 / 3, 1 / 3])).toEqual([34, 33, 33]);
    expect(sum(toFutures([0.3, 0.27, 0.12, 0.31]))).toBe(100);
    expect(toFutures([0.3, 0.27, 0.12, 0.31])).toEqual([30, 27, 12, 31]);
    // Whole counts of the seed preset still add to 100, but lose the half.
    expect(sum(toFutures(SEED, 0))).toBe(100);
    expect(toFutures([0.105, 0.105, 0.79])).toEqual([11, 10, 79]);
  });

  it('keeps tenths when asked, still adding to 100', () => {
    expect(toFutures(SEED, 1)).toEqual([60, 22, 1.5, 16.5]);
    const odd = [0.2743, 0.3168, 0.0412, 0.3677];
    expect(sum(toFutures(odd, 1))).toBe(100);
    expect(toFutures(odd, 1)).toEqual([27.4, 31.7, 4.1, 36.8]);
  });

  it('chooses tenths when any row is under 5 of 100', () => {
    expect(futuresDecimals(SEED)).toBe(1);
    expect(futuresDecimals([0.3, 0.27, 0.12, 0.31])).toBe(0);
    expect(futuresDecimals([0.5, 0, 0.5])).toBe(0);
    expect(futuresText(1.5, 1)).toBe('1.5');
    expect(futuresText(60, 1)).toBe('60');
    expect(futuresText(26.6, 0)).toBe('27');
  });
});

describe('setFutures', () => {
  const table = [row('fails', 'fails', 0.3), row('acquired', 'acquired', 0.27), row('ipo', 'ipo', 0.12), row('still_private', 'still_private', 0.31)];

  it('changes only the edited row and the remainder, and keeps the total at 100', () => {
    const next = setFutures(table, 'ipo', 20);
    expect(next.map((r) => r.probability)).toEqual([0.3, 0.27, 0.2, 0.23]);
    expect(next[0]).toBe(table[0]);
    expect(next[1]).toBe(table[1]);
    expect(sum(next.map((r) => r.probability))).toBe(1);
    expect(remainder(next)).toBe(23);
    // Up to the row's value plus the remainder is allowed: the remainder goes to 0.
    expect(roomFor(table, 'ipo')).toBe(43);
    expect(setFutures(table, 'ipo', 43).map((r) => r.probability)).toEqual([0.3, 0.27, 0.43, 0]);
  });

  it('returns the rows unchanged on an overshoot, a negative entry or an unknown row', () => {
    expect(setFutures(table, 'ipo', 43.5)).toBe(table);
    expect(setFutures(table, 'ipo', 95)).toBe(table);
    expect(setFutures(table, 'ipo', -1)).toBe(table);
    expect(setFutures(table, 'nope', 5)).toBe(table);
    expect(setFutures(table, 'still_private', 5)).toBe(table);
    expect(setFutures(table, 'ipo', Number.NaN)).toBe(table);
  });

  it('lets tenths survive a round trip', () => {
    const next = setFutures(table, 'ipo', 1.5);
    expect(next.find((r) => r.id === 'ipo')?.probability).toBe(0.015);
    expect(remainder(next)).toBe(41.5);
    expect(toFutures(next.map((r) => r.probability), 1)).toEqual([30, 27, 1.5, 41.5]);
    const back = setFutures(next, 'ipo', 12);
    expect(back.map((r) => r.probability)).toEqual(table.map((r) => r.probability));
  });

  it('merges several still-private rows into one, and adds one when there is none', () => {
    const several = [row('a', 'still_private', 0.2), row('fails', 'fails', 0.5), row('b', 'still_private', 0.3)];
    expect(isNormal(several)).toBe(false);
    const merged = normaliseTable(several);
    expect(merged.map((r) => [r.id, r.kind, r.probability])).toEqual([
      ['fails', 'fails', 0.5],
      ['a', 'still_private', 0.5],
    ]);
    expect(isNormal(merged)).toBe(true);
    const edited = setFutures(several, 'fails', 60);
    expect(edited.map((r) => [r.kind, r.probability])).toEqual([
      ['fails', 0.6],
      ['still_private', 0.4],
    ]);
    const none = [row('fails', 'fails', 0.5), row('still_private', 'ipo', 0.2)];
    const filled = normaliseTable(none);
    expect(filled.at(-1)).toMatchObject({ id: 'still_private_2', kind: 'still_private', probability: 0.3 });
    expect(new Set(filled.map((r) => r.id)).size).toBe(filled.length);
  });

  it('adds a row at 0 and gives a removed row back to the remainder', () => {
    const added = addOutcome(table, row('ipo_2', 'ipo', 0));
    expect(added.map((r) => r.id)).toEqual(['fails', 'acquired', 'ipo', 'ipo_2', 'still_private']);
    expect(remainder(added)).toBe(31);
    const removed = removeOutcome(table, 'acquired');
    expect(removed.map((r) => [r.id, r.probability])).toEqual([
      ['fails', 0.3],
      ['ipo', 0.12],
      ['still_private', 0.58],
    ]);
    expect(removeOutcome(table, 'still_private').map((r) => r.id)).toEqual(table.map((r) => r.id));
    expect(futuresByKind(added)).toEqual({ fails: 30, acquired: 27, ipo: 12, still_private: 31 });
  });
});

describe('a typed count', () => {
  // Series C as the editor seeds it: shuts down 30, acquired 27, public 12, still private 31.
  const table = normaliseTable([row('fails', 'fails', 0.3), row('acquired', 'acquired', 0.27), row('ipo', 'ipo', 0.12), row('still_private', 'still_private', 0.31)]);

  it('typing 70 key by key into a row with room for 61 stores nothing, least of all 7', () => {
    expect(roomFor(table, 'fails')).toBe(61);
    // The field commits a finished entry, not each keystroke: what "7" then "70" hand over is 70, once.
    const typed = ['7', '70'];
    const handed = typed.map((text) => finishedEntry(text, 'number', 30));
    expect(handed).toEqual([7, 70]);
    const entry = countEntry(table, 'fails', handed.at(-1) as number);
    // The very same table: the failure odds are still 30 of 100.
    expect(entry.rows).toBe(table);
    expect(entry.rows.find((r) => r.id === 'fails')?.probability).toBe(0.3);
    expect(entry.refused).toBe('You typed 70; only 61 fit, so this stays at 30. Lower another row first.');
    // Had the prefix been written, as it once was, the row would hold 7: the entry is never cut down to a part of itself.
    expect(setFutures(table, 'fails', 7).find((r) => r.id === 'fails')?.probability).toBe(0.07);
  });

  it('takes an entry that fits, and says nothing', () => {
    const entry = countEntry(table, 'fails', 45);
    expect(entry.refused).toBeNull();
    expect(futuresByKind(entry.rows)).toMatchObject({ fails: 45, still_private: 16 });
    expect(countEntry(table, 'fails', 61).refused).toBeNull();
    // The same value again, or a row that is not there: nothing happens.
    expect(countEntry(table, 'fails', 30)).toEqual({ rows: table, refused: null });
    expect(countEntry(table, 'nope', 10)).toEqual({ rows: table, refused: null });
  });

  it('refuses a decimal overshoot and a negative count out loud', () => {
    const full = setFutures(table, 'fails', 61);
    expect(remainder(full)).toBe(0);
    // 61.25 rounds to 61.3, over by a tenth: said, not silently put back.
    expect(countEntry(full, 'fails', 61.25).refused).toBe('You typed 61.3; only 61 fit, so this stays at 61. Lower another row first.');
    expect(countEntry(table, 'fails', -5).refused).toBe('A count cannot be below 0, so this stays at 30.');
    expect(countEntry(table, 'fails', -5).rows).toBe(table);
  });
});

describe('a field that commits when it is left', () => {
  it('hands over the finished entry and nothing for text that is not a number or changes nothing', () => {
    expect(finishedEntry('70', 'number', 30)).toBe(70);
    expect(finishedEntry('30', 'number', 30)).toBeUndefined();
    expect(finishedEntry('abc', 'number', 30)).toBeUndefined();
    expect(finishedEntry('', 'number', 30)).toBeUndefined();
    expect(finishedEntry('', 'number', 30, true)).toBeNull();
    expect(finishedEntry('', 'number', null, true)).toBeUndefined();
    expect(finishedEntry('2.5', 'years', 1)).toBe(2.5);
  });
});
