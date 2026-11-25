//! Results: everything that leaves the engine.
//!
//! * [`Recorder`] is what the simulation loop writes into: one [`YearRecord`] per
//!   calendar year and one [`PathEnd`] per path, plus monthly detail when tracing.
//! * [`Accumulator`] is the aggregate recorder: fixed-size histograms, counts and
//!   whole-dollar sums, so memory does not grow with the number of paths except for
//!   the per-path scalar columns. It is what a worker returns for a chunk of paths.
//! * [`Summary`] is the JSON the UI and the AI read, derived from an accumulator;
//!   [`SummaryDigest`] is its small form for an LLM tool result.
//! * [`compare`] pairs two accumulators made with the same seed and path indices.
//! * [`PathTrace`] (built by [`TraceRecorder`]) is the full ledger of one path.
//!
//! `docs/engine-output.md` documents the output structures field by field.
//!
//! Contract:
//!
//! * Every money value handed to a recorder is in **today's dollars**.
//! * `merge` is exact, commutative and associative: integer counts add, money sums are
//!   kept as whole-dollar `i64` (rounded per path-year, so no floating-point
//!   reassociation), and per-path scalar chunks are kept sorted by first path index
//!   and coalesced. Any chunking and any merge order therefore gives identical bytes.
//! * Series and scalar identities are the enums below. Their `key()` strings are the
//!   names the UI and stored results use: never rename one, only add at the end (and
//!   bump [`Accumulator::VERSION`] when a list changes).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::YearMonth;
use crate::N_ASSETS;

mod accumulator;
mod compare;
mod hist;
mod num;
mod summary;
mod trace;

pub use accumulator::{Accumulator, AccumulatorError, SamplePath, SampleRecorder, ScalarChunk};
pub use compare::{compare, Comparison, MetricDelta, PairedByOutcome, PairedOutcome, PairedScalar};
pub use hist::{hist_bin, hist_bin_edges, hist_bin_value, HIST_BINS, HIST_MAX, HIST_SUB_BINS};
pub use num::{Estimate, Spread};
pub use summary::{
    AgeCurve, Axis, DensityGrid, DigestBands, DigestCause, DigestCurve, DigestEquity, DigestPicks,
    DigestRate, DigestSpending, DigestSpread, EquityConditional, EquityOutcomeShare, EquitySummary,
    FactorShare, FailureAttribution, FailureShare, Headline, MeanSeries, PathPick, QuantileCurves,
    ScalarContrast, ScalarDistribution, Scatter, ScatterColumn, SpendingByYear, SpendingDelivery,
    StatusStack, Summary, SummaryContext, SummaryDigest, Survival, DENSITY_GROUP, DIGEST_CAUSES,
    DISTRIBUTION_BINS, DOTS, MIN_PATHS_PER_OUTCOME, QUANTILE_LEVELS, SCATTER_COLUMNS, SCATTER_MAX,
    SUMMARY_VERSION,
};
pub use trace::{
    PathTrace, TraceAnnual, TraceColumn, TraceRecorder, TraceScalar, TraceTaxLine, TraceTaxReturn,
};

/// Defines a fieldless enum with a dense index, a stable snake_case key and `ALL`.
macro_rules! keyed_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $key:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
        pub enum $name { $($(#[$vmeta])* #[serde(rename = $key)] $variant,)+ }

        impl $name {
            /// Every variant in index order.
            pub const ALL: &'static [$name] = &[$($name::$variant,)+];
            /// Number of variants.
            pub const COUNT: usize = Self::ALL.len();

            /// Dense index into per-series arrays.
            pub const fn index(self) -> usize { self as usize }

            /// Stable name used in JSON output and stored results.
            pub const fn key(self) -> &'static str {
                match self { $($name::$variant => $key,)+ }
            }

            /// The variant with this index, if any.
            pub fn from_index(index: usize) -> Option<$name> {
                Self::ALL.get(index).copied()
            }

            /// The variant with this key, if any.
            pub fn from_key(key: &str) -> Option<$name> {
                Self::ALL.iter().copied().find(|v| v.key() == key)
            }
        }
    };
}

keyed_enum! {
    /// Quantities whose full distribution is kept for every year (histograms). All
    /// are derived by the recorder from the [`YearSeries`] values of a
    /// [`YearRecord`], so they cannot disagree with the composition series.
    HistSeries {
        /// Net worth at year end: accounts plus property value less mortgages and
        /// other debt.
        NetWorth => "net_worth",
        /// Investable assets at year end: cash, taxable, traditional, Roth, HSA.
        Investable => "investable",
        /// Total household spending in the year: the sum of the spending series.
        /// Annualised when the record covers fewer than twelve months.
        Spending => "spending",
        /// Gross income from outside the portfolio (wages, self-employment, Social
        /// Security, pensions, rental, other); annualised like spending.
        GrossIncome => "gross_income",
        /// Total tax paid in the year: the sum of the tax series; annualised.
        TotalTax => "total_tax",
        /// Planned spending in the year (`YearSeries::SpendPlanned`: what spending
        /// would have been with no spending-rule adjustment and no shortfall, in
        /// the same scope as `spending`); annualised. Appended after the original
        /// five.
        SpendPlanned => "spend_planned",
    }
}

impl HistSeries {
    /// True for flows (annualised in the histogram when a record covers a partial
    /// year), false for year-end balances.
    pub const fn is_flow(self) -> bool {
        matches!(
            self,
            HistSeries::Spending
                | HistSeries::GrossIncome
                | HistSeries::TotalTax
                | HistSeries::SpendPlanned
        )
    }
}

keyed_enum! {
    /// Where a household stands at the end of a year (the "rich, broke or dead" view).
    /// Use [`Status::classify`] so every caller applies the same thresholds.
    Status {
        /// Not yet retired (and not depleted).
        Working => "working",
        /// Retired, spending on plan, and real investable assets at least
        /// [`SURPLUS_WEALTH_MULTIPLE`] times their level at retirement.
        RetiredSurplus => "retired_surplus",
        /// Retired and spending at least [`ON_PLAN_TOLERANCE`] of the planned budget.
        RetiredOnPlan => "retired_on_plan",
        /// Retired with spending below [`ON_PLAN_TOLERANCE`] of plan (a spending rule
        /// cut it).
        RetiredCutting => "retired_cutting",
        /// Liquid assets cannot cover essential spending while someone is alive
        /// (working or retired).
        Depleted => "depleted",
        /// Everyone has died. Never recorded: the summary derives it as the paths
        /// with no record for the year.
        Dead => "dead",
    }
}

