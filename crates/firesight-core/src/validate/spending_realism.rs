//! Checks for the opt-in spending realism fields: `spending.vehicles`,
//! `spending.unemployment_cut`.

use super::{check_ids, seg, Checker};
use crate::model::{Category, PropertyUse, ShockLevel};

impl Checker<'_> {
    pub(super) fn spending_realism(&mut self) {
        let p = self.p;
        let s = &p.spending;
        check_ids(
            &mut self.r,
            "/spending/vehicles",
            s.vehicles.iter().map(|v| v.id.as_str()),
        );
        let mut annualised = 0.0;
        for (i, v) in s.vehicles.iter().enumerate() {
            let path = format!("/spending/vehicles/{}", seg(&v.id, i));
            self.r
                .money(&format!("{path}/price"), "Vehicle price", v.price);
            let cycle_ok = self.r.range(
                &format!("{path}/replace_every_years"),
                "Replacement cycle",
                v.replace_every_years,
                1.0,
                50.0,
            );
            if cycle_ok && !(3.0..=20.0).contains(&v.replace_every_years) {
                self.r.warn(
                    "vehicle_cycle_unusual",
                    &format!("{path}/replace_every_years"),
                    format!(
                        "A replacement cycle of {} years is unusual; most households replace a car every 6 to 15 years.",
                        v.replace_every_years
                    ),
                );
            }
            if self.r.range(
                &format!("{path}/age_years"),
                "Vehicle age",
                v.age_years,
                0.0,
                60.0,
            ) && cycle_ok
                && v.age_years > v.replace_every_years
            {
                self.r.warn(
                    "vehicle_overdue",
                    &format!("{path}/age_years"),
                    "The vehicle is already older than its replacement cycle, so it is replaced in the first months of the plan.",
                );
            }
            if self.r.range(
                &format!("{path}/trade_in_fraction"),
                "Trade-in fraction",
                v.trade_in_fraction,
                0.0,
                1.0,
            ) && v.trade_in_fraction > 0.6
            {
                self.r.warn(
                    "vehicle_trade_in_high",
                    &format!("{path}/trade_in_fraction"),
                    "A trade-in covering more than 60% of the new price is unusual; the old vehicle is typically worth well under half of a new one.",
                );
            }
            if let Some(f) = &v.financing {
                let fp = format!("{path}/financing");
                self.r.range(
                    &format!("{fp}/down_fraction"),
                    "Down payment fraction",
                    f.down_fraction,
                    0.0,
                    1.0,
                );
                self.r
                    .range(&format!("{fp}/rate"), "Loan rate", f.rate, 0.0, 0.5);
                if f.term_months == 0 || f.term_months > 120 {
                    self.r.error(
                        "out_of_range",
                        &format!("{fp}/term_months"),
                        format!(
                            "Loan term is {} months; it must be between 1 and 120.",
                            f.term_months
                        ),
                    );
                }
            }
            if let Some(owner) = v.owner {
                self.person_ref(&format!("{path}/owner"), owner);
            }
            if let Some(w) = &v.end {
                self.when(&format!("{path}/end"), w);
            }
            if v.replace_every_years >= 1.0 {
                annualised +=
                    v.price * (1.0 - v.trade_in_fraction.clamp(0.0, 1.0)) / v.replace_every_years;
            }
        }
        let transport: f64 = s
            .items
            .iter()
            .filter(|i| i.category == Category::Transport)
            .map(|i| i.annual_amount())
            .sum();
        if annualised > 0.0 && transport > 0.0 && annualised > 0.5 * transport {
            self.r.warn(
                "vehicles_double_count",
                "/spending/vehicles",
                format!(
                    "Replacing the listed vehicles costs about {annualised:.0} a year on average, more than half of the Transport budget lines ({transport:.0} a year). If those lines include buying or depreciating a car, the cost is counted twice: keep only fuel, insurance and maintenance in them."
                ),
            );
        }

        if s.shocks == ShockLevel::Detailed {
            for (i, prop) in p.properties.iter().enumerate() {
                if matches!(prop.usage, PropertyUse::Primary) && prop.maintenance_rate > 0.015 {
                    self.r.warn(
                        "detailed_shocks_home_maintenance",
                        &format!("/properties/{}/maintenance_rate", seg(&prop.id, i)),
                        "Detailed shocks add home repairs (about 0.4% of the home's value a year on average) on top of this maintenance rate. With a rate above 1.5% the total upkeep is above the usual 1% to 2% rule of thumb; consider lowering it.",
                    );
                }
            }
        }

        if self.r.range(
            "/spending/unemployment_cut",
            "Unemployment spending cut",
            s.unemployment_cut,
            0.0,
            1.0,
        ) && s.unemployment_cut > 0.5
        {
            self.r.warn(
                "unemployment_cut_high",
                "/spending/unemployment_cut",
                "Cutting more than half of discretionary spending during unemployment is far above the evidence (Ganong and Noel 2019 imply 0.1 to 0.25 of discretionary lines).",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{Profile, Vehicle, VehicleFinancing};
    use crate::validate::validate;

    fn with_vehicle(v: Vehicle) -> Profile {
        let mut p = Profile::example();
        p.spending.vehicles = vec![v];
        p
    }

    fn car() -> Vehicle {
        Vehicle {
            id: "car".into(),
            name: "Car".into(),
            price: 40_000.0,
            ..Vehicle::default()
        }
    }

    #[test]
    fn a_sensible_vehicle_and_cut_validate_cleanly() {
        let mut v = car();
        v.price = 20_000.0;
        let mut p = with_vehicle(v);
        p.spending.unemployment_cut = 0.15;
        let r = validate(&p);
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        assert!(
            !r.warnings.iter().any(|w| w.path.starts_with("/spending/v")),
            "{:#?}",
            r.warnings
        );
    }

    #[test]
    fn impossible_values_are_errors() {
        let bad = |f: &dyn Fn(&mut Vehicle), code: &str, path: &str| {
            let mut v = car();
            f(&mut v);
            let r = validate(&with_vehicle(v));
            assert!(
                r.errors.iter().any(|e| e.code == code && e.path == path),
                "{path}: {:#?}",
                r.errors
            );
        };
        bad(
            &|v| v.price = -1.0,
            "negative_amount",
            "/spending/vehicles/@car/price",
        );
        bad(
            &|v| v.replace_every_years = 0.0,
            "out_of_range",
            "/spending/vehicles/@car/replace_every_years",
        );
        bad(
            &|v| v.trade_in_fraction = 1.5,
            "out_of_range",
            "/spending/vehicles/@car/trade_in_fraction",
        );
        bad(
            &|v| {
                v.financing = Some(VehicleFinancing {
                    term_months: 0,
                    ..VehicleFinancing::default()
                })
            },
            "out_of_range",
            "/spending/vehicles/@car/financing/term_months",
        );
        bad(
            &|v| v.owner = Some(5),
            "ref_person",
            "/spending/vehicles/@car/owner",
        );
        bad(
            &|v| v.id = String::new(),
            "id_missing",
            "/spending/vehicles/0/id",
        );

        let mut p = Profile::example();
        p.spending.unemployment_cut = 1.5;
        let r = validate(&p);
        assert!(r
            .errors
            .iter()
            .any(|e| e.path == "/spending/unemployment_cut"));
    }

    #[test]
    fn implausible_values_warn() {
        let mut v = car();
        v.replace_every_years = 2.0;
        v.age_years = 5.0;
        v.trade_in_fraction = 0.8;
        let r = validate(&with_vehicle(v));
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
        for code in [
            "vehicle_cycle_unusual",
            "vehicle_overdue",
            "vehicle_trade_in_high",
        ] {
            assert!(r.warnings.iter().any(|w| w.code == code), "{code}");
        }
        let mut p = Profile::example();
        p.spending.unemployment_cut = 0.7;
        assert!(validate(&p)
            .warnings
            .iter()
            .any(|w| w.code == "unemployment_cut_high"));
    }

    #[test]
    fn detailed_shocks_warn_about_double_counted_home_upkeep() {
        let mut p = Profile::example();
        p.spending.shocks = crate::model::ShockLevel::Detailed;
        p.properties = vec![crate::model::Property {
            id: "home".into(),
            name: "Home".into(),
            usage: crate::model::PropertyUse::Primary,
            value: 400_000.0,
            maintenance_rate: 0.02,
            ..crate::model::Property::default()
        }];
        let r = validate(&p);
        assert!(r
            .warnings
            .iter()
            .any(|w| w.code == "detailed_shocks_home_maintenance"));
        p.properties[0].maintenance_rate = 0.01;
        assert!(!validate(&p)
            .warnings
            .iter()
            .any(|w| w.code == "detailed_shocks_home_maintenance"));
    }

    #[test]
    fn expensive_vehicles_against_a_transport_line_warn_of_double_counting() {
        let mut p = Profile::example();
        p.spending
            .items
            .retain(|i| i.category != crate::model::Category::Transport);
        p.spending.items.push(crate::model::BudgetItem {
            id: "transport".into(),
            category: crate::model::Category::Transport,
            amount: 700.0,
            ..crate::model::BudgetItem::default()
        });
        p.spending.vehicles = vec![car()];
        let r = validate(&p);
        // $40,000 * 0.8 / 10 = $3,200 a year against $8,400: under half, no warning.
        assert!(!r.warnings.iter().any(|w| w.code == "vehicles_double_count"));
        p.spending.vehicles[0].price = 80_000.0;
        let r = validate(&p);
        assert!(r.warnings.iter().any(|w| w.code == "vehicles_double_count"));
    }
}
