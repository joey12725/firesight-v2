//! Money is conserved: every month, on every path, in every market model, the three
//! identities of `sim/record.rs` hold (cash sources equal uses; invested buckets
//! change by contributions, withdrawals and returns; property, mortgages and debts
//! change by the flows reported). `Workspace::set_check(true)` asserts them inside
//! the loop; these tests drive it over the demo households and over a household
//! built to exercise every event kind, income kind and policy at once.

mod common;

use std::collections::BTreeMap;

use common::*;
use firesight_core::model::*;
use firesight_core::sim::{self, Workspace};
use firesight_core::stats::{PathScalar, YearSeries};

/// Everything at once: two earners (one joining later), children, a home and a
/// rental, debts, every event kind, every stress overlay, a glide path with band
/// rebalancing and tax-efficient placement, guardrails and tax-aware withdrawals.
pub fn kitchen_sink() -> Profile {
    let mut a = person("a", 41, Sex::Female);
    a.social_security.claim_age = 68.0;
    let mut b = person("b", 39, Sex::Male);
    b.joins = Some(START + 20);
    b.social_security.basis = SocialSecurityBasis::KnownBenefit {
        monthly_at_fra: 1_900.0,
    };

    let mut job_a = job("job_a", 0, 150_000.0);
    if let IncomeKind::Job(j) = &mut job_a.kind {
        j.growth = WageGrowth::Lifecycle {
            profile: LifecycleProfile::Typical,
            extra: 0.0,
        };
        j.risk.level = RiskLevel::High;
        j.bonus = Some(Bonus {
            target_fraction: 0.1,
            volatility: 0.4,
            month: 3,
        });
        j.commission = Some(Commission {
            expected_annual: 6_000.0,
            volatility: 0.5,
        });
        j.equity = vec![
            EquityGrant {
                id: "rsu".into(),
                unvested_value: 80_000.0,
                vest_months_remaining: 36,
                refresh_annual_value: 20_000.0,
                sell_on_vest: false,
                ..EquityGrant::default()
            },
            EquityGrant {
                id: "options".into(),
                kind: EquityKind::Option {
                    strike_fraction: 0.6,
                },
                unvested_value: 30_000.0,
                vest_months_remaining: 24,
                cliff_months: 6,
                ..EquityGrant::default()
            },
        ];
        j.plan = Some(EmployerPlan {
            employee_max: true,
            roth_fraction: 0.3,
            match_rate: 1.0,
            match_up_to: 0.04,
            after_tax_mega: true,
            ..EmployerPlan::default()
        });
        j.hsa_contribution = 3_000.0;
        j.hsa_employer = 750.0;
    }
    let mut job_b = job("job_b", 1, 70_000.0);
    if let IncomeKind::Job(j) = &mut job_b.kind {
        j.risk.level = RiskLevel::Typical;
        j.equity = vec![EquityGrant {
            id: "startup".into(),
            company: EquityCompany::Private {
                liquidity_hazard: 0.2,
                fail_prob: 0.4,
                exit_volatility: 1.0,
            },
            unvested_value: 40_000.0,
            ..EquityGrant::default()
        }];
        j.plan = Some(EmployerPlan::default());
    }
    let side = Income {
        id: "side".into(),
        owner: 0,
        kind: IncomeKind::SelfEmployment(SelfEmployment {
            net_annual: 18_000.0,
            volatility: 0.4,
            real_growth: 0.01,
            ends_at_retirement: false,
            solo_401k_fraction: 0.1,
            ..SelfEmployment::default()
        }),
        end: Some(When::age(0, 72.0)),
        ..Income::default()
    };
    let pension = Income {
        id: "pension".into(),
        owner: 0,
        kind: IncomeKind::Pension(Pension {
            annual: 9_000.0,
            cola: Cola::Partial { rate: 0.015 },
            survivor_fraction: 0.5,
        }),
        start: Some(When::age(0, 62.0)),
        ..Income::default()
    };
    let annuity = Income {
        id: "annuity".into(),
        owner: 1,
        kind: IncomeKind::Annuity(Annuity {
            annual: 4_000.0,
            real: false,
        }),
        start: Some(When::retirement(3.0)),
        ..Income::default()
    };
    let support = Income {
        id: "support".into(),
        owner: 0,
        kind: IncomeKind::Other(OtherIncome {
            annual: 2_400.0,
            taxable: false,
            real_growth: 0.01,
            ends_at_retirement: true,
        }),
        ..Income::default()
    };

    let mut commute = item("commute", Category::Transport, 300.0, true);
    commute.in_retirement = RetirementChange::Stops;
    let mut travel = item("travel", Category::Travel, 6_000.0, false);
    travel.frequency = Frequency::Annual;
    travel.in_retirement = RetirementChange::Factor { factor: 1.5 };
    let mut hobby = item("hobby", Category::Entertainment, 150.0, false);
    hobby.owner = Some(1);
    hobby.start = Some(When::retirement(-1.0));
    let mut tuition = item("tuition", Category::Education, 500.0, true);
    tuition.end = Some(When::date(START + 60));

    let mut p = Profile {
        name: "kitchen sink".into(),
        start: Some(START),
        household: Household {
            people: vec![a, b],
            children: vec![
                Child {
                    id: "kid".into(),
                    birth: START - 50,
                    college: College::Private,
                    college_share: 0.5,
                    ..Child::default()
                },
                Child {
                    id: "planned".into(),
                    birth: START + 30,
                    costs: ChildCosts::Modeled,
                    childcare_monthly: Some(900.0),
                    college: College::PublicInState,
                    ..Child::default()
                },
            ],
            location: Location {
                state: Some("CA".into()),
                ..Location::default()
            },
        },
        incomes: vec![job_a, job_b, side, pension, annuity, support],
        spending: Spending {
            items: vec![
                item("rent", Category::Rent, 400.0, true),
                item("food", Category::Groceries, 900.0, true),
                item("fun", Category::Dining, 500.0, false),
                commute,
                travel,
                hobby,
                tuition,
            ],
            shocks: ShockLevel::High,
            ..Spending::default()
        },
        accounts: vec![
            account("checking", 0, AccountKind::Cash, 25_000.0),
            account("brokerage", 0, AccountKind::Taxable, 140_000.0),
            account("k401", 0, AccountKind::Traditional, 260_000.0),
            account("roth", 0, AccountKind::Roth, 60_000.0),
            account("hsa", 0, AccountKind::Hsa, 12_000.0),
            account("b_cash", 1, AccountKind::Cash, 8_000.0),
            account("b_ira", 1, AccountKind::Traditional, 45_000.0),
            account("b_roth", 1, AccountKind::Roth, 15_000.0),
        ],
        portfolio: PortfolioPolicy {
            target: Allocation::from_pairs(&[
                (AssetClass::UsStocks, 45.0),
                (AssetClass::IntlStocks, 15.0),
                (AssetClass::Bonds, 20.0),
                (AssetClass::RealEstate, 5.0),
                (AssetClass::Gold, 5.0),
                (AssetClass::Crypto, 4.0),
                (AssetClass::SingleStock, 4.0),
                (AssetClass::Cash, 2.0),
            ]),
            glide: GlidePath::TargetDate {
                years_before: 15.0,
                equity_at_retirement: 0.55,
                years_after: 7.0,
                equity_final: 0.4,
            },
            rebalance: Rebalance::Bands { width: 0.05 },
            location: AssetLocation::TaxEfficient,
            advisory_fee: 0.0025,
            retirement_cash_years: 1.5,
            ..PortfolioPolicy::default()
        },
        properties: vec![
            Property {
                id: "home".into(),
                value: 650_000.0,
                basis: Some(480_000.0),
                mortgage: Some(Mortgage {
                    balance: 380_000.0,
                    rate: 0.045,
                    remaining_months: 280,
                    payment: None,
                    ..Mortgage::default()
                }),
                hoa_monthly: 120.0,
                ..Property::default()
            },
            Property {
                id: "rental".into(),
                usage: PropertyUse::Rental(RentalTerms {
                    monthly_rent: 2_100.0,
                    vacancy: 0.08,
                    management_fee: 0.08,
                    rent_real_growth: 0.005,
                    land_fraction: 0.25,
                    years_in_service: 4.0,
                    dynamics: RentalDynamics::Simple,
                }),
                value: 300_000.0,
                basis: Some(240_000.0),
                mortgage: Some(Mortgage {
                    balance: 190_000.0,
                    rate: 0.06,
                    remaining_months: 300,
                    payment: None,
                    ..Mortgage::default()
                }),
                ..Property::default()
            },
        ],
        debts: vec![
            Debt {
                id: "student".into(),
                kind: DebtKind::Student,
                balance: 22_000.0,
                rate: 0.055,
                remaining_months: Some(90),
                ..Debt::default()
            },
            Debt {
                id: "card".into(),
                kind: DebtKind::CreditCard,
                balance: 6_000.0,
                rate: 0.22,
                ..Debt::default()
            },
        ],
        debt_policy: DebtPolicy {
            strategy: DebtStrategy::Avalanche,
            extra_monthly: 300.0,
            ..DebtPolicy::default()
        },
        retirement: RetirementPlan {
            trigger: RetirementTrigger::WhenFunded(WhenFunded {
                max_age: 66.0,
                ..WhenFunded::default()
            }),
            partner_offset_years: 2.0,
            spending_rule: SpendingRule::Guardrails {
                upper: 0.2,
                lower: 0.2,
                adjust: 0.1,
            },
            ..RetirementPlan::default()
        },
        ..Profile::default()
    };
    let event = |id: &str, months: i32, kind: EventKind| LifeEvent {
        id: id.into(),
        name: id.into(),
        when: When::date(START + months),
        kind,
        ..LifeEvent::default()
    };
    let mut factors = BTreeMap::new();
    factors.insert(Category::Rent, 0.7);
    factors.insert(Category::Groceries, 0.9);
    p.events = vec![
        event(
            "cabin",
            36,
            EventKind::BuyHome(BuyHome {
                property_id: "cabin".into(),
                price: 180_000.0,
                down_payment_fraction: 0.1,
                usage: PropertyUse::Vacation,
                replaces_rent: false,
                ..BuyHome::default()
            }),
        ),
        LifeEvent {
            probability: 0.5,
            timing_uncertainty_years: 2.0,
            ..event(
                "inheritance",
                60,
                EventKind::Windfall {
                    amount: 120_000.0,
                    taxable: false,
                },
            )
        },
        event(
            "bonus_payout",
            14,
            EventKind::Windfall {
                amount: 15_000.0,
                taxable: true,
            },
        ),
        event(
            "car",
            50,
            EventKind::Expense {
                amount: 38_000.0,
                financed: Some(Financing::default()),
            },
        ),
        event(
            "wedding",
            22,
            EventKind::Expense {
                amount: 25_000.0,
                financed: None,
            },
        ),
        event(
            "move",
            72,
            EventKind::Relocate(Relocate {
                state: Some("CO".into()),
                moving_cost: 9_000.0,
                cost_of_living: CostOfLivingChange::ByCategory { factors },
                ..Relocate::default()
            }),
        ),
        event(
            "promotion",
            40,
            EventKind::IncomeChange {
                income: "job_a".into(),
                change: IncomeChangeKind::Scale { factor: 1.15 },
            },
        ),
        event(
            "leave",
            32,
            EventKind::CareerBreak {
                person: 1,
                months: 5,
                income_fraction: 0.5,
            },
        ),
        event(
            "trim",
            90,
            EventKind::SpendingChange {
                target: SpendingTarget::Category {
                    category: Category::Dining,
                },
                change: SpendingChangeKind::Set { annual: 4_000.0 },
            },
        ),
        event(
            "save_more",
            12,
            EventKind::ContributionChange {
                income: "job_b".into(),
                employee_fraction: 0.12,
            },
        ),
        event(
            "sell_rental",
            130,
            EventKind::SellProperty(SellProperty {
                property: "rental".into(),
                ..SellProperty::default()
            }),
        ),
        LifeEvent {
            when: When::retirement(4.0),
            ..event(
                "downsize",
                0,
                EventKind::SellProperty(SellProperty {
                    property: "home".into(),
                    selling_cost_fraction: 0.06,
                    then_rent_monthly: Some(2_400.0),
                }),
            )
        },
        LifeEvent {
            when: When::age(0, 80.0),
            ..event(
                "abroad",
                0,
                EventKind::Relocate(Relocate {
                    country: "PT".into(),
                    foreign_tax_rate: Some(0.2),
                    moving_cost: 15_000.0,
                    cost_of_living: CostOfLivingChange::Factor { overall: 0.8 },
                    ..Relocate::default()
                }),
            )
        },
    ];
    p.assumptions.market.stress = vec![
        Stress::CrashAtRetirement {
            drawdown: 0.35,
            recovery_years: 4.0,
        },
        Stress::LostDecade {
            start: When::retirement(1.0),
        },
        Stress::Stagflation {
            start: When::date(START + 100),
            years: 6.0,
            inflation: 0.07,
        },
    ];
    p
}

