//! Profile checks.
//!
//! [`validate`] returns two lists:
//!
//! * **errors**: the profile cannot be simulated (a reference does not resolve, a
//!   number is outside the range the model is defined for). `Plan::compile` refuses
//!   profiles with errors.
//! * **warnings**: the profile runs but something looks implausible or is probably a
//!   data-entry mistake. The UI shows them next to the field; the AI reads them.
//!
//! Every issue carries a stable `code`, and a `path` in the same syntax patches use
//! (`/incomes/@alex_job/kind/salary`), so a fix can be written as a patch directly.
//! Messages say what is wrong and what a valid value looks like.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::data::Data;
use crate::equity::{self, EquityParams};
use crate::model::*;

mod behavior;
mod care;
mod dynamics;
mod family;
mod housing;
mod housing_realism;
mod market_realism;
mod realism;
mod spending_realism;
// The shared check for a user-supplied distribution lives beside its sampler; it is
// re-exported here so validation code calls it like any other check:
// `check_uncertain(&mut self.r, &path, "Windfall amount", &dist, Bounds::MONEY)`.
use crate::dist::CompiledDist;
pub use crate::dist::{check_uncertain, Bounds};

/// Accepted `equity_premium_adjustment`, and accepted adjustment plus the furthest
/// per-path shift of `equity_premium_uncertainty`, as annual decimals.
const EQUITY_ADJUSTMENT_RANGE: (f64, f64) = (-0.1, 0.1);
/// The same for `inflation_adjustment` and `inflation_uncertainty`.
const INFLATION_ADJUSTMENT_RANGE: (f64, f64) = (-0.05, 0.15);

/// One finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Issue {
    /// Stable machine-readable identifier, for example "ref_income" or "crypto_heavy".
    pub code: String,
    /// Patch-style path of the offending value, for example "/portfolio/target".
    pub path: String,
    /// Plain-language explanation and how to fix it.
    pub message: String,
}

/// The result of validating a profile.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Report {
    /// Problems that prevent simulation.
    pub errors: Vec<Issue>,
    /// Plausibility lints; the profile still runs.
    pub warnings: Vec<Issue>,
}

impl Report {
    /// True when there are no errors (warnings are allowed).
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// A report with a single error; for callers that fail before validation proper.
    pub fn single_error(code: &str, path: &str, message: impl Into<String>) -> Report {
        let mut r = Report::default();
        r.error(code, path, message);
        r
    }

    pub(crate) fn error(&mut self, code: &str, path: &str, message: impl Into<String>) {
        self.errors.push(Issue {
            code: code.into(),
            path: path.into(),
            message: message.into(),
        });
    }

    fn warn(&mut self, code: &str, path: &str, message: impl Into<String>) {
        self.warnings.push(Issue {
            code: code.into(),
            path: path.into(),
            message: message.into(),
        });
    }

    /// Value must be finite and within `lo..=hi`.
    fn range(&mut self, path: &str, what: &str, v: f64, lo: f64, hi: f64) -> bool {
        if !v.is_finite() || v < lo || v > hi {
            let hint = if v > 1.0 && hi <= 1.0 {
                " Rates and fractions are decimals: write 0.05 for 5%."
            } else {
                ""
            };
            self.error(
                "out_of_range",
                path,
                format!("{what} is {v}; it must be between {lo} and {hi}.{hint}"),
            );
            return false;
        }
        true
    }

    /// Money must be finite, not negative and at most [`MONEY_MAX`].
    fn money(&mut self, path: &str, what: &str, v: f64) -> bool {
        if !v.is_finite() || v < 0.0 {
            self.error(
                "negative_amount",
                path,
                format!("{what} is {v}; amounts must be zero or positive dollars."),
            );
            return false;
        }
        if v > MONEY_MAX {
            self.error(
                "amount_too_large",
                path,
                format!("{what} is {v}; amounts must be at most {MONEY_MAX} dollars (a trillion)."),
            );
            return false;
        }
        true
    }

    /// A dollar amount per share: money, and at most [`PER_SHARE_MAX`].
    fn per_share(&mut self, path: &str, what: &str, v: f64) -> bool {
        if !self.money(path, what, v) {
            return false;
        }
        if v > PER_SHARE_MAX {
            self.error(
                "out_of_range",
                path,
                format!("{what} is {v} a share; it must be at most {PER_SHARE_MAX}. Check the figure is per share, not a total."),
            );
            return false;
        }
        true
    }
}

/// Largest dollar amount any field accepts. Totals over simulated lives are kept as
/// whole dollars in 64 bits; nothing a household owns comes near this.
const MONEY_MAX: f64 = 1e12;
/// Largest price, strike or basis per share (the bound on the preferred price).
const PER_SHARE_MAX: f64 = 1_000_000.0;
/// Largest minimum tax credit carried into a plan.
const AMT_CREDIT_MAX: f64 = 1e9;
/// Lowest tender price as a fraction of the share price.
const TENDER_PRICE_FRACTION_MIN: f64 = 0.05;

/// Whole dollars with thousands separators ("$1,458,000"), for messages a person reads.
fn dollars(x: f64) -> String {
    let n = if x.is_finite() { x.round() } else { 0.0 };
    let digits = format!("{:.0}", n.abs());
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 2);
    if n < 0.0 {
        out.push('-');
    }
    out.push('$');
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Dollars and cents with thousands separators ("$1,234.50"), for a price per share.
fn dollars_cents(x: f64) -> String {
    let n = if x.is_finite() { x } else { 0.0 };
    let cents = format!("{:.2}", n.abs());
    let (whole, fraction) = cents.split_once('.').unwrap_or((&cents, "00"));
    let mut out = String::with_capacity(cents.len() + whole.len() / 3 + 2);
    if n < 0.0 {
        out.push('-');
    }
    out.push('$');
    for (i, ch) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.push('.');
    out.push_str(fraction);
    out
}

/// A warning is raised when options reach their ten-year term within this many months.
const OPTION_TERM_SOON_MONTHS: i64 = 24;

/// Path segment for a list element: `@id` when it has a usable id, else its index.
fn seg(id: &str, index: usize) -> String {
    if id_is_valid(id) {
        format!("@{id}")
    } else {
        index.to_string()
    }
}

fn id_is_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn check_ids<'a>(r: &mut Report, list_path: &str, ids: impl Iterator<Item = &'a str>) {
    let mut seen = BTreeSet::new();
    for (i, id) in ids.enumerate() {
        let path = format!("{list_path}/{i}/id");
        if id.is_empty() {
            r.error(
                "id_missing",
                &path,
                "Every element needs a non-empty \"id\" (letters, digits, \"_\", \"-\", \".\"), unique within its list.",
            );
        } else if !id_is_valid(id) {
            r.error(
                "id_invalid",
                &path,
                format!("Id {id:?} may only contain letters, digits, \"_\", \"-\" and \".\", up to 64 characters."),
            );
        } else if !seen.insert(id) {
            r.error(
                "id_duplicate",
                &path,
                format!("Id {id:?} is used more than once in {list_path}; ids must be unique within a list."),
            );
        }
    }
}

fn list_ids<'a>(ids: impl Iterator<Item = &'a str>) -> String {
    let v: Vec<&str> = ids.collect();
    if v.is_empty() {
        "(none)".into()
    } else {
        v.join(", ")
    }
}

/// The checks on one private company that need nothing else from the profile:
/// prices, fractions, months, holdings and the outcome table. `c` is the company's
/// path. Shared by profile validation and the standalone preview.
fn company_checks(r: &mut Report, c: &str, co: &PrivateCompany) {
    if !co.holdings.is_empty() {
        r.range(
            &format!("{c}/preferred_price"),
            "Preferred price per share",
            co.preferred_price,
            0.0001,
            1_000_000.0,
        );
    }
    if let Some(v) = co.fmv_409a {
        r.range(
            &format!("{c}/fmv_409a"),
            "409A value per share",
            v,
            0.0001,
            1_000_000.0,
        );
    }
    if let Some(v) = co.preference_overhang {
        r.range(
            &format!("{c}/preference_overhang"),
            "Money raised as a share of valuation",
            v,
            0.0,
            equity::OVERHANG_MAX,
        );
    }
    if co.lockup_months > 24 {
        r.error(
            "out_of_range",
            &format!("{c}/lockup_months"),
            format!(
                "Lockup is {} months; it must be between 0 and 24.",
                co.lockup_months
            ),
        );
    }
    if co.exercise_window_months > 120 {
        r.error(
            "out_of_range",
            &format!("{c}/exercise_window_months"),
            format!(
                "Exercise window is {} months; it must be between 0 and 120.",
                co.exercise_window_months
            ),
        );
    }
    if r.money(
        &format!("{c}/amt_credit_carryforward"),
        "Minimum tax credit carried forward",
        co.amt_credit_carryforward,
    ) && co.amt_credit_carryforward > AMT_CREDIT_MAX
    {
        r.error(
            "out_of_range",
            &format!("{c}/amt_credit_carryforward"),
            format!(
                "Minimum tax credit carried forward is {}; it must be at most {}.",
                dollars(co.amt_credit_carryforward),
                dollars(AMT_CREDIT_MAX)
            ),
        );
    }
    if let Some(v) = co.tenders.annual_probability {
        r.range(
            &format!("{c}/tenders/annual_probability"),
            "Tender offer chance per year",
            v,
            0.0,
            1.0,
        );
    }
    r.range(
        &format!("{c}/tenders/max_fraction"),
        "Tender limit",
        co.tenders.max_fraction,
        0.0,
        1.0,
    );
    // A tender at no price would give the shares away.
    r.range(
        &format!("{c}/tenders/price_fraction"),
        "Tender price fraction",
        co.tenders.price_fraction,
        TENDER_PRICE_FRACTION_MIN,
        2.0,
    );
    if let Some(v) = co.tenders.next_in_months.filter(|v| *v > 120) {
        r.error(
            "out_of_range",
            &format!("{c}/tenders/next_in_months"),
            format!("The planned tender is {v} months away; it must be between 0 and 120."),
        );
    }
    r.range(
        &format!("{c}/policy/tender_sell_fraction"),
        "Share sold in tenders",
        co.policy.tender_sell_fraction,
        0.0,
        1.0,
    );
    if let SellDown::OverYears { years } = co.policy.sell_down {
        if !(1..=5).contains(&years) {
            r.error(
                "out_of_range",
                &format!("{c}/policy/sell_down/years"),
                format!("Yearly sales after the lockup is {years}; it must be between 1 and 5."),
            );
        }
    }
    if let Some(v) = co.policy.leave_after_months.filter(|v| *v > 600) {
        r.error(
            "out_of_range",
            &format!("{c}/policy/leave_after_months"),
            format!("leave_after_months is {v}; it must be between 0 and 600."),
        );
    }

    if co.holdings.len() > equity::MAX_HOLDINGS {
        r.error(
            "too_many_holdings",
            &format!("{c}/holdings"),
            format!(
                "A company can have at most {} holdings; this one has {}. Combine grants with the same type and a similar strike.",
                equity::MAX_HOLDINGS,
                co.holdings.len()
            ),
        );
    }
    check_ids(
        r,
        &format!("{c}/holdings"),
        co.holdings.iter().map(|x| x.id.as_str()),
    );
    for (j, hd) in co.holdings.iter().enumerate() {
        let h = format!("{c}/holdings/{}", seg(&hd.id, j));
        let shares_ok = r.range(
            &format!("{h}/shares"),
            "Number of shares",
            hd.shares,
            0.0,
            1e10,
        );
        let at_price = hd.shares * co.preferred_price;
        if shares_ok && at_price.is_finite() && at_price > 1e12 {
            r.error(
                "equity_position_too_large",
                &format!("{h}/shares"),
                format!("This holding is worth {} at the preferred price. Check that the price is per share and the count is a number of shares.", dollars(at_price)),
            );
        }
        let is_shares = hd.kind.is_shares();
        let vested_ok = is_shares || (hd.vested >= 0.0 && hd.vested <= hd.shares + 1e-9);
        if !vested_ok {
            r.error(
                "vested_exceeds_shares",
                &format!("{h}/vested"),
                format!(
                    "Vested is {} but the holding has {} in total. Vested must be between 0 and the total.",
                    hd.vested, hd.shares
                ),
            );
        }
        // A per-share figure times the count must stay a plausible amount: the cap
        // on the position at the preferred price does not bound these.
        let total_ok = |r: &mut Report, field: &str, what: &str, each: f64| {
            let total = hd.shares * each;
            if shares_ok && total.is_finite() && total > MONEY_MAX {
                r.error(
                    "equity_position_too_large",
                    &format!("{h}/kind/{field}"),
                    format!("{what} times the number of shares is {}. Check that the figure is per share and the count is a number of shares.", dollars(total)),
                );
            }
        };
        match hd.kind {
            HoldingKind::Iso {
                strike,
                granted_months_ago,
            }
            | HoldingKind::Nso {
                strike,
                granted_months_ago,
            } => {
                if r.per_share(&format!("{h}/kind/strike"), "Strike", strike) {
                    total_ok(r, "strike", "The strike", strike);
                }
                if i64::from(granted_months_ago) >= i64::from(equity::OPTION_TERM_MONTHS) {
                    r.error(
                        "option_term_passed",
                        &format!("{h}/kind/granted_months_ago"),
                        format!(
                            "These options were granted {granted_months_ago} months ago, and options lapse ten years ({} months) after the grant. Remove the holding, or correct the months since the grant.",
                            equity::OPTION_TERM_MONTHS
                        ),
                    );
                }
            }
            HoldingKind::Rsu { .. } => {}
            HoldingKind::Shares {
                basis_per_share,
                fmv_at_exercise,
                months_held,
                ..
            } => {
                if r.per_share(
                    &format!("{h}/kind/basis_per_share"),
                    "Basis per share",
                    basis_per_share,
                ) {
                    total_ok(r, "basis_per_share", "The basis per share", basis_per_share);
                }
                if r.per_share(
                    &format!("{h}/kind/fmv_at_exercise"),
                    "409A value at exercise",
                    fmv_at_exercise,
                ) {
                    total_ok(
                        r,
                        "fmv_at_exercise",
                        "The 409A value at exercise",
                        fmv_at_exercise,
                    );
                }
                if months_held > 1_200 {
                    r.error(
                        "out_of_range",
                        &format!("{h}/kind/months_held"),
                        format!("Months held is {months_held}; it must be between 0 and 1200."),
                    );
                }
            }
        }
        if hd.vest_months_left > 240 {
            r.error(
                "out_of_range",
                &format!("{h}/vest_months_left"),
                "Vesting must finish within 240 months.",
            );
        }
        if hd.cliff_months > hd.vest_months_left {
            r.error(
                "out_of_range",
                &format!("{h}/cliff_months"),
                "The cliff cannot be later than the end of vesting.",
            );
        }
        let unvested = hd.shares - hd.vested;
        if !is_shares && shares_ok && vested_ok && unvested > 1e-9 {
            if hd.vest_months_left == 0 {
                r.error(
                    "unvested_without_schedule",
                    &format!("{h}/vest_months_left"),
                    format!("{unvested} of this holding are unvested but vest_months_left is 0. Give the months until it is fully vested, or set vested to the total."),
                );
            }
            if co.job.is_none() {
                r.error(
                    "equity_unvested_without_job",
                    &format!("{h}/vested"),
                    "This holding has unvested units, but the company is not linked to a job, and equity vests only while employed. Choose the linked job, or set vested to the total.",
                );
            }
        }
    }

    if co.outcomes.len() > equity::MAX_OUTCOMES {
        r.error(
            "too_many_outcomes",
            &format!("{c}/outcomes"),
            format!(
                "An outcome table can have at most {} rows; this one has {}.",
                equity::MAX_OUTCOMES,
                co.outcomes.len()
            ),
        );
    }
    check_ids(
        r,
        &format!("{c}/outcomes"),
        co.outcomes.iter().map(|x| x.id.as_str()),
    );
    let mut sum = 0.0;
    for (k, row) in co.outcomes.iter().enumerate() {
        let o = format!("{c}/outcomes/{}", seg(&row.id, k));
        r.range(
            &format!("{o}/probability"),
            "Probability",
            row.probability,
            0.0,
            1.0,
        );
        sum += row.probability;
        let (from, to) = (row.years_from, row.years_to);
        if row.kind != OutcomeKind::StillPrivate && !(0.0 <= from && from <= to && to <= 30.0) {
            r.error(
                "outcome_years",
                &format!("{o}/years_to"),
                format!("The outcome is set between {from} and {to} years from now. Give an earliest and a latest year with 0 <= earliest <= latest <= 30."),
            );
        }
        let (low, high) = (row.multiple_low, row.multiple_high);
        if matches!(row.kind, OutcomeKind::Acquired | OutcomeKind::Ipo)
            && !(0.001 <= low && low <= high && high <= 1000.0)
        {
            r.error(
                "outcome_multiple",
                &format!("{o}/multiple_high"),
                format!("The price range is {low} to {high} times the last round. Give a low and a high multiple with 0.001 <= low <= high <= 1000."),
            );
        }
    }
    // Sums within 0.005 of 1 are normalised by the engine.
    if !co.outcomes.is_empty() && ((sum - 1.0).abs() > 0.005 || sum.is_nan()) {
        r.error(
            "outcome_probabilities",
            &format!("{c}/outcomes"),
            format!("The outcome probabilities add up to {sum:.3}; they must add up to 1. Rows are mutually exclusive: every future is in exactly one of them."),
        );
    }
}

/// Validates one private company on its own (an element of `private_equity`), for
/// the standalone preview: every check that does not need the rest of the profile
/// (the linked job and the owner are not looked up). Paths are relative to the
/// company, for example `/holdings/@grant/kind/strike`.
pub fn validate_company(company: &PrivateCompany) -> Report {
    let mut r = Report::default();
    company_checks(&mut r, "", company);
    r
}

/// The cost parameters shared by an owned property and a planned purchase.
#[derive(Clone, Copy)]
struct PropertyCosts<'a> {
    tax: Option<f64>,
    ins: f64,
    maint: f64,
    hoa: f64,
    appr: f64,
    vol: f64,
    usage: &'a PropertyUse,
}

struct Checker<'a> {
    p: &'a Profile,
    r: Report,
    n_people: usize,
}

/// Validates a profile. Checks that depend on today's date (ages, events in the past)
/// are skipped when `profile.start` is empty; callers that know the date should fill
/// it first with [`Profile::with_start`].
pub fn validate(profile: &Profile) -> Report {
    let mut c = Checker {
        p: profile,
        r: Report::default(),
        n_people: profile.household.people.len(),
    };
    c.root();
    c.household();
    c.incomes();
    c.private_equity();
    c.spending();
    c.spending_realism();
    c.accounts_and_portfolio();
    c.properties_and_debts();
    c.events();
    c.housing_realism();
    c.household_dynamics();
    c.retirement();
    c.housing_decisions();
    c.assumptions();
    c.family();
    c.realism();
    c.care_errors();
    c.behavior();
    if c.r.errors.is_empty() {
        // Lints read the profile as a whole and assume it is structurally sound.
        c.plausibility();
        c.care_warnings();
    }
    c.r
}

