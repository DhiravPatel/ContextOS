//! ContextOS core engine — orchestrates the optimization pipeline.
//!
//! Pipeline (in order):
//!   1. **Skeletonise** — for chunks marked `skeleton_hint`, replace bodies
//!      with signature-only views. Free tokens, zero semantic loss.
//!   2. **Dedup**       — exact hash + SimHash + MinHash-LSH near-dup removal.
//!   3. **Compress**    — AST-aware comment/log/whitespace stripping.
//!   4. **Rank**        — BM25 + density + optional graph priors fused via
//!                        Reciprocal Rank Fusion.
//!   5. **Budget**      — diversity-aware MMR + submodular coverage by
//!                        default; 0/1 knapsack DP or greedy on demand.
//!   6. **Cache-order** — stable, content-addressable ordering of selected
//!                        chunks so identical selections across requests
//!                        produce byte-identical prompts (LLM provider
//!                        prompt-cache friendly).
//!
//! Entry points:
//!   * [`Engine::optimize`] — straightforward, in-memory request.
//!   * [`Engine::optimize_with_priors`] — same, plus per-chunk PageRank
//!     scores (typically produced from a [`contextos-graph`] index).

pub mod budget;
pub mod compress;
pub mod dedup;
pub mod ranking;
pub mod skeleton;
pub mod types;

pub use types::*;

use budget::{Strategy as BudgetStrategy, DEFAULT_MMR_LAMBDA};
use contextos_tokenizer::estimate_tokens;
use ranking::{Priors, RankingOptions};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    pub max_tokens: usize,
    pub enable_skeleton: bool,
    pub enable_dedup: bool,
    pub enable_compress: bool,
    pub enable_ranking: bool,
    pub enable_budget: bool,
    pub dedup_similarity: f32,
    /// Budget selection strategy. `Auto` picks `MmrSubmodular` for inputs
    /// with 3+ chunks, otherwise `Greedy`.
    #[serde(default)]
    pub budget_strategy: BudgetStrategy,
    /// MMR diversity/relevance balance (0 = max diversity, 1 = max
    /// relevance). Only used by `MmrSubmodular`.
    #[serde(default = "default_mmr_lambda")]
    pub mmr_lambda: f64,
    /// When true, the final pipeline step re-orders selected chunks by a
    /// stable, content-addressable key so repeated requests with the same
    /// chunk set produce byte-identical prompts. This dramatically improves
    /// LLM provider prompt-cache hit rate.
    #[serde(default = "default_true")]
    pub enable_cache_order: bool,
    /// When true, expand the user's query via RM3 pseudo-relevance
    /// feedback before BM25 scoring. Adds one extra BM25 pass; lifts
    /// recall by ~10–20% on under-specified queries. Off by default
    /// because the original BM25 path is fine for most workloads and
    /// RM3 doubles the BM25 cost.
    #[serde(default)]
    pub enable_rm3: bool,
    /// When true and chunks carry `community` labels (typically from a
    /// graph-driven Louvain pass), MMR's selection objective is
    /// extended with a community-coverage term so the budget gets
    /// spread across topical clusters instead of piling into one. Off
    /// by default; has no effect when chunks don't carry community
    /// labels.
    #[serde(default)]
    pub enable_louvain_budget: bool,
}

