import type { BudgetItem, Category, Profile, Spending } from '@/engine/types';
import { at, join } from '@/state/pointer';
import { Disclosure, money, moneyCompact } from '@/ui';
import { ChoiceField, Group, MoneyField, NumField, Optional, OwnerField, PercentField, Realism, Row, SegmentField, SliderField, ToggleField } from '../fields';
import { useProfile } from '../hooks';
import { CATEGORIES, FREQUENCIES, annualAmount, annualInRetirement, budgetTotals, categoryLabel, frequency } from '../labels';
import { MAKE, healthcareRealism, spendingRealism } from '../realism';
import { AddButton, Item, ItemList } from '../Section';
import { useAdd } from '../useAdd';
import { WhenField } from '../WhenField';

const IN_RETIREMENT = [
  { value: 'same', label: 'Stays the same' },
  { value: 'stops', label: 'Stops' },
  { value: 'factor', label: 'Scales by a factor' },
  { value: 'amount', label: 'Changes to a new amount' },
] as const;

function retirementNote(item: BudgetItem): string | null {
  const r = item.in_retirement;
  if (r.type === 'same') return null;
  if (r.type === 'stops') return 'stops in retirement';
  return `${moneyCompact(annualInRetirement(item))}/yr retired`;
}

function BudgetItemRow({ item, index }: { item: BudgetItem; index: number }) {
  const path = join('/spending/items', at(item, index));
  const f = frequency(item.frequency);
  return (
    <Item
      path={path}
      fallbackName={categoryLabel(item.category)}
      figure={`${money(item.amount)}${f.short}`}
      detail={[item.essential ? null : 'discretionary', retirementNote(item)].filter(Boolean).join(' · ')}
    >
      <Row>
        <MoneyField path={`${path}/amount`} label="Amount" suffix={f.short} />
        <ChoiceField path={`${path}/frequency`} label="How often" options={FREQUENCIES} />
      </Row>
      <Row>
        <ChoiceField path={`${path}/category`} label="Category" options={CATEGORIES} />
        <SegmentField
          path={`${path}/essential`}
          label="Kind"
          options={[
            { value: 'yes', label: 'Essential' },
            { value: 'no', label: 'Discretionary' },
          ]}
          read={(v) => (v ? 'yes' : 'no')}
          write={(v) => v === 'yes'}
        />
      </Row>
      <ChoiceField
        path={`${path}/in_retirement`}
        label="In retirement"
        options={IN_RETIREMENT}
        read={(v) => (v as BudgetItem['in_retirement']).type}
        write={(type) => (type === 'factor' ? { type, factor: 0.8 } : type === 'amount' ? { type, amount: item.amount } : { type })}
      />
      {item.in_retirement.type === 'factor' && <SliderField path={`${path}/in_retirement/factor`} label="Factor" kind="percent" min={0} max={2} step={0.05} />}
      {item.in_retirement.type === 'amount' && <MoneyField path={`${path}/in_retirement/amount`} label="New amount" suffix={f.short} />}
      <Disclosure summary="Timing, growth and owner">
        <div className="space-y-3">
          <Row>
            <WhenField path={`${path}/start`} label="Starts" unsetLabel="Already started" />
            <WhenField path={`${path}/end`} label="Ends" unsetLabel="Never" />
          </Row>
          <Row>
            <PercentField path={`${path}/real_growth`} label="Growth above inflation" nullable placeholder="Default" />
            <OwnerField path={`${path}/owner`} label="Whose expense" nullable />
          </Row>
          {item.category === 'rent' && (
            <SegmentField
              path={`${path}/rent_growth`}
              label="Rent rises with"
              options={[
                { value: 'fixed', label: 'Inflation plus growth' },
                { value: 'market', label: 'The local rent market' },
              ]}
            />
          )}
        </div>
      </Disclosure>
    </Item>
  );
}

