import type { EquityGrant, Income, Job, Person } from '@/engine/types';
import { remove, set } from '@/state/patches';
import { at, join } from '@/state/pointer';
import { Disclosure, Field, Switch, moneyCompact, percent } from '@/ui';
import { ChoiceField, Group, MoneyField, NumField, OwnerField, PercentField, Row, SegmentField, SliderField, ToggleField } from '../fields';
import { useEdit, useHelp, useProfile } from '../hooks';
import { AddButton, Item, ItemList } from '../Section';
import { useAdd } from '../useAdd';
import { WhenField, describeWhen } from '../WhenField';

const RISK = [
  { value: 'none', label: 'None' },
  { value: 'low', label: 'Low' },
  { value: 'typical', label: 'Typical' },
  { value: 'high', label: 'High' },
] as const;

const GROWTH = [
  { value: 'lifecycle', label: 'Career curve' },
  { value: 'flat', label: 'Fixed raise' },
] as const;

const CURVE = [
  { value: 'flat', label: 'Flat' },
  { value: 'typical', label: 'Typical' },
  { value: 'steep', label: 'Steep' },
] as const;

/** A sub-object that is either present or null, with a switch to add or remove it. */
function Optional({ path, label, present, make, children }: { path: string; label: string; present: boolean; make: () => unknown; children: React.ReactNode }) {
  const edit = useEdit();
  const help = useHelp(path);
  return (
    <div data-path={path} className="space-y-3">
      <Field inline label={label} help={help}>
        {({ id }) => <Switch id={id} checked={present} onChange={(on) => edit(set(path, on ? make() : null), { label: `${on ? 'Add' : 'Remove'} ${label.toLowerCase()}` })} />}
      </Field>
      {present && <div className="space-y-3 border-l border-line pl-3">{children}</div>}
    </div>
  );
}

function EquityGrants({ path, grants }: { path: string; grants: EquityGrant[] }) {
  const add = useAdd(path);
  return (
    <Group title="Equity grants">
      <ItemList empty="No stock or option grants.">
        {grants.map((g, i) => {
          const p = join(path, at(g, i));
          return (
            <Item key={g.id || i} path={p} nameKey={null} fallbackName={g.kind.type === 'rsu' ? 'RSU grant' : 'Option grant'} figure={moneyCompact(g.unvested_value)} detail="unvested">
              <Row>
                <ChoiceField
                  path={`${p}/kind`}
                  label="Type"
                  options={[
                    { value: 'rsu', label: 'RSUs' },
                    { value: 'option', label: 'Stock options' },
                  ]}
                  read={(v) => (v as EquityGrant['kind']).type}
                  write={(type) => (type === 'option' ? { type, strike_fraction: 0.5 } : { type })}
                />
                <ChoiceField
                  path={`${p}/company`}
                  label="Company"
                  options={[
                    { value: 'public', label: 'Public' },
                    { value: 'private', label: 'Private' },
                  ]}
                  read={(v) => (v as EquityGrant['company']).type}
                  write={(type) => ({ type })}
                />
              </Row>
              <Row>
                <MoneyField path={`${p}/unvested_value`} label="Unvested value today" />
                <MoneyField path={`${p}/refresh_annual_value`} label="New grants each year" suffix="/yr" />
              </Row>
              <Row>
                <NumField path={`${p}/vest_months_remaining`} label="Months left to vest" kind="months" suffix="mo" />
                <NumField path={`${p}/cliff_months`} label="Cliff" kind="months" suffix="mo" />
              </Row>
              {g.kind.type === 'option' && <PercentField path={`${p}/kind/strike_fraction`} label="Strike, as a share of today's price" />}
              {g.company.type === 'public' ? (
                <Row>
                  <PercentField path={`${p}/company/volatility`} label="Volatility" />
                  <NumField path={`${p}/company/beta`} label="Beta" step={0.1} />
                </Row>
              ) : (
                <Row cols={3}>
                  <PercentField path={`${p}/company/liquidity_hazard`} label="Exit chance / yr" />
                  <PercentField path={`${p}/company/fail_prob`} label="Fails" />
                  <PercentField path={`${p}/company/exit_volatility`} label="Exit volatility" />
                </Row>
              )}
              <ToggleField path={`${p}/sell_on_vest`} label="Sell shares as they vest" />
            </Item>
          );
        })}
      </ItemList>
      <AddButton label="Add a grant" onAdd={() => add('grant', { unvested_value: 50_000 }, { unnamed: true })} />
    </Group>
  );
}

