//! Income sources: jobs, self-employment, pensions, annuities and everything else.
//! Social Security lives on the person; rental income lives on the property.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    BusinessFailure, ExitSize, ExitTiming, JobSearchModel, PayCutShape, StockRuin, TailShape,
    WageShockShape, When,
};

/// One source of income belonging to one person.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Income {
    /// Stable identifier used by patches and events, for example "alex_job".
    pub id: String,
    /// Display name, for example "Software engineer at Initech".
    pub name: String,
    /// Who earns it: 0 for the first person in the household, 1 for the second.
    pub owner: usize,
    /// What kind of income this is and its details.
    pub kind: IncomeKind,
    /// When the income begins. Leave empty if it is already being received.
    pub start: Option<When>,
    /// When the income stops regardless of retirement. Leave empty for no fixed end
    /// (jobs still stop at retirement when `ends_at_retirement` is true).
    pub end: Option<When>,
}

/// The kind of an income source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum IncomeKind {
    /// A salaried or hourly job with an employer.
    Job(Job),
    /// Freelance, gig or business income reported on Schedule C.
    SelfEmployment(SelfEmployment),
    /// A defined-benefit pension.
    Pension(Pension),
    /// A purchased annuity paying a fixed income for life.
    Annuity(Annuity),
    /// Any other recurring income: alimony, royalties, family support, a trust.
    Other(OtherIncome),
}

impl Default for IncomeKind {
    fn default() -> Self {
        IncomeKind::Job(Job::default())
    }
}

/// A job with an employer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Job {
    /// Gross base pay per year in today's dollars, before tax and before 401(k)
    /// deferrals. Excludes bonus, commission and equity. For a job that starts in
    /// the future this is the pay it starts on; growth runs from its start.
    pub salary: f64,
    /// How base pay is expected to grow over the career, on top of inflation.
    pub growth: WageGrowth,
    /// How uncertain the job is: random pay shocks and the chance of a layoff.
    pub risk: CareerRisk,
    /// Annual cash bonus, if any.
    pub bonus: Option<Bonus>,
    /// Sales commission, if any.
    pub commission: Option<Commission>,
    /// Stock grants (RSUs or options) from this employer.
    pub equity: Vec<EquityGrant>,
    /// The employer retirement plan (401(k), 403(b) or similar) and how much goes in.
    /// Leave empty if the job has no plan or the person does not contribute.
    pub plan: Option<EmployerPlan>,
    /// The employee's own HSA contribution per year through payroll, in today's
    /// dollars. Requires a high-deductible health plan. 0 for none.
    pub hsa_contribution: f64,
    /// The employer's HSA contribution per year, in today's dollars. 0 for none.
    pub hsa_employer: f64,
    /// Whether this job stops when the household retires. Set false for work that is
    /// meant to continue into retirement and give it an explicit `end` instead.
    pub ends_at_retirement: bool,
}

impl Default for Job {
    fn default() -> Self {
        Self {
            salary: 0.0,
            growth: WageGrowth::default(),
            risk: CareerRisk::default(),
            bonus: None,
            commission: None,
            equity: Vec::new(),
            plan: None,
            hsa_contribution: 0.0,
            hsa_employer: 0.0,
            ends_at_retirement: true,
        }
    }
}

/// How base pay grows in real terms.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum WageGrowth {
    /// The same raise above inflation every year.
    Flat {
        /// Annual raise above inflation as a decimal. 0.0 keeps pace with inflation;
        /// 0.01 is a typical long-run figure.
        real_raise: f64,
    },
    /// An age-dependent curve fitted to earnings data: fast growth early in a
    /// career, a peak in the mid-40s, then a decline in real pay of about 1% a
    /// year to 65 (15% in all on the typical profile, 22% on the steep one). The
    /// late decline in the data partly reflects shorter hours before retirement;
    /// use `Flat` for someone over 45 who expects pay to keep up with inflation.
    Lifecycle {
        /// Shape of the curve.
        #[serde(default)]
        profile: LifecycleProfile,
        /// Extra annual real growth added to the curve at every age, as a decimal.
        /// Usually 0.
        #[serde(default)]
        extra: f64,
    },
}

impl Default for WageGrowth {
    fn default() -> Self {
        WageGrowth::Lifecycle {
            profile: LifecycleProfile::Typical,
            extra: 0.0,
        }
    }
}

