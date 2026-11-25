//! Historical-episode and lost-generation stress overlays
//! (`Stress::HistoricalEpisode`, `Stress::LostGeneration`).
//!
//! A historical episode replays the record: from the start month, US stocks, bonds,
//! bills, inflation, home prices and the recession flag are the record's own months
//! from the episode's first month on, as they were (no equity-premium or inflation
//! adjustment), and international stocks and gold too where the record has them.
//! The 10-year yield and the mortgage rate move by the record's monthly changes from
//! the path's level, so a path does not jump to 1929's rates overnight. Asset
//! classes without history (listed real estate, crypto, the single stock, the custom
//! asset; international stocks before 1975 and gold before 1968) move by their
//! equity beta times the change imposed on US stocks, so a collapsed holding stays
//! collapsed.
//!
//! A lost generation rewrites the window like the lost decade does, at Japanese
//! targets: inflation is scaled to average [`LOST_GENERATION_INFLATION`], US and
//! international stocks to a real total return of [`LOST_GENERATION_EQUITY_REAL`] a
//! year (each month's gross return times one constant, so the path's volatility is
//! kept), and the bill and 10-year yields glide down to [`LOST_GENERATION_BILL_YIELD`]
//! and [`LOST_GENERATION_LONG_YIELD`] over [`LOST_GENERATION_GLIDE_YEARS`] with the
//! path's own month-to-month moves kept. Bonds are priced from the new yield path.
//!
//! After either window, the shift each overlay imposed on the rate levels fades at
//! the pull of the bootstrap's rates to their mean (1/120 a month) instead of
//! snapping back, and bond returns carry the corresponding price change.
//! No random numbers are drawn.

use super::stress::shifted;
use super::{EconomyModel, EconomyPath, MIN_LONG_YIELD, RATE_REVERSION};
use crate::data::History;
use crate::math;
use crate::model::{
    AssetClass, Episode, YearMonth, LOST_GENERATION_BILL_YIELD, LOST_GENERATION_EQUITY_REAL,
    LOST_GENERATION_GLIDE_YEARS, LOST_GENERATION_INFLATION, LOST_GENERATION_LONG_YIELD,
};

/// First month and length of each episode, in [`Episode`] order. Every window
/// except 1966 runs from the month after the US stock market's real peak to the
/// month it regained it in the embedded record (`data/history/monthly.csv`, CRSP
/// total market deflated by CPI-U): peaks 1929-08, 2000-03 and 2007-10, recoveries
/// 1945-02, 2013-02 and 2013-01. Stagflation1966 runs from the Dow's January 1966
/// peak to July 1982, the month before the August 1982 low that began the next bull
/// market; the total market regained its 1966 real level briefly in 1967, so the
/// real-peak rule gives no end (the window is a judgement).
const WINDOWS: [(Episode, YearMonth, usize); 4] = [
    (Episode::GreatDepression1929, YearMonth::ym(1929, 9), 186),
    (Episode::Stagflation1966, YearMonth::ym(1966, 1), 199),
    (Episode::DotCom2000, YearMonth::ym(2000, 4), 155),
    (Episode::Financial2008, YearMonth::ym(2007, 11), 63),
];

/// Length of an episode in months.
pub fn episode_months(episode: Episode) -> usize {
    WINDOWS.iter().find(|w| w.0 == episode).map_or(0, |w| w.2)
}

/// First calendar month of an episode in the record.
pub fn episode_first_month(episode: Episode) -> YearMonth {
    WINDOWS
        .iter()
        .find(|w| w.0 == episode)
        .map_or(YearMonth::ym(1926, 7), |w| w.1)
}

/// One month of the record, unadjusted. `intl` and `gold` are NaN where the record
/// has no value.
#[derive(Clone, Copy, Debug)]
pub(super) struct EpisodeMonth {
    eq: f64,
    intl: f64,
    bond: f64,
    bill: f64,
    cpi: f64,
    home: f64,
    gold: f64,
    d_long: f64,
    d_mortgage: f64,
    recession: u8,
}

/// The record's months of every episode, cut from the history once per model.
#[derive(Clone, Debug, Default)]
pub(super) struct EpisodeTables {
    months: [Vec<EpisodeMonth>; 4],
}

impl EpisodeTables {
    /// Cuts the windows from `h`. An episode outside the record stays empty (and
    /// its overlay does nothing).
    pub(super) fn new(h: &History) -> EpisodeTables {
        let mut out = EpisodeTables::default();
        let home = super::regime::forward_fill(&h.home);
        for (slot, &(_, first, len)) in out.months.iter_mut().zip(&WINDOWS) {
            let Some(a) = h.index_of(first) else { continue };
            if a == 0 || a + len > h.len() {
                continue;
            }
            *slot = (a..a + len)
                .map(|i| EpisodeMonth {
                    eq: h.equity_us[i],
                    intl: h.equity_intl[i],
                    bond: h.bond10[i],
                    bill: h.tbill[i],
                    cpi: h.cpi[i],
                    home: home[i],
                    gold: h.gold[i],
                    d_long: h.long_yield[i] - h.long_yield[i - 1],
                    d_mortgage: h.mortgage30[i] - h.mortgage30[i - 1],
                    recession: h.recession[i],
                })
                .collect();
        }
        out
    }

