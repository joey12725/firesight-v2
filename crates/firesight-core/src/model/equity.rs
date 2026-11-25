//! Private-company equity: stock, options and RSUs in companies that are not listed,
//! each with its own uncertain outcome. The engine side is `crate::equity`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A private company in which the household holds stock, options or RSUs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PrivateCompany {
    /// Stable identifier used by patches, for example "acme".
    pub id: String,
    /// Display name, for example "Acme Robotics".
    pub name: String,
    /// Who holds the equity: 0 for the first person in the household, 1 for the second.
    pub owner: usize,
    /// The id of the job income at this company, if the person works there now.
    /// Unvested holdings vest while that job lasts; leaving it forfeits what is
    /// unvested and starts the exercise window. Leave empty for a former employer
    /// or an investment.
    pub job: Option<String>,
    /// Whether the holdings count. Set false to plan as if this company's stock
    /// and options were worth nothing: no exercises, no proceeds, no tax. The
    /// company's fate is still simulated, so a failure can still end the linked job.
    pub included: bool,
    /// How far along the company is. It sets the typical odds of each outcome,
    /// their timing and price, and how often tender offers happen.
    pub stage: CompanyStage,
    /// Price per share investors paid in the latest funding round, in dollars.
    /// Every outcome price is a multiple of this.
    pub preferred_price: f64,
    /// The company's latest 409A valuation of one common share, in dollars: the
    /// value on which exercising options is taxed. Leave empty to use a typical
    /// fraction of the preferred price for the stage (20% at seed to 65% for a
    /// unicorn). The gap to the preferred price closes as a sale or listing nears.
    pub fmv_409a: Option<f64>,
    /// Money raised from investors as a fraction of the latest post-money
    /// valuation. In a sale for less than the last valuation investors are paid
    /// first, and common stock gets nothing at or below this fraction. Leave empty
    /// for the stage's typical value (0.20 at seed to 0.35 for a unicorn).
    pub preference_overhang: Option<f64>,
    /// Shifts the stage's typical odds: "pessimistic" makes failure 10 points
    /// more likely, "optimistic" up to 10 points less, with the other outcomes
    /// rescaled. Ignored when `outcomes` is filled in.
    pub outlook: Outlook,
    /// Your own table of what might happen, replacing the stage's typical odds.
    /// The rows are mutually exclusive and their probabilities must add up to 1.
    /// Leave empty to use the stage preset. A row does not inherit the stage's
    /// prices or timing: give every "acquired" and "ipo" row its years and its low
    /// and high multiples, or it is a price of exactly 1 times the last round's in
    /// years 1 to 5.
    pub outcomes: Vec<OutcomeRow>,
    /// Tender offers and other company-run sales while it is still private.
    pub tenders: TenderOffers,
    /// Months after an IPO before employees may sell. Usually 6; 0 for a direct
    /// listing.
    pub lockup_months: u32,
    /// Months after leaving the company in which vested options can still be
    /// exercised. Usually 3. For a former employer, the months left from today;
    /// with more than 3 left, incentive options are taxed as non-qualified ones
    /// (the law gives incentive options three months after leaving).
    pub exercise_window_months: u32,
    /// Whether a failure of the company also ends the linked job (the person is
    /// laid off that month and looks for work like after any layoff).
    pub company_failure_ends_job: bool,
    /// Whether shares of this company are qualified small business stock: a US C
    /// corporation in a qualifying business with at most $75 million of gross
    /// assets ($50 million for shares acquired before July 5, 2025) on the day you
    /// got each batch of shares. Gain on such shares held long enough is partly or
    /// wholly free of federal tax. Set false if later exercises will happen after
    /// the company passed that size. Ask the company; false if unsure.
    pub qsbs: bool,
    /// Minimum tax credit carried forward from exercising this company's incentive
    /// options in earlier years (Form 8801), in dollars. 0 if none.
    pub amt_credit_carryforward: f64,
    /// What is held: option grants, RSUs and shares already owned.
    pub holdings: Vec<EquityHolding>,
    /// The household's decisions about this position.
    pub policy: EquityPolicy,
}

