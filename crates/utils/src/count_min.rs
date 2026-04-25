//! Count-Min Sketch — sublinear approximate frequency estimator.
//!
//! A `d × w` table of counters with `d` independent hash functions. To
//! `add(x)`, increment `t[i][h_i(x) mod w]` for each row `i`. To
//! `count(x)`, return `min_i t[i][h_i(x) mod w]`. The estimator never
//! under-counts (every collision can only push counts up) and the
//! over-count is bounded:
//!
//! ```text
//! P(error > εN) ≤ δ   when   w ≥ ⌈e/ε⌉   and   d ≥ ⌈ln(1/δ)⌉
//! ```
//!
//! For our compress-boilerplate use case we want "something that's clearly
//! repeated"; ε = 0.001 (one-tenth of a percent of total adds) and δ =
//! 1e-4 give `w = 2719`, `d = 10` — about 27 KiB of memory regardless of
//! input size. Trivially within the engine's footprint, and constant-
//! time per add/count regardless of the corpus volume.
//!
//! Hashing: we use `stable_hash` so the sketch is reproducible across
//! processes. Each row `i` mixes the input with a deterministic per-row
//! seed.

use crate::stable_hash;
use std::hash::Hash;

#[derive(Debug, Clone)]
pub struct CountMinSketch {
    /// rows × columns counter matrix, row-major.
    counters: Vec<u32>,
    rows: usize,
    cols: usize,
    /// Total number of `add` calls — useful when the caller wants to
    /// turn raw counts into a frequency.
    total_adds: u64,
}

impl CountMinSketch {
    /// Construct a sketch sized for the requested `(epsilon, delta)`
    /// tolerance. Memory cost is `≈ 4 · d · w` bytes; with the defaults
    /// from [`Self::default`] that's ~27 KiB.
    pub fn with_tolerances(epsilon: f64, delta: f64) -> Self {
        let cols = (std::f64::consts::E / epsilon).ceil() as usize;
        let rows = (1.0 / delta).ln().ceil() as usize;
        Self::with_dims(rows.max(1), cols.max(1))
    }

    pub fn with_dims(rows: usize, cols: usize) -> Self {
        Self {
            counters: vec![0u32; rows * cols],
            rows,
            cols,
            total_adds: 0,
        }
    }

    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn cols(&self) -> usize {
        self.cols
    }
    pub fn total(&self) -> u64 {
        self.total_adds
    }

    pub fn add<T: Hash + ?Sized>(&mut self, key: &T) {
        self.add_n(key, 1);
    }

    pub fn add_n<T: Hash + ?Sized>(&mut self, key: &T, n: u32) {
        if n == 0 {
            return;
        }
        let base = stable_hash(key);
        for r in 0..self.rows {
            let col = self.col_of(base, r);
            let cell = &mut self.counters[r * self.cols + col];
            *cell = cell.saturating_add(n);
        }
        self.total_adds = self.total_adds.saturating_add(n as u64);
    }

    /// Min-over-rows count estimate. Always ≥ true count, never larger
    /// than `true_count + ε · total_adds` w.h.p.
    pub fn count<T: Hash + ?Sized>(&self, key: &T) -> u32 {
        let base = stable_hash(key);
        let mut min = u32::MAX;
        for r in 0..self.rows {
            let col = self.col_of(base, r);
            let v = self.counters[r * self.cols + col];
            if v < min {
                min = v;
            }
        }
        if min == u32::MAX {
            0
        } else {
            min
        }
    }

    /// Reset all counters to zero. Cheaper than allocating a new sketch.
    pub fn clear(&mut self) {
        for c in &mut self.counters {
            *c = 0;
        }
        self.total_adds = 0;
    }

    fn col_of(&self, base: u64, row: usize) -> usize {
        // h_r(x) = (base ^ (row · 0x9E3779B97F4A7C15)).rot_left(31)
        // — cheap mixing that avoids storing per-row seeds, with the
        // golden-ratio constant for good distribution.
        let mixed = base ^ ((row as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let h = mixed.rotate_left(31).wrapping_mul(0xC6BC_279692B5_C323);
        (h as usize) % self.cols
    }
}

impl Default for CountMinSketch {
    fn default() -> Self {
        Self::with_tolerances(0.001, 1e-4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_is_lower_bounded_by_true_frequency() {
        let mut s = CountMinSketch::default();
        for _ in 0..100 {
            s.add("alpha");
        }
        for _ in 0..3 {
            s.add("beta");
        }
        // Count-Min never under-counts.
        assert!(s.count("alpha") >= 100);
        assert!(s.count("beta") >= 3);
    }

    #[test]
    fn unseen_key_count_is_small() {
        let mut s = CountMinSketch::default();
        for _ in 0..50 {
            s.add("only_real_key");
        }
        // With ε = 0.001, total_adds = 50, expected over-count ≤ 1 with
        // very high probability. Allow a little slack for randomness in
        // the hash distribution but require it to be far below the true
        // counts.
        assert!(s.count("never_seen") <= 5);
    }

    #[test]
    fn add_n_matches_repeated_add() {
        let mut s1 = CountMinSketch::default();
        let mut s2 = CountMinSketch::default();
        for _ in 0..7 {
            s1.add("x");
        }
        s2.add_n("x", 7);
        assert_eq!(s1.count("x"), s2.count("x"));
        assert_eq!(s1.total(), s2.total());
    }

    #[test]
    fn clear_zeros_the_table() {
        let mut s = CountMinSketch::default();
        s.add_n("foo", 999);
        s.clear();
        assert_eq!(s.count("foo"), 0);
        assert_eq!(s.total(), 0);
    }

    #[test]
    fn deterministic_across_instances() {
        let mut s1 = CountMinSketch::default();
        let mut s2 = CountMinSketch::default();
        for k in ["a", "b", "c", "a", "a"] {
            s1.add(k);
            s2.add(k);
        }
        assert_eq!(s1.count("a"), s2.count("a"));
        assert_eq!(s1.count("b"), s2.count("b"));
    }
}
