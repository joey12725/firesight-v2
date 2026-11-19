//! Account buckets.
//!
//! Responsibility: hold every liquid dollar of the household during a path, by tax
//! treatment and asset class, and implement the mechanics that move money: returns,
//! contributions, withdrawals with their tax character, Roth conversions, rebalancing
//! and the glide path.
//!
//! The user's `profile.accounts` are pooled at compile time into these buckets:
//!
//! * one **cash** bucket (all cash accounts, a single blended yield),
//! * one **taxable** bucket with cost basis per asset class (average-cost method),
//! * per person: **traditional**, **Roth** (contribution basis plus conversion
//!   tranches for the five-year rule) and **HSA**.
//!
//! Contract:
//!
//! * All balances are nominal dollars. No method allocates: every type here is `Copy`
//!   and built from fixed arrays.
//! * Money is conserved: every method returns exactly what moved, and the sum of
//!   returned flows equals the change in [`Accounts::investable`]. `sim`'s conservation
//!   property test depends on this.
//! * A withdrawal never overdraws: it returns the amount actually taken, which is
//!   less than requested when the bucket runs out.
//! * [`Accounts::contribute`] and [`Accounts::withdraw`] do not decide *which* bucket
//!   to use and do not enforce limits; `decumulation`, [`Accounts::save_surplus`] and
//!   [`Accounts::payroll_deferral`] do.
//!
//! Design choices (documented here because other modules rely on them):
//!
//! * **Taxable sales** use average cost per asset class. By default a withdrawal sells
//!   every asset class pro rata to its value; with
//!   [`WithdrawContext::prefer_overweight`] it sells first whatever is overweight
//!   household-wide, so withdrawals double as rebalancing. All realised gains are
//!   treated as long-term.
//! * **Year-to-date contributions** are tracked inside [`Accounts::ytd`] by the two
//!   limit-aware helpers and reset by [`Accounts::roll_year`].
//! * **Roth 401(k) and Roth IRA are one bucket**: employee Roth deferrals raise the
//!   contribution basis like IRA contributions do.
//! * **Inheritance** is a spousal rollover with a full step-up of taxable basis.

use crate::model::{
    AccountKind, AssetClass, AssetLocation, CashYield, EmployerPlan, GlidePath, PortfolioPolicy,
    Profile, N_ASSETS,
};
use crate::tax::{ContributionLimits, FilingStatus};
use crate::{math, MAX_PEOPLE};

/// Dollars held in each asset class, indexed by `AssetClass::index()`.
pub type Holdings = [f64; N_ASSETS];

/// Portfolio weights summing to 1, indexed by `AssetClass::index()`.
pub type Weights = [f64; N_ASSETS];

/// Number of years a Roth conversion must age before it can be withdrawn penalty-free
/// (`data/tax/us-federal-2026.json`: `early_withdrawal.roth_ira.conversion_five_year_rule_years`).
pub const CONVERSION_SEASONING_YEARS: usize = 5;

/// Tax years a Roth account must have been open before earnings can be withdrawn
/// tax-free (`early_withdrawal.roth_ira.five_year_rule_contributions_years`).
pub const ROTH_ACCOUNT_SEASONING_YEARS: u8 = 5;

/// Age from which retirement-account distributions carry no early-withdrawal penalty
/// (`early_withdrawal.penalty_free_age`).
pub const EARLY_WITHDRAWAL_AGE: f64 = 59.5;

/// Age from which non-medical HSA distributions carry no penalty
/// (`early_withdrawal.hsa_non_medical_penalty.penalty_free_age`).
pub const HSA_PENALTY_FREE_AGE: f64 = 65.0;

/// Age from which 401(k) and IRA catch-up contributions are allowed
/// (`retirement_accounts.*.catch_up_age_50_plus`).
pub const CATCH_UP_AGE: f64 = 50.0;

/// Ages (reached during the year) at which the larger "super" 401(k) catch-up
/// applies instead of the age-50 one
/// (`retirement_accounts.employer_plans_401k_403b_457b_tsp.super_catch_up_ages_60_to_63`).
pub const SUPER_CATCH_UP_FROM: f64 = 60.0;
/// Last age of the super catch-up.
pub const SUPER_CATCH_UP_TO: f64 = 63.0;

/// Age from which the HSA catch-up contribution is allowed
/// (`retirement_accounts.hsa.catch_up_age_55_plus`).
pub const HSA_CATCH_UP_AGE: f64 = 55.0;

/// Relative size below which a remainder is floating-point rounding, not money.
const ROUNDING_DUST: f64 = 1e-12;

/// Yields are annual; one month distributes a twelfth.
const MONTHS_PER_YEAR: f64 = 12.0;

/// Asset classes ordered from "belongs in a traditional account" to "belongs in a
/// Roth account" for [`AssetLocation::TaxEfficient`]. The front of the list throws
/// off the most ordinary income per dollar (bond and cash interest, REIT dividends,
/// gold taxed at collectibles rates); the back has the highest expected growth, which
/// is worth most where it is never taxed. Broad stock funds sit in the middle and end
/// up in the taxable bucket, where their qualified dividends and deferred gains are
/// taxed lightly (the usual Bogleheads "tax-efficient fund placement" ordering).
const LOCATION_ORDER: [AssetClass; N_ASSETS] = [
    AssetClass::Bonds,
    AssetClass::RealEstate,
    AssetClass::Cash,
    AssetClass::Gold,
    AssetClass::Custom,
    AssetClass::IntlStocks,
    AssetClass::UsStocks,
    AssetClass::Crypto,
    AssetClass::SingleStock,
];

/// How the yield of an asset class in the taxable bucket is taxed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Distribution {
    /// Taxable interest.
    Interest,
    /// Non-qualified dividends.
    Ordinary,
    /// Qualified dividends.
    Qualified,
}

/// Tax character of each asset class's yield, indexed by `AssetClass::index()`:
/// stock funds pay qualified dividends, bonds and cash pay interest, REIT dividends
/// are non-qualified, and anything the caller says gold, crypto or the custom asset
/// yields is treated as ordinary.
const DISTRIBUTION_KIND: [Distribution; N_ASSETS] = [
    Distribution::Qualified, // UsStocks
    Distribution::Qualified, // IntlStocks
    Distribution::Interest,  // Bonds
    Distribution::Interest,  // Cash
    Distribution::Ordinary,  // RealEstate
    Distribution::Ordinary,  // Gold
    Distribution::Ordinary,  // Crypto
    Distribution::Qualified, // SingleStock
    Distribution::Ordinary,  // Custom
];

/// Identifies one bucket.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bucket {
    /// The pooled cash accounts.
    Cash,
    /// The pooled taxable brokerage accounts.
    Taxable,
    /// A person's traditional (pre-tax) accounts.
    Traditional(usize),
    /// A person's Roth accounts.
    Roth(usize),
    /// A person's HSA.
    Hsa(usize),
}

impl Bucket {
    /// The account kind this bucket pools.
    pub const fn kind(self) -> AccountKind {
        match self {
            Bucket::Cash => AccountKind::Cash,
            Bucket::Taxable => AccountKind::Taxable,
            Bucket::Traditional(_) => AccountKind::Traditional,
            Bucket::Roth(_) => AccountKind::Roth,
            Bucket::Hsa(_) => AccountKind::Hsa,
        }
    }
}

/// Taxable brokerage holdings with average cost basis per asset class.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TaxableBucket {
    /// Market value by asset class.
    pub value: Holdings,
    /// Cost basis by asset class. `value - basis` is the unrealised gain.
    pub basis: Holdings,
}

/// Roth holdings and the bookkeeping for penalty-free access before 59 and a half.
/// Withdrawal ordering follows the IRS rules: contributions, then conversions oldest
/// first, then earnings.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RothBucket {
    /// Market value by asset class.
    pub value: Holdings,
    /// Direct contributions not yet withdrawn: always tax- and penalty-free.
    pub contribution_basis: f64,
    /// Conversions older than five years, not yet withdrawn: tax- and penalty-free.
    pub seasoned_conversions: f64,
    /// Conversions made in each of the last five calendar years, oldest first.
    /// `unseasoned[4]` is the current year. Each January the array shifts and
    /// `unseasoned[0]` moves into `seasoned_conversions`.
    pub unseasoned: [f64; CONVERSION_SEASONING_YEARS],
    /// True once the bucket has ever received money (starts the five-year clock for
    /// tax-free earnings).
    pub opened: bool,
    /// Januaries since the bucket was opened, saturating. Earnings are tax-free once
    /// this reaches [`ROTH_ACCOUNT_SEASONING_YEARS`] and the owner is 59 and a half.
    pub years_open: u8,
}

/// One person's tax-advantaged buckets.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PersonAccounts {
    /// Pre-tax holdings by asset class.
    pub traditional: Holdings,
    /// Roth holdings and basis tracking.
    pub roth: RothBucket,
    /// HSA holdings by asset class.
    pub hsa: Holdings,
}

