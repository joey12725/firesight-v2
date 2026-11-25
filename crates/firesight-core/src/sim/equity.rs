//! Private-company equity in the month loop: builds each company's climate, runs its
//! step (`crate::equity`), and books what comes back.
//!
//! # What is booked where
//!
//! | Flow | Cash | Ledger | Tax year |
//! |---|---|---|---|
//! | Strike paid on an exercise | out | `equity_exercise` | none |
//! | Proceeds of a sale, a cash-out or a tender | in | `income_equity` | wages, income without payroll tax, or gains, by its character |
//! | Shares moved to the single-stock sleeve | none | `income_equity` and `save_taxable` | none (the basis moves with them) |
//! | Payroll tax on wage-type income | out | `tax_payroll` | paid at the event |
//! | Federal withholding at the supplemental rate | out | `tax_federal` | paid at the event |
//!
//! Non-cash amounts (the spread on an option that is exercised and held, the AMT
//! adjustment) touch the tax year only. An exercise is funded from taxable savings:
//! what it takes out of the cash the household protects is raised by a sale the same
//! month (`withdraw_taxable`), whose gain is taxed like any other. The rest of the tax is paid through the
//! instalments of the re-plan made the month after, and the cash for it is held out
//! of the savings sweep (`State::tax_reserve`).
//!
//! # Draws
//!
//! One word per company per month, drawn for every company before anything else in
//! the step. Nothing a household does or is (alive, employed, retired, counted)
//! gates a draw, so two scenarios that differ only in a decision see the same fates
//! and the same tender months.

use crate::accounts::Bucket;
use crate::career::Employment;
use crate::decumulation::cover_shortfall;
use crate::economy::EconomyPath;
use crate::equity::{
    window_open, CompanyStatus, EqEvent, EqEventKind, EquityClimate, EquityMonth, Link,
};
use crate::model::{AssetClass, YearMonth};
use crate::stats::{EquityOutcome, TraceEventKind, YearSeries};
use crate::tax::{self, FilingStatus};

use super::plan::Plan;
use super::state::{State, TAX_FED, TAX_PAYROLL};

/// A whole number with thousands separators ("1,234,567").
fn grouped(x: f64) -> String {
    let n = if x.is_finite() { x.round() } else { 0.0 };
    let digits = format!("{:.0}", n.abs());
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0.0 {
        out.push('-');
    }
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Whole dollars ("$1,234").
fn dollars(x: f64) -> String {
    format!("${}", grouped(x))
}

/// The trace kind an equity event is reported under.
fn trace_kind(kind: EqEventKind) -> TraceEventKind {
    match kind {
        EqEventKind::Exercised => TraceEventKind::OptionsExercised,
        EqEventKind::Tender => TraceEventKind::TenderOffer,
        EqEventKind::Acquired => TraceEventKind::CompanyAcquired,
        EqEventKind::Ipo => TraceEventKind::CompanyIpo,
        EqEventKind::Failed | EqEventKind::WrittenOff => TraceEventKind::CompanyFailed,
        EqEventKind::Forfeited | EqEventKind::Lapsed | EqEventKind::Expired => {
            TraceEventKind::EquityForfeited
        }
        EqEventKind::LockupSale | EqEventKind::LockupKept => TraceEventKind::LockupSale,
    }
}

/// What the label needs to know beyond the event itself.
struct LabelContext<'a> {
    company: &'a str,
    /// Price level of the month (amounts are shown in today's dollars).
    price_level: f64,
    lockup_months: i32,
    /// True in the month the lockup ended (a sale then is the lockup-end sale; a
    /// later one is on the yearly schedule).
    lockup_end: bool,
    /// Years since the plan started.
    years: i32,
    /// A forfeiture this month came from leaving a job that had begun (a layoff,
    /// retirement, a planned departure or the job's end). False when the owner
    /// died or the job never started: nobody left.
    left: bool,
}

/// The line the life story shows for an event. Share prices are nominal with two
/// decimals; money is whole dollars in today's terms, and says so wherever it stands
/// beside a nominal price.
fn label(e: &EqEvent, c: &LabelContext) -> String {
    let name = c.company;
    let money = dollars(e.amount / c.price_level);
    let units = grouped(e.units);
    match e.kind {
        EqEventKind::Exercised => format!("Exercised {units} {name} options for {money}"),
        EqEventKind::Tender => {
            if e.units > 0.0 {
                format!(
                    "{name} tender offer at ${:.2} a share: sold {units} for {money} in today's dollars",
                    e.price
                )
            } else {
                format!(
                    "{name} tender offer at ${:.2} a share: sold nothing",
                    e.price
                )
            }
        }
        EqEventKind::Acquired => {
            if e.price > 0.0 {
                format!(
                    "{name} acquired at ${:.2} a share: {money} paid out in today's dollars",
                    e.price
                )
            } else {
                format!("{name} acquired: common stock gets nothing")
            }
        }
        EqEventKind::Ipo => {
            if c.lockup_months > 0 {
                format!(
                    "{name} goes public at ${:.2} a share; shares locked up for {} months",
                    e.price, c.lockup_months
                )
            } else {
                format!("{name} goes public at ${:.2} a share", e.price)
            }
        }
        EqEventKind::Failed | EqEventKind::WrittenOff => {
            let mut text = if e.kind == EqEventKind::Failed {
                format!("{name} shuts down: its shares and options are worth nothing")
            } else {
                format!(
                    "{name} is written off after {} years without a sale or listing",
                    c.years
                )
            };
            if e.amount > 0.0 {
                text.push_str(&format!(" ({money} paid for its shares is lost)"));
            }
            text
        }
        EqEventKind::Forfeited => {
            // Vested units are in the event only for RSUs that require employment.
            let lead = if c.left {
                format!("Left {name}")
            } else {
                name.to_string()
            };
            let vested = grouped(e.vested);
            let unvested = grouped(e.units - e.vested);
            if e.vested <= 0.0 {
                format!("{lead}: {unvested} unvested forfeited")
            } else if e.units - e.vested <= 0.0 {
                format!("{lead}: {vested} vested RSUs forfeited (they required staying until the IPO or sale)")
            } else {
                format!("{lead}: {unvested} unvested forfeited, and {vested} vested RSUs that required staying until the IPO or sale")
            }
        }
        EqEventKind::Lapsed => format!(
            "{units} vested {name} options lapsed unexercised when the exercise window closed"
        ),
        EqEventKind::Expired => {
            let vested = grouped(e.vested);
            let unvested = grouped(e.units - e.vested);
            let what = if e.units - e.vested <= 0.0 {
                format!("{vested} vested")
            } else if e.vested <= 0.0 {
                format!("{unvested} unvested")
            } else {
                format!("{vested} vested and {unvested} unvested")
            };
            format!("{what} {name} options lapsed unexercised at their ten-year term")
        }
        EqEventKind::LockupSale => {
            if c.lockup_end {
                format!(
                    "{name} lockup ends: sold {units} shares at ${:.2} for {money} in today's dollars",
                    e.price
                )
            } else {
                format!("{name}: scheduled sale of {units} shares for {money}")
            }
        }
        EqEventKind::LockupKept => {
            format!("{name}: kept {units} shares worth {money} as single stock")
        }
    }
}

