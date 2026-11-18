//! Asset classes, allocations and the investment policy.

use std::borrow::Cow;
use std::fmt;
use std::ops::{Index, IndexMut};

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Number of asset classes. Balances and returns are `[f64; N_ASSETS]` indexed by
/// [`AssetClass::index`].
pub const N_ASSETS: usize = 9;

/// An investable asset class.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AssetClass {
    /// US total stock market.
    UsStocks,
    /// Developed-market stocks outside the US.
    IntlStocks,
    /// Intermediate-term US government and investment-grade bonds.
    Bonds,
    /// Treasury bills, money-market funds and savings.
    Cash,
    /// Listed real estate (REITs). A home or rental you own is a property, not this.
    RealEstate,
    /// Gold.
    Gold,
    /// Cryptocurrency, modelled as an extremely volatile asset with a modest expected
    /// return.
    Crypto,
    /// A concentrated position in one company's stock, configured in
    /// `portfolio.single_stock`.
    SingleStock,
    /// A user-defined asset configured in `portfolio.custom_asset`.
    Custom,
}

impl AssetClass {
    /// All asset classes in index order; `ALL[a.index()] == a`.
    pub const ALL: [AssetClass; N_ASSETS] = [
        AssetClass::UsStocks,
        AssetClass::IntlStocks,
        AssetClass::Bonds,
        AssetClass::Cash,
        AssetClass::RealEstate,
        AssetClass::Gold,
        AssetClass::Crypto,
        AssetClass::SingleStock,
        AssetClass::Custom,
    ];

    /// Dense index into `[f64; N_ASSETS]`.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The stable snake_case name used in JSON and in output column names.
    pub const fn key(self) -> &'static str {
        match self {
            AssetClass::UsStocks => "us_stocks",
            AssetClass::IntlStocks => "intl_stocks",
            AssetClass::Bonds => "bonds",
            AssetClass::Cash => "cash",
            AssetClass::RealEstate => "real_estate",
            AssetClass::Gold => "gold",
            AssetClass::Crypto => "crypto",
            AssetClass::SingleStock => "single_stock",
            AssetClass::Custom => "custom",
        }
    }

    /// Inverse of [`AssetClass::key`].
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == key)
    }

    /// Defensive assets (bonds and cash). Everything else is a growth asset; glide
    /// paths move weight between the two groups.
    pub const fn is_defensive(self) -> bool {
        matches!(self, AssetClass::Bonds | AssetClass::Cash)
    }
}

/// Portfolio weights by asset class.
///
/// In JSON this is an object such as `{"us_stocks": 60, "intl_stocks": 20, "bonds": 20}`.
/// Weights are relative: they are divided by their sum, so percentages and fractions
/// both work. Internally a fixed array indexed by [`AssetClass`], so the engine never
/// touches a map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Allocation(pub [f64; N_ASSETS]);

impl Allocation {
    /// All weights zero.
    pub const ZERO: Allocation = Allocation([0.0; N_ASSETS]);

    /// Builds an allocation from `(asset, weight)` pairs; unlisted assets get 0.
    pub fn from_pairs(pairs: &[(AssetClass, f64)]) -> Self {
        let mut a = Self::ZERO;
        for &(asset, w) in pairs {
            a.0[asset.index()] += w;
        }
        a
    }

    /// Everything in one asset.
    pub fn single(asset: AssetClass) -> Self {
        Self::from_pairs(&[(asset, 1.0)])
    }

    /// Sum of the raw weights.
    pub fn total(&self) -> f64 {
        self.0.iter().sum()
    }

    /// Weights scaled to sum to 1. Returns `None` if the sum is not positive or any
    /// weight is negative or not finite (validation reports these).
    pub fn normalized(&self) -> Option<[f64; N_ASSETS]> {
        let total = self.total();
        if !total.is_finite() || total <= 0.0 || self.0.iter().any(|w| !w.is_finite() || *w < 0.0) {
            return None;
        }
        let mut out = self.0;
        for w in &mut out {
            *w /= total;
        }
        Some(out)
    }

    /// Normalised weight of one asset (0 if the allocation is invalid).
    pub fn weight(&self, asset: AssetClass) -> f64 {
        self.normalized().map_or(0.0, |w| w[asset.index()])
    }

    /// Normalised share in growth assets (everything except bonds and cash).
    pub fn growth_share(&self) -> f64 {
        self.normalized().map_or(0.0, |w| {
            AssetClass::ALL
                .iter()
                .filter(|a| !a.is_defensive())
                .map(|a| w[a.index()])
                .sum()
        })
    }

    /// `(asset, raw weight)` for the non-zero entries, in index order.
    pub fn iter_nonzero(&self) -> impl Iterator<Item = (AssetClass, f64)> + '_ {
        AssetClass::ALL
            .into_iter()
            .map(|a| (a, self.0[a.index()]))
            .filter(|(_, w)| *w != 0.0)
    }
}

impl Default for Allocation {
    /// 60% US stocks, 20% international stocks, 20% bonds.
    fn default() -> Self {
        Self::from_pairs(&[
            (AssetClass::UsStocks, 60.0),
            (AssetClass::IntlStocks, 20.0),
            (AssetClass::Bonds, 20.0),
        ])
    }
}