/// Contributions made so far this calendar year, per person, for the annual limits.
/// Maintained by [`Accounts::save_surplus`], [`Accounts::payroll_deferral`] and
/// [`Accounts::contribute_hsa`]; reset by [`Accounts::roll_year`].
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ContributionYtd {
    /// Employee elective deferrals to the workplace plan (pre-tax plus Roth).
    pub k401_employee: [f64; MAX_PEOPLE],
    /// All additions to the workplace plan: employee, employer and after-tax.
    pub k401_additions: [f64; MAX_PEOPLE],
    /// IRA contributions, traditional plus Roth.
    pub ira: [f64; MAX_PEOPLE],
    /// The Roth part of `ira`.
    pub roth_ira: [f64; MAX_PEOPLE],
    /// HSA contributions from every source (payroll, employer, direct).
    pub hsa: [f64; MAX_PEOPLE],
}

/// Every liquid balance of the household.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Accounts {
    /// Cash accounts (checking, savings, emergency fund, retirement cash bucket).
    pub cash: f64,
    /// Taxable brokerage.
    pub taxable: TaxableBucket,
    /// Per-person retirement and health accounts.
    pub person: [PersonAccounts; MAX_PEOPLE],
    /// Contributions so far this year, for the annual limits.
    pub ytd: ContributionYtd,
    /// When true, [`Accounts::buy_mix`] returns the target mix itself instead of
    /// steering new money toward whatever is underweight. Set it when the portfolio
    /// is rebalanced on a schedule anyway: the steering then changes almost nothing
    /// and costs a pass over every holding for each contribution.
    pub plain_mix: bool,
    /// When true, the single-stock holding of the taxable bucket is employer stock
    /// the household has chosen to keep (`sell_on_vest: false`): rebalancing leaves
    /// it alone and arranges everything else around the target. It is sold only
    /// when the taxable bucket is drawn on for cash.
    pub keep_single_stock: bool,
    /// Asset classes known to be held in no bucket: bit `i` set means every
    /// holding (and the taxable basis) of asset class `i` is exactly zero. Most
    /// households hold three or four of the nine classes, and sums and monthly
    /// returns skip the rest (adding a zero never changes a sum, so the results
    /// are the same to the bit). The mask is conservative: 0 means nothing is
    /// known. [`Accounts::apply_returns`] sets bits from what it sees; every
    /// method that can put money into a class clears them. Code that writes
    /// holdings directly must call [`Accounts::holdings_changed`].
    pub(crate) idle: u16,
}

/// Blended interest terms of the pooled cash bucket, fixed at compile time from the
/// profile's cash accounts weighted by balance.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CashTerms {
    /// Share of cash that tracks the T-bill rate.
    pub tracking_share: f64,
    /// Spread over (usually under) the T-bill rate for the tracking share, annual.
    pub spread: f64,
    /// Fixed annual rate earned by the rest.
    pub fixed_rate: f64,
}

impl CashTerms {
    /// Annual yield of the cash bucket when T-bills yield `tbill_annual`. The
    /// tracking share never earns less than zero.
    pub fn annual_rate(&self, tbill_annual: f64) -> f64 {
        self.tracking_share * (tbill_annual + self.spread).max(0.0)
            + (1.0 - self.tracking_share) * self.fixed_rate
    }

    /// The monthly rate from the month's Treasury-bill return, without any
    /// transcendental function: the tracking share earns the bill return plus a
    /// twelfth of the spread (never below zero) and the rest a twelfth of the fixed
    /// rate. Within a basis point a year of [`CashTerms::monthly_rate`] at ordinary
    /// rates; use it in the monthly loop.
    #[inline]
    pub fn monthly_rate_from_bill_return(&self, bill_return: f64) -> f64 {
        self.tracking_share * (bill_return + self.spread / MONTHS_PER_YEAR).max(0.0)
            + (1.0 - self.tracking_share) * self.fixed_rate / MONTHS_PER_YEAR
    }

    /// The compounding monthly rate to pass to [`Accounts::apply_returns`].
    pub fn monthly_rate(&self, tbill_annual: f64) -> f64 {
        math::annual_to_monthly(self.annual_rate(tbill_annual))
    }
}

/// Taxable income thrown off by the cash and taxable buckets in one month.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct InvestmentIncome {
    /// Interest (cash, and the bond and cash sleeves of the taxable bucket).
    pub interest: f64,
    /// Non-qualified dividends (REITs).
    pub ordinary_dividends: f64,
    /// Qualified dividends (stock sleeves).
    pub qualified_dividends: f64,
    /// Fund expenses and advisory fees deducted across all buckets.
    pub fees: f64,
    /// Change in [`Accounts::investable`] caused by this call: market returns plus
    /// cash interest less fees. The dividends and interest above are part of it, not
    /// in addition to it (they were reinvested).
    pub net_change: f64,
}

/// What a withdrawal realised, for the tax return.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Withdrawal {
    /// Dollars actually taken out (less than requested if the bucket ran out).
    pub amount: f64,
    /// Portion taxed as ordinary income (traditional distributions, Roth earnings
    /// before qualification, non-medical HSA).
    pub ordinary: f64,
    /// Long-term capital gain realised (taxable bucket); negative for a loss.
    pub long_term_gain: f64,
    /// Portion subject to the 10% early-distribution penalty (20% for HSA).
    pub penalty_base: f64,
}

/// Facts a withdrawal needs to classify itself.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct WithdrawContext {
    /// The account owner's age in years (for 59 and a half and HSA age 65 rules).
    pub owner_age: f64,
    /// HSA only: dollars of this withdrawal that pay qualified medical expenses and
    /// are therefore tax-free (`f64::INFINITY` when all of it does).
    pub qualified_medical: f64,
    /// True if a 72(t) schedule, the rule of 55 or another exception covers this
    /// withdrawal, so no early-distribution penalty applies.
    pub penalty_exempt: bool,
    /// The household target mix. When set, the withdrawal first sells the asset
    /// classes that are overweight across the whole household (as far as this bucket
    /// holds them), so withdrawals double as rebalancing; `None` sells pro rata to the
    /// bucket's current holdings.
    pub prefer_overweight: Option<Weights>,
}

/// Gains realised by selling in the taxable bucket during a rebalance.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Realized {
    /// Net long-term capital gain (negative for a net loss).
    pub long_term_gain: f64,
    /// Dollars sold across all buckets, for turnover reporting.
    pub turnover: f64,
}

/// One person's facts for the savings waterfall.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PersonSavings {
    /// True if the person is alive and in the household.
    pub present: bool,
    /// Age at the end of the calendar year (catch-up contributions).
    pub age: f64,
    /// Covered by an HSA-eligible health plan and not on Medicare.
    pub hsa_eligible: bool,
    /// A traditional IRA contribution would be deductible (no workplace plan, or
    /// income under the deduction phase-out).
    pub traditional_deductible: bool,
    /// The workplace plan accepts after-tax contributions with in-plan Roth
    /// conversion (`EmployerPlan::after_tax_mega`) and the person is employed there.
    pub after_tax_401k: bool,
}

/// What [`Accounts::save_surplus`] needs to know about the year.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SavingsContext {
    /// This year's limits.
    pub limits: ContributionLimits,
    /// Filing status (selects the Roth IRA phase-out).
    pub filing_status: FilingStatus,
    /// Expected modified AGI for the year (Roth IRA phase-out).
    pub magi: f64,
    /// Household earned income expected for the year; total IRA contributions cannot
    /// exceed it.
    pub earned_income: f64,
    /// Marginal federal ordinary rate on this year's income.
    pub marginal_rate: f64,
    /// The rate the household expects to pay on traditional withdrawals in retirement
    /// (the tax-aware plan's `fill_to_bracket`, 0.12 by default).
    pub retirement_rate: f64,
    /// True if the HSA-eligible plan covers a family, false for self-only coverage.
    pub hsa_family: bool,
    /// Per-person facts.
    pub person: [PersonSavings; MAX_PEOPLE],
}

/// Where a month's surplus went.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SavingsFlows {
    /// Direct HSA contributions (deductible from income tax, not from FICA).
    pub hsa: [f64; MAX_PEOPLE],
    /// Deductible traditional IRA contributions.
    pub traditional_ira: [f64; MAX_PEOPLE],
    /// Roth IRA contributions.
    pub roth_ira: [f64; MAX_PEOPLE],
    /// After-tax 401(k) contributions converted in-plan to Roth.
    pub after_tax_401k: [f64; MAX_PEOPLE],
    /// The rest, invested in the taxable bucket.
    pub taxable: f64,
}

impl SavingsFlows {
    /// Total deposited; equals the surplus passed in.
    pub fn total(&self) -> f64 {
        let mut t = self.taxable;
        for p in 0..MAX_PEOPLE {
            t += self.hsa[p] + self.traditional_ira[p] + self.roth_ira[p] + self.after_tax_401k[p];
        }
        t
    }
}

