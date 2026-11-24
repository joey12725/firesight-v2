//! Migration of FIRESight v1 profiles (the Python app's JSON) to the v2 schema.
//!
//! v1 stored a flat dictionary: an age instead of a birth date, income and budget
//! lists with percent-valued fields, one list mixing assets and debts, allocation as
//! `investment_*` percentages, and "milestones" with boolean flags. It also accepted
//! the wrapper used by its example files, `{ "title", "summary", "profile": {..} }`.
//!
//! Migration is best-effort and loud: every place where v2 needs something v1 never
//! recorded, or where v1's behaviour was a modelling error that v2 does not
//! reproduce, a sentence is added to the returned notes. The UI shows the notes after
//! import so the user can review them. The migrated profile always passes
//! [`crate::validate::validate`] without errors (tested on the three v1 examples).

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::housing::amortized_payment;
use crate::model::*;

/// Start month assumed by [`migrate_v1`], which has no clock. Real callers should use
/// [`migrate_v1_at`] with the current month.
pub const DEFAULT_MIGRATION_START: YearMonth = EXAMPLE_START;

/// Converts a v1 profile to v2, assuming it describes the household as of
/// [`DEFAULT_MIGRATION_START`]. Returns the profile and the list of assumptions made.
pub fn migrate_v1(v1: &Value) -> Result<(Profile, Vec<String>), String> {
    migrate_v1_at(v1, DEFAULT_MIGRATION_START)
}

/// Converts a v1 profile to v2, taking `today` as the date the v1 ages refer to and
/// as the start of the plan.
pub fn migrate_v1_at(v1: &Value, today: YearMonth) -> Result<(Profile, Vec<String>), String> {
    let root = v1.as_object().ok_or("a v1 profile must be a JSON object")?;
    // The example files wrap the profile; saved profiles are bare.
    let (title, summary, src) = match root.get("profile").and_then(Value::as_object) {
        Some(inner) => (text(root, "title"), text(root, "summary"), inner),
        None => (
            text(root, "name").or_else(|| text(root, "title")),
            None,
            root,
        ),
    };
    if !src.contains_key("age") && !src.contains_key("income") && !src.contains_key("budget_items")
    {
        return Err("this does not look like a v1 profile: expected fields such as \"age\", \"income\" and \"budget_items\"".into());
    }

    let mut m = Migration {
        src,
        today,
        notes: Vec::new(),
        ids: BTreeSet::new(),
        profile: Profile {
            start: Some(today),
            ..Profile::default()
        },
        inflation: num(src, "inflation").unwrap_or(0.03),
    };
    m.profile.name = title.unwrap_or_else(|| "Imported v1 profile".to_string());
    m.profile.description = summary.unwrap_or_default();
    m.household();
    m.incomes();
    m.budget();
    m.assets_and_debts();
    m.allocation();
    m.milestones();
    m.retirement();
    m.settings();
    Ok((m.profile, m.notes))
}

struct Migration<'a> {
    src: &'a Map<String, Value>,
    today: YearMonth,
    notes: Vec<String>,
    /// Ids handed out so far, across all lists, so generated ids never collide.
    ids: BTreeSet<String>,
    profile: Profile,
    inflation: f64,
}

fn num(obj: &Map<String, Value>, key: &str) -> Option<f64> {
    match obj.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
    .filter(|v| v.is_finite())
}

fn text(obj: &Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn flag(obj: &Map<String, Value>, key: &str) -> bool {
    obj.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn objects<'v>(obj: &'v Map<String, Value>, key: &str) -> Vec<&'v Map<String, Value>> {
    obj.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_object).collect())
        .unwrap_or_default()
}

/// Percent fields in v1 (`3.0` meaning 3%) to decimals.
fn pct(obj: &Map<String, Value>, key: &str) -> Option<f64> {
    num(obj, key).map(|v| v / 100.0)
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_');
    out.chars().take(40).collect()
}

const STATES: [(&str, &str); 51] = [
    ("alabama", "AL"),
    ("alaska", "AK"),
    ("arizona", "AZ"),
    ("arkansas", "AR"),
    ("california", "CA"),
    ("colorado", "CO"),
    ("connecticut", "CT"),
    ("delaware", "DE"),
    ("district of columbia", "DC"),
    ("florida", "FL"),
    ("georgia", "GA"),
    ("hawaii", "HI"),
    ("idaho", "ID"),
    ("illinois", "IL"),
    ("indiana", "IN"),
    ("iowa", "IA"),
    ("kansas", "KS"),
    ("kentucky", "KY"),
    ("louisiana", "LA"),
    ("maine", "ME"),
    ("maryland", "MD"),
    ("massachusetts", "MA"),
    ("michigan", "MI"),
    ("minnesota", "MN"),
    ("mississippi", "MS"),
    ("missouri", "MO"),
    ("montana", "MT"),
    ("nebraska", "NE"),
    ("nevada", "NV"),
    ("new hampshire", "NH"),
    ("new jersey", "NJ"),
    ("new mexico", "NM"),
    ("new york", "NY"),
    ("north carolina", "NC"),
    ("north dakota", "ND"),
    ("ohio", "OH"),
    ("oklahoma", "OK"),
    ("oregon", "OR"),
    ("pennsylvania", "PA"),
    ("rhode island", "RI"),
    ("south carolina", "SC"),
    ("south dakota", "SD"),
    ("tennessee", "TN"),
    ("texas", "TX"),
    ("utah", "UT"),
    ("vermont", "VT"),
    ("virginia", "VA"),
    ("washington", "WA"),
    ("west virginia", "WV"),
    ("wisconsin", "WI"),
    ("wyoming", "WY"),
];

const COUNTRIES: [(&str, &str); 40] = [
    ("united states", "US"),
    ("usa", "US"),
    ("united states of america", "US"),
    ("canada", "CA"),
    ("mexico", "MX"),
    ("united kingdom", "GB"),
    ("uk", "GB"),
    ("ireland", "IE"),
    ("portugal", "PT"),
    ("spain", "ES"),
    ("france", "FR"),
    ("germany", "DE"),
    ("italy", "IT"),
    ("netherlands", "NL"),
    ("belgium", "BE"),
    ("switzerland", "CH"),
    ("austria", "AT"),
    ("greece", "GR"),
    ("poland", "PL"),
    ("czech republic", "CZ"),
    ("sweden", "SE"),
    ("norway", "NO"),
    ("denmark", "DK"),
    ("finland", "FI"),
    ("australia", "AU"),
    ("new zealand", "NZ"),
    ("japan", "JP"),
    ("south korea", "KR"),
    ("singapore", "SG"),
    ("thailand", "TH"),
    ("vietnam", "VN"),
    ("malaysia", "MY"),
    ("indonesia", "ID"),
    ("philippines", "PH"),
    ("india", "IN"),
    ("costa rica", "CR"),
    ("panama", "PA"),
    ("colombia", "CO"),
    ("argentina", "AR"),
    ("brazil", "BR"),
];

fn state_code(name: &str) -> Option<&'static str> {
    let lower = name.trim().to_ascii_lowercase();
    STATES
        .iter()
        .find(|(n, code)| *n == lower || code.eq_ignore_ascii_case(&lower))
        .map(|(_, code)| *code)
}

fn country_code(name: &str) -> Option<String> {
    let trimmed = name.trim();
    let lower = trimmed.to_ascii_lowercase();
    if let Some((_, code)) = COUNTRIES.iter().find(|(n, _)| *n == lower) {
        return Some((*code).to_string());
    }
    (trimmed.len() == 2 && trimmed.chars().all(|c| c.is_ascii_alphabetic()))
        .then(|| trimmed.to_ascii_uppercase())
}

/// The `YYYY-MM-DD` dates v1 wrote for milestones.
fn date(obj: &Map<String, Value>, key: &str) -> Option<YearMonth> {
    obj.get(key)?
        .as_str()?
        .get(..10)
        .or_else(|| obj.get(key)?.as_str())?
        .parse()
        .ok()
}

