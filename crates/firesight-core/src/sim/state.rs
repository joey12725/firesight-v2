//! Everything that changes along one path, and how a path starts.
//!
//! The state lives in the [`super::Workspace`] and is reset, not rebuilt, for every
//! path: every vector keeps its allocation.

use crate::accounts::{Accounts, SavingsContext, Weights};
use crate::benefits::{BenefitBasis, Claimant, EarningsRecord, MonthlyBenefits};
use crate::career::{JobState, SelfEmploymentState};
use crate::decumulation::{PlanInputs, SpendingState, WithdrawalPlan};
use crate::economy::{EconomyPath, EconomyStreams, ResolvedStress};
use crate::equity::{CompanyState, Link};
use crate::health::{CoveragePhase, HealthState};
use crate::housing::PropertyState;
use crate::model::{EmployerPlan, YearMonth};
use crate::rng::{Rng, Stream};
use crate::stats::{TraceEvent, TraceEventKind, YearRecord, YearSeries};
use crate::tax::{
    FilingStatus, Indexation, LocalTax, StateCode, TaxDetail, TaxInputs, TaxYearParams,
};
use crate::MAX_PEOPLE;

use super::plan::{Plan, Sched, StressPlan, TriggerPlan};
use super::trigger::Requirement;

/// Number of [`YearSeries`].
pub(crate) const N_SERIES: usize = YearSeries::COUNT;

/// Tax categories in which payments and liabilities are tracked.
pub(crate) const TAX_FED: usize = 0;
pub(crate) const TAX_STATE: usize = 1;
pub(crate) const TAX_PAYROLL: usize = 2;
pub(crate) const TAX_PENALTY: usize = 3;
pub(crate) const TAX_SERIES: [YearSeries; 4] = [
    YearSeries::TaxFederal,
    YearSeries::TaxState,
    YearSeries::TaxPayroll,
    YearSeries::TaxPenalties,
];

/// A job on a path.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct JobRt {
    pub st: JobState,
    pub plan: Option<EmployerPlan>,
    pub begun: bool,
    pub ended: bool,
}

/// A self-employment income on a path.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SeRt {
    pub st: SelfEmploymentState,
    pub scale: f64,
    pub ended: bool,
}

/// A pension, annuity or other income on a path.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StreamRt {
    pub scale: f64,
    pub ended: bool,
    pub started: bool,
    /// Price level in the month payments began (fixes a nominal pension).
    pub start_price: f64,
    /// Growth accumulated since it began (partial COLA) or since the plan's start
    /// (real growth of other income).
    pub level: f64,
    /// Nominal amount paid last month, for projections.
    pub last: f64,
}

/// A private company on a path.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CompanyRt {
    pub st: CompanyState,
    /// This month's word from the company's stream, as a uniform.
    pub u_tender: f64,
    /// The owner has been in the household at some point (so there is something to
    /// inherit when they die).
    pub joined: bool,
    /// The owner has died and the holdings have passed to the survivor.
    pub passed_on: bool,
    /// The owner was laid off from the linked job since the last equity step.
    pub sever_next: bool,
}

/// A debt on a path.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DebtRt {
    pub balance: f64,
    pub rate: f64,
    pub payment: f64,
    pub revolving: bool,
}

/// A life event on a path.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EventRt {
    /// Month it fires; `i32::MAX` while it waits for the retirement date.
    pub month: i32,
    /// Timing shift drawn for this path, months.
    pub shift: i32,
    pub done: bool,
    /// Months a purchase has been postponed for lack of funds.
    pub retries: u32,
}