/// One pay period, for [`Accounts::payroll_deferral`].
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PayPeriod {
    /// Gross pay this period (salary and bonus eligible for deferral), nominal dollars.
    pub gross_pay: f64,
    /// Calendar month, 1 to 12.
    pub month: u32,
    /// The employee's age at the end of the calendar year.
    pub age: f64,
}

/// One pay period's workplace-plan contributions.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PayrollDeferral {
    /// Employee pre-tax deferral (reduces taxable wages, not FICA wages).
    pub employee_pretax: f64,
    /// Employee Roth deferral (from after-tax pay).
    pub employee_roth: f64,
    /// Employer match, deposited pre-tax; not part of the employee's pay.
    pub employer: f64,
}

#[inline]
fn sum(h: &Holdings) -> f64 {
    let mut t = 0.0;
    for v in h {
        t += *v;
    }
    t
}

/// Bit mask with one bit per asset class.
const ALL_ASSETS: u16 = (1 << N_ASSETS) - 1;

/// The number of leading asset classes that may be held: every class from the
/// returned index on is idle.
#[inline]
fn live_prefix(idle: u16) -> usize {
    (u16::BITS - (!idle & ALL_ASSETS).leading_zeros()) as usize
}

/// `n` rounded up to a prefix length [`with_prefix`] has code for.
#[inline]
fn supported_prefix(n: usize) -> usize {
    match n {
        0..=3 => 3,
        4..=7 => n,
        _ => N_ASSETS,
    }
}

/// Runs `$body` with the const `N` bound to `$n` rounded up to a supported prefix
/// length, so that loops over `..N` compile to straight-line code.
macro_rules! with_prefix {
    ($n:expr, $f:ident ( $($arg:expr),* )) => {
        match $n {
            0..=3 => $f::<3>($($arg),*),
            4 => $f::<4>($($arg),*),
            5 => $f::<5>($($arg),*),
            6 => $f::<6>($($arg),*),
            7 => $f::<7>($($arg),*),
            _ => $f::<N_ASSETS>($($arg),*),
        }
    };
}

/// Sum of the first `N` holdings, in index order.
#[inline(always)]
fn sum_n<const N: usize>(h: &Holdings) -> f64 {
    let mut t = 0.0;
    for v in &h[..N] {
        t += *v;
    }
    t
}

/// [`sum`] of holdings whose asset classes in `idle` are known to be zero: the
/// same additions in the same order without the trailing zeros, so the same
/// result.
#[inline]
fn sum_live(h: &Holdings, idle: u16) -> f64 {
    if idle == 0 {
        return sum(h);
    }
    debug_assert!(
        (0..N_ASSETS).all(|i| idle & (1 << i) == 0 || h[i] == 0.0),
        "an idle asset class is held: {h:?} (idle mask {idle:#b})"
    );
    with_prefix!(live_prefix(idle), sum_n(h))
}

/// The asset classes a normalised mix buys, as a bit mask.
#[inline]
fn bought(mix: &Weights) -> u16 {
    let mut bits = 0;
    for (i, w) in mix.iter().enumerate() {
        bits |= u16::from(*w != 0.0) << i;
    }
    bits
}

/// `mix` scaled to sum to 1; an unusable mix (zero, negative or non-finite total)
/// puts everything in the cash sleeve.
fn normalized(mix: &Weights) -> Weights {
    let mut out = [0.0; N_ASSETS];
    let mut total = 0.0;
    for (o, w) in out.iter_mut().zip(mix) {
        if w.is_finite() && *w > 0.0 {
            *o = *w;
            total += *w;
        }
    }
    if total > 0.0 && total.is_finite() {
        // One division, not one per asset class: this runs on every contribution.
        let scale = 1.0 / total;
        for o in &mut out {
            *o *= scale;
        }
    } else {
        out = [0.0; N_ASSETS];
        out[AssetClass::Cash.index()] = 1.0;
    }
    out
}

thread_local! {
    /// The last mix [`normalized_memo`] was asked for and its normalised form.
    static NORMALIZED_MEMO: std::cell::Cell<Option<(Weights, Weights, u16)>> =
        const { std::cell::Cell::new(None) };
}

/// [`normalized`] with a one-entry memo, for callers that pass the same mix (the
/// household target) contribution after contribution. The memo is keyed on the bit
/// patterns of the input, so the result is exactly what `normalized` returns.
/// Also returns the asset classes the mix buys ([`bought`]).
fn normalized_memo(mix: &Weights) -> (Weights, u16) {
    NORMALIZED_MEMO.with(|memo| {
        if let Some((key, value, bits)) = memo.get() {
            let mut same = true;
            for (a, b) in key.iter().zip(mix) {
                same &= a.to_bits() == b.to_bits();
            }
            if same {
                return (value, bits);
            }
        }
        let value = normalized(mix);
        let bits = bought(&value);
        memo.set(Some((*mix, value, bits)));
        (value, bits)
    })
}

/// Splits a sale of `amount` (at most the bucket's value) across asset classes.
/// With `household = Some((totals, target))` it sells what is overweight household-wide
/// first, in proportion to the excess, then the rest pro rata. `total` is the
/// bucket's value, `sum(h)`, which every caller has already computed.
fn sell_split(
    h: &Holdings,
    total: f64,
    amount: f64,
    household: Option<(&Holdings, &Weights)>,
) -> Holdings {
    debug_assert!(total.to_bits() == sum(h).to_bits() || total.is_nan());
    let mut out = [0.0; N_ASSETS];
    if total <= 0.0 || amount <= 0.0 {
        return out;
    }
    if amount >= total {
        return *h;
    }
    let mut rest = amount;
    let mut left = *h;
    // What is left after the overweight classes: the whole bucket when nothing
    // was taken from them.
    let mut left_total = total;
    if let Some((totals, target)) = household {
        let after = sum(totals) - amount;
        let mut excess = [0.0; N_ASSETS];
        let mut excess_total = 0.0;
        for i in 0..N_ASSETS {
            excess[i] = (totals[i] - target[i] * after).max(0.0).min(h[i]);
            excess_total += excess[i];
        }
        if excess_total > 0.0 {
            let scale = (amount / excess_total).min(1.0);
            for i in 0..N_ASSETS {
                out[i] = excess[i] * scale;
                left[i] -= out[i];
                rest -= out[i];
            }
            left_total = sum(&left);
        }
    }
    if rest > 0.0 && left_total > 0.0 {
        let scale = (rest / left_total).min(1.0);
        for i in 0..N_ASSETS {
            out[i] += left[i] * scale;
        }
    }
    out
}

/// Removes `sells` from plain holdings.
fn sell_from(h: &mut Holdings, sells: &Holdings, everything: bool) {
    if everything {
        *h = [0.0; N_ASSETS];
        return;
    }
    for i in 0..N_ASSETS {
        h[i] = (h[i] - sells[i]).max(0.0);
    }
}

/// Moves up to `*rest` out of `pot`; returns what moved.
#[inline]
fn take(pot: &mut f64, rest: &mut f64) -> f64 {
    let x = pot.max(0.0).min(*rest);
    *pot -= x;
    *rest -= x;
    x
}

/// Sets plain holdings to `new`; returns dollars sold.
fn retarget(h: &mut Holdings, new: &Holdings) -> f64 {
    let mut sold = 0.0;
    for i in 0..N_ASSETS {
        sold += (h[i] - new[i]).max(0.0);
        h[i] = new[i];
    }
    sold
}

/// The per-holding part of [`Accounts::apply_returns`] for the first `N` asset
/// classes (the rest are idle): applies growth and fees, distributes the taxable
/// bucket's income, adds fees and the change in value to `inc`, and records which
/// classes are still held.
#[inline(always)]
fn grow<const N: usize>(
    acc: &mut Accounts,
    keep: &[f64; N_ASSETS],
    fee: &[f64; N_ASSETS],
    yields: &[f64; N_ASSETS],
    inc: &mut InvestmentIncome,
) {
    let mut fees = [0.0; N_ASSETS];
    let mut change = [0.0; N_ASSETS];
    let mut held = [false; N_ASSETS];
    for i in 0..N {
        let v = acc.taxable.value[i];
        let new = v * keep[i];
        let dist = (v * yields[i] / MONTHS_PER_YEAR).clamp(0.0, new.max(0.0));
        match DISTRIBUTION_KIND[i] {
            Distribution::Interest => inc.interest += dist,
            Distribution::Ordinary => inc.ordinary_dividends += dist,
            Distribution::Qualified => inc.qualified_dividends += dist,
        }
        acc.taxable.basis[i] += dist;
        acc.taxable.value[i] = new;
        fees[i] = v * fee[i];
        change[i] = new - v;
        held[i] = new != 0.0 || acc.taxable.basis[i] != 0.0;
    }
    for p in &mut acc.person {
        for h in [&mut p.traditional, &mut p.roth.value, &mut p.hsa] {
            for i in 0..N {
                let v = h[i];
                let new = v * keep[i];
                h[i] = new;
                fees[i] += v * fee[i];
                change[i] += new - v;
                held[i] |= new != 0.0;
            }
        }
    }
    let mut idle = ALL_ASSETS;
    for (i, held) in held.iter().enumerate().take(N) {
        idle &= !(u16::from(*held) << i);
    }
    acc.idle = idle;
    // Summed per asset class and then across classes: one short chain of adds
    // per class instead of a single chain through every holding, which the
    // processor cannot run in parallel. (The classes not processed would each add
    // a zero, which changes nothing.)
    for i in 0..N {
        inc.fees += fees[i];
        inc.net_change += change[i];
    }
}