function JobFields({ path, job }: { path: string; job: Job }) {
  const k = `${path}/kind`;
  return (
    <>
      <MoneyField path={`${k}/salary`} label="Salary" suffix="/yr" />
      <SegmentField path={`${k}/risk/level`} label="Career risk" options={RISK} help={useHelp(`${k}/risk`)} />
      <SegmentField
        path={`${k}/growth`}
        label="Pay growth"
        options={GROWTH}
        read={(v) => (v as Job['growth']).type}
        write={(type) => (type === 'flat' ? { type, real_raise: 0.01 } : { type, profile: 'typical', extra: 0 })}
      />
      {job.growth.type === 'flat' ? (
        <PercentField path={`${k}/growth/real_raise`} label="Raise above inflation, per year" />
      ) : (
        <Row>
          <SegmentField path={`${k}/growth/profile`} label="Curve" options={CURVE} />
          <PercentField path={`${k}/growth/extra`} label="Extra per year" />
        </Row>
      )}

      <Optional path={`${k}/plan`} label="Workplace retirement plan" present={job.plan !== null} make={() => ({ employee_fraction: 0.06, match_rate: 0.5, match_up_to: 0.06 })}>
        <ToggleField path={`${k}/plan/employee_max`} label="Contribute the legal maximum" />
        {job.plan && !job.plan.employee_max && <SliderField path={`${k}/plan/employee_fraction`} label="You contribute" kind="percent" min={0} max={0.3} step={0.005} />}
        <Row cols={3}>
          <PercentField path={`${k}/plan/roth_fraction`} label="Roth share" />
          <PercentField path={`${k}/plan/match_rate`} label="Match rate" />
          <PercentField path={`${k}/plan/match_up_to`} label="Up to" />
        </Row>
        <ToggleField path={`${k}/plan/after_tax_mega`} label="After-tax contributions (mega backdoor)" />
      </Optional>

      <Optional path={`${k}/bonus`} label="Bonus" present={job.bonus !== null} make={() => ({ target_fraction: 0.1 })}>
        <Row cols={3}>
          <PercentField path={`${k}/bonus/target_fraction`} label="Target" />
          <PercentField path={`${k}/bonus/volatility`} label="Variability" />
          <NumField path={`${k}/bonus/month`} label="Paid in month" kind="integer" min={1} max={12} />
        </Row>
      </Optional>

      <Optional path={`${k}/commission`} label="Commission" present={job.commission !== null} make={() => ({ expected_annual: 10_000 })}>
        <Row>
          <MoneyField path={`${k}/commission/expected_annual`} label="Expected" suffix="/yr" />
          <PercentField path={`${k}/commission/volatility`} label="Variability" />
        </Row>
      </Optional>

      <EquityGrants path={`${k}/equity`} grants={job.equity} />

      <Disclosure summary="HSA and timing">
        <div className="space-y-3">
          <Row>
            <MoneyField path={`${k}/hsa_contribution`} label="Your HSA contribution" suffix="/yr" />
            <MoneyField path={`${k}/hsa_employer`} label="Employer HSA" suffix="/yr" />
          </Row>
          <ToggleField path={`${k}/ends_at_retirement`} label="Ends when you retire" />
          <Row>
            <WhenField path={`${path}/start`} label="Starts" unsetLabel="Already started" />
            <WhenField path={`${path}/end`} label="Ends" unsetLabel="No fixed end" />
          </Row>
        </div>
      </Disclosure>
    </>
  );
}