/// Delivered spending at or above this share of plan counts as "on plan". Guardrail
/// rules move spending in steps of about 10% (Guyton-Klinger), so 5% separates a
/// real cut from rounding and smoothing noise.
pub const ON_PLAN_TOLERANCE: f64 = 0.95;
/// A retired year delivering less than this share of plan is a mild cut (the 90%
/// line of the spending-delivered metrics).
pub const MILD_CUT_RATIO: f64 = 0.90;
/// A retired year delivering less than this share of plan is a deep cut (the 75%
/// line of the spending-delivered metrics).
pub const DEEP_CUT_RATIO: f64 = 0.75;
/// Real investable assets above this multiple of their level at retirement mark the
/// "surplus" status (the "more than 2x starting real wealth" layer of the life-stage
/// chart, `docs/research/landscape.md` 3.1 item 6).
pub const SURPLUS_WEALTH_MULTIPLE: f64 = 2.0;

impl Status {
    /// The status of a living household at a year end.
    ///
    /// `delivered` and `planned` are the year's actual and planned spending,
    /// `investable` the year-end investable assets and `investable_at_retirement` the
    /// same on the retirement date, all in today's dollars.
    pub fn classify(
        retired: bool,
        depleted: bool,
        delivered: f64,
        planned: f64,
        investable: f64,
        investable_at_retirement: f64,
    ) -> Status {
        if depleted {
            Status::Depleted
        } else if !retired {
            Status::Working
        } else if delivered < ON_PLAN_TOLERANCE * planned {
            Status::RetiredCutting
        } else if investable_at_retirement > 0.0
            && investable >= SURPLUS_WEALTH_MULTIPLE * investable_at_retirement
        {
            Status::RetiredSurplus
        } else {
            Status::RetiredOnPlan
        }
    }
}

keyed_enum! {
    /// What a [`YearSeries`] measures; decides how it is averaged and where it sits
    /// in the cash-flow identity.
    SeriesKind {
        /// Year-end asset balance.
        Balance => "balance",
        /// Year-end liability (positive number).
        Liability => "liability",
        /// Income from outside the portfolio (a source of cash).
        Income => "income",
        /// Withdrawal from an account (a source of cash).
        Withdrawal => "withdrawal",
        /// Tax paid (a use of cash).
        Tax => "tax",
        /// Spending (a use of cash).
        Spending => "spending",
        /// Contribution to an account (a use of cash).
        Saving => "saving",
        /// Movement between assets.
        Transfer => "transfer",
        /// Informational flow outside the cash-flow identity.
        Memo => "memo",
    }
}

keyed_enum! {
    /// Per-year balances and flows, summed across paths for the composition charts
    /// and recorded monthly in a trace. Balances are period-end; flows are totals for
    /// the period. Everything is in today's dollars and non-negative unless noted.
    ///
    /// Cash-flow identity the simulation should satisfy every period
    /// ([`YearRecord::cash_flow_residual`] is zero):
    ///
    /// ```text
    /// income + withdrawals + property_sale + save_employer
    ///     = taxes + spending + saving + property_purchase
    /// ```
    ///
    /// (`save_employer` is on both sides: it is money the household never touches.)
    YearSeries {
        /// Balance: cash and savings accounts.
        BalanceCash => "balance_cash",
        /// Balance: taxable brokerage accounts.
        BalanceTaxable => "balance_taxable",
        /// Balance: traditional (pre-tax) retirement accounts.
        BalanceTraditional => "balance_traditional",
        /// Balance: Roth accounts.
        BalanceRoth => "balance_roth",
        /// Balance: HSAs.
        BalanceHsa => "balance_hsa",
        /// Balance: market value of every property owned (homes and rentals).
        PropertyValue => "property_value",
        /// Liability: mortgage principal outstanding on every property.
        Mortgage => "mortgage",
        /// Liability: non-mortgage debt outstanding.
        Debt => "debt",
        /// Income: wages, bonuses, commission and vested equity compensation, gross
        /// (private-company equity under `private_equity` is `income_equity`).
        IncomeWages => "income_wages",
        /// Income: self-employment net profit.
        IncomeSelfEmployment => "income_self_employment",
        /// Income: Social Security benefits, gross.
        IncomeSocialSecurity => "income_social_security",
        /// Income: pensions and annuities.
        IncomePensions => "income_pensions",
        /// Income: rental net operating income (rent less vacancy, management and
        /// operating costs; before the rental's mortgage payment).
        IncomeRental => "income_rental",
        /// Income: everything else, including windfalls.
        IncomeOther => "income_other",
        /// Withdrawal: cash accounts drawn down to cover a deficit.
        WithdrawCash => "withdraw_cash",
        /// Withdrawal: sales from taxable accounts.
        WithdrawTaxable => "withdraw_taxable",
        /// Withdrawal: traditional accounts, including RMDs (conversions excluded).
        WithdrawTraditional => "withdraw_traditional",
        /// Withdrawal: Roth accounts.
        WithdrawRoth => "withdraw_roth",
        /// Withdrawal: HSAs.
        WithdrawHsa => "withdraw_hsa",
        /// Tax: federal income tax including capital gains tax and NIIT, as paid in
        /// the period (withholding, estimates and the April settlement).
        TaxFederal => "tax_federal",
        /// Tax: state income tax, or foreign income tax when living abroad.
        TaxState => "tax_state",
        /// Tax: employee Social Security and Medicare tax and self-employment tax.
        TaxPayroll => "tax_payroll",
        /// Tax: early-withdrawal and other penalties.
        TaxPenalties => "tax_penalties",
        /// Spending: essential budget lines (including rent).
        SpendEssential => "spend_essential",
        /// Spending: discretionary budget lines.
        SpendDiscretionary => "spend_discretionary",
        /// Spending: owned property costs (mortgage principal and interest, property
        /// tax, insurance, maintenance, HOA, PMI).
        SpendHousing => "spend_housing",
        /// Spending: modelled healthcare premiums and out-of-pocket costs.
        SpendHealthcare => "spend_healthcare",
        /// Spending: long-term care.
        SpendLongTermCare => "spend_long_term_care",
        /// Spending: modelled child costs and college.
        SpendChildren => "spend_children",
        /// Spending: payments on non-mortgage debt.
        SpendDebt => "spend_debt",
        /// Spending: irregular shocks and one-off event expenses (cash paid).
        SpendOneOff => "spend_one_off",
        /// Saving: surplus added to cash accounts.
        SaveCash => "save_cash",
        /// Saving: contributions to taxable accounts.
        SaveTaxable => "save_taxable",
        /// Saving: employee contributions to traditional accounts.
        SaveTraditional => "save_traditional",
        /// Saving: employee contributions to Roth accounts.
        SaveRoth => "save_roth",
        /// Saving: employee HSA contributions.
        SaveHsa => "save_hsa",
        /// Saving: employer contributions (401(k) match and employer HSA money).
        SaveEmployer => "save_employer",
        /// Transfer: Roth conversions (traditional to Roth; no cash moves).
        RothConversion => "roth_conversion",
        /// Transfer: cash put into property at purchase (down payment; closing costs
        /// are `spend_one_off`).
        PropertyPurchase => "property_purchase",
        /// Transfer: net cash received from property sales (after the mortgage payoff
        /// and selling costs).
        PropertySale => "property_sale",
        /// Memo: what total spending would have been this period with no spending-rule
        /// cut and no depletion shortfall, in the same scope as the sum of the
        /// spending series. Equal to actual spending while working. The denominator
        /// of every delivered-versus-planned metric.
        SpendPlanned => "spend_planned",
        /// Memo: interest and dividends paid inside cash and taxable accounts (stays
        /// in the account; already part of `investment_return`).
        InvestmentIncome => "investment_income",
        /// Memo: change in account balances not explained by contributions,
        /// withdrawals and conversions: market return net of fees. May be negative.
        InvestmentReturn => "investment_return",
        /// Income: cash from private-company equity before tax: sales of shares in
        /// tender offers, acquisitions and after an IPO, cash-outs of options (net
        /// of the strike) and RSUs, and the value of shares moved to the taxable
        /// account when a lockup ends.
        IncomeEquity => "income_equity",
        /// Transfer: cash paid to exercise options (the strike). The shares bought
        /// are private-company equity, outside net worth until sold.
        EquityExercise => "equity_exercise",
        /// Memo: value at period end of vested private-company holdings at the
        /// company's current share price, before tax and before any strike is
        /// paid for options (in-the-money value). Not part of net worth.
        EquityPaperValue => "equity_paper_value",
    }
}

