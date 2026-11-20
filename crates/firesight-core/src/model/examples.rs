//! Demo households. Fabricated but realistic; they exercise most of the schema and are
//! the fixtures for golden tests. `firesight examples <dir>` writes them to
//! `examples/*.json`.

use super::*;

/// A demo household with the text the UI shows on its card.
#[derive(Clone, Debug, PartialEq)]
pub struct Example {
    /// File stem and stable identifier, for example "dual-income-family".
    pub slug: &'static str,
    /// Card title.
    pub title: &'static str,
    /// One-sentence description.
    pub summary: &'static str,
    /// The profile.
    pub profile: Profile,
}

/// The `start` used for the checked-in `examples/*.json` files.
pub const EXAMPLE_START: YearMonth = YearMonth::ym(2026, 11);

impl Profile {
    /// The default demo household (the dual-income family), starting at
    /// [`EXAMPLE_START`].
    pub fn example() -> Profile {
        dual_income_family(EXAMPLE_START)
    }

    /// The three demo households with `start` set to the given month. Birth dates and
    /// event dates are placed relative to `start`, so the demos never go stale.
    pub fn examples(start: YearMonth) -> Vec<Example> {
        vec![
            Example {
                slug: "early-career-saver",
                title: "Early-career saver",
                summary: "27, single, Denver. A tech salary with RSUs, a student loan to \
                          clear and a first home on the calendar.",
                profile: early_career_saver(start),
            },
            Example {
                slug: "dual-income-family",
                title: "Dual-income family",
                summary: "37 and 36 in Austin with a toddler and a second child planned. Two \
                          salaries, two retirement plans, and a house to buy in two years.",
                profile: dual_income_family(start),
            },
            Example {
                slug: "side-hustle-landlord",
                title: "Side-hustle landlord",
                summary: "33, single, Tampa. A day job, a duplex, weekend gig work and a \
                          crypto sleeve, with a move to Lisbon in five years.",
                profile: side_hustle_landlord(start),
            },
        ]
    }
}

fn item(id: &str, name: &str, category: Category, amount: f64) -> BudgetItem {
    BudgetItem {
        id: id.into(),
        name: name.into(),
        category,
        amount,
        essential: category.default_essential(),
        ..BudgetItem::default()
    }
}

fn account(id: &str, name: &str, owner: usize, kind: AccountKind, balance: f64) -> Account {
    Account {
        id: id.into(),
        name: name.into(),
        owner,
        kind,
        balance,
        ..Account::default()
    }
}

