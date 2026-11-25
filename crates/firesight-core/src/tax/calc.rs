//! The annual return.
//!
//! [`work`] computes every intermediate figure of one household's return into a plain
//! [`Work`] value on the stack; [`compute`] reduces it to a [`TaxResult`] and
//! `explain` prints it. Nothing here allocates.
//!
//! Order of the calculation (US current law):
//!
//! 1. Payroll: employee FICA per person on wages less payroll HSA contributions, up to
//!    the wage base; self-employment tax on 92.35% of net profit, with the OASDI part
//!    limited to the wage base left after wages; additional Medicare tax on the
//!    household's combined earnings above the (frozen) threshold.
//! 2. Income: wages less pre-tax deferrals, self-employment profit, interest,
//!    dividends, capital gains after netting short against long and applying the loss
//!    limit, retirement distributions, conversions, pensions, rental income after the
//!    passive-loss limit, less half of the self-employment tax.
//! 3. Social Security: the two-tier provisional-income formula.
//! 4. State income tax (needed for the SALT deduction).
//! 5. Deductions: standard (plus the age-65 addition and the non-itemiser gift
//!    deduction) against itemised (SALT within the cap and its phase-down, mortgage
//!    interest, gifts above the floor, less the 2/37 limitation); then the senior
//!    deduction and the business-income deduction.
//! 6. Tax: ordinary brackets on ordinary taxable income; long-term gains and
//!    qualified dividends stacked on top through the 0/15/20% thresholds; NIIT.
//!    When `TaxInputs.amt_applies` is set, the alternative minimum tax and the
//!    minimum tax credit (after the credits of step 7, which limit the credit used).
//! 7. Credits: child and other-dependant credit with phase-out and the refundable
//!    limit. Penalties on early and non-medical HSA distributions.
//! 8. ACA premium credit and the IRMAA tier, from their own MAGI definitions.
//!
//! Simplifications, all deliberate:
//!
//! * AMT is computed when `TaxInputs.amt_applies` is set (households with
//!   private-company equity): the exemption and its phase-out, the 26% and 28% rates,
//!   gains at their own rates, the incentive-option adjustment and the minimum tax
//!   credit. State AMT is not modelled. For every other household no AMT is
//!   computed, although a large long-term gain on moderate wages can owe some under
//!   2026 law. Inside the AMT the standard deduction or the state and local tax
//!   deduction is added back and the senior and business-income deductions are
//!   allowed; no other preference or adjustment is modelled.
//! * `marginal_ordinary_rate` and [`room_to_bracket_top`] describe the regular
//!   schedule. In a year when the AMT binds, the Roth-conversion sizer therefore
//!   fills a bracket that is not the binding one. [`effective_marginal_rate`] is a
//!   finite difference on `total` and stays right.
//! * Gain excluded as qualified small business stock is left out of federal income
//!   and taxed by every state as ordinary income (right for the states that do not
//!   follow section 1202, too high elsewhere).
//! * No tax-exempt interest (so IRMAA MAGI equals AGI). No kiddie tax,
//!   education, saver's, child-care, earned-income or foreign tax credits.
//! * Married filing separately is not a status.
//! * The business-income (QBI) deduction treats every self-employed person as a sole
//!   proprietor without employees or depreciable property: 20% of profit below the
//!   threshold, phased out linearly to nothing across the phase-in range.
//! * Rental losses follow the $25,000 allowance of IRC 469(i); the disallowed part is
//!   reported for the caller to carry forward. No real-estate-professional status.
//! * Collectibles (28%) and unrecaptured section 1250 gain (25%) are taxed as ordinary
//!   long-term gains.
//! * Additional standard deduction for blindness is not modelled.
//! * ACA: household income below the statutory 100% floor is priced from the first
//!   band of the table instead of being refused a credit, standing in for Medicaid
//!   (or for the small income top-up anyone in that position would make). The
//!   poverty-line ratio is reported so a caller can treat it differently.
//! * States: one schedule per status (head of household uses single), the state's
//!   standard deduction or, where it has one, mortgage interest plus property tax
//!   plus gifts if larger; personal and dependant exemptions or credits; Social
//!   Security and retirement-income rules as far as the data structures them; gains
//!   exclusions, rate caps, surtaxes and Washington's excise. High-income recapture
//!   rules (NY, CT), exemption phase-outs other than South Carolina's deduction and
//!   Utah's credit, and income limits on retirement exclusions are not modelled.
//!   Local income tax is the statewide average share of income, added flat.
//! * Flat and foreign modes charge one rate on income net of pre-tax deferrals,
//!   including all Social Security and net gains, and nothing else (no payroll tax,
//!   penalties, premium credit or IRMAA).

use super::{FilingStatus, StateYear, TaxInputs, TaxResult, TaxYearParams};
use crate::model::TaxKind;
use crate::MAX_PEOPLE;

/// IRC 469(i): rental real-estate losses up to this amount offset other income.
/// Frozen in nominal dollars since 1986; not in the data files.
const PASSIVE_LOSS_ALLOWANCE: f64 = 25_000.0;
/// IRC 469(i)(3): the allowance shrinks by 50 cents per dollar of MAGI above $100,000.
const PASSIVE_LOSS_PHASEOUT_START: f64 = 100_000.0;
const PASSIVE_LOSS_PHASEOUT_RATE: f64 = 0.5;
/// 26 USC 86: provisional income counts half of benefits, and the first tier taxes
/// half of the excess (the 0.5 in `social_security.benefit_taxation.formula`).
const SS_HALF: f64 = 0.5;
/// The child credit phase-out counts each $1,000 "or fraction thereof" of excess
/// income (`credits.child_tax_credit.note`).
const CHILD_CREDIT_PHASEOUT_STEP: f64 = 1_000.0;
/// Tolerance, in dollars, when comparing income with the ACA cliff, so that income
/// planned to land exactly on the limit is not pushed over by rounding.
const CLIFF_TOLERANCE: f64 = 0.005;
/// Extra income used to measure the effective marginal rate by finite difference.
const MARGINAL_PROBE: f64 = 100.0;
/// Accuracy, in dollars, of [`room_to_bracket_top`].
const ROOM_TOLERANCE: f64 = 0.005;
/// Iteration cap of the bracket-room search (bisection alone needs fewer).
const ROOM_MAX_ITERATIONS: u32 = 60;

