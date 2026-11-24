//! Medicaid as the payer of long-term care once savings are spent down
//! (`spending.healthcare.medicaid`).
//!
//! National defaults of the federal rules (`params.json`
//! `health_mortality_care.medicaid`, 2026 figures):
//!
//! * **Spend-down.** Care is paid privately until countable assets (everything
//!   liquid: cash, taxable, retirement and health accounts) are down to the asset
//!   limit ($2,000, a fixed dollar figure since 1989, not indexed). A home is not
//!   countable, but a single applicant with home equity above the limit ($752,000)
//!   is not eligible.
//! * **Spousal impoverishment.** With a spouse at home (the "community spouse"),
//!   half of the couple's countable assets when the care episode began (the
//!   "snapshot") is protected, at least the minimum and at most the maximum
//!   community spouse resource allowance ($32,532 to $162,660),
//!   and the spouse keeps enough of the patient's income to bring their own up to the
//!   minimum monthly maintenance needs allowance ($2,643.75).
//! * **Patient liability.** Once eligible the person pays toward their care all of
//!   their income less a personal needs allowance (nursing home) or a maintenance
//!   allowance at the special income level (care at home or in assisted living,
//!   $2,982), less their own health insurance premiums and out-of-pocket costs and
//!   the spousal allowance. Medicaid pays the rest of the care cost.
//! * **Estate recovery.** After the death of the recipient and of any surviving
//!   spouse, the state recovers what it paid from the estate, up to its value.
//!
//! Indexed figures (the spousal allowances, the home-equity limit, the special
//! income level) rise with the path's CPI; the asset limit and the personal needs
//! allowance are fixed in dollars, as they have been in law.
//!
//! Not modelled: state variation (asset limits, income caps, medically needy
//! spend-down, more generous allowances), the five-year look-back on gifts,
//! Medicaid planning (annuities, trusts, spending on exempt assets), dual-eligible
//! coverage of Medicare premiums and cost-sharing (the person's own health costs are
//! deducted from the liability instead), and limits on which care settings a state's
//! waivers cover (here Medicaid pays home care and assisted living as well as
//! nursing homes).

use crate::data::Data;

/// Personal needs allowance a nursing-home resident on Medicaid keeps each month,
/// in dollars. A judgement: the federal minimum is $30 (42 U.S.C. 1396a(q)); most
/// states set more and the national figure is not sourced here.
pub const PERSONAL_NEEDS_ALLOWANCE: f64 = 60.0;

/// The rules, in dollars of the plan's start (indexed ones) or nominal (fixed
/// ones).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MedicaidRules {
    /// Countable assets an applicant may keep, nominal.
    pub asset_limit: f64,
    /// Community spouse resource allowance bounds, today's dollars.
    pub csra_min: f64,
    /// Upper bound of the allowance, today's dollars.
    pub csra_max: f64,
    /// Minimum monthly maintenance needs allowance, today's dollars.
    pub mmmna: f64,
    /// Home equity above which a single applicant is not eligible, today's dollars.
    pub home_equity_limit: f64,
    /// Monthly allowance kept by someone receiving care at home or in assisted
    /// living (the special income level), today's dollars.
    pub community_allowance: f64,
    /// Monthly allowance kept by a nursing-home resident, nominal.
    pub personal_needs: f64,
}

/// What one person's month of care looks like to Medicaid. Money is nominal;
/// `price` converts the indexed figures.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CareMonth {
    /// Cost of this month's care.
    pub cost: f64,
    /// The household's countable assets now.
    pub countable: f64,
    /// Equity in the household's home.
    pub home_equity: f64,
    /// True when a spouse lives in the community.
    pub spouse: bool,
    /// The person's own income this month (pensions, Social Security, insurance
    /// benefits).
    pub own_income: f64,
    /// The spouse's own income this month.
    pub spouse_income: f64,
    /// The person's own health premiums and out-of-pocket costs this month.
    pub own_health: f64,
    /// True for a nursing home, false for care at home or in assisted living.
    pub nursing_home: bool,
    /// Price level since the start.
    pub price: f64,
    /// The community spouse's protected assets fixed at the start of the episode
    /// (the snapshot), nominal; `None` computes them from `countable` now.
    pub snapshot: Option<f64>,
}

