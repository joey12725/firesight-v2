//! Simulating a path does not touch the heap once the workspace is warm.
//!
//! This lives in its own test binary because it replaces the global allocator with
//! one that counts allocations made by the current thread.

mod behavior_profiles;
mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use firesight_core::model::Profile;
use firesight_core::sim::{self, Workspace};
use firesight_core::stats::{PathEnd, Recorder, YearRecord};

thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

struct Counting;

// SAFETY: every call is forwarded unchanged to the system allocator; the only
// addition is a thread-local counter with no destructor.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|c| c.set(c.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System.alloc` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.with(|c| c.set(c.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocations() -> u64 {
    ALLOCATIONS.with(Cell::get)
}

/// A recorder that keeps a checksum and nothing else. (The `Accumulator` grows its
/// per-path scalar table, which is the aggregate's memory, not the loop's.)
#[derive(Default)]
struct Sink {
    years: u64,
    total: f64,
}

impl Recorder for Sink {
    fn begin_path(&mut self, _: u64) {}
    fn record_year(&mut self, _: usize, record: &YearRecord) {
        self.years += 1;
        self.total += record.net_worth();
    }
    fn end_path(&mut self, end: &PathEnd) {
        self.total += f64::from(end.scalars[0].max(0.0));
    }
}

#[test]
fn a_warm_workspace_simulates_paths_without_allocating() {
    let mut profiles: Vec<Profile> = Profile::examples(common::START)
        .into_iter()
        .map(|e| e.profile)
        .collect();
    profiles.push(kitchen_sink_lite());
    profiles.push(spending_realism());
    profiles.push(family_features());
    // The opt-in career-realism features, all on, on every demo household.
    let with_realism: Vec<Profile> = profiles
        .iter()
        .cloned()
        .map(common::career_realism_on)
        .collect();
    profiles.extend(with_realism);
    // The tax and policy realism options on, too.
    let mut realism = kitchen_sink_lite();
    realism.assumptions.taxes.policy_risk = Some(firesight_core::model::PolicyRisk {
        ss_haircut_probability: 0.7,
        tax_increase_probability: 0.7,
        ..Default::default()
    });
    realism.portfolio.tax_loss_harvesting = true;
    realism.retirement.qcd = true;
    // And with the spending-realism features under them (QCDs read the month's
    // seasonal and price-wedge factors of giving).
    let mut both = spending_realism();
    both.assumptions.taxes.policy_risk = realism.assumptions.taxes.policy_risk;
    both.portfolio.tax_loss_harvesting = true;
    both.retirement.qcd = true;
    profiles.push(realism);
    profiles.push(both);
    // Every opt-in housing decision on, with savings running out so that they act.
    profiles.push(common::with_housing_decisions(common::overspending(
        kitchen_sink_lite(),
        2.0,
    )));
    profiles.push(common::with_housing_decisions_hecm(common::overspending(
        kitchen_sink_lite(),
        2.0,
    )));
    // The opt-in health, longevity and care features on every demo household.
    let with_care: Vec<Profile> = profiles
        .iter()
        .take(3)
        .map(|p| {
            let mut p = common::care::all_care(p.clone());
            p.name = format!("{} (care features)", p.name);
            p
        })
        .collect();
    profiles.extend(with_care);
    profiles.extend(behavior_profiles::behavior_profiles());
    profiles.push(common::household_dynamics());
    for profile in profiles {
        for kind in common::MARKETS {
            let plan = common::compile(&common::with_market(profile.clone(), kind));
            let mut ws = Workspace::new(&plan);
            let mut sink = Sink::default();
            // Warm up: lazily built tables, and buffers at their full size.
            for i in 0..20 {
                sim::run_path(&plan, 3, i, &mut ws, &mut sink);
            }
            let before = allocations();
            for i in 20..320 {
                sim::run_path(&plan, 3, i, &mut ws, &mut sink);
            }
            let made = allocations() - before;
            assert_eq!(
                made, 0,
                "{} ({kind:?}): {made} allocations over 300 paths",
                profile.name
            );
            assert!(sink.years > 300 && sink.total.is_finite());
        }
    }
}

