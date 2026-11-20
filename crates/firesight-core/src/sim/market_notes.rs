//! Market annotations in a traced path's event log: what the optional market
//! refinements (`model::market_realism`) and the episode stresses did on the path.
//! Computed once from the generated economy when a path is traced, and released
//! into the event log in the month they concern (a retirement-dated stress once the
//! retirement month is known). Nothing here runs outside trace mode.

use crate::economy::{episode_first_month, episode_months, EconomyPath};
use crate::model::{Episode, YearMonth};
use crate::stats::TraceEventKind;

use super::plan::{Plan, Sched, StressPlan};
use super::state::State;

/// One annotation: when it is due, amount in today's dollars (if any), sentence.
#[derive(Clone, Debug, Default)]
pub(crate) struct MarketNotes {
    notes: Vec<(Sched, Option<f64>, String)>,
    done: Vec<bool>,
}

fn episode_name(e: Episode) -> &'static str {
    match e {
        Episode::GreatDepression1929 => "the Great Depression",
        Episode::Stagflation1966 => "the 1966-82 stagflation",
        Episode::DotCom2000 => "the dot-com bust and the 2008 crisis",
        Episode::Financial2008 => "the 2008 financial crisis",
    }
}

impl State {
    /// Collects the annotations of this path (trace mode only).
    #[cold]
    #[inline(never)]
    pub(crate) fn plan_market_notes(&mut self, plan: &Plan, econ: &EconomyPath) {
        let notes = &mut self.market_notes;
        notes.notes.clear();
        if let Some((start, fair, tilt)) = plan.econ.valuation_outlook() {
            notes.notes.push((
                Sched::At(0),
                None,
                format!(
                    "US stocks start at a CAPE of {start:.1} against a normal {fair:.1}: expected to return {:.1} points a year {} than their average over the first decade",
                    tilt.abs() * 100.0,
                    if tilt < 0.0 { "less" } else { "more" }
                ),
            ));
            if let Some(t) = econ
                .valuation_gap
                .windows(2)
                .position(|w| (w[0] > 0.0) != (w[1] > 0.0))
            {
                notes.notes.push((
                    Sched::At(t as i32 + 1),
                    None,
                    "US stock valuations are back at their normal level".to_string(),
                ));
            }
        }
        for stress in &plan.stresses {
            match *stress {
                StressPlan::HistoricalEpisode { start, episode } => {
                    let first: YearMonth = episode_first_month(episode);
                    let months = episode_months(episode);
                    notes.notes.push((
                        start,
                        None,
                        format!(
                            "Markets replay {} from {first} for {:.1} years",
                            episode_name(episode),
                            months as f64 / 12.0
                        ),
                    ));
                }
                StressPlan::LostGeneration { start, months } => notes.notes.push((
                    start,
                    None,
                    format!(
                        "A lost generation begins: {:.0} years of stocks losing ground after inflation, falling prices and near-zero rates",
                        months as f64 / 12.0
                    ),
                )),
                _ => {}
            }
        }
        notes.done.clear();
        notes.done.resize(notes.notes.len(), false);
    }

    /// Moves the annotations due by month `t` into the event log.
    #[cold]
    #[inline(never)]
    pub(crate) fn emit_market_notes(&mut self, t: usize, now: YearMonth) {
        for i in 0..self.market_notes.notes.len() {
            let due = self.market_notes.notes[i].0.month(self.retire_month);
            if self.market_notes.done[i] || due == i32::MAX || due.max(0) as usize > t {
                continue;
            }
            self.market_notes.done[i] = true;
            let (_, amount, label) = self.market_notes.notes[i].clone();
            self.event(now, TraceEventKind::Market, amount, || label);
        }
    }
}