impl State {
    /// One month of every private company: the tender draw, the step, and the
    /// booking of its flows. Called at the start of the careers step, after the
    /// stock index has been updated and before the jobs loop (so a company that
    /// fails this month lays its employee off this month).
    pub(crate) fn equity_month(&mut self, plan: &Plan, econ: &EconomyPath, t: usize) {
        let now = plan.start + t as i32;
        let ti = t as i32;
        self.eq_hold = 0.0;
        self.eq_paper = 0.0;
        // Every company's word for the month, before any branch on state.
        for ci in 0..plan.companies.len() {
            self.companies[ci].u_tender = self.rng_company[ci].uniform();
        }
        let open = window_open(
            econ.recession[t],
            self.eq_index,
            self.eq_peak,
            &plan.equity_params,
        );
        let single_stock_return = econ.returns[AssetClass::SingleStock.index()][t];
        for ci in 0..plan.companies.len() {
            let cp = &plan.companies[ci];
            if matches!(
                self.companies[ci].st.status,
                CompanyStatus::Done | CompanyStatus::Failed
            ) {
                self.companies[ci].sever_next = false;
                continue;
            }
            let owner = cp.owner;
            // Who the income belongs to, and whether the holdings are in the
            // household at all.
            let mut person = owner;
            let mut owner_died = false;
            let mut outside = false;
            if self.present[owner] {
                self.companies[ci].joined = true;
            } else if self.alive[owner] {
                // Not joined yet: the company runs, the household sees nothing.
                outside = true;
            } else {
                let heir = if self.companies[ci].joined {
                    (0..self.n).find(|&o| o != owner && self.present[o])
                } else {
                    None
                };
                match heir {
                    Some(o) => {
                        person = o;
                        if !self.companies[ci].passed_on {
                            self.companies[ci].passed_on = true;
                            owner_died = true;
                        }
                    }
                    None => {
                        // Nobody inherits: the holdings are dropped.
                        let basis = cp.model.abandon(&mut self.companies[ci].st);
                        if self.check {
                            self.chk_eq_flow -= basis;
                        }
                        self.companies[ci].sever_next = false;
                        continue;
                    }
                }
            }

            let (employed, job_started, job_ended) = match cp.job {
                Some(j) if outside => {
                    // The owner has not joined the household, so the job has no
                    // runtime yet: it is held on its own dates, and vesting goes on.
                    let jp = &plan.jobs[j];
                    let started = ti >= jp.own_start;
                    let ended = self.jobs[j].ended || ti >= jp.end.month(self.retire_month);
                    (started && !ended, started, ended)
                }
                Some(j) => {
                    let rt = &self.jobs[j];
                    let jp = &plan.jobs[j];
                    (
                        matches!(
                            rt.st.employment,
                            Employment::Employed | Employment::OnBreak { .. }
                        ),
                        rt.begun,
                        // A job that ends at retirement never starts once its owner
                        // has stopped working.
                        rt.ended
                            || (!rt.begun
                                && jp.ends_at_retirement
                                && self.person_retired[jp.owner]),
                    )
                }
                None => (false, false, true),
            };
            let (liquid, wage_tax_rate, wage_withheld_rate, iso_tax_rate) = if outside {
                (0.0, 0.0, 0.0, 0.0)
            } else {
                // The cash the sweep protects and tax already owed are not spent on
                // strike. Read here, after the companies before this one.
                let reserve = if self.retired {
                    self.retire_cash_cap
                } else {
                    self.efund_target
                } + self.tax_reserve();
                let liquid = (self.acc.cash + self.acc.total(Bucket::Taxable) - reserve).max(0.0);
                // What this month's step could exercise that is taxed as pay. The
                // return is computed (twice) only in a month that has any.
                let candidate = cp
                    .model
                    .pay_spread_at_stake(&self.companies[ci].st, ti)
                    .max(0.0);
                let (_, _, withheld) = self.equity_withholding(person, candidate.max(1.0));
                let withheld_rate = withheld / candidate.max(1.0);
                let tax_rate = if candidate > 0.0 {
                    self.pay_tax_rate(person, candidate).max(withheld_rate)
                } else {
                    withheld_rate
                };
                // A flat rate, no tax, or a household abroad: nothing is withheld at
                // the event and there is no minimum tax.
                let iso_rate = if self.fica_on {
                    self.params.federal.amt_rates[1]
                } else {
                    0.0
                };
                (liquid, tax_rate, withheld_rate, iso_rate)
            };
            let climate = EquityClimate {
                t: ti,
                year: now.year(),
                employed,
                job_started,
                job_ended,
                left_job: self.companies[ci].sever_next,
                owner_died,
                window_open: open,
                u_tender: self.companies[ci].u_tender,
                single_stock_return,
                liquid,
                wage_tax_rate,
                wage_withheld_rate,
                iso_tax_rate,
            };
            self.companies[ci].sever_next = false;
            let link_before = self.companies[ci].st.link;
            let em = cp.model.step(&mut self.companies[ci].st, &climate);
            if self.check {
                self.chk_eq_flow += em.basis_in - em.basis_out;
            }
            // The first failure, sale or listing of the company the outcome
            // statistics describe (a write-off is not one).
            if ci == plan.primary_company && self.eq_event_month == i32::MAX {
                let st = &self.companies[ci].st;
                if st.event_month != i32::MAX
                    && st.outcome.is_some_and(|o| o != EquityOutcome::StillPrivate)
                {
                    self.eq_event_month = st.event_month;
                }
            }
            if em.failed_now && cp.fails_job && link_before == Link::Active {
                if let Some(j) = cp.job {
                    if !self.jobs[j].ended {
                        self.jobs[j].st.force_layoff = true;
                    }
                }
            }
            if outside {
                // Outside the household: only the failure of an employer is seen.
                continue;
            }
            // Someone left only if a job that had begun ended with its holder alive.
            let left = link_before == Link::Active && !owner_died;
            self.book_equity(plan, ci, person, &em, now, econ.price_level[t], left);
        }
    }

    /// Payroll tax and federal withholding taken at the event on `wages` of
    /// wage-type equity income paid to person `p` now: `(payroll tax, of which the
    /// Additional Medicare tax, federal income tax withheld)`. All zero when
    /// nothing is withheld at the event (a flat rate, no tax, a household abroad).
    ///
    /// Payroll takes Social Security up to the year's wage base, Medicare on
    /// everything, and the 0.9% Additional Medicare tax on the person's wages over
    /// the single filer's threshold whatever their filing status (IRC 3102(f)); a
    /// joint return settles the difference. Income tax is withheld at the
    /// supplemental rate, and at the higher one on supplemental pay over the
    /// threshold in the year.
    fn equity_withholding(&self, p: usize, wages: f64) -> (f64, f64, f64) {
        if !self.fica_on || wages <= 0.0 {
            return (0.0, 0.0, 0.0);
        }
        let f = &self.params.federal;
        let before = self.ytd_fica_wages[p];
        let after = before + wages;
        let employer_threshold = f.additional_medicare_threshold[FilingStatus::Single.index()];
        let additional = f.additional_medicare_rate
            * ((after - employer_threshold).max(0.0) - (before - employer_threshold).max(0.0));
        let fica = f.oasdi_rate * (after.min(f.oasdi_wage_base) - before.min(f.oasdi_wage_base))
            + f.medicare_rate * wages
            + additional;
        let room = (f.supplemental_threshold - self.eq_supp_ytd[p]).max(0.0);
        let withheld = f.supplemental_rate * wages.min(room)
            + f.supplemental_rate_high * (wages - room).max(0.0);
        (fica, additional, fica + withheld)
    }

