import type { Child, Household, Person } from '@/engine/types';
import { set } from '@/state/patches';
import { at, join } from '@/state/pointer';
import { Field, Select, Switch, currentYearMonth, moneyCompact, yearsBetween } from '@/ui';
import { CitySearch, CountrySelect } from '../CitySearch';
import { ChoiceField, Group, MoneyField, NumField, Optional, PercentField, PersonField, Realism, Row, SegmentField, SliderField, TextField, ToggleField } from '../fields';
import { useEdit, useHelp, useIssues, usePeople, useProfile, useValue } from '../hooks';
import { US_STATES } from '../labels';
import { MonthField, MonthInput } from '../MonthField';
import { AddButton, Item, ItemList } from '../Section';
import { MAKE, childRealism, familyRealism, personRealism } from '../realism';
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

const PARENT_HEALTH = [
  { value: 'excellent', label: 'Excellent' },
  { value: 'good', label: 'Good' },
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

const OFF_TYPICAL = [
  { value: 'off', label: 'Off' },
  { value: 'typical', label: 'Typical' },
] as const;

/** A month that may be left empty: a switch, and the month once it is on. */
function OptionalMonth({ path, label, short, hint, initial }: { path: string; label: string; short: string; hint?: string; initial?: string }) {
  const value = useValue<string | null>(path);
  const edit = useEdit();
  const help = useHelp(path);
  const issues = useIssues(path);
  return (
    <Field label={label} help={help} issues={issues} hint={value ? undefined : hint}>
      {({ id }) => (
        <div data-path={path} className="flex items-center gap-3">
          <Switch id={id} checked={value !== null} onChange={(on) => edit(set(path, on ? (initial ?? currentYearMonth()) : null))} />
          {value !== null && (
            <div className="flex-1">
              <MonthInput label={short} value={value} onChange={(v) => edit(set(path, v), { coalesce: path })} />
            </div>
          )}
        </div>
      )}
    </Field>
  );
}

function LifePolicies({ path, policies }: { path: string; policies: Person['life_insurance'] }) {
  const add = useAdd(path);
  return (
    <Group title="Life insurance">
      <ItemList empty="No life insurance on this person.">
        {policies.map((policy, i) => {
          const p = join(path, at(policy, i));
          return (
            <Item key={policy.id || i} path={p} fallbackName="Policy" figure={moneyCompact(policy.death_benefit)} detail={policy.kind.type === 'term' ? `term, ${policy.kind.years_remaining} yr left` : 'permanent'}>
              <Row>
                <ChoiceField
                  path={`${p}/kind`}
                  label="Type"
                  options={[
                    { value: 'term', label: 'Term' },
                    { value: 'permanent', label: 'Permanent' },
                  ]}
                  read={(v) => (v as typeof policy.kind).type}
                  write={MAKE.policyKind}
                />
                {policy.kind.type === 'term' && <NumField path={`${p}/kind/years_remaining`} label="Years left" kind="years" suffix="yrs" />}
              </Row>
              <Row>
                <MoneyField path={`${p}/death_benefit`} label="Death benefit" />
                <MoneyField path={`${p}/annual_premium`} label="Premium" suffix="/yr" />
              </Row>
            </Item>
          );
        })}
      </ItemList>
      <AddButton label="Add a policy" onAdd={() => add('Term life', MAKE.lifePolicy())} />
    </Group>
  );
}

/** Disability, long-term-care insurance and life insurance for one adult. */
function PersonRealism({ path, person }: { path: string; person: Person }) {
  const ltc = person.ltc_insurance;
  return (
    <Realism summary="Disability and insurance" on={personRealism(person)}>
      <SegmentField path={`${path}/disability_risk`} label="Risk of a long-term disability" options={OFF_TYPICAL} />
      {person.disability_risk !== 'off' && (
        <Optional path={`${path}/ltd_insurance`} label="Long-term disability insurance" present={person.ltd_insurance !== null} make={MAKE.ltdInsurance}>
          <Row>
            <PercentField path={`${path}/ltd_insurance/replacement`} label="Share of pay replaced" />
            <NumField path={`${path}/ltd_insurance/to_age`} label="Paid until age" kind="years" />
          </Row>
        </Optional>
      )}
      <Optional path={`${path}/ltc_insurance`} label="Long-term-care insurance" present={ltc !== null} make={MAKE.ltcInsurance}>
        <Row>
          <MoneyField path={`${path}/ltc_insurance/annual_premium`} label="Premium" suffix="/yr" />
          <MoneyField path={`${path}/ltc_insurance/daily_benefit`} label="Daily benefit" suffix="/day" />
        </Row>
        <Row>
          <NumField path={`${path}/ltc_insurance/benefit_years`} label="Benefit pool" step={0.5} suffix="yrs" />
          <NumField path={`${path}/ltc_insurance/elimination_days`} label="Waiting period" kind="integer" suffix="days" />
        </Row>
        <Row>
          <ChoiceField
            path={`${path}/ltc_insurance/inflation_rider`}
            label="Inflation rider"
            options={[
              { value: 'none', label: 'None' },
              { value: 'compound', label: 'Compound growth' },
            ]}
            read={(v) => (v as NonNullable<typeof ltc>['inflation_rider']).type}
            write={MAKE.inflationRider}
          />
          {ltc?.inflation_rider.type === 'compound' && <PercentField path={`${path}/ltc_insurance/inflation_rider/rate`} label="Growth per year" />}
        </Row>
        <ToggleField path={`${path}/ltc_insurance/premium_increase_risk`} label="The insurer may raise premiums" />
      </Optional>
      <LifePolicies path={`${path}/life_insurance`} policies={person.life_insurance} />
    </Realism>
  );
}

/** Who takes the first leave. With a second leave in the plan, that one goes to the other parent, so it follows. */
function LeaveTaker({ path, child }: { path: string; child: Child }) {
  const people = usePeople();
  const edit = useEdit();
  const help = useHelp(`${path}/parental_leave/person`);
  const issues = useIssues(`${path}/parental_leave/person`);
  if (people.length < 2) return null;
  return (
    <Field label="Who takes it" help={help} issues={issues}>
      {({ id, invalid }) => (
        <span data-path={`${path}/parental_leave/person`} className="block">
          <Select
            id={id}
            invalid={invalid}
            value={String(child.parental_leave?.person ?? 0)}
            options={people}
            onChange={(v) => {
              const ops = [set(`${path}/parental_leave/person`, Number(v))];
              if (child.second_parental_leave) ops.push(set(`${path}/second_parental_leave/person`, Number(v) === 0 ? 1 : 0));
              edit(ops, { label: 'Change who takes parental leave' });
            }}
          />
        </span>
      )}
    </Field>
  );
}

function LeaveFields({ path }: { path: string }) {
  return (
    <>
      <Row>
        <NumField path={`${path}/months`} label="Length" kind="months" suffix="mo" />
        <PercentField path={`${path}/pay_fraction`} label="Pay during leave" />
      </Row>
    </>
  );
}

/** What is uncertain about a child: whether and when they arrive, parental leave, what college costs, support as an adult. */
function ChildRealism({ path, child, planned }: { path: string; child: Child; planned: boolean }) {
  const people = usePeople();
  const first = child.parental_leave?.person ?? 0;
  return (
    <Realism summary="Uncertainty, leave and later support" on={childRealism(child)}>
      {planned ? (
        <>
          <ChoiceField
            path={`${path}/arrival`}
            label="Arrival"
            options={[
              { value: 'certain', label: 'Certain, in the month above' },
              { value: 'uncertain', label: 'Uncertain: may be late, twins, or not at all' },
            ]}
            read={(v) => (v as Child['arrival']).type}
            write={(type) => ({ type })}
          />
          {child.arrival.type === 'uncertain' && (
            <Row cols={3}>
              <PercentField path={`${path}/arrival/probability`} label="Chance of arriving" />
              <NumField path={`${path}/arrival/delay_mean_months`} label="Typical delay" kind="months" suffix="mo" />
              <PercentField path={`${path}/arrival/twins_probability`} label="Chance of twins" step={0.005} />
            </Row>
          )}
          <Optional path={`${path}/parental_leave`} label="Parental leave at the birth" present={child.parental_leave !== null} make={() => MAKE.parentalLeave(0)}>
            <LeaveTaker path={path} child={child} />
            <LeaveFields path={`${path}/parental_leave`} />
          </Optional>
          {people.length > 1 && child.parental_leave !== null && (
            <Optional path={`${path}/second_parental_leave`} label="Leave for the other parent too" present={child.second_parental_leave !== null} make={() => MAKE.parentalLeave(first === 0 ? 1 : 0)}>
              <LeaveFields path={`${path}/second_parental_leave`} />
            </Optional>
          )}
        </>
      ) : (
        <p className="text-xs leading-snug text-ink-3">An uncertain arrival and parental leave apply to a planned child: give a birth month in the future to set them.</p>
      )}
      <ChoiceField
        path={`${path}/college_cost`}
        label="College cost"
        options={[
          { value: 'fixed', label: 'As planned above' },
          { value: 'stochastic', label: 'Uncertain: tuition, aid, years' },
        ]}
        read={(v) => (v as Child['college_cost']).type}
        write={(type) => ({ type })}
      />
      {child.college_cost.type === 'stochastic' && (
        <>
          <Row>
            <PercentField path={`${path}/college_cost/attend_probability`} label="Chance of attending" />
            <NumField path={`${path}/college_cost/aid_scale`} label="Aid, against typical" step={0.1} />
          </Row>
          <Row>
            <PercentField path={`${path}/college_cost/real_tuition_growth`} label="Tuition growth above inflation" step={0.001} />
            <PercentField path={`${path}/college_cost/tuition_volatility`} label="Its variability" step={0.001} />
          </Row>
        </>
      )}
      <Optional path={`${path}/adult_support`} label="May need support as an adult" present={child.adult_support !== null} make={MAKE.adultSupport}>
        <Row cols={3}>
          <PercentField path={`${path}/adult_support/probability`} label="Chance" />
          <MoneyField path={`${path}/adult_support/annual`} label="Cost" suffix="/yr" />
          <NumField path={`${path}/adult_support/mean_years`} label="Typical spell" step={0.5} suffix="yrs" />
        </Row>
      </Optional>
    </Realism>
  );
}

/** The terms of a divorce: shared by the divorce risk here and the divorce life event. */
export function DivorceTerms({ path, keepsHome }: { path: string; keepsHome: boolean }) {
  const homes = useProfile().properties.length > 0;
  return (
    <>
      <SliderField path={`${path}/asset_split`} label="Share of net worth the departing partner takes" kind="percent" min={0} max={1} step={0.05} />
      {homes && <ToggleField path={`${path}/keeps_home`} label="The person who stays keeps the home" />}
      {homes && !keepsHome && <MoneyField path={`${path}/then_rent_monthly`} label="Rent after the sale" nullable placeholder="4% of the price a year" suffix="/mo" />}
      <Row>
        <MoneyField path={`${path}/legal_costs`} label="Legal costs" />
        <PercentField path={`${path}/spending_factor`} label="Spending after" />
      </Row>
      <Row cols={3}>
        <MoneyField path={`${path}/support/annual`} label="Support" suffix="/yr" />
        <NumField path={`${path}/support/years`} label="For" kind="years" suffix="yrs" />
        <ChoiceField
          path={`${path}/support/direction`}
          label="Direction"
          options={[
            { value: 'pays', label: 'Paid' },
            { value: 'receives', label: 'Received' },
          ]}
        />
      </Row>
    </>
  );
}

type Parent = Household['parents'][number];

function ParentItem({ parent, index }: { parent: Parent; index: number }) {
  const path = join('/household/parents', at(parent, index));
  const people = usePeople();
  const care = parent.care_support;
  return (
    <Item path={path} fallbackName="Parent" figure={moneyCompact(parent.estate * parent.share)} detail={`born ${parent.birth.slice(0, 4)}${care ? ' · may need care' : ''}`}>
      <PersonField path={`${path}/child`} label="Whose parent" />
      <MonthField path={`${path}/birth`} label="Born" />
      <Row>
        <SegmentField path={`${path}/sex`} label="Sex" options={SEX.map((o) => ({ ...o, label: o.label === 'Unspecified' ? 'Unspec.' : o.label }))} />
        <ChoiceField path={`${path}/health`} label="Health" options={PARENT_HEALTH} />
      </Row>
      <Row>
        <MoneyField path={`${path}/estate`} label="Estate today" />
        <PercentField path={`${path}/share`} label="Your share" />
      </Row>
      <PercentField path={`${path}/inherited_ira_fraction`} label="Share held in pre-tax retirement accounts" />
      <ToggleField path={`${path}/care_erodes_estate`} label="Late-life care is paid from the estate first" />
      <Optional path={`${path}/care_support`} label="You help with their care" present={care !== null} make={MAKE.careSupport}>
        <Row>
          <PercentField path={`${path}/care_support/probability`} label="Chance you take part" />
          <MoneyField path={`${path}/care_support/annual_cost`} label="You pay" suffix="/yr" />
        </Row>
        <SliderField path={`${path}/care_support/hours`} label="Work given up by the caregiver" kind="percent" min={0} max={1} step={0.05} />
        {people.length > 1 && (
          <ChoiceField
            path={`${path}/care_support/caregiver`}
            label="Who cuts back"
            options={[{ value: 'own', label: 'The parent’s own child' }, ...people]}
            read={(v) => (v === null || v === undefined ? 'own' : String(v))}
            write={(v) => (v === 'own' ? null : Number(v))}
          />
        )}
      </Optional>
    </Item>
  );
}

/** Parents (inheritance and their care) and the risk of divorce. */
function FamilyRealism({ household, start }: { household: Household; start: string }) {
  const addParent = useAdd('/household/parents');
  const risk = household.divorce_risk;
  const eldest = household.people.map((p) => p.birth).sort()[0] ?? start;
  const parentBirth = `${Number(eldest.slice(0, 4)) - 30}${eldest.slice(4)}`;
  return (
    <Realism summary="Parents, inheritance and divorce" on={familyRealism(household)}>
      <Group title="Parents">
        <ItemList empty="No parents in this plan. Add one to model an inheritance tied to their lifetime, and any help with their care.">
          {household.parents.map((p, i) => (
            <ParentItem key={p.id || i} parent={p} index={i} />
          ))}
        </ItemList>
        <AddButton label="Add a parent" onAdd={() => addParent('Parent', MAKE.parent(parentBirth))} />
      </Group>
      {household.people.length > 1 && (
        <Group title="Marriage">
          <OptionalMonth path="/household/married" label="Married" short="Married" hint="Turn on to give the month of the wedding." initial={start} />
          <SegmentField path="/household/divorce_risk" label="Risk of divorce" options={OFF_TYPICAL} read={(v) => (v as Household['divorce_risk']).type} write={(type) => ({ type })} />
          {risk.type === 'typical' && <DivorceTerms path="/household/divorce_risk" keepsHome={risk.keeps_home} />}
          <p className="text-xs leading-snug text-ink-3">A divorce at a known date is a life event.</p>
        </Group>
      )}
    </Realism>
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
      <OptionalMonth path={`${path}/joins`} label="Joins the household later" short="Joins" hint="Turn on for a future partner." />
      <PersonRealism path={path} person={person} />
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
      <ChildRealism path={path} child={child} planned={child.birth > start} />
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
      <FamilyRealism household={profile.household} start={start} />
    </>
  );
}