const fn series_range(first: YearSeries, last: YearSeries) -> std::ops::Range<usize> {
    first.index()..last.index() + 1
}

const ACCOUNTS: std::ops::Range<usize> =
    series_range(YearSeries::BalanceCash, YearSeries::BalanceHsa);
const INCOME: std::ops::Range<usize> =
    series_range(YearSeries::IncomeWages, YearSeries::IncomeOther);
const WITHDRAWALS: std::ops::Range<usize> =
    series_range(YearSeries::WithdrawCash, YearSeries::WithdrawHsa);
const TAXES: std::ops::Range<usize> =
    series_range(YearSeries::TaxFederal, YearSeries::TaxPenalties);
const SPENDING: std::ops::Range<usize> =
    series_range(YearSeries::SpendEssential, YearSeries::SpendOneOff);
const SAVING: std::ops::Range<usize> = series_range(YearSeries::SaveCash, YearSeries::SaveEmployer);

impl YearSeries {
    /// What the series measures.
    pub fn kind(self) -> SeriesKind {
        let i = self.index();
        // The private-company series sit after the ranges below.
        match self {
            YearSeries::IncomeEquity => return SeriesKind::Income,
            YearSeries::EquityExercise => return SeriesKind::Transfer,
            YearSeries::EquityPaperValue => return SeriesKind::Memo,
            _ => {}
        }
        if ACCOUNTS.contains(&i) || self == YearSeries::PropertyValue {
            SeriesKind::Balance
        } else if matches!(self, YearSeries::Mortgage | YearSeries::Debt) {
            SeriesKind::Liability
        } else if INCOME.contains(&i) {
            SeriesKind::Income
        } else if WITHDRAWALS.contains(&i) {
            SeriesKind::Withdrawal
        } else if TAXES.contains(&i) {
            SeriesKind::Tax
        } else if SPENDING.contains(&i) {
            SeriesKind::Spending
        } else if SAVING.contains(&i) {
            SeriesKind::Saving
        } else if matches!(
            self,
            YearSeries::RothConversion | YearSeries::PropertyPurchase | YearSeries::PropertySale
        ) {
            SeriesKind::Transfer
        } else {
            SeriesKind::Memo
        }
    }

    /// True for period-end stocks (balances, liabilities and the paper value of
    /// private-company holdings), false for flows.
    pub fn is_stock(self) -> bool {
        matches!(self.kind(), SeriesKind::Balance | SeriesKind::Liability)
            || self == YearSeries::EquityPaperValue
    }
}

/// A full set of [`YearSeries`] values, indexed by `YearSeries::index()`.
pub type SeriesValues = [f64; YearSeries::COUNT];

fn sum(values: &SeriesValues, range: std::ops::Range<usize>) -> f64 {
    values[range].iter().sum()
}

/// Investable assets implied by a set of series values.
pub fn investable_of(values: &SeriesValues) -> f64 {
    sum(values, ACCOUNTS)
}

/// Net worth implied by a set of series values.
pub fn net_worth_of(values: &SeriesValues) -> f64 {
    investable_of(values) + values[YearSeries::PropertyValue.index()]
        - values[YearSeries::Mortgage.index()]
        - values[YearSeries::Debt.index()]
}

