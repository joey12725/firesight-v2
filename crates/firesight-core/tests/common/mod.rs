//! Shared builders for the simulation integration tests.
#![allow(dead_code)]

pub mod care;

use firesight_core::data::Data;
use firesight_core::model::*;
use firesight_core::sim::{self, Plan, Workspace};
use firesight_core::stats::{Accumulator, Summary};

pub const START: YearMonth = YearMonth::ym(2026, 11);

pub fn compile(profile: &Profile) -> Plan {
    Plan::compile(profile, Data::embedded()).unwrap_or_else(|r| panic!("{:#?}", r.errors))
}

/// Runs `paths` with the conservation checks on.
pub fn run_checked(plan: &Plan, seed: u64, paths: std::ops::Range<u64>) -> Accumulator {
    let mut ws = Workspace::new(plan);
    ws.set_check(true);
    let mut acc = plan.new_accumulator();
    for i in paths {
        sim::run_path(plan, seed, i, &mut ws, &mut acc);
    }
    acc
}

pub fn run(plan: &Plan, seed: u64, paths: std::ops::Range<u64>) -> Accumulator {
    let mut acc = plan.new_accumulator();
    sim::run_range(plan, seed, paths, &mut acc);
    acc
}

pub fn summary(profile: &Profile, seed: u64, paths: u64) -> Summary {
    let plan = compile(profile);
    plan.summarize(&run(&plan, seed, 0..paths))
}

pub fn item(id: &str, category: Category, monthly: f64, essential: bool) -> BudgetItem {
    BudgetItem {
        id: id.into(),
        name: id.into(),
        category,
        amount: monthly,
        essential,
        ..BudgetItem::default()
    }
}

pub fn account(id: &str, owner: usize, kind: AccountKind, balance: f64) -> Account {
    Account {
        id: id.into(),
        name: id.into(),
        owner,
        kind,
        balance,
        ..Account::default()
    }
}

pub fn person(id: &str, age_years: i32, sex: Sex) -> Person {
    Person {
        id: id.into(),
        name: id.into(),
        birth: START - age_years * 12,
        sex,
        ..Person::default()
    }
}

/// A household with every source of randomness except markets switched off: no
/// tax, no modelled healthcare, no shocks, no career or health risk, fixed lifespan.
pub fn quiet(people: Vec<Person>, die_at: f64) -> Profile {
    Profile {
        name: "quiet".into(),
        start: Some(START),
        household: Household {
            people,
            ..Household::default()
        },
        spending: Spending {
            lifestyle_creep: 0.0,
            retirement_smile: false,
            shocks: ShockLevel::Off,
            healthcare: Healthcare {
                mode: HealthcareMode::InBudget,
                long_term_care: false,
                ..Healthcare::default()
            },
            ..Spending::default()
        },
        portfolio: PortfolioPolicy {
            expense_ratio: 0.0,
            emergency_months: 0.0,
            ..PortfolioPolicy::default()
        },
        retirement: RetirementPlan {
            withdrawal_order: WithdrawalOrder::Conventional,
            ..RetirementPlan::default()
        },
        assumptions: Assumptions {
            // The record as it was: these tests check the engine against history.
            market: MarketModel {
                equity_premium_adjustment: 0.0,
                ..MarketModel::default()
            },
            taxes: TaxModel {
                kind: TaxKind::None,
                ..TaxModel::default()
            },
            longevity: Longevity::FixedAge { age: die_at },
            career_risk: false,
            health_risk: false,
            ..Assumptions::default()
        },
        ..Profile::default()
    }
}

