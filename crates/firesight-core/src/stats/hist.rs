//! The histogram grid and its codec.
//!
//! One grid serves every histogrammed series: it is symmetric about zero, linear
//! near zero and logarithmic (octaves split linearly) beyond.
//!
//! * Exactly `0.0` has its own zero-width bin, so a mass of depleted paths reads back
//!   as exactly zero.
//! * `|x| < $1,024`: linear bins $16 wide.
//! * `$1,024 <= |x| < 2^32` (about $4.29 billion): each power of two is split into 64
//!   equal bins, so a bin is between 0.78% and 1.5625% of its lower edge wide. The
//!   $16 linear bins join the first octave without a seam.
//! * `|x| >= 2^32` is clamped into the outermost bin.
//!
//! The mapping reads the exponent and the top six mantissa bits of the `f64`: no
//! logarithm, a few nanoseconds, and bit-identical on every platform.
//!
//! Bins are indexed in ascending value order: `0` is the most negative, [`ZERO_BIN`]
//! is exact zero, `HIST_BINS - 1` the largest.

/// Sub-bins per power of two, as a bit count (6 = 64 sub-bins, at most 1.5625% wide).
const SUB_BITS: u32 = 6;
/// Sub-bins per power of two.
pub const HIST_SUB_BINS: usize = 1 << SUB_BITS;
/// Magnitudes below `2^LINEAR_EXP` dollars are binned linearly.
const LINEAR_EXP: i32 = 10;
/// Magnitudes of `2^MAX_EXP` dollars and above are clamped into the last bin.
const MAX_EXP: i32 = 32;
/// The upper limit of the grid in dollars (2^32); larger magnitudes are clamped.
pub const HIST_MAX: f64 = 4_294_967_296.0;
/// End of the linear zone in dollars (2^10).
const LINEAR_LIMIT: f64 = 1_024.0;
/// Bins per dollar in the linear zone (64 bins over $1,024).
const LINEAR_SCALE: f64 = HIST_SUB_BINS as f64 / LINEAR_LIMIT;
/// Bins on each side of zero.
pub(crate) const MAG_BINS: usize = HIST_SUB_BINS * (1 + (MAX_EXP - LINEAR_EXP) as usize);
/// Number of bins in each annual histogram.
pub const HIST_BINS: usize = 2 * MAG_BINS + 1;
/// The bin holding exactly zero.
pub(crate) const ZERO_BIN: usize = MAG_BINS;

/// Magnitude bin of a strictly positive value.
#[inline]
fn mag_bin(a: f64) -> usize {
    if a < LINEAR_LIMIT {
        // Multiplying by a power of two is exact, so this is floor(a / 16).
        (a * LINEAR_SCALE) as usize
    } else {
        let bits = a.to_bits();
        let exponent = (bits >> 52) as i32 - 1023;
        if exponent >= MAX_EXP {
            MAG_BINS - 1
        } else {
            HIST_SUB_BINS
                + (exponent - LINEAR_EXP) as usize * HIST_SUB_BINS
                + ((bits >> (52 - SUB_BITS)) as usize & (HIST_SUB_BINS - 1))
        }
    }
}

/// `2^k` exactly.
fn pow2(k: i32) -> f64 {
    f64::from_bits(((1023 + k) as u64) << 52)
}

/// Lower edge of magnitude bin `m`; `mag_edge(MAG_BINS)` is the top of the grid.
pub(crate) fn mag_edge(m: usize) -> f64 {
    if m <= HIST_SUB_BINS {
        m as f64 / LINEAR_SCALE
    } else {
        let octave = (m - HIST_SUB_BINS) / HIST_SUB_BINS;
        let sub = (m - HIST_SUB_BINS) % HIST_SUB_BINS;
        (HIST_SUB_BINS + sub) as f64 * pow2(LINEAR_EXP + octave as i32 - SUB_BITS as i32)
    }
}