/// Shape of the lifecycle wage curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleProfile {
    /// Little age-related growth: pay roughly tracks inflation (many hourly and
    /// public-sector jobs).
    Flat,
    /// The median curve for workers with some college education: real pay rises
    /// about 66% from 25 to its peak at 45 and is 15% below the peak at 65.
    #[default]
    Typical,
    /// Steep early growth typical of professional and technical careers: about
    /// double from 25 to 45, then 22% below the peak at 65.
    Steep,
}

/// Job uncertainty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CareerRisk {
    /// How volatile pay is and how likely a layoff is. "typical" matches economy-wide
    /// averages (a few percent chance of job loss per year, higher in recessions).
    pub level: RiskLevel,
    /// Voluntary job changes with pay jumps. Off by default: pay then follows the
    /// lifecycle curve, which already contains the average gain from job changes.
    /// When on, the person moves between employers at an age-dependent rate and each
    /// move brings a random pay jump; pay for staying put is lowered by the matching
    /// amount so that the average career still follows the curve (movers gain more
    /// than stayers, but nobody is counted twice). Unvested equity is forfeited on a
    /// move. Needs `risk.level` other than none and `assumptions.career_risk` on.
    #[serde(skip_serializing_if = "crate::career_realism::is_default")]
    pub job_mobility: JobMobility,
    /// Pay freezes, temporary pay or hours cuts and smaller bonuses for workers who
    /// keep their job in a recession. Off by default. When on, employed workers in a
    /// recession month (the path's recession flag) face a chance of a freeze (the
    /// raise they would have had is lost for good), a nine-month cut of 5 to 20% that
    /// then recovers, and bonuses 30% smaller until a year after the recession ends.
    /// Layoffs in recessions are separate and already part of `level`. Needs
    /// `assumptions.career_risk` on.
    #[serde(skip_serializing_if = "crate::career_realism::is_default")]
    pub recession_pay: RecessionPay,
    /// What happens to workers laid off from 55 on. Off by default (the career model
    /// then already has longer spells from 55 and larger pay losses from 50). When
    /// "realistic", a layoff from 55 brings a spell up to 1.8 times longer and a pay
    /// loss 1.3 times larger on top of those, and with a probability that rises with
    /// age (15% at 55, 35% at 60, 51% at 64) the worker never works again: they retire
    /// involuntarily, and the household retires on what it has if nobody else is
    /// still working. How often layoffs happen is still set by `level`. Needs
    /// `level` other than none and `assumptions.career_risk` on.
    #[serde(skip_serializing_if = "crate::career_realism::is_default")]
    pub late_career: LateCareer,
    /// The shape of the lasting yearly pay surprise: `normal` (the default) or
    /// `fat_tailed` (quiet most years, a large move in about one year in seven).
    /// Expected pay is the same either way. Ignored when `level` is `none`.
    pub wage_shocks: WageShockShape,
    /// The shape of the lasting pay change after a layoff: `normal` (the default)
    /// or `two_part` (many return at about the old pay, the rest take a loss with
    /// a long tail). The average change is the same either way.
    pub layoff_pay_cut: PayCutShape,
    /// How long unemployment lasts: `drawn_at_layoff` (the default) or `monthly`
    /// (a chance of finding work each month that falls as the search drags on and
    /// follows recessions month by month).
    pub job_search: JobSearchModel,
}

/// How hard a layoff hits older workers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LateCareer {
    /// Only the career model's own age adjustments apply (the default).
    #[default]
    Off,
    /// Longer spells, larger pay losses and a rising chance of never working again
    /// after a layoff at 55 or older, shaped by Health and Retirement Study evidence
    /// of forced early retirement (the numbers are judgements).
    Realistic,
}

/// Whether workers who keep their job still lose pay in a recession.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecessionPay {
    /// Pay is unaffected by recessions except through layoffs (the default).
    #[default]
    Off,
    /// A 25% chance per recession year of a pay freeze, a 15% chance of a temporary
    /// cut of 5 to 20% for nine months, and bonuses at 70% for the recession and the
    /// following year. The sizes are judgements, not estimates.
    Typical,
}

/// How often a person changes employers voluntarily and what it pays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobMobility {
    /// No voluntary job changes are simulated (the default).
    #[default]
    Off,
    /// Economy-wide average: about 30% of workers under 25 change employers in a
    /// year, 20% at 25 to 34, 12% at 35 to 44, 8% at 45 to 54, 5% at 55 to 64. Moves
    /// are rarer in recessions and pay less.
    Typical,
    /// A job-hopping field (technology, sales, consulting): 1.6 times the typical
    /// rate of moves.
    High,
}