fn early_career_saver(start: YearMonth) -> Profile {
    let maya = Person {
        id: "maya".into(),
        name: "Maya".into(),
        birth: start - (27 * 12 + 4),
        sex: Sex::Female,
        ..Person::default()
    };
    let job = Income {
        id: "maya_job".into(),
        name: "Software engineer".into(),
        owner: 0,
        kind: IncomeKind::Job(Job {
            salary: 108_000.0,
            growth: WageGrowth::Lifecycle {
                profile: LifecycleProfile::Steep,
                extra: 0.0,
            },
            risk: CareerRisk {
                level: RiskLevel::Typical,
            },
            bonus: Some(Bonus {
                target_fraction: 0.06,
                volatility: 0.4,
                month: 2,
            }),
            equity: vec![EquityGrant {
                id: "new_hire_rsu".into(),
                kind: EquityKind::Rsu,
                company: EquityCompany::Public {
                    volatility: 0.45,
                    beta: 1.3,
                },
                unvested_value: 60_000.0,
                vest_months_remaining: 36,
                cliff_months: 0,
                refresh_annual_value: 18_000.0,
                sell_on_vest: true,
            }],
            plan: Some(EmployerPlan {
                employee_fraction: 0.10,
                roth_fraction: 0.0,
                match_rate: 0.5,
                match_up_to: 0.06,
                ..EmployerPlan::default()
            }),
            hsa_contribution: 2_000.0,
            hsa_employer: 500.0,
            ..Job::default()
        }),
        ..Income::default()
    };
    let mut travel = item("travel", "Travel", Category::Travel, 3_000.0);
    travel.frequency = Frequency::Annual;
    let mut commute = item(
        "transport",
        "Car, fuel and transit",
        Category::Transport,
        260.0,
    );
    commute.in_retirement = RetirementChange::Factor { factor: 0.7 };

    Profile {
        name: "Early-career saver".into(),
        description: "Demo household: a single software engineer in Denver saving \
                      aggressively toward early retirement."
            .into(),
        start: Some(start),
        household: Household {
            people: vec![maya],
            children: vec![],
            location: Location {
                state: Some("CO".into()),
                city: Some("Denver".into()),
                ..Location::default()
            },
        },
        incomes: vec![job],
        spending: Spending {
            items: vec![
                item("rent", "Rent", Category::Rent, 1_650.0),
                item(
                    "utilities",
                    "Utilities and internet",
                    Category::Utilities,
                    160.0,
                ),
                item("groceries", "Groceries", Category::Groceries, 420.0),
                item("dining", "Restaurants", Category::Dining, 220.0),
                commute,
                item(
                    "car_insurance",
                    "Car and renter's insurance",
                    Category::Insurance,
                    125.0,
                ),
                item(
                    "entertainment",
                    "Entertainment",
                    Category::Entertainment,
                    150.0,
                ),
                item("gym", "Climbing gym", Category::Personal, 45.0),
                item(
                    "personal",
                    "Clothing and personal care",
                    Category::Personal,
                    120.0,
                ),
                travel,
            ],
            lifestyle_creep: 0.3,
            ..Spending::default()
        },
        accounts: vec![
            Account {
                cash_yield: CashYield::Fixed { rate: 0.0 },
                ..account("checking", "Checking", 0, AccountKind::Cash, 5_500.0)
            },
            account(
                "savings",
                "High-yield savings",
                0,
                AccountKind::Cash,
                12_000.0,
            ),
            account("k401", "401(k)", 0, AccountKind::Traditional, 26_000.0),
            Account {
                roth_basis: Some(7_500.0),
                ..account("roth_ira", "Roth IRA", 0, AccountKind::Roth, 9_000.0)
            },
            Account {
                cost_basis: Some(5_200.0),
                ..account("brokerage", "Brokerage", 0, AccountKind::Taxable, 6_000.0)
            },
            account("hsa", "HSA", 0, AccountKind::Hsa, 2_400.0),
        ],
        portfolio: PortfolioPolicy {
            target: Allocation::from_pairs(&[
                (AssetClass::UsStocks, 70.0),
                (AssetClass::IntlStocks, 20.0),
                (AssetClass::Bonds, 10.0),
            ]),
            emergency_months: 4.0,
            ..PortfolioPolicy::default()
        },
        debts: vec![Debt {
            id: "student_loan".into(),
            name: "Student loan".into(),
            kind: DebtKind::Student,
            balance: 18_500.0,
            rate: 0.05,
            payment: None,
            remaining_months: Some(96),
        }],
        debt_policy: DebtPolicy {
            strategy: DebtStrategy::Avalanche,
            extra_monthly: 200.0,
            ..DebtPolicy::default()
        },
        events: vec![LifeEvent {
            id: "first_home".into(),
            name: "Buy a first home".into(),
            when: When::date(start + 43),
            timing_uncertainty_years: 1.0,
            kind: EventKind::BuyHome(BuyHome {
                property_id: "home".into(),
                price: 420_000.0,
                down_payment_fraction: 0.2,
                property_tax_rate: Some(0.005),
                ..BuyHome::default()
            }),
            ..LifeEvent::default()
        }],
        retirement: RetirementPlan {
            // Decades from Social Security, she does not count on it: the target is
            // 3.5% of savings alone.
            trigger: RetirementTrigger::WhenFunded(WhenFunded {
                withdrawal_rate: 0.035,
                count_future_income: false,
                ..WhenFunded::default()
            }),
            ..RetirementPlan::default()
        },
        ..Profile::default()
    }
}