impl Index<AssetClass> for Allocation {
    type Output = f64;
    fn index(&self, asset: AssetClass) -> &f64 {
        &self.0[asset.index()]
    }
}

impl IndexMut<AssetClass> for Allocation {
    fn index_mut(&mut self, asset: AssetClass) -> &mut f64 {
        &mut self.0[asset.index()]
    }
}

impl Serialize for Allocation {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        for (asset, w) in self.iter_nonzero() {
            map.serialize_entry(asset.key(), &w)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Allocation {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Allocation;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object mapping asset class names to weights")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Allocation, A::Error> {
                let mut out = Allocation::ZERO;
                let mut seen = [false; N_ASSETS];
                while let Some(key) = map.next_key::<Cow<'de, str>>()? {
                    let asset = AssetClass::from_key(&key).ok_or_else(|| {
                        let valid: Vec<&str> = AssetClass::ALL.iter().map(|a| a.key()).collect();
                        serde::de::Error::custom(format!(
                            "unknown asset class `{key}`, expected one of: {}",
                            valid.join(", ")
                        ))
                    })?;
                    if std::mem::replace(&mut seen[asset.index()], true) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate asset class `{key}`"
                        )));
                    }
                    out.0[asset.index()] = map.next_value()?;
                }
                Ok(out)
            }
        }
        d.deserialize_map(V)
    }
}

impl JsonSchema for Allocation {
    fn schema_name() -> Cow<'static, str> {
        "Allocation".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let weight =
            |desc: &str| serde_json::json!({ "type": "number", "minimum": 0, "description": desc });
        json_schema!({
            "type": "object",
            "description": "Portfolio weights by asset class. Weights are relative and are \
                divided by their sum, so {\"us_stocks\": 60, \"bonds\": 40} and \
                {\"us_stocks\": 0.6, \"bonds\": 0.4} mean the same. Omitted assets are 0.",
            "additionalProperties": false,
            "properties": {
                "us_stocks": weight("US total stock market."),
                "intl_stocks": weight("Developed-market stocks outside the US."),
                "bonds": weight("Intermediate-term US government and investment-grade bonds."),
                "cash": weight("Treasury bills, money-market funds and savings."),
                "real_estate": weight("Listed real estate funds (REITs)."),
                "gold": weight("Gold."),
                "crypto": weight("Cryptocurrency."),
                "single_stock": weight("One company's stock, configured in portfolio.single_stock."),
                "custom": weight("The user-defined asset in portfolio.custom_asset.")
            }
        })
    }
}

/// How the household invests.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PortfolioPolicy {
    /// Target mix for invested money across all accounts. Cash accounts and the
    /// emergency fund are held on top of this.
    pub target: Allocation,
    /// Whether the mix shifts toward bonds as retirement approaches.
    pub glide: GlidePath,
    /// When the portfolio is brought back to its target mix.
    pub rebalance: Rebalance,
    /// Whether every account holds the same mix or tax-inefficient assets are placed in
    /// tax-sheltered accounts first.
    pub location: AssetLocation,
    /// Average annual fund expense ratio as a decimal (0.001 = 0.10%, typical for
    /// index funds; 0.0075 for actively managed funds).
    pub expense_ratio: f64,
    /// Annual fee paid to a financial adviser as a decimal of assets (0.01 = 1%).
    /// 0 if self-managed.
    pub advisory_fee: f64,
    /// Emergency fund target in months of current spending, held in cash before
    /// anything is invested. Recomputed every year. Typical value 3 to 6.
    pub emergency_months: f64,
    /// Years of spending held in cash during retirement (a "bucket" strategy), from 0
    /// to 5. The bucket is drawn down when stocks are in a drawdown and refilled when
    /// they are not. 0 disables it.
    pub retirement_cash_years: f64,
    /// Definition of the "custom" asset class, if the allocation uses it.
    pub custom_asset: Option<CustomAsset>,
    /// Risk of the "single_stock" asset class (employer stock or a concentrated holding).
    pub single_stock: SingleStock,
}

impl Default for PortfolioPolicy {
    fn default() -> Self {
        Self {
            target: Allocation::default(),
            glide: GlidePath::None,
            rebalance: Rebalance::Annual,
            location: AssetLocation::Mirror,
            expense_ratio: 0.001,
            advisory_fee: 0.0,
            emergency_months: 6.0,
            retirement_cash_years: 0.0,
            custom_asset: None,
            single_stock: SingleStock::default(),
        }
    }
}

