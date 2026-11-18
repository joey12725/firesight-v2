//! Non-mortgage debts and how they are paid down.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A loan or revolving balance that is not a mortgage (mortgages belong to a property).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Debt {
    /// Stable identifier used by patches, for example "student_loan".
    pub id: String,
    /// Display name.
    pub name: String,
    /// Kind of debt. Student loan interest is partly tax-deductible; the others are not.
    pub kind: DebtKind,
    /// Amount owed today, in dollars.
    pub balance: f64,
    /// Annual interest rate as a decimal (0.065 = 6.5% APR). The rate is fixed in
    /// nominal terms.
    pub rate: f64,
    /// Required monthly payment in dollars. Leave empty to derive it from
    /// `remaining_months`; a credit card with neither uses 2% of the balance (minimum
    /// $35), any other debt with neither is assumed to be paid off over five years.
    pub payment: Option<f64>,
    /// Months left on the loan. Leave empty for revolving debt or when `payment` is given.
    pub remaining_months: Option<u32>,
}

impl Default for Debt {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: DebtKind::Other,
            balance: 0.0,
            rate: 0.07,
            payment: None,
            remaining_months: None,
        }
    }
}

/// Kind of non-mortgage debt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DebtKind {
    /// Student loan.
    Student,
    /// Car loan.
    Auto,
    /// Credit card or other revolving balance.
    CreditCard,
    /// Unsecured personal loan.
    Personal,
    /// Anything else.
    #[default]
    Other,
}

/// How spare cash is used to pay debts down faster.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DebtPolicy {
    /// Which debt receives extra payments. "minimum" pays only what is required.
    pub strategy: DebtStrategy,
    /// Extra amount per month, in today's dollars, paid on top of the required
    /// payments. As debts are paid off their payments roll into this amount. 0 for none.
    pub extra_monthly: f64,
}

/// Order in which extra debt payments are applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DebtStrategy {
    /// Pay only the required payments.
    #[default]
    Minimum,
    /// Extra money goes to the highest interest rate first. Cheapest overall.
    Avalanche,
    /// Extra money goes to the smallest balance first. Fastest to clear individual debts.
    Snowball,
}
