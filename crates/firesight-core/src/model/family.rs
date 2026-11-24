//! Household dynamics: divorce, the household's parents, caregiving and life
//! insurance. Every field here defaults to "off", so a profile that does not
//! mention them simulates exactly as before.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::YearMonth;

/// Average attorney fees of a divorce with a full-scope lawyer: Martindale-Nolo
/// Research survey (2019), $11,300 (median $7,000; $10,400 when settled out of
/// court, $17,700 when a dispute went to trial). Taken as today's dollars.
pub const DIVORCE_LEGAL_COSTS: f64 = 11_300.0;

/// Default share of the couple's shared spending that the person who stays spends
/// alone after a divorce. Between the two usual equivalence scales: the OECD
/// square-root scale gives one adult `1 / sqrt(2) = 0.71` of what two spend, the
/// OECD-modified scale `1 / 1.5 = 0.67` (OECD, "What are equivalence scales?").
/// The choice of 0.7 within that range is a judgement, registered in
/// `data/actuarial/params.json` under `unverified_from_memory`.
/// Two households of 0.7 each cost 1.4 times the one they replace.
pub const DIVORCE_SPENDING_FACTOR: f64 = 0.7;

/// Cumulative probability that a first marriage has ended in separation or divorce
/// by 5, 10 and 15 years: 20%, 33% and 43% (Bramlett and Mosher 2001, "First
/// marriage dissolution, divorce, and remarriage: United States", NCHS Advance Data
/// 323, from the 1995 National Survey of Family Growth).
pub const DISSOLUTION_BY_DURATION: [(f64, f64); 3] = [(5.0, 0.20), (10.0, 0.33), (15.0, 0.43)];

/// Annual divorce rate of married people aged 50 to 64 after the first fifteen
/// years of marriage: 10.05 divorces per 1,000 married persons aged 50 and older
/// in 2010 (Brown and Lin 2012, "The gray divorce revolution", Journals of
/// Gerontology Series B 67(6)), the rate of the age group that has since been
/// stable. Applied to younger couples married fifteen years or more too. The figure
/// is quoted from memory and its use below 50 is a judgement: registered in
/// `data/actuarial/params.json` under `unverified_from_memory`.
pub const LATE_DIVORCE_RATE_UNDER_65: f64 = 0.010;

/// Annual divorce rate at 65 and older: 6 per 1,000 married persons in 2019
/// (Carlson, "Age variation in the divorce rate, 1990 & 2019", NCFMR Family
/// Profile FP-21-16, Bowling Green State University).
pub const LATE_DIVORCE_RATE_65_PLUS: f64 = 0.006;

/// What happens when the couple divorces.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DivorceTerms {
    /// Share of the household's net worth (savings and home equity less debts) that
    /// the departing partner (the second person) takes, from 0 to 1. 0.5 is an even
    /// split. It is paid out of savings and retirement accounts in proportion to
    /// their size; transfers between spouses in a divorce are not taxed.
    pub asset_split: f64,
    /// Alimony or child support paid or received after the divorce.
    pub support: Support,
    /// True: the person who stays keeps the home and its mortgage, and the departing
    /// partner's share of its equity comes out of savings. False: the home is sold
    /// (6% selling costs) and the proceeds are split with everything else.
    pub keeps_home: bool,
    /// Rent the person who stays pays after a sale (`keeps_home` false), today's
    /// dollars a month. Leave empty to rent at 4% of the sale price a year.
    pub then_rent_monthly: Option<f64>,
    /// Legal and filing costs paid by the household, today's dollars. Default
    /// $11,300, the average attorney fees of a divorce with a full-scope lawyer
    /// (Martindale-Nolo 2019; median $7,000). Much less when uncontested.
    pub legal_costs: f64,
    /// Shared budget lines are multiplied by this factor after the divorce: one
    /// adult living alone spends about 0.7 of what the couple did (economies of
    /// scale are lost, so two households cost more than one).
    pub spending_factor: f64,
}

impl Default for DivorceTerms {
    fn default() -> Self {
        Self {
            asset_split: 0.5,
            support: Support::default(),
            keeps_home: true,
            then_rent_monthly: None,
            legal_costs: DIVORCE_LEGAL_COSTS,
            spending_factor: DIVORCE_SPENDING_FACTOR,
        }
    }
}