/// Which system `compute` applies.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(super) enum Mode {
    /// US federal and state law.
    #[default]
    Us,
    /// One flat rate on all income.
    Flat(f64),
    /// One flat rate, paid to a foreign country.
    Foreign(f64),
    /// No tax.
    None,
}

#[inline(always)]
pub(super) fn mode(p: &TaxYearParams) -> Mode {
    match (p.kind, p.foreign_tax_rate) {
        (TaxKind::None, _) => Mode::None,
        (TaxKind::Flat { rate }, _) => Mode::Flat(rate),
        (TaxKind::UsCurrentLaw, Some(rate)) => Mode::Foreign(rate),
        (TaxKind::UsCurrentLaw, None) => Mode::Us,
    }
}

/// The state part of the working.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct StateWork {
    pub ss_taxed: f64,
    pub retirement_exclusion: f64,
    pub gains_exclusion: f64,
    pub agi: f64,
    pub deduction: f64,
    pub itemized: bool,
    pub exemptions: f64,
    pub taxable: f64,
    pub tax_before_credits: f64,
    pub credits: f64,
    pub income_tax: f64,
    pub surtax: f64,
    pub excise: f64,
    pub local: f64,
    /// State payroll tax on wages (California SDI), with `state_detail: full`.
    pub payroll: f64,
    /// Tax added back by a high-income recapture of the lower brackets (New York).
    pub recapture: f64,
    pub total: f64,
}

/// Every intermediate figure of the return.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Work {
    pub mode: Mode,
    // Payroll.
    pub fica_wages: [f64; MAX_PEOPLE],
    pub fica: [f64; MAX_PEOPLE],
    pub se_base: [f64; MAX_PEOPLE],
    pub se_tax: [f64; MAX_PEOPLE],
    pub additional_medicare: f64,
    pub half_se: f64,
    // Income.
    pub wages: f64,
    pub self_employment: f64,
    pub deferrals: f64,
    pub earned_income: f64,
    pub investment_income: f64,
    pub ordinary_gain: f64,
    pub pref_gain: f64,
    pub loss_deduction: f64,
    pub loss_carry: f64,
    pub retirement_income: f64,
    pub other_income: f64,
    pub rental_allowed: f64,
    pub rental_suspended: f64,
    pub income_ex_ss: f64,
    pub ss_total: f64,
    pub provisional: f64,
    pub taxable_ss: f64,
    pub agi: f64,
    pub magi_aca: f64,
    pub pref_income: f64,
    pub nii: f64,
    // Deductions.
    pub seniors: u32,
    pub standard: f64,
    pub salt_paid: f64,
    pub salt_cap: f64,
    pub salt: f64,
    pub charity_itemized: f64,
    pub itemized_raw: f64,
    pub itemized_limit: f64,
    pub itemized_net: f64,
    pub deduction: f64,
    pub itemized: bool,
    pub senior: f64,
    pub qbi: f64,
    // Tax.
    pub taxable_income: f64,
    /// Taxable income before the floor at zero (negative when the deductions exceed
    /// income): line 1 of Form 6251, the start of AMT income.
    pub taxable_unfloored: f64,
    pub ordinary_taxable: f64,
    pub pref_taxable: f64,
    pub tax_ordinary: f64,
    pub tax_pref: f64,
    pub niit: f64,
    pub credit_gross: f64,
    pub credit_phaseout: f64,
    pub credit_nonrefundable: f64,
    pub credit_refundable: f64,
    pub penalty_early: f64,
    pub penalty_hsa: f64,
    pub state: StateWork,
    // Health subsidies.
    pub aca_fpl: f64,
    pub aca_ratio: f64,
    pub aca_applicable: f64,
    pub aca_credit: f64,
    pub irmaa_tier: usize,
    pub irmaa_surcharge: f64,
    // Flat and foreign modes.
    pub flat_base: f64,
    pub flat_tax: f64,
    pub total: f64,
    pub marginal: f64,
    // Alternative minimum tax (all zero unless `TaxInputs.amt_applies`, except
    // `amt_credit_out`, which then passes the credit carried in straight through).
    pub amti: f64,
    pub amt_exemption: f64,
    pub tmt: f64,
    pub amt: f64,
    pub amt_credit_generated: f64,
    pub amt_credit_used: f64,
    pub amt_credit_out: f64,
    /// Incentive-option income inside `wages` that bears no payroll tax.
    pub wages_no_fica: f64,
}

/// Payroll taxes (step 1).
#[inline(always)]
fn payroll(p: &TaxYearParams, x: &TaxInputs, w: &mut Work) {
    let f = &p.federal;
    let s = x.filing_status.index();
    let mut medicare_earnings = 0.0;
    for i in 0..MAX_PEOPLE {
        let fica_wages = (x.wages[i].max(0.0) - x.hsa_contributions[i].max(0.0)).max(0.0);
        w.fica_wages[i] = fica_wages;
        w.fica[i] = f.oasdi_rate * fica_wages.min(f.oasdi_wage_base) + f.medicare_rate * fica_wages;

        let net = x.self_employment[i] * f.se_net_earnings_factor;
        let base = if net >= f.se_minimum_earnings && net > 0.0 {
            net
        } else {
            0.0
        };
        let oasdi_room = (f.oasdi_wage_base - fica_wages).max(0.0);
        w.se_base[i] = base;
        w.se_tax[i] = f.se_oasdi_rate * base.min(oasdi_room) + f.se_medicare_rate * base;
        w.half_se += f.se_deduction_fraction * w.se_tax[i];
        medicare_earnings += fica_wages + base;
    }
    // Additional Medicare tax on the household's combined earnings, shared out in
    // proportion to each person's wages and self-employment earnings.
    let excess = (medicare_earnings - f.additional_medicare_threshold[s]).max(0.0);
    w.additional_medicare = f.additional_medicare_rate * excess;
    if w.additional_medicare > 0.0 {
        for i in 0..MAX_PEOPLE {
            w.fica[i] += w.additional_medicare * w.fica_wages[i] / medicare_earnings;
            w.se_tax[i] += w.additional_medicare * w.se_base[i] / medicare_earnings;
        }
    }
}

