//! `Plan::compile`: everything that can be resolved once per profile.
//!
//! Ids become indices, `When` values become month offsets (or stay symbolic when they
//! are relative to a simulated retirement date), budget amounts become monthly real
//! dollars, and every subsystem model (mortality, health, Social Security, careers,
//! properties, tax law, the economy) is built here so the path loop only reads.

use crate::accounts::{self, Accounts, CashTerms, Weights};
use crate::benefits::{BenefitAnnuity, BenefitBasis, EarningsRecord, SocialSecurityModel};
use crate::career::{CareerParams, JobModel, SelfEmploymentModel};
use crate::data::Data;
use crate::decumulation::SpendingPlanner;
use crate::economy::EconomyModel;
use crate::health::{HealthModel, MortalityModel};
use crate::housing::{amortized_payment, PropertyModel};
use crate::math;
use crate::model::{
    Category, ChildCosts, Cola, College, CostOfLivingChange, DebtKind, EmployerPlan, Episode,
    EventKind, IncomeChangeKind, IncomeKind, Profile, PropertyUse, Rebalance, RetirementChange,
    RetirementTrigger, Sex, ShockLevel, SocialSecurityBasis, SpendingChangeKind, SpendingTarget,
    Stress, TaxKind, WageGrowth, WealthBasis, When, WhenContext, WhenFunded, WithdrawalOrder,
    YearMonth,
};
use crate::stats::{Accumulator, Summary, SummaryContext};
use crate::tax::{LocalTax, StateCode, TaxLaw};
use crate::validate::{validate, Report};
use crate::MAX_PEOPLE;

/// Lowest real discount rate used to value future Social Security and pension income
/// in the funded test (roughly the long-run real yield on inflation-protected
/// Treasuries; a judgement). The rate actually used is the funded rule's own
/// withdrawal rate when that is higher: dividing a need by the withdrawal rate
/// values a perpetual real dollar at `1 / rate`, so income still to come has to be
/// discounted at the same rate or the two halves of the test disagree, and a
/// household decades from Social Security would be told it can retire on savings
/// that cannot last until the benefit starts.
pub const INCOME_DISCOUNT_RATE: f64 = 0.02;

/// Horizon a withdrawal rate is taken to be designed for (Bengen's and the Trinity
/// study's thirty years) when the funded test turns it into the discount rate for
/// the years before future income starts.
pub const BRIDGE_HORIZON_YEARS: f64 = 30.0;

/// The real return `rho` at which withdrawing `rate` of the starting balance every
/// year exhausts it in [`BRIDGE_HORIZON_YEARS`]: `(1 - (1 + rho)^-30) / rho = 1 /
/// rate` (1.2% for 4%, 2.8% for 5%, negative below 3.3%).
pub fn bridge_discount_rate(rate: f64) -> f64 {
    let target = 1.0 / rate.clamp(0.005, 0.5);
    let value = |rho: f64| {
        if rho.abs() < 1e-9 {
            BRIDGE_HORIZON_YEARS
        } else {
            (1.0 - math::pow(1.0 + rho, -BRIDGE_HORIZON_YEARS)) / rho
        }
    };
    // The annuity value falls as the rate rises: bisect.
    let (mut lo, mut hi) = (-0.5, 1.0);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if value(mid) > target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// Inflation assumed when valuing a pension without a cost-of-living adjustment in
/// the funded test (the same figure Social Security projections use,
/// [`crate::benefits::PROJECTION_INFLATION`]).
pub const PLANNING_INFLATION: f64 = crate::benefits::PROJECTION_INFLATION;

/// Latest retirement age used to judge success when the trigger has none of its own
/// (`fixed_number`): the default `when_funded.max_age`.
pub const DEFAULT_LATEST_RETIREMENT_AGE: f64 = 70.0;

/// Months a home purchase the household cannot yet afford is retried before the
/// event is dropped.
pub const PURCHASE_RETRY_MONTHS: u32 = 60;

/// Share of pre-tax income a household is taken to spend when its child-cost tier
/// has to be inferred from its budget (no earned income at the start). A judgement:
/// consumer units in the USDA study spend about four fifths of pre-tax income.
pub const CHILD_TIER_SPENDING_SHARE: f64 = 0.8;

/// Selling costs when property is sold as a last resort
/// (`retirement.home_equity: last_resort`): the default of a planned sale.
pub const LAST_RESORT_SELLING_COST: f64 = 0.06;

/// Rent a household pays after selling its home as a last resort, per year as a
/// fraction of the sale price. A judgement: gross rental yields on typical homes
/// are 5 to 7% of value, and a household in this position moves somewhere cheaper.
pub const LAST_RESORT_RENT_YIELD: f64 = 0.04;

/// A point in time that is either a month index (months since `profile.start`) or
/// relative to the simulated retirement month.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Sched {
    /// A fixed month index (may be negative: already in the past).
    At(i32),
    /// Retirement month plus this many months.
    Retire(i32),
    /// Never happens.
    Never,
}

impl Sched {
    /// The month index, or `i32::MAX` if not known or never. `retire_month` is
    /// `i32::MAX` while retirement has not been decided.
    #[inline]
    pub(crate) fn month(self, retire_month: i32) -> i32 {
        match self {
            Sched::At(m) => m,
            Sched::Retire(off) => {
                if retire_month == i32::MAX {
                    i32::MAX
                } else {
                    retire_month.saturating_add(off)
                }
            }
            Sched::Never => i32::MAX,
        }
    }

    fn from_when(when: &When, ctx: &WhenContext) -> Sched {
        match *when {
            When::Retirement { offset_years } => {
                Sched::Retire((offset_years * 12.0).round() as i32)
            }
            _ => when
                .resolve_month_index(ctx)
                .map_or(Sched::Never, Sched::At),
        }
    }

    fn start(when: Option<&When>, ctx: &WhenContext) -> Sched {
        when.map_or(Sched::At(i32::MIN), |w| Sched::from_when(w, ctx))
    }

    pub(crate) fn end(when: Option<&When>, ctx: &WhenContext) -> Sched {
        when.map_or(Sched::Never, |w| Sched::from_when(w, ctx))
    }
}

/// One adult.
#[derive(Clone, Debug)]
pub(crate) struct PersonPlan {
    pub birth: YearMonth,
    pub sex: Sex,
    /// Age in months at `start`.
    pub age0: i32,
    /// Month index at which the person joins the household (0 or less: there from
    /// the start).
    pub joins: i32,
    pub mortality: MortalityModel,
    pub claim_age: f64,
    pub basis: BenefitBasis,
    pub record0: EarningsRecord,
    /// Present value factors of the person's own Social Security benefit.
    pub ss_annuity: BenefitAnnuity,
}