/// The example household with every opt-in spending realism feature on.
fn spending_realism() -> Profile {
    use firesight_core::model::*;
    let mut p = Profile::example();
    p.name = "spending realism".into();
    p.assumptions.career_risk = true;
    p.spending.vehicles = vec![Vehicle {
        id: "car".into(),
        name: "Car".into(),
        price: 35_000.0,
        replace_every_years: 8.0,
        age_years: 5.0,
        financing: Some(VehicleFinancing::default()),
        ..Vehicle::default()
    }];
    p.spending.seasonality = Seasonality::Typical;
    p.spending.category_inflation = CategoryInflation::Stochastic;
    p.spending.shocks = ShockLevel::Detailed;
    p.spending.unemployment_cut = 0.15;
    p
}

/// The two-earner family with every opt-in family feature on: children on the health
/// plan, an uncertain second birth, stochastic college, adult support and leave.
fn family_features() -> Profile {
    use firesight_core::model::*;
    let mut p = Profile::examples(common::START)
        .into_iter()
        .find(|e| e.slug == "dual-income-family")
        .expect("the dual-income example")
        .profile;
    p.name = "family features".into();
    p.spending.healthcare.cover_children = true;
    for (i, c) in p.household.children.iter_mut().enumerate() {
        c.college_cost = CollegeCost::Stochastic {
            attend_probability: 0.95,
            real_tuition_growth: 0.01,
            tuition_volatility: 0.012,
            aid_scale: 1.0,
        };
        c.adult_support = Some(AdultSupport::default());
        if i == 1 {
            c.arrival = Arrival::Uncertain {
                probability: 0.85,
                delay_mean_months: 4.0,
                twins_probability: 0.2,
            };
            c.parental_leave = Some(ParentalLeave::default());
            c.second_parental_leave = Some(ParentalLeave {
                person: 1,
                months: 2,
                pay_fraction: 0.5,
            });
        }
    }
    p
}

/// The opt-in market options (long-run uncertainty, rare disasters, matched bootstrap
/// joins) switched on: their per-path overlays rewrite the economy in place.
#[test]
fn rich_market_options_do_not_allocate() {
    use firesight_core::model::*;
    let mut profile = kitchen_sink_lite();
    profile.name = "rich markets".into();
    let m = &mut profile.assumptions.market;
    m.equity_premium_uncertainty = Some(Uncertain::Normal {
        mean: 0.0,
        sd: 0.015,
    });
    m.inflation_uncertainty = Some(Uncertain::Pert {
        low: -0.005,
        mode: 0.0,
        high: 0.015,
    });
    m.disasters = Disasters {
        annual_probability: 0.05,
        inflation_probability: 0.5,
        ..Disasters::default()
    };
    let mut kinds = common::MARKETS.to_vec();
    kinds.push(MarketKind::Bootstrap {
        mean_block_months: 24,
        from_year: None,
        joins: BootstrapJoins::Matched,
    });
    for kind in kinds {
        let plan = common::compile(&common::with_market(profile.clone(), kind));
        let mut ws = Workspace::new(&plan);
        let mut sink = Sink::default();
        for i in 0..20 {
            sim::run_path(&plan, 3, i, &mut ws, &mut sink);
        }
        let before = allocations();
        for i in 20..320 {
            sim::run_path(&plan, 3, i, &mut ws, &mut sink);
        }
        let made = allocations() - before;
        assert_eq!(made, 0, "{kind:?}: {made} allocations over 300 paths");
        assert!(sink.years > 300 && sink.total.is_finite());
    }
}