impl TaxableBucket {
    /// Sells `sells[i]` of each asset class at average cost; returns the realised gain.
    fn sell(&mut self, sells: &Holdings, everything: bool) -> f64 {
        let mut gain = 0.0;
        for (i, &s) in sells.iter().enumerate() {
            let v = self.value[i];
            if s <= 0.0 || v <= 0.0 {
                continue;
            }
            if everything || s >= v {
                gain += v - self.basis[i];
                self.value[i] = 0.0;
                self.basis[i] = 0.0;
            } else {
                let cost = self.basis[i] * (s / v);
                gain += s - cost;
                self.basis[i] -= cost;
                self.value[i] = v - s;
            }
        }
        gain
    }

    /// Trades to the holdings `new` (same total): sales realise gains at average
    /// cost, purchases add to basis.
    fn trade_to(&mut self, new: &Holdings) -> Realized {
        let mut r = Realized::default();
        for (i, &target) in new.iter().enumerate() {
            let v = self.value[i];
            if target < v {
                let s = v - target;
                let cost = if target <= 0.0 {
                    self.basis[i]
                } else {
                    self.basis[i] * (s / v)
                };
                r.long_term_gain += s - cost;
                r.turnover += s;
                self.basis[i] -= cost;
            } else {
                self.basis[i] += target - v;
            }
            self.value[i] = target;
        }
        r
    }
}

impl RothBucket {
    /// Contributions plus seasoned conversions, capped at the bucket's value.
    fn accessible(&self) -> f64 {
        (self.contribution_basis.max(0.0) + self.seasoned_conversions.max(0.0))
            .min(sum(&self.value))
    }

    /// Applies the ordering rules to a distribution of `amount`; returns
    /// `(ordinary, penalty_base)`.
    fn classify(&mut self, amount: f64, ctx: &WithdrawContext) -> (f64, f64) {
        let mut rest = amount;
        take(&mut self.contribution_basis, &mut rest);
        take(&mut self.seasoned_conversions, &mut rest);
        // A withdrawal of exactly the accessible amount must not leave a rounding
        // residue to be classified (and penalised) as a conversion or earnings.
        if rest <= amount * ROUNDING_DUST {
            rest = 0.0;
        }
        let mut unseasoned = 0.0;
        for tranche in &mut self.unseasoned {
            unseasoned += take(tranche, &mut rest);
        }
        let earnings = rest;
        let of_age = ctx.owner_age >= EARLY_WITHDRAWAL_AGE;
        let qualified = of_age && self.years_open >= ROTH_ACCOUNT_SEASONING_YEARS;
        let ordinary = if qualified { 0.0 } else { earnings };
        let penalty = if of_age || ctx.penalty_exempt {
            0.0
        } else {
            unseasoned + earnings
        };
        (ordinary, penalty)
    }
}

impl Accounts {
    /// Opening balances pooled from `profile.accounts`. Accounts without `holdings`
    /// start at the portfolio target; default basis fractions are applied. Existing
    /// Roth accounts are assumed to be past their five-year clock. When the policy is
    /// [`AssetLocation::TaxEfficient`] and no account states its own holdings, the
    /// opening holdings are placed tax-efficiently (same household mix, no sale).
    pub fn from_profile(profile: &Profile) -> Accounts {
        let target = normalized(&profile.portfolio.target.0);
        let mut acc = Accounts::default();
        let mut explicit = false;
        for a in &profile.accounts {
            let balance = if a.balance.is_finite() {
                a.balance.max(0.0)
            } else {
                0.0
            };
            if balance <= 0.0 {
                continue;
            }
            let p = a.owner.min(MAX_PEOPLE - 1);
            let mix = match a.holdings.as_ref().and_then(|h| h.normalized()) {
                Some(m) if a.kind != AccountKind::Cash => {
                    explicit = true;
                    m
                }
                _ => target,
            };
            match a.kind {
                AccountKind::Cash => acc.cash += balance,
                AccountKind::Taxable => {
                    let basis = a.effective_cost_basis().max(0.0);
                    for (i, w) in mix.iter().enumerate() {
                        acc.taxable.value[i] += balance * w;
                        acc.taxable.basis[i] += basis * w;
                    }
                }
                AccountKind::Traditional => {
                    for (i, w) in mix.iter().enumerate() {
                        acc.person[p].traditional[i] += balance * w;
                    }
                }
                AccountKind::Roth => {
                    let roth = &mut acc.person[p].roth;
                    for (i, w) in mix.iter().enumerate() {
                        roth.value[i] += balance * w;
                    }
                    roth.contribution_basis += a.effective_roth_basis().clamp(0.0, balance);
                    roth.opened = true;
                    roth.years_open = ROTH_ACCOUNT_SEASONING_YEARS;
                }
                AccountKind::Hsa => {
                    for (i, w) in mix.iter().enumerate() {
                        acc.person[p].hsa[i] += balance * w;
                    }
                }
            }
        }
        if !explicit && profile.portfolio.location == AssetLocation::TaxEfficient {
            acc.locate(&target, true);
        }
        acc
    }

    fn holdings(&self, bucket: Bucket) -> Option<&Holdings> {
        match bucket {
            Bucket::Cash => None,
            Bucket::Taxable => Some(&self.taxable.value),
            Bucket::Traditional(p) => self.person.get(p).map(|x| &x.traditional),
            Bucket::Roth(p) => self.person.get(p).map(|x| &x.roth.value),
            Bucket::Hsa(p) => self.person.get(p).map(|x| &x.hsa),
        }
    }

    /// Market value of one bucket (0 for a person index out of range).
    pub fn total(&self, bucket: Bucket) -> f64 {
        match bucket {
            Bucket::Cash => self.cash,
            _ => self
                .holdings(bucket)
                .map_or(0.0, |h| sum_live(h, self.idle)),
        }
    }

    /// Market value by account kind, indexed by `AccountKind::index()`, summed over
    /// people.
    pub fn by_kind(&self) -> [f64; AccountKind::COUNT] {
        #[inline(always)]
        fn kinds<const N: usize>(a: &Accounts) -> [f64; AccountKind::COUNT] {
            let mut out = [0.0; AccountKind::COUNT];
            out[AccountKind::Cash.index()] = a.cash;
            out[AccountKind::Taxable.index()] = sum_n::<N>(&a.taxable.value);
            for p in &a.person {
                out[AccountKind::Traditional.index()] += sum_n::<N>(&p.traditional);
                out[AccountKind::Roth.index()] += sum_n::<N>(&p.roth.value);
                out[AccountKind::Hsa.index()] += sum_n::<N>(&p.hsa);
            }
            out
        }
        if self.idle == 0 {
            return kinds::<N_ASSETS>(self);
        }
        debug_assert!(
            (0..N_ASSETS).all(|i| self.idle & (1 << i) == 0
                || (self.taxable.value[i] == 0.0
                    && self.person.iter().all(|p| p.traditional[i] == 0.0
                        && p.roth.value[i] == 0.0
                        && p.hsa[i] == 0.0))),
            "an idle asset class is held (idle mask {:#b})",
            self.idle
        );
        with_prefix!(live_prefix(self.idle), kinds(self))
    }

    /// Everything spendable: cash, taxable, traditional, Roth and HSA.
    pub fn investable(&self) -> f64 {
        let k = self.by_kind();
        k[0] + k[1] + k[2] + k[3] + k[4]
    }

    /// Household-wide holdings by asset class across all invested buckets (cash bucket
    /// excluded).
    pub fn asset_totals(&self) -> Holdings {
        let mut out = self.taxable.value;
        for p in &self.person {
            for i in 0..N_ASSETS {
                out[i] += p.traditional[i] + p.roth.value[i] + p.hsa[i];
            }
        }
        out
    }

    /// Unrealised gain in the taxable bucket (negative for a net loss).
    pub fn unrealized_gain(&self) -> f64 {
        sum_live(&self.taxable.value, self.idle) - sum_live(&self.taxable.basis, self.idle)
    }

    /// Tells the accounts that holdings were written directly (not through one of
    /// the methods here), so nothing is known about which asset classes are held.
    pub fn holdings_changed(&mut self) {
        self.idle = 0;
    }

    /// Roth money the owner can withdraw today without tax or penalty at any age:
    /// contributions plus seasoned conversions (capped at the bucket's value).
    pub fn roth_accessible(&self, person: usize) -> f64 {
        self.person.get(person).map_or(0.0, |p| p.roth.accessible())
    }