function Timing({ path, endsAtRetirement }: { path: string; endsAtRetirement?: boolean }) {
  return (
    <>
      {endsAtRetirement && <ToggleField path={`${path}/kind/ends_at_retirement`} label="Ends when you retire" />}
      <Row>
        <WhenField path={`${path}/start`} label="Starts" unsetLabel="Already started" />
        <WhenField path={`${path}/end`} label="Ends" unsetLabel="No fixed end" />
      </Row>
    </>
  );
}

function IncomeFields({ path, income }: { path: string; income: Income }) {
  const k = `${path}/kind`;
  const kind = income.kind;
  switch (kind.type) {
    case 'job':
      return <JobFields path={path} job={kind} />;
    case 'self_employment':
      return (
        <>
          <Row>
            <MoneyField path={`${k}/net_annual`} label="Net income" suffix="/yr" />
            <PercentField path={`${k}/volatility`} label="Year-to-year variability" />
          </Row>
          <Row>
            <PercentField path={`${k}/real_growth`} label="Growth above inflation" />
            <PercentField path={`${k}/solo_401k_fraction`} label="Saved in a solo 401(k)" />
          </Row>
          <Timing path={path} endsAtRetirement />
        </>
      );
    case 'pension':
      return (
        <>
          <Row>
            <MoneyField path={`${k}/annual`} label="Benefit" suffix="/yr" />
            <PercentField path={`${k}/survivor_fraction`} label="Survivor receives" />
          </Row>
          <ChoiceField
            path={`${k}/cola`}
            label="Cost-of-living adjustment"
            options={[
              { value: 'full', label: 'Keeps up with inflation' },
              { value: 'partial', label: 'Fixed yearly increase' },
              { value: 'none', label: 'None (loses value over time)' },
            ]}
            read={(v) => (v as typeof kind.cola).type}
            write={(type) => (type === 'partial' ? { type, rate: 0.02 } : { type })}
          />
          {kind.cola.type === 'partial' && <PercentField path={`${k}/cola/rate`} label="Yearly increase" />}
          <Timing path={path} />
        </>
      );
    case 'annuity':
      return (
        <>
          <MoneyField path={`${k}/annual`} label="Payment" suffix="/yr" />
          <ToggleField path={`${k}/real`} label="Rises with inflation" />
          <Timing path={path} />
        </>
      );
    case 'other':
      return (
        <>
          <Row>
            <MoneyField path={`${k}/annual`} label="Amount" suffix="/yr" />
            <PercentField path={`${k}/real_growth`} label="Growth above inflation" />
          </Row>
          <ToggleField path={`${k}/taxable`} label="Taxable" />
          <Timing path={path} endsAtRetirement />
        </>
      );
  }
}

const KIND_LABEL: Record<Income['kind']['type'], string> = {
  job: 'Job',
  self_employment: 'Self-employment',
  pension: 'Pension',
  annuity: 'Annuity',
  other: 'Other income',
};

function headline(income: Income): number {
  const k = income.kind;
  return k.type === 'job' ? k.salary : k.type === 'self_employment' ? k.net_annual : k.annual;
}

