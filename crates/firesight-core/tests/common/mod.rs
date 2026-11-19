//! Shared builders for the simulation integration tests.
#![allow(dead_code)]

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
    },
    MarketKind::Regime,
    MarketKind::Lognormal,
    MarketKind::Historical,
];