/// The classic safe-withdrawal-rate retiree: `portfolio` in a taxable account at
/// `stocks`/bonds, spending `annual` real for `years`, nothing else.
pub fn retiree(portfolio: f64, stocks: f64, annual: f64, years: f64) -> Profile {
    let mut who = person("retiree", 65, Sex::Unspecified);
    who.social_security.basis = SocialSecurityBasis::None;
    let mut p = quiet(vec![who], 65.0 + years);
    p.spending.items = vec![item("living", Category::Other, annual / 12.0, true)];
    p.accounts = vec![Account {
        cost_basis: Some(portfolio),
        ..account("portfolio", 0, AccountKind::Taxable, portfolio)
    }];
    p.portfolio.target = Allocation::from_pairs(&[
        (AssetClass::UsStocks, stocks),
        (AssetClass::Bonds, 1.0 - stocks),
    ]);
    p.retirement.trigger = RetirementTrigger::AlreadyRetired;
    p.retirement.spending_rule = SpendingRule::ConstantReal;
    p
}

pub fn job(id: &str, owner: usize, salary: f64) -> Income {
    Income {
        id: id.into(),
        name: id.into(),
        owner,
        kind: IncomeKind::Job(Job {
            salary,
            growth: WageGrowth::Flat { real_raise: 0.0 },
            risk: CareerRisk {
                level: RiskLevel::None,
                ..CareerRisk::default()
            },
            ..Job::default()
        }),
        ..Income::default()
    }
}

pub fn with_market(mut p: Profile, kind: MarketKind) -> Profile {
    p.assumptions.market.kind = kind;
    p
}

pub const MARKETS: [MarketKind; 4] = [
    MarketKind::Bootstrap {
        mean_block_months: 60,
        from_year: None,
        joins: BootstrapJoins::Uniform,
    },
    MarketKind::Regime,
    MarketKind::Lognormal,
    MarketKind::Historical,
];

/// Switches every opt-in career-realism feature on wherever it applies (jobs keep
/// their own layoff level; `career_risk` is left as the profile has it).
pub fn career_realism_on(mut p: Profile) -> Profile {
    for income in &mut p.incomes {
        match &mut income.kind {
            IncomeKind::Job(j) => {
                j.risk.job_mobility = JobMobility::High;
                j.risk.recession_pay = RecessionPay::Typical;
                j.risk.late_career = LateCareer::Realistic;
            }
            IncomeKind::SelfEmployment(se) => {
                se.business_risk = BusinessRisk::High;
                se.fallback_wage = Some(FallbackWage::default());
            }
            _ => {}
        }
    }
    for person in &mut p.household.people {
        person.disability_risk = DisabilityRisk::Typical;
        person.ltd_insurance = Some(LtdInsurance::default());
    }
    p
}

/// Switches on every opt-in housing decision (home-equity borrowing, downsizing,
/// cash-out refinancing and prepayment, realistic rentals), for the conservation
/// and finite-value tests.
pub fn with_housing_decisions(mut p: Profile) -> Profile {
    p.retirement.home_equity_borrowing = HomeEquityBorrowing::Heloc(Heloc::default());
    p.retirement.downsize = Some(Downsize {
        when: DownsizeWhen::WhenFundedRatioBelow { ratio: 0.9 },
        ..Downsize::default()
    });
    p.debt_policy.cash_out_refinance = Some(CashOutRefinance::default());
    p.debt_policy.mortgage_prepayment = MortgagePrepayment::WhenRateAbove { real_rate: None };
    realistic_rentals(p)
}

/// Every rental (owned or bought later) with realistic dynamics.
pub fn realistic_rentals(mut p: Profile) -> Profile {
    let realistic = |u: &mut PropertyUse| {
        if let PropertyUse::Rental(t) = u {
            t.dynamics = RentalDynamics::Realistic {
                annual_turnover: 0.25,
            };
        }
    };
    for prop in &mut p.properties {
        realistic(&mut prop.usage);
    }
    for e in &mut p.events {
        if let EventKind::BuyHome(b) = &mut e.kind {
            realistic(&mut b.usage);
        }
    }
    p
}

/// The same with a reverse mortgage (a standby line opened when savings fall below
/// five years of spending) in place of the home-equity line, and a move to a rental
/// after the first death in a couple.
pub fn with_housing_decisions_hecm(p: Profile) -> Profile {
    let mut p = with_housing_decisions(p);
    p.retirement.home_equity_borrowing = HomeEquityBorrowing::ReverseMortgage(ReverseMortgage {
        open_when: ReverseMortgageOpen::SavingsBelow { years: 5.0 },
        ..ReverseMortgage::default()
    });
    // Widowed couples move to a rental; single people stay.
    p.retirement.downsize = Some(Downsize {
        when: DownsizeWhen::WhenWidowed,
        rent_instead: true,
        ..Downsize::default()
    });
    p
}