/// Income before Social Security (step 2). `limit_rental` applies the passive-loss
/// rule; the flat modes take rental income as it comes.
#[inline(always)]
fn income(p: &TaxYearParams, x: &TaxInputs, w: &mut Work, limit_rental: bool) {
    let f = &p.federal;
    let mut business = 0.0;
    for i in 0..MAX_PEOPLE {
        // Income from a disqualifying sale of incentive-option shares is pay for
        // income tax but not for payroll tax, so `payroll` never sees it.
        let no_fica = x.wages_no_fica[i].max(0.0);
        let wages = x.wages[i].max(0.0) + no_fica;
        w.wages_no_fica += no_fica;
        let profit = x.self_employment[i];
        // Deferrals cannot exceed the pay they come out of.
        let deferral = (x.pretax_deferrals[i].max(0.0) + x.hsa_contributions[i].max(0.0))
            .min(wages + profit.max(0.0));
        w.wages += wages;
        w.self_employment += profit;
        w.deferrals += deferral;
        // Business income for the QBI deduction: profit less the part of the deferral
        // that wages could not absorb.
        business += (profit - (deferral - wages).max(0.0)).max(0.0);
    }
    // Provisional: turned into the deduction in `deductions`.
    w.qbi = business;
    // Earned income for the refundable child credit is taxable earned income: after
    // pre-tax deferrals and payroll HSA money (Schedule 8812).
    w.earned_income = (w.wages + w.self_employment - w.deferrals - w.half_se).max(0.0);

    // Capital gains: net short against long; a net loss offsets ordinary income up
    // to the limit and the rest carries forward.
    let short = x.short_term_gains;
    let long = x.long_term_gains;
    let net = short + long;
    if net < 0.0 {
        w.loss_deduction = (-net).min(f.capital_loss_limit);
        w.loss_carry = -net - w.loss_deduction;
    } else if long >= 0.0 && short >= 0.0 {
        w.pref_gain = long;
        w.ordinary_gain = short;
    } else if long < 0.0 {
        w.ordinary_gain = net;
    } else {
        w.pref_gain = net;
    }
    let dividends = x.ordinary_dividends.max(0.0) + x.qualified_dividends.max(0.0);
    w.investment_income = x.interest.max(0.0) + dividends;
    w.pref_income = w.pref_gain + x.qualified_dividends.max(0.0);
    w.retirement_income =
        x.traditional_distributions.max(0.0) + x.roth_conversions.max(0.0) + x.pensions.max(0.0);
    // Recaptured depreciation is ordinary income (its rate cap is applied in
    // `federal_tax`); above-the-line adjustments and passive losses released by a
    // sale are ordinary deductions.
    w.other_income =
        x.other_ordinary + x.hsa_nonqualified.max(0.0) + x.depreciation_recapture.max(0.0)
            - x.adjustments.max(0.0)
            - x.passive_loss_released.max(0.0);

    let before_rental = w.wages + w.self_employment - w.deferrals
        + w.investment_income
        + w.ordinary_gain
        + w.pref_gain
        - w.loss_deduction
        + w.retirement_income
        + w.other_income;
    w.rental_allowed = x.rental_net;
    if limit_rental && x.rental_net < 0.0 {
        let allowance = (PASSIVE_LOSS_ALLOWANCE
            - PASSIVE_LOSS_PHASEOUT_RATE * (before_rental - PASSIVE_LOSS_PHASEOUT_START).max(0.0))
        .max(0.0);
        w.rental_allowed = x.rental_net.max(-allowance);
        w.rental_suspended = w.rental_allowed - x.rental_net;
    }
    w.income_ex_ss = before_rental + w.rental_allowed - w.half_se;
    w.nii = (w.investment_income + w.ordinary_gain + w.pref_gain - w.loss_deduction
        + w.rental_allowed)
        .max(0.0);
    for benefit in &x.social_security {
        w.ss_total += benefit.max(0.0);
    }
}

/// Taxable Social Security by the two-tier provisional-income formula (step 3).
///
/// Tier 1 is the smallest of 50% of benefits, 50% of the gap between the two
/// thresholds, and 50% of provisional income over the base threshold. Tier 2 is 85%
/// of provisional income over the upper threshold. The taxable amount is their sum,
/// capped at 85% of benefits.
#[inline(always)]
fn social_security(p: &TaxYearParams, x: &TaxInputs, w: &mut Work) {
    let f = &p.federal;
    let s = x.filing_status.index();
    w.provisional = w.income_ex_ss + SS_HALF * w.ss_total;
    if w.ss_total > 0.0 {
        let base = f.ss_taxation_base[s];
        let upper = f.ss_taxation_upper[s];
        let tier1 = (SS_HALF * w.ss_total)
            .min(SS_HALF * (upper - base))
            .min(SS_HALF * (w.provisional - base).max(0.0));
        let tier2 = f.ss_max_taxable_fraction * (w.provisional - upper).max(0.0);
        w.taxable_ss = (f.ss_max_taxable_fraction * w.ss_total).min(tier1 + tier2);
    }
    w.agi = w.income_ex_ss + w.taxable_ss;
    w.magi_aca = w.agi + (w.ss_total - w.taxable_ss);
}

