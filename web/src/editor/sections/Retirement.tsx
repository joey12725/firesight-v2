import type { RetirementPlan } from '@/engine/types';
import { ChoiceField, Group, MoneyField, NumField, PercentField, Row, SegmentField, SliderField, ToggleField } from '../fields';
import { usePeople, useProfile } from '../hooks';
import { MonthField } from '../MonthField';

type Trigger = RetirementPlan['trigger'];
type Rule = RetirementPlan['spending_rule'];
type Order = RetirementPlan['withdrawal_order'];

const TRIGGERS: { value: Trigger['type']; label: string; explain: string; make: () => unknown }[] = [
  {
    value: 'when_funded',
    label: 'When the plan is funded',
    explain: 'Each simulated life retires as soon as its savings can cover planned spending at the withdrawal rate. Lucky lives retire early, unlucky ones later.',
    make: () => ({ type: 'when_funded' }),
  },
  {
    value: 'at_age',
    label: 'At an age',
    explain: 'Everyone stops at the same age whatever the markets did. The question becomes whether the money lasts.',
    make: () => ({ type: 'at_age', person: 0, age: 60 }),
  },
  { value: 'at_date', label: 'On a date', explain: 'Retire in a fixed month, whatever the balance is then.', make: () => ({ type: 'at_date', at: '2040-01' }) },
  {
    value: 'fixed_number',
    label: 'At a target number',
    explain: 'Retire when savings first reach an amount in today\'s dollars. Simple, but blind to what you spend.',
    make: () => ({ type: 'fixed_number', amount: 1_500_000 }),
  },
  { value: 'already_retired', label: 'Already retired', explain: 'Withdrawals start now. Job income marked as ending at retirement is ignored.', make: () => ({ type: 'already_retired' }) },
];

const RULES: { value: Rule['type']; label: string; explain: string; make: () => unknown }[] = [
  { value: 'constant_real', label: 'Constant spending', explain: 'Spend the budget every year, adjusted for inflation, regardless of the portfolio. Predictable, and the rule most likely to run out.', make: () => ({ type: 'constant_real' }) },
  {
    value: 'guardrails',
    label: 'Guardrails',
    explain: 'Start from the budget; cut spending when the withdrawal rate drifts too high and raise it when it falls well below. Small cuts in bad markets protect the plan.',
    make: () => ({ type: 'guardrails' }),
  },
  {
    value: 'vanguard_dynamic',
    label: 'Dynamic, with yearly limits',
    explain: 'Spending follows the portfolio, but the change from one year to the next is capped on the way up and on the way down.',
    make: () => ({ type: 'vanguard_dynamic' }),
  },
  { value: 'percent_of_portfolio', label: 'Percent of portfolio', explain: 'Spend a fixed share of whatever the portfolio is worth each year. Never runs out; income swings with the market.', make: () => ({ type: 'percent_of_portfolio', rate: 0.04 }) },
  { value: 'vpw', label: 'Variable percentage (VPW)', explain: 'Each year, spend the amount that would use up the portfolio exactly by the end of the plan at expected returns. Never runs out; spending varies with markets.', make: () => ({ type: 'vpw' }) },
  {
    value: 'floor_ceiling',
    label: 'Portfolio share within bounds',
    explain: 'A share of the portfolio each year, never below a floor or above a ceiling relative to the budget.',
    make: () => ({ type: 'floor_ceiling' }),
  },
];

const ORDERS: { value: Order['type']; label: string; explain: string; make: () => unknown }[] = [
  { value: 'conventional', label: 'Conventional', explain: 'Taxable accounts first, then pre-tax, then Roth. Simple; often leaves large taxable withdrawals for later.', make: () => ({ type: 'conventional' }) },
  { value: 'proportional', label: 'Proportional', explain: 'Draw from every account type in proportion to its size, which smooths taxable income across retirement.', make: () => ({ type: 'proportional' }) },
  {
    value: 'tax_aware',
    label: 'Tax-aware',
    explain: 'Fill low tax brackets on purpose each year: with pre-tax withdrawals and, optionally, Roth conversions, while watching health-insurance subsidy limits.',
    make: () => ({ type: 'tax_aware' }),
  },
];

function Explain({ children }: { children: string | undefined }) {
  return children ? <p className="text-xs leading-relaxed text-ink-2">{children}</p> : null;
}