function SocialSecurityFields({ person, index }: { person: Person; index: number }) {
  const path = `${join('/household/people', at(person, index))}/social_security`;
  const basis = person.social_security.basis;
  return (
    <div className="space-y-3">
      <SliderField path={`${path}/claim_age`} label={`${person.name || `Person ${index + 1}`}: claim at age`} kind="years" min={62} max={70} step={1} />
      {basis.type !== 'none' && (
        <Row>
          <ChoiceField
            path={`${path}/basis`}
            label="Benefit estimate"
            options={[
              { value: 'from_earnings', label: 'From my earnings' },
              { value: 'known_benefit', label: 'From my SSA statement' },
              { value: 'none', label: 'No benefit' },
            ]}
            read={(v) => (v as typeof basis).type}
            write={(type) => (type === 'known_benefit' ? { type, monthly_at_fra: 2000 } : { type })}
          />
          {basis.type === 'from_earnings' ? (
            <NumField path={`${path}/basis/career_start_age`} label="Started working at" kind="years" suffix="yrs old" />
          ) : (
            <MoneyField path={`${path}/basis/monthly_at_fra`} label="Monthly at full age" suffix="/mo" />
          )}
        </Row>
      )}
      {basis.type === 'none' && (
        <ChoiceField
          path={`${path}/basis`}
          label="Benefit estimate"
          options={[
            { value: 'from_earnings', label: 'From my earnings' },
            { value: 'known_benefit', label: 'From my SSA statement' },
            { value: 'none', label: 'No benefit' },
          ]}
          read={(v) => (v as typeof basis).type}
          write={(type) => (type === 'known_benefit' ? { type, monthly_at_fra: 2000 } : { type })}
        />
      )}
    </div>
  );
}

export function IncomeSection() {
  const profile = useProfile();
  const add = useAdd('/incomes');
  const edit = useEdit();
  const names = profile.household.people.map((p) => p.name);
  const haircut = profile.assumptions.taxes.social_security_haircut;
  return (
    <>
      <Group title="Income sources">
        <ItemList empty="No income yet. Add a job, a business, or a pension.">
          {profile.incomes.map((income, i) => {
            const path = join('/incomes', at(income, i));
            const timing = [income.start && `from ${describeWhen(income.start, names)}`, income.end && `until ${describeWhen(income.end, names)}`].filter(Boolean).join(', ');
            return (
              <Item
                key={income.id || i}
                path={path}
                fallbackName={KIND_LABEL[income.kind.type]}
                figure={`${moneyCompact(headline(income))}/yr`}
                detail={[KIND_LABEL[income.kind.type], names.length > 1 ? names[income.owner] : null, timing].filter(Boolean).join(' · ')}
              >
                <OwnerField path={`${path}/owner`} label="Whose income" />
                <IncomeFields path={path} income={income} />
              </Item>
            );
          })}
        </ItemList>
        <div className="flex flex-wrap gap-x-1">
          <AddButton label="Job" onAdd={() => add('New job', { kind: { type: 'job', salary: 80_000 } })} />
          <AddButton label="Self-employment" onAdd={() => add('Self-employment', { kind: { type: 'self_employment', net_annual: 40_000 } })} />
          <AddButton label="Pension" onAdd={() => add('Pension', { kind: { type: 'pension', annual: 24_000 }, start: { type: 'age', person: 0, age: 65 } })} />
          <AddButton label="Annuity" onAdd={() => add('Annuity', { kind: { type: 'annuity', annual: 12_000 }, start: { type: 'age', person: 0, age: 65 } })} />
          <AddButton label="Other" onAdd={() => add('Other income', { kind: { type: 'other', annual: 6_000 } })} />
        </div>
      </Group>

      <Group title="Social Security">
        {profile.household.people.map((p, i) => (
          <SocialSecurityFields key={p.id || i} person={p} index={i} />
        ))}
        <div data-path="/assumptions/taxes/social_security_haircut" className="space-y-3">
          <Field inline label="Assume benefits are cut in future" help={useHelp('/assumptions/taxes/social_security_haircut')}>
            {({ id }) => (
              <Switch
                id={id}
                checked={haircut !== null}
                onChange={(on) => edit(on ? set('/assumptions/taxes/social_security_haircut', { from_year: 2034, factor: 0.79 }) : remove('/assumptions/taxes/social_security_haircut'))}
              />
            )}
          </Field>
          {haircut && (
            <Row>
              <NumField path="/assumptions/taxes/social_security_haircut/from_year" label="From year" kind="integer" />
              <PercentField path="/assumptions/taxes/social_security_haircut/factor" label="Share still paid" hint={`A ${percent(1 - haircut.factor, 0)} cut.`} />
            </Row>
          )}
        </div>
      </Group>
    </>
  );
}