/// The part of federally taxable Social Security the state taxes.
#[inline(always)]
fn state_taxable_ss(st: &StateYear, x: &TaxInputs, w: &Work) -> f64 {
    if st.social_security_taxed <= 0.0 || w.taxable_ss <= 0.0 {
        return 0.0;
    }
    let s = x.filing_status.index();
    let threshold = st.ss_exempt_below_agi[s];
    let end = st.ss_phase_out_end_agi[s];
    // Share of benefits exempt on income grounds.
    let exempt = if threshold <= 0.0 {
        0.0
    } else if w.agi <= threshold {
        1.0
    } else if end > threshold {
        1.0 - ((w.agi - threshold) / (end - threshold)).min(1.0)
    } else if st.ss_phase_out_step > 0.0 {
        let steps = ((w.agi - threshold) / st.ss_phase_out_step).ceil();
        (1.0 - st.ss_phase_out_fraction_per_step * steps).max(0.0)
    } else {
        0.0
    };
    let mut taxed = 0.0;
    for i in 0..MAX_PEOPLE {
        let benefit = x.social_security[i].max(0.0);
        if benefit <= 0.0 {
            continue;
        }
        let age = x.ages[i];
        if st.ss_full_exemption_age > 0.0 && age >= st.ss_full_exemption_age {
            continue;
        }
        let person_exempt = if st.ss_exemption_requires_fra && age < st.full_retirement_age {
            0.0
        } else {
            exempt
        };
        taxed += w.taxable_ss * (benefit / w.ss_total) * (1.0 - person_exempt);
    }
    (taxed * st.social_security_taxed).min(st.ss_max_taxable_fraction_of_benefits * w.ss_total)
}

/// State and local income tax (step 4).
#[inline(always)]
fn state_tax(p: &TaxYearParams, x: &TaxInputs, w: &Work) -> StateWork {
    let st = &p.state;
    let s = x.filing_status.index();
    let mut o = StateWork::default();
    if st.capital_gains_excise.len > 0 {
        o.excise = st
            .capital_gains_excise
            .tax((w.pref_gain - st.capital_gains_excise_deduction).max(0.0));
    }
    let schedule = &st.brackets[s];
    if schedule.len > 0 {
        o.ss_taxed = state_taxable_ss(st, x, w);

        // A conversion to a Roth account is a distribution from the traditional
        // one: states that exempt retirement distributions exempt it too.
        let retirement = x.traditional_distributions.max(0.0)
            + x.roth_conversions.max(0.0)
            + x.pensions.max(0.0);
        let mut eligible = 0.0;
        for &age in &x.ages {
            if age > 0.0 && age >= st.retirement_exclusion_age {
                eligible += 1.0;
            }
        }
        if st.retirement_exclusion > 0.0 && eligible > 0.0 {
            o.retirement_exclusion = retirement.min(st.retirement_exclusion * eligible);
        }

        o.gains_exclusion = (w.pref_gain * st.capital_gains_exclusion_fraction)
            .max(st.capital_gains_exclusion_amount.min(w.pref_gain));
        o.agi = w.agi - w.taxable_ss + o.ss_taxed - o.retirement_exclusion - o.gains_exclusion;
        if st.taxes_deferrals {
            for d in &x.pretax_deferrals {
                o.agi += d.max(0.0);
            }
        }
        // The section 1202 exclusion is federal only: the excluded gain is state
        // income, taxed as ordinary income without the state's own gain preferences.
        o.agi += x.qsbs_excluded_gain.max(0.0);

        let mut standard = st.standard_deduction[s];
        let (start, end) = (
            st.standard_deduction_phaseout_start[s],
            st.standard_deduction_phaseout_end[s],
        );
        if end > start {
            standard *= (1.0 - (w.agi - start) / (end - start)).clamp(0.0, 1.0);
        }
        let itemized = if st.allows_itemizing {
            x.mortgage_interest.max(0.0) + x.property_tax.max(0.0) + x.charitable.max(0.0)
        } else {
            0.0
        };
        o.itemized = itemized > standard;
        o.deduction = standard.max(itemized);
        let dependents = f64::from(x.children_under_17 + x.other_dependents);
        o.exemptions = st.personal_exemption[s] + st.dependent_exemption * dependents;
        o.taxable = (o.agi - o.deduction - o.exemptions).max(0.0);

        let mut tax = schedule.tax(o.taxable);
        if st.capital_gains_rate >= 0.0 && w.pref_gain > 0.0 {
            // Gains sit on top of other income and pay no more than the capped rate.
            let gains = (w.pref_gain - o.gains_exclusion).max(0.0).min(o.taxable);
            let capped = schedule.tax(o.taxable - gains) + st.capital_gains_rate * gains;
            tax = tax.min(capped);
        }
        if st.recapture_agi_start > 0.0 {
            o.recapture = recapture_extra(st, s, o.agi, o.taxable);
            tax += o.recapture;
        }
        o.tax_before_credits = tax;
        let standard_credit = (st.standard_credit[s]
            - st.standard_credit_phaseout_rate
                * (o.taxable - st.standard_credit_phaseout_start[s]).max(0.0))
        .max(0.0);
        let ss_credit = if st.ss_credit_rate > 0.0 && w.taxable_ss > 0.0 {
            (st.ss_credit_rate * w.taxable_ss
                - st.ss_credit_phase_out_rate * (w.agi - st.ss_credit_phase_out_start[s]).max(0.0))
            .max(0.0)
        } else {
            0.0
        };
        o.credits = (st.personal_credit[s]
            + st.dependent_credit * dependents
            + standard_credit
            + ss_credit)
            .min(tax);
        o.income_tax = tax - o.credits;

        if st.capital_gains_surtax_rate > 0.0 && w.agi > st.capital_gains_surtax_agi_threshold {
            o.surtax += st.capital_gains_surtax_rate * (w.pref_gain + w.ordinary_gain);
        }
        if st.investment_surtax_rate > 0.0 {
            o.surtax +=
                st.investment_surtax_rate * (w.nii - st.investment_surtax_threshold).max(0.0);
        }
        o.local = if st.local_brackets[s].len > 0 {
            st.local_brackets[s].tax(o.taxable)
        } else if st.local_earned_only {
            let mut earned = 0.0;
            for p in 0..MAX_PEOPLE {
                earned += x.wages[p].max(0.0)
                    + x.wages_no_fica[p].max(0.0)
                    + x.self_employment[p].max(0.0);
            }
            st.local_rate * earned
        } else {
            st.local_rate * o.agi.max(0.0)
        };
    }
    if st.sdi_rate > 0.0 {
        let mut wages = 0.0;
        for w in &x.wages {
            wages += w.max(0.0);
        }
        o.payroll = st.sdi_rate * wages;
    }
    o.total = o.income_tax + o.surtax + o.excise + o.local + o.payroll;
    o
}