    fn get(&self, episode: Episode) -> &[EpisodeMonth] {
        let i = WINDOWS.iter().position(|w| w.0 == episode).unwrap_or(0);
        &self.months[i]
    }
}

/// Classes that follow an imposed US-equity move through their beta in both overlays
/// (international stocks and gold are handled separately).
const FOLLOWERS: [AssetClass; 4] = [
    AssetClass::RealEstate,
    AssetClass::Crypto,
    AssetClass::SingleStock,
    AssetClass::Custom,
];

impl EconomyModel {
    /// The 10-year yield, mortgage rate and bill yield at the start of month `from`.
    fn rates_before(&self, out: &EconomyPath, from: usize) -> (f64, f64, f64) {
        if from == 0 {
            let a = self.adjust.inflation_annual;
            (
                self.base.rate_now.0 + a,
                self.base.rate_now.1 + a,
                out.tbill[0],
            )
        } else {
            (
                out.long_yield[from - 1],
                out.mortgage_rate[from - 1],
                out.tbill[from - 1],
            )
        }
    }

    /// Moves US equities of month `t` to `new` and the follower classes by beta.
    #[inline]
    fn impose_equity(&self, out: &mut EconomyPath, t: usize, new: f64) {
        let us = AssetClass::UsStocks.index();
        let delta = new - out.returns[us][t];
        out.returns[us][t] = new;
        for a in FOLLOWERS {
            let i = a.index();
            out.returns[i][t] = shifted(out.returns[i][t], self.stress_beta[i], delta);
        }
    }

    /// From month `end` on, adds rate shifts that fade at [`RATE_REVERSION`] a month
    /// to the path's own levels, and gives bonds the carry and price change of the
    /// shift. `shifts` are (long, mortgage, bill) at the end of the window.
    fn fade_rate_shifts(&self, out: &mut EconomyPath, end: usize, shifts: (f64, f64, f64)) {
        let (mut s_long, mut s_mort, mut s_bill) = shifts;
        let bonds = AssetClass::Bonds.index();
        let cash = AssetClass::Cash.index();
        let keep = 1.0 - RATE_REVERSION;
        for t in end..out.months {
            let y_prev = if t == 0 {
                out.long_yield[0]
            } else {
                out.long_yield[t - 1]
            };
            let base_prev = y_prev - s_long;
            s_long *= keep;
            s_mort *= keep;
            s_bill *= keep;
            let own = out.long_yield[t];
            let new = (own + s_long).max(MIN_LONG_YIELD);
            // The price change and carry of the shifted yield, over the path's own.
            let extra =
                self.bonds.bond_return(y_prev, new) - self.bonds.bond_return(base_prev, own);
            out.returns[bonds][t] += extra;
            out.long_yield[t] = new;
            out.mortgage_rate[t] = (out.mortgage_rate[t] + s_mort).max(new);
            let bill = (out.tbill[t] + s_bill).max(0.0);
            if bill != out.tbill[t] {
                out.tbill[t] = bill;
                out.returns[cash][t] = math::pow(1.0 + bill, 1.0 / 12.0) - 1.0;
            }
        }
    }

    pub(super) fn apply_episode(&self, out: &mut EconomyPath, from: usize, episode: Episode) {
        let Some(tables) = &self.episodes else {
            return;
        };
        let rows = tables.get(episode);
        if from >= out.months || rows.is_empty() {
            return;
        }
        let end = from.saturating_add(rows.len()).min(out.months);
        let own_last = Self::own_last(out, end);
        let (mut long, mut mort, _) = self.rates_before(out, from);
        let (intl, gold) = (AssetClass::IntlStocks.index(), AssetClass::Gold.index());
        let us = AssetClass::UsStocks.index();
        for (t, m) in (from..end).zip(rows) {
            let delta = m.eq - out.returns[us][t];
            self.impose_equity(out, t, m.eq);
            for (i, record) in [(intl, m.intl), (gold, m.gold)] {
                out.returns[i][t] = if record.is_finite() {
                    record
                } else {
                    shifted(out.returns[i][t], self.stress_beta[i], delta)
                };
            }
            out.returns[AssetClass::Bonds.index()][t] = m.bond;
            out.returns[AssetClass::Cash.index()][t] = m.bill;
            out.tbill[t] = math::powi(1.0 + m.bill, 12) - 1.0;
            out.inflation[t] = m.cpi;
            if m.home.is_finite() {
                out.home_national[t] = m.home;
            }
            out.recession[t] = m.recession;
            long = (long + m.d_long).max(MIN_LONG_YIELD);
            mort = (mort + m.d_mortgage).max(long);
            out.long_yield[t] = long;
            out.mortgage_rate[t] = mort;
        }
        self.finish_window(out, end, own_last);
    }

