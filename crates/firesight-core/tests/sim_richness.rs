//! The optional realism features together.
//!
//! Each feature has its own tests next to the code that wrote it. These tests switch
//! them on in combination, on the shipped example households: every feature at once
//! (in the combinations that are compatible) and each group of five on its own. For
//! every such profile the run completes with the conservation checks on, no value is
//! NaN or infinite, the result does not depend on how the paths are chunked or in
//! which order the chunks are merged, and the result differs from the same household
//! with the features off.
//!
//! Then each of the fifty features on its own: it changes a result somewhere, and (the
//! stream audit) switching it on does not move the market, career, health or lifespan
//! draws of the household it is switched on for. The last tests read the order of
//! events on paths with everything on, for contradictions between features that were
//! written apart from one another, and run every pair of features (ignored: slow).

mod behavior_profiles;
mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::rng::{Rng, Stream};
use firesight_core::sim::{self, Workspace};
use firesight_core::stats::{
    Accumulator, MonthRecord, PathEnd, PathScalar, Recorder, Status, TraceEvent, YearRecord,
    YearSeries,
};
use firesight_core::validate::validate;

const PATHS: u64 = 300;
const SEED: u64 = 50;

// ---- The switches, by group ----------------------------------------------------------

fn bootstrap() -> MarketKind {
    MarketKind::Bootstrap {
        mean_block_months: 60,
        from_year: None,
    }
}

fn arm() -> MortgageKind {
    MortgageKind::Arm {
        fixed_months: 24,
        adjust_every_months: 12,
        margin: 0.0275,
        periodic_cap: 0.02,
        lifetime_cap: 0.05,
    }
}

fn is_couple(p: &Profile) -> bool {
    p.household.people.len() == 2
}

/// A shipped example with the furniture the switches need something to act on, and
/// nothing switched on: a state with detailed rules, a giving line, a planned child,
/// and for a couple a wedding date. This is the baseline each variant is compared
/// with.
fn furnished(slug: &str) -> Profile {
    let mut p = Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == slug)
        .unwrap_or_else(|| panic!("no example {slug}"))
        .profile;
    p.household.location.state = Some(
        match slug {
            "dual-income-family" => "CA",
            "early-career-saver" => "NY",
            _ => "UT",
        }
        .into(),
    );
    if !p
        .spending
        .items
        .iter()
        .any(|i| i.category == Category::Giving)
    {
        p.spending
            .items
            .push(item("giving", Category::Giving, 150.0, false));
    }
    if p.household.children.is_empty() {
        p.household.children.push(Child {
            id: "planned".into(),
            name: "Planned child".into(),
            birth: START + 30,
            college: College::PublicInState,
            ..Child::default()
        });
    }
    if is_couple(&p) {
        p.household.married = Some(START - 48);
    }
    p.simulation.paths = PATHS as u32;
    p
}

/// F01 to F05.
fn spending_on(mut p: Profile) -> Profile {
    p.spending.vehicles = vec![
        Vehicle {
            id: "car".into(),
            name: "Car".into(),
            price: 36_000.0,
            replace_every_years: 10.0,
            age_years: 4.0,
            ..Vehicle::default()
        },
        Vehicle {
            id: "second".into(),
            name: "Second car".into(),
            price: 30_000.0,
            replace_every_years: 9.0,
            age_years: 6.0,
            financing: Some(VehicleFinancing::default()),
            ..Vehicle::default()
        },
    ];
    p.spending.seasonality = Seasonality::Typical;
    p.spending.category_inflation = CategoryInflation::Stochastic;
    p.spending.shocks = ShockLevel::Detailed;
    p.spending.unemployment_cut = 0.15;
    p
}

/// F06 to F10.
fn children_on(mut p: Profile) -> Profile {
    let start = p.start.unwrap();
    let couple = is_couple(&p);
    p.spending.healthcare.cover_children = true;
    for c in &mut p.household.children {
        c.college_cost = CollegeCost::Stochastic {
            attend_probability: DEFAULT_ATTEND_PROBABILITY,
            real_tuition_growth: DEFAULT_REAL_TUITION_GROWTH,
            tuition_volatility: DEFAULT_TUITION_VOLATILITY,
            aid_scale: 1.0,
        };
        c.adult_support = Some(AdultSupport::default());
        if c.birth > start {
            c.arrival = Arrival::Uncertain {
                probability: 0.85,
                delay_mean_months: 4.0,
                twins_probability: 0.2,
            };
            c.parental_leave = Some(ParentalLeave::default());
            if couple {
                c.second_parental_leave = Some(ParentalLeave {
                    person: 1,
                    months: 1,
                    pay_fraction: 1.0,
                });
            }
        }
    }
    p
}

