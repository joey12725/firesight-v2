//! Spending realism: vehicle replacement cycles, seasonal spending, category price
//! wedges, detailed irregular expenses and the unemployment spending cut. Everything
//! here is opt-in through `profile.spending`; with every switch off `SpendXPlan::active`
//! is false, the month loop calls nothing in this file and no random stream is created,
//! so results are bit-identical to a build without it.
//!
//! # Random streams
//!
//! * `Stream::Vehicle(k)`: one word per replacement of vehicle `k` (its date jitter).
//!   The schedule runs to the end of the path even after the vehicle's `end` date or
//!   its owner's death, so the number of words depends on the path's length only.
//! * `Stream::CategoryPrices`: six words every January.
//! * `Stream::DetailedShocks`: eight words every month (a uniform and a size word for
//!   each of four shock types), whether or not a shock can happen. Whether a type fires,
//!   and the clustering it feeds, depend on these words alone: a household that owns no
//!   home still has its "home repair" months, they just cost nothing. So the timing of
//!   every shock is the same in every scenario.
//!
//! None of them depends on a household decision.
//!
//! # Structure
//!
//! Budget lines are summed into an essential and a discretionary total. Seasonality and
//! the category wedge are applied to those totals as a weighted average of per-category
//! factors (weights: the category's share of the total, kept in [`Tally`] by
//! `State::rebudget`), so the hot path pays one multiplication per total.

use crate::career::Employment;
use crate::data::Data;
use crate::math;
use crate::model::{
    Category, CategoryInflation, Profile, PropertyUse, Seasonality, ShockLevel, WhenContext,
};
use crate::rng::{normal_from_bits, Rng, Stream};
use crate::stats::{TraceEventKind, YearSeries};
use crate::MAX_PEOPLE;

use super::plan::{DebtPlan, Plan, Sched};
use super::state::State;

/// Standard deviation of the random shift of a vehicle's replacement date, in months.
/// A judgement ("a small random jitter"): about half a year either way, so that a
/// ten-year cycle is seldom out by more than a year.
pub(crate) const REPLACEMENT_JITTER_SD_MONTHS: f64 = 6.0;
/// The jitter draw is clipped at this many standard deviations (a judgement).
pub(crate) const REPLACEMENT_JITTER_CLIP_SD: f64 = 2.0;
/// Months over which the unemployment spending cut phases in and out (a judgement:
/// Ganong and Noel find the drop at onset, with a further slide of under 1% a month).
pub(crate) const UNEMPLOYMENT_CUT_PHASE_MONTHS: f64 = 3.0;
/// Expected number of further shocks caused by one detailed shock (the branching ratio
/// of the Hawkes clustering; a judgement, 0 would be no clustering, 1 would be unstable).
pub(crate) const SHOCK_BRANCHING_RATIO: f64 = 0.3;
/// Months for the raised shock hazard to halve ("a bad year"; a judgement).
pub(crate) const SHOCK_CLUSTER_HALF_LIFE_MONTHS: f64 = 12.0;
/// The size draw of a detailed shock is capped at this many standard deviations of its
/// log (a judgement; it removes about 1% of the mean of the heaviest type).
pub(crate) const SHOCK_Z_CAP: f64 = 3.5;

/// The yearly wedge draw is limited to this many standard deviations: the largest
/// the 53-bit uniform can produce (8.2), so it only guards the non-number at the top.
pub(crate) const WEDGE_Z_CAP: f64 = 8.5;

/// Number of price-wedge groups.
pub(crate) const N_GROUPS: usize = 6;
const GROUP_KEYS: [&str; N_GROUPS] = [
    "food",
    "utilities",
    "transport",
    "shelter",
    "medical",
    "education",
];
/// Wedge group of each category (`N_GROUPS` = none: the category tracks CPI).
const NO_GROUP: usize = N_GROUPS;
const GROUP_OF: [usize; Category::COUNT] = {
    let mut g = [NO_GROUP; Category::COUNT];
    g[Category::Groceries as usize] = 0;
    g[Category::Dining as usize] = 0;
    g[Category::Utilities as usize] = 1;
    g[Category::Transport as usize] = 2;
    g[Category::Rent as usize] = 3;
    g[Category::Healthcare as usize] = 4;
    g[Category::Education as usize] = 5;
    g[Category::Childcare as usize] = 5;
    g
};

