//! Borrowing against a home: a home-equity line of credit (HELOC), a reverse mortgage
//! (FHA Home Equity Conversion Mortgage, HECM) and a cash-out refinance.
//!
//! Responsibility: the arithmetic of one line on one property: how much can be
//! borrowed, what it costs each month, how the balance grows. The simulation
//! (`sim/housing_ext.rs`) decides when to open and draw, pays the cash flows and books
//! the balances. All amounts are nominal dollars; rates are annual decimals.
//!
//! # HELOC
//!
//! ```text
//! credit limit  = max_combined_ltv x value - mortgage            (set when opened)
//! available     = min(limit, max_combined_ltv x value - mortgage) - balance
//!                 during the draw period, 0 after it or while lines are frozen
//! rate          = Treasury bill yield + spread                   (variable, monthly)
//! payment       = interest only during the draw period, then the level payment
//!                 that repays the balance over the months left of the repayment period
//! ```
//!
//! Sources: the draw/repayment structure and the prime-plus-margin rate are the
//! CFPB's description of a HELOC ("What you should know about home equity lines of
//! credit", 2012); the prime rate is the federal funds target plus 3 points, and
//! Bankrate's average HELOC rate ran 0.3 to 1.4 points over prime in 2025 (7.81% in
//! December 2025 against a 6.75% prime), hence the default spread of 3.5 points over
//! bills. Lenders cut or froze lines when home values fell in 2008: in the Federal
//! Reserve's April 2008 Senior Loan Officer Survey about half of the banks had
//! tightened terms on existing lines, nearly all citing lower collateral values, and
//! about 70% had tightened standards for new ones. The probability of a freeze per downturn is a judgement
//! ([`HelocTerms::freeze_probability`], schema default 0.3).
//!
//! # Reverse mortgage (HECM)
//!
//! ```text
//! maximum claim amount  = min(value, FHA limit, indexed with home prices)
//! principal limit       = PLF(age of the youngest borrower, expected rate) x MCA
//! expected rate         = 10-year Treasury yield + lender margin, at least 3%
//! financed at closing   = 2% x MCA (upfront mortgage insurance) + origination fee
//!                         + other closing costs + the existing mortgage, if any
//! line of credit        = principal limit - financed at closing
//! each month            balance and unused line both grow at
//!                       (bill yield + margin + 0.5% annual mortgage insurance) / 12
//! first 12 months       disbursements limited to 60% of the principal limit (or the
//!                       mandatory obligations plus 10%, if more)
//! repaid                at sale or at the death of the last borrower: the lesser of
//!                       the balance and what the home fetches (non-recourse)
//! ```
//!
//! Sources: HUD Mortgagee Letter 2017-12 (2% upfront and 0.5% annual mortgage
//! insurance premium, the principal limit factor table from October 2017; the grid
//! below is HUD's published excerpt of that table at ages 62, 72, 82 and 92 and
//! expected rates of 3% to 8%, interpolated linearly in between); HUD Mortgagee Letter
//! 2025-22 (2026 national maximum claim amount $1,249,125); 24 CFR 206.31 (origination
//! fee: the greater of $2,500 or 2% of the first $200,000 of the maximum claim amount
//! plus 1% above it, at most $6,000); HUD Mortgagee Letter 2013-27 (60% first-year
//! disbursement limit).
//!
//! # Simplifications
//!
//! * A HELOC is opened only on the main home, at the moment it is first needed, and
//!   the lender asks for nothing but equity (no income test). Closing costs of a line
//!   are taken as zero (lenders commonly waive them).
//! * HELOC interest is not deducted: since 2018 interest on home-equity debt is
//!   deductible only when the money improves the home.
//! * A HECM borrower is the youngest adult present; non-borrowing spouses are not
//!   modelled. The expected rate is not rounded to an eighth of a point, and the PLF
//!   is interpolated between the published grid points rather than read from the full
//!   table. Interest accrued on a HECM is deductible only when paid, which is ignored.

use crate::housing::amortized_payment;

/// A downturn for home-equity lenders: a recession, or national home prices at least
/// this far below their running peak. A judgement (`data/actuarial/params.json`,
/// `housing_decisions.heloc.housing_slump_drawdown`): the 2007-09 line cuts came with
/// price falls of this order before the recession was declared.
pub const HOUSING_SLUMP_DRAWDOWN: f64 = 0.10;

