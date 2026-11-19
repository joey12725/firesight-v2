import type { Child, Person } from '@/engine/types';
import { set } from '@/state/patches';
import { at, join } from '@/state/pointer';
import { Field, Select, Switch, currentYearMonth, yearsBetween } from '@/ui';
import { CitySearch, CountrySelect } from '../CitySearch';
import { ChoiceField, Group, MoneyField, PercentField, Row, SegmentField, TextField } from '../fields';
import { useEdit, useHelp, useIssues, useProfile, useValue } from '../hooks';
import { US_STATES } from '../labels';
import { MonthField, MonthInput } from '../MonthField';
import { AddButton, Item, ItemList } from '../Section';
import { useAdd } from '../useAdd';

const SEX = [
  { value: 'female', label: 'Female' },
  { value: 'male', label: 'Male' },
  { value: 'unspecified', label: 'Unspecified' },
] as const;

const HEALTH = [
  { value: 'excellent', label: 'Excellent' },
  { value: 'average', label: 'Average' },
  { value: 'poor', label: 'Poor' },
] as const;

const COLLEGE = [
  { value: 'none', label: 'No college costs' },
  { value: 'public_in_state', label: 'Public, in-state' },
  { value: 'public_out_of_state', label: 'Public, out-of-state' },
  { value: 'private', label: 'Private' },
  { value: 'custom', label: 'Custom amount' },
] as const;

function JoinsField({ path }: { path: string }) {
  const value = useValue<string | null>(path);
  const edit = useEdit();
  const help = useHelp(path);
  const issues = useIssues(path);
  return (
    <Field label="Joins the household later" help={help} issues={issues} hint={value ? undefined : 'Turn on for a future partner.'}>
      {({ id }) => (
        <div data-path={path} className="flex items-center gap-3">
          <Switch id={id} checked={value !== null} onChange={(on) => edit(set(path, on ? currentYearMonth() : null))} />
          {value !== null && (
            <div className="flex-1">
              <MonthInput label="Joins" value={value} onChange={(v) => edit(set(path, v), { coalesce: path })} />
            </div>
          )}
        </div>
      )}
    </Field>
  );
}

function PersonItem({ person, index, start }: { person: Person; index: number; start: string }) {
  const path = join('/household/people', at(person, index));
  return (
    <Item path={path} fallbackName={`Person ${index + 1}`} removable={index > 0} detail={`age ${yearsBetween(person.birth, start)}`}>
      <MonthField path={`${path}/birth`} label="Born" />
      <Row>
        <SegmentField path={`${path}/sex`} label="Sex" options={SEX.map((o) => ({ ...o, label: o.label === 'Unspecified' ? 'Unspec.' : o.label }))} />
        <SegmentField path={`${path}/health`} label="Health" options={HEALTH.map((o) => ({ ...o, label: o.label === 'Excellent' ? 'Excell.' : o.label }))} />
      </Row>
      <JoinsField path={`${path}/joins`} />
    </Item>
  );
}

function ChildItem({ child, index, start }: { child: Child; index: number; start: string }) {
  const path = join('/household/children', at(child, index));
  const age = yearsBetween(child.birth, start);
  return (
    <Item path={path} fallbackName="Child" detail={child.birth > start ? `planned ${child.birth.slice(0, 4)}` : `age ${age}`}>
      <MonthField path={`${path}/birth`} label="Born (or expected)" hint="A future month plans a child." />
      <Row>
        <ChoiceField
          path={`${path}/costs`}
          label="Everyday costs"
          options={[
            { value: 'modeled', label: 'Estimate for me' },
            { value: 'in_budget', label: 'Already in my budget' },
          ]}
        />
        <MoneyField path={`${path}/childcare_monthly`} label="Childcare, ages 0-4" nullable placeholder="Estimate" suffix="/mo" />
      </Row>
      <Row>
        <ChoiceField
          path={`${path}/college`}
          label="College"
          options={COLLEGE}
          read={(v) => (v as Child['college']).type}
          write={(type) => (type === 'custom' ? { type, annual: 30_000 } : { type })}
        />
        {child.college.type === 'custom' ? (
          <MoneyField path={`${path}/college/annual`} label="Cost per year" suffix="/yr" />
        ) : (
          <PercentField path={`${path}/college_share`} label="Share you pay" />
        )}
      </Row>
      {child.college.type === 'custom' && <PercentField path={`${path}/college_share`} label="Share you pay" />}
    </Item>
  );
}

