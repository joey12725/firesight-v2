import { useState, type ReactNode } from 'react';
import { exerciseCostVested, paperValue } from '@/engine/equity-math';
import type { EquityPreview } from '@/engine/equity-types';
import type { EquityHolding, PatchOp, PrivateCompany, Profile } from '@/engine/types';
import { append, set } from '@/state/patches';
import { at, join, uniqueId } from '@/state/pointer';
import { Button, Disclosure, Field, IssueList, NumberInput, Popover, Select, Switch, money, moneyCompact, moneyPrice, percent, type NumberKind } from '@/ui';
import { OutcomeEditor } from '../equity/OutcomeEditor';
import { PreviewStrip } from '../equity/PreviewStrip';
import {
  LIMITS,
  followCountOps,
  followMonthsLeftOps,
  from409aOps,
  grantAgeBasis,
  holdingValue,
  isOption,
  isoYearlyAmount,
  linkJobOps,
  monthsLeftIsEstimate,
  newCompany,
  newHolding,
  priceIsEstimated,
  splitIsoOps,
  stageOps,
  switchKindOps,
  unvested,
  vests,
  type HoldingType,
} from '../equity/model';
import { companyDetail, holdingDetail, nothingFigure, optionLine, tenderHint, tenderLine, unitLine, wholePercent } from '../equity/text';
import { canShowPayoffs, previewOf, stageDefaults, usePreview } from '../equity/usePreview';
import { ChoiceField, Group, MoneyField, NumField, OwnerField, PercentField, Row, SegmentField, SliderField, ToggleField } from '../fields';
import { useEdit, useHelp, useIssues, useProfile, useValue } from '../hooks';
import { COMPANY_STAGES, HOLDING_KINDS, stageLabel } from '../labels';
import { MISSING_JOB } from '../issue-words';
import { Item, ItemList, markFresh } from '../Section';
import { useAdd } from '../useAdd';

const EXERCISE = [
  { value: 'at_liquidity', label: 'Only when there is a buyer' },
  { value: 'vested_now', label: 'Vested ones now' },
  { value: 'as_they_vest', label: 'Now, and each batch as it vests' },
] as const;

const AT_DEADLINE = [
  { value: 'exercise', label: 'Exercise what I can' },
  { value: 'forfeit', label: 'Let them go' },
] as const;

const SELL_DOWN = [
  { value: 'all_at_lockup_end', label: 'Sell everything' },
  { value: 'over_years', label: 'Sell over several years' },
  { value: 'hold', label: 'Keep the shares' },
] as const;

const COUNT_LABEL: Record<HoldingType, string> = { iso: 'Options', nso: 'Options', rsu: 'Units', shares: 'Shares' };

// Help written for the editor where the schema's text speaks of the file format (ids,
// decimals, field names). The schema text stays as it is: the assistant and people who
// edit the JSON read it.
const HELP = {
  job: 'The job in this plan at this company, if you work there now. What is unvested keeps vesting while that job lasts; leaving it forfeits what is unvested and starts the exercise window. Choose None for a former employer or an investment.',
  included: "Switch off to plan as if this stock and these options were worth nothing: no exercises, no proceeds, no tax. The company's fate is still simulated, so a failure can still end the linked job.",
  kind: 'Incentive options (ISOs) can count toward the alternative minimum tax when exercised and are taxed as a gain when the shares are sold. On non-qualified options (NSOs) the gap between the strike and the 409A value is taxed as pay when exercised. RSUs are shares the company hands over, here at an IPO or sale, taxed as pay then. Shares you own were exercised earlier or bought. Your grant letter or equity portal says which you have.',
  vested: 'How many of these you have earned so far by staying. Your equity portal shows it. The rest vest month by month over the months left, once any cliff has passed.',
  fmvAtExercise: 'The 409A value of one share on the day you exercised, from Form 3921 (box 4). It sets how much of a later gain the alternative minimum tax has already counted. Leave 0 if you do not know: it is then taken to equal what you paid.',
  amtCredit: "If exercising this company's incentive options made you pay alternative minimum tax in an earlier year, the credit you carry forward is on Form 8801 (the last line). It comes back in later years as it can be used. Leave 0 if none.",
  qsbs: "Shares of a US C corporation with at most $75 million of gross assets ($50 million for shares acquired before July 5, 2025) on the day you got them can be partly or wholly free of federal tax on the gain once held long enough. The company can tell you whether its stock qualifies. Leave off if unsure. Two simplifications to know: a sale three or four years after you got the shares is shown too favourably (the part of the gain that is not excluded is taxed at the ordinary long-term rates, where the law charges up to 28%), and state tax is charged on the excluded gain in every state, which is too much outside California, Alabama, Mississippi and Pennsylvania.",
  overhang:
    "What investors have put in, as a percent of the company's latest valuation. In a sale for less than that valuation investors are paid first, and common stock gets nothing if the price is at or below this percent of it. Leave empty for the stage's typical value (20% at seed to 35% for the largest companies).",
  tenderChance:
    "The chance in any one year that the company runs a tender offer, in percent, averaged over everything that might become of it. Leave empty for the stage's typical value (0.5% at seed to 15% for the largest companies). 0 for never.",
  tenderPlanned: 'Switch on if the company has announced a tender offer. It then happens for certain in the month you give, at the limit and price below.',
  tenderLimit: 'The most you may sell in one tender, as a percent of the shares you own and your vested options. Typically 20%.',
  tenderPrice:
    "The tender price as a percent of the company's share price in that month. 100% is no discount; 90% is a 10% discount. The share price starts at the latest round's price and moves toward the simulated outcome, so you cannot set a dollar price. Between 5% and 200%.",
  tenderSell: 'How much of what a tender allows you would sell: 0% keeps everything, 100% sells up to the limit each time.',
} as const;