/// Annual FHA mortgage insurance premium on a HECM balance (ML 2017-12).
pub const HECM_ANNUAL_MIP: f64 = 0.005;
/// Upfront FHA mortgage insurance premium, share of the maximum claim amount
/// (ML 2017-12).
pub const HECM_UPFRONT_MIP: f64 = 0.02;
/// National HECM maximum claim amount for 2026 (ML 2025-22), dollars of the year
/// the plan's data describe; indexed with home prices on a path.
pub const HECM_MAX_CLAIM_2026: f64 = 1_249_125.0;
/// Youngest age at which a HECM can be taken (24 CFR 206.33).
pub const HECM_MIN_AGE: f64 = 62.0;
/// Floor of the expected rate used to look up the principal limit factor (the
/// 2017 table starts at 3%).
pub const HECM_EXPECTED_RATE_FLOOR: f64 = 0.03;
/// Share of the principal limit that may be disbursed in the first 12 months
/// (ML 2013-27).
pub const HECM_FIRST_YEAR_SHARE: f64 = 0.60;
/// Extra share allowed on top of mandatory obligations in the first year
/// (ML 2013-27).
pub const HECM_FIRST_YEAR_EXTRA: f64 = 0.10;
/// Origination fee limits (24 CFR 206.31): minimum, cap, and the 2% / 1% tiers
/// around $200,000 of maximum claim amount. Fixed dollars in the regulation.
pub const HECM_ORIGINATION_MIN: f64 = 2_500.0;
pub const HECM_ORIGINATION_MAX: f64 = 6_000.0;
pub const HECM_ORIGINATION_TIER: f64 = 200_000.0;

/// Ages of the principal limit factor grid (HUD's 2017 selected factors).
pub const PLF_AGES: [f64; 4] = [62.0, 72.0, 82.0, 92.0];
/// Expected rates of the grid.
pub const PLF_RATES: [f64; 6] = [0.03, 0.04, 0.05, 0.06, 0.07, 0.08];
/// Principal limit factors for case numbers from 2 October 2017, by age (rows) and
/// expected rate (columns). HUD, "HECM Program: Selected Principal Limit Factors"
/// (August 2017, published with ML 2017-12).
pub const PLF_TABLE: [[f64; 6]; 4] = [
    [0.524, 0.470, 0.410, 0.357, 0.312, 0.272],
    [0.588, 0.524, 0.467, 0.416, 0.372, 0.332],
    [0.658, 0.605, 0.556, 0.511, 0.471, 0.434],
    [0.750, 0.715, 0.682, 0.650, 0.620, 0.592],
];

/// Which kind of line is open on a property.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineKind {
    /// Nothing open.
    #[default]
    Closed,
    /// A home-equity line of credit.
    Heloc,
    /// A reverse mortgage line of credit.
    Hecm,
}

/// One line on one property.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct EquityLine {
    /// What is open.
    pub kind: LineKind,
    /// Owed: draws, interest and (HECM) financed costs and insurance.
    pub balance: f64,
    /// HELOC: the credit limit set at opening. HECM: the unused line of credit.
    pub limit: f64,
    /// Months since the line was opened.
    pub months_open: u32,
    /// HECM: what may still be disbursed in the first 12 months.
    pub first_year_room: f64,
}

impl EquityLine {
    /// True while a line is open.
    pub fn is_open(&self) -> bool {
        !matches!(self.kind, LineKind::Closed)
    }

    /// What the household owes on the line as a liability against a home worth
    /// `value`: the balance, but never more than the home for a non-recourse HECM.
    pub fn liability(&self, value: f64) -> f64 {
        match self.kind {
            LineKind::Hecm => self.balance.min(value.max(0.0)),
            _ => self.balance,
        }
    }
}

/// Compiled HELOC terms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HelocTerms {
    /// Largest (mortgage + line) / value.
    pub max_cltv: f64,
    /// Spread over the bill yield.
    pub spread: f64,
    /// Months of the draw (interest-only) period.
    pub draw_months: u32,
    /// Months of the repayment period after it.
    pub repay_months: u32,
    /// Probability that lines freeze when a downturn begins.
    pub freeze_probability: f64,
}

/// One month of a HELOC: what is paid.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct HelocMonth {
    /// Interest charged and paid.
    pub interest: f64,
    /// Principal repaid (zero during the draw period).
    pub principal: f64,
}

impl HelocTerms {
    /// The line's annual rate given the bill yield.
    pub fn rate(&self, tbill: f64) -> f64 {
        (tbill + self.spread).max(0.0)
    }

    /// Credit limit of a line opened today on a home worth `value` with `mortgage`
    /// owed on it.
    pub fn credit_limit(&self, value: f64, mortgage: f64) -> f64 {
        (self.max_cltv * value - mortgage).max(0.0)
    }

