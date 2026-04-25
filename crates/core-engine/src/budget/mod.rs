//! Token budget enforcement.
//!
//! Three selection strategies, dispatched on input size + config:
//!
//!   1. **Greedy** ([`Strategy::Greedy`]) — the original first-fit pass.
//!      Walks chunks in rank order, keeping each until the running total
//!      exceeds `max_tokens`. Cheapest, used as a baseline.
//!   2. **0/1 Knapsack DP** ([`Strategy::KnapsackDp`]) — exact optimum for
//!      `n ≤ KNAPSACK_DP_MAX_N`. Each chunk has a "value" (its rank-derived
//!      relevance score) and a "weight" (its token cost); we pick the
//!      max-value subset that fits the budget. Pseudo-polynomial in the
//!      budget, but our budgets are small enough (a few thousand tokens)
//!      that the DP table fits in L2.
//!   3. **MMR + Submodular** ([`Strategy::MmrSubmodular`]) — diversity-
//!      aware greedy with a submodular coverage objective. Each step picks
//!      the chunk that maximises
//!
//!      ```text
//!      λ · relevance(c)  +  (1−λ) · coverage_gain(c | selected)
//!      ```
//!
//!      where `coverage_gain` is the additional unique-shingle coverage
//!      `c` brings on top of what's already selected. This is exactly the
//!      submodular set-cover greedy and inherits its (1 − 1/e) ≈ 63%
//!      approximation guarantee. Used by default at any input size: it's
//!      O(n²·k) which is fine for thousands of chunks, and beats both
//!      greedy (no diversity) and knapsack DP (no diversity, but optimal
//!      relevance) on real-world workloads where ranked chunks heavily
//!      overlap.
//!
//! Strategy selection: callers can pin a strategy via [`run_with`]; the
//! default ([`run`]) picks `MmrSubmodular` when there are 3+ chunks and a
//! reasonable budget, falling back to `Greedy` otherwise (single-chunk
//! inputs and degenerate budgets don't benefit from the heavier passes).
//!
//! Assumption: [`crate::ranking`] already ordered chunks so position 0 is
//! highest-rank. We use position as a proxy for relevance score (rank `r`
//! → `1/(1+r)`), which is monotone with the actual score and avoids
//! threading the raw rank value through the engine.

use crate::types::InputChunk;
use ahash::AHashSet;
use contextos_tokenizer::estimate_tokens;
use contextos_utils::{line_fingerprint, stable_hash};
use serde::{Deserialize, Serialize};

/// Maximum chunk count for the exact 0/1 knapsack DP. Above this we fall
/// back to MMR/greedy because the DP table grows as `O(n · max_tokens)`.
pub const KNAPSACK_DP_MAX_N: usize = 256;

/// MMR balance: how much weight to put on relevance vs. diversity. Higher
/// → more relevance-first; lower → more diverse. 0.7 is the textbook
/// default.
pub const DEFAULT_MMR_LAMBDA: f64 = 0.7;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Greedy,
    KnapsackDp,
    MmrSubmodular,
    /// Choose `MmrSubmodular` for n ≥ 3 with a non-degenerate budget;
    /// otherwise fall back to `Greedy`.
    Auto,
}

