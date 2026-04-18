//! ContextOS core engine — orchestrates the optimization pipeline.
//!
//! Pipeline, in order:
//!   1. **Dedup**       remove byte-identical / near-identical chunks.
//!   2. **Compress**    strip comments, debug logs, dead whitespace.
//!   3. **Rank**        score chunks against the user's query/intent.
//!   4. **Budget**      greedily include highest-scoring chunks until token
//!                      budget is exhausted.
//!
//! The entry point is [`Engine::optimize`]. It accepts a batch of
//! [`InputChunk`]s and returns an [`OptimizationResult`] with before/after
//! token counts so callers (CLI, extension, tests) can verify savings.

pub mod budget;
pub mod compress;
pub mod dedup;
pub mod ranking;
pub mod types;

pub use types::*;

use contextos_tokenizer::estimate_tokens;
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    pub max_tokens: usize,
    pub enable_dedup: bool,
    pub enable_compress: bool,
    pub enable_ranking: bool,
    pub enable_budget: bool,
    pub dedup_similarity: f32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_tokens: 8_000,
            enable_dedup: true,
            enable_compress: true,
            enable_ranking: true,
            enable_budget: true,
            dedup_similarity: 0.92,
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
        let started = Instant::now();
        let original_tokens: usize = input
            .chunks
            .iter()
            .map(|c| estimate_tokens(&c.content))
            .sum();

        let mut chunks = input.chunks;

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
            ranking::run(&mut chunks, input.query.as_deref());
        }

        let budget_stats = if self.config.enable_budget {
            budget::run(&mut chunks, self.config.max_tokens)
        } else {
            budget::Stats {
                kept: chunks.len(),
                dropped: 0,
                final_tokens: chunks.iter().map(|c| estimate_tokens(&c.content)).sum(),
            }
        };

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
                dedup: dedup_stats,
                compress: compress_stats,
                budget: budget_stats,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStats {
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
    fn budget_enforces_cap() {
        let engine = Engine::new(EngineConfig {
            max_tokens: 5,
            ..Default::default()
        });
        let chunks = (0..20)
            .map(|i| chunk(&format!("c{i}"), &format!("some code line number {i}")))
            .collect();
        let res = engine.optimize(OptimizationRequest { chunks, query: None });
        assert!(res.final_tokens <= 20, "must honor the budget within tolerance");
    }
}