const SHOCK_KINDS: usize = 4;
const SHOCK_HOME: usize = 0;
const SHOCK_CAR: usize = 1;
const SHOCK_LABELS: [&str; SHOCK_KINDS] = [
    "Home repair",
    "Car repair",
    "Medical-adjacent expense",
    "Family emergency",
];
const SHOCK_KEYS: [&str; SHOCK_KINDS] = [
    "home_repair",
    "car_repair",
    "medical_adjacent",
    "family_emergency",
];
const SHOCK_BASE_FIELDS: [&str; SHOCK_KINDS] = [
    "median_fraction_of_home_value",
    "median_fraction_of_transport_spending",
    "median_fraction_of_annual_spending",
    "median_fraction_of_annual_spending",
];

fn category_key(c: Category) -> &'static str {
    match c {
        Category::Rent => "rent",
        Category::Utilities => "utilities",
        Category::Groceries => "groceries",
        Category::Dining => "dining",
        Category::Transport => "transport",
        Category::Healthcare => "healthcare",
        Category::Insurance => "insurance",
        Category::Childcare => "childcare",
        Category::Education => "education",
        Category::Travel => "travel",
        Category::Entertainment => "entertainment",
        Category::Personal => "personal",
        Category::Giving => "giving",
        Category::Other => "other",
    }
}

/// One vehicle, compiled.
#[derive(Clone, Debug)]
pub(crate) struct VehiclePlan {
    pub name: String,
    /// Replacement price in today's dollars.
    pub price: f64,
    pub cycle_months: i32,
    /// Month of the first replacement before jitter.
    pub first_month: i32,
    pub trade_in: f64,
    /// `(down fraction, debt slot, rate, term months)`.
    pub financing: Option<(f64, usize, f64, u32)>,
    pub owner: Option<usize>,
    pub end: Sched,
}

/// Wedge parameters of one group.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WedgeGroup {
    pub phi: f64,
    pub sigma: f64,
}

/// One detailed shock type, compiled.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ShockKind {
    /// Monthly hazard before clustering: `lambda (1 - n) / 12`.
    pub rate: f64,
    pub log_median: f64,
    pub sigma: f64,
}

/// Detailed irregular expenses, compiled.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DetailedPlan {
    pub kinds: [ShockKind; SHOCK_KINDS],
    /// Monthly decay of the excitement.
    pub decay: f64,
    /// Excitement added by one shock.
    pub jump: f64,
    /// Excitement at the start of a path: its long-run mean `n / (1 - n)`, so that the
    /// expected hazard is the calibrated one from the first month.
    pub excite0: f64,
    /// Expected annual home-repair cost as a fraction of the home's value. The plan
    /// takes it out of an owner-occupied home's `maintenance_rate`.
    pub home_rate: f64,
}

/// Everything compiled from the new `spending` fields.
#[derive(Clone, Debug)]
pub(crate) struct SpendXPlan {
    /// Any switch is on (the state needs the extra buffers and streams).
    pub active: bool,
    /// Seasonality, the wedge or the unemployment cut act on the monthly budget.
    pub monthly: bool,
    /// Per-category totals are kept at each rebudget.
    pub tally: bool,
    /// Seasonality is on.
    pub seasonal: bool,
    pub vehicles: Vec<VehiclePlan>,
    /// Monthly factor of each category, January first, each row averaging 1 (all ones
    /// when seasonality is off).
    pub season: [[f64; 12]; Category::COUNT],
    pub wedge: Option<[WedgeGroup; N_GROUPS]>,
    pub unemployment_cut: f64,
    pub detailed: Option<DetailedPlan>,
}

impl SpendXPlan {
    /// A plan with every feature off.
    pub(crate) fn off() -> SpendXPlan {
        SpendXPlan {
            active: false,
            monthly: false,
            tally: false,
            seasonal: false,
            vehicles: Vec::new(),
            season: [[1.0; 12]; Category::COUNT],
            wedge: None,
            unemployment_cut: 0.0,
            detailed: None,
        }
    }

