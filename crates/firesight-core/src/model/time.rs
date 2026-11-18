//! Calendar months and symbolic dates.

use std::borrow::Cow;
use std::fmt;
use std::ops::{Add, AddAssign, Sub};
use std::str::FromStr;

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::MAX_PEOPLE;

/// A calendar month, written `"YYYY-MM"` in JSON (for example `"2031-09"`).
///
/// Stored as months since year 0, so ordering and month arithmetic are integer
/// operations and ages never accumulate floating-point error.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct YearMonth(i32);

impl YearMonth {
    /// Builds a month from a calendar year and a month in `1..=12`.
    pub const fn new(year: i32, month: u32) -> Option<Self> {
        if month >= 1 && month <= 12 && year >= 0 && year <= 9999 {
            Some(Self(year * 12 + month as i32 - 1))
        } else {
            None
        }
    }

    /// Like [`YearMonth::new`] but panics on an invalid month. For constants and tests.
    pub const fn ym(year: i32, month: u32) -> Self {
        match Self::new(year, month) {
            Some(v) => v,
            None => panic!("invalid year-month"),
        }
    }

    /// Calendar year.
    pub const fn year(self) -> i32 {
        self.0.div_euclid(12)
    }

    /// Month of the year, `1..=12`.
    pub const fn month(self) -> u32 {
        self.0.rem_euclid(12) as u32 + 1
    }

    /// Months since year 0; the raw ordinal.
    pub const fn ordinal(self) -> i32 {
        self.0
    }

    /// Inverse of [`YearMonth::ordinal`].
    pub const fn from_ordinal(ordinal: i32) -> Self {
        Self(ordinal)
    }

    /// This month shifted by `months` (may be negative).
    pub const fn add_months(self, months: i32) -> Self {
        Self(self.0.saturating_add(months))
    }

    /// This month shifted by a fractional number of years, rounded to the nearest month.
    pub fn add_years_f64(self, years: f64) -> Self {
        // The cast saturates; so must the addition (an age of 1e9 is a validation
        // error to report, not an overflow).
        Self(self.0.saturating_add((years * 12.0).round() as i32))
    }

    /// Whole months from `self` to `later` (negative if `later` is earlier).
    pub const fn months_until(self, later: YearMonth) -> i32 {
        later.0 - self.0
    }

    /// Age in whole months at `now` of someone born in `self` (negative before birth).
    pub const fn age_months_at(self, now: YearMonth) -> i32 {
        now.0 - self.0
    }

    /// Age in years (fractional, in twelfths) at `now` of someone born in `self`.
    pub fn age_years_at(self, now: YearMonth) -> f64 {
        f64::from(now.0 - self.0) / 12.0
    }

    /// The month in which someone born in `self` reaches `age` years (nearest month).
    pub fn at_age(self, age: f64) -> Self {
        self.add_years_f64(age)
    }
}

impl Default for YearMonth {
    /// January 2000. Only used where a struct needs a placeholder; every user-facing
    /// date is either required or optional in the schema.
    fn default() -> Self {
        Self::ym(2000, 1)
    }
}

impl Add<i32> for YearMonth {
    type Output = YearMonth;
    fn add(self, months: i32) -> YearMonth {
        self.add_months(months)
    }
}

impl AddAssign<i32> for YearMonth {
    fn add_assign(&mut self, months: i32) {
        self.0 += months;
    }
}

impl Sub<i32> for YearMonth {
    type Output = YearMonth;
    fn sub(self, months: i32) -> YearMonth {
        self.add_months(-months)
    }
}

impl Sub<YearMonth> for YearMonth {
    type Output = i32;
    /// Months between two dates: `later - earlier` is positive.
    fn sub(self, earlier: YearMonth) -> i32 {
        self.0 - earlier.0
    }
}

impl fmt::Display for YearMonth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year(), self.month())
    }
}

/// Error returned when a string is not a `YYYY-MM` month.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid month {0:?}: expected \"YYYY-MM\" such as \"2031-09\"")]
pub struct ParseYearMonthError(pub String);

impl FromStr for YearMonth {
    type Err = ParseYearMonthError;