/// Who pays this month's care.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Split {
    /// Paid by the household.
    pub household: f64,
    /// Paid by Medicaid.
    pub medicaid: f64,
    /// True when Medicaid pays any of it.
    pub eligible: bool,
}

impl MedicaidRules {
    /// The 2026 national defaults from `params.json`.
    pub fn from_data(data: &Data) -> MedicaidRules {
        let m = &data.actuarial["health_mortality_care"]["medicaid"];
        let num = |k: &str| {
            m[k].as_f64()
                .unwrap_or_else(|| panic!("embedded data: medicaid.{k}"))
        };
        MedicaidRules {
            asset_limit: num("asset_limit_individual"),
            csra_min: num("csra_min_2026"),
            csra_max: num("csra_max_2026"),
            mmmna: num("mmmna_min_2026"),
            home_equity_limit: num("home_equity_limit_2026"),
            community_allowance: num("special_income_level_2026"),
            personal_needs: PERSONAL_NEEDS_ALLOWANCE,
        }
    }

    /// Assets of a couple the community spouse keeps.
    pub fn protected(&self, countable: f64, price: f64) -> f64 {
        (0.5 * countable.max(0.0)).clamp(self.csra_min * price, self.csra_max * price)
    }

    /// Splits one month's care between the household and Medicaid.
    pub fn split(&self, m: &CareMonth) -> Split {
        let private = Split {
            household: m.cost,
            medicaid: 0.0,
            eligible: false,
        };
        if m.cost <= 0.0 {
            return private;
        }
        if !m.spouse && m.home_equity > self.home_equity_limit * m.price {
            return private;
        }
        let protected = if m.spouse {
            m.snapshot
                .unwrap_or_else(|| self.protected(m.countable, m.price))
        } else {
            0.0
        };
        let excess = m.countable - protected - self.asset_limit;
        if excess >= m.cost {
            return private;
        }
        let allowance = if m.nursing_home {
            self.personal_needs
        } else {
            self.community_allowance * m.price
        };
        let spousal = if m.spouse {
            (self.mmmna * m.price - m.spouse_income).max(0.0)
        } else {
            0.0
        };
        let liability = (m.own_income - allowance - m.own_health - spousal).max(0.0);
        let household = (excess.max(0.0) + liability).min(m.cost);
        Split {
            household,
            medicaid: m.cost - household,
            eligible: true,
        }
    }
}