    /// Compiles the new spending fields. Financed vehicles add a debt slot each to
    /// `debts`. Returns the plan and, for detailed shocks, the expected annual cost
    /// at the start as a fraction of the annual budget (to be netted out of it).
    pub(crate) fn compile(
        profile: &Profile,
        data: &Data,
        ctx: &WhenContext,
        debts: &mut Vec<DebtPlan>,
        start_budget_annual: f64,
    ) -> (SpendXPlan, f64) {
        let s = &profile.spending;
        let realism = &data.actuarial["spending_realism"];
        let mut plan = SpendXPlan::off();

        for v in &s.vehicles {
            let financing = v.financing.map(|f| {
                let slot = debts.len();
                debts.push(DebtPlan {
                    name: if v.name.is_empty() {
                        v.id.clone()
                    } else {
                        v.name.clone()
                    },
                    balance: 0.0,
                    rate: f.rate.max(0.0),
                    payment: 0.0,
                    revolving: false,
                });
                (
                    f.down_fraction.clamp(0.0, 1.0),
                    slot,
                    f.rate.max(0.0),
                    f.term_months.max(1),
                )
            });
            let cycle_months = (v.replace_every_years * 12.0).round().max(12.0) as i32;
            let left = ((v.replace_every_years - v.age_years) * 12.0).round();
            plan.vehicles.push(VehiclePlan {
                name: if v.name.is_empty() {
                    v.id.clone()
                } else {
                    v.name.clone()
                },
                price: v.price.max(0.0),
                cycle_months,
                first_month: left.max(0.0) as i32,
                trade_in: v.trade_in_fraction.clamp(0.0, 1.0),
                financing,
                owner: v.owner,
                end: Sched::end(v.end.as_ref(), ctx),
            });
        }

        if s.seasonality == Seasonality::Typical {
            for c in Category::ALL {
                let row = &realism["seasonality"]["weights"][category_key(c)];
                if let Some(w) = row.as_array() {
                    let mut vals = [1.0; 12];
                    for (m, x) in w.iter().take(12).enumerate() {
                        vals[m] = x.as_f64().expect("embedded data: seasonality weight");
                    }
                    let mean: f64 = vals.iter().sum::<f64>() / 12.0;
                    for m in 0..12 {
                        plan.season[c.index()][m] = vals[m] / mean;
                    }
                }
            }
        }

        if s.category_inflation == CategoryInflation::Stochastic {
            let mut groups = [WedgeGroup::default(); N_GROUPS];
            for (g, key) in GROUP_KEYS.iter().enumerate() {
                let node = &realism["category_inflation"]["groups"][*key];
                groups[g] = WedgeGroup {
                    phi: node["phi"].as_f64().expect("embedded data: wedge phi"),
                    sigma: node["sigma_eps"]
                        .as_f64()
                        .expect("embedded data: wedge sigma"),
                };
            }
            plan.wedge = Some(groups);
        }

        plan.unemployment_cut = s.unemployment_cut.clamp(0.0, 1.0);

        let mut mean_fraction = 0.0;
        if s.shocks == ShockLevel::Detailed {
            let node = &realism["detailed_shocks"]["types"];
            let n = SHOCK_BRANCHING_RATIO;
            let mut kinds = [ShockKind::default(); SHOCK_KINDS];
            let mut total_rate = 0.0;
            let mut means = [0.0; SHOCK_KINDS];
            for k in 0..SHOCK_KINDS {
                let t = &node[SHOCK_KEYS[k]];
                let lambda = t["lambda_per_year"]
                    .as_f64()
                    .expect("embedded data: lambda");
                let median = t[SHOCK_BASE_FIELDS[k]]
                    .as_f64()
                    .expect("embedded data: median");
                let sigma = t["sigma"].as_f64().expect("embedded data: sigma");
                kinds[k] = ShockKind {
                    rate: lambda * (1.0 - n) / 12.0,
                    log_median: math::ln(median),
                    sigma,
                };
                total_rate += kinds[k].rate;
                // Expected cost of a shock as a fraction of its base (the cap on the
                // draw is ignored: it removes about 1% of the mean).
                means[k] = lambda * median * math::exp(0.5 * sigma * sigma);
            }
            let decay = math::exp(-std::f64::consts::LN_2 / SHOCK_CLUSTER_HALF_LIFE_MONTHS);
            plan.detailed = Some(DetailedPlan {
                kinds,
                decay,
                jump: n * (1.0 - decay) / total_rate,
                excite0: n / (1.0 - n),
                home_rate: means[SHOCK_HOME],
            });
            // Expected annual cost at the start, in today's dollars. Home repairs are
            // not netted here: the budget never contained owned-home costs (they are
            // computed from the property). `Plan::compile` takes their expected cost
            // out of the home's maintenance_rate instead.
            let transport: f64 = s
                .items
                .iter()
                .filter(|i| i.category == Category::Transport && i.start.is_none())
                .map(|i| i.annual_amount())
                .sum();
            let car_base = transport + vehicle_annual_cost(&plan.vehicles);
            let expected =
                means[SHOCK_CAR] * car_base + (means[2] + means[3]) * start_budget_annual;
            if start_budget_annual > 0.0 {
                mean_fraction = (expected / start_budget_annual).min(0.25);
            }
        }

        plan.seasonal = s.seasonality == Seasonality::Typical;
        plan.tally = plan.wedge.is_some()
            || s.seasonality == Seasonality::Typical
            || plan.detailed.is_some();
        plan.monthly = plan.wedge.is_some()
            || s.seasonality == Seasonality::Typical
            || plan.unemployment_cut > 0.0;
        plan.active = plan.monthly || plan.tally || !plan.vehicles.is_empty();
        (plan, mean_fraction)
    }
}