impl Migration<'_> {
    fn note(&mut self, text: impl Into<String>) {
        self.notes.push(text.into());
    }

    fn id(&mut self, wanted: &str, fallback: &str) -> String {
        let base = match slug(wanted) {
            s if s.is_empty() => fallback.to_string(),
            s => s,
        };
        let mut candidate = base.clone();
        let mut n = 2;
        while !self.ids.insert(candidate.clone()) {
            candidate = format!("{base}_{n}");
            n += 1;
        }
        candidate
    }

    fn household(&mut self) {
        let src = self.src;
        let age = num(src, "age").unwrap_or(30.0).clamp(16.0, 100.0);
        self.note(format!(
            "v1 stored ages, not birth dates: birth months are derived from age {age:.0} as of {} and should be corrected.",
            self.today
        ));
        let mut people = vec![Person {
            id: self.id("you", "you"),
            name: "You".into(),
            birth: self.today - (age * 12.0).round() as i32,
            ..Person::default()
        }];
        if flag(src, "married") {
            let spouse_age = num(src, "spouse_age").unwrap_or(age).clamp(16.0, 100.0);
            people.push(Person {
                id: self.id("partner", "partner"),
                name: "Partner".into(),
                birth: self.today - (spouse_age * 12.0).round() as i32,
                ..Person::default()
            });
        }
        self.note("Sex and health were not recorded in v1; average mortality is assumed. Social Security is estimated from earnings with a claim age of 67 (v1 had no Social Security).");

        let dependents = num(src, "dependents").unwrap_or(0.0).max(0.0) as usize;
        let mut children = Vec::new();
        for i in 0..dependents.min(10) {
            let id = self.id(&format!("child_{}", i + 1), "child");
            children.push(Child {
                id,
                name: format!("Child {}", i + 1),
                birth: self.today - 8 * 12,
                costs: ChildCosts::InBudget,
                childcare_monthly: None,
                college: College::None,
                college_share: 1.0,
                ..Child::default()
            });
        }
        if dependents > 0 {
            self.note(format!(
                "v1 recorded {dependents} dependant(s) without ages: each is assumed to be 8 years old, with costs already in the budget and no college plan. Set real birth months and a college plan."
            ));
        }

        let mut location = Location::default();
        let country_name = text(src, "country").unwrap_or_else(|| "United States".into());
        match country_code(&country_name) {
            Some(code) => location.country = code,
            None => self.note(format!(
                "Country {country_name:?} was not recognised; the United States is assumed."
            )),
        }
        if location.is_us() {
            if let Some(state) = text(src, "state") {
                match state_code(&state) {
                    Some(code) => location.state = Some(code.to_string()),
                    None => self.note(format!("State {state:?} was not recognised; no state income tax is applied until it is set.")),
                }
            }
        } else {
            self.note("The household lives outside the US: set household.location.foreign_tax_rate to its effective income tax rate.");
        }
        location.city = text(src, "city");
        self.profile.household = Household {
            people,
            children,
            location,
        };
    }

    fn real_growth(&mut self, inc: &Map<String, Value>, name: &str) -> f64 {
        if flag(inc, "has_growth")
            && text(inc, "growth_type").as_deref().unwrap_or("percentage") == "percentage"
        {
            let nominal = pct(inc, "growth_percentage").unwrap_or(0.0);
            let real = ((nominal - self.inflation) * 1e4).round() / 1e4;
            let real = real.clamp(-0.1, 0.15);
            self.note(format!("{name}: v1 grew this income {:.1}% a year in nominal dollars. It is imported as a flat raise of {:.1}% a year above inflation (v1's {:.1}% inflation assumption taken off), for as long as the income lasts.", nominal * 100.0, real * 100.0, self.inflation * 100.0));
            real
        } else {
            self.note(format!("{name}: v1 held this income flat in nominal dollars (falling behind inflation every year). It now keeps pace with inflation."));
            0.0
        }
    }

    fn incomes(&mut self) {
        let src = self.src;
        let list = objects(src, "income");
        let n_people = self.profile.household.people.len();
        let mut incomes: Vec<Income> = Vec::new();
        let mut jobs_seen = 0usize;
        // Bonuses, commission and options attach to a job, so jobs go first.
        for inc in list.iter().filter(|i| {
            text(i, "income_type").as_deref().unwrap_or("salary/hourly") == "salary/hourly"
        }) {
            let name = text(inc, "name").unwrap_or_else(|| "Job".into());
            let amount = num(inc, "amount").unwrap_or(0.0).max(0.0);
            let frequency = text(inc, "frequency").unwrap_or_else(|| "annual".into());
            // Same conversions as v1 (4.33 weeks per month) so migrated pay matches.
            let (mut salary, hourly) = match frequency.as_str() {
                "monthly" => (amount * 12.0, amount * 12.0 / 2080.0),
                "weekly" => (amount * 4.33 * 12.0, amount / 40.0),
                "biweekly" => (amount * 2.17 * 12.0, amount * 26.0 / 2080.0),
                "hourly" => (amount * 40.0 * 4.33 * 12.0, amount),
                _ => (amount, amount / 2080.0),
            };
            if frequency == "hourly" {
                self.note(format!("{name}: hourly pay of {amount} was converted to an annual salary assuming 40 hours a week."));
            }
            if flag(inc, "overtime_eligible") {
                let hours = num(inc, "monthly_overtime_hours").unwrap_or(12.0);
                let rate = num(inc, "overtime_rate").unwrap_or(1.5);
                let overtime = hours * hourly * rate * 12.0;
                salary += overtime;
                self.note(format!("{name}: overtime ({hours} hours a month at {rate}x) was folded into the salary as {overtime:.0} a year."));
            }
            let real_raise = self.real_growth(inc, &name);
            let owner = if n_people == 2 { jobs_seen % 2 } else { 0 };
            jobs_seen += 1;
            let id = self.id(&name, "job");
            incomes.push(Income {
                id,
                name,
                owner,
                kind: IncomeKind::Job(Job {
                    salary: salary.round(),
                    growth: WageGrowth::Flat { real_raise },
                    ..Job::default()
                }),
                ..Income::default()
            });
        }
        if jobs_seen > 0 {
            self.note("Jobs: v1 had no retirement plan contributions, employer match, payroll tax or job-loss risk. Jobs now carry typical career risk and no 401(k) plan; add a plan to each job to model contributions and match.");
            if n_people == 2 && jobs_seen > 1 {
                self.note("v1 did not record who earns which income: the first salary is assigned to you, the second to your partner, alternating. Check each income's owner.");
            }
        }
        let first_job = incomes
            .iter()
            .position(|i| matches!(i.kind, IncomeKind::Job(_)));

        for inc in &list {
            let kind = text(inc, "income_type").unwrap_or_else(|| "salary/hourly".into());
            let name = text(inc, "name").unwrap_or_else(|| kind.replace('_', " "));
            match kind.as_str() {
                "salary/hourly" => {}
                "bonus" => {
                    let amount = num(inc, "amount").unwrap_or(0.0).max(0.0);
                    let per_year = match text(inc, "bonus_frequency").as_deref() {
                        Some("monthly") => 12.0,
                        Some("quarterly") => 4.0,
                        Some("biannual") => 2.0,
                        _ => 1.0,
                    };
                    let annual = amount * per_year;
                    let job = first_job.and_then(|i| match &mut incomes[i].kind {
                        IncomeKind::Job(j) if j.salary > 0.0 && j.bonus.is_none() => Some(j),
                        _ => None,
                    });
                    match job {
                        Some(job) => {
                            let fraction = (annual / job.salary * 1e4).round() / 1e4;
                            job.bonus = Some(Bonus {
                                target_fraction: fraction,
                                volatility: 0.3,
                                month: 12,
                            });
                            self.note(format!("{name}: the bonus of {annual:.0} a year is attached to the first job as {:.1}% of salary, paid in December, with typical year-to-year variation (v1 paid it with certainty).", fraction * 100.0));
                        }
                        None => {
                            let id = self.id(&name, "bonus");
                            incomes.push(other_income(id, &name, annual, true, true));
                            self.note(format!("{name}: no job to attach the bonus to, so it is kept as other taxable income of {annual:.0} a year that ends at retirement."));
                        }
                    }
                }
                "commission" => {
                    let target = num(inc, "sales_target").unwrap_or(100_000.0);
                    let sd = num(inc, "sales_standard_deviation").unwrap_or(20_000.0);
                    let mut expected = if text(inc, "commission_type").as_deref() == Some("fixed") {
                        num(inc, "commission_fixed_amount").unwrap_or(1_000.0)
                    } else {
                        target * pct(inc, "commission_percentage").unwrap_or(0.05)
                    };
                    let cap = num(inc, "commission_cap").unwrap_or(0.0);
                    if cap > 0.0 {
                        expected = expected.min(cap);
                    }
                    let volatility = if target > 0.0 {
                        (sd / target).clamp(0.0, 3.0)
                    } else {
                        0.4
                    };
                    let commission = Commission {
                        expected_annual: expected.max(0.0),
                        volatility,
                    };
                    let job = first_job.and_then(|i| match &mut incomes[i].kind {
                        IncomeKind::Job(j) if j.commission.is_none() => Some(j),
                        _ => None,
                    });
                    match job {
                        Some(job) => {
                            job.commission = Some(commission);
                            self.note(format!("{name}: commission of about {expected:.0} a year is attached to the first job."));
                        }
                        None => {
                            let id = self.id(&name, "commission");
                            incomes.push(Income {
                                id,
                                name: name.clone(),
                                kind: IncomeKind::SelfEmployment(SelfEmployment {
                                    net_annual: expected.max(0.0),
                                    volatility,
                                    ..SelfEmployment::default()
                                }),
                                ..Income::default()
                            });
                            self.note(format!("{name}: no job to attach the commission to, so it is treated as self-employment income."));
                        }
                    }
                }
                "stock_options" => {
                    let pre_ipo = text(inc, "stock_options_type")
                        .as_deref()
                        .unwrap_or("pre_ipo")
                        == "pre_ipo";
                    let price = if pre_ipo {
                        num(inc, "estimated_ipo_price").unwrap_or(20.0)
                    } else {
                        num(inc, "current_stock_price").unwrap_or(10.0)
                    };
                    let strike = num(inc, "strike_price").unwrap_or(1.0);
                    let count = num(inc, "number_of_options").unwrap_or(10_000.0);
                    let grant_id = self.id(&name, "options");
                    let grant = EquityGrant {
                        id: grant_id,
                        kind: EquityKind::Option {
                            strike_fraction: if price > 0.0 {
                                (strike / price).clamp(0.0, 10.0)
                            } else {
                                1.0
                            },
                        },
                        company: if pre_ipo {
                            EquityCompany::Private {
                                liquidity_hazard: 0.12,
                                fail_prob: 0.5,
                                exit_volatility: 1.0,
                            }
                        } else {
                            EquityCompany::Public {
                                volatility: 0.45,
                                beta: 1.3,
                            }
                        },
                        unvested_value: (count * price).max(0.0),
                        vest_months_remaining: (num(inc, "vesting_period").unwrap_or(4.0) * 12.0)
                            .clamp(0.0, 240.0)
                            as u32,
                        cliff_months: (num(inc, "vesting_cliff").unwrap_or(1.0) * 12.0)
                            .clamp(0.0, 240.0) as u32,
                        refresh_annual_value: 0.0,
                        sell_on_vest: true,
                    };
                    let job = first_job.and_then(|i| match &mut incomes[i].kind {
                        IncomeKind::Job(j) => Some(j),
                        _ => None,
                    });
                    match job {
                        Some(job) => {
                            let mut grant = grant;
                            grant.cliff_months = grant.cliff_months.min(grant.vest_months_remaining);
                            job.equity.push(grant);
                            self.note(format!("{name}: {count:.0} options are attached to the first job as a grant worth {:.0} at {price} a share, assumed entirely unvested with the full vesting period remaining. v1's estimated IPO date, exercise strategy, ISO/NSO type and 83(b) election have no v2 equivalent; a private company's outcome is now random.", count * price));
                        }
                        None => self.note(format!("{name}: stock options need a job to attach to and none exists; the grant was dropped.")),
                    }
                }
                "rental" => self.rental(inc, &name),
                "gig_work" => {
                    let gross = num(inc, "gigs_per_month").unwrap_or(20.0)
                        * num(inc, "average_take_home_per_gig").unwrap_or(100.0);
                    let net =
                        gross * (1.0 - pct(inc, "gig_expenses_percentage").unwrap_or(0.2)) * 12.0;
                    let real_growth = self.real_growth(inc, &name);
                    let id = self.id(&name, "gig");
                    incomes.push(Income {
                        id,
                        name: name.clone(),
                        kind: IncomeKind::SelfEmployment(SelfEmployment {
                            net_annual: net.max(0.0).round(),
                            volatility: 0.25,
                            real_growth,
                            ..SelfEmployment::default()
                        }),
                        ..Income::default()
                    });
                    self.note(format!("{name}: gig work becomes self-employment income of {net:.0} a year after expenses, with 25% year-to-year variation and self-employment tax (v1 applied neither)."));
                }
                "government_assistance" | "other" => {
                    let amount = num(inc, "amount").unwrap_or(0.0).max(0.0);
                    let annual = if text(inc, "frequency").as_deref() == Some("annual") {
                        amount
                    } else {
                        amount * 12.0
                    };
                    let taxable = kind == "other";
                    let id = self.id(&name, "income");
                    incomes.push(other_income(id, &name, annual, taxable, false));
                    self.note(format!("{name}: kept as {} income of {annual:.0} a year that continues in retirement and keeps pace with inflation.", if taxable { "taxable" } else { "untaxed" }));
                }
                other => self.note(format!(
                    "{name}: income type {other:?} is not known and was dropped."
                )),
            }
        }
        self.profile.incomes = incomes;
    }

    fn rental(&mut self, inc: &Map<String, Value>, name: &str) {
        let units = num(inc, "number_of_units").unwrap_or(1.0).max(1.0);
        let monthly_rent = units * num(inc, "rent_per_unit").unwrap_or(1000.0).max(0.0);
        let annual_rent = monthly_rent * 12.0;
        // v1 never recorded what the property is worth. A 7% gross rental yield is a
        // mid-range figure for US residential rentals.
        let mut value = (annual_rent / 0.07 / 1000.0).round() * 1000.0;
        let mut mortgage = None;
        if inc
            .get("has_mortgage")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        {
            let payment = num(inc, "mortgage_monthly_payment")
                .unwrap_or(1500.0)
                .max(0.0);
            let rate = pct(inc, "mortgage_interest_rate")
                .unwrap_or(0.035)
                .clamp(0.0, 0.25);
            let months = (num(inc, "mortgage_remaining_years").unwrap_or(30.0) * 12.0)
                .round()
                .clamp(1.0, 600.0) as u32;
            // Balance implied by the payment: payment / payment-per-dollar.
            let per_dollar = amortized_payment(1.0, rate, months);
            let balance = if per_dollar > 0.0 {
                (payment / per_dollar).round()
            } else {
                0.0
            };
            if balance > 0.0 {
                // Keep at least 20% equity so the estimate is not underwater.
                value = value.max((balance / 0.8 / 1000.0).ceil() * 1000.0);
                mortgage = Some(Mortgage {
                    balance,
                    rate,
                    remaining_months: months,
                    payment: Some(payment),
                });
            }
        }
        let other_costs = pct(inc, "non_tax_costs_percentage")
            .unwrap_or(0.10)
            .clamp(0.0, 1.0);
        let maintenance_rate = if value > 0.0 {
            ((annual_rent * other_costs / value) * 1e4).round() / 1e4
        } else {
            0.01
        };
        let state = text(inc, "rental_state");
        let home_state = self.profile.household.location.state.clone();
        if let (Some(rs), Some(hs)) = (state.as_deref().and_then(state_code), home_state.as_deref())
        {
            if rs != hs {
                self.note(format!("{name}: the rental is in {rs} but v2 taxes rental income in the household's state ({hs})."));
            }
        }
        let id = self.id(name, "rental");
        self.profile.properties.push(Property {
            id,
            name: name.to_string(),
            usage: PropertyUse::Rental(RentalTerms {
                monthly_rent,
                vacancy: (1.0 - pct(inc, "occupancy_rate").unwrap_or(0.95)).clamp(0.0, 1.0),
                management_fee: pct(inc, "property_management_fee")
                    .unwrap_or(0.08)
                    .clamp(0.0, 0.5),
                rent_real_growth: 0.0,
                land_fraction: 0.2,
                years_in_service: 0.0,
            }),
            value,
            basis: None,
            mortgage,
            property_tax_rate: None,
            insurance_rate: 0.0,
            maintenance_rate: maintenance_rate.clamp(0.0, 0.1),
            ..Property::default()
        });
        self.note(format!(
            "{name}: v1 treated a rental as income only. It is now a property: its value is ESTIMATED at {value:.0} from the rent (7% gross yield, at least 20% equity), the mortgage balance is derived from the payment, rate and remaining term, v1's \"non-tax costs\" become the maintenance rate, and property tax uses the state average. Rent keeps pace with inflation. Enter the real value and basis."
        ));
    }

    fn budget(&mut self) {
        let src = self.src;
        let mut items = Vec::new();
        let mut has_health = false;
        for b in objects(src, "budget_items") {
            let kind = text(b, "type").unwrap_or_else(|| "other".into());
            let name = text(b, "name").unwrap_or_else(|| kind.clone());
            let lower = name.to_ascii_lowercase();
            let contains = |words: &[&str]| words.iter().any(|w| lower.contains(w));
            let category = match kind.as_str() {
                "rent" | "mortgage" => Category::Rent,
                "utilities" => Category::Utilities,
                "groceries" => Category::Groceries,
                "dining_out" => Category::Dining,
                "gas" | "public_transport" => Category::Transport,
                "healthcare" => Category::Healthcare,
                "clothing" | "gym" => Category::Personal,
                "entertainment" => Category::Entertainment,
                _ if contains(&["child", "daycare", "nanny"]) => Category::Childcare,
                _ if contains(&["travel", "vacation", "holiday", "trip"]) => Category::Travel,
                _ if contains(&["insurance"]) => Category::Insurance,
                _ if contains(&["tuition", "school", "education"]) => Category::Education,
                _ if contains(&["charity", "donation", "gift", "tithe"]) => Category::Giving,
                _ => Category::Other,
            };
            has_health |= category == Category::Healthcare;
            let frequency = match text(b, "frequency").as_deref() {
                Some("weekly") => Frequency::Weekly,
                Some("annual") => Frequency::Annual,
                _ => Frequency::Monthly,
            };
            let amount = num(b, "value").unwrap_or(0.0).max(0.0);
            let in_retirement = if flag(b, "changes_if_not_working") {
                RetirementChange::Amount {
                    amount: num(b, "value_if_not_working").unwrap_or(amount).max(0.0),
                }
            } else {
                RetirementChange::Same
            };
            if kind == "mortgage" {
                self.note(format!("{name}: v1 kept a mortgage as a budget line with no loan behind it. It is imported as a housing cost that rises with inflation and never ends; replace it with a property and its mortgage so that the payment stays fixed and stops when the loan is repaid."));
            }
            let display = if name == kind {
                title_case(&kind)
            } else {
                name.clone()
            };
            let id = self.id(&name, "item");
            let display_note = display.clone();
            items.push(BudgetItem {
                id,
                name: display,
                category,
                amount,
                frequency,
                essential: category.default_essential(),
                in_retirement,
                // v1 budget lines kept pace with inflation and no more (its one
                // growth setting becomes lifestyle creep below): the category
                // defaults of v2 (rent +0.5%, healthcare +1.5%, childcare +1% a
                // year above inflation) are not applied to imported lines.
                real_growth: Some(0.0),
                ..BudgetItem::default()
            });
            if category == Category::Childcare {
                self.note(format!("{display_note}: imported as a childcare line that keeps pace with inflation and never ends. Give it an end date (for example when the children start school or turn 18)."));
            }
        }
        if !items.is_empty() {
            self.note("Budget lines keep pace with inflation exactly, as in v1 (real_growth 0). v2's defaults for new lines grow rent, healthcare and childcare a little faster than inflation; set real_growth to null on a line to use them.");
        }
        if has_health {
            self.profile.spending.healthcare.mode = HealthcareMode::InBudget;
            self.note("Healthcare: the v1 budget has healthcare lines, so healthcare mode is \"in_budget\" and the engine adds no premiums. Long-term care risk is still modelled. Switch to \"modeled\" and remove the lines to use the engine's estimates.");
        }

        let creep = num(src, "expenditure_growth_beyond_inflation").unwrap_or(0.0);
        if creep > 0.0 {
            let share = (creep / 0.02).clamp(0.0, 1.0);
            self.profile.spending.lifestyle_creep = (share * 100.0).round() / 100.0;
            self.note(format!(
                "v1 grew all spending {:.1}% a year faster than inflation for ever, which compounds to implausible budgets. v2 instead lets {:.0}% of each real pay rise turn into spending (lifestyle_creep); spending stops creeping when income stops growing.",
                creep * 100.0,
                self.profile.spending.lifestyle_creep * 100.0
            ));
        } else {
            self.profile.spending.lifestyle_creep = 0.0;
        }
        self.profile.spending.items = items;
        self.note("v1 had no random expenses, retirement spending decline or survivor adjustment; v2's defaults for those are switched on (typical shocks, the retirement spending smile, 70% survivor spending).");
    }

    fn assets_and_debts(&mut self) {
        let src = self.src;
        let mut retirement_seen = false;
        for a in objects(src, "assets_and_debts") {
            let kind = text(a, "type").unwrap_or_else(|| "Checking Account".into());
            let name = text(a, "name").unwrap_or_else(|| kind.clone());
            let value = num(a, "value").unwrap_or(0.0).max(0.0);
            let rate = pct(a, "interest_rate");
            let account =
                |m: &mut Self, account_kind: AccountKind, holdings: Option<AssetClass>| {
                    let id = m.id(&name, "account");
                    m.profile.accounts.push(Account {
                        id,
                        name: name.clone(),
                        kind: account_kind,
                        balance: value,
                        holdings: holdings.map(Allocation::single),
                        ..Account::default()
                    });
                };
            let debt = |m: &mut Self, debt_kind: DebtKind, default_rate: f64| {
                let years = num(a, "remaining_term").unwrap_or(0.0);
                let id = m.id(&name, "debt");
                m.profile.debts.push(Debt {
                    id,
                    name: name.clone(),
                    kind: debt_kind,
                    balance: value,
                    rate: rate.unwrap_or(default_rate).clamp(0.0, 0.6),
                    payment: None,
                    remaining_months: (years > 0.0)
                        .then(|| (years * 12.0).round().clamp(1.0, 600.0) as u32),
                });
                if years <= 0.0 {
                    m.note(format!("{name}: no remaining term was given (v1 then never repaid the debt); a five-year payoff is assumed, or 2% minimum payments for a credit card."));
                }
            };
            match kind.as_str() {
                "Checking Account" => {
                    let id = self.id(&name, "checking");
                    self.profile.accounts.push(Account { id, name: name.clone(), kind: AccountKind::Cash, balance: value, cash_yield: CashYield::Fixed { rate: 0.0 }, ..Account::default() });
                }
                "Savings Account" | "High Yield Savings" | "Certificate of Deposit" => {
                    account(self, AccountKind::Cash, None);
                    if let Some(r) = rate {
                        self.note(format!("{name}: v1 paid a fixed {:.1}% for ever. The account now tracks simulated Treasury bill rates less half a point, as savings rates do.", r * 100.0));
                    }
                }
                "Retirement Accounts" => {
                    retirement_seen = true;
                    account(self, AccountKind::Traditional, None);
                }
                "Index Funds" | "Brokerage Account" | "Investment Account" => account(self, AccountKind::Taxable, None),
                "Bonds" | "Peer-to-Peer Lending" => {
                    account(self, AccountKind::Taxable, Some(AssetClass::Bonds));
                    if kind == "Peer-to-Peer Lending" {
                        self.note(format!("{name}: peer-to-peer lending has no v2 asset class and is modelled as bonds."));
                    }
                }
                "Individual Stocks" => account(self, AccountKind::Taxable, Some(AssetClass::SingleStock)),
                "Cryptocurrency" => account(self, AccountKind::Taxable, Some(AssetClass::Crypto)),
                "Home (Paid Off)" | "Home (Being Paid Off)" | "Real Estate" | "Rental Real Estate" => {
                    let usage = if kind.starts_with("Home") { PropertyUse::Primary } else { PropertyUse::Vacation };
                    let id = self.id(&name, "property");
                    self.profile.properties.push(Property { id, name: name.clone(), usage, value, ..Property::default() });
                    self.note(match kind.as_str() {
                        "Home (Being Paid Off)" => format!("{name}: imported as a home worth {value:.0} with NO mortgage, because v1 did not record the loan. Add the mortgage balance, rate and remaining term to the property."),
                        "Home (Paid Off)" => format!("{name}: imported as a mortgage-free home worth {value:.0}. Property tax, insurance and maintenance are now charged at default rates."),
                        _ => format!("{name}: imported as a property worth {value:.0} that earns no rent. If it is let, change its use to \"rental\" and enter the rent."),
                    });
                }
                "Other Assets" => self.note(format!("{name}: \"Other Assets\" worth {value:.0} (cars, collectibles and the like) cannot be spent in retirement and were left out. Add them as an account if they are investments.")),
                "Credit Card Debt" => debt(self, DebtKind::CreditCard, 0.18),
                "Auto Loan" => debt(self, DebtKind::Auto, 0.05),
                "Personal Loan" => debt(self, DebtKind::Personal, 0.08),
                "Student Loan" => debt(self, DebtKind::Student, 0.04),
                "Mortgage" => {
                    let years = num(a, "remaining_term").unwrap_or(30.0);
                    let months = (years * 12.0).round().clamp(1.0, 600.0) as u32;
                    let loan = Mortgage { balance: value, rate: rate.unwrap_or(0.04).clamp(0.0, 0.25), remaining_months: months, payment: None };
                    match self.profile.properties.iter_mut().find(|p| matches!(p.usage, PropertyUse::Primary) && p.mortgage.is_none()) {
                        Some(home) => {
                            home.mortgage = Some(loan);
                            let home_name = home.name.clone();
                            self.note(format!("{name}: attached to the home \"{home_name}\"."));
                        }
                        None => {
                            debt(self, DebtKind::Other, 0.04);
                            self.note(format!("{name}: no home was listed to attach this mortgage to, so it is kept as a plain debt. Create the property and move the loan onto it."));
                        }
                    }
                }
                "Other Debts" => debt(self, DebtKind::Other, 0.10),
                other => self.note(format!("{name}: asset or debt type {other:?} is not known and was dropped (value {value:.0}).")),
            }
        }
        if retirement_seen {
            self.note("\"Retirement Accounts\" are imported as traditional (pre-tax) accounts owned by you, because v1 did not separate Roth from traditional or record the owner. Split them into the real accounts.");
        }
        if self
            .profile
            .accounts
            .iter()
            .any(|a| a.kind == AccountKind::Taxable)
        {
            self.note("Taxable accounts: v1 had no cost basis; 60% of each balance is assumed to be basis.");
        }
        if !self.profile.debts.is_empty() {
            self.note("Debts are paid on their schedules with no extra payments (debt_policy.strategy \"minimum\").");
        }
    }

    fn allocation(&mut self) {
        let src = self.src;
        let get = |key: &str| num(src, key).unwrap_or(0.0).max(0.0);
        let retirement = get("investment_retirement_accounts");
        let other = get("investment_other");
        let p2p = get("investment_peer_to_peer_lending");
        let mut target = Allocation::ZERO;
        target[AssetClass::UsStocks] = get("investment_index_funds") + 0.7 * retirement + other;
        target[AssetClass::Bonds] = get("investment_bonds") + 0.3 * retirement + p2p;
        target[AssetClass::RealEstate] = get("investment_rental_real_estate");
        target[AssetClass::Cash] = get("investment_high_yield_savings");
        target[AssetClass::SingleStock] = get("investment_individual_stocks");
        target[AssetClass::Crypto] = get("investment_cryptocurrency");
        if target.total() <= 0.0 {
            self.note(
                "No investment allocation was found; the default 60/20/20 portfolio is used.",
            );
            return;
        }
        self.profile.portfolio.target = target;
        if retirement > 0.0 {
            self.note(format!("Allocation: v1's \"Retirement Accounts\" slice ({retirement:.0}%) was an account type, not an asset; it is split 70% US stocks and 30% bonds."));
        }
        if p2p > 0.0 {
            self.note(format!(
                "Allocation: peer-to-peer lending ({p2p:.0}%) is modelled as bonds."
            ));
        }
        if other > 0.0 {
            self.note(format!("Allocation: \"Other\" ({other:.0}%) is modelled as US stocks. Define portfolio.custom_asset to model it separately."));
        }
        if target[AssetClass::RealEstate] > 0.0 {
            self.note("Allocation: \"Rental Real Estate\" is modelled as listed real estate funds (REITs). Directly owned rentals belong under properties.");
        }
        self.note("v1 never rebalanced and used made-up return processes per asset; v2 rebalances annually and draws returns from market history.");
    }

    fn milestones(&mut self) {
        let src = self.src;
        for (i, ms) in objects(src, "milestones").into_iter().enumerate() {
            let Some(at) = date(ms, "date") else {
                self.note(format!("Milestone {}: no readable date; dropped.", i + 1));
                continue;
            };
            let when = When::date(at);
            let event = |m: &mut Self, name: &str, fallback: &str, kind: EventKind| {
                let id = m.id(name, fallback);
                m.profile.events.push(LifeEvent {
                    id,
                    name: name.to_string(),
                    when,
                    kind,
                    ..LifeEvent::default()
                });
            };

            if flag(ms, "buy_a_house") {
                let price = num(ms, "house_price").unwrap_or(300_000.0).max(0.0);
                let down = num(ms, "down_payment").unwrap_or(60_000.0).max(0.0);
                let mut rate = num(ms, "mortgage_rate").unwrap_or(0.04);
                if rate > 1.0 {
                    rate /= 100.0;
                }
                let maintenance = num(ms, "annual_maintenance_cost");
                let property_id = self.id("home", "home");
                let buy = BuyHome {
                    property_id,
                    price,
                    down_payment_fraction: if price > 0.0 {
                        (down / price).clamp(0.0, 1.0)
                    } else {
                        0.2
                    },
                    rate: Some(rate.clamp(0.0, 0.25)),
                    term_years: num(ms, "mortgage_term_years")
                        .unwrap_or(30.0)
                        .clamp(1.0, 50.0) as u32,
                    replaces_rent: true,
                    property_tax_rate: num(ms, "property_tax_rate").map(|r| r.clamp(0.0, 0.06)),
                    maintenance_rate: match maintenance {
                        Some(cost) if price > 0.0 => {
                            ((cost / price) * 1e4).round().clamp(0.0, 1000.0) / 1e4
                        }
                        _ => Property::DEFAULT_MAINTENANCE_RATE,
                    },
                    hoa_monthly: num(ms, "hoa").unwrap_or(0.0).max(0.0),
                    ..BuyHome::default()
                };
                event(self, "Buy a home", "buy_home", EventKind::BuyHome(buy));
                self.note(format!("Home purchase in {at}: the price of {price:.0} is read as today's dollars and will be scaled by simulated home-price growth; the mortgage starts at the v1 rate of {:.2}% (set it to null to use the simulated market rate) and, as v1 had no refinancing, automatic refinancing is switched off for this plan (debt_policy.refinance_mortgages). Closing costs of 3% and homeowner's insurance are now included, which v1 left out.", rate * 100.0));
                self.profile.debt_policy.refinance_mortgages = false;
            }
            if flag(ms, "change_location") {
                let country_name = text(ms, "country").unwrap_or_else(|| "United States".into());
                let country = country_code(&country_name).unwrap_or_else(|| {
                    self.note(format!("Move in {at}: country {country_name:?} was not recognised; the United States is assumed."));
                    "US".to_string()
                });
                let city = text(ms, "city");
                let adjustment = text(ms, "expense_adjustment").unwrap_or_default();
                let cost_of_living = match adjustment.as_str() {
                    "Manual adjustment" => CostOfLivingChange::Factor {
                        overall: num(ms, "manual_expense_adjustment")
                            .unwrap_or(1.0)
                            .clamp(0.1, 10.0),
                    },
                    "Adjust for similar quality of life" => {
                        self.note(format!("Move in {at}: v1's automatic cost-of-living adjustment used average salaries rather than prices. Spending is left unchanged; pick the cities in the app to apply the corrected price indices."));
                        CostOfLivingChange::Unchanged
                    }
                    _ => CostOfLivingChange::Unchanged,
                };
                let abroad = country != "US";
                let relocate = Relocate {
                    state: if abroad {
                        None
                    } else {
                        text(ms, "state")
                            .as_deref()
                            .and_then(state_code)
                            .map(str::to_string)
                    },
                    city: city.clone(),
                    foreign_tax_rate: abroad.then_some(0.2),
                    moving_cost: num(ms, "moving_cost").unwrap_or(5_000.0).max(0.0),
                    cost_of_living,
                    country,
                };
                let label = format!("Move to {}", city.unwrap_or(country_name));
                if abroad {
                    self.note(format!("{label}: v1 kept charging US state tax after a move abroad. A flat 20% foreign income tax is assumed instead; set the real effective rate. The cost-of-living factor now applies to every budget line but not to mortgage or debt payments."));
                } else if relocate.state.is_none() {
                    self.note(format!("{label}: v1 did not record the destination state, so no state income tax applies after the move until it is set."));
                }
                event(self, &label, "move", EventKind::Relocate(relocate));
            }
            if flag(ms, "major_expense") {
                let name = text(ms, "item_name").unwrap_or_else(|| "Major purchase".into());
                let kind = match text(ms, "purchase_type").as_deref() {
                    Some("finance") => {
                        let total = num(ms, "total_cost").unwrap_or(0.0).max(0.0);
                        let down = num(ms, "down_payment").unwrap_or(0.0).max(0.0);
                        EventKind::Expense {
                            amount: total,
                            financed: Some(Financing {
                                down_fraction: if total > 0.0 {
                                    (down / total).clamp(0.0, 1.0)
                                } else {
                                    0.0
                                },
                                rate: pct(ms, "interest_rate").unwrap_or(0.05).clamp(0.0, 0.6),
                                term_months: num(ms, "loan_term_months")
                                    .unwrap_or(36.0)
                                    .clamp(1.0, 600.0)
                                    as u32,
                            }),
                        }
                    }
                    Some("lease") => {
                        let months = num(ms, "lease_term_months")
                            .unwrap_or(36.0)
                            .clamp(1.0, 600.0);
                        let total = num(ms, "monthly_payment").unwrap_or(0.0).max(0.0) * months;
                        self.note(format!("{name}: the lease is imported as a {total:.0} expense paid in equal instalments over {months:.0} months at no interest. Residual value and buy-out are not modelled."));
                        EventKind::Expense {
                            amount: total,
                            financed: Some(Financing {
                                down_fraction: 0.0,
                                rate: 0.0,
                                term_months: months as u32,
                            }),
                        }
                    }
                    _ => EventKind::Expense {
                        amount: num(ms, "total_cost").unwrap_or(0.0).max(0.0),
                        financed: None,
                    },
                };
                event(self, &name, "expense", kind);
            }
            if flag(ms, "major_windfall") {
                let amount = num(ms, "windfall_amount").unwrap_or(0.0).max(0.0);
                event(
                    self,
                    "Windfall",
                    "windfall",
                    EventKind::Windfall {
                        amount,
                        taxable: false,
                    },
                );
                self.note(format!(
                    "Windfall in {at}: {amount:.0} is treated as untaxed (an inheritance or gift)."
                ));
            }
            if flag(ms, "family_change") {
                self.family_change(ms, at);
            }
            if flag(ms, "change_income") || flag(ms, "change_budget") {
                self.note(format!("Milestone in {at}: v1's \"change income\" and \"change budget\" flags never did anything in the v1 engine and carried no details, so nothing was imported. Add an income_change or spending_change event."));
            }
        }
        if !self.profile.events.is_empty() {
            self.note("Milestone amounts are read as today's dollars (v1 treated them as dollars of the milestone date).");
        }
    }

    fn family_change(&mut self, ms: &Map<String, Value>, at: YearMonth) {
        match text(ms, "family_change_type")
            .as_deref()
            .unwrap_or("marriage")
        {
            "marriage" => {
                if self.profile.household.people.len() >= 2 {
                    self.note(format!(
                        "Marriage in {at}: the household already has two adults; ignored."
                    ));
                    return;
                }
                let you = self.profile.household.people[0].birth;
                let birth = match num(ms, "spouse_age") {
                    Some(age) => self.today - (age.clamp(16.0, 100.0) * 12.0).round() as i32,
                    None => you,
                };
                let id = self.id("partner", "partner");
                self.profile.household.people.push(Person {
                    id,
                    name: "Partner".into(),
                    birth,
                    joins: Some(at),
                    ..Person::default()
                });
                self.note(format!("Marriage in {at}: a partner joins the household then, with no income, accounts or spending of their own. Add theirs."));
            }
            "birth" => {
                let count = if flag(ms, "twins") { 2 } else { 1 };
                for _ in 0..count {
                    let n = self.profile.household.children.len() + 1;
                    let id = self.id(&format!("child_{n}"), "child");
                    self.profile.household.children.push(Child {
                        id,
                        name: format!("Child {n}"),
                        birth: at,
                        ..Child::default()
                    });
                }
                self.note(format!("Birth in {at}: v1 only changed the tax filing. v2 adds {count} child(ren) with modelled costs of raising them to 18 and no college plan."));
            }
            other => self.note(format!(
                "Family change {other:?} in {at} has no v2 equivalent and was not imported."
            )),
        }
    }

    fn retirement(&mut self) {
        let src = self.src;
        let method =
            text(src, "retirement_goal_method").unwrap_or_else(|| "Safe Withdrawal Rate".into());
        let mut extra_income = 0.0;
        match method.as_str() {
            "Fixed Value" => {
                let amount = num(src, "fixed_retirement_goal")
                    .unwrap_or(3_000_000.0)
                    .max(0.0);
                self.profile.retirement.trigger = RetirementTrigger::FixedNumber {
                    amount,
                    basis: WealthBasis::Investable,
                };
                self.note(format!("Retirement goal: retire when investable assets reach {amount:.0} in today's dollars. v1 compared a nominal net worth that included home equity; v2 counts only spendable assets."));
            }
            "Simulate Additional Years with Reduced Income" => {
                self.profile.retirement.trigger =
                    RetirementTrigger::WhenFunded(WhenFunded::default());
                extra_income = num(src, "additional_retirement_income")
                    .unwrap_or(0.0)
                    .max(0.0);
                self.note("Retirement goal: v1's \"simulate additional years with reduced income\" test (ten nested simulations of ten years) is replaced by the standard funded test at a 4% withdrawal rate. The list of incomes to remove is not needed: jobs end at retirement by default.");
            }
            _ => {
                let rate = num(src, "withdrawal_rate")
                    .unwrap_or(0.04)
                    .clamp(0.005, 0.2);
                self.profile.retirement.trigger = RetirementTrigger::WhenFunded(WhenFunded {
                    withdrawal_rate: rate,
                    ..WhenFunded::default()
                });
                extra_income = num(src, "average_post_retirement_income")
                    .unwrap_or(0.0)
                    .max(0.0);
                self.note(format!("Retirement goal: retire when investable assets can fund retirement spending at a {:.1}% withdrawal rate. v1 tested net worth including home equity; v2 counts only spendable assets, and also counts Social Security and tax.", rate * 100.0));
                self.scale_retirement_budget(num(src, "post_retirement_expenses").unwrap_or(0.0));
            }
        }
        if extra_income > 0.0 {
            let id = self.id("retirement_income", "retirement_income");
            let mut income = other_income(
                id,
                "Post-retirement income (from v1)",
                extra_income,
                true,
                false,
            );
            income.start = Some(When::retirement(0.0));
            self.profile.incomes.push(income);
            self.note(format!("Post-retirement income of {extra_income:.0} a year becomes a taxable income that starts at retirement and keeps pace with inflation."));
        }
        self.note("Retirement: v1 stopped at the retirement date. v2 simulates the whole retirement with tax-aware withdrawals, constant real spending and penalty-free early access where possible.");
    }

    /// v1 took retirement spending as one number; v2 derives it from the budget.
    /// Lines without an explicit "if not working" amount are scaled so the budget in
    /// retirement adds up to the v1 figure.
    fn scale_retirement_budget(&mut self, target: f64) {
        if target <= 0.0 {
            return;
        }
        let items = &mut self.profile.spending.items;
        let (mut fixed, mut flexible) = (0.0, 0.0);
        for it in items.iter() {
            match it.in_retirement {
                RetirementChange::Same => flexible += it.annual_amount(),
                _ => fixed += it.annual_amount_in_retirement(),
            }
        }
        if flexible <= 0.0 {
            return;
        }
        let factor = (((target - fixed) / flexible) * 1000.0).round() / 1000.0;
        let factor = factor.clamp(0.5, 3.0);
        if (factor - 1.0).abs() < 0.02 {
            return;
        }
        for it in items.iter_mut() {
            if it.in_retirement == RetirementChange::Same {
                it.in_retirement = RetirementChange::Factor { factor };
            }
        }
        self.note(format!(
            "v1's retirement spending target was {target:.0} a year, while the budget adds up to {:.0}. Budget lines without their own retirement amount are scaled by {factor} in retirement to match (limited to between 0.5 and 3).",
            fixed + flexible
        ));
    }

    fn settings(&mut self) {
        let src = self.src;
        if let Some(years) = num(src, "simulate_years") {
            self.note(format!("v1 simulated {years:.0} years; v2 always simulates to the end of life (age 100 at most)."));
        }
        if let Some(n) = num(src, "num_simulations") {
            self.note(format!("v1 ran {n:.0} simulations; v2 runs 10,000."));
        }
        let delta = ((self.inflation - 0.03) * 1e4).round() / 1e4;
        if delta.abs() >= 0.0025 {
            self.profile.assumptions.market.inflation_adjustment = delta.clamp(-0.05, 0.15);
            self.note(format!("v1 used a constant {:.1}% inflation. v2 simulates inflation from history (about 3% on average), shifted by {:+.1} points to respect that setting.", self.inflation * 100.0, delta * 100.0));
        } else {
            self.note("v1 used a constant 3% inflation; v2 simulates inflation from history.");
        }
        if src.get("custom_tax_brackets").is_some_and(|v| !v.is_null()) {
            self.note("Custom tax brackets from v1 were not imported; v2 applies current US federal and state law.");
        }
    }
}

