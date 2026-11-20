//! Checks of the household-behaviour settings (`sim/behavior`).

use super::Checker;
use crate::model::JobSearch;

impl Checker<'_> {
    /// Errors for impossible behaviour settings, warnings for implausible ones.
    pub(super) fn behavior(&mut self) {
        if let Some(r) = self.p.retirement.return_to_work {
            let base = "/retirement/return_to_work";
            self.r.range(
                &format!("{base}/funded_ratio_below"),
                "The funded ratio that sends a retiree back to work",
                r.funded_ratio_below,
                0.0,
                5.0,
            );
            self.r.range(
                &format!("{base}/until_age"),
                "The age to which a retiree may go back to work",
                r.until_age,
                18.0,
                100.0,
            );
            self.r.range(
                &format!("{base}/pay_fraction"),
                "The share of former pay earned on going back to work",
                r.pay_fraction,
                0.0,
                1.5,
            );
            if let Some(pay) = r.former_annual_pay {
                self.r
                    .money(&format!("{base}/former_annual_pay"), "Former pay", pay);
            }
            if let JobSearch::Fixed { months } = r.search {
                self.r.range(
                    &format!("{base}/search/months"),
                    "The length of the job search",
                    months,
                    0.0,
                    120.0,
                );
            }
            if r.funded_ratio_below > 1.0 && r.funded_ratio_below.is_finite() {
                self.r.warn(
                    "return_to_work_when_funded",
                    &format!("{base}/funded_ratio_below"),
                    "A threshold above 1 sends a fully funded household back to work, and it stops again the next January: use a value below 1 (0.8 means 20% short).",
                );
            }
            if r.pay_fraction > 1.0 {
                self.r.warn(
                    "return_to_work_pay_high",
                    &format!("{base}/pay_fraction"),
                    "Retirees who go back to work usually earn less than before; more than their former pay is unusual.",
                );
            }
            if r.until_age > 75.0 && r.until_age.is_finite() {
                self.r.warn(
                    "return_to_work_late",
                    &format!("{base}/until_age"),
                    "Fewer than one in ten Americans over 75 work for pay; finding a job after 75 is unlikely.",
                );
            }
        }
    }
}