    /// Tax per dollar that `spread` of pay to person `p` would add to this year's
    /// return, everything included (federal and state income tax, payroll tax, the
    /// Additional Medicare tax): the year as last planned, with the one-off equity
    /// amounts so far, with and without the spread. The average over the whole
    /// spread, so it overstates the rate on a part of it.
    fn pay_tax_rate(&self, p: usize, spread: f64) -> f64 {
        let mut without = self.eq_base;
        super::year::add_once(&mut without, &self.ty_once);
        without.amt_credit_carryforward = self.amt_credit;
        let mut with = without;
        with.wages[p] += spread;
        let delta = tax::compute_no_irmaa(&self.params, &with).total
            - tax::compute_no_irmaa(&self.params, &without).total;
        if delta.is_finite() {
            (delta / spread).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Applies one company's month to cash, the ledger, the tax year and the trace.
    #[allow(clippy::too_many_arguments)]
    fn book_equity(
        &mut self,
        plan: &Plan,
        ci: usize,
        p: usize,
        em: &EquityMonth,
        now: YearMonth,
        price: f64,
        left: bool,
    ) {
        let cp = &plan.companies[ci];
        if em.exercise_cost > 0.0 {
            self.pay(YearSeries::EquityExercise, em.exercise_cost);
        }
        if em.cash_in > 0.0 {
            self.recv(YearSeries::IncomeEquity, em.cash_in);
        }
        if em.to_sleeve_value > 0.0 {
            // Shares kept after a lockup: income in kind, saved into the taxable
            // account as single stock with the basis they had.
            let v = em.to_sleeve_value;
            let single = AssetClass::SingleStock.index();
            self.recv(YearSeries::IncomeEquity, v);
            self.acc.cash -= v;
            self.acc.holdings_changed();
            self.acc.taxable.value[single] += v;
            self.acc.taxable.basis[single] += em.to_sleeve_basis;
            self.acc.keep_single_stock = true;
            self.led[YearSeries::SaveTaxable.index()] += v;
            // No tax is due on the move. The gain goes with the shares; the year's
            // close charges the equity with the tax it would bear if sold.
            self.eq_kept_gain += v - em.to_sleeve_basis;
        }

        // Wage-type income: payroll tax and supplemental withholding at the event.
        let wages = em.wages_cash + em.wages_noncash;
        let (mut fica, mut withheld) = (0.0, 0.0);
        if wages > 0.0 {
            self.ty.wages[p] += wages;
            self.ty_once.wages[p] += wages;
            self.year_earned[p] += wages;
            if self.fica_on {
                let (payroll, additional, all) = self.equity_withholding(p, wages);
                fica = payroll;
                withheld = all - payroll;
                self.eq_addl_medicare_ytd += additional;
                self.ytd_fica_wages[p] += wages;
                self.eq_supp_ytd[p] += wages;
                self.pay(YearSeries::TaxPayroll, fica);
                self.paid[TAX_PAYROLL] += fica;
                self.pay(YearSeries::TaxFederal, withheld);
                self.paid[TAX_FED] += withheld;
                self.eq_wh_ytd += withheld;
            }
        }
        // The rest of the tax amounts, each also in the year's one-off tally.
        self.ty.wages_no_fica[p] += em.iso_income;
        self.ty_once.wages_no_fica[p] += em.iso_income;
        self.ty.short_term_gains += em.short_term_gain;
        self.ty_once.short_term_gains += em.short_term_gain;
        self.ty.long_term_gains += em.long_term_gain;
        self.ty_once.long_term_gains += em.long_term_gain;
        self.ty.amt_iso_adjustment += em.amt_adjustment;
        self.ty_once.amt_iso_adjustment += em.amt_adjustment;
        self.ty.amt_gain_adjustment += em.amt_gain_adjustment;
        self.ty_once.amt_gain_adjustment += em.amt_gain_adjustment;
        self.ty.qsbs_excluded_gain += em.qsbs_excluded;
        self.ty_once.qsbs_excluded_gain += em.qsbs_excluded;

        self.eq_gross_real += (em.cash_in + em.to_sleeve_value) / price;
        self.eq_cost_real += em.exercise_cost / price;
        self.eq_tender_real += em.tender_proceeds / price;
        self.eq_paper += em.paper_value;

        // Held out of this month's sweep: the estimated minimum tax on incentive
        // options exercised (and, further down, the net cash of an event).
        self.eq_hold += em.tax_estimate;
        // An exercise is paid for from taxable savings, not from the cash the
        // household keeps: whatever the strike, the tax withheld on the spread and
        // the minimum tax set aside took out of the protected cash (the emergency
        // fund or retirement cash, plus tax owed) is raised by selling taxable
        // investments now. `liquid` counted exactly those savings, so no
        // retirement account is touched.
        if em.exercise_cost > 0.0 {
            let protected = if self.retired {
                self.retire_cash_cap
            } else {
                self.efund_target
            } + self.tax_reserve();
            let spent = em.exercise_cost + fica + withheld + em.tax_estimate;
            let need = (protected - self.acc.cash)
                .max(0.0)
                .min(spent)
                .min(self.acc.total(Bucket::Taxable));
            if need > 0.005 {
                let target = self.target;
                let ex = cover_shortfall(
                    need,
                    plan.profile.retirement.early_access,
                    &mut self.acc,
                    &self.pinputs,
                    &mut self.ty,
                    if plan.plain_mix { None } else { Some(&target) },
                );
                self.ledger_withdrawals(&ex);
            }
        }

        let taxable = wages != 0.0
            || em.iso_income != 0.0
            || em.short_term_gain != 0.0
            || em.long_term_gain != 0.0
            || em.amt_adjustment != 0.0
            || em.amt_gain_adjustment != 0.0
            || em.qsbs_excluded != 0.0;
        // An event with nothing held (a company counted at zero, or one that is
        // sold or listed after everything lapsed) moves no money and no tax: it
        // must not re-plan the year, or listing such a company would change the
        // household's withholding.
        let moved =
            taxable || em.cash_in != 0.0 || em.exercise_cost != 0.0 || em.to_sleeve_value != 0.0;
        if em.replan && moved {
            // The net cash stays out of the sweep until next month's plan has sized
            // the instalments.
            self.eq_replan = true;
            self.eq_hold += (em.cash_in - fica - withheld).max(0.0);
        }
        if taxable {
            self.eq_event_year = true;
            if !em.replan {
                self.eq_unplanned = true;
            }
        }

        if self.detail {
            let st = &self.companies[ci].st;
            let months = (now - plan.start).max(0);
            let context = LabelContext {
                company: &cp.name,
                price_level: price,
                lockup_months: cp.model.lockup_months,
                lockup_end: months <= st.lockup_end,
                years: (months + 6) / 12,
                left,
            };
            for e in em.events() {
                let amount = match e.kind {
                    EqEventKind::Ipo => None,
                    _ => Some(e.amount / price),
                };
                self.event(now, trace_kind(e.kind), amount, || label(e, &context));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Data;
    use crate::equity::{self, draw_fate, ipo_month, EquityParams};
    use crate::model::{
        Account, AccountKind, CompanyStage, DeadlinePolicy, EquityHolding, ExerciseTiming,
        HoldingKind, Longevity, OutcomeKind, OutcomeRow, PrivateCompany, Profile,
        RetirementTrigger, ShockLevel,
    };
    use crate::rng::{Rng, Stream};
    use crate::stats::{PathEnd, Recorder, YearRecord};

    const START: YearMonth = YearMonth::ym(2026, 11);

    struct Null;

    impl Recorder for Null {
        fn begin_path(&mut self, _: u64) {}
        fn record_year(&mut self, _: usize, _: &YearRecord) {}
        fn end_path(&mut self, _: &PathEnd) {}
    }

    /// Keeps the trace events and the tax returns of a path.
    struct Events(Vec<crate::stats::TraceEvent>, Vec<(i32, tax::TaxResult)>);

    impl Recorder for Events {
        fn begin_path(&mut self, _: u64) {}
        fn record_year(&mut self, _: usize, _: &YearRecord) {}
        fn end_path(&mut self, _: &PathEnd) {}
        fn record_event(&mut self, event: crate::stats::TraceEvent) {
            self.0.push(event);
        }
        fn record_tax_return(&mut self, year: i32, detail: &tax::TaxDetail) {
            self.1.push((year, detail.result));
        }
    }

    /// One row that happens for certain in `month` at `multiple`.
    fn certain(kind: OutcomeKind, month: u32, multiple: f64) -> Vec<OutcomeRow> {
        let years = f64::from(month) / 12.0;
        vec![OutcomeRow {
            id: "only".into(),
            kind,
            probability: 1.0,
            years_from: years,
            years_to: years,
            multiple_low: multiple,
            multiple_high: multiple,
        }]
    }

    /// The first demo household with career and health risk and spending shocks off,
    /// $150,000 more in a brokerage account, and one company linked to its job:
    /// 40,000 incentive options at $1 (half vested, the rest over two years) at a
    /// $10 preferred price and a $4 409A value, no tender offers.
    fn household(edit: impl FnOnce(&mut PrivateCompany)) -> Profile {
        let mut p = Profile::examples(START).remove(0).profile;
        p.assumptions.career_risk = false;
        p.assumptions.health_risk = false;
        p.assumptions.longevity = Longevity::FixedAge { age: 85.0 };
        p.spending.shocks = ShockLevel::Off;
        p.events.clear();
        p.accounts.push(Account {
            id: "more".into(),
            name: "More".into(),
            kind: AccountKind::Taxable,
            balance: 150_000.0,
            cost_basis: Some(150_000.0),
            ..Account::default()
        });
        let mut co = PrivateCompany {
            id: "acme".into(),
            name: "Acme".into(),
            job: Some(p.incomes[0].id.clone()),
            stage: CompanyStage::SeriesC,
            preferred_price: 10.0,
            fmv_409a: Some(4.0),
            holdings: vec![EquityHolding {
                id: "iso".into(),
                name: "Options".into(),
                kind: HoldingKind::Iso {
                    strike: 1.0,
                    granted_months_ago: 24,
                },
                shares: 40_000.0,
                vested: 20_000.0,
                vest_months_left: 24,
                cliff_months: 0,
            }],
            ..PrivateCompany::default()
        };
        co.tenders.annual_probability = Some(0.0);
        edit(&mut co);
        p.private_equity = vec![co];
        p
    }

    fn compile(profile: &Profile) -> Plan {
        Plan::compile(profile, Data::embedded()).unwrap_or_else(|r| panic!("{:#?}", r.errors))
    }

    /// A path begun and ready for month 0, with the conservation checks on.
    fn begin(plan: &Plan, seed: u64, path: u64) -> (State, EconomyPath) {
        let mut state = State::new(plan);
        let mut econ = EconomyPath::with_capacity(plan.horizon_months() as usize);
        state.begin(plan, seed, path, &mut econ, false, true);
        (state, econ)
    }

    #[test]
    fn the_preview_and_the_simulation_draw_the_same_fate() {
        // `draw_fate` on the company's stream gives the fate `State::begin` stores,
        // apart from an IPO month that was placed in an open window.
        let profile = household(|_| {});
        let plan = compile(&profile);
        let params = EquityParams::from_data(Data::embedded());
        let table = equity::resolved_table(&profile.private_equity[0], &params);
        let mut state = State::new(&plan);
        let mut econ = EconomyPath::with_capacity(plan.horizon_months() as usize);
        let (mut ipos, mut moved) = (0, 0);
        for path in 0..500 {
            state.begin(&plan, 9, path, &mut econ, false, false);
            let mut rng = Rng::stream(9, path, Stream::PrivateCompany(0));
            let mut drawn = draw_fate(&table, &params, &mut rng);
            let stored = state.companies[0].st.fate;
            if drawn.kind == OutcomeKind::Ipo {
                ipos += 1;
                let placed = ipo_month(&econ, drawn.month, &params);
                moved += u32::from(placed != drawn.month);
                drawn.month = placed;
            }
            assert_eq!(stored, drawn, "path {path}");
            // Exactly four words were taken at path start.
            assert_eq!(state.rng_company[0].next_u64(), rng.next_u64());
            assert_eq!(state.companies[0].st.link, Link::Active);
        }
        assert!(ipos > 40 && moved > 0, "{ipos} IPOs, {moved} moved");
    }

    #[test]
    fn the_company_stream_is_read_once_a_month_whatever_happens() {
        // Four words at the start and one a month, for every company, whatever the
        // household decides and whatever becomes of it: afterwards the stream stands
        // at word 4 + months.
        let variants: Vec<Profile> = vec![
            household(|_| {}),
            household(|c| c.included = false),
            household(|c| {
                c.policy.exercise = ExerciseTiming::VestedNow;
                c.policy.leave_after_months = Some(3);
                c.tenders.annual_probability = Some(1.0);
            }),
            household(|c| c.outcomes = certain(OutcomeKind::Fails, 5, 1.0)),
            household(|c| {
                c.job = None;
                c.holdings[0].vested = 40_000.0;
                c.holdings[0].vest_months_left = 0;
                c.policy.at_deadline = DeadlinePolicy::Forfeit;
            }),
        ];
        for (i, profile) in variants.iter().enumerate() {
            let mut two = profile.clone();
            let mut second = two.private_equity[0].clone();
            second.id = "second".into();
            two.private_equity.push(second);
            let plan = compile(&two);
            for path in 0..6 {
                let (mut state, mut econ) = begin(&plan, 4, path);
                for t in 0..state.months {
                    state.month(&plan, &mut econ, &mut Null, t);
                }
                for c in 0..2 {
                    let mut expected = Rng::stream(4, path, Stream::PrivateCompany(c));
                    for _ in 0..4 + state.months {
                        expected.next_u64();
                    }
                    assert_eq!(
                        state.rng_company[c].next_u64(),
                        expected.next_u64(),
                        "variant {i} path {path} company {c}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_short_layoff_still_severs_the_link() {
        let plan = compile(&household(|_| {}));
        let (mut state, mut econ) = begin(&plan, 3, 0);
        for t in 0..5 {
            state.month(&plan, &mut econ, &mut Null, t);
        }
        assert_eq!(state.companies[0].st.link, Link::Active);
        assert!(state.companies[0].st.holdings[0].unvested > 0.0);
        // A layoff in month 5 (forced, as a failing employer would).
        state.jobs[0].st.force_layoff = true;
        state.month(&plan, &mut econ, &mut Null, 5);
        assert!(state.companies[0].sever_next);
        assert_eq!(state.companies[0].st.link, Link::Active);
        // Back at work within the month: the job is employed again when the equity
        // step next looks, and the link is severed all the same.
        state.jobs[0].st.employment = Employment::Employed;
        state.jobs[0].st.spell_months_left = 0.0;
        state.month(&plan, &mut econ, &mut Null, 6);
        let st = state.companies[0].st;
        assert_eq!(st.link, Link::Severed);
        assert_eq!((st.severed_month, st.deadline), (6, 9));
        assert_eq!(st.holdings[0].unvested, 0.0);
        assert!(st.holdings[0].vested > 20_000.0);
        assert!(!state.companies[0].sever_next);
        // Being re-hired does not bring it back.
        state.month(&plan, &mut econ, &mut Null, 7);
        assert_eq!(state.companies[0].st.link, Link::Severed);
        assert_eq!(state.jobs[0].st.employment, Employment::Employed);
    }

    /// Wealth as the trigger counts it at the end of month `t`, today's dollars.
    fn net_wealth(state: &State, econ: &EconomyPath, t: usize) -> (f64, f64) {
        let gross = state.acc.investable() - state.debt_total();
        let net = gross - state.equity_tax_owed();
        let price = econ.price_level[t + 1];
        (gross / price, net / price)
    }

    #[test]
    fn tax_owed_does_not_count_toward_retiring() {
        // A large payout in December (month 13): the tax beyond what was withheld
        // is assessed in January and paid in April. Until then it is in the bank
        // but it is not wealth: the household reaches its number exactly when its
        // savings net of the reserve do, never on the gross figure.
        let with_target = |amount: f64| {
            let mut profile = household(|c| {
                c.outcomes = certain(OutcomeKind::Acquired, 13, 4.0);
                c.holdings[0].kind = HoldingKind::Rsu {
                    must_be_employed_at_event: false,
                };
                c.holdings[0].shares = 50_000.0;
                c.holdings[0].vested = 50_000.0;
                c.holdings[0].vest_months_left = 0;
            });
            profile.retirement.trigger = RetirementTrigger::FixedNumber {
                amount,
                basis: crate::model::WealthBasis::Investable,
            };
            compile(&profile)
        };
        // First with a number out of reach, to see what January looks like.
        let plan = with_target(1e12);
        let (mut state, mut econ) = begin(&plan, 6, 0);
        for t in 0..=14 {
            state.month(&plan, &mut econ, &mut Null, t);
        }
        let (gross, net) = net_wealth(&state, &econ, 14);
        assert!(
            gross - net > 100_000.0,
            "tax owed in January: {:.0}",
            gross - net
        );
        // Then with a number between the two figures.
        let amount = 0.5 * (gross + net);
        let plan = with_target(amount);
        let (mut held_back, mut gap_months) = (0, 0);
        for path in 0..10 {
            let (mut state, mut econ) = begin(&plan, 6, path);
            let mut funded = false;
            for t in 0..24 {
                state.month(&plan, &mut econ, &mut Null, t);
                if state.retired {
                    break;
                }
                let (gross, net) = net_wealth(&state, &econ, t);
                funded |= net >= amount;
                assert_eq!(
                    state.fi_month != i32::MAX,
                    funded,
                    "path {path} month {t}: net {net:.0}, gross {gross:.0}, target {amount:.0}"
                );
                if (14..=16).contains(&t) {
                    // January to March: the balance of the December payout's tax.
                    assert!(
                        state.equity_tax_owed() > 100_000.0 && state.eq_due,
                        "path {path} month {t}: reserve {:.0}",
                        state.equity_tax_owed()
                    );
                    held_back += 1;
                    gap_months += u32::from(gross >= amount && net < amount);
                }
                if t == 18 {
                    // Paid in April: nothing is held back any more.
                    assert!(
                        state.equity_tax_owed() < 1.0 && !state.eq_due,
                        "path {path}"
                    );
                }
            }
        }
        // (Fewer than thirty: on some paths the market carries the net figure over
        // the number and the household retires.)
        assert!(held_back >= 9, "{held_back}");
        // The number was chosen so that the tax owed decides it: reached on the
        // gross figure, not reached net of the reserve.
        assert!(gap_months >= 1, "the reserve decided {gap_months} months");
    }

    #[test]
    fn a_deadline_exercise_spends_only_spare_savings() {
        // Many vested options and a three-month window: the household exercises
        // what its cash and taxable savings above the emergency fund and tax owed
        // can pay for, strike and tax together, and no more.
        let big = |c: &mut PrivateCompany| {
            c.outcomes = certain(OutcomeKind::StillPrivate, 0, 1.0);
            c.holdings[0].kind = HoldingKind::Iso {
                strike: 2.0,
                granted_months_ago: 24,
            };
            c.holdings[0].shares = 200_000.0;
            c.holdings[0].vested = 200_000.0;
            c.holdings[0].vest_months_left = 0;
        };
        let one = household(big);
        let mut two = one.clone();
        let mut second = two.private_equity[0].clone();
        second.id = "second".into();
        second.holdings[0].kind = HoldingKind::Nso {
            strike: 2.0,
            granted_months_ago: 24,
        };
        two.private_equity.push(second);
        let mut retiring = one.clone();
        retiring.retirement.trigger = RetirementTrigger::AtDate { at: START + 6 };
        // (profile, laid off in month 6?, month of the deadline)
        for (name, profile, layoff, deadline) in [
            ("laid off", &one, true, 10),
            ("two companies", &two, true, 10),
            ("retired", &retiring, false, 9),
        ] {
            let plan = compile(profile);
            for path in 0..8 {
                let (mut state, mut econ) = begin(&plan, 8, path);
                for t in 0..deadline {
                    if layoff && t == 6 {
                        state.jobs[0].st.force_layoff = true;
                    }
                    state.month(&plan, &mut econ, &mut Null, t);
                    assert_eq!(
                        state.led[YearSeries::EquityExercise.index()],
                        0.0,
                        "{name}: exercised before the deadline, month {t}"
                    );
                }
                let before = state.acc.cash + state.acc.total(Bucket::Taxable);
                state.month(&plan, &mut econ, &mut Null, deadline);
                let led = &state.led;
                let strike = led[YearSeries::EquityExercise.index()];
                assert!(strike > 10_000.0, "{name} path {path}: strike {strike:.0}");
                // Not everything could be afforded: the rest lapsed.
                let exercised: f64 = state.companies.iter().map(|rt| rt.st.lot_shares()).sum();
                assert!(
                    exercised > 0.0 && exercised < 200_000.0,
                    "{name}: {exercised}"
                );
                assert!(state
                    .companies
                    .iter()
                    .all(|rt| rt.st.holdings[0].vested == 0.0));
                // A working household pays for it without touching a retirement
                // account (a retired one draws its living from them as planned).
                if layoff {
                    for series in [
                        YearSeries::WithdrawTraditional,
                        YearSeries::WithdrawRoth,
                        YearSeries::WithdrawHsa,
                    ] {
                        assert_eq!(
                            led[series.index()],
                            0.0,
                            "{name} path {path}: {} drawn on",
                            series.key()
                        );
                    }
                }
                assert!(!state.depleted_now, "{name} path {path}");
                // What is left covers the cash reserve and the tax held back, give
                // or take the month's own spending and tax.
                let after = state.acc.cash + state.acc.total(Bucket::Taxable);
                let reserve = if state.retired {
                    state.retire_cash_cap
                } else {
                    state.efund_target
                } + state.tax_reserve();
                let month_flows = crate::stats::spending_of(led) + crate::stats::total_tax_of(led);
                assert!(
                    after + month_flows + 0.03 * before >= reserve,
                    "{name} path {path}: {after:.0} left of {before:.0}, reserve {reserve:.0}"
                );
                // The estimated minimum tax on what was exercised is held back, and
                // the strike came out of taxable savings, not out of the cash kept.
                assert!(state.eq_hold > 0.0, "{name}: nothing held for the AMT");
                assert!(
                    led[YearSeries::WithdrawTaxable.index()] > 0.5 * strike,
                    "{name} path {path}: strike {strike:.0} not funded from taxable savings"
                );
            }
        }
    }

    #[test]
    fn numbers_print_with_separators() {
        assert_eq!(grouped(0.0), "0");
        assert_eq!(grouped(999.4), "999");
        assert_eq!(grouped(1_000.0), "1,000");
        assert_eq!(grouped(1_234_567.5), "1,234,568");
        assert_eq!(grouped(-12_345.0), "-12,345");
        assert_eq!(grouped(f64::NAN), "0");
        assert_eq!(dollars(48_000.2), "$48,000");
    }

    #[test]
    fn every_event_has_a_plain_label() {
        let c = LabelContext {
            company: "Acme",
            price_level: 2.0,
            lockup_months: 6,
            lockup_end: true,
            years: 30,
            left: true,
        };
        let e = |kind, units, price, amount| EqEvent {
            kind,
            units,
            vested: 0.0,
            price,
            amount,
        };
        let split = |kind, units, vested, context: &LabelContext| {
            label(
                &EqEvent {
                    kind,
                    units,
                    vested,
                    price: 10.0,
                    amount: 0.0,
                },
                context,
            )
        };
        let line = |kind, units, price, amount| label(&e(kind, units, price, amount), &c);
        // Amounts are nominal in the event and shown in today's dollars (half here).
        assert_eq!(
            line(EqEventKind::Exercised, 12_000.0, 2.0, 24_000.0),
            "Exercised 12,000 Acme options for $12,000"
        );
        assert_eq!(
            line(EqEventKind::Tender, 1_500.0, 12.5, 37_500.0),
            "Acme tender offer at $12.50 a share: sold 1,500 for $18,750 in today's dollars"
        );
        assert_eq!(
            line(EqEventKind::Tender, 0.0, 12.5, 0.0),
            "Acme tender offer at $12.50 a share: sold nothing"
        );
        assert_eq!(
            line(EqEventKind::Acquired, 10_000.0, 30.0, 600_000.0),
            "Acme acquired at $30.00 a share: $300,000 paid out in today's dollars"
        );
        assert_eq!(
            line(EqEventKind::Acquired, 0.0, 0.0, 0.0),
            "Acme acquired: common stock gets nothing"
        );
        assert_eq!(
            line(EqEventKind::Ipo, 0.0, 41.257, 0.0),
            "Acme goes public at $41.26 a share; shares locked up for 6 months"
        );
        assert_eq!(
            line(EqEventKind::Failed, 0.0, 0.0, 0.0),
            "Acme shuts down: its shares and options are worth nothing"
        );
        assert_eq!(
            line(EqEventKind::Failed, 5_000.0, 0.0, 20_000.0),
            "Acme shuts down: its shares and options are worth nothing ($10,000 paid for its shares is lost)"
        );
        assert_eq!(
            line(EqEventKind::WrittenOff, 0.0, 0.0, 0.0),
            "Acme is written off after 30 years without a sale or listing"
        );
        assert_eq!(
            line(EqEventKind::Forfeited, 36_000.0, 10.0, 0.0),
            "Left Acme: 36,000 unvested forfeited"
        );
        // Vested RSUs that required staying are named as what they are.
        assert_eq!(
            split(EqEventKind::Forfeited, 46_000.0, 10_000.0, &c),
            "Left Acme: 36,000 unvested forfeited, and 10,000 vested RSUs that required staying until the IPO or sale"
        );
        assert_eq!(
            split(EqEventKind::Forfeited, 10_000.0, 10_000.0, &c),
            "Left Acme: 10,000 vested RSUs forfeited (they required staying until the IPO or sale)"
        );
        // Nobody left when the owner died or the job never started.
        let nobody_left = LabelContext { left: false, ..c };
        assert_eq!(
            split(EqEventKind::Forfeited, 36_000.0, 0.0, &nobody_left),
            "Acme: 36,000 unvested forfeited"
        );
        assert_eq!(
            split(EqEventKind::Lapsed, 4_000.0, 4_000.0, &c),
            "4,000 vested Acme options lapsed unexercised when the exercise window closed"
        );
        // At the ten-year term the unvested options go too, and are counted apart.
        assert_eq!(
            split(EqEventKind::Expired, 25_361.0, 15_361.0, &c),
            "15,361 vested and 10,000 unvested Acme options lapsed unexercised at their ten-year term"
        );
        assert_eq!(
            split(EqEventKind::Expired, 4_000.0, 4_000.0, &c),
            "4,000 vested Acme options lapsed unexercised at their ten-year term"
        );
        assert_eq!(
            split(EqEventKind::Expired, 4_000.0, 0.0, &c),
            "4,000 unvested Acme options lapsed unexercised at their ten-year term"
        );
        assert_eq!(
            line(EqEventKind::LockupSale, 2_000.0, 50.0, 100_000.0),
            "Acme lockup ends: sold 2,000 shares at $50.00 for $50,000 in today's dollars"
        );
        assert_eq!(
            line(EqEventKind::LockupKept, 2_000.0, 50.0, 100_000.0),
            "Acme: kept 2,000 shares worth $50,000 as single stock"
        );
        let later = LabelContext {
            lockup_end: false,
            lockup_months: 0,
            ..c
        };
        assert_eq!(
            label(
                &e(EqEventKind::LockupSale, 2_000.0, 50.0, 100_000.0),
                &later
            ),
            "Acme: scheduled sale of 2,000 shares for $50,000"
        );
        assert_eq!(
            label(&e(EqEventKind::Ipo, 0.0, 8.0, 0.0), &later),
            "Acme goes public at $8.00 a share"
        );
        // Every kind maps to one of the seven trace kinds.
        assert_eq!(
            trace_kind(EqEventKind::WrittenOff),
            TraceEventKind::CompanyFailed
        );
        assert_eq!(
            trace_kind(EqEventKind::Lapsed),
            TraceEventKind::EquityForfeited
        );
        assert_eq!(
            trace_kind(EqEventKind::Expired),
            TraceEventKind::EquityForfeited
        );
        assert_eq!(
            trace_kind(EqEventKind::LockupKept),
            TraceEventKind::LockupSale
        );
    }

    /// Runs months `from..=to`, returning the ledger of each.
    fn months(
        plan: &Plan,
        state: &mut State,
        econ: &mut EconomyPath,
        range: std::ops::RangeInclusive<usize>,
        mut each: impl FnMut(usize, &State),
    ) {
        for t in range {
            state.month(plan, econ, &mut Null, t);
            each(t, state);
        }
    }

    /// 200,000 vested non-qualified options at $1 with a 409A value of $9 in a
    /// company that stays private: an exercise is taxed as pay on $8 an option.
    fn many_nsos(c: &mut PrivateCompany) {
        c.outcomes = certain(OutcomeKind::StillPrivate, 0, 1.0);
        c.fmv_409a = Some(9.0);
        c.holdings[0].kind = HoldingKind::Nso {
            strike: 1.0,
            granted_months_ago: 24,
        };
        c.holdings[0].shares = 200_000.0;
        c.holdings[0].vested = 200_000.0;
        c.holdings[0].vest_months_left = 0;
    }

    #[test]
    fn an_nso_exercise_is_sized_from_its_tax_not_its_withholding() {
        // Withholding on the spread is 22% and the tax is nearer 40%. Sized from the
        // withholding, the exercise took every spare dollar and the rest of the tax
        // then emptied the household: debt, and spending cut to income. Sized from
        // the tax, the household comes through the exercise year and the April
        // after it with its spending and its emergency fund intact.
        let rich = |edit: fn(&mut PrivateCompany)| {
            let mut p = household(edit);
            p.accounts.push(Account {
                id: "much".into(),
                name: "Much more".into(),
                kind: AccountKind::Taxable,
                balance: 450_000.0,
                cost_basis: Some(450_000.0),
                ..Account::default()
            });
            p
        };
        let at_deadline = rich(|c| {
            many_nsos(c);
            c.policy.leave_after_months = Some(2);
        });
        let now = rich(|c| {
            many_nsos(c);
            c.policy.exercise = ExerciseTiming::VestedNow;
        });
        for (name, profile, exercise_month) in [("deadline", &at_deadline, 5), ("now", &now, 0)] {
            let plan = compile(profile);
            for path in 0..6 {
                let (mut state, mut econ) = begin(&plan, 12, path);
                let debt0 = state.debt_total();
                let mut exercised = 0.0;
                let mut tax_paid = 0.0;
                // To May 2028 (month 18): the exercise year or the one after, and
                // the April in which its return is settled.
                months(&plan, &mut state, &mut econ, 0..=18, |t, state| {
                    let led = &state.led;
                    let strike = led[YearSeries::EquityExercise.index()];
                    if t == exercise_month {
                        assert!(strike > 20_000.0, "{name} path {path}: strike {strike:.0}");
                        exercised = state.companies[0].st.lot_shares();
                        // The tax beyond what was withheld is set aside at once.
                        assert!(
                            state.eq_hold > 0.1 * 8.0 * exercised,
                            "{name} path {path}: held {:.0} on a spread of {:.0}",
                            state.eq_hold,
                            8.0 * exercised
                        );
                    } else {
                        assert_eq!(strike, 0.0, "{name} path {path} month {t}");
                    }
                    tax_paid += crate::stats::total_tax_of(led);
                    assert!(
                        !state.depleted_now,
                        "{name} path {path} month {t}: depleted"
                    );
                    assert!(
                        state.debt_total() <= debt0 + 1.0,
                        "{name} path {path} month {t}: debt {:.0} from {debt0:.0}",
                        state.debt_total()
                    );
                    assert_eq!(
                        led[YearSeries::WithdrawTraditional.index()]
                            + led[YearSeries::WithdrawRoth.index()],
                        0.0,
                        "{name} path {path} month {t}: a retirement account was drawn on"
                    );
                    // Spending is never cut below the plan.
                    assert!(
                        crate::stats::spending_of(led) >= 0.999 * state.planned,
                        "{name} path {path} month {t}: spent {:.0} of {:.0}",
                        crate::stats::spending_of(led),
                        state.planned
                    );
                });
                // Part of the grant, not all of it: about $600,000 of savings
                // cannot pay $200,000 of strike and the tax on $1.6 million. (Sized
                // from the 22% withheld it bought about 170,000 options.)
                assert!(
                    exercised > 20_000.0 && exercised < 155_000.0,
                    "{name} path {path}: {exercised:.0} exercised"
                );
                // The tax did come: well over the 22% that was withheld.
                assert!(
                    tax_paid > 0.30 * 8.0 * exercised,
                    "{name} path {path}: tax {tax_paid:.0} on a spread of {:.0}",
                    8.0 * exercised
                );
            }
        }
    }

    #[test]
    fn additional_medicare_tax_on_equity_pay_is_withheld_at_the_event() {
        // 50,000 vested RSUs paid out at $40 in July 2027 (month 8): $2 million of
        // pay. The 0.9% on it over $200,000 is taken with the rest of the payroll
        // tax that month, not left for the following April.
        let plan = compile(&household(|c| {
            c.outcomes = certain(OutcomeKind::Acquired, 8, 4.0);
            c.holdings[0].kind = HoldingKind::Rsu {
                must_be_employed_at_event: false,
            };
            c.holdings[0].shares = 50_000.0;
            c.holdings[0].vested = 50_000.0;
            c.holdings[0].vest_months_left = 0;
        }));
        for path in 0..4 {
            let (mut state, mut econ) = begin(&plan, 14, path);
            let mut payroll = [0.0; 19];
            let mut withheld_ytd = 0.0;
            months(&plan, &mut state, &mut econ, 0..=18, |t, state| {
                payroll[t] = state.led[YearSeries::TaxPayroll.index()];
                if t == 8 {
                    withheld_ytd = state.eq_addl_medicare_ytd;
                }
            });
            let f = &state.params.federal;
            // The month of the payout: 0.9% of the pay above the threshold, the
            // salary so far having used part of it.
            assert!(
                withheld_ytd > f.additional_medicare_rate * (2_000_000.0 - 200_000.0)
                    && withheld_ytd <= f.additional_medicare_rate * 2_000_000.0,
                "path {path}: {withheld_ytd:.0} withheld"
            );
            assert!(payroll[8] > payroll[7] + withheld_ytd, "path {path}");
            // April 2028 (month 17) settles the 2027 return. What payroll tax is left
            // is the 0.9% on salary earned after the payout, which no payroll
            // withholds in this model: a small figure, not $16,000.
            let april_extra = payroll[17] - payroll[16];
            assert!(
                april_extra.abs() < 0.1 * withheld_ytd,
                "path {path}: {april_extra:.0} more payroll tax in April, {withheld_ytd:.0} was withheld in July"
            );
            // Withheld once: the year's payroll tax is what the return says.
            assert_eq!(state.eq_addl_medicare_ytd, 0.0, "reset at the year's close");
        }
    }

    #[test]
    fn shares_kept_as_single_stock_are_charged_the_tax_on_their_gain() {
        // 20,000 shares bought at $1, listed at $30 in month 3 with no lockup. One
        // plan sells them, the other keeps them as single stock. Kept shares enter
        // the equity's proceeds at their value untaxed, so the tax their gain would
        // bear is charged to the equity: otherwise keeping looks better after tax
        // than selling by the whole deferred tax.
        let shares = |hold: bool| {
            compile(&household(move |c| {
                c.outcomes = certain(OutcomeKind::Ipo, 3, 3.0);
                c.lockup_months = 0;
                c.holdings[0].kind = HoldingKind::Shares {
                    basis_per_share: 1.0,
                    months_held: 30,
                    from_iso: false,
                    fmv_at_exercise: 0.0,
                };
                c.holdings[0].shares = 20_000.0;
                if hold {
                    c.policy.sell_down = crate::model::SellDown::Hold;
                }
            }))
        };
        let (sell, hold) = (shares(false), shares(true));
        for path in 0..4 {
            let run = |plan: &Plan| {
                let (mut state, mut econ) = begin(plan, 15, path);
                // To February 2028: the 2027 return, with the sale or the move, is closed.
                months(plan, &mut state, &mut econ, 0..=15, |_, _| {});
                (state.eq_gross_real, state.eq_tax_real, state.eq_kept_gain)
            };
            let (sold_gross, sold_tax, _) = run(&sell);
            let (kept_gross, kept_tax, pending) = run(&hold);
            assert_eq!(pending, 0.0, "charged at the year's close");
            assert!(
                (sold_gross - kept_gross).abs() < 1e-6 * sold_gross,
                "path {path}: {sold_gross:.0} sold, {kept_gross:.0} kept"
            );
            assert!(
                sold_tax > 0.15 * sold_gross,
                "path {path}: tax {sold_tax:.0} on {sold_gross:.0}"
            );
            // (Not to the dollar: the plan that sold also holds the cash.)
            assert!(
                (kept_tax - sold_tax).abs() < 0.10 * sold_tax,
                "path {path}: tax {sold_tax:.0} when sold, {kept_tax:.0} charged when kept"
            );
        }
    }

    #[test]
    fn equity_vests_while_its_owner_has_yet_to_join_the_household() {
        // The second demo household, with the second person joining in January
        // 2029 (month 26) and holding options that vest over four years in the job
        // they already have. The job has no runtime until they join; the vesting
        // goes on all the same, and nothing is paid, exercised or forfeited.
        let with_joins = |joins: Option<YearMonth>| {
            let mut p = Profile::examples(START).remove(1).profile;
            p.assumptions.career_risk = false;
            p.assumptions.health_risk = false;
            p.spending.shocks = ShockLevel::Off;
            p.household.people[1].joins = joins;
            let job = p
                .incomes
                .iter()
                .find(|i| i.owner == 1 && matches!(i.kind, crate::model::IncomeKind::Job(_)))
                .expect("the second person has a job")
                .id
                .clone();
            let mut co = PrivateCompany {
                id: "acme".into(),
                name: "Acme".into(),
                owner: 1,
                job: Some(job),
                stage: CompanyStage::SeriesC,
                preferred_price: 10.0,
                fmv_409a: Some(4.0),
                outcomes: certain(OutcomeKind::Acquired, 40, 3.0),
                holdings: vec![EquityHolding {
                    id: "iso".into(),
                    name: "Options".into(),
                    kind: HoldingKind::Iso {
                        strike: 1.0,
                        granted_months_ago: 0,
                    },
                    shares: 48_000.0,
                    vested: 0.0,
                    vest_months_left: 48,
                    cliff_months: 0,
                }],
                ..PrivateCompany::default()
            };
            co.tenders.annual_probability = Some(0.0);
            p.private_equity = vec![co];
            p
        };
        let there = compile(&with_joins(None));
        let later = compile(&with_joins(Some(START + 26)));
        let (mut a, mut econ_a) = begin(&there, 16, 0);
        let (mut b, mut econ_b) = begin(&later, 16, 0);
        assert_eq!(
            b.companies[0].st.link,
            Link::Active,
            "held on its own dates"
        );
        assert!(!b.present[1]);
        for t in 0..=39 {
            a.month(&there, &mut econ_a, &mut Null, t);
            b.month(&later, &mut econ_b, &mut Null, t);
            let (ha, hb) = (a.companies[0].st.holdings[0], b.companies[0].st.holdings[0]);
            assert_eq!(
                (ha.vested, ha.unvested),
                (hb.vested, hb.unvested),
                "month {t}: the schedule does not wait for the join"
            );
            assert_eq!(b.companies[0].st.link, Link::Active, "month {t}");
            assert_eq!(b.present[1], t >= 26, "month {t}");
            assert_eq!(b.eq_gross_real, 0.0);
        }
        assert_eq!(a.companies[0].st.holdings[0].vested, 40_000.0);
        // The sale in month 40 pays both households the same.
        a.month(&there, &mut econ_a, &mut Null, 40);
        b.month(&later, &mut econ_b, &mut Null, 40);
        let paid = |s: &State| s.led[YearSeries::IncomeEquity.index()];
        assert!(paid(&a) > 1_000_000.0, "{}", paid(&a));
        assert_eq!(paid(&a), paid(&b));
    }

    #[test]
    fn a_first_part_year_takes_the_minimum_tax_on_contributions_as_they_are() {
        // The plan starts in November. 200,000 incentive options are exercised in
        // the first month as far as savings allow, in a year of two simulated months.
        // The regular income is annualised (times six) and scaled back; the minimum
        // tax of the exercise is taken whole, so the base it stands on must deduct
        // a contribution made in those months once, not six times.
        let plan = compile(&household(|c| {
            c.outcomes = certain(OutcomeKind::StillPrivate, 0, 1.0);
            c.holdings[0].shares = 200_000.0;
            c.holdings[0].vested = 200_000.0;
            c.holdings[0].vest_months_left = 0;
            c.policy.exercise = ExerciseTiming::VestedNow;
        }));
        let close = |contribution: f64| {
            let mut state = State::new(&plan);
            let mut econ = EconomyPath::with_capacity(plan.horizon_months() as usize);
            state.begin(&plan, 17, 0, &mut econ, true, true);
            state.month(&plan, &mut econ, &mut Null, 0);
            state.month(&plan, &mut econ, &mut Null, 1);
            // (As many as savings pay for: a spread of some hundreds of thousands.)
            assert!(state.ty_once.amt_iso_adjustment > 100_000.0);
            // What was deducted in November and December, set by hand so that the
            // two runs differ in nothing else.
            state.ty.adjustments = contribution;
            let mut returns = Events(Vec::new(), Vec::new());
            state.month(&plan, &mut econ, &mut returns, 2);
            let (year, result) = *returns.1.last().expect("a return");
            assert_eq!(year, 2026);
            (result, state.amt_credit)
        };
        let (none, credit_none) = close(0.0);
        let (some, credit_some) = close(7_500.0);
        assert!(none.amt > 10_000.0, "AMT {:.0}", none.amt);
        // The return the exercise stands on: income lower by the contribution.
        assert!(
            (none.agi - some.agi - 7_500.0).abs() < 1.0,
            "AGI {:.0} without and {:.0} with a $7,500 contribution",
            none.agi,
            some.agi
        );
        // The AMT moves by the contribution at AMT rates less the regular tax it
        // saves, not by six times that: between 0 and 35% of $7,500.
        let change = none.amt - some.amt;
        assert!(
            change.abs() < 0.35 * 7_500.0,
            "AMT {:.0} without, {:.0} with",
            none.amt,
            some.amt
        );
        assert!((credit_none - credit_some).abs() < 0.35 * 7_500.0);
    }

    #[test]
    fn a_re_hire_after_leaving_the_company_is_at_a_new_employer() {
        // The company fails in month 5 and lays its employee off. The income stands
        // for whatever job is held next: the re-hire is not back at the company.
        let mut profile = household(|c| c.outcomes = certain(OutcomeKind::Fails, 5, 1.0));
        profile.assumptions.career_risk = true;
        let plan = compile(&profile);
        let job = plan.jobs[0].name.clone();
        let mut found = 0;
        for path in 0..4 {
            let mut state = State::new(&plan);
            let mut econ = EconomyPath::with_capacity(plan.horizon_months() as usize);
            state.begin(&plan, 18, path, &mut econ, true, true);
            let mut events = Events(Vec::new(), Vec::new());
            for t in 0..60 {
                state.month(&plan, &mut econ, &mut events, t);
            }
            let labels: Vec<&str> = events
                .0
                .iter()
                .filter(|e| e.kind == TraceEventKind::Rehired)
                .map(|e| e.label.as_str())
                .collect();
            for label in labels {
                assert_eq!(
                    label,
                    format!("Back at work at a new employer (was {job})"),
                    "path {path}"
                );
                found += 1;
            }
        }
        assert!(found >= 2, "{found} re-hires in four lives");
    }

    #[test]
    fn the_preview_deflates_at_the_engines_own_inflation() {
        // The editor's preview states payouts in today's dollars at one yearly
        // rate; the simulation deflates by its own price path. The rate is the one
        // that reproduces the median simulated price level ten years out, so the
        // two screens do not differ by inflation alone.
        let plan = compile(&household(|_| {}));
        let params = EquityParams::from_data(Data::embedded());
        let mut state = State::new(&plan);
        let mut econ = EconomyPath::with_capacity(plan.horizon_months() as usize);
        let mut levels: Vec<f64> = (0..2_000)
            .map(|path| {
                state.begin(&plan, 19, path, &mut econ, false, false);
                econ.price_level[120]
            })
            .collect();
        levels.sort_by(f64::total_cmp);
        let median = levels[levels.len() / 2];
        let preview = crate::math::powi(1.0 + params.preview_inflation, 10);
        assert!(
            (median / preview - 1.0).abs() < 0.015,
            "price level after ten years: {median:.4} at the median of the simulation, {preview:.4} in the preview"
        );
    }
}
