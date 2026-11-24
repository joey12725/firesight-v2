import type { Assumptions, Stress } from '@/engine/types';
import { append, remove, set } from '@/state/patches';
import { Button, IconButton, Popover } from '@/ui';
import { ChoiceField, Group, NumField, Optional, PercentField, Realism, Row, SegmentField, SliderField, ToggleField } from '../fields';
import { useEdit, useProfile } from '../hooks';
import { MAKE, lifeRealism, marketRealism, taxRealism } from '../realism';
import { WhenField } from '../WhenField';

type Market = Assumptions['market']['kind'];

const MODELS: { value: Market['type']; label: string; explain: string; make: () => unknown }[] = [
  {
    value: 'bootstrap',
    label: 'History, reshuffled',
    explain:
      'Stitches futures together from multi-year stretches of real market history, keeping stocks, bonds and inflation in step. Crashes, recoveries and long droughts look like the real ones. It cannot produce anything worse than history did.',
    make: () => ({ type: 'bootstrap' }),
  },
  {
    value: 'regime',
    label: 'Regimes',
    explain: 'A statistical model that switches between calm and crisis periods. It can produce futures history never did, at the price of resting on fitted parameters you cannot inspect in a chart of the past.',
    make: () => ({ type: 'regime' }),
  },
  {
    value: 'lognormal',
    label: 'Textbook random walk',
    explain: 'Each month is an independent draw with the historical average and volatility. Simple and widely used; it has no crashes that cluster and no recoveries, so it understates both short-term risk and long-term stability.',
    make: () => ({ type: 'lognormal' }),
  },
  {
    value: 'fat_tailed',
    label: 'Random walk with fat tails',
    explain: 'The textbook model with the same averages, plus what it leaves out: rare extreme months, volatile spells that cluster and fade, and markets that fall together in a sell-off. Crashes are deeper and more frequent than in the plain random walk.',
    make: () => ({ type: 'fat_tailed' }),
  },
  {
    value: 'historical',
    label: 'History, replayed',
    explain: 'Each path starts in a different real month and lives through what followed, in order. Easy to audit, but there are only so many decades of history, and the recent starts run out of future.',
    make: () => ({ type: 'historical' }),
  },
];

const STRESSES: { type: Stress['type']; label: string; note: string; make: () => unknown }[] = [
  { type: 'crash_at_retirement', label: 'Crash at retirement', note: 'Stocks fall hard in the year you retire', make: () => ({ type: 'crash_at_retirement' }) },
  { type: 'lost_decade', label: 'Lost decade', note: 'Ten years in which stocks return nothing after inflation', make: () => ({ type: 'lost_decade', start: { type: 'retirement', offset_years: 0 } }) },
  { type: 'stagflation', label: 'Stagflation', note: 'High inflation with weak stock and bond returns', make: () => ({ type: 'stagflation', start: { type: 'retirement', offset_years: 0 } }) },
  { type: 'historical_episode', label: 'A historical episode', note: '1929, 1966, 2000 or 2008, replayed as it happened', make: MAKE.historicalEpisode },
  { type: 'lost_generation', label: 'Lost generation', note: 'Decades of flat stocks, falling prices and near-zero rates', make: MAKE.lostGeneration },
];

const EPISODES = [
  { value: 'great_depression1929', label: 'Great Depression, 1929 to 1945' },
  { value: 'stagflation1966', label: 'Stagflation, 1966 to 1982' },
  { value: 'dot_com2000', label: 'Dot-com bust and after, 2000 to 2013' },
  { value: 'financial2008', label: 'Financial crisis, 2007 to 2013' },
] as const;