fn paths() -> u64 {
    400
}

#[test]
fn demo_households_conserve_money_in_every_market_model() {
    for example in Profile::examples(START) {
        for kind in MARKETS {
            let plan = compile(&with_market(example.profile.clone(), kind));
            let acc = run_checked(&plan, 11, 0..paths());
            assert_eq!(acc.paths, paths(), "{} {kind:?}", example.slug);
        }
    }
}

#[test]
fn every_feature_at_once_conserves_money() {
    let profile = kitchen_sink();
    for kind in MARKETS {
        let plan = compile(&with_market(profile.clone(), kind));
        let acc = run_checked(&plan, 5, 0..paths());
        // The household actually lives through the things the profile sets up.
        let retired = acc
            .scalar_column(PathScalar::RetirementAge)
            .iter()
            .filter(|v| v.is_finite())
            .count();
        assert!(retired as u64 > paths() / 2, "{kind:?}: {retired} retired");
    }
}

/// Policy risk, tax-loss harvesting, qualified charitable distributions and full
/// state detail all on, on top of everything else.
pub fn with_realism_options(mut p: Profile) -> Profile {
    p.assumptions.taxes.policy_risk = Some(PolicyRisk {
        ss_haircut_probability: 0.7,
        tax_increase_probability: 0.7,
        ..PolicyRisk::default()
    });
    p.assumptions.taxes.chained_cpi = true;
    p.assumptions.taxes.state_detail = StateDetail::Full;
    p.portfolio.tax_loss_harvesting = true;
    p.retirement.qcd = true;
    p
}

