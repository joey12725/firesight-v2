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
examples/            demo households (v2 schema) and the three v1 profiles for migration tests
```

The browser does all simulation. There is no backend state: profiles live in the
browser (IndexedDB) and travel as JSON files or share links. The only server code is
the optional LLM proxy.

### Determinism rules (core)

* `f(profile, data, seed, path_index)` is a pure function. Same inputs, same bits, on
  native and wasm. No `HashMap` iteration order in results, no time, no global RNG.
* Every random subsystem draws from its own named stream:
  `Rng::stream(seed, path_index, Stream::X)`, with `Stream` in `core::rng`. The
  identifiers are part of the reproducibility contract: they are never renumbered,
  only added to.

  | Stream | Id | Drawn for |
  |---|---|---|
  | `Market` | 1 | asset returns, rates, recessions |
  | `Inflation` | 2 | inflation innovations outside the joint market draw |
  | `EquityComp` | 3 | employer stock prices, private-company outcomes |
  | `Shocks` | 4 | irregular expenses |
  | `Events` | 5 | whether and when probabilistic life events occur |
  | `Career(person)` | 0x0100_0000 + i | wage shocks, layoffs, re-employment |
  | `Health(person)` | 0x0200_0000 + i | medical costs, long-term care |
  | `Longevity(person)` | 0x0300_0000 + i | age at death |
  | `Housing(property)` | 0x0400_0000 + i | a property's own price path |
  | `CategoryPrices` | 1100 | category price wedges (`category_inflation`) |
  | `DetailedShocks` | 1101 | typed irregular expenses (`shocks: detailed`) |
  | `Vehicle(k)` | 0x1100_0000 + k | replacement-date jitter |
  | `CollegePrices` | 1200 | the path of college prices |
  | `ChildArrival(child)` | 0x1200_0000 + i | an uncertain birth |
  | `ChildCollege(child)` | 0x1280_0000 + 2i | attendance, years, aid |
  | `ChildSupport(child)` | 0x1280_0000 + 2i + 1 | support as an adult |
  | `RentMarket` | 1300 | the local rent index (market-rent lines) |
  | `Liquidity` | 1301 | time to close and discount of slow home sales |
  | `InsuranceDrift` | 1302 | the homeowner's-insurance premium index |
  | `Maintenance(property)` | 0x1300_0000 + i | major repairs (lumpy maintenance) |
  | `CareerExt(kind, i)` | 0x1400_0000 + kind x 0x1_0000 + i | job changes, recession pay, disability, late-career layoffs, business cycles (kind 0 job, 1 person, 2 self-employment) |
  | `PolicyRisk` | 1500 | the path's policy changes |
  | `HomeEquityCredit` | 1600 | lenders freezing home-equity lines |
  | `RentalMarket(property)` | 0x1600_0000 + i | tenants, rents and repairs of a realistic rental |
  | `CoupleLongevity` | 1700 | linked lifespans |
  | `LtcPremiumRates` | 1701 | class-wide long-term-care premium increases |
  | `HealthStates(person)` | 0x1700_0000 + i | the yearly health-state path |
  | `MarketTails` | 1800 | Student-t mixing of the fat-tailed market model |
  | `PanicSelling` | 1900 | whether a panic-prone household sells in a fall |
  | `OneMoreYear` | 1901 | whether a funded household works one more year |
  | `ReturnToWork(person)` | 0x1900_0000 + i | length of a retiree's job search |
  | `Divorce` | 2000 | when a couple with `divorce_risk` divorces |
  | `SpousalCare` | 2001 | whether a working partner cuts back to give care |
  | `Parent(i)` | 0x1a00_0000 + i | a parent's lifetime and late-life care |

  The first nine are the base model's. Every other stream belongs to one opt-in
  feature and is drawn only when that feature is on, a fixed number of times a
  month or a path whatever the household decides. Changing a decision in a
  scenario, or switching a feature on, must not shift the random numbers of any
  other subsystem. This is what makes scenario comparison a paired experiment
  (common random numbers), and it is the v1 idea we keep and make exact.
  `tests/sim_richness.rs` checks that the ids are unique and that no feature moves
  the market, career, health or longevity draws.
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

Household { people: Vec<Person> (1..=2), children: Vec<Child>, location: Location,
            married: Option<YearMonth>,              // marriage date: divorce hazard, ten-year Social Security rule
            divorce_risk: Off | Typical(DivorceTerms),  // NCHS rates by length of marriage, own stream
            parents: Vec<Parent> (0..=4) }
Parent    { id, name, child: usize, birth, sex, health,    // lifetime from the life table, Stream::Parent(i)
            estate (today's $), share (1.0), inherited_ira_fraction,  // IRA part paid out over 10 years
            care_erodes_estate: bool (true),
            care_support: Option<CareSupport> }
CareSupport { probability (1.0), annual_cost, hours (0..1), caregiver: Option<usize> }  // during the parent's care spell
DivorceTerms { asset_split (0.5), support: { annual, years, direction: Pays|Receives },
               keeps_home (true), then_rent_monthly: Option<f64>, legal_costs (11_300),
               spending_factor (0.7) }
Person    { id, name, birth: YearMonth, sex: Female|Male|Unspecified,
            health: Excellent|Good|Average|Poor,     // scales mortality; default Good (0.85x)
            joins: Option<YearMonth>,               // future partner: not in household before this
            social_security: SocialSecurity,
            disability_risk: Off|Typical,           // opt-in long-term disability: ends work, pays SSDI (methodology 6.3)
            ltd_insurance: Option<LtdInsurance{ replacement (0..=1, default 0.6), to_age (default 65) }>,
            ltc_insurance: Option<LtcInsurance>,       // None (default): no policy
            life_insurance: Vec<LifePolicy> (0..=8 in all) }
LifePolicy { id, name, kind: Term { years_remaining } | Permanent,
             death_benefit, annual_premium }        // nominal, level; benefit tax-free to survivor or estate
LtcInsurance { annual_premium, daily_benefit, benefit_years, elimination_days,   // start-of-plan dollars, level premium
               inflation_rider: None | Compound{rate},
               premium_increase_risk: bool (true) }    // class-wide increases from Stream::LtcPremiumRates; lapse when unaffordable
SocialSecurity { claim_age: f64 (62..=70, default 67),
                 claiming: Fixed (default) | Adaptive{ earliest (62), latest (70) },  // sim/behavior/claiming.rs
                 basis: FromEarnings { career_start_age (default 22) }
                      | KnownBenefit { monthly_at_fra }      // from an SSA statement, today's $
                      | None }
Child     { id, name, birth: YearMonth (may be in the future = planned),
            costs: Modeled | InBudget,              // Modeled: USDA-style cost by age and income tier
            childcare_monthly: Option<f64>,         // ages 0-4 override
            college: None | PublicInState | PublicOutOfState | Private | Custom{annual},
            college_share: f64 (fraction parents pay, default 1.0),
            arrival: Certain | Uncertain{ probability (0.85), delay_mean_months (4), twins_probability (0.03) },
            college_cost: Fixed | Stochastic{ attend_probability (0.95), real_tuition_growth (0.01),
                                              tuition_volatility (0.012), aid_scale (1) },
            adult_support: Option<{ probability (0.3), annual (12000), mean_years (2) }>,
            parental_leave: Option<{ person (0), months (3), pay_fraction (0.6) }>,
            second_parental_leave: Option<{ person, months, pay_fraction }> }   // the other parent
                                                    // Uncertain: planned children only; random births per path
Location  { country (default "US"), state: Option<String> (2-letter), city: Option<String>,
            foreign_tax_rate: Option<f64> }         // flat effective income tax when not in the US

Income { id, name, owner: usize (person index), kind, start: Option<When>, end: Option<When> }
When   = Date{at: YearMonth} | Age{person, age} | Retirement{offset_years} | Start
IncomeKind =
  Job { salary, growth: Flat{real_raise} | Lifecycle{profile: Flat|Typical|Steep, extra},
        risk: CareerRisk { level: None|Low|Typical|High,   // wage shocks + layoff hazard
                           job_mobility: Off|Typical|High,    // opt-in voluntary job changes with pay jumps (methodology 6.1)
                           recession_pay: Off|Typical,        // opt-in freezes, cuts, smaller bonuses in recessions (6.2)
                           late_career: Off|Realistic },      // opt-in older-worker layoffs: longer spells, bigger scars, may never work again (6.4)
        bonus: Option<Bonus{ target_fraction, volatility }>,
        commission: Option<Commission{ expected_annual, volatility }>,
        equity: Vec<EquityGrant>,
        plan: Option<EmployerPlan>, hsa_contribution: f64, hsa_employer: f64,
        ends_at_retirement: bool (true) }
  SelfEmployment { net_annual, volatility, real_growth, ends_at_retirement, solo_401k_fraction,
                   business_risk: Off|Typical|High,         // opt-in cycles and failure (methodology 6.5)
                   years_operating,                         // age of the business at the start (failure curve)
                   fallback_wage: Option<FallbackWage{ annual, search_months }> }
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

Spending { items: Vec<BudgetItem>,
           lifestyle_creep: f64,                   // share of REAL income growth that becomes spending (0..1). Not a perpetual rate.
           retirement_smile: bool,                 // Blanchett real decline through the 70s, healthcare rises separately
           shocks: Off|Typical|High|Detailed,      // irregular expenses, compound Poisson
                                                   // (Detailed: four typed shocks, heavy tails, clustering)
           healthcare: Healthcare { mode: Modeled|InBudget, early_retirement: Aca|Cobra|Custom{annual},
                                    long_term_care: bool, quality: Bronze|Silver|Gold,
                                    cover_children: bool (false),            // children on the household plan
                                    children_coverage_end_age: f64 (26),
                                    medicaid: bool (false),  // spend-down, spousal allowances, estate recovery; medicaid_rate
                                    spousal_care: bool (false) },   // partner provides half of home care
           survivor_factor: f64 (0.7),
           // Spending realism, all opt-in (defaults leave results bit-identical):
           vehicles: Vec<Vehicle>,                 // lumpy replacement purchases, empty by default
           seasonality: Off|Typical,               // monthly weights by category, annual total unchanged
           category_inflation: Off|Stochastic,     // persistent AR(1) price wedge per category vs CPI
           unemployment_cut: f64 (0) }             // share of discretionary spending cut during a spell
Vehicle { id, name, price (today's $), replace_every_years (10), age_years (0),
          trade_in_fraction (0.2), financing: Option<{down_fraction, rate, term_months}>,
          owner: Option<usize>, end: Option<When> }
BudgetItem { id, name, category: Category, amount, frequency: Monthly|Annual|Weekly|Biweekly|Quarterly,
             essential: bool, real_growth: Option<f64> (default by category),
             start: Option<When>, end: Option<When>,
             in_retirement: Same | Stops | Factor{x} | Amount{x},
             owner: Option<usize>,
             rent_growth: Fixed | Market (Fixed) }  // Market: rent category only, follows the path's local rent index
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
                  single_stock: SingleStock{ volatility, beta },
                  tax_loss_harvesting: bool (false),    // realise losses, stay invested, reset basis
                  behavior: Disciplined (default)             // sim/behavior/panic.rs
                          | PanicProne{ drawdown_trigger (0.25), probability (0.08), share (0.5),
                                        reentry: { recovered_within (0.05), max_months (36) } },
                  pause_deferrals_in_distress: bool (false),  // sim/behavior/distress.rs
                  distress: { keep_match (true), high_interest_rate (0.08) } }
Allocation = map AssetClass -> weight (normalised)
AssetClass = UsStocks | IntlStocks | Bonds | Cash | RealEstate | Gold | Crypto | SingleStock | Custom

Property { id, name, use: Primary | Rental{..RentalTerms} | Vacation,
           value, basis: Option<f64>,
           mortgage: Option<Mortgage{ balance, rate, remaining_months, payment: Option, kind: MortgageKind }>,
           property_tax_rate: Option<f64> (default from state), insurance_rate, maintenance_rate, hoa_monthly,
           appreciation_vs_national: f64, idiosyncratic_volatility: f64,
           maintenance: Smooth | Lumpy (Smooth),                      // Lumpy: routine half + random major repairs
           assessment: Market | Capped{ annual_cap, assessed_value: Option } (Market),
           insurance_real_growth: Fixed | Stochastic (Fixed) }       // Stochastic: persistent real drift + repricing jumps
MortgageKind = Fixed | Arm{ fixed_months, adjust_every_months, margin, periodic_cap, lifetime_cap }
               // resets to the simulated bill yield + margin within caps, payment re-amortised over the months left
RentalTerms { monthly_rent, vacancy, management_fee, rent_real_growth, land_fraction,
              dynamics: Simple (default) | Realistic{ annual_turnover (0.25) } }

Debt { id, name, kind: Student|Auto|CreditCard|Personal|Other, balance, rate,
       payment: Option<f64>, remaining_months: Option<u32> }
DebtPolicy { strategy: Minimum | Avalanche | Snowball, extra_monthly: f64,
             refinance_mortgages: bool (true),  // each January, if the market rate is 1+ point lower and 4 years of saving exceed the costs
             cash_out_refinance: Option<{ max_ltv (0.80), rate_premium (0.0025), term_years (30) }>,  // before penalised withdrawals
             mortgage_prepayment: Never (default) | WhenRateAbove{ real_rate: Option<f64> (None = after-tax 10y yield) } }

LifeEvent { id, name, when: When, probability: f64 (1.0), timing_uncertainty_years: f64 (0), kind }
EventKind =
  BuyHome { price, down_payment_fraction, rate: Option<f64> (None = simulated market rate), term_years,
            replaces_rent: bool, closing_cost_fraction, ..property params,
            maintenance, assessment, insurance_real_growth, mortgage_kind: MortgageKind }
  SellProperty { property: id, selling_cost_fraction, then_rent_monthly: Option<f64> }
  Relocate { state, city, country, moving_cost, cost_of_living: Unchanged | Factor{overall} | ByCategory{map} }
  Windfall { amount, taxable: bool }
  Expense  { amount, financed: Option<{ down_fraction, rate, term_months }> }
  IncomeChange { income: id, change: SetSalary{x} | Scale{x} | End }
  CareerBreak { person, months, income_fraction }
  SpendingChange { target: Item{id} | Category{c} | All, change: Scale{x} | Set{x} }
  ContributionChange { income: id, employee_fraction }
  Divorce(DivorceTerms)                            // the second person leaves the household

RetirementPlan {
  trigger: AtAge{person, age} | AtDate{at}
         | WhenFunded{ withdrawal_rate (0.04), basis: Investable|NetWorth, count_future_income: bool,
                       min_age, max_age,
                       confirmation: Option<{ months (6), max_equity_drawdown (0.2) }>,  // sim/behavior/confirm.rs
                       one_more_year: Option<{ probability (0.25) }> }
         | FixedNumber{ amount, basis }
         | AlreadyRetired,
  partner_offset_years: f64,                     // second earner keeps working this much longer (can be negative)
  spending_rule: ConstantReal | PercentOfPortfolio{rate} | Guardrails{upper, lower, adjust}
               | VanguardDynamic{ceiling, floor, rate?} | Vpw | FloorCeiling{floor, ceiling, rate?},
  withdrawal_order: Conventional | Proportional
                  | TaxAware{ fill_to_bracket: f64, roth_conversions: bool, mind_aca: bool },
  early_access: Auto | PayPenalty,               // Auto: taxable, Roth basis, seasoned conversions, 72(t)
  bequest_target: f64,
  qcd: bool (false),                              // pay Giving lines from the IRA as QCDs from 70.5
  home_equity: Never | LastResort (default),     // LastResort: sell property before cutting essentials; reported as home_sale_rate
  home_equity_borrowing: None (default)         // borrow against the main home before selling it or cutting essentials
    | Heloc{ max_combined_ltv (0.80), spread (0.035 over bills), draw_years (10), repay_years (20),
             freeze_probability (0.3, per downturn) }
    | ReverseMortgage{ open_when: LastResort (default) | SavingsBelow{ years },   // HECM from 62
                       lender_margin (0.02), other_closing_costs (3000, today's $) },
  downsize: Option<{ when: AtAge{ person, age } | WhenChildrenLeave (default) | WhenWidowed
                          | WhenFundedRatioBelow{ ratio },
                    to_value_fraction (0.6), rent_instead (false),
                    selling_cost_fraction (0.06), buying_cost_fraction (0.03) }>,
  return_to_work: Option<{ funded_ratio_below (0.8), until_age (70), pay_fraction (0.5),
                           former_annual_pay: Option<f64>,
                           search: Typical | Fixed{months} }>,  // off by default; sim/behavior/work.rs
}
Assumptions {
  market: MarketModel { kind: Bootstrap{ mean_block_months (60), from_year: Option } | Regime | Lognormal
                              | FatTailed        // lognormal means and vols; Student-t shocks, GARCH(1,1) volatility, calm/turbulent correlations
                              | Historical,       // every start month in history, in order: path i = cohort i
                        equity_premium_adjustment: f64 (annual, default -0.015: US 1926+ history less 1.5 points),
                        inflation_adjustment: f64,
                        stress: Vec<Stress>,
                        bond_pricing: Sampled (default) | FromYield,     // bootstrap: bond returns priced from the path's yield
                        valuation: Option<{ starting_cape (40), fair_cape (17.3), half_life_years (15) }>,  // None (default): no valuation tilt
                        dynamic_yields: bool (false) },  // dividend yields move with prices, recessions and valuation; total returns unchanged
  Stress = CrashAtRetirement{ drawdown, recovery_years }  // back to the pre-crash nominal level after recovery_years, not to the unstressed path
         | LostDecade{ start: When }
         | Stagflation{ start: When, years, inflation },
         | HistoricalEpisode{ start: When, episode: GreatDepression1929 | Stagflation1966 | DotCom2000 | Financial2008 }  // the record's months, all series
         | LostGeneration{ start: When, years (20) },  // Japan-style: stocks -2% real a year, -0.3% inflation, yields glide to 1%
  taxes: TaxModel { kind: UsCurrentLaw | Flat{rate} | None, index_brackets: bool (true),
                    social_security_haircut: Option<{ from_year, factor }>,
                    chained_cpi: bool (false),     // thresholds on the chained index (brackets, IRA/HSA limits) grow 0.25 points a year slower
                    state_detail: Basic | Full,    // Full: CA SDI on wages, NY bracket recapture, UT Social Security credit
                    policy_risk: Option<{ ss_haircut_probability, ss_haircut_factor, earliest_year,
                                          tax_increase_probability, tax_increase_points }> },  // per-path policy draw
  longevity: Stochastic{ improvement: bool,
                         couple_dependence: Independent (default) | Linked,  // Frank copula + bereavement; Stream::CoupleLongevity
                         income_gradient: bool (false),                     // Chetty et al. 2016 multiplier on mortality
                         income_percentile: Option<f64> }                   // 1-100, overrides the ranked income
           | FixedAge{ age },
  career_risk: bool (true), health_risk: bool (true),
  housing_liquidity: Instant | Realistic (Instant),  // Realistic: sales take months and may close at a discount
  health_model: Independent (default) | States,   // yearly healthy/chronic/frail chain scaling OOP, care and mortality; Stream::HealthStates(person)
}
SimulationSettings { paths: u32 (10_000), seed: u64, end_age: u32 (100) }
```