/// A child's modelled costs, today's dollars per year by age in whole years.
#[derive(Clone, Debug)]
pub(crate) struct ChildPlan {
    /// Month index of birth (negative: already born).
    pub birth: i32,
    /// Core cost by age 0..18 (before the family-size adjustment); zeros when the
    /// budget already holds the child's costs.
    pub core: [f64; 18],
    /// Childcare by age 0..18 (non-zero for ages 0 to 4 only).
    pub care: [f64; 18],
    /// College cost per year for ages 18 to 21, the parents' share.
    pub college: f64,
    /// What a whole year at each age 0..22 adds to the funded test's lump for
    /// costs still due: the year's cost times twelve months over twelve, exactly
    /// as the test computes it.
    pub lump_year: [f64; 22],
}

/// Annual real growth of childcare and college prices above CPI
/// (`children.childcare.inflation`, `children.college.inflation_over_cpi.default_pct`).
pub(crate) const CHILD_SERVICE_REAL_GROWTH: f64 = 0.01;

/// A job income.
#[derive(Clone, Debug)]
pub(crate) struct JobPlan {
    pub owner: usize,
    pub name: String,
    pub model: JobModel,
    pub plan: Option<EmployerPlan>,
    pub hsa_real: f64,
    pub hsa_employer_real: f64,
    pub bonus_target: f64,
    pub commission_real: f64,
    pub start: Sched,
    pub end: Sched,
    pub ends_at_retirement: bool,
}

/// A self-employment income.
#[derive(Clone, Debug)]
pub(crate) struct SePlan {
    pub owner: usize,
    /// Position in `profile.incomes`: the index of its `Stream::Business`.
    pub stream: usize,
    pub model: SelfEmploymentModel,
    pub solo_fraction: f64,
    pub start: Sched,
    pub end: Sched,
    pub ends_at_retirement: bool,
}

/// Kind of a non-earned income stream.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum StreamKind {
    Pension {
        cola: Cola,
        survivor_fraction: f64,
    },
    Annuity {
        real: bool,
    },
    Other {
        taxable: bool,
        growth: f64,
        ends_at_retirement: bool,
    },
}

/// A pension, annuity or other income.
#[derive(Clone, Debug)]
pub(crate) struct StreamPlan {
    pub owner: usize,
    pub kind: StreamKind,
    /// Today's dollars per year.
    pub annual: f64,
    pub start: Sched,
    pub end: Sched,
    /// Present value at each month (conditional on the owner being alive) of one
    /// real dollar a year from the start date, for pensions and annuities with a
    /// fixed start date; empty otherwise.
    pub pv: Vec<f64>,
}

/// A budget line.
#[derive(Clone, Debug)]
pub(crate) struct ItemPlan {
    pub category: Category,
    pub essential: bool,
    /// Today's dollars per month while working.
    pub monthly: f64,
    /// Today's dollars per month once retired.
    pub monthly_retired: f64,
    /// Annual growth above CPI.
    pub growth: f64,
    pub start: Sched,
    pub end: Sched,
    pub owner: Option<usize>,
    /// A rent line that follows the path's rent market (`rent_growth: market`).
    pub market_rent: bool,
}

/// A property slot: `profile.properties` first, then one per `buy_home` event.
#[derive(Clone, Debug)]
pub(crate) struct PropPlan {
    pub id: String,
    pub name: String,
    pub model: PropertyModel,
}

/// A non-mortgage debt slot: `profile.debts` first, then one per financed expense.
#[derive(Clone, Debug)]
pub(crate) struct DebtPlan {
    pub name: String,
    pub balance: f64,
    pub rate: f64,
    /// Fixed nominal monthly payment; 0 for a revolving minimum.
    pub payment: f64,
    /// Credit-card style minimum: 2% of the balance, at least $35.
    pub revolving: bool,
}

/// Which income an event changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum IncomeRef {
    Job(usize),
    Se(usize),
    Stream(usize),
}

/// Which budget lines an event changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SpendRef {
    Item(usize),
    Category(Category),
    All,
}

/// A compiled event action.
#[derive(Clone, Debug)]
pub(crate) enum EvKind {
    BuyHome {
        slot: usize,
        replaces_rent: bool,
    },
    Sell {
        slot: Option<usize>,
        cost_fraction: f64,
        then_rent: f64,
    },
    Relocate {
        state: StateCode,
        in_us: bool,
        foreign_rate: Option<f64>,
        local: Box<LocalTax>,
        moving_cost: f64,
        factors: [f64; Category::COUNT],
    },
    Windfall {
        amount: f64,
        taxable: bool,
    },
    Expense {
        amount: f64,
        /// `(down fraction, debt slot, rate, term months)`.
        financed: Option<(f64, usize, f64, u32)>,
    },
    IncomeChange {
        target: Option<IncomeRef>,
        change: IncomeChangeKind,
    },
    CareerBreak {
        person: usize,
        months: u32,
        fraction: f64,
    },
    SpendingChange {
        target: SpendRef,
        change: SpendingChangeKind,
    },
    ContributionChange {
        job: Option<usize>,
        fraction: f64,
    },
    Divorce(crate::model::DivorceTerms),
}

/// A compiled life event.
#[derive(Clone, Debug)]
pub(crate) struct EventPlan {
    pub name: String,
    pub when: Sched,
    pub probability: f64,
    pub uncertainty_months: f64,
    pub kind: EvKind,
}

/// A compiled stress overlay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum StressPlan {
    Crash {
        drawdown: f64,
        recovery_months: usize,
    },
    LostDecade {
        start: Sched,
    },
    Stagflation {
        start: Sched,
        months: usize,
        inflation: f64,
    },
    HistoricalEpisode {
        start: Sched,
        episode: Episode,
    },
    LostGeneration {
        start: Sched,
        months: usize,
    },
}

/// The retirement trigger with its dates resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TriggerPlan {
    /// Retired from the first month.
    Already,
    /// Retire in a fixed month.
    AtMonth(i32),
    /// Retire when funded, between `min_month` and `max_month`.
    Funded {
        rule: WhenFunded,
        min_month: i32,
        max_month: i32,
    },
    /// Retire when wealth reaches a fixed real amount.
    Fixed { amount: f64, basis: WealthBasis },
}

/// Irregular-expense parameters (`expense_shocks.model`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct ShockPlan {
    /// Probability of a shock in a month.
    pub monthly_probability: f64,
    /// `ln` of the median shock as a fraction of annual spending.
    pub log_median: f64,
    pub sigma: f64,
    /// Expected annual cost as a fraction of annual spending, taken out of the
    /// baseline budget so that shocks add variance, not level (0 when shocks are off).
    pub mean_fraction: f64,
}