/// Total spending implied by a set of series values.
pub fn spending_of(values: &SeriesValues) -> f64 {
    sum(values, SPENDING)
}

/// Gross outside income implied by a set of series values.
pub fn gross_income_of(values: &SeriesValues) -> f64 {
    sum(values, INCOME) + values[YearSeries::IncomeEquity.index()]
}

/// Total tax implied by a set of series values.
pub fn total_tax_of(values: &SeriesValues) -> f64 {
    sum(values, TAXES)
}

/// Sources of cash less uses of cash; zero when the period's flows balance.
pub fn cash_flow_residual_of(values: &SeriesValues) -> f64 {
    let sources = sum(values, INCOME)
        + values[YearSeries::IncomeEquity.index()]
        + sum(values, WITHDRAWALS)
        + values[YearSeries::PropertySale.index()]
        + values[YearSeries::SaveEmployer.index()];
    let uses = sum(values, TAXES)
        + sum(values, SPENDING)
        + sum(values, SAVING)
        + values[YearSeries::PropertyPurchase.index()]
        + values[YearSeries::EquityExercise.index()];
    sources - uses
}

keyed_enum! {
    /// The unit of a [`PathScalar`], for formatting and rounding.
    ScalarUnit {
        /// Today's dollars.
        Dollars => "dollars",
        /// An age in years (fractional, in twelfths).
        Age => "age",
        /// A number of calendar years.
        Years => "years",
        /// A number of months.
        Months => "months",
        /// A dimensionless ratio (1.0 = 100%).
        Ratio => "ratio",
        /// An annual rate as a decimal fraction.
        Rate => "rate",
        /// An enum index.
        Index => "index",
        /// 0.0 or 1.0.
        Flag => "flag",
    }
}

keyed_enum! {
    /// One number per path, stored for every path as `f32`. `NaN` means "did not
    /// happen" or "not applicable". Scalars marked *derived* are computed by the
    /// recorder from the path's year records ([`PathScalar::is_derived`]); the
    /// simulation supplies the others in [`PathEnd`].
    PathScalar {
        /// Age of the first person in the first month the retirement trigger was
        /// satisfied; NaN if never.
        FiAge => "fi_age",
        /// Age of the first person in the month retirement began (job income with
        /// `ends_at_retirement` stopped); NaN if never.
        RetirementAge => "retirement_age",
        /// Age of the first person in the first month liquid assets could not cover
        /// essential spending; NaN if never.
        DepletionAge => "depletion_age",
        /// Age at death of the first person, or their age at the horizon if alive.
        DeathAge0 => "death_age_0",
        /// Age at death of the second person (their own age), or at the horizon; NaN
        /// in a one-person household.
        DeathAge1 => "death_age_1",
        /// Investable assets on the retirement date; NaN if never retired.
        InvestableAtRetirement => "investable_at_retirement",
        /// Annualised real return of the investable portfolio over the first ten
        /// years of retirement (fewer if the path ends sooner); NaN if never retired.
        EarlyRetirementReturn => "early_retirement_return",
        /// Worst peak-to-trough fall of the real investable balance from market
        /// returns, as a positive fraction.
        WorstDrawdown => "worst_drawdown",
        /// Total months any person spent involuntarily unemployed.
        MonthsUnemployed => "months_unemployed",
        /// Total long-term care cost.
        LongTermCareCost => "long_term_care_cost",
        /// Annualised CPI inflation over the path.
        Inflation => "inflation",
        /// Funded ratio on the retirement date (present value of assets and future
        /// income over present value of planned spending); NaN if not computed.
        FundedRatio => "funded_ratio",
        /// First cause of failure as a `FailureCause` index; NaN if the path
        /// succeeded.
        FailureCause => "failure_cause",
        /// 1.0 if the path met the success definition (retired by the plan's latest
        /// age and never depleted while anyone was alive), else 0.0.
        Success => "success",
        /// Derived: number of calendar years recorded (the household's last death or
        /// the horizon).
        YearsSimulated => "years_simulated",
        /// Derived: net worth in the last recorded year.
        TerminalNetWorth => "terminal_net_worth",
        /// Derived: highest year-end net worth.
        PeakNetWorth => "peak_net_worth",
        /// Derived: lowest year-end investable balance in retirement; NaN if never
        /// retired.
        MinInvestable => "min_investable",
        /// Derived: total tax paid over the path.
        LifetimeTax => "lifetime_tax",
        /// Derived: total spending over the path.
        LifetimeSpending => "lifetime_spending",
        /// Derived: the share of planned retirement spending that was delivered:
        /// the sum over retired years of `min(delivered, planned)` divided by the sum
        /// of planned spending. Never above 1.0 (spending beyond the plan is in
        /// `extra_spending_ratio`); NaN if never retired.
        DeliveredSpendingRatio => "delivered_spending_ratio",
        /// Derived: the lowest single-year delivered / planned ratio in retirement;
        /// NaN if never retired.
        WorstSpendingRatio => "worst_spending_ratio",
        /// Derived: number of recorded years in retirement.
        YearsRetired => "years_retired",
        /// Derived: retired years delivering less than 90% of planned spending.
        YearsBelow90 => "years_below_90",
        /// Derived: retired years delivering less than 75% of planned spending.
        YearsBelow75 => "years_below_75",
        /// Derived: years ending in the `Depleted` status.
        YearsDepleted => "years_depleted",
        /// Supplied: first person's age in the first month the household's savings,
        /// with no further contributions and grown at the plan's expected real
        /// return, would meet the funded test at the planned (or latest) retirement
        /// age ("Coast FI"). Appended after the derived scalars to keep indices
        /// stable.
        CoastFiAge => "coast_fi_age",
        /// Derived: retirement spending above the plan (a spending rule raising
        /// spending in good times) as a fraction of planned retirement spending:
        /// the sum over retired years of `max(delivered - planned, 0)` over the sum
        /// of planned spending. NaN if never retired.
        ExtraSpendingRatio => "extra_spending_ratio",
        /// Supplied: the portfolio draw of the first retired year (the budget less
        /// income after tax) as a fraction of investable assets on the retirement
        /// date. The rate the household actually starts on, whatever the trigger's
        /// rule says; NaN if never retired.
        InitialWithdrawalRate => "initial_withdrawal_rate",
        /// Supplied: first person's age when a home was sold to fund retirement
        /// because savings had run out (`retirement.home_equity: last_resort`); NaN
        /// if that never happened.
        HomeSaleAge => "home_sale_age",
        /// Derived: the mean over retired years of `min(delivered / planned, 1)`:
        /// the share of the plan delivered in a typical retired year, every year
        /// weighing the same whatever its budget. NaN if never retired.
        PlanDeliveredShare => "plan_delivered_share",
        /// Supplied: the first person's own age when their Social Security benefit
        /// is first paid; NaN if it never is.
        SocialSecurityClaimAge0 => "social_security_claim_age_0",
        /// Supplied: the second person's own age when their benefit is first paid.
        SocialSecurityClaimAge1 => "social_security_claim_age_1",
        /// Supplied: the first person's own age when they stop working (the
        /// household's retirement date); NaN if they never do.
        PersonRetirementAge0 => "person_retirement_age_0",
        /// Supplied: the second person's own age when they stop working (the
        /// retirement date plus `partner_offset_years`); NaN if single or never.
        PersonRetirementAge1 => "person_retirement_age_1",
        /// Supplied: first person's age in the month the household's last mortgage
        /// is paid off (by its schedule or by a sale) and none is taken out
        /// afterwards; NaN if it never had a mortgage or still has one at the end.
        MortgagePayoffAge => "mortgage_payoff_age",
        /// Supplied: what happened to the household's main private company
        /// (`EquityOutcome` index); NaN if the plan has none. With several
        /// companies this is the one with the largest position at the start.
        EquityOutcome => "equity_outcome",
        /// Supplied: first person's age when that company failed, was acquired or
        /// went public; NaN if none of those happened within the life or there is
        /// no company.
        EquityEventAge => "equity_event_age",
        /// Supplied: cash received from private-company equity over the life,
        /// before tax, in today's dollars: every company's `income_equity`. 0 when
        /// nothing was received; NaN if the plan has no private company.
        EquityGrossProceeds => "equity_gross_proceeds",
        /// Supplied: cash paid to exercise options over the life, today's dollars.
        EquityExerciseCost => "equity_exercise_cost",
        /// Supplied: the part of the gross proceeds that came from tender offers.
        EquityTenderProceeds => "equity_tender_proceeds",
        /// Supplied: gross proceeds less exercise cost less the tax the equity
        /// caused (each year's tax with the equity amounts less the tax without
        /// them, including AMT and the later use of its credit). Negative when
        /// more was paid in than came back.
        EquityNetProceeds => "equity_net_proceeds",
    }
}