    /// Applies one month of returns to every invested bucket, interest to cash, and
    /// deducts `fee_monthly` (expense ratio plus advisory fee, as a monthly fraction
    /// of end-of-month value) from invested buckets. `returns` are total returns for
    /// the month by asset class (a return below -100% is treated as -100%).
    /// `yields` are annual dividend/interest yields by asset class; a twelfth of each
    /// is the part of the taxable bucket's return that is distributed this month
    /// (interest for bonds and cash, non-qualified dividends for REITs, qualified
    /// dividends for stocks). Distributions are reinvested in place and raise basis.
    /// `cash_rate_monthly` comes from [`CashTerms::monthly_rate`].
    pub fn apply_returns(
        &mut self,
        returns: &[f64; N_ASSETS],
        cash_rate_monthly: f64,
        fee_monthly: f64,
        yields: &[f64; N_ASSETS],
    ) -> InvestmentIncome {
        let mut inc = InvestmentIncome::default();
        let interest = self.cash.max(0.0) * cash_rate_monthly;
        self.cash += interest;
        inc.interest = interest;
        inc.net_change = interest;

        let mut keep = [0.0; N_ASSETS];
        let mut fee = [0.0; N_ASSETS];
        for i in 0..N_ASSETS {
            let growth = (1.0 + returns[i]).max(0.0);
            fee[i] = growth * fee_monthly;
            keep[i] = growth - fee[i];
        }

        // Asset classes held nowhere are skipped: their holdings stay zero and add
        // nothing to any total. (Not when the return or yield of a class that would
        // be skipped is not finite: zero times infinity is not zero.)
        let mut prefix = supported_prefix(live_prefix(self.idle));
        let mut finite = true;
        for i in prefix..N_ASSETS {
            finite &= keep[i].is_finite() & fee[i].is_finite() & yields[i].is_finite();
        }
        if !finite {
            prefix = N_ASSETS;
        }
        with_prefix!(prefix, grow(self, &keep, &fee, yields, &mut inc));
        inc
    }

    /// Adds `amount` to a bucket, bought in the proportions `mix` (ignored for cash;
    /// an all-zero mix buys the cash sleeve). For Roth buckets the amount raises
    /// contribution basis; for the taxable bucket it raises cost basis. Limits are the
    /// caller's job. Amounts that are zero, negative or not finite are ignored, as is
    /// a person index out of range.
    pub fn contribute(&mut self, bucket: Bucket, amount: f64, mix: &Weights) {
        if !amount.is_finite() || amount <= 0.0 {
            return;
        }
        if bucket == Bucket::Cash {
            self.cash += amount;
            return;
        }
        // With a plain mix every purchase is made at the household target: the
        // same weights month after month.
        let (mix, bits) = if self.plain_mix {
            normalized_memo(mix)
        } else {
            let mix = normalized(mix);
            (mix, bought(&mix))
        };
        self.idle &= !bits;
        match bucket {
            Bucket::Cash => {}
            Bucket::Taxable => {
                for (i, w) in mix.iter().enumerate() {
                    self.taxable.value[i] += amount * w;
                    self.taxable.basis[i] += amount * w;
                }
            }
            Bucket::Traditional(p) => {
                if let Some(x) = self.person.get_mut(p) {
                    for (i, w) in mix.iter().enumerate() {
                        x.traditional[i] += amount * w;
                    }
                }
            }
            Bucket::Roth(p) => {
                if let Some(x) = self.person.get_mut(p) {
                    for (i, w) in mix.iter().enumerate() {
                        x.roth.value[i] += amount * w;
                    }
                    x.roth.contribution_basis += amount;
                    x.roth.opened = true;
                }
            }
            Bucket::Hsa(p) => {
                if let Some(x) = self.person.get_mut(p) {
                    for (i, w) in mix.iter().enumerate() {
                        x.hsa[i] += amount * w;
                    }
                }
            }
        }
    }

    /// The mix in which to buy `amount` of new money so the household moves toward
    /// `target`: asset classes that would be underweight after the purchase, in
    /// proportion to their shortfall. Pass the result to [`Accounts::contribute`].
    pub fn buy_mix(&self, amount: f64, target: &Weights) -> Weights {
        if self.plain_mix {
            // The caller's target is already normalised (it comes from
            // `glide_target`); `contribute` normalises again in any case.
            return *target;
        }
        let totals = self.asset_totals();
        let after = sum(&totals) + amount.max(0.0);
        let mut out = [0.0; N_ASSETS];
        let mut total = 0.0;
        for i in 0..N_ASSETS {
            out[i] = (target[i] * after - totals[i]).max(0.0);
            total += out[i];
        }
        if total > 0.0 && total.is_finite() {
            for o in &mut out {
                *o /= total;
            }
            out
        } else {
            normalized(target)
        }
    }

    /// Takes up to `amount` out of a bucket and reports its tax character.
    ///
    /// * Cash: no tax effect.
    /// * Taxable: realises long-term gain or loss at average cost.
    /// * Traditional: all ordinary income; penalty base before 59 and a half unless
    ///   `ctx.penalty_exempt`.
    /// * Roth: contributions first (free), then conversions oldest first (seasoned
    ///   ones free; unseasoned ones untaxed but penalty base before 59 and a half),
    ///   then earnings (ordinary income and penalty base before 59 and a half;
    ///   ordinary income only after it while the account is under five years old;
    ///   free after both).
    /// * HSA: free up to `ctx.qualified_medical`; the rest is ordinary income and,
    ///   before 65, penalty base (at the 20% HSA rate).
    pub fn withdraw(&mut self, bucket: Bucket, amount: f64, ctx: &WithdrawContext) -> Withdrawal {
        if amount.is_nan() || amount <= 0.0 {
            return Withdrawal::default();
        }
        if bucket == Bucket::Cash {
            let a = amount.min(self.cash.max(0.0));
            self.cash -= a;
            return Withdrawal {
                amount: a,
                ..Withdrawal::default()
            };
        }
        let Some(h) = self.holdings(bucket) else {
            return Withdrawal::default();
        };
        let total = sum_live(h, self.idle);
        if total <= 0.0 {
            return Withdrawal::default();
        }
        let everything = amount >= total;
        let a = amount.min(total);
        let sells = if everything {
            *h
        } else if let Some(target) = &ctx.prefer_overweight {
            let totals = self.asset_totals();
            sell_split(h, total, a, Some((&totals, target)))
        } else {
            sell_split(h, total, a, None)
        };
        let mut w = Withdrawal {
            amount: a,
            ..Withdrawal::default()
        };
        match bucket {
            Bucket::Cash => {}
            Bucket::Taxable => {
                w.long_term_gain = self.taxable.sell(&sells, everything);
            }
            Bucket::Traditional(p) => {
                sell_from(&mut self.person[p].traditional, &sells, everything);
                w.ordinary = a;
                if ctx.owner_age < EARLY_WITHDRAWAL_AGE && !ctx.penalty_exempt {
                    w.penalty_base = a;
                }
            }
            Bucket::Roth(p) => {
                let roth = &mut self.person[p].roth;
                sell_from(&mut roth.value, &sells, everything);
                let (ordinary, penalty) = roth.classify(a, ctx);
                w.ordinary = ordinary;
                w.penalty_base = penalty;
                if everything {
                    roth.contribution_basis = 0.0;
                    roth.seasoned_conversions = 0.0;
                    roth.unseasoned = [0.0; CONVERSION_SEASONING_YEARS];
                }
            }
            Bucket::Hsa(p) => {
                sell_from(&mut self.person[p].hsa, &sells, everything);
                let medical = if ctx.qualified_medical.is_nan() {
                    0.0
                } else {
                    ctx.qualified_medical.clamp(0.0, a)
                };
                w.ordinary = a - medical;
                if ctx.owner_age < HSA_PENALTY_FREE_AGE && !ctx.penalty_exempt {
                    w.penalty_base = w.ordinary;
                }
            }
        }
        w
    }

    /// Moves up to `amount` from a person's traditional bucket to their Roth bucket,
    /// asset mix preserved, and records the conversion tranche for the current year.
    /// Returns the amount converted, all of which is ordinary income (never
    /// penalised).
    pub fn convert_to_roth(&mut self, person: usize, amount: f64) -> f64 {
        if amount.is_nan() || amount <= 0.0 || person >= MAX_PEOPLE {
            return 0.0;
        }
        let p = &mut self.person[person];
        let total = sum(&p.traditional);
        if total <= 0.0 {
            return 0.0;
        }
        let everything = amount >= total;
        let a = amount.min(total);
        let moves = sell_split(&p.traditional, total, a, None);
        sell_from(&mut p.traditional, &moves, everything);
        for (v, m) in p.roth.value.iter_mut().zip(&moves) {
            *v += *m;
        }
        p.roth.unseasoned[CONVERSION_SEASONING_YEARS - 1] += a;
        p.roth.opened = true;
        a
    }