impl PrivateCompany {
    /// Parses one company (an element of `private_equity`) from JSON, reporting the
    /// path of the offending field on error like [`super::Profile::from_json`] does
    /// (for example `holdings[0].kind: unknown variant ...`).
    pub fn from_json(json: &str) -> Result<PrivateCompany, String> {
        let de = &mut serde_json::Deserializer::from_str(json);
        serde_path_to_error::deserialize(de).map_err(|e| super::profile::format_path_error(&e))
    }
}

impl Default for PrivateCompany {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            owner: 0,
            job: None,
            included: true,
            stage: CompanyStage::SeriesB,
            preferred_price: 0.0,
            fmv_409a: None,
            preference_overhang: None,
            outlook: Outlook::Typical,
            outcomes: Vec::new(),
            tenders: TenderOffers::default(),
            lockup_months: crate::equity::DEFAULT_LOCKUP_MONTHS,
            exercise_window_months: crate::equity::ISO_POST_TERMINATION_MONTHS,
            company_failure_ends_job: true,
            qsbs: false,
            amt_credit_carryforward: 0.0,
            holdings: Vec::new(),
            policy: EquityPolicy::default(),
        }
    }
}

/// How far along a private company is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompanyStage {
    /// Pre-seed or seed funded.
    Seed,
    /// Has raised a Series A.
    SeriesA,
    /// Has raised a Series B.
    #[default]
    SeriesB,
    /// Has raised a Series C.
    SeriesC,
    /// Series D or later, valued below $1 billion.
    Late,
    /// Valued at $1 billion or more, or preparing to go public.
    Unicorn,
}

impl CompanyStage {
    /// Every stage, in the order of the per-stage arrays in `params.json`.
    pub const ALL: [CompanyStage; 6] = [
        CompanyStage::Seed,
        CompanyStage::SeriesA,
        CompanyStage::SeriesB,
        CompanyStage::SeriesC,
        CompanyStage::Late,
        CompanyStage::Unicorn,
    ];

    /// Index into the per-stage arrays.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The name used in JSON and in `params.json`.
    pub const fn key(self) -> &'static str {
        match self {
            CompanyStage::Seed => "seed",
            CompanyStage::SeriesA => "series_a",
            CompanyStage::SeriesB => "series_b",
            CompanyStage::SeriesC => "series_c",
            CompanyStage::Late => "late",
            CompanyStage::Unicorn => "unicorn",
        }
    }
}

/// A lean on the stage's typical odds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outlook {
    /// Failure 10 points more likely than typical for the stage.
    Pessimistic,
    /// The stage's typical odds.
    #[default]
    Typical,
    /// Failure up to 10 points less likely than typical for the stage.
    Optimistic,
}

/// One possible outcome for a company. Rows are mutually exclusive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct OutcomeRow {
    /// Stable identifier used by patches, for example "ipo".
    pub id: String,
    /// What happens.
    pub kind: OutcomeKind,
    /// Probability of this row, as a decimal. All rows must add up to 1.
    pub probability: f64,
    /// Earliest time the outcome could happen, in years from today.
    pub years_from: f64,
    /// Latest time the outcome could happen, in years from today. The date is
    /// equally likely anywhere between the two. Ignored for "still_private".
    pub years_to: f64,
    /// Low end of the price at the outcome, as a multiple: one outcome in ten of
    /// this kind is below it. For an IPO it is the IPO price per share over
    /// today's preferred price; the price when the lockup ends varies around it.
    /// For a sale it is the sale price of the whole company over its valuation at
    /// the latest round: below 1 investors are paid first and common stock gets
    /// less (see `preference_overhang`). Ignored for "fails" and "still_private".
    pub multiple_low: f64,
    /// High end of the price multiple: one outcome in ten of this kind is above it.
    pub multiple_high: f64,
}