/// Replacement prices spread over the cycles, net of trade-in, per year, today's dollars.
fn vehicle_annual_cost(vehicles: &[VehiclePlan]) -> f64 {
    vehicles
        .iter()
        .map(|v| v.price * (1.0 - v.trade_in) * 12.0 / f64::from(v.cycle_months))
        .sum()
}

/// Real monthly budget by category, essential and discretionary (the weights of the
/// monthly factors).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Tally {
    pub e: [f64; Category::COUNT],
    pub d: [f64; Category::COUNT],
    /// Rent lines that follow the path's rent market (`rent_growth: market`),
    /// essential and discretionary. Kept out of `e` and `d` because their rent index
    /// already moves against CPI, so the shelter wedge must not act on them again;
    /// they still take the rent category's seasonal factor.
    pub market_rent: [f64; 2],
}

/// The wedge process of one group after `steps` Januaries: its mean-correction needs the
/// variance of the log, which is known in closed form.
#[inline]
pub(crate) fn wedge_step(g: WedgeGroup, x: f64, var: f64, z: f64) -> (f64, f64, f64) {
    let x = g.phi * x + g.sigma * z;
    let var = g.phi * g.phi * var + g.sigma * g.sigma;
    (x, var, math::exp(x - 0.5 * var))
}

/// Per-path state of the new features.
#[derive(Clone, Debug)]
pub(crate) struct SpendXState {
    pub tally: Tally,
    pub rng_vehicle: Vec<Rng>,
    /// Unjittered month of each vehicle's next replacement.
    pub veh_base: Vec<i32>,
    /// Month of each vehicle's next scheduled replacement.
    pub veh_next: Vec<i32>,
    pub rng_prices: Rng,
    pub x: [f64; N_GROUPS],
    pub var: [f64; N_GROUPS],
    /// Price multiplier of each group (1 when the wedge is off).
    pub wedge: [f64; N_GROUPS],
    /// Current share of discretionary spending cut for unemployment.
    pub cut_level: f64,
    pub rng_shocks: Rng,
    /// Excitement of the shock hazard.
    pub excite: f64,
}

impl SpendXState {
    pub(crate) fn new(plan: &Plan) -> SpendXState {
        let rng = Rng::from_seed(0);
        let n = plan.sx.vehicles.len();
        SpendXState {
            tally: Tally::default(),
            rng_vehicle: vec![rng.clone(); n],
            veh_base: vec![0; n],
            veh_next: vec![i32::MAX; n],
            rng_prices: rng.clone(),
            x: [0.0; N_GROUPS],
            var: [0.0; N_GROUPS],
            wedge: [1.0; N_GROUPS],
            cut_level: 0.0,
            rng_shocks: rng,
            excite: 0.0,
        }
    }

    /// Resets for path `path`. Allocation-free.
    pub(crate) fn begin(&mut self, plan: &Plan, seed: u64, path: u64) {
        let sx = &plan.sx;
        if !sx.active {
            return;
        }
        for (k, v) in sx.vehicles.iter().enumerate() {
            self.rng_vehicle[k] = Rng::stream(seed, path, Stream::Vehicle(k));
            self.veh_base[k] = v.first_month;
            let j = jitter_months(self.rng_vehicle[k].next_u64());
            self.veh_next[k] = (v.first_month + j).max(1);
        }
        if sx.wedge.is_some() {
            self.rng_prices = Rng::stream(seed, path, Stream::CategoryPrices);
        }
        self.x = [0.0; N_GROUPS];
        self.var = [0.0; N_GROUPS];
        self.wedge = [1.0; N_GROUPS];
        self.cut_level = 0.0;
        if sx.detailed.is_some() {
            self.rng_shocks = Rng::stream(seed, path, Stream::DetailedShocks);
        }
        self.excite = sx.detailed.map_or(0.0, |d| d.excite0);
        self.tally = Tally::default();
    }
}