fn dual_income_family(start: YearMonth) -> Profile {
    let priya = Person {
        id: "priya".into(),
        name: "Priya".into(),
        birth: start - (37 * 12 + 2),
        sex: Sex::Female,
        ..Person::default()
    };
    let daniel = Person {
        id: "daniel".into(),
        name: "Daniel".into(),
        birth: start - (36 * 12 + 7),
        sex: Sex::Male,
        social_security: SocialSecurity {
            claim_age: 67.0,
            basis: SocialSecurityBasis::FromEarnings {
                career_start_age: 24.0,
            },
        },
        ..Person::default()
    };
    let college = College::PublicInState;
    let children = vec![
        Child {
            id: "anika".into(),
            name: "Anika".into(),
            birth: start - (3 * 12 + 1),
            costs: ChildCosts::Modeled,
            childcare_monthly: Some(1_400.0),
            college,
            college_share: 0.75,
        },
        Child {
            id: "second_child".into(),
            name: "Second child (planned)".into(),
            birth: start + 14,
            costs: ChildCosts::Modeled,
            childcare_monthly: Some(1_600.0),
            college,
            college_share: 0.75,
        },
    ];
    let pm_job = Income {
        id: "priya_job".into(),
        name: "Product manager".into(),
        owner: 0,
        kind: IncomeKind::Job(Job {
            salary: 128_000.0,
            bonus: Some(Bonus {
                target_fraction: 0.07,
                volatility: 0.35,
                month: 12,
            }),
            plan: Some(EmployerPlan {
                employee_fraction: 0.10,
                roth_fraction: 0.25,
                match_rate: 0.5,
                match_up_to: 0.06,
                ..EmployerPlan::default()
            }),
            hsa_contribution: 4_000.0,
            hsa_employer: 1_000.0,
            ..Job::default()
        }),
        ..Income::default()
    };
    let nurse_job = Income {
        id: "daniel_job".into(),
        name: "Registered nurse".into(),
        owner: 1,
        kind: IncomeKind::Job(Job {
            salary: 92_000.0,
            growth: WageGrowth::Lifecycle {
                profile: LifecycleProfile::Flat,
                extra: 0.0,
            },
            risk: CareerRisk {
                level: RiskLevel::Low,
            },
            plan: Some(EmployerPlan {
                employee_fraction: 0.08,
                roth_fraction: 0.0,
                match_rate: 1.0,
                match_up_to: 0.04,
                ..EmployerPlan::default()
            }),
            ..Job::default()
        }),
        ..Income::default()
    };
    let mut vacations = item("vacations", "Family vacations", Category::Travel, 7_000.0);
    vacations.frequency = Frequency::Annual;
    vacations.in_retirement = RetirementChange::Factor { factor: 1.5 };
    let mut gas = item(
        "transport",
        "Two cars: fuel and upkeep",
        Category::Transport,
        420.0,
    );
    gas.in_retirement = RetirementChange::Factor { factor: 0.6 };
    let home_month = start + 24;

    Profile {
        name: "Dual-income family".into(),
        description: "Demo household: two earners in Austin with a toddler, a second child \
                      planned, and a home purchase in two years."
            .into(),
        start: Some(start),
        household: Household {
            people: vec![priya, daniel],
            children,
            location: Location {
                state: Some("TX".into()),
                city: Some("Austin".into()),
                ..Location::default()
            },
        },
        incomes: vec![pm_job, nurse_job],
        spending: Spending {
            items: vec![
                item("rent", "Rent", Category::Rent, 2_400.0),
                item(
                    "utilities",
                    "Utilities, internet and phones",
                    Category::Utilities,
                    340.0,
                ),
                item("groceries", "Groceries", Category::Groceries, 1_150.0),
                item("dining", "Restaurants", Category::Dining, 380.0),
                gas,
                item(
                    "insurance",
                    "Car, life and umbrella insurance",
                    Category::Insurance,
                    310.0,
                ),
                item(
                    "clothing",
                    "Clothing and personal care",
                    Category::Personal,
                    220.0,
                ),
                item(
                    "entertainment",
                    "Entertainment and hobbies",
                    Category::Entertainment,
                    250.0,
                ),
                item("giving", "Charity and gifts", Category::Giving, 150.0),
                vacations,
            ],
            ..Spending::default()
        },
        accounts: vec![
            Account {
                cash_yield: CashYield::Fixed { rate: 0.0 },
                ..account("checking", "Joint checking", 0, AccountKind::Cash, 14_000.0)
            },
            account(
                "savings",
                "Savings (emergency and down payment)",
                0,
                AccountKind::Cash,
                96_000.0,
            ),
            account(
                "priya_401k",
                "Priya's 401(k)",
                0,
                AccountKind::Traditional,
                150_000.0,
            ),
            Account {
                roth_basis: Some(19_000.0),
                ..account(
                    "priya_roth",
                    "Priya's Roth 401(k) and IRA",
                    0,
                    AccountKind::Roth,
                    31_000.0,
                )
            },
            account(
                "daniel_403b",
                "Daniel's 403(b)",
                1,
                AccountKind::Traditional,
                70_000.0,
            ),
            Account {
                cost_basis: Some(51_000.0),
                ..account(
                    "brokerage",
                    "Joint brokerage",
                    0,
                    AccountKind::Taxable,
                    68_000.0,
                )
            },
            account("hsa", "HSA", 0, AccountKind::Hsa, 9_000.0),
        ],
        portfolio: PortfolioPolicy {
            target: Allocation::from_pairs(&[
                (AssetClass::UsStocks, 55.0),
                (AssetClass::IntlStocks, 20.0),
                (AssetClass::Bonds, 20.0),
                (AssetClass::RealEstate, 5.0),
            ]),
            glide: GlidePath::TargetDate {
                years_before: 15.0,
                equity_at_retirement: 0.6,
                years_after: 7.0,
                equity_final: 0.45,
            },
            location: AssetLocation::TaxEfficient,
            ..PortfolioPolicy::default()
        },
        debts: vec![Debt {
            id: "minivan".into(),
            name: "Minivan loan".into(),
            kind: DebtKind::Auto,
            balance: 21_000.0,
            rate: 0.065,
            payment: None,
            remaining_months: Some(48),
        }],
        events: vec![
            LifeEvent {
                id: "buy_home".into(),
                name: "Buy a house".into(),
                when: When::date(home_month),
                timing_uncertainty_years: 0.5,
                kind: EventKind::BuyHome(BuyHome {
                    property_id: "home".into(),
                    price: 520_000.0,
                    down_payment_fraction: 0.15,
                    property_tax_rate: Some(0.018),
                    ..BuyHome::default()
                }),
                ..LifeEvent::default()
            },
            LifeEvent {
                id: "parental_leave".into(),
                name: "Daniel's parental leave".into(),
                when: When::date(start + 14),
                kind: EventKind::CareerBreak {
                    person: 1,
                    months: 4,
                    income_fraction: 0.6,
                },
                ..LifeEvent::default()
            },
            LifeEvent {
                id: "kitchen".into(),
                name: "Kitchen remodel".into(),
                when: When::date(home_month + 30),
                probability: 0.8,
                timing_uncertainty_years: 1.0,
                kind: EventKind::Expense {
                    amount: 45_000.0,
                    financed: Some(Financing {
                        down_fraction: 0.33,
                        rate: 0.075,
                        term_months: 60,
                    }),
                },
            },
            LifeEvent {
                id: "inheritance".into(),
                name: "Possible inheritance".into(),
                when: When::age(0, 58.0),
                probability: 0.5,
                timing_uncertainty_years: 5.0,
                kind: EventKind::Windfall {
                    amount: 120_000.0,
                    taxable: false,
                },
            },
        ],
        retirement: RetirementPlan {
            trigger: RetirementTrigger::WhenFunded(WhenFunded {
                withdrawal_rate: 0.035,
                min_age: Some(50.0),
                max_age: 67.0,
                ..WhenFunded::default()
            }),
            partner_offset_years: 0.0,
            spending_rule: SpendingRule::Guardrails {
                upper: 0.2,
                lower: 0.2,
                adjust: 0.1,
            },
            ..RetirementPlan::default()
        },
        ..Profile::default()
    }
}