impl Default for OutcomeRow {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: OutcomeKind::Fails,
            probability: 0.0,
            years_from: 1.0,
            years_to: 5.0,
            multiple_low: 1.0,
            multiple_high: 1.0,
        }
    }
}

/// What happens to a private company.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    /// It shuts down or is sold for parts: shares and options are worth nothing.
    #[default]
    Fails,
    /// It is bought for cash. Shares and vested options are paid out at the sale
    /// price.
    Acquired,
    /// It goes public. Shares can be sold after the lockup.
    Ipo,
    /// No sale, listing or failure in the next ten years (or by the latest year in
    /// your table, if later). The same odds then apply to the following ten years.
    /// A company still private after three such rounds is written off: its shares
    /// and options are counted as worth nothing.
    StillPrivate,
}

impl OutcomeKind {
    /// The name used in JSON.
    pub const fn key(self) -> &'static str {
        match self {
            OutcomeKind::Fails => "fails",
            OutcomeKind::Acquired => "acquired",
            OutcomeKind::Ipo => "ipo",
            OutcomeKind::StillPrivate => "still_private",
        }
    }
}

/// Company-run chances to sell while it is still private.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TenderOffers {
    /// Chance per year that the company runs a tender offer, as a decimal,
    /// averaged over everything that might become of it. Leave empty for the
    /// stage's typical value (0.005 at seed to 0.15 for a unicorn). 0 for never.
    /// Tenders happen only in futures where the company ends at or above its
    /// last round's price, and come correspondingly more often there (never more
    /// than once a year); in futures where it ends below, there are none. None
    /// either while the market for new listings is shut.
    pub annual_probability: Option<f64>,
    /// A tender offer that has already been announced: months from today until it
    /// happens. It happens for certain, on the terms below. Its price is the
    /// model's share price in that month, not a figure you set. Leave empty if none.
    pub next_in_months: Option<u32>,
    /// Largest fraction of vested holdings that may be sold in one tender.
    /// Typically 0.20.
    pub max_fraction: f64,
    /// Tender price as a fraction of the model's share price in that month, which
    /// starts at the latest round's price and moves toward the simulated outcome.
    /// 1.0 is no discount; 0.9 a 10% discount. Between 0.05 and 2.
    pub price_fraction: f64,
    /// Whether people who have left the company may sell. In about half of tenders
    /// they may not.
    pub former_employees: bool,
}

impl Default for TenderOffers {
    fn default() -> Self {
        Self {
            annual_probability: None,
            next_in_months: None,
            max_fraction: 0.20,
            price_fraction: 1.0,
            former_employees: false,
        }
    }
}

/// One grant or block of shares in a private company.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EquityHolding {
    /// Stable identifier used by patches, for example "grant_2023".
    pub id: String,
    /// Display name, for example "New-hire grant".
    pub name: String,
    /// What it is: incentive options, non-qualified options, RSUs or shares.
    pub kind: HoldingKind,
    /// Number of options, units or shares in the holding, vested and unvested
    /// together. For options, exclude any already exercised (enter those as shares).
    pub shares: f64,
    /// How many of them have vested as of today. Ignored for shares already owned.
    pub vested: f64,
    /// Months until the unvested part is fully vested. Vesting is monthly and even.
    pub vest_months_left: u32,
    /// Months until the next vest when a cliff has not passed yet. 0 if it has.
    pub cliff_months: u32,
}

