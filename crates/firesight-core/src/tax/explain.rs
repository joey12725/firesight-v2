//! The return as labelled lines, for the UI's 1040-style breakdown and for the AI to
//! quote. Allocates freely; never called inside the simulation loop.

use super::calc::{result, work, Mode, Work};
use super::{Brackets, TaxDetail, TaxInputs, TaxLine, TaxYearParams};
use crate::MAX_PEOPLE;

/// Whole dollars with thousands separators: `-1,234,568`.
fn usd(x: f64) -> String {
    if !x.is_finite() {
        return if x.is_nan() {
            "n/a".into()
        } else {
            "no limit".into()
        };
    }
    let rounded = x.abs().round() as u64;
    let digits = rounded.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if x < 0.0 && rounded > 0 {
        out.push('-');
    }
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A rate as a percentage with up to two decimals: `0.0765` gives `7.65%`.
fn pct(rate: f64) -> String {
    let text = format!("{:.2}", rate * 100.0);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text}%")
}

/// "12,400 at 10% + 38,000 at 12%": how a schedule taxes the slice of income between
/// `from` and `to`.
fn bracket_note(schedule: &Brackets, from: f64, to: f64) -> String {
    let mut parts = Vec::new();
    for i in 0..schedule.len {
        let lower = schedule.lower[i].max(from);
        let upper = if i + 1 < schedule.len {
            schedule.lower[i + 1].min(to)
        } else {
            to
        };
        if upper > lower {
            parts.push(format!(
                "{} at {}",
                usd(upper - lower),
                pct(schedule.rate[i])
            ));
        }
    }
    if parts.is_empty() {
        "nothing to tax".into()
    } else {
        parts.join(" + ")
    }
}

struct Lines(Vec<TaxLine>);

impl Lines {
    fn push(&mut self, key: &str, label: &str, amount: f64, note: String) {
        self.0.push(TaxLine {
            key: key.into(),
            label: label.into(),
            amount,
            note,
        });
    }

    /// Adds the line only when the amount is not zero.
    fn push_nonzero(&mut self, key: &str, label: &str, amount: f64, note: String) {
        if amount != 0.0 {
            self.push(key, label, amount, note);
        }
    }
}

fn flat_lines(p: &TaxYearParams, w: &Work, lines: &mut Lines) {
    lines.push(
        "income",
        "Income",
        w.flat_base,
        "All income net of pre-tax contributions, including Social Security and net capital gains"
            .into(),
    );
    match w.mode {
        Mode::Flat(rate) => lines.push(
            "flat_tax",
            "Flat tax",
            w.flat_tax,
            format!("{} of income (flat-rate tax assumption)", pct(rate)),
        ),
        Mode::Foreign(rate) => lines.push(
            "foreign_tax",
            "Foreign income tax",
            w.flat_tax,
            format!(
                "{} of income (flat effective rate while living outside the US)",
                pct(rate)
            ),
        ),
        _ => lines.push(
            "no_tax",
            "Tax",
            0.0,
            "The plan assumes no tax at all".into(),
        ),
    }
    lines.push(
        "total",
        "Total tax",
        w.total,
        format!("Tax year {}", p.year),
    );
}