impl PathScalar {
    /// True if the recorder computes this scalar from the year records and
    /// overwrites whatever [`PathEnd`] holds for it.
    pub const fn is_derived(self) -> bool {
        (self.index() >= PathScalar::YearsSimulated.index()
            && self.index() <= PathScalar::YearsDepleted.index())
            || self.index() == PathScalar::ExtraSpendingRatio.index()
            || self.index() == PathScalar::PlanDeliveredShare.index()
    }

    /// The unit of the value.
    pub const fn unit(self) -> ScalarUnit {
        match self {
            PathScalar::FiAge
            | PathScalar::CoastFiAge
            | PathScalar::RetirementAge
            | PathScalar::DepletionAge
            | PathScalar::DeathAge0
            | PathScalar::HomeSaleAge
            | PathScalar::SocialSecurityClaimAge0
            | PathScalar::SocialSecurityClaimAge1
            | PathScalar::PersonRetirementAge0
            | PathScalar::PersonRetirementAge1
            | PathScalar::MortgagePayoffAge
            | PathScalar::EquityEventAge
            | PathScalar::DeathAge1 => ScalarUnit::Age,
            PathScalar::InvestableAtRetirement
            | PathScalar::LongTermCareCost
            | PathScalar::TerminalNetWorth
            | PathScalar::PeakNetWorth
            | PathScalar::MinInvestable
            | PathScalar::LifetimeTax
            | PathScalar::LifetimeSpending
            | PathScalar::EquityGrossProceeds
            | PathScalar::EquityExerciseCost
            | PathScalar::EquityTenderProceeds
            | PathScalar::EquityNetProceeds => ScalarUnit::Dollars,
            PathScalar::EarlyRetirementReturn
            | PathScalar::Inflation
            | PathScalar::InitialWithdrawalRate => ScalarUnit::Rate,
            PathScalar::WorstDrawdown
            | PathScalar::FundedRatio
            | PathScalar::DeliveredSpendingRatio
            | PathScalar::ExtraSpendingRatio
            | PathScalar::PlanDeliveredShare
            | PathScalar::WorstSpendingRatio => ScalarUnit::Ratio,
            PathScalar::MonthsUnemployed => ScalarUnit::Months,
            PathScalar::FailureCause | PathScalar::EquityOutcome => ScalarUnit::Index,
            PathScalar::Success => ScalarUnit::Flag,
            PathScalar::YearsSimulated
            | PathScalar::YearsRetired
            | PathScalar::YearsBelow90
            | PathScalar::YearsBelow75
            | PathScalar::YearsDepleted => ScalarUnit::Years,
        }
    }
}

keyed_enum! {
    /// Attribution of a failed path to its first cause, chosen by the simulation.
    FailureCause {
        /// Poor market returns early in retirement.
        MarketSequence => "market_sequence",
        /// Unemployment or lost earnings before retirement.
        JobLoss => "job_loss",
        /// Medical or long-term care costs.
        Health => "health",
        /// Outliving the money despite ordinary returns.
        Longevity => "longevity",
        /// Planned spending too high for the savings even on a median path.
        Overspending => "overspending",
        /// High inflation eroding fixed income or raising costs.
        Inflation => "inflation",
        /// None of the above stands out.
        Other => "other",
    }
}