/// A coarse risk dial.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// No pay shocks and no layoffs: base pay follows the growth curve exactly. A
    /// bonus or commission keeps the volatility entered for it (switch
    /// `assumptions.career_risk` off to silence those too).
    None,
    /// Stable employment such as tenured or government work.
    Low,
    /// Economy-wide average.
    #[default]
    Typical,
    /// Cyclical or volatile work: startups, sales, construction.
    High,
}

/// An annual cash bonus.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Bonus {
    /// Target bonus as a fraction of base salary (0.10 = 10% of salary).
    pub target_fraction: f64,
    /// How much the payout varies from year to year, as a standard deviation relative
    /// to the target (0.3 means payouts typically land within 30% of target). 0 for a
    /// guaranteed bonus.
    pub volatility: f64,
    /// Calendar month in which the bonus is paid, 1 to 12. Default 3 (March).
    pub month: u32,
}

impl Default for Bonus {
    fn default() -> Self {
        Self {
            target_fraction: 0.10,
            volatility: 0.3,
            month: 3,
        }
    }
}

/// Sales commission paid on top of base salary.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Commission {
    /// Expected commission per year in today's dollars.
    pub expected_annual: f64,
    /// Year-to-year variation as a standard deviation relative to the expected amount
    /// (0.4 = 40%). Commission is paid monthly and never goes below zero.
    pub volatility: f64,
}

impl Default for Commission {
    fn default() -> Self {
        Self {
            expected_annual: 0.0,
            volatility: 0.4,
        }
    }
}

/// An employer retirement plan such as a 401(k) or 403(b).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EmployerPlan {
    /// Share of salary the employee contributes (0.10 = 10%). Capped at the IRS limit.
    /// Ignored when `employee_max` is true.
    pub employee_fraction: f64,
    /// Contribute the IRS maximum every year (including catch-up contributions from
    /// age 50) instead of a fixed share of salary.
    pub employee_max: bool,
    /// Share of the employee's contribution that goes to the Roth side of the plan
    /// (0 = all pre-tax, 1 = all Roth).
    pub roth_fraction: f64,
    /// Employer match per dollar the employee contributes (0.5 = 50 cents on the dollar,
    /// 1.0 = dollar for dollar). 0 for no match.
    pub match_rate: f64,
    /// The match applies to employee contributions up to this fraction of salary
    /// (0.06 = the first 6% of pay).
    pub match_up_to: f64,
    /// Whether the plan allows after-tax contributions with in-plan Roth conversion
    /// ("mega backdoor Roth"), which lets surplus savings fill the plan up to the
    /// overall IRS limit.
    pub after_tax_mega: bool,
}

impl Default for EmployerPlan {
    fn default() -> Self {
        Self {
            employee_fraction: 0.06,
            employee_max: false,
            roth_fraction: 0.0,
            match_rate: 0.5,
            match_up_to: 0.06,
            after_tax_mega: false,
        }
    }
}

/// A grant of employer stock that vests over time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EquityGrant {
    /// Stable identifier used by patches, for example "rsu_2024".
    pub id: String,
    /// Restricted stock units or stock options.
    pub kind: EquityKind,
    /// Whether the employer's stock is publicly traded, which decides when vested
    /// shares can be sold.
    pub company: EquityCompany,
    /// Value of the not-yet-vested part of the grant at today's share price, in
    /// today's dollars. For options, the value of the underlying shares, not the spread.
    pub unvested_value: f64,
    /// Months until the grant is fully vested. Vesting is monthly and even after the cliff.
    pub vest_months_remaining: u32,
    /// Months until the first vest. 0 if the cliff has already passed.
    pub cliff_months: u32,
    /// Value of the new grant received each year while employed ("refreshers"), in
    /// today's dollars. Each refresh vests over four years. 0 for none.
    pub refresh_annual_value: f64,
    /// Sell shares as soon as they vest and invest the proceeds according to the
    /// portfolio. When false, vested shares are kept as a single-stock position in
    /// the taxable account: rebalancing leaves them alone, and they are sold only
    /// when that account is drawn on for cash.
    pub sell_on_vest: bool,
}

impl Default for EquityGrant {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: EquityKind::Rsu,
            company: EquityCompany::default(),
            unvested_value: 0.0,
            vest_months_remaining: 48,
            cliff_months: 0,
            refresh_annual_value: 0.0,
            sell_on_vest: true,
        }
    }
}

/// The kind of equity award.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EquityKind {
    /// Restricted stock units: taxed as wages at the share value when they vest.
    #[default]
    Rsu,
    /// Stock options: worth the share price minus the strike price, and nothing if the
    /// price is below the strike.
    Option {
        /// Strike price as a fraction of today's share price (0.4 means the option is
        /// 60% in the money; 1.0 means at the money).
        strike_fraction: f64,
    },
}