function LocationFields() {
  const location = useProfile().household.location;
  const edit = useEdit();
  const cityHelp = useHelp('/household/location/city');
  const cityIssues = useIssues('/household/location/city');
  const stateHelp = useHelp('/household/location/state');
  const stateIssues = useIssues('/household/location/state');
  const isUs = location.country === 'US';
  return (
    <Group title="Where you live">
      <Row>
        <Field label="Country" help={useHelp('/household/location/country')} issues={useIssues('/household/location/country')}>
          {({ id, invalid }) => (
            <span data-path="/household/location/country" className="block">
              <CountrySelect
                id={id}
                invalid={invalid}
                value={location.country}
                onChange={(country) => {
                  // The city belongs to the old country; a state only exists in the US.
                  const ops = [set('/household/location/country', country), set('/household/location/city', null)];
                  if (country !== 'US') ops.push(set('/household/location/state', null));
                  edit(ops, { label: 'Change country' });
                }}
              />
            </span>
          )}
        </Field>
        {isUs ? (
          <Field label="State" help={stateHelp} issues={stateIssues}>
            {({ id, invalid }) => (
              <span data-path="/household/location/state" className="block">
                <Select id={id} invalid={invalid} value={location.state ?? ''} placeholder="Choose a state" options={US_STATES} onChange={(v) => edit(set('/household/location/state', v))} />
              </span>
            )}
          </Field>
        ) : (
          <PercentField path="/household/location/foreign_tax_rate" label="Income tax rate there" nullable placeholder="None" />
        )}
      </Row>
      <Field
        label="City"
        help={cityHelp}
        issues={cityIssues}
        hint={isUs ? 'Scales living costs if you plan a move, and decides city income tax where one is levied. The city table does not carry states, so choose the state yourself.' : 'Scales living costs if you plan a move.'}
      >
        {({ id, invalid }) => (
          <span data-path="/household/location/city" className="block">
            <CitySearch
              id={id}
              invalid={invalid}
              value={location.city}
              onPick={(city, text) => {
                const ops = [set('/household/location/city', text || null)];
                // A city from the table sets its country when that differs. Within the US nothing else moves: the table
                // has no states, so the state stays as chosen (and a state is never guessed from a city name).
                if (city?.countryCode && city.countryCode !== location.country) {
                  ops.push(set('/household/location/country', city.countryCode));
                  if (city.countryCode !== 'US') ops.push(set('/household/location/state', null));
                }
                edit(ops, { label: `Set city to ${text || 'none'}` });
              }}
            />
          </span>
        )}
      </Field>
    </Group>
  );
}

export function HouseholdSection() {
  const profile = useProfile();
  const { people, children } = profile.household;
  const start = profile.start ?? currentYearMonth();
  const addPerson = useAdd('/household/people');
  const addChild = useAdd('/household/children');
  return (
    <>
      <TextField path="/name" label="Plan name" />
      <TextField path="/description" label="Description" placeholder="A line about this household, shown under its name" />
      <Group title="Adults">
        <ItemList empty="Add the adults this plan covers.">
          {people.map((p, i) => (
            <PersonItem key={p.id || i} person={p} index={i} start={start} />
          ))}
        </ItemList>
        {people.length < 2 && (
          <AddButton label={people.length === 0 ? 'Add a person' : 'Add a partner'} onAdd={() => addPerson(people.length === 0 ? 'You' : 'Partner', { birth: people[0]?.birth ?? '1990-01' })} />
        )}
      </Group>
      <Group title="Children">
        <ItemList empty="No children in this plan. Planned children can be added with a future birth month.">
          {children.map((c, i) => (
            <ChildItem key={c.id || i} child={c} index={i} start={start} />
          ))}
        </ItemList>
        <AddButton label="Add a child" onAdd={() => addChild(`Child ${children.length + 1}`, { birth: start })} />
      </Group>
      <LocationFields />
    </>
  );
}
