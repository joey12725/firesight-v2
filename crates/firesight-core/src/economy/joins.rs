//! State-matched block joins for the bootstrap (`joins: matched`).
//!
//! With uniform joins a new block starts at any row of the history, so a recession
//! month can be followed by a boom month from another era and an inflation episode
//! is cut off at every seam. Matched joins restart from a row that followed
//! conditions like the present ones.
//!
//! Every row is classified, once, at construction, by three things known at the end
//! of that month:
//!
//! * the recession flag (2 values);
//! * inflation over the 12 months ending with it, by tercile of the rows used (3);
//! * the real US equity return over the same 12 months, by tercile (3).
//!
//! That gives up to 18 cells. A cell with fewer than [`MIN_CELL`] rows is merged
//! with its neighbours: first the three equity terciles of its recession-and-inflation
//! group become one cell, then the whole recession state. If a recession state is
//! still too thin as one cell (a window from 2010 holds two recession months), its
//! rows join the cells of the other state that share their inflation and equity
//! terciles: the recession flag is then not matched at all, but inflation and the
//! stock market still are. Only when that leaves a thin cell too does everything
//! become one cell, which is the same as uniform joins; no window validation
//! accepts gets that far (at least [`MIN_BOOTSTRAP_CELLS`] cells remain, a test
//! checks every starting year).
//!
//! At a restart after row `i` the generator picks a row `p` uniformly from the cell
//! of `i` (which contains `i` itself) and continues from `p + 1`, wrapping from the
//! last row to the first: the path goes on as history went on after some month that
//! looked like this one. The pick uses the same 32 bits of the same word as the
//! uniform restart, so the number of draws is unchanged.
//!
//! What is preserved: the cells are a partition of the rows, so if the row before a
//! restart is uniformly distributed over the history, so is the row after it. The
//! first month of a path is uniform, hence every month's row is uniform, exactly as
//! with uniform joins: the average of every series in every month is the same under
//! both settings. What changes is the dependence across a seam, and with it the
//! spread of multi-year outcomes.
//!
//! Simplifications: the state is three coarse features, not a distance over the
//! whole economy; the valuation level and the interest-rate level are not matched
//! (rate levels do not jump at seams in either setting); the terciles are of the
//! rows in use, so `from_year` changes them.

use crate::math;

use super::Core;

/// Smallest cell that is kept on its own. A judgement: a year of months gives a
/// restart a real choice; below that the path would mostly replay one episode.
pub const MIN_CELL: usize = 12;
/// Months in the trailing window of the two continuous features.
pub const TRAILING_MONTHS: usize = 12;

/// Fewest cells a window that validation accepts is left with (three inflation
/// terciles), so matched joins never silently become uniform ones.
pub const MIN_BOOTSTRAP_CELLS: usize = 3;

/// Rows per cell id.
fn counts(cell: &[usize]) -> [usize; 18] {
    let mut count = [0usize; 18];
    for &c in cell {
        count[c] += 1;
    }
    count
}

/// For each width in turn, collapses every group of `width` consecutive ids onto
/// its first id when any occupied member of the group has fewer than [`MIN_CELL`]
/// rows.
fn merge_thin(cell: &mut [usize], widths: &[usize]) {
    for &width in widths {
        let count = counts(cell);
        for group in (0..18).step_by(width) {
            let thin = count[group..group + width]
                .iter()
                .any(|&k| k > 0 && k < MIN_CELL);
            if thin {
                for c in cell.iter_mut() {
                    if (group..group + width).contains(c) {
                        *c = group;
                    }
                }
            }
        }
    }
}

/// For each row, the slice of `members` holding the rows of its cell.
#[derive(Clone, Debug)]
pub(super) struct JoinTable {
    /// `(first, length)` into `members`, per row.
    range: Vec<(u32, u32)>,
    /// Row indices grouped by cell, ascending within a cell.
    members: Vec<u32>,
}

