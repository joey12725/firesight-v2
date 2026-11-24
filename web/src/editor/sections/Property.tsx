import type { Debt, Property } from '@/engine/types';
import { set } from '@/state/patches';
import { at, join } from '@/state/pointer';
import { Disclosure, Field, Switch, moneyCompact, percent } from '@/ui';
import { ChoiceField, Group, MoneyField, NumField, Optional, PercentField, Realism, Row, SegmentField, ToggleField } from '../fields';
import { useEdit, useHelp, useProfile, useValue } from '../hooks';
import { MAKE, debtPolicyRealism, propertyRealism } from '../realism';
import { AddButton, Item, ItemList } from '../Section';
import { useAdd } from '../useAdd';

const USE = [
  { value: 'primary', label: 'Home' },
  { value: 'rental', label: 'Rental' },
  { value: 'vacation', label: 'Vacation' },
] as const;

type CostModel = Pick<Property, 'maintenance' | 'assessment' | 'insurance_real_growth'>;

/** Running costs and price behaviour shared by owned properties and planned purchases. */
export function PropertyCostFields({ path }: { path: string }) {
  const costs = useValue<CostModel>(path);
  return (
    <div className="space-y-3">
      <Row cols={3}>
        <PercentField path={`${path}/property_tax_rate`} label="Property tax" nullable placeholder="State avg" step={0.001} />
        <PercentField path={`${path}/insurance_rate`} label="Insurance" step={0.001} />
        <PercentField path={`${path}/maintenance_rate`} label="Upkeep" step={0.001} />
      </Row>
      <Row cols={3}>
        <MoneyField path={`${path}/hoa_monthly`} label="HOA" suffix="/mo" />
        <PercentField path={`${path}/appreciation_vs_national`} label="vs national" step={0.005} />
        <PercentField path={`${path}/idiosyncratic_volatility`} label="Own volatility" />
      </Row>
      <Realism summary="Lumpy repairs, assessment caps and insurance repricing" on={propertyRealism(costs)}>
        <SegmentField
          path={`${path}/maintenance`}
          label="Upkeep arrives as"
          options={[
            { value: 'smooth', label: 'A smooth cost' },
            { value: 'lumpy', label: 'Major repairs' },
          ]}
        />
        <Row>
          <ChoiceField
            path={`${path}/assessment`}
            label="Tax assessed on"
            options={[
              { value: 'market', label: 'Market value' },
              { value: 'capped', label: 'A capped value' },
            ]}
            read={(v) => (v as CostModel['assessment']).type}
            write={MAKE.assessment}
          />
          {costs.assessment.type === 'capped' && <PercentField path={`${path}/assessment/annual_cap`} label="Yearly cap" step={0.005} />}
        </Row>
        {costs.assessment.type === 'capped' && <MoneyField path={`${path}/assessment/assessed_value`} label="Assessed value today" nullable placeholder="Value or purchase price" />}
        <SegmentField
          path={`${path}/insurance_real_growth`}
          label="Insurance premiums"
          options={[
            { value: 'fixed', label: 'Follow the value' },
            { value: 'stochastic', label: 'Drift and reprice' },
          ]}
        />
      </Realism>
    </div>
  );
}

type RentalUse = Extract<Property['use'], { type: 'rental' }>;
type MortgageKind = NonNullable<Property['mortgage']>['kind'];

/** Fixed or adjustable rate, at `path` (an existing mortgage's `kind`, or a purchase's `mortgage_kind`). */
export function MortgageKindFields({ path }: { path: string }) {
  const kind = useValue<MortgageKind>(path);
  return (
    <>
      <SegmentField
        path={path}
        label="Rate type"
        options={[
          { value: 'fixed', label: 'Fixed' },
          { value: 'arm', label: 'Adjustable (ARM)' },
        ]}
        read={(v) => (v as MortgageKind).type}
        write={MAKE.mortgageKind}
      />
      {kind.type === 'arm' && (
        <>
          <Row>
            <NumField path={`${path}/fixed_months`} label="Fixed for" kind="months" suffix="mo" />
            <NumField path={`${path}/adjust_every_months`} label="Then resets every" kind="months" suffix="mo" />
          </Row>
          <Row cols={3}>
            <PercentField path={`${path}/margin`} label="Margin" step={0.00125} />
            <PercentField path={`${path}/periodic_cap`} label="Cap per reset" />
            <PercentField path={`${path}/lifetime_cap`} label="Lifetime cap" />
          </Row>
        </>
      )}
    </>
  );
}