    /// What can be drawn now: zero after the draw period or while lines are frozen;
    /// otherwise the limit less the balance, cut further if the home has lost value
    /// (lenders reduce lines to keep within the combined loan-to-value).
    pub fn available(&self, line: &EquityLine, value: f64, mortgage: f64, frozen: bool) -> f64 {
        if frozen || line.months_open >= self.draw_months {
            return 0.0;
        }
        let limit = if line.is_open() {
            line.limit
        } else {
            self.credit_limit(value, mortgage)
        };
        (limit.min(self.max_cltv * value - mortgage) - line.balance).max(0.0)
    }

    /// Advances an open line by one month at bill yield `tbill`: interest only during
    /// the draw period, then the level payment over what is left of the repayment
    /// period. The line closes once the term is over and nothing is owed.
    pub fn month(&self, line: &mut EquityLine, tbill: f64) -> HelocMonth {
        if line.kind != LineKind::Heloc {
            return HelocMonth::default();
        }
        let rate = self.rate(tbill);
        let interest = line.balance * rate / 12.0;
        let mut principal = 0.0;
        if line.months_open >= self.draw_months && line.balance > 0.0 {
            let left = (self.draw_months + self.repay_months).saturating_sub(line.months_open);
            principal = if left <= 1 {
                line.balance
            } else {
                (amortized_payment(line.balance, rate, left) - interest).clamp(0.0, line.balance)
            };
        }
        line.balance -= principal;
        if line.balance < 0.005 {
            line.balance = 0.0;
        }
        line.months_open += 1;
        if line.months_open >= self.draw_months + self.repay_months && line.balance == 0.0 {
            *line = EquityLine::default();
        }
        HelocMonth {
            interest,
            principal,
        }
    }
}

/// HUD's principal limit factor at `age` (youngest borrower, whole years are not
/// required) and `expected_rate`, interpolated linearly on the published grid. Zero
/// below 62; ages above 92 use the 92 row. Rates below 3% use the 3% column; above 8%
/// the last slope is extended (factors keep falling), with a floor of zero.
pub fn principal_limit_factor(age: f64, expected_rate: f64) -> f64 {
    // Also catches a NaN age.
    if age.is_nan() || age < HECM_MIN_AGE {
        return 0.0;
    }
    let rate = expected_rate.max(HECM_EXPECTED_RATE_FLOOR);
    let row = |r: &[f64; 6]| -> f64 {
        let last = PLF_RATES.len() - 1;
        let mut j = 0;
        while j + 1 < last && rate > PLF_RATES[j + 1] {
            j += 1;
        }
        let w = (rate - PLF_RATES[j]) / (PLF_RATES[j + 1] - PLF_RATES[j]);
        r[j] + w * (r[j + 1] - r[j])
    };
    let age = age.min(PLF_AGES[PLF_AGES.len() - 1]);
    let mut i = 0;
    while i + 2 < PLF_AGES.len() && age > PLF_AGES[i + 1] {
        i += 1;
    }
    let w = ((age - PLF_AGES[i]) / (PLF_AGES[i + 1] - PLF_AGES[i])).clamp(0.0, 1.0);
    let a = row(&PLF_TABLE[i]);
    let b = row(&PLF_TABLE[i + 1]);
    (a + w * (b - a)).max(0.0)
}

/// The largest origination fee a HECM lender may charge on a maximum claim amount
/// (24 CFR 206.31).
pub fn hecm_origination_fee(max_claim: f64) -> f64 {
    let tiered = 0.02 * max_claim.min(HECM_ORIGINATION_TIER)
        + 0.01 * (max_claim - HECM_ORIGINATION_TIER).max(0.0);
    tiered.clamp(HECM_ORIGINATION_MIN, HECM_ORIGINATION_MAX)
}

/// What opening a HECM does.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct HecmOpening {
    /// Maximum claim amount.
    pub max_claim: f64,
    /// Principal limit.
    pub principal_limit: f64,
    /// Insurance premium, origination fee and other costs, all financed.
    pub costs: f64,
    /// The existing mortgage, repaid out of the principal limit.
    pub mortgage_payoff: f64,
    /// The line: balance and unused credit after closing.
    pub line: EquityLine,
}

