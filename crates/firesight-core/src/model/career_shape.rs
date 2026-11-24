//! Opt-in shapes for the random parts of a job: the yearly pay surprise, the pay cut
//! after a layoff and how long a job search lasts. Every default here is the
//! behaviour the engine had before these options existed.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The shape of the lasting yearly surprise in a job's pay, for the same expected
/// pay and the same year-to-year variability.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WageShockShape {
    /// The bell curve: every year brings a moderate surprise, up as often as down.
    /// The default.
    #[default]
    Normal,
    /// Most years pay barely moves (about half the usual surprise), and in about
    /// one year in seven it moves a lot (about twice the usual surprise), a little
    /// more often down than up. Expected pay and the overall variability are the
    /// same as under `normal`. Closer to how earnings records look.
    FatTailed,
}

/// The shape of the lasting change in pay when a new job is found after a layoff,
/// for the same average change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PayCutShape {
    /// One bell curve around the average loss (8% in normal times, 15% around
    /// recessions, more from age 50 and after a long search): a raise is as likely
    /// as a loss twice the average. The default.
    #[default]
    Normal,
    /// Two kinds of outcome. Some people (45% in normal times, 35% around
    /// recessions, a fifth fewer from age 50) find work at about the old pay, give
    /// or take 5%. The rest take a loss with a long tail: usually modest,
    /// occasionally three times the average. The average change is the same as
    /// under `normal`.
    TwoPart,
}

/// How the length of a spell of unemployment is decided.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobSearchModel {
    /// The length is fixed on the day of the layoff from the state of the economy
    /// that day (3.6 months on average in normal times, 6.6 around recessions).
    /// The default.
    #[default]
    DrawnAtLayoff,
    /// Each month out of work brings a chance of finding a job. The chance falls
    /// the longer the search has lasted (to 35% of its first-month level, mostly
    /// within a year) and follows the economy month by month, so a recession that
    /// arrives during a search lengthens it. The average search is the same as
    /// under `drawn_at_layoff` when the economy does not change during it.
    Monthly,
}
