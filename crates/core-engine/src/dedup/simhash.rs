//! SimHash for token-level near-duplicate detection.
//!
//! Complements [`super::minhash`]. Where MinHash treats a chunk as a *set* of
//! shingles and estimates Jaccard, SimHash treats it as a *bag* of weighted
//! features and estimates cosine-style similarity via Hamming distance.
//!
//! For source code this catches a different failure mode: two chunks that
//! share the same set of identifiers but in different orderings/frequencies
//! (e.g. small refactors that move statements around) often score lower on
//! Jaccard but very high on SimHash because the dominant tokens are the same.
//!
//! Algorithm (Charikar 2002):
//!   1. Tokenise the chunk into weighted features.
//!   2. For each feature, compute a 64-bit hash.
//!   3. For each bit position `i` in the 64-bit fingerprint, sum the feature
//!      weight if bit `i` is 1, subtract it if bit `i` is 0.
//!   4. Final fingerprint bit `i` = sign of the column sum.
//!
//! Two chunks are "similar" when the Hamming distance between their
//! fingerprints is small. For 64-bit SimHash, ≤ 3 bits flipped is the
//! commonly-cited boundary for "near-duplicate".

use ahash::AHashMap;
use contextos_utils::{fast_hash, tokenize_words};

/// Number of bits in the fingerprint. 64 is the workhorse choice — fits in a
/// register, and 64-bit Hamming distance is one popcnt instruction.
pub const BITS: u32 = 64;

/// Default Hamming-distance threshold for "near-duplicate" at 64 bits.
/// Empirically: ≤3 catches edits up to a few lines without collapsing
/// genuinely distinct functions.
pub const DEFAULT_HAMMING_THRESHOLD: u32 = 3;

/// SimHash on tiny token bags is noise — two chunks like `fn x() {}` and
/// `fn y() {}` share their entire alphanumeric vocabulary after stopword
/// removal and would collapse incorrectly. Below this many unique tokens we
/// emit the sentinel `0` so the dedup pass falls through to MinHash/Jaccard.
pub const MIN_UNIQUE_TOKENS: usize = 4;

/// 64-bit SimHash fingerprint of a chunk's token bag.
///
/// Returns `0` (a sentinel meaning "skip") when the chunk has fewer than
/// [`MIN_UNIQUE_TOKENS`] distinct tokens — too few features to fingerprint
/// reliably.
pub fn simhash(text: &str) -> u64 {
    let tokens = tokenize_words(text);
    if tokens.is_empty() {
        return 0;
    }

    // Weight each token by its frequency in the chunk. This is the standard
    // Charikar formulation; weighting by tf-idf would require a corpus.
    let mut weights: AHashMap<String, i32> = AHashMap::new();
    for t in tokens {
        *weights.entry(t).or_insert(0) += 1;
    }
    if weights.len() < MIN_UNIQUE_TOKENS {
        return 0;
    }

    let mut columns = [0i64; BITS as usize];
    for (token, w) in &weights {
        let h = fast_hash(token);
        for (i, col) in columns.iter_mut().enumerate() {
            if (h >> i) & 1 == 1 {
                *col += *w as i64;
            } else {
                *col -= *w as i64;
            }
        }
    }

    let mut fp = 0u64;
    for (i, col) in columns.iter().enumerate() {
        if *col > 0 {
            fp |= 1u64 << i;
        }
    }
    fp
}

/// Hamming distance between two 64-bit SimHash fingerprints.
#[inline]
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Estimate cosine-style similarity in [0, 1] from a Hamming distance.
/// At 64 bits, distance 0 → 1.0, distance 32 (random) → 0.0.
#[inline]
pub fn similarity(a: u64, b: u64) -> f64 {
    let d = hamming(a, b) as f64;
    1.0 - (d / BITS as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_has_zero_distance() {
        let a = simhash("fn add(a: i32, b: i32) -> i32 { a + b }");
        let b = simhash("fn add(a: i32, b: i32) -> i32 { a + b }");
        assert_eq!(hamming(a, b), 0);
    }

    #[test]
    fn whitespace_only_changes_are_close() {
        let a = simhash("fn add(a: i32, b: i32) -> i32 { a + b }");
        let b = simhash("fn   add(a: i32,   b: i32) -> i32   { a + b }");
        assert!(hamming(a, b) <= DEFAULT_HAMMING_THRESHOLD);
    }

    #[test]
    fn unrelated_text_has_large_distance() {
        let a = simhash("parse stripe webhook payment intent verification");
        let b = simhash("render canvas pixel shader fragment opengl context");
        assert!(hamming(a, b) > 8);
    }

    #[test]
    fn empty_text_returns_zero() {
        assert_eq!(simhash(""), 0);
    }

    #[test]
    fn similarity_is_inverse_of_hamming() {
        let a = simhash("alpha beta gamma delta epsilon");
        let b = simhash("alpha beta gamma delta epsilon");
        assert!((similarity(a, b) - 1.0).abs() < 1e-9);
        let c = simhash("xxx yyy zzz www qqq");
        assert!(similarity(a, c) < 0.8);
    }
}
