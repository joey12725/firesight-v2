//! Market annotations in a traced path's event log: what the optional market
//! refinements (`model::market_realism`) did on the path. Computed once from the
//! generated economy when a path is traced, and released into the event log in the
//! month they concern. Nothing here runs outside trace mode.

use crate::economy::EconomyPath;
use crate::model::YearMonth;
use crate::stats::TraceEventKind;

use super::plan::Plan;
use super::state::State;

/// One annotation: month index, amount in today's dollars (if any), sentence.
#[derive(Clone, Debug, Default)]
pub(crate) struct MarketNotes {
    notes: Vec<(usize, Option<f64>, String)>,
    next: usize,
}

impl State {
    /// Collects the annotations of this path (trace mode only).
    #[cold]
    #[inline(never)]
    pub(crate) fn plan_market_notes(&mut self, plan: &Plan, econ: &EconomyPath) {
        let notes = &mut self.market_notes;
        notes.notes.clear();
        notes.next = 0;
        if let Some((start, fair, tilt)) = plan.econ.valuation_outlook() {
            notes.notes.push((
                0,
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
                    t + 1,
                    None,
                    "US stock valuations are back at their normal level".to_string(),
                ));
            }
        }
        notes.notes.sort_by_key(|n| n.0);
    }

    /// Moves the annotations due by month `t` into the event log.
    #[cold]
    #[inline(never)]
    pub(crate) fn emit_market_notes(&mut self, t: usize, now: YearMonth) {
        while self.market_notes.next < self.market_notes.notes.len()
            && self.market_notes.notes[self.market_notes.next].0 <= t
        {
            let (_, amount, label) = self.market_notes.notes[self.market_notes.next].clone();
            self.market_notes.next += 1;
            self.event(now, TraceEventKind::Market, amount, || label);
        }
    }
}