fn income_lines(x: &TaxInputs, w: &Work, lines: &mut Lines) {
    let mut wages_note = String::from("Gross pay including bonuses and vested equity");
    if w.wages_no_fica > 0.0 {
        wages_note.push_str(&format!(
            "; includes {} of incentive-option income that bears no payroll tax",
            usd(w.wages_no_fica)
        ));
    }
    lines.push_nonzero("wages", "Wages", w.wages, wages_note);
    lines.push_nonzero(
        "self_employment",
        "Self-employment profit",
        w.self_employment,
        "Net profit before self-employment tax".into(),
    );
    lines.push_nonzero(
        "pretax_contributions",
        "Pre-tax retirement and HSA contributions",
        -w.deferrals,
        "Deferred from this year's income".into(),
    );
    lines.push_nonzero(
        "investment_income",
        "Interest and dividends",
        w.investment_income,
        format!(
            "Interest {}, ordinary dividends {}, qualified dividends {}",
            usd(x.interest),
            usd(x.ordinary_dividends),
            usd(x.qualified_dividends)
        ),
    );
    let gains = w.ordinary_gain + w.pref_gain - w.loss_deduction;
    if x.short_term_gains != 0.0 || x.long_term_gains != 0.0 {
        let mut note = format!(
            "Short-term {}, long-term {}",
            usd(x.short_term_gains),
            usd(x.long_term_gains)
        );
        if w.loss_deduction > 0.0 {
            note.push_str(&format!(
                "; net loss deductible up to {}",
                usd(w.loss_deduction)
            ));
        }
        if w.loss_carry > 0.0 {
            note.push_str(&format!("; {} carried to next year", usd(w.loss_carry)));
        }
        lines.push("capital_gains", "Capital gains", gains, note);
    }
    // For information: not part of income, so not in the sum to AGI.
    lines.push_nonzero(
        "qsbs_excluded_gain",
        "Gain excluded as qualified small business stock",
        x.qsbs_excluded_gain.max(0.0),
        "Left out of federal income; state income tax still applies".into(),
    );
    lines.push_nonzero(
        "retirement_income",
        "Retirement distributions and pensions",
        w.retirement_income,
        format!(
            "Traditional withdrawals {}, Roth conversions {}, pensions and annuities {}",
            usd(x.traditional_distributions),
            usd(x.roth_conversions),
            usd(x.pensions)
        ),
    );
    if x.rental_net != 0.0 {
        let note = if w.rental_suspended > 0.0 {
            format!(
                "Rental loss of {} limited by the passive-loss rule; {} carried forward",
                usd(-x.rental_net),
                usd(w.rental_suspended)
            )
        } else {
            "Rent less expenses and depreciation".into()
        };
        lines.push("rental_income", "Rental income", w.rental_allowed, note);
    }
    lines.push_nonzero(
        "other_income",
        "Other income",
        x.other_ordinary + x.hsa_nonqualified.max(0.0) + x.depreciation_recapture.max(0.0),
        "Other taxable income, including non-medical HSA withdrawals and recaptured depreciation"
            .into(),
    );
    lines.push_nonzero(
        "adjustments",
        "IRA and HSA contributions deducted",
        -x.adjustments.max(0.0),
        "Deductible contributions made outside payroll".into(),
    );
    lines.push_nonzero(
        "passive_loss_released",
        "Rental losses released by a sale",
        -x.passive_loss_released.max(0.0),
        "Losses suspended in earlier years, deductible when the property is sold".into(),
    );
    lines.push_nonzero(
        "half_se_tax_deduction",
        "Deduction for half of self-employment tax",
        -w.half_se,
        "Half of the Social Security and Medicare parts of self-employment tax".into(),
    );
    if w.ss_total > 0.0 {
        lines.push(
            "social_security_benefits",
            "Social Security benefits",
            w.ss_total,
            "Gross benefits for the year".into(),
        );
        lines.push(
            "taxable_social_security",
            "Taxable Social Security",
            w.taxable_ss,
            format!(
                "Provisional income {} (other income plus half of benefits); {} of benefits are taxable",
                usd(w.provisional),
                pct(w.taxable_ss / w.ss_total)
            ),
        );
    }
    lines.push(
        "agi",
        "Adjusted gross income",
        w.agi,
        "Income after adjustments, including taxable Social Security".into(),
    );
}

