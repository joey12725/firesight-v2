// The words on the Cash flow view, computed from the model. Kept apart from the
// components so they can be tested and reused as plain text for image exports.
import type { Summary } from '@/engine/output-types';
import { moneyCompact } from '@/ui';
import { quantileSeries } from '@/ui/charts';
import { handoverIndex, sharesOfUses, type CashFlowModel, type Group } from './model';

export interface Segment {
  text: string;
  strong?: boolean;
}

export const plain = (segments: Segment[]) => segments.map((s) => s.text).join('');

const ageAt = (model: CashFlowModel, i: number) => Math.floor(model.time.ages[i] ?? 0);
const value = (groups: Group[], key: string, i: number) => groups.find((g) => g.key === key)?.values[i] ?? 0;
const share = (part: number, whole: number) => (whole > 0 ? part / whole : 0);
/** Lower-cases a label for mid-sentence use, keeping proper names. */
export const lower = (label: string) => label.toLowerCase().replace(/\broth\b/g, 'Roth').replace(/\bhsa\b/g, 'HSA').replace(/social security/g, 'Social Security');
/** The index nearest an age, kept inside the years that carry flows. */
const indexNear = (model: CashFlowModel, age: number) => Math.min(model.end, Math.max(model.flowStart, model.time.indexAt(age)));

function largest(groups: Group[], i: number): Group | null {
  let best: Group | null = null;
  for (const g of groups) if ((g.values[i] ?? 0) > 0 && (!best || (g.values[i] ?? 0) > (best.values[i] ?? 0))) best = g;
  return best;
}

/** A year well into retirement for this plan: eight years after savings take over, or 70, whichever is later, if the plan gets there. */
export function retirementIndex(model: CashFlowModel): number | null {
  const h = handoverIndex(model);
  if (h === null) return null;
  const i = Math.max(h + 8, indexNear(model, 70));
  return i <= model.end - 1 ? i : Math.min(model.end, h + 2);
}

/** The view's opening sentence: who pays for this life, and when that changes. */
export function leadSentence(model: CashFlowModel): Segment[] {
  const i0 = model.flowStart;
  const total = model.totalIn[i0] ?? 0;
  const top = largest(model.moneyIn, i0);
  if (!top || total <= 0) return [{ text: 'No money moves in this plan.' }];
  const out: Segment[] = [
    { text: 'At ' },
    { text: String(ageAt(model, i0)), strong: true },
    { text: `, ${Math.round(share(top.values[i0] ?? 0, total) * 100)} of every 100 dollars coming in are ` },
    { text: lower(top.label), strong: true },
    { text: '.' },
  ];
  const h = handoverIndex(model);
  if (h === null) {
    out.push({ text: ' Work pays for most of this life: withdrawals from savings never overtake earnings.' });
    return out;
  }
  const r = retirementIndex(model) ?? h;
  out.push({ text: ' From ' }, { text: String(ageAt(model, h)), strong: true }, { text: ` savings take over: by ${ageAt(model, r)} withdrawals supply ` }, { text: String(Math.round(share(value(model.moneyIn, 'withdrawals', r), model.totalIn[r] ?? 0) * 100)), strong: true });
  const ss = share(value(model.moneyIn, 'social_security', r), model.totalIn[r] ?? 0);
  if (ss >= 0.05) out.push({ text: ' and Social Security ' }, { text: String(Math.round(ss * 100)), strong: true });
  out.push({ text: ' of every 100.' });
  return out;
}

/** Added to the caption of the money-in chart when startup equity is one of its bands. */
export const EQUITY_CAPTION = 'Startup equity is an average over lives in which it paid and lives in which it did not; the Equity view shows how likely each is.';

export function flowsTakeaway(model: CashFlowModel): string {
  const i0 = model.flowStart;
  const now = sharesOfUses(model, i0);
  if (now.total <= 0) return 'No money moves in this plan.';
  const first = `At ${ageAt(model, i0)}, of every $100 that comes in, $${Math.round(now.spent * 100)} is spent, $${Math.round(now.taxed * 100)} goes to tax and $${Math.round(now.saved * 100)} is saved.`;
  const r = retirementIndex(model);
  if (r === null) return first;
  const then = sharesOfUses(model, r);
  const change = (model.totalOut[r] ?? 0) / Math.max(1, model.totalOut[i0] ?? 1);
  const size = change < 0.9 || change > 1.1 ? `with ${Math.round(change * 100)}% as much money moving` : 'with about as much money moving';
  return `${first} At ${ageAt(model, r)}, ${size}, $${Math.round(then.spent * 100)} is spent and $${Math.round(then.taxed * 100)} taxed.`;
}

export function yearTakeaway(model: CashFlowModel, i: number): string {
  const total = model.totalIn[i] ?? 0;
  if (total <= 0) return `Nothing comes in at ${ageAt(model, i)}.`;
  const ranked = model.moneyIn.filter((g) => (g.values[i] ?? 0) > 0).sort((a, b) => (b.values[i] ?? 0) - (a.values[i] ?? 0));
  const top = ranked[0];
  const second = ranked[1];
  const u = sharesOfUses(model, i);
  const pct = (g: Group) => Math.round(share(g.values[i] ?? 0, total) * 100);
  const sources = top ? `${lower(top.label)} (${pct(top)}%)${second && pct(second) >= 10 ? ` and ${lower(second.label)} (${pct(second)}%)` : ''}` : 'nowhere';
  const saved = u.saved >= 0.02 ? ` and $${Math.round(u.saved * 100)} saved` : '';
  return `At ${ageAt(model, i)} an average of ${moneyCompact(total)} comes in, mostly from ${sources}. Of every $100, $${Math.round(u.spent * 100)} is spent, $${Math.round(u.taxed * 100)} taxed${saved}.`;
}

