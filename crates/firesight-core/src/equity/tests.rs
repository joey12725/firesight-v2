//! Unit tests of the private-company equity module.
//!
//! Every frequency test states its sample size and a bound of four standard errors
//! around a value computed here from the parameters, so an edit to `params.json` fails
//! with the new number in the message and nobody tunes a seed.

use super::*;
use crate::economy::{EconomyModel, EconomyStreams};
use crate::model::{
    EquityPolicy, MarketKind, MarketModel, PortfolioPolicy, TenderOffers, N_ASSETS,
};

const START: YearMonth = YearMonth::ym(2026, 11);
const CAPS: [f64; 2] = [15_000_000.0, 10_000_000.0];
/// More cash than any test exercise needs.
const AMPLE: f64 = 1e15;

fn params() -> EquityParams {
    EquityParams::from_data(Data::embedded())
}

fn stream(path: u64) -> Rng {
    Rng::stream(7, path, Stream::PrivateCompany(0))
}

/// A company at $10 a share linked to a job, with a 409A value of $4.
fn company() -> PrivateCompany {
    PrivateCompany {
        id: "acme".into(),
        name: "Acme".into(),
        job: Some("job".into()),
        preferred_price: 10.0,
        fmv_409a: Some(4.0),
        ..PrivateCompany::default()
    }
}

fn holding(kind: HoldingKind, shares: f64, vested: f64, vest_months_left: u32) -> EquityHolding {
    EquityHolding {
        id: format!("h{shares}"),
        name: String::new(),
        kind,
        shares,
        vested,
        vest_months_left,
        cliff_months: 0,
    }
}

fn iso(strike: f64) -> HoldingKind {
    HoldingKind::Iso {
        strike,
        granted_months_ago: 30,
    }
}

fn nso(strike: f64) -> HoldingKind {
    HoldingKind::Nso {
        strike,
        granted_months_ago: 30,
    }
}

fn rsu() -> HoldingKind {
    HoldingKind::Rsu {
        must_be_employed_at_event: false,
    }
}

fn shares(basis_per_share: f64, months_held: u32) -> HoldingKind {
    HoldingKind::Shares {
        basis_per_share,
        months_held,
        from_iso: false,
        fmv_at_exercise: 0.0,
    }
}

fn iso_shares(basis_per_share: f64, months_held: u32, fmv_at_exercise: f64) -> HoldingKind {
    HoldingKind::Shares {
        basis_per_share,
        months_held,
        from_iso: true,
        fmv_at_exercise,
    }
}

fn compile(c: &PrivateCompany) -> CompanyModel {
    CompanyModel::compile(c, &params(), CAPS, START)
}

/// A fate of the given kind, with the common multiple and the path end the fate draw
/// would give it. `m` is the multiple at the outcome.
fn fate(kind: OutcomeKind, month: i32, m: f64, model: &CompanyModel) -> Fate {
    let (x, m_end) = match kind {
        OutcomeKind::Acquired => {
            let x = common_multiple(m, model.table.overhang);
            (x, x.max(model.params.fail_path_end_multiple))
        }
        OutcomeKind::Ipo => (m, m),
        OutcomeKind::Fails => (0.0, model.params.fail_path_end_multiple),
        OutcomeKind::StillPrivate => (0.0, 1.0),
    };
    Fate {
        kind,
        month,
        round: 0,
        m,
        x,
        m_end,
        lockup: 1.0,
    }
}

/// A company that stays private on a flat price path for thirty years and is then
/// written off (so it runs no tender offers).
fn flat(model: &CompanyModel) -> Fate {
    fate(OutcomeKind::StillPrivate, 360, 1.0, model)
}

/// A company on a flat price path that lists at its last round's price after
/// thirty years: private throughout a test, and not heading below its last round.
fn level(model: &CompanyModel) -> Fate {
    fate(OutcomeKind::Ipo, 360, 1.0, model)
}

fn state(model: &CompanyModel, f: Fate) -> CompanyState {
    let mut st = model.initial_state(Link::Active);
    st.fate = f;
    st
}

/// The climate of month `t` for someone employed at the company, window open, with
/// ample cash and no tax on an exercise.
fn climate(t: i32) -> EquityClimate {
    EquityClimate {
        t,
        year: START.add_months(t).year(),
        employed: true,
        job_started: true,
        window_open: true,
        u_tender: 0.999_999,
        liquid: AMPLE,
        ..EquityClimate::default()
    }
}

/// Sums of a run of months, with every event and the month it happened in.
#[derive(Default)]
struct Totals {
    em: EquityMonth,
    events: Vec<(i32, EqEvent)>,
}

impl Totals {
    fn add(&mut self, t: i32, m: &EquityMonth) {
        let s = &mut self.em;
        s.exercise_cost += m.exercise_cost;
        s.wages_cash += m.wages_cash;
        s.wages_noncash += m.wages_noncash;
        s.iso_income += m.iso_income;
        s.share_proceeds += m.share_proceeds;
        s.cash_in += m.cash_in;
        s.short_term_gain += m.short_term_gain;
        s.long_term_gain += m.long_term_gain;
        s.amt_adjustment += m.amt_adjustment;
        s.amt_gain_adjustment += m.amt_gain_adjustment;
        s.qsbs_excluded += m.qsbs_excluded;
        s.tax_estimate += m.tax_estimate;
        s.to_sleeve_value += m.to_sleeve_value;
        s.to_sleeve_basis += m.to_sleeve_basis;
        s.tender_proceeds += m.tender_proceeds;
        s.basis_in += m.basis_in;
        s.basis_out += m.basis_out;
        s.units_out += m.units_out;
        s.paper_value = m.paper_value;
        s.failed_now |= m.failed_now;
        s.replan |= m.replan;
        self.events.extend(m.events().iter().map(|e| (t, *e)));
    }

    fn count(&self, kind: EqEventKind) -> usize {
        self.events.iter().filter(|(_, e)| e.kind == kind).count()
    }

    fn first(&self, kind: EqEventKind) -> Option<(i32, EqEvent)> {
        self.events.iter().copied().find(|(_, e)| e.kind == kind)
    }
}

/// Steps the company through the months given; `adjust` edits each month's climate.
fn run(
    model: &CompanyModel,
    st: &mut CompanyState,
    months: std::ops::RangeInclusive<i32>,
    adjust: impl Fn(&mut EquityClimate),
) -> Totals {
    let mut totals = Totals::default();
    for t in months {
        let mut c = climate(t);
        adjust(&mut c);
        let em = model.step(st, &c);
        totals.add(t, &em);
    }
    totals
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs()))
}

#[track_caller]
fn assert_close(a: f64, b: f64, what: &str) {
    assert!(close(a, b), "{what}: {a} against {b}");
}

/// Asserts that `count` out of `n` is within four binomial standard errors of `p`.
#[track_caller]
fn assert_share(count: usize, n: usize, p: f64, what: &str) {
    let observed = count as f64 / n as f64;
    let bound = 4.0 * (p * (1.0 - p) / n as f64).sqrt();
    assert!(
        (observed - p).abs() <= bound + 1e-12,
        "{what}: observed {observed:.5} in {n} draws, expected {p:.5} plus or minus {bound:.5}"
    );
}

/// A uniform that lands in the middle of row `k` of a table.
fn u_for_row(table: &ResolvedTable, k: usize) -> f64 {
    let below = if k == 0 {
        0.0
    } else {
        table.rows[k - 1].cumulative
    };
    below + 0.5 * table.rows[k].probability
}

fn typical(stage: CompanyStage, p: &EquityParams) -> ResolvedTable {
    preset_table(stage, Outlook::Typical, p.overhang[stage.index()], p)
}

fn row(kind: OutcomeKind, probability: f64, years: (f64, f64), multiple: (f64, f64)) -> OutcomeRow {
    OutcomeRow {
        id: format!("{}_{}", kind.key(), years.0),
        kind,
        probability,
        years_from: years.0,
        years_to: years.1,
        multiple_low: multiple.0,
        multiple_high: multiple.1,
    }
}

/// The company of the fourth demo household, from the example itself so the two
/// cannot drift.
fn demo_company() -> PrivateCompany {
    let examples = crate::model::Profile::examples(crate::model::EXAMPLE_START);
    examples[3].profile.private_equity[0].clone()
}

// ---------------------------------------------------------------------------
// Parameters and the outcome table
// ---------------------------------------------------------------------------

#[test]
fn parameters_match_the_data_file() {
    let p = params();
    for s in CompanyStage::ALL {
        let i = s.index();
        let sum = p.p_fails[i] + p.p_acquired[i] + p.p_ipo[i] + p.p_still_private[i];
        assert!((sum - 1.0).abs() < 1e-9, "{}: {sum}", s.key());
        let table = typical(s, &p);
        assert_eq!(table.n, 4);
        assert_eq!(table.rows[3].cumulative, 1.0);
        assert_eq!(table.round_months, 120);
    }
    assert_eq!(p.horizon_months, 120);
    assert_eq!(p.rounds, 3);
    assert_eq!(p.z_cap, 2.326);
    assert_eq!(p.window_open_share, 0.70);
    assert_eq!(p.tender_min_gap_months, 12);
    assert_eq!(p.preview_inflation, 0.029);
    assert_eq!(p.fail_path_end_multiple, 0.25);
    assert_eq!(p.ipo_window_search_months, 60);
    assert_eq!(p.outlook_failure_bounds, [0.01, 0.95]);
    assert_eq!(p.preview_draws, 4000);
    // The stage defaults the doc comments quote.
    assert_eq!((p.fmv_ratio[0], p.fmv_ratio[5]), (0.20, 0.65));
    assert_eq!((p.overhang[0], p.overhang[5]), (0.20, 0.35));
    assert_eq!(
        (p.tender_probability[0], p.tender_probability[5]),
        (0.005, 0.15)
    );
}

#[test]
fn outcome_frequencies_match_the_table() {
    const N: usize = 200_000;
    let p = params();
    for stage in CompanyStage::ALL {
        let table = typical(stage, &p);
        let q = table.still_private_probability();
        let mut first = [0usize; 3];
        let mut lifetime = [0usize; 3];
        let mut written_off = 0usize;
        for i in 0..N {
            let f = draw_fate(&table, &p, &mut stream(i as u64));
            let slot = match f.kind {
                OutcomeKind::Fails => 0,
                OutcomeKind::Acquired => 1,
                OutcomeKind::Ipo => 2,
                OutcomeKind::StillPrivate => {
                    written_off += 1;
                    assert_eq!(f.month, 360);
                    continue;
                }
            };
            lifetime[slot] += 1;
            match f.round {
                0 => {
                    first[slot] += 1;
                    assert!((1..=120).contains(&f.month), "{f:?}");
                }
                1 => assert!((121..=240).contains(&f.month), "{f:?}"),
                _ => assert!((241..=360).contains(&f.month), "{f:?}"),
            }
        }
        let factor = (1.0 - math::powi(q, 3)) / (1.0 - q);
        for (slot, name) in ["fails", "acquired", "ipo"].iter().enumerate() {
            let pk = table.rows[slot].probability;
            let what = format!("{} {name}", stage.key());
            assert_share(first[slot], N, pk, &format!("{what}, first round"));
            assert_share(lifetime[slot], N, pk * factor, &format!("{what}, lifetime"));
        }
        assert_share(
            written_off,
            N,
            math::powi(q, 3),
            &format!("{} written off", stage.key()),
        );
        assert_close(lifetime_factor(q, 3), factor, "lifetime factor");
    }
}

#[test]
fn outlook_moves_failure_and_keeps_the_total() {
    let p = params();
    for stage in CompanyStage::ALL {
        let base = p.stage_probabilities(stage, Outlook::Typical);
        for (outlook, sign) in [(Outlook::Pessimistic, 1.0), (Outlook::Optimistic, -1.0)] {
            let moved = p.stage_probabilities(stage, outlook);
            let sum: f64 = moved.iter().sum();
            assert!(
                (sum - 1.0).abs() < 1e-12,
                "{} {outlook:?}: {sum}",
                stage.key()
            );
            let expected = if stage == CompanyStage::Unicorn && outlook == Outlook::Optimistic {
                -0.09
            } else {
                sign * 0.10
            };
            assert!(
                (moved[0] - base[0] - expected).abs() < 1e-12,
                "{} {outlook:?}: failure moved by {}",
                stage.key(),
                moved[0] - base[0]
            );
            for k in 1..4 {
                assert_close(moved[k] / moved[1], base[k] / base[1], "ratio kept");
            }
            let table = preset_table(stage, outlook, 0.3, &p);
            assert_eq!(table.rows[0].probability, moved[0]);
        }
    }
    // The outlook is ignored once a custom table is in force.
    let mut c = company();
    c.outcomes = vec![row(OutcomeKind::Fails, 1.0, (1.0, 2.0), (1.0, 1.0))];
    c.outlook = Outlook::Optimistic;
    assert_eq!(resolved_table(&c, &p).rows[0].probability, 1.0);
}

#[test]
fn preset_timing_follows_the_cut_off_lognormal() {
    const N: usize = 100_000;
    let p = params();
    for stage in CompanyStage::ALL {
        let table = typical(stage, &p);
        let s = stage.index();
        let medians = [p.fails_median[s], p.acquired_median[s], p.ipo_median[s]];
        for (k, median_years) in medians.into_iter().enumerate() {
            let sigma = p.timing_sigma[s];
            let top = normal_cdf(math::ln(10.0 / median_years) / sigma);
            // CDF of the month before rounding.
            let cdf =
                |months: f64| normal_cdf(math::ln(months / (12.0 * median_years)) / sigma) / top;
            let expected_median =
                12.0 * median_years * math::exp(sigma * inverse_normal_cdf(0.5 * top));
            let cut = expected_median.floor();
            let u1 = u_for_row(&table, k);
            let mut rng = stream(1_000 + (s * 3 + k) as u64);
            let mut months = Vec::with_capacity(N);
            for _ in 0..N {
                let f = fate_from_words(&table, &p, u1, rng.uniform(), 0, 0);
                assert_eq!(f.kind, table.rows[k].kind);
                assert!((1..=120).contains(&f.month), "{f:?}");
                months.push(f.month);
            }
            // A month is the rounded draw, so P(month <= j) = cdf(j + 0.5).
            let at_or_below = months.iter().filter(|m| f64::from(**m) <= cut).count();
            let what = format!("{} row {k}: dates at or before month {cut}", stage.key());
            assert_share(at_or_below, N, cdf(cut + 0.5), &what);
            months.sort_unstable();
            let sample_median = f64::from(months[N / 2]);
            assert!(
                (sample_median - expected_median).abs() <= 1.0,
                "{} row {k}: median {sample_median} against {expected_median:.2}",
                stage.key()
            );
            if stage == CompanyStage::Seed && k == 0 {
                let first_year = months.iter().filter(|m| **m <= 12).count() as f64 / N as f64;
                assert!(
                    (0.04..=0.09).contains(&first_year),
                    "seed failures in the first twelve months: {first_year}"
                );
                assert_share(
                    months.iter().filter(|m| **m <= 12).count(),
                    N,
                    cdf(12.5),
                    "seed",
                );
            }
        }
    }
}

#[test]
fn custom_table_is_normalised_and_followed() {
    const N: usize = 200_000;
    let p = params();
    let mut c = company();
    // Probabilities that sum to 1.004: inside what validation accepts.
    c.outcomes = vec![
        row(OutcomeKind::Fails, 0.301, (1.0, 3.0), (1.0, 1.0)),
        row(OutcomeKind::Acquired, 0.251, (2.0, 6.0), (0.5, 3.0)),
        row(OutcomeKind::Ipo, 0.151, (3.0, 8.0), (2.0, 10.0)),
        row(OutcomeKind::StillPrivate, 0.201, (0.0, 0.0), (1.0, 1.0)),
        row(OutcomeKind::Acquired, 0.100, (1.0, 1.0), (1.5, 1.5)),
    ];
    let table = resolved_table(&c, &p);
    assert_eq!(table.n, 5);
    assert_eq!(table.round_months, 120);
    let total: f64 = table.rows().iter().map(|r| r.probability).sum();
    assert!((total - 1.0).abs() < 1e-12);
    assert_close(table.rows[0].probability, 0.301 / 1.004, "normalised");

    let mut counts = [0usize; 5];
    let mut ipo_months = Vec::new();
    let mut sale_multiples = Vec::new();
    for i in 0..N {
        let mut rng = stream(i as u64);
        let (u1, u2, w3, w4) = (rng.uniform(), rng.uniform(), rng.next_u64(), rng.next_u64());
        let f = fate_from_words(&table, &p, u1, u2, w3, w4);
        assert_eq!(f, draw_fate(&table, &p, &mut stream(i as u64)));
        if f.round != 0 || f.kind == OutcomeKind::StillPrivate {
            continue;
        }
        let k = table.rows().iter().position(|r| u1 < r.cumulative).unwrap();
        counts[k] += 1;
        match k {
            1 => sale_multiples.push(f.m),
            2 => ipo_months.push(f64::from(f.month)),
            4 => {
                assert_eq!(f.month, 12);
                assert!((f.m - 1.5).abs() < 1e-12, "a constant multiple: {}", f.m);
            }
            _ => {}
        }
    }
    for k in [0, 1, 2, 4] {
        assert_share(counts[k], N, table.rows[k].probability, &format!("row {k}"));
    }

    // Dates: uniform on months 36 to 96.
    let n = ipo_months.len() as f64;
    let mean = ipo_months.iter().sum::<f64>() / n;
    let sd = ((60.0_f64 * 60.0 + 1.0) / 12.0).sqrt();
    assert!(
        (mean - 66.0).abs() <= 4.0 * sd / n.sqrt(),
        "mean IPO month {mean} over {n} draws, expected 66 plus or minus {}",
        4.0 * sd / n.sqrt()
    );
    assert_eq!(
        ipo_months.iter().copied().fold(f64::INFINITY, f64::min),
        36.0
    );
    assert_eq!(ipo_months.iter().copied().fold(0.0, f64::max), 96.0);

    // Multiples: one in ten below the low end, one in ten above the high end, and
    // the median at the geometric mean.
    let n = sale_multiples.len();
    let below = sale_multiples.iter().filter(|m| **m < 0.5).count();
    let above = sale_multiples.iter().filter(|m| **m > 3.0).count();
    let under_median = sale_multiples
        .iter()
        .filter(|m| **m < 1.5_f64.sqrt())
        .count();
    assert_share(below, n, 0.10, "sales below the low multiple");
    assert_share(above, n, 0.10, "sales above the high multiple");
    assert_share(under_median, n, 0.50, "sales below the geometric mean");

    // A table whose latest row ends at year 15 has rounds of 180 months.
    c.outcomes = vec![
        row(OutcomeKind::Acquired, 0.5, (12.0, 15.0), (1.0, 2.0)),
        row(OutcomeKind::StillPrivate, 0.5, (0.0, 0.0), (1.0, 1.0)),
    ];
    let long = resolved_table(&c, &p);
    assert_eq!(long.round_months, 180);
    let second = fate_from_words(&long, &p, 0.60, 0.5, 0, 0);
    assert_eq!((second.kind, second.round), (OutcomeKind::Acquired, 1));
    assert!((180 + 144..=360).contains(&second.month), "{second:?}");
    assert_eq!(fate_from_words(&long, &p, 0.99, 0.5, 0, 0).month, 540);
}

