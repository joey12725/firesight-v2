# FIRESight v2 architecture

This is the engineering contract for the rewrite. `docs/v1-review.md` says what the
Python version did and where it was wrong; `docs/methodology.md` explains the model to
users. This file says how the code is laid out and what each part must do.

## 1. Shape of the system

```
crates/
  firesight-core/    pure Rust, no I/O, no threads, no wall clock. The whole model.
  firesight-wasm/    wasm-bindgen wrapper around core (thin, JSON + typed arrays)
  firesight-cli/     native runner (rayon): run, compare, solve, bench, trace, schema
data/                embedded parameter sets (tax law, market history, life tables)
tools/               python3 stdlib scripts that rebuild data/ from public sources
web/                 Vite + React + TypeScript UI; engine runs in a Web Worker pool
api/                 one Vercel function: optional streaming proxy to the Claude API
docs/                methodology, v1 review, research notes
examples/            the four demo households (v2 schema) and the three v1 profiles for migration tests
```

The browser does all simulation. There is no backend state: profiles live in the
browser (IndexedDB) and travel as JSON files or share links. The only server code is
the optional LLM proxy.

### Determinism rules (core)

* `f(profile, data, seed, path_index)` is a pure function. Same inputs, same bits, on
  native and wasm. No `HashMap` iteration order in results, no time, no global RNG.
* Every random subsystem draws from its own named stream:
  `Rng::stream(seed, path_index, Stream::X)`, with `Stream` in
  `{Market, Inflation, Career(person), EquityComp, Health(person), Longevity(person),
  Shocks, Housing(property), Events, PrivateCompany(company)}`. Changing a decision in
  a scenario must not shift the random numbers of any unrelated subsystem. This is what
  makes scenario comparison a paired experiment (common random numbers), and it is the
  v1 idea we keep and make exact.
* `PrivateCompany(i)` is the stream of the `i`-th entry of `private_equity`. It gives
  exactly four words at path start (the company's fate: which outcome, when, at what
  price, and the price after the lockup) and exactly one word every month (whether a
  tender offer arrives), drawn whether or not it is used. No other stream is read for a
  company and a company reads no other stream, so adding one, counting it at zero or
  changing any decision about it leaves every other draw of the path where it was, and
  the company's own fate too.
* Path `i` can be re-simulated alone at any time with a full ledger (`trace_path`).
  The aggregate run therefore never stores per-path time series.

### Units

* Every money amount in a `Profile` is in **today's dollars** (dollars at
  `profile.start`). The engine simulates in nominal dollars with a simulated CPI and
  deflates on output. Every output is in today's dollars unless its name ends in
  `_nominal`.
* Rates are annual decimal fractions (`0.04`), never percents.
* Time is `YearMonth`. One step is one calendar month. Ages are derived from birth
  `YearMonth`, never accumulated floats.

## 2. The profile schema (`core::model`)

Rust types are the single source of truth. `schemars` derives a JSON Schema from them
(`firesight-cli schema > web/src/engine/profile.schema.json`), TypeScript types are
generated from that schema, and the same schema is what the AI tools validate against.
All structs: `#[serde(default, deny_unknown_fields, rename_all = "snake_case")]`, enums
internally tagged with `"type"`. Every list element that can be patched has a stable
string `id`. Doc comments on every field (they become schema descriptions that the UI
shows as help text and the model reads).

