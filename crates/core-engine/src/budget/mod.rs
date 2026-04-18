//! Token budget enforcement.
//!
//! Walks chunks in rank order, keeping each until the running total exceeds
//! `max_tokens`. The last partially-fitting chunk is either kept whole (if
//! it's within a small slack ratio) or dropped, so the hard cap is nearly
//! always respected within 5%.
//!
//! Assumption: [`crate::ranking`] already ordered chunks so that dropping
//! from the tail costs the least relevance.

use crate::types::InputChunk;
use contextos_tokenizer::estimate_tokens;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub kept: usize,
    pub dropped: usize,
    pub final_tokens: usize,
}

pub fn run(chunks: &mut Vec<InputChunk>, max_tokens: usize) -> Stats {
    if max_tokens == 0 {
        let dropped = chunks.len();
        chunks.clear();
        return Stats {
            kept: 0,
            dropped,
            final_tokens: 0,
        };
    }

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
            // One final overshoot allowed; caller knows via stats.
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
    }
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
    fn keeps_highest_until_budget() {
        let mut v = vec![
            c("a", &"a".repeat(400)),
            c("b", &"b".repeat(400)),
            c("c", &"c".repeat(400)),
        ];
        let stats = run(&mut v, 150);
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
}