    /// Starts a new calendar year: ages conversion tranches by one year (the oldest
    /// becomes seasoned), advances each open Roth bucket's five-year clock and resets
    /// the year-to-date contribution counters. Call each January before planning.
    pub fn roll_year(&mut self) {
        for p in &mut self.person {
            let r = &mut p.roth;
            r.seasoned_conversions += r.unseasoned[0];
            for k in 1..CONVERSION_SEASONING_YEARS {
                r.unseasoned[k - 1] = r.unseasoned[k];
            }
            r.unseasoned[CONVERSION_SEASONING_YEARS - 1] = 0.0;
            if r.opened {
                r.years_open = r.years_open.saturating_add(1);
            }
        }
        self.ytd = ContributionYtd::default();
    }

    /// Realises up to `gain` of unrealised long-term gain in the taxable bucket by
    /// selling and immediately rebuying (no wash-sale rule applies to gains), which
    /// raises basis without changing holdings. Returns the gain realised. Used to
    /// harvest gains in the 0% bracket.
    pub fn harvest_gains(&mut self, gain: f64) -> f64 {
        if gain.is_nan() || gain <= 0.0 {
            return 0.0;
        }
        let t = &mut self.taxable;
        let mut available = 0.0;
        for i in 0..N_ASSETS {
            available += (t.value[i] - t.basis[i]).max(0.0);
        }
        if available <= 0.0 {
            return 0.0;
        }
        let scale = (gain / available).min(1.0);
        for i in 0..N_ASSETS {
            t.basis[i] += (t.value[i] - t.basis[i]).max(0.0) * scale;
        }
        available * scale
    }

    /// Trades every invested bucket so that the household-wide mix equals `target`,
    /// placing asset classes by `location`. Sales in tax-advantaged buckets are free;
    /// sales in the taxable bucket realise gains, which are returned.
    ///
    /// * [`AssetLocation::Mirror`]: every bucket ends at `target`.
    /// * [`AssetLocation::TaxEfficient`]: the household mix ends at `target`. The
    ///   taxable bucket sells only what it must (holdings above the household target
    ///   for that asset class) plus anything that can be sold without a gain, and
    ///   buys what the location order wants it to hold. The tax-advantaged buckets
    ///   absorb the rest: traditional accounts take bonds, REITs and cash first, Roth
    ///   and HSA take what remains (the highest-growth assets). So the target is met
    ///   by trading in tax-advantaged buckets first and taxable sales are minimal.
    pub fn rebalance_to(&mut self, target: &Weights, location: AssetLocation) -> Realized {
        self.idle = 0;
        let kept = self.set_aside_kept_stock();
        let r = self.rebalance_rest(target, location);
        self.restore_kept_stock(kept);
        r
    }

    /// Takes employer stock that is being kept out of the taxable bucket for the
    /// duration of a rebalance; returns its `(value, basis)`.
    fn set_aside_kept_stock(&mut self) -> (f64, f64) {
        if !self.keep_single_stock {
            return (0.0, 0.0);
        }
        let i = AssetClass::SingleStock.index();
        let kept = (self.taxable.value[i], self.taxable.basis[i]);
        self.taxable.value[i] = 0.0;
        self.taxable.basis[i] = 0.0;
        kept
    }

    fn restore_kept_stock(&mut self, kept: (f64, f64)) {
        let i = AssetClass::SingleStock.index();
        self.taxable.value[i] += kept.0;
        self.taxable.basis[i] += kept.1;
    }

    /// Moves the household mix toward `target` by trading only inside the
    /// tax-advantaged buckets: the taxable bucket is left exactly as it is, so
    /// nothing is realised. Each tax-advantaged bucket ends with the same mix, the
    /// household target less what the taxable bucket already holds (as far as their
    /// size allows). Returns the dollars sold.
    pub fn rebalance_sheltered(&mut self, target: &Weights) -> f64 {
        self.idle = 0;
        let w = normalized(target);
        let kept = self.set_aside_kept_stock();
        let mut sheltered = 0.0;
        for p in &self.person {
            sheltered += sum(&p.traditional) + sum(&p.roth.value) + sum(&p.hsa);
        }
        let mut turnover = 0.0;
        if sheltered > 0.0 {
            let total = sheltered + sum(&self.taxable.value);
            let mut want = [0.0; N_ASSETS];
            let mut want_total = 0.0;
            for i in 0..N_ASSETS {
                want[i] = (w[i] * total - self.taxable.value[i]).max(0.0);
                want_total += want[i];
            }
            if want_total > 0.0 {
                let mut new = [0.0; N_ASSETS];
                for p in &mut self.person {
                    for h in [&mut p.traditional, &mut p.roth.value, &mut p.hsa] {
                        let t = sum(h);
                        if t <= 0.0 {
                            continue;
                        }
                        for i in 0..N_ASSETS {
                            new[i] = want[i] / want_total * t;
                        }
                        turnover += retarget(h, &new);
                    }
                }
            }
        }
        self.restore_kept_stock(kept);
        turnover
    }

    fn rebalance_rest(&mut self, target: &Weights, location: AssetLocation) -> Realized {
        let w = normalized(target);
        match location {
            AssetLocation::Mirror => {
                let mut r = Realized::default();
                let t = sum(&self.taxable.value);
                let mut new = [0.0; N_ASSETS];
                for i in 0..N_ASSETS {
                    new[i] = w[i] * t;
                }
                if t > 0.0 {
                    r = self.taxable.trade_to(&new);
                }
                for p in &mut self.person {
                    for h in [&mut p.traditional, &mut p.roth.value, &mut p.hsa] {
                        let t = sum(h);
                        if t <= 0.0 {
                            continue;
                        }
                        for i in 0..N_ASSETS {
                            new[i] = w[i] * t;
                        }
                        r.turnover += retarget(h, &new);
                    }
                }
                r
            }
            AssetLocation::TaxEfficient => self.locate(&w, false),
        }
    }

    /// Tax-efficient placement at the household mix `w` (normalised). With
    /// `taxable_free` the taxable bucket is re-arranged without realising anything
    /// (opening balances only): total basis is kept and spread pro rata.
    fn locate(&mut self, w: &Weights, taxable_free: bool) -> Realized {
        let mut r = Realized::default();
        let t_tax = sum(&self.taxable.value);
        let mut deferred = 0.0;
        let mut tax_free = 0.0;
        for p in &self.person {
            deferred += sum(&p.traditional);
            tax_free += sum(&p.roth.value) + sum(&p.hsa);
        }
        let total = t_tax + deferred + tax_free;
        if total <= 0.0 {
            return r;
        }
        let mut desired = [0.0; N_ASSETS];
        for i in 0..N_ASSETS {
            desired[i] = w[i] * total;
        }

        // What the taxable bucket would hold if everything could move for free:
        // traditional takes from the front of the order, Roth/HSA from the back.
        let mut ideal = desired;
        let mut cap = deferred;
        for a in LOCATION_ORDER {
            let x = ideal[a.index()].min(cap);
            ideal[a.index()] -= x;
            cap -= x;
        }
        let mut cap = tax_free;
        for a in LOCATION_ORDER.iter().rev() {
            let x = ideal[a.index()].min(cap);
            ideal[a.index()] -= x;
            cap -= x;
        }

        let mut new_tax = self.taxable.value;
        if taxable_free {
            let basis = sum(&self.taxable.basis);
            let ideal_total = sum(&ideal);
            if t_tax > 0.0 && ideal_total > 0.0 {
                for i in 0..N_ASSETS {
                    new_tax[i] = ideal[i] / ideal_total * t_tax;
                    self.taxable.basis[i] = basis * (new_tax[i] / t_tax);
                }
                self.taxable.value = new_tax;
            }
        } else if t_tax > 0.0 {
            let mut proceeds = 0.0;
            for i in 0..N_ASSETS {
                let v = new_tax[i];
                // Free to sell down to the ideal when there is no gain to realise;
                // otherwise sell only what exceeds the whole household's target.
                let limit = if self.taxable.basis[i] >= v {
                    ideal[i]
                } else {
                    desired[i]
                };
                if v > limit {
                    proceeds += v - limit;
                    new_tax[i] = limit;
                }
            }
            if proceeds > 0.0 {
                let mut room = [0.0; N_ASSETS];
                let mut room_total = 0.0;
                for i in 0..N_ASSETS {
                    room[i] = (ideal[i] - new_tax[i]).max(0.0);
                    room_total += room[i];
                }
                if room_total > 0.0 {
                    // The rooms always add up to at least the proceeds, because both
                    // `ideal` and the pre-sale holdings sum to the bucket's value.
                    let scale = (proceeds / room_total).min(1.0);
                    for i in 0..N_ASSETS {
                        new_tax[i] += room[i] * scale;
                    }
                }
                r = self.taxable.trade_to(&new_tax);
            }
        }

        // The sheltered buckets hold the household target less the taxable bucket.
        let mut rest = [0.0; N_ASSETS];
        for i in 0..N_ASSETS {
            rest[i] = (desired[i] - new_tax[i]).max(0.0);
        }
        let mut trad = [0.0; N_ASSETS];
        let mut cap = deferred;
        for a in LOCATION_ORDER {
            let x = rest[a.index()].min(cap);
            trad[a.index()] = x;
            rest[a.index()] -= x;
            cap -= x;
        }
        let trad_total = sum(&trad);
        let free_total = sum(&rest);
        let mut new = [0.0; N_ASSETS];
        for p in &mut self.person {
            let t = sum(&p.traditional);
            if t > 0.0 && trad_total > 0.0 {
                for i in 0..N_ASSETS {
                    new[i] = trad[i] / trad_total * t;
                }
                r.turnover += retarget(&mut p.traditional, &new);
            }
            for h in [&mut p.roth.value, &mut p.hsa] {
                let t = sum(h);
                if t > 0.0 && free_total > 0.0 {
                    for i in 0..N_ASSETS {
                        new[i] = rest[i] / free_total * t;
                    }
                    r.turnover += retarget(h, &new);
                }
            }
        }
        r
    }

