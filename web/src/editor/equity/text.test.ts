// The editor's sentences, on the real engine's preview of the demo company and on
// shaped previews for the edge cases.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { equityPreview, exampleProfiles, initEngineSync } from '@/engine/api';
import type { EquityPreview } from '@/engine/equity-types';
import type { EquityHolding, OutcomeRow, PrivateCompany } from '@/engine/types';
import { equitySummary } from '../summaries';
import { apportion, companyDetail, holdingDetail, holdingsNoun, kindHeading, monthsAhead, multipleHint, nothingCounts, nothingFigure, optionLine, outcomeDetail, previewCaption, previewNotes, previewSentences, tenderHint, tenderLine, unitLine, yearsRange } from './text';

// At load, not in beforeAll: one describe block reads the demo while it is collected.
initEngineSync(readFileSync(new URL('../../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));

const startup = () => exampleProfiles().find((e) => e.slug === 'startup-employee')!.profile;
const demoCompany = (): PrivateCompany => startup().private_equity[0]!;
const shaped = (over: Partial<EquityPreview>): EquityPreview => ({ ...equityPreview(demoCompany()), ...over });

describe('the preview sentences', () => {
  it('split the futures that pay nothing by why, adding up', () => {
    const preview = equityPreview(demoCompany());
    const n = nothingCounts(preview);
    expect(n.nothing).toBe(Math.round(preview.nothing_share * 100));
    expect(n.fails + n.writtenOff + n.soldForTooLittle + n.other).toBe(n.nothing);
    const [first, second] = previewSentences(preview, true);
    // "Within thirty years": the odds table beside it counts ten, and a company still private then is drawn again.
    const lead = `In ${n.nothing} of 100 futures this pays nothing: the company shuts down within thirty years in ${n.fails}, is sold for too little to reach common stock in ${n.soldForTooLittle}, and is still private after thirty years in ${n.writtenOff}.`;
    expect(first).toBe(n.other >= 1 ? `${lead} In ${n.other} more the options end up under water or lapse first.` : lead);
    // The demo's middle future pays nothing.
    expect(preview.quantiles[2]).toBe(0);
    // Rounded as the chart's axis rounds: a quantile of a rough model is not known to the dollar.
    expect(second).toMatch(/^The middle future is one of those\. The best ten pay more than \$[\d.]+[kM]\.$/);
    expect(nothingFigure(preview)).toBe(`${n.nothing} of 100 pay nothing`);
  });

  it('cover a paying middle, an empty top, certain failure and no failure', () => {
    const paying = previewSentences(shaped({ nothing_share: 0.2, outcome_shares: [0.2, 0, 0.4, 0.4], acquisition_zero_share: 0, quantiles: [0, 1000, 250_000, 900_000, 2_000_000] }), true);
    expect(paying).toEqual(['In 20 of 100 futures this pays nothing: the company shuts down within thirty years in 20.', 'The middle future pays $250k; the best ten pay more than $2M.']);

    const rare = shaped({ nothing_share: 0.95, outcome_shares: [0.9, 0.05, 0.03, 0.02], acquisition_zero_share: 0, quantiles: [0, 0, 0, 0, 0] });
    rare.dots = [...Array.from({ length: 95 }, () => 0), 1, 2, 3, 4, 50_000];
    expect(previewSentences(rare, true)[1]).toBe('In the rest it pays up to $50k.');

    const certain = previewSentences(shaped({ nothing_share: 1, outcome_shares: [1, 0, 0, 0], acquisition_zero_share: 0, quantiles: [0, 0, 0, 0, 0] }), true);
    expect(certain).toEqual(['In 100 of 100 futures this pays nothing: the company shuts down within thirty years in 100.']);

    const never = previewSentences(shaped({ nothing_share: 0, outcome_shares: [0, 0, 0, 1], acquisition_zero_share: 0, quantiles: [10, 20, 30, 40, 50] }), false);
    expect(never[0]).toBe('In every one of 100 futures this pays something.');

    // A sale that only misses common stock, and options under water on top.
    const sold = previewSentences(shaped({ nothing_share: 0.5, outcome_shares: [0, 0, 1, 0], acquisition_zero_share: 0.3, quantiles: [0, 0, 5, 10, 20], deadline_before_outcome_share: 0 }), true);
    expect(sold[0]).toBe('In 50 of 100 futures this pays nothing: the company is sold for too little to reach common stock in 30. In 20 more the options end up under water.');
    const underwater = previewSentences(shaped({ nothing_share: 0.5, outcome_shares: [0, 0, 0, 1], acquisition_zero_share: 0, quantiles: [0, 0, 5, 10, 20], deadline_before_outcome_share: 0 }), true);
    expect(underwater[0]).toBe('In 50 of 100 futures this pays nothing: the options end up under water.');
    // Where a deadline can come before the outcome, lapsing is the other way options end up paying nothing.
    const lapsing = previewSentences(shaped({ nothing_share: 0.5, outcome_shares: [0, 0, 0, 1], acquisition_zero_share: 0, quantiles: [0, 0, 5, 10, 20], deadline_before_outcome_share: 0.2 }), true);
    expect(lapsing[0]).toBe('In 50 of 100 futures this pays nothing: the options end up under water or lapse first.');
  });

  it("never use an em dash and say today's dollars once", () => {
    const preview = equityPreview(demoCompany());
    const all = [...previewSentences(preview, true), previewCaption(preview, false, true), previewCaption(preview, true, false)].join(' ');
    expect(all).not.toMatch(/—|–/);
    const caption = previewCaption(preview, true, true);
    expect(caption.match(/today's dollars/g)).toHaveLength(1);
    expect(caption).toContain('with the strike price already taken off the options');
    expect(caption).toContain(`Worth $1,458,000 on paper at the last round's price`);
    expect(caption).toMatch(/\(typical odds: \d+\.\d\)\.$/);
    expect(previewCaption(preview, false, false)).toMatch(/^Before tax, in today's dollars/);
  });

  it('does not let a planned tender or a company left out pass as "pays nothing" unqualified', () => {
    const company = demoCompany();
    const preview = equityPreview(company);
    const k = Math.round(preview.nothing_share * 100);
    const planned = { ...company, tenders: { ...company.tenders, next_in_months: 2 } };
    expect(previewSentences(preview, true, planned)[0]).toMatch(new RegExp(`^In ${k} of 100 futures this pays nothing at the company's outcome, apart from the tender you plan in 2 months: `));
    expect(nothingFigure(preview, planned)).toBe(`${k} of 100 pay nothing at the outcome`);
    expect(nothingFigure(preview, company)).toBe(`${k} of 100 pay nothing`);
    expect(nothingFigure(preview, { ...planned, included: false })).toBe('Not counted');
    expect(previewCaption(equityPreview(planned), false, true, planned)).toContain('Tender offers before the outcome are not in these amounts, the planned one included;');
    expect(monthsAhead(0)).toBe('this month');
    expect(monthsAhead(1)).toBe('in 1 month');
    expect(monthsAhead(96)).toBe('in about 8 years');
  });

  it('says what the amounts assume about staying, by whether there is a job and a plan to leave', () => {
    const company = demoCompany();
    const preview = equityPreview(company);
    // Employed, staying: said, with the caution that the plan's own results are lower where the job ends first.
    expect(previewCaption(preview, false, true, company)).toContain("Assumes you stay until the outcome and sell then; where the job ends first (a layoff, or leaving) less has vested, so the plan's own results are lower.");
    expect(previewCaption(preview, false, true, company)).toContain("on paper at the last round's price, vested and unvested together;");
    // No linked job: nobody is staying anywhere, and everything is vested.
    const former = { ...company, job: null, holdings: company.holdings.map((h) => ({ ...h, vested: h.shares, vest_months_left: 0, cliff_months: 0 })) };
    const formerCaption = previewCaption(equityPreview(former), false, true, former);
    expect(formerCaption).not.toMatch(/you stay/);
    expect(formerCaption).toContain('Assumes you sell at the outcome.');
    expect(formerCaption).not.toContain('vested and unvested together');
    // A planned departure is counted by the engine's preview, and the strip says so in full-size text.
    const leaving = { ...company, policy: { ...company.policy, leave_after_months: 12 } };
    const leavingPreview = equityPreview(leaving);
    expect(leavingPreview.paper_value_total).toBe(preview.paper_value_total);
    expect(leavingPreview.quantiles[4]).toBeLessThan(preview.quantiles[4]);
    expect(previewCaption(leavingPreview, false, true, leaving)).not.toMatch(/you stay/);
    const notes = previewNotes(leavingPreview, leaving);
    expect(notes[0]).toBe('These amounts count your plan to leave in 12 months: what is unvested then is forfeited and the exercise window starts.');
    const strict = { ...leaving, holdings: leaving.holdings.map((h) => (h.kind.type === 'rsu' ? { ...h, kind: { ...h.kind, must_be_employed_at_event: true } } : h)) };
    expect(previewNotes(equityPreview(strict), strict)[0]).toMatch(/and the exercise window starts, and vested RSUs that require staying are lost if the company is still private\.$/);
    // The exercise window after leaving is the first option deadline, and its cost is said.
    expect(leavingPreview.option_deadline_months).toBe(12 + leaving.exercise_window_months);
    expect(previewNotes(preview, company)[0]).toMatch(/^The first option deadline, the end of a grant's ten-year term, is in about 7 years\./);
    expect(notes[1]).toMatch(/^The first option deadline, the end of the exercise window after you leave, is in 15 months\. In \d+ of 100 futures a deadline comes before the company's outcome and the options have to be exercised with cash or lost\. Exercising at the first one costs about \$[\d.]+k before tax, and the amounts here assume you can pay it\.$/);
    const lapsing = { ...leaving, policy: { ...leaving.policy, at_deadline: 'forfeit' as const } };
    expect(previewNotes(equityPreview(lapsing), lapsing)[1]).toMatch(/and those options lapse, as you chose\.$/);
    // Nothing to add for a company with no options and no plan to leave.
    const plain = { ...company, holdings: company.holdings.filter((h) => h.kind.type === 'shares') };
    expect(previewNotes(equityPreview(plain), plain)).toEqual([]);
    for (const text of [...notes, formerCaption]) expect(text).not.toMatch(/—|–|undefined|NaN/);
  });

  it('echoes what a planned tender would sell', () => {
    const company = demoCompany();
    expect(tenderLine(company)).toBeNull();
    const planned = { ...company, tenders: { ...company.tenders, next_in_months: 2 } };
    // 10,000 shares and the options vested two months on count; half of the 20% limit, all from the shares, at $14.
    expect(tenderLine(planned)).toBe(
      "Sells about 7,428 of the 74,276 shares and vested options you will have then, at $14.00 a share: about $104k before tax, after the strike. That price is today's; the tender is priced from the company's simulated share price in that month. RSUs cannot be sold in a tender. The same limit and price apply to any later tender.",
    );
    expect(tenderLine({ ...planned, policy: { ...planned.policy, tender_sell_fraction: 0 } })).toMatch(/^Nothing is sold in it, because you chose to sell 0%/);
    expect(tenderLine({ ...planned, job: null })).toMatch(/^Nothing is sold in it: it is not open to people who have left/);
    expect(tenderLine({ ...planned, policy: { ...planned.policy, leave_after_months: 1 } })).toMatch(/^Nothing is sold in it: it is not open to people who have left/);
    expect(tenderLine({ ...planned, job: null, tenders: { ...planned.tenders, former_employees: true } })).toMatch(/^Sells about 6,800 of the 68,000 shares and vested options you will have then/);
    expect(tenderLine({ ...planned, holdings: planned.holdings.filter((h) => h.kind.type === 'rsu') })).toMatch(/^Nothing is sold in it: a tender takes shares you own and vested options/);
  });

  it('apportion whole counts', () => {
    expect(apportion(55, [0.43475, 0.03075, 0.0855, 0])).toEqual([43, 3, 9, 0]);
    expect(apportion(10, [1, 1, 1])).toEqual([4, 3, 3]);
    expect(apportion(5, [0, 0])).toEqual([0, 0]);
  });
});

describe('holdings in words', () => {
  const [iso, nso, rsu, shares] = demoCompany().holdings as [EquityHolding, EquityHolding, EquityHolding, EquityHolding];

  it('describes each kind in the collapsed row', () => {
    expect(holdingDetail(iso)).toBe('50,000 vested, 10,000 unvested, $1.50 strike');
    expect(holdingDetail(nso)).toBe('8,000 vested, 32,000 unvested, $4.00 strike');
    expect(holdingDetail(rsu)).toBe('12,000 units, none vested');
    expect(holdingDetail(shares)).toBe('10,000 shares, $1.50 basis');
    expect(holdingDetail({ ...iso, vested: 60_000 })).toBe('60,000 vested, $1.50 strike');
    expect(holdingDetail({ ...iso, vested: 0, kind: { type: 'iso', strike: 0.0125, granted_months_ago: 1 } })).toBe('60,000 options, none vested, $0.0125 strike');
  });

  it('states the cost to exercise and the value after the strike, in whole dollars from exact prices', () => {
    expect(optionLine(iso, 14)).toBe("Costs $90,000 to exercise in full ($75,000 for the vested part), before tax. Worth $750,000 after the strike at the last round's price.");
    expect(optionLine({ ...iso, kind: { type: 'iso', strike: 20, granted_months_ago: 1 } }, 14)).toContain("Worth nothing at the last round's price");
    expect(optionLine(iso, 0)).toBe('Costs $90,000 to exercise in full ($75,000 for the vested part), before tax.');
    expect(optionLine(rsu, 14)).toBeNull();
    expect(unitLine(rsu, 14)).toBe("Worth $168,000 at the last round's price.");
    expect(unitLine(iso, 14)).toBeNull();
    expect(holdingsNoun(demoCompany())).toBe('122,000 options, RSUs, and shares');
    expect(holdingsNoun({ holdings: [iso] })).toBe('60,000 options');
    expect(companyDetail('Series C', 4)).toBe('Series C, 4 holdings');
    expect(companyDetail('Seed', 0)).toBe('Seed, no holdings');
  });
});

describe('outcomes in words', () => {
  const row = (kind: OutcomeRow['kind'], low: number, high: number): OutcomeRow => ({ id: kind, kind, probability: 0.25, years_from: 1.5, years_to: 6, multiple_low: low, multiple_high: high });

  it('prices a sale at what common stock gets, never the company multiple times the round price', () => {
    const sale = outcomeDetail(row('acquired', 0.1666, 2.161), 14, 0.3);
    expect(sale).toBe('in 1.5 to 6 years, $0 to $30.25 a share for common stock (the company sells for 0.17 to 2.2 times its last valuation)');
    expect(sale).not.toContain('$2.33');
    expect(outcomeDetail(row('ipo', 1.1, 3.614), 14, 0.3)).toBe('in 1.5 to 6 years, $15.40 to $50.60 a share at the IPO');
    expect(outcomeDetail(row('fails', 1, 1), 14, 0.3)).toBe('in 1.5 to 6 years');
    expect(outcomeDetail(row('still_private', 1, 1), 14, 0.3)).toBe('The same odds then apply to the next ten years. After thirty the equity is counted as worth nothing.');
    // Without a price it speaks in multiples.
    expect(outcomeDetail(row('ipo', 1.1, 3.614), 0, 0.3)).toBe("in 1.5 to 6 years, at 1.1 to 3.6 times the last round's price");
    expect(outcomeDetail(row('acquired', 0.1666, 2.161), 0, 0.3)).toBe('in 1.5 to 6 years; the company sells for 0.17 to 2.2 times its last valuation');
    expect(yearsRange({ years_from: 2, years_to: 2 })).toBe('in 2 years');
  });

  it('writes the hints of a custom row', () => {
    expect(multipleHint(row('ipo', 1.1, 3.614), 14, 0.3, null)).toBe('$15.40 to $50.60 a share at the IPO; one in ten below, one in ten above. The price when the lockup ends varies around it.');
    expect(multipleHint(row('acquired', 0.65, 2), 14, 0.3, 23)).toBe('$7.00 to $28.00 a share for common stock; one in ten below, one in ten above. Common stock gets nothing in 23 of 100 such sales.');
    expect(multipleHint(row('acquired', 0.65, 2), 0, 0.3, null)).toBe('One in ten below, one in ten above.');
    expect(kindHeading('Is acquired', 27, 27, 0)).toBe('Is acquired: 27 of 100 in your table, 27 typical');
    expect(kindHeading('Goes public', 1.5, 0, 1)).toBe('Goes public: 1.5 of 100 in your table, none typical');
  });

  it('turns a yearly tender chance into words', () => {
    // The chance is an average over every future; tenders come only where the company does well.
    expect(tenderHint(0.04)).toBe("An average over every future of the company: about one tender every 25 years. They come only in futures where the company ends at or above its last round's price, and more often there.");
    expect(tenderHint(0.04, 0.123)).toMatch(/more often there\. One comes before the outcome in about 12 of 100 futures\.$/);
    expect(tenderHint(0.95)).toMatch(/^As often as the model allows: at most one a year\./);
    expect(tenderHint(0)).toBe('No tender offers, apart from one you plan below.');
    expect(tenderHint(1, 0.28, 0.3)).toBe("More than the model can deliver: it runs at about 30% a year on average over every future, a tender every year where the company ends at or above its last round's price and none elsewhere. One comes before the outcome in about 28 of 100 futures.");
    expect(tenderHint(0.04, null, 0.04)).toMatch(/^An average over every future/);
  });
});

describe('equitySummary', () => {
  it('leads with how often it pays nothing, and falls back to the paper value', () => {
    const p = startup();
    const k = Math.round(equityPreview(p.private_equity[0]!).nothing_share * 100);
    expect(equitySummary(p)).toBe(`Northwind Robotics: 122,000 options, RSUs, and shares, nothing in ${k} of 100 futures`);
    expect(equitySummary({ ...p, private_equity: [] })).toBe('None');
    const excluded = { ...p.private_equity[0]!, included: false };
    expect(equitySummary({ ...p, private_equity: [excluded] })).toMatch(/futures \(not counted\)$/);
    expect(equitySummary({ ...p, private_equity: [p.private_equity[0]!, { ...excluded, id: 'b' }] })).toBe('2 companies');
    expect(equitySummary({ ...p, private_equity: [excluded, { ...excluded, id: 'b' }] })).toBe('2 companies (not counted)');
    // No holdings: no preview to lead with.
    expect(equitySummary({ ...p, private_equity: [{ ...p.private_equity[0]!, holdings: [] }] })).toBe("Northwind Robotics: $0 on paper at the last round's price");
    // The three older demos have no companies.
    for (const e of exampleProfiles().filter((x) => x.slug !== 'startup-employee')) expect(equitySummary(e.profile)).toBe('None');
  });
});