#[test]
fn multiples_are_capped_at_the_first_and_last_percentile() {
    const N: usize = 1_000_000;
    let p = params();
    let table = typical(CompanyStage::Seed, &p);
    let u1 = u_for_row(&table, 2);
    let (lo, hi) = (40.0 * math::exp(-2.326), 40.0 * math::exp(2.326));
    let mut rng = stream(0);
    let (mut at_lo, mut at_hi) = (0usize, 0usize);
    for _ in 0..N {
        let f = fate_from_words(&table, &p, u1, 0.5, rng.next_u64(), 0);
        assert_eq!(f.kind, OutcomeKind::Ipo);
        assert!(
            f.m >= lo * (1.0 - 1e-12) && f.m <= hi * (1.0 + 1e-12),
            "{}",
            f.m
        );
        at_lo += usize::from(f.m <= lo * (1.0 + 1e-12));
        at_hi += usize::from(f.m >= hi * (1.0 - 1e-12));
    }
    // The cap holds the first and the last percentile at the bound.
    let tail = 1.0 - normal_cdf(2.326);
    assert_share(at_lo, N, tail, "multiples at the lower cap");
    assert_share(at_hi, N, tail, "multiples at the upper cap");

    let mut c = company();
    c.outcomes = vec![row(OutcomeKind::Ipo, 1.0, (1.0, 2.0), (0.001, 1000.0))];
    let wide = resolved_table(&c, &p);
    for _ in 0..100_000 {
        let f = fate_from_words(&wide, &p, 0.5, 0.5, rng.next_u64(), rng.next_u64());
        assert!(f.m.is_finite() && (1e-4..=1e4).contains(&f.m), "{}", f.m);
        assert!(f.lockup.is_finite() && f.x == f.m && f.m_end == f.m);
    }
}

#[test]
fn the_overhang_rule_is_exact_at_its_ends() {
    for l in [0.0, 0.2, 0.3, 0.95] {
        assert_eq!(common_multiple(l, l), 0.0);
        assert_eq!(common_multiple(1.0, l), 1.0);
        assert_eq!(common_multiple(2.0, l), 2.0);
    }
    assert_close(common_multiple(0.65, 0.30), 0.5, "half way up");
    assert_eq!(common_multiple(0.1, 0.30), 0.0);
    // Overhangs beyond the bound are held at it.
    assert_eq!(
        common_multiple(0.96, 2.0),
        common_multiple(0.96, OVERHANG_MAX)
    );

    const N: usize = 200_000;
    let p = params();
    let table = typical(CompanyStage::SeriesC, &p);
    let u1 = u_for_row(&table, 1);
    let mut rng = stream(3);
    let mut zero = 0usize;
    for _ in 0..N {
        let f = fate_from_words(&table, &p, u1, 0.5, rng.next_u64(), 0);
        assert_eq!(f.kind, OutcomeKind::Acquired);
        zero += usize::from(f.x == 0.0);
        assert_eq!(f.m_end, f.x.max(0.25));
    }
    let expected = normal_cdf(math::ln(0.30 / 0.60) / 1.0);
    assert!((expected - 0.244).abs() < 0.001);
    assert_share(
        zero,
        N,
        expected,
        "Series C acquisitions that pay common nothing",
    );
}

#[test]
fn preset_implied_numbers_are_pinned() {
    const N: usize = 100_000;
    let p = params();
    let c = p.z_cap;
    for stage in CompanyStage::ALL {
        let s = stage.index();
        let table = typical(stage, &p);
        let l = p.overhang[s];
        let mut rng = stream(50 + s as u64);

        // Acquisitions in which common stock gets nothing.
        let u1 = u_for_row(&table, 1);
        let zero = (0..N)
            .filter(|_| fate_from_words(&table, &p, u1, 0.5, rng.next_u64(), 0).x == 0.0)
            .count();
        // The normal behind a multiple is held within the cap, so a threshold beyond
        // the cap is never crossed.
        let share_below = |z: f64| if z > -c { normal_cdf(z) } else { 0.0 };
        let expected = share_below(math::ln(l / p.acquisition_median[s]) / p.acquisition_sigma[s]);
        assert!(expected > 0.1, "{}: {expected}", stage.key());
        assert_share(
            zero,
            N,
            expected,
            &format!("{} sales paying nothing", stage.key()),
        );

        // IPOs priced below the last round.
        let u1 = u_for_row(&table, 2);
        let below = (0..N)
            .filter(|_| fate_from_words(&table, &p, u1, 0.5, rng.next_u64(), 0).m < 1.0)
            .count();
        let expected =
            share_below(math::ln(1.0 / p.ipo_multiple_median[s]) / p.ipo_multiple_sigma[s]);
        assert_share(
            below,
            N,
            expected,
            &format!("{} IPOs below the round", stage.key()),
        );
        // Seed and Series A IPOs are never below the round once the cap is applied.
        assert_eq!(expected == 0.0, s < 2, "{}: {expected}", stage.key());

        // The implied multiple against an independent trapezoid over z.
        const POINTS: usize = 200_001;
        let step = 2.0 * c / (POINTS - 1) as f64;
        let tail = 1.0 - normal_cdf(c);
        let density = |z: f64| math::exp(-0.5 * z * z) / (2.0 * std::f64::consts::PI).sqrt();
        let mean_of = |value: &dyn Fn(f64) -> f64| {
            let mut sum = 0.5 * (value(-c) * density(-c) + value(c) * density(c));
            for i in 1..POINTS - 1 {
                let z = -c + step * i as f64;
                sum += value(z) * density(z);
            }
            sum * step + tail * (value(-c) + value(c))
        };
        let sale = mean_of(&|z| {
            common_multiple(
                p.acquisition_median[s] * math::exp(p.acquisition_sigma[s] * z),
                l,
            )
        });
        let ipo = mean_of(&|z| p.ipo_multiple_median[s] * math::exp(p.ipo_multiple_sigma[s] * z));
        let q = p.p_still_private[s];
        let factor = (1.0 - math::powi(q, 3)) / (1.0 - q);
        let expected = factor * (p.p_acquired[s] * sale + p.p_ipo[s] * ipo * p.lockup_mean);
        let got = implied_multiple(&table, l, &p);
        assert!(
            (got / expected - 1.0).abs() < 0.005,
            "{}: implied multiple {got} against {expected}",
            stage.key()
        );

        // The preview reports the same number whatever the draws.
        let mut co = company();
        co.stage = stage;
        for draws in [100, 1_000] {
            let pv = preview(&co, &p, draws);
            assert_eq!(pv.implied_multiple, got);
            assert_eq!(pv.typical_implied_multiple, got);
        }
    }
}

// ---------------------------------------------------------------------------
// Prices
// ---------------------------------------------------------------------------

#[test]
fn the_price_path_is_a_log_line_to_the_outcome() {
    let m = compile(&company());
    let ipo = fate(OutcomeKind::Ipo, 40, 4.0, &m);
    assert_eq!(m.price_at(&ipo, 0), 10.0);
    assert_close(m.price_at(&ipo, 40), 40.0, "IPO price");
    assert_close(m.price_at(&ipo, 20), 20.0, "half way in logs");
    let fails = fate(OutcomeKind::Fails, 40, 1.0, &m);
    assert_close(
        m.price_at(&fails, 40),
        2.5,
        "a failing path ends at a quarter",
    );
    assert_close(m.price_at(&fails, 20), 5.0, "and is at a half half way");
    // A sale that pays common 0.1 of the round heads to the floor, not below it.
    let overhang = m.table.overhang;
    let low = fate(
        OutcomeKind::Acquired,
        40,
        overhang + 0.1 * (1.0 - overhang),
        &m,
    );
    assert_close(low.x, 0.1, "common multiple");
    assert_close(m.price_at(&low, 40), 2.5, "floor");
    let high = fate(OutcomeKind::Acquired, 40, 3.0, &m);
    assert_close(m.price_at(&high, 40), 30.0, "a good sale");
    let written_off = flat(&m);
    for t in [0, 100, 359, 360] {
        assert_eq!(m.price_at(&written_off, t), 10.0);
    }

    // The step follows the same path, and the event price is exact.
    let mut st = state(&m, low);
    let before = run(&m, &mut st, 0..=39, |_| {});
    assert_close(st.price, m.price_at(&low, 39), "price before the event");
    assert_eq!(before.events.len(), 0);
    let at = run(&m, &mut st, 40..=40, |_| {});
    assert_close(st.price, 1.0, "common is paid P0 times x");
    assert_eq!(at.first(EqEventKind::Acquired).unwrap().1.price, st.price);
}

#[test]
fn the_409a_ratio_closes_toward_the_event() {
    let m = compile(&company());
    let overhang = m.table.overhang;
    for f in [
        fate(OutcomeKind::Ipo, 40, 4.0, &m),
        fate(OutcomeKind::Acquired, 40, 3.0, &m),
        fate(
            OutcomeKind::Acquired,
            40,
            overhang + 0.1 * (1.0 - overhang),
            &m,
        ),
        fate(OutcomeKind::Acquired, 40, 0.1, &m),
        fate(OutcomeKind::Fails, 40, 1.0, &m),
        flat(&m),
    ] {
        assert_close(m.fmv_at(&f, 0), 4.0, "F(0) is the 409A value entered");
        let at_event = m.fmv_at(&f, f.month);
        match f.kind {
            OutcomeKind::Ipo => assert_close(at_event, 10.0 * f.m, "IPO"),
            OutcomeKind::Acquired => assert_close(at_event, 10.0 * f.x, "sale"),
            _ => {
                for t in [0, 10, f.month] {
                    assert_close(m.fmv_ratio_at(&f, t), 0.4, "constant ratio");
                }
            }
        }
        // Between the two the value moves monotonically toward the event.
        let mid = m.fmv_at(&f, f.month / 2);
        assert!(mid >= at_event.min(4.0) - 1e-9 && mid <= at_event.max(4.0) + 1e-9);
    }
    // Without a 409A value the stage's typical fraction applies.
    let mut c = company();
    c.fmv_409a = None;
    c.stage = CompanyStage::Unicorn;
    let m = compile(&c);
    assert_close(m.fmv_at(&flat(&m), 0), 6.5, "unicorn default");
    assert_close(fmv_in_force(&c, &params()), 6.5, "the same in the preview");
}

#[test]
fn the_lockup_factor_has_the_stated_mean() {
    const N: usize = 1_000_000;
    let p = params();
    let table = typical(CompanyStage::Late, &p);
    let mut rng = stream(11);
    let (lo, hi) = (math::exp(-2.326 * 0.45), math::exp(2.326 * 0.45));
    let (mut sum, mut sum_sq) = (0.0, 0.0);
    for _ in 0..N {
        let f = fate_from_words(&table, &p, 0.5, 0.5, 0, rng.next_u64());
        let centred = f.lockup * p.lockup_norm / p.lockup_mean;
        assert!(
            centred >= lo * (1.0 - 1e-12) && centred <= hi * (1.0 + 1e-12),
            "{centred}"
        );
        sum += f.lockup;
        sum_sq += f.lockup * f.lockup;
    }
    let mean = sum / N as f64;
    let sd = (sum_sq / N as f64 - mean * mean).sqrt();
    let bound = 4.0 * sd / (N as f64).sqrt();
    assert!(
        (mean - p.lockup_mean).abs() <= bound,
        "mean lockup factor {mean} over {N} draws, expected {} plus or minus {bound}",
        p.lockup_mean
    );
    assert_eq!(p.lockup_mean, 1.0);
}

// ---------------------------------------------------------------------------
// Tender offers and the window
// ---------------------------------------------------------------------------

fn tender_company(annual_probability: f64) -> PrivateCompany {
    let mut c = company();
    c.tenders.annual_probability = Some(annual_probability);
    c
}

/// Months with a tender in a run over `months`, with a fresh uniform each month.
fn tender_months(
    m: &CompanyModel,
    f: Fate,
    months: std::ops::RangeInclusive<i32>,
    rng: &mut Rng,
    window_open: bool,
) -> Vec<(i32, f64)> {
    let mut st = state(m, f);
    let mut out = Vec::new();
    for t in months {
        let mut c = climate(t);
        c.window_open = window_open;
        c.u_tender = rng.uniform();
        let em = m.step(&mut st, &c);
        if let Some(e) = em.event(EqEventKind::Tender) {
            out.push((t, e.price));
        }
    }
    out
}

#[test]
fn tenders_arrive_at_the_annual_rate_and_at_most_once_a_year() {
    const N: usize = 20_000;
    let p = params();
    let m = compile(&tender_company(0.15));
    // The yearly chance entered is an average over every future; only a company
    // not heading below its last round runs tenders, at a higher rate.
    let high = tender_rate(&m.table, &p, 0.15);
    assert!(high > 0.15 && high < 1.0, "{high}");
    let h = (1.0 - math::pow(1.0 - high, 1.0 / 12.0)) / p.window_open_share;
    assert_close(m.tender_hazard, h, "monthly hazard");

    // Expected tenders in months 1 to 119: a tender needs none in the 11 months before.
    let mut q = [0.0_f64; 120];
    for t in 1..120usize {
        let recent: f64 = q[t.saturating_sub(11).max(1)..t].iter().sum();
        q[t] = h * (1.0 - recent);
    }
    let expected: f64 = q.iter().sum();

    let mut rng = stream(21);
    let (mut sum, mut sum_sq) = (0.0, 0.0);
    for _ in 0..N {
        let months = tender_months(&m, level(&m), 0..=119, &mut rng, true);
        for pair in months.windows(2) {
            assert!(pair[1].0 - pair[0].0 >= 12, "{months:?}");
        }
        assert!(months.iter().all(|(t, _)| *t >= 1));
        let k = months.len() as f64;
        sum += k;
        sum_sq += k * k;
    }
    let mean = sum / N as f64;
    let sd = (sum_sq / N as f64 - mean * mean).sqrt();
    let bound = 4.0 * sd / (N as f64).sqrt();
    assert!(
        (mean - expected).abs() <= bound,
        "tenders per company over 119 months: {mean} in {N} companies, expected {expected:.4} plus or minus {bound:.4}"
    );

    // At a yearly chance of 1 there is exactly one every twelve months.
    let always = compile(&tender_company(1.0));
    assert_eq!(always.tender_hazard, 1.0);
    let months = tender_months(&always, level(&always), 0..=119, &mut rng, true);
    let expected: Vec<i32> = (0..10).map(|k| 1 + 12 * k).collect();
    assert_eq!(months.iter().map(|(t, _)| *t).collect::<Vec<_>>(), expected);
    // None with the window shut.
    assert!(tender_months(&always, level(&always), 0..=119, &mut rng, false).is_empty());
    // None in a company that will be written off, whatever the chance.
    assert!(tender_months(&always, flat(&always), 0..=119, &mut rng, true).is_empty());
    // None in or after the month of the outcome.
    let sale = fate(OutcomeKind::Acquired, 30, 2.0, &always);
    let months = tender_months(&always, sale, 0..=119, &mut rng, true);
    assert_eq!(
        months.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
        [1, 13, 25]
    );
    // And none at a yearly chance of 0.
    let never = compile(&tender_company(0.0));
    assert!(tender_months(&never, level(&never), 0..=119, &mut rng, true).is_empty());
}

#[test]
fn companies_heading_down_run_no_tenders() {
    let mut c = tender_company(1.0);
    c.tenders.price_fraction = 0.9;
    let m = compile(&c);
    assert_eq!(m.tender_hazard, 1.0);
    let mut rng = stream(22);
    let overhang = m.table.overhang;
    let down = [
        fate(OutcomeKind::Fails, 200, 1.0, &m),
        fate(OutcomeKind::Acquired, 200, 0.9, &m),
        fate(OutcomeKind::Acquired, 200, overhang, &m),
        fate(OutcomeKind::Ipo, 200, 0.8, &m),
    ];
    for path in 0..10_000 {
        let f = down[path % down.len()];
        assert!(f.m_end < 1.0, "{f:?}");
        assert!(tender_months(&m, f, 0..=40, &mut rng, true).is_empty());
    }
    // A company that will be written off sits on a flat path at the last round's
    // price for thirty years. It runs none either.
    for _ in 0..200 {
        assert!(tender_months(&m, flat(&m), 0..=99, &mut rng, true).is_empty());
    }
    // Where the company is not heading down, tenders are never priced below the
    // last round's price times the fraction.
    for f in [
        fate(OutcomeKind::Ipo, 100, 3.0, &m),
        fate(OutcomeKind::Acquired, 100, 1.0, &m),
        level(&m),
    ] {
        let months = tender_months(&m, f, 0..=99, &mut rng, true);
        assert_eq!(months.len(), 9);
        for (t, price) in months {
            assert!(price >= 10.0 * 0.9 - 1e-9, "month {t}: {price}");
            assert_close(price, m.price_at(&f, t) * 0.9, "tender price");
        }
    }
    // A tender at no price sells nothing (validation refuses such a fraction).
    let mut free = m;
    free.tender_price_fraction = 0.0;
    free.tender_planned = 3;
    let mut st = state(&free, level(&free));
    st.lots[0] = Lot {
        shares: 100.0,
        cost: 100.0,
        acquired: -30,
        granted: -42,
        amt_year: i32::MIN,
        ..Lot::default()
    };
    st.n_lots = 1;
    let all = run(&free, &mut st, 0..=3, |c| c.u_tender = 0.999_999);
    let (_, e) = all.first(EqEventKind::Tender).unwrap();
    assert_eq!((e.units, e.amount, st.lot_shares()), (0.0, 0.0, 100.0));
}

#[test]
fn the_tender_rates_average_to_the_chance_entered() {
    // `tender_exposure` against the fates themselves: months spent private on a
    // path heading to or above the last round, and on one heading below it.
    const N: u64 = 200_000;
    let p = params();
    let mut rates = Vec::new();
    for stage in CompanyStage::ALL {
        let table = typical(stage, &p);
        let [up, down] = tender_exposure(&table, &p);
        let (mut up_drawn, mut down_drawn) = (0.0, 0.0);
        for path in 0..N {
            let f = draw_fate(&table, &p, &mut stream(path));
            if f.kind == OutcomeKind::StillPrivate {
                continue;
            }
            if f.m_end >= 1.0 {
                up_drawn += f64::from(f.month);
            } else {
                down_drawn += f64::from(f.month);
            }
        }
        let (up_drawn, down_drawn) = (up_drawn / N as f64, down_drawn / N as f64);
        assert!(
            (up - up_drawn).abs() <= 0.03 * up_drawn + 0.05,
            "{}: {up:.3} months heading up by formula, {up_drawn:.3} drawn",
            stage.key()
        );
        assert!(
            (down - down_drawn).abs() <= 0.03 * down_drawn + 0.05,
            "{}: {down:.3} months heading down by formula, {down_drawn:.3} drawn",
            stage.key()
        );
        // Over those company-months the rate averages to the stage's chance.
        let chance = p.tender_probability[stage.index()];
        let high = tender_rate(&table, &p, chance);
        assert_close(high * up / (up + down), chance, "average rate");
        assert!(high > chance && high < 0.5, "{}: {high}", stage.key());
        rates.push((high * 1000.0).round() / 10.0);
    }
    // The yearly chance where tenders happen, in percent, as params.json quotes it.
    assert_eq!(rates, [3.2, 2.3, 5.6, 13.2, 22.3, 36.8]);
    // Capped at 1; nothing to divide by when no row has a price.
    let table = typical(CompanyStage::Seed, &p);
    assert_eq!(tender_rate(&table, &p, 0.9), 1.0);
    // What is delivered then is less than what was entered, and the preview says so.
    let [up, down] = tender_exposure(&table, &p);
    assert_close(
        tender_probability_met(&table, &p, 0.9),
        up / (up + down),
        "met",
    );
    assert!(tender_probability_met(&table, &p, 0.9) < 0.5);
    assert_eq!(tender_probability_met(&table, &p, 0.01), 0.01);
    let mut capped = company();
    capped.tenders.annual_probability = Some(1.0);
    let shown = preview(&capped, &p, 200).tender_annual_probability;
    let capped_table = resolved_table(&capped, &p);
    assert_close(
        shown,
        tender_probability_met(&capped_table, &p, 1.0),
        "preview",
    );
    assert!(shown < 0.6, "{shown}");
    assert_eq!(tender_rate(&table, &p, 0.0), 0.0);
    assert_eq!(tender_rate(&table, &p, f64::NAN), 0.0);
    let mut c = company();
    c.outcomes = vec![row(OutcomeKind::StillPrivate, 1.0, (0.0, 0.0), (1.0, 1.0))];
    let only_private = resolved_table(&c, &p);
    assert_eq!(tender_exposure(&only_private, &p), [0.0, 0.0]);
    assert_eq!(tender_rate(&only_private, &p, 0.2), 0.2);
    // A table with no failure row reports a failure probability of plain zero.
    assert!(only_private
        .probability_of(OutcomeKind::Fails)
        .is_sign_positive());
    assert!(ResolvedTable::default()
        .still_private_probability()
        .is_sign_positive());
}