/// What the state recovers from an estate: what it paid, up to the estate's value.
pub fn estate_recovery(paid: f64, estate: f64) -> f64 {
    paid.max(0.0).min(estate.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> MedicaidRules {
        MedicaidRules::from_data(Data::embedded())
    }

    fn single_nursing_home() -> CareMonth {
        CareMonth {
            cost: 10_000.0,
            countable: 1_500.0,
            home_equity: 0.0,
            spouse: false,
            own_income: 2_500.0,
            spouse_income: 0.0,
            own_health: 400.0,
            nursing_home: true,
            price: 1.0,
            snapshot: None,
        }
    }

    #[test]
    fn rules_are_the_2026_national_figures() {
        let r = rules();
        assert_eq!(r.asset_limit, 2_000.0);
        assert_eq!((r.csra_min, r.csra_max), (32_532.0, 162_660.0));
        assert_eq!(r.mmmna, 2_643.75);
        assert_eq!(r.home_equity_limit, 752_000.0);
        assert_eq!(r.community_allowance, 2_982.0);
    }

    #[test]
    fn a_single_resident_pays_income_less_allowances() {
        // Spent down: liability = 2,500 - 60 - 400 = 2,040; Medicaid pays 7,960.
        let s = rules().split(&single_nursing_home());
        assert!(s.eligible);
        assert!((s.household - 2_040.0).abs() < 1e-9, "{s:?}");
        assert!((s.medicaid - 7_960.0).abs() < 1e-9);
    }

    #[test]
    fn spend_down_is_paid_privately_first() {
        let r = rules();
        // $50,000 left: pays everything.
        let s = r.split(&CareMonth {
            countable: 50_000.0,
            ..single_nursing_home()
        });
        assert!(!s.eligible && s.household == 10_000.0);
        // $5,000 left: the $3,000 above the limit plus the liability, then Medicaid.
        let s = r.split(&CareMonth {
            countable: 5_000.0,
            ..single_nursing_home()
        });
        assert!(s.eligible);
        assert!((s.household - (3_000.0 + 2_040.0)).abs() < 1e-9, "{s:?}");
        // A paid-for home above the equity limit makes a single applicant ineligible.
        let s = r.split(&CareMonth {
            home_equity: 800_000.0,
            ..single_nursing_home()
        });
        assert!(!s.eligible);
    }

    #[test]
    fn a_community_spouse_keeps_assets_and_income() {
        let r = rules();
        // $200,000 countable: half is 100,000, inside the allowance bounds, so the
        // excess is 200,000 - 100,000 - 2,000 = 98,000: still private.
        let couple = CareMonth {
            countable: 200_000.0,
            spouse: true,
            spouse_income: 1_200.0,
            ..single_nursing_home()
        };
        assert!(!r.split(&couple).eligible);
        // $60,000 countable: half is 30,000, below the minimum, so 32,532 is
        // protected; excess 60,000 - 32,532 - 2,000 = 25,468 > 10,000: private.
        assert!(
            !r.split(&CareMonth {
                countable: 60_000.0,
                ..couple
            })
            .eligible
        );
        // $40,000: excess 5,468 < 10,000: eligible. The spouse keeps
        // 2,643.75 - 1,200 = 1,443.75 of the patient's income; liability
        // 2,500 - 60 - 400 - 1,443.75 = 596.25.
        let s = r.split(&CareMonth {
            countable: 40_000.0,
            ..couple
        });
        assert!(s.eligible);
        assert!((s.household - (5_468.0 + 596.25)).abs() < 1e-9, "{s:?}");
        // With the snapshot taken when $300,000 was held, $150,000 stays protected:
        // $40,000 is now below it, so the excess is 0 and only the liability is
        // paid.
        let s = r.split(&CareMonth {
            countable: 40_000.0,
            snapshot: Some(r.protected(300_000.0, 1.0)),
            ..couple
        });
        assert_eq!(r.protected(300_000.0, 1.0), 150_000.0);
        assert!((s.household - 596.25).abs() < 1e-9, "{s:?}");
        // A snapshot above the maximum is capped by it.
        assert_eq!(r.protected(1_000_000.0, 1.0), 162_660.0);
        // The home does not count against a couple.
        let s = r.split(&CareMonth {
            countable: 40_000.0,
            home_equity: 2_000_000.0,
            ..couple
        });
        assert!(s.eligible);
    }

    #[test]
    fn care_at_home_keeps_the_maintenance_allowance_and_indexes() {
        let r = rules();
        let home = CareMonth {
            nursing_home: false,
            own_income: 3_500.0,
            ..single_nursing_home()
        };
        // 3,500 - 2,982 - 400 = 118.
        let s = r.split(&home);
        assert!((s.household - 118.0).abs() < 1e-9, "{s:?}");
        // At twice today's prices the allowance doubles; the $2,000 limit does not.
        let s = r.split(&CareMonth {
            price: 2.0,
            own_income: 7_000.0,
            own_health: 800.0,
            cost: 20_000.0,
            ..home
        });
        assert!((s.household - (7_000.0 - 5_964.0 - 800.0)).abs() < 1e-9);
        let s = r.split(&CareMonth {
            price: 2.0,
            countable: 3_000.0,
            own_income: 0.0,
            cost: 20_000.0,
            ..home
        });
        assert!((s.household - 1_000.0).abs() < 1e-9, "{s:?}");
    }

    #[test]
    fn estate_recovery_is_capped_by_the_estate() {
        assert_eq!(estate_recovery(150_000.0, 400_000.0), 150_000.0);
        assert_eq!(estate_recovery(150_000.0, 90_000.0), 90_000.0);
        assert_eq!(estate_recovery(150_000.0, -5_000.0), 0.0);
    }
}