/// The whole state of one path.
#[derive(Clone, Debug)]
pub(crate) struct State {
    // Run identity.
    pub detail: bool,
    pub check: bool,
    /// Months this path runs.
    pub months: usize,
    // Random streams.
    pub rng_career: [Rng; MAX_PEOPLE],
    pub rng_equity: Rng,
    pub rng_health: [Rng; MAX_PEOPLE],
    pub rng_shocks: Rng,
    pub rng_housing: Vec<Rng>,
    /// One stream per private company: four words at path start, one a month.
    pub rng_company: Vec<Rng>,
    // People.
    pub n: usize,
    pub death_month: [u32; MAX_PEOPLE],
    pub alive: [bool; MAX_PEOPLE],
    /// Alive and in the household.
    pub present: [bool; MAX_PEOPLE],
    pub person_retired: [bool; MAX_PEOPLE],
    pub health: [HealthState; MAX_PEOPLE],
    pub last_phase: [CoveragePhase; MAX_PEOPLE],
    pub in_facility: [bool; MAX_PEOPLE],
    pub records: [EarningsRecord; MAX_PEOPLE],
    pub claimants: [Option<Claimant>; MAX_PEOPLE],
    pub ss_claimed: [bool; MAX_PEOPLE],
    /// First month Social Security can pay anyone.
    pub ss_from: usize,
    /// Last computed benefits (nominal at `ss_cache_price`) and when to recompute.
    pub ss_cache: MonthlyBenefits,
    pub ss_cache_price: f64,
    pub ss_next_change: i32,
    pub ss_dirty: bool,
    /// True once one partner of a couple has died.
    pub survivor: bool,
    /// Calendar year in which a partner died (joint return that year).
    pub death_year: i32,
    // Balance sheet.
    pub acc: Accounts,
    pub jobs: Vec<JobRt>,
    pub selfemp: Vec<SeRt>,
    pub streams: Vec<StreamRt>,
    pub props: Vec<PropertyState>,
    pub debts: Vec<DebtRt>,
    pub companies: Vec<CompanyRt>,
    pub freed_payments: f64,
    /// False once every debt is repaid (a financed expense switches it back on).
    pub debts_active: bool,
    // Budget.
    pub item_scale: Vec<f64>,
    pub ess_real: f64,
    pub disc_real: f64,
    pub next_item_change: i32,
    pub budget_dirty: bool,
    /// Rent paid after selling a home, today's dollars per month.
    pub extra_rent: f64,
    pub creep: f64,
    pub income_hwm: f64,
    pub smile: f64,
    pub smile_target: f64,
    pub disc_mult: f64,
    pub service_growth: f64,
    pub kids_until: i32,
    /// Cached modelled child cost per month, today's dollars (negative: stale).
    pub kids_real: f64,
    // Location and tax law.
    pub tax_state: StateCode,
    pub in_us: bool,
    pub foreign_rate: Option<f64>,
    pub local: LocalTax,
    pub index: Indexation,
    pub anchor_cpi: f64,
    pub anchor_price: f64,
    pub prev_cpi_factor: f64,
    pub params: TaxYearParams,
    /// Filing statuses (bit `FilingStatus::index()`) whose tables in `params` have
    /// been indexed for the year, and the state `params` was built for.
    pub law_ready: u8,
    pub law_state: StateCode,
    pub fica_on: bool,
    // The tax year in progress.
    pub ty: TaxInputs,
    pub paid: [f64; 4],
    pub due: [f64; 4],
    pub ytd_fica_wages: [f64; MAX_PEOPLE],
    pub year_earned: [f64; MAX_PEOPLE],
    /// Months of the current calendar year inside the simulation.
    pub year_months: u32,
    pub loss_carry: f64,
    pub rental_carry: f64,
    pub magi_hist: [f64; 2],
    pub magi_known: u32,
    pub prev_interest: f64,
    pub prev_ordinary_div: f64,
    pub prev_qualified_div: f64,
    pub prev_rental_net: f64,
    pub prev_mortgage_interest: f64,
    pub prev_property_tax: f64,
    pub wh_rate: f64,
    pub installment: f64,
    pub share_state: f64,
    pub share_se: f64,
    pub aca_advance_m: f64,
    /// Premium credit expected per month at the last plan, today's dollars.
    pub aca_guess_real: f64,
    pub aca_advance_ytd: f64,
    pub aca_premium_ytd: f64,
    pub aca_people: u32,
    pub irmaa_m: [f64; MAX_PEOPLE],
    pub filing: FilingStatus,
    pub age_ye: [f64; MAX_PEOPLE],
    // Policies in force.
    pub target: Weights,
    pub efund_target: f64,
    pub sctx: SavingsContext,
    pub has_plan: bool,
    /// True until a plan made this month has had its first instalment executed.
    pub plan_fresh: bool,
    pub wplan: WithdrawalPlan,
    pub pinputs: PlanInputs,
    pub exec_frac: f64,
    pub sp_state: SpendingState,
    pub planner_started: bool,
    pub rmd_started: [bool; MAX_PEOPLE],
    pub sepp: [f64; MAX_PEOPLE],
    pub sepp_until: [i32; MAX_PEOPLE],
    pub retire_cash_cap: f64,
    /// True once the taxable account has been left out of a rebalance to protect
    /// the premium credit (see `State::rebalance`).
    pub sheltered_rebalancing: bool,
    // Retirement.
    pub retired: bool,
    pub retire_month: i32,
    pub fi_month: i32,
    pub coast_month: i32,
    pub req: Requirement,
    pub req_dirty: bool,
    pub inv_at_ret: f64,
    pub funded_ratio: f64,
    pub ret_index: f64,
    pub ret_months: u32,
    pub initial_wr: f64,
    // Events and stress.
    pub events: Vec<EventRt>,
    pub next_event: i32,
    // Market trackers.
    pub home_index: f64,
    pub eq_ring: [f64; 12],
    pub eq12: f64,
    pub eq_index: f64,
    pub eq_peak: f64,
    pub port_index: f64,
    pub port_peak: f64,
    pub worst_dd: f64,
    pub year_port: f64,
    pub prior_return_negative: bool,
    pub price_jan: f64,
    pub prior_inflation: f64,
    pub coverage_gap: u32,
    // Depletion.
    pub depleted_now: bool,
    pub depleted_year: bool,
    pub ever_depleted: bool,
    pub depletion_month: i32,
    /// Month property was first sold because savings had run out.
    pub home_sale_month: i32,
    /// Month each person's Social Security benefit was first paid.
    pub ss_first_paid: [i32; MAX_PEOPLE],
    /// Month each person stopped working.
    pub stopped_work: [i32; MAX_PEOPLE],
    /// True once the household has owed a mortgage.
    pub had_mortgage: bool,
    /// Month the last mortgage was paid off (`i32::MAX` while one is owed).
    pub mortgage_payoff_month: i32,
    pub ltc_real: f64,
    /// Long-term care cost so far when the money first ran out.
    pub ltc_at_depletion: f64,
    /// Months of unemployment so far when the money first ran out.
    pub unemployed_at_depletion: f64,
    // The month and year being recorded.
    pub led: [f64; N_SERIES],
    pub planned: f64,
    pub cash_start: f64,
    pub yr: YearRecord,
    pub yr_months: u8,
    pub last_spend: f64,
    pub wage_like: f64,
    pub port_return: f64,
    // Private-company equity (all unused when the plan has none).
    /// Copy of `Plan::has_equity`.
    pub has_equity: bool,
    /// The year's one-off equity amounts, which are also in `ty`: a partial first
    /// year is annualised without them.
    pub ty_once: TaxInputs,
    /// Minimum tax credit carried into the current year, nominal.
    pub amt_credit: f64,
    /// Wage-type equity income so far this year per person (the supplemental
    /// withholding threshold).
    pub eq_supp_ytd: [f64; MAX_PEOPLE],
    /// Federal tax withheld on equity income so far this year.
    pub eq_wh_ytd: f64,
    /// Additional Medicare tax withheld on equity income so far this year.
    pub eq_addl_medicare_ytd: f64,
    /// Untaxed gain of shares moved to the single-stock sleeve this year (value
    /// less basis): the year's close charges the equity with the tax it would bear.
    pub eq_kept_gain: f64,
    /// The year as the last annual plan expected it, without the one-off equity
    /// amounts and at the scale of the return (annualised in a part year): the base
    /// on which the tax of an exercise is priced.
    pub eq_base: TaxInputs,
    /// Cash held out of this month's sweep: net proceeds of the month's events and
    /// the estimated tax on incentive options exercised.
    pub eq_hold: f64,
    /// Instalments still to pay in a year with an equity event.
    pub eq_reserve: f64,
    /// An event this month warrants re-planning the year next month.
    pub eq_replan: bool,
    /// Small taxable amounts since the last plan (swept into a re-plan each quarter).
    pub eq_unplanned: bool,
    /// The year has had a taxable equity event.
    pub eq_event_year: bool,
    /// The balance due next April is for a year that had one.
    pub eq_due: bool,
    /// Paper value of vested holdings after this month's step, nominal.
    pub eq_paper: f64,
    /// Lifetime totals, today's dollars: cash and share value received, strike
    /// paid, the part received in tenders, and the tax the equity caused.
    pub eq_gross_real: f64,
    pub eq_cost_real: f64,
    pub eq_tender_real: f64,
    pub eq_tax_real: f64,
    /// Month of the primary company's failure, sale or listing; `i32::MAX` if none.
    pub eq_event_month: i32,
    // Conservation checks (check mode).
    pub chk_eq_start: f64,
    pub chk_eq_flow: f64,
    pub chk_other_start: f64,
    pub chk_prop_start: f64,
    pub chk_mort_start: f64,
    pub chk_debt_start: f64,
    pub chk_ret_other: f64,
    pub chk_prop_flow: f64,
    pub chk_mort_flow: f64,
    pub chk_debt_flow: f64,
    // Trace buffers (detail mode only).
    pub trace_events: Vec<TraceEvent>,
    pub tax_details: Vec<(i32, TaxDetail)>,
}