#[test]
fn tax_and_policy_realism_options_conserve_money() {
    let profile = with_realism_options(kitchen_sink());
    for kind in MARKETS {
        let plan = compile(&with_market(profile.clone(), kind));
        let acc = run_checked(&plan, 5, 0..paths());
        assert_eq!(acc.paths, paths(), "{kind:?}");
    }
    for example in Profile::examples(START) {
        let plan = compile(&with_realism_options(example.profile));
        run_checked(&plan, 11, 0..paths());
    }
}

#[test]
fn care_features_conserve_money() {
    // The opt-in health, longevity and care features on the demo households and on
    // everything at once.
    for example in Profile::examples(START) {
        let plan = compile(&care::all_care(example.profile.clone()));
        let acc = run_checked(&plan, 13, 0..paths());
        assert_eq!(acc.paths, paths(), "{}", example.slug);
    }
    for kind in MARKETS {
        let plan = compile(&with_market(care::all_care(kitchen_sink()), kind));
        let acc = run_checked(&plan, 5, 0..paths());
        assert_eq!(acc.paths, paths(), "{kind:?}");
    }
    let plan = compile(&care::all_care(kitchen_sink()));
    let mut ws = Workspace::new(&plan);
    ws.set_check(true);
    for path in 0..20 {
        sim::trace_path_in(&plan, 5, path, &mut ws);
    }
}

