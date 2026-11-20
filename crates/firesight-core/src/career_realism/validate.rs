//! Profile checks for the career-realism fields. Called from `validate.rs` with one
//! line per kind of income or person; the findings are pushed onto the same report.

use crate::model::{DisabilityRisk, Job, JobMobility, LateCareer, Person, RecessionPay, RiskLevel};
use crate::validate::{Issue, Report};

fn issue(code: &str, path: String, message: &str) -> Issue {
    Issue {
        code: code.into(),
        path,
        message: message.into(),
    }
}

/// Checks the realism fields of a job. `k` is the path of the income's `kind`;
/// `career_risk` is `assumptions.career_risk`.
pub fn job(r: &mut Report, k: &str, job: &Job, career_risk: bool) {
    let inert = job.risk.level == RiskLevel::None || !career_risk;
    if job.risk.job_mobility != JobMobility::Off && inert {
        r.warnings.push(issue(
            "job_mobility_inert",
            format!("{k}/risk/job_mobility"),
            "Job changes are part of career risk and do nothing while the job's risk \
             level is none or assumptions.career_risk is off. Set the risk level to \
             low, typical or high, or set job_mobility to off.",
        ));
    }
    if job.risk.late_career != LateCareer::Off && inert {
        r.warnings.push(issue(
            "late_career_inert",
            format!("{k}/risk/late_career"),
            "Late-career layoff effects are part of career risk and do nothing while \
             the job's risk level is none or assumptions.career_risk is off. Set the \
             risk level to low, typical or high, or set late_career to off.",
        ));
    }
    if job.risk.recession_pay != RecessionPay::Off && !career_risk {
        r.warnings.push(issue(
            "recession_pay_inert",
            format!("{k}/risk/recession_pay"),
            "Recession pay effects are part of career risk and do nothing while \
             assumptions.career_risk is off. Switch career_risk on or set \
             recession_pay to off.",
        ));
    }
}

/// Checks the disability fields of a person. `path` is the person's path; `works` says
/// whether any job or self-employment income belongs to them.
pub fn person(r: &mut Report, path: &str, person: &Person, works: bool) {
    if person.disability_risk == DisabilityRisk::Off {
        if person.ltd_insurance.is_some() {
            r.warnings.push(issue(
                "ltd_inert",
                format!("{path}/ltd_insurance"),
                "LTD insurance pays only when disability_risk is on; set disability_risk \
                 to typical or remove ltd_insurance.",
            ));
        }
        return;
    }
    if !works {
        r.warnings.push(issue(
            "disability_without_work",
            format!("{path}/disability_risk"),
            "This person has no job or self-employment income, so a disability would \
             only start Social Security Disability Insurance. Add an income or turn \
             disability_risk off.",
        ));
    }
    if let Some(l) = &person.ltd_insurance {
        if !l.replacement.is_finite() || !(0.0..=1.0).contains(&l.replacement) {
            r.errors.push(issue(
                "out_of_range",
                format!("{path}/ltd_insurance/replacement"),
                &format!(
                    "LTD replacement is {}; it must be between 0 and 1 (0.6 is typical).",
                    l.replacement
                ),
            ));
        } else if l.replacement > 0.8 {
            r.warnings.push(issue(
                "ltd_replacement_high",
                format!("{path}/ltd_insurance/replacement"),
                "Group LTD policies rarely replace more than 70% of pay. Check the figure.",
            ));
        }
        if !l.to_age.is_finite() || !(40.0..=80.0).contains(&l.to_age) {
            r.errors.push(issue(
                "out_of_range",
                format!("{path}/ltd_insurance/to_age"),
                &format!(
                    "LTD end age is {}; it must be between 40 and 80 (65 is typical).",
                    l.to_age
                ),
            ));
        }
    }
}