fn deduction_lines(p: &TaxYearParams, x: &TaxInputs, w: &Work, lines: &mut Lines) {
    let f = &p.federal;
    let s = x.filing_status.index();
    if w.itemized {
        let mut note = format!(
            "State and local taxes {} (paid {}, cap {}), mortgage interest {}, gifts {}",
            usd(w.salt),
            usd(w.salt_paid),
            usd(w.salt_cap),
            usd(x.mortgage_interest),
            usd(w.charity_itemized)
        );
        if w.itemized_limit > 0.0 {
            note.push_str(&format!(
                ", less {} top-bracket limitation",
                usd(w.itemized_limit)
            ));
        }
        note.push_str(&format!(
            "; more than the standard deduction of {}",
            usd(w.standard)
        ));
        lines.push("deduction", "Itemized deductions", -w.deduction, note);
    } else {
        let mut note = format!("Standard deduction {}", usd(f.standard_deduction[s]));
        if w.seniors > 0 {
            note.push_str(&format!(
                " plus {} for {} aged 65 or over",
                usd(f.additional_deduction_65[s] * f64::from(w.seniors)),
                w.seniors
            ));
        }
        let gifts = x.charitable.max(0.0).min(f.charitable_non_itemizer[s]);
        if gifts > 0.0 {
            note.push_str(&format!(" plus {} of cash gifts", usd(gifts)));
        }
        note.push_str(&format!(
            "; itemized deductions would be {}",
            usd(w.itemized_net)
        ));
        lines.push("deduction", "Standard deduction", -w.deduction, note);
    }
    lines.push_nonzero(
        "senior_deduction",
        "Senior deduction",
        -w.senior,
        format!(
            "{} per person aged 65 or over, reduced by {} of income above {}",
            usd(f.senior_deduction),
            pct(f.senior_phaseout_rate),
            usd(f.senior_phaseout[s])
        ),
    );
    lines.push_nonzero(
        "qbi_deduction",
        "Qualified business income deduction",
        -w.qbi,
        format!(
            "{} of self-employment profit, within the income limits",
            pct(f.qbi_rate)
        ),
    );
    lines.push(
        "taxable_income",
        "Taxable income",
        w.taxable_income,
        format!(
            "Ordinary income {} plus long-term gains and qualified dividends {}",
            usd(w.ordinary_taxable),
            usd(w.pref_taxable)
        ),
    );
}

fn federal_lines(p: &TaxYearParams, x: &TaxInputs, w: &Work, lines: &mut Lines) {
    let f = &p.federal;
    let s = x.filing_status.index();
    lines.push(
        "federal_ordinary_tax",
        "Federal tax on ordinary income",
        w.tax_ordinary,
        bracket_note(&f.ordinary[s], 0.0, w.ordinary_taxable),
    );
    if w.pref_taxable > 0.0 {
        lines.push(
            "federal_capital_gains_tax",
            "Federal tax on long-term gains and qualified dividends",
            w.tax_pref,
            format!(
                "Stacked on top of ordinary income: {}",
                bracket_note(&f.capital_gains[s], w.ordinary_taxable, w.taxable_income)
            ),
        );
    }
    if x.amt_applies {
        lines.push_nonzero(
            "amt",
            "Alternative minimum tax",
            w.amt,
            format!(
                "Tentative minimum tax {} less regular tax {}; AMT income {} after an exemption of {}; incentive-option adjustment {}",
                usd(w.tmt),
                usd(w.tax_ordinary + w.tax_pref),
                usd(w.amti),
                usd(w.amt_exemption),
                usd(x.amt_iso_adjustment)
            ),
        );
        lines.push_nonzero(
            "amt_credit",
            "Minimum tax credit used",
            -w.amt_credit_used,
            format!(
                "{} carried in, {} carried to next year",
                usd(x.amt_credit_carryforward.max(0.0)),
                usd(w.amt_credit_out)
            ),
        );
    }
    if w.credit_gross > 0.0 {
        let mut note = format!(
            "{} children under 17 and {} other dependants: {}",
            x.children_under_17,
            x.other_dependents,
            usd(w.credit_gross)
        );
        if w.credit_phaseout > 0.0 {
            note.push_str(&format!(
                ", reduced by {} because income is above {}",
                usd(w.credit_phaseout),
                usd(f.child_credit_phaseout[s])
            ));
        }
        if w.credit_refundable > 0.0 {
            note.push_str(&format!(
                "; {} of it is paid as a refund",
                usd(w.credit_refundable)
            ));
        }
        lines.push(
            "child_credits",
            "Child and dependant credits",
            -(w.credit_nonrefundable + w.credit_refundable),
            note,
        );
    }
    lines.push(
        "federal_income_tax",
        "Federal income tax after credits",
        w.tax_ordinary + w.tax_pref - w.credit_nonrefundable - w.credit_refundable,
        if w.amt > 0.0 || w.amt_credit_used > 0.0 {
            "Tax on ordinary income and gains less child and dependant credits; the alternative minimum tax and the minimum tax credit are on their own lines"
                .into()
        } else {
            "Tax on ordinary income and gains less credits".into()
        },
    );
    lines.push_nonzero(
        "niit",
        "Net investment income tax",
        w.niit,
        format!(
            "{} of the smaller of investment income {} and income above {}",
            pct(f.niit_rate),
            usd(w.nii),
            usd(f.niit_threshold[s])
        ),
    );
    for i in 0..MAX_PEOPLE {
        lines.push_nonzero(
            &format!("fica_{i}"),
            &format!("Social Security and Medicare tax, person {}", i + 1),
            w.fica[i],
            format!(
                "{} on wages up to {} plus {} on all wages of {}",
                pct(f.oasdi_rate),
                usd(f.oasdi_wage_base),
                pct(f.medicare_rate),
                usd(w.fica_wages[i])
            ),
        );
        lines.push_nonzero(
            &format!("se_tax_{i}"),
            &format!("Self-employment tax, person {}", i + 1),
            w.se_tax[i],
            format!(
                "{} Social Security and {} Medicare on {} ({} of profit)",
                pct(f.se_oasdi_rate),
                pct(f.se_medicare_rate),
                usd(w.se_base[i]),
                pct(f.se_net_earnings_factor)
            ),
        );
    }
    lines.push_nonzero(
        "additional_medicare_tax",
        "Additional Medicare tax (included above)",
        w.additional_medicare,
        format!(
            "{} of earnings above {}",
            pct(f.additional_medicare_rate),
            usd(f.additional_medicare_threshold[s])
        ),
    );
    lines.push_nonzero(
        "early_withdrawal_penalty",
        "Early withdrawal penalty",
        w.penalty_early,
        format!(
            "{} of {} withdrawn early",
            pct(f.early_withdrawal_penalty),
            usd(x.early_distributions)
        ),
    );
    lines.push_nonzero(
        "hsa_penalty",
        "HSA non-medical withdrawal penalty",
        w.penalty_hsa,
        format!(
            "{} of {} withdrawn for non-medical use",
            pct(f.hsa_penalty),
            usd(x.hsa_nonqualified)
        ),
    );
}

