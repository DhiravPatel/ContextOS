//! RM3 — relevance-model pseudo-relevance feedback.
//!
//! Lavrenko & Croft (2001) showed that you can almost always improve a
//! BM25 retrieval by:
//!
//!   1. Running an initial retrieval with the user's query.
//!   2. Treating the top-`k` docs as a (rough) relevance set.
//!   3. Building a "relevance model" — a probability distribution over
//!      vocabulary, weighted by `tf` in those top docs and by their
//!      query score — and pulling out the highest-weight terms.
//!   4. Mixing those expansion terms into the original query at a
//!      small weight (RM3 = `(1−α) · original + α · expansion`).
//!   5. Running BM25 again with the expanded query.
//!
//! The "3" in RM3 refers to the original-query interpolation step
//! (RM1 and RM2 are pure-expansion variants that perform worse on most
//! benchmarks).
//!
//! ## What we compute
//!
//! [`expand_query`] returns the *expanded* token list. We compute it as:
//!
//! ```text
//! expansion(t)  =  Σ_{d ∈ topK}  bm25(d, q) · tf(t, d) / |d|
//! ```
//!
//! and keep the top `n_terms` tokens by `expansion()`. We then prepend
//! the original query terms so any token already in the query is
//! over-represented (the (1−α) part), giving a simple discrete
//! approximation of the standard RM3 mixing.
//!
//! ## Why we want this
//!
//! Code search queries are often under-specified. "parse stripe webhook"
//! is a fine intent but the actual code may name those concepts as
//! `webhookHandler`, `intentValidator`, `parsePayload`. RM3 finds those
//! associations *from the corpus itself* — no embeddings, no model
//! calls, no extra dependencies. Empirically RM3 lifts BM25 recall by
//! 10–25% on retrieval benchmarks.

use super::bm25;
use ahash::AHashMap;
use contextos_utils::tokenize_words;

/// Default expansion size. 8 tokens is a good "kept enough to recall
/// neighbouring concepts, not so many that the expanded query drowns
/// the original".
pub const DEFAULT_TOP_TERMS: usize = 8;

/// Default count of top documents used as the pseudo-relevance set.
pub const DEFAULT_TOP_DOCS: usize = 5;

/// Standard RM3 mixing weight α — controls how much weight the
/// expansion contributes vs. the original query. 0.3 is a robust
/// default across retrieval benchmarks.
pub const DEFAULT_ALPHA: f64 = 0.3;