/// A standard normal from one random word, limited to `+-cap`. The uniform of the
/// largest word rounds to exactly 1, where the inverse CDF is not a number: that is
/// the upper tail, so it returns `cap`.
#[inline]
pub(crate) fn capped_normal(bits: u64, cap: f64) -> f64 {
    let z = normal_from_bits(bits);
    if z.is_nan() {
        cap
    } else {
        z.clamp(-cap, cap)
    }
}

/// The date shift in months from one random word.
#[inline]
pub(crate) fn jitter_months(bits: u64) -> i32 {
    let z = capped_normal(bits, REPLACEMENT_JITTER_CLIP_SD);
    (z * REPLACEMENT_JITTER_SD_MONTHS).round() as i32
}

impl State {
    /// Monthly multipliers on the essential and discretionary budget, and the share of
    /// discretionary spending kept after the unemployment cut. `(1, 1, 1)` exactly
    /// when no monthly feature is on.
    #[inline]
    pub(crate) fn sx_factors(&mut self, plan: &Plan, t: usize, month: u32) -> (f64, f64, f64) {
        if !plan.sx.monthly {
            return (1.0, 1.0, 1.0);
        }
        self.sx_factors_slow(plan, t, month)
    }

    #[inline(never)]
    fn sx_factors_slow(&mut self, plan: &Plan, t: usize, month: u32) -> (f64, f64, f64) {
        let sx = &plan.sx;
        if let Some(groups) = &sx.wedge {
            if month == 1 && t > 0 {
                for g in 0..N_GROUPS {
                    let z = capped_normal(self.sx.rng_prices.next_u64(), WEDGE_Z_CAP);
                    let (x, var, w) = wedge_step(groups[g], self.sx.x[g], self.sx.var[g], z);
                    self.sx.x[g] = x;
                    self.sx.var[g] = var;
                    self.sx.wedge[g] = w;
                }
            }
        }
        let (mut ess, mut disc) = (1.0, 1.0);
        if sx.wedge.is_some() || sx.seasonal {
            let m = (month - 1) as usize;
            let (mut fe, mut we, mut fd, mut wd) = (0.0, 0.0, 0.0, 0.0);
            for c in 0..Category::COUNT {
                let wedge = if GROUP_OF[c] < N_GROUPS {
                    self.sx.wedge[GROUP_OF[c]]
                } else {
                    1.0
                };
                let f = sx.season[c][m] * wedge;
                fe += self.sx.tally.e[c] * f;
                we += self.sx.tally.e[c];
                fd += self.sx.tally.d[c] * f;
                wd += self.sx.tally.d[c];
            }
            let f = sx.season[Category::Rent.index()][m];
            fe += self.sx.tally.market_rent[0] * f;
            we += self.sx.tally.market_rent[0];
            fd += self.sx.tally.market_rent[1] * f;
            wd += self.sx.tally.market_rent[1];
            if we > 0.0 {
                ess = fe / we;
            }
            if wd > 0.0 {
                disc = fd / wd;
            }
        }
        let mut keep = 1.0;
        if sx.unemployment_cut > 0.0 {
            let mut unemployed = false;
            for (j, jp) in plan.jobs.iter().enumerate() {
                // A disabled person's job is parked in the unemployed state
                // (`rz_disability.rs`), but a disability is not an unemployment
                // spell: it can last to retirement, and its benefits are not
                // unemployment insurance. The cut is for job loss only.
                let disabled = plan.rz.any && self.rz.dis.get(jp.owner).is_some_and(|d| d.disabled);
                if jp.owner < MAX_PEOPLE
                    && self.present[jp.owner]
                    && !disabled
                    && matches!(self.jobs[j].st.employment, Employment::Unemployed { .. })
                {
                    unemployed = true;
                }
            }
            let step = sx.unemployment_cut / UNEMPLOYMENT_CUT_PHASE_MONTHS;
            let before = self.sx.cut_level;
            let level = if unemployed {
                (before + step).min(sx.unemployment_cut)
            } else {
                (before - step).max(0.0)
            };
            self.sx.cut_level = level;
            if before == 0.0 && level > 0.0 {
                let share = sx.unemployment_cut;
                let now = plan.start + t as i32;
                self.event(now, TraceEventKind::SpendingCut, None, || {
                    format!(
                        "Unemployed: discretionary spending cut by {:.0}%",
                        share * 100.0
                    )
                });
            } else if before > 0.0 && level == 0.0 {
                let now = plan.start + t as i32;
                self.event(now, TraceEventKind::SpendingRaise, None, || {
                    "Back to work: discretionary spending restored".to_string()
                });
            }
            keep = 1.0 - level;
        }
        (ess, disc, keep)
    }