export function RentalFields({ path }: { path: string }) {
  const use = useValue<RentalUse>(path);
  return (
    <div className="space-y-3">
      <Row>
        <MoneyField path={`${path}/monthly_rent`} label="Rent" suffix="/mo" />
        <PercentField path={`${path}/vacancy`} label="Vacancy" />
      </Row>
      <Row cols={3}>
        <PercentField path={`${path}/management_fee`} label="Management" />
        <PercentField path={`${path}/rent_real_growth`} label="Rent growth" />
        <PercentField path={`${path}/land_fraction`} label="Land share" />
      </Row>
      <NumField path={`${path}/years_in_service`} label="Years already rented out" kind="years" suffix="yrs" />
      <Realism summary="Vacancy spells, market rents and tenant costs" on={use.dynamics.type === 'realistic' ? 1 : 0}>
        <SegmentField
          path={`${path}/dynamics`}
          label="Tenants and rents"
          options={[
            { value: 'simple', label: 'Simple' },
            { value: 'realistic', label: 'Realistic' },
          ]}
          read={(v) => (v as RentalUse['dynamics']).type}
          write={(type) => ({ type })}
        />
        {use.dynamics.type === 'realistic' && <PercentField path={`${path}/dynamics/annual_turnover`} label="Tenancies ending each year" />}
      </Realism>
    </div>
  );
}

function PropertyItem({ property, index }: { property: Property; index: number }) {
  const path = join('/properties', at(property, index));
  const edit = useEdit();
  const mortgageHelp = useHelp(`${path}/mortgage`);
  const equity = property.value - (property.mortgage?.balance ?? 0);
  return (
    <Item path={path} fallbackName="Property" figure={moneyCompact(property.value)} detail={`${USE.find((u) => u.value === property.use.type)?.label} · ${moneyCompact(equity)} equity`}>
      <SegmentField path={`${path}/use`} label="Use" options={USE} read={(v) => (v as Property['use']).type} write={(type) => (type === 'rental' ? { type, monthly_rent: 2000 } : { type })} />
      <Row>
        <MoneyField path={`${path}/value`} label="Value today" />
        <MoneyField path={`${path}/basis`} label="Purchase price (basis)" nullable placeholder="Same as value" />
      </Row>
      {property.use.type === 'rental' && <RentalFields path={`${path}/use`} />}
      <div data-path={`${path}/mortgage`} className="space-y-3">
        <Field inline label="Has a mortgage" help={mortgageHelp}>
          {({ id }) => (
            <Switch
              id={id}
              checked={property.mortgage !== null}
              onChange={(on) => edit(set(`${path}/mortgage`, on ? { balance: Math.round(property.value * 0.7), rate: 0.065, remaining_months: 360 } : null))}
            />
          )}
        </Field>
        {property.mortgage && (
          <div className="space-y-3 border-l border-line pl-3">
            <Row>
              <MoneyField path={`${path}/mortgage/balance`} label="Balance" />
              <PercentField path={`${path}/mortgage/rate`} label="Rate" step={0.00125} />
            </Row>
            <Row>
              <NumField path={`${path}/mortgage/remaining_months`} label="Time left" kind="months" suffix="mo" />
              <MoneyField path={`${path}/mortgage/payment`} label="Payment (P&I)" nullable placeholder="Computed" suffix="/mo" />
            </Row>
            <MortgageKindFields path={`${path}/mortgage/kind`} />
          </div>
        )}
      </div>
      <Disclosure summary="Running costs and appreciation">
        <PropertyCostFields path={path} />
      </Disclosure>
    </Item>
  );
}

const DEBT_KINDS = [
  { value: 'student', label: 'Student loan' },
  { value: 'auto', label: 'Car loan' },
  { value: 'credit_card', label: 'Credit card' },
  { value: 'personal', label: 'Personal loan' },
  { value: 'other', label: 'Other' },
] as const;

function DebtItem({ debt, index }: { debt: Debt; index: number }) {
  const path = join('/debts', at(debt, index));
  return (
    <Item path={path} fallbackName="Debt" figure={moneyCompact(debt.balance)} detail={`${percent(debt.rate)} APR`}>
      <Row>
        <MoneyField path={`${path}/balance`} label="Balance" />
        <PercentField path={`${path}/rate`} label="Interest rate" step={0.0025} />
      </Row>
      <ChoiceField path={`${path}/kind`} label="Type" options={DEBT_KINDS} />
      <Row>
        <MoneyField path={`${path}/payment`} label="Payment" nullable placeholder="Computed" suffix="/mo" />
        <NumField path={`${path}/remaining_months`} label="Time left" kind="months" nullable placeholder="Open-ended" suffix="mo" />
      </Row>
    </Item>
  );
}