/// Tax added by a state's recapture of the benefit of its lower brackets for high
/// earners (New York, Tax Law 601(d)-(e)), on top of the schedule's tax on `taxable`.
///
/// Above `recapture_agi_start` of state AGI the benefit is taken back tier by tier:
///
/// * Taxable income up to the top of bracket `j0` (`recapture_first_bracket`): the
///   tax is moved toward a flat rate of that bracket on all of it, by the fraction
///   `(agi - start) / width` (rounded to four places, as in the state's worksheets),
///   capped at one.
/// * Taxable income in a higher bracket `m`: the benefit up to the top of bracket
///   `m - 1` is already fully recaptured (`base`, the flat tax at that rate on
///   `lower[m]` less the schedule's tax there), and the further benefit up to the top
///   of bracket `m` (`increment`) comes in over `width` of AGI above `lower[m]`.
/// * Above `recapture_agi_top` of AGI every dollar pays the top rate.
///
/// Zero when the state has no such rule, the profile has not asked for it, or AGI
/// is at or below the start.
pub(super) fn recapture_extra(st: &StateYear, s: usize, agi: f64, taxable: f64) -> f64 {
    let b = &st.brackets[s];
    if st.recapture_agi_start <= 0.0 || b.len < 2 || agi <= st.recapture_agi_start || taxable <= 0.0
    {
        return 0.0;
    }
    let table = b.tax(taxable);
    if agi > st.recapture_agi_top {
        return b.rate[b.len - 1] * taxable - table;
    }
    let width = st.recapture_phase_width;
    let phase = |excess: f64| ((excess.clamp(0.0, width) / width) * 10_000.0).round() / 10_000.0;
    let first = st.recapture_first_bracket[s];
    let mut m = 0;
    for (i, lower) in b.lower.iter().enumerate().take(b.len) {
        if taxable > *lower {
            m = i;
        }
    }
    if m <= first {
        return (b.rate[first] * taxable - table) * phase(agi - st.recapture_agi_start);
    }
    let m = m.min(b.len - 2);
    let base = b.rate[m - 1] * b.lower[m] - b.tax(b.lower[m]);
    let top = b.lower[m + 1];
    let increment = b.rate[m] * top - b.tax(top) - base;
    base + increment * phase(agi - b.lower[m])
}

/// Deductions and taxable income (step 5).
#[inline(always)]
fn deductions(p: &TaxYearParams, x: &TaxInputs, w: &mut Work) {
    let f = &p.federal;
    let s = x.filing_status.index();
    for &age in &x.ages {
        if age >= f.senior_age {
            w.seniors += 1;
        }
    }
    let seniors = f64::from(w.seniors);
    let gifts = x.charitable.max(0.0);

    w.standard = f.standard_deduction[s]
        + f.additional_deduction_65[s] * seniors
        + gifts.min(f.charitable_non_itemizer[s]);

    w.salt_paid = if x.state_tax_paid > 0.0 {
        x.state_tax_paid
    } else {
        w.state.total.max(0.0)
    } + x.property_tax.max(0.0);
    w.salt_cap = if f.salt_phase_down_threshold.is_finite() {
        (f.salt_cap - f.salt_phase_down_rate * (w.agi - f.salt_phase_down_threshold).max(0.0))
            .max(f.salt_floor.min(f.salt_cap))
    } else {
        f.salt_cap
    };
    w.salt = w.salt_paid.min(w.salt_cap);
    w.charity_itemized = (gifts - f.charitable_floor_rate * w.agi.max(0.0)).max(0.0);
    w.itemized_raw = w.salt + x.mortgage_interest.max(0.0) + w.charity_itemized;

    w.senior = seniors
        * (f.senior_deduction - f.senior_phaseout_rate * (w.agi - f.senior_phaseout[s]).max(0.0))
            .max(0.0);

    // Itemised deductions lose 2/37 of the part that offsets top-bracket income.
    let schedule = &f.ordinary[s];
    let top_start = schedule.lower[schedule.len.saturating_sub(1)];
    let in_top = (w.agi - w.senior - top_start).max(0.0);
    w.itemized_limit = f.itemized_limit_fraction * w.itemized_raw.min(in_top);
    w.itemized_net = w.itemized_raw - w.itemized_limit;
    w.itemized = w.itemized_net > w.standard;
    w.deduction = w.standard.max(w.itemized_net);

    let unfloored = w.agi - w.deduction - w.senior;
    let before_qbi = unfloored.max(0.0);
    let business = w.qbi - w.half_se;
    w.qbi = if business > 0.0 {
        let width = f.qbi_phase_in_width[s];
        let phase = if width > 0.0 {
            (1.0 - (before_qbi - f.qbi_threshold[s]) / width).clamp(0.0, 1.0)
        } else if before_qbi <= f.qbi_threshold[s] {
            1.0
        } else {
            0.0
        };
        f.qbi_rate * business.min((before_qbi - w.pref_income).max(0.0)) * phase
    } else {
        // No business income (most returns): the product below is the rate times
        // zero times a phase in [0, 1], which is zero.
        0.0
    };

    w.taxable_income = (before_qbi - w.qbi).max(0.0);
    w.taxable_unfloored = unfloored - w.qbi;
    w.pref_taxable = w.pref_income.min(w.taxable_income);
    w.ordinary_taxable = w.taxable_income - w.pref_taxable;
}