    /// The multiplier this calendar `month` puts on the lines of category `c`
    /// (seasonality times the path's price wedge of its group), as
    /// [`State::sx_factors`] weighs it into the month's budget. Read after
    /// `sx_factors` has run for the month; exactly 1 when no monthly feature is on.
    pub(crate) fn sx_category_factor(&self, plan: &Plan, c: Category, month: u32) -> f64 {
        let sx = &plan.sx;
        if !sx.monthly || !(sx.wedge.is_some() || sx.seasonal) {
            return 1.0;
        }
        let ci = c.index();
        let wedge = if GROUP_OF[ci] < N_GROUPS {
            self.sx.wedge[GROUP_OF[ci]]
        } else {
            1.0
        };
        sx.season[ci][(month - 1) as usize] * wedge
    }

    /// The share of discretionary spending kept after the unemployment cut this
    /// month (the third value of [`State::sx_factors`]); 1 when the cut is off.
    pub(crate) fn sx_discretionary_keep(&self, plan: &Plan) -> f64 {
        if !plan.sx.monthly || plan.sx.unemployment_cut <= 0.0 {
            return 1.0;
        }
        1.0 - self.sx.cut_level
    }

    /// What the vehicles the household is still replacing cost a year on average, in
    /// today's dollars: each one's price net of trade-in spread over its cycle. The
    /// funded test adds it to the annual need, like a home's running costs: the
    /// replacements are lumpy, but they are a cost the retirement has to carry. A
    /// vehicle whose owner is gone or whose `end` date has passed (read against
    /// `retire_month`) is left out. Loan interest on a financed vehicle is not counted.
    pub(crate) fn sx_vehicle_need(&self, plan: &Plan, ti: i32, retire_month: i32) -> f64 {
        let mut annual = 0.0;
        for v in &plan.sx.vehicles {
            let owner_gone = v.owner.is_some_and(|o| o >= MAX_PEOPLE || !self.present[o]);
            if owner_gone || ti >= v.end.month(retire_month) {
                continue;
            }
            annual += v.price * (1.0 - v.trade_in) * 12.0 / f64::from(v.cycle_months);
        }
        annual
    }

    /// Replaces vehicles that are due this month.
    #[inline]
    pub(crate) fn sx_vehicles(&mut self, plan: &Plan, t: usize, price: f64) {
        if plan.sx.vehicles.is_empty() {
            return;
        }
        let ti = t as i32;
        for k in 0..plan.sx.vehicles.len() {
            if ti >= self.sx.veh_next[k] {
                self.sx_replace(plan, k, t, price);
            }
        }
    }

    #[inline(never)]
    fn sx_replace(&mut self, plan: &Plan, k: usize, t: usize, price: f64) {
        let ti = t as i32;
        let v = &plan.sx.vehicles[k];
        // A vehicle whose owner is not in the household (not yet joined, or dead) or
        // whose end date has passed is not bought, but its schedule runs on: the stream
        // gives one word per scheduled date whatever the household does.
        let owner_gone = v.owner.is_some_and(|o| o >= MAX_PEOPLE || !self.present[o]);
        if !(owner_gone || ti >= v.end.month(self.retire_month)) {
            self.sx_buy_vehicle(plan, k, t, price);
        }
        // The next replacement: one cycle after the unjittered date of this one.
        let base = self.sx.veh_base[k].saturating_add(v.cycle_months);
        self.sx.veh_base[k] = base;
        let j = jitter_months(self.sx.rng_vehicle[k].next_u64());
        self.sx.veh_next[k] = (base + j).max(ti + 1);
    }

