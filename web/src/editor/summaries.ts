// One-line summaries shown on collapsed editor sections.
import { paperValue } from '@/engine/equity-math';
import type { Profile } from '@/engine/types';
import { moneyCompact, percent, yearsBetween, currentYearMonth } from '@/ui';
import { equityShare, toPercents } from './allocation';
import { holdingsNoun } from './equity/text';
import { canShowPayoffs, previewOf } from './equity/usePreview';
import { annualIncome, budgetTotals } from './labels';

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

export function householdSummary(p: Profile): string {
  const start = p.start ?? currentYearMonth();
  const people = p.household.people.map((x) => `${x.name || 'Person'}, ${yearsBetween(x.birth, start)}`);
  const kids = p.household.children.length;
  const loc = p.household.location;
  const where = [loc.city, loc.country === 'US' ? loc.state : loc.country].filter(Boolean).join(', ');
  return [people.join(' and ') || 'No one yet', kids ? plural(kids, 'child', 'children') : null, where || null].filter(Boolean).join(' · ');
}

export function incomeSummary(p: Profile): string {
  if (p.incomes.length === 0) return 'No income';
  return `${moneyCompact(annualIncome(p))}/yr from ${plural(p.incomes.length, 'source')}`;
}

/**
 * Leads with how often the position pays nothing whenever the engine can say; the paper
 * value is the most flattering number available and is shown only without a preview.
 */
export function equitySummary(p: Profile): string {
  const companies = p.private_equity ?? [];
  if (companies.length === 0) return 'None';
  const uncounted = companies.every((c) => !c.included) ? ' (not counted)' : '';
  const [only] = companies;
  if (companies.length > 1 || !only) return `${companies.length} companies${uncounted}`;
  const name = only.name || 'Company';
  const preview = canShowPayoffs(only) ? previewOf(only) : null;
  if (preview) return `${name}: ${holdingsNoun(only)}, nothing in ${Math.round(preview.nothing_share * 100)} of 100 futures${uncounted}`;
  return `${name}: ${moneyCompact(paperValue(only).total)} on paper at the last round's price${uncounted}`;
}

export function spendingSummary(p: Profile): string {
  const t = budgetTotals(p);
  if (p.spending.items.length === 0) return 'No budget lines';
  return `${moneyCompact(t.now)}/yr now · ${moneyCompact(t.retired)}/yr in retirement`;
}

export function accountsSummary(p: Profile): string {
  const total = p.accounts.reduce((n, a) => n + a.balance, 0);
  const weights = toPercents(p.portfolio.target);
  const mix = weights.length ? `${equityShare(weights)}% growth assets` : 'no target mix';
  return p.accounts.length === 0 ? `No accounts · ${mix}` : `${moneyCompact(total)} in ${plural(p.accounts.length, 'account')} · ${mix}`;
}

export function propertySummary(p: Profile): string {
  const value = p.properties.reduce((n, x) => n + x.value, 0);
  const mortgages = p.properties.reduce((n, x) => n + (x.mortgage?.balance ?? 0), 0);
  const debts = p.debts.reduce((n, d) => n + d.balance, 0);
  const parts = [
    p.properties.length ? `${plural(p.properties.length, 'property', 'properties')} worth ${moneyCompact(value)}` : 'No property',
    mortgages + debts > 0 ? `${moneyCompact(mortgages + debts)} owed` : 'no debt',
  ];
  return parts.join(' · ');
}

export function eventsSummary(p: Profile): string {
  if (p.events.length === 0) return 'Nothing planned';
  const names = p.events.slice(0, 2).map((e) => e.name || e.kind.type.replace(/_/g, ' '));
  const more = p.events.length - names.length;
  return names.join(', ') + (more > 0 ? ` and ${more} more` : '');
}

export function retirementSummary(p: Profile): string {
  const t = p.retirement.trigger;
  const when =
    t.type === 'when_funded'
      ? `When funded at ${percent(t.withdrawal_rate)}`
      : t.type === 'at_age'
        ? `At age ${t.age}`
        : t.type === 'at_date'
          ? `In ${t.at.slice(0, 4)}`
          : t.type === 'fixed_number'
            ? `At ${moneyCompact(t.amount)}`
            : 'Already retired';
  const rule: Record<string, string> = {
    constant_real: 'constant spending',
    percent_of_portfolio: 'percent of portfolio',
    guardrails: 'guardrails',
    vanguard_dynamic: 'dynamic spending',
    vpw: 'variable percentage',
    floor_ceiling: 'bounded portfolio share',
  };
  return `${when} · ${rule[p.retirement.spending_rule.type]}`;
}

export function assumptionsSummary(p: Profile): string {
  const model: Record<string, string> = { bootstrap: 'History reshuffled', regime: 'Regime model', lognormal: 'Random walk', historical: 'History replayed' };
  const m = p.assumptions.market;
  const parts = [model[m.kind.type] ?? m.kind.type];
  if (m.equity_premium_adjustment !== 0) parts.push(`stocks ${m.equity_premium_adjustment > 0 ? '+' : ''}${percent(m.equity_premium_adjustment)}`);
  if (m.stress.length) parts.push(plural(m.stress.length, 'stress test'));
  parts.push(p.assumptions.taxes.kind.type === 'us_current_law' ? 'US tax law' : p.assumptions.taxes.kind.type === 'flat' ? 'flat tax' : 'no taxes');
  return parts.join(' · ');
}