/// Maps a today's-dollar value to its histogram bin. Zero and NaN go to the zero bin;
/// infinities and magnitudes beyond [`HIST_MAX`] to the outermost bins.
#[inline]
pub fn hist_bin(value: f64) -> usize {
    if value > 0.0 {
        ZERO_BIN + 1 + mag_bin(value)
    } else if value < 0.0 {
        ZERO_BIN - 1 - mag_bin(-value)
    } else {
        ZERO_BIN
    }
}

/// The value range `[lo, hi)` of a bin (`(lo, hi]` mirrored for negative bins). The
/// zero bin is `(0, 0)`.
pub fn hist_bin_edges(bin: usize) -> (f64, f64) {
    debug_assert!(bin < HIST_BINS);
    if bin > ZERO_BIN {
        let m = bin - ZERO_BIN - 1;
        (mag_edge(m), mag_edge(m + 1))
    } else if bin < ZERO_BIN {
        let m = ZERO_BIN - 1 - bin;
        (-mag_edge(m + 1), -mag_edge(m))
    } else {
        (0.0, 0.0)
    }
}

/// Representative value of a bin: its midpoint (quantiles interpolate linearly inside
/// a bin, so the midpoint is the bin's median under that convention).
pub fn hist_bin_value(bin: usize) -> f64 {
    let (lo, hi) = hist_bin_edges(bin);
    0.5 * (lo + hi)
}

/// Reads quantiles from one histogram. `levels` must be ascending in `0..=1`; `out`
/// receives one value per level (NaN when the histogram is empty). Within a bin the
/// mass is taken as uniform (linear interpolation).
pub(crate) fn quantiles(counts: &[u32], levels: &[f64], out: &mut [f64]) {
    debug_assert_eq!(counts.len(), HIST_BINS);
    debug_assert_eq!(levels.len(), out.len());
    let total: u64 = counts.iter().map(|&c| u64::from(c)).sum();
    if total == 0 {
        out.fill(f64::NAN);
        return;
    }
    let n = total as f64;
    let mut cum = 0u64;
    let mut li = 0;
    let mut last_hi = 0.0;
    let mut bin = 0;
    while bin < counts.len() {
        // Most bins are empty: step over them eight at a time.
        if counts
            .get(bin..bin + 8)
            .is_some_and(|g| g.iter().fold(0, |a, &c| a | c) == 0)
        {
            bin += 8;
            continue;
        }
        let c = counts[bin];
        bin += 1;
        if c == 0 {
            continue;
        }
        let bin = bin - 1;
        let next = cum + u64::from(c);
        let (lo, hi) = hist_bin_edges(bin);
        while li < levels.len() && levels[li] * n <= next as f64 {
            let frac = ((levels[li] * n - cum as f64) / f64::from(c)).clamp(0.0, 1.0);
            out[li] = lo + frac * (hi - lo);
            li += 1;
        }
        last_hi = hi;
        cum = next;
        if li == levels.len() {
            return;
        }
    }
    // Only reachable through rounding of `level * n` at the very top.
    out[li..].fill(last_hi);
}

fn put_varint(mut v: u64, out: &mut Vec<u8>) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn get_varint(bytes: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    let mut shift = 0;
    loop {
        let b = *bytes.get(*pos)?;
        *pos += 1;
        if shift >= 64 {
            return None;
        }
        v |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
    }
}

/// A gap of this many empty bins or more ends a run: each empty bin inside a run
/// costs one byte, a new run costs two (skip and length).
const RUN_BREAK: usize = 3;