fn side_hustle_landlord(start: YearMonth) -> Profile {
    let sam = Person {
        id: "sam".into(),
        name: "Sam".into(),
        birth: start - (33 * 12 + 9),
        sex: Sex::Male,
        ..Person::default()
    };
    let move_month = start + 58;
    let job = Income {
        id: "sam_job".into(),
        name: "Logistics analyst".into(),
        owner: 0,
        kind: IncomeKind::Job(Job {
            salary: 78_000.0,
            plan: Some(EmployerPlan {
                employee_fraction: 0.08,
                roth_fraction: 0.5,
                match_rate: 1.0,
                match_up_to: 0.03,
                ..EmployerPlan::default()
            }),
            ..Job::default()
        }),
        ..Income::default()
    };
    let delivery = Income {
        id: "delivery".into(),
        name: "Weekend food delivery".into(),
        owner: 0,
        kind: IncomeKind::SelfEmployment(SelfEmployment {
            net_annual: 4_750.0,
            volatility: 0.3,
            ..SelfEmployment::default()
        }),
        end: Some(When::date(move_month)),
        ..Income::default()
    };
    let consulting = Income {
        id: "consulting".into(),
        name: "Remote supply-chain consulting".into(),
        owner: 0,
        kind: IncomeKind::SelfEmployment(SelfEmployment {
            net_annual: 48_000.0,
            volatility: 0.35,
            real_growth: 0.01,
            ends_at_retirement: true,
            solo_401k_fraction: 0.0,
        }),
        start: Some(When::date(move_month)),
        ..Income::default()
    };
    let mut health = item(
        "health",
        "Health insurance share and copays",
        Category::Healthcare,
        210.0,
    );
    health.in_retirement = RetirementChange::Amount { amount: 260.0 };
    let duplex = Property {
        id: "duplex".into(),
        name: "Seminole Heights duplex".into(),
        usage: PropertyUse::Rental(RentalTerms {
            monthly_rent: 3_100.0,
            vacancy: 0.06,
            management_fee: 0.0,
            rent_real_growth: 0.0,
            land_fraction: 0.25,
            years_in_service: 4.0,
        }),
        value: 385_000.0,
        basis: Some(310_000.0),
        mortgage: Some(Mortgage {
            balance: 268_000.0,
            rate: 0.0425,
            remaining_months: 312,
            payment: None,
            ..Mortgage::default()
        }),
        property_tax_rate: Some(0.011),
        insurance_rate: 0.011,
        maintenance_rate: 0.012,
        ..Property::default()
    };

    Profile {
        name: "Side-hustle landlord".into(),
        description: "Demo household: a single renter in Tampa who owns a duplex, does gig \
                      work, holds some crypto and plans to move to Lisbon."
            .into(),
        start: Some(start),
        household: Household {
            people: vec![sam],
            children: vec![],
            location: Location {
                state: Some("FL".into()),
                city: Some("Tampa".into()),
                ..Location::default()
            },
        },
        incomes: vec![job, delivery, consulting],
        spending: Spending {
            items: vec![
                item("rent", "Rent", Category::Rent, 1_450.0),
                item(
                    "utilities",
                    "Utilities and internet",
                    Category::Utilities,
                    190.0,
                ),
                item("groceries", "Groceries", Category::Groceries, 450.0),
                item("dining", "Restaurants", Category::Dining, 260.0),
                item("transport", "Car and fuel", Category::Transport, 330.0),
                item("insurance", "Car insurance", Category::Insurance, 140.0),
                health,
                item(
                    "entertainment",
                    "Entertainment",
                    Category::Entertainment,
                    160.0,
                ),
                item("personal", "Personal", Category::Personal, 110.0),
            ],
            // Healthcare is a budget line here because the plan is to live in Portugal,
            // outside the US insurance phases the engine models.
            healthcare: Healthcare {
                mode: HealthcareMode::InBudget,
                early_retirement: EarlyRetirementCoverage::Custom { annual: 3_100.0 },
                long_term_care: true,
                quality: PlanTier::Silver,
            },
            lifestyle_creep: 0.2,
            ..Spending::default()
        },
        accounts: vec![
            Account {
                cash_yield: CashYield::Fixed { rate: 0.0 },
                ..account("checking", "Checking", 0, AccountKind::Cash, 7_000.0)
            },
            account(
                "savings",
                "High-yield savings",
                0,
                AccountKind::Cash,
                18_000.0,
            ),
            account("k401", "401(k)", 0, AccountKind::Traditional, 34_000.0),
            Account {
                roth_basis: Some(22_000.0),
                ..account(
                    "roth",
                    "Roth IRA and Roth 401(k)",
                    0,
                    AccountKind::Roth,
                    30_000.0,
                )
            },
            Account {
                cost_basis: Some(6_000.0),
                holdings: Some(Allocation::single(AssetClass::Crypto)),
                ..account("crypto", "Crypto wallet", 0, AccountKind::Taxable, 9_000.0)
            },
            Account {
                cost_basis: Some(10_500.0),
                ..account("brokerage", "Brokerage", 0, AccountKind::Taxable, 12_000.0)
            },
        ],
        portfolio: PortfolioPolicy {
            target: Allocation::from_pairs(&[
                (AssetClass::UsStocks, 60.0),
                (AssetClass::IntlStocks, 20.0),
                (AssetClass::Bonds, 10.0),
                (AssetClass::RealEstate, 5.0),
                (AssetClass::Crypto, 5.0),
            ]),
            rebalance: Rebalance::Bands { width: 0.05 },
            ..PortfolioPolicy::default()
        },
        properties: vec![duplex],
        debts: vec![Debt {
            id: "credit_card".into(),
            name: "Credit card".into(),
            kind: DebtKind::CreditCard,
            balance: 2_400.0,
            rate: 0.22,
            payment: Some(250.0),
            remaining_months: None,
        }],
        debt_policy: DebtPolicy {
            strategy: DebtStrategy::Avalanche,
            extra_monthly: 150.0,
            ..DebtPolicy::default()
        },
        events: vec![
            LifeEvent {
                id: "move_to_lisbon".into(),
                name: "Move to Lisbon".into(),
                when: When::date(move_month),
                timing_uncertainty_years: 1.0,
                kind: EventKind::Relocate(Relocate {
                    country: "PT".into(),
                    state: None,
                    city: Some("Lisbon".into()),
                    foreign_tax_rate: Some(0.2),
                    moving_cost: 9_000.0,
                    cost_of_living: CostOfLivingChange::Factor { overall: 0.85 },
                }),
                ..LifeEvent::default()
            },
            LifeEvent {
                id: "quit_day_job".into(),
                name: "Leave the day job".into(),
                when: When::date(move_month),
                kind: EventKind::IncomeChange {
                    income: "sam_job".into(),
                    change: IncomeChangeKind::End,
                },
                ..LifeEvent::default()
            },
        ],
        retirement: RetirementPlan {
            trigger: RetirementTrigger::WhenFunded(WhenFunded::default()),
            spending_rule: SpendingRule::VanguardDynamic {
                ceiling: 0.05,
                floor: 0.025,
                rate: None,
            },
            ..RetirementPlan::default()
        },
        ..Profile::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_round_trip_through_json() {
        for ex in Profile::examples(EXAMPLE_START) {
            let json = ex.profile.to_json_pretty();
            let back = Profile::from_json(&json).unwrap_or_else(|e| panic!("{}: {e}", ex.slug));
            assert_eq!(back, ex.profile, "{}", ex.slug);
        }
    }

    #[test]
    fn examples_validate_cleanly() {
        for ex in Profile::examples(EXAMPLE_START) {
            let report = crate::validate::validate(&ex.profile);
            assert!(report.errors.is_empty(), "{}: {:?}", ex.slug, report.errors);
        }
    }

    /// The files in `examples/` are generated by `firesight examples examples`; this
    /// fails when the Rust definitions change without regenerating them.
    #[test]
    fn checked_in_example_files_are_current() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
        for ex in Profile::examples(EXAMPLE_START) {
            let path = format!("{dir}/{}.json", ex.slug);
            let on_disk = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!("{path}: {e}; run `cargo run -p firesight-cli -- examples examples`")
            });
            assert_eq!(
                on_disk.trim_end(),
                ex.profile.to_json_pretty(),
                "{path} is stale; run `cargo run -p firesight-cli -- examples examples`"
            );
        }
    }
}