impl State {
    /// Buffers sized for a plan.
    pub(crate) fn new(plan: &Plan) -> State {
        let rng = Rng::from_seed(0);
        State {
            detail: false,
            check: false,
            months: 0,
            rng_career: [rng.clone(), rng.clone()],
            rng_equity: rng.clone(),
            rng_health: [rng.clone(), rng.clone()],
            rng_shocks: rng.clone(),
            rng_housing: vec![rng.clone(); plan.props.len()],
            rng_company: vec![rng.clone(); plan.companies.len()],
            n: plan.people.len(),
            death_month: [0; MAX_PEOPLE],
            alive: [false; MAX_PEOPLE],
            present: [false; MAX_PEOPLE],
            person_retired: [false; MAX_PEOPLE],
            health: [HealthState::default(); MAX_PEOPLE],
            last_phase: [CoveragePhase::None; MAX_PEOPLE],
            in_facility: [false; MAX_PEOPLE],
            records: [EarningsRecord::default(); MAX_PEOPLE],
            claimants: [None; MAX_PEOPLE],
            ss_claimed: [false; MAX_PEOPLE],
            ss_from: 0,
            ss_cache: MonthlyBenefits::default(),
            ss_cache_price: 1.0,
            ss_next_change: 0,
            ss_dirty: true,
            survivor: false,
            death_year: i32::MIN,
            acc: Accounts::default(),
            jobs: vec![JobRt::default(); plan.jobs.len()],
            selfemp: vec![
                SeRt {
                    st: SelfEmploymentState::default(),
                    scale: 1.0,
                    ended: false,
                };
                plan.selfemp.len()
            ],
            streams: vec![
                StreamRt {
                    scale: 1.0,
                    ended: false,
                    started: false,
                    start_price: 1.0,
                    level: 1.0,
                    last: 0.0,
                };
                plan.streams.len()
            ],
            props: vec![PropertyState::default(); plan.props.len()],
            debts: vec![DebtRt::default(); plan.debts.len()],
            companies: vec![CompanyRt::default(); plan.companies.len()],
            freed_payments: 0.0,
            debts_active: true,
            item_scale: vec![1.0; plan.items.len()],
            ess_real: 0.0,
            disc_real: 0.0,
            next_item_change: i32::MAX,
            budget_dirty: true,
            extra_rent: 0.0,
            creep: 1.0,
            income_hwm: 0.0,
            smile: 1.0,
            smile_target: 0.0,
            disc_mult: 1.0,
            service_growth: 1.0,
            kids_until: 0,
            kids_real: -1.0,
            tax_state: StateCode::NONE,
            in_us: true,
            foreign_rate: None,
            local: LocalTax::default(),
            index: Indexation::base(plan.start.year()),
            anchor_cpi: 1.0,
            anchor_price: 1.0,
            prev_cpi_factor: 1.0,
            params: TaxYearParams::default(),
            law_ready: 0,
            law_state: StateCode::NONE,
            fica_on: false,
            ty: TaxInputs::default(),
            paid: [0.0; 4],
            due: [0.0; 4],
            ytd_fica_wages: [0.0; MAX_PEOPLE],
            year_earned: [0.0; MAX_PEOPLE],
            year_months: 12,
            loss_carry: 0.0,
            rental_carry: 0.0,
            magi_hist: [0.0; 2],
            magi_known: 0,
            prev_interest: 0.0,
            prev_ordinary_div: 0.0,
            prev_qualified_div: 0.0,
            prev_rental_net: 0.0,
            prev_mortgage_interest: 0.0,
            prev_property_tax: 0.0,
            wh_rate: 0.0,
            installment: 0.0,
            share_state: 0.0,
            share_se: 0.0,
            aca_advance_m: 0.0,
            aca_guess_real: 0.0,
            aca_advance_ytd: 0.0,
            aca_premium_ytd: 0.0,
            aca_people: 0,
            irmaa_m: [0.0; MAX_PEOPLE],
            filing: FilingStatus::Single,
            age_ye: [0.0; MAX_PEOPLE],
            target: plan.target0,
            efund_target: 0.0,
            sctx: SavingsContext::default(),
            has_plan: false,
            plan_fresh: false,
            wplan: WithdrawalPlan::default(),
            pinputs: PlanInputs::default(),
            exec_frac: 0.0,
            sp_state: SpendingState::default(),
            planner_started: false,
            rmd_started: [false; MAX_PEOPLE],
            sepp: [0.0; MAX_PEOPLE],
            sepp_until: [0; MAX_PEOPLE],
            retire_cash_cap: 0.0,
            sheltered_rebalancing: false,
            retired: false,
            retire_month: i32::MAX,
            fi_month: i32::MAX,
            coast_month: i32::MAX,
            req: Requirement::default(),
            req_dirty: true,
            inv_at_ret: f64::NAN,
            funded_ratio: f64::NAN,
            ret_index: 1.0,
            ret_months: 0,
            initial_wr: f64::NAN,
            events: vec![EventRt::default(); plan.events.len()],
            next_event: i32::MAX,
            home_index: 1.0,
            eq_ring: [0.0; 12],
            eq12: 0.0,
            eq_index: 1.0,
            eq_peak: 1.0,
            port_index: 1.0,
            port_peak: 1.0,
            worst_dd: 0.0,
            year_port: 1.0,
            prior_return_negative: false,
            price_jan: 1.0,
            prior_inflation: 0.0,
            coverage_gap: u32::MAX,
            depleted_now: false,
            depleted_year: false,
            ever_depleted: false,
            depletion_month: i32::MAX,
            home_sale_month: i32::MAX,
            ss_first_paid: [i32::MAX; MAX_PEOPLE],
            stopped_work: [i32::MAX; MAX_PEOPLE],
            had_mortgage: false,
            mortgage_payoff_month: i32::MAX,
            ltc_real: 0.0,
            ltc_at_depletion: 0.0,
            unemployed_at_depletion: 0.0,
            led: [0.0; N_SERIES],
            planned: 0.0,
            cash_start: 0.0,
            yr: YearRecord::default(),
            yr_months: 0,
            last_spend: 0.0,
            wage_like: 0.0,
            port_return: 0.0,
            has_equity: plan.has_equity,
            ty_once: TaxInputs::default(),
            amt_credit: 0.0,
            eq_supp_ytd: [0.0; MAX_PEOPLE],
            eq_wh_ytd: 0.0,
            eq_addl_medicare_ytd: 0.0,
            eq_kept_gain: 0.0,
            eq_base: TaxInputs::default(),
            eq_hold: 0.0,
            eq_reserve: 0.0,
            eq_replan: false,
            eq_unplanned: false,
            eq_event_year: false,
            eq_due: false,
            eq_paper: 0.0,
            eq_gross_real: 0.0,
            eq_cost_real: 0.0,
            eq_tender_real: 0.0,
            eq_tax_real: 0.0,
            eq_event_month: i32::MAX,
            chk_eq_start: 0.0,
            chk_eq_flow: 0.0,
            chk_other_start: 0.0,
            chk_prop_start: 0.0,
            chk_mort_start: 0.0,
            chk_debt_start: 0.0,
            chk_ret_other: 0.0,
            chk_prop_flow: 0.0,
            chk_mort_flow: 0.0,
            chk_debt_flow: 0.0,
            trace_events: Vec::new(),
            tax_details: Vec::new(),
        }
    }

