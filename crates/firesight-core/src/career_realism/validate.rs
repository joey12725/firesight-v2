//! Profile checks for the career-realism fields. Called from `validate.rs` with one
//! line per kind of income or person; the findings are pushed onto the same report.

use crate::model::{Job, JobMobility, RecessionPay, RiskLevel};
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