/// A profile compiled for simulation. Immutable; shared by reference across paths.
#[derive(Clone, Debug)]
pub struct Plan {
    pub(crate) profile: Profile,
    pub(crate) start: YearMonth,
    pub(crate) horizon_months: u32,
    pub(crate) people: Vec<PersonPlan>,
    pub(crate) children: Vec<ChildPlan>,
    /// Bit `k` set when some child's birthday falls in a month with `t % 12 == k`.
    pub(crate) birthday_months: u32,
    /// Opt-in family features (children's health cover, uncertain births, ...).
    pub(crate) family: super::family::FamilyPlan,
    pub(crate) jobs: Vec<JobPlan>,
    pub(crate) selfemp: Vec<SePlan>,
    /// Opt-in career-realism settings (`career_realism`).
    pub(crate) rz: crate::career_realism::RealismPlan,
    pub(crate) streams: Vec<StreamPlan>,
    pub(crate) items: Vec<ItemPlan>,
    pub(crate) props: Vec<PropPlan>,
    /// Which opt-in housing realism options are on.
    pub(crate) hx: super::housing_ext::HxPlan,
    pub(crate) debts: Vec<DebtPlan>,
    pub(crate) events: Vec<EventPlan>,
    pub(crate) stresses: Vec<StressPlan>,
    pub(crate) trigger: TriggerPlan,
    /// Months the second person keeps working after the household retires.
    pub(crate) partner_offset: i32,
    /// Age of the first person by which the plan expects retirement, for the
    /// success test of paths that never retire.
    pub(crate) latest_retirement_age: f64,
    /// Real discount rate of the funded test's bridge: the return at which the
    /// test's withdrawal rate exhausts a portfolio in [`BRIDGE_HORIZON_YEARS`].
    pub(crate) bridge_discount: f64,
    /// `(1 + bridge_discount)^-(m / 12)` for a wait of `m` months, `m` up to the
    /// horizon: the powers the bridge needs, computed once.
    pub(crate) bridge_pow: Vec<f64>,
    /// `(1 + growth)^years` for each budget line and each year of the plan
    /// (`item_growth[line * growth_years + years]`), and the same for rent taken
    /// on after a home sale and for childcare and college prices.
    pub(crate) item_growth: Vec<f64>,
    pub(crate) rent_growth: Vec<f64>,
    pub(crate) service_growth: Vec<f64>,
    /// Years covered by the growth tables.
    pub(crate) growth_years: usize,
    /// Month by which Coast-FI is judged (the planned or latest retirement month).
    pub(crate) coast_month: i32,
    /// `(1 + expected real return)^-(years from month t to coast_month)`: what a
    /// dollar needed at the coast month is worth in month `t`.
    pub(crate) coast_discount: Vec<f64>,
    pub(crate) econ: EconomyModel,
    pub(crate) law: TaxLaw,
    pub(crate) ss: SocialSecurityModel,
    pub(crate) health: HealthModel,
    pub(crate) planner: SpendingPlanner,
    pub(crate) accounts0: Accounts,
    /// Accounts of a partner who joins later, added in the month they join.
    pub(crate) joiner_accounts: Accounts,
    pub(crate) cash_terms: CashTerms,
    pub(crate) fee_monthly: f64,
    pub(crate) target0: Weights,
    /// Expected real return of the target mix after fees, for planning tests.
    pub(crate) expected_real_return: f64,
    pub(crate) shocks: ShockPlan,
    /// Opt-in spending realism (vehicles, seasonality, price wedges, detailed shocks,
    /// unemployment cut); see `spendx.rs`.
    pub(crate) sx: super::spendx::SpendXPlan,
    pub(crate) state0: StateCode,
    pub(crate) in_us0: bool,
    pub(crate) foreign_rate0: Option<f64>,
    pub(crate) local0: LocalTax,
    /// Home-sale exclusion by filing status index.
    pub(crate) home_exclusion: [f64; 3],
    /// Mortgage debt on which interest is deductible.
    pub(crate) acquisition_debt_limit: f64,
    /// Rate the tax-aware plan fills to, or the default expectation of the
    /// retirement tax rate.
    pub(crate) retirement_rate: f64,
    pub(crate) rebalance_band: Option<f64>,
    /// Annual cash yield of the custom asset class.
    pub(crate) custom_yield: f64,
    pub(crate) rebalance_annual: bool,
    /// True when the portfolio is rebalanced on a schedule, so contributions and
    /// withdrawals need not steer the mix themselves.
    pub(crate) plain_mix: bool,
    /// Bit `m` set when some job pays its bonus in calendar month `m`, or `m` is
    /// January: the months in which the trailing 12-month stock return is used.
    pub(crate) eq12_months: u32,
    /// Household earned-income rate at the start, today's dollars per year.
    pub(crate) income_rate0: f64,
    /// Housing decisions (home-equity borrowing, downsizing, refinancing, rental
    /// dynamics), all opt-in.
    pub(crate) hd: super::housing_decisions::HdPlan,
    /// The opt-in health, longevity and care features (`sim/care.rs`).
    pub(crate) care: super::care::CarePlan,
    /// Behaviour features (`sim/behavior`), all off by default.
    pub(crate) beh: super::behavior::BehaviorPlan,
    /// Household dynamics: divorce, parents, caregiving, life insurance.
    pub(crate) dynamics: super::dynamics::DynamicsPlan,
}

fn num(v: &serde_json::Value, path: &[&str]) -> f64 {
    let mut cur = v;
    for key in path {
        cur = &cur[*key];
    }
    cur.as_f64()
        .unwrap_or_else(|| panic!("embedded data: missing number at {}", path.join(".")))
}

/// Price level at the end of the history relative to `from` (CPI-U compounded).
fn cpi_since(data: &Data, from: YearMonth) -> f64 {
    let h = &data.history;
    let first = h.index_of(from).unwrap_or(0);
    let mut level = 1.0;
    for c in &h.cpi[first..] {
        if c.is_finite() {
            level *= 1.0 + c;
        }
    }
    level
}