#[test]
fn a_planned_tender_happens_for_certain() {
    let mut c = tender_company(0.0);
    c.tenders.next_in_months = Some(4);
    let m = compile(&c);
    let mut rng = stream(23);
    // A failing company, the window shut: the announced tender still happens.
    let failing = fate(OutcomeKind::Fails, 10, 1.0, &m);
    let months = tender_months(&m, failing, 0..=20, &mut rng, false);
    assert_eq!(months.len(), 1);
    assert_eq!(months[0].0, 4);
    assert_close(months[0].1, m.price_at(&failing, 4), "priced on the path");
    // Not if the company's event comes first.
    let sooner = fate(OutcomeKind::Fails, 3, 1.0, &m);
    assert!(tender_months(&m, sooner, 0..=20, &mut rng, false).is_empty());
    // Nor in the month of the event itself.
    let same = fate(OutcomeKind::Acquired, 4, 2.0, &m);
    assert!(tender_months(&m, same, 0..=20, &mut rng, false).is_empty());

    // A planned tender counts for the gap of later ones.
    let mut c = tender_company(1.0);
    c.tenders.next_in_months = Some(6);
    let m = compile(&c);
    let months = tender_months(&m, level(&m), 0..=30, &mut rng, true);
    assert_eq!(
        months.iter().map(|(t, _)| *t).collect::<Vec<_>>(),
        [1, 6, 18, 30]
    );
}

fn synthetic_economy(months: usize) -> EconomyPath {
    EconomyPath {
        months,
        returns: std::array::from_fn::<_, N_ASSETS, _>(|_| vec![0.0; months]),
        recession: vec![0; months],
        ..EconomyPath::default()
    }
}

#[test]
fn an_ipo_month_avoids_a_shut_window() {
    let p = params();
    let shut = |econ: &mut EconomyPath, from: usize, to: usize| {
        econ.recession[from..=to].fill(1);
    };
    // An open month is kept.
    let mut econ = synthetic_economy(400);
    shut(&mut econ, 15, 30);
    for month in [1, 14, 31, 399] {
        assert_eq!(ipo_month(&econ, month, &p), month);
    }
    // A month in a shut spell moves to the nearer open edge.
    assert_eq!(ipo_month(&econ, 20, &p), 14);
    assert_eq!(ipo_month(&econ, 26, &p), 31);
    // The earlier one on a tie (months 15 to 25 shut, month 20 is six from both).
    let mut econ = synthetic_economy(400);
    shut(&mut econ, 15, 25);
    assert_eq!(ipo_month(&econ, 20, &p), 14);
    // A spell longer than the search bound on both sides leaves the month alone.
    let mut econ = synthetic_economy(400);
    shut(&mut econ, 30, 170);
    assert_eq!(ipo_month(&econ, 100, &p), 100);
    assert_eq!(ipo_month(&econ, 89, &p), 29);
    assert_eq!(ipo_month(&econ, 111, &p), 171);
    // Never before month 1.
    let mut econ = synthetic_economy(400);
    shut(&mut econ, 1, 5);
    assert_eq!(ipo_month(&econ, 2, &p), 6);
    // The stock index shuts the window too: a 30% fall in month 10, flat after.
    let mut econ = synthetic_economy(400);
    econ.returns[AssetClass::UsStocks.index()][10] = -0.30;
    assert_eq!(ipo_month(&econ, 9, &p), 9);
    assert_eq!(ipo_month(&econ, 12, &p), 9);
    assert_eq!(ipo_month(&econ, 200, &p), 200);
    // The test is the one the monthly step is given.
    assert!(window_open(0, 0.81, 1.0, &p));
    assert!(!window_open(0, 0.79, 1.0, &p));
    assert!(!window_open(1, 1.0, 1.0, &p));
    // A month beyond the path is left alone.
    assert_eq!(ipo_month(&econ, 400, &p), 400);
    assert_eq!(ipo_month(&econ, 5_000, &p), 5_000);
}

#[test]
fn the_window_open_share_matches_the_data() {
    const PATHS: u64 = 2_000;
    const MONTHS: usize = 120;
    let p = params();
    let data = Data::embedded();
    let kinds = [
        MarketModel::default().kind,
        MarketKind::Regime,
        MarketKind::Lognormal,
    ];
    assert!(matches!(kinds[0], MarketKind::Bootstrap { .. }));
    for kind in kinds {
        let market = MarketModel {
            kind,
            ..MarketModel::default()
        };
        let model = EconomyModel::new(&market, data).with_assets(&PortfolioPolicy::default());
        let mut econ = EconomyPath::default();
        let mut open = 0usize;
        for path in 0..PATHS {
            let mut market_rng = Rng::stream(1, path, Stream::Market);
            let mut inflation_rng = Rng::stream(1, path, Stream::Inflation);
            model.generate(
                &mut EconomyStreams {
                    market: &mut market_rng,
                    inflation: &mut inflation_rng,
                },
                path,
                MONTHS,
                &mut econ,
            );
            let (mut index, mut peak) = (1.0_f64, 1.0_f64);
            for t in 0..MONTHS {
                index *= 1.0 + econ.returns[AssetClass::UsStocks.index()][t];
                peak = peak.max(index);
                open += usize::from(window_open(econ.recession[t], index, peak, &p));
            }
        }
        let share = open as f64 / (PATHS as usize * MONTHS) as f64;
        assert!(
            (share - p.window_open_share).abs() <= 0.04,
            "{kind:?}: the window is open {share:.3} of the time, the data says {}",
            p.window_open_share
        );
    }
}

// ---------------------------------------------------------------------------
// Vesting and exercises
// ---------------------------------------------------------------------------

#[test]
fn vesting_arithmetic_with_a_cliff() {
    let mut c = company();
    c.holdings = vec![EquityHolding {
        cliff_months: 12,
        ..holding(nso(2.0), 48_000.0, 0.0, 48)
    }];
    let m = compile(&c);
    let mut st = state(&m, flat(&m));
    let mut vested = Vec::new();
    for t in 0..50 {
        m.step(&mut st, &climate(t));
        vested.push(st.holdings[0].vested);
        assert_close(
            st.holdings[0].vested,
            units_vested_by(&c.holdings[0], t),
            "closed form",
        );
        assert_close(
            st.holdings[0].vested + st.holdings[0].unvested,
            48_000.0,
            "total",
        );
    }
    assert!(
        vested[..11].iter().all(|v| *v == 0.0),
        "nothing for 11 months"
    );
    assert_close(vested[11], 12_000.0, "the cliff pays the accrued lump");
    for k in 12..48 {
        assert_close(vested[k] - vested[k - 1], 1_000.0, "then 1,000 a month");
    }
    assert_eq!(vested[47], 48_000.0);
    assert_eq!(vested[49], 48_000.0);
    assert_eq!(st.holdings[0].unvested, 0.0);

    // No cliff: even from the first month. Shares are all there from the start.
    let even = holding(rsu(), 1_200.0, 600.0, 6);
    assert_eq!(units_vested_by(&even, -1), 600.0);
    assert_close(units_vested_by(&even, 0), 700.0, "first month");
    assert_eq!(units_vested_by(&even, 5), 1_200.0);
    assert_eq!(
        units_vested_by(&holding(shares(1.0, 0), 50.0, 0.0, 0), 0),
        50.0
    );
}

fn exercise_now(kind: HoldingKind) -> (CompanyModel, CompanyState, EquityMonth) {
    let mut c = company();
    c.holdings = vec![holding(kind, 1_000.0, 1_000.0, 0)];
    c.policy.exercise = ExerciseTiming::VestedNow;
    let m = compile(&c);
    let mut st = state(&m, flat(&m));
    let mut cl = climate(0);
    cl.wage_tax_rate = 0.30;
    cl.wage_withheld_rate = 0.30;
    cl.iso_tax_rate = 0.28;
    let em = m.step(&mut st, &cl);
    (m, st, em)
}

#[test]
fn exercise_cost_is_conserved() {
    for kind in [nso(1.0), iso(1.0)] {
        let (_, st, em) = exercise_now(kind);
        assert_eq!(em.exercise_cost, 1_000.0);
        assert_eq!(st.holdings[0].vested, 0.0);
        assert_eq!(st.n_lots, 1);
        assert_eq!(st.lots[0].shares, 1_000.0);
        assert_eq!(em.basis_in, st.lots[0].cost);
        assert_eq!(em.basis_out, 0.0);
        assert_eq!(em.cash_in, 0.0);
        assert_eq!(em.units_out, 0.0);
        assert!(em.replan);
        let e = em.event(EqEventKind::Exercised).unwrap();
        assert_eq!((e.units, e.price, e.amount), (1_000.0, 1.0, 1_000.0));
        assert_eq!(st.lots[0].acquired, 0);
        assert_eq!(st.lots[0].granted, -30);
        // Held shares are paper value at the last round's price.
        assert_eq!(em.paper_value, 10_000.0);
    }
}

#[test]
fn nso_exercise_is_wages_on_the_409a_spread() {
    let (_, st, em) = exercise_now(nso(1.0));
    assert_eq!(em.wages_noncash, 1_000.0 * (4.0 - 1.0));
    assert_eq!(em.wages_cash, 0.0);
    assert_eq!(st.lots[0].cost, 1_000.0 * 4.0);
    assert_eq!(st.lots[0].amt_extra, 0.0);
    assert!(!st.lots[0].iso);
    assert_eq!(st.lots[0].amt_year, i32::MIN);
    assert_eq!((em.amt_adjustment, em.tax_estimate), (0.0, 0.0));
    // A strike above the 409A value (and below the round): no spread, basis the strike.
    let (_, st, em) = exercise_now(nso(6.0));
    assert_eq!(em.wages_noncash, 0.0);
    assert_eq!(st.lots[0].cost, 6_000.0);
}

#[test]
fn iso_exercise_is_an_amt_adjustment_and_no_wages() {
    let (_, st, em) = exercise_now(iso(1.0));
    assert_eq!(em.amt_adjustment, 1_000.0 * (4.0 - 1.0));
    assert_eq!(
        (em.wages_noncash, em.wages_cash, em.iso_income),
        (0.0, 0.0, 0.0)
    );
    assert_eq!(st.lots[0].cost, 1_000.0);
    assert_eq!(st.lots[0].amt_extra, 3_000.0);
    assert!(st.lots[0].iso);
    assert_eq!(st.lots[0].amt_year, 2026);
    assert_close(em.tax_estimate, 0.28 * 3_000.0, "AMT estimate");
}

/// A model and state holding one ISO lot of 1,000 shares: strike 2, value at
/// exercise 5, acquired at month 0, granted at `granted`.
fn iso_lot(granted: i32, amt_year: i32) -> (CompanyModel, CompanyState) {
    let m = compile(&company());
    let mut st = state(&m, flat(&m));
    st.lots[0] = Lot {
        shares: 1_000.0,
        cost: 2_000.0,
        amt_extra: 3_000.0,
        acquired: 0,
        granted,
        amt_year,
        iso: true,
        qsbs: false,
    };
    st.n_lots = 1;
    (m, st)
}

fn sell_all_at(m: &CompanyModel, st: &mut CompanyState, t: i32, price: f64) -> EquityMonth {
    let mut em = EquityMonth::default();
    let shares = st.lots[0].shares;
    m.sell_from_lot(st, 0, shares, price, &climate(t), &mut em);
    em
}

#[test]
fn iso_sales_qualifying_and_disqualifying() {
    // Held exactly 12 months: disqualifying and short-term. 13: qualifying, long-term.
    let (m, mut st) = iso_lot(-30, 2026);
    let em = sell_all_at(&m, &mut st, 12, 9.0);
    assert_eq!(em.iso_income, 3_000.0);
    assert_eq!((em.short_term_gain, em.long_term_gain), (4_000.0, 0.0));
    let (m, mut st) = iso_lot(-30, 2026);
    let em = sell_all_at(&m, &mut st, 13, 9.0);
    assert_eq!(em.iso_income, 0.0);
    assert_eq!((em.short_term_gain, em.long_term_gain), (0.0, 7_000.0));

    // Granted 24 months before the sale: disqualifying (but long-term). 25: qualifying.
    let (m, mut st) = iso_lot(-4, 2026);
    let em = sell_all_at(&m, &mut st, 20, 9.0);
    assert_eq!(em.iso_income, 3_000.0);
    assert_eq!((em.short_term_gain, em.long_term_gain), (0.0, 4_000.0));
    let (m, mut st) = iso_lot(-5, 2026);
    let em = sell_all_at(&m, &mut st, 20, 9.0);
    assert_eq!(em.iso_income, 0.0);
    assert_eq!(em.long_term_gain, 7_000.0);

    // Disqualifying, by price. Above the value at exercise: the spread is ordinary
    // and the rest a capital gain.
    let (m, mut st) = iso_lot(-30, 2026);
    let em = sell_all_at(&m, &mut st, 6, 9.0);
    assert_eq!((em.iso_income, em.short_term_gain), (3_000.0, 4_000.0));
    // Between the strike and that value: the gain is ordinary, no capital gain.
    let (m, mut st) = iso_lot(-30, 2026);
    let em = sell_all_at(&m, &mut st, 6, 3.5);
    assert_eq!((em.iso_income, em.short_term_gain), (1_500.0, 0.0));
    // Below the strike: no ordinary income, a capital loss.
    let (m, mut st) = iso_lot(-30, 2026);
    let em = sell_all_at(&m, &mut st, 6, 1.5);
    assert_eq!((em.iso_income, em.short_term_gain), (0.0, -500.0));

    // Every sale: the cash is the proceeds, counted once; the basis leaves.
    assert_eq!((em.share_proceeds, em.cash_in), (1_500.0, 1_500.0));
    assert_eq!(em.basis_out, 2_000.0);
    assert_eq!(
        (st.lots[0].shares, st.lots[0].cost, st.lots[0].amt_extra),
        (0.0, 0.0, 0.0)
    );

    // A partial sale takes the basis and the AMT basis pro rata.
    let (m, mut st) = iso_lot(-30, 2026);
    let mut em = EquityMonth::default();
    m.sell_from_lot(&mut st, 0, 250.0, 9.0, &climate(6), &mut em);
    assert_eq!(
        (em.iso_income, em.short_term_gain, em.basis_out),
        (750.0, 1_000.0, 500.0)
    );
    assert_eq!(
        (st.lots[0].shares, st.lots[0].cost, st.lots[0].amt_extra),
        (750.0, 1_500.0, 2_250.0)
    );
}

#[test]
fn a_same_year_iso_sale_reverses_the_adjustment() {
    // Exercised at the start (November 2026) and sold in December: no AMT income is
    // left, and the sale is a disqualifying disposition.
    let mut c = company();
    c.holdings = vec![holding(iso(1.0), 1_000.0, 1_000.0, 0)];
    c.policy.exercise = ExerciseTiming::VestedNow;
    let m = compile(&c);
    let mut st = state(&m, fate(OutcomeKind::Acquired, 1, 2.0, &m));
    let year = run(&m, &mut st, 0..=1, |_| {});
    assert_eq!(year.em.amt_adjustment, 0.0);
    assert_eq!(year.em.amt_gain_adjustment, 0.0);
    // The spread at exercise was 3 a share; the sale at 20 leaves 16 of capital gain.
    assert_eq!(year.em.iso_income, 3_000.0);
    assert_eq!(year.em.short_term_gain, 16_000.0);

    // A qualifying sale in a later year lowers the AMT gain by min(spread, gain) and
    // leaves ordinary AMT income alone.
    let (m, mut st) = iso_lot(-30, 2026);
    let em = sell_all_at(&m, &mut st, 14, 9.0);
    assert_eq!((em.amt_gain_adjustment, em.amt_adjustment), (-3_000.0, 0.0));
    let (m, mut st) = iso_lot(-30, 2026);
    let em = sell_all_at(&m, &mut st, 14, 3.5);
    assert_eq!((em.amt_gain_adjustment, em.amt_adjustment), (-1_500.0, 0.0));
    // A later-year disqualifying sale: the reverse.
    let (m, mut st) = iso_lot(-30, 2026);
    let em = sell_all_at(&m, &mut st, 6, 9.0);
    assert_eq!((em.amt_gain_adjustment, em.amt_adjustment), (0.0, -3_000.0));
    // Sold at a loss in a later year: nothing is released.
    let (m, mut st) = iso_lot(-30, 2026);
    let em = sell_all_at(&m, &mut st, 14, 1.5);
    assert_eq!((em.amt_gain_adjustment, em.amt_adjustment), (0.0, 0.0));
    assert_eq!(em.long_term_gain, -500.0);
    // A lot with no AMT basis makes no adjustment.
    let (m, mut st) = iso_lot(-30, i32::MIN);
    st.lots[0].amt_extra = 0.0;
    let em = sell_all_at(&m, &mut st, 14, 9.0);
    assert_eq!((em.amt_gain_adjustment, em.amt_adjustment), (0.0, 0.0));
}

#[test]
fn iso_shares_exercised_earlier_this_year_count_for_amt() {
    // Held 3 months at a November start: exercised in August of the same year.
    let mut c = company();
    c.holdings = vec![holding(iso_shares(1.5, 3, 4.0), 1_000.0, 0.0, 0)];
    let m = compile(&c);
    assert_eq!(m.start_amt_adjustment(), 2_500.0);
    let mut st = state(&m, fate(OutcomeKind::Acquired, 1, 2.0, &m));
    assert_eq!(st.lots[0].amt_year, 2026);
    assert_eq!(st.lots[0].amt_extra, 2_500.0);
    // Sold in December: the adjustment is undone, so the year nets to zero.
    let sold = run(&m, &mut st, 0..=1, |_| {});
    assert_eq!(m.start_amt_adjustment() + sold.em.amt_adjustment, 0.0);
    assert_eq!(sold.em.iso_income, 2_500.0);

    // Held 10 months: exercised in January, still this year. 11: last December.
    c.holdings = vec![holding(iso_shares(1.5, 10, 4.0), 1_000.0, 0.0, 0)];
    assert_eq!(compile(&c).start_amt_adjustment(), 2_500.0);
    c.holdings = vec![holding(iso_shares(1.5, 11, 4.0), 1_000.0, 0.0, 0)];
    let m = compile(&c);
    assert_eq!(m.start_amt_adjustment(), 0.0);
    let st = state(&m, flat(&m));
    assert_eq!(st.lots[0].amt_year, i32::MIN);
    assert_eq!(st.lots[0].amt_extra, 2_500.0);
    // Shares that did not come from incentive options carry no AMT basis.
    c.holdings = vec![HoldingKind::Shares {
        basis_per_share: 1.5,
        months_held: 3,
        from_iso: false,
        fmv_at_exercise: 4.0,
    }]
    .into_iter()
    .map(|k| holding(k, 1_000.0, 0.0, 0))
    .collect();
    let m = compile(&c);
    assert_eq!(m.start_amt_adjustment(), 0.0);
    assert_eq!(m.lots[0].amt_extra, 0.0);
    // Not counted at all when the company is not included.
    c.included = false;
    let m = compile(&c);
    assert_eq!((m.n_lots, m.n_holdings), (0, 0));
}