export function balanceTakeaway(model: CashFlowModel, summary: Summary): string {
  const ages = model.time.ages;
  const at = (ages[model.end] ?? 0) >= 86 ? indexNear(model, 85) : Math.max(0, model.end - 2);
  const mean = model.netWorth[at] ?? 0;
  const median = quantileSeries(summary, 'net_worth', 0.5)[at];
  const biggest = largest(model.assets, at);
  const assets = model.assets.reduce((t, g) => t + (g.values[at] ?? 0), 0);
  if (assets <= 0) return 'This plan holds no assets.';
  const hold = biggest ? `, ${Math.round(share(biggest.values[at] ?? 0, assets) * 100)}% of it in ${lower(biggest.label)}` : '';
  const pulled =
    median === null || median === undefined ? '' : mean > median * 1.15 ? ` The middle life has ${moneyCompact(median)}: the average is pulled up by the luckiest lives.` : ` The middle life has ${moneyCompact(median)}.`;
  const owed = model.liabilities.reduce((t, g) => t + (g.values[at] ?? 0), 0);
  const worth = owed >= assets * 0.005 ? `, and is worth ${moneyCompact(mean)} after debts` : ' and owes nothing';
  return `At ${ageAt(model, at)} the average household owns ${moneyCompact(assets)}${hold}${worth}.${pulled}`;
}

export function accountsTakeaway(model: CashFlowModel): string {
  const total = (groups: Group[], from: number, to: number) => groups.map((g) => ({ g, sum: g.values.slice(from, to + 1).reduce((t, v) => t + v, 0) })).sort((a, b) => b.sum - a.sum);
  const h = handoverIndex(model);
  const saveEnd = h === null ? model.end : Math.max(model.flowStart, h - 1);
  const saved = total(model.savingBy, model.flowStart, saveEnd);
  const savedAll = saved.reduce((t, x) => t + x.sum, 0);
  const s0 = saved[0];
  const saving = s0 && savedAll > 0 ? `While working, ${Math.round((s0.sum / savedAll) * 100)} of every 100 dollars saved go into ${lower(s0.g.label)}.` : 'Nothing is saved in this plan.';
  if (h === null || model.withdrawBy.length === 0) return saving;
  const early = total(model.withdrawBy, h, Math.min(model.end, h + 9));
  const late = total(model.withdrawBy, Math.min(model.end, h + 20), model.end);
  const e0 = early[0];
  const l0 = late[0];
  if (!e0 || e0.sum <= 0) return saving;
  const later = l0 && l0.sum > 0 && l0.g.key !== e0.g.key && h + 20 < model.end ? `; from about ${ageAt(model, h + 20)} they come mostly from ${lower(l0.g.label)}` : '';
  return `${saving} The first ten years of withdrawals lean on ${lower(e0.g.label)}${later}.`;
}

export function taxTakeaway(model: CashFlowModel, summary: Summary): string {
  let peak = model.flowStart;
  for (let i = model.flowStart; i <= model.end; i++) if ((model.taxShare[i] ?? 0) > (model.taxShare[peak] ?? 0) && (model.workingShare[i] ?? 0) >= 0.5) peak = i;
  const r = retirementIndex(model);
  const peakShare = model.taxShare[peak] ?? 0;
  if (peakShare <= 0) return 'No tax is paid in this plan.';
  const lead = `While working, tax takes most at ${ageAt(model, peak)}: $${Math.round(peakShare * 100)} of every $100 coming in`;
  const retired = r !== null && model.taxShare[r] !== null && model.taxShare[r] !== undefined ? `, against $${Math.round((model.taxShare[r] ?? 0) * 100)} at ${ageAt(model, r)}` : '';
  const t = summary.headline.lifetime_tax;
  const life = t.p50 !== null ? ` Over a lifetime the middle life pays ${moneyCompact(t.p50)}${t.p10 !== null && t.p90 !== null ? `; 8 in 10 pay between ${moneyCompact(t.p10)} and ${moneyCompact(t.p90)}` : ''}.` : '';
  return `${lead}${retired}.${life}`;
}

export function ratesTakeaway(model: CashFlowModel, initial?: { p10: number | null; p50: number | null; p90: number | null }): string {
  const saving: number[] = [];
  model.savingRate.forEach((v, i) => {
    if (v !== null && (model.workingShare[i] ?? 0) >= 0.5) saving.push(v);
  });
  const mean = saving.length ? saving.reduce((t, v) => t + v, 0) / saving.length : null;
  const h = handoverIndex(model);
  const lead = mean === null ? 'There are no working years to save in.' : `While most lives are still working, ${Math.round(mean * 100)} of every 100 dollars earned are saved.`;
  if (h === null) return lead;
  const e = Math.min(model.end, h + 2);
  const early = model.withdrawalRate[e];
  if (early === null || early === undefined) return lead;
  const r = retirementIndex(model);
  const l = r === null ? null : Math.min(model.end, r + 10);
  const late = l === null ? null : model.withdrawalRate[l];
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;
  const then = l !== null && l > e + 3 && late !== null && late !== undefined ? `, and ${pct(late)} at ${ageAt(model, l)}` : '';
  const first = initial && initial.p50 !== null ? ` The middle retired life starts on ${pct(initial.p50)}${initial.p10 !== null && initial.p90 !== null ? ` (${pct(initial.p10)} to ${pct(initial.p90)} in 8 of 10)` : ''}.` : '';
  return `${lead} At ${ageAt(model, e)}, early in retirement, the average household draws ${pct(early)} of its savings in a year${then}.${first}`;
}
