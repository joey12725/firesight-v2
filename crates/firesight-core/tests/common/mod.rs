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