#[test]
fn traces_conserve_money_and_show_the_events() {
    let plan = compile(&kitchen_sink());
    let mut ws = Workspace::new(&plan);
    ws.set_check(true);
    let mut seen = std::collections::BTreeSet::new();
    for path in 0..40 {
        let trace = sim::trace_path_in(&plan, 5, path, &mut ws);
        for e in &trace.events {
            seen.insert(e.kind.key());
        }
        // The annual cash-flow identity holds on the recorded (today's-dollar,
        // whole-dollar) columns too.
        let col = |s: YearSeries| -> Vec<f64> {
            trace
                .annual
                .columns
                .iter()
                .find(|c| c.key == s.key())
                .map_or(vec![0.0; trace.annual.years.len()], |c| c.values.clone())
        };
        let mut residual = vec![0.0; trace.annual.years.len()];
        for s in YearSeries::ALL {
            use firesight_core::stats::SeriesKind::*;
            let sign = match s.kind() {
                Income | Withdrawal => 1.0,
                Tax | Spending | Saving => -1.0,
                _ => 0.0,
            };
            let sign = match s {
                YearSeries::PropertySale => 1.0,
                YearSeries::PropertyPurchase => -1.0,
                YearSeries::SaveEmployer => 0.0,
                _ => sign,
            };
            for (r, v) in residual.iter_mut().zip(col(*s)) {
                *r += sign * v;
            }
        }
        for (year, r) in trace.annual.years.iter().zip(&residual) {
            assert!(r.abs() <= 25.0, "path {path} year {year}: residual {r}");
        }
    }
    for kind in [
        "retired",
        "bought_home",
        "sold_home",
        "relocated",
        "windfall",
        "major_expense",
        "expense_shock",
        "career_break",
        "partner_joined",
        "claimed_social_security",
        "died",
        "debt_paid_off",
        "laid_off",
        "life_event",
    ] {
        assert!(
            seen.contains(kind),
            "no {kind} event in 40 traced paths: {seen:?}"
        );
    }
}

