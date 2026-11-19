import { useEffect, useState } from 'react';
import type { Category, EventKind, LifeEvent, Profile, When } from '@/engine/types';
import { set } from '@/state/patches';
import { at, join, uniqueId } from '@/state/pointer';
import { Button, Disclosure, Field, Popover, Select, Switch, moneyCompact, percent } from '@/ui';
import { CitySearch, CountrySelect } from '../CitySearch';
import { findCity, loadCities, relocationFactors, type City } from '../cities';
import { ChoiceField, Group, MoneyField, NumField, PercentField, Row, SliderField, ToggleField } from '../fields';
import { useEdit, useHelp, useIssues, usePeople, useProfile, useValue } from '../hooks';
import { CATEGORIES, US_STATES, categoryLabel } from '../labels';
import { Item, ItemList } from '../Section';
import { useAdd } from '../useAdd';
import { WhenField, describeWhen } from '../WhenField';
import { PropertyCostFields, RentalFields } from './Property';

type KindType = EventKind['type'];

const KINDS: { type: KindType; label: string; note: string; make: (p: Profile) => Record<string, unknown> | null }[] = [
  {
    type: 'buy_home',
    label: 'Buy a home',
    note: 'Down payment, mortgage and running costs',
    make: (p) => ({
      type: 'buy_home',
      price: 450_000,
      // The purchased home becomes a property; its id must not clash with an existing one.
      property_id: uniqueId('home', [...p.properties.map((x) => x.id), ...p.events.flatMap((e) => (e.kind.type === 'buy_home' ? [e.kind.property_id] : []))]),
    }),
  },
  {
    type: 'sell_property',
    label: 'Sell a property',
    note: 'Sell something you own; optionally rent afterwards',
    make: (p) => (p.properties[0] ? { type: 'sell_property', property: p.properties[0].id } : null),
  },
  { type: 'relocate', label: 'Relocate', note: 'Move city, state or country; living costs rescale', make: () => ({ type: 'relocate' }) },
  { type: 'windfall', label: 'Windfall', note: 'Inheritance, gift or a business sale', make: () => ({ type: 'windfall', amount: 50_000 }) },
  { type: 'expense', label: 'Big expense', note: 'A wedding, a car, a renovation', make: () => ({ type: 'expense', amount: 25_000 }) },
  {
    type: 'income_change',
    label: 'Income change',
    note: 'A new salary, a pay cut, or a job ending',
    make: (p) => (p.incomes[0] ? { type: 'income_change', income: p.incomes[0].id, change: { type: 'scale', factor: 0.5 } } : null),
  },
  { type: 'career_break', label: 'Career break', note: 'Time out of work, with or without partial pay', make: () => ({ type: 'career_break', person: 0, months: 12 }) },
  { type: 'spending_change', label: 'Spending change', note: 'Scale or reset part of the budget', make: () => ({ type: 'spending_change', target: { type: 'all' }, change: { type: 'scale', factor: 0.9 } }) },
  {
    type: 'contribution_change',
    label: 'Savings rate change',
    note: 'Change a workplace plan contribution',
    make: (p) => {
      const job = p.incomes.find((i) => i.kind.type === 'job' && i.kind.plan !== null);
      return job ? { type: 'contribution_change', income: job.id, employee_fraction: 0.15 } : null;
    },
  },
];

const kindLabel = (t: KindType) => KINDS.find((k) => k.type === t)?.label ?? t;

function headline(kind: EventKind): string | undefined {
  switch (kind.type) {
    case 'buy_home':
      return moneyCompact(kind.price);
    case 'windfall':
      return `+${moneyCompact(kind.amount)}`;
    case 'expense':
      return moneyCompact(kind.amount);
    case 'career_break':
      return `${kind.months} mo`;
    default:
      return undefined;
  }
}

/** A sortable month index for a When; events tied to retirement sort after dated ones. */
function sortKey(when: When, profile: Profile): number {
  const ym = (s: string) => {
    const [y = 0, m = 1] = s.split('-').map(Number);
    return y * 12 + m;
  };
  switch (when.type) {
    case 'start':
      return 0;
    case 'date':
      return ym(when.at);
    case 'age':
      return ym(profile.household.people[when.person]?.birth ?? '1990-01') + when.age * 12;
    case 'retirement':
      return 1e6 + when.offset_years * 12;
  }
}