/// A household with events, stresses, a rental and a partner who joins: the paths
/// through the loop the demo households do not take.
fn kitchen_sink_lite() -> Profile {
    use firesight_core::model::*;
    let mut p = Profile::example();
    p.name = "events".into();
    p.assumptions.market.stress = vec![
        Stress::CrashAtRetirement {
            drawdown: 0.4,
            recovery_years: 5.0,
        },
        Stress::Stagflation {
            start: When::retirement(2.0),
            years: 5.0,
            inflation: 0.08,
        },
    ];
    p.events.push(LifeEvent {
        id: "move".into(),
        name: "Move".into(),
        when: When::retirement(1.0),
        kind: EventKind::Relocate(Relocate {
            state: Some("FL".into()),
            city: Some("Miami".into()),
            moving_cost: 8_000.0,
            cost_of_living: CostOfLivingChange::Factor { overall: 0.9 },
            ..Relocate::default()
        }),
        ..LifeEvent::default()
    });
    p.events.push(LifeEvent {
        id: "car".into(),
        name: "Car".into(),
        when: When::date(common::START + 30),
        timing_uncertainty_years: 1.0,
        kind: EventKind::Expense {
            amount: 30_000.0,
            financed: Some(Financing::default()),
        },
        ..LifeEvent::default()
    });
    p
}

#[path = "realism_profiles/mod.rs"]
mod realism_profiles;

/// The optional market refinements allocate nothing per path either, retirement-dated
/// stresses included (they rewrite the path inside the month loop).
#[test]
fn the_market_refinements_simulate_paths_without_allocating() {
    for example in Profile::examples(common::START) {
        for (label, profile) in realism_profiles::variants(&example.profile) {
            let plan = common::compile(&profile);
            let mut ws = Workspace::new(&plan);
            let mut sink = Sink::default();
            for i in 0..20 {
                sim::run_path(&plan, 3, i, &mut ws, &mut sink);
            }
            let before = allocations();
            for i in 20..220 {
                sim::run_path(&plan, 3, i, &mut ws, &mut sink);
            }
            let made = allocations() - before;
            assert_eq!(
                made, 0,
                "{} ({label}): {made} allocations over 200 paths",
                example.slug
            );
        }
    }
}

/// The opt-in sleeve shapes (fat tails, crypto cycles, loss events on the custom
/// asset) switched on, with a portfolio that holds all three sleeves: the cycle and
/// loss draws happen inside the economy generator.
#[test]
fn rich_asset_options_do_not_allocate() {
    use firesight_core::model::*;
    let mut profile = kitchen_sink_lite();
    profile.name = "rich assets".into();
    let pf = &mut profile.portfolio;
    pf.target = Allocation::from_pairs(&[
        (AssetClass::UsStocks, 40.0),
        (AssetClass::Bonds, 15.0),
        (AssetClass::Crypto, 15.0),
        (AssetClass::SingleStock, 15.0),
        (AssetClass::Custom, 15.0),
    ]);
    pf.single_stock.tails = TailShape::Fat;
    pf.single_stock.ruin_hazard = Some(0.05);
    pf.crypto = CryptoAssumptions {
        outlook: CryptoOutlook::Believer,
        token: CryptoToken::OtherTokens,
        dynamics: CryptoDynamics::cycles(),
    };
    pf.custom_asset = Some(CustomAsset {
        tails: TailShape::Fat,
        loss_event: Some(LossEvent {
            annual_probability: 0.1,
            ..LossEvent::default()
        }),
        ..CustomAsset::default()
    });
    for kind in common::MARKETS {
        let plan = common::compile(&common::with_market(profile.clone(), kind));
        let mut ws = Workspace::new(&plan);
        let mut sink = Sink::default();
        for i in 0..20 {
            sim::run_path(&plan, 3, i, &mut ws, &mut sink);
        }
        let before = allocations();
        for i in 20..320 {
            sim::run_path(&plan, 3, i, &mut ws, &mut sink);
        }
        let made = allocations() - before;
        assert_eq!(made, 0, "{kind:?}: {made} allocations over 300 paths");
        assert!(sink.years > 300 && sink.total.is_finite());
    }
}