function StressRow({ stress, index }: { stress: Stress; index: number }) {
  const path = `/assumptions/market/stress/${index}`;
  const edit = useEdit();
  const meta = STRESSES.find((s) => s.type === stress.type);
  return (
    <li data-path={path} className="space-y-3 rounded-md border border-line bg-surface-2 p-3">
      <div className="flex items-center justify-between">
        <span className="text-sm font-medium text-ink-1">{meta?.label}</span>
        <IconButton icon="trash" size="sm" variant="danger" label={`Remove ${meta?.label}`} onClick={() => edit(remove(path), { label: `Remove ${meta?.label}` })} />
      </div>
      {stress.type === 'crash_at_retirement' && (
        <Row>
          <PercentField path={`${path}/drawdown`} label="Fall" />
          <NumField path={`${path}/recovery_years`} label="Recovery" kind="years" suffix="yrs" />
        </Row>
      )}
      {stress.type === 'lost_decade' && <WhenField path={`${path}/start`} label="Starts" kinds={['retirement', 'date', 'age', 'start']} />}
      {stress.type === 'stagflation' && (
        <>
          <WhenField path={`${path}/start`} label="Starts" kinds={['retirement', 'date', 'age', 'start']} />
          <Row>
            <NumField path={`${path}/years`} label="Lasts" kind="years" suffix="yrs" />
            <PercentField path={`${path}/inflation`} label="Inflation" />
          </Row>
        </>
      )}
      {stress.type === 'historical_episode' && (
        <>
          <ChoiceField path={`${path}/episode`} label="Episode" options={EPISODES} />
          <WhenField path={`${path}/start`} label="Starts" kinds={['retirement', 'date', 'age', 'start']} />
        </>
      )}
      {stress.type === 'lost_generation' && (
        <>
          <WhenField path={`${path}/start`} label="Starts" kinds={['retirement', 'date', 'age', 'start']} />
          <NumField path={`${path}/years`} label="Lasts" kind="years" suffix="yrs" />
        </>
      )}
    </li>
  );
}

