//! Simulating a path does not touch the heap once the workspace is warm.
//!
//! This lives in its own test binary because it replaces the global allocator with
//! one that counts allocations made by the current thread.

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