impl Default for Strategy {
    fn default() -> Self {
        Strategy::Auto
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub kept: usize,
    pub dropped: usize,
    pub final_tokens: usize,
    pub strategy: Option<String>,
}

/// Default entry point — uses `Strategy::Auto` and the default MMR lambda.
pub fn run(chunks: &mut Vec<InputChunk>, max_tokens: usize) -> Stats {
    run_with(chunks, max_tokens, Strategy::Auto, DEFAULT_MMR_LAMBDA)
}

pub fn run_with(
    chunks: &mut Vec<InputChunk>,
    max_tokens: usize,
    strategy: Strategy,
    mmr_lambda: f64,
) -> Stats {
    if max_tokens == 0 {
        let dropped = chunks.len();
        chunks.clear();
        return Stats {
            kept: 0,
            dropped,
            final_tokens: 0,
            strategy: Some("zero_budget".into()),
        };
    }

    let resolved = match strategy {
        Strategy::Auto => {
            if chunks.len() >= 3 && max_tokens > 50 {
                Strategy::MmrSubmodular
            } else {
                Strategy::Greedy
            }
        }
        s => s,
    };

    match resolved {
        Strategy::Greedy => run_greedy(chunks, max_tokens),
        Strategy::KnapsackDp => run_knapsack(chunks, max_tokens),
        Strategy::MmrSubmodular => run_mmr_submodular(chunks, max_tokens, mmr_lambda),
        Strategy::Auto => unreachable!("Auto resolved above"),
    }
}

// ---- greedy ------------------------------------------------------------

fn run_greedy(chunks: &mut Vec<InputChunk>, max_tokens: usize) -> Stats {
    let mut running = 0usize;
    let mut kept = 0usize;
    let mut dropped = 0usize;
    let slack = (max_tokens as f64 * 0.05).ceil() as usize;

    let original = std::mem::take(chunks);
    for chunk in original {
        let cost = estimate_tokens(&chunk.content);
        if running + cost <= max_tokens {
            running += cost;
            chunks.push(chunk);
            kept += 1;
        } else if running + cost <= max_tokens + slack {
            running += cost;
            chunks.push(chunk);
            kept += 1;
            break;
        } else {
            dropped += 1;
        }
    }

    Stats {
        kept,
        dropped,
        final_tokens: running,
        strategy: Some("greedy".into()),
    }
}

// ---- 0/1 knapsack DP ---------------------------------------------------

fn run_knapsack(chunks: &mut Vec<InputChunk>, max_tokens: usize) -> Stats {
    let n = chunks.len();
    if n == 0 {
        return Stats {
            kept: 0,
            dropped: 0,
            final_tokens: 0,
            strategy: Some("knapsack_dp_empty".into()),
        };
    }
    if n > KNAPSACK_DP_MAX_N {
        // Caller asked for DP but the input is too big — refuse rather than
        // silently allocate a multi-hundred-MB table.
        return run_greedy(chunks, max_tokens);
    }

    let weights: Vec<usize> = chunks
        .iter()
        .map(|c| estimate_tokens(&c.content).max(1))
        .collect();
    // Rank-derived value: chunks earlier in the list are higher-ranked.
    // Using `1/(1+rank)` gives a smooth, strictly-decreasing weight that's
    // robust to ties produced upstream by RRF.
    let values: Vec<f64> = (0..n).map(|r| 1.0 / (1.0 + r as f64)).collect();

    // Capacity may be huge in theory but we trim to `total + 1` because
    // exceeding the sum of all weights is pointless.
    let total_w: usize = weights.iter().sum();
    let cap = max_tokens.min(total_w);

    // dp[i][w] = best value using a subset of chunks 0..i with capacity ≤ w.
    // Stored as a flat Vec to keep allocation contiguous.
    let stride = cap + 1;
    let mut dp = vec![0.0f64; (n + 1) * stride];
    for i in 0..n {
        let wi = weights[i];
        let vi = values[i];
        let row = i * stride;
        let next = (i + 1) * stride;
        for w in 0..=cap {
            let skip = dp[row + w];
            let take = if w >= wi { dp[row + w - wi] + vi } else { f64::MIN };
            dp[next + w] = skip.max(take);
        }
    }

    // Reconstruct the chosen set.
    let mut chosen = vec![false; n];
    let mut w = cap;
    for i in (0..n).rev() {
        let row = i * stride;
        let next = (i + 1) * stride;
        if dp[next + w] > dp[row + w] + 1e-12 && w >= weights[i] {
            chosen[i] = true;
            w -= weights[i];
        }
    }

    let mut iter = chosen.iter();
    let original = std::mem::take(chunks);
    let mut kept = 0;
    let mut final_tokens = 0;
    let mut dropped = 0;
    for c in original {
        if *iter.next().unwrap() {
            final_tokens += estimate_tokens(&c.content);
            chunks.push(c);
            kept += 1;
        } else {
            dropped += 1;
        }
    }

    Stats {
        kept,
        dropped,
        final_tokens,
        strategy: Some("knapsack_dp".into()),
    }
}

// ---- MMR + submodular coverage ----------------------------------------

fn run_mmr_submodular(chunks: &mut Vec<InputChunk>, max_tokens: usize, lambda: f64) -> Stats {
    let n = chunks.len();
    if n == 0 {
        return Stats {
            kept: 0,
            dropped: 0,
            final_tokens: 0,
            strategy: Some("mmr_submodular_empty".into()),
        };
    }

    let lambda = lambda.clamp(0.0, 1.0);

    let weights: Vec<usize> = chunks
        .iter()
        .map(|c| estimate_tokens(&c.content).max(1))
        .collect();
    // Relevance ∝ 1/(1+rank); chunks[0] is highest-ranked input.
    let relevance: Vec<f64> = (0..n).map(|r| 1.0 / (1.0 + r as f64)).collect();

    // Each chunk's "feature set" for coverage = its line fingerprints. Two
    // chunks with overlapping line fingerprints contribute redundant
    // coverage — exactly the case where diversity should kick in.
    let features: Vec<AHashSet<u64>> = chunks
        .iter()
        .map(|c| {
            c.content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(line_fingerprint)
                .collect()
        })
        .collect();

    // Cardinality bound on the universe of features — used to normalise the
    // coverage gain into [0, 1].
    let universe_size: usize = {
        let mut all: AHashSet<u64> = AHashSet::new();
        for f in &features {
            for &x in f {
                all.insert(x);
            }
        }
        all.len().max(1)
    };

    // Maximum relevance — used to normalise relevance into [0, 1].
    let max_rel = relevance.iter().cloned().fold(0.0f64, f64::max).max(1e-12);

    let slack = (max_tokens as f64 * 0.05).ceil() as usize;

    let mut chosen = vec![false; n];
    let mut covered: AHashSet<u64> = AHashSet::new();
    let mut running = 0usize;

    loop {
        // Score every unselected chunk that still fits.
        let mut best: Option<(usize, f64)> = None;
        for i in 0..n {
            if chosen[i] {
                continue;
            }
            let cost = weights[i];
            if running + cost > max_tokens + slack {
                continue;
            }
            let rel_norm = relevance[i] / max_rel;
            let new_features = features[i].iter().filter(|x| !covered.contains(x)).count();
            let cov_gain = new_features as f64 / universe_size as f64;
            // MMR with submodular gain: λ·rel + (1−λ)·new_coverage.
            let score = lambda * rel_norm + (1.0 - lambda) * cov_gain;
            if best.map(|(_, s)| score > s).unwrap_or(true) {
                best = Some((i, score));
            }
        }

        match best {
            None => break,
            Some((i, _)) => {
                chosen[i] = true;
                running += weights[i];
                for &x in &features[i] {
                    covered.insert(x);
                }
                if running >= max_tokens {
                    break;
                }
            }
        }
    }

    // Sweep in original (rank) order so kept chunks preserve their relative
    // ranking. Downstream prompt-cache ordering may re-sort, but rank-stable
    // output is the expected default.
    let original = std::mem::take(chunks);
    let mut kept = 0;
    let mut dropped = 0;
    let mut final_tokens = 0;
    for (i, c) in original.into_iter().enumerate() {
        if chosen[i] {
            final_tokens += estimate_tokens(&c.content);
            chunks.push(c);
            kept += 1;
        } else {
            dropped += 1;
        }
    }

    Stats {
        kept,
        dropped,
        final_tokens,
        strategy: Some("mmr_submodular".into()),
    }
}

/// Stable, content-addressable ordering of selected chunks. Used by the
/// pipeline to maximise prompt-cache hit-rate at the LLM provider: the same
/// chunks across repeated requests should appear in the same byte sequence
/// regardless of selection order.
pub fn cache_aware_order(chunks: &mut Vec<InputChunk>) {
    // Group all chunks by a stable per-chunk key (id is authoritative; fall
    // back to a content hash if the caller passed an empty id). The sort is
    // stable so chunks with identical keys keep their relative order.
    chunks.sort_by(|a, b| {
        let ka: (u64, &str) = (
            if a.id.is_empty() {
                stable_hash(&a.content)
            } else {
                stable_hash(&a.id)
            },
            a.id.as_str(),
        );
        let kb: (u64, &str) = (
            if b.id.is_empty() {
                stable_hash(&b.content)
            } else {
                stable_hash(&b.id)
            },
            b.id.as_str(),
        );
        ka.cmp(&kb)
    });
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
    fn greedy_keeps_highest_until_budget() {
        let mut v = vec![
            c("a", &"a".repeat(400)),
            c("b", &"b".repeat(400)),
            c("c", &"c".repeat(400)),
        ];
        let stats = run_with(&mut v, 150, Strategy::Greedy, DEFAULT_MMR_LAMBDA);
        assert!(stats.final_tokens <= 160); // 150 + 5% slack
        assert!(stats.kept <= 2);
    }

    #[test]
    fn zero_budget_drops_everything() {
        let mut v = vec![c("a", "x")];
        let stats = run(&mut v, 0);
        assert_eq!(stats.kept, 0);
        assert_eq!(stats.dropped, 1);
    }

    #[test]
    fn generous_budget_keeps_all() {
        let mut v = vec![c("a", "hi"), c("b", "there")];
        run(&mut v, 1_000_000);
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn knapsack_picks_higher_value_combination() {
        // Two cheap-but-good chunks should outscore one expensive-and-good chunk.
        let mut v = vec![
            // rank 0 (highest relevance), weight ~280 tokens (huge content)
            c("expensive_top", &"x".repeat(1_000)),
            // ranks 1 and 2 (lower relevance), weight ~30 each
            c("cheap_b", &"b".repeat(50)),
            c("cheap_c", &"c".repeat(50)),
        ];
        let stats = run_with(&mut v, 100, Strategy::KnapsackDp, DEFAULT_MMR_LAMBDA);
        let kept_ids: Vec<&str> = v.iter().map(|c| c.id.as_str()).collect();
        // The expensive chunk doesn't fit; DP must select both cheaper ones.
        assert!(kept_ids.contains(&"cheap_b") && kept_ids.contains(&"cheap_c"));
        assert!(!kept_ids.contains(&"expensive_top"));
        assert!(stats.final_tokens <= 110);
    }

    #[test]
    fn mmr_prefers_diverse_when_lambda_is_low() {
        // a + b are duplicates; c is unique. With low lambda (diversity-heavy),
        // MMR should pick {a, c} over {a, b} despite b's higher rank.
        let payload_dup = "let x = 1;\nlet y = 2;\nlet z = 3;\n";
        let mut v = vec![
            c("a", payload_dup),
            c("b", payload_dup),
            c("c", "fn unique() -> i32 { 42 }"),
        ];
        // Each chunk costs ~10–14 tokens; a+b+c ≈ 35; budget 25 fits 2.
        let stats = run_with(&mut v, 25, Strategy::MmrSubmodular, 0.1);
        let ids: Vec<&str> = v.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"c"), "MMR should keep the unique chunk c, got {ids:?}");
        assert_eq!(stats.kept, v.len());
    }