```text
Profile {
  schema_version: 2, name, start: Option<YearMonth>,
  household:  Household,
  incomes:    Vec<Income>,
  private_equity: Vec<PrivateCompany> (0..=4),
  spending:   Spending,
  accounts:   Vec<Account>,
  portfolio:  PortfolioPolicy,
  properties: Vec<Property>,
  debts:      Vec<Debt>,  debt_policy: DebtPolicy,
  events:     Vec<LifeEvent>,
  retirement: RetirementPlan,
  assumptions: Assumptions,
  simulation: SimulationSettings,
}

Household { people: Vec<Person> (1..=2), children: Vec<Child>, location: Location }
Person    { id, name, birth: YearMonth, sex: Female|Male|Unspecified,
            health: Excellent|Good|Average|Poor,     // scales mortality; default Good (0.85x)
            joins: Option<YearMonth>,               // future partner: not in household before this
            social_security: SocialSecurity }
SocialSecurity { claim_age: f64 (62..=70, default 67),
                 basis: FromEarnings { career_start_age (default 22) }
                      | KnownBenefit { monthly_at_fra }      // from an SSA statement, today's $
                      | None }
Child     { id, name, birth: YearMonth (may be in the future = planned),
            costs: Modeled | InBudget,              // Modeled: USDA-style cost by age and income tier
            childcare_monthly: Option<f64>,         // ages 0-4 override
            college: None | PublicInState | PublicOutOfState | Private | Custom{annual},
            college_share: f64 (fraction parents pay, default 1.0) }
Location  { country (default "US"), state: Option<String> (2-letter), city: Option<String>,
            foreign_tax_rate: Option<f64> }         // flat effective income tax when not in the US

Income { id, name, owner: usize (person index), kind, start: Option<When>, end: Option<When> }
When   = Date{at: YearMonth} | Age{person, age} | Retirement{offset_years} | Start
IncomeKind =
  Job { salary, growth: Flat{real_raise} | Lifecycle{profile: Flat|Typical|Steep, extra},
        risk: CareerRisk { level: None|Low|Typical|High }   // wage shocks + layoff hazard
        bonus: Option<Bonus{ target_fraction, volatility }>,
        commission: Option<Commission{ expected_annual, volatility }>,
        equity: Vec<EquityGrant>,
        plan: Option<EmployerPlan>, hsa_contribution: f64, hsa_employer: f64,
        ends_at_retirement: bool (true) }
  SelfEmployment { net_annual, volatility, real_growth, ends_at_retirement, solo_401k_fraction }
  Pension  { annual, cola: Full|None|Partial{rate}, survivor_fraction }
  Annuity  { annual, real: bool }
  Other    { annual, taxable: bool, real_growth, ends_at_retirement }
EmployerPlan { employee_fraction (of salary, or) employee_max: bool, roth_fraction,
               match_rate, match_up_to (fraction of salary), after_tax_mega: bool(false) }
EquityGrant { id, kind: Rsu | Option{ strike_fraction },     // strike as fraction of current price
              company: Public{ volatility, beta } | Private{ liquidity_hazard, fail_prob, .. },
              unvested_value, vest_months_remaining, cliff_months,
              refresh_annual_value,                // new grants each year while employed
              sell_on_vest: bool }
// Job.equity keeps the older single-event model for a private employer. A position
// worth modelling in detail goes in private_equity instead:

PrivateCompany { id, name, owner: usize, job: Option<income id>,   // the job that vests it; None = former employer or investment
                 included: bool (true),               // false: counted at zero; the fate is still simulated
                 stage: Seed|SeriesA|SeriesB|SeriesC|Late|Unicorn,  // preset odds, timing, prices, tender rate
                 preferred_price,                     // latest round, nominal $ per share; every outcome price is a multiple of it
                 fmv_409a: Option<f64>,               // value an exercise is taxed on; default a stage fraction of the price
                 preference_overhang: Option<f64>,    // money raised / post-money; common gets nothing at or below it
                 outlook: Pessimistic|Typical|Optimistic,           // failure 10 points more or (up to) less likely
                 outcomes: Vec<OutcomeRow>,           // the user's own table; empty = the stage preset
                 tenders: TenderOffers, lockup_months (6), exercise_window_months (3),
                 company_failure_ends_job: bool (true), qsbs: bool (false),
                 amt_credit_carryforward: f64,        // Form 8801 credit from earlier years
                 holdings: Vec<EquityHolding> (0..=8), policy: EquityPolicy }
OutcomeRow   { id, kind: Fails|Acquired|Ipo|StillPrivate, probability,     // rows are exclusive and sum to 1
               years_from, years_to,                  // uniform date in the range
               multiple_low, multiple_high }          // 10th and 90th percentile, x the last round (whole company for an acquisition)
TenderOffers { annual_probability: Option<f64> (stage default), next_in_months: Option<u32>,
               max_fraction (0.2), price_fraction (1.0), former_employees: bool }
EquityHolding { id, name, kind, shares, vested, vest_months_left, cliff_months }
HoldingKind  = Iso{ strike, granted_months_ago } | Nso{ strike, granted_months_ago }
             | Rsu{ must_be_employed_at_event }
             | Shares{ basis_per_share, months_held, from_iso, fmv_at_exercise }
EquityPolicy { exercise: AtLiquidity | VestedNow | AsTheyVest,
               at_deadline: Exercise (if affordable) | Forfeit,     // when options would lapse
               tender_sell_fraction (0.5),
               sell_down: AllAtLockupEnd | OverYears{years} | Hold,
               leave_after_months: Option<u32> }       // leave the company, keep the salary

Spending { items: Vec<BudgetItem>,
           lifestyle_creep: f64,                   // share of REAL income growth that becomes spending (0..1). Not a perpetual rate.
           retirement_smile: bool,                 // Blanchett real decline through the 70s, healthcare rises separately
           shocks: Off|Typical|High,               // irregular expenses, compound Poisson
           healthcare: Healthcare { mode: Modeled|InBudget, early_retirement: Aca|Cobra|Custom{annual},
                                    long_term_care: bool, quality: Bronze|Silver|Gold },
           survivor_factor: f64 (0.7) }
BudgetItem { id, name, category: Category, amount, frequency: Monthly|Annual|Weekly|Biweekly|Quarterly,
             essential: bool, real_growth: Option<f64> (default by category),
             start: Option<When>, end: Option<When>,
             in_retirement: Same | Stops | Factor{x} | Amount{x},
             owner: Option<usize> }
Category = Rent | Utilities | Groceries | Dining | Transport | Healthcare | Insurance | Childcare
         | Education | Travel | Entertainment | Personal | Giving | Other

Account { id, name, owner: usize, kind: Cash|Taxable|Traditional|Roth|Hsa,
          balance, cost_basis: Option<f64>,          // taxable: default 60% of balance
          roth_basis: Option<f64>,                   // Roth contributions (penalty-free), default 50%
          cash_yield: TracksBills{spread} | Fixed{rate},   // Cash only
          holdings: Option<Allocation> }             // current mix if it differs from the target
PortfolioPolicy { target: Allocation, glide: None | TargetDate{..} | BondTent{..} | Custom{points},
                  rebalance: Never | Annual | Bands{width},
                  location: Mirror | TaxEfficient,
                  expense_ratio: f64, advisory_fee: f64,
                  emergency_months: f64 (6),          // of current spending, recomputed every year
                  retirement_cash_years: f64 (0..5),  // bucket: refill only when equities are not in drawdown
                  custom_asset: Option<CustomAsset{ name, mean, volatility, equity_corr, income_yield }>,
                  single_stock: SingleStock{ volatility, beta } }
Allocation = map AssetClass -> weight (normalised)
AssetClass = UsStocks | IntlStocks | Bonds | Cash | RealEstate | Gold | Crypto | SingleStock | Custom

Property { id, name, use: Primary | Rental{..RentalTerms} | Vacation,
           value, basis: Option<f64>, mortgage: Option<Mortgage{ balance, rate, remaining_months, payment: Option }>,
           property_tax_rate: Option<f64> (default from state), insurance_rate, maintenance_rate, hoa_monthly,
           appreciation_vs_national: f64, idiosyncratic_volatility: f64 }
RentalTerms { monthly_rent, vacancy, management_fee, rent_real_growth, land_fraction }

Debt { id, name, kind: Student|Auto|CreditCard|Personal|Other, balance, rate,
       payment: Option<f64>, remaining_months: Option<u32> }
DebtPolicy { strategy: Minimum | Avalanche | Snowball, extra_monthly: f64,
             refinance_mortgages: bool (true) }  // each January, if the market rate is 1+ point lower and 4 years of saving exceed the costs

LifeEvent { id, name, when: When, probability: f64 (1.0), timing_uncertainty_years: f64 (0), kind }
EventKind =
  BuyHome { price, down_payment_fraction, rate: Option<f64> (None = simulated market rate), term_years,
            replaces_rent: bool, closing_cost_fraction, ..property params }
  SellProperty { property: id, selling_cost_fraction, then_rent_monthly: Option<f64> }
  Relocate { state, city, country, moving_cost, cost_of_living: Unchanged | Factor{overall} | ByCategory{map} }
  Windfall { amount, taxable: bool }
  Expense  { amount, financed: Option<{ down_fraction, rate, term_months }> }
  IncomeChange { income: id, change: SetSalary{x} | Scale{x} | End }
  CareerBreak { person, months, income_fraction }
  SpendingChange { target: Item{id} | Category{c} | All, change: Scale{x} | Set{x} }
  ContributionChange { income: id, employee_fraction }

RetirementPlan {
  trigger: AtAge{person, age} | AtDate{at}
         | WhenFunded{ withdrawal_rate (0.04), basis: Investable|NetWorth, count_future_income: bool,
                       min_age, max_age }
         | FixedNumber{ amount, basis }
         | AlreadyRetired,
  partner_offset_years: f64,                     // second earner keeps working this much longer (can be negative)
  spending_rule: ConstantReal | PercentOfPortfolio{rate} | Guardrails{upper, lower, adjust}
               | VanguardDynamic{ceiling, floor, rate?} | Vpw | FloorCeiling{floor, ceiling, rate?},
  withdrawal_order: Conventional | Proportional
                  | TaxAware{ fill_to_bracket: f64, roth_conversions: bool, mind_aca: bool },
  early_access: Auto | PayPenalty,               // Auto: taxable, Roth basis, seasoned conversions, 72(t)
  bequest_target: f64,
  home_equity: Never | LastResort (default),     // LastResort: sell property before cutting essentials; reported as home_sale_rate
}
Assumptions {
  market: MarketModel { kind: Bootstrap{ mean_block_months (60), from_year: Option } | Regime | Lognormal
                              | Historical,       // every start month in history, in order: path i = cohort i
                        equity_premium_adjustment: f64 (annual, default -0.015: US 1926+ history less 1.5 points),
                        inflation_adjustment: f64,
                        stress: Vec<Stress> },
  Stress = CrashAtRetirement{ drawdown, recovery_years }  // back to the pre-crash nominal level after recovery_years, not to the unstressed path
         | LostDecade{ start: When }
         | Stagflation{ start: When, years, inflation },
  taxes: TaxModel { kind: UsCurrentLaw | Flat{rate} | None, index_brackets: bool (true),
                    social_security_haircut: Option<{ from_year, factor }> },
  longevity: Stochastic{ improvement: bool } | FixedAge{ age },
  career_risk: bool (true), health_risk: bool (true),
}
SimulationSettings { paths: u32 (10_000), seed: u64, end_age: u32 (100) }
```