/// Opens a HECM, or returns `None` if the youngest borrower is under 62 or the
/// principal limit does not cover the costs and the existing mortgage.
///
/// `limit_today` is the FHA maximum claim amount scaled to the month (nominal);
/// `other_costs` the other closing costs in nominal dollars.
pub fn open_hecm(
    age: f64,
    value: f64,
    mortgage: f64,
    limit_today: f64,
    long_yield: f64,
    margin: f64,
    other_costs: f64,
) -> Option<HecmOpening> {
    if age < HECM_MIN_AGE || value <= 0.0 {
        return None;
    }
    let max_claim = value.min(limit_today);
    let plf = principal_limit_factor(age, long_yield + margin);
    let principal_limit = plf * max_claim;
    let costs = HECM_UPFRONT_MIP * max_claim + hecm_origination_fee(max_claim) + other_costs;
    let mandatory = costs + mortgage.max(0.0);
    if principal_limit <= mandatory {
        return None;
    }
    let first_year_cap = (HECM_FIRST_YEAR_SHARE * principal_limit)
        .max(mandatory + HECM_FIRST_YEAR_EXTRA * principal_limit)
        .min(principal_limit);
    Some(HecmOpening {
        max_claim,
        principal_limit,
        costs,
        mortgage_payoff: mortgage.max(0.0),
        line: EquityLine {
            kind: LineKind::Hecm,
            balance: mandatory,
            limit: principal_limit - mandatory,
            months_open: 0,
            first_year_room: (first_year_cap - mandatory).max(0.0),
        },
    })
}

/// Annual rate at which a HECM balance and its unused line grow.
pub fn hecm_growth_rate(tbill: f64, margin: f64) -> f64 {
    (tbill + margin).max(0.0) + HECM_ANNUAL_MIP
}

/// One month of a HECM: the balance and the unused line grow; returns the interest
/// and insurance added to the balance.
pub fn hecm_month(line: &mut EquityLine, tbill: f64, margin: f64) -> f64 {
    if line.kind != LineKind::Hecm {
        return 0.0;
    }
    let g = hecm_growth_rate(tbill, margin) / 12.0;
    let accrued = line.balance * g;
    line.balance += accrued;
    line.limit *= 1.0 + g;
    line.months_open += 1;
    accrued
}

/// What can be drawn from a HECM line now.
pub fn hecm_available(line: &EquityLine) -> f64 {
    if line.kind != LineKind::Hecm {
        return 0.0;
    }
    if line.months_open < 12 {
        line.limit.min(line.first_year_room).max(0.0)
    } else {
        line.limit.max(0.0)
    }
}