    /// Largest absolute difference between the current household mix and `target`,
    /// for band rebalancing (0 when nothing is invested).
    pub fn max_drift(&self, target: &Weights) -> f64 {
        let mut totals = self.asset_totals();
        if self.keep_single_stock {
            let i = AssetClass::SingleStock.index();
            totals[i] = (totals[i] - self.taxable.value[i]).max(0.0);
        }
        let total = sum(&totals);
        if total <= 0.0 {
            return 0.0;
        }
        let mut drift: f64 = 0.0;
        for i in 0..N_ASSETS {
            drift = drift.max((totals[i] / total - target[i]).abs());
        }
        drift
    }

    /// Moves a deceased person's buckets to the survivor (spousal rollover: the
    /// survivor treats them as their own, including Roth basis and conversion
    /// tranches) or, with no survivor, leaves them in place for the estate. Either
    /// way the taxable bucket's basis steps up to market value (a full step-up, as in
    /// community-property states; common-law states step up only the decedent's
    /// half).
    pub fn inherit(&mut self, deceased: usize, survivor: Option<usize>) {
        if deceased >= MAX_PEOPLE {
            return;
        }
        self.idle = 0;
        self.taxable.basis = self.taxable.value;
        let Some(s) = survivor else {
            return;
        };
        if s >= MAX_PEOPLE || s == deceased {
            return;
        }
        let d = std::mem::take(&mut self.person[deceased]);
        let p = &mut self.person[s];
        for i in 0..N_ASSETS {
            p.traditional[i] += d.traditional[i];
            p.roth.value[i] += d.roth.value[i];
            p.hsa[i] += d.hsa[i];
        }
        p.roth.contribution_basis += d.roth.contribution_basis;
        p.roth.seasoned_conversions += d.roth.seasoned_conversions;
        for k in 0..CONVERSION_SEASONING_YEARS {
            p.roth.unseasoned[k] += d.roth.unseasoned[k];
        }
        p.roth.opened |= d.roth.opened;
        p.roth.years_open = p.roth.years_open.max(d.roth.years_open);
    }

    /// HSA contribution room left this year for `person`: the self-only or family
    /// limit plus the age-55 catch-up, less contributions so far. The family limit is
    /// shared: what the other person has contributed beyond their own catch-up counts
    /// against it.
    pub fn hsa_room(
        &self,
        person: usize,
        age: f64,
        other_age: f64,
        family: bool,
        limits: &ContributionLimits,
    ) -> f64 {
        if person >= MAX_PEOPLE {
            return 0.0;
        }
        let catch_up = |a: f64| {
            if a >= HSA_CATCH_UP_AGE {
                limits.hsa_catch_up
            } else {
                0.0
            }
        };
        let mut used = self.ytd.hsa[person];
        let base = if family {
            for (o, other) in self.ytd.hsa.iter().enumerate() {
                if o != person {
                    used += (other - catch_up(other_age)).max(0.0);
                }
            }
            limits.hsa_family
        } else {
            limits.hsa_self
        };
        (base + catch_up(age) - used).max(0.0)
    }

    /// Deposits up to `amount` into a person's HSA, capped at `room` (from
    /// [`Accounts::hsa_room`]), and counts it toward the year's limit. Use this for
    /// payroll and employer HSA contributions. Returns the amount deposited.
    pub fn contribute_hsa(&mut self, person: usize, amount: f64, room: f64, mix: &Weights) -> f64 {
        if person >= MAX_PEOPLE || amount.is_nan() || room.is_nan() {
            return 0.0;
        }
        let x = amount.min(room).max(0.0);
        if x > 0.0 {
            self.contribute(Bucket::Hsa(person), x, mix);
            self.ytd.hsa[person] += x;
        }
        x
    }

    /// The savings waterfall: deposits a month's investable `surplus` (new money, not
    /// already in any bucket) in this order and returns what went where.
    ///
    /// 1. **HSA** for each eligible person, up to the year's remaining limit.
    /// 2. **IRA** for each present person, up to the remaining limit (with the age-50
    ///    catch-up) and the household's earned income. Rule: a *traditional* IRA when
    ///    the contribution is deductible and today's marginal rate is above the rate
    ///    expected in retirement (`marginal_rate > retirement_rate`: deduct now, pay
    ///    less later); otherwise a *Roth* IRA up to the share of the limit the MAGI
    ///    phase-out allows, with any remaining deductible room going to a traditional
    ///    IRA.
    /// 3. **After-tax 401(k)** with in-plan Roth conversion where the plan allows it,
    ///    up to the overall additions limit.
    /// 4. **Taxable** for the rest.
    ///
    /// Every purchase uses [`Accounts::buy_mix`] toward `target`, so new savings
    /// steer the household mix to target. Year-to-date amounts are tracked in
    /// [`Accounts::ytd`].
    pub fn save_surplus(
        &mut self,
        surplus: f64,
        ctx: &SavingsContext,
        target: &Weights,
    ) -> SavingsFlows {
        let mut flows = SavingsFlows::default();
        if !surplus.is_finite() || surplus <= 0.0 {
            return flows;
        }
        let mut rest = surplus;
        let limits = &ctx.limits;

        for p in 0..MAX_PEOPLE {
            let who = &ctx.person[p];
            if rest <= 0.0 || !who.present || !who.hsa_eligible {
                continue;
            }
            let other_age = ctx.person[MAX_PEOPLE - 1 - p].age;
            let room = self.hsa_room(p, who.age, other_age, ctx.hsa_family, limits);
            let x = rest.min(room);
            if x > 0.0 {
                let mix = self.buy_mix(x, target);
                flows.hsa[p] = self.contribute_hsa(p, x, room, &mix);
                rest -= flows.hsa[p];
            }
        }

        let s = ctx.filing_status.index();
        let (start, end) = (
            limits.roth_ira_phaseout_start[s],
            limits.roth_ira_phaseout_end[s],
        );
        let roth_share = if ctx.magi <= start {
            1.0
        } else if ctx.magi >= end || end <= start {
            0.0
        } else {
            (end - ctx.magi) / (end - start)
        };
        let prefer_traditional = ctx.marginal_rate > ctx.retirement_rate;
        for p in 0..MAX_PEOPLE {
            let who = &ctx.person[p];
            if rest <= 0.0 || !who.present {
                continue;
            }
            let limit = limits.ira
                + if who.age >= CATCH_UP_AGE {
                    limits.ira_catch_up
                } else {
                    0.0
                };
            let ytd_all: f64 = self.ytd.ira.iter().sum();
            let room = (limit - self.ytd.ira[p])
                .min(ctx.earned_income - ytd_all)
                .max(0.0);
            let mut ira = rest.min(room);
            if ira <= 0.0 {
                continue;
            }
            let mut traditional = 0.0;
            if who.traditional_deductible && prefer_traditional {
                traditional = ira;
                ira = 0.0;
            }
            let roth = ira.min((limit * roth_share - self.ytd.roth_ira[p]).max(0.0));
            if who.traditional_deductible {
                traditional += ira - roth;
            }
            if roth > 0.0 {
                let mix = self.buy_mix(roth, target);
                self.contribute(Bucket::Roth(p), roth, &mix);
                self.ytd.roth_ira[p] += roth;
            }
            if traditional > 0.0 {
                let mix = self.buy_mix(traditional, target);
                self.contribute(Bucket::Traditional(p), traditional, &mix);
            }
            self.ytd.ira[p] += roth + traditional;
            flows.roth_ira[p] = roth;
            flows.traditional_ira[p] = traditional;
            rest -= roth + traditional;
        }

        for p in 0..MAX_PEOPLE {
            let who = &ctx.person[p];
            if rest <= 0.0 || !who.present || !who.after_tax_401k {
                continue;
            }
            let limit = limits.k401_total
                + if who.age >= CATCH_UP_AGE {
                    limits.k401_catch_up
                } else {
                    0.0
                };
            let x = rest.min((limit - self.ytd.k401_additions[p]).max(0.0));
            if x > 0.0 {
                let mix = self.buy_mix(x, target);
                self.contribute(Bucket::Roth(p), x, &mix);
                self.ytd.k401_additions[p] += x;
                flows.after_tax_401k[p] = x;
                rest -= x;
            }
        }

        if rest > 0.0 {
            let mix = self.buy_mix(rest, target);
            self.contribute(Bucket::Taxable, rest, &mix);
            flows.taxable = rest;
        }
        flows
    }