fn state_lines(p: &TaxYearParams, x: &TaxInputs, w: &Work, lines: &mut Lines) {
    let st = &p.state;
    let s = x.filing_status.index();
    let o = &w.state;
    let name = st.code.as_str();
    if st.brackets[s].len > 0 {
        let mut note = format!("Federal AGI {}", usd(w.agi));
        if w.taxable_ss > 0.0 {
            note.push_str(&format!(
                ", with {} of Social Security taxed by the state",
                usd(o.ss_taxed)
            ));
        }
        if o.retirement_exclusion > 0.0 {
            note.push_str(&format!(
                ", less {} of excluded retirement income",
                usd(o.retirement_exclusion)
            ));
        }
        if o.gains_exclusion > 0.0 {
            note.push_str(&format!(
                ", less {} of excluded capital gains",
                usd(o.gains_exclusion)
            ));
        }
        if x.qsbs_excluded_gain > 0.0 {
            note.push_str(&format!(
                ", plus {} of gain excluded federally as qualified small business stock",
                usd(x.qsbs_excluded_gain)
            ));
        }
        note.push_str(&format!(
            ", less {} deduction of {} and exemptions of {}",
            if o.itemized { "itemized" } else { "standard" },
            usd(o.deduction),
            usd(o.exemptions)
        ));
        lines.push(
            "state_taxable_income",
            &format!("{name} taxable income"),
            o.taxable,
            note,
        );
        let mut note = bracket_note(&st.brackets[s], 0.0, o.taxable);
        if o.credits > 0.0 {
            note.push_str(&format!(", less credits of {}", usd(o.credits)));
        }
        if o.surtax > 0.0 {
            note.push_str(&format!(", plus investment surtax of {}", usd(o.surtax)));
        }
        lines.push(
            "state_income_tax",
            &format!("{name} income tax"),
            o.income_tax + o.surtax,
            note,
        );
    } else if o.excise == 0.0 {
        lines.push(
            "state_income_tax",
            "State income tax",
            0.0,
            if st.code == super::StateCode::NONE {
                "No state income tax applies".into()
            } else {
                format!("{name} does not tax this income")
            },
        );
    }
    lines.push_nonzero(
        "state_capital_gains_excise",
        &format!("{name} capital gains excise"),
        o.excise,
        format!(
            "On long-term gains above {}",
            usd(st.capital_gains_excise_deduction)
        ),
    );
    lines.push_nonzero(
        "local_income_tax",
        "Local income tax",
        o.local,
        if st.local_brackets[x.filing_status.index()].len > 0 {
            format!(
                "The city's graduated schedule (top rate {}) on state taxable income {}",
                pct(st.local_rate),
                usd(o.taxable)
            )
        } else if st.local_earned_only {
            format!("{} of wages and business profit", pct(st.local_rate))
        } else {
            format!(
                "{} of state adjusted income {}",
                pct(st.local_rate),
                usd(o.agi)
            )
        },
    );
}