#[test]
fn housing_decisions_conserve_money() {
    // Spending high enough that savings run out: lines are opened and drawn, homes
    // downsized and sold, loans refinanced, with every opt-in housing decision on.
    let profiles = [
        with_housing_decisions(overspending(kitchen_sink(), 2.5)),
        with_housing_decisions(kitchen_sink()),
        with_housing_decisions_hecm(overspending(kitchen_sink(), 2.5)),
    ];
    for profile in profiles {
        for kind in MARKETS {
            let plan = compile(&with_market(profile.clone(), kind));
            let acc = run_checked(&plan, 9, 0..paths());
            assert_eq!(acc.paths, paths());
        }
    }
    for example in Profile::examples(START) {
        let plan = compile(&with_housing_decisions(overspending(
            example.profile.clone(),
            1.6,
        )));
        run_checked(&plan, 9, 0..paths());
        let plan = compile(&with_housing_decisions_hecm(overspending(
            example.profile,
            1.6,
        )));
        run_checked(&plan, 9, 0..paths());
    }
}

#[path = "realism_profiles/mod.rs"]
mod realism_profiles;

#[test]
fn optional_market_refinements_conserve_money() {
    let mut bases: Vec<Profile> = Profile::examples(START)
        .into_iter()
        .map(|e| e.profile)
        .collect();
    bases.push(kitchen_sink());
    for base in &bases {
        for (label, profile) in realism_profiles::variants(base) {
            let plan = compile(&profile);
            let acc = run_checked(&plan, 13, 0..200);
            assert_eq!(acc.paths, 200, "{} {label}", base.name);
        }
    }
}
