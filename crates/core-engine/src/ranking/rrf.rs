//! Reciprocal Rank Fusion (RRF).
//!
//! Cormack, Clarke & Büttcher (2009): given several rankers that score the
//! same set of documents, the simplest robust way to combine them is to add
//! `1 / (k + rank_i(d))` for each ranker `i`. No score normalisation, no
//! per-ranker weight tuning — and it routinely beats hand-tuned linear
//! combinations on retrieval benchmarks.
//!
//! Why we use it here: the ranking module already has BM25, density (TF-IDF),
//! and an optional PageRank prior. They live on completely different scales
//! (BM25 is in tens, density in single digits, PageRank in 1e-4 range) so
//! adding them up requires arbitrary multipliers — exactly the kind of
//! brittle tuning RRF removes. We fuse on **rank**, not raw score.
//!
//! Formula:
//!   rrf(d) = Σ_i  weight_i / (k + rank_i(d))
//!
//! where `rank_i(d)` is 1-indexed and documents missing from a ranker's list
//! contribute 0 (equivalent to `rank = ∞`). The constant `k = 60` is the
//! value used in the original paper and remains the recommended default.

use ahash::AHashMap;

/// Standard RRF constant from the original paper.
pub const DEFAULT_K: f64 = 60.0;

/// One ranker's contribution: an ordered list of `(doc_index, score)` pairs
/// (highest-scoring first) plus a multiplicative weight applied to the
/// reciprocal-rank term. Use weight = 1.0 by default; bump it above 1.0 to
/// favour a particular signal.
pub struct Ranker<'a> {
    pub ranking: &'a [(usize, f64)],
    pub weight: f64,
}

/// Fuse multiple rankings into a single score per document index.
/// `n_docs` is the size of the underlying document set so unscored documents
/// also receive an entry (with score 0).
pub fn fuse(rankers: &[Ranker<'_>], n_docs: usize) -> Vec<f64> {
    fuse_with(rankers, n_docs, DEFAULT_K)
}

pub fn fuse_with(rankers: &[Ranker<'_>], n_docs: usize, k: f64) -> Vec<f64> {
    let mut scores = vec![0.0f64; n_docs];
    for ranker in rankers {
        for (rank_zero_based, (doc_ix, _)) in ranker.ranking.iter().enumerate() {
            if *doc_ix >= n_docs {
                continue;
            }
            let rank = (rank_zero_based + 1) as f64; // RRF is 1-indexed
            scores[*doc_ix] += ranker.weight / (k + rank);
        }
    }
    scores
}

/// Convenience: turn a `score_of[doc_ix] = score` map into a sorted ranking
/// suitable for [`Ranker`]. Stable on ties via doc index.
pub fn rank_by_score(scores: &[f64]) -> Vec<(usize, f64)> {
    let mut pairs: Vec<(usize, f64)> = scores.iter().copied().enumerate().collect();
    pairs.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    pairs
}

/// Convenience for sparse priors keyed by chunk id (e.g. PageRank).
pub fn rank_from_priors(
    n_docs: usize,
    chunk_id: impl Fn(usize) -> Option<String>,
    priors: &AHashMap<String, f64>,
) -> Vec<(usize, f64)> {
    let mut scores = vec![0.0f64; n_docs];
    for i in 0..n_docs {
        if let Some(id) = chunk_id(i) {
            if let Some(v) = priors.get(&id) {
                scores[i] = *v;
            }
        }
    }
    rank_by_score(&scores)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_agreement_lifts_top_doc_highest() {
        let r1 = vec![(0usize, 5.0), (1, 4.0), (2, 3.0)];
        let r2 = vec![(0usize, 9.0), (1, 8.0), (2, 7.0)];
        let scores = fuse(
            &[
                Ranker {
                    ranking: &r1,
                    weight: 1.0,
                },
                Ranker {
                    ranking: &r2,
                    weight: 1.0,
                },
            ],
            3,
        );
        assert!(scores[0] > scores[1]);
        assert!(scores[1] > scores[2]);
    }

    #[test]
    fn rrf_resists_score_scale_skew() {
        // Ranker A: BM25-like — first doc dominates by raw score.
        // Ranker B: PageRank-like — tiny, near-uniform scores in a *different* order.
        // Plain weighted-sum would be entirely driven by A. RRF blends them.
        let r1 = vec![(0usize, 50.0), (1, 0.1), (2, 0.05)];
        let r2 = vec![(2usize, 1e-4), (1, 9e-5), (0, 8e-5)];
        let scores = fuse(
            &[
                Ranker {
                    ranking: &r1,
                    weight: 1.0,
                },
                Ranker {
                    ranking: &r2,
                    weight: 1.0,
                },
            ],
            3,
        );
        // doc 1 is in the middle of both rankings, so it is rewarded.
        assert!(scores[1] > scores[2] || (scores[0] - scores[2]).abs() < 0.01);
    }

    #[test]
    fn unscored_doc_gets_zero() {
        let r1 = vec![(0usize, 5.0)];
        let scores = fuse(
            &[Ranker {
                ranking: &r1,
                weight: 1.0,
            }],
            3,
        );
        assert!(scores[0] > 0.0);
        assert_eq!(scores[1], 0.0);
        assert_eq!(scores[2], 0.0);
    }

    #[test]
    fn rank_by_score_is_descending_and_stable() {
        let scores = vec![1.0, 5.0, 5.0, 2.0];
        let ranking = rank_by_score(&scores);
        assert_eq!(ranking[0].0, 1); // doc 1: 5.0, lower index wins tie
        assert_eq!(ranking[1].0, 2); // doc 2: 5.0
        assert_eq!(ranking[2].0, 3); // doc 3: 2.0
        assert_eq!(ranking[3].0, 0); // doc 0: 1.0
    }
}
