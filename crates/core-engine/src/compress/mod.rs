//! Compression pass — in-place strip of comments, debug logs and whitespace.
//!
//! Delegates the language-aware heavy lifting to `contextos-parser`. This
//! module just orchestrates the call for each chunk and records stats.
//!
//! ## Boilerplate detection (Count-Min)
//!
//! On top of the AST strip we run an optional pass that uses a
//! [`CountMinSketch`] to identify *whole lines* that recur many times
//! across the input. License headers, copyright banners, repeated
//! `#region` markers, identical log-formatter lines — the classic
//! boilerplate that BM25 doesn't care about and the LLM doesn't need.
//! Once a line is confirmed boilerplate (≥ `BOILERPLATE_THRESHOLD`
//! occurrences in the corpus) we remove its *subsequent* occurrences,
//! leaving the first one in place so the LLM still sees it once.
//! Lossless: the byte-for-byte content of every line we *keep* is
//! original; we only delete repeats.

use crate::types::{ChunkKind, InputChunk};
use contextos_parser::{strip, StripOptions};
use contextos_tokenizer::estimate_tokens;
use contextos_utils::{normalize_whitespace, CountMinSketch};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// Lines repeating more than this across the corpus are flagged as
/// boilerplate (subject to a length floor — short lines like `}` or `;`
/// are skipped because they trivially recur in code). The Count-Min
/// estimate never under-counts, so this threshold gives us a one-sided
/// guarantee: if we strip a line, it really did appear ≥ `THRESHOLD`
/// times.
const BOILERPLATE_THRESHOLD: u32 = 4;
const BOILERPLATE_MIN_LEN: usize = 24;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub tokens_before: usize,
    pub tokens_after: usize,
    pub bytes_before: usize,
    pub bytes_after: usize,
    pub chunks_touched: usize,
    /// Number of repeated boilerplate lines collapsed (first occurrence
    /// preserved per line text).
    pub boilerplate_collapsed: usize,
}

pub fn run(chunks: &mut [InputChunk]) -> Stats {
    let opts = StripOptions::default();

    let (b_tokens, b_bytes): (usize, usize) = chunks
        .iter()
        .map(|c| (estimate_tokens(&c.content), c.content.len()))
        .fold((0, 0), |(t, b), (tt, bb)| (t + tt, b + bb));

    // Pass 1: language-aware strip is embarrassingly parallel.
    chunks.par_iter_mut().for_each(|c| {
        if matches!(c.kind, ChunkKind::Code | ChunkKind::Selection) {
            let compressed = strip(&c.content, c.language, opts);
            if compressed.len() < c.content.len() {
                c.content = compressed;
            }
        }
    });

    // Pass 2: cross-chunk boilerplate collapse via Count-Min Sketch.
    let boilerplate_collapsed = collapse_boilerplate(chunks);

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
        boilerplate_collapsed,
    }
}

/// Two-pass cross-chunk dedup of repeated lines.
///
/// First pass populates a Count-Min sketch with the normalised line
/// fingerprint of every line. Second pass walks each chunk and drops
/// lines whose count meets the boilerplate threshold *after* their
/// first appearance in the corpus (using a small exact set so we keep
/// one instance — the LLM still sees a sample of the boilerplate).
fn collapse_boilerplate(chunks: &mut [InputChunk]) -> usize {
    if chunks.len() < 2 {
        return 0;
    }
    let mut sketch = CountMinSketch::default();
    for c in chunks.iter() {
        for line in c.content.lines() {
            let norm = normalize_whitespace(line);
            if norm.len() < BOILERPLATE_MIN_LEN {
                continue;
            }
            sketch.add(&norm);
        }
    }

    // Track which boilerplate lines we've already kept once.
    let mut kept_once: ahash::AHashSet<String> = ahash::AHashSet::new();
    let mut removed_total = 0usize;

    for c in chunks.iter_mut() {
        if !matches!(c.kind, ChunkKind::Code | ChunkKind::Selection | ChunkKind::Doc) {
            continue;
        }
        let mut new_lines: Vec<&str> = Vec::with_capacity(c.content.lines().count());
        let mut removed_local = 0usize;
        for line in c.content.lines() {
            let norm = normalize_whitespace(line);
            let is_short = norm.len() < BOILERPLATE_MIN_LEN;
            let count = if is_short { 0 } else { sketch.count(&norm) };
            if count >= BOILERPLATE_THRESHOLD {
                // First time we encounter this exact line: keep it.
                if kept_once.insert(norm) {
                    new_lines.push(line);
                } else {
                    removed_local += 1;
                }
            } else {
                new_lines.push(line);
            }
        }
        if removed_local > 0 {
            // Preserve trailing newline behaviour roughly — join with `\n`.
            c.content = new_lines.join("\n");
            removed_total += removed_local;
        }
    }
    removed_total
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
            skeleton_hint: false,
            community: None,
        }];
        let stats = run(&mut v);
        assert!(stats.bytes_after < stats.bytes_before);
        assert!(!v[0].content.contains("console.log"));
        assert!(v[0].content.contains("return a + b"));
    }
}