export function AssumptionsSection() {
  const { assumptions, household } = useProfile();
  const edit = useEdit();
  const m = '/assumptions/market';
  const model = assumptions.market.kind;
  const taxes = assumptions.taxes;
  return (
    <>
      <Group title="Markets">
        <ChoiceField path={`${m}/kind`} label="How futures are generated" options={MODELS} read={(v) => (v as Market).type} write={(type) => MODELS.find((x) => x.value === type)?.make()} />
        <p className="text-xs leading-relaxed text-ink-2">{MODELS.find((x) => x.value === model.type)?.explain}</p>
        {model.type === 'bootstrap' && (
          <Row>
            <NumField path={`${m}/kind/mean_block_months`} label="Typical stretch length" kind="months" suffix="mo" />
            <NumField path={`${m}/kind/from_year`} label="Use history from" kind="integer" nullable placeholder="All of it" />
          </Row>
        )}
        <SliderField
          path={`${m}/equity_premium_adjustment`}
          label="Stock returns versus history"
          kind="percent"
          min={-0.03}
          max={0.02}
          step={0.0025}
          hint="Per year, against the US record since 1926."
        />
        <p className="text-xs leading-relaxed text-ink-2" data-testid="equity-adjustment-note">
          {Math.abs(assumptions.market.equity_premium_adjustment + 0.015) < 1e-9 ? (
            <>
              <b className="font-medium text-ink-1">The default is −1.5 points, not zero.</b> US stocks returned 7.2% a year after inflation since 1926, the best large market of the century; the rest of the world managed about
              4.5%, and estimates from today’s prices run 3 to 5%. Planning on the full record would lift “the plan holds” by roughly 2 to 7 lives in 100.
            </>
          ) : Math.abs(assumptions.market.equity_premium_adjustment) < 1e-9 ? (
            <>
              <b className="font-medium text-ink-1">Using the US record as it was</b> (7.2% a year after inflation), the most generous setting most planners would defend. The default is −1.5 points.
            </>
          ) : (
            <>Stocks are planned at {(assumptions.market.equity_premium_adjustment * 100).toFixed(2).replace(/\.?0+$/, '').replace('-', '−')} points a year against the US record of 7.2% after inflation. The default is −1.5 points.</>
          )}{' '}
          {Math.abs(assumptions.market.equity_premium_adjustment + 0.015) >= 1e-9 && (
            <button type="button" className="rounded-xs font-medium text-accent-ink hover:underline" onClick={() => edit(set(`${m}/equity_premium_adjustment`, -0.015), { label: 'Reset stock returns to the default' })}>
              Reset to the default
            </button>
          )}
        </p>
        <SliderField path={`${m}/inflation_adjustment`} label="Inflation versus history" kind="percent" min={-0.02} max={0.03} step={0.0025} hint="Per year, added to simulated inflation." />
        <Realism summary="Valuations, bond pricing and dividend yields" on={marketRealism(assumptions.market)}>
          {model.type !== 'historical' && (
            <Optional path={`${m}/valuation`} label="Start from today’s stock valuation" present={assumptions.market.valuation !== null} make={MAKE.valuation}>
              <Row cols={3}>
                <NumField path={`${m}/valuation/starting_cape`} label="CAPE today" step={1} />
                <NumField path={`${m}/valuation/fair_cape`} label="Normal CAPE" step={0.5} />
                <NumField path={`${m}/valuation/half_life_years`} label="Half-life" step={1} suffix="yrs" />
              </Row>
            </Optional>
          )}
          {model.type === 'bootstrap' && (
            <SegmentField
              path={`${m}/bond_pricing`}
              label="Bond returns"
              options={[
                { value: 'sampled', label: 'From history' },
                { value: 'from_yield', label: 'Priced from the yield' },
              ]}
            />
          )}
          <ToggleField path={`${m}/dynamic_yields`} label="Dividend and interest yields move with the market" />
        </Realism>
      </Group>

      <Group title="Stress tests">
        {assumptions.market.stress.length > 0 ? (
          <ul className="space-y-1.5">
            {assumptions.market.stress.map((s, i) => (
              <StressRow key={`${s.type}-${i}`} stress={s} index={i} />
            ))}
          </ul>
        ) : (
          <p className="text-xs leading-snug text-ink-3">None. A stress test forces a bad episode into every simulated life, on top of the usual randomness.</p>
        )}
        <Popover
          label="Add a stress test"
          className="w-64 p-1"
          trigger={
            <Button size="sm" variant="ghost" icon="plus" className="-ml-2">
              Add a stress test
            </Button>
          }
        >
          {(close) => (
            <ul>
              {STRESSES.map((s) => (
                <li key={s.type}>
                  <button
                    type="button"
                    className="block w-full rounded-sm px-2.5 py-1.5 text-left hover:bg-surface-3"
                    onClick={() => {
                      edit(append(`${m}/stress`, s.make()), { label: `Add stress test: ${s.label}` });
                      close();
                    }}
                  >
                    <span className="block text-sm font-medium text-ink-1">{s.label}</span>
                    <span className="block text-xs text-ink-3">{s.note}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </Popover>
      </Group>

      <Group title="Taxes">
        <Row>
          <ChoiceField
            path="/assumptions/taxes/kind"
            label="Tax model"
            options={[
              { value: 'us_current_law', label: 'US federal and state law' },
              { value: 'flat', label: 'One flat rate' },
              { value: 'none', label: 'No taxes' },
            ]}
            read={(v) => (v as typeof taxes.kind).type}
            write={(type) => (type === 'flat' ? { type, rate: 0.2 } : { type })}
          />
          {taxes.kind.type === 'flat' && <PercentField path="/assumptions/taxes/kind/rate" label="Rate" />}
        </Row>
        {taxes.kind.type === 'us_current_law' && <ToggleField path="/assumptions/taxes/index_brackets" label="Brackets keep up with inflation" />}
        <p className="text-xs leading-snug text-ink-3">The Social Security benefit cut is set under Income.</p>
        <Realism summary="Chained CPI, state detail and policy risk" on={taxRealism(taxes)}>
          {taxes.kind.type === 'us_current_law' && (
            <>
              <ToggleField path="/assumptions/taxes/chained_cpi" label="Index thresholds to the chained CPI" />
              <SegmentField
                path="/assumptions/taxes/state_detail"
                label="State rules"
                options={[
                  { value: 'basic', label: 'Basic' },
                  { value: 'full', label: 'Full (CA, NY, UT extras)' },
                ]}
              />
            </>
          )}
          <Optional path="/assumptions/taxes/policy_risk" label="Policy may change along the way" present={taxes.policy_risk !== null} make={MAKE.policyRisk}>
            <Row cols={3}>
              <PercentField path="/assumptions/taxes/policy_risk/ss_haircut_probability" label="Chance of a benefit cut" />
              <PercentField path="/assumptions/taxes/policy_risk/ss_haircut_factor" label="Share still paid" />
              <NumField path="/assumptions/taxes/policy_risk/earliest_year" label="Not before" kind="integer" />
            </Row>
            <Row>
              <PercentField path="/assumptions/taxes/policy_risk/tax_increase_probability" label="Chance of a tax rise" />
              <PercentField path="/assumptions/taxes/policy_risk/tax_increase_points" label="Points added to each rate" />
            </Row>
          </Optional>
        </Realism>
      </Group>

      <Group title="Lifespan and risks">
        <Row>
          <ChoiceField
            path="/assumptions/longevity"
            label="Lifespans"
            options={[
              { value: 'stochastic', label: 'Vary, from life tables' },
              { value: 'fixed_age', label: 'Everyone lives to an age' },
            ]}
            read={(v) => (v as typeof assumptions.longevity).type}
            write={(type) => (type === 'fixed_age' ? { type, age: 95 } : { type, improvement: true })}
          />
          {assumptions.longevity.type === 'fixed_age' ? (
            <NumField path="/assumptions/longevity/age" label="Age" kind="years" />
          ) : (
            <NumField path="/simulation/end_age" label="Plan no further than" kind="integer" suffix="yrs old" />
          )}
        </Row>
        {assumptions.longevity.type === 'stochastic' && <ToggleField path="/assumptions/longevity/improvement" label="People keep living longer" />}
        <ToggleField path="/assumptions/career_risk" label="Career risk: layoffs and pay shocks" />
        <ToggleField path="/assumptions/health_risk" label="Health risk: medical and care costs vary" />
        <Realism summary="Linked lifespans, health states and slow home sales" on={lifeRealism(assumptions)}>
          {assumptions.longevity.type === 'stochastic' && (
            <>
              {household.people.length > 1 && (
                <SegmentField
                  path="/assumptions/longevity/couple_dependence"
                  label="Partners’ lifespans"
                  options={[
                    { value: 'independent', label: 'Independent' },
                    { value: 'linked', label: 'Linked' },
                  ]}
                  read={(v) => (v as { type: 'independent' | 'linked' }).type}
                  write={(type) => ({ type })}
                />
              )}
              <ToggleField path="/assumptions/longevity/income_gradient" label="Higher incomes live longer" />
              {assumptions.longevity.income_gradient && (
                <PercentField path="/assumptions/longevity/income_percentile" label="Household income percentile" nullable placeholder="From the plan’s income" />
              )}
            </>
          )}
          <SegmentField
            path="/assumptions/health_model"
            label="Health over time"
            options={[
              { value: 'independent', label: 'Independent draws' },
              { value: 'states', label: 'Health states' },
            ]}
            read={(v) => (v as typeof assumptions.health_model).type}
            write={(type) => ({ type })}
          />
          <SegmentField
            path="/assumptions/housing_liquidity"
            label="Selling a home"
            options={[
              { value: 'instant', label: 'Closes at once' },
              { value: 'realistic', label: 'Takes time' },
            ]}
          />
        </Realism>
      </Group>
    </>
  );
}
