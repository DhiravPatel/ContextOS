//! MinHash + LSH for probabilistic near-duplicate detection.
//!
//! The pairwise Jaccard pass in [`super::run`] is O(n²) in the worst case.
//! This module gives us O(n · perm) construction and O(n) candidate lookup
//! by banding the signature into LSH buckets.
//!
//! We keep it simple and dependency-free:
//!   * `PERMUTATIONS = 128` — good recall/precision trade for ~0.8 threshold.
//!   * `BANDS = 16`, `ROWS = 8` — tuned so P(collision | Jaccard=0.85) ≈ 0.99.
//!
//! Hash family: two independent 64-bit AHashers seeded from constants; for
//! each permutation `i`, `h_i(x) = h1(x) + i · h2(x)` (standard trick — no
//! need to keep 128 separate hash states).

use ahash::{AHashMap, AHashSet};
use std::hash::{BuildHasher, Hasher};

pub const PERMUTATIONS: usize = 128;
pub const BANDS: usize = 16;
pub const ROWS: usize = PERMUTATIONS / BANDS;

pub type Signature = [u64; PERMUTATIONS];

pub fn signature_of(shingles: &[u64]) -> Signature {
    let mut sig = [u64::MAX; PERMUTATIONS];
    if shingles.is_empty() {
        return sig;
    }
    for &x in shingles {
        let h1 = mix(x, 0x9E37_79B9_7F4A_7C15);
        let h2 = mix(x, 0xC2B2_AE35_07C3_B3F5).max(1);
        for i in 0..PERMUTATIONS {
            let hi = h1.wrapping_add((i as u64).wrapping_mul(h2));
            if hi < sig[i] {
                sig[i] = hi;
            }
        }
    }
    sig
}

fn mix(x: u64, seed: u64) -> u64 {
    let hasher = ahash::RandomState::with_seeds(seed, seed ^ 0xDEAD_BEEF, 0x1234_5678, 0x87_65_43_21);
    let mut h = hasher.build_hasher();
    h.write_u64(x);
    h.finish()
}

/// Estimate Jaccard similarity from two signatures (fraction of equal
/// permutations). Cheap: O(PERMUTATIONS).
pub fn jaccard(a: &Signature, b: &Signature) -> f64 {
    let mut eq = 0usize;
    for i in 0..PERMUTATIONS {
        if a[i] == b[i] {
            eq += 1;
        }
    }
    eq as f64 / PERMUTATIONS as f64
}

/// Shingle a document into `w`-token rolling windows, hashed.
/// For source code, `w = 5` (lines) or `w = 8` (tokens) works well.
pub fn line_shingles(text: &str, width: usize) -> Vec<u64> {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let width = width.max(1).min(lines.len());
    let mut out = Vec::with_capacity(lines.len().saturating_sub(width) + 1);
    for i in 0..=lines.len() - width {
        let joined = lines[i..i + width].join("\n");
        out.push(contextos_utils::fast_hash(&joined));
    }
    out
}

/// LSH index: for each band, bucket docs by the hash of that band slice.
/// Two docs whose Jaccard ≥ ~0.8 collide in at least one band with high
/// probability.
pub struct LshIndex {
    buckets: Vec<AHashMap<u64, Vec<usize>>>, // per-band
}

impl LshIndex {
    pub fn new() -> Self {
        Self {
            buckets: (0..BANDS).map(|_| AHashMap::new()).collect(),
        }
    }

    pub fn insert(&mut self, doc_ix: usize, sig: &Signature) {
        for b in 0..BANDS {
            let key = band_hash(sig, b);
            self.buckets[b].entry(key).or_default().push(doc_ix);
        }
    }

    /// All prior-inserted doc indices that share at least one band with `sig`.
    pub fn candidates(&self, sig: &Signature) -> AHashSet<usize> {
        let mut out = AHashSet::new();
        for b in 0..BANDS {
            let key = band_hash(sig, b);
            if let Some(ids) = self.buckets[b].get(&key) {
                for &i in ids {
                    out.insert(i);
                }
            }
        }
        out
    }
}

impl Default for LshIndex {
    fn default() -> Self {
        Self::new()
    }
}

fn band_hash(sig: &Signature, band: usize) -> u64 {
    let start = band * ROWS;
    let slice = &sig[start..start + ROWS];
    let mut h: u64 = 0xcbf29ce484222325;
    for &x in slice {
        h ^= x;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_texts_signature_matches() {
        let a = line_shingles("a\nb\nc\nd\ne", 3);
        let b = line_shingles("a\nb\nc\nd\ne", 3);
        assert!((jaccard(&signature_of(&a), &signature_of(&b)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn disjoint_signatures_low_jaccard() {
        let a = line_shingles("cat\ndog\nfish\nbird\nrabbit", 3);
        let b = line_shingles("alpha\nbeta\ngamma\ndelta\nepsilon", 3);
        assert!(jaccard(&signature_of(&a), &signature_of(&b)) < 0.3);
    }

    #[test]
    fn lsh_finds_similar_docs() {
        let base = "line one\nline two\nline three\nline four\nline five\nline six";
        let near = "line one\nline two\nline three\nline four\nline five\nline seven";
        let far = "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta";

        let sa = signature_of(&line_shingles(base, 3));
        let sb = signature_of(&line_shingles(near, 3));
        let sc = signature_of(&line_shingles(far, 3));

        let mut idx = LshIndex::new();
        idx.insert(0, &sa);
        idx.insert(1, &sb);
        idx.insert(2, &sc);

        let cands = idx.candidates(&sa);
        assert!(cands.contains(&1), "near-duplicate must appear as candidate");
    }
}