`core::legacy` converts a v1 profile dict (the format in `examples/v1/*.json`) into this
schema, with a list of the assumptions it had to make.

`core::validate` returns hard errors (cannot simulate) and warnings (plausibility lints:
spending above income with no assets, no healthcare line and healthcare `InBudget`,
allocation with more than 20% crypto, retirement spending below 50% of current, and so
on). The UI shows them; the AI reads them.

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

The opt-in features (`docs/methodology.md` section 18) enter this loop through hooks,
each behind a flag of the compiled plan that is false when the feature is off, so
the default path executes the code it always did. Where they sit:

```
path start   lifetimes redrawn jointly (linked lifespans), health-state paths, policy
             draws, births, college prices, parents' lifetimes, divorce month
start of t   career-realism variates drawn; disability onset and recovery
  1.         household dynamics (divorce, a parent's death or care spell, life
             insurance, a partner starting to give care); births and parental leave;
             insurance, rent-market and sale-terms draws
  3.         after each job's step: disability, recession pay, late-career layoff,
             job change; business cycle and fallback wage; return to work
  4.         adaptive claiming before Social Security is paid; disability insurance
             after it (the larger of the two, not both)
  6.         seasonal, category-price and unemployment factors on the budget;
             children's health cover; spousal care, then long-term-care insurance,
             then Medicaid on the care bill; vehicles; detailed shocks
  7.         ARM resets, lumpy repairs, realistic rentals; sales that close;
             home-equity interest, reverse-mortgage accrual, downsizing
  8-9.       QCDs; cash-out refinance before penalised withdrawals; home-equity
             borrowing before a sale or a cut to essentials; saving paused in
             distress; home-equity repayment and mortgage prepayment from surplus
 10.         tax-loss harvesting; panic selling and re-entry
 11.         retirement confirmation and the one-more-year draw
```

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
| `tax` | Federal (ordinary, LTCG stacking, NIIT, AMT ignored), FICA and SE tax, state, credits, Social Security benefit taxation, standard vs itemised, ACA premium credit, IRMAA, early-withdrawal penalty, RMD; all thresholds indexable by a CPI factor |
| `benefits` | Social Security: indexed earnings record -> AIME -> PIA -> claim adjustment, spousal and survivor, trust-fund haircut |
| `career` | Lifecycle wage curve, persistent/transitory shocks, layoff and re-employment, bonus, commission, equity compensation |
| `health` | Premiums by phase (employer, ACA, Medicare), out-of-pocket, long-term care, mortality draws |
| `housing` | Properties, mortgages, PMI, purchase and sale, rentals, depreciation |
| `accounts` | Buckets (cash, taxable with basis, traditional, Roth with basis and conversion tranches, HSA) x asset classes; contributions waterfall; withdrawals; rebalancing; glide path |
| `decumulation` | Spending rules and the annual withdrawal / Roth-conversion plan |
| `sim` | `Plan`, path state, the monthly loop, trigger logic |
| `stats` | Mergeable accumulators and the final `Summary` |
| `career_realism` | Opt-in: job mobility, recession pay, disability, late-career layoffs, business cycles and failure |
| `health_states`, `longevity_links`, `medicaid`, `ltc_insurance` | Opt-in: the health-state chain, linked lifespans, Medicaid's long-term-care rules, long-term-care insurance |
| `home_equity`, `rental_market`, `housing/{arm,ext,insurance,maintenance}` | Opt-in: home-equity lines and reverse mortgages, realistic rentals, adjustable rates, lumpy maintenance, assessment caps, insurance repricing |
| `policy` | Opt-in: per-path policy risk |
| `sim/{spendx,family,housing_ext,housing_decisions,cash_out,downsize,care,tax_realism,realism,rz_*}`, `sim/behavior`, `sim/dynamics` | The simulation-side hooks of the opt-in features (spending, children, housing, care, tax, careers, household decisions, household changes) |
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
* **Per-year sums** of the 43 `YearSeries`: balances by bucket kind, property value,
  mortgages, debts; flows by kind (wages, Social Security, pensions, rental,
  withdrawals by account kind, taxes by kind, spending by group, savings by
  destination).