    /// Accepts `YYYY-MM` and, for convenience when importing, `YYYY-MM-DD` (the day is
    /// dropped).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseYearMonthError(s.to_string());
        let mut parts = s.trim().split('-');
        let (y, m) = (parts.next().ok_or_else(err)?, parts.next().ok_or_else(err)?);
        if let Some(day) = parts.next() {
            if day.len() != 2 || day.parse::<u32>().is_err() || parts.next().is_some() {
                return Err(err());
            }
        }
        if y.len() != 4 || m.len() != 2 {
            return Err(err());
        }
        let year: i32 = y.parse().map_err(|_| err())?;
        let month: u32 = m.parse().map_err(|_| err())?;
        YearMonth::new(year, month).ok_or_else(err)
    }
}

impl Serialize for YearMonth {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for YearMonth {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = Cow::<str>::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for YearMonth {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "YearMonth".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^[0-9]{4}-(0[1-9]|1[0-2])$",
            "description": "A calendar month written as YYYY-MM, for example \"2031-09\".",
            "examples": ["2031-09"]
        })
    }
}

/// A point in time that may depend on the plan: a fixed month, somebody's age, or the
/// (possibly simulated) retirement date.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum When {
    /// A fixed calendar month.
    Date {
        /// The month, as "YYYY-MM".
        at: YearMonth,
    },
    /// The month a household member reaches an age.
    Age {
        /// Which person: 0 is the first person in `household.people`, 1 the second.
        #[serde(default)]
        person: usize,
        /// Age in years; fractions are allowed (59.5, 66.5).
        age: f64,
    },
    /// Relative to the household's retirement date, which is simulated when the
    /// retirement trigger is `when_funded` and so can differ on every path.
    Retirement {
        /// Years after retirement (negative means before). 0 is the retirement month.
        #[serde(default)]
        offset_years: f64,
    },
    /// The first simulated month (`profile.start`).
    #[default]
    Start,
}

/// The facts needed to turn a [`When`] into a month.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WhenContext {
    /// First simulated month.
    pub start: YearMonth,
    /// Birth month of each person; `None` for a slot with no person.
    pub births: [Option<YearMonth>; MAX_PEOPLE],
    /// The household retirement month if it is known. `None` while a `when_funded`
    /// trigger has not fired yet on this path.
    pub retirement: Option<YearMonth>,
}

impl When {
    /// Shorthand for `When::Date`.
    pub const fn date(at: YearMonth) -> Self {
        When::Date { at }
    }

    /// Shorthand for `When::Age`.
    pub const fn age(person: usize, age: f64) -> Self {
        When::Age { person, age }
    }

    /// Shorthand for `When::Retirement`.
    pub const fn retirement(offset_years: f64) -> Self {
        When::Retirement { offset_years }
    }

    /// True if the date depends on the retirement month.
    pub const fn depends_on_retirement(&self) -> bool {
        matches!(self, When::Retirement { .. })
    }

    /// Resolves to a month. Returns `None` when the answer is not knowable yet
    /// (a retirement-relative date before retirement is known) or refers to a person
    /// who does not exist. A retirement-relative date with a negative offset cannot
    /// be honoured exactly when retirement is simulated; callers treat it as
    /// "unknown until retirement", at which point it resolves to a month that may
    /// already have passed and takes effect immediately.
    pub fn resolve(&self, ctx: &WhenContext) -> Option<YearMonth> {
        match *self {
            When::Date { at } => Some(at),
            When::Age { person, age } => ctx
                .births
                .get(person)
                .copied()
                .flatten()
                .map(|b| b.at_age(age)),
            When::Retirement { offset_years } => {
                ctx.retirement.map(|r| r.add_years_f64(offset_years))
            }
            When::Start => Some(ctx.start),
        }
    }

    /// Resolves to a month index relative to `ctx.start` (0 = first simulated month,
    /// negative = already in the past).
    pub fn resolve_month_index(&self, ctx: &WhenContext) -> Option<i32> {
        self.resolve(ctx).map(|ym| ym - ctx.start)
    }
}

