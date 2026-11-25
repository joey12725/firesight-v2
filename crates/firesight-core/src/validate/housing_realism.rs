//! Validation of the opt-in housing realism options.

use super::{seg, Checker};
use crate::model::*;

impl Checker<'_> {
    pub(super) fn housing_realism(&mut self) {
        let p = self.p;
        for (i, pr) in p.properties.iter().enumerate() {
            let path = format!("/properties/{}", seg(&pr.id, i));
            self.realism_property(
                &path,
                (pr.maintenance, pr.assessment, pr.insurance_real_growth),
                (pr.maintenance_rate, pr.insurance_rate, pr.value),
            );
            if let Some(m) = &pr.mortgage {
                self.realism_mortgage(&format!("{path}/mortgage/kind"), m.kind);
            }
        }
        for (i, ev) in p.events.iter().enumerate() {
            if let EventKind::BuyHome(b) = &ev.kind {
                let path = format!("/events/{}/kind", seg(&ev.id, i));
                self.realism_property(
                    &path,
                    (b.maintenance, b.assessment, b.insurance_real_growth),
                    (b.maintenance_rate, b.insurance_rate, b.price),
                );
                self.realism_mortgage(&format!("{path}/mortgage_kind"), b.mortgage_kind);
            }
        }
        for (i, item) in p.spending.items.iter().enumerate() {
            if item.rent_growth == RentGrowth::Market {
                let path = format!("/spending/items/{}/rent_growth", seg(&item.id, i));
                if item.category != Category::Rent {
                    self.r.error(
                        "rent_growth_category",
                        &path,
                        "rent_growth \"market\" applies only to budget lines in the rent category.",
                    );
                } else if item.real_growth.is_some() {
                    self.r.warn(
                        "rent_growth_ignored",
                        &path,
                        "real_growth is ignored on a line whose rent_growth is \"market\"; the rent index sets its growth.",
                    );
                }
            }
        }
        if p.assumptions.housing_liquidity == HousingLiquidity::Realistic
            && p.properties.is_empty()
            && !p
                .events
                .iter()
                .any(|e| matches!(e.kind, EventKind::BuyHome(_)))
        {
            self.r.warn(
                "housing_liquidity_unused",
                "/assumptions/housing_liquidity",
                "housing_liquidity is \"realistic\" but the household owns no property and buys none, so it has no effect.",
            );
        }
    }

    /// `options` are the maintenance, assessment and insurance settings; `costs` the
    /// maintenance rate, insurance rate and value (or price).
    fn realism_property(
        &mut self,
        path: &str,
        options: (MaintenanceModel, Assessment, InsuranceGrowth),
        costs: (f64, f64, f64),
    ) {
        let (maintenance, assessment, insurance) = options;
        let (maintenance_rate, insurance_rate, value) = costs;
        if maintenance == MaintenanceModel::Lumpy && maintenance_rate <= 0.0 {
            self.r.warn(
                "lumpy_without_cost",
                &format!("{path}/maintenance"),
                "Lumpy maintenance spends a share of maintenance_rate, which is 0, so there are no repairs.",
            );
        }
        if insurance == InsuranceGrowth::Stochastic && insurance_rate <= 0.0 {
            self.r.warn(
                "insurance_drift_without_cost",
                &format!("{path}/insurance_real_growth"),
                "Insurance drift scales the premium, which is 0 because insurance_rate is 0.",
            );
        }
        if let Assessment::Capped {
            annual_cap,
            assessed_value,
        } = assessment
        {
            let cap_path = format!("{path}/assessment");
            if self.r.range(
                &format!("{cap_path}/annual_cap"),
                "Assessment cap",
                annual_cap,
                0.0,
                0.25,
            ) && annual_cap > 0.06
            {
                self.r.warn(
                    "assessment_cap_high",
                    &format!("{cap_path}/annual_cap"),
                    "Assessment caps are usually 2% to 5% a year (California's is 2%).",
                );
            }
            if let Some(a) = assessed_value {
                if self
                    .r
                    .money(&format!("{cap_path}/assessed_value"), "Assessed value", a)
                    && value > 0.0
                    && a > value
                {
                    self.r.warn(
                        "assessed_above_value",
                        &format!("{cap_path}/assessed_value"),
                        "The assessed value is above the market value; it will be lowered to the market value.",
                    );
                }
            }
        }
    }

    fn realism_mortgage(&mut self, path: &str, kind: MortgageKind) {
        let MortgageKind::Arm {
            fixed_months,
            adjust_every_months,
            margin,
            periodic_cap,
            lifetime_cap,
        } = kind
        else {
            return;
        };
        if fixed_months > 600 {
            self.r.error(
                "out_of_range",
                &format!("{path}/fixed_months"),
                "fixed_months must be at most 600.",
            );
        }
        if adjust_every_months == 0 || adjust_every_months > 120 {
            self.r.error(
                "out_of_range",
                &format!("{path}/adjust_every_months"),
                "adjust_every_months must be between 1 and 120 (12 for a 5/1 ARM, 6 for a 5/6).",
            );
        }
        if self
            .r
            .range(&format!("{path}/margin"), "ARM margin", margin, 0.0, 0.15)
            && margin > 0.05
        {
            self.r.warn(
                "arm_margin_high",
                &format!("{path}/margin"),
                "ARM margins are usually 2% to 3% over the index.",
            );
        }
        let pc = self.r.range(
            &format!("{path}/periodic_cap"),
            "Periodic cap",
            periodic_cap,
            0.0,
            0.1,
        );
        let lc = self.r.range(
            &format!("{path}/lifetime_cap"),
            "Lifetime cap",
            lifetime_cap,
            0.0,
            0.2,
        );
        if pc && lc && lifetime_cap < periodic_cap {
            self.r.warn(
                "arm_caps_inverted",
                &format!("{path}/lifetime_cap"),
                "The lifetime cap is below the periodic cap, so the periodic cap never binds. Typical caps are 2% periodic and 5% lifetime.",
            );
        }
    }
}