function CostOfLiving({ path, kind }: { path: string; kind: Extract<EventKind, { type: 'relocate' }> }) {
  const profile = useProfile();
  const edit = useEdit();
  const [cities, setCities] = useState<City[] | null>(null);
  const help = useHelp(`${path}/cost_of_living`);
  const origin = profile.household.location;
  useEffect(() => {
    let live = true;
    loadCities().then((c) => live && setCities(c), () => undefined);
    return () => {
      live = false;
    };
  }, []);

  const from = cities ? findCity(cities, origin.city, origin.country) : undefined;
  const to = cities ? findCity(cities, kind.city, kind.country) : undefined;
  const family = profile.household.children.length > 0;
  const col = kind.cost_of_living;
  const resolve = () => from && to && edit(set(`${path}/cost_of_living`, { type: 'by_category', factors: relocationFactors(from, to, family) }), { label: 'Resolve living costs from the city table' });

  return (
    <Group title="Living costs after the move">
      <Field label="How costs change" help={help}>
        {({ id }) => (
          <Select
            id={id}
            value={col.type}
            options={[
              { value: 'unchanged', label: 'Unchanged' },
              { value: 'factor', label: 'One overall factor' },
              { value: 'by_category', label: 'By category' },
            ]}
            onChange={(type) =>
              edit(
                set(
                  `${path}/cost_of_living`,
                  type === 'factor' ? { type, overall: 1 } : type === 'by_category' ? { type, factors: from && to ? relocationFactors(from, to, family) : {} } : { type },
                ),
              )
            }
          />
        )}
      </Field>
      {col.type === 'factor' && <SliderField path={`${path}/cost_of_living/overall`} label="Overall factor" kind="percent" min={0.3} max={2} step={0.01} />}
      {col.type === 'by_category' && (
        <div className="space-y-2">
          <ul className="grid grid-cols-2 gap-x-4 gap-y-1">
            {CATEGORIES.filter((c) => col.factors[c.value] !== undefined).map((c) => (
              <li key={c.value} className="flex items-baseline justify-between border-b border-line py-1 text-xs">
                <span className="text-ink-2">{c.label}</span>
                <FactorInput path={`${path}/cost_of_living/factors/${c.value}`} label={categoryLabel(c.value as Category)} />
              </li>
            ))}
          </ul>
          <p className="text-xs leading-snug text-ink-3">
            {from && to
              ? `Ratios of ${to.city} to ${from.city} price indices by category. Healthcare, insurance, giving, education and travel are not scaled.`
              : cities
                ? `To fill these from the city table, pick both your current city (Household) and the destination from the search.`
                : 'Loading the city table…'}
          </p>
          {from && to && (
            <Button size="sm" icon="rotate" onClick={resolve}>
              Recompute from {from.city} → {to.city}
            </Button>
          )}
        </div>
      )}
    </Group>
  );
}

function FactorInput({ path, label }: { path: string; label: string }) {
  const value = useValue<number>(path);
  const edit = useEdit();
  return (
    <input
      aria-label={`${label} factor, percent`}
      className="num w-14 bg-transparent text-right text-ink-1 outline-none focus-visible:underline"
      value={Math.round(value * 100)}
      inputMode="numeric"
      onChange={(e) => {
        const n = Number(e.target.value);
        if (Number.isFinite(n) && n > 0) edit(set(path, n / 100), { coalesce: path });
      }}
    />
  );
}