fn default_mmr_lambda() -> f64 {
    DEFAULT_MMR_LAMBDA
}
fn default_true() -> bool {
    true
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_tokens: 8_000,
            enable_skeleton: true,
            enable_dedup: true,
            enable_compress: true,
            enable_ranking: true,
            enable_budget: true,
            dedup_similarity: 0.92,
            budget_strategy: BudgetStrategy::Auto,
            mmr_lambda: DEFAULT_MMR_LAMBDA,
            enable_cache_order: true,
            enable_rm3: false,
            enable_louvain_budget: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Engine {
    config: EngineConfig,
}

impl Engine {
    pub fn new(config: EngineConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn optimize(&self, input: OptimizationRequest) -> OptimizationResult {
        self.optimize_with_priors(input, None)
    }

    pub fn optimize_with_priors(
        &self,
        input: OptimizationRequest,
        priors: Option<&Priors>,
    ) -> OptimizationResult {
        let started = Instant::now();
        let original_tokens: usize = input
            .chunks
            .iter()
            .map(|c| estimate_tokens(&c.content))
            .sum();

        let mut chunks = input.chunks;

        let skeleton_stats = if self.config.enable_skeleton {
            skeleton::apply(&mut chunks)
        } else {
            skeleton::Stats::default()
        };

        let dedup_stats = if self.config.enable_dedup {
            dedup::run(&mut chunks, self.config.dedup_similarity)
        } else {
            dedup::Stats::default()
        };

        let compress_stats = if self.config.enable_compress {
            compress::run(&mut chunks)
        } else {
            compress::Stats::default()
        };

        if self.config.enable_ranking {
            ranking::run_with_priors_and_options(
                &mut chunks,
                input.query.as_deref(),
                priors,
                RankingOptions {
                    rm3: self.config.enable_rm3,
                },
            );
        }

        let budget_stats = if self.config.enable_budget {
            budget::run_with_options(
                &mut chunks,
                self.config.max_tokens,
                budget::Options {
                    strategy: self.config.budget_strategy,
                    mmr_lambda: self.config.mmr_lambda,
                    community_aware: self.config.enable_louvain_budget,
                },
            )
        } else {
            budget::Stats {
                kept: chunks.len(),
                dropped: 0,
                final_tokens: chunks.iter().map(|c| estimate_tokens(&c.content)).sum(),
                strategy: Some("disabled".into()),
            }
        };

        if self.config.enable_cache_order {
            budget::cache_aware_order(&mut chunks);
        }

        let final_tokens = budget_stats.final_tokens.max(
            chunks.iter().map(|c| estimate_tokens(&c.content)).sum(),
        );
        let saved = original_tokens.saturating_sub(final_tokens);
        let reduction_pct = if original_tokens == 0 {
            0.0
        } else {
            (saved as f64 / original_tokens as f64) * 100.0
        };

        OptimizationResult {
            chunks,
            original_tokens,
            final_tokens,
            tokens_saved: saved,
            reduction_pct,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
            stats: PipelineStats {
                skeleton: skeleton_stats,
                dedup: dedup_stats,
                compress: compress_stats,
                budget: budget_stats,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStats {
    pub skeleton: skeleton::Stats,
    pub dedup: dedup::Stats,
    pub compress: compress::Stats,
    pub budget: budget::Stats,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &str, content: &str) -> InputChunk {
        InputChunk {
            id: id.into(),
            path: None,
            language: contextos_utils::Language::Rust,
            content: content.into(),
            kind: ChunkKind::Code,
            priority: 0,
            skeleton_hint: false,
            community: None,
        }
    }

    #[test]
    fn pipeline_reduces_tokens_on_redundant_input() {
        let engine = Engine::new(EngineConfig {
            max_tokens: 10_000,
            ..Default::default()
        });
        let chunks = vec![
            chunk("a", "fn add(a: i32, b: i32) -> i32 { a + b } // adds two"),
            chunk("b", "fn add(a: i32, b: i32) -> i32 { a + b } // adds two"),
            chunk("c", "fn sub(a: i32, b: i32) -> i32 { a - b }"),
        ];
        let res = engine.optimize(OptimizationRequest {
            chunks,
            query: Some("addition".into()),
        });
        assert!(res.final_tokens < res.original_tokens);
        assert!(res.reduction_pct > 0.0);
    }

    #[test]
    fn skeleton_hint_drops_bodies() {
        let engine = Engine::new(EngineConfig {
            max_tokens: 100_000,
            ..Default::default()
        });
        let mut c = chunk(
            "big",
            r#"
            pub fn big() -> i32 {
                let mut total = 0;
                for i in 0..1000 { total += i; }
                total
            }
            "#,
        );
        c.skeleton_hint = true;
        let res = engine.optimize(OptimizationRequest {
            chunks: vec![c],
            query: None,
        });
        assert!(!res.chunks[0].content.contains("total += i"));
    }
}