/// Tax, credits and penalties (steps 6 and 7).
#[inline(always)]
fn federal_tax(p: &TaxYearParams, x: &TaxInputs, w: &mut Work) {
    let f = &p.federal;
    let s = x.filing_status.index();
    w.tax_ordinary = f.ordinary[s].tax(w.ordinary_taxable);
    // Unrecaptured section 1250 gain sits on top of other ordinary income and is
    // taxed at the ordinary rates but never above 25%.
    let recapture = x.depreciation_recapture.max(0.0).min(w.ordinary_taxable);
    if recapture > 0.0 {
        let without = f.ordinary[s].tax(w.ordinary_taxable - recapture);
        let capped = without + super::UNRECAPTURED_1250_MAX_RATE * recapture;
        w.tax_ordinary = w.tax_ordinary.min(capped);
    }
    w.tax_pref =
        f.capital_gains[s].tax(w.taxable_income) - f.capital_gains[s].tax(w.ordinary_taxable);
    w.niit = f.niit_rate * w.nii.min((w.agi - f.niit_threshold[s]).max(0.0));
    // While ordinary income is still below the deductions, one more dollar of it is
    // not taxed at ordinary rates (it can still push gains up their own schedule).
    w.marginal = if w.agi - w.pref_income - w.deduction - w.senior - w.qbi >= 0.0 {
        f.ordinary[s].marginal_rate(w.ordinary_taxable)
    } else {
        0.0
    };

    let children = f64::from(x.children_under_17);
    w.credit_gross =
        children * f.child_tax_credit + f64::from(x.other_dependents) * f.other_dependent_credit;
    if w.credit_gross > 0.0 {
        let steps =
            ((w.agi - f.child_credit_phaseout[s]).max(0.0) / CHILD_CREDIT_PHASEOUT_STEP).ceil();
        w.credit_phaseout = (steps * f.child_credit_phaseout_per_1000).min(w.credit_gross);
        let credit = w.credit_gross - w.credit_phaseout;
        w.credit_nonrefundable = credit.min(w.tax_ordinary + w.tax_pref);
        w.credit_refundable = (credit - w.credit_nonrefundable)
            .min(children * f.child_credit_refundable_max)
            .min(
                f.child_credit_refundable_rate
                    * (w.earned_income - f.child_credit_earned_floor).max(0.0),
            );
    }

    w.penalty_early = f.early_withdrawal_penalty * x.early_distributions.max(0.0);
    w.penalty_hsa = f.hsa_penalty * x.hsa_nonqualified.max(0.0);

    if x.amt_applies {
        minimum_tax(p, x, w);
    }
}

/// Tentative minimum tax, and the AMT income and exemption it rests on, for an
/// incentive-option adjustment `adj` to ordinary income and an adjustment `gadj`
/// (never positive) to long-term gains: `(tax, AMT income, exemption)`.
///
/// AMT income is regular taxable income with the standard deduction or the state
/// and local tax deduction added back. It starts from taxable income before the
/// floor at zero (Form 6251 line 1 may be negative), so a deduction that income did
/// not use is not added back. Long-term gains and qualified dividends keep
/// the regular 0/15/20% schedule, stacked on regular ordinary taxable income as
/// Form 6251 Part III does, and the result is capped at the flat 26/28% computation
/// (Form 6251 line 40). A negative gain adjustment comes off the gain taxed at those
/// rates, not off income taxed at 26% or 28%: the regular gain was computed from the
/// strike, the AMT gain from the value at exercise. It is therefore held to the
/// long-term gain that is in taxable income: where losses or an exclusion have
/// already removed the regular gain there is nothing left for it to come off (the
/// $3,000 of net capital loss the AMT would allow against other income is left out).
#[inline(always)]
fn tentative_minimum_tax(
    p: &TaxYearParams,
    x: &TaxInputs,
    w: &Work,
    adj: f64,
    gadj: f64,
) -> (f64, f64, f64) {
    let f = &p.federal;
    let s = x.filing_status.index();
    let addback = if w.itemized { w.salt } else { w.standard };
    let gadj = gadj.max(-w.pref_gain.max(0.0).min(w.pref_taxable));
    let amti = (w.taxable_unfloored + addback + adj + gadj).max(0.0);
    let exemption = (f.amt_exemption[s]
        - f.amt_phaseout_rate * (amti - f.amt_phaseout_start[s]).max(0.0))
    .max(0.0);
    let base = (amti - exemption).max(0.0);
    let pref = (w.pref_taxable + gadj).max(0.0).min(base);
    let ord = base - pref;
    let flat = |v: f64| {
        f.amt_rates[0] * v.min(f.amt_rate_break) + f.amt_rates[1] * (v - f.amt_rate_break).max(0.0)
    };
    let gains_tax = f.capital_gains[s].tax(w.ordinary_taxable + pref)
        - f.capital_gains[s].tax(w.ordinary_taxable);
    ((flat(ord) + gains_tax).min(flat(base)), amti, exemption)
}

/// Alternative minimum tax and the minimum tax credit (Form 6251, Form 8801). Runs
/// only when `TaxInputs.amt_applies`, after the regular tax and the credits.
///
/// The credit a year generates is the AMT with the incentive-option adjustments
/// less the AMT without them: AMT that large gains cause through the exemption
/// phase-out is an exclusion item and creates no credit. The credit carried in is
/// used up to the regular tax (after non-refundable credits) less the tentative
/// minimum tax.
#[inline(always)]
fn minimum_tax(p: &TaxYearParams, x: &TaxInputs, w: &mut Work) {
    let regular = w.tax_ordinary + w.tax_pref;
    let adj = x.amt_iso_adjustment;
    let gadj = x.amt_gain_adjustment.min(0.0);
    let (tmt, amti, exemption) = tentative_minimum_tax(p, x, w, adj, gadj);
    w.tmt = tmt;
    w.amti = amti;
    w.amt_exemption = exemption;
    w.amt = (tmt - regular).max(0.0);
    let amt_without = if adj == 0.0 && gadj == 0.0 {
        w.amt
    } else {
        (tentative_minimum_tax(p, x, w, 0.0, 0.0).0 - regular).max(0.0)
    };
    w.amt_credit_generated = (w.amt - amt_without).max(0.0);
    let carried_in = x.amt_credit_carryforward.max(0.0);
    w.amt_credit_used = carried_in.min((regular - tmt - w.credit_nonrefundable).max(0.0));
    w.amt_credit_out = carried_in - w.amt_credit_used + w.amt_credit_generated;
}

/// Poverty line for the marketplace household.
#[inline(always)]
fn poverty_line(p: &TaxYearParams, x: &TaxInputs) -> f64 {
    p.federal.fpl_base
        + p.federal.fpl_per_person * f64::from(x.aca_household_size.saturating_sub(1))
}