/// The opt-in career shapes (fat-tailed wage shocks, the two-part pay cut and the
/// monthly job search) switched on for every job at the riskiest level: their
/// tables are built when the plan is compiled and only read on the path.
#[test]
fn rich_career_options_do_not_allocate() {
    use firesight_core::model::*;
    let mut profile = kitchen_sink_lite();
    profile.name = "rich careers".into();
    let mut jobs = 0;
    for income in &mut profile.incomes {
        if let IncomeKind::Job(job) = &mut income.kind {
            job.risk = CareerRisk {
                level: RiskLevel::High,
                wage_shocks: WageShockShape::FatTailed,
                layoff_pay_cut: PayCutShape::TwoPart,
                job_search: JobSearchModel::Monthly,
                ..CareerRisk::default()
            };
            jobs += 1;
        }
    }
    assert!(jobs > 0);
    for kind in common::MARKETS {
        let plan = common::compile(&common::with_market(profile.clone(), kind));
        let mut ws = Workspace::new(&plan);
        let mut sink = Sink::default();
        for i in 0..20 {
            sim::run_path(&plan, 3, i, &mut ws, &mut sink);
        }
        let before = allocations();
        for i in 20..320 {
            sim::run_path(&plan, 3, i, &mut ws, &mut sink);
        }
        let made = allocations() - before;
        assert_eq!(made, 0, "{kind:?}: {made} allocations over 300 paths");
        assert!(sink.years > 300 && sink.total.is_finite());
    }
}

/// The opt-in business and employer-stock options switched on: a business that
/// can close and pays in lumps (its own stream, two words a month), a listed
/// employer with fat tails, a ruin hazard and the layoff link, and a private one
/// with a lognormal wait and a Pareto tail. Their tables are built when the plan
/// is compiled and only read on the path.
#[test]
fn rich_business_options_do_not_allocate() {
    use firesight_core::model::*;
    let mut profile = kitchen_sink_lite();
    profile.name = "rich business".into();
    let job = |id: &str, company: EquityCompany| Income {
        id: id.into(),
        owner: 0,
        kind: IncomeKind::Job(Job {
            salary: 60_000.0,
            risk: CareerRisk {
                level: RiskLevel::High,
                ..CareerRisk::default()
            },
            equity: vec![EquityGrant {
                id: "grant".into(),
                company,
                unvested_value: 40_000.0,
                refresh_annual_value: 10_000.0,
                sell_on_vest: false,
                ..EquityGrant::default()
            }],
            ..Job::default()
        }),
        ..Income::default()
    };
    profile.incomes.push(Income {
        id: "shop".into(),
        owner: 0,
        kind: IncomeKind::SelfEmployment(SelfEmployment {
            net_annual: 30_000.0,
            failure: BusinessFailure::Hazard {
                young_annual_probability: 0.3,
                mature_annual_probability: 0.1,
                young_years: 5.0,
                years_in_business: Some(0.0),
                recession_multiplier: 3.0,
                then: AfterFailure::Restart {
                    gap_months: 6,
                    income_fraction: 0.8,
                },
            },
            monthly_lumpiness: 1.0,
            ..SelfEmployment::default()
        }),
        ..Income::default()
    });
    profile.incomes.push(job(
        "listed",
        EquityCompany::Public {
            volatility: 0.7,
            beta: 1.3,
            tails: TailShape::Fat,
            ruin: StockRuin::Stated {
                annual_probability: 0.1,
            },
            layoff_link: true,
        },
    ));
    profile.incomes.push(job(
        "startup",
        EquityCompany::Private {
            liquidity_hazard: 0.12,
            fail_prob: 0.4,
            exit_volatility: 1.0,
            exit_timing: ExitTiming::lognormal(),
            exit_size: ExitSize::ParetoTail,
        },
    ));
    for kind in common::MARKETS {
        let plan = common::compile(&common::with_market(profile.clone(), kind));
        let mut ws = Workspace::new(&plan);
        let mut sink = Sink::default();
        for i in 0..20 {
            sim::run_path(&plan, 3, i, &mut ws, &mut sink);
        }
        let before = allocations();
        for i in 20..320 {
            sim::run_path(&plan, 3, i, &mut ws, &mut sink);
        }
        let made = allocations() - before;
        assert_eq!(made, 0, "{kind:?}: {made} allocations over 300 paths");
        assert!(sink.years > 300 && sink.total.is_finite());
    }
}