/// Present value at each month of one real dollar a month paid from `start` while
/// alive, with the payment's real value multiplied by `decay` every month after it
/// starts (below 1 for a pension without a cost-of-living adjustment).
fn pv_table(n: usize, start: i32, survival: &[f64], decay: f64, rate: f64) -> Vec<f64> {
    let discount = 1.0 / (1.0 + math::annual_to_monthly(rate));
    let mut out = vec![0.0; n];
    let mut next = 0.0;
    for t in (0..n).rev() {
        let alive = survival.get(t).copied().unwrap_or(0.0);
        let value = if alive > 0.0 {
            let months_paid = t as i32 - start;
            let pay = if months_paid >= 0 {
                math::powi(decay, months_paid)
            } else {
                0.0
            };
            let survive = survival.get(t + 1).copied().unwrap_or(0.0) / alive;
            pay + discount * survive * next
        } else {
            0.0
        };
        out[t] = value;
        next = value;
    }
    out
}

impl Plan {
    /// Validates and compiles a profile.
    ///
    /// Fails with the validation report if there are errors, or with a single
    /// `start_unresolved` error if `profile.start` is empty: core has no clock, so
    /// the caller must fill it first (`Profile::with_start`).
    pub fn compile(profile: &Profile, data: &Data) -> Result<Plan, Report> {
        let Some(start) = profile.start else {
            return Err(Report::single_error(
                "start_unresolved",
                "/start",
                "The profile has no start month. Set \"start\" to the current month (YYYY-MM) before simulating.",
            ));
        };
        let report = validate(profile);
        if !report.is_ok() {
            return Err(report);
        }
        let ctx = profile
            .when_context()
            .expect("start is set, so the context exists");
        // The horizon runs until the youngest adult would reach end_age; individual
        // paths end earlier when everyone has died.
        let youngest = profile
            .household
            .people
            .iter()
            .map(|p| p.birth)
            .max()
            .expect("validated: at least one person");
        let end = youngest + (profile.simulation.end_age * 12) as i32;
        let horizon_months = (end - start).max(1) as u32;
        let n = horizon_months as usize;

        let law = TaxLaw::from_data(data, &profile.assumptions.taxes)
            .map_err(|e| Report::single_error("data", "/assumptions/taxes", e.to_string()))?;
        let ss = SocialSecurityModel::new(
            data,
            profile.assumptions.taxes.social_security_haircut,
            start,
        );
        let location = &profile.household.location;
        let in_us0 = location.is_us();
        let state0 = if in_us0 {
            location
                .state
                .as_deref()
                .and_then(StateCode::parse)
                .unwrap_or(StateCode::NONE)
        } else {
            StateCode::NONE
        };
        let health = HealthModel::new(
            &profile.spending.healthcare,
            location.state.as_deref(),
            profile.assumptions.health_risk,
            data,
        );
        let career_params = CareerParams::from_data(data);
        let funded_rate = match profile.retirement.trigger {
            RetirementTrigger::WhenFunded(rule) => rule.withdrawal_rate,
            _ => WhenFunded::default().withdrawal_rate,
        };
        let income_discount = funded_rate.max(INCOME_DISCOUNT_RATE);
        let bridge_discount = bridge_discount_rate(funded_rate);

        // Incomes.
        let mut jobs = Vec::new();
        let mut selfemp = Vec::new();
        let mut streams = Vec::new();
        let mut income_refs: Vec<(String, IncomeRef)> = Vec::new();
        let mut earnings = [0.0f64; MAX_PEOPLE];
        let mut growth = [WageGrowth::default(); MAX_PEOPLE];
        let mut growth_pay = [0.0f64; MAX_PEOPLE];
        let mut survival: Vec<Vec<f64>> = Vec::new();
        let mut mortality = Vec::new();
        for (i, person) in profile.household.people.iter().enumerate() {
            let m = MortalityModel::new_scaled(
                person.birth,
                person.sex,
                person.health,
                start,
                &profile.assumptions.longevity,
                crate::longevity_links::income_multiplier(profile, i, data),
                data,
            );
            survival.push(m.survival_table().to_vec());
            mortality.push(m);
        }
        for income in &profile.incomes {
            let owner = income.owner.min(profile.household.people.len() - 1);
            let person = &profile.household.people[owner];
            let age0 = person.birth.age_years_at(start);
            let joins = person.joins.map_or(i32::MIN, |j| j - start);
            let mut begin = Sched::start(income.start.as_ref(), &ctx);
            if let Sched::At(m) = begin {
                begin = Sched::At(m.max(joins));
            }
            let finish = Sched::end(income.end.as_ref(), &ctx);
            let active_now = begin.month(i32::MAX) <= 0;
            match &income.kind {
                IncomeKind::Job(job) => {
                    if active_now {
                        earnings[owner] += job.salary
                            * (1.0 + job.bonus.map_or(0.0, |b| b.target_fraction))
                            + job.commission.map_or(0.0, |c| c.expected_annual);
                        // The past is rebuilt on the curve of the main job.
                        if job.salary >= growth_pay[owner] {
                            growth_pay[owner] = job.salary;
                            growth[owner] = job.growth;
                        }
                    }
                    income_refs.push((income.id.clone(), IncomeRef::Job(jobs.len())));
                    jobs.push(JobPlan {
                        owner,
                        name: if income.name.is_empty() {
                            income.id.clone()
                        } else {
                            income.name.clone()
                        },
                        model: JobModel::with_params(
                            job,
                            age0,
                            profile.assumptions.career_risk,
                            career_params.clone(),
                        ),
                        plan: job.plan,
                        hsa_real: job.hsa_contribution.max(0.0),
                        hsa_employer_real: job.hsa_employer.max(0.0),
                        bonus_target: job.bonus.map_or(0.0, |b| b.target_fraction),
                        commission_real: job.commission.map_or(0.0, |c| c.expected_annual),
                        start: begin,
                        end: finish,
                        ends_at_retirement: job.ends_at_retirement,
                    });
                }
                IncomeKind::SelfEmployment(se) => {
                    if active_now {
                        earnings[owner] += se.net_annual.max(0.0);
                    }
                    income_refs.push((income.id.clone(), IncomeRef::Se(selfemp.len())));
                    selfemp.push(SePlan {
                        owner,
                        stream: profile
                            .incomes
                            .iter()
                            .position(|x| std::ptr::eq(x, income))
                            .unwrap_or(selfemp.len()),
                        model: SelfEmploymentModel::with_start(
                            se,
                            profile.assumptions.career_risk,
                            active_now,
                            data,
                        ),
                        solo_fraction: se.solo_401k_fraction.clamp(0.0, 1.0),
                        start: begin,
                        end: finish,
                        ends_at_retirement: se.ends_at_retirement,
                    });
                }
                IncomeKind::Pension(_) | IncomeKind::Annuity(_) | IncomeKind::Other(_) => {
                    let (kind, annual) = match &income.kind {
                        IncomeKind::Pension(p) => (
                            StreamKind::Pension {
                                cola: p.cola,
                                survivor_fraction: p.survivor_fraction.clamp(0.0, 1.0),
                            },
                            p.annual,
                        ),
                        IncomeKind::Annuity(a) => (StreamKind::Annuity { real: a.real }, a.annual),
                        IncomeKind::Other(o) => (
                            StreamKind::Other {
                                taxable: o.taxable,
                                growth: o.real_growth,
                                ends_at_retirement: o.ends_at_retirement,
                            },
                            o.annual,
                        ),
                        _ => unreachable!(),
                    };
                    let decay = match kind {
                        StreamKind::Pension {
                            cola: Cola::None, ..
                        }
                        | StreamKind::Annuity { real: false } => {
                            1.0 / (1.0 + math::annual_to_monthly(PLANNING_INFLATION))
                        }
                        StreamKind::Pension {
                            cola: Cola::Partial { rate },
                            ..
                        } => {
                            (1.0 + math::annual_to_monthly(rate))
                                / (1.0 + math::annual_to_monthly(PLANNING_INFLATION))
                        }
                        _ => 1.0,
                    };
                    let pv = match (kind, begin) {
                        (StreamKind::Other { .. }, _) => Vec::new(),
                        (_, Sched::At(m)) => {
                            pv_table(n, m.max(0), &survival[owner], decay, income_discount)
                        }
                        _ => Vec::new(),
                    };
                    income_refs.push((income.id.clone(), IncomeRef::Stream(streams.len())));
                    streams.push(StreamPlan {
                        owner,
                        kind,
                        annual: annual.max(0.0),
                        start: begin,
                        end: finish,
                        pv,
                    });
                }
            }
        }

        // People.
        let mut people = Vec::new();
        for (i, person) in profile.household.people.iter().enumerate() {
            let age0 = person.birth.age_months_at(start);
            let basis = BenefitBasis::from(&person.social_security.basis);
            let record0 = match person.social_security.basis {
                SocialSecurityBasis::FromEarnings { career_start_age } => {
                    // On the curve the career model follows from here on, so that
                    // the last rebuilt year and the first simulated one are one curve.
                    EarningsRecord::backfill_with(
                        earnings[i],
                        f64::from(age0) / 12.0,
                        career_start_age,
                        &ss,
                        |age| crate::career::wage_curve_level(&growth[i], age, &career_params),
                    )
                }
                _ => EarningsRecord::default(),
            };
            let ss_annuity = ss.annuity(
                person.birth,
                person.social_security.planned_claim_age(),
                income_discount,
                &survival[i],
            );
            people.push(PersonPlan {
                birth: person.birth,
                sex: person.sex,
                age0,
                joins: person.joins.map_or(i32::MIN, |j| j - start),
                mortality: mortality[i].clone(),
                claim_age: person.social_security.planned_claim_age(),
                basis,
                record0,
                ss_annuity,
            });
        }
        let income_rate0: f64 = earnings.iter().sum();
        let care = super::care::CarePlan::compile(profile, &people, data);
        let ages0: Vec<i32> = people.iter().map(|p| p.age0).collect();
        let beh = super::behavior::BehaviorPlan::compile(profile, data, &ages0);

        // Budget.
        let items: Vec<ItemPlan> = profile
            .spending
            .items
            .iter()
            .map(|item| {
                let per_month = item.frequency.per_year() / 12.0;
                let monthly = item.amount * per_month;
                let monthly_retired = match item.in_retirement {
                    RetirementChange::Same => monthly,
                    RetirementChange::Stops => 0.0,
                    RetirementChange::Factor { factor } => monthly * factor,
                    RetirementChange::Amount { amount } => amount * per_month,
                };
                let market_rent = item.category == Category::Rent
                    && item.rent_growth == crate::model::RentGrowth::Market;
                ItemPlan {
                    category: item.category,
                    essential: item.essential,
                    monthly,
                    monthly_retired,
                    growth: if market_rent {
                        0.0
                    } else {
                        item.effective_real_growth()
                    },
                    start: Sched::start(item.start.as_ref(), &ctx),
                    end: Sched::end(item.end.as_ref(), &ctx),
                    owner: item.owner,
                    market_rent,
                }
            })
            .collect();

        // Children.
        let kids = &data.actuarial["children"];
        let usda_factor = cpi_since(data, YearMonth::ym(2015, 7));
        let care_factor = cpi_since(data, YearMonth::ym(2025, 7));
        let tiers = [
            "low_income_under_59200",
            "middle_income_59200_to_107400",
            "high_income_over_107400",
        ];
        // The USDA tiers are by pre-tax income. A household with little or no pay
        // at the start (retired, between jobs) is placed by what it spends instead:
        // its budget grossed up by `CHILD_TIER_SPENDING_SHARE`.
        let budget0: f64 = profile
            .spending
            .items
            .iter()
            .filter(|item| item.start.is_none())
            .map(|item| item.amount * item.frequency.per_year())
            .sum();
        let tier_income = income_rate0.max(budget0 / CHILD_TIER_SPENDING_SHARE);
        let tier = if tier_income < 59_200.0 * usda_factor {
            0
        } else if tier_income < 107_400.0 * usda_factor {
            1
        } else {
            2
        };
        let bands = ["0-2", "3-5", "6-8", "9-11", "12-14", "15-17"];
        let core_values = &kids["recommended_core_cost_2015_dollars"]["values"][tiers[tier]];
        let care_defaults = &kids["childcare"]["default_full_time_annual_2025_dollars"];
        let college_defaults = &kids["college"]["default_annual_cost_today_dollars"];
        let children: Vec<ChildPlan> = profile
            .household
            .children
            .iter()
            .map(|child| {
                let mut core = [0.0; 18];
                let mut care = [0.0; 18];
                if child.costs == ChildCosts::Modeled {
                    for (age, slot) in core.iter_mut().enumerate() {
                        *slot = num(core_values, &[bands[age / 3]]) * usda_factor;
                    }
                    for (age, slot) in care.iter_mut().enumerate().take(5) {
                        *slot = match child.childcare_monthly {
                            Some(m) => m.max(0.0) * 12.0,
                            None => {
                                let key = match age {
                                    0 | 1 => "age_0_1",
                                    2 => "age_2",
                                    _ => "age_3_4",
                                };
                                num(care_defaults, &[key]) * care_factor
                            }
                        };
                    }
                }
                let college = match child.college {
                    College::None => 0.0,
                    College::PublicInState => num(college_defaults, &["public_in_state"]),
                    College::PublicOutOfState => num(college_defaults, &["public_out_of_state"]),
                    College::Private => num(college_defaults, &["private"]),
                    College::Custom { annual } => annual.max(0.0),
                } * child.college_share.clamp(0.0, 1.0);
                let mut lump_year = [0.0; 22];
                for (years, lump) in lump_year.iter_mut().enumerate() {
                    let annual = if years < 18 {
                        core[years] + care[years]
                    } else {
                        college
                    };
                    *lump = annual * f64::from(12) / 12.0;
                }
                ChildPlan {
                    birth: child.birth - start,
                    core,
                    care,
                    college,
                    lump_year,
                }
            })
            .collect();
        let mut children = children;
        super::family::drop_usda_health_share(
            &mut children,
            profile,
            data,
            tiers[tier],
            usda_factor,
        );

        // Properties: existing ones, then one slot per purchase event.
        let default_property_tax = law.property_tax_rate(state0);
        let mut props: Vec<PropPlan> = profile
            .properties
            .iter()
            .map(|p| PropPlan {
                id: p.id.clone(),
                name: if p.name.is_empty() {
                    p.id.clone()
                } else {
                    p.name.clone()
                },
                model: PropertyModel::from_property(p, default_property_tax, data),
            })
            .collect();

        // Debts: existing ones, then one slot per financed expense.
        let mut debts: Vec<DebtPlan> = profile
            .debts
            .iter()
            .map(|d| {
                let balance = d.balance.max(0.0);
                let (payment, revolving) = match (d.payment, d.remaining_months) {
                    (Some(p), _) => (p.max(0.0), false),
                    (None, Some(m)) => (amortized_payment(balance, d.rate, m.max(1)), false),
                    (None, None) if d.kind == DebtKind::CreditCard => (0.0, true),
                    (None, None) => (amortized_payment(balance, d.rate, 60), false),
                };
                DebtPlan {
                    name: if d.name.is_empty() {
                        d.id.clone()
                    } else {
                        d.name.clone()
                    },
                    balance,
                    rate: d.rate.max(0.0),
                    payment,
                    revolving,
                }
            })
            .collect();

        // Events.
        let mut events = Vec::new();
        for event in &profile.events {
            let kind = match &event.kind {
                EventKind::BuyHome(buy) => {
                    let rate = buy.property_tax_rate.unwrap_or(default_property_tax);
                    let slot = props.len();
                    props.push(PropPlan {
                        id: buy.property_id.clone(),
                        name: if event.name.is_empty() {
                            buy.property_id.clone()
                        } else {
                            event.name.clone()
                        },
                        model: PropertyModel::from_purchase(buy, rate, data),
                    });
                    EvKind::BuyHome {
                        slot,
                        replaces_rent: buy.replaces_rent
                            && matches!(buy.usage, PropertyUse::Primary),
                    }
                }
                EventKind::SellProperty(sell) => EvKind::Sell {
                    // Purchases made by earlier events are already in the table.
                    slot: props.iter().position(|p| p.id == sell.property),
                    cost_fraction: sell.selling_cost_fraction.max(0.0),
                    then_rent: sell.then_rent_monthly.unwrap_or(0.0).max(0.0),
                },
                EventKind::Relocate(r) => {
                    let in_us = r.country.eq_ignore_ascii_case("US");
                    let state = if in_us {
                        r.state
                            .as_deref()
                            .and_then(StateCode::parse)
                            .unwrap_or(StateCode::NONE)
                    } else {
                        StateCode::NONE
                    };
                    let mut factors = [1.0; Category::COUNT];
                    match &r.cost_of_living {
                        CostOfLivingChange::Unchanged => {}
                        CostOfLivingChange::Factor { overall } => {
                            factors = [overall.max(0.0); Category::COUNT];
                        }
                        CostOfLivingChange::ByCategory { factors: map } => {
                            for (category, f) in map {
                                factors[category.index()] = f.max(0.0);
                            }
                        }
                    }
                    EvKind::Relocate {
                        state,
                        in_us,
                        foreign_rate: if in_us { None } else { r.foreign_tax_rate },
                        local: Box::new(law.local_tax(state, r.city.as_deref())),
                        moving_cost: r.moving_cost.max(0.0),
                        factors,
                    }
                }
                EventKind::Windfall { amount, taxable } => EvKind::Windfall {
                    amount: amount.max(0.0),
                    taxable: *taxable,
                },
                EventKind::Expense { amount, financed } => EvKind::Expense {
                    amount: amount.max(0.0),
                    financed: financed.map(|f| {
                        let slot = debts.len();
                        debts.push(DebtPlan {
                            name: if event.name.is_empty() {
                                event.id.clone()
                            } else {
                                event.name.clone()
                            },
                            balance: 0.0,
                            rate: f.rate.max(0.0),
                            payment: 0.0,
                            revolving: false,
                        });
                        (
                            f.down_fraction.clamp(0.0, 1.0),
                            slot,
                            f.rate.max(0.0),
                            f.term_months.max(1),
                        )
                    }),
                },
                EventKind::IncomeChange { income, change } => EvKind::IncomeChange {
                    target: income_refs
                        .iter()
                        .find(|(id, _)| id == income)
                        .map(|(_, r)| *r),
                    change: *change,
                },
                EventKind::CareerBreak {
                    person,
                    months,
                    income_fraction,
                } => EvKind::CareerBreak {
                    person: *person,
                    months: *months,
                    fraction: *income_fraction,
                },
                EventKind::SpendingChange { target, change } => EvKind::SpendingChange {
                    target: match target {
                        SpendingTarget::Item { id } => profile
                            .spending
                            .items
                            .iter()
                            .position(|i| &i.id == id)
                            .map_or(SpendRef::All, SpendRef::Item),
                        SpendingTarget::Category { category } => SpendRef::Category(*category),
                        SpendingTarget::All => SpendRef::All,
                    },
                    change: *change,
                },
                EventKind::ContributionChange {
                    income,
                    employee_fraction,
                } => EvKind::ContributionChange {
                    job: income_refs.iter().find_map(|(id, r)| match r {
                        IncomeRef::Job(j) if id == income => Some(*j),
                        _ => None,
                    }),
                    fraction: employee_fraction.clamp(0.0, 1.0),
                },
                EventKind::Divorce(terms) => EvKind::Divorce(*terms),
            };
            events.push(EventPlan {
                name: if event.name.is_empty() {
                    event.id.clone()
                } else {
                    event.name.clone()
                },
                when: Sched::from_when(&event.when, &ctx),
                probability: event.probability,
                uncertainty_months: (event.timing_uncertainty_years * 12.0).max(0.0),
                kind,
            });
        }

        let (sx, sx_mean_fraction) =
            super::spendx::SpendXPlan::compile(profile, data, &ctx, &mut debts, budget0);
        if let Some(det) = &sx.detailed {
            for prop in &mut props {
                prop.model.carve_out_maintenance(det.home_rate);
            }
        }

        let stresses = profile
            .assumptions
            .market
            .stress
            .iter()
            .map(|s| match *s {
                Stress::CrashAtRetirement {
                    drawdown,
                    recovery_years,
                } => StressPlan::Crash {
                    drawdown,
                    recovery_months: (recovery_years * 12.0).round().max(0.0) as usize,
                },
                Stress::LostDecade { start } => StressPlan::LostDecade {
                    start: Sched::from_when(&start, &ctx),
                },
                Stress::Stagflation {
                    start,
                    years,
                    inflation,
                } => StressPlan::Stagflation {
                    start: Sched::from_when(&start, &ctx),
                    months: (years * 12.0).round().max(0.0) as usize,
                    inflation,
                },
                Stress::HistoricalEpisode { start, episode } => StressPlan::HistoricalEpisode {
                    start: Sched::from_when(&start, &ctx),
                    episode,
                },
                Stress::LostGeneration { start, years } => StressPlan::LostGeneration {
                    start: Sched::from_when(&start, &ctx),
                    months: (years * 12.0).round().max(0.0) as usize,
                },
            })
            .collect();

        // Retirement trigger.
        let first_birth = profile.household.people[0].birth;
        let age_month = |age: f64| first_birth.at_age(age) - start;
        let (trigger, latest_retirement_age, coast_month) = match profile.retirement.trigger {
            RetirementTrigger::AlreadyRetired => {
                (TriggerPlan::Already, first_birth.age_years_at(start), 0)
            }
            RetirementTrigger::AtAge { person, age } => {
                let birth = profile
                    .household
                    .people
                    .get(person)
                    .map_or(first_birth, |p| p.birth);
                let m = (birth.at_age(age) - start).max(0);
                (
                    TriggerPlan::AtMonth(m),
                    first_birth.age_years_at(start + m),
                    m,
                )
            }
            RetirementTrigger::AtDate { at } => {
                let m = (at - start).max(0);
                (
                    TriggerPlan::AtMonth(m),
                    first_birth.age_years_at(start + m),
                    m,
                )
            }
            RetirementTrigger::WhenFunded(rule) => {
                let max_month = age_month(rule.max_age).max(0);
                (
                    TriggerPlan::Funded {
                        rule,
                        min_month: rule.min_age.map_or(0, age_month).max(0),
                        max_month,
                    },
                    rule.max_age,
                    max_month,
                )
            }
            RetirementTrigger::FixedNumber { amount, basis } => (
                TriggerPlan::Fixed { amount, basis },
                DEFAULT_LATEST_RETIREMENT_AGE,
                age_month(DEFAULT_LATEST_RETIREMENT_AGE).max(0),
            ),
        };

        // Accounts: a partner who joins later brings theirs then.
        let joiner = profile
            .household
            .people
            .get(1)
            .and_then(|p| p.joins)
            .is_some_and(|j| j > start);
        let (accounts0, joiner_accounts) = if joiner {
            let mut base = profile.clone();
            base.accounts.retain(|a| a.owner != 1);
            let mut theirs = profile.clone();
            theirs.accounts.retain(|a| a.owner == 1);
            (
                Accounts::from_profile(&base),
                Accounts::from_profile(&theirs),
            )
        } else {
            (Accounts::from_profile(profile), Accounts::default())
        };

        let policy = &profile.portfolio;
        let target0 = accounts::glide_target(policy, None);
        let planner = SpendingPlanner::from_data(profile.retirement.spending_rule, data);
        let fee_annual = policy.expense_ratio.max(0.0) + policy.advisory_fee.max(0.0);
        let expected_real_return = planner.params.vpw_real_return(&target0) - fee_annual;

        let shock_model = &data.actuarial["expense_shocks"]["model"];
        let lambda = match profile.spending.shocks {
            ShockLevel::Off => 0.0,
            ShockLevel::Typical => num(shock_model, &["lambda_per_year", "default"]),
            ShockLevel::High => 2.0 * num(shock_model, &["lambda_per_year", "default"]),
            ShockLevel::Detailed => 0.0,
        };
        let median = num(shock_model, &["median_fraction_of_annual_spending"]);
        let sigma = num(shock_model, &["sigma"]);
        let monthly_probability = 1.0 - math::exp(-lambda / 12.0);
        let shocks = ShockPlan {
            monthly_probability,
            log_median: math::ln(median),
            sigma,
            mean_fraction: 12.0 * monthly_probability * median * math::exp(0.5 * sigma * sigma)
                + sx_mean_fraction,
        };

        let exclusion = &data.tax_federal["capital_gains_and_property"]["home_sale_exclusion"];
        let home_exclusion = if exclusion.is_object() {
            [
                num(exclusion, &["single"]),
                num(exclusion, &["married_joint"]),
                num(exclusion, &["head_of_household"]),
            ]
        } else {
            [250_000.0, 500_000.0, 250_000.0]
        };
        let acquisition_debt_limit =
            find_number(&data.tax_federal, "acquisition_debt_limit").unwrap_or(750_000.0);

        let econ =
            EconomyModel::new(&profile.assumptions.market, data).with_assets(&profile.portfolio);
        let retirement_rate = match profile.retirement.withdrawal_order {
            WithdrawalOrder::TaxAware {
                fill_to_bracket, ..
            } => fill_to_bracket,
            _ => 0.12,
        };
        let (rebalance_band, rebalance_annual) = match policy.rebalance {
            Rebalance::Never => (None, false),
            Rebalance::Annual => (None, true),
            Rebalance::Bands { width } => (Some(width.max(0.0)), false),
        };
        let mut eq12_months = 1u32 << 1;
        for income in &profile.incomes {
            if let IncomeKind::Job(job) = &income.kind {
                if let Some(b) = job.bonus {
                    eq12_months |= 1 << b.month.clamp(1, 12);
                }
            }
        }
        let local0 = law.local_tax(state0, location.city.as_deref());
        let foreign_rate0 = if in_us0 {
            None
        } else if matches!(profile.assumptions.taxes.kind, TaxKind::UsCurrentLaw) {
            Some(location.foreign_tax_rate.unwrap_or(0.0))
        } else {
            None
        };

        let coast_step = 1.0 / (1.0 + math::annual_to_monthly(expected_real_return.max(-0.5)));
        let coast_discount: Vec<f64> = (0..=n as i32)
            .map(|t| math::powi(coast_step, (coast_month - t).max(0)))
            .collect();

        let bridge_pow: Vec<f64> = (0..=n as i32 + 12)
            .map(|m| math::pow(1.0 + bridge_discount, -(f64::from(m) / 12.0)))
            .collect();
        let growth_years = n / 12 + 2;
        let growth_table = |growth: f64| {
            (0..growth_years as i32).map(move |years| math::powi(1.0 + growth, years))
        };
        let item_growth: Vec<f64> = items
            .iter()
            .flat_map(|item| growth_table(item.growth))
            .collect();
        let rent_growth: Vec<f64> = growth_table(Category::Rent.default_real_growth()).collect();
        let service_growth: Vec<f64> = growth_table(CHILD_SERVICE_REAL_GROWTH).collect();

        let hx = super::housing_ext::HxPlan::compile(profile, &props);
        let hd = super::housing_decisions::HdPlan::compile(profile, &props);

        Ok(Plan {
            profile: profile.clone(),
            start,
            horizon_months,
            people,
            birthday_months: children
                .iter()
                .fold(0, |mask, c| mask | 1 << c.birth.rem_euclid(12)),
            children,
            family: super::family::compile(profile, start),
            jobs,
            selfemp,
            rz: crate::career_realism::RealismPlan::compile(profile, data),
            streams,
            items,
            props,
            hx,
            debts,
            events,
            stresses,
            trigger,
            partner_offset: (profile.retirement.partner_offset_years * 12.0).round() as i32,
            latest_retirement_age,
            bridge_discount,
            bridge_pow,
            item_growth,
            rent_growth,
            service_growth,
            growth_years,
            coast_month,
            coast_discount,
            econ,
            law,
            ss,
            health,
            planner,
            accounts0,
            joiner_accounts,
            cash_terms: accounts::cash_terms(profile),
            fee_monthly: fee_annual / 12.0,
            target0,
            expected_real_return,
            shocks,
            sx,
            state0,
            in_us0,
            foreign_rate0,
            local0,
            home_exclusion,
            acquisition_debt_limit,
            retirement_rate,
            rebalance_band,
            custom_yield: policy
                .custom_asset
                .as_ref()
                .map_or(0.0, |c| c.income_yield.max(0.0)),
            rebalance_annual,
            plain_mix: !matches!(policy.rebalance, Rebalance::Never),
            eq12_months,
            income_rate0,
            hd,
            care,
            beh,
            dynamics: super::dynamics::DynamicsPlan::compile(profile, data, start, n),
        })
    }