#[test]
fn an_iso_exercised_late_is_taxed_as_an_nso() {
    // A twelve-month window: the deadline exercise comes twelve months after leaving.
    let mut c = company();
    c.holdings = vec![holding(iso(1.0), 1_000.0, 1_000.0, 0)];
    c.exercise_window_months = 12;
    c.policy.leave_after_months = Some(2);
    let m = compile(&c);
    let mut st = state(&m, flat(&m));
    let before = run(&m, &mut st, 0..=13, |_| {});
    assert_eq!(before.em.exercise_cost, 0.0);
    assert_eq!(
        (st.link, st.severed_month, st.deadline),
        (Link::Severed, 2, 14)
    );
    let at = run(&m, &mut st, 14..=14, |_| {});
    assert_eq!(at.em.exercise_cost, 1_000.0);
    assert_eq!(at.em.wages_noncash, 3_000.0);
    assert_eq!((at.em.amt_adjustment, at.em.tax_estimate), (0.0, 0.0));
    assert!(!st.lots[0].iso);
    assert_eq!(st.lots[0].cost, 4_000.0);

    // With the usual three months the same exercise keeps its treatment.
    c.exercise_window_months = 3;
    let m = compile(&c);
    let mut st = state(&m, flat(&m));
    let all = run(&m, &mut st, 0..=5, |_| {});
    assert_eq!(all.first(EqEventKind::Exercised).unwrap().0, 5);
    assert_eq!(
        (all.em.wages_noncash, all.em.amt_adjustment),
        (0.0, 3_000.0)
    );
    assert!(st.lots[0].iso);

    // A former employer with two months left exercises ISOs as ISOs.
    c.job = None;
    c.policy.leave_after_months = None;
    c.exercise_window_months = 2;
    let m = compile(&c);
    let mut st = m.initial_state(Link::Active);
    st.fate = flat(&m);
    assert_eq!(
        (st.link, st.severed_month, st.deadline),
        (Link::Severed, 0, 2)
    );
    let all = run(&m, &mut st, 0..=2, |c| c.employed = false);
    assert_eq!(all.first(EqEventKind::Exercised).unwrap().0, 2);
    assert_eq!(
        (all.em.wages_noncash, all.em.amt_adjustment),
        (0.0, 3_000.0)
    );

    // One with six months left: non-qualified from month 0.
    c.exercise_window_months = 6;
    c.policy.exercise = ExerciseTiming::VestedNow;
    let m = compile(&c);
    let mut st = m.initial_state(Link::Pending);
    st.fate = flat(&m);
    assert_eq!(
        (st.link, st.severed_month, st.deadline),
        (Link::Severed, -4, 6)
    );
    let now = run(&m, &mut st, 0..=0, |c| c.employed = false);
    assert_eq!(
        (now.em.wages_noncash, now.em.amt_adjustment),
        (3_000.0, 0.0)
    );
    assert!(!st.lots[0].iso);
}

#[test]
fn exercise_decisions_do_not_see_the_price_path() {
    let mut c = company();
    c.holdings = vec![
        holding(nso(2.0), 1_000.0, 1_000.0, 0),
        holding(iso(3.0), 500.0, 500.0, 0),
        // Struck above the last round: never exercised for cash, whatever is coming.
        holding(nso(12.0), 700.0, 700.0, 0),
    ];
    c.policy.leave_after_months = Some(6);
    let m = compile(&c);
    let fates = [
        fate(OutcomeKind::Fails, 60, 1.0, &m),
        fate(OutcomeKind::Ipo, 60, 5.0, &m),
    ];
    // Savings that cover part of the strike: the affordable share is the same.
    for liquid in [AMPLE, 1_750.0] {
        let mut exercised = Vec::new();
        for f in fates {
            let mut st = state(&m, f);
            let all = run(&m, &mut st, 0..=20, |c| c.liquid = liquid);
            let (month, e) = all.first(EqEventKind::Exercised).unwrap();
            assert_eq!(month, 9);
            assert_eq!(
                st.holdings[2].vested, 0.0,
                "the option above the round lapsed"
            );
            assert_eq!(all.first(EqEventKind::Lapsed).unwrap().0, 9);
            exercised.push((e.units, e.amount, st.lot_shares()));
        }
        assert_eq!(exercised[0], exercised[1]);
        if liquid == AMPLE {
            assert_eq!(exercised[0], (1_500.0, 3_500.0, 1_500.0));
        } else {
            assert_close(exercised[0].1, 1_750.0, "strike paid");
        }
    }
    // The same holds for exercising now and as options vest.
    c.policy.leave_after_months = None;
    c.policy.exercise = ExerciseTiming::AsTheyVest;
    c.holdings.push(holding(nso(12.0), 240.0, 0.0, 24));
    c.holdings.push(holding(nso(1.0), 240.0, 0.0, 24));
    let m = compile(&c);
    let mut units = Vec::new();
    for kind in [OutcomeKind::Fails, OutcomeKind::Ipo] {
        let mut st = state(&m, fate(kind, 60, 5.0, &m));
        let all = run(&m, &mut st, 0..=30, |_| {});
        assert_eq!(st.holdings[2].vested, 700.0);
        assert_eq!(st.holdings[3].vested, 240.0);
        assert_eq!(st.holdings[4].vested, 0.0);
        units.push(all.em.exercise_cost);
    }
    assert_eq!(units[0], units[1]);
    assert_eq!(units[0], 2_000.0 + 1_500.0 + 240.0);
}

#[test]
fn a_deadline_exercise_buys_what_is_affordable_and_the_rest_lapses() {
    let deadline = |kind: HoldingKind, policy: DeadlinePolicy, liquid: f64| {
        let mut c = company();
        c.holdings = vec![holding(kind, 1_000.0, 1_000.0, 0)];
        c.policy.leave_after_months = Some(1);
        c.policy.at_deadline = policy;
        let m = compile(&c);
        let mut st = state(&m, flat(&m));
        let all = run(&m, &mut st, 0..=4, |c| {
            c.liquid = liquid;
            // All of the tax on pay is taken at the event here.
            c.wage_tax_rate = 0.40;
            c.wage_withheld_rate = 0.40;
            c.iso_tax_rate = 0.28;
        });
        (all, st)
    };
    // NSO: each option needs the strike of 2 plus 40% of the spread of 2.
    let (all, st) = deadline(nso(2.0), DeadlinePolicy::Exercise, 0.4 * 2_800.0);
    assert_eq!(all.first(EqEventKind::Exercised).unwrap().0, 4);
    assert_close(all.em.exercise_cost, 800.0, "40% of the strike");
    assert_close(st.lot_shares(), 400.0, "40% exercised");
    assert_close(all.em.wages_noncash, 800.0, "spread on 400 options");
    assert_eq!(all.em.tax_estimate, 0.0);
    let (month, lapsed) = all.first(EqEventKind::Lapsed).unwrap();
    assert_eq!(month, 4);
    assert_close(lapsed.units, 600.0, "the rest lapse");
    assert_close(lapsed.amount, 600.0 * 8.0, "paper value given up");
    assert_eq!(st.holdings[0].vested, 0.0);
    assert!(all.em.replan);

    // ISO: the strike plus 28% of the spread.
    let (all, st) = deadline(iso(2.0), DeadlinePolicy::Exercise, 0.4 * 2_560.0);
    assert_close(st.lot_shares(), 400.0, "40% exercised");
    assert_close(
        all.em.tax_estimate,
        0.28 * 800.0,
        "the AMT estimate on the ISO part",
    );
    assert_close(all.em.amt_adjustment, 800.0, "spread on 400 options");
    assert_eq!(all.em.wages_noncash, 0.0);

    // With enough, everything is exercised and nothing lapses; with nothing, the
    // reverse.
    let (all, st) = deadline(nso(2.0), DeadlinePolicy::Exercise, 2_800.0);
    assert_eq!(st.lot_shares(), 1_000.0);
    assert_eq!(all.count(EqEventKind::Lapsed), 0);
    let (all, st) = deadline(nso(2.0), DeadlinePolicy::Exercise, 0.0);
    assert_eq!((st.lot_shares(), all.em.exercise_cost), (0.0, 0.0));
    assert_eq!(all.first(EqEventKind::Lapsed).unwrap().1.units, 1_000.0);

    // "Forfeit" exercises none, however much cash there is.
    let (all, st) = deadline(nso(2.0), DeadlinePolicy::Forfeit, AMPLE);
    assert_eq!((st.lot_shares(), all.em.exercise_cost), (0.0, 0.0));
    assert_eq!(all.count(EqEventKind::Exercised), 0);
    assert_eq!(all.first(EqEventKind::Lapsed).unwrap().1.units, 1_000.0);
    assert!(!all.em.replan);
}

#[test]
fn options_lapse_ten_years_after_the_grant() {
    let mut c = company();
    c.holdings = vec![holding(
        HoldingKind::Nso {
            strike: 2.0,
            granted_months_ago: 100,
        },
        1_000.0,
        1_000.0,
        0,
    )];
    c.policy.at_deadline = DeadlinePolicy::Forfeit;
    let m = compile(&c);
    assert_eq!(m.holdings[0].term_month, 20);
    let mut st = state(&m, flat(&m));
    let all = run(&m, &mut st, 0..=25, |_| {});
    assert_eq!(st.link, Link::Active, "still employed");
    let (month, e) = all.first(EqEventKind::Expired).unwrap();
    assert_eq!((month, e.units, e.vested), (20, 1_000.0, 1_000.0));
    assert_eq!(all.events.len(), 1);

    // What has not vested by the term lapses with it, and the event says how much
    // of each: here 16 months of vesting are left when the term comes after 20
    // steps of 25 a month.
    c.holdings[0].vested = 100.0;
    c.holdings[0].vest_months_left = 36;
    let m = compile(&c);
    let mut st = state(&m, flat(&m));
    let all = run(&m, &mut st, 0..=25, |_| {});
    let (month, e) = all.first(EqEventKind::Expired).unwrap();
    assert_eq!(month, 20);
    assert_close(e.vested, 100.0 + 21.0 * 25.0, "vested at the term");
    assert_close(e.units, 1_000.0, "the whole grant");
    assert_eq!(all.count(EqEventKind::Lapsed), 0);
    assert_eq!(st.total_units(), 0.0);
    c.holdings[0].vested = 1_000.0;
    c.holdings[0].vest_months_left = 0;

    // Under the default policy they are exercised that month.
    c.policy.at_deadline = DeadlinePolicy::Exercise;
    let m = compile(&c);
    let mut st = state(&m, flat(&m));
    let all = run(&m, &mut st, 0..=25, |_| {});
    assert_eq!(all.first(EqEventKind::Exercised).unwrap().0, 20);
    assert_eq!(all.count(EqEventKind::Lapsed), 0);
    assert_eq!(all.count(EqEventKind::Expired), 0);
    assert_eq!(st.lot_shares(), 1_000.0);
    assert_eq!(st.lots[0].granted, -100);
}

// ---------------------------------------------------------------------------
// Liquidity
// ---------------------------------------------------------------------------

fn tender_holdings() -> Vec<EquityHolding> {
    vec![
        holding(shares(1.0, 20), 1_000.0, 0.0, 0),
        holding(nso(2.0), 2_000.0, 2_000.0, 0),
        holding(iso(3.0), 3_000.0, 3_000.0, 0),
        holding(nso(50.0), 5_000.0, 5_000.0, 0),
        holding(rsu(), 4_000.0, 4_000.0, 0),
    ]
}

#[test]
fn a_tender_sells_lots_first_then_nsos_then_isos_up_to_the_cap() {
    let tender = |max_fraction: f64, sell: f64, job: bool, former: bool| {
        let mut c = tender_company(0.0);
        c.holdings = tender_holdings();
        c.tenders.next_in_months = Some(2);
        c.tenders.max_fraction = max_fraction;
        c.tenders.former_employees = former;
        c.policy.tender_sell_fraction = sell;
        c.policy.at_deadline = DeadlinePolicy::Forfeit;
        if !job {
            c.job = None;
        }
        let m = compile(&c);
        let mut st = m.initial_state(Link::Active);
        st.fate = flat(&m);
        let all = run(&m, &mut st, 0..=2, |c| c.employed = job);
        (all, st)
    };
    // Eligible: 1,000 shares and 5,000 options in the money. Half of a 20% cap: 600.
    let (all, st) = tender(0.20, 0.5, true, false);
    let (month, e) = all.first(EqEventKind::Tender).unwrap();
    assert_eq!(month, 2);
    assert_close(e.units, 600.0, "fraction times cap times eligible");
    assert_eq!(e.price, 10.0);
    assert_close(st.lot_shares(), 400.0, "lots sell first");
    assert_eq!(
        (st.holdings[1].vested, st.holdings[2].vested),
        (2_000.0, 3_000.0)
    );
    assert_close(all.em.share_proceeds, 6_000.0, "shares at the tender price");
    assert_close(all.em.tender_proceeds, 6_000.0, "all of it from the tender");
    assert_close(all.em.long_term_gain, 600.0 * 9.0, "gain on the shares");
    assert!(all.em.replan);

    // A cap of 3,000: the 1,000 shares, then the 2,000 NSOs, and no ISO.
    let (all, st) = tender(1.0, 0.5, true, false);
    assert_eq!(st.n_lots, 0);
    assert_eq!(
        (st.holdings[1].vested, st.holdings[2].vested),
        (0.0, 3_000.0)
    );
    assert_close(all.em.wages_cash, 2_000.0 * 8.0, "NSOs sold: wages");
    assert_eq!(all.em.iso_income, 0.0);
    // A cap of 4,500: 1,500 of the ISOs too, as income without payroll tax.
    let (all, st) = tender(1.0, 0.75, true, false);
    assert_close(st.holdings[2].vested, 1_500.0, "ISOs last");
    assert_close(all.em.iso_income, 1_500.0 * 7.0, "same-day ISO sale");
    assert_close(all.em.wages_cash, 16_000.0, "NSOs");
    assert_close(all.em.cash_in, 10_000.0 + 16_000.0 + 10_500.0, "cash");
    assert_close(all.em.tender_proceeds, all.em.cash_in, "tender proceeds");
    assert_eq!(all.em.exercise_cost, 0.0, "a cashless sale");
    assert_eq!(all.em.amt_adjustment, 0.0);
    // RSUs cannot be tendered and options under water are skipped.
    assert_eq!(
        (st.holdings[3].vested, st.holdings[4].vested),
        (5_000.0, 4_000.0)
    );

    // Someone who has left sells nothing unless the company lets them.
    let (all, st) = tender(1.0, 0.75, false, false);
    let e = all.first(EqEventKind::Tender).unwrap().1;
    assert_eq!((e.units, e.amount), (0.0, 0.0));
    assert_eq!((all.em.cash_in, st.lot_shares()), (0.0, 1_000.0));
    let (all, _) = tender(1.0, 0.75, false, true);
    assert_close(
        all.first(EqEventKind::Tender).unwrap().1.units,
        4_500.0,
        "allowed",
    );
    // Selling nothing by choice: the tender is still logged.
    let (all, _) = tender(0.20, 0.0, true, false);
    assert_eq!(all.first(EqEventKind::Tender).unwrap().1.units, 0.0);
    assert!(!all.em.replan);
}

#[test]
fn an_acquisition_pays_everything_out() {
    let mut c = company();
    c.holdings = vec![
        holding(shares(1.0, 20), 1_000.0, 0.0, 0),
        holding(nso(2.0), 4_800.0, 2_400.0, 24),
        holding(iso(3.0), 1_000.0, 1_000.0, 0),
        holding(nso(50.0), 500.0, 500.0, 0),
        holding(rsu(), 100.0, 100.0, 0),
    ];
    let m = compile(&c);
    let mut st = state(&m, fate(OutcomeKind::Acquired, 12, 2.0, &m));
    let before = run(&m, &mut st, 0..=11, |_| {});
    assert_eq!(before.em.cash_in, 0.0);
    let at = run(&m, &mut st, 12..=12, |_| {});
    // Thirteen months of vesting at 100 a month: 3,700 vested, 1,100 to go.
    assert_close(
        at.em.share_proceeds,
        20_000.0,
        "lots sold at the deal price",
    );
    assert_close(at.em.long_term_gain, 19_000.0, "gain on the lots");
    let wages = 3_700.0 * 18.0 + 1_000.0 * 17.0 + 100.0 * 20.0;
    assert_close(
        at.em.wages_cash,
        wages,
        "options and RSUs paid as wages, ISOs too",
    );
    assert_eq!(at.em.iso_income, 0.0);
    assert_close(at.em.cash_in, 20_000.0 + wages, "cash");
    assert!(at.em.replan && !at.em.failed_now);
    assert_eq!(st.status, CompanyStatus::Acquired);
    assert_eq!(st.outcome, Some(EquityOutcome::Acquired));
    assert_eq!(st.event_month, 12);
    assert_eq!(
        st.holdings[3].vested, 0.0,
        "under-water options are cancelled"
    );
    let e = at.first(EqEventKind::Acquired).unwrap().1;
    assert_eq!(e.price, 20.0);
    assert_close(e.amount, at.em.cash_in, "event cash");
    assert_close(
        e.units,
        1_000.0 + 3_700.0 + 1_000.0 + 500.0 + 100.0,
        "event units",
    );
    assert_eq!(at.em.paper_value, 0.0, "nothing vested is left");

    // What is unvested keeps vesting and is paid at the deal price while the link lasts.
    let next = run(&m, &mut st, 13..=14, |_| {});
    assert_close(next.em.wages_cash, 200.0 * 18.0, "two months of vesting");
    assert!(!next.em.replan);
    assert_eq!(st.price, 20.0);
    // It stops when the link ends.
    let end = run(&m, &mut st, 15..=20, |c| c.employed = false);
    assert_eq!(end.em.cash_in, 0.0);
    assert_close(
        end.first(EqEventKind::Forfeited).unwrap().1.units,
        900.0,
        "forfeited",
    );
    assert_eq!(st.status, CompanyStatus::Done);

    // With nothing unvested the company is done at once.
    c.holdings.remove(1);
    let m = compile(&c);
    let mut st = state(&m, fate(OutcomeKind::Acquired, 12, 2.0, &m));
    run(&m, &mut st, 0..=12, |_| {});
    assert_eq!(st.status, CompanyStatus::Done);
    assert_eq!(st.total_units(), 0.0);

    // A sale that pays common nothing: the basis is a capital loss, nothing is paid.
    let mut st = state(&m, fate(OutcomeKind::Acquired, 12, 0.1, &m));
    let all = run(&m, &mut st, 0..=12, |_| {});
    assert_eq!(all.em.cash_in, 0.0);
    assert_eq!(all.em.long_term_gain, -1_000.0);
    assert_eq!(st.status, CompanyStatus::Done);
    assert!(!all.em.failed_now);
}