/// F11 to F15.
fn housing_costs_on(mut p: Profile) -> Profile {
    let cap = Assessment::Capped {
        annual_cap: 0.02,
        assessed_value: None,
    };
    for pr in &mut p.properties {
        pr.maintenance = MaintenanceModel::Lumpy;
        pr.assessment = cap;
        pr.insurance_real_growth = InsuranceGrowth::Stochastic;
        if let Some(m) = &mut pr.mortgage {
            m.kind = arm();
        }
    }
    for ev in &mut p.events {
        if let EventKind::BuyHome(b) = &mut ev.kind {
            b.maintenance = MaintenanceModel::Lumpy;
            b.assessment = cap;
            b.insurance_real_growth = InsuranceGrowth::Stochastic;
            b.mortgage_kind = arm();
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

/// F16 to F20.
fn career_on(p: Profile) -> Profile {
    let mut p = career_realism_on(p);
    p.assumptions.career_risk = true;
    p
}

/// F21 to F25.
fn tax_policy_on(mut p: Profile) -> Profile {
    p.assumptions.taxes.chained_cpi = true;
    p.assumptions.taxes.state_detail = StateDetail::Full;
    p.assumptions.taxes.policy_risk = Some(PolicyRisk::default());
    p.portfolio.tax_loss_harvesting = true;
    p.retirement.qcd = true;
    p
}

/// F26 or F27 (`reverse`), and F28 to F30.
fn home_finance_on(p: Profile, reverse: bool) -> Profile {
    if reverse {
        with_housing_decisions_hecm(p)
    } else {
        with_housing_decisions(p)
    }
}

/// F31 to F35.
fn health_on(p: Profile) -> Profile {
    care::all_care(p)
}

/// F36 to F40 in the bootstrap (`fat_tails` false), or F37, F39 and F40 in the
/// fat-tailed model, where bond pricing from the yield does not apply.
fn markets_on(mut p: Profile, fat_tails: bool) -> Profile {
    let m = &mut p.assumptions.market;
    m.valuation = Some(Valuation::default());
    m.dynamic_yields = true;
    if fat_tails {
        m.kind = MarketKind::FatTailed;
    } else {
        m.kind = bootstrap();
        m.bond_pricing = BondPricing::FromYield;
        m.stress.push(Stress::HistoricalEpisode {
            start: When::date(START + 40),
            episode: Episode::Financial2008,
        });
        m.stress.push(Stress::LostGeneration {
            start: When::retirement(8.0),
            years: 20.0,
        });
    }
    p
}

/// F41 to F45.
fn behaviour_on(p: Profile) -> Profile {
    behavior_profiles::with_behaviors(p)
}

/// F46 to F50.
fn household_on(mut p: Profile) -> Profile {
    let couple = is_couple(&p);
    if couple {
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
    }
    let oldest = p.household.people.iter().map(|x| x.birth).min().unwrap();
    p.household.parents = vec![Parent {
        id: "mother".into(),
        name: "Mother".into(),
        child: 0,
        birth: oldest - 30 * 12,
        sex: Sex::Female,
        estate: 450_000.0,
        share: 0.5,
        inherited_ira_fraction: 0.5,
        care_support: Some(CareSupport {
            probability: 0.6,
            annual_cost: 7_242.0,
            hours: 0.5,
            caregiver: None,
        }),
        ..Parent::default()
    }];
    if couple {
        p.household.parents.push(Parent {
            id: "father_in_law".into(),
            name: "Father-in-law".into(),
            child: 1,
            birth: oldest - 34 * 12,
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
        });
    }
    for (k, person) in p.household.people.iter_mut().enumerate() {
        person.life_insurance = vec![if k == 0 {
            LifePolicy {
                id: "term".into(),
                name: "Term life".into(),
                kind: PolicyKind::Term {
                    years_remaining: 20.0,
                },
                death_benefit: 1_000_000.0,
                annual_premium: 1_100.0,
            }
        } else {
            LifePolicy {
                id: "whole".into(),
                name: "Whole life".into(),
                kind: PolicyKind::Permanent,
                death_benefit: 250_000.0,
                annual_premium: 3_000.0,
            }
        }];
    }
    p
}

/// Every feature at once. The two flavours cover the options that exclude one
/// another: a home-equity line with the bootstrap extensions, or a reverse mortgage
/// with the fat-tailed market.
fn everything_on(p: Profile, second_flavour: bool) -> Profile {
    let p = spending_on(p);
    let p = children_on(p);
    let p = housing_costs_on(p);
    let p = career_on(p);
    let p = tax_policy_on(p);
    let p = home_finance_on(p, second_flavour);
    let p = health_on(p);
    let p = markets_on(p, second_flavour);
    let p = behaviour_on(p);
    household_on(p)
}

/// `(label, baseline, with the features on)` for one example.
fn variants(slug: &str) -> Vec<(String, Profile, Profile)> {
    let base = furnished(slug);
    let b = || base.clone();
    let list: Vec<(&str, Profile)> = vec![
        ("spending", spending_on(b())),
        ("children", children_on(b())),
        ("housing costs", housing_costs_on(b())),
        ("career", career_on(b())),
        ("tax and policy", tax_policy_on(b())),
        ("home finance (line of credit)", home_finance_on(b(), false)),
        (
            "home finance (reverse mortgage)",
            home_finance_on(b(), true),
        ),
        ("health and mortality", health_on(b())),
        ("markets (bootstrap)", markets_on(b(), false)),
        ("markets (fat tails)", markets_on(b(), true)),
        ("behaviour", behaviour_on(b())),
        ("household", household_on(b())),
        ("everything (first flavour)", everything_on(b(), false)),
        ("everything (second flavour)", everything_on(b(), true)),
    ];
    list.into_iter()
        .map(|(label, on)| (format!("{slug}: {label}"), base.clone(), on))
        .collect()
}

const SLUGS: [&str; 3] = [
    "dual-income-family",
    "early-career-saver",
    "side-hustle-landlord",
];

// ---- The checks -------------------------------------------------------------------------

/// Forwards to an accumulator after checking every number.
struct Checked<'a> {
    inner: Accumulator,
    label: &'a str,
    path: u64,
    detail: bool,
    months: u64,
}

impl Recorder for Checked<'_> {
    fn begin_path(&mut self, path_index: u64) {
        self.path = path_index;
        self.inner.begin_path(path_index);
    }

    fn record_year(&mut self, year_index: usize, record: &YearRecord) {
        let at = |s: &YearSeries| {
            format!(
                "{} path {} year {year_index} {}",
                self.label,
                self.path,
                s.key()
            )
        };
        assert!((1..=12).contains(&record.months));
        assert_ne!(record.status, Status::Dead);
        for (s, v) in YearSeries::ALL.iter().zip(record.values) {
            assert!(v.is_finite(), "{}: {v}", at(s));
            assert!(
                v >= -1.0 || *s == YearSeries::InvestmentReturn,
                "{} is negative: {v}",
                at(s)
            );
            assert!(v.abs() < 1e13, "{} = {v}", at(s));
        }
        assert!(
            record.cash_flow_residual().abs() < 1.0 + 1e-9 * record.gross_income().abs(),
            "{} path {} year {year_index}: cash-flow residual {}",
            self.label,
            self.path,
            record.cash_flow_residual()
        );
        self.inner.record_year(year_index, record);
    }

    fn end_path(&mut self, end: &PathEnd) {
        for s in PathScalar::ALL {
            let v = end.get(*s);
            assert!(
                !v.is_infinite(),
                "{} path {} scalar {}: {v}",
                self.label,
                self.path,
                s.key()
            );
        }
        for s in [
            PathScalar::DeathAge0,
            PathScalar::WorstDrawdown,
            PathScalar::MonthsUnemployed,
            PathScalar::LongTermCareCost,
            PathScalar::Inflation,
            PathScalar::Success,
        ] {
            assert!(
                end.get(s).is_finite(),
                "{} path {} scalar {}",
                self.label,
                self.path,
                s.key()
            );
        }
        let success = end.get(PathScalar::Success);
        assert!(success == 0.0 || success == 1.0);
        assert_eq!(success == 1.0, end.get(PathScalar::FailureCause).is_nan());
        self.inner.end_path(end);
    }

    fn wants_detail(&self) -> bool {
        self.detail
    }

    fn record_month(&mut self, record: &MonthRecord) {
        self.months += 1;
        assert!(record.price_level.is_finite() && record.price_level > 0.0);
        assert!(record.portfolio_return.is_finite() && record.portfolio_return >= -1.0);
        for v in record.values.iter().chain(&record.asset_returns) {
            assert!(v.is_finite(), "{} path {}", self.label, self.path);
        }
    }

    fn record_event(&mut self, event: TraceEvent) {
        assert!(event.amount.map_or(true, f64::is_finite), "{event:?}");
        assert!(!event.label.is_empty());
    }
}

/// Runs every check on one profile and returns the bytes of its aggregate.
fn check(label: &str, profile: &Profile) -> Vec<u8> {
    let report = validate(profile);
    assert!(report.is_ok(), "{label}: {:#?}", report.errors);
    // The profile survives its own JSON.
    let json = profile.to_json_pretty();
    let back = Profile::from_json(&json).unwrap_or_else(|e| panic!("{label}: {e}"));
    assert_eq!(&back, profile, "{label}: JSON round trip");

    let plan = compile(profile);
    // Conservation checks on, every recorded number checked.
    let mut ws = Workspace::new(&plan);
    ws.set_check(true);
    let mut rec = Checked {
        inner: plan.new_accumulator(),
        label,
        path: 0,
        detail: false,
        months: 0,
    };
    for i in 0..PATHS {
        sim::run_path(&plan, SEED, i, &mut ws, &mut rec);
    }
    let whole = rec.inner.to_bytes();
    let summary = plan.summarize(&rec.inner);
    assert_eq!(summary.paths, PATHS);
    let text = serde_json::to_string(&summary).unwrap();
    assert!(
        !text.contains("NaN") && !text.contains("Infinity") && !text.contains(":inf"),
        "{label}: a non-number in the summary"
    );

    // A few paths in detail, and one as a trace.
    let mut detail = Checked {
        inner: plan.new_accumulator(),
        label,
        path: 0,
        detail: true,
        months: 0,
    };
    for i in 0..12 {
        sim::run_path(&plan, SEED, i, &mut ws, &mut detail);
    }
    assert!(detail.months > 12 * 12, "{label}");
    let trace = serde_json::to_string(&sim::trace_path(&plan, SEED, 5)).unwrap();
    assert!(
        !trace.contains("NaN") && !trace.contains("Infinity") && !trace.contains(":inf"),
        "{label}: a non-number in a trace"
    );

    // Chunks of uneven size, merged in a shuffled order after crossing a byte
    // boundary, give the same bytes as one run (and as the checked run).
    let bounds = [0, 7, 50, 51, 120, 199, 260, PATHS];
    let mut chunks: Vec<Accumulator> = bounds
        .windows(2)
        .map(|w| run(&plan, SEED, w[0]..w[1]))
        .collect();
    let mut rng = Rng::stream(3, 0, Stream::Events);
    for i in (1..chunks.len()).rev() {
        chunks.swap(i, rng.below(i + 1));
    }
    let mut merged = plan.new_accumulator();
    for chunk in &chunks {
        let decoded = Accumulator::from_bytes(&chunk.to_bytes()).unwrap();
        merged.merge(&decoded).unwrap();
    }
    assert_eq!(
        merged.to_bytes(),
        whole,
        "{label}: chunking or merge order changed the result"
    );
    whole
}

fn check_example(slug: &str) {
    let base = furnished(slug);
    let base_bytes = check(&format!("{slug}: baseline"), &base);
    for (label, baseline, on) in variants(slug) {
        assert_eq!(baseline, base);
        let bytes = check(&label, &on);
        assert_ne!(bytes, base_bytes, "{label}: the features changed nothing");
    }
}

#[test]
fn dual_income_family_with_the_features_on() {
    check_example("dual-income-family");
}

#[test]
fn early_career_saver_with_the_features_on() {
    check_example("early-career-saver");
}

#[test]
fn side_hustle_landlord_with_the_features_on() {
    check_example("side-hustle-landlord");
}

/// Stressed versions: the same households spending far more than they can afford,
/// so that every source of emergency money (the line of credit, the reverse
/// mortgage, cash-out refinancing, forced sales, an early claim, a return to work,
/// Medicaid) is reached on many paths at once.
#[test]
fn overspending_households_with_everything_on() {
    for slug in SLUGS {
        for flavour in [false, true] {
            for factor in [1.6, 2.5] {
                let p = overspending(everything_on(furnished(slug), flavour), factor);
                check(&format!("{slug}: overspending x{factor} {flavour}"), &p);
            }
        }
    }
}

/// Every feature at once under each market model (the first flavour's bootstrap
/// extensions are dropped where the model is not the bootstrap).
#[test]
fn everything_on_under_every_market_model() {
    for slug in SLUGS {
        for kind in [
            MarketKind::Regime,
            MarketKind::Lognormal,
            MarketKind::Historical,
        ] {
            let mut p = everything_on(furnished(slug), true);
            p.assumptions.market.kind = kind;
            check(&format!("{slug}: everything under {kind:?}"), &p);
        }
    }
}

// ---- One feature at a time ------------------------------------------------------------

/// What a feature is allowed to move among the base draws.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Moves {
    /// Nothing: market, career, health and lifespans are those of the baseline.
    Nothing,
    /// The lifespans themselves, by design (the same uniforms read against a
    /// different mortality curve or joint law).
    Lifespans,
    /// The market series, by design (the market tests in `economy` show which series
    /// and that the market stream's own draws are untouched).
    Market,
}