/** A value that is either set or null, with a switch that turns it on at `initial`. */
function OptionalValue({ path, label, help, present, initial, children }: { path: string; label: string; help?: ReactNode; present: boolean; initial: unknown; children: ReactNode }) {
  const edit = useEdit();
  const schemaHelp = useHelp(path);
  return (
    <div className="space-y-3">
      <Field inline label={label} help={help ?? schemaHelp}>
        {({ id }) => <Switch id={id} checked={present} onChange={(on) => edit(set(path, on ? initial : null), { label: `${label}: ${on ? 'yes' : 'no'}` })} />}
      </Field>
      {present && <div className="space-y-3 border-l border-line pl-3">{children}</div>}
    </div>
  );
}

/**
 * A number field whose edit may carry further operations (an estimate that follows the
 * value). Otherwise the same as `NumField`: help from the schema, issues, `data-path`.
 */
function LinkedNumField({ path, label, kind, hint, help, suffix, also }: { path: string; label: string; kind: NumberKind; hint?: ReactNode; help?: ReactNode; suffix?: ReactNode; also: (value: number) => PatchOp[] }) {
  const value = useValue<number | null>(path);
  const schemaHelp = useHelp(path);
  const issues = useIssues(path);
  const edit = useEdit();
  return (
    <Field label={label} help={help ?? schemaHelp} hint={hint} issues={issues}>
      {({ id, describedBy, invalid }) => (
        <span data-path={path} className="block">
          <NumberInput id={id} aria-describedby={describedBy} invalid={invalid} kind={kind} min={0} suffix={suffix} value={value ?? null} onChange={(v) => v !== null && edit([set(path, v), ...also(v)], { coalesce: path })} />
        </span>
      )}
    </Field>
  );
}

