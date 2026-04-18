//! Relevance ranking.
//!
//! Two-scorer cascade:
//!   * **BM25** (`bm25.rs`) — query/document relevance.
//!   * **TF-IDF density** — fallback when no query is provided, rewards
//!     chunks with lots of distinctive terms.
//!
//! Then we add bumps for `priority`, `kind`, and an optional **PageRank
//! prior** supplied by the graph crate so centrally-connected symbols float
//! to the top even when their text doesn't match the query directly.

pub mod bm25;

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

    let query_terms: Vec<String> = query
        .map(|q| tokenize_words(q))
        .unwrap_or_default();

    let mut scored: Vec<(usize, f64)> = (0..chunks.len())
        .map(|i| {
            let base = if query_terms.is_empty() {
                density_score(&tokenised[i], &df, n_docs)
            } else {
                corpus.score(i, &query_terms) * 10.0
                    + density_score(&tokenised[i], &df, n_docs) * 0.5
            };
            (i, base)
        })
        .collect();

    for (i, score) in scored.iter_mut() {
        let c = &chunks[*i];
        *score += c.priority as f64 * 0.5;
        *score += match c.kind {
            ChunkKind::Selection => 5.0,
            ChunkKind::Diagnostic => 2.0,
            ChunkKind::Code => 0.0,
            ChunkKind::Doc => -0.5,
            ChunkKind::Comment => -1.0,
        };
        if let Some(p) = priors {
            if let Some(v) = p.get(&c.id) {
                // PageRank scores are tiny (1e-4 range). Scale so they're
                // comparable to BM25 output.
                *score += v * 1000.0;
            }
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let reordered: Vec<InputChunk> = scored
        .into_iter()
        .map(|(i, _)| chunks[i].clone())
        .collect();
    *chunks = reordered;
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