struct Feature {
    id: &'static str,
    /// Applied to the furnished example to give the baseline of this feature (the
    /// identity for all but the two that need another market model underneath).
    base: fn(Profile) -> Profile,
    on: fn(Profile) -> Profile,
    moves: Moves,
}

fn same(p: Profile) -> Profile {
    p
}

fn each_job(mut p: Profile, f: fn(&mut CareerRisk)) -> Profile {
    p.assumptions.career_risk = true;
    for income in &mut p.incomes {
        if let IncomeKind::Job(j) = &mut income.kind {
            f(&mut j.risk);
        }
    }
    p
}

fn future_children(mut p: Profile, f: fn(&mut Child)) -> Profile {
    let start = p.start.unwrap();
    for c in &mut p.household.children {
        if c.birth > start {
            f(c);
        }
    }
    p
}

fn each_home(mut p: Profile, f: fn(&mut Property), g: fn(&mut BuyHome)) -> Profile {
    for pr in &mut p.properties {
        f(pr);
    }
    for ev in &mut p.events {
        if let EventKind::BuyHome(b) = &mut ev.kind {
            g(b);
        }
    }
    p
}

/// The fifty features, each switched on alone.
fn features() -> Vec<Feature> {
    let f = |id, on, moves| Feature {
        id,
        base: same,
        on,
        moves,
    };
    let n = Moves::Nothing;
    vec![
        f(
            "F01 vehicles",
            |p| {
                let mut q = p.clone();
                q.spending.vehicles = spending_on(p).spending.vehicles;
                q
            },
            n,
        ),
        f(
            "F02 seasonality",
            |mut p| {
                p.spending.seasonality = Seasonality::Typical;
                p
            },
            n,
        ),
        f(
            "F03 category inflation",
            |mut p| {
                p.spending.category_inflation = CategoryInflation::Stochastic;
                p
            },
            n,
        ),
        f(
            "F04 detailed shocks",
            |mut p| {
                p.spending.shocks = ShockLevel::Detailed;
                p
            },
            n,
        ),
        f(
            "F05 unemployment cut",
            |mut p| {
                p.spending.unemployment_cut = 0.15;
                p
            },
            n,
        ),
        f(
            "F06 children on the health plan",
            |mut p| {
                p.spending.healthcare.cover_children = true;
                p
            },
            n,
        ),
        f(
            "F07 uncertain arrival",
            |p| {
                future_children(p, |c| {
                    c.arrival = Arrival::Uncertain {
                        probability: 0.85,
                        delay_mean_months: 4.0,
                        twins_probability: 0.2,
                    }
                })
            },
            n,
        ),
        f(
            "F08 college cost",
            |mut p| {
                for c in &mut p.household.children {
                    c.college_cost = CollegeCost::Stochastic {
                        attend_probability: DEFAULT_ATTEND_PROBABILITY,
                        real_tuition_growth: DEFAULT_REAL_TUITION_GROWTH,
                        tuition_volatility: DEFAULT_TUITION_VOLATILITY,
                        aid_scale: 1.0,
                    };
                }
                p
            },
            n,
        ),
        f(
            "F09 adult support",
            |mut p| {
                for c in &mut p.household.children {
                    c.adult_support = Some(AdultSupport::default());
                }
                p
            },
            n,
        ),
        f(
            "F10 parental leave",
            |p| future_children(p, |c| c.parental_leave = Some(ParentalLeave::default())),
            n,
        ),
        f(
            "F11 lumpy maintenance",
            |p| {
                each_home(
                    p,
                    |pr| pr.maintenance = MaintenanceModel::Lumpy,
                    |b| b.maintenance = MaintenanceModel::Lumpy,
                )
            },
            n,
        ),
        f(
            "F12 assessment cap and insurance drift",
            |p| {
                each_home(
                    p,
                    |pr| {
                        pr.assessment = Assessment::Capped {
                            annual_cap: 0.02,
                            assessed_value: None,
                        };
                        pr.insurance_real_growth = InsuranceGrowth::Stochastic;
                    },
                    |b| {
                        b.assessment = Assessment::Capped {
                            annual_cap: 0.02,
                            assessed_value: None,
                        };
                        b.insurance_real_growth = InsuranceGrowth::Stochastic;
                    },
                )
            },
            n,
        ),
        f(
            "F13 market rent",
            |mut p| {
                for item in &mut p.spending.items {
                    if item.category == Category::Rent {
                        item.rent_growth = RentGrowth::Market;
                    }
                }
                p
            },
            n,
        ),
        f(
            "F14 slow home sales",
            |mut p| {
                p.assumptions.housing_liquidity = HousingLiquidity::Realistic;
                p
            },
            n,
        ),
        f(
            "F15 adjustable-rate mortgage",
            |p| {
                each_home(
                    p,
                    |pr| {
                        if let Some(m) = &mut pr.mortgage {
                            m.kind = arm();
                        }
                    },
                    |b| b.mortgage_kind = arm(),
                )
            },
            n,
        ),
        f(
            "F16 job mobility",
            |p| each_job(p, |r| r.job_mobility = JobMobility::High),
            n,
        ),
        f(
            "F17 recession pay",
            |p| each_job(p, |r| r.recession_pay = RecessionPay::Typical),
            n,
        ),
        f(
            "F18 disability",
            |mut p| {
                for person in &mut p.household.people {
                    person.disability_risk = DisabilityRisk::Typical;
                    person.ltd_insurance = Some(LtdInsurance::default());
                }
                p
            },
            n,
        ),
        f(
            "F19 late career",
            |p| each_job(p, |r| r.late_career = LateCareer::Realistic),
            n,
        ),
        f(
            "F20 business risk",
            |mut p| {
                p.assumptions.career_risk = true;
                for income in &mut p.incomes {
                    if let IncomeKind::SelfEmployment(se) = &mut income.kind {
                        se.business_risk = BusinessRisk::High;
                        se.fallback_wage = Some(FallbackWage::default());
                    }
                }
                p
            },
            n,
        ),
        f(
            "F21 chained CPI",
            |mut p| {
                p.assumptions.taxes.chained_cpi = true;
                p
            },
            n,
        ),
        f(
            "F22 state detail",
            |mut p| {
                p.assumptions.taxes.state_detail = StateDetail::Full;
                p
            },
            n,
        ),
        f(
            "F23 policy risk",
            |mut p| {
                p.assumptions.taxes.policy_risk = Some(PolicyRisk::default());
                p
            },
            n,
        ),
        f(
            "F24 tax-loss harvesting",
            |mut p| {
                p.portfolio.tax_loss_harvesting = true;
                p
            },
            n,
        ),
        f(
            "F25 qualified charitable distributions",
            |mut p| {
                p.retirement.qcd = true;
                p
            },
            n,
        ),
        f(
            "F26 home-equity line",
            |mut p| {
                p.retirement.home_equity_borrowing = HomeEquityBorrowing::Heloc(Heloc::default());
                p
            },
            n,
        ),
        f(
            "F27 reverse mortgage",
            |mut p| {
                p.retirement.home_equity_borrowing =
                    HomeEquityBorrowing::ReverseMortgage(ReverseMortgage {
                        open_when: ReverseMortgageOpen::SavingsBelow { years: 5.0 },
                        ..ReverseMortgage::default()
                    });
                p
            },
            n,
        ),
        f(
            "F28 downsizing",
            |mut p| {
                p.retirement.downsize = Some(Downsize {
                    when: DownsizeWhen::WhenFundedRatioBelow { ratio: 0.9 },
                    ..Downsize::default()
                });
                p
            },
            n,
        ),
        f(
            "F29 cash-out refinancing and prepayment",
            |mut p| {
                p.debt_policy.cash_out_refinance = Some(CashOutRefinance::default());
                p.debt_policy.mortgage_prepayment =
                    MortgagePrepayment::WhenRateAbove { real_rate: None };
                p
            },
            n,
        ),
        f("F30 realistic rentals", realistic_rentals, n),
        f(
            "F31 health states",
            |mut p| {
                p.assumptions.health_model = HealthModelKind::States;
                p
            },
            n,
        ),
        f(
            "F32 linked lifespans",
            |mut p| {
                if let Longevity::Stochastic {
                    couple_dependence, ..
                } = &mut p.assumptions.longevity
                {
                    *couple_dependence = CoupleDependence::Linked;
                }
                p
            },
            Moves::Lifespans,
        ),
        f(
            "F33 income gradient",
            |mut p| {
                if let Longevity::Stochastic {
                    income_gradient, ..
                } = &mut p.assumptions.longevity
                {
                    *income_gradient = true;
                }
                p
            },
            Moves::Lifespans,
        ),
        f(
            "F34 Medicaid",
            |mut p| {
                p.spending.healthcare.medicaid = true;
                p
            },
            n,
        ),
        f(
            "F35 long-term-care insurance",
            |mut p| {
                for person in &mut p.household.people {
                    person.ltc_insurance = Some(LtcInsurance::default());
                }
                p
            },
            n,
        ),
        f(
            "F36 bonds from the yield",
            |mut p| {
                p.assumptions.market.bond_pricing = BondPricing::FromYield;
                p
            },
            Moves::Market,
        ),
        f(
            "F37 valuation",
            |mut p| {
                p.assumptions.market.valuation = Some(Valuation::default());
                p
            },
            Moves::Market,
        ),
        f(
            "F38 episode and lost-generation stresses",
            |mut p| {
                let m = &mut p.assumptions.market;
                m.stress.push(Stress::HistoricalEpisode {
                    start: When::date(START + 40),
                    episode: Episode::Financial2008,
                });
                m.stress.push(Stress::LostGeneration {
                    start: When::retirement(8.0),
                    years: 20.0,
                });
                p
            },
            Moves::Market,
        ),
        Feature {
            id: "F39 fat tails",
            base: |p| with_market(p, MarketKind::Lognormal),
            on: |p| with_market(p, MarketKind::FatTailed),
            moves: Moves::Market,
        },
        f(
            "F40 dynamic yields",
            |mut p| {
                p.assumptions.market.dynamic_yields = true;
                p
            },
            n,
        ),
        f(
            "F41 return to work",
            |mut p| {
                p.retirement.return_to_work = Some(ReturnToWork {
                    funded_ratio_below: 0.9,
                    ..ReturnToWork::default()
                });
                p
            },
            n,
        ),
        f(
            "F42 panic selling",
            |mut p| {
                p.portfolio.behavior = InvestorBehavior::PanicProne(PanicSelling {
                    probability: 0.5,
                    ..PanicSelling::default()
                });
                p
            },
            n,
        ),
        f(
            "F43 adaptive claiming",
            |mut p| {
                for person in &mut p.household.people {
                    person.social_security.claiming = Claiming::Adaptive {
                        earliest: 62.0,
                        latest: 70.0,
                    };
                }
                p
            },
            n,
        ),
        f(
            "F44 pausing deferrals",
            |mut p| {
                p.portfolio.pause_deferrals_in_distress = true;
                p
            },
            n,
        ),
        f(
            "F45 retirement confirmation",
            |mut p| {
                if let RetirementTrigger::WhenFunded(rule) = &mut p.retirement.trigger {
                    rule.confirmation = Some(RetirementConfirmation::default());
                    rule.one_more_year = Some(OneMoreYear { probability: 0.3 });
                }
                p
            },
            n,
        ),
        f(
            "F46 divorce",
            |p| {
                let on = household_on(p.clone());
                let mut q = p;
                q.household.divorce_risk = on.household.divorce_risk;
                q.events = on.events;
                q
            },
            n,
        ),
        f(
            "F47 parents and inheritance",
            |p| {
                let mut on = household_on(p.clone());
                for parent in &mut on.household.parents {
                    parent.care_support = None;
                }
                let mut q = p;
                q.household.parents = on.household.parents;
                q
            },
            n,
        ),
        f(
            "F48 caring for a parent",
            |p| {
                let on = household_on(p.clone());
                let mut q = p;
                q.household.parents = on.household.parents;
                q
            },
            n,
        ),
        f(
            "F49 spousal care",
            |mut p| {
                p.spending.healthcare.spousal_care = is_couple(&p);
                p
            },
            n,
        ),
        f(
            "F50 life insurance",
            |p| {
                let on = household_on(p.clone());
                let mut q = p;
                for (a, b) in q.household.people.iter_mut().zip(on.household.people) {
                    a.life_insurance = b.life_insurance;
                }
                q
            },
            n,
        ),
    ]
}