    /// One pay period of workplace-plan contributions for `person`: computes the
    /// employee deferral and the employer match, deposits them and returns the
    /// amounts.
    ///
    /// * Employee: `plan.employee_fraction` of `pay.gross_pay`, or with
    ///   `plan.employee_max` the remaining annual limit spread evenly over the months
    ///   left in the year. Capped at the elective deferral limit (plus the catch-up
    ///   from age 50, by age at year end) less deferrals so far this year, and at the
    ///   pay itself.
    /// * `plan.roth_fraction` of it goes to the Roth bucket, the rest to traditional.
    /// * Employer: `plan.match_rate` per dollar on the employee's deferral up to
    ///   `plan.match_up_to` of this period's pay (per-period match, no year-end
    ///   true-up), capped by the overall additions limit; deposited to traditional.
    pub fn payroll_deferral(
        &mut self,
        person: usize,
        plan: &EmployerPlan,
        pay: &PayPeriod,
        limits: &ContributionLimits,
        target: &Weights,
    ) -> PayrollDeferral {
        let PayPeriod {
            gross_pay,
            month,
            age,
        } = *pay;
        let mut out = PayrollDeferral::default();
        if person >= MAX_PEOPLE || !gross_pay.is_finite() || gross_pay <= 0.0 {
            return out;
        }
        // SECURE 2.0: a larger catch-up replaces the age-50 one in the years a
        // person turns 60 to 63.
        let catch_up = if (SUPER_CATCH_UP_FROM..=SUPER_CATCH_UP_TO).contains(&age)
            && limits.k401_catch_up_60_63 > 0.0
        {
            limits.k401_catch_up_60_63
        } else if age >= CATCH_UP_AGE {
            limits.k401_catch_up
        } else {
            0.0
        };
        let room = (limits.k401_employee + catch_up - self.ytd.k401_employee[person]).max(0.0);
        // Pay above the 401(a)(17) limit earns no deferral percentage and no match:
        // spread over the year, a twelfth of the limit per month.
        let eligible_pay = if limits.compensation_limit > 0.0 {
            gross_pay.min(limits.compensation_limit / MONTHS_PER_YEAR)
        } else {
            gross_pay
        };
        let desired = if plan.employee_max {
            let months_left = f64::from(13 - month.clamp(1, 12));
            room / months_left
        } else {
            plan.employee_fraction * eligible_pay
        };
        let additions_room =
            (limits.k401_total + catch_up - self.ytd.k401_additions[person]).max(0.0);
        let employee = desired
            .min(room)
            .min(gross_pay)
            .min(additions_room)
            .max(0.0);
        let additions_room = additions_room - employee;
        let matched = employee.min(plan.match_up_to.max(0.0) * eligible_pay);
        out.employer = (plan.match_rate.max(0.0) * matched).min(additions_room);
        out.employee_roth = employee * plan.roth_fraction.clamp(0.0, 1.0);
        out.employee_pretax = employee - out.employee_roth;

        let pretax = out.employee_pretax + out.employer;
        if pretax > 0.0 {
            let mix = self.buy_mix(pretax, target);
            self.contribute(Bucket::Traditional(person), pretax, &mix);
        }
        if out.employee_roth > 0.0 {
            let mix = self.buy_mix(out.employee_roth, target);
            self.contribute(Bucket::Roth(person), out.employee_roth, &mix);
        }
        self.ytd.k401_employee[person] += employee;
        self.ytd.k401_additions[person] += employee + out.employer;
        out
    }
}

/// Straight line through `(x0, y0)` and `(x1, y1)` at `x`.
fn lerp(x: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> f64 {
    if x1 == x0 {
        y0
    } else {
        y0 + (y1 - y0) * (x - x0) / (x1 - x0)
    }
}

/// Equity (growth-asset) fraction `years` before retirement under a glide path, given
/// the fraction `base` of the plain target.
fn glide_equity(glide: &GlidePath, base: f64, years: f64) -> f64 {
    match glide {
        GlidePath::None => base,
        GlidePath::TargetDate {
            years_before,
            equity_at_retirement,
            years_after,
            equity_final,
        } => {
            if years >= *years_before {
                base
            } else if years >= 0.0 {
                lerp(years, 0.0, *equity_at_retirement, *years_before, base)
            } else if -years < *years_after {
                lerp(
                    -years,
                    0.0,
                    *equity_at_retirement,
                    *years_after,
                    *equity_final,
                )
            } else {
                *equity_final
            }
        }
        GlidePath::BondTent {
            years_before,
            equity_at_retirement,
            years_after,
        } => {
            if years >= *years_before {
                base
            } else if years >= 0.0 {
                lerp(years, 0.0, *equity_at_retirement, *years_before, base)
            } else if -years < *years_after {
                lerp(-years, 0.0, *equity_at_retirement, *years_after, base)
            } else {
                base
            }
        }
        GlidePath::Custom { points } => {
            // Nearest point on each side of `years`, found without sorting.
            let mut below: Option<(f64, f64)> = None;
            let mut above: Option<(f64, f64)> = None;
            for pt in points {
                let (x, e) = (pt.years_to_retirement, pt.equity_fraction);
                if x.is_nan() || e.is_nan() {
                    continue;
                }
                if x <= years && below.map_or(true, |(bx, _)| x > bx) {
                    below = Some((x, e));
                }
                if x >= years && above.map_or(true, |(ax, _)| x < ax) {
                    above = Some((x, e));
                }
            }
            match (below, above) {
                (Some((x0, e0)), Some((x1, e1))) => lerp(years, x0, e0, x1, e1),
                (Some((_, e)), None) | (None, Some((_, e))) => e,
                (None, None) => base,
            }
        }
    }
}

/// The target mix in force `years_to_retirement` years before (positive) or after
/// (negative) retirement under the policy's glide path: the growth/defensive split
/// moves along the glide while proportions within each group follow `policy.target`.
/// `None` years (retirement date unknown yet) returns the plain target.
///
/// When the glide needs a group the target does not hold at all, growth money goes to
/// US stocks and defensive money to bonds. An unusable target falls back to all cash.
pub fn glide_target(policy: &PortfolioPolicy, years_to_retirement: Option<f64>) -> Weights {
    let base = normalized(&policy.target.0);
    let Some(years) = years_to_retirement else {
        return base;
    };
    if years.is_nan() || matches!(policy.glide, GlidePath::None) {
        return base;
    }
    let mut growth = 0.0;
    for a in AssetClass::ALL {
        if !a.is_defensive() {
            growth += base[a.index()];
        }
    }
    let defensive = 1.0 - growth;
    let equity = glide_equity(&policy.glide, growth, years);
    let equity = if equity.is_nan() {
        growth
    } else {
        equity.clamp(0.0, 1.0)
    };
    let mut out = [0.0; N_ASSETS];
    for a in AssetClass::ALL {
        let i = a.index();
        if a.is_defensive() {
            if defensive > 0.0 {
                out[i] = base[i] / defensive * (1.0 - equity);
            }
        } else if growth > 0.0 {
            out[i] = base[i] / growth * equity;
        }
    }
    if growth <= 0.0 {
        out[AssetClass::UsStocks.index()] = equity;
    }
    if defensive <= 0.0 {
        out[AssetClass::Bonds.index()] = 1.0 - equity;
    }
    out
}

/// Blended cash terms from the profile's cash accounts, weighted by balance. With no
/// cash balance the default yield ([`CashYield::default`]) applies.
pub fn cash_terms(profile: &Profile) -> CashTerms {
    let mut tracking = 0.0;
    let mut spread_sum = 0.0;
    let mut fixed = 0.0;
    let mut fixed_sum = 0.0;
    for a in &profile.accounts {
        if a.kind != AccountKind::Cash || !a.balance.is_finite() || a.balance <= 0.0 {
            continue;
        }
        match a.cash_yield {
            CashYield::TracksBills { spread } => {
                tracking += a.balance;
                spread_sum += a.balance * spread;
            }
            CashYield::Fixed { rate } => {
                fixed += a.balance;
                fixed_sum += a.balance * rate;
            }
        }
    }
    let total = tracking + fixed;
    if total <= 0.0 {
        return match CashYield::default() {
            CashYield::TracksBills { spread } => CashTerms {
                tracking_share: 1.0,
                spread,
                fixed_rate: 0.0,
            },
            CashYield::Fixed { rate } => CashTerms {
                tracking_share: 0.0,
                spread: 0.0,
                fixed_rate: rate,
            },
        };
    }
    CashTerms {
        tracking_share: tracking / total,
        spread: if tracking > 0.0 {
            spread_sum / tracking
        } else {
            0.0
        },
        fixed_rate: if fixed > 0.0 { fixed_sum / fixed } else { 0.0 },
    }
}

#[cfg(test)]
mod tests;
