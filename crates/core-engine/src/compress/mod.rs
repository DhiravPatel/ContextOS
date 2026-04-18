//! Compression pass — in-place strip of comments, debug logs and whitespace.
//!
//! Delegates the language-aware heavy lifting to `contextos-parser`. This
//! module just orchestrates the call for each chunk and records stats.

use crate::types::{ChunkKind, InputChunk};
use contextos_parser::{strip, StripOptions};
use contextos_tokenizer::estimate_tokens;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub tokens_before: usize,
    pub tokens_after: usize,
    pub bytes_before: usize,
    pub bytes_after: usize,
    pub chunks_touched: usize,
}

pub fn run(chunks: &mut [InputChunk]) -> Stats {
    let opts = StripOptions::default();

    let (b_tokens, b_bytes): (usize, usize) = chunks
        .iter()
        .map(|c| (estimate_tokens(&c.content), c.content.len()))
        .fold((0, 0), |(t, b), (tt, bb)| (t + tt, b + bb));

    // Compression is embarrassingly parallel.
    chunks.par_iter_mut().for_each(|c| {
        if matches!(c.kind, ChunkKind::Code | ChunkKind::Selection) {
            let compressed = strip(&c.content, c.language, opts);
            if compressed.len() < c.content.len() {
                c.content = compressed;
            }
        }
    });

    let (a_tokens, a_bytes): (usize, usize) = chunks
        .iter()
        .map(|c| (estimate_tokens(&c.content), c.content.len()))
        .fold((0, 0), |(t, b), (tt, bb)| (t + tt, b + bb));

    Stats {
        tokens_before: b_tokens,
        tokens_after: a_tokens,
        bytes_before: b_bytes,
        bytes_after: a_bytes,
        chunks_touched: chunks.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChunkKind, InputChunk};
    use contextos_utils::Language;

    #[test]
    fn shrinks_commented_code() {
        let src = r#"
            // this is a long explanatory comment
            function add(a, b) {
                console.log('adding', a, b);
                return a + b;
            }
        "#;
        let mut v = vec![InputChunk {
            id: "a".into(),
            path: None,
            language: Language::JavaScript,
            content: src.into(),
            kind: ChunkKind::Code,
            priority: 0,
        }];
        let stats = run(&mut v);
        assert!(stats.bytes_after < stats.bytes_before);
        assert!(!v[0].content.contains("console.log"));
        assert!(v[0].content.contains("return a + b"));
    }
}