keyed_enum! {
    /// What happened to a private company on one path.
    EquityOutcome {
        /// It failed: shares and options became worthless.
        Failed => "failed",
        /// No sale, listing or failure within the life: the event was dated after
        /// the life ended, or the company was written off after thirty years.
        StillPrivate => "still_private",
        /// It was acquired for cash.
        Acquired => "acquired",
        /// It went public.
        Ipo => "ipo",
    }
}

keyed_enum! {
    /// Kind of an entry in a traced path's event log.
    TraceEventKind {
        /// The retirement trigger was first satisfied.
        FinanciallyIndependent => "financially_independent",
        /// Retirement began.
        Retired => "retired",
        /// A person lost their job.
        LaidOff => "laid_off",
        /// A person found work again.
        Rehired => "rehired",
        /// A career break started or ended.
        CareerBreak => "career_break",
        /// Equity compensation vested or a private company had a liquidity event.
        EquityEvent => "equity_event",
        /// A property was bought.
        BoughtHome => "bought_home",
        /// A property was sold.
        SoldHome => "sold_home",
        /// The household moved.
        Relocated => "relocated",
        /// A windfall arrived.
        Windfall => "windfall",
        /// A planned one-off expense.
        MajorExpense => "major_expense",
        /// An irregular expense shock.
        ExpenseShock => "expense_shock",
        /// A Roth conversion.
        RothConversion => "roth_conversion",
        /// A spending rule cut spending.
        SpendingCut => "spending_cut",
        /// A spending rule raised or restored spending.
        SpendingRaise => "spending_raise",
        /// Liquid assets ran out.
        Depleted => "depleted",
        /// Liquid assets recovered after a depletion.
        Recovered => "recovered",
        /// Social Security was claimed.
        ClaimedSocialSecurity => "claimed_social_security",
        /// Health cover changed phase (employer, ACA, COBRA, Medicare).
        HealthCoverage => "health_coverage",
        /// A long-term care episode started or ended.
        LongTermCare => "long_term_care",
        /// Required minimum distributions began.
        RmdStarted => "rmd_started",
        /// A debt or mortgage was paid off.
        DebtPaidOff => "debt_paid_off",
        /// A child was born, started college or left the household.
        Child => "child",
        /// A partner joined the household.
        PartnerJoined => "partner_joined",
        /// A person died.
        Died => "died",
        /// A market move worth annotating (crash, recovery, stress overlay).
        Market => "market",
        /// A profile life event not covered above.
        LifeEvent => "life_event",
        /// Anything else.
        Other => "other",
        /// Options in a private company were exercised for cash.
        OptionsExercised => "options_exercised",
        /// A private company ran a tender offer.
        TenderOffer => "tender_offer",
        /// A private company was acquired.
        CompanyAcquired => "company_acquired",
        /// A private company went public.
        CompanyIpo => "company_ipo",
        /// A private company failed, or was written off after thirty years without
        /// a sale or listing.
        CompanyFailed => "company_failed",
        /// Unvested equity was forfeited or vested options lapsed.
        EquityForfeited => "equity_forfeited",
        /// Shares were sold or kept when an IPO lockup ended or on the sale
        /// schedule.
        LockupSale => "lockup_sale",
    }
}

/// Number of paths at the start of a run whose annual series are kept verbatim.
pub const SAMPLE_PATHS: u64 = 96;

/// One household-year, in today's dollars. The histogram quantities (net worth,
/// investable, spending, gross income, total tax) are derived from `values`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct YearRecord {
    /// Status at the end of the period ([`Status::classify`]); never `Dead`.
    pub status: Status,
    /// True if the retirement date has passed by the end of the period.
    pub retired: bool,
    /// Months of the calendar year this record covers (1..=12): fewer than 12 in the
    /// first year of a plan that does not start in January and in the year of the
    /// last death.
    pub months: u8,
    /// Period-end balances and period flows, indexed by `YearSeries::index()`.
    pub values: SeriesValues,
}

impl Default for YearRecord {
    fn default() -> Self {
        Self {
            status: Status::Working,
            retired: false,
            months: 12,
            values: [0.0; YearSeries::COUNT],
        }
    }
}

impl YearRecord {
    /// The value of one series.
    pub fn get(&self, series: YearSeries) -> f64 {
        self.values[series.index()]
    }

    /// Sets the value of one series.
    pub fn set(&mut self, series: YearSeries, value: f64) {
        self.values[series.index()] = value;
    }

    /// Net worth at the end of the period.
    pub fn net_worth(&self) -> f64 {
        net_worth_of(&self.values)
    }

    /// Investable assets at the end of the period.
    pub fn investable(&self) -> f64 {
        investable_of(&self.values)
    }

    /// Total spending in the period.
    pub fn spending(&self) -> f64 {
        spending_of(&self.values)
    }

    /// Gross outside income in the period.
    pub fn gross_income(&self) -> f64 {
        gross_income_of(&self.values)
    }

    /// Total tax paid in the period.
    pub fn total_tax(&self) -> f64 {
        total_tax_of(&self.values)
    }

    /// The histogrammed quantities, indexed by `HistSeries::index()` (not
    /// annualised).
    pub fn hist_values(&self) -> [f64; HistSeries::COUNT] {
        [
            self.net_worth(),
            self.investable(),
            self.spending(),
            self.gross_income(),
            self.total_tax(),
            self.get(YearSeries::SpendPlanned),
        ]
    }

    /// Sources of cash less uses of cash (see [`YearSeries`]); zero when the flows
    /// balance. For the simulation's conservation tests.
    pub fn cash_flow_residual(&self) -> f64 {
        cash_flow_residual_of(&self.values)
    }
}

/// The per-path scalars the simulation supplies, indexed by `PathScalar::index()`.
/// Entries for derived scalars are ignored (the recorder overwrites them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathEnd {
    /// One value per [`PathScalar`]; `NaN` where not applicable.
    pub scalars: [f32; PathScalar::COUNT],
}

impl Default for PathEnd {
    fn default() -> Self {
        Self {
            scalars: [f32::NAN; PathScalar::COUNT],
        }
    }
}

impl PathEnd {
    /// Sets one scalar.
    pub fn set(&mut self, scalar: PathScalar, value: f64) {
        self.scalars[scalar.index()] = value as f32;
    }