`core::legacy` converts a v1 profile dict (the format in `examples/v1/*.json`) into this
schema, with a list of the assumptions it had to make.

`core::validate` returns hard errors (cannot simulate) and warnings (plausibility lints:
spending above income with no assets, no healthcare line and healthcare `InBudget`,
allocation with more than 20% crypto, retirement spending below 50% of current, and so
on). The UI shows them; the AI reads them. For `private_equity` the errors cover the
references (owner, linked job, one company per job), the bounds on prices, shares and
dates, an outcome table that does not sum to 1, and holdings that cannot exist (unvested
units with no job to vest them); the warnings cover a position many times the
household's savings (`equity_dominant`), an incentive grant over the $100,000 a year
limit, an exercise the savings cannot pay for, a table more optimistic than the stage's
typical odds, and several companies (the outcome statistics describe the largest).

`core::patch` applies a list of operations to a profile JSON value:
`{op: "set"|"remove"|"append", path, value}` where `path` is a JSON pointer in which an
array segment may be `@<id>` to address an element by its `id`. The result is
re-validated. Scenarios are stored as `{name, patch}` against a base profile, never as
full copies.

## 3. One simulated life (`core::sim`)

Compile once per profile (`Plan::compile(&Profile, &Data) -> Plan`): resolve ids to
indices, convert frequencies to monthly, pre-index tax tables, precompute amortisation,
lifecycle wage curve, life-table CDFs. Then per path, with all buffers reused:

```
draw lifetimes (Longevity streams) -> horizon T months (capped by end_age)
economy.generate(T)   -> per month: cpi, returns[asset], tbill, long_yield, mortgage_rate,
                         home_national, recession flag
resolve stochastic events (probability, timing uncertainty) on the Events stream

for each month t:
  1. calendar, ages, deaths (survivor transition), partner joining, children ageing in/out
  2. events due this month
  3. career: wage shock (annual), layoff / re-employment hazard (depends on recession flag),
     bonus month, vesting, liquidity events
  3a. private-company equity (only when the plan has any), per company, after one
     word is drawn for each: link to the job (start, or sever on a layoff, a planned
     departure, retirement or death, which forfeits what is unvested and starts the
     exercise window); death of the owner (basis stepped up, passed to the survivor);
     vesting; exercise now (`vested_now`, `as_they_vest`); the terminal event in its
     fate month (failure: everything worthless and the basis a capital loss, the linked
     job lost; acquisition: everything paid in cash; IPO: lockup starts); a tender
     offer; lockup end (options and RSUs settled for cash, shares sold, scheduled or
     moved to the single-stock sleeve); option deadlines (exercise what is affordable
     or lapse); scheduled sales after the lockup; write-off after thirty years
  4. income: wages, self-employment, pensions, Social Security (claim, COLA, earnings test),
     rental net operating income
  5. payroll: FICA, withholding at the year's planned rate, 401(k)/HSA deferrals + match
  6. spending: budget (CPI x category real growth x lifestyle creep x retirement rule),
     children, healthcare premium + out of pocket, shocks, long-term care
  7. housing: P&I (fixed nominal), property tax, insurance, maintenance, PMI; appreciation
  8. debts: minimum payments, then the extra payment by strategy
  9. cash settlement: surplus -> emergency fund -> savings waterfall (HSA, IRA, taxable);
     deficit -> cash buffer -> withdrawal plan (ordering, penalties, realised gains)
 10. returns applied to every bucket and asset class, fees, dividends/interest tallied for tax
 11. FI / retirement trigger check; insolvency check
 12. record (annual sample every December, or every month when tracing)

  each January: annual plan. Index tax parameters and limits by CPI; set this year's
     retirement spending by the spending rule; choose Roth conversion amount and withdrawal
     sources; estimate tax for withholding; RMDs; glide path target; rebalance.
  each April: settle last year's actual tax against what was withheld/estimated.
```