/// The kind of a holding.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HoldingKind {
    /// Incentive stock options (ISOs). Exercising is not taxed as pay but counts
    /// for the alternative minimum tax; shares held a year after exercise and two
    /// after the grant are taxed as long-term gains.
    Iso {
        /// Exercise price per share, in dollars.
        strike: f64,
        /// Months since the grant date, 0 to 119. Options lapse ten years after
        /// the grant: what is still unexercised then is exercised for cash or
        /// lost, as `policy.at_deadline` says.
        #[serde(default = "default_granted_months_ago")]
        granted_months_ago: u32,
    },
    /// Non-qualified stock options (NSOs). The gap between the share value and the
    /// strike is taxed as pay when exercised.
    Nso {
        /// Exercise price per share, in dollars.
        strike: f64,
        /// Months since the grant date, 0 to 119. Options lapse ten years after
        /// the grant: what is still unexercised then is exercised for cash or
        /// lost, as `policy.at_deadline` says.
        #[serde(default = "default_granted_months_ago")]
        granted_months_ago: u32,
    },
    /// Restricted stock units that pay out only at an IPO or a sale of the company
    /// (double-trigger). Taxed as pay when they pay out.
    Rsu {
        /// Whether vested units are lost on leaving the company before it goes
        /// public or is sold. Many plans let them survive; set true if yours does
        /// not. Leaving after the company has gone public, during the lockup,
        /// loses nothing that has vested.
        #[serde(default)]
        must_be_employed_at_event: bool,
    },
    /// Shares already owned, from exercising options earlier or from buying them.
    Shares {
        /// What you paid per share, in dollars (the strike, plus any amount taxed
        /// as pay when you exercised). Per share, not the total paid.
        basis_per_share: f64,
        /// Months since you got the shares.
        #[serde(default)]
        months_held: u32,
        /// Whether the shares came from exercising incentive options (ISOs).
        #[serde(default)]
        from_iso: bool,
        /// For ISO shares: the 409A value per share on the day you exercised (on
        /// Form 3921). Leave 0 if unknown; it is then taken to equal the basis.
        #[serde(default)]
        fmv_at_exercise: f64,
    },
}

impl Default for HoldingKind {
    fn default() -> Self {
        HoldingKind::Nso {
            strike: 0.0,
            granted_months_ago: default_granted_months_ago(),
        }
    }
}

impl HoldingKind {
    /// The strike of an option, `None` for RSUs and shares.
    pub const fn strike(&self) -> Option<f64> {
        match *self {
            HoldingKind::Iso { strike, .. } | HoldingKind::Nso { strike, .. } => Some(strike),
            _ => None,
        }
    }

    /// True for shares already owned.
    pub const fn is_shares(&self) -> bool {
        matches!(self, HoldingKind::Shares { .. })
    }
}

/// The grant age assumed when none is given: two years.
pub fn default_granted_months_ago() -> u32 {
    24
}

/// The household's decisions about one company's equity.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EquityPolicy {
    /// When to pay to exercise options.
    pub exercise: ExerciseTiming,
    /// What to do with vested options that are about to lapse: at the end of the
    /// exercise window after leaving, or ten years after the grant.
    pub at_deadline: DeadlinePolicy,
    /// Share of the amount allowed that is sold in each tender offer. 0 sells
    /// nothing, 1 the maximum.
    pub tender_sell_fraction: f64,
    /// What to do with shares once an IPO lockup ends. Applies to shares already
    /// owned by then (from exercising earlier, or bought). Options still
    /// unexercised and RSUs are sold for cash at the lockup's end whatever is
    /// chosen here.
    pub sell_down: SellDown,
    /// Plan to leave this company this many months from today: what is unvested
    /// then is forfeited and the exercise window starts. The job income itself is
    /// not changed (it stands for whatever job you hold). Leave empty to stay.
    pub leave_after_months: Option<u32>,
}

impl Default for EquityPolicy {
    fn default() -> Self {
        Self {
            exercise: ExerciseTiming::AtLiquidity,
            at_deadline: DeadlinePolicy::Exercise,
            tender_sell_fraction: 0.5,
            sell_down: SellDown::AllAtLockupEnd,
            leave_after_months: None,
        }
    }
}

/// When options are exercised.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExerciseTiming {
    /// Never pay to exercise early. Options are exercised and sold together when
    /// there is a buyer: a tender offer, a sale of the company or after an IPO.
    #[default]
    AtLiquidity,
    /// Exercise everything vested today, now, as far as savings above the
    /// emergency fund can pay for it. Options that vest later wait.
    VestedNow,
    /// The same now, and each later batch as it vests.
    AsTheyVest,
}