/// Every feature, switched on alone, changes the result of at least one of the
/// shipped examples (none is a switch that does nothing), and the profile it gives
/// validates and round-trips on all three.
#[test]
fn every_feature_alone_changes_a_result() {
    const N: u64 = 60;
    for feature in features() {
        let mut moved = Vec::new();
        for slug in SLUGS {
            let base = (feature.base)(furnished(slug));
            let on = (feature.on)(base.clone());
            let report = validate(&on);
            assert!(
                report.is_ok(),
                "{} on {slug}: {:#?}",
                feature.id,
                report.errors
            );
            assert_eq!(Profile::from_json(&on.to_json_pretty()).unwrap(), on);
            if on == base {
                continue;
            }
            let a = run_checked(&compile(&base), SEED, 0..N).to_bytes();
            let b = run_checked(&compile(&on), SEED, 0..N).to_bytes();
            if a != b {
                moved.push(slug);
            }
        }
        assert!(!moved.is_empty(), "{} changes nothing anywhere", feature.id);
    }
}

// ---- The stream audit -------------------------------------------------------------------

struct Quiet;

impl Recorder for Quiet {
    fn begin_path(&mut self, _: u64) {}
    fn record_year(&mut self, _: usize, _: &YearRecord) {}
    fn end_path(&mut self, _: &PathEnd) {}
}