* **Per-path scalars**, 27 columns of `f32`: FI age, Coast-FI age, retirement age,
  depletion age, death ages, investable at retirement, funded ratio, real portfolio
  return in the first 10 retirement years, worst drawdown, months unemployed,
  long-term-care cost, inflation, failure cause, success, and the derived ones
  (terminal and peak net worth, minimum investable, lifetime tax and spending,
  delivered/planned spending ratios, years retired, cut and depleted). These feed
  distributions, scatterplots and failure attribution, and let the UI pick "the path
  at the 10th percentile" and ask for its trace.
* **Sample paths**: annual net worth, investable and spending for the first 96 paths.

`Summary::from(&Accumulator)` produces the JSON the UI and the AI consume: headline
metrics with standard errors, quantile curves, FI-by-age cumulative curve, survival
curves, attribution of failures (first cause: market sequence, job loss, health/LTC,
longevity, overspending, inflation, other), and compact tables. `docs/engine-output.md`
is the field-by-field reference.

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
* Regression check: `tools/bench-check.sh` runs the bench on the three shipped
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
* Golden tests: the demo profiles' `Summary` at a fixed seed. Three of the four use
  the default model throughout and pin it; `full-life` pins a selection of the
  opt-in features.
* The opt-in features together (`tests/sim_richness.rs`): every feature at once,
  each group, each feature alone and (slow, ignored) every pair, with conservation
  checks, finite values, chunking invariance and a difference from the baseline;
  the stream audit; and a check of the order of events for contradictions between
  features.
* Economy calibration tests: simulated moments within tolerance of
  `data/history/calibration.json`.

See `docs/methodology.md` for how each relationship is modelled and what was measured.