/// What to do with vested options that are about to lapse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeadlinePolicy {
    /// Exercise as many as savings can pay for, tax included; the rest lapse.
    /// The tax counted is all that the exercise adds to the year's return, not
    /// only what is withheld. The emergency fund, retirement accounts and money
    /// already owed in tax are not spent on it.
    #[default]
    Exercise,
    /// Let them lapse.
    Forfeit,
}

/// What to do with shares once an IPO lockup ends. Applies to shares already owned
/// by then. Options still unexercised and RSUs are sold for cash at the lockup's
/// end whatever is chosen here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SellDown {
    /// Sell everything when the lockup ends.
    #[default]
    AllAtLockupEnd,
    /// Sell in equal yearly parts, the first when the lockup ends.
    OverYears {
        /// Number of yearly sales, 1 to 5.
        years: u32,
    },
    /// Keep the shares already owned as a single-stock position in the taxable
    /// account. Shares that qualify as small business stock (`qsbs`) are instead
    /// held as they are until their gain is fully excluded, and sold then: the
    /// single-stock position cannot carry the exclusion.
    Hold,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_company_parses_from_the_smallest_useful_json() {
        let c: PrivateCompany = serde_json::from_value(json!({
            "id": "acme",
            "preferred_price": 10,
            "holdings": [
                {"id": "g", "kind": {"type": "nso", "strike": 4}, "shares": 1000},
                {"id": "r", "kind": {"type": "rsu"}, "shares": 10},
                {"id": "s", "kind": {"type": "shares", "basis_per_share": 1}, "shares": 5}
            ],
            "policy": {"sell_down": {"type": "over_years", "years": 3}}
        }))
        .unwrap();
        assert!(c.included && c.company_failure_ends_job && !c.qsbs);
        assert_eq!(c.stage, CompanyStage::SeriesB);
        assert_eq!((c.lockup_months, c.exercise_window_months), (6, 3));
        assert_eq!(
            c.holdings[0].kind,
            HoldingKind::Nso {
                strike: 4.0,
                granted_months_ago: 24
            }
        );
        assert_eq!(c.policy.sell_down, SellDown::OverYears { years: 3 });
        assert_eq!(c.policy.tender_sell_fraction, 0.5);
        assert_eq!(c.tenders, TenderOffers::default());
        let back: PrivateCompany =
            serde_json::from_value(serde_json::to_value(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn a_strike_is_required_and_unknown_fields_are_rejected() {
        let e = serde_json::from_value::<EquityHolding>(json!({"kind": {"type": "iso"}}));
        assert!(e.unwrap_err().to_string().contains("strike"));
        let e = serde_json::from_value::<PrivateCompany>(json!({"stagee": "seed"}));
        assert!(e.unwrap_err().to_string().contains("stagee"));
        let e = serde_json::from_value::<EquityPolicy>(json!({
            "sell_down": {"type": "over_years", "years": 2, "x": 1}
        }));
        assert!(e.is_err());
        let e = serde_json::from_value::<EquityPolicy>(json!({"sell_down": {"type": "sometimes"}}));
        assert!(e.is_err());
    }

    #[test]
    fn enums_use_snake_case_strings() {
        assert_eq!(
            serde_json::to_value(CompanyStage::SeriesC).unwrap(),
            "series_c"
        );
        assert_eq!(
            serde_json::to_value(OutcomeKind::StillPrivate).unwrap(),
            "still_private"
        );
        assert_eq!(
            serde_json::to_value(ExerciseTiming::AsTheyVest).unwrap(),
            "as_they_vest"
        );
        assert_eq!(
            serde_json::to_value(SellDown::AllAtLockupEnd).unwrap(),
            json!({"type": "all_at_lockup_end"})
        );
        for (i, s) in CompanyStage::ALL.iter().enumerate() {
            assert_eq!(s.index(), i);
            assert_eq!(serde_json::to_value(s).unwrap(), s.key());
        }
    }
}