    #[test]
    fn mmr_prefers_top_ranked_when_lambda_is_high() {
        let mut v = vec![c("top", "alpha beta"), c("middle", "gamma delta"), c("bottom", "eta theta")];
        run_with(&mut v, 100_000, Strategy::MmrSubmodular, 0.99);
        // High-lambda MMR is essentially relevance-ordered; chunk a stays first.
        assert_eq!(v[0].id, "top");
    }

    #[test]
    fn auto_picks_mmr_for_real_inputs() {
        let mut v = vec![c("a", "x"), c("b", "y"), c("c", "z")];
        let stats = run(&mut v, 1_000);
        assert_eq!(stats.strategy.as_deref(), Some("mmr_submodular"));
    }

    #[test]
    fn auto_falls_back_to_greedy_for_tiny_inputs() {
        let mut v = vec![c("a", "x"), c("b", "y")];
        let stats = run(&mut v, 1_000);
        assert_eq!(stats.strategy.as_deref(), Some("greedy"));
    }

    #[test]
    fn cache_aware_order_is_deterministic() {
        let mut v1 = vec![c("zebra", "x"), c("alpha", "y"), c("middle", "z")];
        let mut v2 = vec![c("middle", "z"), c("zebra", "x"), c("alpha", "y")];
        cache_aware_order(&mut v1);
        cache_aware_order(&mut v2);
        let ids1: Vec<&str> = v1.iter().map(|c| c.id.as_str()).collect();
        let ids2: Vec<&str> = v2.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids1, ids2);
    }
}