/// Every budget line `factor` times larger, so that savings run out on many paths.
pub fn overspending(mut p: Profile, factor: f64) -> Profile {
    for item in &mut p.spending.items {
        item.amount *= factor;
    }
    p
}

/// The dual-income demo with every household-dynamics feature switched on
/// (divorce risk and a divorce event, parents with estates and care, spousal
/// caregiving, life insurance), for the conservation and finite-value tests.
pub fn household_dynamics() -> Profile {
    let mut p = Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == "dual-income-family")
        .unwrap()
        .profile;
    p.household.married = Some(START - 48);
    p.household.divorce_risk = DivorceRisk::Typical(DivorceTerms {
        keeps_home: false,
        support: Support {
            annual: 9_000.0,
            years: 4.0,
            direction: SupportDirection::Receives,
        },
        ..DivorceTerms::default()
    });
    p.events.push(LifeEvent {
        id: "divorce".into(),
        name: "Divorce".into(),
        when: When::date(START + 200),
        probability: 0.3,
        kind: EventKind::Divorce(DivorceTerms {
            support: Support {
                annual: 15_000.0,
                years: 6.0,
                direction: SupportDirection::Pays,
            },
            ..DivorceTerms::default()
        }),
        ..LifeEvent::default()
    });
    p.spending.healthcare.spousal_care = true;
    p.household.parents = vec![
        Parent {
            id: "mom".into(),
            child: 0,
            birth: START - 74 * 12,
            sex: Sex::Female,
            estate: 450_000.0,
            share: 0.5,
            inherited_ira_fraction: 0.5,
            ..Parent::default()
        },
        Parent {
            id: "dad_in_law".into(),
            child: 1,
            birth: START - 79 * 12,
            sex: Sex::Male,
            estate: 300_000.0,
            share: 1.0,
            inherited_ira_fraction: 0.3,
            care_support: Some(CareSupport {
                probability: 0.6,
                annual_cost: 7_242.0,
                hours: 0.5,
                caregiver: None,
            }),
            ..Parent::default()
        },
    ];
    p.household.people[0].life_insurance = vec![LifePolicy {
        id: "term".into(),
        name: "Term life".into(),
        kind: PolicyKind::Term {
            years_remaining: 15.0,
        },
        death_benefit: 1_000_000.0,
        annual_premium: 1_100.0,
    }];
    p.household.people[1].life_insurance = vec![LifePolicy {
        id: "whole".into(),
        name: "Whole life".into(),
        kind: PolicyKind::Permanent,
        death_benefit: 250_000.0,
        annual_premium: 3_000.0,
    }];
    p
}

// ---------------------------------------------------------------------------
// Private-company equity
// ---------------------------------------------------------------------------

/// The startup employee, the fourth demo household: a single engineer in San
/// Francisco with a Series C salary, about $110,000 of cash and taxable savings,
/// and incentive options, non-qualified options, RSUs and exercised shares in her
/// employer.
pub fn startup_employee() -> Profile {
    let example = Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == "startup-employee")
        .expect("the startup-employee demo");
    example.profile
}

/// An outcome table with one row that happens for certain: `kind` between
/// `years_from` and `years_to` from the start at exactly `multiple` times the last
/// round (the multiple is ignored for a failure and for "still private").
pub fn certain(
    kind: OutcomeKind,
    years_from: f64,
    years_to: f64,
    multiple: f64,
) -> Vec<OutcomeRow> {
    vec![OutcomeRow {
        id: kind.key().into(),
        kind,
        probability: 1.0,
        years_from,
        years_to,
        multiple_low: multiple,
        multiple_high: multiple,
    }]
}

