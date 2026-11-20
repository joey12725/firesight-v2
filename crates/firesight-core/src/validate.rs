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

use crate::model::*;

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

    fn error(&mut self, code: &str, path: &str, message: impl Into<String>) {
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

    /// Money must be finite and not negative.
    fn money(&mut self, path: &str, what: &str, v: f64) -> bool {
        if !v.is_finite() || v < 0.0 {
            self.error(
                "negative_amount",
                path,
                format!("{what} is {v}; amounts must be zero or positive dollars."),
            );
            return false;
        }
        true
    }
}

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
    c.spending();
    c.accounts_and_portfolio();
    c.properties_and_debts();
    c.events();
    c.retirement();
    c.assumptions();
    if c.r.errors.is_empty() {
        // Lints read the profile as a whole and assume it is structurally sound.
        c.plausibility();
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
                EquityCompany::Public { volatility, beta } => {
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
                } => {
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
            }
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

    fn assumptions(&mut self) {
        let a = &self.p.assumptions;
        if let MarketKind::Bootstrap {
            mean_block_months,
            from_year,
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
            -0.1,
            0.1,
        );
        self.r.range(
            "/assumptions/market/inflation_adjustment",
            "Inflation adjustment",
            a.market.inflation_adjustment,
            -0.05,
            0.15,
        );
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
            }
        }
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
        if p.assumptions.market.equity_premium_adjustment > 0.02 {
            self.r.warn("returns_optimistic", "/assumptions/market/equity_premium_adjustment", "Adding more than two points a year to historical stock returns is very optimistic.");
        }
        if p.household.location.is_us() && p.household.location.state.is_none() {
            self.r.warn("state_missing", "/household/location/state", "No US state is set, so state income tax is not included and property tax uses the national average.");
        }
        if !p.household.location.is_us() && p.household.location.foreign_tax_rate.is_none() {
            self.r.warn("foreign_tax_missing", "/household/location/foreign_tax_rate", "The household lives outside the US and no foreign_tax_rate is set, so no income tax is applied.");
        }
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
            assert!(r.warnings.is_empty(), "{}: {:#?}", ex.slug, r.warnings);
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
}