impl Checker<'_> {
    fn person_ref(&mut self, path: &str, person: usize) {
        if person >= self.n_people {
            self.r.error(
                "ref_person",
                path,
                format!(
                    "Person index {person} does not exist; the household has {} person(s), so valid values are {}.",
                    self.n_people,
                    if self.n_people == 2 { "0 and 1" } else { "0" }
                ),
            );
        }
    }

    fn when(&mut self, path: &str, when: &When) {
        match *when {
            When::Date { at } => {
                if !(1900..=2200).contains(&at.year()) {
                    self.r.error(
                        "date_range",
                        path,
                        format!("Date {at} is outside 1900 to 2200."),
                    );
                }
            }
            When::Age { person, age } => {
                self.person_ref(&format!("{path}/person"), person);
                self.r.range(&format!("{path}/age"), "Age", age, 0.0, 120.0);
            }
            When::Retirement { offset_years } => {
                self.r.range(
                    &format!("{path}/offset_years"),
                    "Offset",
                    offset_years,
                    -60.0,
                    60.0,
                );
            }
            When::Start => {}
        }
    }

    fn window(&mut self, path: &str, start: Option<&When>, end: Option<&When>) {
        if let Some(w) = start {
            self.when(&format!("{path}/start"), w);
        }
        if let Some(w) = end {
            self.when(&format!("{path}/end"), w);
        }
        if let (Some(s), Some(e), Some(ctx)) = (start, end, self.p.when_context()) {
            if let (Some(s), Some(e)) = (s.resolve(&ctx), e.resolve(&ctx)) {
                if e <= s {
                    self.r.error(
                        "window_empty",
                        &format!("{path}/end"),
                        format!(
                            "The end ({e}) is not after the start ({s}), so this never applies."
                        ),
                    );
                }
            }
        }
    }

    fn allocation(&mut self, path: &str, a: &Allocation) {
        for (asset, w) in a.iter_nonzero() {
            if !w.is_finite() || w < 0.0 {
                self.r.error(
                    "allocation_negative",
                    &format!("{path}/{}", asset.key()),
                    format!(
                        "Weight for {} is {w}; weights must be zero or positive.",
                        asset.key()
                    ),
                );
            }
        }
        if a.total().is_nan() || a.total() <= 0.0 {
            self.r.error(
                "allocation_empty",
                path,
                "The allocation has no positive weights. Give at least one, for example {\"us_stocks\": 60, \"bonds\": 40}.",
            );
        }
    }

    fn root(&mut self) {
        let p = self.p;
        if p.schema_version != SCHEMA_VERSION {
            self.r.error(
                "schema_version",
                "/schema_version",
                format!(
                    "schema_version is {}; this engine reads version {SCHEMA_VERSION}. Version 1 profiles must be migrated first.",
                    p.schema_version
                ),
            );
        }
        if let Some(start) = p.start {
            if !(1990..=2100).contains(&start.year()) {
                self.r.error(
                    "date_range",
                    "/start",
                    format!("Start {start} is outside 1990 to 2100."),
                );
            }
        }
        let s = &p.simulation;
        if s.paths == 0 || s.paths > 10_000_000 {
            self.r.error(
                "out_of_range",
                "/simulation/paths",
                "paths must be between 1 and 10,000,000.",
            );
        }
        if !(50..=120).contains(&s.end_age) {
            self.r.error(
                "out_of_range",
                "/simulation/end_age",
                "end_age must be between 50 and 120.",
            );
        }
    }

    fn household(&mut self) {
        let p = self.p;
        let h = &p.household;
        if h.people.is_empty() || h.people.len() > crate::MAX_PEOPLE {
            self.r.error(
                "people_count",
                "/household/people",
                format!(
                    "A household needs one or two people; this one has {}.",
                    h.people.len()
                ),
            );
        }
        check_ids(
            &mut self.r,
            "/household/people",
            h.people.iter().map(|x| x.id.as_str()),
        );
        check_ids(
            &mut self.r,
            "/household/children",
            h.children.iter().map(|x| x.id.as_str()),
        );

        for (i, person) in h.people.iter().enumerate() {
            let path = format!("/household/people/{}", seg(&person.id, i));
            if let Some(start) = p.start {
                let age = person.birth.age_years_at(start);
                if !(16.0..=110.0).contains(&age) {
                    self.r.error(
                        "age_range",
                        &format!("{path}/birth"),
                        format!("Born {} means age {age:.1} at the start ({start}); adults must be between 16 and 110.", person.birth),
                    );
                } else if age >= f64::from(p.simulation.end_age) {
                    self.r.error(
                        "age_range",
                        "/simulation/end_age",
                        format!(
                            "end_age {} is not above the current age of {} ({age:.0}).",
                            p.simulation.end_age,
                            display_name(&person.name, &person.id)
                        ),
                    );
                }
            }
            crate::career_realism::validate::person(
                &mut self.r,
                &path,
                person,
                p.incomes.iter().any(|inc| {
                    inc.owner == i
                        && matches!(inc.kind, IncomeKind::Job(_) | IncomeKind::SelfEmployment(_))
                }),
            );
            if let Some(joins) = person.joins {
                if i == 0 {
                    self.r.error("joins_first_person", &format!("{path}/joins"), "Only the second person can join later; the first person is in the household from the start.");
                }
                if joins <= person.birth {
                    self.r.error(
                        "date_range",
                        &format!("{path}/joins"),
                        "The join date must be after the person's birth.",
                    );
                }
            }
            let ss = &person.social_security;
            self.r.range(
                &format!("{path}/social_security/claim_age"),
                "Social Security claim age",
                ss.claim_age,
                62.0,
                70.0,
            );
            match ss.basis {
                SocialSecurityBasis::FromEarnings { career_start_age } => {
                    self.r.range(
                        &format!("{path}/social_security/basis/career_start_age"),
                        "Career start age",
                        career_start_age,
                        14.0,
                        70.0,
                    );
                }
                SocialSecurityBasis::KnownBenefit { monthly_at_fra } => {
                    self.r.money(
                        &format!("{path}/social_security/basis/monthly_at_fra"),
                        "Monthly benefit",
                        monthly_at_fra,
                    );
                }
                SocialSecurityBasis::None => {}
            }
        }

        for (i, child) in h.children.iter().enumerate() {
            let path = format!("/household/children/{}", seg(&child.id, i));
            if let Some(start) = p.start {
                let age = child.birth.age_years_at(start);
                if !(-30.0..=40.0).contains(&age) {
                    self.r.error("age_range", &format!("{path}/birth"), format!("Child born {} is {age:.0} years old at the start; expected between 30 years in the future and age 40.", child.birth));
                }
            }
            if let Some(c) = child.childcare_monthly {
                self.r
                    .money(&format!("{path}/childcare_monthly"), "Childcare cost", c);
            }
            if let College::Custom { annual } = child.college {
                self.r
                    .money(&format!("{path}/college/annual"), "College cost", annual);
            }
            self.r.range(
                &format!("{path}/college_share"),
                "College share",
                child.college_share,
                0.0,
                1.0,
            );
        }

        self.location(
            "/household/location",
            &h.location.country,
            h.location.state.as_deref(),
            h.location.foreign_tax_rate,
        );
    }

    fn location(
        &mut self,
        path: &str,
        country: &str,
        state: Option<&str>,
        foreign_tax_rate: Option<f64>,
    ) {
        let country_ok = country.len() == 2 && country.chars().all(|c| c.is_ascii_uppercase());
        if !country_ok {
            self.r.error("country_code", &format!("{path}/country"), format!("Country {country:?} must be a two-letter upper-case ISO code such as \"US\", \"CA\" or \"PT\"."));
        }
        let is_us = country == "US";
        match state {
            Some(s) if is_us && !US_STATE_CODES.contains(&s) => {
                self.r.error("state_code", &format!("{path}/state"), format!("State {s:?} is not a two-letter US state code such as \"CO\", \"TX\" or \"DC\"."));
            }
            Some(_) if !is_us && country_ok => {
                self.r.error("state_code", &format!("{path}/state"), "A state can only be given when the country is \"US\"; remove it or set it to null.");
            }
            _ => {}
        }
        if let Some(rate) = foreign_tax_rate {
            self.r.range(
                &format!("{path}/foreign_tax_rate"),
                "Foreign tax rate",
                rate,
                0.0,
                0.7,
            );
        }
    }

    fn incomes(&mut self) {
        let p = self.p;
        check_ids(
            &mut self.r,
            "/incomes",
            p.incomes.iter().map(|x| x.id.as_str()),
        );
        for (i, inc) in p.incomes.iter().enumerate() {
            let path = format!("/incomes/{}", seg(&inc.id, i));
            self.person_ref(&format!("{path}/owner"), inc.owner);
            self.window(&path, inc.start.as_ref(), inc.end.as_ref());
            let k = format!("{path}/kind");
            match &inc.kind {
                IncomeKind::Job(job) => self.job(&k, job),
                IncomeKind::SelfEmployment(se) => {
                    crate::career_realism::validate::self_employment(
                        &mut self.r,
                        &k,
                        se,
                        p.assumptions.career_risk,
                    );
                    self.r.money(
                        &format!("{k}/net_annual"),
                        "Net self-employment income",
                        se.net_annual,
                    );
                    self.r.range(
                        &format!("{k}/volatility"),
                        "Volatility",
                        se.volatility,
                        0.0,
                        2.0,
                    );
                    self.r.range(
                        &format!("{k}/real_growth"),
                        "Real growth",
                        se.real_growth,
                        -0.2,
                        0.2,
                    );
                    self.r.range(
                        &format!("{k}/solo_401k_fraction"),
                        "Solo 401(k) fraction",
                        se.solo_401k_fraction,
                        0.0,
                        1.0,
                    );
                    self.business(&k, se);
                }
                IncomeKind::Pension(pn) => {
                    self.r.money(&format!("{k}/annual"), "Pension", pn.annual);
                    self.r.range(
                        &format!("{k}/survivor_fraction"),
                        "Survivor fraction",
                        pn.survivor_fraction,
                        0.0,
                        1.0,
                    );
                    if let Cola::Partial { rate } = pn.cola {
                        self.r
                            .range(&format!("{k}/cola/rate"), "COLA rate", rate, 0.0, 0.1);
                    }
                }
                IncomeKind::Annuity(a) => {
                    self.r
                        .money(&format!("{k}/annual"), "Annuity payment", a.annual);
                }
                IncomeKind::Other(o) => {
                    self.r.money(&format!("{k}/annual"), "Income", o.annual);
                    self.r.range(
                        &format!("{k}/real_growth"),
                        "Real growth",
                        o.real_growth,
                        -0.2,
                        0.2,
                    );
                }
            }
        }
    }

    /// The opt-in risks of a self-employment income.
    fn business(&mut self, k: &str, se: &SelfEmployment) {
        self.r.range(
            &format!("{k}/monthly_lumpiness"),
            "Monthly lumpiness",
            se.monthly_lumpiness,
            0.0,
            2.0,
        );
        let BusinessFailure::Hazard {
            young_annual_probability,
            mature_annual_probability,
            young_years,
            years_in_business,
            recession_multiplier,
            then,
        } = se.failure
        else {
            return;
        };
        let f = format!("{k}/failure");
        self.r.range(
            &format!("{f}/young_annual_probability"),
            "Chance of a young business closing",
            young_annual_probability,
            0.0,
            0.5,
        );
        self.r.range(
            &format!("{f}/mature_annual_probability"),
            "Chance of an established business closing",
            mature_annual_probability,
            0.0,
            0.5,
        );
        self.r.range(
            &format!("{f}/young_years"),
            "Years a business counts as young",
            young_years,
            0.0,
            30.0,
        );
        if let Some(years) = years_in_business {
            self.r.range(
                &format!("{f}/years_in_business"),
                "Years in business",
                years,
                0.0,
                100.0,
            );
        }
        self.r.range(
            &format!("{f}/recession_multiplier"),
            "Recession multiplier",
            recession_multiplier,
            1.0,
            10.0,
        );
        if let AfterFailure::Restart {
            gap_months,
            income_fraction,
        } = then
        {
            self.r.range(
                &format!("{f}/then/gap_months"),
                "Months without income",
                f64::from(gap_months),
                0.0,
                120.0,
            );
            self.r.range(
                &format!("{f}/then/income_fraction"),
                "Profit after a restart",
                income_fraction,
                0.0,
                2.0,
            );
        }
    }

    fn job(&mut self, k: &str, job: &Job) {
        crate::career_realism::validate::job(&mut self.r, k, job, self.p.assumptions.career_risk);
        self.r.money(&format!("{k}/salary"), "Salary", job.salary);
        match job.growth {
            WageGrowth::Flat { real_raise } => {
                self.r.range(
                    &format!("{k}/growth/real_raise"),
                    "Real raise",
                    real_raise,
                    -0.1,
                    0.15,
                );
            }
            WageGrowth::Lifecycle { extra, .. } => {
                self.r.range(
                    &format!("{k}/growth/extra"),
                    "Extra real growth",
                    extra,
                    -0.1,
                    0.1,
                );
            }
        }
        if let Some(b) = &job.bonus {
            self.r.range(
                &format!("{k}/bonus/target_fraction"),
                "Bonus target",
                b.target_fraction,
                0.0,
                5.0,
            );
            self.r.range(
                &format!("{k}/bonus/volatility"),
                "Bonus volatility",
                b.volatility,
                0.0,
                3.0,
            );
            if !(1..=12).contains(&b.month) {
                self.r.error(
                    "out_of_range",
                    &format!("{k}/bonus/month"),
                    "Bonus month must be 1 (January) to 12 (December).",
                );
            }
        }
        if let Some(c) = &job.commission {
            self.r.money(
                &format!("{k}/commission/expected_annual"),
                "Commission",
                c.expected_annual,
            );
            self.r.range(
                &format!("{k}/commission/volatility"),
                "Commission volatility",
                c.volatility,
                0.0,
                3.0,
            );
        }
        if let Some(plan) = &job.plan {
            let pp = format!("{k}/plan");
            self.r.range(
                &format!("{pp}/employee_fraction"),
                "Employee contribution",
                plan.employee_fraction,
                0.0,
                1.0,
            );
            self.r.range(
                &format!("{pp}/roth_fraction"),
                "Roth fraction",
                plan.roth_fraction,
                0.0,
                1.0,
            );
            self.r.range(
                &format!("{pp}/match_rate"),
                "Match rate",
                plan.match_rate,
                0.0,
                3.0,
            );
            self.r.range(
                &format!("{pp}/match_up_to"),
                "Match limit",
                plan.match_up_to,
                0.0,
                1.0,
            );
        }
        self.r.money(
            &format!("{k}/hsa_contribution"),
            "HSA contribution",
            job.hsa_contribution,
        );
        self.r.money(
            &format!("{k}/hsa_employer"),
            "Employer HSA contribution",
            job.hsa_employer,
        );
        check_ids(
            &mut self.r,
            &format!("{k}/equity"),
            job.equity.iter().map(|g| g.id.as_str()),
        );
        for (gi, g) in job.equity.iter().enumerate() {
            let gp = format!("{k}/equity/{}", seg(&g.id, gi));
            self.r.money(
                &format!("{gp}/unvested_value"),
                "Unvested value",
                g.unvested_value,
            );
            self.r.money(
                &format!("{gp}/refresh_annual_value"),
                "Refresh value",
                g.refresh_annual_value,
            );
            if g.cliff_months > g.vest_months_remaining && g.unvested_value > 0.0 {
                self.r.error(
                    "out_of_range",
                    &format!("{gp}/cliff_months"),
                    "The cliff cannot be later than the end of vesting.",
                );
            }
            if g.vest_months_remaining > 240 {
                self.r.error(
                    "out_of_range",
                    &format!("{gp}/vest_months_remaining"),
                    "Vesting must finish within 240 months.",
                );
            }
            if let EquityKind::Option { strike_fraction } = g.kind {
                self.r.range(
                    &format!("{gp}/kind/strike_fraction"),
                    "Strike fraction",
                    strike_fraction,
                    0.0,
                    10.0,
                );
            }
            match g.company {
                EquityCompany::Public {
                    volatility,
                    beta,
                    tails: _,
                    ruin,
                    layoff_link: _,
                } => {
                    if let StockRuin::Stated { annual_probability } = ruin {
                        self.r.range(
                            &format!("{gp}/company/ruin/annual_probability"),
                            "Chance of the shares becoming worthless",
                            annual_probability,
                            0.0,
                            0.5,
                        );
                    }
                    self.r.range(
                        &format!("{gp}/company/volatility"),
                        "Volatility",
                        volatility,
                        0.0,
                        2.0,
                    );
                    self.r
                        .range(&format!("{gp}/company/beta"), "Beta", beta, -1.0, 4.0);
                }
                EquityCompany::Private {
                    liquidity_hazard,
                    fail_prob,
                    exit_volatility,
                    exit_timing,
                    exit_size: _,
                } => {
                    if let ExitTiming::Lognormal {
                        median_years,
                        sigma,
                    } = exit_timing
                    {
                        self.r.range(
                            &format!("{gp}/company/exit_timing/median_years"),
                            "Typical wait for the outcome",
                            median_years,
                            0.25,
                            40.0,
                        );
                        self.r.range(
                            &format!("{gp}/company/exit_timing/sigma"),
                            "Spread of the wait",
                            sigma,
                            0.05,
                            2.0,
                        );
                    }
                    self.r.range(
                        &format!("{gp}/company/liquidity_hazard"),
                        "Liquidity hazard",
                        liquidity_hazard,
                        0.0,
                        1.0,
                    );
                    self.r.range(
                        &format!("{gp}/company/fail_prob"),
                        "Failure probability",
                        fail_prob,
                        0.0,
                        1.0,
                    );
                    self.r.range(
                        &format!("{gp}/company/exit_volatility"),
                        "Exit volatility",
                        exit_volatility,
                        0.0,
                        3.0,
                    );
                }
            }
        }
    }

    fn spending(&mut self) {
        let p = self.p;
        let s = &p.spending;
        check_ids(
            &mut self.r,
            "/spending/items",
            s.items.iter().map(|x| x.id.as_str()),
        );
        self.r.range(
            "/spending/lifestyle_creep",
            "Lifestyle creep",
            s.lifestyle_creep,
            0.0,
            1.0,
        );
        self.r.range(
            "/spending/survivor_factor",
            "Survivor factor",
            s.survivor_factor,
            0.3,
            1.2,
        );
        if let EarlyRetirementCoverage::Custom { annual } = s.healthcare.early_retirement {
            self.r.money(
                "/spending/healthcare/early_retirement/annual",
                "Healthcare cost",
                annual,
            );
        }
        for (i, it) in s.items.iter().enumerate() {
            let path = format!("/spending/items/{}", seg(&it.id, i));
            self.r
                .money(&format!("{path}/amount"), "Budget amount", it.amount);
            if let Some(g) = it.real_growth {
                self.r
                    .range(&format!("{path}/real_growth"), "Real growth", g, -0.2, 0.2);
            }
            if let Some(owner) = it.owner {
                self.person_ref(&format!("{path}/owner"), owner);
            }
            self.window(&path, it.start.as_ref(), it.end.as_ref());
            match it.in_retirement {
                RetirementChange::Factor { factor } => {
                    self.r.range(
                        &format!("{path}/in_retirement/factor"),
                        "Retirement factor",
                        factor,
                        0.0,
                        10.0,
                    );
                }
                RetirementChange::Amount { amount } => {
                    self.r.money(
                        &format!("{path}/in_retirement/amount"),
                        "Retirement amount",
                        amount,
                    );
                }
                RetirementChange::Same | RetirementChange::Stops => {}
            }
        }
    }

    fn accounts_and_portfolio(&mut self) {
        let p = self.p;
        check_ids(
            &mut self.r,
            "/accounts",
            p.accounts.iter().map(|x| x.id.as_str()),
        );
        for (i, a) in p.accounts.iter().enumerate() {
            let path = format!("/accounts/{}", seg(&a.id, i));
            self.person_ref(&format!("{path}/owner"), a.owner);
            self.r
                .money(&format!("{path}/balance"), "Balance", a.balance);
            if let Some(b) = a.cost_basis {
                self.r.money(&format!("{path}/cost_basis"), "Cost basis", b);
            }
            if let Some(b) = a.roth_basis {
                // A basis above the balance is legitimate after losses, so only the
                // sign is checked.
                self.r.money(&format!("{path}/roth_basis"), "Roth basis", b);
            }
            match a.cash_yield {
                CashYield::TracksBills { spread } => {
                    self.r.range(
                        &format!("{path}/cash_yield/spread"),
                        "Spread",
                        spread,
                        -0.1,
                        0.1,
                    );
                }
                CashYield::Fixed { rate } => {
                    self.r.range(
                        &format!("{path}/cash_yield/rate"),
                        "Interest rate",
                        rate,
                        0.0,
                        0.25,
                    );
                }
            }
            if let Some(h) = &a.holdings {
                self.allocation(&format!("{path}/holdings"), h);
            }
        }

        let pf = &p.portfolio;
        self.allocation("/portfolio/target", &pf.target);
        let uses_custom = pf.target[AssetClass::Custom] > 0.0
            || p.accounts
                .iter()
                .any(|a| a.holdings.is_some_and(|h| h[AssetClass::Custom] > 0.0));
        if uses_custom && pf.custom_asset.is_none() {
            self.r.error("custom_asset_missing", "/portfolio/custom_asset", "The allocation uses the \"custom\" asset class, so portfolio.custom_asset must define its name, mean, volatility and equity_corr.");
        }
        if let Some(c) = &pf.custom_asset {
            self.r.range(
                "/portfolio/custom_asset/mean",
                "Expected return",
                c.mean,
                -0.2,
                0.5,
            );
            self.r.range(
                "/portfolio/custom_asset/volatility",
                "Volatility",
                c.volatility,
                0.0,
                2.0,
            );
            self.r.range(
                "/portfolio/custom_asset/equity_corr",
                "Correlation",
                c.equity_corr,
                -1.0,
                1.0,
            );
        }
        self.r.range(
            "/portfolio/single_stock/volatility",
            "Volatility",
            pf.single_stock.volatility,
            0.0,
            2.0,
        );
        self.r.range(
            "/portfolio/single_stock/beta",
            "Beta",
            pf.single_stock.beta,
            -1.0,
            4.0,
        );
        // Opt-in shapes of the single stock, crypto and the custom asset.
        if let Some(hazard) = pf.single_stock.ruin_hazard {
            self.r.range(
                "/portfolio/single_stock/ruin_hazard",
                "Chance of total loss per year",
                hazard,
                0.0,
                0.5,
            );
        }
        if let CryptoDynamics::Cycles {
            calm_months,
            wild_months,
            volatility_ratio,
        } = pf.crypto.dynamics
        {
            let (lo, hi) = crate::economy::CYCLE_MONTHS_RANGE;
            self.r.range(
                "/portfolio/crypto/dynamics/calm_months",
                "Length of a calm spell in months",
                calm_months,
                lo,
                hi,
            );
            self.r.range(
                "/portfolio/crypto/dynamics/wild_months",
                "Length of a wild spell in months",
                wild_months,
                lo,
                hi,
            );
            let (lo, hi) = crate::economy::CYCLE_RATIO_RANGE;
            self.r.range(
                "/portfolio/crypto/dynamics/volatility_ratio",
                "Wild-to-calm volatility ratio",
                volatility_ratio,
                lo,
                hi,
            );
        }
        if let Some(event) = pf.custom_asset.as_ref().and_then(|c| c.loss_event.as_ref()) {
            self.r.range(
                "/portfolio/custom_asset/loss_event/annual_probability",
                "Chance of a loss per year",
                event.annual_probability,
                0.0,
                0.5,
            );
            check_uncertain(
                &mut self.r,
                "/portfolio/custom_asset/loss_event/loss",
                "Share lost",
                &event.loss,
                Bounds::FRACTION,
            );
        }
        self.r.range(
            "/portfolio/expense_ratio",
            "Expense ratio",
            pf.expense_ratio,
            0.0,
            0.05,
        );
        self.r.range(
            "/portfolio/advisory_fee",
            "Advisory fee",
            pf.advisory_fee,
            0.0,
            0.05,
        );
        self.r.range(
            "/portfolio/emergency_months",
            "Emergency fund months",
            pf.emergency_months,
            0.0,
            60.0,
        );
        self.r.range(
            "/portfolio/retirement_cash_years",
            "Retirement cash years",
            pf.retirement_cash_years,
            0.0,
            5.0,
        );
        if let Rebalance::Bands { width } = pf.rebalance {
            self.r.range(
                "/portfolio/rebalance/width",
                "Band width",
                width,
                0.005,
                0.5,
            );
        }
        match &pf.glide {
            GlidePath::None => {}
            GlidePath::TargetDate {
                years_before,
                equity_at_retirement,
                years_after,
                equity_final,
            } => {
                self.r.range(
                    "/portfolio/glide/years_before",
                    "Years before",
                    *years_before,
                    0.0,
                    60.0,
                );
                self.r.range(
                    "/portfolio/glide/years_after",
                    "Years after",
                    *years_after,
                    0.0,
                    60.0,
                );
                self.r.range(
                    "/portfolio/glide/equity_at_retirement",
                    "Equity fraction",
                    *equity_at_retirement,
                    0.0,
                    1.0,
                );
                self.r.range(
                    "/portfolio/glide/equity_final",
                    "Equity fraction",
                    *equity_final,
                    0.0,
                    1.0,
                );
            }
            GlidePath::BondTent {
                years_before,
                equity_at_retirement,
                years_after,
            } => {
                self.r.range(
                    "/portfolio/glide/years_before",
                    "Years before",
                    *years_before,
                    0.0,
                    60.0,
                );
                self.r.range(
                    "/portfolio/glide/years_after",
                    "Years after",
                    *years_after,
                    0.0,
                    60.0,
                );
                self.r.range(
                    "/portfolio/glide/equity_at_retirement",
                    "Equity fraction",
                    *equity_at_retirement,
                    0.0,
                    1.0,
                );
            }
            GlidePath::Custom { points } => {
                if points.is_empty() {
                    self.r.error(
                        "glide_empty",
                        "/portfolio/glide/points",
                        "A custom glide path needs at least one point.",
                    );
                }
                for (i, pt) in points.iter().enumerate() {
                    self.r.range(
                        &format!("/portfolio/glide/points/{i}/equity_fraction"),
                        "Equity fraction",
                        pt.equity_fraction,
                        0.0,
                        1.0,
                    );
                    self.r.range(
                        &format!("/portfolio/glide/points/{i}/years_to_retirement"),
                        "Years to retirement",
                        pt.years_to_retirement,
                        -80.0,
                        80.0,
                    );
                }
            }
        }
    }

    fn property_params(&mut self, path: &str, costs: &PropertyCosts<'_>) {
        let PropertyCosts {
            tax,
            ins,
            maint,
            hoa,
            appr,
            vol,
            usage,
        } = *costs;
        if let Some(t) = tax {
            self.r.range(
                &format!("{path}/property_tax_rate"),
                "Property tax rate",
                t,
                0.0,
                0.06,
            );
        }
        self.r.range(
            &format!("{path}/insurance_rate"),
            "Insurance rate",
            ins,
            0.0,
            0.06,
        );
        self.r.range(
            &format!("{path}/maintenance_rate"),
            "Maintenance rate",
            maint,
            0.0,
            0.1,
        );
        self.r.money(&format!("{path}/hoa_monthly"), "HOA fee", hoa);
        self.r.range(
            &format!("{path}/appreciation_vs_national"),
            "Relative appreciation",
            appr,
            -0.1,
            0.1,
        );
        self.r.range(
            &format!("{path}/idiosyncratic_volatility"),
            "Volatility",
            vol,
            0.0,
            0.5,
        );
        if let PropertyUse::Rental(t) = usage {
            let u = format!("{path}/use");
            self.r
                .money(&format!("{u}/monthly_rent"), "Rent", t.monthly_rent);
            self.r
                .range(&format!("{u}/vacancy"), "Vacancy", t.vacancy, 0.0, 1.0);
            self.r.range(
                &format!("{u}/management_fee"),
                "Management fee",
                t.management_fee,
                0.0,
                0.5,
            );
            self.r.range(
                &format!("{u}/rent_real_growth"),
                "Rent growth",
                t.rent_real_growth,
                -0.1,
                0.1,
            );
            self.r.range(
                &format!("{u}/land_fraction"),
                "Land fraction",
                t.land_fraction,
                0.0,
                1.0,
            );
            self.r.range(
                &format!("{u}/years_in_service"),
                "Years in service",
                t.years_in_service,
                0.0,
                100.0,
            );
        }
    }

    fn properties_and_debts(&mut self) {
        let p = self.p;
        check_ids(
            &mut self.r,
            "/properties",
            p.properties.iter().map(|x| x.id.as_str()),
        );
        for (i, pr) in p.properties.iter().enumerate() {
            let path = format!("/properties/{}", seg(&pr.id, i));
            self.r
                .money(&format!("{path}/value"), "Property value", pr.value);
            if let Some(b) = pr.basis {
                self.r.money(&format!("{path}/basis"), "Basis", b);
            }
            if let Some(m) = &pr.mortgage {
                let mp = format!("{path}/mortgage");
                self.r
                    .money(&format!("{mp}/balance"), "Mortgage balance", m.balance);
                self.r
                    .range(&format!("{mp}/rate"), "Mortgage rate", m.rate, 0.0, 0.25);
                if m.balance > 0.0 && (m.remaining_months == 0 || m.remaining_months > 600) {
                    self.r.error(
                        "out_of_range",
                        &format!("{mp}/remaining_months"),
                        "A mortgage with a balance needs between 1 and 600 remaining months.",
                    );
                }
                if let Some(pay) = m.payment {
                    self.r
                        .money(&format!("{mp}/payment"), "Mortgage payment", pay);
                    if pay.is_finite() && m.balance > 0.0 && pay <= m.balance * m.rate / 12.0 {
                        self.r.error("payment_below_interest", &format!("{mp}/payment"), format!("A payment of {pay:.0} does not cover the monthly interest of {:.0}, so the loan would never be repaid. Remove the payment to have it computed.", m.balance * m.rate / 12.0));
                    }
                }
            }
            self.property_params(
                &path,
                &PropertyCosts {
                    tax: pr.property_tax_rate,
                    ins: pr.insurance_rate,
                    maint: pr.maintenance_rate,
                    hoa: pr.hoa_monthly,
                    appr: pr.appreciation_vs_national,
                    vol: pr.idiosyncratic_volatility,
                    usage: &pr.usage,
                },
            );
        }

        check_ids(&mut self.r, "/debts", p.debts.iter().map(|x| x.id.as_str()));
        for (i, d) in p.debts.iter().enumerate() {
            let path = format!("/debts/{}", seg(&d.id, i));
            self.r
                .money(&format!("{path}/balance"), "Debt balance", d.balance);
            self.r
                .range(&format!("{path}/rate"), "Interest rate", d.rate, 0.0, 0.6);
            if let Some(pay) = d.payment {
                self.r.money(&format!("{path}/payment"), "Payment", pay);
            }
            if d.remaining_months.is_some_and(|m| m > 600) {
                self.r.error(
                    "out_of_range",
                    &format!("{path}/remaining_months"),
                    "Remaining months must be at most 600.",
                );
            }
        }
        self.r.money(
            "/debt_policy/extra_monthly",
            "Extra debt payment",
            p.debt_policy.extra_monthly,
        );
    }

    fn events(&mut self) {
        let p = self.p;
        check_ids(
            &mut self.r,
            "/events",
            p.events.iter().map(|x| x.id.as_str()),
        );

        // Properties that exist at some point: owned now or created by a purchase event.
        let mut property_ids: Vec<&str> = p.properties.iter().map(|x| x.id.as_str()).collect();
        for (i, ev) in p.events.iter().enumerate() {
            if let EventKind::BuyHome(b) = &ev.kind {
                let path = format!("/events/{}/kind/property_id", seg(&ev.id, i));
                if !id_is_valid(&b.property_id) {
                    self.r.error("id_invalid", &path, "property_id must be a non-empty id (letters, digits, \"_\", \"-\", \".\").");
                } else if property_ids.contains(&b.property_id.as_str()) {
                    self.r.error("id_duplicate", &path, format!("property_id {:?} is already used by another property or purchase; choose a different one.", b.property_id));
                } else {
                    property_ids.push(&b.property_id);
                }
            }
        }

        for (i, ev) in p.events.iter().enumerate() {
            let path = format!("/events/{}", seg(&ev.id, i));
            self.when(&format!("{path}/when"), &ev.when);
            self.r.range(
                &format!("{path}/probability"),
                "Probability",
                ev.probability,
                0.0,
                1.0,
            );
            self.r.range(
                &format!("{path}/timing_uncertainty_years"),
                "Timing uncertainty",
                ev.timing_uncertainty_years,
                0.0,
                30.0,
            );
            let k = format!("{path}/kind");
            match &ev.kind {
                EventKind::BuyHome(b) => {
                    self.r.money(&format!("{k}/price"), "Price", b.price);
                    self.r.range(
                        &format!("{k}/down_payment_fraction"),
                        "Down payment",
                        b.down_payment_fraction,
                        0.0,
                        1.0,
                    );
                    if let Some(rate) = b.rate {
                        self.r
                            .range(&format!("{k}/rate"), "Mortgage rate", rate, 0.0, 0.25);
                    }
                    if !(1..=50).contains(&b.term_years) {
                        self.r.error(
                            "out_of_range",
                            &format!("{k}/term_years"),
                            "Mortgage term must be between 1 and 50 years.",
                        );
                    }
                    self.r.range(
                        &format!("{k}/closing_cost_fraction"),
                        "Closing costs",
                        b.closing_cost_fraction,
                        0.0,
                        0.2,
                    );
                    self.property_params(
                        &k,
                        &PropertyCosts {
                            tax: b.property_tax_rate,
                            ins: b.insurance_rate,
                            maint: b.maintenance_rate,
                            hoa: b.hoa_monthly,
                            appr: b.appreciation_vs_national,
                            vol: b.idiosyncratic_volatility,
                            usage: &b.usage,
                        },
                    );
                }
                EventKind::SellProperty(s) => {
                    if !property_ids.contains(&s.property.as_str()) {
                        self.r.error(
                            "ref_property",
                            &format!("{k}/property"),
                            format!(
                                "No property with id {:?}. Properties and planned purchases: {}.",
                                s.property,
                                list_ids(property_ids.iter().copied())
                            ),
                        );
                    }
                    self.r.range(
                        &format!("{k}/selling_cost_fraction"),
                        "Selling costs",
                        s.selling_cost_fraction,
                        0.0,
                        0.3,
                    );
                    if let Some(rent) = s.then_rent_monthly {
                        self.r
                            .money(&format!("{k}/then_rent_monthly"), "Rent", rent);
                    }
                }
                EventKind::Relocate(rl) => {
                    self.location(&k, &rl.country, rl.state.as_deref(), rl.foreign_tax_rate);
                    self.r
                        .money(&format!("{k}/moving_cost"), "Moving cost", rl.moving_cost);
                    match &rl.cost_of_living {
                        CostOfLivingChange::Unchanged => {}
                        CostOfLivingChange::Factor { overall } => {
                            self.r.range(
                                &format!("{k}/cost_of_living/overall"),
                                "Cost-of-living factor",
                                *overall,
                                0.1,
                                10.0,
                            );
                        }
                        CostOfLivingChange::ByCategory { factors } => {
                            for f in factors.values() {
                                self.r.range(
                                    &format!("{k}/cost_of_living/factors"),
                                    "Cost-of-living factor",
                                    *f,
                                    0.1,
                                    10.0,
                                );
                            }
                        }
                    }
                }
                EventKind::Windfall { amount, .. } => {
                    self.r.money(&format!("{k}/amount"), "Windfall", *amount);
                }
                EventKind::Expense { amount, financed } => {
                    self.r.money(&format!("{k}/amount"), "Expense", *amount);
                    if let Some(f) = financed {
                        self.r.range(
                            &format!("{k}/financed/down_fraction"),
                            "Down payment",
                            f.down_fraction,
                            0.0,
                            1.0,
                        );
                        self.r
                            .range(&format!("{k}/financed/rate"), "Loan rate", f.rate, 0.0, 0.6);
                        if !(1..=600).contains(&f.term_months) {
                            self.r.error(
                                "out_of_range",
                                &format!("{k}/financed/term_months"),
                                "Loan term must be between 1 and 600 months.",
                            );
                        }
                    }
                }
                EventKind::IncomeChange { income, change } => {
                    self.income_ref(&format!("{k}/income"), income, false);
                    match *change {
                        IncomeChangeKind::SetSalary { amount } => {
                            self.r
                                .money(&format!("{k}/change/amount"), "New amount", amount);
                        }
                        IncomeChangeKind::Scale { factor } => {
                            self.r.range(
                                &format!("{k}/change/factor"),
                                "Scale factor",
                                factor,
                                0.0,
                                20.0,
                            );
                        }
                        IncomeChangeKind::End => {}
                    }
                }
                EventKind::CareerBreak {
                    person,
                    months,
                    income_fraction,
                } => {
                    self.person_ref(&format!("{k}/person"), *person);
                    if !(1..=240).contains(months) {
                        self.r.error(
                            "out_of_range",
                            &format!("{k}/months"),
                            "A career break must last between 1 and 240 months.",
                        );
                    }
                    self.r.range(
                        &format!("{k}/income_fraction"),
                        "Income fraction",
                        *income_fraction,
                        0.0,
                        1.0,
                    );
                }
                EventKind::SpendingChange { target, change } => {
                    if let SpendingTarget::Item { id } = target {
                        if !p.spending.items.iter().any(|it| &it.id == id) {
                            self.r.error(
                                "ref_item",
                                &format!("{k}/target/id"),
                                format!(
                                    "No budget item with id {id:?}. Budget item ids: {}.",
                                    list_ids(p.spending.items.iter().map(|it| it.id.as_str()))
                                ),
                            );
                        }
                    }
                    match *change {
                        SpendingChangeKind::Scale { factor } => {
                            self.r.range(
                                &format!("{k}/change/factor"),
                                "Scale factor",
                                factor,
                                0.0,
                                20.0,
                            );
                        }
                        SpendingChangeKind::Set { annual } => {
                            self.r
                                .money(&format!("{k}/change/annual"), "New amount", annual);
                        }
                    }
                }
                EventKind::ContributionChange {
                    income,
                    employee_fraction,
                } => {
                    self.income_ref(&format!("{k}/income"), income, true);
                    self.r.range(
                        &format!("{k}/employee_fraction"),
                        "Employee contribution",
                        *employee_fraction,
                        0.0,
                        1.0,
                    );
                }
                EventKind::Divorce(terms) => self.divorce_event(&k, terms),
            }
        }
    }

    fn private_equity(&mut self) {
        let p = self.p;
        let n = p.private_equity.len();
        if n > equity::MAX_COMPANIES {
            self.r.error(
                "too_many_companies",
                "/private_equity",
                format!(
                    "A plan can hold at most {} private companies; this one has {n}. Combine or remove the smallest positions.",
                    equity::MAX_COMPANIES
                ),
            );
        }
        check_ids(
            &mut self.r,
            "/private_equity",
            p.private_equity.iter().map(|x| x.id.as_str()),
        );
        for (i, co) in p.private_equity.iter().enumerate() {
            let c = format!("/private_equity/{}", seg(&co.id, i));
            self.person_ref(&format!("{c}/owner"), co.owner);
            if let Some(job) = &co.job {
                match p.incomes.iter().find(|inc| inc.id == *job) {
                    None => self.income_ref(&format!("{c}/job"), job, false),
                    Some(inc) if !matches!(inc.kind, IncomeKind::Job(_)) => {
                        self.r.error(
                            "ref_income",
                            &format!("{c}/job"),
                            format!("Income {job:?} is not a job. Link a company to the job that grants the equity, or leave job empty for a former employer or an investment."),
                        );
                    }
                    Some(inc) if inc.owner != co.owner => {
                        self.r.error(
                            "equity_owner_mismatch",
                            &format!("{c}/owner"),
                            format!(
                                "This company's equity belongs to person {} but the linked job {job:?} belongs to person {}. Set owner to {}.",
                                co.owner, inc.owner, inc.owner
                            ),
                        );
                    }
                    Some(_) => {}
                }
            }
            company_checks(&mut self.r, &c, co);
        }
    }

    /// Lints on private-company equity. `liquid` is the sum of all account balances.
    fn equity_plausibility(&mut self, liquid: f64, budget: f64) {
        let p = self.p;
        for (i, inc) in p.incomes.iter().enumerate() {
            if let IncomeKind::Job(job) = &inc.kind {
                if job
                    .equity
                    .iter()
                    .any(|g| matches!(g.company, EquityCompany::Private { .. }))
                {
                    self.r.warn(
                        "private_grant_legacy",
                        &format!("/incomes/{}/kind/equity", seg(&inc.id, i)),
                        "Private-company grants on a job use the older single-event model. Move this position to private_equity for tender offers, exercise decisions and capital-gains tax.",
                    );
                }
            }
        }
        if p.private_equity.is_empty() {
            return;
        }
        let spendable: f64 = p
            .accounts
            .iter()
            .filter(|a| matches!(a.kind, AccountKind::Cash | AccountKind::Taxable))
            .map(|a| a.balance)
            .sum::<f64>()
            - p.portfolio.emergency_months * budget / 12.0;
        let ctx = p.when_context();
        for (i, co) in p.private_equity.iter().enumerate() {
            let c = format!("/private_equity/{}", seg(&co.id, i));
            let p0 = co.preferred_price;
            let (_, value) = equity::position_value(co);
            if co.included && value > 0.0 && value > 2.0 * liquid {
                self.r.warn(
                    "equity_dominant",
                    &format!("{c}/holdings"),
                    format!("This position is worth about {} at the last round's price, more than twice the household's savings. The outcome is uncertain and often nothing: compare the results with this company not counted.", dollars(value)),
                );
            }
            if co.fmv_409a.is_some_and(|v| v > p0) {
                self.r.warn(
                    "fmv_above_preferred",
                    &format!("{c}/fmv_409a"),
                    "The 409A value is above the preferred price, which is unusual. Check the two figures are per share and from the same date.",
                );
            }
            let owner = p.household.people.get(co.owner);
            if let (Some(person), Some(start)) = (owner, p.start) {
                if person.joins.is_some_and(|j| j > start) {
                    self.r.warn(
                        "equity_owner_joins_later",
                        &format!("{c}/owner"),
                        format!(
                            "Anything this company pays before {} joins the household is not counted, and options are not exercised before then. Vesting goes on meanwhile if the linked job has started.",
                            display_name(&person.name, &person.id)
                        ),
                    );
                }
            }
            let job = co
                .job
                .as_ref()
                .and_then(|id| p.incomes.iter().find(|inc| inc.id == *id));
            if let (Some(inc), Some(ctx)) = (job, ctx.as_ref()) {
                // A start that cannot be dated yet (relative to retirement) is in
                // the future too.
                let later = inc
                    .start
                    .as_ref()
                    .is_some_and(|w| !w.resolve(ctx).is_some_and(|at| at <= ctx.start));
                if later {
                    self.r.warn(
                        "equity_job_not_started",
                        &format!("{c}/job"),
                        "The linked job has not started yet, so nothing vests until it does.",
                    );
                }
            }
            let mut strike_cost = 0.0;
            for (j, hd) in co.holdings.iter().enumerate() {
                let h = format!("{c}/holdings/{}", seg(&hd.id, j));
                let yearly = equity::iso_yearly_amount(hd);
                if yearly > equity::ISO_ANNUAL_LIMIT {
                    self.r.warn(
                        "iso_over_100k_limit",
                        &format!("{h}/kind"),
                        format!("Only $100,000 of incentive options (shares times strike) can first become exercisable in a calendar year; about {} a year does here. The excess is taxed as non-qualified options, which costs more to exercise. Your grant documents show the split: enter the two parts as an ISO holding and an NSO holding.", dollars(yearly)),
                    );
                }
                if matches!(hd.kind, HoldingKind::Iso { .. })
                    && co.job.is_none()
                    && co.exercise_window_months > equity::ISO_POST_TERMINATION_MONTHS
                {
                    self.r.warn(
                        "iso_former_employer",
                        &format!("{h}/kind"),
                        format!(
                            "Incentive options exercised more than three months after leaving are taxed as non-qualified options. With {} months left to exercise, these are modelled that way.",
                            co.exercise_window_months
                        ),
                    );
                }
                if let HoldingKind::Shares {
                    basis_per_share,
                    fmv_at_exercise,
                    ..
                } = hd.kind
                {
                    if p0 > 0.0 && basis_per_share > p0 {
                        self.r.warn(
                            "basis_above_price",
                            &format!("{h}/kind/basis_per_share"),
                            format!(
                                "The basis is {} a share, above the latest preferred price of {}. Check the figure is what you paid per share, not the total paid: a basis this high turns every sale into a tax loss.",
                                dollars_cents(basis_per_share),
                                dollars_cents(p0)
                            ),
                        );
                    }
                    if p0 > 0.0 && fmv_at_exercise > p0 {
                        self.r.warn(
                            "fmv_at_exercise_above_price",
                            &format!("{h}/kind/fmv_at_exercise"),
                            format!(
                                "The 409A value at exercise is {} a share, above the latest preferred price of {}. Check the figure is per share, not a total.",
                                dollars_cents(fmv_at_exercise),
                                dollars_cents(p0)
                            ),
                        );
                    }
                }
                if let HoldingKind::Iso {
                    strike,
                    granted_months_ago,
                }
                | HoldingKind::Nso {
                    strike,
                    granted_months_ago,
                } = hd.kind
                {
                    let left =
                        i64::from(equity::OPTION_TERM_MONTHS) - i64::from(granted_months_ago);
                    if left > 0 && left < i64::from(hd.vest_months_left) {
                        self.r.warn(
                            "option_term_before_vesting",
                            &format!("{h}/kind/granted_months_ago"),
                            format!(
                                "These options reach their ten-year term in {left} {}, before they finish vesting in {}. What has not vested by then lapses. Check the months since the grant and the months of vesting left.",
                                if left == 1 { "month" } else { "months" },
                                hd.vest_months_left
                            ),
                        );
                    }
                    if left > 0 && left <= OPTION_TERM_SOON_MONTHS {
                        let vested = hd.vested.clamp(0.0, hd.shares.max(0.0));
                        let then = match co.policy.at_deadline {
                            DeadlinePolicy::Exercise if p0 > 0.0 && strike < p0 => format!(
                                "the plan exercises what it can afford then, about {} of strike for the options vested today plus the tax, and the rest lapse",
                                dollars(vested * strike)
                            ),
                            _ => "the plan lets them lapse then".to_string(),
                        };
                        self.r.warn(
                            "option_term_soon",
                            &format!("{h}/kind/granted_months_ago"),
                            format!(
                                "These options reach their ten-year term in {left} {}: unless a sale, listing or tender offer comes first, {then}.",
                                if left == 1 { "month" } else { "months" }
                            ),
                        );
                    }
                }
                if let Some(strike) = hd.kind.strike() {
                    if p0 > 0.0 && strike >= p0 {
                        self.r.warn(
                            "strike_above_price",
                            &format!("{h}/kind/strike"),
                            "The strike is at or above the latest preferred price, so these options pay only if the price rises.",
                        );
                    } else {
                        strike_cost += hd.vested.clamp(0.0, hd.shares.max(0.0)) * strike;
                    }
                }
            }
            if co.included
                && co.policy.exercise != ExerciseTiming::AtLiquidity
                && strike_cost > spendable.max(0.0)
            {
                self.r.warn(
                    "exercise_unaffordable",
                    &format!("{c}/policy/exercise"),
                    format!(
                        "Exercising the vested options now costs {} before tax, more than the {} in cash and taxable accounts above the emergency fund. Only the affordable part is exercised.",
                        dollars(strike_cost),
                        dollars(spendable.max(0.0))
                    ),
                );
            }
            if co.included && co.qsbs && co.policy.sell_down == SellDown::Hold {
                self.r.warn(
                    "qsbs_hold",
                    &format!("{c}/policy/sell_down"),
                    "Shares kept as a single stock cannot carry the small-business-stock exclusion in this model. Shares that qualify are therefore held as they are until their gain is fully excluded (five years after you got them) and sold then; only the other shares are kept.",
                );
            }
            if let Some(entered) = co.tenders.annual_probability {
                // Only companies heading to or above their last round run tenders,
                // so a chance entered as an average over every future can be too
                // high to meet (`equity::tender_rate` stops at once a year).
                let params = EquityParams::from_data(Data::embedded());
                let table = equity::resolved_table(co, &params);
                let [up, down] = equity::tender_exposure(&table, &params);
                let most = if up > 0.0 { up / (up + down) } else { 1.0 };
                if co.included && entered.is_finite() && entered > most + 0.005 {
                    self.r.warn(
                        "tender_chance_capped",
                        &format!("{c}/tenders/annual_probability"),
                        format!(
                            "A tender offer chance of {:.0}% a year cannot be met. Tender offers happen only in futures where the company ends at or above its last round's price, at most once a year, and with these odds that allows about {:.0}% a year on average over every future. The plan runs at that. To count a tender you know is coming, give the months until it instead.",
                            entered * 100.0,
                            most * 100.0
                        ),
                    );
                }
            }
            for (k, row) in co.outcomes.iter().enumerate() {
                // The schema's defaults for a row are a price of exactly 1x in years 1
                // to 5: a row written with only a probability gets them.
                if matches!(row.kind, OutcomeKind::Acquired | OutcomeKind::Ipo)
                    && row.probability > 0.0
                    && row.multiple_low == row.multiple_high
                {
                    self.r.warn(
                        "outcome_flat_price",
                        &format!("{c}/outcomes/{}", seg(&row.id, k)),
                        format!(
                            "This row has one exact price, {} times the last round's, with no spread, in years {} to {}. A row given only a probability gets 1 times in years 1 to 5, not the stage's typical prices and timing. Give the low and high multiples and the years, or leave the table empty to use the typical odds.",
                            row.multiple_low, row.years_from, row.years_to
                        ),
                    );
                }
            }
            if !co.outcomes.is_empty() {
                let params = EquityParams::from_data(Data::embedded());
                let table = equity::resolved_table(co, &params);
                let typical = equity::preset_table(
                    co.stage,
                    Outlook::Typical,
                    params.overhang[co.stage.index()],
                    &params,
                );
                let a = equity::implied_multiple(&table, table.overhang, &params);
                let b = equity::implied_multiple(&typical, typical.overhang, &params);
                let fails = table.probability_of(OutcomeKind::Fails);
                let typical_fails = typical.probability_of(OutcomeKind::Fails);
                if fails < 0.5 * typical_fails || a > 2.0 * b {
                    self.r.warn(
                        "outcomes_optimistic",
                        &format!("{c}/outcomes"),
                        format!(
                            "Your odds put a share at about {a:.1} times the last round's price; typical odds for this stage put it at {b:.1}. Failure is {:.0}% in your table against about {:.0}% typically. Results rest on your view.",
                            fails * 100.0,
                            typical_fails * 100.0
                        ),
                    );
                }
            }
        }
        let counted = p
            .private_equity
            .iter()
            .filter(|co| co.included && !co.holdings.is_empty())
            .count();
        if counted > 1 {
            let main = &p.private_equity[equity::primary_company(&p.private_equity)];
            self.r.warn(
                "several_companies",
                "/private_equity",
                format!(
                    "Outcome statistics describe {}, the largest position; proceeds and costs are totalled over all companies.",
                    display_name(&main.name, &main.id)
                ),
            );
        }
    }

    fn income_ref(&mut self, path: &str, id: &str, needs_plan: bool) {
        let p = self.p;
        match p.incomes.iter().find(|inc| inc.id == id) {
            None => self.r.error(
                "ref_income",
                path,
                format!(
                    "No income with id {id:?}. Income ids: {}.",
                    list_ids(p.incomes.iter().map(|x| x.id.as_str()))
                ),
            ),
            Some(inc)
                if needs_plan && !matches!(&inc.kind, IncomeKind::Job(j) if j.plan.is_some()) =>
            {
                self.r.error("ref_income", path, format!("Income {id:?} is not a job with an employer plan, so its contribution cannot be changed. Add a plan to the job first."));
            }
            Some(_) => {}
        }
    }

    fn retirement(&mut self) {
        let p = self.p;
        let rt = &p.retirement;
        match rt.trigger {
            RetirementTrigger::AtAge { person, age } => {
                self.person_ref("/retirement/trigger/person", person);
                self.r.range(
                    "/retirement/trigger/age",
                    "Retirement age",
                    age,
                    18.0,
                    100.0,
                );
            }
            RetirementTrigger::AtDate { at } => {
                if !(1990..=2200).contains(&at.year()) {
                    self.r.error(
                        "date_range",
                        "/retirement/trigger/at",
                        format!("Retirement date {at} is outside 1990 to 2200."),
                    );
                }
            }
            RetirementTrigger::WhenFunded(w) => {
                self.r.range(
                    "/retirement/trigger/withdrawal_rate",
                    "Withdrawal rate",
                    w.withdrawal_rate,
                    0.005,
                    0.2,
                );
                self.r.range(
                    "/retirement/trigger/max_age",
                    "Maximum retirement age",
                    w.max_age,
                    18.0,
                    100.0,
                );
                if let Some(min) = w.min_age {
                    if self.r.range(
                        "/retirement/trigger/min_age",
                        "Minimum retirement age",
                        min,
                        18.0,
                        100.0,
                    ) && min > w.max_age
                    {
                        self.r.error(
                            "out_of_range",
                            "/retirement/trigger/min_age",
                            format!("min_age ({min}) is above max_age ({}).", w.max_age),
                        );
                    }
                }
            }
            RetirementTrigger::FixedNumber { amount, .. } => {
                self.r
                    .money("/retirement/trigger/amount", "Target amount", amount);
            }
            RetirementTrigger::AlreadyRetired => {}
        }
        self.r.range(
            "/retirement/partner_offset_years",
            "Partner offset",
            rt.partner_offset_years,
            -40.0,
            40.0,
        );
        self.r.money(
            "/retirement/bequest_target",
            "Bequest target",
            rt.bequest_target,
        );
        let sr = "/retirement/spending_rule";
        match rt.spending_rule {
            SpendingRule::ConstantReal | SpendingRule::Vpw => {}
            SpendingRule::PercentOfPortfolio { rate } => {
                self.r
                    .range(&format!("{sr}/rate"), "Spending rate", rate, 0.005, 0.2);
            }
            SpendingRule::Guardrails {
                upper,
                lower,
                adjust,
            } => {
                self.r
                    .range(&format!("{sr}/upper"), "Upper guardrail", upper, 0.01, 1.0);
                self.r
                    .range(&format!("{sr}/lower"), "Lower guardrail", lower, 0.01, 1.0);
                self.r
                    .range(&format!("{sr}/adjust"), "Adjustment", adjust, 0.01, 0.5);
            }
            SpendingRule::VanguardDynamic { ceiling, floor, .. } => {
                self.r
                    .range(&format!("{sr}/ceiling"), "Ceiling", ceiling, 0.0, 0.5);
                self.r
                    .range(&format!("{sr}/floor"), "Floor", floor, 0.0, 0.5);
            }
            SpendingRule::FloorCeiling { floor, ceiling, .. } => {
                self.r
                    .range(&format!("{sr}/floor"), "Floor", floor, 0.0, 1.0);
                self.r
                    .range(&format!("{sr}/ceiling"), "Ceiling", ceiling, 1.0, 5.0);
            }
        }
        if let WithdrawalOrder::TaxAware {
            fill_to_bracket, ..
        } = rt.withdrawal_order
        {
            self.r.range(
                "/retirement/withdrawal_order/fill_to_bracket",
                "Bracket rate",
                fill_to_bracket,
                0.0,
                0.5,
            );
        }
    }

    /// Long-run uncertainty and rare disasters (`assumptions.market`).
    /// A long-run uncertainty moves an adjustment for a whole simulated life, so
    /// the adjustment plus the furthest shift either way (the range less its
    /// average) must itself be an adjustment the validator would accept. `dist` has
    /// passed `check_uncertain`. The error goes to the field that sets the
    /// offending end of the range.
    fn shifted_adjustment(
        &mut self,
        path: &str,
        what: &str,
        dist: &Uncertain,
        (name, adjustment): (&str, f64),
        (lo, hi): (f64, f64),
    ) {
        // An adjustment outside its own range is reported at its own path.
        if !(lo..=hi).contains(&adjustment) {
            return;
        }
        let compiled = CompiledDist::new(dist);
        let mean = compiled.mean();
        let (lowest, highest) = (
            adjustment + (compiled.min() - mean),
            adjustment + (compiled.max() - mean),
        );
        // Room for rounding: the average is that of the compiled sampler, which
        // differs from the textbook one in about the eighth decimal.
        const SLACK: f64 = 1e-6;
        let (low_field, high_field) = match dist {
            Uncertain::Uniform { .. } | Uncertain::Triangular { .. } | Uncertain::Pert { .. } => {
                ("low", "high")
            }
            Uncertain::Normal { .. } => ("sd", "sd"),
            Uncertain::Lognormal { .. } => ("p90", "p90"),
            Uncertain::Quantiles { .. } => ("p10", "p90"),
        };
        let (too_low, too_high) = (lowest < lo - SLACK, highest > hi + SLACK);
        // Six decimals are plenty for a rate and hide the rounding of the sums.
        let shown = |x: f64| (x * 1e6).round() / 1e6;
        let (mean, lowest, highest) = (shown(mean), shown(lowest), shown(highest));
        let mut report = |field: &str, reached: f64, side: &str| {
            self.r.error(
                "out_of_range",
                &format!("{path}/{field}"),
                format!(
                    "{what}: with {name} at {adjustment}, the furthest shift {side} (the range less its average of {mean}) takes it to {reached} a year for a whole simulated life; {name} plus the shift must stay between {lo} and {hi}. Narrow the range."
                ),
            );
        };
        if too_low {
            report(low_field, lowest, "down");
        }
        if too_high {
            report(high_field, highest, "up");
        }
    }

    fn market_risk(&mut self) {
        let m = &self.p.assumptions.market;
        if let Some(dist) = &m.equity_premium_uncertainty {
            let path = "/assumptions/market/equity_premium_uncertainty";
            let what = "Uncertainty about the long-run stock return";
            if check_uncertain(&mut self.r, path, what, dist, Bounds::new(-0.2, 0.2)) {
                self.shifted_adjustment(
                    path,
                    what,
                    dist,
                    ("equity_premium_adjustment", m.equity_premium_adjustment),
                    EQUITY_ADJUSTMENT_RANGE,
                );
            }
        }
        if let Some(dist) = &m.inflation_uncertainty {
            let path = "/assumptions/market/inflation_uncertainty";
            let what = "Uncertainty about long-run inflation";
            if check_uncertain(&mut self.r, path, what, dist, Bounds::new(-0.1, 0.1)) {
                self.shifted_adjustment(
                    path,
                    what,
                    dist,
                    ("inflation_adjustment", m.inflation_adjustment),
                    INFLATION_ADJUSTMENT_RANGE,
                );
            }
        }
        let d = &m.disasters;
        let base = "/assumptions/market/disasters";
        self.r.range(
            &format!("{base}/annual_probability"),
            "Annual probability of a disaster",
            d.annual_probability,
            0.0,
            0.1,
        );
        check_uncertain(
            &mut self.r,
            &format!("{base}/depth"),
            "Disaster depth",
            &d.depth,
            Bounds::new(0.0, crate::economy::MAX_DISASTER_DEPTH),
        );
        let (shortest, longest) = crate::economy::DISASTER_MONTHS_RANGE;
        check_uncertain(
            &mut self.r,
            &format!("{base}/years"),
            "Disaster length in years",
            &d.years,
            Bounds::new(shortest as f64 / 12.0, longest as f64 / 12.0),
        );
        self.r.range(
            &format!("{base}/inflation_probability"),
            "Probability of an inflation burst",
            d.inflation_probability,
            0.0,
            1.0,
        );
        check_uncertain(
            &mut self.r,
            &format!("{base}/inflation"),
            "Extra inflation in a disaster",
            &d.inflation,
            Bounds::new(0.0, crate::economy::MAX_BURST_INFLATION),
        );
    }

    fn assumptions(&mut self) {
        let a = &self.p.assumptions;
        if let MarketKind::Bootstrap {
            mean_block_months,
            from_year,
            ..
        } = a.market.kind
        {
            if !(1..=600).contains(&mean_block_months) {
                self.r.error(
                    "out_of_range",
                    "/assumptions/market/kind/mean_block_months",
                    "mean_block_months must be between 1 and 600.",
                );
            }
            if from_year.is_some_and(|y| !(1800..=2020).contains(&y)) {
                self.r.error("out_of_range", "/assumptions/market/kind/from_year", "from_year must be between 1800 and 2020 so that enough history remains to resample.");
            } else if let Some(year) = from_year {
                if let Some(message) = crate::economy::short_history_warning(year) {
                    self.r.warn(
                        "short_history",
                        "/assumptions/market/kind/from_year",
                        message,
                    );
                }
            }
        }
        self.r.range(
            "/assumptions/market/equity_premium_adjustment",
            "Equity premium adjustment",
            a.market.equity_premium_adjustment,
            EQUITY_ADJUSTMENT_RANGE.0,
            EQUITY_ADJUSTMENT_RANGE.1,
        );
        self.r.range(
            "/assumptions/market/inflation_adjustment",
            "Inflation adjustment",
            a.market.inflation_adjustment,
            INFLATION_ADJUSTMENT_RANGE.0,
            INFLATION_ADJUSTMENT_RANGE.1,
        );
        self.market_risk();
        for (i, s) in a.market.stress.iter().enumerate() {
            let sp = format!("/assumptions/market/stress/{i}");
            match s {
                Stress::CrashAtRetirement {
                    drawdown,
                    recovery_years,
                } => {
                    self.r
                        .range(&format!("{sp}/drawdown"), "Drawdown", *drawdown, 0.0, 0.95);
                    self.r.range(
                        &format!("{sp}/recovery_years"),
                        "Recovery",
                        *recovery_years,
                        0.0,
                        40.0,
                    );
                }
                Stress::LostDecade { start } => self.when(&format!("{sp}/start"), start),
                Stress::Stagflation {
                    start,
                    years,
                    inflation,
                } => {
                    self.when(&format!("{sp}/start"), start);
                    self.r
                        .range(&format!("{sp}/years"), "Duration", *years, 0.0, 40.0);
                    self.r.range(
                        &format!("{sp}/inflation"),
                        "Inflation",
                        *inflation,
                        0.0,
                        0.5,
                    );
                }
                Stress::HistoricalEpisode { start, .. } => self.when(&format!("{sp}/start"), start),
                Stress::LostGeneration { start, years } => {
                    self.when(&format!("{sp}/start"), start);
                    market_realism::lost_generation_years(&sp, *years, &mut self.r);
                }
            }
        }
        market_realism::check(&a.market, &mut self.r);
        if let TaxKind::Flat { rate } = a.taxes.kind {
            self.r.range(
                "/assumptions/taxes/kind/rate",
                "Flat tax rate",
                rate,
                0.0,
                0.9,
            );
        }
        if let Some(h) = a.taxes.social_security_haircut {
            self.r.range(
                "/assumptions/taxes/social_security_haircut/factor",
                "Benefit factor",
                h.factor,
                0.0,
                1.0,
            );
            if !(2000..=2200).contains(&h.from_year) {
                self.r.error(
                    "out_of_range",
                    "/assumptions/taxes/social_security_haircut/from_year",
                    "from_year must be between 2000 and 2200.",
                );
            }
        }
        if let Longevity::FixedAge { age } = a.longevity {
            self.r.range(
                "/assumptions/longevity/age",
                "Age at death",
                age,
                50.0,
                120.0,
            );
        }
    }

    // ---- plausibility lints -------------------------------------------------------

    fn plausibility(&mut self) {
        let p = self.p;
        let budget = p.annual_budget();
        let retirement_budget = p.annual_budget_in_retirement();
        let income = current_gross_income(p);
        let liquid: f64 = p.accounts.iter().map(|a| a.balance).sum();
        let cash: f64 = p
            .accounts
            .iter()
            .filter(|a| a.kind == AccountKind::Cash)
            .map(|a| a.balance)
            .sum();
        let retired = matches!(p.retirement.trigger, RetirementTrigger::AlreadyRetired);

        if p.spending.items.is_empty() {
            self.r.warn("no_spending", "/spending/items", "The budget is empty, so the plan assumes the household spends nothing. Add budget items.");
        }
        if p.incomes.is_empty() && liquid <= 0.0 {
            self.r.warn(
                "no_income_no_assets",
                "/incomes",
                "There is no income and no savings, so there is nothing to simulate.",
            );
        }
        if budget > 0.0 && income > 0.0 && budget > income && liquid < budget && !retired {
            self.r.warn("spending_exceeds_income", "/spending/items", format!("The budget ({budget:.0} a year) is above gross income ({income:.0}) and savings cover less than a year of it. Check for a monthly amount entered as annual or the reverse."));
        }
        if budget > 0.0 && income > 0.0 && budget < 0.2 * income && !retired {
            self.r.warn("budget_looks_incomplete", "/spending/items", format!("The budget ({budget:.0} a year) is under 20% of gross income ({income:.0}). That is an unusually high savings rate; check that the budget is complete."));
        }
        if budget > 0.0 && retirement_budget < 0.5 * budget {
            self.r.warn("retirement_spending_low", "/spending/items", format!("Spending in retirement ({retirement_budget:.0} a year) is less than half of spending today ({budget:.0}). Most households spend 70% to 100% of their pre-retirement budget."));
        }
        let has_health_items = p
            .spending
            .items
            .iter()
            .any(|i| i.category == Category::Healthcare);
        match p.spending.healthcare.mode {
            HealthcareMode::InBudget if !has_health_items && !p.spending.items.is_empty() => {
                self.r.warn("no_healthcare_budget", "/spending/healthcare/mode", "Healthcare is set to \"in_budget\" but no budget item has the healthcare category, so the plan has no medical costs. Add one or switch the mode to \"modeled\".");
            }
            HealthcareMode::Modeled if has_health_items => {
                self.r.warn("healthcare_double_count", "/spending/healthcare/mode", "Healthcare is modelled by the engine and there are also healthcare budget items, so it is counted twice. Remove the items or switch the mode to \"in_budget\".");
            }
            _ => {}
        }
        let has_childcare_items = p
            .spending
            .items
            .iter()
            .any(|i| i.category == Category::Childcare);
        if has_childcare_items
            && p.household
                .children
                .iter()
                .any(|c| c.costs == ChildCosts::Modeled)
        {
            self.r.warn("child_costs_double_count", "/household/children", "Child costs are modelled by the engine and the budget also has childcare items, so they may be counted twice. Set the children's costs to \"in_budget\" or remove the items.");
        }

        if let Some(w) = p.portfolio.target.normalized() {
            let crypto = w[AssetClass::Crypto.index()];
            if crypto > 0.20 {
                self.r.warn("crypto_heavy", "/portfolio/target/crypto", format!("{:.0}% of the portfolio is in crypto. It is modelled with extreme volatility, so results will swing widely.", crypto * 100.0));
            }
            let single = w[AssetClass::SingleStock.index()];
            if single > 0.20 {
                self.r.warn("single_stock_heavy", "/portfolio/target/single_stock", format!("{:.0}% of the portfolio is in a single stock. Concentration adds risk without adding expected return.", single * 100.0));
            }
            let growth = p.portfolio.target.growth_share();
            let oldest = p.start.and_then(|s| {
                p.household
                    .people
                    .iter()
                    .map(|x| x.birth.age_years_at(s))
                    .reduce(f64::max)
            });
            if growth > 0.95
                && matches!(p.portfolio.glide, GlidePath::None)
                && oldest.is_some_and(|a| a >= 55.0)
            {
                self.r.warn("no_defensive_assets", "/portfolio/target", "The portfolio is almost entirely growth assets with no glide path, close to or in retirement. A crash early in retirement does the most damage; consider bonds or a glide path.");
            }
            if w[AssetClass::Cash.index()] > 0.4 {
                self.r.warn("cash_heavy", "/portfolio/target/cash", "More than 40% of the investment target is cash, which has historically barely kept up with inflation. The emergency fund is held separately and need not be in the target.");
            }
        }
        if p.portfolio.expense_ratio + p.portfolio.advisory_fee > 0.01 {
            self.r.warn("high_fees", "/portfolio/expense_ratio", format!("Total investment fees are {:.2}% a year. Over decades that can consume a quarter of the portfolio; index funds cost under 0.1%.", (p.portfolio.expense_ratio + p.portfolio.advisory_fee) * 100.0));
        }
        if budget > 0.0 && !retired && (p.portfolio.emergency_months < 1.0 || cash < budget / 12.0)
        {
            self.r.warn("no_emergency_fund", "/portfolio/emergency_months", "There is less than one month of spending in cash. A job loss or surprise bill would force selling investments or borrowing.");
        }

        if let RetirementTrigger::WhenFunded(w) = p.retirement.trigger {
            if w.withdrawal_rate > 0.05 {
                self.r.warn("withdrawal_rate_high", "/retirement/trigger/withdrawal_rate", format!("A {:.1}% withdrawal rate has failed often historically over 30 years or more. 3.5% to 4% is the usual range.", w.withdrawal_rate * 100.0));
            }
        }
        for (i, person) in p.household.people.iter().enumerate() {
            let earns = p.incomes.iter().any(|inc| {
                inc.owner == i
                    && matches!(inc.kind, IncomeKind::Job(_) | IncomeKind::SelfEmployment(_))
            });
            if matches!(
                person.social_security.basis,
                SocialSecurityBasis::FromEarnings { .. }
            ) && !earns
            {
                self.r.warn(
                    "social_security_no_earnings",
                    &format!("/household/people/{}/social_security/basis", seg(&person.id, i)),
                    "Social Security is estimated from earnings, but this person has no job or self-employment income in the plan, so the estimate is zero. Enter the benefit from their SSA statement (known_benefit) instead.",
                );
            }
        }
        if let SpendingRule::PercentOfPortfolio { rate } = p.retirement.spending_rule {
            if rate > 0.06 {
                self.r.warn("withdrawal_rate_high", "/retirement/spending_rule/rate", "Spending more than 6% of the portfolio each year shrinks it quickly in real terms.");
            }
        }
        if retired
            && p.incomes.iter().any(|i| {
                matches!(&i.kind, IncomeKind::Job(j) if j.ends_at_retirement) && i.start.is_none()
            })
        {
            self.r.warn("retired_with_jobs", "/retirement/trigger", "The household is marked as already retired, so jobs that end at retirement pay nothing. Remove them or set ends_at_retirement to false.");
        }
        if self.n_people == 1 && p.retirement.partner_offset_years != 0.0 {
            self.r.warn(
                "partner_offset_unused",
                "/retirement/partner_offset_years",
                "partner_offset_years has no effect in a one-person household.",
            );
        }

        for (i, inc) in p.incomes.iter().enumerate() {
            let k = format!("/incomes/{}/kind", seg(&inc.id, i));
            if let IncomeKind::Job(job) = &inc.kind {
                if job.salary > 0.0 && job.salary < 12_000.0 {
                    self.r.warn("salary_looks_monthly", &format!("{k}/salary"), format!("A salary of {:.0} a year is very low. Salaries are annual amounts; multiply a monthly figure by 12.", job.salary));
                }
                if job.equity.len() > crate::career::MAX_GRANTS {
                    self.r.warn("too_many_grants", &format!("{k}/equity"), format!("Only the first {} equity grants of a job are simulated; combine the smaller ones.", crate::career::MAX_GRANTS));
                }
                if job.equity.windows(2).any(|w| w[0].company != w[1].company) {
                    self.r.warn("mixed_grant_companies", &format!("{k}/equity"), "All grants of one job follow the company settings of its first grant. Put grants from another employer on their own income.");
                }
                if job.salary > 2_000_000.0 {
                    self.r.warn(
                        "salary_very_high",
                        &format!("{k}/salary"),
                        "A base salary above 2,000,000 is unusual; check for extra digits.",
                    );
                }
                if let Some(plan) = &job.plan {
                    if plan.match_rate > 0.0
                        && !plan.employee_max
                        && plan.employee_fraction + 1e-9 < plan.match_up_to
                    {
                        self.r.warn("match_not_captured", &format!("{k}/plan/employee_fraction"), format!("Contributing {:.0}% of salary leaves part of the employer match unclaimed; the match applies up to {:.0}%.", plan.employee_fraction * 100.0, plan.match_up_to * 100.0));
                    }
                }
                let equity: f64 = job.equity.iter().map(|g| g.unvested_value).sum();
                if equity > 0.0
                    && liquid > 0.0
                    && equity > 2.0 * liquid
                    && job
                        .equity
                        .iter()
                        .any(|g| matches!(g.company, EquityCompany::Private { .. }))
                {
                    self.r.warn("private_equity_dominant", &format!("{k}/equity"), "Unvested private-company equity is more than twice the household's savings. Its outcome is highly uncertain and often zero.");
                }
            }
        }
        for (i, person) in p.household.people.iter().enumerate() {
            if let SocialSecurityBasis::KnownBenefit { monthly_at_fra } =
                person.social_security.basis
            {
                if monthly_at_fra > 4_500.0 {
                    self.r.warn("social_security_high", &format!("/household/people/{}/social_security/basis/monthly_at_fra", seg(&person.id, i)), "A benefit above 4,500 a month at full retirement age exceeds the maximum Social Security pays. Enter the monthly figure for one person.");
                }
            }
        }

        for (i, d) in p.debts.iter().enumerate() {
            let path = format!("/debts/{}", seg(&d.id, i));
            if d.balance > 0.0
                && d.rate >= 0.10
                && p.debt_policy.strategy == DebtStrategy::Minimum
                && p.debt_policy.extra_monthly == 0.0
            {
                self.r.warn("high_interest_debt", &path, format!("This debt costs {:.0}% a year and only minimum payments are planned. Paying it off first beats any investment return; set debt_policy.strategy to \"avalanche\".", d.rate * 100.0));
            }
            if let Some(pay) = d.payment {
                if d.balance > 0.0 && pay <= d.balance * d.rate / 12.0 {
                    self.r.warn("payment_below_interest", &format!("{path}/payment"), "The payment does not cover the monthly interest, so the balance will keep growing.");
                }
            }
            if d.balance > 0.0
                && d.payment.is_none()
                && d.remaining_months.is_none()
                && d.kind != DebtKind::CreditCard
            {
                self.r.warn(
                    "debt_schedule_assumed",
                    &path,
                    "No payment or remaining term is given, so a five-year payoff is assumed.",
                );
            }
        }
        for (i, pr) in p.properties.iter().enumerate() {
            let path = format!("/properties/{}", seg(&pr.id, i));
            if let Some(m) = &pr.mortgage {
                if m.balance > pr.value && pr.value > 0.0 {
                    self.r.warn(
                        "mortgage_underwater",
                        &format!("{path}/mortgage/balance"),
                        "The mortgage balance is larger than the property's value.",
                    );
                }
            }
            if let PropertyUse::Rental(t) = &pr.usage {
                if pr.value > 0.0 && t.monthly_rent * 12.0 > 0.25 * pr.value {
                    self.r.warn("rent_yield_implausible", &format!("{path}/use/monthly_rent"), "Annual rent is more than 25% of the property's value; check that the rent is monthly and the value is the full market value.");
                }
                if t.monthly_rent == 0.0 {
                    self.r.warn(
                        "rental_without_rent",
                        &format!("{path}/use/monthly_rent"),
                        "The property is marked as a rental but has no rent.",
                    );
                }
            }
        }
        let has_rent = p
            .spending
            .items
            .iter()
            .any(|i| i.category == Category::Rent && i.amount > 0.0 && i.end.is_none());
        if has_rent
            && p.properties
                .iter()
                .any(|x| matches!(x.usage, PropertyUse::Primary))
        {
            self.r.warn("rent_and_primary_home", "/spending/items", "The budget has rent and the household also owns a primary home. Mortgage, property tax, insurance and maintenance are computed from the property; remove the rent item unless both are really paid.");
        }

        if let Some(start) = p.start {
            for (i, ev) in p.events.iter().enumerate() {
                if let When::Date { at } = ev.when {
                    if at < start {
                        self.r.warn("event_in_past", &format!("/events/{}/when", seg(&ev.id, i)), format!("The event date {at} is before the start of the plan ({start}), so it will never happen. Remove it or move it."));
                    }
                }
            }
        }
        for (i, ev) in p.events.iter().enumerate() {
            if let EventKind::BuyHome(b) = &ev.kind {
                if b.down_payment_fraction < 0.035 {
                    self.r.warn(
                        "down_payment_low",
                        &format!("/events/{}/kind/down_payment_fraction", seg(&ev.id, i)),
                        "A down payment under 3.5% is below what most lenders accept.",
                    );
                }
            }
        }

        let sim = &p.simulation;
        if sim.end_age < 90 {
            self.r.warn("end_age_low", "/simulation/end_age", "Stopping before age 90 hides the risk of a long life: about one in three 65-year-olds lives past 90.");
        }
        if sim.paths < 1_000 {
            self.r.warn("few_paths", "/simulation/paths", "Fewer than 1,000 paths gives noisy results; success rates can be off by several points.");
        }
        if sim.seed > (1u64 << 53) {
            self.r.warn(
                "seed_not_js_safe",
                "/simulation/seed",
                "The seed is above 2^53 and will lose precision when it passes through JavaScript.",
            );
        }
        if let Longevity::FixedAge { age } = p.assumptions.longevity {
            if age < 85.0 {
                self.r.warn("fixed_longevity_low", "/assumptions/longevity/age", "A fixed lifespan under 85 is below the life expectancy of a healthy retiree; the plan may run out of money in the years it ignores.");
            }
        }
        if matches!(p.assumptions.market.kind, MarketKind::Historical)
            && p.assumptions.market.equity_premium_adjustment != 0.0
        {
            self.r.warn("adjusted_history", "/assumptions/market/equity_premium_adjustment", "The historical replay is being run with stock returns shifted from the record. Set the adjustment to 0 to replay history as it happened.");
        }
        if matches!(p.assumptions.market.kind, MarketKind::Historical) {
            let m = &p.assumptions.market;
            for (on, field) in [
                (
                    m.equity_premium_uncertainty.is_some(),
                    "equity_premium_uncertainty",
                ),
                (m.inflation_uncertainty.is_some(), "inflation_uncertainty"),
                (
                    m.disasters.annual_probability > 0.0,
                    "disasters/annual_probability",
                ),
            ] {
                if on {
                    self.r.warn("adjusted_history", &format!("/assumptions/market/{field}"), "The historical replay is being run with random changes laid over the record, so its paths are no longer history as it happened. Switch this off to replay history exactly.");
                }
            }
        }
        if p.assumptions.market.equity_premium_adjustment > 0.02 {
            self.r.warn("returns_optimistic", "/assumptions/market/equity_premium_adjustment", "Adding more than two points a year to historical stock returns is very optimistic.");
        }
        if p.household.location.is_us() && p.household.location.state.is_none() {
            self.r.warn("state_missing", "/household/location/state", "No US state is set, so state income tax is not included and property tax uses the national average.");
        }
        if !p.household.location.is_us() && p.household.location.foreign_tax_rate.is_none() {
            self.r.warn("foreign_tax_missing", "/household/location/foreign_tax_rate", "The household lives outside the US and no foreign_tax_rate is set, so no income tax is applied.");
        }
        self.equity_plausibility(liquid, budget);
    }
}

