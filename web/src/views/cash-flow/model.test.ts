// The Cash flow transforms, checked against real engine output for the four demo
// households (the same wasm the app runs), plus the layout of the flow diagram.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import type { Summary } from '@/engine/output-types';
import * as wasm from '@/engine/pkg/firesight_wasm.js';
import type { ExampleProfile } from '@/engine/types';
import { layoutFlow, type FlowNode } from '@/ui/charts/ext/cashflow-FlowDiagram';
import { MIN_ALIVE, cashFlowModel, handoverIndex, hasEquityBand, livesAbroad, sharesOfUses, yearNodes } from './model';
import { EQUITY_CAPTION, accountsTakeaway, balanceTakeaway, flowsTakeaway, leadSentence, lower, plain, ratesTakeaway, taxTakeaway, yearTakeaway } from './text';

const summaries = new Map<string, Summary>();

beforeAll(() => {
  wasm.initSync({ module: readFileSync(new URL('../../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)) });
  for (const example of JSON.parse(wasm.exampleProfiles()) as ExampleProfile[]) {
    const json = JSON.stringify(example.profile);
    const engine = new wasm.Engine(json);
    try {
      summaries.set(example.slug, JSON.parse(wasm.summarize(engine.runChunk(0, 300), json)) as Summary);
    } finally {
      engine.free();
    }
  }
  // The engine is slow to warm up when the machine is busy.
}, 120_000);

const SLUGS = ['dual-income-family', 'early-career-saver', 'side-hustle-landlord', 'startup-employee'];

describe('cashFlowModel on real engine output', () => {
  for (const slug of SLUGS) {
    describe(slug, () => {
      const model = () => cashFlowModel(summaries.get(slug) as Summary);

      it('balances: money in equals money out in every year drawn', () => {
        const m = model();
        for (let i = m.flowStart; i <= m.end; i++) {
          const total = m.totalIn[i] as number;
          expect(total).toBeGreaterThan(0);
          // Means are rounded to whole dollars per series, so allow a dollar per series.
          expect(Math.abs(total - (m.totalOut[i] as number))).toBeLessThan(40);
        }
      });

      it('draws only years with enough lives, never the final part-year, and skips a part first year', () => {
        const summary = summaries.get(slug) as Summary;
        const m = model();
        expect(m.end).toBeLessThan(summary.axis.years.length - 1);
        expect((summary.axis.alive[m.end] as number) / summary.paths).toBeGreaterThanOrEqual(MIN_ALIVE);
        expect(m.flowStart).toBe(summary.axis.first_year_months < 12 ? 1 : 0);
      });

      it('nets money that only passes through cash', () => {
        const summary = summaries.get(slug) as Summary;
        const m = model();
        const series = (key: string) => summary.composition.find((c) => c.series === key)?.values ?? [];
        const from = m.moneyIn.find((g) => g.key === 'withdrawals')?.parts.find((p) => p.key === 'withdraw_cash')?.values ?? [];
        const to = m.moneyOut.find((g) => g.key === 'saving')?.parts.find((p) => p.key === 'save_cash')?.values ?? [];
        for (let i = m.flowStart; i <= m.end; i++) {
          expect(Math.min(from[i] as number, to[i] as number)).toBe(0);
          expect((from[i] as number) - (to[i] as number)).toBeCloseTo((series('withdraw_cash')[i] ?? 0) - (series('save_cash')[i] ?? 0), 6);
        }
      });

      it('keeps every group non-negative and its parts adding up', () => {
        const m = model();
        for (const g of [...m.moneyIn, ...m.moneyOut, ...m.assets, ...m.liabilities, ...m.savingBy, ...m.withdrawBy, ...m.taxes]) {
          g.values.forEach((v, i) => {
            expect(v).toBeGreaterThanOrEqual(0);
            expect(g.parts.reduce((t, p) => t + (p.values[i] as number), 0)).toBeCloseTo(v, 6);
          });
        }
      });

      it('net worth is assets less debts, and rates are sane', () => {
        const summary = summaries.get(slug) as Summary;
        const m = model();
        const series = (key: string) => summary.composition.find((c) => c.series === key)?.values ?? [];
        for (let i = 0; i <= m.end; i++) {
          const assets = ['balance_cash', 'balance_taxable', 'balance_traditional', 'balance_roth', 'balance_hsa', 'property_value'].reduce((t, k) => t + (series(k)[i] ?? 0), 0);
          expect(m.netWorth[i]).toBeCloseTo(assets - (series('mortgage')[i] ?? 0) - (series('debt')[i] ?? 0), 6);
          for (const rate of [m.taxShare[i], m.savingRate[i], m.withdrawalRate[i]]) {
            if (rate !== null && rate !== undefined) {
              expect(rate).toBeGreaterThanOrEqual(0);
              expect(rate).toBeLessThan(1.5);
            }
          }
        }
        expect(m.savingRate[0] === null || m.flowStart === 0).toBe(true);
      });

      it('the flow diagram shows nearly all of each year and its two sides agree', () => {
        const m = model();
        const nodes = yearNodes(m);
        expect(new Set([...nodes.sources, ...nodes.uses].map((n) => n.key)).size).toBe(nodes.sources.length + nodes.uses.length);
        for (let i = m.flowStart; i <= m.end; i += 5) {
          const sources = nodes.sources.reduce((t, n) => t + (n.values[i] as number), 0);
          const uses = nodes.uses.reduce((t, n) => t + (n.values[i] as number), 0);
          expect(sources / (m.totalIn[i] as number)).toBeGreaterThan(0.97);
          expect(sources / (m.totalIn[i] as number)).toBeLessThanOrEqual(1 + 1e-9);
          expect(uses / (m.totalOut[i] as number)).toBeGreaterThan(0.97);
          const u = sharesOfUses(m, i);
          expect(u.spent + u.taxed + u.saved).toBeCloseTo(1, 6);
        }
      });

      it('writes sentences with figures and no holes', () => {
        const summary = summaries.get(slug) as Summary;
        const m = model();
        const h = handoverIndex(m);
        const texts = [plain(leadSentence(m)), flowsTakeaway(m), yearTakeaway(m, m.flowStart), yearTakeaway(m, m.end), balanceTakeaway(m, summary), accountsTakeaway(m), taxTakeaway(m, summary), ratesTakeaway(m)];
        for (const text of texts) {
          expect(text.length).toBeGreaterThan(20);
          expect(text).not.toMatch(/NaN|undefined|null|Infinity/);
          expect(text).toMatch(/\d/);
          expect(text.trim()).toMatch(/\.$/);
        }
        if (h !== null) expect(plain(leadSentence(m))).toContain(`From ${Math.floor(m.time.ages[h] as number)}`);
      });
    });
  }
});

describe('lower', () => {
  it('keeps proper names when a label moves mid-sentence', () => {
    expect(lower('Roth accounts')).toBe('Roth accounts');
    expect(lower('Social Security')).toBe('Social Security');
    expect(lower('Taxable accounts')).toBe('taxable accounts');
    expect(lower('HSA')).toBe('HSA');
  });
});

describe('layoutFlow', () => {
  const node = (key: string, value: number, group = 'a'): FlowNode => ({ key, label: key, group, color: '#000', value });
  const options = { width: 600, height: 300, labelWidth: 120 };

  it('draws both sides on one scale through a trunk as tall as the total', () => {
    const layout = layoutFlow([node('wages', 80), node('withdrawals', 20, 'b')], [node('spend', 50), node('tax', 20, 'b'), node('save', 30, 'c')], options);
    expect(layout.total).toBe(100);
    expect(layout.trunk.height).toBeCloseTo(100 * layout.scale, 6);
    expect(layout.sources[0]?.height).toBeCloseTo(80 * layout.scale, 6);
    expect(layout.uses.reduce((t, n) => t + n.height, 0)).toBeCloseTo(layout.trunk.height, 6);
    // Everything fits inside the height given.
    for (const n of [...layout.sources, ...layout.uses]) {
      expect(n.y).toBeGreaterThanOrEqual(-1e-6);
      expect(n.y + n.height).toBeLessThanOrEqual(300 + 1e-6);
    }
  });

  it('keeps nodes in order without overlap and gives zero nodes no height', () => {
    const layout = layoutFlow([node('a', 10), node('b', 0), node('c', 30, 'x'), node('d', 60, 'x')], [node('out', 100)], options);
    expect(layout.sources[1]?.height).toBe(0);
    expect(layout.sources[1]?.ribbon).toBe('');
    const drawn = layout.sources.filter((n) => n.height > 0);
    for (let i = 1; i < drawn.length; i++) expect((drawn[i] as { y: number }).y).toBeGreaterThanOrEqual((drawn[i - 1] as { y: number; height: number }).y + (drawn[i - 1] as { height: number }).height);
  });

  it('never lets two labels collide, and leaves slivers unlabelled', () => {
    const uses = [node('big', 900), ...Array.from({ length: 8 }, (_, i) => node(`small${i}`, 10)), node('sliver', 0.5)];
    const layout = layoutFlow([node('in', 980.5)], uses, { ...options, labelGap: 15 });
    const ys = layout.uses.map((n) => n.labelY).filter((y): y is number => y !== null).sort((a, b) => a - b);
    expect(ys.length).toBe(9);
    for (let i = 1; i < ys.length; i++) expect((ys[i] as number) - (ys[i - 1] as number)).toBeGreaterThanOrEqual(15 - 1e-6);
    expect(layout.uses[layout.uses.length - 1]?.labelY).toBeNull();
    expect(layout.uses.some((n) => n.moved)).toBe(true);
  });

  it('copes with nothing at all', () => {
    const layout = layoutFlow([node('in', 0)], [node('out', 0)], options);
    expect(layout.scale).toBe(0);
    expect(layout.sources[0]?.labelY).toBeNull();
  });
});

describe('foreign income tax', () => {
  it('is named when the plan moves the household abroad, and only then', () => {
    const profiles = new Map((JSON.parse(wasm.exampleProfiles()) as ExampleProfile[]).map((e) => [e.slug, e.profile]));
    const landlord = profiles.get('side-hustle-landlord') as Parameters<typeof livesAbroad>[0];
    const family = profiles.get('dual-income-family') as Parameters<typeof livesAbroad>[0];
    expect(livesAbroad(landlord)).toBe(true);
    expect(livesAbroad(family)).toBe(false);
    const abroad = cashFlowModel(summaries.get('side-hustle-landlord') as Summary, { abroad: true });
    expect(abroad.taxes.find((g) => g.key === 'tax_state')?.label).toBe('State and foreign income tax');
    expect(yearNodes(abroad, { abroad: true }).uses.find((n) => n.key === 'out:tax_state')?.label).toBe('State and foreign tax');
    expect(cashFlowModel(summaries.get('dual-income-family') as Summary).taxes.every((g) => g.label !== 'State and foreign income tax')).toBe(true);
  });
});

describe('startup equity', () => {
  it('is its own band of money in, exercising is money out, and the year still balances', () => {
    const summary = summaries.get('startup-employee') as Summary;
    const m = cashFlowModel(summary);
    const series = (key: string) => summary.composition.find((c) => c.series === key)?.values ?? [];
    expect(hasEquityBand(m)).toBe(true);
    const band = m.moneyIn.find((g) => g.key === 'equity');
    expect(band?.label).toBe('Startup equity');
    expect(band?.parts.map((p) => p.key)).toEqual(['income_equity']);
    // Straight after wages in the stack, in a colour no other source uses.
    expect(m.moneyIn.map((g) => g.key).indexOf('equity')).toBe(m.moneyIn.map((g) => g.key).indexOf('wages') + 1);
    expect(m.moneyIn.filter((g) => g.color === band?.color)).toHaveLength(1);
    const exercise = m.moneyOut.find((g) => g.key === 'other_out')?.parts.find((p) => p.key === 'equity_exercise');
    expect(exercise?.label).toBe('Exercising options');
    let proceeds = 0;
    let paid = 0;
    for (let i = m.flowStart; i <= m.end; i++) {
      expect(band?.values[i]).toBe(Math.max(0, series('income_equity')[i] ?? 0));
      expect(exercise?.values[i]).toBe(Math.max(0, series('equity_exercise')[i] ?? 0));
      proceeds += band?.values[i] ?? 0;
      paid += exercise?.values[i] ?? 0;
      // The identity the view rests on, with the two new series in it.
      expect(Math.abs((m.totalIn[i] as number) - (m.totalOut[i] as number))).toBeLessThan(40);
    }
    expect(proceeds).toBeGreaterThan(100_000);
    expect(paid).toBeGreaterThan(1_000);
    // Paper value is a memo: it is in neither side and not on the balance sheet.
    expect([...m.moneyIn, ...m.moneyOut, ...m.assets].flatMap((g) => g.parts.map((p) => p.key))).not.toContain('equity_paper_value');
    // Equity proceeds are income for the saving rate.
    const i = m.flowStart + 2;
    const gross = ['income_wages', 'income_equity', 'income_self_employment', 'income_social_security', 'income_pensions', 'income_rental', 'income_other', 'save_employer'].reduce((t, k) => t + Math.max(0, series(k)[i] ?? 0), 0);
    const saved = m.moneyOut.find((g) => g.key === 'saving')?.values[i] ?? 0;
    expect(m.savingRate[i]).toBeCloseTo(saved / gross, 9);
    const nodes = yearNodes(m);
    expect(nodes.sources.find((n) => n.key === 'in:income_equity')?.label).toBe('Startup equity');
    expect(EQUITY_CAPTION).toMatch(/^Startup equity is an average over lives in which it paid and lives in which it did not; the Equity view shows how likely each is\.$/);
  });

  it('is absent from a plan without a private company', () => {
    for (const slug of ['dual-income-family', 'early-career-saver', 'side-hustle-landlord']) {
      const m = cashFlowModel(summaries.get(slug) as Summary);
      expect(hasEquityBand(m), slug).toBe(false);
      expect(yearNodes(m).uses.some((n) => n.key === 'out:equity_exercise'), slug).toBe(false);
    }
  });
});