    /// The path's own rate levels in the last month of a window, before it is
    /// rewritten: (long, mortgage, bill).
    fn own_last(out: &EconomyPath, end: usize) -> (f64, f64, f64) {
        let i = end - 1;
        (out.long_yield[i], out.mortgage_rate[i], out.tbill[i])
    }

    /// Fades the rate shifts the window left (rewritten levels less the path's own
    /// `own_last`) from month `end` on, then rebuilds the price level.
    fn finish_window(&self, out: &mut EconomyPath, end: usize, own_last: (f64, f64, f64)) {
        if end < out.months {
            let i = end - 1;
            let shifts = (
                out.long_yield[i] - own_last.0,
                out.mortgage_rate[i] - own_last.1,
                out.tbill[i] - own_last.2,
            );
            self.fade_rate_shifts(out, end, shifts);
        }
        out.rebuild_price_level();
    }

    pub(super) fn apply_lost_generation(&self, out: &mut EconomyPath, from: usize, months: usize) {
        if from >= out.months || months == 0 {
            return;
        }
        let end = from.saturating_add(months).min(out.months);
        let n = (end - from) as f64;
        let own_last = Self::own_last(out, end);
        // Inflation: the path's monthly variation, scaled to the target average.
        let mut price = 1.0;
        for i in &out.inflation[from..end] {
            price *= 1.0 + i;
        }
        let target_monthly = 1.0 + math::annual_to_monthly(LOST_GENERATION_INFLATION);
        let k_inflation = target_monthly / math::pow(price, 1.0 / n);
        for t in from..end {
            out.inflation[t] = (1.0 + out.inflation[t]) * k_inflation - 1.0;
            out.home_national[t] = (1.0 + out.home_national[t]) * k_inflation - 1.0;
        }
        // Rates: glide to the floors over the glide years, keeping the path's moves.
        let (y0, _, b0) = self.rates_before(out, from);
        let (own_y0, own_b0) = if from == 0 {
            (y0, b0)
        } else {
            (out.long_yield[from - 1], out.tbill[from - 1])
        };
        let glide = (LOST_GENERATION_GLIDE_YEARS * 12.0).max(1.0);
        let bonds = AssetClass::Bonds.index();
        let cash = AssetClass::Cash.index();
        let mut y_prev = y0;
        for t in from..end {
            let f = (((t - from + 1) as f64) / glide).min(1.0);
            let long_target = y0 + (LOST_GENERATION_LONG_YIELD - y0) * f;
            let bill_target = b0 + (LOST_GENERATION_BILL_YIELD - b0) * f;
            let long = (out.long_yield[t] - own_y0 + long_target).max(MIN_LONG_YIELD);
            let shift = long - out.long_yield[t];
            out.long_yield[t] = long;
            out.mortgage_rate[t] = (out.mortgage_rate[t] + shift).max(long);
            let bill = (out.tbill[t] - own_b0 + bill_target).max(0.0);
            out.tbill[t] = bill;
            out.returns[cash][t] = math::pow(1.0 + bill, 1.0 / 12.0) - 1.0;
            out.returns[bonds][t] = self.bonds.bond_return(y_prev, long);
            y_prev = long;
        }
        // Stocks: a constant factor per month so the real total hits the target.
        let (us, intl) = (AssetClass::UsStocks.index(), AssetClass::IntlStocks.index());
        let (mut real_us, mut real_intl) = (1.0, 1.0);
        for t in from..end {
            let deflator = 1.0 + out.inflation[t];
            real_us *= (1.0 + out.returns[us][t]) / deflator;
            real_intl *= (1.0 + out.returns[intl][t]) / deflator;
        }
        let target = math::ln_1p(LOST_GENERATION_EQUITY_REAL) / 12.0;
        let k_us = math::exp(target - math::ln(real_us) / n);
        let k_intl = math::exp(target - math::ln(real_intl) / n);
        let gold = AssetClass::Gold.index();
        for t in from..end {
            let new = (1.0 + out.returns[us][t]) * k_us - 1.0;
            let delta = new - out.returns[us][t];
            self.impose_equity(out, t, new);
            out.returns[gold][t] = shifted(out.returns[gold][t], self.stress_beta[gold], delta);
            out.returns[intl][t] = (1.0 + out.returns[intl][t]) * k_intl - 1.0;
        }
        self.finish_window(out, end, own_last);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_the_documented_dates() {
        assert_eq!(episode_months(Episode::GreatDepression1929), 186);
        assert_eq!(
            episode_first_month(Episode::GreatDepression1929) + 185,
            YearMonth::ym(1945, 2)
        );
        assert_eq!(
            episode_first_month(Episode::Stagflation1966) + 198,
            YearMonth::ym(1982, 7)
        );
        assert_eq!(
            episode_first_month(Episode::DotCom2000) + 154,
            YearMonth::ym(2013, 2)
        );
        assert_eq!(
            episode_first_month(Episode::Financial2008) + 62,
            YearMonth::ym(2013, 1)
        );
    }
}