#[test]
fn an_ipo_lists_in_its_fate_month() {
    let mut c = company();
    c.holdings = vec![holding(nso(2.0), 1_000.0, 1_000.0, 0)];
    let m = compile(&c);
    // The window is shut all along: the month was placed at path start.
    let mut st = state(&m, fate(OutcomeKind::Ipo, 10, 3.0, &m));
    let before = run(&m, &mut st, 0..=9, |c| c.window_open = false);
    assert_eq!(st.status, CompanyStatus::Private);
    assert_eq!(before.events.len(), 0);
    let at = run(&m, &mut st, 10..=10, |c| c.window_open = false);
    assert_eq!(st.status, CompanyStatus::Lockup);
    assert_eq!(st.outcome, Some(EquityOutcome::Ipo));
    assert_eq!((st.event_month, st.lockup_end), (10, 16));
    assert_close(st.price, 30.0, "the IPO price");
    let e = at.first(EqEventKind::Ipo).unwrap().1;
    assert_close(e.price, 30.0, "event price");
    // Nothing is taxed or paid at the listing.
    assert_eq!(
        (at.em.cash_in, at.em.wages_cash, at.em.replan),
        (0.0, 0.0, false)
    );
    assert_close(
        at.em.paper_value,
        1_000.0 * 28.0,
        "paper value at the IPO price",
    );

    // `begin` places the month in an open window when it has an economy path.
    let mut econ = synthetic_economy(400);
    econ.recession[5..=200].fill(1);
    let mut ipo_only = company();
    ipo_only.outcomes = vec![row(OutcomeKind::Ipo, 1.0, (2.0, 2.0), (3.0, 3.0))];
    let m = compile(&ipo_only);
    assert_eq!(m.begin(Link::Active, &mut stream(0), None).fate.month, 24);
    assert_eq!(
        m.begin(Link::Active, &mut stream(0), Some(&econ))
            .fate
            .month,
        4
    );
    // Other outcomes are not moved.
    ipo_only.outcomes[0].kind = OutcomeKind::Acquired;
    let m = compile(&ipo_only);
    assert_eq!(
        m.begin(Link::Active, &mut stream(0), Some(&econ))
            .fate
            .month,
        24
    );
}

#[test]
fn lockup_end_settles_options_and_rsus_for_cash() {
    let mut c = company();
    c.holdings = vec![
        holding(nso(2.0), 1_000.0, 1_000.0, 0),
        holding(rsu(), 100.0, 100.0, 0),
        holding(iso(3.0), 1_000.0, 1_000.0, 0),
    ];
    let m = compile(&c);
    let mut f = fate(OutcomeKind::Ipo, 10, 3.0, &m);
    f.lockup = 1.2;
    let mut st = state(&m, f);
    let during = run(&m, &mut st, 0..=15, |_| {});
    assert_eq!(during.em.cash_in, 0.0);
    // In lockup the price moves from the IPO price toward the lockup-end price.
    assert_close(
        st.price,
        30.0 * math::pow(1.2, 5.0 / 6.0),
        "price in lockup",
    );
    let end = run(&m, &mut st, 16..=16, |_| {});
    assert_close(st.price, 36.0, "P0 times m times L");
    assert_close(
        end.em.wages_cash,
        1_000.0 * 34.0 + 100.0 * 36.0,
        "NSO and RSU as wages",
    );
    assert_close(
        end.em.iso_income,
        1_000.0 * 33.0,
        "ISO as income without payroll tax",
    );
    assert_close(
        end.em.cash_in,
        end.em.wages_cash + end.em.iso_income,
        "cash",
    );
    assert_eq!((end.em.share_proceeds, end.em.exercise_cost), (0.0, 0.0));
    assert!(end.em.replan);
    let e = end.first(EqEventKind::LockupSale).unwrap().1;
    assert_close(e.units, 2_100.0, "units settled");
    assert_close(e.amount, end.em.cash_in, "event cash");
    assert_eq!(st.status, CompanyStatus::Done);

    // With no lockup the same happens in the IPO month, at the IPO price.
    c.lockup_months = 0;
    let m = compile(&c);
    let mut st = state(&m, f);
    let all = run(&m, &mut st, 0..=10, |_| {});
    assert_eq!(all.first(EqEventKind::Ipo).unwrap().0, 10);
    assert_eq!(all.first(EqEventKind::LockupSale).unwrap().0, 10);
    assert_close(
        all.em.wages_cash,
        1_000.0 * 28.0 + 100.0 * 30.0,
        "L is unused",
    );
}

#[test]
fn selling_over_years_sells_equal_parts_a_year_apart() {
    let mut c = company();
    c.holdings = vec![
        holding(shares(1.0, 20), 600.0, 0.0, 0),
        holding(shares(2.0, 5), 300.0, 0.0, 0),
    ];
    c.policy.sell_down = SellDown::OverYears { years: 3 };
    let m = compile(&c);
    let mut st = state(&m, fate(OutcomeKind::Ipo, 10, 3.0, &m));
    let all = run(&m, &mut st, 0..=60, |c| c.single_stock_return = 0.01);
    let sales: Vec<(i32, EqEvent)> = all
        .events
        .iter()
        .copied()
        .filter(|(_, e)| e.kind == EqEventKind::LockupSale)
        .collect();
    assert_eq!(sales.len(), 3);
    for (k, (month, e)) in sales.iter().enumerate() {
        assert_eq!(*month, 16 + 12 * k as i32);
        assert_close(e.units, 300.0, "a third of the original shares");
        assert_close(
            e.price,
            30.0 * math::powi(1.01, 12 * k as i32),
            "single-stock return",
        );
        assert_close(e.amount, e.units * e.price, "cash");
    }
    assert_eq!(st.status, CompanyStatus::Done);
    assert_eq!((st.n_lots, st.sales_left), (0, 0));
    // First in, first out: the older lot goes first.
    let basis = 600.0 * 1.0 + 300.0 * 2.0;
    assert_close(all.em.basis_out, basis, "all basis leaves");
    assert_close(
        all.em.share_proceeds - basis,
        all.em.long_term_gain,
        "all long-term",
    );
    assert_eq!(all.em.short_term_gain, 0.0);

    // One yearly sale is everything at lockup end.
    c.policy.sell_down = SellDown::OverYears { years: 1 };
    let m = compile(&c);
    let mut st = state(&m, fate(OutcomeKind::Ipo, 10, 3.0, &m));
    let all = run(&m, &mut st, 0..=16, |_| {});
    assert_close(
        all.first(EqEventKind::LockupSale).unwrap().1.units,
        900.0,
        "all",
    );
    assert_eq!(st.status, CompanyStatus::Done);
    // And so does the default.
    c.policy.sell_down = SellDown::AllAtLockupEnd;
    let m = compile(&c);
    let mut st = state(&m, fate(OutcomeKind::Ipo, 10, 3.0, &m));
    let all = run(&m, &mut st, 0..=16, |_| {});
    assert_close(all.em.share_proceeds, 900.0 * 30.0, "all at lockup end");
    assert_eq!(st.status, CompanyStatus::Done);
}

#[test]
fn holding_moves_shares_to_the_sleeve_once_they_qualify() {
    for (granted_months_ago, moves_at) in [(30, 13), (5, 20)] {
        let mut c = company();
        c.holdings = vec![
            holding(shares(1.0, 20), 1_000.0, 0.0, 0),
            holding(
                HoldingKind::Iso {
                    strike: 1.0,
                    granted_months_ago,
                },
                500.0,
                500.0,
                0,
            ),
        ];
        c.policy.exercise = ExerciseTiming::VestedNow;
        c.policy.sell_down = SellDown::Hold;
        c.lockup_months = 4;
        let m = compile(&c);
        // Exercised at month 0, listed at month 1, lockup over at month 5.
        let mut st = state(&m, fate(OutcomeKind::Ipo, 1, 3.0, &m));
        let to_end = run(&m, &mut st, 0..=5, |_| {});
        let (month, kept) = to_end.first(EqEventKind::LockupKept).unwrap();
        assert_eq!(month, 5);
        assert_close(kept.units, 1_000.0, "the old lot moves at lockup end");
        assert_close(to_end.em.to_sleeve_value, 30_000.0, "value moved");
        assert_close(to_end.em.to_sleeve_basis, 1_000.0, "basis moved");
        assert_close(to_end.em.basis_out, 1_000.0, "basis out");
        assert_eq!(st.status, CompanyStatus::Public);
        assert_eq!(st.n_lots, 1);
        // The young ISO lot stays as paper value.
        assert_close(to_end.em.paper_value, 500.0 * 30.0, "paper value");

        let waiting = run(&m, &mut st, 6..=moves_at - 1, |_| {});
        assert_eq!(waiting.em.to_sleeve_value, 0.0);
        assert_eq!(st.n_lots, 1);
        let moved = run(&m, &mut st, moves_at..=moves_at, |_| {});
        assert_close(
            moved.em.to_sleeve_value,
            500.0 * 30.0,
            "moved once qualifying",
        );
        assert_close(moved.em.to_sleeve_basis, 500.0, "at its regular basis");
        assert_eq!(st.status, CompanyStatus::Done);

        // Nothing was ever sold, so nothing was taxed as a short-term or
        // disqualifying sale.
        for part in [&to_end, &waiting, &moved] {
            assert_eq!(part.em.short_term_gain, 0.0);
            assert_eq!(part.em.long_term_gain, 0.0);
            assert_eq!(part.em.iso_income, 0.0);
            assert_eq!(part.em.cash_in, 0.0);
        }
    }
}

#[test]
fn a_failure_is_a_capital_loss_of_the_basis() {
    for (months_held, long) in [(11, false), (12, true)] {
        let mut c = company();
        c.holdings = vec![
            holding(shares(5.0, months_held), 100.0, 0.0, 0),
            holding(nso(2.0), 1_000.0, 400.0, 12),
            holding(rsu(), 50.0, 50.0, 0),
        ];
        let m = compile(&c);
        let mut st = state(&m, fate(OutcomeKind::Fails, 1, 1.0, &m));
        let all = run(&m, &mut st, 0..=1, |_| {});
        let loss = if long {
            all.em.long_term_gain
        } else {
            all.em.short_term_gain
        };
        assert_eq!(loss, -500.0, "held {} months", months_held + 1);
        assert_eq!(all.em.long_term_gain + all.em.short_term_gain, -500.0);
        assert_eq!(all.em.basis_out, 500.0);
        assert_eq!(
            (all.em.amt_adjustment, all.em.amt_gain_adjustment),
            (0.0, 0.0)
        );
        assert!(all.em.failed_now && all.em.replan);
        assert_eq!(st.status, CompanyStatus::Failed);
        assert_eq!(st.outcome, Some(EquityOutcome::Failed));
        assert_eq!((st.n_lots, st.total_units()), (0, 0.0));
        assert_eq!((all.em.cash_in, all.em.paper_value), (0.0, 0.0));
        let e = all.first(EqEventKind::Failed).unwrap().1;
        assert_eq!((e.units, e.amount), (100.0, 500.0));
        // Afterwards the step does nothing.
        let after = run(&m, &mut st, 2..=5, |_| {});
        assert_eq!(after.em, EquityMonth::default());
    }

    // Not counted: the failure is still reported (it can end the job), with no money.
    let mut c = company();
    c.holdings = vec![holding(shares(5.0, 20), 100.0, 0.0, 0)];
    c.included = false;
    let m = compile(&c);
    let mut st = state(&m, fate(OutcomeKind::Fails, 1, 1.0, &m));
    let all = run(&m, &mut st, 0..=1, |_| {});
    assert!(all.em.failed_now && !all.em.replan);
    assert_eq!((all.em.long_term_gain, all.em.basis_out), (0.0, 0.0));
    assert_eq!(all.count(EqEventKind::Failed), 1);
}

#[test]
fn a_company_private_for_thirty_years_is_written_off() {
    let mut c = company();
    c.outcomes = vec![row(OutcomeKind::StillPrivate, 1.0, (0.0, 0.0), (1.0, 1.0))];
    c.tenders.annual_probability = Some(0.0);
    c.holdings = vec![
        holding(shares(5.0, 20), 100.0, 0.0, 0),
        holding(rsu(), 100.0, 100.0, 0),
    ];
    let m = compile(&c);
    for path in 0..20 {
        let st = m.begin(Link::Active, &mut stream(path), None);
        assert_eq!(st.fate.kind, OutcomeKind::StillPrivate);
        assert_eq!((st.fate.month, st.fate.m_end), (360, 1.0));
    }
    let mut st = m.begin(Link::Active, &mut stream(0), None);
    let before = run(&m, &mut st, 0..=359, |_| {});
    assert_eq!(before.events.len(), 0);
    assert_eq!(
        before.em.paper_value,
        200.0 * 10.0,
        "at the last round's price"
    );
    assert_eq!(st.status, CompanyStatus::Private);
    let at = run(&m, &mut st, 360..=360, |_| {});
    assert_eq!(at.em.long_term_gain, -500.0);
    assert_eq!(at.em.basis_out, 500.0);
    assert!(!at.em.failed_now, "a write-off does not touch the job");
    assert_eq!(st.status, CompanyStatus::Done);
    assert_eq!(st.outcome, Some(EquityOutcome::StillPrivate));
    assert_eq!(st.total_units(), 0.0);
    assert_eq!(at.count(EqEventKind::WrittenOff), 1);
    assert_eq!(at.events.len(), 1);
    assert_eq!(at.em.paper_value, 0.0);
    let after = run(&m, &mut st, 361..=400, |_| {});
    assert_eq!(after.em, EquityMonth::default());
}

// ---------------------------------------------------------------------------
// The job, death, deadlines
// ---------------------------------------------------------------------------

#[test]
fn leaving_forfeits_unvested_and_starts_the_window() {
    let mut c = company();
    c.holdings = vec![holding(nso(2.0), 4_800.0, 2_400.0, 24)];
    c.policy.at_deadline = DeadlinePolicy::Forfeit;
    type Adjust = fn(&mut EquityClimate);
    let cases: [(&str, Option<u32>, Adjust); 4] = [
        ("laid off", None, |c| c.left_job = c.t == 5),
        ("job ended", None, |c| {
            c.job_ended = c.t >= 5;
            c.employed = c.t < 5;
        }),
        ("unemployed", None, |c| c.employed = c.t != 5),
        ("planned departure", Some(5), |_| {}),
    ];
    for (what, leave, adjust) in cases {
        c.policy.leave_after_months = leave;
        let m = compile(&c);
        let mut st = state(&m, flat(&m));
        // A layoff with a spell of half a month: `left_job` is set while `employed`
        // is true again. Whatever the cause, a re-hire never brings the link back.
        let all = run(&m, &mut st, 0..=7, adjust);
        let (month, e) = all.first(EqEventKind::Forfeited).unwrap();
        assert_eq!(month, 5, "{what}");
        assert_close(e.units, 1_900.0, what);
        assert_close(e.amount, 1_900.0 * 8.0, what);
        assert_eq!(
            (st.link, st.severed_month, st.deadline),
            (Link::Severed, 5, 8),
            "{what}"
        );
        assert_close(st.holdings[0].vested, 2_900.0, what);
        assert_eq!(st.holdings[0].unvested, 0.0, "{what}");
        assert_eq!(all.count(EqEventKind::Forfeited), 1, "{what}");
        let at_deadline = run(&m, &mut st, 8..=8, |_| {});
        assert_close(
            at_deadline.first(EqEventKind::Lapsed).unwrap().1.units,
            2_900.0,
            what,
        );
        assert_eq!(st.total_units(), 0.0, "{what}");
    }

    // A job that has not started: nothing vests until it does.
    c.policy.leave_after_months = None;
    let m = compile(&c);
    let mut st = m.initial_state(Link::Pending);
    st.fate = flat(&m);
    run(&m, &mut st, 0..=3, |c| {
        c.job_started = false;
        c.employed = false;
    });
    assert_eq!(st.link, Link::Pending);
    assert_eq!(st.holdings[0].vested, 2_400.0);
    run(&m, &mut st, 4..=4, |_| {});
    assert_eq!(st.link, Link::Active);
    assert_close(
        st.holdings[0].vested,
        2_500.0,
        "vesting starts with the job",
    );

    // A job that ends without starting: the link is severed and gets a deadline.
    let mut st = m.initial_state(Link::Pending);
    st.fate = flat(&m);
    let all = run(&m, &mut st, 0..=6, |c| {
        c.job_started = false;
        c.employed = false;
        c.job_ended = c.t >= 6;
    });
    assert_eq!(
        (st.link, st.severed_month, st.deadline),
        (Link::Severed, 6, 9)
    );
    assert_close(
        all.first(EqEventKind::Forfeited).unwrap().1.units,
        2_400.0,
        "unvested",
    );
}

#[test]
fn rsus_survive_leaving_unless_the_plan_says_otherwise() {
    for must_be_employed_at_event in [false, true] {
        let mut c = company();
        c.holdings = vec![holding(
            HoldingKind::Rsu {
                must_be_employed_at_event,
            },
            300.0,
            100.0,
            20,
        )];
        c.policy.leave_after_months = Some(2);
        let m = compile(&c);
        let mut st = state(&m, fate(OutcomeKind::Acquired, 30, 2.0, &m));
        let all = run(&m, &mut st, 0..=30, |_| {});
        let forfeited = all.first(EqEventKind::Forfeited).unwrap().1.units;
        if must_be_employed_at_event {
            assert_close(forfeited, 300.0, "vested units are lost too");
            assert_eq!(all.em.cash_in, 0.0);
        } else {
            assert_close(forfeited, 180.0, "only the unvested units");
            // RSUs have no deadline: they wait for the event.
            assert_eq!(all.count(EqEventKind::Lapsed), 0);
            assert_close(all.em.wages_cash, 120.0 * 20.0, "paid at the sale");
        }
        assert_eq!(st.status, CompanyStatus::Done);
    }
}

#[test]
fn death_steps_up_the_lots_and_passes_them_on() {
    let mut c = company();
    c.qsbs = true;
    c.holdings = vec![
        holding(iso_shares(1.5, 14, 4.0), 1_000.0, 0.0, 0),
        holding(nso(2.0), 600.0, 300.0, 30),
    ];
    c.policy.at_deadline = DeadlinePolicy::Forfeit;
    let m = compile(&c);
    let f = fate(OutcomeKind::Ipo, 40, 4.0, &m);
    let mut st = state(&m, f);
    run(&m, &mut st, 0..=4, |_| {});
    let at = run(&m, &mut st, 5..=5, |c| c.owner_died = true);
    let value = m.fmv_at(&f, 5);
    let lot = st.lots[0];
    assert_close(
        lot.cost,
        1_000.0 * value,
        "basis is the 409A value at death",
    );
    assert_eq!(lot.amt_extra, 0.0);
    assert!(!lot.iso && !lot.qsbs);
    assert_eq!(lot.amt_year, i32::MIN);
    assert!(5 - lot.acquired > LONG_TERM_MONTHS, "long-term at once");
    assert_close(at.em.basis_out, 1_500.0, "old basis out");
    assert_close(at.em.basis_in, lot.cost, "new basis in");
    // The link ends: unvested is forfeited, vested options pass on under the deadline.
    assert_eq!((st.link, st.deadline), (Link::Severed, 8));
    assert_close(
        at.first(EqEventKind::Forfeited).unwrap().1.units,
        250.0,
        "unvested",
    );
    assert_close(st.holdings[1].vested, 350.0, "vested options pass on");
    // A sale a month later is long-term on the stepped-up basis, with no ISO income.
    let mut em = EquityMonth::default();
    m.sell_from_lot(&mut st, 0, 1_000.0, value + 1.0, &climate(6), &mut em);
    assert_close(em.long_term_gain, 1_000.0, "gain since death");
    assert_eq!(
        (em.iso_income, em.amt_adjustment, em.qsbs_excluded),
        (0.0, 0.0, 0.0)
    );

    // With nobody to inherit, everything is dropped and the basis is returned.
    let mut st = state(&m, f);
    run(&m, &mut st, 0..=4, |_| {});
    assert_eq!(m.abandon(&mut st), 1_500.0);
    assert_eq!(st.status, CompanyStatus::Done);
    assert_eq!((st.n_lots, st.total_units()), (0, 0.0));
    assert_eq!(m.step(&mut st, &climate(5)), EquityMonth::default());
}