    /// Pays for the replacement of vehicle `k` in month `t`.
    fn sx_buy_vehicle(&mut self, plan: &Plan, k: usize, t: usize, price: f64) {
        let v = &plan.sx.vehicles[k];
        let net = v.price * (1.0 - v.trade_in) * price;
        let now = plan.start + t as i32;
        match v.financing {
            Some((down, slot, rate, term)) => {
                let cash = net * down;
                let loan = net - cash;
                self.spend(YearSeries::SpendOneOff, cash);
                let d = &mut self.debts[slot];
                if d.balance <= 0.0 && d.payment > 0.0 {
                    // The last loan on this vehicle was paid off and its payment was
                    // released to the extra debt payment; the new loan takes it back.
                    self.freed_payments = (self.freed_payments - d.payment).max(0.0);
                }
                d.balance += loan;
                d.rate = rate;
                d.payment = crate::housing::amortized_payment(d.balance, rate, term);
                self.debts_active = true;
                if self.check {
                    self.chk_debt_flow += loan;
                }
            }
            None => self.spend(YearSeries::SpendOneOff, net),
        }
        self.event(
            now,
            TraceEventKind::MajorExpense,
            Some(v.price * (1.0 - v.trade_in)),
            || format!("Replace vehicle: {}", v.name),
        );
    }

    /// Draws the month's detailed irregular expenses.
    #[inline]
    pub(crate) fn sx_shocks(&mut self, plan: &Plan, t: usize, price: f64) {
        if let Some(det) = &plan.sx.detailed {
            self.sx_shocks_slow(plan, det, t, price);
        }
    }

