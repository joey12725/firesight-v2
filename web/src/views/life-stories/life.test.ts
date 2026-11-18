// The Life stories transforms, checked against real traces from the engine for the three
// demo households, plus the lane layout used for the event labels.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import type { PathTrace, Summary } from '@/engine/output-types';
import * as wasm from '@/engine/pkg/firesight_wasm.js';
import type { ExampleProfile, Profile } from '@/engine/types';
import { layoutLanes } from '@/ui/charts/ext/cashflow-lanes';
import { crowdFor, yearEnds } from './Timeline';
import { buildLife, fallAt, headline, lifeChoices, narrative, taxReturn, yearLedger, type Life } from './life';
import { accountTakeaway, flowTakeaway, marketTakeaway, timelineTakeaway } from './words';

interface Fixture {
  profile: Profile;
  summary: Summary;
  traces: PathTrace[];
}
const fixtures = new Map<string, Fixture>();

beforeAll(() => {
  wasm.initSync({ module: readFileSync(new URL('../../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)) });
  for (const example of JSON.parse(wasm.exampleProfiles()) as ExampleProfile[]) {
    const json = JSON.stringify(example.profile);
    const engine = new wasm.Engine(json);
    try {
      const summary = JSON.parse(wasm.summarize(engine.runChunk(0, 300), json)) as Summary;
      const traces = summary.picks.map((p) => JSON.parse(engine.trace(p.path_index)) as PathTrace);
      fixtures.set(example.slug, { profile: example.profile as Profile, summary, traces });
    } finally {
      engine.free();
    }
  }
  // The engine is slow to warm up when the machine is busy.
}, 120_000);

const SLUGS = ['dual-income-family', 'early-career-saver', 'side-hustle-landlord'];
const lives = (slug: string): { life: Life; summary: Summary }[] => {
  const f = fixtures.get(slug) as Fixture;
  return f.traces.map((trace) => ({ life: buildLife(trace, f.profile), summary: f.summary }));
};

describe('buildLife on real traces', () => {
  for (const slug of SLUGS) {
    describe(slug, () => {
      it('puts the life on the same age axis as the summary', () => {
        for (const { life, summary } of lives(slug)) {
          expect(life.hasAges).toBe(true);
          life.time.years.forEach((year, y) => {
            const i = summary.axis.years.indexOf(year);
            expect(i).toBeGreaterThanOrEqual(0);
            expect(life.time.ages[y]).toBeCloseTo(summary.axis.ages[i] as number, 1);
          });
          expect(life.monthAges.length).toBe(life.trace.months);
          // Month ages advance by a twelfth and end in the last year.
          expect((life.monthAges[12] as number) - (life.monthAges[0] as number)).toBeCloseTo(1, 6);
          expect(life.monthYear[life.trace.months - 1]).toBe(life.time.length - 1);
          expect(life.time.domain[0]).toBeCloseTo(life.monthAges[0] as number, 6);
          const ends = yearEnds(life);
          expect(ends[ends.length - 1]).toBe(life.trace.months - 1);
        }
      });

      it('edits the event log: repeats folded, pairs joined, everything in order', () => {
        for (const { life } of lives(slug)) {
          const raw = life.trace.events;
          const count = (kind: string) => life.events.filter((e) => e.kind === kind).length;
          expect(count('roth_conversion')).toBeLessThanOrEqual(Math.max(1, Math.ceil(raw.filter((e) => e.kind === 'roth_conversion').length / 2)));
          expect(count('spending_raise')).toBeLessThanOrEqual(raw.filter((e) => e.kind === 'spending_raise').length);
          // A layoff followed by a rehire becomes one entry.
          const rehired = raw.filter((e) => e.kind === 'rehired').length;
          expect(count('rehired')).toBeLessThanOrEqual(Math.max(0, rehired - Math.min(rehired, raw.filter((e) => e.kind === 'laid_off').length)));
          expect(life.events.length).toBeLessThan(Math.max(8, raw.length));
          for (let i = 1; i < life.events.length; i++) expect((life.events[i] as { month: number }).month).toBeGreaterThanOrEqual((life.events[i - 1] as { month: number }).month);
          for (const e of life.events) {
            expect(e.title.length).toBeGreaterThan(2);
            expect(e.title.length).toBeLessThanOrEqual(26);
            expect(e.detail).not.toMatch(/NaN|undefined|null|Person \d/);
            expect(e.detail.trim()).toMatch(/\.$/);
            expect(e.year).toBe(life.monthYear[e.month]);
          }
          // Small unplanned expenses are counted, not marked.
          const shocks = raw.filter((e) => e.kind === 'expense_shock').length;
          expect(life.smallShocks.count + count('expense_shock')).toBe(shocks);
        }
      });

      it('tells the life in whole sentences that point at real years', () => {
        for (const { life, summary } of lives(slug)) {
          const story = narrative(life, summary);
          expect(story.length).toBeGreaterThan(3);
          for (const s of story) {
            expect(s.text).not.toMatch(/NaN|undefined|null|Person \d|Infinity/);
            expect(s.text.trim()).toMatch(/\.$/);
            if (s.year !== undefined) {
              expect(s.year).toBeGreaterThanOrEqual(0);
              expect(s.year).toBeLessThan(life.time.length);
            }
          }
          expect(narrative(life, summary)).toEqual(story);
          expect(headline(life)).toMatch(/^Work (optional at \d|never optional)/);
          for (const text of [timelineTakeaway(life, crowdFor(life, summary), yearEnds(life)), flowTakeaway(life), accountTakeaway(life), marketTakeaway(life)]) {
            expect(text).not.toMatch(/NaN|undefined|null|Infinity/);
            expect(text).toMatch(/\d/);
          }
        }
      });

      it('balances every year of the ledger and converts the tax return to today’s dollars', () => {
        for (const { life } of lives(slug)) {
          for (let y = 0; y < life.time.length; y++) {
            const ledger = yearLedger(life, y);
            // Whole-dollar rounding per series per month.
            expect(Math.abs(ledger.totalIn - ledger.totalOut)).toBeLessThan(30 * ledger.months);
            const netWorth = ledger.balances[ledger.balances.length - 1];
            expect(netWorth?.key).toBe('net_worth');
            const parts = ledger.balances.slice(0, -1).reduce((t, l) => t + l.amount, 0);
            expect(Math.abs(parts - (netWorth?.amount ?? 0))).toBeLessThan(20);
          }
          const filed = taxReturn(life, Math.min(5, life.time.length - 1));
          expect(filed).not.toBeNull();
          for (const line of filed?.lines ?? []) expect(line.real).toBeCloseTo(line.nominal / (filed?.priceLevel ?? 1), 6);
          expect(filed?.lines.some((l) => l.key === 'total' && l.strong)).toBe(true);
        }
      });

      it('measures the market this life lived through', () => {
        for (const { life } of lives(slug)) {
          expect(life.growth.length).toBe(life.trace.months);
          expect(life.growth.every((g) => g > 0)).toBe(true);
          const worst = life.scalar('worst_drawdown');
          if (life.drawdown && worst !== null) expect(life.drawdown.depth).toBeCloseTo(worst, 1);
          if (life.drawdown) {
            expect(life.drawdown.peak).toBeLessThan(life.drawdown.trough);
            expect(fallAt(life, life.drawdown.trough)).toBeCloseTo(life.drawdown.depth, 6);
          }
          expect(fallAt(life, 0)).toBe(0);
        }
      });

      it('offers distinct lives, each with its rule', () => {
        const { summary } = fixtures.get(slug) as Fixture;
        const choices = lifeChoices(summary);
        expect(choices.length).toBeGreaterThanOrEqual(3);
        expect(new Set(choices.map((c) => c.pathIndex)).size).toBe(choices.length);
        for (const c of choices) {
          expect(c.rule.length).toBeGreaterThan(10);
          expect(c.describes).not.toMatch(/NaN|undefined|null/);
        }
        const earliest = choices.find((c) => c.key === 'earliest');
        const latest = choices.find((c) => c.key === 'latest');
        const fi = summary.scatter.columns.find((c) => c.scalar === 'fi_age')?.values ?? [];
        const ageOf = (path: number) => fi[summary.scatter.paths.indexOf(path)] as number;
        if (earliest && latest) expect(ageOf(earliest.pathIndex)).toBeLessThanOrEqual(ageOf(latest.pathIndex));
      });
    });
  }

  it('falls back to calendar years when the plan is not at hand', () => {
    const f = fixtures.get('dual-income-family') as Fixture;
    const life = buildLife(f.traces[0] as PathTrace, null);
    expect(life.hasAges).toBe(false);
    expect(Math.floor(life.time.ages[0] as number)).toBe(life.time.years[0]);
    expect(life.names).toEqual(['Person 1', 'Person 2']);
    expect(headline(life)).not.toMatch(/ends at/);
  });
});

describe('layoutLanes', () => {
  const overlap = (a: { left: number; width: number }, b: { left: number; width: number }, gap: number) => a.left < b.left + b.width + gap && b.left < a.left + a.width + gap;

  it('never lets two labels in a row collide', () => {
    const items = Array.from({ length: 30 }, (_, i) => ({ x: 20 + i * 17, width: 60 + (i % 4) * 15, priority: (i * 7) % 5 }));
    const placed = layoutLanes(items, { min: 0, max: 560, rows: 4, gap: 8 });
    const shown = placed.map((p, i) => ({ ...p, width: (items[i] as { width: number }).width })).filter((p) => p.row >= 0);
    expect(shown.length).toBeGreaterThan(8);
    for (let i = 0; i < shown.length; i++) {
      const a = shown[i] as (typeof shown)[number];
      expect(a.left).toBeGreaterThanOrEqual(0);
      expect(a.left + a.width).toBeLessThanOrEqual(560);
      for (let j = i + 1; j < shown.length; j++) {
        const b = shown[j] as (typeof shown)[number];
        if (a.row === b.row) expect(overlap(a, b, 8 - 1e-9)).toBe(false);
      }
    }
  });

  it('gives the most important labels the rows nearest the plot and drops the least important first', () => {
    const items = [
      { x: 100, width: 80, priority: 1 },
      { x: 105, width: 80, priority: 5 },
      { x: 110, width: 80, priority: 3 },
    ];
    const placed = layoutLanes(items, { min: 0, max: 400, rows: 2 });
    expect(placed[1]?.row).toBe(0);
    expect(placed[2]?.row).toBe(1);
    expect(placed[0]?.row).toBe(-1);
  });

  it('centres a lone label on its mark and keeps it inside the frame', () => {
    expect(layoutLanes([{ x: 200, width: 50, priority: 1 }], { min: 0, max: 400, rows: 1 })[0]).toEqual({ row: 0, left: 175 });
    expect(layoutLanes([{ x: 395, width: 50, priority: 1 }], { min: 0, max: 400, rows: 1 })[0]).toEqual({ row: 0, left: 350 });
    expect(layoutLanes([{ x: 5, width: 500, priority: 1 }], { min: 0, max: 400, rows: 1 })[0]?.row).toBe(-1);
  });
});