Private-company equity and the tax year. `equity::CompanyModel::step` returns the
month's cash and tax amounts and draws nothing; `sim/equity.rs` books them. Pay-type
income (non-qualified spreads, RSUs, cashed-out options) bears FICA and federal
supplemental withholding (22%, 37% above $1,000,000 in the year) in the event month.
The month after any event that moved money or tax, the year is planned again, as it is
each January, so that the rest of the tax (state, the federal shortfall, the net
investment income tax, the AMT) is paid in level instalments over the months left; an
event in December is settled in April. Cash owed is not swept into investments and does
not count toward the retirement test: the event month holds back the estimated tax, the
re-plan holds back the instalments still to pay, and the test subtracts both and the
April balance of a year with an equity event. An exercise is paid from cash and taxable
savings above the emergency fund (the retirement cash reserve once retired) and above
tax owed, with the tax it causes counted in its cost, and never from a retirement
account.

The one-off rule for a first year that starts part-way through: regular income is taxed
at its annual rate and scaled back to the months simulated, as before, but an equity
amount is an event, not a rate, so it is not multiplied up. The year's return is computed
without the one-off amounts (annualised, scaled back) and with them added unscaled; the
tax is the first plus the whole of the difference. The same pair of returns gives the
tax the equity caused, which is what `equity_net_proceeds` subtracts.

### What "financially independent", "retired" and "success" mean

* **FI date**: first month the trigger is satisfied. For `WhenFunded` the test is
  `investable assets >= (planned retirement spending incl. modelled healthcare and estimated
  tax - present value of income that continues) / withdrawal_rate`. Home equity is not
  spendable and does not count unless `basis: NetWorth`.
* **Retired**: job incomes with `ends_at_retirement` stop at the FI date (plus the partner
  offset).
* **Depleted**: liquid accounts (cash, taxable, traditional, Roth, HSA) cannot cover the
  month's essential spending. Recorded with age; the path continues on Social Security
  and other income with spending cut to what is available, so the size of the shortfall
  is measured, not just the fact of it.
* **Success**: never depleted while anyone is alive, the bequest target met, and
  retired (or died first) by the plan's latest retirement age. Reported
  together with spending actually delivered vs planned, because a path that "succeeds" by
  cutting spending 40% under a guardrail rule is not the same as one that did not.

## 4. Subsystem modules (each a file or directory in `core/src`, each with unit tests)