    #[inline(never)]
    fn sx_shocks_slow(&mut self, plan: &Plan, det: &DetailedPlan, t: usize, price: f64) {
        // Eight words every month, used or not.
        let mut u = [0.0; SHOCK_KINDS];
        let mut bits = [0u64; SHOCK_KINDS];
        for k in 0..SHOCK_KINDS {
            u[k] = self.sx.rng_shocks.uniform();
            bits[k] = self.sx.rng_shocks.next_u64();
        }
        let annual = (self.ess_real + self.disc_real) * 12.0 * price;
        let transport = (self.sx.tally.e[Category::Transport.index()]
            + self.sx.tally.d[Category::Transport.index()])
            * 12.0
            * price;
        let vehicles = vehicle_annual_cost(&plan.sx.vehicles) * price;
        let mut home = 0.0;
        for p in &self.props {
            if p.owned && matches!(p.usage, PropertyUse::Primary) {
                home += p.value;
            }
        }
        let bases = [home, transport + vehicles, annual, annual];
        let hazard = 1.0 + self.sx.excite;
        let mut fired = 0.0;
        for k in 0..SHOCK_KINDS {
            let kind = &det.kinds[k];
            let p = 1.0 - math::exp(-kind.rate * hazard);
            if u[k] >= p {
                continue;
            }
            // The excitement follows the draws, not the household: a type with nothing
            // to break (no home, no car) still counts, so the others keep their
            // calibrated frequency and their timing in every scenario.
            fired += 1.0;
            if bases[k] > 0.0 {
                let z = capped_normal(bits[k], SHOCK_Z_CAP);
                let cost = bases[k] * math::exp(kind.log_median + kind.sigma * z);
                if cost > 0.0 {
                    self.spend(YearSeries::SpendOneOff, cost);
                    let now = plan.start + t as i32;
                    self.event(
                        now,
                        TraceEventKind::ExpenseShock,
                        Some(cost / price),
                        || SHOCK_LABELS[k].to_string(),
                    );
                }
            }
        }
        self.sx.excite = self.sx.excite * det.decay + det.jump * fired;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Vehicle;

    fn vp(price: f64, trade_in: f64, cycle_months: i32) -> VehiclePlan {
        VehiclePlan {
            name: "v".into(),
            price,
            cycle_months,
            first_month: 0,
            trade_in,
            financing: None,
            owner: None,
            end: Sched::Never,
        }
    }

    #[test]
    fn judgement_constants_match_the_data_file() {
        let j = &crate::data::Data::embedded().actuarial["spending_realism"]["judgement_constants"];
        let num = |k: &str| j[k].as_f64().unwrap();
        assert_eq!(
            REPLACEMENT_JITTER_SD_MONTHS,
            num("replacement_jitter_sd_months")
        );
        assert_eq!(
            REPLACEMENT_JITTER_CLIP_SD,
            num("replacement_jitter_clip_sd")
        );
        assert_eq!(
            UNEMPLOYMENT_CUT_PHASE_MONTHS,
            num("unemployment_cut_phase_months")
        );
        assert_eq!(SHOCK_BRANCHING_RATIO, num("shock_branching_ratio"));
        assert_eq!(
            SHOCK_CLUSTER_HALF_LIFE_MONTHS,
            num("shock_cluster_half_life_months")
        );
        assert_eq!(SHOCK_Z_CAP, num("shock_z_cap"));
    }

    #[test]
    fn jitter_is_zero_at_the_median_and_clipped_at_two_sd() {
        // The middle word maps to the median of the normal: no shift.
        assert_eq!(jitter_months(1u64 << 63), 0);
        // The extreme words clip at 2 sd = 12 months.
        assert_eq!(jitter_months(0), -12);
        assert_eq!(jitter_months(u64::MAX), 12);
        // One sd above the median is a uniform of 0.8413: 6 months.
        let one_sd = ((0.841_344_746_068_543 * (1u64 << 53) as f64) as u64) << 11;
        assert_eq!(jitter_months(one_sd), 6);
    }

    #[test]
    fn wedge_step_matches_hand_values_and_the_mean_correction() {
        let g = WedgeGroup {
            phi: 0.5,
            sigma: 0.1,
        };
        // First step with z = 1: x = 0.1, Var = 0.01, multiplier exp(0.1 - 0.005).
        let (x, var, w) = wedge_step(g, 0.0, 0.0, 1.0);
        assert!((x - 0.1).abs() < 1e-15 && (var - 0.01).abs() < 1e-15);
        assert!((w - crate::math::exp(0.1f64 - 0.005)).abs() < 1e-15);
        // Second step with z = -1: x = 0.05 - 0.1, Var = 0.25 x 0.01 + 0.01 = 0.0125.
        let (x, var, w) = wedge_step(g, x, var, -1.0);
        assert!((x + 0.05).abs() < 1e-15 && (var - 0.0125).abs() < 1e-15);
        assert!((w - crate::math::exp(-0.05f64 - 0.00625)).abs() < 1e-15);
    }

    #[test]
    fn the_wedge_has_mean_one_at_every_horizon() {
        // The calibrated utilities wedge (phi 0.923, sigma 0.0305) over 30 years: its
        // log has variance 0.0062, so without the correction the mean would be 1.0031,
        // 17 standard errors above 1 with this many draws.
        let g = WedgeGroup {
            phi: 0.923,
            sigma: 0.0305,
        };
        let n = 200_000;
        let mut rng = Rng::stream(1, 0, Stream::CategoryPrices);
        for steps in [1usize, 5, 30] {
            let mut sum = 0.0;
            for _ in 0..n {
                let (mut x, mut var, mut w) = (0.0, 0.0, 1.0);
                for _ in 0..steps {
                    let z = capped_normal(rng.next_u64(), WEDGE_Z_CAP);
                    (x, var, w) = wedge_step(g, x, var, z);
                }
                sum += w;
            }
            let mean = sum / n as f64;
            assert!((mean - 1.0).abs() < 0.0015, "{steps} steps: mean {mean}");
        }
    }

    #[test]
    fn annualised_vehicle_cost_is_the_net_price_over_the_cycle() {
        // 40,000 less a quarter, every 120 months: 3,000 a year.
        assert!((vehicle_annual_cost(&[vp(40_000.0, 0.25, 120)]) - 3_000.0).abs() < 1e-9);
        // Two of them.
        let two = [vp(40_000.0, 0.25, 120), vp(20_000.0, 0.0, 96)];
        assert!((vehicle_annual_cost(&two) - (3_000.0 + 2_500.0)).abs() < 1e-9);
    }

    #[test]
    fn the_first_replacement_is_the_cycle_less_the_age() {
        use crate::data::Data;
        let mut p = crate::model::Profile::example();
        let mut v = Vehicle {
            id: "c".into(),
            price: 30_000.0,
            replace_every_years: 8.0,
            age_years: 3.0,
            ..Vehicle::default()
        };
        let compile = |v: &Vehicle, p: &mut crate::model::Profile| {
            p.spending.vehicles = vec![v.clone()];
            let ctx = p.when_context().unwrap();
            let mut debts = Vec::new();
            SpendXPlan::compile(p, Data::embedded(), &ctx, &mut debts, 50_000.0).0
        };
        let plan = compile(&v, &mut p);
        assert_eq!(plan.vehicles[0].first_month, 60);
        assert_eq!(plan.vehicles[0].cycle_months, 96);
        v.age_years = 12.0;
        assert_eq!(compile(&v, &mut p).vehicles[0].first_month, 0);
    }
}