    /// Starts path `path`: draws lifetimes, generates the economy, resolves events
    /// and resets every balance. Allocation-free once the workspace is warm.
    pub(crate) fn begin(
        &mut self,
        plan: &Plan,
        seed: u64,
        path: u64,
        econ: &mut EconomyPath,
        detail: bool,
        check: bool,
    ) {
        self.detail = detail;
        self.check = check;
        self.n = plan.people.len();

        // Lifetimes first: they fix the horizon whatever else the scenario changes.
        let mut last = 1u32;
        for i in 0..MAX_PEOPLE {
            self.rng_career[i] = Rng::stream(seed, path, Stream::Career(i));
            self.rng_health[i] = Rng::stream(seed, path, Stream::Health(i));
            self.alive[i] = false;
            self.present[i] = false;
            self.person_retired[i] = false;
            self.claimants[i] = None;
            self.ss_claimed[i] = false;
            self.in_facility[i] = false;
            self.last_phase[i] = CoveragePhase::None;
            self.death_month[i] = 0;
            self.health[i] = HealthState::default();
            self.records[i] = EarningsRecord::default();
        }
        for (i, person) in plan.people.iter().enumerate() {
            let mut rng = Rng::stream(seed, path, Stream::Longevity(i));
            let d = person.mortality.draw_months_to_death(&mut rng);
            self.death_month[i] = d;
            last = last.max(d);
            self.alive[i] = true;
            self.present[i] = person.joins <= 0;
            self.health[i] = plan.health.initial_state(
                person.sex,
                person.age0.max(0) as u32,
                d,
                &mut self.rng_health[i],
            );
            self.records[i] = person.record0;
        }
        self.months = last.min(plan.horizon_months).max(1) as usize;
        let months = self.months;

        let mut market = Rng::stream(seed, path, Stream::Market);
        let mut inflation = Rng::stream(seed, path, Stream::Inflation);
        plan.econ.generate(
            &mut EconomyStreams {
                market: &mut market,
                inflation: &mut inflation,
            },
            path,
            months,
            econ,
        );
        self.rng_equity = Rng::stream(seed, path, Stream::EquityComp);
        self.rng_shocks = Rng::stream(seed, path, Stream::Shocks);
        for (i, rng) in self.rng_housing.iter_mut().enumerate() {
            *rng = Rng::stream(seed, path, Stream::Housing(i));
        }

        // Retirement date when the trigger fixes it.
        self.retired = false;
        self.retire_month = match plan.trigger {
            TriggerPlan::Already => 0,
            TriggerPlan::AtMonth(m) => m,
            _ => i32::MAX,
        };
        self.fi_month = i32::MAX;
        self.coast_month = i32::MAX;
        self.req = Requirement::default();
        self.req_dirty = true;
        self.inv_at_ret = f64::NAN;
        self.funded_ratio = f64::NAN;
        self.ret_index = 1.0;
        self.ret_months = 0;
        self.initial_wr = f64::NAN;

        // Stress overlays with a date known now.
        for stress in &plan.stresses {
            self.apply_stress(plan, econ, stress, 0, false);
        }

        // Events: two draws each, in profile order, whatever their settings.
        let mut rng = Rng::stream(seed, path, Stream::Events);
        self.next_event = i32::MAX;
        for (rt, ev) in self.events.iter_mut().zip(&plan.events) {
            let occurs = rng.uniform() < ev.probability;
            let u = rng.uniform();
            let shift = ((2.0 * u - 1.0) * ev.uncertainty_months).round() as i32;
            *rt = EventRt {
                month: i32::MAX,
                shift,
                done: !occurs,
                retries: 0,
            };
            if occurs {
                match ev.when {
                    // An event dated before the start has already happened.
                    Sched::At(m) if m < 0 => rt.done = true,
                    Sched::At(m) => rt.month = (m + shift).max(0),
                    Sched::Retire(off) if self.retire_month != i32::MAX => {
                        rt.month = (self.retire_month + off + shift).max(0);
                    }
                    Sched::Retire(_) => {}
                    Sched::Never => rt.done = true,
                }
                if !rt.done {
                    self.next_event = self.next_event.min(rt.month);
                }
            }
        }

        // Balance sheet.
        self.acc = plan.accounts0;
        self.acc.plain_mix = plan.plain_mix;
        for (rt, jp) in self.jobs.iter_mut().zip(&plan.jobs) {
            let started = jp.start.month(self.retire_month) <= 0;
            *rt = JobRt {
                st: if started {
                    jp.model.initial_state()
                } else {
                    jp.model.initial_state_not_started()
                },
                plan: jp.plan,
                begun: started,
                ended: false,
            };
        }
        for (rt, sp) in self.selfemp.iter_mut().zip(&plan.selfemp) {
            *rt = SeRt {
                st: sp.model.initial_state(),
                scale: 1.0,
                ended: false,
            };
        }
        self.streams.fill(StreamRt {
            scale: 1.0,
            ended: false,
            started: false,
            start_price: 1.0,
            level: 1.0,
            last: 0.0,
        });
        for (st, pp) in self.props.iter_mut().zip(&plan.props) {
            *st = pp.model.initial_state();
        }
        for (rt, dp) in self.debts.iter_mut().zip(&plan.debts) {
            *rt = DebtRt {
                balance: dp.balance,
                rate: dp.rate,
                payment: dp.payment,
                revolving: dp.revolving,
            };
        }
        self.freed_payments = 0.0;
        self.debts_active = !self.debts.is_empty();

        // Social Security.
        self.ss_from = usize::MAX;
        for (i, person) in plan.people.iter().enumerate() {
            let pia = match person.basis {
                BenefitBasis::KnownPia(x) => Some(x),
                BenefitBasis::FromEarnings => {
                    Some(
                        plan.ss
                            .pia_real(&self.records[i], person.birth, plan.start, 1.0),
                    )
                }
                BenefitBasis::None => None,
            };
            if self.present[i] {
                self.claimants[i] = Some(Claimant::new(person.birth, person.claim_age, pia));
            }
            // Survivor benefits can start at 60, own benefits at 62.
            let from = (60 * 12 - person.age0).max(0) as usize;
            self.ss_from = self.ss_from.min(from);
        }
        self.survivor = false;
        self.death_year = i32::MIN;
        self.ss_cache = MonthlyBenefits::default();
        self.ss_cache_price = 1.0;
        self.ss_next_change = 0;
        self.ss_dirty = true;

        // Budget.
        self.item_scale.fill(1.0);
        self.budget_dirty = true;
        self.next_item_change = i32::MAX;
        self.extra_rent = 0.0;
        self.creep = 1.0;
        self.income_hwm = plan.income_rate0;
        self.smile = 1.0;
        self.smile_target = 0.0;
        self.disc_mult = 1.0;
        self.service_growth = 1.0;
        self.kids_real = -1.0;
        self.kids_until = plan
            .children
            .iter()
            .map(|c| c.birth + 22 * 12)
            .max()
            .unwrap_or(0);

        // Location and tax.
        self.tax_state = plan.state0;
        self.in_us = plan.in_us0;
        self.foreign_rate = plan.foreign_rate0;
        self.local = plan.local0;
        self.anchor_cpi = 1.0;
        self.anchor_price = 1.0;
        self.prev_cpi_factor = 1.0;
        self.ty = TaxInputs::default();
        self.paid = [0.0; 4];
        self.due = [0.0; 4];
        self.year_earned = [0.0; MAX_PEOPLE];
        self.year_months = 13 - plan.start.month();
        self.loss_carry = 0.0;
        self.rental_carry = 0.0;
        self.magi_hist = [0.0; 2];
        self.magi_known = 0;
        self.prev_interest = f64::NAN;
        self.prev_ordinary_div = 0.0;
        self.prev_qualified_div = 0.0;
        self.prev_rental_net = 0.0;
        self.prev_mortgage_interest = 0.0;
        self.prev_property_tax = 0.0;
        self.wh_rate = 0.0;
        self.installment = 0.0;
        self.share_state = 0.0;
        self.share_se = 0.0;
        self.aca_advance_m = 0.0;
        self.aca_guess_real = 0.0;
        self.aca_advance_ytd = 0.0;
        self.aca_premium_ytd = 0.0;
        self.aca_people = 0;
        self.irmaa_m = [0.0; MAX_PEOPLE];
        // Pay earned before the start month counts toward this year's wage base.
        let before = f64::from(plan.start.month() - 1) / 12.0;
        self.ytd_fica_wages = [0.0; MAX_PEOPLE];
        for jp in &plan.jobs {
            if jp.start.month(i32::MAX) <= 0 {
                self.ytd_fica_wages[jp.owner] +=
                    jp.model.expected_real_salary(0.0) * (1.0 + jp.bonus_target) * before;
            }
        }

        self.target = plan.target0;
        self.efund_target = 0.0;
        self.sctx = SavingsContext::default();
        self.has_plan = false;
        self.plan_fresh = false;
        self.wplan = WithdrawalPlan::default();
        self.pinputs = PlanInputs::default();
        self.exec_frac = 0.0;
        self.sp_state = SpendingState::default();
        self.planner_started = false;
        self.rmd_started = [false; MAX_PEOPLE];
        self.sepp = [0.0; MAX_PEOPLE];
        self.sepp_until = [0; MAX_PEOPLE];
        self.retire_cash_cap = 0.0;
        self.sheltered_rebalancing = false;

        self.home_index = 1.0;
        self.eq_ring = [0.0; 12];
        self.eq12 = 0.0;
        self.eq_index = 1.0;
        self.eq_peak = 1.0;
        self.port_index = 1.0;
        self.port_peak = 1.0;
        self.worst_dd = 0.0;
        self.year_port = 1.0;
        self.prior_return_negative = false;
        self.price_jan = 1.0;
        self.prior_inflation = 0.0;
        self.coverage_gap = u32::MAX;

        self.depleted_now = false;
        self.depleted_year = false;
        self.ever_depleted = false;
        self.depletion_month = i32::MAX;
        self.home_sale_month = i32::MAX;
        self.ss_first_paid = [i32::MAX; MAX_PEOPLE];
        self.stopped_work = [i32::MAX; MAX_PEOPLE];
        self.had_mortgage = false;
        self.mortgage_payoff_month = i32::MAX;
        self.ltc_real = 0.0;
        self.ltc_at_depletion = 0.0;
        self.unemployed_at_depletion = 0.0;
        self.led = [0.0; N_SERIES];
        self.planned = 0.0;
        self.cash_start = self.acc.cash;
        self.yr = YearRecord::default();
        self.yr_months = 0;
        self.last_spend = 0.0;
        self.wage_like = 0.0;
        self.port_return = 0.0;
        self.trace_events.clear();
        self.tax_details.clear();

        // Private companies: the fate of each is drawn here, four words from its own
        // stream whatever its settings, after the economy path exists (an IPO month
        // is placed in an open window of that path).
        self.has_equity = plan.has_equity;
        self.ty_once = TaxInputs::default();
        self.amt_credit = plan.amt_credit0;
        self.eq_supp_ytd = [0.0; MAX_PEOPLE];
        self.eq_wh_ytd = 0.0;
        self.eq_addl_medicare_ytd = 0.0;
        self.eq_kept_gain = 0.0;
        self.eq_base = TaxInputs::default();
        self.eq_hold = 0.0;
        self.eq_reserve = 0.0;
        self.eq_replan = false;
        self.eq_unplanned = false;
        self.eq_event_year = false;
        self.eq_due = false;
        self.eq_paper = 0.0;
        self.eq_gross_real = 0.0;
        self.eq_cost_real = 0.0;
        self.eq_tender_real = 0.0;
        self.eq_tax_real = 0.0;
        self.eq_event_month = i32::MAX;
        self.chk_eq_start = 0.0;
        self.chk_eq_flow = 0.0;
        for (i, cp) in plan.companies.iter().enumerate() {
            self.rng_company[i] = Rng::stream(seed, path, Stream::PrivateCompany(i));
            // A job whose owner joins the household later has no runtime until then,
            // but it is held on its own dates all the same: the equity vests meanwhile.
            let link = match cp.job {
                Some(j) if self.jobs[j].begun => Link::Active,
                Some(j) if !self.present[cp.owner] && plan.jobs[j].own_start <= 0 => Link::Active,
                _ => Link::Pending,
            };
            let st = cp.model.begin(link, &mut self.rng_company[i], Some(&*econ));
            let joined = self.present[cp.owner];
            self.companies[i] = CompanyRt {
                st,
                u_tender: 0.0,
                joined,
                passed_on: false,
                sever_next: false,
            };
            if joined {
                // Incentive options exercised earlier this calendar year count toward
                // this year's minimum tax; a sale later in the year reverses them.
                let spread = cp.model.start_amt_adjustment();
                self.ty.amt_iso_adjustment += spread;
                self.ty_once.amt_iso_adjustment += spread;
                self.eq_event_year |= spread > 0.0;
            }
        }
        // The law, filing status and year-end ages of the first year must exist
        // before month 0's calendar step: a household retired from the start runs
        // the funded test before the first annual plan, and would otherwise read
        // whatever the previous path in this workspace left behind.
        self.init_first_year(plan);
    }