#[test]
fn options_under_water_after_an_ipo_are_sold_or_lapse() {
    let mut c = company();
    c.holdings = vec![holding(nso(25.0), 1_000.0, 1_000.0, 0)];
    c.lockup_months = 0;
    let m = compile(&c);
    // Listed at $20 in month 1 with no lockup; the option is struck at $25.
    let mut st = state(&m, fate(OutcomeKind::Ipo, 1, 2.0, &m));
    let listed = run(&m, &mut st, 0..=1, |c| c.single_stock_return = 0.10);
    assert_eq!(st.status, CompanyStatus::Public);
    assert_eq!((listed.em.cash_in, listed.em.paper_value), (0.0, 0.0));
    // 22.00 and 24.20: still under water. 26.62 in month 4: sold.
    let waiting = run(&m, &mut st, 2..=3, |c| c.single_stock_return = 0.10);
    assert_eq!(waiting.em.cash_in, 0.0);
    assert_eq!(st.holdings[0].vested, 1_000.0);
    let sold = run(&m, &mut st, 4..=4, |c| c.single_stock_return = 0.10);
    assert_close(st.price, 20.0 * math::powi(1.1, 3), "price");
    assert_close(
        sold.em.wages_cash,
        1_000.0 * (26.62 - 25.0),
        "sold above the strike",
    );
    assert!(!sold.em.replan);
    assert_eq!(st.status, CompanyStatus::Done);

    // On a flat price they lapse at the deadline, and the company is done.
    c.policy.leave_after_months = Some(3);
    let m = compile(&c);
    let mut st = state(&m, fate(OutcomeKind::Ipo, 1, 2.0, &m));
    let all = run(&m, &mut st, 0..=6, |_| {});
    let (month, e) = all.first(EqEventKind::Lapsed).unwrap();
    assert_eq!((month, e.units, e.amount), (6, 1_000.0, 0.0));
    assert_eq!(all.em.cash_in, 0.0);
    assert_eq!(
        all.em.exercise_cost, 0.0,
        "nothing is exercised for cash once listed"
    );
    assert_eq!(st.status, CompanyStatus::Done);
}

// ---------------------------------------------------------------------------
// Events, lots, conservation
// ---------------------------------------------------------------------------

#[test]
fn a_month_logs_one_event_of_each_kind() {
    let mut c = company();
    c.holdings = (0..8)
        .map(|i| EquityHolding {
            id: format!("g{i}"),
            ..holding(nso(1.0 + f64::from(i)), 100.0, 100.0, 0)
        })
        .collect();
    c.policy.exercise = ExerciseTiming::VestedNow;
    let m = compile(&c);
    let mut st = state(&m, flat(&m));
    let em = m.step(&mut st, &climate(0));
    assert_eq!(em.n_events, 1);
    let e = em.events[0];
    let cost: f64 = (0..8).map(|i| 100.0 * (1.0 + f64::from(i))).sum();
    assert_eq!(e.kind, EqEventKind::Exercised);
    assert_eq!((e.units, e.amount), (800.0, cost));
    assert_close(e.price, cost / 800.0, "price is cost over units");
    assert_eq!(em.exercise_cost, cost);

    // A link that ends, an exercise, an IPO with no lockup, a sale, a move to the
    // sleeve and a lapse, all in one month: six events, none dropped.
    let mut c = company();
    c.holdings = vec![
        holding(nso(2.0), 1_000.0, 400.0, 12),
        holding(nso(50.0), 300.0, 300.0, 0),
        holding(rsu(), 200.0, 200.0, 0),
        holding(shares(1.0, 20), 500.0, 0.0, 0),
    ];
    c.policy.exercise = ExerciseTiming::VestedNow;
    c.policy.sell_down = SellDown::Hold;
    c.policy.leave_after_months = Some(0);
    c.exercise_window_months = 0;
    c.lockup_months = 0;
    let m = compile(&c);
    let mut f = fate(OutcomeKind::Ipo, 0, 2.0, &m);
    f.month = 0;
    let mut st = state(&m, f);
    let em = m.step(&mut st, &climate(0));
    let kinds: Vec<EqEventKind> = em.events().iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [
            EqEventKind::Forfeited,
            EqEventKind::Exercised,
            EqEventKind::Ipo,
            EqEventKind::LockupKept,
            EqEventKind::LockupSale,
            EqEventKind::Lapsed,
        ]
    );
    assert!(em.n_events < MAX_EVENTS);
    assert_eq!(em.event(EqEventKind::Forfeited).unwrap().units, 600.0);
    assert_eq!(em.event(EqEventKind::Exercised).unwrap().units, 400.0);
    assert_eq!(em.event(EqEventKind::LockupKept).unwrap().units, 500.0);
    assert_eq!(em.event(EqEventKind::LockupSale).unwrap().units, 200.0);
    assert_eq!(em.event(EqEventKind::Lapsed).unwrap().units, 300.0);

    // `push` adds to an entry of the same kind and never overflows.
    let mut em = EquityMonth::default();
    em.push(EqEventKind::Exercised, 10.0, 2.0, 20.0);
    em.push(EqEventKind::Exercised, 30.0, 4.0, 120.0);
    em.push(EqEventKind::Tender, 5.0, 9.0, 45.0);
    assert_eq!(em.n_events, 2);
    assert_eq!(em.events[0].units, 40.0);
    assert_eq!(em.events[0].price, 3.5);
    assert_eq!(em.events[1].price, 9.0);
}

#[test]
fn the_iso_limit_is_measured_over_the_whole_grant() {
    let big = holding(
        HoldingKind::Iso {
            strike: 5.0,
            granted_months_ago: 48,
        },
        400_000.0,
        400_000.0,
        0,
    );
    assert_eq!(iso_yearly_amount(&big), 500_000.0);
    assert!(iso_yearly_amount(&big) > ISO_ANNUAL_LIMIT);
    let demo = demo_company();
    assert_eq!(iso_yearly_amount(&demo.holdings[0]), 22_500.0);
    assert!(demo
        .holdings
        .iter()
        .all(|h| iso_yearly_amount(h) <= ISO_ANNUAL_LIMIT));
    // A span under a year counts as one year; other kinds have no limit.
    let fresh = holding(
        HoldingKind::Iso {
            strike: 5.0,
            granted_months_ago: 0,
        },
        30_000.0,
        30_000.0,
        0,
    );
    assert_eq!(iso_yearly_amount(&fresh), 150_000.0);
    assert_eq!(iso_yearly_amount(&holding(nso(5.0), 1e6, 1e6, 0)), 0.0);
}

#[test]
fn qsbs_tiers_and_cap() {
    // A lot of 1,000 shares at a basis of 1, sold at 20: a gain of 19,000.
    let sale = |months_held: u32, held: i32, caps: [f64; 2]| {
        let mut c = company();
        c.qsbs = true;
        c.holdings = vec![holding(shares(1.0, months_held), 1_000.0, 0.0, 0)];
        let m = CompanyModel::compile(&c, &params(), caps, START);
        let mut st = state(&m, flat(&m));
        assert!(st.lots[0].qsbs);
        let mut em = EquityMonth::default();
        let t = held - months_held as i32;
        m.sell_from_lot(&mut st, 0, 1_000.0, 20.0, &climate(t), &mut em);
        (em, st)
    };
    // Acquired ten months before a November 2026 start: January 2026, the new regime.
    for (held, fraction) in [
        (35, 0.0),
        (36, 0.5),
        (47, 0.5),
        (48, 0.75),
        (59, 0.75),
        (60, 1.0),
    ] {
        let (em, st) = sale(10, held, CAPS);
        assert_close(
            em.qsbs_excluded,
            fraction * 19_000.0,
            &format!("held {held}"),
        );
        assert_close(
            em.long_term_gain,
            (1.0 - fraction) * 19_000.0,
            "the rest is taxed",
        );
        assert_eq!(st.qsbs_excluded, em.qsbs_excluded);
        assert_eq!(em.share_proceeds, 20_000.0, "the cash is unchanged");
    }
    // Acquired twenty months before: March 2025, the old regime. All or nothing,
    // and "more than five years".
    for (held, fraction) in [(48, 0.0), (60, 0.0), (61, 1.0)] {
        let (em, _) = sale(20, held, CAPS);
        assert_close(
            em.qsbs_excluded,
            fraction * 19_000.0,
            &format!("old, held {held}"),
        );
    }
    // July 2025 is the first month of the new regime (16 months before the start).
    assert_close(sale(16, 36, CAPS).0.qsbs_excluded, 9_500.0, "July 2025");
    assert_eq!(sale(17, 36, CAPS).0.qsbs_excluded, 0.0, "June 2025");

    // Each regime has its own cap.
    assert_close(
        sale(10, 60, [5_000.0, 1.0]).0.qsbs_excluded,
        5_000.0,
        "new cap",
    );
    assert_close(
        sale(20, 61, [1.0, 7_000.0]).0.qsbs_excluded,
        7_000.0,
        "old cap",
    );

    // The cap binds across two sales: one running total per company.
    let mut c = company();
    c.qsbs = true;
    c.holdings = vec![holding(shares(1.0, 10), 1_000.0, 0.0, 0)];
    let m = CompanyModel::compile(&c, &params(), [12_000.0, 12_000.0], START);
    let mut st = state(&m, flat(&m));
    let mut first = EquityMonth::default();
    m.sell_from_lot(&mut st, 0, 500.0, 20.0, &climate(50), &mut first);
    assert_close(first.qsbs_excluded, 9_500.0, "the first half, in full");
    let mut second = EquityMonth::default();
    m.sell_from_lot(&mut st, 0, 500.0, 20.0, &climate(51), &mut second);
    assert_close(second.qsbs_excluded, 2_500.0, "what is left of the cap");
    assert_close(second.long_term_gain, 7_000.0, "the rest is taxed");
    assert_close(st.qsbs_excluded, 12_000.0, "running total");

    // No exclusion without the flag, and none on a loss.
    let mut c = company();
    c.holdings = vec![holding(shares(1.0, 10), 1_000.0, 0.0, 0)];
    let m = compile(&c);
    let mut st = state(&m, flat(&m));
    let mut em = EquityMonth::default();
    m.sell_from_lot(&mut st, 0, 1_000.0, 20.0, &climate(60), &mut em);
    assert_eq!(em.qsbs_excluded, 0.0);
    c.qsbs = true;
    let m = compile(&c);
    let mut st = state(&m, flat(&m));
    let mut em = EquityMonth::default();
    m.sell_from_lot(&mut st, 0, 1_000.0, 0.5, &climate(60), &mut em);
    assert_eq!((em.qsbs_excluded, em.long_term_gain), (0.0, -500.0));
}

#[test]
fn lots_pool_by_year_and_never_overflow() {
    let mut c = company();
    c.holdings = (0..8)
        .map(|i| EquityHolding {
            id: format!("s{i}"),
            ..holding(shares(1.0, 12 * (i + 1)), 100.0, 0.0, 0)
        })
        .collect();
    let m = compile(&c);
    let mut st = state(&m, flat(&m));
    assert_eq!(st.n_lots, 8);
    let (mut total_shares, mut total_cost, mut total_extra) = (800.0, 800.0, 0.0);
    let mut most = 0;
    for t in 0..240 {
        let iso = t % 2 == 0;
        let new = Lot {
            shares: 10.0,
            cost: if iso { 20.0 } else { 50.0 },
            amt_extra: if iso { 30.0 } else { 0.0 },
            acquired: t,
            granted: -24,
            amt_year: if iso {
                START.add_months(t).year()
            } else {
                i32::MIN
            },
            iso,
            qsbs: false,
        };
        let full = st.n_lots == MAX_LOTS;
        let pooled = st.lots().iter().any(|l| {
            l.iso == iso
                && l.amt_year == new.amt_year
                && START.add_months(l.acquired).year() == START.add_months(t).year()
        });
        let oldest_before = st.lots().iter().map(|l| l.acquired).min().unwrap();
        m.add_lot(&mut st, new);
        total_shares += new.shares;
        total_cost += new.cost;
        total_extra += new.amt_extra;
        most = most.max(st.n_lots);
        assert!(st.n_lots <= MAX_LOTS);
        assert_close(st.lot_shares(), total_shares, "shares preserved");
        assert_close(st.lot_basis(), total_cost, "cost preserved");
        assert_close(
            st.lots().iter().map(|l| l.amt_extra).sum::<f64>(),
            total_extra,
            "AMT basis preserved",
        );
        // The new exercise keeps its own acquisition month and flag.
        let own = st.lots().iter().find(|l| l.acquired == t && l.iso == iso);
        assert!(own.is_some(), "month {t}");
        if full && !pooled {
            // A slot was freed by merging two old lots of one kind, not the new one.
            assert_eq!(st.n_lots, MAX_LOTS);
            assert!(own.unwrap().shares == 10.0);
            let oldest_after = st.lots().iter().map(|l| l.acquired).min().unwrap();
            assert!(oldest_after >= oldest_before);
        }
    }
    assert_eq!(most, MAX_LOTS);
    // Lots of a calendar year and kind are pooled: the last year holds one of each.
    let last_year = START.add_months(239).year();
    let in_last_year = st
        .lots()
        .iter()
        .filter(|l| START.add_months(l.acquired).year() == last_year)
        .count();
    assert_eq!(in_last_year, 2);

    // Sales take the oldest lot first, and an emptied lot frees its slot.
    let before = st.n_lots;
    let oldest = st.lots[st.oldest_lot().unwrap()];
    let mut em = EquityMonth::default();
    let sold = m.sell_shares(&mut st, oldest.shares, 10.0, &climate(240), &mut em);
    assert_close(sold, oldest.shares, "sold the oldest lot");
    assert_eq!(st.n_lots, before - 1);
    assert_close(em.basis_out, oldest.cost, "its basis left");
    assert!(st.lots().iter().all(|l| l.shares > 0.0));
    assert_eq!(st.lots[st.n_lots], Lot::default());
    // Selling everything leaves no lot and no dust.
    m.sell_all(&mut st, 10.0, &climate(240), &mut em);
    assert_eq!((st.n_lots, st.lot_shares(), st.lot_basis()), (0, 0.0, 0.0));
    assert_close(em.basis_out, total_cost, "all basis left");
}

#[test]
fn shares_are_conserved() {
    let p = params();
    let mut rng = Rng::from_seed(0x5EED);
    let mut pick = |n: usize| (rng.uniform() * n as f64) as usize;
    let mut outcomes = [0usize; 4];
    for case in 0..2_000u64 {
        let mut c = company();
        c.stage = CompanyStage::ALL[pick(6)];
        c.qsbs = pick(2) == 0;
        c.lockup_months = [0, 6, 12][pick(3)];
        c.exercise_window_months = [0, 3, 12, 84][pick(4)];
        c.tenders.annual_probability = Some([0.0, 0.15, 1.0][pick(3)]);
        c.tenders.max_fraction = [0.2, 1.0][pick(2)];
        c.tenders.former_employees = pick(2) == 0;
        c.tenders.next_in_months = [None, Some(7)][pick(2)];
        c.policy = EquityPolicy {
            exercise: [
                ExerciseTiming::AtLiquidity,
                ExerciseTiming::VestedNow,
                ExerciseTiming::AsTheyVest,
            ][pick(3)],
            at_deadline: [DeadlinePolicy::Exercise, DeadlinePolicy::Forfeit][pick(2)],
            tender_sell_fraction: [0.0, 0.5, 1.0][pick(3)],
            sell_down: [
                SellDown::AllAtLockupEnd,
                SellDown::OverYears { years: 3 },
                SellDown::Hold,
            ][pick(3)],
            leave_after_months: [None, Some(0), Some(30)][pick(3)],
        };
        if pick(4) == 0 {
            c.job = None;
        }
        let employed_here = c.job.is_some();
        for i in 0..1 + pick(8) {
            let total = 1_000.0 * (1 + pick(50)) as f64;
            let vested = if employed_here {
                total * pick(5) as f64 / 4.0
            } else {
                total
            };
            let vest_months_left = if vested < total {
                1 + pick(60) as u32
            } else {
                0
            };
            let kind = match pick(4) {
                0 => HoldingKind::Iso {
                    strike: [0.5, 3.0, 12.0][pick(3)],
                    granted_months_ago: [0, 30, 100][pick(3)],
                },
                1 => HoldingKind::Nso {
                    strike: [0.5, 3.0, 12.0][pick(3)],
                    granted_months_ago: [0, 30, 100][pick(3)],
                },
                2 => HoldingKind::Rsu {
                    must_be_employed_at_event: pick(2) == 0,
                },
                _ => HoldingKind::Shares {
                    basis_per_share: [0.5, 3.0][pick(2)],
                    months_held: [2, 14, 40][pick(3)],
                    from_iso: pick(2) == 0,
                    fmv_at_exercise: 4.0,
                },
            };
            c.holdings.push(EquityHolding {
                id: format!("h{i}"),
                cliff_months: vest_months_left.min([0, 12][pick(2)]),
                ..holding(kind, total, vested, vest_months_left)
            });
        }
        let layoff = [i32::MAX, 3, 50][pick(3)];
        let death = [i32::MAX, 20][pick(2)];
        let liquid = [0.0, 20_000.0, AMPLE][pick(3)];

        let m = CompanyModel::compile(&c, &p, [50_000.0, 50_000.0], START);
        let mut st = m.begin(Link::Active, &mut stream(case), None);
        outcomes[st.fate.kind as usize] += 1;
        let start_units = st.total_units();
        let start_basis = st.lot_basis();
        let mut tender_rng = stream(case);
        let (mut units_out, mut basis_flow, mut basis_moved) = (0.0, 0.0, 0.0);
        let mut last = EquityMonth::default();
        for t in 0..=400 {
            let mut cl = climate(t);
            cl.left_job = t == layoff;
            cl.employed = employed_here && t != layoff;
            cl.owner_died = t == death;
            cl.liquid = liquid;
            cl.wage_tax_rate = 0.35;
            cl.iso_tax_rate = 0.28;
            cl.u_tender = tender_rng.uniform();
            cl.single_stock_return = 0.02 * (tender_rng.uniform() - 0.45);
            let em = m.step(&mut st, &cl);
            units_out += em.units_out;
            basis_flow += em.basis_in - em.basis_out;
            basis_moved += em.basis_in + em.basis_out;
            for v in [
                em.exercise_cost,
                em.wages_cash,
                em.wages_noncash,
                em.iso_income,
                em.share_proceeds,
                em.cash_in,
                em.qsbs_excluded,
                em.tax_estimate,
                em.to_sleeve_value,
                em.to_sleeve_basis,
                em.tender_proceeds,
                em.paper_value,
            ] {
                assert!(v.is_finite() && v >= -1e-6, "case {case} month {t}: {em:?}");
            }
            assert!(em.amt_gain_adjustment <= 0.0);
            assert!(em.tender_proceeds <= em.cash_in + 1e-6);
            assert!(em.share_proceeds <= em.cash_in + 1e-6);
            assert!(em.exercise_cost <= liquid + 1e-6, "case {case} month {t}");
            assert!(st.n_lots <= MAX_LOTS && em.n_events <= MAX_EVENTS);
            last = em;
        }
        let scale = 1.0 + start_units;
        assert!(
            (start_units - units_out - st.total_units()).abs() <= 1e-6 * scale,
            "case {case}: {start_units} at the start, {units_out} out, {} held ({c:?})",
            st.total_units()
        );
        assert!(
            (start_basis + basis_flow - st.lot_basis()).abs()
                <= 1e-9 * (1.0 + start_basis + basis_moved),
            "case {case}: basis {start_basis} + {basis_flow} against {}",
            st.lot_basis()
        );
        assert!(st.qsbs_excluded <= 50_000.0 + 1e-6);
        // Whatever happened, a company whose event has passed is settled or waiting
        // on something real.
        if matches!(st.status, CompanyStatus::Done | CompanyStatus::Failed) {
            assert_eq!(st.total_units(), 0.0, "case {case}");
            assert_eq!(last.paper_value, 0.0);
        } else if matches!(st.status, CompanyStatus::Public | CompanyStatus::Acquired) {
            assert!(
                st.total_units() > 0.0,
                "case {case}: {:?} with nothing held",
                st.status
            );
        }
    }
    // The cases covered every kind of fate.
    assert!(outcomes.iter().all(|n| *n > 5), "{outcomes:?}");
}

