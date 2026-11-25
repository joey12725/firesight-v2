// The Life stories transforms, checked against real traces from the engine for the four
// demo households, plus the lane layout used for the event labels.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import type { PathTrace, Summary } from '@/engine/output-types';
import * as wasm from '@/engine/pkg/firesight_wasm.js';
import type { ExampleProfile, Profile } from '@/engine/types';
import { layoutLanes } from '@/ui/charts/ext/cashflow-lanes';
import { PAPER_VALUE_LABEL, accountLaneTable, paperValueLine } from './Lanes';
import { crowdFor, yearEnds } from './Timeline';
import { COMPANY_TITLE_MAX, buildLife, fallAt, forfeiture, headline, lifeChoices, narrative, taxReturn, taxYears, yearLedger, type Life } from './life';
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

const SLUGS = ['dual-income-family', 'early-career-saver', 'side-hustle-landlord', 'startup-employee'];
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
          // Conversions at most 14 months apart fold into one entry; a longer gap starts another. (A bound of
          // half the raw count does not hold: a life can convert in isolated years.)
          const months = raw.filter((e) => e.kind === 'roth_conversion').map((e) => Number(e.month.slice(0, 4)) * 12 + Number(e.month.slice(5)));
          const conversionRuns = months.filter((m, i) => i === 0 || m - (months[i - 1] as number) > 14).length;
          expect(count('roth_conversion')).toBe(conversionRuns);
          expect(count('spending_raise')).toBeLessThanOrEqual(raw.filter((e) => e.kind === 'spending_raise').length);
          // A layoff followed by a rehire becomes one entry.
          const rehired = raw.filter((e) => e.kind === 'rehired').length;
          expect(count('rehired')).toBeLessThanOrEqual(Math.max(0, rehired - Math.min(rehired, raw.filter((e) => e.kind === 'laid_off').length)));
          expect(life.events.length).toBeLessThan(Math.max(8, raw.length));
          for (let i = 1; i < life.events.length; i++) expect((life.events[i] as { month: number }).month).toBeGreaterThanOrEqual((life.events[i - 1] as { month: number }).month);
          for (const e of life.events) {
            expect(e.title.length).toBeGreaterThan(2);
            // A title that names the company gets a little more room.
            expect(e.title.length).toBeLessThanOrEqual(e.kind.startsWith('company_') || e.kind === 'lockup_sale' ? COMPANY_TITLE_MAX : 26);
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

describe('startup equity in a life', () => {
  const EQUITY = ['options_exercised', 'tender_offer', 'company_acquired', 'company_ipo', 'company_failed', 'equity_forfeited', 'lockup_sale'];
  /** Traces lives of the startup demo until one has an event of `kind` (and passes `also`). */
  function find(kind: string, also: (trace: PathTrace) => boolean = () => true): { life: Life; summary: Summary } {
    const f = fixtures.get('startup-employee') as Fixture;
    const json = JSON.stringify(f.profile);
    const engine = new wasm.Engine(json);
    try {
      for (let path = 0; path < 400; path++) {
        const trace = JSON.parse(engine.trace(path)) as PathTrace;
        if (trace.events.some((e) => e.kind === kind) && also(trace)) return { life: buildLife(trace, f.profile), summary: f.summary };
      }
    } finally {
      engine.free();
    }
    throw new Error(`no life with ${kind} among the first 400`);
  }
  const sound = (life: Life) => {
    for (const e of life.events) {
      expect(e.title.length, e.title).toBeLessThanOrEqual(e.kind.startsWith('company_') || e.kind === 'lockup_sale' ? COMPANY_TITLE_MAX : 26);
      expect(e.detail).not.toMatch(/NaN|undefined|null|Person \d/);
      expect(e.detail.trim()).toMatch(/\.$/);
    }
  };

  it('a life in which the company fails: one troubling event, told in the story, nothing left on paper', () => {
    const { life, summary } = find('company_failed', (t) => /shuts down/.test(t.events.find((e) => e.kind === 'company_failed')?.label ?? ''));
    sound(life);
    const failed = life.events.filter((e) => e.kind === 'company_failed');
    expect(failed).toHaveLength(1);
    expect(failed[0]?.title).toBe('Northwind Robotics fails');
    expect(failed[0]?.priority).toBe(5);
    expect(failed[0]?.trouble).toBe(true);
    expect(failed[0]?.detail).toMatch(/^Northwind Robotics shuts down at age \d+: its shares and options are worth nothing\./);
    // The demo already holds 10,000 shares bought for $15,000: that cost is lost with the company.
    expect(failed[0]?.detail).toMatch(/(The \$[\d.]+[kM] paid for its shares is lost\.|What was paid for its shares is lost: \$[\d.]+[kM] in today's dollars at this date, a smaller figure than when it was paid because of the inflation since\.)$/);
    const story = narrative(life, summary).map((s) => s.text);
    expect(story.filter((t) => t.startsWith('Northwind Robotics shuts down'))).toHaveLength(1);
    // On paper until the company is gone, then zero, then no line at all.
    const paper = paperValueLine(life);
    expect(paper).not.toBeNull();
    expect(paper?.[0]).toBeGreaterThan(100_000);
    const year = failed[0]?.year as number;
    expect(life.annual('equity_paper_value')[year]).toBe(0);
    expect(paper?.[Math.min(life.time.length - 1, year + 3)] ?? null).toBeNull();
    expect(accountLaneTable(life).columns.some((c) => c.label === PAPER_VALUE_LABEL)).toBe(true);
    const ledger = yearLedger(life, 0);
    expect(ledger.memo.find((l) => l.key === 'equity_paper_value')?.label).toBe('Startup equity on paper');
    // Paper value is never part of the balance sheet.
    expect(ledger.balances.some((l) => l.key === 'equity_paper_value')).toBe(false);
  });

  it('a life in which the company lists: the listing, the sales after the lockup as one entry, the proceeds in the ledger', () => {
    const { life, summary } = find('company_ipo', (t) => t.events.filter((e) => e.kind === 'lockup_sale').length >= 2);
    sound(life);
    const ipo = life.events.filter((e) => e.kind === 'company_ipo');
    expect(ipo).toHaveLength(1);
    expect(ipo[0]?.title).toBe('Northwind Robotics goes public');
    expect(ipo[0]?.priority).toBe(5);
    expect(ipo[0]?.trouble).toBeFalsy();
    expect(ipo[0]?.detail).toMatch(/^Northwind Robotics goes public at age \d+, at \$[\d,.]+ a share; the shares cannot be sold for 6 months\.$/);
    // Two yearly sales (the demo sells over two years) are one decision: one entry, with the total.
    const raw = life.trace.events.filter((e) => e.kind === 'lockup_sale');
    const sales = life.events.filter((e) => e.kind === 'lockup_sale');
    expect(raw.length).toBeGreaterThanOrEqual(2);
    expect(sales).toHaveLength(1);
    expect(sales[0]?.title).toMatch(/^Sells shares: \$[\d.]+[kM]$/);
    expect(sales[0]?.detail).toMatch(new RegExp(`^Sells Northwind Robotics shares for \\$[\\d.]+[kM] before tax in ${raw.length} sales `));
    const story = narrative(life, summary).map((s) => s.text);
    expect(story.some((t) => t.startsWith('Northwind Robotics goes public'))).toBe(true);
    expect(story.some((t) => t.startsWith('Sells Northwind Robotics shares'))).toBe(true);
    // The proceeds are income in the year's ledger, and every year still balances.
    const proceeds = life.annual('income_equity');
    const y = proceeds.findIndex((v) => v > 0);
    expect(y).toBeGreaterThanOrEqual(0);
    const ledger = yearLedger(life, y);
    expect(ledger.moneyIn.find((s) => s.title === 'Income')?.lines.find((l) => l.key === 'income_equity')?.label).toBe('Startup equity proceeds');
    for (let k = 0; k < life.time.length; k++) {
      const l = yearLedger(life, k);
      expect(Math.abs(l.totalIn - l.totalOut)).toBeLessThan(30 * l.months);
    }
    expect(life.scalar('equity_gross_proceeds') as number).toBeGreaterThan(0);
  });

  it('exercises, tenders and forfeitures read as events, and a tender that sells nothing is left out', () => {
    const exercised = find('options_exercised').life;
    sound(exercised);
    const e = exercised.events.find((x) => x.kind === 'options_exercised');
    expect(e?.title).toMatch(/^(Exercises options: \$[\d.]+[kM]|Exercises as options vest)$/);
    expect(e?.detail).toMatch(/any tax on the exercises? comes on top\.$/);
    const y = exercised.annual('equity_exercise').findIndex((v) => v > 0);
    expect(yearLedger(exercised, y).moneyOut.find((s) => s.title === 'Spending')?.lines.find((l) => l.key === 'equity_exercise')?.label).toBe('Exercising options');

    const tender = find('tender_offer', (t) => t.events.some((x) => x.kind === 'tender_offer' && (x.amount ?? 0) > 0)).life;
    sound(tender);
    const sold = tender.trace.events.filter((x) => x.kind === 'tender_offer' && (x.amount ?? 0) > 0).length;
    expect(tender.events.filter((x) => x.kind === 'tender_offer')).toHaveLength(sold);
    expect(tender.events.find((x) => x.kind === 'tender_offer')?.title).toMatch(/^Tender offer: \$[\d.]+[kM]$/);

    const forfeited = find('equity_forfeited').life;
    sound(forfeited);
    const f = forfeited.events.find((x) => x.kind === 'equity_forfeited');
    expect(['Unvested equity forfeited', 'Vested RSUs forfeited', 'Options lapse']).toContain(f?.title);
    expect(f?.trouble).toBe(true);
  });

  it('reads every wording of a forfeiture label', () => {
    expect(forfeiture('Left Northwind Robotics: 3,592 unvested forfeited')).toMatchObject({ kind: 'forfeited', left: true, company: 'Northwind Robotics', unvested: '3,592', rsus: null });
    expect(forfeiture('Left Acme: 1,200 unvested forfeited, and 800 vested RSUs that required staying until the IPO or sale')).toMatchObject({ left: true, company: 'Acme', unvested: '1,200', rsus: '800' });
    expect(forfeiture('Left Acme: 800 vested RSUs forfeited (they required staying until the IPO or sale)')).toMatchObject({ left: true, unvested: null, rsus: '800' });
    // No "Left": the owner died or the job never started. Nobody left anything.
    expect(forfeiture('Acme: 5,000 unvested forfeited')).toMatchObject({ kind: 'forfeited', left: false, company: 'Acme', unvested: '5,000' });
    expect(forfeiture('4,797 vested Northwind Robotics options lapsed unexercised when the exercise window closed')).toMatchObject({ kind: 'lapsed', vested: '4,797', unvested: null, term: false });
    expect(forfeiture('1,000 vested and 250 unvested Acme options lapsed unexercised at their ten-year term')).toMatchObject({ kind: 'lapsed', vested: '1,000', unvested: '250', term: true });
  });

  /** A life built from the demo's first trace with its event log replaced: for wording that depends on the order of events. */
  function staged(events: { month: string; kind: string; label: string; amount: number | null }[]): Life {
    const f = fixtures.get('startup-employee') as Fixture;
    const trace = f.traces[0] as PathTrace;
    return buildLife({ ...trace, events: events as PathTrace['events'] }, f.profile);
  }
  const JOB = 'Staff engineer at Northwind Robotics';

  it('keeps the company name in a job label, and does not report layoffs from a company that has shut down', () => {
    const start = (fixtures.get('startup-employee') as Fixture).traces[0]!.start;
    const year = Number(start.slice(0, 4));
    const at = (y: number, m: number) => `${year + y}-${String(m).padStart(2, '0')}`;
    const life = staged([
      { month: at(3, 6), kind: 'company_failed', label: 'Northwind Robotics shuts down: its shares and options are worth nothing ($13,919 paid for its shares is lost)', amount: 13_919 },
      { month: at(3, 6), kind: 'laid_off', label: `Laid off from ${JOB}`, amount: null },
      { month: at(3, 7), kind: 'rehired', label: `Back at work at a new employer (was ${JOB})`, amount: 114_729 },
      { month: at(3, 10), kind: 'laid_off', label: `Laid off from ${JOB}`, amount: null },
      { month: at(4, 1), kind: 'rehired', label: `Back at work at a new employer (was ${JOB})`, amount: 108_983 },
    ]);
    const layoffs = life.events.filter((e) => e.kind === 'laid_off').map((e) => e.detail);
    expect(layoffs).toHaveLength(2);
    // The job goes with the company, said once, with the company's capitals intact.
    expect(layoffs[0]).toMatch(/^Loses the job as staff engineer at Northwind Robotics at age \d+, when the company shuts down; back at work at a new employer after 1 month at \$115k a year\.$/);
    // The later layoff is from the job that followed, not from the company that no longer exists.
    expect(layoffs[1]).toMatch(/^Laid off from the job that followed at age \d+.*; back at work after 3 months at \$109k a year\.$/);
    for (const text of layoffs) expect(text).not.toMatch(/northwind robotics/);
    expect(layoffs[1]).not.toMatch(/Northwind/);
    // An ordinary layoff from the company, before anything else: the label is lowered at its first letter only.
    const plain = staged([
      { month: at(2, 1), kind: 'laid_off', label: `Laid off from ${JOB}`, amount: null },
      { month: at(2, 4), kind: 'rehired', label: `Back at work at a new employer (was ${JOB})`, amount: 150_000 },
    ]);
    expect(plain.events.find((e) => e.kind === 'laid_off')?.detail).toMatch(/^Laid off as staff engineer at Northwind Robotics at age \d+.*; back at work at a new employer after 3 months at \$150k a year\.$/);
  });

  it('explains a loss that is smaller than what was paid, and a lapse beside an exercise', () => {
    const start = (fixtures.get('startup-employee') as Fixture).traces[0]!.start;
    const year = Number(start.slice(0, 4));
    const at = (y: number, m: number) => `${year + y}-${String(m).padStart(2, '0')}`;
    const life = staged([
      { month: at(1, 3), kind: 'options_exercised', label: 'Exercised 36,650 Northwind Robotics options for $55,224', amount: 55_224 },
      { month: at(1, 3), kind: 'equity_forfeited', label: '23,442 vested Northwind Robotics options lapsed unexercised when the exercise window closed', amount: 245_188 },
      { month: at(6, 4), kind: 'company_failed', label: 'Northwind Robotics shuts down: its shares and options are worth nothing ($60,270 paid for its shares is lost)', amount: 60_270 },
    ]);
    // The household chose to exercise and could not pay for all of it: said, so the lapse does not read as a choice or a fault.
    expect(life.events.find((e) => e.kind === 'equity_forfeited')?.detail).toMatch(
      /^23,442 vested options lapse unexercised at age \d+, when the exercise window closes, worth \$245k on paper\. Savings above the emergency fund could not cover the strike and the tax on these\.$/,
    );
    // Paid $55k (plus what was held), "lost" $60k years later: both in today's dollars at their own dates, and the sentence says so.
    expect(life.events.find((e) => e.kind === 'company_failed')?.detail).toMatch(/What was paid for its shares is lost: \$60k in today's dollars at this date, a smaller figure than when it was paid because of the inflation since\.$/);
    // Without an earlier exercise there is nothing to reconcile.
    const held = staged([{ month: at(3, 6), kind: 'company_failed', label: 'Northwind Robotics shuts down: its shares and options are worth nothing ($13,919 paid for its shares is lost)', amount: 13_919 }]);
    expect(held.events.find((e) => e.kind === 'company_failed')?.detail).toMatch(/The \$14k paid for its shares is lost\.$/);
    // A lapse with no exercise beside it, at the ten-year term, with unvested options too.
    const term = staged([{ month: at(5, 1), kind: 'equity_forfeited', label: '1,000 vested and 250 unvested Northwind Robotics options lapsed unexercised at their ten-year term', amount: 9_000 }]);
    expect(term.events.find((e) => e.kind === 'equity_forfeited')?.detail).toMatch(/^1,000 vested and 250 unvested options lapse unexercised at age \d+, at the end of their ten-year term, worth \$9k on paper\.$/);
    // Nobody "leaves" when the label does not say so.
    const died = staged([{ month: at(5, 1), kind: 'equity_forfeited', label: 'Northwind Robotics: 5,000 unvested forfeited', amount: 20_000 }]);
    expect(died.events.find((e) => e.kind === 'equity_forfeited')?.detail).toMatch(/^At age \d+, 5,000 unvested shares or options at Northwind Robotics are forfeited, worth \$20k on paper\.$/);
    const rsus = staged([{ month: at(5, 1), kind: 'equity_forfeited', label: 'Left Northwind Robotics: 800 vested RSUs forfeited (they required staying until the IPO or sale)', amount: 11_000 }]);
    expect(rsus.events.find((e) => e.kind === 'equity_forfeited')).toMatchObject({ title: 'Vested RSUs forfeited' });
    expect(rsus.events.find((e) => e.kind === 'equity_forfeited')?.detail).toMatch(/^Leaving Northwind Robotics at age \d+ forfeits 800 vested RSUs that required staying until the IPO or sale, worth \$11k on paper\.$/);
  });

  it('every equity event of every traced demo life is told once, before or after retirement', () => {
    const f = fixtures.get('startup-employee') as Fixture;
    for (const trace of f.traces) {
      const life = buildLife(trace, f.profile);
      const story = narrative(life, f.summary).map((s) => s.text);
      for (const e of life.events.filter((x) => EQUITY.includes(x.kind) && x.priority > 0)) expect(story.filter((t) => t === e.detail), e.detail).toHaveLength(1);
    }
  });

  it('offers a life for each way the equity can go, with the reason it is shown', () => {
    const f = fixtures.get('startup-employee') as Fixture;
    const choices = lifeChoices(f.summary);
    for (const key of ['equity_failed', 'equity_paid_median', 'equity_paid_p90']) {
      const pick = f.summary.picks.find((p) => p.label === key);
      const choice = choices.find((c) => c.pathIndex === pick?.path_index);
      expect(choice, key).toBeDefined();
      expect(choice?.because ?? choice?.why).toBeTruthy();
      expect(choice?.title.length).toBeGreaterThan(3);
    }
    expect(choices.find((c) => c.key === 'equity_failed')?.title).toBe('Northwind Robotics fails');
    // The other demos get no equity lives.
    expect(lifeChoices((fixtures.get('dual-income-family') as Fixture).summary).some((c) => c.key.startsWith('equity_'))).toBe(false);
  });
});