    /// Applies one stress overlay if its date is known. `at_retirement` is true when
    /// called from the retirement month `t`; otherwise only fixed dates are applied.
    pub(crate) fn apply_stress(
        &self,
        plan: &Plan,
        econ: &mut EconomyPath,
        stress: &StressPlan,
        t: usize,
        at_retirement: bool,
    ) {
        let resolve = |s: Sched| -> Option<usize> {
            match s {
                Sched::At(m) if !at_retirement => Some(m.max(0) as usize),
                Sched::Retire(off) if at_retirement => {
                    Some((t as i32 + off).max(t as i32) as usize)
                }
                _ => None,
            }
        };
        let resolved = match *stress {
            StressPlan::Crash {
                drawdown,
                recovery_months,
            } => at_retirement.then_some(ResolvedStress::Crash {
                at_month: t,
                drawdown,
                recovery_months,
            }),
            StressPlan::LostDecade { start } => {
                resolve(start).map(|from_month| ResolvedStress::LostDecade { from_month })
            }
            StressPlan::Stagflation {
                start,
                months,
                inflation,
            } => resolve(start).map(|from_month| ResolvedStress::Stagflation {
                from_month,
                months,
                inflation,
            }),
        };
        if let Some(r) = resolved {
            plan.econ.apply_stress(&r, econ);
        }
    }