| Module | Responsibility |
|---|---|
| `model` | The schema above, defaults, schemars derives |
| `rng` | SplitMix64 seeding, xoshiro256++, normal / student-t / lognormal / exponential / poisson, named streams |
| `data` | Embedded datasets parsed once: `data/history/monthly.csv`, `data/tax/*.json`, `data/actuarial/*` |
| `economy` | Joint scenario generator: stationary bootstrap, regime-switching, lognormal, historical cohorts, stress overlays; derived asset classes (intl, REIT, gold, crypto, single stock, custom) from factor exposures + idiosyncratic noise |
| `tax` | Federal (ordinary, LTCG stacking, NIIT; the AMT with its credit carryforward, computed only for plans with `private_equity`; incentive-option income without FICA; the QSBS exclusion), FICA and SE tax, state, credits, Social Security benefit taxation, standard vs itemised, ACA premium credit, IRMAA, early-withdrawal penalty, RMD; all thresholds indexable by a CPI factor |
| `benefits` | Social Security: indexed earnings record -> AIME -> PIA -> claim adjustment, spousal and survivor, trust-fund haircut |
| `career` | Lifecycle wage curve, persistent/transitory shocks, layoff and re-employment, bonus, commission, equity compensation |
| `equity` | Private-company positions: the outcome table in force (stage preset or custom), the fate draw, the price and 409A paths, tender offers and the IPO window, vesting, exercise, lots with holding periods and incentive-option status, sales, QSBS, and the standalone preview. Contract: `CompanyModel` and `CompanyState` are fixed-size and `Copy` (at most 4 companies, 8 holdings, 16 lots, 8 events a month); every draw happens before the month's `EquityClimate` is built, so `step` takes no `Rng`; nothing in the month loop allocates; exercise decisions see the last round's price, never the fate |
| `health` | Premiums by phase (employer, ACA, Medicare), out-of-pocket, long-term care, mortality draws |
| `housing` | Properties, mortgages, PMI, purchase and sale, rentals, depreciation |
| `accounts` | Buckets (cash, taxable with basis, traditional, Roth with basis and conversion tranches, HSA) x asset classes; contributions waterfall; withdrawals; rebalancing; glide path |
| `decumulation` | Spending rules and the annual withdrawal / Roth-conversion plan |
| `sim` | `Plan`, path state, the monthly loop, trigger logic |
| `stats` | Mergeable accumulators and the final `Summary` |
| `scenario` | Patch application, lever definitions for sensitivity, bisection solver state machine, grid sweeps |
| `legacy` | v1 profile migration |
| `validate` | Errors and warnings |

## 5. Results (`core::stats`)

`Accumulator` is mergeable (`merge(&mut self, other)`), so worker chunks combine in any
order. It holds:

* **Annual histograms** on a symmetric grid of 2,945 bins (an exact-zero bin, `$16`
  linear bins below `$1,024`, then 64 bins per doubling up to `2^32`, negative side
  included: 0.8% to 1.6% wide) for: net worth, investable assets, spending, gross
  income, total tax. These give every quantile band and the density fan without
  storing paths.
* **Status counts per year**: working, retired with surplus, retired and on plan,
  retired and cutting spending, depleted; dead is derived. (The "rich, broke or dead"
  view.)
* **Per-year sums** of the 46 `YearSeries`: balances by bucket kind, property value,
  mortgages, debts; flows by kind (wages, Social Security, pensions, rental,
  withdrawals by account kind, taxes by kind, spending by group, savings by
  destination), and for private-company equity the proceeds (`income_equity`), the
  cash paid to exercise (`equity_exercise`) and the paper value of what is still held
  (`equity_paper_value`, a memo: it is in no balance and not in net worth).
* **Per-path scalars**, 42 columns of `f32`: FI age, Coast-FI age, retirement age,
  depletion age, death ages, investable at retirement, funded ratio, real portfolio
  return in the first 10 retirement years, worst drawdown, months unemployed,
  long-term-care cost, inflation, failure cause, success, and the derived ones
  (terminal and peak net worth, minimum investable, lifetime tax and spending,
  delivered/planned spending ratios, years retired, cut and depleted), and six for
  private-company equity: the outcome of the main company (failed, still private,
  acquired, IPO), the age at that event, gross proceeds, exercise cost, tender
  proceeds, and net proceeds after cost and the tax the equity caused. These feed
  distributions, scatterplots and failure attribution, and let the UI pick "the path
  at the 10th percentile" and ask for its trace.
