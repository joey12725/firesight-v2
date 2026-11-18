//! Financial accounts as the user describes them. The engine pools them into tax
//! buckets (see `crate::accounts`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Allocation;

/// One financial account, or a group of similar accounts added together.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Account {
    /// Stable identifier used by patches, for example "alex_401k".
    pub id: String,
    /// Display name.
    pub name: String,
    /// Whose account it is: 0 for the first person, 1 for the second. Matters for
    /// retirement accounts and HSAs (age rules, required distributions, inheritance);
    /// cash and taxable accounts are treated as jointly owned.
    pub owner: usize,
    /// Tax treatment of the account.
    pub kind: AccountKind,
    /// Current balance in dollars.
    pub balance: f64,
    /// Taxable accounts only: what was originally paid for the holdings, which decides
    /// how much of a sale is taxable gain. Leave empty to assume 60% of the balance.
    pub cost_basis: Option<f64>,
    /// Roth accounts only: total direct contributions, which can be withdrawn at any
    /// age without tax or penalty. Leave empty to assume 50% of the balance.
    pub roth_basis: Option<f64>,
    /// Cash accounts only: the interest the account earns.
    pub cash_yield: CashYield,
    /// The account's current investment mix, if it differs from the portfolio target.
    /// Leave empty to assume it is already at target. Ignored for cash accounts.
    pub holdings: Option<Allocation>,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            owner: 0,
            kind: AccountKind::Taxable,
            balance: 0.0,
            cost_basis: None,
            roth_basis: None,
            cash_yield: CashYield::default(),
            holdings: None,
        }
    }
}

impl Account {
    /// Default share of a taxable balance assumed to be cost basis.
    pub const DEFAULT_COST_BASIS_FRACTION: f64 = 0.6;
    /// Default share of a Roth balance assumed to be contributions.
    pub const DEFAULT_ROTH_BASIS_FRACTION: f64 = 0.5;

    /// Cost basis in dollars (taxable accounts), applying the default when unset.
    pub fn effective_cost_basis(&self) -> f64 {
        self.cost_basis
            .unwrap_or(self.balance * Self::DEFAULT_COST_BASIS_FRACTION)
    }

    /// Roth contribution basis in dollars, applying the default when unset.
    pub fn effective_roth_basis(&self) -> f64 {
        self.roth_basis
            .unwrap_or(self.balance * Self::DEFAULT_ROTH_BASIS_FRACTION)
    }
}

/// Tax treatment of an account.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    /// Checking, savings, money-market, CDs. Earns interest, taxed each year.
    Cash,
    /// An ordinary brokerage account: dividends are taxed each year and gains when sold.
    #[default]
    Taxable,
    /// Pre-tax retirement money: traditional 401(k), 403(b), 457, traditional IRA,
    /// SEP. Taxed as income when withdrawn.
    Traditional,
    /// After-tax retirement money: Roth 401(k) and Roth IRA. Tax-free in retirement.
    Roth,
    /// Health savings account: tax-free for medical costs.
    Hsa,
}

impl AccountKind {
    /// Number of account kinds.
    pub const COUNT: usize = 5;

    /// All kinds in index order.
    pub const ALL: [AccountKind; AccountKind::COUNT] = [
        AccountKind::Cash,
        AccountKind::Taxable,
        AccountKind::Traditional,
        AccountKind::Roth,
        AccountKind::Hsa,
    ];

    /// Dense index for per-kind arrays.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Stable snake_case name used in output column names.
    pub const fn key(self) -> &'static str {
        match self {
            AccountKind::Cash => "cash",
            AccountKind::Taxable => "taxable",
            AccountKind::Traditional => "traditional",
            AccountKind::Roth => "roth",
            AccountKind::Hsa => "hsa",
        }
    }
}

/// Interest earned by a cash account.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CashYield {
    /// The yield follows short-term Treasury bill rates, as high-yield savings and
    /// money-market funds do.
    TracksBills {
        /// Annual amount added to the T-bill rate, as a decimal; usually negative
        /// (-0.005 = half a point below bills). The yield never goes below zero.
        #[serde(default)]
        spread: f64,
    },
    /// A fixed nominal rate, for example a checking account paying nothing.
    Fixed {
        /// Annual interest rate as a decimal (0.0 for a typical checking account).
        rate: f64,
    },
}

impl Default for CashYield {
    fn default() -> Self {
        CashYield::TracksBills { spread: -0.005 }
    }
}