/// Whether a `[start, end)` window is open in month `now`. `None` bounds are open:
/// no start means "already running", no end means "never stops". A bound that cannot
/// be resolved yet (retirement-relative, before retirement) counts as not reached.
pub fn window_active(
    start: Option<&When>,
    end: Option<&When>,
    now: YearMonth,
    ctx: &WhenContext,
) -> bool {
    let started = match start {
        None => true,
        Some(w) => w.resolve(ctx).is_some_and(|s| now >= s),
    };
    let ended = match end {
        None => false,
        Some(w) => w.resolve(ctx).is_some_and(|e| now >= e),
    };
    started && !ended
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_format_round_trip() {
        let ym: YearMonth = "2031-09".parse().unwrap();
        assert_eq!((ym.year(), ym.month()), (2031, 9));
        assert_eq!(ym.to_string(), "2031-09");
        assert_eq!("2031-09-01".parse::<YearMonth>().unwrap(), ym);
        for bad in [
            "2031-13",
            "2031-00",
            "31-09",
            "2031/09",
            "2031-9",
            "",
            "2031-09-1x",
        ] {
            assert!(bad.parse::<YearMonth>().is_err(), "{bad}");
        }
    }

    #[test]
    fn ordering_and_arithmetic() {
        let a = YearMonth::ym(2026, 11);
        let b = a + 3;
        assert_eq!(b, YearMonth::ym(2027, 2));
        assert!(b > a);
        assert_eq!(b - a, 3);
        assert_eq!(a - 11, YearMonth::ym(2025, 12));
        assert_eq!(a.months_until(b), 3);
        assert_eq!(
            YearMonth::ym(1990, 5).age_years_at(YearMonth::ym(2026, 11)),
            36.5
        );
        assert_eq!(YearMonth::ym(1990, 5).at_age(59.5), YearMonth::ym(2049, 11));
    }

    #[test]
    fn serde_as_string() {
        let ym = YearMonth::ym(2026, 1);
        assert_eq!(serde_json::to_string(&ym).unwrap(), "\"2026-01\"");
        assert_eq!(
            serde_json::from_str::<YearMonth>("\"2026-01\"").unwrap(),
            ym
        );
        assert!(serde_json::from_str::<YearMonth>("\"Jan 2026\"").is_err());
    }

    #[test]
    fn when_resolution() {
        let ctx = WhenContext {
            start: YearMonth::ym(2026, 1),
            births: [Some(YearMonth::ym(1990, 1)), None],
            retirement: None,
        };
        assert_eq!(When::Start.resolve(&ctx), Some(ctx.start));
        assert_eq!(
            When::age(0, 40.0).resolve(&ctx),
            Some(YearMonth::ym(2030, 1))
        );
        assert_eq!(When::age(1, 40.0).resolve(&ctx), None);
        assert_eq!(When::retirement(0.0).resolve(&ctx), None);
        let retired = WhenContext {
            retirement: Some(YearMonth::ym(2040, 6)),
            ..ctx
        };
        assert_eq!(
            When::retirement(1.5).resolve(&retired),
            Some(YearMonth::ym(2041, 12))
        );
        assert_eq!(
            When::date(YearMonth::ym(2027, 1)).resolve_month_index(&ctx),
            Some(12)
        );

        let end = When::retirement(0.0);
        assert!(window_active(
            None,
            Some(&end),
            YearMonth::ym(2039, 1),
            &ctx
        ));
        assert!(!window_active(
            None,
            Some(&end),
            YearMonth::ym(2041, 1),
            &retired
        ));
        assert!(!window_active(
            Some(&end),
            None,
            YearMonth::ym(2039, 1),
            &ctx
        ));
    }

    #[test]
    fn when_json_shape() {
        let w: When = serde_json::from_str(r#"{"type":"age","person":1,"age":65}"#).unwrap();
        assert_eq!(w, When::age(1, 65.0));
        assert_eq!(
            serde_json::to_string(&When::Start).unwrap(),
            r#"{"type":"start"}"#
        );
        assert!(serde_json::from_str::<When>(r#"{"type":"date","at":"2030-01","x":1}"#).is_err());
    }
}
