//! Household behaviour that is not a mechanical rule: how investors react to a
//! crash, how a person decides when to claim Social Security and when a household
//! stops saving. Every setting here
//! is opt-in; the default is the household the engine has always modelled.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How the household behaves when the stock market falls.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvestorBehavior {
    /// Sticks to the plan whatever the market does (the default).
    #[default]
    Disciplined,
    /// May sell stocks after a deep fall and buy back only once the market has
    /// recovered or a waiting period has passed. A behavioural assumption, not a
    /// forecast: the defaults are calibrated to how retirement-plan savers acted in
    /// 2008 (Vanguard, How America Saves 2009).
    PanicProne(PanicSelling),
}

impl InvestorBehavior {
    /// True for the default, disciplined household.
    pub fn is_disciplined(&self) -> bool {
        matches!(self, InvestorBehavior::Disciplined)
    }
}

/// Parameters of panic selling.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PanicSelling {
    /// How far US stocks must fall from their peak before the household may panic, as a
    /// decimal: 0.25 is a 25% fall (a bear market).
    pub drawdown_trigger: f64,
    /// Chance that the household panics in a given fall, decided once per fall (the
    /// first month the trigger is reached). Default 0.08.
    pub probability: f64,
    /// Share of the stocks it then sells and holds as cash, as a decimal. Default 0.5.
    pub share: f64,
    /// When it buys back.
    pub reentry: Reentry,
}

impl Default for PanicSelling {
    fn default() -> Self {
        Self {
            drawdown_trigger: 0.25,
            probability: 0.08,
            share: 0.5,
            reentry: Reentry::default(),
        }
    }
}

/// When a household that sold in a panic buys its stocks back: at whichever of the two
/// comes first.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Reentry {
    /// Buy back once US stocks are within this share of their old peak again, as a
    /// decimal: 0.05 means "back to 5% below the peak".
    pub recovered_within: f64,
    /// Buy back after this many months out of the market in any case.
    pub max_months: u32,
}

impl Default for Reentry {
    fn default() -> Self {
        Self {
            recovered_within: 0.05,
            max_months: 36,
        }
    }
}

/// How a person decides when to claim Social Security.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Claiming {
    /// Claim at `claim_age` whatever happens (the default).
    #[default]
    Fixed,
    /// Plan to claim at `latest`, but claim earlier (from `earliest`, once the person
    /// has stopped working) when savings could no longer pay the household's way
    /// until then, and at once if the household is cutting essentials. In a couple the
    /// lower earner claims first, so the higher earner's benefit, which the survivor
    /// keeps, grows longest. `claim_age` is not used.
    Adaptive {
        /// Earliest age at which to claim (62 to 70). Default 62.
        #[serde(default = "default_earliest_claim")]
        earliest: f64,
        /// Age to delay to while savings can bridge (62 to 70). Default 70.
        #[serde(default = "default_latest_claim")]
        latest: f64,
    },
}

impl Claiming {
    /// True for the default, fixed claim age.
    pub fn is_fixed(&self) -> bool {
        matches!(self, Claiming::Fixed)
    }
}

fn default_earliest_claim() -> f64 {
    62.0
}

fn default_latest_claim() -> f64 {
    70.0
}

/// When a household in distress stops saving (`portfolio.pause_deferrals_in_distress`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DistressRules {
    /// Keep contributing what earns the full employer match during a pause (true,
    /// the default); false stops payroll deferrals altogether.
    pub keep_match: bool,
    /// A debt at this interest rate or more counts as high-interest, as a decimal.
    /// Default 0.08: credit cards (21.6% on average in 2024) and most personal loans,
    /// not typical student or car loans.
    pub high_interest_rate: f64,
}

impl Default for DistressRules {
    fn default() -> Self {
        Self {
            keep_match: true,
            high_interest_rate: 0.08,
        }
    }
}

impl DistressRules {
    /// True for the default rules.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}