/// How the growth/defensive split changes around retirement. The "equity fraction" is
/// the share of the portfolio in growth assets (everything except bonds and cash);
/// within each group the proportions of `target` are kept.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum GlidePath {
    /// The target mix never changes.
    #[default]
    None,
    /// Like a target-date fund: `target` applies until the glide begins, then the
    /// equity fraction falls in a straight line to its retirement level and on to its
    /// final level.
    TargetDate {
        /// Years before retirement at which de-risking begins. Typical 15 to 25.
        #[serde(default = "d_years_before")]
        years_before: f64,
        /// Equity fraction at the retirement date (0.5 = 50% growth assets).
        #[serde(default = "d_equity_at_retirement")]
        equity_at_retirement: f64,
        /// Years after retirement over which the glide continues down to `equity_final`.
        #[serde(default = "d_years_after")]
        years_after: f64,
        /// Equity fraction held from the end of the glide onwards.
        #[serde(default = "d_equity_final")]
        equity_final: f64,
    },
    /// A "bond tent": the equity fraction dips to its lowest at retirement, when a
    /// crash hurts most, then climbs back to the target mix.
    BondTent {
        /// Years before retirement at which the shift into bonds begins.
        #[serde(default = "d_tent_years")]
        years_before: f64,
        /// Equity fraction at the retirement date, the low point of the tent.
        #[serde(default = "d_tent_equity")]
        equity_at_retirement: f64,
        /// Years after retirement over which the mix returns to `target`.
        #[serde(default = "d_tent_years")]
        years_after: f64,
    },
    /// An explicit schedule, interpolated in straight lines between points and held
    /// flat outside them.
    Custom {
        /// The schedule, in any order.
        points: Vec<GlidePoint>,
    },
}

fn d_years_before() -> f64 {
    20.0
}
fn d_equity_at_retirement() -> f64 {
    0.55
}
fn d_years_after() -> f64 {
    7.0
}
fn d_equity_final() -> f64 {
    0.35
}
fn d_tent_years() -> f64 {
    10.0
}
fn d_tent_equity() -> f64 {
    0.40
}

/// One point of a custom glide path.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct GlidePoint {
    /// Years until retirement at this point: positive before retirement, negative after
    /// (10 = ten years before, -5 = five years into retirement).
    pub years_to_retirement: f64,
    /// Equity fraction at this point, from 0 to 1.
    pub equity_fraction: f64,
}

/// When the portfolio is rebalanced to target.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Rebalance {
    /// Never sell to rebalance; new savings and withdrawals still steer toward target.
    Never,
    /// Once a year, in January.
    #[default]
    Annual,
    /// Whenever any asset class drifts from its target weight by more than a band.
    Bands {
        /// Allowed drift in absolute weight (0.05 = five percentage points).
        width: f64,
    },
}

/// How asset classes are split between account types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetLocation {
    /// Every account holds the same mix.
    #[default]
    Mirror,
    /// Bonds and REITs go to traditional accounts first, the highest-growth assets to
    /// Roth, and tax-efficient stock funds to taxable accounts, keeping the overall mix
    /// on target.
    TaxEfficient,
}

/// A user-defined asset class.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CustomAsset {
    /// Display name, for example "Private credit fund".
    pub name: String,
    /// Expected annual return in nominal terms, as a decimal (0.08 = 8%).
    pub mean: f64,
    /// Annual volatility as a decimal (0.15 = 15%).
    pub volatility: f64,
    /// Correlation with US stocks, from -1 to 1.
    pub equity_corr: f64,
}

impl Default for CustomAsset {
    fn default() -> Self {
        Self {
            name: "Custom asset".into(),
            mean: 0.06,
            volatility: 0.12,
            equity_corr: 0.3,
        }
    }
}

/// Risk parameters of the single-stock asset class. Its expected return is what its
/// market exposure implies; concentration adds risk, not return.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SingleStock {
    /// Annual volatility of the stock as a decimal. About 0.30 for a large company,
    /// 0.50 or more for a small or fast-growing one.
    pub volatility: f64,
    /// Sensitivity to the US stock market (1.0 moves one-for-one with it).
    pub beta: f64,
}

impl Default for SingleStock {
    fn default() -> Self {
        Self {
            volatility: 0.35,
            beta: 1.1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_index_and_keys_are_consistent() {
        for (i, a) in AssetClass::ALL.iter().enumerate() {
            assert_eq!(a.index(), i);
            assert_eq!(AssetClass::from_key(a.key()), Some(*a));
            assert_eq!(serde_json::to_value(a).unwrap(), serde_json::json!(a.key()));
        }
    }

    #[test]
    fn allocation_serde_is_a_sparse_map() {
        let a: Allocation = serde_json::from_str(r#"{"us_stocks": 70, "bonds": 30}"#).unwrap();
        assert_eq!(a[AssetClass::UsStocks], 70.0);
        assert!((a.weight(AssetClass::Bonds) - 0.3).abs() < 1e-12);
        assert!((a.growth_share() - 0.7).abs() < 1e-12);
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            r#"{"us_stocks":70.0,"bonds":30.0}"#
        );
        let err = serde_json::from_str::<Allocation>(r#"{"stonks": 1}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("stonks") && err.contains("us_stocks"), "{err}");
    }

    #[test]
    fn invalid_allocations_do_not_normalise() {
        assert!(Allocation::ZERO.normalized().is_none());
        let mut a = Allocation::default();
        a[AssetClass::Gold] = -1.0;
        assert!(a.normalized().is_none());
    }
}
