//! The tax functions the simulation loop calls must not touch the heap.
//!
//! This lives in its own test binary because it replaces the global allocator with
//! one that counts allocations made by the current thread.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use firesight_core::data::Data;
use firesight_core::model::TaxModel;
use firesight_core::tax::{
    compute, effective_marginal_rate, irmaa_monthly_surcharge, rmd_divisor, room_to_aca_cliff,
    room_to_bracket_top, FilingStatus, Indexation, StateCode, TaxInputs, TaxLaw,
};

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

#[test]
fn annual_tax_functions_do_not_allocate() {
    let law = TaxLaw::from_data(Data::embedded(), &TaxModel::default()).unwrap();
    // Touch the lazily built RMD table before counting.
    assert!(rmd_divisor(80, 1950).is_some());
    let states =
        ["CA", "NY", "TX", "CO", "WA", "MN", "UT", "HI"].map(|s| StateCode::parse(s).unwrap());

    let before = allocations();
    let mut checksum = 0.0;
    for (i, &state) in states.iter().enumerate() {
        let index = law.index_from_cpi(2026 + i as i32 * 3, 1.0 + 0.09 * i as f64);
        let p = law.for_year(index, state);
        for k in 0..50u32 {
            let scale = f64::from(k);
            let x = TaxInputs {
                filing_status: FilingStatus::ALL[(k % 3) as usize],
                ages: [45.0 + scale, 40.0 + scale],
                adults: 2,
                children_under_17: k % 3,
                other_dependents: k % 2,
                state,
                wages: [4_000.0 * scale, 1_500.0 * scale],
                self_employment: [300.0 * scale, 0.0],
                pretax_deferrals: [400.0 * scale, 0.0],
                hsa_contributions: [50.0 * scale, 0.0],
                interest: 20.0 * scale,
                ordinary_dividends: 10.0 * scale,
                qualified_dividends: 60.0 * scale,
                short_term_gains: -30.0 * scale,
                long_term_gains: 900.0 * scale,
                traditional_distributions: 700.0 * scale,
                roth_conversions: 100.0 * scale,
                pensions: 200.0 * scale,
                social_security: [500.0 * scale, 300.0 * scale],
                rental_net: -100.0 * scale,
                other_ordinary: 10.0 * scale,
                mortgage_interest: 300.0 * scale,
                property_tax: 150.0 * scale,
                state_tax_paid: 0.0,
                charitable: 40.0 * scale,
                early_distributions: 20.0 * scale,
                hsa_nonqualified: 5.0 * scale,
                aca_household_size: 2,
                aca_months: 12,
                aca_benchmark_premium: 16_000.0,
                on_medicare: [k > 20, k > 25],
                ..TaxInputs::default()
            };
            let r = compute(&p, &x);
            checksum += r.total + r.aca_premium_credit + r.irmaa_surcharge;
            checksum += room_to_bracket_top(&p, &x, 0.12).min(1e9);
            checksum += room_to_bracket_top(&p, &x, 0.24).min(1e9);
            checksum += room_to_aca_cliff(&p, &x).min(1e9);
            checksum += effective_marginal_rate(&p, &x);
            checksum += irmaa_monthly_surcharge(&p, x.filing_status, r.magi_irmaa);
            checksum += rmd_divisor(60 + k, 1950 + k as i32).unwrap_or(0.0);

            // The same household with private-company equity: AMT on, an
            // incentive-option adjustment, a qualifying-sale adjustment, a credit
            // carried in, option income without payroll tax and an excluded gain.
            let equity = TaxInputs {
                amt_applies: true,
                amt_iso_adjustment: 9_000.0 * scale,
                amt_gain_adjustment: -400.0 * scale,
                amt_credit_carryforward: 1_200.0 * scale,
                wages_no_fica: [800.0 * scale, 0.0],
                qsbs_excluded_gain: 5_000.0 * scale,
                ..x
            };
            let r = compute(&p, &equity);
            checksum += r.total + r.amt + r.amt_credit_used + r.amt_credit_carryforward;
            checksum += r.tentative_minimum_tax;
            checksum += room_to_bracket_top(&p, &equity, 0.24).min(1e9);
            checksum += effective_marginal_rate(&p, &equity);
        }
        // An indexation with nothing derived, and a household abroad.
        let abroad = law
            .for_year(Indexation::base(2030), StateCode::NONE)
            .with_foreign_rate(Some(0.2));
        checksum += compute(&abroad, &TaxInputs::default()).total;
    }
    let after = allocations();
    assert!(checksum.is_finite());
    assert_eq!(after - before, 0, "tax functions allocated on the heap");
}

/// The parameter and input types cross the simulation loop by value.
#[test]
fn tax_values_stay_copy() {
    fn is_copy<T: Copy>() {}
    is_copy::<TaxInputs>();
    is_copy::<firesight_core::tax::TaxResult>();
    is_copy::<firesight_core::tax::FederalYear>();
    is_copy::<firesight_core::tax::TaxYearParams>();
}
