//! Okapi BM25 ranker.
//!
//! Replaces the naïve TF-IDF in `ranking::mod` when a query is present.
//! BM25 accounts for document length and term saturation, which matters a
//! lot for source code where some files are 50 lines and others 2000.
//!
//!     score(D, Q) = Σ IDF(q) · ((tf · (k1+1)) / (tf + k1·(1 - b + b·|D|/avgdl)))
//!
//! with `k1 = 1.5`, `b = 0.75` (standard defaults).

use ahash::AHashMap;

const K1: f64 = 1.5;
const B: f64 = 0.75;

pub struct Corpus {
    pub docs: Vec<Vec<String>>,
    pub doc_lens: Vec<usize>,
    pub avg_dl: f64,
    pub df: AHashMap<String, usize>,
}

impl Corpus {
    pub fn build(docs: Vec<Vec<String>>) -> Self {
        let doc_lens: Vec<usize> = docs.iter().map(|d| d.len()).collect();
        let total: usize = doc_lens.iter().sum();
        let avg_dl = if docs.is_empty() {
            0.0
        } else {
            total as f64 / docs.len() as f64
        };
        let mut df: AHashMap<String, usize> = AHashMap::new();
        for doc in &docs {
            let mut seen: ahash::AHashSet<&String> = ahash::AHashSet::new();
            for w in doc {
                if seen.insert(w) {
                    *df.entry(w.clone()).or_insert(0) += 1;
                }
            }
        }
        Self {
            docs,
            doc_lens,
            avg_dl,
            df,
        }
    }

    pub fn score(&self, doc_ix: usize, query: &[String]) -> f64 {
        if query.is_empty() || doc_ix >= self.docs.len() {
            return 0.0;
        }
        let doc = &self.docs[doc_ix];
        if doc.is_empty() {
            return 0.0;
        }
        let dl = self.doc_lens[doc_ix] as f64;
        let n = self.docs.len() as f64;

        // tf per term
        let mut tf: AHashMap<&String, usize> = AHashMap::new();
        for w in doc {
            *tf.entry(w).or_insert(0) += 1;
        }

        let mut score = 0.0;
        for term in query {
            let df_t = *self.df.get(term).unwrap_or(&0) as f64;
            if df_t == 0.0 {
                continue;
            }
            let idf = (((n - df_t + 0.5) / (df_t + 0.5)) + 1.0).ln();
            let tf_t = tf.get(term).copied().unwrap_or(0) as f64;
            if tf_t == 0.0 {
                continue;
            }
            let norm = 1.0 - B + B * (dl / self.avg_dl.max(1.0));
            let numerator = tf_t * (K1 + 1.0);
            let denominator = tf_t + K1 * norm;
            score += idf * (numerator / denominator);
        }
        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Vec<String> {
        s.split_whitespace().map(|w| w.to_string()).collect()
    }

    #[test]
    fn query_terms_boost_matching_doc() {
        let c = Corpus::build(vec![
            d("parse stripe webhook payment intent"),
            d("render ui pixel shader"),
            d("calculate total sum order"),
        ]);
        let q = d("stripe payment");
        let s0 = c.score(0, &q);
        let s1 = c.score(1, &q);
        assert!(s0 > s1);
    }

    #[test]
    fn empty_query_is_zero() {
        let c = Corpus::build(vec![d("anything goes here")]);
        assert_eq!(c.score(0, &[]), 0.0);
    }
}