fn other_income(
    id: String,
    name: &str,
    annual: f64,
    taxable: bool,
    ends_at_retirement: bool,
) -> Income {
    Income {
        id,
        name: name.to_string(),
        kind: IncomeKind::Other(OtherIncome {
            annual,
            taxable,
            real_growth: 0.0,
            ends_at_retirement,
        }),
        ..Income::default()
    }
}

fn title_case(s: &str) -> String {
    let spaced = s.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => spaced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::validate;
    use serde_json::json;

    fn example(name: &str) -> Value {
        let path = format!(
            "{}/../../examples/v1/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(
            &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")),
        )
        .unwrap()
    }

    fn migrated(name: &str) -> (Profile, Vec<String>) {
        let (profile, notes) = migrate_v1(&example(name)).unwrap();
        let report = validate(&profile);
        assert!(report.errors.is_empty(), "{name}: {:#?}", report.errors);
        // The result must survive a JSON round trip unchanged.
        assert_eq!(
            Profile::from_json(&profile.to_json_pretty()).unwrap(),
            profile
        );
        assert!(
            notes.len() >= 8,
            "{name}: expected the assumptions to be listed"
        );
        // Imported budget lines keep pace with inflation, as the notes say: none
        // picks up a category's faster default growth.
        assert!(profile
            .spending
            .items
            .iter()
            .all(|item| item.effective_real_growth() == 0.0));
        assert!(notes
            .iter()
            .any(|n| n.contains("keep pace with inflation exactly")));
        (profile, notes)
    }

    #[test]
    fn early_career_saver_migrates() {
        let (p, notes) = migrated("early-career-saver");
        assert_eq!(p.name, "Early-career saver");
        assert_eq!(p.household.people.len(), 1);
        assert_eq!(
            p.household.people[0].birth,
            DEFAULT_MIGRATION_START - 27 * 12
        );
        assert_eq!(p.household.location.state.as_deref(), Some("CO"));
        let IncomeKind::Job(job) = &p.incomes[0].kind else {
            panic!()
        };
        assert_eq!(job.salary, 108_000.0);
        assert_eq!(job.growth, WageGrowth::Flat { real_raise: 0.01 });
        let bonus = job.bonus.unwrap();
        assert!((bonus.target_fraction - 6_000.0 / 108_000.0).abs() < 1e-4);
        assert_eq!(p.incomes.len(), 1);
        // Dining and healthcare had "if not working" amounts.
        let dining = p
            .spending
            .items
            .iter()
            .find(|i| i.category == Category::Dining)
            .unwrap();
        assert_eq!(
            dining.in_retirement,
            RetirementChange::Amount { amount: 150.0 }
        );
        let travel = p
            .spending
            .items
            .iter()
            .find(|i| i.category == Category::Travel)
            .unwrap();
        assert_eq!(
            (travel.amount, travel.frequency),
            (3000.0, Frequency::Annual)
        );
        assert_eq!(p.spending.healthcare.mode, HealthcareMode::InBudget);
        assert_eq!(p.spending.lifestyle_creep, 0.5);
        // Accounts: checking, savings, 401(k).
        assert_eq!(
            p.accounts.iter().map(|a| a.kind).collect::<Vec<_>>(),
            [
                AccountKind::Cash,
                AccountKind::Cash,
                AccountKind::Traditional
            ]
        );
        assert_eq!(p.debts[0].kind, DebtKind::Student);
        assert_eq!(
            (p.debts[0].rate, p.debts[0].remaining_months),
            (0.05, Some(96))
        );
        // 70 index + 20 retirement (70/30) + 5 bonds + 5 savings.
        assert_eq!(p.portfolio.target[AssetClass::UsStocks], 84.0);
        assert_eq!(p.portfolio.target[AssetClass::Bonds], 11.0);
        assert_eq!(p.portfolio.target[AssetClass::Cash], 5.0);
        let EventKind::BuyHome(buy) = &p.events[0].kind else {
            panic!()
        };
        assert_eq!(p.events[0].when, When::date(YearMonth::ym(2030, 6)));
        assert_eq!(
            (
                buy.price,
                buy.down_payment_fraction,
                buy.rate,
                buy.term_years
            ),
            (420_000.0, 0.2, Some(0.06), 30)
        );
        assert_eq!(buy.property_tax_rate, Some(0.005));
        assert!((buy.maintenance_rate - 4000.0 / 420_000.0).abs() < 1e-4);
        assert!(
            matches!(p.retirement.trigger, RetirementTrigger::WhenFunded(w) if w.withdrawal_rate == 0.04)
        );
        assert!(notes.iter().any(|n| n.contains("birth months are derived")));
    }

    #[test]
    fn dual_income_family_migrates() {
        let (p, notes) = migrated("dual-income-family");
        assert_eq!(p.household.people.len(), 2);
        assert_eq!(p.household.children.len(), 2);
        assert_eq!(p.household.location.state.as_deref(), Some("TX"));
        // Two salaries alternate between the adults; the nurse is hourly with overtime.
        assert_eq!(
            p.incomes.iter().map(|i| i.owner).collect::<Vec<_>>(),
            [0, 1]
        );
        let IncomeKind::Job(nurse) = &p.incomes[1].kind else {
            panic!()
        };
        let expected = 41.0 * 40.0 * 4.33 * 12.0 + 10.0 * 41.0 * 1.5 * 12.0;
        assert!((nurse.salary - expected).abs() <= 0.5, "{}", nurse.salary);
        let IncomeKind::Job(pm) = &p.incomes[0].kind else {
            panic!()
        };
        assert!(pm.bonus.is_some() && nurse.bonus.is_none());
        // "Childcare & Activities" and "Vacations" were typed "other" in v1.
        assert!(p
            .spending
            .items
            .iter()
            .any(|i| i.category == Category::Childcare));
        assert!(p
            .spending
            .items
            .iter()
            .any(|i| i.category == Category::Travel));
        assert_eq!(p.debts[0].kind, DebtKind::Auto);
        assert_eq!(p.debts[0].remaining_months, Some(48));
        let EventKind::Expense {
            amount,
            financed: Some(f),
        } = &p.events[0].kind
        else {
            panic!()
        };
        assert_eq!(*amount, 45_000.0);
        assert!((f.down_fraction - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!((f.rate, f.term_months), (0.075, 60));
        assert!(
            matches!(p.retirement.trigger, RetirementTrigger::WhenFunded(w) if w.withdrawal_rate == 0.035)
        );
        assert!(notes
            .iter()
            .any(|n| n.contains("mortgage as a budget line")));
        assert!(notes.iter().any(|n| n.contains("dependant")));
    }

    #[test]
    fn side_hustle_landlord_migrates() {
        let (p, notes) = migrated("side-hustle-landlord");
        assert_eq!(p.household.location.state.as_deref(), Some("FL"));
        // The duplex becomes a property with a derived mortgage balance.
        let duplex = &p.properties[0];
        let PropertyUse::Rental(terms) = &duplex.usage else {
            panic!()
        };
        assert_eq!(terms.monthly_rent, 3100.0);
        assert!((terms.vacancy - 0.06).abs() < 1e-9);
        let m = duplex.mortgage.unwrap();
        assert_eq!(
            (m.rate, m.remaining_months, m.payment),
            (0.0425, 312, Some(1700.0))
        );
        assert!((amortized_payment(m.balance, m.rate, m.remaining_months) - 1700.0).abs() < 0.01);
        assert!(duplex.value >= m.balance / 0.8);
        assert!(notes.iter().any(|n| n.contains("ESTIMATED")));
        // Gig work: 24 gigs x $22 less 25% expenses, times 12.
        let gig = p.incomes.iter().find_map(|i| match &i.kind {
            IncomeKind::SelfEmployment(se) => Some(se.net_annual),
            _ => None,
        });
        assert_eq!(gig, Some(4752.0));
        // Post-retirement income starts at retirement.
        let post = p
            .incomes
            .iter()
            .find(|i| i.start == Some(When::retirement(0.0)))
            .unwrap();
        assert!(matches!(&post.kind, IncomeKind::Other(o) if o.annual == 12_000.0));
        // Crypto wallet keeps its holdings.
        let crypto = p
            .accounts
            .iter()
            .find(|a| a.name == "Crypto Wallet")
            .unwrap();
        assert_eq!(
            crypto.holdings,
            Some(Allocation::single(AssetClass::Crypto))
        );
        assert_eq!(p.debts[0].kind, DebtKind::CreditCard);
        let EventKind::Relocate(r) = &p.events[0].kind else {
            panic!()
        };
        assert_eq!(
            (r.country.as_str(), r.city.as_deref(), r.moving_cost),
            ("PT", Some("Lisbon"), 9000.0)
        );
        assert_eq!(
            r.cost_of_living,
            CostOfLivingChange::Factor { overall: 0.85 }
        );
        assert_eq!(r.foreign_tax_rate, Some(0.2));
        assert_eq!(p.spending.lifestyle_creep, 0.25);
    }

    #[test]
    fn bare_profiles_and_odd_inputs() {
        let bare = example("early-career-saver")["profile"].clone();
        let (p, _) = migrate_v1_at(&bare, YearMonth::ym(2027, 3)).unwrap();
        assert_eq!(p.start, Some(YearMonth::ym(2027, 3)));
        assert_eq!(p.household.people[0].birth, YearMonth::ym(2000, 3));
        assert!(validate(&p).errors.is_empty());

        assert!(migrate_v1(&json!([1, 2])).is_err());
        assert!(migrate_v1(&json!({"schema_version": 2, "household": {}}))
            .unwrap_err()
            .contains("v1"));

        // Every optional v1 feature at once.
        let v1 = json!({
            "age": 40, "married": false, "state": "New York", "country": "USA", "inflation": 0.04,
            "income": [
                {"income_type": "commission", "name": "Sales", "commission_type": "percentage", "commission_percentage": 5, "sales_target": 200000, "sales_standard_deviation": 50000},
                {"income_type": "stock_options", "name": "Options", "stock_options_type": "pre_ipo", "number_of_options": 1000, "strike_price": 2, "estimated_ipo_price": 20},
                {"income_type": "salary/hourly", "name": "Day Job", "amount": 5000, "frequency": "monthly", "has_growth": false},
                {"income_type": "government_assistance", "name": "Benefit", "amount": 300, "frequency": "monthly"},
                {"income_type": "mystery", "name": "?"}
            ],
            "budget_items": [{"type": "rent", "value": 1500, "frequency": "monthly"}],
            "assets_and_debts": [
                {"type": "Home (Being Paid Off)", "name": "House", "value": 400000},
                {"type": "Mortgage", "name": "Home loan", "value": 250000, "interest_rate": 5, "remaining_term": 20},
                {"type": "Other Assets", "name": "Car", "value": 9000},
                {"type": "Personal Loan", "name": "Loan", "value": 5000}
            ],
            "milestones": [
                {"date": "2030-01-01", "family_change": true, "family_change_type": "marriage", "spouse_age": 38},
                {"date": "2031-05-01", "family_change": true, "family_change_type": "birth", "twins": true},
                {"date": "2032-01-01", "major_windfall": true, "windfall_amount": 50000},
                {"date": "2033-01-01", "major_expense": true, "purchase_type": "lease", "item_name": "Car lease", "monthly_payment": 400, "lease_term_months": 36},
                {"date": "bad", "major_windfall": true}
            ],
            "retirement_goal_method": "Fixed Value", "fixed_retirement_goal": 2000000
        });
        let (p, notes) = migrate_v1(&v1).unwrap();
        let report = validate(&p);
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(p.household.location.state.as_deref(), Some("NY"));
        let IncomeKind::Job(job) = &p.incomes[0].kind else {
            panic!()
        };
        assert_eq!(job.salary, 60_000.0);
        assert_eq!(job.commission.unwrap().expected_annual, 10_000.0);
        assert!(matches!(
            job.equity[0].company,
            EquityCompany::Private { .. }
        ));
        assert_eq!(job.equity[0].unvested_value, 20_000.0);
        assert_eq!(p.properties[0].mortgage.unwrap().balance, 250_000.0);
        assert_eq!(p.household.people.len(), 2);
        assert_eq!(p.household.people[1].joins, Some(YearMonth::ym(2030, 1)));
        assert_eq!(p.household.children.len(), 2);
        assert_eq!(p.events.len(), 2);
        assert_eq!(p.assumptions.market.inflation_adjustment, 0.01);
        assert!(
            matches!(p.retirement.trigger, RetirementTrigger::FixedNumber { amount, .. } if amount == 2_000_000.0)
        );
        assert!(notes.iter().any(|n| n.contains("mystery")));
        assert!(notes.iter().any(|n| n.contains("no readable date")));
    }
}