    /// Reads one scalar.
    pub fn get(&self, scalar: PathScalar) -> f32 {
        self.scalars[scalar.index()]
    }

    /// Sets `Success` and `FailureCause` together: `None` is a successful path.
    pub fn set_outcome(&mut self, failure: Option<FailureCause>) {
        match failure {
            None => {
                self.set(PathScalar::Success, 1.0);
                self.scalars[PathScalar::FailureCause.index()] = f32::NAN;
            }
            Some(cause) => {
                self.set(PathScalar::Success, 0.0);
                self.set(PathScalar::FailureCause, cause.index() as f64);
            }
        }
    }

    /// Sets `EquityOutcome` for a plan with a private company: `None` is a company
    /// with no sale, listing or failure within the life (`still_private`).
    pub fn set_equity_outcome(&mut self, outcome: Option<EquityOutcome>) {
        let outcome = outcome.unwrap_or(EquityOutcome::StillPrivate);
        self.set(PathScalar::EquityOutcome, outcome.index() as f64);
    }
}

/// One month of one path for tracing, in today's dollars. Net worth and investable
/// assets are derived from `values`, exactly as for a [`YearRecord`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonthRecord {
    /// Calendar month.
    pub month: YearMonth,
    /// Price level relative to the start (nominal = real x price level).
    pub price_level: f64,
    /// Status at month end.
    pub status: Status,
    /// True once the retirement date has passed.
    pub retired: bool,
    /// Month-end balances and the month's flows, indexed by `YearSeries::index()`.
    pub values: SeriesValues,
    /// Nominal return of the whole investable portfolio this month, net of fees
    /// (a monthly rate, for example 0.012).
    pub portfolio_return: f64,
    /// Nominal return of each asset class this month, indexed by
    /// `AssetClass::index()`.
    pub asset_returns: [f64; N_ASSETS],
}

impl MonthRecord {
    /// A record for `month` with everything else zero and the price level at 1.
    pub fn new(month: YearMonth) -> MonthRecord {
        MonthRecord {
            month,
            price_level: 1.0,
            status: Status::Working,
            retired: false,
            values: [0.0; YearSeries::COUNT],
            portfolio_return: 0.0,
            asset_returns: [0.0; N_ASSETS],
        }
    }

    /// Net worth at month end.
    pub fn net_worth(&self) -> f64 {
        net_worth_of(&self.values)
    }

    /// Investable assets at month end.
    pub fn investable(&self) -> f64 {
        investable_of(&self.values)
    }
}

/// An entry in a traced path's event log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TraceEvent {
    /// When it happened.
    pub month: YearMonth,
    /// What kind of event.
    pub kind: TraceEventKind,
    /// Human-readable sentence, for example "Roth conversion of $38,000".
    pub label: String,
    /// The amount involved in today's dollars, when there is one.
    pub amount: Option<f64>,
}

/// What the simulation loop writes into. The loop is generic over the recorder, so
/// the aggregate run pays nothing for tracing.
///
/// Call sequence for one path: `begin_path`, then one `record_year` per calendar year
/// in order from year index 0 with no gaps, ending with the year of the household's
/// last death or the horizon, then `end_path`. Nothing is recorded after everyone
/// has died.
pub trait Recorder {
    /// Called once before a path's first month.
    fn begin_path(&mut self, path_index: u64);

    /// Called each December, and at the final month if the path ends mid-year, with
    /// the year's record. `year_index` is 0 for the calendar year of `profile.start`.
    fn record_year(&mut self, year_index: usize, record: &YearRecord);

    /// Called once after a path's last month.
    fn end_path(&mut self, end: &PathEnd);

    /// Whether the loop should build and pass monthly records and events. The
    /// aggregate recorder says no, and the loop skips the work.
    fn wants_detail(&self) -> bool {
        false
    }

    /// Monthly detail, once per simulated month in order; only called when
    /// [`Recorder::wants_detail`] is true.
    fn record_month(&mut self, _record: &MonthRecord) {}

    /// An event-log entry; only called when [`Recorder::wants_detail`] is true.
    fn record_event(&mut self, _event: TraceEvent) {}

    /// The finished tax return for a year; only called when detail is wanted.
    fn record_tax_return(&mut self, _year: i32, _detail: &crate::tax::TaxDetail) {}
}

