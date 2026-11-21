//! The opt-in housing realism options do not touch the heap in the month loop
//! either (the allocator harness is the one of `sim_no_alloc.rs`; it has to live in
//! its own test binary because it replaces the global allocator).

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use firesight_core::model::*;
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

/// Every option on, wherever the profile has something for it to act on.
fn all_options_on(mut p: Profile) -> Profile {
    let cap = Assessment::Capped {
        annual_cap: 0.02,
        assessed_value: None,
    };
    let arm = MortgageKind::Arm {
        fixed_months: 24,
        adjust_every_months: 12,
        margin: 0.0275,
        periodic_cap: 0.02,
        lifetime_cap: 0.05,
    };
    for pr in &mut p.properties {
        pr.maintenance = MaintenanceModel::Lumpy;
        pr.assessment = cap;
        pr.insurance_real_growth = InsuranceGrowth::Stochastic;
        if let Some(m) = &mut pr.mortgage {
            m.kind = arm;
        }
    }
    for ev in &mut p.events {
        if let EventKind::BuyHome(b) = &mut ev.kind {
            b.maintenance = MaintenanceModel::Lumpy;
            b.assessment = cap;
            b.insurance_real_growth = InsuranceGrowth::Stochastic;
            b.mortgage_kind = arm;
        }
    }
    for item in &mut p.spending.items {
        if item.category == Category::Rent {
            item.rent_growth = RentGrowth::Market;
        }
    }
    p.assumptions.housing_liquidity = HousingLiquidity::Realistic;
    p
}

#[test]
fn the_housing_options_simulate_without_allocating() {
    let mut profiles: Vec<Profile> = Profile::examples(common::START)
        .into_iter()
        .map(|e| all_options_on(e.profile))
        .collect();
    // A poorer copy of each, so that last-resort sales are listed and bridged.
    let poorer: Vec<Profile> = profiles
        .iter()
        .cloned()
        .map(|mut p| {
            for a in &mut p.accounts {
                a.balance *= 0.2;
            }
            p.name.push_str(" (poorer)");
            p
        })
        .collect();
    profiles.extend(poorer);
    for profile in profiles {
        let plan = common::compile(&profile);
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
        assert_eq!(
            made, 0,
            "{}: {made} allocations over 300 paths",
            profile.name
        );
        assert!(sink.years > 300 && sink.total.is_finite());
    }
}
