//! Relevance ranking.
//!
//! Three independent rankers, fused via Reciprocal Rank Fusion:
//!   * **BM25** (`bm25.rs`) — query/document relevance.
//!   * **TF-IDF density** — query-free distinctiveness signal.
//!   * **Prior** — optional external signal (e.g. graph PageRank keyed by
//!     chunk id), normalised to a per-chunk rank.
//!
//! Each ranker emits a sorted list of `(doc_index, score)` pairs and
//! [`rrf::fuse`] combines them on **rank** rather than raw score. This
//! sidesteps the previous implementation's brittle multipliers (BM25 × 10,
//! density × 0.5, PageRank × 1000) which were tuned to bring three very
//! differently-scaled signals onto the same axis. RRF requires no such
//! calibration and is empirically equal-or-better than tuned weighted sums
//! on retrieval benchmarks.
//!
//! After fusion we add **structural bumps** for `priority` and `ChunkKind`
//! at a much smaller scale than before — they nudge ties, they no longer
//! dominate the score.

pub mod bm25;
pub mod rrf;

use crate::types::{ChunkKind, InputChunk};
use ahash::AHashMap;
use contextos_utils::tokenize_words;

/// Optional external signals (e.g. graph PageRank keyed by chunk id).
pub type Priors = AHashMap<String, f64>;

pub fn run(chunks: &mut Vec<InputChunk>, query: Option<&str>) {
    run_with_priors(chunks, query, None)
}

pub fn run_with_priors(
    chunks: &mut Vec<InputChunk>,
    query: Option<&str>,
    priors: Option<&Priors>,
) {
    if chunks.len() < 2 {
        return;
    }

    let tokenised: Vec<Vec<String>> = chunks
        .iter()
        .map(|c| tokenize_words(&c.content))
        .collect();

    let corpus = bm25::Corpus::build(tokenised.clone());
    let n_docs = chunks.len() as f64;

    let mut df: AHashMap<String, usize> = AHashMap::new();
    for doc in &tokenised {
        let mut seen: ahash::AHashSet<&String> = ahash::AHashSet::new();
        for w in doc {
            if seen.insert(w) {
                *df.entry(w.clone()).or_insert(0) += 1;
            }
        }
    }

    let query_terms: Vec<String> = query.map(|q| tokenize_words(q)).unwrap_or_default();

    // Per-ranker score vectors (one entry per chunk index).
    let bm25_scores: Vec<f64> = if query_terms.is_empty() {
        vec![0.0; chunks.len()]
    } else {
        (0..chunks.len())
            .map(|i| corpus.score(i, &query_terms))
            .collect()
    };
    let density_scores: Vec<f64> = (0..chunks.len())
        .map(|i| density_score(&tokenised[i], &df, n_docs))
        .collect();
    let prior_scores: Vec<f64> = (0..chunks.len())
        .map(|i| match priors {
            Some(p) => p.get(&chunks[i].id).copied().unwrap_or(0.0),
            None => 0.0,
        })
        .collect();

    let bm25_ranking = rrf::rank_by_score(&bm25_scores);
    let density_ranking = rrf::rank_by_score(&density_scores);
    let prior_ranking = rrf::rank_by_score(&prior_scores);

    // When a query is present, BM25 carries the most direct relevance signal
    // and we lean on it; density is the only signal in query-free mode.
    let (bm25_w, density_w, prior_w) = if query_terms.is_empty() {
        (0.0, 1.5, 1.0)
    } else {
        (1.5, 0.7, 1.0)
    };

    let mut rankers: Vec<rrf::Ranker<'_>> = Vec::with_capacity(3);
    // A ranker contributes only if its top doc has a strictly positive score.
    // Otherwise every doc ties at zero and RRF would inject pure index noise
    // (whoever appears first wins). Common case: the query terms don't match
    // anything in the corpus after compress strips comments.
    if bm25_w > 0.0 && top_score(&bm25_ranking) > 0.0 {
        rankers.push(rrf::Ranker {
            ranking: &bm25_ranking,
            weight: bm25_w,
        });
    }
    if top_score(&density_ranking) > 0.0 {
        rankers.push(rrf::Ranker {
            ranking: &density_ranking,
            weight: density_w,
        });
    }
    if priors.is_some() && top_score(&prior_ranking) > 0.0 {
        rankers.push(rrf::Ranker {
            ranking: &prior_ranking,
            weight: prior_w,
        });
    }

    let fused = rrf::fuse(&rankers, chunks.len());

    // Structural bumps live on the same scale as a single RRF term
    // (~1/k = ~1/60). They tip ties without overpowering the relevance
    // signal — the old multipliers were 10–1000× larger and effectively
    // *replaced* the ranker output.
    const BUMP: f64 = 1.0 / rrf::DEFAULT_K;
    let mut scored: Vec<(usize, f64)> = (0..chunks.len()).map(|i| (i, fused[i])).collect();
    for (i, score) in scored.iter_mut() {
        let c = &chunks[*i];
        *score += c.priority as f64 * BUMP;
        *score += match c.kind {
            ChunkKind::Selection => 5.0 * BUMP,
            ChunkKind::Diagnostic => 2.0 * BUMP,
            ChunkKind::Code => 0.0,
            ChunkKind::Doc => -0.5 * BUMP,
            ChunkKind::Comment => -1.0 * BUMP,
        };
    }

    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    let reordered: Vec<InputChunk> = scored.into_iter().map(|(i, _)| chunks[i].clone()).collect();
    *chunks = reordered;
}

fn top_score(ranking: &[(usize, f64)]) -> f64 {
    ranking.first().map(|(_, s)| *s).unwrap_or(0.0)
}

fn density_score(doc: &[String], df: &AHashMap<String, usize>, n_docs: f64) -> f64 {
    if doc.is_empty() {
        return 0.0;
    }
    let doc_len = doc.len() as f64;
    let mut tf: AHashMap<&String, usize> = AHashMap::new();
    for w in doc {
        *tf.entry(w).or_insert(0) += 1;
    }
    tf.iter()
        .map(|(w, count)| {
            let df_w = *df.get(*w).unwrap_or(&1) as f64;
            let idf = ((n_docs + 1.0) / (df_w + 1.0)).ln() + 1.0;
            (*count as f64 / doc_len) * idf
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChunkKind, InputChunk};
    use contextos_utils::Language;

    fn c(id: &str, content: &str) -> InputChunk {
        InputChunk {
            id: id.into(),
            path: None,
            language: Language::Rust,
            content: content.into(),
            kind: ChunkKind::Code,
            priority: 0,
            skeleton_hint: false,
        }
    }

    #[test]
    fn bm25_surfaces_query_match() {
        let mut v = vec![
            c("unrelated", "fn render_ui() { draw_pixels(); }"),
            c("target", "fn parse_auth_token(s: &str) -> Token {}"),
            c("noise", "struct Pixel;"),
        ];
        run(&mut v, Some("auth token"));
        assert_eq!(v[0].id, "target");
    }

    #[test]
    fn pagerank_prior_tilts_ranking() {
        let mut v = vec![
            c("central", "pub fn router() {}"),
            c("leaf", "pub fn unused() {}"),
        ];
        let mut priors: Priors = AHashMap::new();
        priors.insert("central".into(), 0.9);
        run_with_priors(&mut v, None, Some(&priors));
        assert_eq!(v[0].id, "central");
    }
}