    /// The compiled profile.
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// First simulated month.
    pub fn start(&self) -> YearMonth {
        self.start
    }

    /// Maximum number of months any path can run.
    pub fn horizon_months(&self) -> u32 {
        self.horizon_months
    }

    /// Number of calendar years touched by the horizon; the length of every per-year
    /// array in the results.
    pub fn horizon_years(&self) -> u32 {
        let last = self.start + (self.horizon_months as i32 - 1);
        (last.year() - self.start.year() + 1) as u32
    }

    /// The seed from `profile.simulation.seed`.
    pub fn seed(&self) -> u64 {
        self.profile.simulation.seed
    }

    /// The number of paths requested by `profile.simulation.paths`.
    pub fn paths(&self) -> u64 {
        u64::from(self.profile.simulation.paths)
    }

    /// An empty accumulator sized for this plan.
    pub fn new_accumulator(&self) -> Accumulator {
        Accumulator::new(self.horizon_years())
    }

    /// The run facts a [`Summary`] needs besides the accumulator.
    pub fn summary_context(&self) -> SummaryContext {
        SummaryContext {
            start: self.start,
            first_birth: self.people[0].birth,
            second_birth: self.people.get(1).map(|p| p.birth),
        }
    }

    /// The summary of an accumulator filled from this plan.
    ///
    /// The accumulator is taken to have been run with the profile's own seed
    /// ([`Plan::seed`]); use [`Plan::summarize_seeded`] for another.
    pub fn summarize(&self, acc: &Accumulator) -> Summary {
        self.summarize_seeded(acc, self.seed())
    }