function Totals() {
  const t = budgetTotals(useProfile());
  const cell = (label: string, total: number, essential: number) => (
    <div>
      <div className="eyebrow">{label}</div>
      <div className="num mt-0.5 text-xl font-medium tracking-tight text-ink-1">
        {money(Math.round(total))}
        <span className="ml-1 text-xs font-normal text-ink-3">/yr</span>
      </div>
      <div className="mt-0.5 text-xs text-ink-3">
        <span className="num text-ink-2">{money(Math.round(total / 12))}</span>/mo · <span className="num text-ink-2">{total > 0 ? Math.round((essential / total) * 100) : 0}%</span> essential
      </div>
    </div>
  );
  return (
    <div className="grid grid-cols-2 gap-4 border-y border-line py-3" aria-live="polite">
      {cell('Budget now', t.now, t.essentialNow)}
      {cell('Budget in retirement', t.retired, t.essentialRetired)}
      <p className="col-span-2 text-xs leading-snug text-ink-3">
        Sums of the lines below in today's dollars, ignoring start and end dates. Modelled healthcare, children, housing and taxes are added by the simulation.
      </p>
    </div>
  );
}

function HealthcareFields({ healthcare }: { healthcare: Spending['healthcare'] }) {
  const p = '/spending/healthcare';
  return (
    <Group title="Healthcare">
      <SegmentField
        path={`${p}/mode`}
        label="Health costs"
        options={[
          { value: 'modeled', label: 'Estimate for me' },
          { value: 'in_budget', label: 'Already in my budget' },
        ]}
      />
      {healthcare.mode === 'modeled' && (
        <>
          <Row>
            <ChoiceField
              path={`${p}/early_retirement`}
              label="Cover before Medicare"
              options={[
                { value: 'aca', label: 'Marketplace (ACA)' },
                { value: 'cobra', label: 'COBRA, then ACA' },
                { value: 'custom', label: 'Custom premium' },
              ]}
              read={(v) => (v as Spending['healthcare']['early_retirement']).type}
              write={(type) => (type === 'custom' ? { type, annual: 12_000 } : { type })}
            />
            {healthcare.early_retirement.type === 'custom' ? (
              <MoneyField path={`${p}/early_retirement/annual`} label="Premium" suffix="/yr" />
            ) : (
              <SegmentField
                path={`${p}/quality`}
                label="Plan tier"
                options={[
                  { value: 'bronze', label: 'Bronze' },
                  { value: 'silver', label: 'Silver' },
                  { value: 'gold', label: 'Gold' },
                ]}
              />
            )}
          </Row>
          <ToggleField path={`${p}/long_term_care`} label="Include the risk of long-term care" />
          <Realism summary="Children’s cover, Medicaid and care at home" on={healthcareRealism(healthcare)}>
            <ToggleField path={`${p}/cover_children`} label="Children are on the health plan" />
            {healthcare.cover_children && <NumField path={`${p}/children_coverage_end_age`} label="Until age" kind="years" min={18} max={26} />}
            <ToggleField path={`${p}/medicaid`} label="Medicaid pays for care once savings are spent" />
            <ToggleField path={`${p}/spousal_care`} label="A partner provides part of the care at home" />
          </Realism>
        </>
      )}
    </Group>
  );
}

type Vehicle = Profile['spending']['vehicles'][number];

function VehicleItem({ vehicle, index }: { vehicle: Vehicle; index: number }) {
  const path = join('/spending/vehicles', at(vehicle, index));
  return (
    <Item path={path} fallbackName="Vehicle" figure={moneyCompact(vehicle.price)} detail={`every ${vehicle.replace_every_years} yr`}>
      <Row>
        <MoneyField path={`${path}/price`} label="Replacement price" />
        <PercentField path={`${path}/trade_in_fraction`} label="Covered by the trade-in" />
      </Row>
      <Row>
        <NumField path={`${path}/replace_every_years`} label="Replaced every" step={1} suffix="yrs" />
        <NumField path={`${path}/age_years`} label="Age now" step={1} suffix="yrs" />
      </Row>
      <Optional path={`${path}/financing`} label="Financed with a loan" present={vehicle.financing !== null} make={MAKE.vehicleFinancing}>
        <Row cols={3}>
          <PercentField path={`${path}/financing/down_fraction`} label="Paid up front" />
          <PercentField path={`${path}/financing/rate`} label="Rate" />
          <NumField path={`${path}/financing/term_months`} label="Term" kind="months" suffix="mo" />
        </Row>
      </Optional>
      <Row>
        <WhenField path={`${path}/end`} label="Stop replacing" unsetLabel="Never" />
        <OwnerField path={`${path}/owner`} label="Whose vehicle" nullable />
      </Row>
    </Item>
  );
}