/// The startup employee with the company's outcome forced, and with career risk,
/// health risk, spending shocks and tender offers off unless the caller turns them
/// back on: what is left to vary is the market.
pub fn startup_with(outcomes: Vec<OutcomeRow>) -> Profile {
    let mut p = startup_employee();
    p.assumptions.career_risk = false;
    p.assumptions.health_risk = false;
    p.assumptions.longevity = Longevity::FixedAge { age: 90.0 };
    p.spending.shocks = ShockLevel::Off;
    let co = &mut p.private_equity[0];
    co.outcomes = outcomes;
    co.tenders.annual_probability = Some(0.0);
    p
}

/// The sum of a monthly trace column over months `range` (absent means zero).
pub fn column_sum(
    trace: &firesight_core::stats::PathTrace,
    key: &str,
    range: std::ops::Range<usize>,
) -> f64 {
    trace.column(key).map_or(0.0, |v| {
        v.iter()
            .enumerate()
            .filter(|(i, _)| range.contains(i))
            .map(|(_, x)| *x)
            .sum()
    })
}

/// A monthly trace column, zeros when the series never moved.
pub fn column(trace: &firesight_core::stats::PathTrace, key: &str) -> Vec<f64> {
    trace
        .column(key)
        .map_or_else(|| vec![0.0; trace.months as usize], <[f64]>::to_vec)
}

/// Months (since the start) of the traced events of one kind.
pub fn event_months(trace: &firesight_core::stats::PathTrace, kind: &str) -> Vec<i32> {
    trace
        .events
        .iter()
        .filter(|e| e.kind.key() == kind)
        .map(|e| e.month - trace.start)
        .collect()
}

/// What one path's event log and equity ledger hold: a recorder much cheaper than a
/// full trace, for tests that look at many paths.
#[derive(Clone, Debug, Default)]
pub struct EquityLog {
    /// `(month since the start, kind key, label)` of every event.
    pub events: Vec<(i32, &'static str, String)>,
    /// Months simulated.
    pub months: u32,
    /// Lifetime `income_equity`, `equity_exercise`, today's dollars.
    pub income: f64,
    pub exercise: f64,
    /// Months in which `equity_paper_value` was positive.
    pub paper_months: u32,
    /// The path's scalars.
    pub scalars: Vec<f32>,
}

impl EquityLog {
    /// Months of the events of one kind.
    pub fn months_of(&self, kind: &str) -> Vec<i32> {
        self.events
            .iter()
            .filter(|e| e.1 == kind)
            .map(|e| e.0)
            .collect()
    }

    /// The first failure, sale or listing: `(month, kind key, label)`.
    pub fn terminal(&self) -> Option<&(i32, &'static str, String)> {
        self.events
            .iter()
            .find(|e| matches!(e.1, "company_failed" | "company_acquired" | "company_ipo"))
    }
}

impl firesight_core::stats::Recorder for EquityLog {
    fn begin_path(&mut self, _: u64) {
        *self = EquityLog::default();
    }
    fn record_year(&mut self, _: usize, _: &firesight_core::stats::YearRecord) {}
    fn end_path(&mut self, end: &firesight_core::stats::PathEnd) {
        self.scalars = end.scalars.to_vec();
    }
    fn wants_detail(&self) -> bool {
        true
    }
    fn record_month(&mut self, m: &firesight_core::stats::MonthRecord) {
        use firesight_core::stats::YearSeries;
        self.months += 1;
        self.income += m.values[YearSeries::IncomeEquity.index()];
        self.exercise += m.values[YearSeries::EquityExercise.index()];
        if m.values[YearSeries::EquityPaperValue.index()] > 0.0 {
            self.paper_months += 1;
        }
    }
    fn record_event(&mut self, e: firesight_core::stats::TraceEvent) {
        self.events.push((e.month - START, e.kind.key(), e.label));
    }
}

/// The event log of one path.
pub fn equity_log(plan: &Plan, seed: u64, path: u64, ws: &mut Workspace) -> EquityLog {
    let mut log = EquityLog::default();
    sim::run_path(plan, seed, path, ws, &mut log);
    log
}