/// Alimony or child support after a divorce. Under current federal law (divorces
/// from 2019 on) neither is deductible for the payer nor taxable for the recipient.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Support {
    /// Amount per year in today's dollars, fixed in dollars at the divorce date
    /// (court orders are rarely indexed). 0 for none.
    pub annual: f64,
    /// Number of years it is paid.
    pub years: f64,
    /// Whether the person who stays pays it or receives it.
    pub direction: SupportDirection,
}

/// Who pays support.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportDirection {
    /// The household (the person who stays) pays it: essential spending.
    #[default]
    Pays,
    /// The household receives it: untaxed other income.
    Receives,
}

/// Whether the plan includes the risk that the couple divorces.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DivorceRisk {
    /// No divorce unless a `divorce` event says so.
    #[default]
    Off,
    /// Divorce at US rates by length of marriage and age: about 20% of first
    /// marriages end within 5 years, 33% within 10 and 43% within 15 (NCHS,
    /// Bramlett and Mosher 2001), then about 1% a year to 65 and 0.6% after. Needs
    /// `household.married`. These rates include separations and are for first
    /// marriages; a long, stable marriage may be at lower risk.
    Typical(DivorceTerms),
}

impl DivorceRisk {
    /// True when no divorce hazard is modelled.
    pub fn is_off(&self) -> bool {
        matches!(self, DivorceRisk::Off)
    }
}

/// Share of the hours of paid home care that a partner at home provides instead,
/// with `spending.healthcare.spousal_care` on. A judgement (registered in
/// `data/actuarial/params.json` under `unverified_from_memory`): spouses are the
/// main unpaid caregivers of married people who need care, and married people
/// receiving home care use markedly fewer paid hours, but no source found gives
/// the share of paid hours replaced.
pub const SPOUSAL_CARE_SHARE: f64 = 0.5;

/// Probability that a working partner cuts their hours when the other partner
/// enters home care: 15% of working family caregivers went part time or cut
/// their hours (AARP and National Alliance for Caregiving, "Caregiving in the
/// U.S. 2020", as reported by Next Avenue; not checked against the report's
/// tables, and for all family caregivers, not spouses only).
pub const SPOUSAL_CARE_REDUCE_PROBABILITY: f64 = 0.15;

/// Probability that a working partner stops working instead: 6% of working
/// caregivers quit and 5% retired early (same source as
/// [`SPOUSAL_CARE_REDUCE_PROBABILITY`]).
pub const SPOUSAL_CARE_STOP_PROBABILITY: f64 = 0.11;

/// Share of full-time work given up by a partner who cuts their hours: half
/// time. A judgement.
pub const SPOUSAL_CARE_HOURS_CUT: f64 = 0.5;

/// Most parents a profile can list.
pub const MAX_PARENTS: usize = 4;

/// Years within which an inherited IRA must be emptied by a beneficiary who is not
/// a spouse, minor child, disabled or close in age: ten (SECURE Act of 2019, IRC
/// 401(a)(9)(H)).
pub const INHERITED_IRA_YEARS: u32 = 10;

/// A parent (or another relative) of someone in the household, whose death leaves
/// the household an inheritance and whose late-life care can use up the estate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Parent {
    /// Stable identifier used by patches, for example "mom".
    pub id: String,
    /// Display name, shown in the event log.
    pub name: String,
    /// Which person of the household (0 or 1) is this parent's child. After a
    /// divorce the parents of the partner who left no longer matter to the plan.
    pub child: usize,
    /// Month of birth, "YYYY-MM". The parent's lifetime is drawn from the SSA
    /// life table for this age and sex, on a random stream of its own.
    pub birth: YearMonth,
    /// Sex, for the life table and the chance of needing long-term care.
    pub sex: super::Sex,
    /// Health, which scales the mortality rates as for the household's own people.
    pub health: super::HealthStatus,
    /// What the parent would leave if they died today, today's dollars, all heirs
    /// together. Held constant in real terms until death, less the cost of any
    /// late-life care.
    pub estate: f64,
    /// Share of the estate that comes to the household, from 0 to 1 (0.5 for one
    /// of two children).
    pub share: f64,
    /// Fraction of the household's inheritance that is in traditional IRAs and
    /// 401(k)s. That part is an inherited IRA: it is paid out in equal parts over
    /// the ten years after the death and taxed as ordinary income (no penalty at any
    /// age). The rest arrives untaxed with its cost basis stepped up to its value.
    pub inherited_ira_fraction: f64,
    /// True (the default): paid long-term care at the end of the parent's life
    /// (drawn as for the household's own people, by sex and age at death) is paid
    /// out of the estate before anything is inherited.
    pub care_erodes_estate: bool,
    /// The household's part in this parent's care, if any. The care spell is the
    /// parent's paid long-term care at the end of life (drawn as above); during it
    /// the household may pay part of the cost and one person may work less.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub care_support: Option<CareSupport>,
}