* **Sample paths**: annual net worth, investable and spending for the first 96 paths.

`Summary::from(&Accumulator)` produces the JSON the UI and the AI consume: headline
metrics with standard errors, quantile curves, FI-by-age cumulative curve, survival
curves, attribution of failures (first cause: market sequence, job loss, health/LTC,
longevity, overspending, inflation, other), and compact tables. For a plan with
private-company equity it adds `Summary.equity`: the share of lives with each outcome,
how often the equity pays nothing or costs more than it returns, the spread of
proceeds, and the plan's headline results among the lives with each outcome. It is
computed from the scalars alone, so it merges exactly like everything else.
`docs/engine-output.md` is the field-by-field reference.

The serialised accumulator carries a layout version (`Accumulator::VERSION`, now 6:
six scalars and three series were appended for private-company equity). Bytes of
another version are refused on decode and on merge, never reinterpreted.

`trace_path(i) -> PathTrace`: monthly ledger of every balance and flow, the annual tax
return lines, and an event log ("laid off", "bought home", "Roth conversion $38k",
"guardrail cut spending 10%").

## 6. wasm API (`firesight-wasm`)

```ts
class Engine {
  constructor(profileJson: string)            // validate + compile; throws a JSON error list
  horizonYears(): number
  warnings(): string
  runChunk(firstPath: number, count: number): Uint8Array   // serialised Accumulator
  trace(path: number): string                 // PathTrace JSON
}
class Merger {                                // holds the running accumulator of one run
  constructor()
  add(bytes: Uint8Array): void
  paths(): number
  summarize(profileJson: string): string      // Summary JSON
  digest(profileJson: string): string         // SummaryDigest JSON
  bytes(): Uint8Array
}
function mergeChunks(a: Uint8Array, b: Uint8Array): Uint8Array
function summarize(acc: Uint8Array, profileJson: string): string   // Summary JSON
function digest(acc: Uint8Array, profileJson: string): string      // SummaryDigest JSON
function compare(a: Uint8Array, b: Uint8Array, baselineProfileJson: string): string // Comparison JSON, B minus A
function planPreview(profileJson: string): string        // PlanPreview JSON, no simulation
function validate(profileJson: string): string           // { errors, warnings }
function applyPatch(profileJson: string, patchJson: string): string
function migrateV1(v1Json: string): string    // { profile, notes }
function levers(profileJson: string): string  // sensitivity levers as { id, label, low, high } patches
function schema(): string
function defaultProfile(): string
function exampleProfiles(): string
function taxDetail(profileJson: string, year: number, inputsJson: string): string // explain a tax computation
function equityPreview(companyJson: string, draws: number): string // EquityPreview JSON for one element of private_equity; no household simulated; draws 0 = default
```

A profile with no `start` starts in the current month of the caller's clock.

The web app owns a pool of `navigator.hardwareConcurrency - 1` workers. A run is split
into chunks of ~500 paths handed out to idle workers; results merge on arrival, so
charts sharpen progressively and a new edit cancels the stale run. Solver, sensitivity
and heatmap sweeps are the same mechanism with different patches and smaller path
counts.

## 7. Performance budget

* No allocation inside the monthly loop. Path state and economy buffers live in a
  reusable `Workspace`.
* Tax is computed once a year (plus the January plan), never monthly.
* Target on one M-series core, native, 60-year horizon, typical two-earner profile:
  at least 5,000 paths per second; wasm at least 70% of native.
  100,000 paths in a few seconds on an 8-core laptop in the browser.
* `firesight bench [profile] [--paths N] [--repeat R]` reports paths/sec and
  month-steps/sec on one thread and paths/sec on all threads, each the best of `R`
  runs, and prints a one-line JSON summary last (`... | tail -1`).
* Regression check: `tools/bench-check.sh` runs the bench on the four shipped
  examples and fails if any is more than 25% below `tools/bench-baseline.json`.
  What is compared is a score, single-thread paths/sec divided by the speed of a
  calibration kernel timed in the same process, so that a slower, faster or busy
  machine does not move it; `tools/bench-check.sh --record` re-records the baseline
  (do that on the runner class that runs the check, and after an intended change).
  `firesight bench --check FILE` is the same check for one profile.