/// Draws `amount` from a line (the caller has checked what is available).
pub fn draw(line: &mut EquityLine, amount: f64) {
    line.balance += amount;
    if line.kind == LineKind::Hecm {
        line.limit -= amount;
        if line.months_open < 12 {
            line.first_year_room -= amount;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms() -> HelocTerms {
        HelocTerms {
            max_cltv: 0.8,
            spread: 0.035,
            draw_months: 120,
            repay_months: 240,
            freeze_probability: 0.3,
        }
    }

    #[test]
    fn heloc_limit_and_availability() {
        let h = terms();
        // 0.8 x 500,000 - 250,000 = 150,000.
        assert_eq!(h.credit_limit(500_000.0, 250_000.0), 150_000.0);
        let mut line = EquityLine {
            kind: LineKind::Heloc,
            limit: 150_000.0,
            ..EquityLine::default()
        };
        draw(&mut line, 40_000.0);
        assert_eq!(h.available(&line, 500_000.0, 250_000.0, false), 110_000.0);
        // The home falls to 400,000: the combined limit is 320,000 - 250,000 = 70,000,
        // of which 40,000 is drawn.
        assert_eq!(h.available(&line, 400_000.0, 250_000.0, false), 30_000.0);
        assert_eq!(h.available(&line, 500_000.0, 250_000.0, true), 0.0);
        line.months_open = 120;
        assert_eq!(h.available(&line, 500_000.0, 250_000.0, false), 0.0);
        // Under water: nothing.
        assert_eq!(h.credit_limit(300_000.0, 250_000.0), 0.0);
    }

    #[test]
    fn heloc_is_interest_only_then_amortises() {
        let h = terms();
        let mut line = EquityLine {
            kind: LineKind::Heloc,
            balance: 60_000.0,
            limit: 100_000.0,
            months_open: 0,
            first_year_room: 0.0,
        };
        // Bills at 3.5%: rate 7%, interest 60,000 x 0.07 / 12 = 350.
        let m = h.month(&mut line, 0.035);
        assert!((m.interest - 350.0).abs() < 1e-9);
        assert_eq!(m.principal, 0.0);
        assert_eq!(line.balance, 60_000.0);
        line.months_open = 120;
        // Level payment over 240 months at 7%: 465.18; principal = 465.18 - 350.
        let m = h.month(&mut line, 0.035);
        let payment = amortized_payment(60_000.0, 0.07, 240);
        assert!((payment - 465.18).abs() < 0.01, "{payment}");
        assert!((m.principal - (payment - 350.0)).abs() < 1e-9);
        // Repaid exactly by the end of the term at a constant rate.
        for _ in 0..239 {
            h.month(&mut line, 0.035);
        }
        assert_eq!(line, EquityLine::default());
    }

    #[test]
    fn plf_reproduces_the_published_grid_and_interpolates() {
        for (i, age) in PLF_AGES.iter().enumerate() {
            for (j, rate) in PLF_RATES.iter().enumerate() {
                assert!((principal_limit_factor(*age, *rate) - PLF_TABLE[i][j]).abs() < 1e-12);
            }
        }
        // Halfway between 62 and 72 at 5.5%: mean of 0.410, 0.357, 0.467, 0.416.
        let mid = (0.410 + 0.357 + 0.467 + 0.416) / 4.0;
        assert!((principal_limit_factor(67.0, 0.055) - mid).abs() < 1e-12);
        assert_eq!(principal_limit_factor(61.9, 0.05), 0.0);
        // Floor at 3%, ages above 92 as 92.
        assert_eq!(principal_limit_factor(62.0, 0.01), 0.524);
        assert_eq!(principal_limit_factor(99.0, 0.03), 0.750);
        // Above 8% the factor keeps falling: 0.272 - 0.040 at 9%.
        assert!((principal_limit_factor(62.0, 0.09) - 0.232).abs() < 1e-12);
        // Older and lower rates borrow more.
        assert!(principal_limit_factor(75.0, 0.06) > principal_limit_factor(65.0, 0.06));
        assert!(principal_limit_factor(75.0, 0.05) > principal_limit_factor(75.0, 0.06));
    }

    #[test]
    fn origination_fee_follows_the_regulation() {
        // HUD's examples (ML 2008-34): 100,000 -> 2,500 (the floor); 417,000 -> 6,000.
        assert_eq!(hecm_origination_fee(100_000.0), 2_500.0);
        assert_eq!(hecm_origination_fee(250_000.0), 4_500.0);
        assert_eq!(hecm_origination_fee(417_000.0), 6_000.0);
    }

    #[test]
    fn hecm_opening_costs_and_growth() {
        // Age 72, home 400,000, no mortgage, 10-year yield 3% + 2% margin = 5%:
        // PLF 0.467, principal limit 186,800. Costs: 8,000 MIP + 6,000 origination +
        // 3,000 other = 17,000. Line 169,800; first year at most 60% = 112,080.
        let o = open_hecm(72.0, 400_000.0, 0.0, 1_249_125.0, 0.03, 0.02, 3_000.0).unwrap();
        assert!((o.principal_limit - 186_800.0).abs() < 1e-6);
        assert!((o.costs - 17_000.0).abs() < 1e-9);
        assert!((o.line.balance - 17_000.0).abs() < 1e-9);
        assert!((o.line.limit - 169_800.0).abs() < 1e-6);
        assert!((hecm_available(&o.line) - (112_080.0 - 17_000.0)).abs() < 1e-6);
        let mut line = o.line;
        // Bills 3%, margin 2%, insurance 0.5%: 5.5% a year.
        let accrued = hecm_month(&mut line, 0.03, 0.02);
        assert!((accrued - 17_000.0 * 0.055 / 12.0).abs() < 1e-9);
        assert!((line.limit - 169_800.0 * (1.0 + 0.055 / 12.0)).abs() < 1e-6);
        // Non-recourse: never owes more than the home.
        line.balance = 500_000.0;
        assert_eq!(line.liability(400_000.0), 400_000.0);
        // A large mortgage the limit cannot cover: no loan. Too young: no loan.
        assert!(open_hecm(72.0, 400_000.0, 180_000.0, 1e7, 0.03, 0.02, 3_000.0).is_none());
        assert!(open_hecm(61.0, 400_000.0, 0.0, 1e7, 0.03, 0.02, 3_000.0).is_none());
        // The claim amount is capped by the FHA limit.
        let big = open_hecm(72.0, 3_000_000.0, 0.0, 1_249_125.0, 0.03, 0.02, 0.0).unwrap();
        assert_eq!(big.max_claim, 1_249_125.0);
    }
}