    /// Money received into cash.
    #[inline]
    pub(crate) fn recv(&mut self, series: YearSeries, amount: f64) {
        self.acc.cash += amount;
        self.led[series.index()] += amount;
    }

    /// Money paid out of cash. The cash balance may go negative within a month; the
    /// settlement step funds it or records the shortfall.
    #[inline]
    pub(crate) fn pay(&mut self, series: YearSeries, amount: f64) {
        self.acc.cash -= amount;
        self.led[series.index()] += amount;
    }

    /// Spending that is part of the plan as well as paid.
    #[inline]
    pub(crate) fn spend(&mut self, series: YearSeries, amount: f64) {
        self.pay(series, amount);
        self.planned += amount;
    }

    /// Age of person `i` in whole months in month `t`.
    #[inline]
    pub(crate) fn age_months(&self, plan: &Plan, i: usize, t: usize) -> i32 {
        plan.people[i].age0 + t as i32
    }

    /// Age of person `i` in years in month `t`.
    #[inline]
    pub(crate) fn age(&self, plan: &Plan, i: usize, t: usize) -> f64 {
        f64::from(self.age_months(plan, i, t)) / 12.0
    }

    /// Logs an event for the trace (detail mode only; the label is built lazily).
    #[inline(always)]
    pub(crate) fn event(
        &mut self,
        now: YearMonth,
        kind: TraceEventKind,
        amount: Option<f64>,
        label: impl FnOnce() -> String,
    ) {
        if self.detail {
            self.push_event(now, kind, amount, label);
        }
    }

    /// The body of [`State::event`], kept out of line: the simulation loop has
    /// dozens of event sites and only a trace ever reaches one.
    #[cold]
    #[inline(never)]
    fn push_event(
        &mut self,
        now: YearMonth,
        kind: TraceEventKind,
        amount: Option<f64>,
        label: impl FnOnce() -> String,
    ) {
        self.trace_events.push(TraceEvent {
            month: now,
            kind,
            label: label(),
            amount,
        });
    }

    /// Non-mortgage debt outstanding.
    pub(crate) fn debt_total(&self) -> f64 {
        let mut t = 0.0;
        for d in &self.debts {
            t += d.balance;
        }
        t
    }

    /// Market value and mortgage principal of every property owned.
    pub(crate) fn property_totals(&self) -> (f64, f64) {
        let (mut value, mut mortgage) = (0.0, 0.0);
        for p in &self.props {
            if p.owned {
                value += p.value;
                mortgage += p.mortgage_balance();
            }
        }
        (value, mortgage)
    }
}