### How the engine stays fast (and bit-identical)

Every optimisation in the engine keeps results identical to the bit; the golden
digests (`tests/sim_golden.rs`) and the native/wasm comparison are the proof. The
rules: never reorder floating-point arithmetic, never skip a random draw, and skip
work only when its result is provably unchanged or provably a zero that is added.

* **Asset classes nobody holds.** `Accounts::idle` records which of the nine asset
  classes are held in no bucket (set by `apply_returns` from what it sees, cleared by
  everything that can buy into a class). Sums and the monthly return step then run
  over the leading classes only, in the same order: adding a zero never changes a
  sum. Code that writes holdings directly must call `Accounts::holdings_changed`
  (debug builds assert that an idle class really is empty).
* **The tax law of the year.** Each January only the tables of the filing status the
  household files under are indexed (`State::build_law` / `ensure_law`), and the
  parameters that do not depend on the year are copied once per path, not once per
  year (`TaxLaw::for_year_statuses_into` with `refresh`).
* **Tables for path-independent values.** Growth factors of budget lines, the
  funded test's discount powers and a child's remaining cost by age are computed at
  compile time with the same calls the month loop used to make.
* **The withdrawal plan.** Bucket totals are summed once per plan, not once per
  candidate (`decumulation::Balances`), and a withdrawal context is built only for a
  source that is drawn on.
* **Cold code out of line.** Trace events, the monthly detail record and the
  conservation checks are `#[cold]` functions, so the month loop the optimiser sees
  is the one that runs.
* **Build settings** (`.cargo/config.toml`): a larger LLVM inlining budget for every
  target and `simd128` for wasm. `tools/pgo-build.sh` builds a profile-guided native
  CLI (3 to 5% faster again; optional, needs `llvm-profdata`).

Measured on a 10-core M-series laptop, best of five runs: 5,670 paths/sec on one
core and about 32,000 on ten threads for the dual-income example natively (4,460
and 26,500 before this work), 6,100 for the early-career example and 8,270 for
the landlord; in wasm (node, one worker) 3,970, 4,260 and 5,660, about 70% of
native (2,930, 3,180 and 4,460 before, 66%).

Measuring: wall-clock numbers on a busy machine are noisy, and the position of
the main thread's stack alone moves them by about 5% (the bench therefore times on
a spawned thread). To compare two builds use instructions retired, which does not
depend on load: `/usr/bin/time -l firesight run <profile> --paths 4000 --threads 1`
on macOS, `perf stat` on Linux. The dual-income example takes about 3.64 million
instructions per simulated life.

## 8. Testing

* Unit tests per module with hand-computed cases (tax returns checked against published
  worked examples, amortisation, PIA examples from SSA, RMD).
* Property tests: money is conserved every month (sum of flows equals change in
  balances), no NaN, results independent of chunking and merge order, identical seeds
  give identical bytes, stream isolation (changing spending does not change market
  draws).
* Golden tests: the four demo profiles' `SummaryDigest` at a fixed seed
  (`tests/golden/`). The three older ones did not change when private-company equity
  was added, which is the proof that a profile without it is simulated bit for bit as
  before.
* Private-company equity. Unit tests of the mechanics (`equity/tests.rs`): outcome
  frequencies, dates and multiples against the table with stated sample sizes and
  four-standard-error bounds computed from the parameters; the overhang rule; vesting,
  exercise, lots, incentive-option holding periods, QSBS tiers; shares conserved over
  random policies and fates. Integration tests (`tests/`): every forced outcome crossed
  with every exercise timing, sell-down policy and market model conserves money every
  month; adding an empty company moves nothing under a flat tax and only AMT years
  under current law; a policy change keeps every fate; counting a company at zero keeps
  its fate and removes its money; paper value is in no balance and no test; a payout is
  taxed before it is invested; the equity scalars reconcile with the ledger; the demo's
  outcome shares match the lifetime odds; the preview and the simulation agree on how
  often the equity pays nothing; the month loop of the demo allocates nothing.
* Economy calibration tests: simulated moments within tolerance of
  `data/history/calibration.json`.

See `docs/methodology.md` for how each relationship is modelled and what was measured.