/// One path's base draws: lifespans, the market series, and the position of the
/// career and health streams after every month.
struct Draws {
    deaths: [u32; 2],
    positions: Vec<sim::StreamAudit>,
    /// Per month: the bits of every asset return, inflation and the recession flag.
    market: Vec<Vec<u64>>,
}

fn draws(plan: &sim::Plan, ws: &mut Workspace, path: u64) -> Draws {
    let (deaths, positions) = sim::audit_streams(plan, SEED, path, ws, &mut Quiet);
    let e = &ws.economy;
    let market = (0..positions.len())
        .map(|t| {
            let mut row: Vec<u64> = e.returns_at(t).iter().map(|r| r.to_bits()).collect();
            row.push(e.inflation[t].to_bits());
            row.push(u64::from(e.recession[t]));
            row
        })
        .collect();
    Draws {
        deaths,
        positions,
        market,
    }
}

/// The audit of one feature on one example: over the months both versions of a path
/// live through, the career and health streams stand at the same position after
/// every month (for the people alive in both), the lifespans are the same unless
/// the feature is one that moves them by design, and the market series are the same
/// unless the feature is a market feature.
fn audit(feature: &Feature, slug: &str) {
    const N: u64 = 40;
    let base = (feature.base)(furnished(slug));
    let on = (feature.on)(base.clone());
    if on == base {
        return;
    }
    let (plan_a, plan_b) = (compile(&base), compile(&on));
    let (mut ws_a, mut ws_b) = (Workspace::new(&plan_a), Workspace::new(&plan_b));
    let at = |path: u64| format!("{} on {slug}, path {path}", feature.id);
    for path in 0..N {
        let a = draws(&plan_a, &mut ws_a, path);
        let b = draws(&plan_b, &mut ws_b, path);
        if feature.moves != Moves::Lifespans {
            assert_eq!(a.deaths, b.deaths, "{}: lifespans moved", at(path));
        }
        let months = a.positions.len().min(b.positions.len());
        assert!(months > 0);
        for t in 0..months {
            let (x, y) = (&a.positions[t], &b.positions[t]);
            for p in 0..2 {
                if x.career[p] != 0 && y.career[p] != 0 {
                    assert_eq!(
                        x.career[p],
                        y.career[p],
                        "{}: career stream of person {p} moved in month {t}",
                        at(path)
                    );
                    assert_eq!(
                        x.health[p],
                        y.health[p],
                        "{}: health stream of person {p} moved in month {t}",
                        at(path)
                    );
                }
            }
            if feature.moves != Moves::Market {
                assert_eq!(
                    a.market[t],
                    b.market[t],
                    "{}: market moved in month {t}",
                    at(path)
                );
            }
        }
    }
}