impl JoinTable {
    pub(super) fn new(rows: &[Core]) -> JoinTable {
        let n = rows.len();
        // Trailing means of monthly log inflation and log real equity return, over
        // the months available (fewer than twelve for the first rows).
        let mut inflation = Vec::with_capacity(n);
        let mut equity = Vec::with_capacity(n);
        let log = |x: f64| math::ln((1.0 + x).max(1e-9));
        for i in 0..n {
            let from = (i + 1).saturating_sub(TRAILING_MONTHS);
            let (mut cpi, mut eq) = (0.0, 0.0);
            for r in &rows[from..=i] {
                let c = log(r.cpi);
                cpi += c;
                eq += log(r.eq) - c;
            }
            let count = (i + 1 - from) as f64;
            inflation.push(cpi / count);
            equity.push(eq / count);
        }
        let tercile = |values: &[f64]| -> Vec<u8> {
            let mut sorted = values.to_vec();
            sorted.sort_by(f64::total_cmp);
            let cut = |k: usize| sorted.get(k * n / 3).copied().unwrap_or(f64::INFINITY);
            let (lo, hi) = (cut(1), cut(2));
            values
                .iter()
                .map(|&v| u8::from(v >= lo) + u8::from(v >= hi))
                .collect()
        };
        let (inf_class, eq_class) = (tercile(&inflation), tercile(&equity));
        // Cell ids: recession * 9 + inflation tercile * 3 + equity tercile.
        let original: Vec<usize> = (0..n)
            .map(|i| {
                usize::from(rows[i].recession.min(1)) * 9
                    + usize::from(inf_class[i]) * 3
                    + usize::from(eq_class[i])
            })
            .collect();
        let mut cell = original.clone();
        // Merge thin cells: a group of `width` consecutive ids collapses onto its
        // first id when any occupied member of it is below the minimum.
        merge_thin(&mut cell, &[3, 9]);
        // A recession state that is too thin even as one cell is folded into the
        // other state, tercile by tercile, so that the two continuous features are
        // still matched. `original` is each row's id before any merging; `merged`
        // says where rows of an original id now are.
        let mut merged = [usize::MAX; 18];
        for (&id, &c) in original.iter().zip(&cell) {
            merged[id] = c;
        }
        let count = counts(&cell);
        for state in [0usize, 9] {
            let thin = count[state..state + 9]
                .iter()
                .any(|&k| k > 0 && k < MIN_CELL);
            if !thin {
                continue;
            }
            let other = 9 - state;
            for (c, &id) in cell.iter_mut().zip(&original) {
                if (state..state + 9).contains(&id) {
                    let twin = merged[id - state + other];
                    // No row of the other state in that tercile pair: the row
                    // starts that cell (and the pass below merges it if thin).
                    *c = if twin == usize::MAX {
                        id - state + other
                    } else {
                        twin
                    };
                }
            }
            // One state at most can be folded: the other must receive its rows.
            break;
        }
        // Whatever is still thin merges as before, in the end into one cell.
        merge_thin(&mut cell, &[3, 9, 18]);
        let mut start = [0u32; 19];
        for &c in &cell {
            start[c + 1] += 1;
        }
        for c in 0..18 {
            start[c + 1] += start[c];
        }
        let mut next = start;
        let mut members = vec![0u32; n];
        for (i, &c) in cell.iter().enumerate() {
            members[next[c] as usize] = i as u32;
            next[c] += 1;
        }
        let range = cell
            .iter()
            .map(|&c| (start[c], start[c + 1] - start[c]))
            .collect();
        JoinTable { range, members }
    }

    /// The row a new block starts from when the row just emitted was `current`:
    /// one past a uniformly chosen member of `current`'s cell. `bits` is 32 uniform
    /// bits.
    #[inline]
    pub(super) fn restart(&self, current: usize, bits: u64) -> usize {
        let (first, len) = self.range[current];
        let pick = first as usize + ((bits * u64::from(len)) >> 32) as usize;
        let next = self.members[pick] as usize + 1;
        if next == self.members.len() {
            0
        } else {
            next
        }
    }

    /// The rows of the cell of `row`.
    #[cfg(test)]
    pub(super) fn cell_of(&self, row: usize) -> &[u32] {
        let (first, len) = self.range[row];
        &self.members[first as usize..(first + len) as usize]
    }

    /// Number of distinct cells.
    #[cfg(test)]
    pub(super) fn cell_count(&self) -> usize {
        let mut firsts: Vec<u32> = self.range.iter().map(|r| r.0).collect();
        firsts.sort_unstable();
        firsts.dedup();
        firsts.len()
    }
}
