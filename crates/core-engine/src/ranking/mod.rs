//! Relevance ranking.
//!
//! Scores each chunk with a lightweight TF-IDF against the caller-supplied
//! query, then applies priority and kind bonuses. Writes the score into a
//! parallel vector (sort order is baked into the chunk list afterwards).
//!
//! This is intentionally not perfect — the goal is to cheaply bubble the
//! most plausibly relevant chunks to the top so that [`budget`](crate::budget)
//! fills its quota with useful material.

use crate::types::{ChunkKind, InputChunk};
use ahash::AHashMap;
use contextos_utils::tokenize_words;

pub fn run(chunks: &mut Vec<InputChunk>, query: Option<&str>) {
    if chunks.len() < 2 {
        return;
    }

    // Tokenise each chunk once and build document frequencies.
    let tokenised: Vec<Vec<String>> = chunks
        .iter()
        .map(|c| tokenize_words(&c.content))
        .collect();

    let mut df: AHashMap<String, usize> = AHashMap::new();
    for doc in &tokenised {
        let mut seen: ahash::AHashSet<&String> = ahash::AHashSet::new();
        for w in doc {
            if seen.insert(w) {
                *df.entry(w.clone()).or_insert(0) += 1;
            }
        }
    }
    let n_docs = chunks.len() as f64;

    let query_terms: Vec<String> = query
        .map(|q| tokenize_words(q))
        .unwrap_or_default();

    let mut scored: Vec<(usize, f64)> = tokenised
        .iter()
        .enumerate()
        .map(|(i, doc)| (i, score_doc(doc, &query_terms, &df, n_docs)))
        .collect();

    // Apply per-chunk bumps (priority + chunk-kind bias).
    for (i, score) in scored.iter_mut() {
        let c = &chunks[*i];
        *score += c.priority as f64 * 0.5;
        *score += match c.kind {
            ChunkKind::Selection => 5.0, // user explicitly pointed at this
            ChunkKind::Diagnostic => 2.0,
            ChunkKind::Code => 0.0,
            ChunkKind::Doc => -0.5,
            ChunkKind::Comment => -1.0,
        };
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let reordered: Vec<InputChunk> = scored
        .into_iter()
        .map(|(i, _)| chunks[i].clone())
        .collect();
    *chunks = reordered;
}

fn score_doc(
    doc: &[String],
    query: &[String],
    df: &AHashMap<String, usize>,
    n_docs: f64,
) -> f64 {
    if doc.is_empty() {
        return 0.0;
    }

    // Base score: density of "distinctive" terms (high IDF words).
    let doc_len = doc.len() as f64;
    let mut tf: AHashMap<&String, usize> = AHashMap::new();
    for w in doc {
        *tf.entry(w).or_insert(0) += 1;
    }

    let base: f64 = tf
        .iter()
        .map(|(w, count)| {
            let df_w = *df.get(*w).unwrap_or(&1) as f64;
            let idf = ((n_docs + 1.0) / (df_w + 1.0)).ln() + 1.0;
            (*count as f64 / doc_len) * idf
        })
        .sum();

    if query.is_empty() {
        return base;
    }

    // Query score: sum of TF-IDF for each query term in the doc.
    let q_score: f64 = query
        .iter()
        .map(|qt| {
            let count = tf.get(qt).copied().unwrap_or(0) as f64;
            if count == 0.0 {
                0.0
            } else {
                let df_w = *df.get(qt).unwrap_or(&1) as f64;
                let idf = ((n_docs + 1.0) / (df_w + 1.0)).ln() + 1.0;
                (count / doc_len) * idf
            }
        })
        .sum();

    base + q_score * 10.0 // query matches dominate when present
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
        }
    }

    #[test]
    fn query_matching_chunks_bubble_up() {
        let mut v = vec![
            c("unrelated", "fn render_ui() { draw_pixels(); }"),
            c("target", "fn parse_auth_token(s: &str) -> Token {}"),
            c("also_unrelated", "struct Pixel;"),
        ];
        run(&mut v, Some("auth token"));
        assert_eq!(v[0].id, "target");
    }

    #[test]
    fn selection_kind_is_boosted_even_without_query() {
        let mut v = vec![
            c("a", "some random code here"),
            InputChunk {
                kind: ChunkKind::Selection,
                ..c("b", "selected by user")
            },
        ];
        run(&mut v, None);
        assert_eq!(v[0].id, "b");
    }
}