export function RetirementSection() {
  const { retirement } = useProfile();
  const people = usePeople();
  const { trigger, spending_rule: rule, withdrawal_order: order } = retirement;
  const t = '/retirement/trigger';
  const r = '/retirement/spending_rule';
  const o = '/retirement/withdrawal_order';
  return (
    <>
      <Group title="When work stops">
        <ChoiceField path={t} label="Retire" options={TRIGGERS} read={(v) => (v as Trigger).type} write={(type) => TRIGGERS.find((x) => x.value === type)?.make()} />
        <Explain>{TRIGGERS.find((x) => x.value === trigger.type)?.explain}</Explain>
        {trigger.type === 'when_funded' && (
          <>
            <SliderField path={`${t}/withdrawal_rate`} label="Withdrawal rate that counts as funded" kind="percent" min={0.02} max={0.07} step={0.0025} />
            <Row>
              <NumField path={`${t}/min_age`} label="Not before age" kind="years" nullable placeholder="Any age" />
              <NumField path={`${t}/max_age`} label="No later than age" kind="years" />
            </Row>
            <SegmentField
              path={`${t}/basis`}
              label="Count"
              options={[
                { value: 'investable', label: 'Investable assets' },
                { value: 'net_worth', label: 'Net worth incl. home' },
              ]}
            />
            <ToggleField path={`${t}/count_future_income`} label="Count pensions and Social Security still to come" />
          </>
        )}
        {trigger.type === 'at_age' && (
          <Row>
            {people.length > 1 && <ChoiceField path={`${t}/person`} label="Whose age" options={people} read={(v) => String(v)} write={(v) => Number(v)} />}
            <NumField path={`${t}/age`} label="Age" kind="years" />
          </Row>
        )}
        {trigger.type === 'at_date' && <MonthField path={`${t}/at`} label="Month" />}
        {trigger.type === 'fixed_number' && (
          <Row>
            <MoneyField path={`${t}/amount`} label="Target" />
            <ChoiceField
              path={`${t}/basis`}
              label="Count"
              options={[
                { value: 'investable', label: 'Investable assets' },
                { value: 'net_worth', label: 'Net worth' },
              ]}
            />
          </Row>
        )}
        {people.length > 1 && trigger.type !== 'already_retired' && (
          <NumField path="/retirement/partner_offset_years" label="Second earner keeps working for" kind="years" suffix="more yrs" hint="Negative if they stop earlier." />
        )}
      </Group>

      <Group title="Spending in retirement">
        <ChoiceField path={r} label="Spending rule" options={RULES} read={(v) => (v as Rule).type} write={(type) => RULES.find((x) => x.value === type)?.make()} />
        <Explain>{RULES.find((x) => x.value === rule.type)?.explain}</Explain>
        {rule.type === 'percent_of_portfolio' && <SliderField path={`${r}/rate`} label="Share spent each year" kind="percent" min={0.02} max={0.08} step={0.0025} />}
        {rule.type === 'guardrails' && (
          <Row cols={3}>
            <PercentField path={`${r}/upper`} label="Upper rail" />
            <PercentField path={`${r}/lower`} label="Lower rail" />
            <PercentField path={`${r}/adjust`} label="Adjustment" />
          </Row>
        )}
        {rule.type === 'vanguard_dynamic' && (
          <Row>
            <PercentField path={`${r}/ceiling`} label="Most it rises in a year" />
            <PercentField path={`${r}/floor`} label="Most it falls in a year" />
          </Row>
        )}
        {rule.type === 'floor_ceiling' && (
          <Row>
            <PercentField path={`${r}/floor`} label="Floor, of budget" />
            <PercentField path={`${r}/ceiling`} label="Ceiling, of budget" />
          </Row>
        )}
      </Group>

      <Group title="Where withdrawals come from">
        <ChoiceField path={o} label="Withdrawal order" options={ORDERS} read={(v) => (v as Order).type} write={(type) => ORDERS.find((x) => x.value === type)?.make()} />
        <Explain>{ORDERS.find((x) => x.value === order.type)?.explain}</Explain>
        {order.type === 'tax_aware' && (
          <>
            <ChoiceField
              path={`${o}/fill_to_bracket`}
              label="Fill income up to the top of the"
              options={['0.1', '0.12', '0.22', '0.24', '0.32'].map((v) => ({ value: v, label: `${Math.round(Number(v) * 100)}% bracket` }))}
              read={(v) => String(v)}
              write={(v) => Number(v)}
            />
            <ToggleField path={`${o}/roth_conversions`} label="Use spare bracket room for Roth conversions" />
            <ToggleField path={`${o}/mind_aca`} label="Keep income under health-subsidy limits" />
          </>
        )}
        <SegmentField
          path="/retirement/early_access"
          label="Before age 59½"
          options={[
            { value: 'auto', label: 'Avoid penalties' },
            { value: 'pay_penalty', label: 'Pay the 10% penalty' },
          ]}
        />
      </Group>

      <Group title="Legacy">
        <MoneyField path="/retirement/bequest_target" label="Amount to leave behind" hint="In today's dollars. Zero means spending the last dollar is fine." />
      </Group>
    </>
  );
}