/// Whether employer stock can be sold freely.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EquityCompany {
    /// A listed company: shares can be sold when they vest.
    Public {
        /// Annual volatility of the share price as a decimal. A large established
        /// company is about 0.30; a volatile growth company 0.50 or more.
        #[serde(default = "default_stock_volatility")]
        volatility: f64,
        /// Sensitivity to the US stock market: 1.0 moves with the market, above 1
        /// amplifies it.
        #[serde(default = "default_beta")]
        beta: f64,
        /// How often the company's own monthly surprises are extreme: `normal` (the
        /// bell curve) or `fat` (a few far wilder months, same volatility and same
        /// average return). Default `normal`.
        #[serde(default)]
        tails: TailShape,
        /// Whether the shares can become worthless for good: `never` (the default),
        /// `by_volatility` (a chance each year set by `volatility` and `beta`) or
        /// `stated`. Unvested grants are then worth nothing and no further grants
        /// arrive from this employer.
        #[serde(default)]
        ruin: StockRuin,
        /// Ties the job to the share price. When true, the chance of a layoff is
        /// three times as high while the shares have fallen more than 40% over the
        /// last twelve months for reasons of the company's own (not the market),
        /// and the job is lost if the shares become worthless. Has no effect when
        /// the job's risk level is `none`. Default false.
        #[serde(default)]
        layoff_link: bool,
    },
    /// A private company: shares are worth nothing spendable until an IPO or sale.
    Private {
        /// Chance per year that the company has its outcome (an IPO, a sale or a
        /// shutdown), as a decimal. 0.12 means about eight years on average.
        #[serde(default = "default_liquidity_hazard")]
        liquidity_hazard: f64,
        /// Probability that the outcome is a failure and the equity is worth nothing.
        /// Around 0.5 for a venture-backed company.
        #[serde(default = "default_fail_prob")]
        fail_prob: f64,
        /// Not used by the engine at present: the size of a successful outcome is
        /// drawn from three outcome classes (modest, good, large) whose mix depends on
        /// `fail_prob`. Kept so that existing profiles still load.
        #[serde(default = "default_exit_volatility")]
        exit_volatility: f64,
        /// How the date of the outcome is decided: `constant_hazard` (the default:
        /// the same chance every year) or `lognormal` (unlikely in the first year or
        /// two, then most likely around a typical wait that you state).
        #[serde(default)]
        exit_timing: ExitTiming,
        /// How large the best outcomes can be: `classes` (the default: at most 20
        /// times the paper value) or `pareto_tail` (a few outcomes far above that,
        /// up to 200 times, with the same average payout).
        #[serde(default)]
        exit_size: ExitSize,
    },
}

fn default_stock_volatility() -> f64 {
    0.35
}
fn default_beta() -> f64 {
    1.2
}
fn default_liquidity_hazard() -> f64 {
    0.12
}
fn default_fail_prob() -> f64 {
    0.5
}
fn default_exit_volatility() -> f64 {
    1.0
}

impl Default for EquityCompany {
    fn default() -> Self {
        EquityCompany::Public {
            volatility: default_stock_volatility(),
            beta: default_beta(),
            tails: TailShape::Normal,
            ruin: StockRuin::Never,
            layoff_link: false,
        }
    }
}

/// Self-employment income.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SelfEmployment {
    /// Net profit per year in today's dollars, after business expenses and before
    /// income and self-employment tax. For work that starts in the future this is
    /// the profit of its first year; growth runs from its start.
    pub net_annual: f64,
    /// Year-to-year variation in profit as a standard deviation relative to the
    /// expected amount (0.25 = 25%).
    pub volatility: f64,
    /// Expected annual growth above inflation, as a decimal.
    pub real_growth: f64,
    /// Whether the work stops when the household retires.
    pub ends_at_retirement: bool,
    /// Share of net profit contributed to a solo 401(k) or SEP, pre-tax (0.10 = 10%).
    /// Capped at the IRS limit.
    pub solo_401k_fraction: f64,
    /// Whether the business has cycles and can fail. Off by default (profit then
    /// follows only `volatility`). When on, profit also moves with a persistent
    /// good-or-bad regime and falls in recessions, and each month the business may
    /// fail at rates that fall with its age (about 20% in the first year, 34% still
    /// open after ten); a failed business pays nothing for good. "high" is a riskier
    /// business: bigger swings and a failure hazard 1.5 times higher. Needs
    /// `assumptions.career_risk` on.
    #[serde(skip_serializing_if = "crate::career_realism::is_default")]
    pub business_risk: BusinessRisk,
    /// Years the business had already been running when the plan starts. Sets its
    /// place on the failure curve; ignored for work that starts later (age 0 then).
    /// Only used with `business_risk` on. 0 means it is brand new.
    #[serde(skip_serializing_if = "crate::career_realism::is_default")]
    pub years_operating: f64,
    /// Paid work the person takes after the business fails, if any. Needs
    /// `business_risk` on. Leave empty for no income after a failure.
    #[serde(skip_serializing_if = "crate::career_realism::is_default")]
    pub fallback_wage: Option<FallbackWage>,
    /// Whether the business can close: `never` (the default) or `hazard` (a chance
    /// each year, higher for a young business and in recessions, followed by
    /// either a restart at lower profit or no income at all). A closure is a risk
    /// on top of the profit entered: it lowers average lifetime profit.
    pub failure: BusinessFailure,
    /// How unevenly profit arrives from month to month within a year, as a
    /// standard deviation relative to the average month (0.5 = months typically
    /// within 50% of the average; 1.0 for work paid in a few large invoices). The
    /// expected total for the year does not change. 0, the default, pays the same
    /// amount every month of a year.
    pub monthly_lumpiness: f64,
}

