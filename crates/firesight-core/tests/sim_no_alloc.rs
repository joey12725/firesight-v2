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
    profiles.push(family_features());
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