#[test]
fn the_step_draws_nothing() {
    // By signature: the step takes no random stream, and everything it reads and
    // returns is plain data.
    let _step: fn(&CompanyModel, &mut CompanyState, &EquityClimate) -> EquityMonth =
        CompanyModel::step;
    let _abandon: fn(&CompanyModel, &mut CompanyState) -> f64 = CompanyModel::abandon;
    fn plain<T: Copy + Send + Sync + 'static>() {}
    plain::<CompanyModel>();
    plain::<CompanyState>();
    plain::<EquityClimate>();
    plain::<EquityMonth>();
    plain::<EquityParams>();
    plain::<Fate>();
    // Stepping twice from the same state with the same climate gives the same month.
    let m = compile(&demo_company());
    let st = m.begin(Link::Active, &mut stream(5), None);
    let (mut a, mut b) = (st, st);
    for t in 0..200 {
        assert_eq!(m.step(&mut a, &climate(t)), m.step(&mut b, &climate(t)));
    }
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// The preview
// ---------------------------------------------------------------------------

#[test]
fn the_preview_matches_the_table_and_is_deterministic() {
    const DRAWS: u32 = 50_000;
    let p = params();
    let demo = demo_company();
    let pv = preview(&demo, &p, DRAWS);
    let again = preview(&demo, &p, DRAWS);
    assert_eq!(
        serde_json::to_string(&pv).unwrap(),
        serde_json::to_string(&again).unwrap()
    );
    assert_eq!(pv.draws, DRAWS);
    assert_eq!(preview(&demo, &p, 0).draws, p.preview_draws);

    // Shares by what finally happens: fails, written off, acquired, ipo.
    let s = CompanyStage::SeriesC.index();
    let q = p.p_still_private[s];
    let factor = (1.0 - math::powi(q, 3)) / (1.0 - q);
    let expected = [
        p.p_fails[s] * factor,
        math::powi(q, 3),
        p.p_acquired[s] * factor,
        p.p_ipo[s] * factor,
    ];
    let n = DRAWS as usize;
    for (k, name) in ["fails", "written off", "acquired", "ipo"]
        .iter()
        .enumerate()
    {
        let count = (pv.outcome_shares[k] * f64::from(DRAWS)).round() as usize;
        assert_share(count, n, expected[k], name);
    }
    assert!((pv.outcome_shares.iter().sum::<f64>() - 1.0).abs() < 1e-12);
    assert!(pv.nothing_share >= pv.outcome_shares[0] + pv.outcome_shares[1] - 1e-12);
    assert!(pv.nothing_share < 1.0);

    // The table in force is the preset, exactly.
    let kinds: Vec<OutcomeKind> = pv.table.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        [
            OutcomeKind::Fails,
            OutcomeKind::Acquired,
            OutcomeKind::Ipo,
            OutcomeKind::StillPrivate
        ]
    );
    let probabilities: Vec<f64> = pv.table.iter().map(|r| r.probability).collect();
    assert_eq!(
        probabilities,
        [
            p.p_fails[s],
            p.p_acquired[s],
            p.p_ipo[s],
            p.p_still_private[s]
        ]
    );
    assert_eq!(pv.table, pv.typical);
    assert_eq!(pv.table[2].id, "ipo");
    // Display years are half years inside the round; multiples the 10th and 90th
    // percentiles.
    for r in &pv.table[..3] {
        assert!(r.years_from > 0.0 && r.years_from < r.years_to && r.years_to <= 10.0);
        assert_eq!((r.years_from * 2.0).fract(), 0.0);
    }
    assert_close(
        pv.table[2].multiple_low,
        3.0 * math::exp(-Z90 * 0.7),
        "IPO low multiple",
    );
    assert_close(
        pv.table[2].multiple_high,
        3.0 * math::exp(Z90 * 0.7),
        "IPO high multiple",
    );

    // Values in force and the position today.
    assert_eq!(pv.fmv_409a, 5.0);
    assert_eq!(pv.preference_overhang, 0.30);
    assert_eq!(pv.tender_annual_probability, 0.04);
    assert_eq!(pv.paper_value_total, 1_458_000.0);
    assert_eq!(
        pv.paper_value_vested,
        50_000.0 * 12.5 + 8_000.0 * 10.0 + 10_000.0 * 14.0
    );
    assert_eq!(pv.exercise_cost_vested, 50_000.0 * 1.5 + 8_000.0 * 4.0);
    assert_eq!(pv.exercise_cost_total, 60_000.0 * 1.5 + 40_000.0 * 4.0);
    assert_eq!(pv.spread_iso_vested, 50_000.0 * 3.5);
    assert_eq!(pv.spread_nso_vested, 8_000.0 * 1.0);
    assert_eq!(
        position_value(&demo),
        (pv.paper_value_vested, pv.paper_value_total)
    );

    // The payoffs.
    assert_eq!(pv.dots.len(), 100);
    assert!(pv.dots.windows(2).all(|w| w[0] <= w[1]));
    assert!(pv.quantiles.windows(2).all(|w| w[0] <= w[1]));
    assert!(pv.if_paid.windows(2).all(|w| w[0] <= w[1]));
    assert!(pv.if_paid[0] > 0.0);
    assert_eq!(
        pv.quantiles[0], 0.0,
        "one draw in ten pays nothing at this stage"
    );
    assert!(pv.years_to_payout_p50 > 1.0 && pv.years_to_payout_p50 < 30.0);
    assert!(pv.acquisition_zero_share > 0.15 && pv.acquisition_zero_share < 0.35);
    assert!(pv.ipo_below_last_round_share > 0.0 && pv.ipo_below_last_round_share < 0.2);
    assert!(pv.tender_before_share > 0.0 && pv.tender_before_share < 0.5);

    // Payoffs are deflated: a certain payout of 100 in ten years.
    let mut certain = company();
    certain.preferred_price = 1.0;
    certain.holdings = vec![holding(shares(0.0, 0), 100.0, 0.0, 0)];
    certain.outcomes = vec![row(OutcomeKind::Acquired, 1.0, (10.0, 10.0), (1.0, 1.0))];
    let pv = preview(&certain, &p, 200);
    let expected = 100.0 / math::powi(1.0 + p.preview_inflation, 10);
    for v in pv.dots.iter().chain(&pv.quantiles).chain(&pv.if_paid) {
        assert_close(*v, expected, "deflated payout");
    }
    assert_eq!(pv.nothing_share, 0.0);
    assert_eq!(pv.outcome_shares, [0.0, 0.0, 1.0, 0.0]);
    assert_eq!(pv.years_to_payout_p50, 10.0);
    assert!(
        (pv.implied_multiple - 1.0).abs() < 1e-6,
        "{}",
        pv.implied_multiple
    );
    // The custom table is shown as entered, normalised.
    assert_eq!(pv.table, certain.outcomes);
    assert_eq!(pv.typical.len(), 4);
    // `included` is ignored: the preview always shows the holdings.
    certain.included = false;
    assert_close(
        preview(&certain, &p, 200).quantiles[2],
        expected,
        "not counted",
    );

    // A planned tender before the outcome makes one certain; none at a chance of 0.
    certain.tenders = TenderOffers {
        annual_probability: Some(0.0),
        ..TenderOffers::default()
    };
    assert_eq!(preview(&certain, &p, 200).tender_before_share, 0.0);
    certain.tenders.next_in_months = Some(6);
    assert_eq!(preview(&certain, &p, 200).tender_before_share, 1.0);
    certain.tenders = TenderOffers {
        annual_probability: Some(0.15),
        ..TenderOffers::default()
    };
    assert_close(
        preview(&certain, &p, 200).tender_before_share,
        1.0 - math::powi(1.0 - (1.0 - math::pow(0.85, 1.0 / 12.0)), 119),
        "119 months at the monthly chance",
    );

    // Nothing ever pays: the conditional figures are null in JSON and read back.
    let mut nothing = company();
    nothing.holdings = vec![holding(shares(1.0, 0), 100.0, 0.0, 0)];
    nothing.outcomes = vec![row(OutcomeKind::Fails, 1.0, (1.0, 2.0), (1.0, 1.0))];
    let pv = preview(&nothing, &p, 200);
    assert_eq!(pv.nothing_share, 1.0);
    assert!(pv.if_paid.iter().all(|v| v.is_nan()) && pv.years_to_payout_p50.is_nan());
    let json = serde_json::to_value(&pv).unwrap();
    assert!(json["if_paid"][1].is_null() && json["years_to_payout_p50"].is_null());
    let back: EquityPreview = serde_json::from_value(json).unwrap();
    assert!(back.years_to_payout_p50.is_nan());
    assert_eq!(back.dots, pv.dots);
    // A company with nothing entered yet still previews.
    let empty = preview(&PrivateCompany::default(), &p, 100);
    assert_eq!((empty.paper_value_total, empty.nothing_share), (0.0, 1.0));
}

#[test]
fn the_preview_and_the_model_draw_the_same_fate() {
    let p = params();
    let demo = demo_company();
    let table = resolved_table(&demo, &p);
    let m = compile(&demo);
    assert_eq!(m.table, table);
    for path in 0..500 {
        let expected = draw_fate(&table, &p, &mut stream(path));
        let mut rng = stream(path);
        let st = m.begin(Link::Active, &mut rng, None);
        assert_eq!(st.fate, expected);
        // Exactly four words, whatever the company's settings.
        let mut reference = stream(path);
        for _ in 0..4 {
            reference.next_u64();
        }
        assert_eq!(rng.next_u64(), reference.next_u64());
    }
    // A company that is not counted, or has no holdings, draws the same fate.
    let mut other = demo.clone();
    other.included = false;
    let excluded = compile(&other);
    other.included = true;
    other.holdings.clear();
    other.policy.exercise = ExerciseTiming::VestedNow;
    let bare = compile(&other);
    for path in 0..100 {
        let fate = m.begin(Link::Active, &mut stream(path), None).fate;
        assert_eq!(
            excluded.begin(Link::Pending, &mut stream(path), None).fate,
            fate
        );
        assert_eq!(bare.begin(Link::Active, &mut stream(path), None).fate, fate);
    }
    // The primary company is the largest position, included or not; ties go first.
    let small = company();
    assert_eq!(primary_company(&[small.clone(), demo.clone()]), 1);
    assert_eq!(
        primary_company(&[excluded_company(&demo), small.clone()]),
        0
    );
    assert_eq!(primary_company(&[small.clone(), small]), 0);
    assert_eq!(primary_company(&[]), 0);
}

fn excluded_company(c: &PrivateCompany) -> PrivateCompany {
    PrivateCompany {
        included: false,
        ..c.clone()
    }
}

// ---------------------------------------------------------------------------
// Review fixes
// ---------------------------------------------------------------------------

#[test]
fn rsus_that_require_employment_are_safe_once_the_company_has_listed() {
    // 10,000 vested RSUs that require employment at the event; the company lists at
    // $30 in month 6 with a six-month lockup. Staying pays 300,000 at month 12.
    let rsus = || {
        let mut c = company();
        c.holdings = vec![holding(
            HoldingKind::Rsu {
                must_be_employed_at_event: true,
            },
            10_000.0,
            10_000.0,
            0,
        )];
        c.lockup_months = 6;
        c
    };
    let m = compile(&rsus());
    let ipo = fate(OutcomeKind::Ipo, 6, 3.0, &m);
    let stay = run(&m, &mut state(&m, ipo), 0..=12, |_| {});
    assert_close(stay.em.wages_cash, 300_000.0, "paid at lockup end");

    // Leaving in month 9, during the lockup: the event has happened, nothing
    // vested is lost, and the units are paid when the lockup ends.
    let mut c = rsus();
    c.policy.leave_after_months = Some(9);
    let m = compile(&c);
    let mut st = state(&m, ipo);
    let all = run(&m, &mut st, 0..=12, |_| {});
    assert_eq!(st.severed_month, 9);
    assert_eq!(all.count(EqEventKind::Forfeited), 0);
    assert_close(all.em.wages_cash, 300_000.0, "left in the lockup");
    let (month, sale) = all.first(EqEventKind::LockupSale).unwrap();
    assert_eq!((month, sale.units), (12, 10_000.0));

    // A layoff, the end of the job and the owner's death in the lockup likewise
    // (after a death the survivor is paid).
    for (what, cut) in [
        (
            "laid off",
            (|c: &mut EquityClimate| c.left_job = c.t == 8) as fn(&mut EquityClimate),
        ),
        ("job ended", |c| c.job_ended = c.t >= 8),
        ("not employed", |c| c.employed = c.t < 8),
        ("owner died", |c| c.owner_died = c.t == 8),
    ] {
        let m = compile(&rsus());
        let mut st = state(&m, ipo);
        let all = run(&m, &mut st, 0..=12, cut);
        assert_eq!(st.link, Link::Severed, "{what}");
        assert_eq!(all.count(EqEventKind::Forfeited), 0, "{what}");
        assert_close(all.em.wages_cash, 300_000.0, what);
    }

    // Leaving one month before the listing still loses them, and the event says
    // they were vested.
    let mut c = rsus();
    c.policy.leave_after_months = Some(5);
    let m = compile(&c);
    let mut st = state(&m, ipo);
    let all = run(&m, &mut st, 0..=12, |_| {});
    let (month, e) = all.first(EqEventKind::Forfeited).unwrap();
    assert_eq!((month, e.units, e.vested), (5, 10_000.0, 10_000.0));
    assert_eq!(all.em.cash_in, 0.0);
    // So does leaving in the very month it lists: the link is cut first.
    let mut c = rsus();
    c.policy.leave_after_months = Some(6);
    let m = compile(&c);
    let all = run(&m, &mut state(&m, ipo), 0..=12, |_| {});
    assert_eq!(all.em.cash_in, 0.0);

    // Unvested units are forfeited in every status, and counted as unvested.
    let mut c = rsus();
    c.holdings[0].vested = 4_000.0;
    c.holdings[0].vest_months_left = 60;
    c.policy.leave_after_months = Some(9);
    let m = compile(&c);
    let all = run(&m, &mut state(&m, ipo), 0..=12, |_| {});
    let (_, e) = all.first(EqEventKind::Forfeited).unwrap();
    assert_close(e.units, 6_000.0 - 9.0 * 100.0, "unvested at month 9");
    assert_eq!(e.vested, 0.0);
    assert_close(
        all.em.wages_cash,
        (4_000.0 + 900.0) * 30.0,
        "the vested units",
    );
}

#[test]
fn a_window_lapse_and_a_term_lapse_are_different_events() {
    // Two grants, both vested. The link is cut in month 2 with a three-month
    // window; the second grant's term falls in month 4, before the window closes.
    let mut c = company();
    c.holdings = vec![
        holding(nso(2.0), 1_000.0, 1_000.0, 0),
        holding(
            HoldingKind::Nso {
                strike: 2.0,
                granted_months_ago: 116,
            },
            700.0,
            700.0,
            0,
        ),
    ];
    c.policy.at_deadline = DeadlinePolicy::Forfeit;
    c.policy.leave_after_months = Some(2);
    let m = compile(&c);
    let mut st = state(&m, flat(&m));
    let all = run(&m, &mut st, 0..=6, |_| {});
    let (month, expired) = all.first(EqEventKind::Expired).unwrap();
    assert_eq!((month, expired.units, expired.vested), (4, 700.0, 700.0));
    let (month, lapsed) = all.first(EqEventKind::Lapsed).unwrap();
    assert_eq!((month, lapsed.units, lapsed.vested), (5, 1_000.0, 1_000.0));
    assert_close(all.em.units_out, 1_700.0, "every option left");
}

#[test]
fn an_nso_exercise_sets_aside_the_tax_beyond_withholding() {
    // 1,000 options at $1 with a 409A value of $4: a spread of $3 each. At 50%
    // tax in all, of which 30% is withheld, each option costs 1 + 1.5.
    let mut c = company();
    c.holdings = vec![holding(nso(1.0), 1_000.0, 1_000.0, 0)];
    c.policy.exercise = ExerciseTiming::VestedNow;
    let m = compile(&c);
    assert_close(
        m.pay_spread_at_stake(&state(&m, flat(&m)), 0),
        3_000.0,
        "spread at stake",
    );
    let rates = |c: &mut EquityClimate| {
        c.wage_tax_rate = 0.5;
        c.wage_withheld_rate = 0.3;
    };
    let mut st = state(&m, flat(&m));
    let mut cl = climate(0);
    rates(&mut cl);
    let em = m.step(&mut st, &cl);
    assert_eq!(em.wages_noncash, 3_000.0);
    assert_close(em.tax_estimate, 0.2 * 3_000.0, "set aside");
    // With $1,000 to spend, 400 are exercised: 400 x (1 + 0.5 x 3).
    let mut st = state(&m, flat(&m));
    cl.liquid = 1_000.0;
    let em = m.step(&mut st, &cl);
    assert_close(em.exercise_cost, 400.0, "what is affordable, tax included");
    assert_close(em.tax_estimate, 0.2 * 400.0 * 3.0, "set aside");
    assert_close(
        em.exercise_cost + 0.3 * em.wages_noncash + em.tax_estimate,
        1_000.0,
        "strike, withholding and the rest of the tax use the money exactly",
    );
    // Withholding above the rate (never in practice) sets nothing aside.
    cl.wage_withheld_rate = 0.9;
    let em = m.step(&mut state(&m, flat(&m)), &cl);
    assert_eq!(em.tax_estimate, 0.0);

    // What is at stake, month by month. Waiting for a sale: nothing.
    let mut c = company();
    c.holdings = vec![
        holding(nso(1.0), 1_000.0, 400.0, 6),
        holding(iso(1.0), 500.0, 500.0, 0),
        holding(nso(12.0), 900.0, 900.0, 0),
    ];
    let wait = compile(&c);
    let st = state(&wait, flat(&wait));
    assert_eq!(wait.pay_spread_at_stake(&st, 0), 0.0);
    assert_eq!(wait.pay_spread_at_stake(&st, 7), 0.0);
    // Cut loose with the window closing: the vested NSOs in the money, and from
    // the fourth month after leaving the incentive options too.
    let mut st = state(&wait, flat(&wait));
    run(&wait, &mut st, 0..=0, |c| c.left_job = true);
    assert_eq!(st.deadline, 3);
    assert_eq!(wait.pay_spread_at_stake(&st, 2), 0.0);
    assert_close(
        wait.pay_spread_at_stake(&st, 3),
        400.0 * 3.0,
        "at the deadline",
    );
    st.severed_month = -1;
    assert_close(
        wait.pay_spread_at_stake(&st, 3),
        900.0 * 3.0,
        "an incentive option exercised late is pay",
    );
    // As they vest: this month's 100, from month 1 (month 0 is the exercise of
    // what is vested, plus the month's own).
    c.policy.exercise = ExerciseTiming::AsTheyVest;
    let vest = compile(&c);
    let st = state(&vest, flat(&vest));
    assert_close(vest.pay_spread_at_stake(&st, 0), 500.0 * 3.0, "month 0");
    assert_close(vest.pay_spread_at_stake(&st, 1), 100.0 * 3.0, "month 1");
    // Nothing once listed, and nothing for a company that is not counted.
    let mut listed = state(&vest, flat(&vest));
    listed.status = CompanyStatus::Lockup;
    assert_eq!(vest.pay_spread_at_stake(&listed, 0), 0.0);
    c.included = false;
    let out = compile(&c);
    assert_eq!(out.pay_spread_at_stake(&state(&out, flat(&out)), 0), 0.0);
}