/// Stream audit: switching any one feature on leaves the market, career, health and
/// lifespan draws of the household where they were.
#[test]
fn no_feature_moves_the_base_draws() {
    for feature in features() {
        for slug in SLUGS {
            audit(&feature, slug);
        }
    }
}

/// The market features leave alone what they say they leave alone: income yields
/// that move with the market change no return; a valuation tilt changes only the
/// equity-like series; bonds priced from the yield change only bonds; a stress
/// changes nothing before it starts.
#[test]
fn market_features_move_only_their_own_series() {
    let series = |f: &Feature, slug: &str, path: u64| {
        let base = (f.base)(furnished(slug));
        let on = (f.on)(base.clone());
        let (pa, pb) = (compile(&base), compile(&on));
        let (mut wa, mut wb) = (Workspace::new(&pa), Workspace::new(&pb));
        (draws(&pa, &mut wa, path), draws(&pb, &mut wb, path))
    };
    let all = features();
    let get = |id: &str| all.iter().find(|f| f.id.starts_with(id)).unwrap();
    // The equity-like classes carry the tilt (international stocks and the classes
    // that follow US stocks by a beta); these do not.
    let untilted = [
        AssetClass::Bonds.index(),
        AssetClass::Cash.index(),
        AssetClass::Gold.index(),
        N_ASSETS,
        N_ASSETS + 1,
    ];
    let us = AssetClass::UsStocks.index();
    let bonds = AssetClass::Bonds.index();
    for slug in SLUGS {
        for path in 0..20 {
            let (a, b) = series(get("F37"), slug, path);
            let mut moved = false;
            for (x, y) in a.market.iter().zip(&b.market) {
                moved |= x[us] != y[us];
                for k in untilted {
                    assert_eq!(x[k], y[k], "valuation moved series {k}");
                }
            }
            assert!(moved);
            let (a, b) = series(get("F36"), slug, path);
            for (x, y) in a.market.iter().zip(&b.market) {
                for k in 0..x.len() {
                    if k != bonds {
                        assert_eq!(x[k], y[k], "bond pricing moved series {k}");
                    }
                }
            }
            let (a, b) = series(get("F38"), slug, path);
            let before = 40.min(a.market.len()).min(b.market.len());
            assert_eq!(a.market[..before], b.market[..before]);
        }
    }
}