/// Average yearly out-of-pocket spending of a family caregiver: $7,242 (AARP,
/// "Caregiving Out-of-Pocket Costs Study", 2021; housing more than half, medical
/// 17%). A reference for `CareSupport::annual_cost`, not a default.
pub const CAREGIVER_OUT_OF_POCKET_ANNUAL: f64 = 7_242.0;

/// What the household does while a parent needs care.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CareSupport {
    /// Probability, from 0 to 1, that the household takes part when the parent
    /// needs care (drawn once per path on the parent's own random stream).
    pub probability: f64,
    /// What the household pays toward the care each year of the spell, today's
    /// dollars, rising with medical costs (no more than the care costs). The
    /// estate pays the rest. Family caregivers spend $7,242 a year on average
    /// (AARP 2021).
    pub annual_cost: f64,
    /// Share of full-time work the caregiver gives up during the spell: 0 none,
    /// 0.5 half time, 1 a full career break. Their pay, and so their Social
    /// Security record, falls by that share; they return to their job afterwards.
    pub hours: f64,
    /// Who cuts back: a person index. Leave empty for the parent's own child.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caregiver: Option<usize>,
}

impl Default for CareSupport {
    fn default() -> Self {
        Self {
            probability: 1.0,
            annual_cost: 0.0,
            hours: 0.0,
            caregiver: None,
        }
    }
}

impl Default for Parent {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            child: 0,
            birth: YearMonth::default(),
            sex: super::Sex::default(),
            health: super::HealthStatus::default(),
            estate: 0.0,
            share: 1.0,
            inherited_ira_fraction: 0.0,
            care_erodes_estate: true,
            care_support: None,
        }
    }
}

/// Most life insurance policies a profile can hold, over all people.
pub const MAX_POLICIES: usize = 8;

/// A life insurance policy on one person's life.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LifePolicy {
    /// Stable identifier used by patches, for example "term_20".
    pub id: String,
    /// Display name, shown in the event log.
    pub name: String,
    /// Term or permanent.
    pub kind: PolicyKind,
    /// Paid at the insured's death while the policy is in force, in dollars of the
    /// plan's start, fixed in nominal terms (policies are not indexed). Tax-free
    /// to the beneficiary (IRC 101(a)): the surviving partner, or the estate (it
    /// then counts toward the bequest target) when nobody is left.
    pub death_benefit: f64,
    /// Premium per year, in dollars of the start, level in nominal terms as the
    /// premiums of term and whole life policies are. Essential spending while the
    /// policy is in force. The policy lapses in the first month the household has
    /// run out of savings and cannot pay it.
    pub annual_premium: f64,
}

impl Default for LifePolicy {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: PolicyKind::default(),
            death_benefit: 0.0,
            annual_premium: 0.0,
        }
    }
}

/// How long a life insurance policy lasts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PolicyKind {
    /// Term life: in force for the years remaining of its term, then it ends.
    Term {
        /// Years of the term still to run from the start of the plan.
        years_remaining: f64,
    },
    /// Whole or other permanent life: in force for life while premiums are paid
    /// (cash value is not modelled).
    Permanent,
}

impl Default for PolicyKind {
    fn default() -> Self {
        PolicyKind::Term {
            years_remaining: 20.0,
        }
    }
}