function HoldingFields({ companyPath, company, holding, path }: { companyPath: string; company: PrivateCompany; holding: EquityHolding; path: string }) {
  const edit = useEdit();
  const kindIssues = useIssues(`${path}/kind`);
  const k = holding.kind;
  const option = k.type === 'iso' || k.type === 'nso';
  // The same measure as the engine's warning (checked in model.test.ts).
  const overLimit = isoYearlyAmount(holding) > LIMITS.isoYearly;
  // A company with no linked job has nothing unvested, so the vested count is not asked for (it follows the total).
  const allVested = company.job === null && unvested(holding) <= 0;
  // Whether the grant age and the months left are still the editor's own estimates is read
  // from the holding itself, so it is the same after undo, a reload or a switch of plan.
  const ageBasis = grantAgeBasis(holding);
  const estimatingLeft = monthsLeftIsEstimate(holding);
  // Estimates that follow an edit of the count or of the vested count.
  const follow = (shares: number, vested: number): PatchOp[] => followCountOps(path, holding, company.job !== null, shares, vested);
  const lapse = 'Options lapse ten years after the grant.';
  const ageHint =
    ageBasis === 'months_left' ? `Estimated from the months left to vest, on a four-year schedule. ${lapse}` : ageBasis === 'vested' && holding.vested > 0 ? `Estimated from how much has vested. ${lapse}` : lapse;

  const split = () => edit(splitIsoOps(companyPath, company, holding.id), { label: 'Split at the $100,000 limit' });
  const derived = optionLine(holding, company.preferred_price) ?? unitLine(holding, company.preferred_price);

  return (
    <>
      <Field label="Type" help={HELP.kind} issues={kindIssues}>
        {({ id, describedBy }) => (
          <span data-path={`${path}/kind`} className="block">
            <Select
              id={id}
              aria-describedby={describedBy}
              value={k.type}
              options={HOLDING_KINDS.map((x) => ({ value: x.value, label: x.menu }))}
              onChange={(type) => edit(switchKindOps(path, holding, type), { label: `Type: ${HOLDING_KINDS.find((x) => x.value === type)?.menu ?? type}` })}
            />
          </span>
        )}
      </Field>
      {overLimit && (
        <div className="space-y-1.5">
          <p className="text-xs leading-snug text-ink-3">Your grant documents show the exact split; this is an estimate.</p>
          <Button size="sm" variant="secondary" disabled={company.holdings.length >= LIMITS.holdings} onClick={split}>
            Split at the $100,000 limit
          </Button>
        </div>
      )}
      {vests(holding) && allVested ? (
        <LinkedNumField
          path={`${path}/shares`}
          label={COUNT_LABEL[k.type]}
          kind="count"
          hint="All counted as vested, because this company is not linked to a job. If you still work there and some are unvested, choose the job under Linked job first."
          also={(v) => follow(v, holding.vested)}
        />
      ) : vests(holding) ? (
        <Row>
          <LinkedNumField path={`${path}/shares`} label={COUNT_LABEL[k.type]} kind="count" also={(v) => follow(v, holding.vested)} />
          <LinkedNumField path={`${path}/vested`} label="Vested so far" kind="count" help={HELP.vested} also={(v) => follow(holding.shares, v)} />
        </Row>
      ) : (
        <NumField path={`${path}/shares`} label={COUNT_LABEL[k.type]} kind="count" min={0} />
      )}
      {option && (
        <>
          <MoneyField path={`${path}/kind/strike`} label="Strike per share" step={0.01} min={0} max={LIMITS.perShare} />
          <NumField path={`${path}/kind/granted_months_ago`} label="Months since the grant" kind="months" suffix="mo" min={0} max={LIMITS.grantAgeMonths} hint={ageHint} />
        </>
      )}
      {vests(holding) && unvested(holding) > 0 && (
        <LinkedNumField
          path={`${path}/vest_months_left`}
          label="Months left to vest"
          kind="months"
          suffix="mo"
          hint={estimatingLeft && holding.vested > 0 ? 'Estimated from how much has vested, on a four-year schedule.' : undefined}
          also={(v) => followMonthsLeftOps(path, holding, v)}
        />
      )}
      {vests(holding) && unvested(holding) > 0 && (
        <Disclosure summary="Cliff" defaultOpen={holding.cliff_months > 0}>
          <NumField path={`${path}/cliff_months`} label="Months to the cliff" kind="months" suffix="mo" min={0} />
        </Disclosure>
      )}
      {k.type === 'rsu' && <ToggleField path={`${path}/kind/must_be_employed_at_event`} label="Lost if I leave before the IPO or sale" />}
      {k.type === 'shares' && (
        <>
          <Row>
            <MoneyField path={`${path}/kind/basis_per_share`} label="Paid per share" step={0.01} min={0} max={LIMITS.perShare} />
            <NumField path={`${path}/kind/months_held`} label="Months held" kind="months" suffix="mo" min={0} />
          </Row>
          <ToggleField path={`${path}/kind/from_iso`} label="From exercising ISOs" />
          {k.from_iso && (
            <MoneyField path={`${path}/kind/fmv_at_exercise`} label="409A value when exercised" step={0.01} min={0} max={LIMITS.perShare} help={HELP.fmvAtExercise} hint="Per share, on Form 3921. Leave 0 if you do not know." />
          )}
        </>
      )}
      {derived && <p className="text-xs leading-relaxed text-ink-3">{derived}</p>}
    </>
  );
}

