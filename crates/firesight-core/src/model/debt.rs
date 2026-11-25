//! Non-mortgage debts and how they are paid down.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{CashOutRefinance, MortgagePrepayment};

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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DebtPolicy {
    /// Which debt receives extra payments. "minimum" pays only what is required.
    pub strategy: DebtStrategy,
    /// Extra amount per month, in today's dollars, paid on top of the required
    /// payments. As debts are paid off their payments roll into this amount. 0 for none.
    pub extra_monthly: f64,
    /// Refinance a mortgage when market rates have fallen far enough to repay the
    /// cost of doing so: each January, if the market rate is at least one point
    /// below the loan's rate and four years of interest saved exceed the closing
    /// costs (2% of the balance plus $2,000 in today's dollars), the loan is
    /// refinanced at the market rate over the term it had left and the costs are
    /// paid. Set to false to keep every mortgage at its original rate.
    pub refinance_mortgages: bool,
    /// Refinance the main home for cash instead of withdrawing retirement money with
    /// the 10% early-withdrawal penalty: a new loan at the market mortgage rate (plus
    /// a premium) up to a loan-to-value limit, with closing costs, at most once a
    /// year, and only when the penalty avoided outweighs the closing costs and the
    /// extra interest. Leave empty never to do so.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cash_out_refinance: Option<CashOutRefinance>,
    /// Whether spare cash pays a mortgage down early instead of being invested in the
    /// taxable account: never (default), or only while the mortgage's rate beats the
    /// return on safe investments after tax.
    #[serde(skip_serializing_if = "MortgagePrepayment::is_never")]
    pub mortgage_prepayment: MortgagePrepayment,
}

impl Default for DebtPolicy {
    fn default() -> Self {
        Self {
            strategy: DebtStrategy::default(),
            extra_monthly: 0.0,
            refinance_mortgages: true,
            cash_out_refinance: None,
            mortgage_prepayment: MortgagePrepayment::Never,
        }
    }
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