fn display_name<'a>(name: &'a str, id: &'a str) -> &'a str {
    if name.is_empty() {
        id
    } else {
        name
    }
}

/// Gross annual income being received today: incomes with no start date (pensions and
/// annuities included), bonuses and commissions at target.
fn current_gross_income(p: &Profile) -> f64 {
    let mut total = 0.0;
    for inc in p.incomes.iter().filter(|i| i.start.is_none()) {
        total += match &inc.kind {
            IncomeKind::Job(j) => {
                j.salary * (1.0 + j.bonus.map_or(0.0, |b| b.target_fraction))
                    + j.commission.map_or(0.0, |c| c.expected_annual)
            }
            IncomeKind::SelfEmployment(s) => s.net_annual,
            IncomeKind::Pension(x) => x.annual,
            IncomeKind::Annuity(x) => x.annual,
            IncomeKind::Other(x) => x.annual,
        };
    }
    for pr in &p.properties {
        if let PropertyUse::Rental(t) = &pr.usage {
            total += t.monthly_rent * 12.0 * (1.0 - t.vacancy);
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::{apply_to_profile, PatchOp};
    use serde_json::json;

    fn base() -> Profile {
        Profile::example()
    }

    fn patched(ops: &[PatchOp]) -> Report {
        validate(&apply_to_profile(&base(), ops).unwrap())
    }

    fn has(issues: &[Issue], code: &str, path: &str) -> bool {
        issues.iter().any(|i| i.code == code && i.path == path)
    }

    #[track_caller]
    fn assert_error(ops: &[PatchOp], code: &str, path: &str) {
        let r = patched(ops);
        assert!(
            has(&r.errors, code, path),
            "expected error {code} at {path}, got {:#?}",
            r.errors
        );
    }

    #[track_caller]
    fn assert_warning(ops: &[PatchOp], code: &str, path: &str) {
        let r = patched(ops);
        assert!(r.errors.is_empty(), "unexpected errors {:#?}", r.errors);
        assert!(
            has(&r.warnings, code, path),
            "expected warning {code} at {path}, got {:#?}",
            r.warnings
        );
    }

    #[test]
    fn demo_profiles_are_clean() {
        for ex in Profile::examples(crate::model::EXAMPLE_START) {
            let r = validate(&ex.profile);
            assert!(r.errors.is_empty(), "{}: {:#?}", ex.slug, r.errors);
            // The startup employee's option position is many times her savings:
            // the one warning a demo is meant to show.
            let expected: &[&str] = if ex.slug == "startup-employee" {
                &["equity_dominant"]
            } else {
                &[]
            };
            let codes: Vec<&str> = r.warnings.iter().map(|w| w.code.as_str()).collect();
            assert_eq!(codes, expected, "{}: {:#?}", ex.slug, r.warnings);
        }
    }

    #[test]
    fn empty_profile_reports_people_count() {
        let r = validate(&Profile::default());
        assert!(has(&r.errors, "people_count", "/household/people"));
        assert!(!r.is_ok());
    }

    #[test]
    fn three_people_is_an_error() {
        let extra = json!({"id": "third", "birth": "1990-01"});
        assert_error(
            &[PatchOp::append("/household/people", extra)],
            "people_count",
            "/household/people",
        );
    }

    #[test]
    fn ids_must_be_present_unique_and_well_formed() {
        assert_error(
            &[PatchOp::set("/incomes/1/id", "priya_job")],
            "id_duplicate",
            "/incomes/1/id",
        );
        assert_error(
            &[PatchOp::set("/accounts/0/id", "")],
            "id_missing",
            "/accounts/0/id",
        );
        assert_error(
            &[PatchOp::set("/debts/0/id", "car/loan")],
            "id_invalid",
            "/debts/0/id",
        );
        assert_error(
            &[PatchOp::append(
                "/properties",
                json!({"id": "home", "value": 1.0}),
            )],
            "id_duplicate",
            "/events/@buy_home/kind/property_id",
        );
    }

    #[test]
    fn references_must_resolve() {
        assert_error(
            &[PatchOp::set("/incomes/@daniel_job/owner", 2)],
            "ref_person",
            "/incomes/@daniel_job/owner",
        );
        assert_error(
            &[PatchOp::set("/accounts/@hsa/owner", 5)],
            "ref_person",
            "/accounts/@hsa/owner",
        );
        assert_error(
            &[PatchOp::set(
                "/events/@inheritance/when",
                json!({"type": "age", "person": 3, "age": 60}),
            )],
            "ref_person",
            "/events/@inheritance/when/person",
        );
        let ev =
            |kind: serde_json::Value| PatchOp::append("/events", json!({"id": "x", "kind": kind}));
        assert_error(
            &[ev(
                json!({"type": "income_change", "income": "nope", "change": {"type": "end"}}),
            )],
            "ref_income",
            "/events/@x/kind/income",
        );
        assert_error(
            &[ev(json!({"type": "sell_property", "property": "cabin"}))],
            "ref_property",
            "/events/@x/kind/property",
        );
        assert_error(
            &[ev(
                json!({"type": "spending_change", "target": {"type": "item", "id": "yacht"}, "change": {"type": "scale", "factor": 2}}),
            )],
            "ref_item",
            "/events/@x/kind/target/id",
        );
        // Selling a home bought by an earlier event resolves.
        let r = patched(&[ev(json!({"type": "sell_property", "property": "home"}))]);
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        let r = validate(
            &apply_to_profile(
                &base(),
                &[ev(json!({"type": "sell_property", "property": "cabin"}))],
            )
            .unwrap(),
        );
        assert!(
            r.errors[0].message.contains("home"),
            "{}",
            r.errors[0].message
        );
    }

    #[test]
    fn allocation_errors() {
        assert_error(
            &[PatchOp::set("/portfolio/target", json!({}))],
            "allocation_empty",
            "/portfolio/target",
        );
        assert_error(
            &[PatchOp::set(
                "/portfolio/target",
                json!({"us_stocks": 110, "bonds": -10}),
            )],
            "allocation_negative",
            "/portfolio/target/bonds",
        );
        assert_error(
            &[PatchOp::set(
                "/portfolio/target",
                json!({"us_stocks": 90, "custom": 10}),
            )],
            "custom_asset_missing",
            "/portfolio/custom_asset",
        );
    }

    #[test]
    fn rates_entered_as_percents_are_rejected_with_a_hint() {
        let r = patched(&[PatchOp::set("/debts/@minivan/rate", 6.5)]);
        let issue = r
            .errors
            .iter()
            .find(|i| i.path == "/debts/@minivan/rate")
            .unwrap();
        assert_eq!(issue.code, "out_of_range");
        assert!(issue.message.contains("0.05 for 5%"), "{}", issue.message);
        assert_error(
            &[PatchOp::set("/retirement/trigger/withdrawal_rate", 4)],
            "out_of_range",
            "/retirement/trigger/withdrawal_rate",
        );
        assert_error(
            &[PatchOp::set(
                "/household/people/@priya/social_security/claim_age",
                59,
            )],
            "out_of_range",
            "/household/people/@priya/social_security/claim_age",
        );
        assert_error(
            &[PatchOp::set("/events/@kitchen/probability", 1.5)],
            "out_of_range",
            "/events/@kitchen/probability",
        );
        assert_error(
            &[PatchOp::set("/accounts/@hsa/balance", -5)],
            "negative_amount",
            "/accounts/@hsa/balance",
        );
    }

    #[test]
    fn dates_must_be_sane() {
        assert_error(
            &[PatchOp::set("/household/people/@priya/birth", "2020-01")],
            "age_range",
            "/household/people/@priya/birth",
        );
        assert_error(
            &[PatchOp::set("/simulation/end_age", 30)],
            "out_of_range",
            "/simulation/end_age",
        );
        assert_error(
            &[PatchOp::set("/household/location/state", "Texas")],
            "state_code",
            "/household/location/state",
        );
        assert_error(
            &[PatchOp::set("/household/location/country", "Portugal")],
            "country_code",
            "/household/location/country",
        );
        assert_error(
            &[
                PatchOp::set(
                    "/spending/items/@rent/start",
                    json!({"type": "date", "at": "2030-01"}),
                ),
                PatchOp::set(
                    "/spending/items/@rent/end",
                    json!({"type": "date", "at": "2029-01"}),
                ),
            ],
            "window_empty",
            "/spending/items/@rent/end",
        );
        assert_error(
            &[PatchOp::set("/household/people/@priya/joins", "2030-01")],
            "joins_first_person",
            "/household/people/@priya/joins",
        );
    }

    #[test]
    fn date_checks_are_skipped_without_a_start() {
        let mut p = base();
        p.start = None;
        p.household.people[0].birth = YearMonth::ym(1900, 1);
        assert!(validate(&p).errors.is_empty());
    }

    #[test]
    fn plausibility_warnings() {
        assert_warning(
            &[PatchOp::set(
                "/portfolio/target",
                json!({"us_stocks": 70, "crypto": 30}),
            )],
            "crypto_heavy",
            "/portfolio/target/crypto",
        );
        assert_warning(
            &[PatchOp::set(
                "/portfolio/target",
                json!({"us_stocks": 50, "bonds": 20, "single_stock": 30}),
            )],
            "single_stock_heavy",
            "/portfolio/target/single_stock",
        );
        assert_warning(
            &[PatchOp::set("/spending/healthcare/mode", "in_budget")],
            "no_healthcare_budget",
            "/spending/healthcare/mode",
        );
        assert_warning(
            &[PatchOp::append(
                "/spending/items",
                json!({"id": "meds", "category": "healthcare", "amount": 100}),
            )],
            "healthcare_double_count",
            "/spending/healthcare/mode",
        );
        assert_warning(
            &[PatchOp::append(
                "/spending/items",
                json!({"id": "daycare", "category": "childcare", "amount": 100}),
            )],
            "child_costs_double_count",
            "/household/children",
        );
        assert_warning(
            &[PatchOp::set("/retirement/trigger/withdrawal_rate", 0.06)],
            "withdrawal_rate_high",
            "/retirement/trigger/withdrawal_rate",
        );
        assert_warning(
            &[PatchOp::set("/portfolio/advisory_fee", 0.0125)],
            "high_fees",
            "/portfolio/expense_ratio",
        );
        assert_warning(
            &[PatchOp::set("/portfolio/emergency_months", 0)],
            "no_emergency_fund",
            "/portfolio/emergency_months",
        );
        assert_warning(
            &[PatchOp::set("/incomes/@priya_job/kind/salary", 9000)],
            "salary_looks_monthly",
            "/incomes/@priya_job/kind/salary",
        );
        assert_warning(
            &[PatchOp::set(
                "/incomes/@priya_job/kind/plan/employee_fraction",
                0.03,
            )],
            "match_not_captured",
            "/incomes/@priya_job/kind/plan/employee_fraction",
        );
        assert_warning(
            &[PatchOp::set("/debts/@minivan/rate", 0.24)],
            "high_interest_debt",
            "/debts/@minivan",
        );
        assert_warning(
            &[PatchOp::set("/debts/@minivan/payment", 50)],
            "payment_below_interest",
            "/debts/@minivan/payment",
        );
        assert_warning(
            &[PatchOp::set("/simulation/end_age", 85)],
            "end_age_low",
            "/simulation/end_age",
        );
        assert_warning(
            &[PatchOp::set("/simulation/paths", 200)],
            "few_paths",
            "/simulation/paths",
        );
        assert_warning(
            &[PatchOp::set(
                "/assumptions/longevity",
                json!({"type": "fixed_age", "age": 80}),
            )],
            "fixed_longevity_low",
            "/assumptions/longevity/age",
        );
        assert_warning(
            &[PatchOp::set(
                "/events/@buy_home/when",
                json!({"type": "date", "at": "2020-01"}),
            )],
            "event_in_past",
            "/events/@buy_home/when",
        );
        assert_warning(
            &[PatchOp::set("/household/location/state", json!(null))],
            "state_missing",
            "/household/location/state",
        );
        assert_warning(
            &[PatchOp::set(
                "/retirement/trigger",
                json!({"type": "already_retired"}),
            )],
            "retired_with_jobs",
            "/retirement/trigger",
        );
        assert_warning(
            &[PatchOp::append(
                "/properties",
                json!({"id": "condo", "value": 300000, "mortgage": {"balance": 350000}}),
            )],
            "mortgage_underwater",
            "/properties/@condo/mortgage/balance",
        );
        assert_warning(
            &[PatchOp::append(
                "/properties",
                json!({"id": "condo", "value": 300000}),
            )],
            "rent_and_primary_home",
            "/spending/items",
        );
    }

    #[test]
    fn budget_level_warnings() {
        let scale = |f: f64| -> Vec<PatchOp> {
            base()
                .spending
                .items
                .iter()
                .map(|i| PatchOp::set(format!("/spending/items/@{}/amount", i.id), i.amount * f))
                .collect()
        };
        assert_warning(&scale(0.2), "budget_looks_incomplete", "/spending/items");
        let mut ops = scale(6.0);
        ops.extend(
            base()
                .accounts
                .iter()
                .map(|a| PatchOp::set(format!("/accounts/@{}/balance", a.id), 1000)),
        );
        assert_warning(&ops, "spending_exceeds_income", "/spending/items");
        let ops: Vec<PatchOp> = base()
            .spending
            .items
            .iter()
            .map(|i| {
                PatchOp::set(
                    format!("/spending/items/@{}/in_retirement", i.id),
                    json!({"type": "factor", "factor": 0.3}),
                )
            })
            .collect();
        assert_warning(&ops, "retirement_spending_low", "/spending/items");
        assert_warning(
            &[PatchOp::set("/spending/items", json!([]))],
            "no_spending",
            "/spending/items",
        );
    }

    #[test]
    fn report_serialises_for_the_ui() {
        let r = validate(&Profile::default());
        let v = serde_json::to_value(&r).unwrap();
        assert!(v["errors"][0]["code"].is_string());
        assert!(v["warnings"].is_array());
    }

    #[test]
    fn market_risk_options_are_checked() {
        use serde_json::json;
        let base = "/assumptions/market";
        // Sensible settings pass.
        let r = patched(&[
            PatchOp::set(
                format!("{base}/equity_premium_uncertainty"),
                json!({"type": "normal", "mean": 0.0, "sd": 0.01}),
            ),
            PatchOp::set(
                format!("{base}/inflation_uncertainty"),
                json!({"type": "triangular", "low": -0.005, "mode": 0.0, "high": 0.015}),
            ),
            PatchOp::set(format!("{base}/disasters/annual_probability"), json!(0.01)),
            PatchOp::set(format!("{base}/kind/joins"), json!("matched")),
        ]);
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        assert!(r.warnings.is_empty(), "{:#?}", r.warnings);
        // A spread that reaches beyond 20 points a year (4 sd).
        assert_error(
            &[PatchOp::set(
                format!("{base}/equity_premium_uncertainty"),
                json!({"type": "normal", "mean": 0.0, "sd": 0.06}),
            )],
            "out_of_range",
            &format!("{base}/equity_premium_uncertainty/sd"),
        );
        assert_error(
            &[PatchOp::set(
                format!("{base}/inflation_uncertainty"),
                json!({"type": "uniform", "low": 0.02, "high": -0.02}),
            )],
            "dist_order",
            &format!("{base}/inflation_uncertainty/high"),
        );
        // The adjustment plus the furthest shift must itself be a valid adjustment:
        // each range alone is inside its own limits (20 and 10 points), but lasts a
        // lifetime on top of the adjustment. Default stock adjustment -0.015 and a
        // normal with sd 0.025 reaches -0.015 - 4 x 0.025 = -0.115, below -0.1.
        assert_error(
            &[PatchOp::set(
                format!("{base}/equity_premium_uncertainty"),
                json!({"type": "normal", "mean": 0.0, "sd": 0.025}),
            )],
            "out_of_range",
            &format!("{base}/equity_premium_uncertainty/sd"),
        );
        // sd 0.02 reaches -0.095 and +0.065: accepted.
        let r = patched(&[PatchOp::set(
            format!("{base}/equity_premium_uncertainty"),
            json!({"type": "normal", "mean": 0.0, "sd": 0.02}),
        )]);
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        // Only the spread counts, not where the range is centred: 0.30 to 0.40 is a
        // shift of -0.05 to +0.05, which a zero adjustment has room for; but at the
        // highest adjustment (0.1) the upward half does not fit.
        let centred_high = json!({"type": "uniform", "low": 0.0, "high": 0.1});
        let r = patched(&[
            PatchOp::set(format!("{base}/equity_premium_adjustment"), json!(0.0)),
            PatchOp::set(
                format!("{base}/equity_premium_uncertainty"),
                centred_high.clone(),
            ),
        ]);
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        assert_error(
            &[
                PatchOp::set(format!("{base}/equity_premium_adjustment"), json!(0.1)),
                PatchOp::set(format!("{base}/equity_premium_uncertainty"), centred_high),
            ],
            "out_of_range",
            &format!("{base}/equity_premium_uncertainty/high"),
        );
        // Inflation: a uniform -0.1..0.1 fits an adjustment of 0.05 exactly
        // (-0.05..0.15) and no other; at the default 0 the low end reaches -0.1.
        let widest = json!({"type": "uniform", "low": -0.1, "high": 0.1});
        let r = patched(&[
            PatchOp::set(format!("{base}/inflation_adjustment"), json!(0.05)),
            PatchOp::set(format!("{base}/inflation_uncertainty"), widest.clone()),
        ]);
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        assert_error(
            &[PatchOp::set(
                format!("{base}/inflation_uncertainty"),
                widest.clone(),
            )],
            "out_of_range",
            &format!("{base}/inflation_uncertainty/low"),
        );
        // The reviewer's corner: the highest adjustments with the widest ranges.
        let r = patched(&[
            PatchOp::set(format!("{base}/equity_premium_adjustment"), json!(0.1)),
            PatchOp::set(
                format!("{base}/equity_premium_uncertainty"),
                json!({"type": "uniform", "low": -0.2, "high": 0.2}),
            ),
            PatchOp::set(format!("{base}/inflation_adjustment"), json!(0.15)),
            PatchOp::set(format!("{base}/inflation_uncertainty"), widest),
        ]);
        // The upward halves do not fit (0.1 + 0.2 and 0.15 + 0.1); the downward
        // halves do (0.1 - 0.2 = -0.1 and 0.15 - 0.1 = 0.05).
        for (at, rejected) in [
            ("equity_premium_uncertainty/high", true),
            ("inflation_uncertainty/high", true),
            ("equity_premium_uncertainty/low", false),
            ("inflation_uncertainty/low", false),
        ] {
            assert_eq!(
                has(&r.errors, "out_of_range", &format!("{base}/{at}")),
                rejected,
                "{at}: {:#?}",
                r.errors
            );
        }
        assert!(r.errors[0].message.contains("takes it to 0.3 a year"));
        for (field, value) in [
            ("annual_probability", json!(0.2)),
            ("annual_probability", json!(-0.01)),
            ("inflation_probability", json!(1.5)),
        ] {
            let path = format!("{base}/disasters/{field}");
            assert_error(&[PatchOp::set(&path, value)], "out_of_range", &path);
        }
        // A depth of 100%, a disaster shorter than a year, a burst above 25 points.
        for (field, value, at) in [
            (
                "depth",
                json!({"type": "pert", "low": 0.2, "mode": 0.5, "high": 1.0}),
                "high",
            ),
            (
                "years",
                json!({"type": "uniform", "low": 0.5, "high": 5.0}),
                "low",
            ),
            (
                "years",
                json!({"type": "uniform", "low": 5.0, "high": 60.0}),
                "high",
            ),
            (
                "inflation",
                json!({"type": "uniform", "low": 0.0, "high": 0.4}),
                "high",
            ),
        ] {
            let path = format!("{base}/disasters/{field}");
            assert_error(
                &[PatchOp::set(&path, value)],
                "out_of_range",
                &format!("{path}/{at}"),
            );
        }
        // Random overlays on the historical replay get the adjusted-history warning.
        let r = patched(&[
            PatchOp::set(format!("{base}/kind"), json!({"type": "historical"})),
            PatchOp::set(format!("{base}/equity_premium_adjustment"), json!(0.0)),
            PatchOp::set(format!("{base}/disasters/annual_probability"), json!(0.01)),
        ]);
        assert!(has(
            &r.warnings,
            "adjusted_history",
            "/assumptions/market/disasters/annual_probability"
        ));
    }

    /// The opt-in business and employer-stock options: every number is checked at
    /// the path an editor control binds to, and the defaults are clean.
    #[test]
    fn business_and_employer_stock_options_are_checked() {
        let business = |se: serde_json::Value| {
            PatchOp::append("/incomes", json!({"id": "shop", "owner": 0, "kind": se}))
        };
        let k = "/incomes/@shop/kind";
        let hazard = |extra: serde_json::Value| {
            let mut failure = json!({"type": "hazard"});
            for (key, value) in extra.as_object().unwrap() {
                failure[key] = value.clone();
            }
            business(json!({"type": "self_employment", "net_annual": 50_000.0, "failure": failure}))
        };
        // Everything at its default, switched on, is valid.
        let r = patched(&[
            hazard(json!({})),
            PatchOp::set(format!("{k}/monthly_lumpiness"), json!(1.0)),
        ]);
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        let r = patched(&[hazard(
            json!({"then": {"type": "stops"}, "years_in_business": 2.0}),
        )]);
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        for (extra, at) in [
            (
                json!({"young_annual_probability": 0.6}),
                "young_annual_probability",
            ),
            (
                json!({"mature_annual_probability": -0.01}),
                "mature_annual_probability",
            ),
            (json!({"young_years": 31.0}), "young_years"),
            (json!({"years_in_business": -1.0}), "years_in_business"),
            (json!({"recession_multiplier": 0.5}), "recession_multiplier"),
            (
                json!({"recession_multiplier": 11.0}),
                "recession_multiplier",
            ),
            (
                json!({"then": {"type": "restart", "gap_months": 121}}),
                "then/gap_months",
            ),
            (
                json!({"then": {"type": "restart", "income_fraction": 2.5}}),
                "then/income_fraction",
            ),
        ] {
            assert_error(
                &[hazard(extra)],
                "out_of_range",
                &format!("{k}/failure/{at}"),
            );
        }
        for bad in [-0.1, 2.5] {
            assert_error(
                &[business(
                    json!({"type": "self_employment", "monthly_lumpiness": bad}),
                )],
                "out_of_range",
                &format!("{k}/monthly_lumpiness"),
            );
        }

        let job = |company: serde_json::Value| {
            PatchOp::append(
                "/incomes",
                json!({"id": "tech", "owner": 0, "kind": {"type": "job", "salary": 100_000.0,
                    "equity": [{"id": "rsu", "unvested_value": 10_000.0, "company": company}]}}),
            )
        };
        let c = "/incomes/@tech/kind/equity/@rsu/company";
        for company in [
            json!({"type": "public", "tails": "fat", "ruin": {"type": "by_volatility"}, "layoff_link": true}),
            json!({"type": "public", "ruin": {"type": "stated"}}),
            json!({"type": "private", "exit_timing": {"type": "lognormal"}, "exit_size": "pareto_tail"}),
        ] {
            let r = patched(&[job(company)]);
            assert!(r.errors.is_empty(), "{:#?}", r.errors);
        }
        for bad in [-0.01, 0.51] {
            assert_error(
                &[job(
                    json!({"type": "public", "ruin": {"type": "stated", "annual_probability": bad}}),
                )],
                "out_of_range",
                &format!("{c}/ruin/annual_probability"),
            );
        }
        for (timing, at) in [
            (
                json!({"type": "lognormal", "median_years": 0.1}),
                "median_years",
            ),
            (
                json!({"type": "lognormal", "median_years": 41.0}),
                "median_years",
            ),
            (json!({"type": "lognormal", "sigma": 0.01}), "sigma"),
            (json!({"type": "lognormal", "sigma": 2.5}), "sigma"),
        ] {
            assert_error(
                &[job(json!({"type": "private", "exit_timing": timing}))],
                "out_of_range",
                &format!("{c}/exit_timing/{at}"),
            );
        }
    }

    // ---- private-company equity ----

    /// Appends a valid company `acme` for Priya, linked to her job, with one fully
    /// vested option holding `g`. Every key is written out, so patches can reach it.
    fn acme() -> PatchOp {
        let company = PrivateCompany {
            id: "acme".into(),
            name: "Acme".into(),
            job: Some("priya_job".into()),
            preferred_price: 10.0,
            holdings: vec![EquityHolding {
                id: "g".into(),
                kind: HoldingKind::Nso {
                    strike: 2.0,
                    granted_months_ago: 24,
                },
                shares: 100.0,
                vested: 100.0,
                ..EquityHolding::default()
            }],
            ..PrivateCompany::default()
        };
        PatchOp::append("/private_equity", serde_json::to_value(company).unwrap())
    }

    fn acme_with(path: &str, value: serde_json::Value) -> [PatchOp; 2] {
        [
            acme(),
            PatchOp::set(format!("/private_equity/@acme{path}"), value),
        ]
    }

    #[test]
    fn a_plain_company_is_clean() {
        let r = patched(&[acme()]);
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        assert!(r.warnings.is_empty(), "{:#?}", r.warnings);
        // No company, or an empty list written out: nothing new is reported.
        assert_eq!(
            patched(&[PatchOp::set("/private_equity", json!([]))]),
            patched(&[])
        );
    }

    #[test]
    fn company_lists_have_limits_and_ids() {
        let many: Vec<_> = (0..5).map(|i| json!({"id": format!("c{i}")})).collect();
        assert_error(
            &[PatchOp::set("/private_equity", json!(many))],
            "too_many_companies",
            "/private_equity",
        );
        assert_error(
            &[PatchOp::set(
                "/private_equity",
                json!([{"id": "acme"}, {"id": "acme"}]),
            )],
            "id_duplicate",
            "/private_equity/1/id",
        );
        let holdings: Vec<_> = (0..9)
            .map(|i| json!({"id": format!("h{i}"), "kind": {"type": "rsu"}, "shares": 1, "vested": 1}))
            .collect();
        assert_error(
            &acme_with("/holdings", json!(holdings)),
            "too_many_holdings",
            "/private_equity/@acme/holdings",
        );
        assert_error(
            &acme_with("/holdings", json!([{"kind": {"type": "rsu"}}])),
            "id_missing",
            "/private_equity/@acme/holdings/0/id",
        );
        let rows: Vec<_> = (0..9)
            .map(|i| json!({"id": format!("r{i}"), "kind": "fails", "probability": 1.0 / 9.0}))
            .collect();
        assert_error(
            &acme_with("/outcomes", json!(rows)),
            "too_many_outcomes",
            "/private_equity/@acme/outcomes",
        );
        assert_error(
            &acme_with(
                "/outcomes",
                json!([
                    {"id": "a", "kind": "fails", "probability": 0.5},
                    {"id": "a", "kind": "still_private", "probability": 0.5}
                ]),
            ),
            "id_duplicate",
            "/private_equity/@acme/outcomes/1/id",
        );
    }

    #[test]
    fn company_references_must_resolve() {
        assert_error(
            &acme_with("/owner", json!(5)),
            "ref_person",
            "/private_equity/@acme/owner",
        );
        assert_error(
            &acme_with("/job", json!("nope")),
            "ref_income",
            "/private_equity/@acme/job",
        );
        let r = patched(&[
            PatchOp::append(
                "/incomes",
                json!({"id": "royalties", "kind": {"type": "other", "annual": 1000}}),
            ),
            acme(),
            PatchOp::set("/private_equity/@acme/job", "royalties"),
        ]);
        let issue = r
            .errors
            .iter()
            .find(|i| i.code == "ref_income" && i.path == "/private_equity/@acme/job")
            .unwrap_or_else(|| panic!("{:#?}", r.errors));
        assert!(issue.message.contains("is not a job"), "{}", issue.message);
        // Daniel is person 1; the linked job is Priya's.
        let r = patched(&acme_with("/owner", json!(1)));
        let issue = r
            .errors
            .iter()
            .find(|i| i.code == "equity_owner_mismatch")
            .unwrap_or_else(|| panic!("{:#?}", r.errors));
        assert_eq!(issue.path, "/private_equity/@acme/owner");
        assert!(
            issue.message.contains("Set owner to 0"),
            "{}",
            issue.message
        );
    }

    #[test]
    fn company_numbers_must_be_in_range() {
        let c = "/private_equity/@acme";
        for (path, value) in [
            ("/preferred_price", json!(0)),
            ("/preferred_price", json!(2_000_000)),
            ("/fmv_409a", json!(0)),
            ("/preference_overhang", json!(0.99)),
            ("/lockup_months", json!(25)),
            ("/exercise_window_months", json!(121)),
            ("/tenders/annual_probability", json!(1.5)),
            ("/tenders/max_fraction", json!(2)),
            ("/tenders/price_fraction", json!(3)),
            ("/tenders/next_in_months", json!(121)),
            ("/policy/tender_sell_fraction", json!(-0.1)),
            ("/policy/leave_after_months", json!(601)),
        ] {
            assert_error(
                &acme_with(path, value),
                "out_of_range",
                &format!("{c}{path}"),
            );
        }
        for years in [0, 6] {
            assert_error(
                &acme_with(
                    "/policy/sell_down",
                    json!({"type": "over_years", "years": years}),
                ),
                "out_of_range",
                &format!("{c}/policy/sell_down/years"),
            );
        }
        assert_error(
            &acme_with("/amt_credit_carryforward", json!(-1)),
            "negative_amount",
            &format!("{c}/amt_credit_carryforward"),
        );
        // A price is needed only once there is something to price.
        let r = patched(&[PatchOp::append("/private_equity", json!({"id": "empty"}))]);
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        // The limits themselves are accepted.
        let r = patched(&[
            acme(),
            PatchOp::set(format!("{c}/lockup_months"), 24),
            PatchOp::set(format!("{c}/exercise_window_months"), 120),
            PatchOp::set(format!("{c}/preference_overhang"), 0.95),
            PatchOp::set(format!("{c}/tenders/next_in_months"), 120),
            PatchOp::set(format!("{c}/policy/leave_after_months"), 600),
            PatchOp::set(
                format!("{c}/policy/sell_down"),
                json!({"type": "over_years", "years": 5}),
            ),
        ]);
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
    }

    #[test]
    fn holdings_must_be_consistent() {
        let h = "/private_equity/@acme/holdings/@g";
        let holding = |v: serde_json::Value| acme_with("/holdings", json!([v]));
        assert_error(
            &holding(json!({"id": "g", "kind": {"type": "rsu"}, "shares": -1})),
            "out_of_range",
            &format!("{h}/shares"),
        );
        assert_error(
            &holding(json!({"id": "g", "kind": {"type": "rsu"}, "shares": 2e10, "vested": 2e10})),
            "out_of_range",
            &format!("{h}/shares"),
        );
        // 1e10 shares at $1,000 is 1e13.
        assert_error(
            &[
                acme(),
                PatchOp::set("/private_equity/@acme/preferred_price", 1000),
                PatchOp::set(
                    "/private_equity/@acme/holdings",
                    json!([{"id": "g", "kind": {"type": "rsu"}, "shares": 1e10, "vested": 1e10}]),
                ),
            ],
            "equity_position_too_large",
            &format!("{h}/shares"),
        );
        for vested in [-1.0, 101.0] {
            assert_error(
                &holding(
                    json!({"id": "g", "kind": {"type": "rsu"}, "shares": 100, "vested": vested}),
                ),
                "vested_exceeds_shares",
                &format!("{h}/vested"),
            );
        }
        // `vested` is ignored for shares already owned.
        let r = patched(&holding(
            json!({"id": "g", "kind": {"type": "shares", "basis_per_share": 1}, "shares": 100}),
        ));
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        assert_error(
            &holding(
                json!({"id": "g", "kind": {"type": "iso", "strike": -1}, "shares": 1, "vested": 1}),
            ),
            "negative_amount",
            &format!("{h}/kind/strike"),
        );
        assert_error(
            &holding(
                json!({"id": "g", "kind": {"type": "shares", "basis_per_share": -1}, "shares": 1}),
            ),
            "negative_amount",
            &format!("{h}/kind/basis_per_share"),
        );
        assert_error(
            &holding(
                json!({"id": "g", "kind": {"type": "shares", "basis_per_share": 1, "fmv_at_exercise": -2}, "shares": 1}),
            ),
            "negative_amount",
            &format!("{h}/kind/fmv_at_exercise"),
        );
        assert_error(
            &holding(
                json!({"id": "g", "kind": {"type": "rsu"}, "shares": 100, "vest_months_left": 241}),
            ),
            "out_of_range",
            &format!("{h}/vest_months_left"),
        );
        assert_error(
            &holding(json!({"id": "g", "kind": {"type": "rsu"}, "shares": 100, "vested": 40})),
            "unvested_without_schedule",
            &format!("{h}/vest_months_left"),
        );
        assert_error(
            &holding(
                json!({"id": "g", "kind": {"type": "rsu"}, "shares": 100, "vest_months_left": 12, "cliff_months": 13}),
            ),
            "out_of_range",
            &format!("{h}/cliff_months"),
        );
        assert_error(
            &[
                acme(),
                PatchOp::remove("/private_equity/@acme/job"),
                PatchOp::set(
                    "/private_equity/@acme/holdings",
                    json!([{"id": "g", "kind": {"type": "rsu"}, "shares": 100, "vest_months_left": 12}]),
                ),
            ],
            "equity_unvested_without_job",
            &format!("{h}/vested"),
        );
    }

    #[test]
    fn outcome_tables_must_be_consistent() {
        let c = "/private_equity/@acme/outcomes";
        let table = |rows: serde_json::Value| acme_with("/outcomes", rows);
        assert_error(
            &table(json!([{"id": "a", "kind": "fails", "probability": 1.5}])),
            "out_of_range",
            &format!("{c}/@a/probability"),
        );
        assert_error(
            &table(json!([
                {"id": "a", "kind": "fails", "probability": 0.5},
                {"id": "b", "kind": "still_private", "probability": 0.4}
            ])),
            "outcome_probabilities",
            c,
        );
        // A sum within 0.005 of 1 is accepted and normalised by the engine.
        let r = patched(&table(json!([
            {"id": "a", "kind": "fails", "probability": 0.6},
            {"id": "b", "kind": "still_private", "probability": 0.396}
        ])));
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        for (from, to) in [(3.0, 2.0), (-1.0, 2.0), (1.0, 31.0)] {
            assert_error(
                &table(
                    json!([{"id": "a", "kind": "fails", "probability": 1, "years_from": from, "years_to": to}]),
                ),
                "outcome_years",
                &format!("{c}/@a/years_to"),
            );
        }
        // Years are ignored for a "still_private" row.
        let r = patched(&table(json!([
            {"id": "a", "kind": "still_private", "probability": 1, "years_from": 9, "years_to": 2}
        ])));
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        for (low, high) in [(2.0, 1.0), (0.0, 1.0), (1.0, 2000.0)] {
            assert_error(
                &table(
                    json!([{"id": "a", "kind": "ipo", "probability": 1, "multiple_low": low, "multiple_high": high}]),
                ),
                "outcome_multiple",
                &format!("{c}/@a/multiple_high"),
            );
        }
        // Multiples are ignored for a failure.
        let r = patched(&table(json!([
            {"id": "a", "kind": "fails", "probability": 1, "multiple_low": 5, "multiple_high": 0}
        ])));
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
    }

    #[test]
    fn equity_plausibility_warnings() {
        let c = "/private_equity/@acme";
        let holding = |v: serde_json::Value| acme_with("/holdings", json!([v]));
        assert_warning(
            &holding(
                json!({"id": "g", "kind": {"type": "rsu"}, "shares": 10_000_000, "vested": 10_000_000}),
            ),
            "equity_dominant",
            &format!("{c}/holdings"),
        );
        // Not counted: no warning about its size.
        let r = patched(&[
            acme(),
            PatchOp::set(
                format!("{c}/holdings"),
                json!([{"id": "g", "kind": {"type": "rsu"}, "shares": 10_000_000, "vested": 10_000_000}]),
            ),
            PatchOp::set(format!("{c}/included"), false),
        ]);
        assert!(!r.warnings.iter().any(|i| i.code == "equity_dominant"));

        // 400,000 options at $5, fully vested, granted four years ago: 500,000 a year.
        let r = patched(&holding(json!({
            "id": "g", "kind": {"type": "iso", "strike": 5, "granted_months_ago": 48},
            "shares": 400_000, "vested": 400_000
        })));
        let issue = r
            .warnings
            .iter()
            .find(|i| i.code == "iso_over_100k_limit")
            .unwrap_or_else(|| panic!("{:#?}", r));
        assert_eq!(issue.path, format!("{c}/holdings/@g/kind"));
        assert!(
            issue.message.contains("about $500,000 a year"),
            "{}",
            issue.message
        );

        assert_warning(
            &[
                acme(),
                PatchOp::remove(format!("{c}/job")),
                PatchOp::set(format!("{c}/exercise_window_months"), 12),
                PatchOp::set(
                    format!("{c}/holdings"),
                    json!([{"id": "g", "kind": {"type": "iso", "strike": 2}, "shares": 100, "vested": 100}]),
                ),
            ],
            "iso_former_employer",
            &format!("{c}/holdings/@g/kind"),
        );
        assert_warning(
            &[
                PatchOp::set("/household/people/@daniel/joins", "2030-01"),
                acme(),
                PatchOp::set(format!("{c}/job"), "daniel_job"),
                PatchOp::set(format!("{c}/owner"), 1),
            ],
            "equity_owner_joins_later",
            &format!("{c}/owner"),
        );
        assert_warning(
            &holding(
                json!({"id": "g", "kind": {"type": "nso", "strike": 10}, "shares": 100, "vested": 100}),
            ),
            "strike_above_price",
            &format!("{c}/holdings/@g/kind/strike"),
        );
        assert_warning(
            &acme_with("/fmv_409a", json!(11)),
            "fmv_above_preferred",
            &format!("{c}/fmv_409a"),
        );
        assert_warning(
            &[
                PatchOp::set(
                    "/incomes/@priya_job/start",
                    json!({"type": "date", "at": "2030-01"}),
                ),
                acme(),
            ],
            "equity_job_not_started",
            &format!("{c}/job"),
        );
        assert_warning(
            &[
                acme(),
                PatchOp::set(format!("{c}/policy/exercise"), "vested_now"),
                PatchOp::set(
                    format!("{c}/holdings"),
                    json!([{"id": "g", "kind": {"type": "nso", "strike": 9}, "shares": 1_000_000, "vested": 1_000_000}]),
                ),
            ],
            "exercise_unaffordable",
            &format!("{c}/policy/exercise"),
        );
        // The same options with the default policy are never exercised for cash.
        let r = patched(&holding(
            json!({"id": "g", "kind": {"type": "nso", "strike": 9}, "shares": 1_000_000, "vested": 1_000_000}),
        ));
        assert!(!r.warnings.iter().any(|i| i.code == "exercise_unaffordable"));

        let r = patched(&[
            acme(),
            PatchOp::append(
                "/private_equity",
                json!({
                    "id": "bigger", "name": "Bigger Co", "preferred_price": 50,
                    "holdings": [{"id": "s", "kind": {"type": "shares", "basis_per_share": 1}, "shares": 100}]
                }),
            ),
        ]);
        let issue = r
            .warnings
            .iter()
            .find(|i| i.code == "several_companies")
            .unwrap_or_else(|| panic!("{:#?}", r));
        assert_eq!(issue.path, "/private_equity");
        assert!(issue.message.contains("Bigger Co"), "{}", issue.message);

        assert_warning(
            &[PatchOp::append(
                "/incomes/@priya_job/kind/equity",
                json!({"id": "old", "company": {"type": "private"}, "unvested_value": 1000}),
            )],
            "private_grant_legacy",
            "/incomes/@priya_job/kind/equity",
        );
    }

    #[test]
    fn an_optimistic_outcome_table_is_flagged() {
        let c = "/private_equity/@acme/outcomes";
        // Series B fails 36% of the time in the preset: 10% is under half of that.
        let r = patched(&acme_with(
            "/outcomes",
            json!([
                {"id": "fails", "kind": "fails", "probability": 0.10},
                {"id": "sale", "kind": "acquired", "probability": 0.60, "multiple_low": 0.5, "multiple_high": 2},
                {"id": "private", "kind": "still_private", "probability": 0.30}
            ]),
        ));
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        let issue = r
            .warnings
            .iter()
            .find(|i| i.code == "outcomes_optimistic")
            .unwrap_or_else(|| panic!("{:#?}", r.warnings));
        assert_eq!(issue.path, c);
        assert!(
            issue
                .message
                .contains("Failure is 10% in your table against about 36% typically"),
            "{}",
            issue.message
        );
        // Typical failure odds but a very high price: flagged by the implied multiple.
        assert_warning(
            &acme_with(
                "/outcomes",
                json!([
                    {"id": "fails", "kind": "fails", "probability": 0.40},
                    {"id": "ipo", "kind": "ipo", "probability": 0.60, "multiple_low": 50, "multiple_high": 100}
                ]),
            ),
            "outcomes_optimistic",
            c,
        );
        // A table close to the preset is not.
        let r = patched(&acme_with(
            "/outcomes",
            json!([
                {"id": "fails", "kind": "fails", "probability": 0.40},
                {"id": "sale", "kind": "acquired", "probability": 0.35, "multiple_low": 0.2, "multiple_high": 2},
                {"id": "private", "kind": "still_private", "probability": 0.25}
            ]),
        ));
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        assert!(
            !r.warnings.iter().any(|i| i.code == "outcomes_optimistic"),
            "{:#?}",
            r.warnings
        );
    }

    #[test]
    fn per_share_figures_and_amounts_are_bounded() {
        let c = "/private_equity/@acme";
        let h = format!("{c}/holdings/@g");
        let holding = |v: serde_json::Value| acme_with("/holdings", json!([v]));
        // A total typed into a per-share field far beyond any price.
        for (kind, field) in [
            (json!({"type": "nso", "strike": 2_000_000}), "strike"),
            (
                json!({"type": "shares", "basis_per_share": 1e7}),
                "basis_per_share",
            ),
            (
                json!({"type": "shares", "basis_per_share": 1, "from_iso": true, "fmv_at_exercise": 1e9}),
                "fmv_at_exercise",
            ),
        ] {
            assert_error(
                &holding(json!({"id": "g", "kind": kind, "shares": 100, "vested": 100})),
                "out_of_range",
                &format!("{h}/kind/{field}"),
            );
        }
        // Each within its bound, but a trillion dollars in all.
        for (kind, field) in [
            (json!({"type": "nso", "strike": 900_000}), "strike"),
            (
                json!({"type": "shares", "basis_per_share": 900_000}),
                "basis_per_share",
            ),
            (
                json!({"type": "shares", "basis_per_share": 1, "from_iso": true, "fmv_at_exercise": 900_000}),
                "fmv_at_exercise",
            ),
        ] {
            assert_error(
                &holding(json!({"id": "g", "kind": kind, "shares": 2e6, "vested": 2e6})),
                "equity_position_too_large",
                &format!("{h}/kind/{field}"),
            );
        }
        assert_error(
            &acme_with("/amt_credit_carryforward", json!(2e9)),
            "out_of_range",
            &format!("{c}/amt_credit_carryforward"),
        );
        assert_error(
            &holding(json!({
                "id": "g", "kind": {"type": "shares", "basis_per_share": 1, "months_held": 5000},
                "shares": 1
            })),
            "out_of_range",
            &format!("{h}/kind/months_held"),
        );
        // No dollar amount anywhere may pass a trillion: the profile that overflowed
        // the yearly totals (a 409A value at exercise of 1e15) is refused.
        assert_error(
            &holding(json!({
                "id": "g",
                "kind": {"type": "shares", "basis_per_share": 1, "from_iso": true, "fmv_at_exercise": 1e15, "months_held": 3},
                "shares": 10_000
            })),
            "amount_too_large",
            &format!("{h}/kind/fmv_at_exercise"),
        );
        let r = patched(&[PatchOp::set("/accounts/0/balance", 1e13)]);
        assert!(
            r.errors
                .iter()
                .any(|i| i.code == "amount_too_large" && i.path.ends_with("/balance")),
            "{:#?}",
            r.errors
        );
        let r = patched(&[PatchOp::set("/accounts/0/balance", 1e12)]);
        assert!(!r.errors.iter().any(|i| i.code == "amount_too_large"));
    }

    #[test]
    fn a_basis_above_the_price_is_questioned() {
        let c = "/private_equity/@acme";
        let h = format!("{c}/holdings/@g");
        let holding = |v: serde_json::Value| acme_with("/holdings", json!([v]));
        // $15,000 paid for 10,000 shares, typed as the basis per share.
        let r = patched(&holding(json!({
            "id": "g", "kind": {"type": "shares", "basis_per_share": 15_000}, "shares": 10_000
        })));
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        let issue = r
            .warnings
            .iter()
            .find(|i| i.code == "basis_above_price")
            .unwrap_or_else(|| panic!("{:#?}", r.warnings));
        assert_eq!(issue.path, format!("{h}/kind/basis_per_share"));
        assert!(
            issue.message.contains("$15,000.00 a share")
                && issue.message.contains("$10.00")
                && issue.message.contains("not the total paid"),
            "{}",
            issue.message
        );
        assert_warning(
            &holding(json!({
                "id": "g",
                "kind": {"type": "shares", "basis_per_share": 1.5, "from_iso": true, "fmv_at_exercise": 5000},
                "shares": 10_000
            })),
            "fmv_at_exercise_above_price",
            &format!("{h}/kind/fmv_at_exercise"),
        );
        // At or below the price: nothing to say (a basis equal to it is a purchase
        // in the round).
        let r = patched(&holding(json!({
            "id": "g",
            "kind": {"type": "shares", "basis_per_share": 10, "from_iso": true, "fmv_at_exercise": 10},
            "shares": 10_000
        })));
        assert!(
            !r.warnings
                .iter()
                .any(|i| i.code == "basis_above_price" || i.code == "fmv_at_exercise_above_price"),
            "{:#?}",
            r.warnings
        );
        assert_eq!(dollars_cents(1_234.5), "$1,234.50");
        assert_eq!(dollars_cents(0.05), "$0.05");
        assert_eq!(dollars_cents(f64::NAN), "$0.00");
    }

    #[test]
    fn options_past_or_near_their_term_are_reported() {
        let c = "/private_equity/@acme";
        let h = format!("{c}/holdings/@g");
        let grant = |ago: u32, vested: u32, left: u32| {
            acme_with(
                "/holdings",
                json!([{
                    "id": "g", "kind": {"type": "iso", "strike": 2, "granted_months_ago": ago},
                    "shares": 1000, "vested": vested, "vest_months_left": left
                }]),
            )
        };
        // Ten years or more after the grant: the options have lapsed.
        for ago in [120, 125, 4_000_000_000] {
            assert_error(
                &grant(ago, 1000, 0),
                "option_term_passed",
                &format!("{h}/kind/granted_months_ago"),
            );
        }
        let r = patched(&grant(119, 1000, 0));
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        // Within two years of the term: when, and what the plan does then.
        let soon = |ops: &[PatchOp]| {
            let r = patched(ops);
            assert!(r.errors.is_empty(), "{:#?}", r.errors);
            r.warnings
                .iter()
                .find(|i| i.code == "option_term_soon")
                .map(|i| i.message.clone())
        };
        let message = soon(&grant(100, 1000, 0)).expect("a warning");
        assert!(
            message.contains("in 20 months") && message.contains("about $2,000 of strike"),
            "{message}"
        );
        let mut ops = grant(100, 1000, 0).to_vec();
        ops.push(PatchOp::set(format!("{c}/policy/at_deadline"), "forfeit"));
        let message = soon(&ops).expect("a warning");
        assert!(message.contains("lets them lapse"), "{message}");
        assert!(soon(&grant(96, 1000, 0)).is_some(), "24 months left");
        assert!(soon(&grant(95, 1000, 0)).is_none(), "25 months left");
        // The term falls inside the vesting that is left.
        assert_warning(
            &grant(100, 400, 36),
            "option_term_before_vesting",
            &format!("{h}/kind/granted_months_ago"),
        );
        let r = patched(&grant(60, 400, 36));
        assert!(
            !r.warnings.iter().any(|i| i.code.starts_with("option_term")),
            "{:#?}",
            r.warnings
        );
    }

    #[test]
    fn a_tender_cannot_be_priced_at_nothing() {
        let c = "/private_equity/@acme";
        for fraction in [0.0, 0.04, -1.0] {
            assert_error(
                &acme_with("/tenders/price_fraction", json!(fraction)),
                "out_of_range",
                &format!("{c}/tenders/price_fraction"),
            );
        }
        for fraction in [0.05, 1.0, 2.0] {
            let r = patched(&acme_with("/tenders/price_fraction", json!(fraction)));
            assert!(r.errors.is_empty(), "{fraction}: {:#?}", r.errors);
        }
    }

    #[test]
    fn keeping_small_business_stock_is_explained() {
        let c = "/private_equity/@acme";
        let ops = [
            acme(),
            PatchOp::set(format!("{c}/qsbs"), true),
            PatchOp::set(format!("{c}/policy/sell_down"), json!({"type": "hold"})),
        ];
        assert_warning(&ops, "qsbs_hold", &format!("{c}/policy/sell_down"));
        let r = patched(&ops[..2]);
        assert!(!r.warnings.iter().any(|i| i.code == "qsbs_hold"));
    }

    #[test]
    fn a_row_with_only_a_probability_is_flagged() {
        let c = "/private_equity/@acme";
        // Written with probabilities alone, a row is a price of exactly 1x in years 1
        // to 5, not the stage's typical price and timing.
        let ops = acme_with(
            "/outcomes",
            json!([
                {"id": "fails", "kind": "fails", "probability": 0.5},
                {"id": "ipo", "kind": "ipo", "probability": 0.5}
            ]),
        );
        assert_warning(&ops, "outcome_flat_price", &format!("{c}/outcomes/@ipo"));
        let r = patched(&ops);
        assert!(!has(
            &r.warnings,
            "outcome_flat_price",
            &format!("{c}/outcomes/@fails")
        ));
        // A row with a spread is not flagged, nor is one with no chance.
        let r = patched(&acme_with(
            "/outcomes",
            json!([
                {"id": "fails", "kind": "fails", "probability": 0.5},
                {"id": "ipo", "kind": "ipo", "probability": 0.5, "multiple_low": 1.2, "multiple_high": 7},
                {"id": "sale", "kind": "acquired", "probability": 0.0}
            ]),
        ));
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        assert!(
            !r.warnings.iter().any(|i| i.code == "outcome_flat_price"),
            "{:#?}",
            r.warnings
        );
    }

    #[test]
    fn a_tender_chance_too_high_to_meet_is_flagged() {
        let c = "/private_equity/@acme";
        let path = format!("{c}/tenders/annual_probability");
        assert_warning(
            &acme_with("/tenders/annual_probability", json!(1.0)),
            "tender_chance_capped",
            &path,
        );
        // A modest chance can be met in full, and the stage's own chance always can.
        for ops in [
            acme_with("/tenders/annual_probability", json!(0.05)).to_vec(),
            vec![acme()],
        ] {
            let r = patched(&ops);
            assert!(r.errors.is_empty(), "{:#?}", r.errors);
            assert!(
                !r.warnings.iter().any(|i| i.code == "tender_chance_capped"),
                "{:#?}",
                r.warnings
            );
        }
    }

    #[test]
    fn one_company_is_checked_on_its_own_like_in_a_profile() {
        // The same checks, with paths relative to the company.
        let mut company = PrivateCompany {
            id: "acme".into(),
            preferred_price: 10.0,
            holdings: vec![EquityHolding {
                id: "g".into(),
                kind: HoldingKind::Nso {
                    strike: 2.0,
                    granted_months_ago: 24,
                },
                shares: 100.0,
                vested: 100.0,
                ..EquityHolding::default()
            }],
            ..PrivateCompany::default()
        };
        assert_eq!(validate_company(&company), Report::default());
        // A bare company, as the editor previews a stage's typical odds.
        let bare = PrivateCompany {
            preferred_price: 1.0,
            ..PrivateCompany::default()
        };
        assert!(validate_company(&bare).is_ok());
        company.holdings[0].vested = 40.0;
        company.lockup_months = 99;
        let r = validate_company(&company);
        assert!(has(&r.errors, "out_of_range", "/lockup_months"), "{r:#?}");
        // Unvested units with no job to vest in.
        assert!(
            has(
                &r.errors,
                "unvested_without_schedule",
                "/holdings/@g/vest_months_left"
            ) && has(
                &r.errors,
                "equity_unvested_without_job",
                "/holdings/@g/vested"
            ),
            "{r:#?}"
        );
    }

    #[test]
    fn a_table_with_no_failure_row_prints_zero_not_minus_zero() {
        let c = "/private_equity/@acme";
        let r = patched(&acme_with(
            "/outcomes",
            json!([{
                "id": "a", "kind": "acquired", "probability": 1, "years_from": 1, "years_to": 2,
                "multiple_low": 3, "multiple_high": 3
            }]),
        ));
        let issue = r
            .warnings
            .iter()
            .find(|i| i.code == "outcomes_optimistic")
            .unwrap_or_else(|| panic!("{:#?}", r));
        assert_eq!(issue.path, format!("{c}/outcomes"));
        assert!(
            issue.message.contains("Failure is 0% in your table"),
            "{}",
            issue.message
        );
    }
}