function Holdings({ path, company }: { path: string; company: PrivateCompany }) {
  const listPath = `${path}/holdings`;
  const add = useAdd(listPath);
  const priced = company.preferred_price > 0;
  const full = company.holdings.length >= LIMITS.holdings;
  const linked = company.job !== null;
  // Findings about the holdings as a whole (the position dwarfs the savings) have no field of their own.
  const issues = useIssues(listPath);
  return (
    <Group title="What you hold">
      <ItemList empty="Nothing entered yet. Add each grant, and any shares you already own.">
        {company.holdings.map((h, i) => {
          const p = join(listPath, at(h, i));
          return (
            <Item key={h.id || i} path={p} fallbackName="Holding" figure={priced ? moneyCompact(holdingValue(h, company.preferred_price)) : undefined} detail={holdingDetail(h)}>
              <HoldingFields companyPath={path} company={company} holding={h} path={p} />
            </Item>
          );
        })}
      </ItemList>
      <IssueList issues={issues} />
      <Popover
        label="Add a holding"
        className="w-72 p-1"
        trigger={
          <Button size="sm" variant="ghost" icon="plus" className="-ml-2" disabled={!priced || full}>
            Add a holding
          </Button>
        }
      >
        {(close) => (
          <ul>
            {HOLDING_KINDS.map((kind) => (
              <li key={kind.value}>
                <button
                  type="button"
                  onClick={() => {
                    const { name, ...value } = newHolding(kind.value, linked);
                    add(name, value, { idHint: kind.value === 'shares' ? 'shares' : `${kind.value}_grant` });
                    close();
                  }}
                  className="block w-full rounded-sm px-2.5 py-1.5 text-left hover:bg-surface-3"
                >
                  <span className="block text-sm font-medium text-ink-1">{kind.menu}</span>
                  <span className="block text-xs text-ink-3">{kind.note}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </Popover>
      {!priced && <p className="text-xs text-ink-3">Enter the latest price per share first.</p>}
      {priced && full && <p className="text-xs text-ink-3">A company can have at most {LIMITS.holdings} holdings. Combine grants with the same type and a similar strike.</p>}
    </Group>
  );
}

/** "I only know the 409A value": estimates the round price from it at the stage's typical fraction. */
function From409a({ path, company }: { path: string; company: PrivateCompany }) {
  const edit = useEdit();
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState<number | null>(null);
  const ratio = stageDefaults(company.stage)?.fmvRatio ?? 0;
  if (!(ratio > 0)) return null;
  const isEstimate = priceIsEstimated(company, ratio);
  const note = `Estimated from the 409A value at a typical ${wholePercent(ratio)}% for this stage. Replace it when you know the round price.`;
  // The note is shown only while the price really is that estimate (a change of stage moves the two together).
  const typedNote = value !== null && value > 0 && isEstimate ? note : `The price per share is set to this divided by a typical ${wholePercent(ratio)}% for this stage.`;
  if (!open) {
    return (
      <div className="space-y-1">
        {isEstimate && <p className="text-xs leading-snug text-ink-3">{note}</p>}
        <button
          type="button"
          onClick={() => {
            setValue(company.fmv_409a);
            setOpen(true);
          }}
          className="rounded-xs text-xs font-medium text-ink-2 underline decoration-line-strong underline-offset-2 hover:text-ink-1"
        >
          I only know the 409A value
        </button>
      </div>
    );
  }
  return (
    <div className="space-y-2 border-l border-line pl-3">
      <Field label="409A value per share, from your option paperwork" hint={typedNote}>
        {({ id, describedBy }) => (
          <NumberInput
            id={id}
            aria-describedby={describedBy}
            kind="money"
            step={0.01}
            min={0}
            nullable
            value={value}
            onChange={(v) => {
              setValue(v);
              if (v !== null && v > 0) edit(from409aOps(path, v, ratio), { coalesce: `${path}#from-409a`, label: 'Estimate the price from the 409A value' });
            }}
          />
        )}
      </Field>
      <Button size="sm" variant="ghost" className="-ml-2" onClick={() => setOpen(false)}>
        Done
      </Button>
    </div>
  );
}

function LinkedJob({ path, company, profile }: { path: string; company: PrivateCompany; profile: Profile }) {
  const edit = useEdit();
  const found = useIssues(`${path}/job`);
  // Shown after an unlink that had to mark unvested units as vested, until the company is linked again.
  const [vestedNote, setVestedNote] = useState(false);
  const jobs = profile.incomes.filter((i) => i.kind.type === 'job');
  const options = [{ value: '', label: 'None (former employer or investment)' }, ...jobs.map((j) => ({ value: j.id, label: j.name || 'Job' }))];
  // An id that matches no job (the job was removed) stays visible as an option, so the error has a control to sit on.
  // The engine names the missing id, which the household never saw: say what happened and what to do instead.
  const missing = company.job !== null && !jobs.some((j) => j.id === company.job);
  if (missing && company.job !== null) options.push({ value: company.job, label: 'Removed job' });
  const issues = missing ? [{ severity: 'error' as const, message: MISSING_JOB }] : found;
  return (
    <Field
      label="Linked job"
      help={HELP.job}
      issues={issues}
      hint={
        vestedNote && company.job === null
          ? 'Unvested units were set to vested: nothing vests at a company you no longer work for. Lower the counts to what you actually kept, or press Undo to get the link and the old counts back.'
          : jobs.length === 0
            ? 'There is no job in the plan yet. Add it under Income first if you work at this company and some of your equity is still vesting.'
            : undefined
      }
    >
      {({ id, describedBy, invalid }) => (
        <span data-path={`${path}/job`} className="block">
          <Select
            id={id}
            aria-describedby={describedBy}
            invalid={invalid}
            value={company.job ?? ''}
            options={options}
            onChange={(value) => {
              const job = jobs.find((j) => j.id === value);
              const { ops, vestedChanged } = linkJobOps(path, company, job ? { id: job.id, owner: job.owner } : null);
              if (edit(ops, { label: job ? `Link to ${job.name || 'the job'}` : 'No linked job' })) setVestedNote(vestedChanged);
            }}
          />
        </span>
      )}
    </Field>
  );
}

/**
 * The stage. A price that was estimated from the 409A value follows a change of stage
 * in the same edit, so the estimate and the note about it stay true.
 */
function StageField({ path, company }: { path: string; company: PrivateCompany }) {
  const edit = useEdit();
  const help = useHelp(`${path}/stage`);
  const issues = useIssues(`${path}/stage`);
  return (
    <Field label="Stage" help={help} issues={issues} hint="Sets typical odds, timing and prices.">
      {({ id, describedBy, invalid }) => (
        <span data-path={`${path}/stage`} className="block">
          <Select
            id={id}
            aria-describedby={describedBy}
            invalid={invalid}
            value={company.stage}
            options={COMPANY_STAGES}
            onChange={(stage) => edit(stageOps(path, company, stage, (s) => stageDefaults(s)?.fmvRatio ?? 0), { label: `Stage: ${stageLabel(stage)}` })}
          />
        </span>
      )}
    </Field>
  );
}

function CompanyFields({ path, company, profile, preview }: { path: string; company: PrivateCompany; profile: Profile; preview: EquityPreview | null }) {
  const defaults = stageDefaults(company.stage);
  const zeroSales = preview ? Math.round(preview.acquisition_zero_share * 100) : null;
  return (
    <Group title="Company">
      <StageField path={path} company={company} />
      <MoneyField path={`${path}/preferred_price`} label="Latest price per share" step={0.01} min={0} max={LIMITS.perShare} hint="What investors paid in the last round." />
      <From409a path={path} company={company} />
      <NumField
        path={`${path}/fmv_409a`}
        label="409A value per share"
        kind="money"
        nullable
        step={0.01}
        min={0}
        placeholder={preview && company.preferred_price > 0 ? moneyPrice(preview.fmv_409a).replace('$', '') : undefined}
        hint={defaults ? `Leave empty for a typical ${wholePercent(defaults.fmvRatio)}% of the round price.` : undefined}
      />
      <LinkedJob path={path} company={company} profile={profile} />
      {company.job === null && <OwnerField path={`${path}/owner`} label="Whose" />}
      <ToggleField path={`${path}/included`} label="Count it in the plan" help={HELP.included} />
      <Disclosure summary="More about the company">
        <div className="space-y-3">
          <PercentField
            path={`${path}/preference_overhang`}
            label="Money raised as a share of valuation"
            help={HELP.overhang}
            nullable
            min={0}
            max={0.95}
            placeholder={defaults ? wholePercent(defaults.overhang) : undefined}
            hint={zeroSales === null ? undefined : `In ${zeroSales} of 100 sales common stock gets nothing.`}
          />
          <NumField path={`${path}/lockup_months`} label="IPO lockup" kind="months" suffix="mo" min={0} max={24} />
          <NumField path={`${path}/exercise_window_months`} label="Exercise window after leaving" kind="months" suffix="mo" min={0} max={120} />
          {company.job !== null && <ToggleField path={`${path}/company_failure_ends_job`} label="A failure also costs the job" />}
          <ToggleField path={`${path}/qsbs`} label="Qualified small business stock" help={HELP.qsbs} />
          <MoneyField path={`${path}/amt_credit_carryforward`} label="Minimum tax credit carried forward" min={0} help={HELP.amtCredit} hint="From Form 8801, if exercising incentive options made you pay minimum tax before." />
        </div>
      </Disclosure>
    </Group>
  );
}

function Tenders({ path, company, preview }: { path: string; company: PrivateCompany; preview: EquityPreview | null }) {
  const t = `${path}/tenders`;
  const defaults = stageDefaults(company.stage);
  const inForce = company.tenders.annual_probability ?? preview?.tender_annual_probability ?? defaults?.tender ?? 0;
  const planned = company.tenders.next_in_months !== null;
  const echo = tenderLine(company);
  return (
    <Group title="Tender offers">
      <PercentField
        path={`${t}/annual_probability`}
        label="Chance per year"
        help={HELP.tenderChance}
        nullable
        min={0}
        max={1}
        placeholder={defaults ? String(Number((defaults.tender * 100).toPrecision(3))) : undefined}
        hint={tenderHint(inForce, canShowPayoffs(company) && preview ? preview.tender_before_share : null, company.tenders.annual_probability !== null && preview ? preview.tender_annual_probability : null)}
      />
      <OptionalValue path={`${t}/next_in_months`} label="A tender is planned" help={HELP.tenderPlanned} present={planned} initial={3}>
        <NumField path={`${t}/next_in_months`} label="Months from now" kind="months" suffix="mo" min={0} max={120} hint="It happens for certain, at the limit and price below." />
        {echo && (
          <p data-testid="tender-echo" className="text-xs leading-relaxed text-ink-2">
            {echo}
          </p>
        )}
      </OptionalValue>
      <PercentField path={`${t}/max_fraction`} label="Most you may sell each time" help={HELP.tenderLimit} min={0} max={1} hint="Of the shares you own and your vested options. Typically 20%." />
      {/* Open while a tender is planned: its price is one of the things just entered. */}
      <Disclosure key={planned ? 'planned' : 'none'} summary="Price and eligibility" defaultOpen={planned}>
        <div className="space-y-3">
          <PercentField
            path={`${t}/price_fraction`}
            label="Price, as a percent of the share price then"
            help={HELP.tenderPrice}
            min={LIMITS.tenderPriceMin}
            max={LIMITS.tenderPriceMax}
            hint="100% is no discount. The share price starts at the latest round's price and moves with the company's simulated fate."
          />
          <ToggleField path={`${t}/former_employees`} label="Open to people who have left" />
        </div>
      </Disclosure>
    </Group>
  );
}

function Decisions({ path, company }: { path: string; company: PrivateCompany }) {
  const p = `${path}/policy`;
  const policy = company.policy;
  const options = company.holdings.some(isOption);
  const sold = policy.tender_sell_fraction * company.tenders.max_fraction;
  return (
    <Group title="Your decisions">
      {options && (
        <ChoiceField
          path={`${p}/exercise`}
          label="Exercise options"
          options={EXERCISE}
          hint={policy.exercise === 'at_liquidity' ? 'Nothing is paid until there is a tender offer, a sale or an IPO.' : `Costs ${money(exerciseCostVested(company))} now, before tax. Only what savings above the emergency fund can pay for is exercised.`}
        />
      )}
      {options && (
        <SegmentField
          path={`${p}/at_deadline`}
          label="If options are about to lapse"
          options={AT_DEADLINE}
          hint={policy.at_deadline === 'exercise' ? 'Exercise what savings above my emergency fund can pay for, tax included. The rest lapse.' : 'Pay nothing. The options lapse and are worth nothing.'}
        />
      )}
      <SliderField
        path={`${p}/tender_sell_fraction`}
        label="In a tender offer, sell"
        help={HELP.tenderSell}
        kind="percent"
        min={0}
        max={1}
        step={0.05}
        hint={`${percent(policy.tender_sell_fraction, 0)} of what is allowed: ${percent(sold, 1)} of the shares you own and your vested options, each time.`}
      />
      <ChoiceField
        path={`${p}/sell_down`}
        label="After an IPO lockup"
        options={SELL_DOWN}
        read={(v) => (v as PrivateCompany['policy']['sell_down']).type}
        write={(type) => (type === 'over_years' ? { type, years: 3 } : { type })}
        hint={
          policy.sell_down.type === 'hold'
            ? 'Shares you own by then are kept as a single stock in the taxable account. Options and RSUs still open then are sold for cash.'
            : policy.sell_down.type === 'over_years'
              ? 'Applies to shares you own by then, from exercising earlier or bought. Options still unexercised and RSUs are sold for cash when the lockup ends, whatever is chosen here.'
              : undefined
        }
      />
      {policy.sell_down.type === 'over_years' && <NumField path={`${p}/sell_down/years`} label="Yearly sales" kind="integer" min={1} max={LIMITS.sellDownYears} hint="Equal parts, the first when the lockup ends." />}
      <OptionalValue
        path={`${p}/leave_after_months`}
        label="I plan to leave this company"
        help="Switch on to plan a departure. What is unvested in that month is forfeited and the exercise window starts. The salary under Income is not changed: it stands for whatever job you hold."
        present={policy.leave_after_months !== null} initial={12}>
        <NumField path={`${p}/leave_after_months`} label="Months from now" kind="months" suffix="mo" min={0} max={600} hint="What is unvested then is forfeited and the exercise window starts. The salary is not changed." />
      </OptionalValue>
    </Group>
  );
}

/** The fields of one open company. Mounted only while its item is open, so the preview runs only then. */
function CompanyBody({ path, company, profile }: { path: string; company: PrivateCompany; profile: Profile }) {
  const preview = usePreview(company);
  return (
    <>
      <PreviewStrip company={company} preview={preview} />
      <CompanyFields path={path} company={company} profile={profile} preview={preview} />
      <Holdings path={path} company={company} />
      <Group title="What might happen">
        <OutcomeEditor path={path} company={company} preview={preview} />
      </Group>
      <Tenders path={path} company={company} preview={preview} />
      <Decisions path={path} company={company} />
    </>
  );
}

/** The collapsed row's figure: how often it pays nothing, or the paper value when there is no preview. */
function companyFigure(company: PrivateCompany): ReactNode {
  // A company left out of the plan must not look like a counted one when collapsed.
  if (!company.included) return <span className="text-warn">Not counted</span>;
  const preview = canShowPayoffs(company) ? previewOf(company) : null;
  if (preview) return nothingFigure(preview, company);
  const paper = paperValue(company).total;
  return paper > 0 ? `${moneyCompact(paper)} on paper` : undefined;
}

export function EquitySection() {
  const profile = useProfile();
  const edit = useEdit();
  const companies = profile.private_equity;
  const full = companies.length >= LIMITS.companies;
  const addCompany = () => {
    const id = uniqueId('company', companies.map((c) => c.id));
    markFresh(`/private_equity/@${id}`);
    edit(append('/private_equity', { id, ...newCompany(profile) }), { label: 'Add a company' });
  };
  return (
    <Group>
      <ItemList empty="No private-company stock or options.">
        {companies.map((company, i) => {
          const path = join('/private_equity', at(company, i));
          return (
            <Item key={company.id || i} path={path} fallbackName="Company" figure={companyFigure(company)} detail={companyDetail(stageLabel(company.stage), company.holdings.length)}>
              <CompanyBody path={path} company={company} profile={profile} />
            </Item>
          );
        })}
      </ItemList>
      <Button size="sm" variant="ghost" icon="plus" className="-ml-2" disabled={full} onClick={addCompany}>
        Add a company
      </Button>
      {full && <p className="text-xs text-ink-3">A plan can hold at most {LIMITS.companies} private companies. Combine or remove the smallest positions.</p>}
      <p className="text-xs leading-relaxed text-ink-3">For stock and options in a company that is not publicly traded. Publicly traded RSUs stay on the job that grants them, under Income.</p>
    </Group>
  );
}
