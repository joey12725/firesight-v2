//! Profiles with the household-behaviour features (`sim/behavior`) switched on, for
//! the conservation, finite-value and no-allocation tests.
#![allow(dead_code)]

use crate::common::*;
use firesight_core::model::*;

/// A 58-year-old earning $90,000 who must stop at 60 with savings that cannot carry
/// a $60,000 budget: the plan is well short when it begins.
pub fn underfunded() -> Profile {
    let mut who = person("worker", 58, Sex::Female);
    who.social_security.claim_age = 67.0;
    let mut p = Profile {
        name: "underfunded".into(),
        start: Some(START),
        household: Household {
            people: vec![who],
            location: Location {
                state: Some("TX".into()),
                ..Location::default()
            },
            ..Household::default()
        },
        incomes: vec![job("job", 0, 90_000.0)],
        accounts: vec![
            account("cash", 0, AccountKind::Cash, 20_000.0),
            account("ira", 0, AccountKind::Traditional, 420_000.0),
            account("brokerage", 0, AccountKind::Taxable, 120_000.0),
        ],
        ..Profile::default()
    };
    p.spending.items = vec![
        item("home", Category::Rent, 2_200.0, true),
        item("food", Category::Groceries, 900.0, true),
        item("other", Category::Other, 1_900.0, false),
    ];
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 60.0,
    };
    p.simulation.paths = 400;
    p
}

/// `p` with every behaviour feature on, set so that it acts often.
pub fn with_behaviors(mut p: Profile) -> Profile {
    p.retirement.return_to_work = Some(ReturnToWork {
        funded_ratio_below: 0.9,
        ..ReturnToWork::default()
    });
    p.portfolio.behavior = InvestorBehavior::PanicProne(PanicSelling {
        probability: 0.5,
        ..PanicSelling::default()
    });
    p.portfolio.pause_deferrals_in_distress = true;
    for person in &mut p.household.people {
        person.social_security.claiming = Claiming::Adaptive {
            earliest: 62.0,
            latest: 70.0,
        };
    }
    p
}

/// The demo households and the underfunded household, every behaviour on.
pub fn behavior_profiles() -> Vec<Profile> {
    let mut v: Vec<Profile> = Profile::examples(START)
        .into_iter()
        .map(|e| with_behaviors(e.profile))
        .collect();
    v.push(with_behaviors(underfunded()));
    v
}