/// Every stream has its own identifier: the fixed ones, and the per-index ones over
/// far more indices than a profile can have.
#[test]
fn stream_ids_are_unique() {
    let mut ids = std::collections::BTreeMap::new();
    let mut add = |s: Stream| {
        if let Some(other) = ids.insert(s.id(), s) {
            panic!("{s:?} and {other:?} share id {}", s.id());
        }
    };
    for s in [
        Stream::Market,
        Stream::Inflation,
        Stream::EquityComp,
        Stream::Shocks,
        Stream::Events,
        Stream::CategoryPrices,
        Stream::DetailedShocks,
        Stream::CollegePrices,
        Stream::InsuranceDrift,
        Stream::RentMarket,
        Stream::Liquidity,
        Stream::PolicyRisk,
        Stream::HomeEquityCredit,
        Stream::CoupleLongevity,
        Stream::LtcPremiumRates,
        Stream::MarketTails,
        Stream::PanicSelling,
        Stream::OneMoreYear,
        Stream::Divorce,
        Stream::SpousalCare,
    ] {
        add(s);
    }
    for i in 0..4096 {
        for s in [
            Stream::Career(i),
            Stream::Health(i),
            Stream::Longevity(i),
            Stream::Housing(i),
            Stream::Vehicle(i),
            Stream::ChildArrival(i),
            Stream::ChildCollege(i),
            Stream::ChildSupport(i),
            Stream::Maintenance(i),
            Stream::RentalMarket(i),
            Stream::HealthStates(i),
            Stream::ReturnToWork(i),
            Stream::Parent(i),
        ] {
            add(s);
        }
        for kind in 0..=u8::MAX {
            add(Stream::CareerExt(kind, i));
        }
    }
}