#[inline(always)]
fn on_marketplace(x: &TaxInputs) -> bool {
    x.aca_household_size > 0 && x.aca_months > 0
}

/// ACA premium credit and, with `IRMAA`, the IRMAA tier (step 8).
#[inline(always)]
fn health<const IRMAA: bool>(p: &TaxYearParams, x: &TaxInputs, w: &mut Work) {
    let f = &p.federal;
    if on_marketplace(x) {
        let income = w.magi_aca.max(0.0);
        w.aca_fpl = poverty_line(p, x);
        w.aca_ratio = if w.aca_fpl > 0.0 {
            income / w.aca_fpl
        } else {
            0.0
        };
        let over_cliff = f.aca.cliff && income > f.aca.max_ratio * w.aca_fpl + CLIFF_TOLERANCE;
        if !over_cliff && x.aca_benchmark_premium > 0.0 {
            w.aca_applicable = f.aca.applicable_percentage(w.aca_ratio);
            let months = f64::from(x.aca_months.min(12)) / 12.0;
            w.aca_credit = (x.aca_benchmark_premium - w.aca_applicable * income).max(0.0) * months;
        }
    }
    if !IRMAA {
        return;
    }
    w.irmaa_tier = irmaa_tier(p, x.filing_status, w.agi);
    let mut enrolled = 0.0;
    for &on_medicare in &x.on_medicare {
        if on_medicare {
            enrolled += 1.0;
        }
    }
    w.irmaa_surcharge = 12.0 * enrolled * f.irmaa[x.filing_status.index()].monthly[w.irmaa_tier];
}

/// The working of one return up to taxable income (steps 1 to 5).
#[inline(always)]
fn work_to_taxable(p: &TaxYearParams, x: &TaxInputs) -> Work {
    let mut w = Work {
        mode: mode(p),
        // A stock, not a flow: it passes through untouched unless the AMT runs.
        amt_credit_out: x.amt_credit_carryforward.max(0.0),
        ..Work::default()
    };
    if w.mode != Mode::Us {
        income(p, x, &mut w, false);
        w.qbi = 0.0;
        w.flat_base = (w.income_ex_ss + w.ss_total).max(0.0);
        w.agi = w.flat_base;
        w.magi_aca = w.flat_base;
        w.taxable_income = w.flat_base;
        w.marginal = match w.mode {
            Mode::Flat(rate) | Mode::Foreign(rate) => rate,
            _ => 0.0,
        };
        w.flat_tax = w.marginal * w.flat_base;
        w.total = w.flat_tax;
        return w;
    }
    payroll(p, x, &mut w);
    income(p, x, &mut w, true);
    social_security(p, x, &mut w);
    w.state = state_tax(p, x, &w);
    deductions(p, x, &mut w);
    w
}

/// The whole working of one return.
#[inline(always)]
pub(super) fn work(p: &TaxYearParams, x: &TaxInputs) -> Work {
    work_with::<true>(p, x)
}

/// The whole working of one return; without `IRMAA` the IRMAA tier and surcharge
/// are left at zero (nothing else depends on them).
#[inline(always)]
fn work_with<const IRMAA: bool>(p: &TaxYearParams, x: &TaxInputs) -> Work {
    let mut w = work_to_taxable(p, x);
    if w.mode != Mode::Us {
        return w;
    }
    federal_tax(p, x, &mut w);
    health::<IRMAA>(p, x, &mut w);
    let mut payroll_total = 0.0;
    for i in 0..MAX_PEOPLE {
        payroll_total += w.fica[i] + w.se_tax[i];
    }
    w.total = w.tax_ordinary + w.tax_pref - w.credit_nonrefundable - w.credit_refundable
        + w.niit
        + payroll_total
        + w.state.total
        + w.penalty_early
        + w.penalty_hsa
        + w.amt
        - w.amt_credit_used;
    w
}

#[inline(always)]
pub(super) fn result(w: &Work) -> TaxResult {
    let mut r = TaxResult {
        total: w.total,
        marginal_ordinary_rate: w.marginal,
        taxable_income: w.taxable_income,
        agi: w.agi,
        magi: w.magi_aca,
        magi_irmaa: w.agi,
        capital_loss_carryforward: w.loss_carry,
        rental_loss_suspended: w.rental_suspended,
        amt_credit_carryforward: w.amt_credit_out,
        ..TaxResult::default()
    };
    match w.mode {
        Mode::None => {}
        Mode::Flat(_) => r.federal_ordinary = w.flat_tax,
        Mode::Foreign(_) => r.foreign = w.flat_tax,
        Mode::Us => {
            r.federal_ordinary = w.tax_ordinary;
            r.federal_capital_gains = w.tax_pref;
            r.niit = w.niit;
            r.fica = w.fica;
            r.se_tax = w.se_tax;
            r.state = w.state.total;
            r.local = w.state.local;
            r.penalties = w.penalty_early + w.penalty_hsa;
            r.credits = w.credit_nonrefundable + w.credit_refundable;
            r.aca_premium_credit = w.aca_credit;
            r.aca_fpl_ratio = w.aca_ratio;
            r.irmaa_surcharge = w.irmaa_surcharge;
            r.irmaa_tier = w.irmaa_tier as u32;
            r.taxable_social_security = w.taxable_ss;
            r.deduction = w.deduction;
            r.itemized = w.itemized;
            r.tentative_minimum_tax = w.tmt;
            r.amt = w.amt;
            r.amt_credit_used = w.amt_credit_used;
        }
    }
    r
}

/// Computes one household's tax for one year. Pure and allocation-free.
///
/// All amounts are nominal dollars of the tax year. `params` must come from
/// [`TaxLaw::for_year`](super::TaxLaw::for_year) for the household's state.
#[inline]
pub fn compute(params: &TaxYearParams, inputs: &TaxInputs) -> TaxResult {
    debug_assert!(
        params.state.code == inputs.state || params.state.code == super::StateCode::NONE,
        "TaxInputs.state must match the state the parameters were built for"
    );
    result(&work(params, inputs))
}