/// One lot of 1,000 qualified small business shares from incentive options: basis
/// 2,000, spread at exercise 3,000, acquired at the start (the new regime).
fn qsbs_iso_lot(caps: [f64; 2]) -> (CompanyModel, CompanyState) {
    let mut c = company();
    c.qsbs = true;
    let m = CompanyModel::compile(&c, &params(), caps, START);
    let mut st = state(&m, flat(&m));
    st.lots[0] = Lot {
        shares: 1_000.0,
        cost: 2_000.0,
        amt_extra: 3_000.0,
        acquired: 0,
        granted: -30,
        amt_year: 2026,
        iso: true,
        qsbs: true,
    };
    st.n_lots = 1;
    (m, st)
}

#[test]
fn the_amt_gain_adjustment_follows_the_small_business_exclusion() {
    // Sold at $22: a regular gain of 20,000 and an AMT gain of 17,000 (measured
    // from the value at exercise). Each tax excludes its own gain.
    // Hand figures: adjustment = (AMT gain taxed) - (regular gain taxed).
    for (held, fraction) in [(20, 0.0), (36, 0.5), (48, 0.75), (60, 1.0)] {
        let (m, mut st) = qsbs_iso_lot(CAPS);
        let em = sell_all_at(&m, &mut st, held, 22.0);
        assert_close(em.qsbs_excluded, fraction * 20_000.0, "regular exclusion");
        assert_close(
            em.long_term_gain,
            (1.0 - fraction) * 20_000.0,
            "regular gain",
        );
        let amt_gain = (1.0 - fraction) * 17_000.0;
        assert_close(
            em.amt_gain_adjustment,
            amt_gain - em.long_term_gain,
            &format!("held {held}"),
        );
        assert_close(
            em.amt_gain_adjustment,
            -(1.0 - fraction) * 3_000.0,
            "the part of the spread not excluded",
        );
        assert_eq!(em.amt_adjustment, 0.0);
    }
    // The dollar cap binds on both: 5,000 excluded from each, so the whole spread
    // comes off, as with no exclusion.
    let (m, mut st) = qsbs_iso_lot([5_000.0, 5_000.0]);
    let em = sell_all_at(&m, &mut st, 60, 22.0);
    assert_close(em.qsbs_excluded, 5_000.0, "capped");
    assert_close(em.amt_gain_adjustment, -3_000.0, "cap binds on both");
    // The cap binds on the regular gain only (18,000 of 20,000; the AMT gain of
    // 17,000 fits): regular taxed 2,000, AMT taxed 0.
    let (m, mut st) = qsbs_iso_lot([18_000.0, 18_000.0]);
    let em = sell_all_at(&m, &mut st, 60, 22.0);
    assert_close(em.long_term_gain, 2_000.0, "regular gain over the cap");
    assert_close(em.amt_gain_adjustment, -2_000.0, "cap binds on one");
    // A gain smaller than the spread: the AMT sees no gain at all.
    let (m, mut st) = qsbs_iso_lot(CAPS);
    let em = sell_all_at(&m, &mut st, 36, 3.0);
    assert_close(em.long_term_gain, 500.0, "half of 1,000");
    assert_close(em.amt_gain_adjustment, -500.0, "all that is taxed");
    // A disqualifying sale and a sale in the year of exercise are untouched by it.
    let (m, mut st) = qsbs_iso_lot(CAPS);
    let em = sell_all_at(&m, &mut st, 1, 22.0);
    assert_eq!((em.amt_adjustment, em.amt_gain_adjustment), (-3_000.0, 0.0));
    let (m, mut st) = qsbs_iso_lot(CAPS);
    st.lots[0].granted = 0;
    let em = sell_all_at(&m, &mut st, 20, 22.0);
    assert_eq!(em.iso_income, 3_000.0, "within two years of the grant");
    assert_eq!((em.amt_adjustment, em.amt_gain_adjustment), (-3_000.0, 0.0));
    // Without the flag nothing changes: the whole spread comes off the gain.
    let (m, mut st) = iso_lot(-30, 2026);
    let em = sell_all_at(&m, &mut st, 60, 22.0);
    assert_eq!(em.amt_gain_adjustment, -3_000.0);
}

#[test]
fn a_failure_in_the_year_of_exercise_undoes_the_amt_adjustment() {
    // Exercised in January 2027 (month 2), failed in October 2027 (month 11): the
    // shares are taken to be disposed of within the year, so no AMT income is left.
    let mut c = company();
    c.holdings = vec![holding(iso(1.0), 1_000.0, 1_000.0, 0)];
    c.policy.exercise = ExerciseTiming::VestedNow;
    let exercised = |fail_month: i32| {
        let m = compile(&c);
        let mut st = state(&m, fate(OutcomeKind::Fails, fail_month, 1.0, &m));
        // Exercise in month 2 by hand (the policy exercises in month 0 only).
        let mut em = EquityMonth::default();
        let mut liquid = AMPLE;
        let fmv = 4.0;
        m.exercise(&mut st, 0, 1_000.0, fmv, &mut liquid, &climate(2), &mut em);
        assert_eq!(em.amt_adjustment, 3_000.0);
        assert_eq!(st.lots[0].amt_year, 2027);
        let rest = run(&m, &mut st, 3..=fail_month, |_| {});
        (em.amt_adjustment + rest.em.amt_adjustment, rest)
    };
    let (net, rest) = exercised(11);
    assert_eq!(net, 0.0, "same calendar year");
    assert_close(
        rest.em.short_term_gain,
        -1_000.0,
        "the strike is a capital loss",
    );
    assert!(rest.em.failed_now);
    // Failed the January after (month 14): the adjustment stands.
    let (net, rest) = exercised(14);
    assert_eq!(net, 3_000.0, "the year after");
    assert_close(
        rest.em.short_term_gain,
        -1_000.0,
        "the strike is a capital loss",
    );
    // Shares exercised before the plan started, earlier in its calendar year,
    // carry their spread into the first year; a failure that year undoes it too.
    let mut c = company();
    c.holdings = vec![holding(iso_shares(1.0, 3, 4.0), 1_000.0, 0.0, 0)];
    let m = compile(&c);
    assert_eq!(m.start_amt_adjustment(), 3_000.0);
    let mut st = state(&m, fate(OutcomeKind::Fails, 1, 1.0, &m));
    let all = run(&m, &mut st, 0..=1, |_| {});
    assert_eq!(all.em.amt_adjustment, -3_000.0);
}

#[test]
fn keeping_shares_does_not_give_up_the_small_business_exclusion() {
    // 1,000 shares at a basis of $1, held ten months at the start (acquired in
    // January 2026, the new regime), listed at $30 in month 1 with a four-month
    // lockup. The plan keeps its shares.
    let mut c = company();
    c.qsbs = true;
    c.holdings = vec![holding(shares(1.0, 10), 1_000.0, 0.0, 0)];
    c.policy.sell_down = SellDown::Hold;
    c.lockup_months = 4;
    let m = compile(&c);
    let ipo = fate(OutcomeKind::Ipo, 1, 3.0, &m);
    let mut st = state(&m, ipo);
    // To month 49 the lot has been held 59 months: it waits, as paper value.
    let waiting = run(&m, &mut st, 0..=49, |_| {});
    assert_eq!(waiting.em.to_sleeve_value, 0.0);
    assert_eq!(waiting.count(EqEventKind::LockupKept), 0);
    assert_eq!(waiting.em.cash_in, 0.0);
    assert_eq!((st.status, st.n_lots), (CompanyStatus::Public, 1));
    assert_close(waiting.em.paper_value, 30_000.0, "still on paper");
    // In month 50 it has been held five years: sold, the whole gain excluded.
    let sold = run(&m, &mut st, 50..=50, |_| {});
    assert_close(sold.em.share_proceeds, 30_000.0, "sold");
    assert_close(sold.em.qsbs_excluded, 29_000.0, "excluded in full");
    assert_eq!(sold.em.long_term_gain, 0.0);
    assert_eq!(sold.em.to_sleeve_value, 0.0);
    assert!(sold.em.replan);
    assert_eq!(
        sold.first(EqEventKind::LockupSale).unwrap().1.units,
        1_000.0
    );
    assert_eq!(st.status, CompanyStatus::Done);

    // Without the flag the lot moves when the lockup ends, as before.
    c.qsbs = false;
    let m = compile(&c);
    let mut st = state(&m, ipo);
    let all = run(&m, &mut st, 0..=5, |_| {});
    assert_close(all.em.to_sleeve_value, 30_000.0, "moved at lockup end");
    assert_eq!(st.status, CompanyStatus::Done);
    // With the cap used up there is nothing to wait for either.
    c.qsbs = true;
    let m = CompanyModel::compile(&c, &params(), [0.0, 0.0], START);
    let mut st = state(&m, ipo);
    let all = run(&m, &mut st, 0..=5, |_| {});
    assert_close(all.em.to_sleeve_value, 30_000.0, "nothing left to exclude");
    // Nor for shares under water.
    let m = compile(&c);
    let mut st = state(&m, fate(OutcomeKind::Ipo, 1, 0.05, &m));
    let all = run(&m, &mut st, 0..=5, |_| {});
    assert_close(all.em.to_sleeve_value, 500.0, "no gain to exclude");
    // A lot already past five years is sold when the lockup ends.
    c.holdings = vec![holding(shares(1.0, 70), 1_000.0, 0.0, 0)];
    let m = compile(&c);
    let mut st = state(&m, ipo);
    let all = run(&m, &mut st, 0..=5, |_| {});
    assert_close(all.em.qsbs_excluded, 29_000.0, "excluded at lockup end");
    let (month, sale) = all.first(EqEventKind::LockupSale).unwrap();
    assert_eq!((month, sale.units), (5, 1_000.0));
}

/// What the model pays on each of the preview's draws, sorted: cash in less strike
/// paid, never below zero, with ample cash and no inflation.
fn stepped_payoffs(c: &PrivateCompany, p: &EquityParams, draws: u32) -> Vec<f64> {
    let m = CompanyModel::compile(c, p, CAPS, START);
    let mut out = Vec::new();
    for i in 0..draws {
        let mut rng = Rng::stream(PREVIEW_SEED, u64::from(i), Stream::PrivateCompany(0));
        let mut st = m.begin(Link::Active, &mut rng, None);
        // To the payout: the event's month, or the lockup's end for a listing.
        // (Units that vest after a sale are paid as they vest; the preview counts
        // what has vested by the sale.)
        let last = if st.fate.kind == OutcomeKind::Ipo {
            st.fate.month + m.lockup_months
        } else {
            st.fate.month
        };
        let all = run(&m, &mut st, 0..=last, |_| {});
        out.push((all.em.cash_in - all.em.exercise_cost).max(0.0));
    }
    out.sort_by(f64::total_cmp);
    out
}

#[test]
fn the_preview_applies_the_option_deadlines_as_the_model_does() {
    const DRAWS: u32 = 2_000;
    let mut p = params();
    p.preview_inflation = 0.0;
    let dots_of = |sorted: &[f64]| -> Vec<f64> {
        (0..100)
            .map(|i| sorted[((f64::from(i) + 0.5) / 100.0 * sorted.len() as f64) as usize])
            .collect()
    };
    let mut former = company();
    former.job = None;
    former.tenders.annual_probability = Some(0.0);
    former.holdings = vec![
        holding(iso(1.5), 60_000.0, 60_000.0, 0),
        holding(nso(12.0), 5_000.0, 5_000.0, 0),
    ];
    let mut leaving = company();
    leaving.tenders.annual_probability = Some(0.0);
    leaving.policy.leave_after_months = Some(10);
    leaving.holdings = vec![
        // Its ten-year term falls in month 20, after the window closes in month 13.
        holding(
            HoldingKind::Nso {
                strike: 2.0,
                granted_months_ago: 100,
            },
            8_000.0,
            2_000.0,
            40,
        ),
        // Its term falls in month 4, before the departure.
        holding(
            HoldingKind::Iso {
                strike: 3.0,
                granted_months_ago: 116,
            },
            4_000.0,
            1_000.0,
            30,
        ),
        holding(
            HoldingKind::Rsu {
                must_be_employed_at_event: true,
            },
            3_000.0,
            1_500.0,
            15,
        ),
        holding(rsu(), 2_000.0, 500.0, 15),
        holding(shares(1.0, 20), 700.0, 0.0, 0),
    ];
    let mut staying = leaving.clone();
    staying.policy.leave_after_months = None;
    // (Staying past a sale keeps vesting and is paid as it vests; the preview
    // counts what has vested by the sale. Vest everything so the two agree.)
    for h in &mut staying.holdings {
        h.vested = h.shares;
        h.vest_months_left = 0;
    }
    for (name, base) in [
        ("former", &former),
        ("leaving", &leaving),
        ("staying", &staying),
    ] {
        for at_deadline in [DeadlinePolicy::Forfeit, DeadlinePolicy::Exercise] {
            let mut c = base.clone();
            c.policy.at_deadline = at_deadline;
            let pv = preview(&c, &p, DRAWS);
            let stepped = stepped_payoffs(&c, &p, DRAWS);
            let nothing = stepped.iter().filter(|v| **v <= 0.0).count() as f64 / f64::from(DRAWS);
            assert_close(
                pv.nothing_share,
                nothing,
                &format!("{name} {at_deadline:?}: nothing"),
            );
            for (i, (a, b)) in pv.dots.iter().zip(dots_of(&stepped)).enumerate() {
                assert!(
                    (a - b).abs() <= 1e-6 * (1.0 + b.abs()),
                    "{name} {at_deadline:?}: dot {i} is {a} in the preview, {b} stepped"
                );
            }
        }
    }

    // A former employer under "let them lapse": the options can never pay, and the
    // preview says so (it used to show the payout of someone who stays).
    let p = params();
    former.policy.at_deadline = DeadlinePolicy::Forfeit;
    let pv = preview(&former, &p, DRAWS);
    assert_eq!(pv.nothing_share, 1.0);
    assert_eq!(pv.quantiles, [0.0; 5]);
    assert_eq!(pv.option_deadline_months, Some(3));
    assert_eq!(pv.option_deadline_cost, 0.0);
    assert!(pv.deadline_before_outcome_share > 0.99);
    // Exercising at the deadline: the cash needed is reported, for the options in
    // the money only.
    former.policy.at_deadline = DeadlinePolicy::Exercise;
    let pv = preview(&former, &p, DRAWS);
    assert_eq!(pv.option_deadline_months, Some(3));
    assert_eq!(pv.option_deadline_cost, 60_000.0 * 1.5);
    assert!(pv.nothing_share < 1.0 && pv.quantiles[4] > 0.0);

    // The ten-year term for someone who stays. A grant made 119 months ago lapses
    // next month, one made 40 months ago in month 80.
    let grant = |ago: u32, at_deadline| {
        let mut c = company();
        c.holdings = vec![holding(
            HoldingKind::Iso {
                strike: 1.5,
                granted_months_ago: ago,
            },
            60_000.0,
            60_000.0,
            0,
        )];
        c.policy.at_deadline = at_deadline;
        preview(&c, &p, DRAWS)
    };
    let fresh = grant(0, DeadlinePolicy::Forfeit);
    let mid = grant(40, DeadlinePolicy::Forfeit);
    let old = grant(119, DeadlinePolicy::Forfeit);
    assert_eq!(
        (
            fresh.option_deadline_months,
            mid.option_deadline_months,
            old.option_deadline_months
        ),
        (Some(120), Some(80), Some(1))
    );
    assert!(fresh.nothing_share < mid.nothing_share && mid.nothing_share < old.nothing_share);
    assert_eq!(old.nothing_share, 1.0);
    assert!(mid.deadline_before_outcome_share > 0.05 && mid.deadline_before_outcome_share < 0.5);
    // RSUs and shares have no deadline.
    let mut c = company();
    c.holdings = vec![
        holding(rsu(), 100.0, 100.0, 0),
        holding(shares(1.0, 5), 10.0, 0.0, 0),
    ];
    let pv = preview(&c, &p, 200);
    assert_eq!(pv.option_deadline_months, None);
    assert_eq!(pv.deadline_before_outcome_share, 0.0);
}

#[test]
fn the_preview_refuses_a_company_the_simulation_would_refuse() {
    let bare = r#"{"stage": "series_c", "preferred_price": 1}"#;
    assert!(preview_json(bare, 100).is_ok(), "a bare company is valid");
    let with = |edit: &str| {
        format!(
            r#"{{"stage": "series_c", "preferred_price": 14, "holdings": [{{"id": "g", "shares": 1000, "vested": 1000, "kind": {edit}}}]}}"#
        )
    };
    assert!(preview_json(&with(r#"{"type": "nso", "strike": 4}"#), 100).is_ok());
    for (kind, path, word) in [
        (
            r#"{"type": "nso", "strike": -4}"#,
            "holdings/@g/kind/strike",
            "Strike",
        ),
        (
            r#"{"type": "nso", "strike": 4, "granted_months_ago": 130}"#,
            "holdings/@g/kind/granted_months_ago",
            "ten years",
        ),
        (
            r#"{"type": "shares", "basis_per_share": 20000000}"#,
            "holdings/@g/kind/basis_per_share",
            "per share",
        ),
    ] {
        let e = preview_json(&with(kind), 100).unwrap_err();
        assert!(e.starts_with(path) && e.contains(word), "{e}");
    }
    for (company, path) in [
        (
            r#"{"preferred_price": -1, "holdings": [{"id": "s", "shares": 5, "kind": {"type": "rsu"}}]}"#,
            "preferred_price",
        ),
        (
            r#"{"preferred_price": 1, "holdings": [{"id": "s", "shares": -5, "kind": {"type": "rsu"}}]}"#,
            "holdings/@s/shares",
        ),
        (
            r#"{"preferred_price": 1, "tenders": {"price_fraction": 0}}"#,
            "tenders/price_fraction",
        ),
        (
            r#"{"preferred_price": 1, "outcomes": [{"id": "a", "kind": "ipo", "probability": 1.5, "multiple_low": 1, "multiple_high": 2}]}"#,
            "outcomes/@a/probability",
        ),
    ] {
        let e = preview_json(company, 100).unwrap_err();
        assert!(e.starts_with(path), "{e}");
    }
    // More holdings or rows than the engine carries.
    let many: Vec<String> = (0..9)
        .map(|i| {
            format!(r#"{{"id": "h{i}", "shares": 1, "kind": {{"type": "rsu"}}, "vested": 1}}"#)
        })
        .collect();
    let e = preview_json(
        &format!(
            r#"{{"preferred_price": 1, "holdings": [{}]}}"#,
            many.join(",")
        ),
        100,
    )
    .unwrap_err();
    assert!(e.starts_with("holdings") && e.contains("at most 8"), "{e}");
}
