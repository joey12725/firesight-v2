//! Account buckets. **Interface stub: method bodies are `todo!()`.**
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
//! * All balances are nominal dollars. No method allocates.
//! * Money is conserved: every method returns exactly what moved, and the sum of
//!   returned flows equals the change in [`Accounts::total`]. `sim`'s conservation
//!   property test depends on this.
//! * A withdrawal never overdraws: it returns the amount actually taken, which is
//!   less than requested when the bucket runs out.
//! * These methods do not decide *which* bucket to use; `decumulation` and the
//!   savings waterfall in `sim` do.

// STUB: remove this allow when the module is implemented.
#![allow(dead_code, unused_variables)]

use crate::model::{AccountKind, AssetLocation, PortfolioPolicy, Profile, N_ASSETS};
use crate::MAX_PEOPLE;

/// Dollars held in each asset class, indexed by `AssetClass::index()`.
pub type Holdings = [f64; N_ASSETS];

/// Portfolio weights summing to 1, indexed by `AssetClass::index()`.
pub type Weights = [f64; N_ASSETS];

/// Number of years a Roth conversion must age before it can be withdrawn penalty-free.
pub const CONVERSION_SEASONING_YEARS: usize = 5;

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

/// Every liquid balance of the household.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Accounts {
    /// Cash accounts (checking, savings, emergency fund, retirement cash bucket).
    pub cash: f64,
    /// Taxable brokerage.
    pub taxable: TaxableBucket,
    /// Per-person retirement and health accounts.
    pub person: [PersonAccounts; MAX_PEOPLE],
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
}

/// What a withdrawal realised, for the tax return.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Withdrawal {
    /// Dollars actually taken out (less than requested if the bucket ran out).
    pub amount: f64,
    /// Portion taxed as ordinary income (traditional distributions, Roth earnings
    /// before qualification, non-medical HSA).
    pub ordinary: f64,
    /// Long-term capital gain realised (taxable bucket).
    pub long_term_gain: f64,
    /// Portion subject to the 10% early-distribution penalty (20% for HSA).
    pub penalty_base: f64,
}

/// Facts a withdrawal needs to classify itself.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct WithdrawContext {
    /// The account owner's age in years (for 59 and a half and HSA age 65 rules).
    pub owner_age: f64,
    /// True if the money pays qualified medical expenses (HSA only).
    pub medical: bool,
    /// True if a 72(t) schedule or another penalty exception covers this withdrawal.
    pub penalty_exempt: bool,
    /// Sell assets in proportion to this mix; `None` sells pro rata to current
    /// holdings. Lets withdrawals double as rebalancing.
    pub prefer_overweight: Option<Weights>,
}

/// Gains realised by selling in the taxable bucket during a rebalance.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Realized {
    /// Net long-term capital gain (negative for a net loss).
    pub long_term_gain: f64,
    /// Dollars traded, for turnover reporting.
    pub turnover: f64,
}

impl Accounts {
    /// Opening balances pooled from `profile.accounts`. Accounts without `holdings`
    /// start at the portfolio target; default basis fractions are applied.
    pub fn from_profile(profile: &Profile) -> Accounts {
        todo!("accounts owner")
    }

    /// Market value of one bucket.
    pub fn total(&self, bucket: Bucket) -> f64 {
        todo!("accounts owner")
    }

    /// Market value by account kind, indexed by `AccountKind::index()`, summed over
    /// people.
    pub fn by_kind(&self) -> [f64; AccountKind::COUNT] {
        todo!("accounts owner")
    }

    /// Everything spendable: cash, taxable, traditional, Roth and HSA.
    pub fn investable(&self) -> f64 {
        todo!("accounts owner")
    }

    /// Household-wide holdings by asset class across all invested buckets (cash bucket
    /// excluded).
    pub fn asset_totals(&self) -> Holdings {
        todo!("accounts owner")
    }

    /// Unrealised gain in the taxable bucket.
    pub fn unrealized_gain(&self) -> f64 {
        todo!("accounts owner")
    }

    /// Roth money the owner can withdraw today without tax or penalty: contributions
    /// plus seasoned conversions (capped at the bucket's value).
    pub fn roth_accessible(&self, person: usize) -> f64 {
        todo!("accounts owner")
    }

    /// Applies one month of returns to every invested bucket, interest to cash, and
    /// deducts `fee_monthly` (expense ratio plus advisory fee, as a monthly fraction)
    /// from invested buckets. `yields` are annual dividend/interest yields by asset
    /// class and decide how much of the taxable bucket's return is taxable this year;
    /// distributions are reinvested in place and raise basis.
    pub fn apply_returns(
        &mut self,
        returns: &[f64; N_ASSETS],
        cash_rate_monthly: f64,
        fee_monthly: f64,
        yields: &[f64; N_ASSETS],
    ) -> InvestmentIncome {
        todo!("accounts owner")
    }

    /// Adds `amount` to a bucket, bought in the proportions `mix`. For Roth buckets
    /// the amount raises contribution basis; for the taxable bucket it raises cost
    /// basis. Limits are the caller's job.
    pub fn contribute(&mut self, bucket: Bucket, amount: f64, mix: &Weights) {
        todo!("accounts owner")
    }

    /// Takes up to `amount` out of a bucket and reports its tax character.
    pub fn withdraw(&mut self, bucket: Bucket, amount: f64, ctx: &WithdrawContext) -> Withdrawal {
        todo!("accounts owner")
    }

    /// Moves up to `amount` from a person's traditional bucket to their Roth bucket,
    /// asset mix preserved, and records the conversion tranche for the current year.
    /// Returns the amount converted, all of which is ordinary income.
    pub fn convert_to_roth(&mut self, person: usize, amount: f64) -> f64 {
        todo!("accounts owner")
    }

    /// Ages conversion tranches by one year. Call each January before planning.
    pub fn roll_year(&mut self) {
        todo!("accounts owner")
    }

    /// Trades every invested bucket so that the household-wide mix equals `target`,
    /// placing asset classes by `location`. Sales in tax-advantaged buckets are free;
    /// sales in the taxable bucket realise gains, which are returned.
    pub fn rebalance_to(&mut self, target: &Weights, location: AssetLocation) -> Realized {
        todo!("accounts owner")
    }

    /// Largest absolute difference between the current household mix and `target`,
    /// for band rebalancing.
    pub fn max_drift(&self, target: &Weights) -> f64 {
        todo!("accounts owner")
    }

    /// Moves a deceased person's buckets to the survivor (spousal rollover) or, with
    /// no survivor, leaves them for the estate.
    pub fn inherit(&mut self, deceased: usize, survivor: Option<usize>) {
        todo!("accounts owner")
    }
}

/// The target mix in force `years_to_retirement` years before (positive) or after
/// (negative) retirement under the policy's glide path: the growth/defensive split
/// moves along the glide while proportions within each group follow `policy.target`.
/// `None` years (retirement date unknown yet) returns the plain target.
pub fn glide_target(policy: &PortfolioPolicy, years_to_retirement: Option<f64>) -> Weights {
    todo!("accounts owner")
}

/// Blended cash terms from the profile's cash accounts, weighted by balance.
pub fn cash_terms(profile: &Profile) -> CashTerms {
    todo!("accounts owner")
}