/// The job a person takes when their business fails.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct FallbackWage {
    /// Gross wage per year in today's dollars, paid until the person retires or the
    /// self-employment income's own end date, whichever comes first.
    pub annual: f64,
    /// Months between the failure and the first pay (the job search).
    pub search_months: u32,
}

impl Default for FallbackWage {
    fn default() -> Self {
        Self {
            annual: 40_000.0,
            search_months: 4,
        }
    }
}

/// Whether a self-employment income has cycles and can fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BusinessRisk {
    /// Profit varies only by `volatility` and the business never fails (the default).
    #[default]
    Off,
    /// Persistent good and bad years, lower profit in recessions, and failure at the
    /// rates of US business establishments (BLS: 79.6% survive the first year, about
    /// 35% reach ten).
    Typical,
    /// A riskier business: larger swings, bigger recession losses and failure 1.5
    /// times as likely as for a typical establishment (for example a sole
    /// proprietorship or a young venture).
    High,
}

impl Default for SelfEmployment {
    fn default() -> Self {
        Self {
            net_annual: 0.0,
            volatility: 0.25,
            real_growth: 0.0,
            ends_at_retirement: true,
            solo_401k_fraction: 0.0,
            business_risk: BusinessRisk::Off,
            years_operating: 0.0,
            fallback_wage: None,
            failure: BusinessFailure::Never,
            monthly_lumpiness: 0.0,
        }
    }
}

/// A defined-benefit pension. Set the income's `start` to the age payments begin.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Pension {
    /// Annual benefit in today's dollars at the time payments start.
    pub annual: f64,
    /// Whether payments rise with inflation once they start.
    pub cola: Cola,
    /// Fraction of the benefit a surviving partner keeps after the owner dies
    /// (0.5 for a 50% joint-and-survivor option, 0 for a single-life pension).
    pub survivor_fraction: f64,
}

impl Default for Pension {
    fn default() -> Self {
        Self {
            annual: 0.0,
            cola: Cola::None,
            survivor_fraction: 0.0,
        }
    }
}

/// Cost-of-living adjustment on a pension.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Cola {
    /// Payments rise fully with inflation (most government pensions).
    Full,
    /// Payments are fixed in dollars and lose value to inflation (most private pensions).
    #[default]
    None,
    /// Payments rise by a fixed percentage each year regardless of inflation.
    Partial {
        /// Annual increase as a decimal (0.02 = 2% per year).
        rate: f64,
    },
}

/// A lifetime annuity. Set the income's `start` to when payments begin.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Annuity {
    /// Annual payment in today's dollars at the time payments start.
    pub annual: f64,
    /// True if payments rise with inflation; false if they are fixed in dollars.
    pub real: bool,
}

/// Any other recurring income.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct OtherIncome {
    /// Amount per year in today's dollars. It keeps pace with inflation.
    pub annual: f64,
    /// Whether it is taxed as ordinary income. Gifts and child support are not taxable.
    pub taxable: bool,
    /// Expected annual growth above inflation, as a decimal. Usually 0.
    pub real_growth: f64,
    /// Whether it stops when the household retires.
    pub ends_at_retirement: bool,
}

impl Default for OtherIncome {
    fn default() -> Self {
        Self {
            annual: 0.0,
            taxable: true,
            real_growth: 0.0,
            ends_at_retirement: false,
        }
    }
}