fn health_lines(p: &TaxYearParams, x: &TaxInputs, w: &Work, lines: &mut Lines) {
    let f = &p.federal;
    if x.aca_household_size > 0 && x.aca_months > 0 {
        let note = if w.aca_credit > 0.0 {
            format!(
                "Income {} is {} of the poverty line ({} for {} people); expected to pay {} of income toward the benchmark plan of {}",
                usd(w.magi_aca),
                pct(w.aca_ratio),
                usd(w.aca_fpl),
                x.aca_household_size,
                pct(w.aca_applicable),
                usd(x.aca_benchmark_premium)
            )
        } else if f.aca.cliff && w.aca_ratio > f.aca.max_ratio {
            format!(
                "Income {} is {} of the poverty line, above the {} limit: no credit",
                usd(w.magi_aca),
                pct(w.aca_ratio),
                pct(f.aca.max_ratio)
            )
        } else {
            format!(
                "Income {} is {} of the poverty line; the expected contribution covers the benchmark plan",
                usd(w.magi_aca),
                pct(w.aca_ratio)
            )
        };
        lines.push(
            "aca_premium_credit",
            "Health insurance premium tax credit (reduces premiums, not in total)",
            w.aca_credit,
            note,
        );
    }
    lines.push_nonzero(
        "irmaa_surcharge",
        "Medicare income-related surcharge (billed two years later, not in total)",
        w.irmaa_surcharge,
        format!(
            "Income {} is in surcharge tier {}",
            usd(w.agi),
            w.irmaa_tier
        ),
    );
}

/// Like [`compute`](super::compute), but also returns the working. Allocates; never
/// used in the loop.
pub fn explain(params: &TaxYearParams, inputs: &TaxInputs) -> TaxDetail {
    let w = work(params, inputs);
    let mut lines = Lines(Vec::with_capacity(32));
    if w.mode != Mode::Us {
        flat_lines(params, &w, &mut lines);
    } else {
        income_lines(inputs, &w, &mut lines);
        deduction_lines(params, inputs, &w, &mut lines);
        federal_lines(params, inputs, &w, &mut lines);
        state_lines(params, inputs, &w, &mut lines);
        let income = w.agi + (w.ss_total - w.taxable_ss);
        lines.push(
            "total",
            "Total tax",
            w.total,
            if income > 0.0 {
                format!(
                    "{} of income of {}; the next dollar of ordinary income is in the {} federal bracket",
                    pct(w.total / income),
                    usd(income),
                    pct(w.marginal)
                )
            } else {
                format!("Tax year {}", params.year)
            },
        );
        health_lines(params, inputs, &w, &mut lines);
    }
    TaxDetail {
        result: result(&w),
        lines: lines.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_money_and_rates() {
        assert_eq!(usd(0.0), "0");
        assert_eq!(usd(999.4), "999");
        assert_eq!(usd(1_234_567.8), "1,234,568");
        assert_eq!(usd(-32_200.0), "-32,200");
        assert_eq!(usd(-0.2), "0");
        assert_eq!(usd(f64::INFINITY), "no limit");
        assert_eq!(pct(0.0765), "7.65%");
        assert_eq!(pct(0.22), "22%");
        assert_eq!(pct(0.0), "0%");
    }

    #[test]
    fn bracket_notes_describe_each_slice() {
        let b = Brackets::from_pairs(&[(0.0, 0.10), (10_000.0, 0.20)]);
        assert_eq!(
            bracket_note(&b, 0.0, 25_000.0),
            "10,000 at 10% + 15,000 at 20%"
        );
        assert_eq!(
            bracket_note(&b, 8_000.0, 12_000.0),
            "2,000 at 10% + 2,000 at 20%"
        );
        assert_eq!(bracket_note(&b, 0.0, 0.0), "nothing to tax");
    }
}