/// Calls `f(start, end)` for each run of a block: maximal stretches that begin and
/// end with a non-empty bin and contain no gap of [`RUN_BREAK`] empty bins.
fn for_each_run(block: &[u32], mut f: impl FnMut(usize, usize)) {
    let mut i = 0;
    while i < block.len() {
        // Most of a block is empty: skip empty bins eight at a time (an OR of eight
        // counts is one vector compare) before looking at single bins.
        while let Some(group) = block.get(i..i + 8) {
            if group.iter().fold(0, |a, &c| a | c) != 0 {
                break;
            }
            i += 8;
        }
        if i >= block.len() {
            break;
        }
        if block[i] == 0 {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 1;
        let mut j = i + 1;
        while j < block.len() && j - end < RUN_BREAK {
            if block[j] != 0 {
                end = j + 1;
            }
            j += 1;
        }
        f(start, end);
        i = end;
    }
}

/// Packs dense histograms (`blocks` consecutive blocks of [`HIST_BINS`] counts) into
/// bytes. Per block: the number of runs, then for each run the gap since the previous
/// run's end, its length and its counts, all LEB128 varints. An empty block is one
/// byte; a block costs roughly one or two bytes per occupied bin. The encoding is a
/// pure function of the counts, so equal histograms give equal bytes.
pub(crate) fn pack(hist: &[u32]) -> Vec<u8> {
    debug_assert_eq!(hist.len() % HIST_BINS, 0);
    let mut out = Vec::with_capacity(hist.len() / 32);
    // The runs of the block being packed: found in one scan of the block, then
    // written after their count.
    let mut runs: Vec<(usize, usize)> = Vec::with_capacity(64);
    for block in hist.chunks_exact(HIST_BINS) {
        runs.clear();
        for_each_run(block, |start, end| runs.push((start, end)));
        put_varint(runs.len() as u64, &mut out);
        let mut prev_end = 0;
        for &(start, end) in &runs {
            put_varint((start - prev_end) as u64, &mut out);
            put_varint((end - start) as u64, &mut out);
            for &c in &block[start..end] {
                put_varint(u64::from(c), &mut out);
            }
            prev_end = end;
        }
    }
    out
}

/// Walks packed bytes for `hist.len() / HIST_BINS` blocks, calling `f(slot, count)`
/// for every packed count. `None` (after possibly some calls) if the bytes are
/// malformed.
fn for_each_packed(
    bytes: &[u8],
    hist: &mut [u32],
    mut f: impl FnMut(&mut u32, u32),
) -> Option<()> {
    let mut pos = 0;
    for block in hist.chunks_exact_mut(HIST_BINS) {
        let runs = get_varint(bytes, &mut pos)?;
        let mut at = 0usize;
        for _ in 0..runs {
            let skip = usize::try_from(get_varint(bytes, &mut pos)?).ok()?;
            let len = usize::try_from(get_varint(bytes, &mut pos)?).ok()?;
            let start = at.checked_add(skip)?;
            let end = start.checked_add(len)?;
            if len == 0 || end > HIST_BINS {
                return None;
            }
            for slot in &mut block[start..end] {
                f(slot, u32::try_from(get_varint(bytes, &mut pos)?).ok()?);
            }
            at = end;
        }
    }
    (pos == bytes.len()).then_some(())
}

/// Inverse of [`pack`] for `blocks` blocks. `None` if the bytes are malformed.
pub(crate) fn unpack(bytes: &[u8], blocks: usize) -> Option<Vec<u32>> {
    let mut hist = vec![0u32; blocks * HIST_BINS];
    for_each_packed(bytes, &mut hist, |slot, c| *slot = c)?;
    Some(hist)
}

/// Adds packed counts ([`pack`] output for `hist.len() / HIST_BINS` blocks) to
/// `hist`: the same as adding [`unpack`]'s result element-wise, touching only the
/// packed bins. Returns false, with `hist` unchanged, if the bytes are malformed.
pub(crate) fn add_packed(bytes: &[u8], hist: &mut [u32]) -> bool {
    // A dry run first (it writes nothing), so malformed input changes nothing.
    if !packed_is_valid(bytes, hist.len() / HIST_BINS) {
        return false;
    }
    for_each_packed(bytes, hist, |slot, c| *slot += c).is_some()
}

/// True if `bytes` is a well-formed packing of `blocks` blocks.
fn packed_is_valid(bytes: &[u8], blocks: usize) -> bool {
    let mut pos = 0;
    for _ in 0..blocks {
        let Some(runs) = get_varint(bytes, &mut pos) else {
            return false;
        };
        let mut at = 0usize;
        for _ in 0..runs {
            let (Some(skip), Some(len)) = (get_varint(bytes, &mut pos), get_varint(bytes, &mut pos))
            else {
                return false;
            };
            let (Ok(skip), Ok(len)) = (usize::try_from(skip), usize::try_from(len)) else {
                return false;
            };
            let Some(end) = at.checked_add(skip).and_then(|s| s.checked_add(len)) else {
                return false;
            };
            if len == 0 || end > HIST_BINS {
                return false;
            }
            for _ in 0..len {
                match get_varint(bytes, &mut pos) {
                    Some(c) if u32::try_from(c).is_ok() => {}
                    _ => return false,
                }
            }
            at = end;
        }
    }
    pos == bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    #[test]
    fn grid_constants() {
        assert_eq!(MAG_BINS, 64 * 23);
        assert_eq!(HIST_BINS, 2945);
        assert_eq!(mag_edge(MAG_BINS), HIST_MAX);
        assert_eq!(mag_edge(HIST_SUB_BINS), LINEAR_LIMIT);
        assert_eq!(pow2(10), 1024.0);
        assert_eq!(pow2(-3), 0.125);
    }

    #[test]
    fn known_values_map_to_expected_bins() {
        assert_eq!(hist_bin(0.0), ZERO_BIN);
        assert_eq!(hist_bin(-0.0), ZERO_BIN);
        assert_eq!(hist_bin(f64::NAN), ZERO_BIN);
        // Linear zone: $16 bins.
        assert_eq!(hist_bin(0.01), ZERO_BIN + 1);
        assert_eq!(hist_bin(15.99), ZERO_BIN + 1);
        assert_eq!(hist_bin(16.0), ZERO_BIN + 2);
        assert_eq!(hist_bin(1023.99), ZERO_BIN + 64);
        assert_eq!(hist_bin(-0.01), ZERO_BIN - 1);
        assert_eq!(hist_bin(-16.0), ZERO_BIN - 2);
        // First octave: [1024, 2048) in 64 bins of $16.
        assert_eq!(hist_bin(1024.0), ZERO_BIN + 65);
        assert_eq!(hist_bin(1039.99), ZERO_BIN + 65);
        assert_eq!(hist_bin(1040.0), ZERO_BIN + 66);
        assert_eq!(hist_bin(2048.0), ZERO_BIN + 1 + 128);
        // $1,000,000 = 2^19 * 1.9073..., sub-bin floor(0.9073 * 64) = 58.
        assert_eq!(hist_bin(1_000_000.0), ZERO_BIN + 1 + 64 + 9 * 64 + 58);
        assert_eq!(
            hist_bin_edges(hist_bin(1_000_000.0)),
            (999_424.0, 1_007_616.0)
        );
        assert_eq!(hist_bin(-1_000_000.0), ZERO_BIN - 1 - (64 + 9 * 64 + 58));
        assert_eq!(
            hist_bin_edges(hist_bin(-1_000_000.0)),
            (-1_007_616.0, -999_424.0)
        );
        // Clamping.
        assert_eq!(hist_bin(HIST_MAX), HIST_BINS - 1);
        assert_eq!(hist_bin(1e300), HIST_BINS - 1);
        assert_eq!(hist_bin(f64::INFINITY), HIST_BINS - 1);
        assert_eq!(hist_bin(f64::NEG_INFINITY), 0);
        assert_eq!(hist_bin(-1e12), 0);
        assert_eq!(hist_bin(HIST_MAX * (1.0 - 1e-12)), HIST_BINS - 1);
        assert_eq!(hist_bin_value(ZERO_BIN), 0.0);
        assert_eq!(hist_bin_value(ZERO_BIN + 1), 8.0);
        assert_eq!(hist_bin_value(ZERO_BIN - 1), -8.0);
    }

    #[test]
    fn bins_tile_the_line_and_round_trip() {
        let mut prev_hi = -HIST_MAX;
        for bin in 0..HIST_BINS {
            let (lo, hi) = hist_bin_edges(bin);
            assert_eq!(lo, prev_hi, "bin {bin} is contiguous");
            if bin == ZERO_BIN {
                assert_eq!((lo, hi), (0.0, 0.0));
                continue;
            }
            assert!(hi > lo);
            prev_hi = hi;
            let mid = hist_bin_value(bin);
            assert_eq!(hist_bin(mid), bin);
            if bin > ZERO_BIN {
                // Lower edge inclusive, upper edge exclusive.
                assert_eq!(hist_bin(lo.max(f64::MIN_POSITIVE)), bin);
                assert_eq!(hist_bin(f64::from_bits(hi.to_bits() - 1)), bin);
            } else {
                assert_eq!(hist_bin(hi.min(-f64::MIN_POSITIVE)), bin);
                assert_eq!(hist_bin(-f64::from_bits((-lo).to_bits() - 1)), bin);
            }
            // Mirror symmetry.
            let mirror = 2 * ZERO_BIN - bin;
            assert_eq!(hist_bin_edges(mirror), (-hi, -lo));
        }
        assert_eq!(prev_hi, HIST_MAX);
    }

    #[test]
    fn relative_resolution_is_better_than_two_and_a_half_percent() {
        let mut worst: f64 = 0.0;
        for bin in hist_bin(1_000.0)..=hist_bin(1e9) {
            let (lo, hi) = hist_bin_edges(bin);
            worst = worst.max((hi - lo) / lo);
        }
        // $16 on the bin starting at $992 is the widest in relative terms.
        assert!(worst <= 16.0 / 992.0 + 1e-12, "{worst}");
        assert!(worst < 0.025);
        assert!(hist_bin(1e9) < HIST_BINS - 1);
    }

    fn exact_quantile(sorted: &[f64], q: f64) -> f64 {
        let pos = q * (sorted.len() - 1) as f64;
        let i = pos.floor() as usize;
        let frac = pos - i as f64;
        if i + 1 < sorted.len() {
            sorted[i] + frac * (sorted[i + 1] - sorted[i])
        } else {
            sorted[i]
        }
    }

    #[test]
    fn quantiles_match_exact_quantiles_of_lognormal_samples() {
        // Median $800k with sigma 1.2: spans roughly $50k to $13M between the 1st and
        // 99th percentiles, as late-life net worth does.
        let mut rng = Rng::from_seed(20_261_117);
        let mut counts = vec![0u32; HIST_BINS];
        let mut samples = Vec::with_capacity(100_000);
        for _ in 0..100_000 {
            let x = rng.lognormal(crate::math::ln(800_000.0), 1.2);
            counts[hist_bin(x)] += 1;
            samples.push(x);
        }
        samples.sort_by(f64::total_cmp);
        let levels = [
            0.001, 0.01, 0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95, 0.99, 0.999,
        ];
        let mut got = [0.0; 11];
        quantiles(&counts, &levels, &mut got);
        for (q, g) in levels.iter().zip(got) {
            let exact = exact_quantile(&samples, *q);
            let rel = (g - exact).abs() / exact;
            // Bins are at most 1.5625% wide; interpolation gets well inside that.
            assert!(rel < 0.004, "q={q}: {g} vs {exact} ({rel})");
        }
    }

    #[test]
    fn quantiles_handle_zero_negative_and_empty() {
        let levels = [0.0, 0.1, 0.5, 0.9, 1.0];
        let mut out = [0.0; 5];
        let mut counts = vec![0u32; HIST_BINS];
        quantiles(&counts, &levels, &mut out);
        assert!(out.iter().all(|v| v.is_nan()));

        // 60% exactly zero, 20% at -$5,000, 20% at $1,000,000.
        counts[hist_bin(0.0)] = 60;
        counts[hist_bin(-5_000.0)] = 20;
        counts[hist_bin(1_000_000.0)] = 20;
        quantiles(&counts, &levels, &mut out);
        let (nlo, nhi) = hist_bin_edges(hist_bin(-5_000.0));
        let (plo, phi) = hist_bin_edges(hist_bin(1_000_000.0));
        assert_eq!(out[0], nlo);
        assert_eq!(out[1], nlo + 0.5 * (nhi - nlo));
        assert_eq!(out[2], 0.0, "the median of a mass at zero is exactly zero");
        assert_eq!(out[3], plo + 0.5 * (phi - plo));
        assert_eq!(out[4], phi);

        // A single observation.
        let mut one = vec![0u32; HIST_BINS];
        one[hist_bin(250.0)] = 1;
        quantiles(&one, &levels, &mut out);
        assert_eq!(out[0], 240.0);
        assert_eq!(out[2], 248.0);
        assert_eq!(out[4], 256.0);
    }

    #[test]
    fn varints_round_trip() {
        for v in [
            0u64,
            1,
            127,
            128,
            300,
            16_383,
            16_384,
            u32::MAX as u64,
            u64::MAX,
        ] {
            let mut buf = Vec::new();
            put_varint(v, &mut buf);
            let mut pos = 0;
            assert_eq!(get_varint(&buf, &mut pos), Some(v));
            assert_eq!(pos, buf.len());
        }
        assert_eq!(get_varint(&[0x80], &mut 0), None);
        assert_eq!(get_varint(&[0xff; 11], &mut 0), None);
    }

    #[test]
    fn pack_round_trips_and_is_compact() {
        let mut rng = Rng::from_seed(7);
        let blocks = 6;
        let mut hist = vec![0u32; blocks * HIST_BINS];
        // Block 0 empty; 1 a single bin; 2 dense; 3 sparse with gaps of every size;
        // 4 the two extreme bins; 5 large counts.
        hist[HIST_BINS + 17] = 1;
        for _ in 0..5_000 {
            let x = rng.lognormal(crate::math::ln(500_000.0), 0.8);
            hist[2 * HIST_BINS + hist_bin(x)] += 1;
        }
        let mut at = 5;
        for gap in 1..40 {
            hist[3 * HIST_BINS + at] = gap as u32;
            at += gap;
        }
        hist[4 * HIST_BINS] = 3;
        hist[5 * HIST_BINS - 1] = 4;
        hist[5 * HIST_BINS + ZERO_BIN] = u32::MAX;
        hist[5 * HIST_BINS + ZERO_BIN + 1] = 70_000;

        let bytes = pack(&hist);
        assert_eq!(unpack(&bytes, blocks).as_deref(), Some(&hist[..]));
        // Empty block: one byte. Single bin: runs, skip, len, count.
        assert_eq!(bytes[0], 0);
        assert_eq!(&bytes[1..5], &[1, 17, 1, 1]);
        let occupied = hist.iter().filter(|&&c| c != 0).count();
        assert!(bytes.len() < 3 * occupied, "{} bytes", bytes.len());

        // Malformed input is rejected, never a panic.
        assert!(unpack(&bytes[..bytes.len() - 1], blocks).is_none());
        assert!(unpack(&bytes, blocks + 1).is_none());
        assert!(unpack(&bytes, blocks - 1).is_none());
        assert!(unpack(&[1, 0, 0], 1).is_none(), "zero-length run");
        assert!(
            unpack(&[1, 0xff, 0x7f, 1, 1], 1).is_none(),
            "run past the end"
        );
        assert!(unpack(&[], 0).is_some());
    }

    #[test]
    fn runs_split_only_at_long_gaps() {
        let mut block = vec![0u32; HIST_BINS];
        for i in [10, 11, 13, 16, 20, 21] {
            block[i] = 1;
        }
        let mut runs = Vec::new();
        for_each_run(&block, |a, b| runs.push((a, b)));
        // 13 -> 16 has two empty bins (kept in the run); 16 -> 20 has three (split).
        assert_eq!(runs, vec![(10, 17), (20, 22)]);
    }
}