/// Compute the expanded query for an existing tokenised corpus and a
/// raw query string. Returns the *full* token list to feed back into
/// BM25, original terms first (so they keep their weight via repetition
/// of the unique word) followed by the expansion terms.
pub fn expand_query(
    corpus: &bm25::Corpus,
    query_terms: &[String],
    top_docs: usize,
    top_terms: usize,
    alpha: f64,
) -> Vec<String> {
    if query_terms.is_empty() || corpus.docs.is_empty() {
        return query_terms.to_vec();
    }

    // Step 1+2: BM25 over the corpus, take the top `top_docs` doc indices.
    let mut scored: Vec<(usize, f64)> = (0..corpus.docs.len())
        .map(|i| (i, corpus.score(i, query_terms)))
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    scored.truncate(top_docs);
    let any_relevant = scored.iter().any(|(_, s)| *s > 0.0);
    if !any_relevant {
        return query_terms.to_vec();
    }

    // Step 3: build the relevance model — sum bm25(d, q) · tf(t, d) /
    // |d| across the relevance set.
    let total_score: f64 = scored.iter().map(|(_, s)| s.max(0.0)).sum();
    let total_score = total_score.max(1e-12);
    let mut weights: AHashMap<String, f64> = AHashMap::new();
    let query_set: ahash::AHashSet<&String> = query_terms.iter().collect();

    for (doc_ix, score) in &scored {
        if *score <= 0.0 {
            continue;
        }
        let doc = &corpus.docs[*doc_ix];
        if doc.is_empty() {
            continue;
        }
        let dl = doc.len() as f64;
        let pi = (*score) / total_score;
        let mut tf: AHashMap<&String, usize> = AHashMap::new();
        for w in doc {
            *tf.entry(w).or_insert(0) += 1;
        }
        for (w, count) in tf {
            // Skip terms that are already in the query — we'll add
            // those back at the end via the original-query interpolation.
            if query_set.contains(w) {
                continue;
            }
            let tf_norm = count as f64 / dl;
            *weights.entry(w.clone()).or_insert(0.0) += pi * tf_norm;
        }
    }

    // Step 4: keep top `top_terms` by weight.
    let mut ranked: Vec<(String, f64)> = weights.into_iter().collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    ranked.truncate(top_terms);

    // Step 5: build the expanded query. We approximate the
    // `(1−α)·original + α·expansion` mix by repeating original-query
    // terms `m` times and expansion terms `m_exp` times where the
    // ratio reflects α (BM25 is multiplicative in tf so this is the
    // natural discrete encoding without changing the BM25 implementation).
    let alpha = alpha.clamp(0.05, 0.95);
    // Choose multipliers that give a stable ratio close to α : (1−α).
    // 10× the original-query terms, then ⌈α · 10 / (1−α)⌉ × the expansion
    // — small integers so BM25 doesn't blow up its arithmetic.
    let q_mult = 10usize;
    let e_mult = ((alpha / (1.0 - alpha)) * q_mult as f64).round().max(1.0) as usize;

    let mut out: Vec<String> = Vec::with_capacity(query_terms.len() * q_mult + ranked.len() * e_mult);
    for _ in 0..q_mult {
        for t in query_terms {
            out.push(t.clone());
        }
    }
    for (term, _) in &ranked {
        for _ in 0..e_mult {
            out.push(term.clone());
        }
    }
    out
}

/// Convenience: tokenise a raw query then expand. Used by the ranking
/// pipeline when RM3 is enabled.
pub fn expand_raw(corpus: &bm25::Corpus, query: &str) -> Vec<String> {
    let qt = tokenize_words(query);
    expand_query(corpus, &qt, DEFAULT_TOP_DOCS, DEFAULT_TOP_TERMS, DEFAULT_ALPHA)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Vec<String> {
        s.split_whitespace().map(|w| w.to_string()).collect()
    }

    #[test]
    fn expansion_pulls_in_associated_terms() {
        // Doc 0 mentions "stripe payment intent"; doc 1 is unrelated.
        // Query is "stripe payment". RM3 should pull in "intent".
        let corpus = bm25::Corpus::build(vec![
            d("stripe payment intent webhook validation parse"),
            d("ui pixel shader render canvas opengl"),
            d("alpha beta gamma delta epsilon zeta"),
        ]);
        let query = d("stripe payment");
        let expanded = expand_query(&corpus, &query, 3, 5, 0.3);

        // Original terms should still be present.
        assert!(expanded.contains(&"stripe".to_string()));
        assert!(expanded.contains(&"payment".to_string()));
        // At least one of the doc-0-only words should make it in.
        let pulled_in = ["intent", "webhook", "validation", "parse"]
            .iter()
            .any(|w| expanded.contains(&w.to_string()));
        assert!(pulled_in, "expected RM3 to pull in associated term, got {expanded:?}");
    }

    #[test]
    fn expansion_with_no_matches_returns_original() {
        let corpus = bm25::Corpus::build(vec![d("alpha beta gamma"), d("delta epsilon zeta")]);
        let query = d("nothing matches here");
        let expanded = expand_query(&corpus, &query, 3, 5, 0.3);
        assert_eq!(expanded, query);
    }

    #[test]
    fn expansion_does_not_drop_original_terms() {
        let corpus = bm25::Corpus::build(vec![
            d("auth token validate session jwt"),
            d("auth token validate session jwt"),
        ]);
        let query = d("auth");
        let expanded = expand_query(&corpus, &query, 2, 3, 0.3);
        // "auth" must still appear, and at least one expansion term too.
        assert!(expanded.iter().any(|t| t == "auth"));
        assert!(expanded.len() > query.len());
    }
}