/// [`compute`] without the IRMAA figures (`irmaa_tier` and `irmaa_surcharge` are
/// left at zero; every other field is the same to the bit). For callers that read
/// the surcharge elsewhere ([`irmaa_monthly_surcharge`]): the simulation.
#[inline]
pub fn compute_no_irmaa(params: &TaxYearParams, inputs: &TaxInputs) -> TaxResult {
    result(&work_with::<false>(params, inputs))
}

/// `compute(params, inputs).taxable_income`, without working out the tax itself.
#[inline]
pub fn taxable_income(params: &TaxYearParams, inputs: &TaxInputs) -> f64 {
    work_to_taxable(params, inputs).taxable_income
}

/// IRMAA tier (0 = no surcharge) for a MAGI and filing status. The caller applies the
/// two-year lookback: pass the MAGI of two years ago with the parameters of the
/// premium year.
#[inline]
pub fn irmaa_tier(params: &TaxYearParams, status: FilingStatus, magi: f64) -> usize {
    let table = &params.federal.irmaa[status.index()];
    let mut tier = 0;
    for i in 1..table.len {
        // The top tier starts at its threshold, the others just above theirs.
        let reached = if i + 1 == table.len {
            magi >= table.over[i]
        } else {
            magi > table.over[i]
        };
        if reached {
            tier = i;
        }
    }
    tier
}

/// IRMAA surcharge per person per month (Part B plus Part D) for a MAGI and filing
/// status, in dollars of the parameters' year.
pub fn irmaa_monthly_surcharge(params: &TaxYearParams, status: FilingStatus, magi: f64) -> f64 {
    params.federal.irmaa[status.index()].monthly[irmaa_tier(params, status, magi)]
}

/// How much more ordinary income (a Roth conversion, a traditional withdrawal) fits
/// before federal ordinary taxable income passes the top of the bracket taxed at
/// `rate`.
///
/// This is the amount of *income*, not of taxable income: where an extra dollar also
/// makes Social Security taxable or shrinks a deduction, fewer dollars fit, and the
/// search accounts for that. Returns 0 when income is already above that bracket and
/// infinity for the top bracket, a rate that is not in the schedule, or the flat and
/// no-tax modes. Accurate to half a cent; costs a handful of `compute` calls.
pub fn room_to_bracket_top(params: &TaxYearParams, inputs: &TaxInputs, rate: f64) -> f64 {
    if mode(params) != Mode::Us {
        return f64::INFINITY;
    }
    let top = params.federal.ordinary[inputs.filing_status.index()].top_of_rate(rate);
    if !top.is_finite() {
        return f64::INFINITY;
    }
    let taxable_with = |extra: f64| {
        let mut probe = *inputs;
        probe.roth_conversions += extra;
        work(params, &probe).ordinary_taxable
    };
    let mut lo = 0.0;
    let mut f_lo = taxable_with(0.0);
    if f_lo >= top {
        return 0.0;
    }
    // Find an amount that overshoots. One dollar of income adds at least one dollar
    // of taxable income in almost every case, so the first guess usually does.
    let mut hi = top - f_lo;
    let mut f_hi = taxable_with(hi);
    let mut iterations = 0;
    while f_hi <= top {
        lo = hi;
        f_lo = f_hi;
        if top - f_lo < ROOM_TOLERANCE {
            return lo;
        }
        iterations += 1;
        if iterations > ROOM_MAX_ITERATIONS {
            return lo;
        }
        hi = lo + 2.0 * (top - f_lo) + 1.0;
        f_hi = taxable_with(hi);
    }
    // Interpolate, with a bisection step whenever interpolation fails to halve the
    // interval, keeping f(lo) <= top < f(hi).
    while iterations < ROOM_MAX_ITERATIONS && hi - lo > ROOM_TOLERANCE {
        iterations += 1;
        let width = hi - lo;
        let guess = lo + (top - f_lo) * width / (f_hi - f_lo);
        let x = guess.clamp(lo, hi);
        let fx = taxable_with(x);
        if fx <= top {
            lo = x;
            f_lo = fx;
            if top - f_lo < ROOM_TOLERANCE {
                return lo;
            }
        } else {
            hi = x;
            f_hi = fx;
        }
        if hi - lo > 0.5 * width {
            let mid = 0.5 * (lo + hi);
            let f_mid = taxable_with(mid);
            if f_mid <= top {
                lo = mid;
                f_lo = f_mid;
            } else {
                hi = mid;
                f_hi = f_mid;
            }
        }
    }
    lo
}

/// How much more income fits before the household's ACA income passes the level
/// above which there is no premium credit (400% of the poverty line under current
/// law). Every kind of income counts dollar for dollar, including the untaxed part
/// of Social Security.
///
/// Returns 0 when already above, and infinity when nobody is on a marketplace plan,
/// when the law in force has no cliff, or in the flat and no-tax modes.
pub fn room_to_aca_cliff(params: &TaxYearParams, inputs: &TaxInputs) -> f64 {
    if mode(params) != Mode::Us || !on_marketplace(inputs) || !params.federal.aca.cliff {
        return f64::INFINITY;
    }
    let w = work(params, inputs);
    (params.federal.aca.max_ratio * poverty_line(params, inputs) - w.magi_aca.max(0.0)).max(0.0)
}

/// All-in marginal rate on one more dollar of ordinary income: the change in `total`
/// plus the premium credit lost, per dollar, measured over the next $100. Unlike
/// [`TaxResult::marginal_ordinary_rate`] this includes state tax, the taxation of
/// more Social Security, phase-outs and the ACA credit (so it can exceed 1 at the
/// cliff). IRMAA is not included.
pub fn effective_marginal_rate(params: &TaxYearParams, inputs: &TaxInputs) -> f64 {
    let base = work(params, inputs);
    let mut probe = *inputs;
    probe.other_ordinary += MARGINAL_PROBE;
    let more = work(params, &probe);
    ((more.total - more.aca_credit) - (base.total - base.aca_credit)) / MARGINAL_PROBE
}
