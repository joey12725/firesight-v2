import type { Debt, Property } from '@/engine/types';
import { set } from '@/state/patches';
import { at, join } from '@/state/pointer';
import { Disclosure, Field, Switch, moneyCompact, percent } from '@/ui';
import { ChoiceField, Group, MoneyField, NumField, PercentField, Row, SegmentField, ToggleField } from '../fields';
import { useEdit, useHelp, useProfile } from '../hooks';
import { AddButton, Item, ItemList } from '../Section';
import { useAdd } from '../useAdd';

const USE = [
  { value: 'primary', label: 'Home' },
  { value: 'rental', label: 'Rental' },
  { value: 'vacation', label: 'Vacation' },
] as const;

/** Running costs and price behaviour shared by owned properties and planned purchases. */
export function PropertyCostFields({ path }: { path: string }) {
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
    </div>
  );
}

export function RentalFields({ path }: { path: string }) {
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
      {(profile.properties.some((p) => p.mortgage) || profile.events.some((e) => e.kind.type === 'buy_home')) && (
        <Group title="Mortgages">
          <ToggleField
            path="/debt_policy/refinance_mortgages"
            label="Refinance when rates fall"
            hint="Each January, if the market rate is a point or more below the loan’s and four years of saved interest cover the closing costs."
          />
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
