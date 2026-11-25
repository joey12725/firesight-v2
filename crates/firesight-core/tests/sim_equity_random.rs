//! Random private-company positions on every demo household, run with the
//! conservation checks on (`Workspace::set_check`): one to four companies with random
//! stages, outcome tables, holdings of every kind, tender terms and policies, under
//! current law and a flat tax and in every market model. The generator is a fixed
//! linear congruential sequence, so the cases are the same on every run.
//!
//! Besides the four identities asserted inside the month loop, every life's equity
//! scalars must be finite and consistent with each other. Net proceeds have no
//! simple lower bound: the minimum tax on a large incentive-option exercise can be
//! many times the strike paid, and it is counted against the equity.
//!
//! `EQUITY_RANDOM_CASES` and `EQUITY_RANDOM_PATHS` widen the run (the defaults take
//! a few seconds); 4,000 cases of 60 lives passed when the test was written.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::stats::PathScalar;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn unit(&mut self) -> f64 {
        (self.next() % 1_000_000) as f64 / 1_000_000.0
    }
    fn flip(&mut self) -> bool {
        self.below(2) == 0
    }
}

fn random_company(r: &mut Lcg, id: &str, job: Option<String>, owner: usize) -> PrivateCompany {
    let stages = [
        CompanyStage::Seed,
        CompanyStage::SeriesA,
        CompanyStage::SeriesB,
        CompanyStage::SeriesC,
        CompanyStage::Late,
        CompanyStage::Unicorn,
    ];
    let p0 = [0.5, 3.0, 12.0, 80.0][r.below(4) as usize];
    let mut co = PrivateCompany {
        id: id.into(),
        name: format!("Co {id}"),
        owner,
        job: job.clone(),
        included: r.below(8) != 0,
        stage: stages[r.below(6) as usize],
        preferred_price: p0,
        fmv_409a: if r.flip() {
            Some(p0 * (0.1 + 0.8 * r.unit()))
        } else {
            None
        },
        preference_overhang: if r.flip() { Some(0.9 * r.unit()) } else { None },
        outlook: [Outlook::Pessimistic, Outlook::Typical, Outlook::Optimistic][r.below(3) as usize],
        lockup_months: [0, 6, 6, 12][r.below(4) as usize],
        exercise_window_months: if job.is_some() {
            [0, 3, 3, 24, 84][r.below(5) as usize]
        } else {
            [1, 3, 12, 60][r.below(4) as usize]
        },
        company_failure_ends_job: r.flip(),
        qsbs: r.flip(),
        amt_credit_carryforward: if r.below(4) == 0 {
            20_000.0 * r.unit()
        } else {
            0.0
        },
        ..PrivateCompany::default()
    };
    co.tenders.annual_probability = [None, Some(0.0), Some(0.5), Some(1.0)][r.below(4) as usize];
    co.tenders.next_in_months = if r.below(3) == 0 {
        Some(r.below(40) as u32)
    } else {
        None
    };
    co.tenders.max_fraction = [0.2, 0.5, 1.0][r.below(3) as usize];
    co.tenders.price_fraction = [1.0, 0.8, 1.3][r.below(3) as usize];
    co.tenders.former_employees = r.flip();
    match r.below(4) {
        0 => {}
        1 => {
            // Multi-row custom table.
            co.outcomes = vec![
                OutcomeRow {
                    id: "f".into(),
                    kind: OutcomeKind::Fails,
                    probability: 0.25,
                    years_from: 0.0,
                    years_to: 4.0,
                    ..OutcomeRow::default()
                },
                OutcomeRow {
                    id: "a".into(),
                    kind: OutcomeKind::Acquired,
                    probability: 0.3,
                    years_from: 0.5,
                    years_to: 12.0,
                    multiple_low: 0.05,
                    multiple_high: 6.0,
                },
                OutcomeRow {
                    id: "i".into(),
                    kind: OutcomeKind::Ipo,
                    probability: 0.3,
                    years_from: 0.0,
                    years_to: 6.0,
                    multiple_low: 0.3,
                    multiple_high: 30.0,
                },
                OutcomeRow {
                    id: "s".into(),
                    kind: OutcomeKind::StillPrivate,
                    probability: 0.15,
                    ..OutcomeRow::default()
                },
            ];
        }
        2 => {
            let kind =
                [OutcomeKind::Ipo, OutcomeKind::Acquired, OutcomeKind::Fails][r.below(3) as usize];
            let from = 5.0 * r.unit();
            co.outcomes = certain(kind, from, from + 3.0 * r.unit(), 0.2 + 8.0 * r.unit());
        }
        _ => {
            co.outcomes = certain(OutcomeKind::StillPrivate, 0.0, 0.0, 1.0);
        }
    }
    let n = r.below(5) as usize + usize::from(r.below(6) != 0);
    for h in 0..n {
        let shares = [500.0, 20_000.0, 150_000.0, 1_000_000.0][r.below(4) as usize];
        let strike = p0 * [0.01, 0.2, 0.6, 1.0, 1.5][r.below(5) as usize];
        let granted = [0, 12, 30, 100, 118][r.below(5) as usize];
        let kind = match r.below(4) {
            0 => HoldingKind::Iso {
                strike,
                granted_months_ago: granted,
            },
            1 => HoldingKind::Nso {
                strike,
                granted_months_ago: granted,
            },
            2 => HoldingKind::Rsu {
                must_be_employed_at_event: r.flip(),
            },
            _ => {
                let iso = r.flip();
                HoldingKind::Shares {
                    basis_per_share: strike,
                    months_held: [0, 2, 9, 14, 30, 70][r.below(6) as usize],
                    from_iso: iso,
                    fmv_at_exercise: if iso && r.flip() { strike * 3.0 } else { 0.0 },
                }
            }
        };
        let unvested_ok = job.is_some() && !kind.is_shares();
        let (vested, left, cliff) = if unvested_ok && r.below(3) != 0 {
            let left = [1, 12, 36, 48][r.below(4) as usize];
            let cliff = if r.flip() { 0 } else { left.min(9) };
            ((shares * r.unit()).floor(), left, cliff)
        } else {
            (shares, 0, 0)
        };
        co.holdings.push(EquityHolding {
            id: format!("h{h}"),
            name: format!("Holding {h}"),
            kind,
            shares,
            vested,
            vest_months_left: left,
            cliff_months: cliff,
        });
    }
    co.policy.exercise = [
        ExerciseTiming::AtLiquidity,
        ExerciseTiming::VestedNow,
        ExerciseTiming::AsTheyVest,
    ][r.below(3) as usize];
    co.policy.at_deadline = if r.flip() {
        DeadlinePolicy::Exercise
    } else {
        DeadlinePolicy::Forfeit
    };
    co.policy.tender_sell_fraction = [0.0, 0.5, 1.0][r.below(3) as usize];
    co.policy.sell_down = match r.below(3) {
        0 => SellDown::AllAtLockupEnd,
        1 => SellDown::OverYears {
            years: 1 + r.below(5) as u32,
        },
        _ => SellDown::Hold,
    };
    co.policy.leave_after_months = if r.below(4) == 0 {
        Some(r.below(60) as u32)
    } else {
        None
    };
    co
}