    /// [`Plan::summarize`] for an accumulator run with `seed`. The picked paths
    /// (`Summary::picks`) are simulated again to give their annual series
    /// (`Summary::pick_paths`); a path that does not reproduce the accumulator's
    /// record (the seed is not the one the run used) is left out.
    pub fn summarize_seeded(&self, acc: &Accumulator, seed: u64) -> Summary {
        let mut summary = Summary::from_accumulator(acc, &self.summary_context());
        self.fill_pick_paths(&mut summary, acc, seed);
        summary
    }

    /// Fills `summary.pick_paths` for a summary of `acc` (run with `seed`) made
    /// without them: the part of [`Plan::summarize_seeded`] after
    /// `Summary::from_accumulator`, for a caller that already has the summary.
    pub fn fill_pick_paths(&self, summary: &mut Summary, acc: &Accumulator, seed: u64) {
        let mut workspace = super::Workspace::new(self);
        let mut recorder = crate::stats::SampleRecorder::default();
        for pick in &summary.picks {
            let index = pick.path_index;
            if summary.pick_paths.iter().any(|p| p.path_index == index) {
                continue;
            }
            super::run_path(self, seed, index, &mut workspace, &mut recorder);
            let terminal = acc
                .path_scalars(index)
                .map(|row| f64::from(row[crate::stats::PathScalar::TerminalNetWorth.index()]));
            let last = recorder.sample.net_worth.last().copied();
            let same = match (terminal, last) {
                (Some(t), Some(l)) => (t - l as f64).abs() <= 1.0 + 1e-6 * t.abs(),
                _ => false,
            };
            if same {
                summary.pick_paths.push(recorder.sample.clone());
            }
        }
    }

    /// Number of start months the `historical` market model can replay (0 for the
    /// other models). Path `i` replays cohort `i mod cohort_count`.
    pub fn cohort_count(&self) -> usize {
        self.econ.cohort_count()
    }

    /// Remaining life expectancy of each adult at the start, in years.
    pub fn life_expectancies(&self) -> Vec<f64> {
        self.people
            .iter()
            .map(|p| p.mortality.life_expectancy())
            .collect()
    }
}

/// Depth-first search for the first numeric value stored under `key`.
fn find_number(value: &serde_json::Value, key: &str) -> Option<f64> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(v) = map.get(key).and_then(serde_json::Value::as_f64) {
                return Some(v);
            }
            map.values().find_map(|v| find_number(v, key))
        }
        serde_json::Value::Array(items) => items.iter().find_map(|v| find_number(v, key)),
        _ => None,
    }
}
