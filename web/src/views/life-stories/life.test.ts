// The Life stories transforms, checked against real traces from the engine for the
// demo households, plus the lane layout used for the event labels.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import type { PathTrace, Summary } from '@/engine/output-types';
import * as wasm from '@/engine/pkg/firesight_wasm.js';
import type { ExampleProfile, Profile } from '@/engine/types';
import { layoutLanes } from '@/ui/charts/ext/cashflow-lanes';
import { crowdFor, yearEnds } from './Timeline';
import { buildLife, fallAt, headline, lifeChoices, narrative, taxReturn, taxYears, yearLedger, type Life } from './life';
import { accountTakeaway, flowTakeaway, marketTakeaway, taxLaneTakeaway, timelineTakeaway } from './words';

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

const SLUGS = ['dual-income-family', 'early-career-saver', 'side-hustle-landlord', 'full-life'];
const lives = (slug: string): { life: Life; summary: Summary }[] => {
  const f = fixtures.get(slug) as Fixture;
  return f.traces.map((trace) => ({ life: buildLife(trace, f.profile), summary: f.summary }));
};

describe('events of the optional realism features', () => {
  // One event of each wording the engine logs, placed a year apart on a real life.
  const LOGGED: [kind: PathTrace['events'][number]['kind'], label: string, amount: number | null][] = [
    ['life_event', 'Person 1 becomes disabled at 52', 31_000],
    ['life_event', 'Person 1 starts disability insurance (SSDI)', null],
    ['life_event', 'Person 1 recovers and can work again', null],
    ['life_event', 'Changed jobs from Acme (pay +12%)', 120_000],
    ['life_event', 'Pay freeze at Acme: no raise this year', null],
    ['life_event', 'Pay cut of 10% at Acme for nine months', null],
    ['life_event', 'Involuntary retirement at 58: laid off from Acme and does not find work again', null],
    ['life_event', 'Business failed: self-employment income of person 2 ends', null],
    ['life_event', 'Divorce after 14 years of marriage: partner leaves with $412k', 412_000],
    ['life_event', 'Major repair on Home', 18_000],
    ['life_event', 'Insurance repriced: premiums up 15%', null],
    ['life_event', 'Rent: lease not renewed, new lease at market (+6.5% after inflation)', null],
    ['life_event', 'Home: adjustable rate reset from 5.25% to 7.25%, payment 2,840 a month', null],
    ['life_event', 'Listed Home for sale: expected to close in 7 months at 4.0% below market', null],
    ['life_event', 'Took a reverse mortgage on Home', 150_000],
    ['life_event', 'Opened a home-equity line on Home', 90_000],
    ['life_event', 'Cash-out refinance of Home at 6.75%', 80_000],
    ['life_event', 'Person 1 looks for work again: savings cover 71% of the plan', 0.71],
    ['life_event', 'Person 1 claims Social Security at 62.5: savings are nearly gone', 40_000],
    ['life_event', 'Retirement postponed: stocks are 27% below their peak', 0.27],
    ['life_event', 'Funded, but working one more year', null],
    ['life_event', 'Inherited IRA distribution (9 more to come)', 12_000],
    ['life_event', 'Inherited IRA distribution (8 more to come)', 12_000],
    ['spending_cut', 'Unemployed: discretionary spending cut by 10%', null],
    ['spending_raise', 'Back to work: discretionary spending restored', null],
    ['expense_shock', 'Home repair', 40_000],
    ['major_expense', 'Replace vehicle: Family car', 28_000],
    ['child', 'Mia born (twins), 5 months later than planned', null],
    ['child', 'Mia starts college: 5 years, aid 31% off the published price', null],
    ['child', 'Mia needs support for 20 months', 12_000],
    ['child', 'Children are on the health plan', 6_000],
    ['career_break', 'Parental leave for Mia: person 2 (4 months)', null],
    ['long_term_care', 'Mom needs care for 2.4 years; the household pays $7k a year', 7_000],
    ['long_term_care', 'Person 2 cares for person 1 at home, half the hours, and keeps working', null],
    ['windfall', 'Inheritance from Mom: $180k', 180_000],
    ['windfall', 'Life insurance pays out: Term life', 500_000],
    ['market', 'Panic sale: 50% of stocks moved to cash after a 31% fall', 20_000],
    ['market', 'Back into stocks after 14 months in cash', null],
    ['market', 'Lenders freeze home-equity lines', null],
    ['tax_loss_harvest', 'Harvested a capital loss and stayed invested (basis reset)', 4_000],
    ['tax_loss_harvest', 'Harvested a capital loss and stayed invested (basis reset)', 6_000],
    ['qualified_charitable_distribution', 'Person 1 gives from the IRA as a qualified charitable distribution', 3_000],
    ['policy_change', 'Policy risk: Social Security benefits cut to 77% of scheduled', 0.77],
    ['policy_change', 'Policy risk: ordinary income tax rates rise 2.0 points', 0.02],
    ['health_state', 'Person 1 health: chronic condition', null],
    ['bereavement', 'Person 2 dies within six months of being widowed (bereavement effect)', null],
    ['medicaid', "Savings spent down: Medicaid pays for person 1's care", null],
    ['ltc_insurance', 'Long-term-care insurance starts paying for person 1', null],
    ['ltc_insurance', "Person 2's long-term-care insurance lapses: the premium is unaffordable", null],
    ['other', 'Saving paused: high-interest debt', null],
  ];

  const staged = () => {
    const f = fixtures.get('dual-income-family') as Fixture;
    const base = f.traces[0] as PathTrace;
    const [y = 2026, m = 1] = base.start.split('-').map(Number);
    // Consecutive harvests and inherited-IRA payouts fall in neighbouring years so they fold; everything else is a year apart.
    const events = LOGGED.map(([kind, label, amount], i) => ({ kind, label, amount, month: `${y + 1 + Math.min(i, Math.floor(base.months / 12) - 3)}-${String(m).padStart(2, '0')}` }));
    return buildLife({ ...base, events }, f.profile);
  };
  const find = (life: Life, title: string) => life.events.find((e) => e.title === title);

  it('reads as plain sentences with short titles', () => {
    const life = staged();
    for (const e of life.events) {
      expect(e.title.length, e.title).toBeLessThanOrEqual(26);
      expect(e.detail, e.detail).not.toMatch(/NaN|undefined|null|[Pp]erson \d|Policy risk|\.\.$/);
      expect(e.detail.trim(), e.detail).toMatch(/\.$/);
      // An age the engine already wrote into the sentence is not said twice.
      expect(e.detail, e.detail).not.toMatch(/at \d+(\.\d)?[:,]? .*at age \d+\.$|at \d+ at age/);
    }
    expect(find(life, 'Disability')?.detail).toMatch(/^Priya becomes disabled at 52\.$/);
    expect(find(life, 'Disability')?.trouble).toBe(true);
    expect(find(life, 'Forced to retire')?.detail).toMatch(/^Involuntary retirement at 58: laid off from Acme and does not find work again\.$/);
    expect(find(life, 'Divorce')?.detail).toMatch(/partner leaves with \$412k at age \d+\.$/);
    expect(find(life, 'Changes jobs')?.detail).toMatch(/^Changed jobs from Acme \(pay \+12%\) at age \d+\.$/);
    expect(find(life, 'Mortgage rate resets')).toBeDefined();
    expect(find(life, 'Claims early or late')?.detail).toMatch(/^Priya claims Social Security at 62\.5: savings are nearly gone\.$/);
    expect(find(life, 'Tightens its belt')?.detail).toMatch(/^Discretionary spending cut by 10% while out of work, from age \d+\.$/);
    expect(find(life, 'Home repair')?.detail).toMatch(/^Home repair: an unplanned expense of \$40k at age \d+\.$/);
    expect(find(life, 'Benefits cut')?.detail).toMatch(/^The law changes: Social Security benefits cut to 77% of scheduled from age \d+\.$/);
    expect(find(life, 'Taxes rise')).toBeDefined();
    expect(find(life, 'Medicaid')?.detail).toMatch(/Medicaid pays for Priya's care at age \d+\.$/);
    expect(find(life, 'Health changes')?.detail).toMatch(/^Priya’s health becomes: chronic condition at age \d+\.$/);
    expect(find(life, 'Care at home')?.detail).toMatch(/^Daniel cares for person 1 at home|^Daniel cares for Priya at home/);
    expect(find(life, 'A parent needs care')).toBeDefined();
    expect(find(life, 'Panic sale')?.trouble).toBe(true);
    expect(find(life, 'New vehicle')?.detail).toMatch(/^Replaces the family car at age \d+ for \$28k\.$/);
    expect(find(life, 'Inheritance')?.detail).toMatch(/^Inheritance from Mom: \$180k at age \d+\.$/);
    expect(find(life, 'Life insurance pays')?.detail).toMatch(/^Life insurance pays out: Term life at age \d+: \$500k\.$/);
    expect(find(life, 'Supports a child')?.detail).toMatch(/Mia needs support for 20 months at age \d+: \$12k a year\.$/);
  });

  it('folds the yearly repeats and keeps them apart from the spending rule', () => {
    const life = staged();
    const count = (kind: string) => life.events.filter((e) => e.kind === kind).length;
    expect(count('tax_loss_harvest')).toBe(1);
    expect(find(life, 'Harvests tax losses')?.detail).toMatch(/^Realises \$10k of losses in the taxable account between ages \d+ and \d+, in 2 sales/);
    expect(count('qualified_charitable_distribution')).toBe(1);
    expect(life.events.filter((e) => e.title === 'Inherited IRA')).toHaveLength(1);
    expect(find(life, 'Inherited IRA')?.detail).toMatch(/over 2 years/);
    // Belt-tightening while out of work is not one of the spending rule's cuts.
    expect(find(life, 'Spending cut')).toBeUndefined();
    const story = narrative(life, null).map((s) => s.text).join(' ');
    expect(story).not.toMatch(/The spending rule/);
    expect(story).toMatch(/becomes disabled at 52\./);
    expect(story).not.toMatch(/health plan/);
    // Each turn of the life is told once, and the vehicle cycle in one sentence.
    const told = narrative(life, null).map((s) => s.text);
    expect(new Set(told).size).toBe(told.length);
    expect(told.filter((t) => /Replaces the/.test(t))).toHaveLength(1);
  });
});

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
          // Conversions more than 14 months apart are separate runs, so two isolated ones stay two.
          expect(count('roth_conversion')).toBeLessThanOrEqual(Math.max(2, Math.ceil(raw.filter((e) => e.kind === 'roth_conversion').length / 2)));
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

describe('tax by year and repeated shortfalls', () => {
  for (const slug of SLUGS) {
    it(`${slug}: reads each return's rates and never writes a broken sentence`, () => {
      for (const { life } of lives(slug)) {
        const rows = taxYears(life);
        expect(rows.length).toBe(life.time.length);
        rows.forEach((r, y) => {
          const filed = life.trace.tax_returns.some((t) => t.year === life.time.years[y]);
          expect(r.marginal !== null).toBe(filed);
          if (r.marginal !== null) {
            expect(r.marginal).toBeGreaterThanOrEqual(0);
            expect(r.marginal).toBeLessThan(0.6);
          }
          if (r.effective !== null) expect(r.effective).toBeGreaterThanOrEqual(0);
          if (r.taxable !== null) expect(r.taxable).toBeGreaterThanOrEqual(0);
        });
        const words = taxLaneTakeaway(life);
        expect(words).not.toMatch(/undefined|NaN|null/);
        expect(words.endsWith('.')).toBe(true);
        // Only the first shortfall is an event; later ones are unlabelled repeats, and the story tells it once.
        const short = life.events.filter((e) => e.kind === 'depleted');
        expect(short.length).toBe(life.trace.events.filter((e) => e.kind === 'depleted').length);
        expect(short.filter((e) => e.priority > 0).length).toBe(short.length ? 1 : 0);
        const story = narrative(life, null).map((x) => x.text);
        expect(story.filter((t) => /^Savings are exhausted/.test(t)).length).toBe(short.length ? 1 : 0);
      }
    });
  }

  it('marks years lived abroad, where the rate is the flat foreign rate', () => {
    const abroad = lives('side-hustle-landlord').flatMap(({ life }) => taxYears(life).filter((r) => r.foreign));
    expect(abroad.length).toBeGreaterThan(0);
    for (const { life } of lives('dual-income-family')) expect(taxYears(life).some((r) => r.foreign)).toBe(false);
  });

  it('describes a pick that is not in the scatter sample from the pick itself', () => {
    const { summary } = lives('dual-income-family')[0] as { summary: Summary };
    const trimmed: Summary = { ...summary, scatter: { paths: [], columns: summary.scatter.columns.map((c) => ({ ...c, values: [] })) } };
    const choices = lifeChoices(trimmed);
    expect(choices.find((c) => c.key === 'median')?.describes).toMatch(/^Ends with \$/);
  });
});