function EventFields({ path, kind }: { path: string; kind: EventKind }) {
  const profile = useProfile();
  const people = usePeople();
  const edit = useEdit();
  const k = `${path}/kind`;
  const incomes = profile.incomes.map((i) => ({ value: i.id, label: i.name || i.id }));
  const cityIssues = useIssues(`${k}/city`);
  switch (kind.type) {
    case 'buy_home':
      return (
        <>
          <Row>
            <MoneyField path={`${k}/price`} label="Price, today's dollars" />
            <SliderField path={`${k}/down_payment_fraction`} label="Down payment" kind="percent" min={0} max={1} step={0.01} />
          </Row>
          <Row cols={3}>
            <PercentField path={`${k}/rate`} label="Mortgage rate" nullable placeholder="Market" step={0.00125} />
            <NumField path={`${k}/term_years`} label="Term" kind="years" suffix="yrs" />
            <PercentField path={`${k}/closing_cost_fraction`} label="Closing costs" />
          </Row>
          <ToggleField path={`${k}/replaces_rent`} label="Replaces the rent in my budget" />
          <ChoiceField
            path={`${k}/use`}
            label="Use"
            options={[
              { value: 'primary', label: 'Home' },
              { value: 'rental', label: 'Rental' },
              { value: 'vacation', label: 'Vacation home' },
            ]}
            read={(v) => (v as typeof kind.use).type}
            write={(type) => (type === 'rental' ? { type, monthly_rent: 2000 } : { type })}
          />
          {kind.use.type === 'rental' && <RentalFields path={`${k}/use`} />}
          <Disclosure summary="Running costs and appreciation">
            <PropertyCostFields path={k} />
          </Disclosure>
        </>
      );
    case 'sell_property':
      return (
        <>
          <ChoiceField path={`${k}/property`} label="Which property" options={profile.properties.map((p) => ({ value: p.id, label: p.name || p.id }))} />
          <Row>
            <PercentField path={`${k}/selling_cost_fraction`} label="Selling costs" />
            <MoneyField path={`${k}/then_rent_monthly`} label="Then rent for" nullable placeholder="No rent" suffix="/mo" />
          </Row>
        </>
      );
    case 'relocate':
      return (
        <>
          <Field label="Destination city" issues={cityIssues}>
            {({ id }) => (
              <CitySearch
                id={id}
                value={kind.city}
                onPick={(city, text) => {
                  const ops = [set(`${k}/city`, text || null)];
                  if (city?.countryCode && city.countryCode !== kind.country) {
                    ops.push(set(`${k}/country`, city.countryCode));
                    if (city.countryCode !== 'US') ops.push(set(`${k}/state`, null));
                  }
                  edit(ops, { label: `Move to ${text || 'a new place'}` });
                }}
              />
            )}
          </Field>
          <Row>
            <Field label="Country">
              {({ id }) => (
                <CountrySelect
                  id={id}
                  value={kind.country}
                  onChange={(country) => edit(country === 'US' ? [set(`${k}/country`, country)] : [set(`${k}/country`, country), set(`${k}/state`, null)], { label: 'Change destination country' })}
                />
              )}
            </Field>
            {kind.country === 'US' ? (
              <ChoiceField path={`${k}/state`} label="State" options={US_STATES} read={(v) => (v as string | null) ?? ''} />
            ) : (
              <PercentField path={`${k}/foreign_tax_rate`} label="Income tax rate there" nullable placeholder="None" />
            )}
          </Row>
          <MoneyField path={`${k}/moving_cost`} label="Cost of the move" />
          <CostOfLiving path={k} kind={kind} />
        </>
      );
    case 'windfall':
      return (
        <>
          <MoneyField path={`${k}/amount`} label="Amount, today's dollars" />
          <ToggleField path={`${k}/taxable`} label="Taxable as income" />
        </>
      );
    case 'expense':
      return (
        <>
          <MoneyField path={`${k}/amount`} label="Amount, today's dollars" />
          <div className="space-y-3">
            <Field inline label="Financed with a loan" help={<FinancingHelp path={`${k}/financed`} />}>
              {({ id }) => <Switch id={id} checked={kind.financed !== null} onChange={(on) => edit(set(`${k}/financed`, on ? { down_fraction: 0.2, rate: 0.07, term_months: 60 } : null))} />}
            </Field>
            {kind.financed && (
              <Row cols={3}>
                <PercentField path={`${k}/financed/down_fraction`} label="Paid up front" />
                <PercentField path={`${k}/financed/rate`} label="Rate" />
                <NumField path={`${k}/financed/term_months`} label="Term" kind="months" suffix="mo" />
              </Row>
            )}
          </div>
        </>
      );
    case 'income_change':
      return (
        <>
          <ChoiceField path={`${k}/income`} label="Which income" options={incomes} />
          <Row>
            <ChoiceField
              path={`${k}/change`}
              label="What happens"
              options={[
                { value: 'scale', label: 'Scales by a factor' },
                { value: 'set_salary', label: 'New salary' },
                { value: 'end', label: 'Ends' },
              ]}
              read={(v) => (v as typeof kind.change).type}
              write={(type) => (type === 'scale' ? { type, factor: 0.5 } : type === 'set_salary' ? { type, amount: 60_000 } : { type })}
            />
            {kind.change.type === 'scale' && <PercentField path={`${k}/change/factor`} label="New level" />}
            {kind.change.type === 'set_salary' && <MoneyField path={`${k}/change/amount`} label="Salary" suffix="/yr" />}
          </Row>
        </>
      );
    case 'career_break':
      return (
        <>
          {people.length > 1 && <ChoiceField path={`${k}/person`} label="Who" options={people} read={(v) => String(v)} write={(v) => Number(v)} />}
          <Row>
            <NumField path={`${k}/months`} label="Length" kind="months" suffix="mo" />
            <PercentField path={`${k}/income_fraction`} label="Pay during the break" />
          </Row>
        </>
      );
    case 'spending_change':
      return (
        <>
          <Row>
            <ChoiceField
              path={`${k}/target`}
              label="Applies to"
              options={[
                { value: 'all', label: 'The whole budget' },
                { value: 'category', label: 'One category' },
                { value: 'item', label: 'One budget line' },
              ]}
              read={(v) => (v as typeof kind.target).type}
              write={(type) =>
                type === 'category' ? { type, category: 'travel' } : type === 'item' ? { type, id: profile.spending.items[0]?.id ?? '' } : { type }
              }
            />
            {kind.target.type === 'category' && <ChoiceField path={`${k}/target/category`} label="Category" options={CATEGORIES} />}
            {kind.target.type === 'item' && (
              <ChoiceField path={`${k}/target/id`} label="Budget line" options={profile.spending.items.map((i) => ({ value: i.id, label: i.name || i.id }))} />
            )}
          </Row>
          <Row>
            <ChoiceField
              path={`${k}/change`}
              label="Change"
              options={[
                { value: 'scale', label: 'Scale by a factor' },
                { value: 'set', label: 'Set a yearly amount' },
              ]}
              read={(v) => (v as typeof kind.change).type}
              write={(type) => (type === 'scale' ? { type, factor: 0.9 } : { type, annual: 12_000 })}
            />
            {kind.change.type === 'scale' ? <PercentField path={`${k}/change/factor`} label="New level" /> : <MoneyField path={`${k}/change/annual`} label="Amount" suffix="/yr" />}
          </Row>
        </>
      );
    case 'contribution_change':
      return (
        <Row>
          <ChoiceField path={`${k}/income`} label="Which job" options={incomes} />
          <PercentField path={`${k}/employee_fraction`} label="New contribution" />
        </Row>
      );
  }
}