// ---- Features that must not contradict one another ---------------------------------------

/// Collects each path's events and checks their order for contradictions between
/// features written apart from one another.
#[derive(Default)]
struct Story {
    events: Vec<TraceEvent>,
    /// Names of the jobs of the second person (the partner who leaves in a divorce).
    partner_jobs: Vec<String>,
    label: String,
    path: u64,
    divorces: u64,
    disabilities: u64,
}

impl Recorder for Story {
    fn begin_path(&mut self, path_index: u64) {
        self.events.clear();
        self.path = path_index;
    }

    fn record_year(&mut self, _: usize, _: &YearRecord) {}

    fn end_path(&mut self, _: &PathEnd) {
        let mut gone = [false; 2];
        let mut disabled = [false; 2];
        let mut in_care = [false; 2];
        for e in &self.events {
            let l = &e.label;
            let at = || format!("{} path {}: {l:?}", self.label, self.path);
            for p in 0..2 {
                if !l.starts_with(&format!("Person {}", p + 1)) {
                    continue;
                }
                // Nothing more happens to someone who has died or left.
                assert!(!gone[p] || l.contains("dies"), "after leaving, {}", at());
                let seeks_work = l.contains("looks for work") || l.contains("back at work after");
                // A disabled person neither goes back to work nor claims a
                // retirement benefit early on top of disability insurance.
                assert!(!(disabled[p] && seeks_work), "while disabled, {}", at());
                assert!(
                    !(disabled[p] && l.contains("claims Social Security")),
                    "while disabled, {}",
                    at()
                );
                assert!(!(in_care[p] && seeks_work), "while in care, {}", at());
                if l.contains("becomes disabled") {
                    disabled[p] = true;
                    self.disabilities += 1;
                }
                disabled[p] &= !l.contains("recovers");
                in_care[p] |= l.contains("begins long-term care");
                gone[p] |= l.contains("dies");
            }
            // The partner's jobs are no longer the household's after a divorce.
            if gone[1] {
                for job in &self.partner_jobs {
                    assert!(
                        !l.contains(job.as_str()),
                        "after the partner left, {}",
                        at()
                    );
                }
            }
            if l.contains("partner leaves") {
                gone[1] = true;
                self.divorces += 1;
            }
        }
    }

    fn wants_detail(&self) -> bool {
        true
    }

    fn record_event(&mut self, event: TraceEvent) {
        self.events.push(event);
    }
}

/// With every feature on, the order of events on a path makes sense: no one who has
/// died or left the household does anything afterwards, a disabled person or one in
/// long-term care does not go back to work, disability insurance and an early
/// retirement claim are not drawn together, and a departed partner's jobs are
/// neither changed, cut nor taken leave from.
#[test]
fn features_do_not_contradict_one_another() {
    let (mut divorces, mut disabilities) = (0, 0);
    for slug in SLUGS {
        for flavour in [false, true] {
            for factor in [1.0, 1.6] {
                let p = overspending(everything_on(furnished(slug), flavour), factor);
                let plan = compile(&p);
                let mut ws = Workspace::new(&plan);
                let mut story = Story {
                    label: format!("{slug} {flavour} x{factor}"),
                    partner_jobs: p
                        .incomes
                        .iter()
                        .filter(|i| i.owner == 1)
                        .map(|i| i.name.clone())
                        .collect(),
                    ..Story::default()
                };
                for i in 0..PATHS {
                    sim::run_path(&plan, SEED, i, &mut ws, &mut story);
                }
                divorces += story.divorces;
                disabilities += story.disabilities;
            }
        }
    }
    // The cases the checks are about did come up.
    assert!(
        divorces > 100 && disabilities > 100,
        "{divorces} {disabilities}"
    );
}

/// Every pair of features on every example, overspending so that money runs short,
/// with the conservation checks on. Slow (about 1,200 pairs): run it by hand with
/// `cargo test -p firesight-core --test sim_richness -- --ignored every_pair`.
#[test]
#[ignore = "slow: every pair of the fifty features"]
fn every_pair_of_features_conserves_money() {
    let all = features();
    for slug in SLUGS {
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                let base = (b.base)((a.base)(furnished(slug)));
                let on = (b.on)((a.on)(base.clone()));
                if !validate(&on).is_ok() {
                    // A pair validation rejects (see `validate`): nothing to run.
                    continue;
                }
                let label = format!("{slug}: {} + {}", a.id, b.id);
                let plan = compile(&overspending(on, 1.6));
                let mut ws = Workspace::new(&plan);
                ws.set_check(true);
                let mut rec = Checked {
                    inner: plan.new_accumulator(),
                    label: &label,
                    path: 0,
                    detail: false,
                    months: 0,
                };
                for path in 0..40 {
                    sim::run_path(&plan, SEED, path, &mut ws, &mut rec);
                }
            }
        }
    }
}