function Vehicles({ vehicles }: { vehicles: Vehicle[] }) {
  const add = useAdd('/spending/vehicles');
  return (
    <Realism summary="Vehicles replaced on a cycle" on={vehicles.length}>
      <ItemList empty="No vehicles. Add one to buy a replacement every few years instead of spreading its cost evenly; leave its purchase out of the Transport lines.">
        {vehicles.map((v, i) => (
          <VehicleItem key={v.id || i} vehicle={v} index={i} />
        ))}
      </ItemList>
      <AddButton label="Add a vehicle" onAdd={() => add('Car', MAKE.vehicle())} />
    </Realism>
  );
}

export function SpendingSection() {
  const profile = useProfile();
  const { items } = profile.spending;
  const add = useAdd('/spending/items');
  const groups = CATEGORIES.map((c) => ({
    ...c,
    items: items.map((item, index) => ({ item, index })).filter((x) => x.item.category === c.value),
  })).filter((g) => g.items.length > 0);

  return (
    <>
      <Totals />
      <div className="space-y-4">
        {groups.map((g) => (
          <div key={g.value}>
            <div className="mb-1.5 flex items-baseline justify-between">
              <h4 className="eyebrow">{g.label}</h4>
              <span className="num text-xs text-ink-3">{moneyCompact(g.items.reduce((n, x) => n + annualAmount(x.item), 0))}/yr</span>
            </div>
            <ul className="space-y-1.5">
              {g.items.map(({ item, index }) => (
                <BudgetItemRow key={item.id || index} item={item} index={index} />
              ))}
            </ul>
          </div>
        ))}
        {items.length === 0 && <p className="text-xs text-ink-3">No budget lines yet. Add what you spend in a typical month or year.</p>}
        <AddButton label="Add a budget line" onAdd={() => add('New expense', { category: 'other' satisfies Category, amount: 100, frequency: 'monthly' })} />
      </div>

      <Group title="How spending changes">
        <SliderField path="/spending/lifestyle_creep" label="Lifestyle creep" kind="percent" min={0} max={1} step={0.05} hint="Share of each real raise that turns into more spending." />
        <ToggleField path="/spending/retirement_smile" label="Spending declines gently through retirement" />
        <SegmentField
          path="/spending/shocks"
          label="Unplanned expenses"
          options={[
            { value: 'off', label: 'Off' },
            { value: 'typical', label: 'Typical' },
            { value: 'high', label: 'High' },
            { value: 'detailed', label: 'Detailed' },
          ]}
        />
        <PercentField path="/spending/survivor_factor" label="Survivor's spending" />
        <Realism summary="Seasons, personal inflation and job loss" on={spendingRealism(profile.spending)}>
          <SegmentField
            path="/spending/seasonality"
            label="Spending through the year"
            options={[
              { value: 'off', label: 'Even months' },
              { value: 'typical', label: 'Typical seasons' },
            ]}
          />
          <SegmentField
            path="/spending/category_inflation"
            label="Prices of each category"
            options={[
              { value: 'off', label: 'Follow inflation' },
              { value: 'stochastic', label: 'Drift on their own' },
            ]}
          />
          <SliderField path="/spending/unemployment_cut" label="Discretionary spending cut while out of work" kind="percent" min={0} max={0.5} step={0.01} />
        </Realism>
        <Vehicles vehicles={profile.spending.vehicles} />
      </Group>

      <HealthcareFields healthcare={profile.spending.healthcare} />
    </>
  );
}