#[test]
fn random_positions_conserve_money_and_report_sane_scalars() {
    let cases: u64 = std::env::var("EQUITY_RANDOM_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(240);
    let paths: u64 = std::env::var("EQUITY_RANDOM_PATHS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let examples = Profile::examples(START);
    let mut r = Lcg(0x9E3779B97F4A7C15);
    let mut with_cost = 0u64;
    let mut with_gross = 0u64;
    let mut total = 0u64;
    for case in 0..cases {
        let base = &examples[(case % examples.len() as u64) as usize];
        let mut p = base.profile.clone();
        p.private_equity.clear();
        let people = p.household.people.len();
        let n = 1 + r.below(4) as usize;
        for i in 0..n {
            let owner = r.below(people as u64) as usize;
            let job = p
                .incomes
                .iter()
                .find(|inc| matches!(inc.kind, IncomeKind::Job(_)) && inc.owner == owner)
                .map(|inc| inc.id.clone())
                .filter(|_| r.below(4) != 0);
            p.private_equity
                .push(random_company(&mut r, &format!("c{i}"), job, owner));
        }
        match r.below(6) {
            0 => p.assumptions.taxes.kind = TaxKind::Flat { rate: 0.25 },
            1 => p.assumptions.market.kind = MarketKind::Regime,
            2 => p.assumptions.market.kind = MarketKind::Lognormal,
            _ => {}
        }
        let report = firesight_core::validate::validate(&p);
        assert!(
            report.errors.is_empty(),
            "case {case}: generator made an invalid profile: {:#?}",
            report.errors
        );
        let plan = compile(&p);
        let acc = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_checked(&plan, 100 + case, 0..paths)
        }))
        .unwrap_or_else(|_| {
            panic!(
                "case {case} ({}) panicked with:\n{}",
                base.slug,
                serde_json::to_string(&p.private_equity).unwrap()
            )
        });
        assert_eq!(acc.paths, paths);
        let any_included_holding = p
            .private_equity
            .iter()
            .any(|c| c.included && !c.holdings.is_empty());
        // Basis of shares held at the start: a loss on them saves tax.
        let basis0: f64 = p
            .private_equity
            .iter()
            .filter(|c| c.included)
            .flat_map(|c| c.holdings.iter())
            .map(|h| match h.kind {
                HoldingKind::Shares {
                    basis_per_share, ..
                } => h.shares * basis_per_share,
                _ => 0.0,
            })
            .sum();
        for (path, row) in acc.rows() {
            total += 1;
            let get = |s: PathScalar| f64::from(row[s.index()]);
            let outcome = get(PathScalar::EquityOutcome);
            let gross = get(PathScalar::EquityGrossProceeds);
            let cost = get(PathScalar::EquityExerciseCost);
            let tender = get(PathScalar::EquityTenderProceeds);
            let net = get(PathScalar::EquityNetProceeds);
            let age = get(PathScalar::EquityEventAge);
            let ctx = || {
                format!(
                    "case {case} ({}) path {path}: outcome {outcome} gross {gross} cost {cost} tender {tender} net {net}\n{}",
                    base.slug,
                    serde_json::to_string(&p.private_equity).unwrap()
                )
            };
            assert!(
                (0.0..=3.0).contains(&outcome) && outcome.fract() == 0.0,
                "{}",
                ctx()
            );
            assert!(gross.is_finite() && gross >= 0.0, "{}", ctx());
            assert!(cost.is_finite() && cost >= 0.0, "{}", ctx());
            assert!(tender.is_finite() && tender >= 0.0, "{}", ctx());
            assert!(tender <= gross * 1.0001 + 1.0, "{}", ctx());
            assert!(net.is_finite(), "{}", ctx());
            // Net proceeds cannot be far above gross: only the tax saved by a loss
            // (strikes paid, or the basis of shares held at the start) adds to them.
            assert!(
                net <= gross * 1.0001 + 0.6 * (cost + basis0) + 60_000.0,
                "{}",
                ctx()
            );
            if outcome == 1.0 {
                assert!(age.is_nan(), "{}", ctx());
            } else {
                assert!(age.is_finite() && age > 0.0, "{}", ctx());
            }
            if !any_included_holding {
                assert!(gross == 0.0 && cost == 0.0 && tender == 0.0, "{}", ctx());
            }
            with_cost += u64::from(cost > 0.0);
            with_gross += u64::from(gross > 0.0);
            let tnw = get(PathScalar::TerminalNetWorth);
            // (A depleted household can end a little below zero: unpaid bills are an
            // overdraft.)
            assert!(tnw.is_finite(), "{}", ctx());
        }
    }
    eprintln!("{total} lives, {with_gross} with proceeds, {with_cost} with an exercise");
    assert!(with_gross > total / 20 && with_cost > total / 50);
}