export function PropertySection() {
  const profile = useProfile();
  const add = useAdd('/properties');
  // Planned purchases reserve their property id too.
  const reserved = profile.events.flatMap((e) => (e.kind.type === 'buy_home' ? [e.kind.property_id] : []));
  const addProperty = (name: string, value: Record<string, unknown>) => add(name, value, { taken: reserved });
  const addDebt = useAdd('/debts');
  const mortgaged = profile.properties.some((p) => p.mortgage) || profile.events.some((e) => e.kind.type === 'buy_home');
  // A cash-out refinance needs only an owned main home: one with no mortgage has the most room to borrow.
  const hasHome = profile.properties.some((p) => p.use.type === 'primary') || profile.events.some((e) => e.kind.type === 'buy_home' && e.kind.use.type === 'primary');
  return (
    <>
      <Group title="Property">
        <ItemList empty="No property owned. A planned purchase goes under Life events.">
          {profile.properties.map((p, i) => (
            <PropertyItem key={p.id || i} property={p} index={i} />
          ))}
        </ItemList>
        <div className="flex flex-wrap gap-x-1">
          <AddButton label="Home" onAdd={() => addProperty('Home', { use: { type: 'primary' }, value: 400_000 })} />
          <AddButton label="Rental" onAdd={() => addProperty('Rental property', { use: { type: 'rental', monthly_rent: 2000 }, value: 300_000 })} />
          <AddButton label="Vacation home" onAdd={() => addProperty('Vacation home', { use: { type: 'vacation' }, value: 300_000 })} />
        </div>
      </Group>
      {(mortgaged || hasHome) && (
        <Group title={mortgaged ? 'Mortgages' : 'Borrowing against the home'}>
          {mortgaged && (
            <ToggleField
              path="/debt_policy/refinance_mortgages"
              label="Refinance when rates fall"
              hint="Each January, if the market rate is a point or more below the loan’s and four years of saved interest cover the closing costs."
            />
          )}
          <Realism summary={mortgaged ? 'Cash-out refinancing and paying down early' : 'Cash-out refinancing'} on={debtPolicyRealism(profile.debt_policy)}>
            <Optional path="/debt_policy/cash_out_refinance" label="Refinance for cash before penalised withdrawals" present={profile.debt_policy.cash_out_refinance !== null} make={MAKE.cashOutRefinance}>
              <Row cols={3}>
                <PercentField path="/debt_policy/cash_out_refinance/max_ltv" label="Up to, of value" />
                <PercentField path="/debt_policy/cash_out_refinance/rate_premium" label="Rate premium" step={0.00125} />
                <NumField path="/debt_policy/cash_out_refinance/term_years" label="Term" kind="years" suffix="yrs" />
              </Row>
            </Optional>
            {mortgaged && (
              <Row>
                <ChoiceField
                  path="/debt_policy/mortgage_prepayment"
                  label="Prepay the mortgage"
                  options={[
                    { value: 'never', label: 'Never' },
                    { value: 'when_rate_above', label: 'When its rate is high' },
                  ]}
                  read={(v) => (v as typeof profile.debt_policy.mortgage_prepayment).type}
                  write={(type) => ({ type })}
                />
                {profile.debt_policy.mortgage_prepayment.type === 'when_rate_above' && (
                  <PercentField path="/debt_policy/mortgage_prepayment/real_rate" label="Real rate to beat" nullable placeholder="Automatic" />
                )}
              </Row>
            )}
          </Realism>
        </Group>
      )}

      <Group title="Debts">
        <ItemList empty="No debts besides mortgages.">
          {profile.debts.map((d, i) => (
            <DebtItem key={d.id || i} debt={d} index={i} />
          ))}
        </ItemList>
        <AddButton label="Add a debt" onAdd={() => addDebt('New loan', { kind: 'other', balance: 10_000, rate: 0.07, remaining_months: 60 })} />
        {profile.debts.length > 0 && (
          <Row>
            <ChoiceField
              path="/debt_policy/strategy"
              label="Payoff strategy"
              options={[
                { value: 'minimum', label: 'Minimum payments' },
                { value: 'avalanche', label: 'Highest rate first' },
                { value: 'snowball', label: 'Smallest balance first' },
              ]}
            />
            <MoneyField path="/debt_policy/extra_monthly" label="Extra each month" suffix="/mo" />
          </Row>
        )}
      </Group>
    </>
  );
}