function FinancingHelp({ path }: { path: string }) {
  return <>{useHelp(path)}</>;
}

function EventItem({ event, index, names }: { event: LifeEvent; index: number; names: string[] }) {
  const path = join('/events', at(event, index));
  const uncertain = event.probability < 1 || event.timing_uncertainty_years > 0;
  const detail = [
    describeWhen(event.when, names),
    kindLabel(event.kind.type),
    event.probability < 1 ? `${percent(event.probability, 0)} likely` : null,
    event.timing_uncertainty_years > 0 ? `±${event.timing_uncertainty_years} yr` : null,
  ]
    .filter(Boolean)
    .join(' · ');
  return (
    <Item path={path} fallbackName={kindLabel(event.kind.type)} figure={headline(event.kind)} detail={detail}>
      <WhenField path={`${path}/when`} label="When" kinds={['date', 'age', 'retirement', 'start']} />
      <EventFields path={path} kind={event.kind} />
      <Disclosure summary="How certain is this?" defaultOpen={uncertain}>
        <div className="space-y-3">
          <SliderField path={`${path}/probability`} label="Chance it happens" kind="percent" min={0} max={1} step={0.05} />
          <SliderField path={`${path}/timing_uncertainty_years`} label="Timing could slip by up to" min={0} max={10} step={0.5} suffix="years" />
        </div>
      </Disclosure>
    </Item>
  );
}

export function EventsSection() {
  const profile = useProfile();
  const add = useAdd('/events');
  const names = profile.household.people.map((p) => p.name);
  const ordered = profile.events.map((event, index) => ({ event, index })).sort((a, b) => sortKey(a.event.when, profile) - sortKey(b.event.when, profile));
  const inTwoYears = (() => {
    const [y = 2026, m = '01'] = (profile.start ?? '2026-01').split('-');
    return `${Number(y) + 2}-${m}`;
  })();

  return (
    <Group>
      <ItemList empty="Nothing planned yet. Add the big things you expect: a home, a move, a break from work.">
        {ordered.map(({ event, index }) => (
          <EventItem key={event.id || index} event={event} index={index} names={names} />
        ))}
      </ItemList>
      <Popover
        label="Add a life event"
        className="w-72 p-1"
        trigger={
          <Button size="sm" variant="ghost" icon="plus" className="-ml-2">
            Add a life event
          </Button>
        }
      >
        {(close) => (
          <ul>
            {KINDS.map((kind) => {
              const made = kind.make(profile);
              return (
                <li key={kind.type}>
                  <button
                    type="button"
                    disabled={made === null}
                    onClick={() => {
                      if (made) add(kind.label, { when: { type: 'date', at: inTwoYears }, kind: made }, { idHint: kind.type });
                      close();
                    }}
                    className="block w-full rounded-sm px-2.5 py-1.5 text-left hover:bg-surface-3 disabled:opacity-45"
                  >
                    <span className="block text-sm font-medium text-ink-1">{kind.label}</span>
                    <span className="block text-xs text-ink-3">{made === null ? 'Needs a property, income or workplace plan first' : kind.note}</span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </Popover>
    </Group>
  );
}