#[cfg(test)]
pub(crate) mod testkit;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_keys_are_unique_dense_and_match_serde() {
        fn check<T: Serialize + Copy>(
            all: &[T],
            key: fn(T) -> &'static str,
            index: fn(T) -> usize,
        ) {
            let mut keys: Vec<&str> = all.iter().map(|v| key(*v)).collect();
            for (i, v) in all.iter().enumerate() {
                assert_eq!(index(*v), i);
                assert_eq!(serde_json::to_value(v).unwrap(), serde_json::json!(key(*v)));
            }
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(keys.len(), all.len());
        }
        check(HistSeries::ALL, HistSeries::key, HistSeries::index);
        check(Status::ALL, Status::key, Status::index);
        check(SeriesKind::ALL, SeriesKind::key, SeriesKind::index);
        check(YearSeries::ALL, YearSeries::key, YearSeries::index);
        check(ScalarUnit::ALL, ScalarUnit::key, ScalarUnit::index);
        check(PathScalar::ALL, PathScalar::key, PathScalar::index);
        check(FailureCause::ALL, FailureCause::key, FailureCause::index);
        check(EquityOutcome::ALL, EquityOutcome::key, EquityOutcome::index);
        assert_eq!(EquityOutcome::COUNT, 4);
        check(
            TraceEventKind::ALL,
            TraceEventKind::key,
            TraceEventKind::index,
        );
        assert_eq!(Status::COUNT, 6);
        assert_eq!(YearSeries::COUNT, 46);
        assert_eq!(PathScalar::COUNT, 42);
        assert_eq!(YearSeries::from_key("mortgage"), Some(YearSeries::Mortgage));
        assert_eq!(PathScalar::from_index(0), Some(PathScalar::FiAge));
        assert_eq!(PathScalar::from_key("nope"), None);
        let back: PathScalar = serde_json::from_str("\"death_age_0\"").unwrap();
        assert_eq!(back, PathScalar::DeathAge0);
    }

    #[test]
    fn series_kinds_partition_the_list() {
        let count = |k: SeriesKind| YearSeries::ALL.iter().filter(|s| s.kind() == k).count();
        assert_eq!(count(SeriesKind::Balance), 6);
        assert_eq!(count(SeriesKind::Liability), 2);
        assert_eq!(count(SeriesKind::Income), 7);
        assert_eq!(count(SeriesKind::Withdrawal), 5);
        assert_eq!(count(SeriesKind::Tax), 4);
        assert_eq!(count(SeriesKind::Spending), 8);
        assert_eq!(count(SeriesKind::Saving), 6);
        assert_eq!(count(SeriesKind::Transfer), 4);
        assert_eq!(count(SeriesKind::Memo), 4);
        assert!(YearSeries::Mortgage.is_stock());
        assert!(YearSeries::EquityPaperValue.is_stock());
        assert!(!YearSeries::IncomeEquity.is_stock());
        assert!(!YearSeries::SpendPlanned.is_stock());
        assert_eq!(
            PathScalar::ALL.iter().filter(|s| s.is_derived()).count(),
            14
        );
        assert!(!PathScalar::Success.is_derived());
        assert!(PathScalar::YearsSimulated.is_derived());
    }

    #[test]
    fn derived_quantities_and_cash_flow_identity() {
        let mut r = YearRecord::default();
        r.set(YearSeries::BalanceCash, 10_000.0);
        r.set(YearSeries::BalanceTaxable, 50_000.0);
        r.set(YearSeries::BalanceTraditional, 200_000.0);
        r.set(YearSeries::BalanceRoth, 30_000.0);
        r.set(YearSeries::BalanceHsa, 5_000.0);
        r.set(YearSeries::PropertyValue, 400_000.0);
        r.set(YearSeries::Mortgage, 250_000.0);
        r.set(YearSeries::Debt, 15_000.0);
        assert_eq!(r.investable(), 295_000.0);
        assert_eq!(r.net_worth(), 295_000.0 + 400_000.0 - 250_000.0 - 15_000.0);

        // Wages 120k + employer match 4k; tax 30k; spending 60k; saving 34k of which
        // 4k is the match.
        r.set(YearSeries::IncomeWages, 120_000.0);
        r.set(YearSeries::TaxFederal, 18_000.0);
        r.set(YearSeries::TaxState, 4_000.0);
        r.set(YearSeries::TaxPayroll, 8_000.0);
        r.set(YearSeries::SpendEssential, 35_000.0);
        r.set(YearSeries::SpendDiscretionary, 15_000.0);
        r.set(YearSeries::SpendHousing, 10_000.0);
        r.set(YearSeries::SaveTraditional, 20_000.0);
        r.set(YearSeries::SaveTaxable, 10_000.0);
        r.set(YearSeries::SaveEmployer, 4_000.0);
        r.set(YearSeries::SpendPlanned, 60_000.0);
        r.set(YearSeries::InvestmentReturn, -12_345.0);
        assert_eq!(r.gross_income(), 120_000.0);
        assert_eq!(r.total_tax(), 30_000.0);
        assert_eq!(r.spending(), 60_000.0);
        assert_eq!(r.cash_flow_residual(), 0.0);
        assert_eq!(
            r.hist_values(),
            [430_000.0, 295_000.0, 60_000.0, 120_000.0, 30_000.0, 60_000.0]
        );
        // A home purchase funded from taxable balances.
        r.set(YearSeries::WithdrawTaxable, 80_000.0);
        r.set(YearSeries::PropertyPurchase, 80_000.0);
        assert_eq!(r.cash_flow_residual(), 0.0);
        r.set(YearSeries::PropertySale, 1.0);
        assert_eq!(r.cash_flow_residual(), 1.0);
        // Private-company equity: proceeds are a source, the strike a use, and the
        // paper value is in neither.
        r.set(YearSeries::PropertySale, 0.0);
        r.set(YearSeries::IncomeEquity, 50_000.0);
        r.set(YearSeries::EquityExercise, 20_000.0);
        r.set(YearSeries::SaveCash, 30_000.0);
        r.set(YearSeries::EquityPaperValue, 1_000_000.0);
        assert_eq!(r.cash_flow_residual(), 0.0);
        assert_eq!(r.gross_income(), 170_000.0);
        assert_eq!(r.net_worth(), 295_000.0 + 400_000.0 - 250_000.0 - 15_000.0);
    }

    #[test]
    fn status_classification() {
        let c = Status::classify;
        assert_eq!(c(false, false, 50.0, 50.0, 1.0, 0.0), Status::Working);
        assert_eq!(c(false, true, 50.0, 50.0, 0.0, 0.0), Status::Depleted);
        assert_eq!(c(true, true, 50.0, 50.0, 0.0, 100.0), Status::Depleted);
        assert_eq!(
            c(true, false, 96.0, 100.0, 150.0, 100.0),
            Status::RetiredOnPlan
        );
        assert_eq!(
            c(true, false, 94.0, 100.0, 500.0, 100.0),
            Status::RetiredCutting
        );
        assert_eq!(
            c(true, false, 100.0, 100.0, 200.0, 100.0),
            Status::RetiredSurplus
        );
        assert_eq!(
            c(true, false, 100.0, 100.0, 199.0, 100.0),
            Status::RetiredOnPlan
        );
        // No assets at retirement: never "surplus".
        assert_eq!(
            c(true, false, 100.0, 100.0, 0.0, 0.0),
            Status::RetiredOnPlan
        );
    }

    #[test]
    fn path_end_outcome() {
        let mut end = PathEnd::default();
        assert!(end.get(PathScalar::Success).is_nan());
        end.set_outcome(Some(FailureCause::Health));
        assert_eq!(end.get(PathScalar::Success), 0.0);
        assert_eq!(end.get(PathScalar::FailureCause), 2.0);
        end.set_outcome(None);
        assert_eq!(end.get(PathScalar::Success), 1.0);
        assert!(end.get(PathScalar::FailureCause).is_nan());
    }
}
