//! Checks of the household-behaviour settings (`sim/behavior`).

use super::{seg, Checker};
use crate::model::{Claiming, InvestorBehavior, JobSearch, SocialSecurityBasis};

impl Checker<'_> {
    /// Errors for impossible behaviour settings, warnings for implausible ones.
    pub(super) fn behavior(&mut self) {
        self.panic_selling();
        self.adaptive_claiming();
        if self.p.portfolio.pause_deferrals_in_distress {
            self.r.range(
                "/portfolio/distress/high_interest_rate",
                "The interest rate from which a debt counts as high-interest",
                self.p.portfolio.distress.high_interest_rate,
                0.0,
                1.0,
            );
        }
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

    fn panic_selling(&mut self) {
        let InvestorBehavior::PanicProne(p) = self.p.portfolio.behavior else {
            return;
        };
        let base = "/portfolio/behavior";
        self.r.range(
            &format!("{base}/drawdown_trigger"),
            "The stock-market fall that can set off a panic sale",
            p.drawdown_trigger,
            0.01,
            0.95,
        );
        self.r.range(
            &format!("{base}/probability"),
            "The chance of a panic sale in a fall",
            p.probability,
            0.0,
            1.0,
        );
        self.r.range(
            &format!("{base}/share"),
            "The share of stocks sold in a panic",
            p.share,
            0.0,
            1.0,
        );
        self.r.range(
            &format!("{base}/reentry/recovered_within"),
            "How close to their peak stocks must be before buying back",
            p.reentry.recovered_within,
            0.0,
            0.95,
        );
        if p.reentry.max_months == 0 {
            self.r.error(
                "panic_reentry_months",
                &format!("{base}/reentry/max_months"),
                "A household that buys back in the month it sells has not sold: use at least 1 month.",
            );
        }
        if p.reentry.recovered_within >= p.drawdown_trigger {
            self.r.warn(
                "panic_reentry_immediate",
                &format!("{base}/reentry/recovered_within"),
                "The buy-back level is at or below the fall that triggers the sale, so the household buys back the next month: use a smaller value than drawdown_trigger.",
            );
        }
        if p.probability > 0.5 {
            self.r.warn(
                "panic_probability_high",
                &format!("{base}/probability"),
                "In 2008 about 2% of Vanguard plan savers sold all their stocks and 16% traded at all; a panic chance above one half is far beyond what was observed.",
            );
        }
    }

    fn adaptive_claiming(&mut self) {
        for (i, person) in self.p.household.people.iter().enumerate() {
            let ss = &person.social_security;
            let Claiming::Adaptive { earliest, latest } = ss.claiming else {
                continue;
            };
            let base = format!(
                "/household/people/{}/social_security/claiming",
                seg(&person.id, i)
            );
            let ok_early = self.r.range(
                &format!("{base}/earliest"),
                "The earliest Social Security claim age",
                earliest,
                62.0,
                70.0,
            );
            let ok_late = self.r.range(
                &format!("{base}/latest"),
                "The latest Social Security claim age",
                latest,
                62.0,
                70.0,
            );
            if ok_early && ok_late && earliest > latest {
                self.r.error(
                    "claiming_order",
                    &format!("{base}/earliest"),
                    "The earliest claim age is after the latest one.",
                );
            }
            if matches!(ss.basis, SocialSecurityBasis::None) {
                self.r.warn(
                    "claiming_without_benefit",
                    &base,
                    "This person has no Social Security benefit, so the claiming strategy has no effect.",
                );
            }
        }
    }
}
