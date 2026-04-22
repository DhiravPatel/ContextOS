//! Deduplication pass.
//!
//! Three levels, cascading:
//!   1. **Exact chunk dedup** — drop chunks whose whitespace-normalized
//!      content hash has already been seen. O(n).
//!   2. **MinHash + LSH** (for n ≥ `LSH_THRESHOLD`) — bucket by banded
//!      MinHash signatures and compare only within-bucket pairs. Turns a
//!      pathological O(n²) into O(n) on repo-scale inputs.
//!   3. **Line-set Jaccard** (small n) — direct Jaccard over line
//!      fingerprints. Kept for tiny inputs where LSH has startup overhead.

pub mod minhash;

use crate::types::InputChunk;
use ahash::AHashMap;
use contextos_utils::{fast_hash, line_fingerprint, normalize_whitespace};
use minhash::{line_shingles, signature_of, LshIndex, Signature};
use serde::{Deserialize, Serialize};

/// Above this chunk count we switch from O(n²) Jaccard to MinHash-LSH.
const LSH_THRESHOLD: usize = 64;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub exact_removed: usize,
    pub near_removed: usize,
    pub kept: usize,
    pub used_lsh: bool,
}

pub fn run(chunks: &mut Vec<InputChunk>, similarity_threshold: f32) -> Stats {
    let before = chunks.len();
    let mut seen_exact: AHashMap<u64, ()> = AHashMap::new();
    chunks.retain(|c| {
        let key = fast_hash(&normalize_whitespace(&c.content));
        seen_exact.insert(key, ()).is_none()
    });
    let after_exact = chunks.len();

    let (near_removed, used_lsh) = if chunks.len() >= LSH_THRESHOLD {
        let removed = dedup_with_lsh(chunks, similarity_threshold as f64);
        (removed, true)
    } else {
        let removed = dedup_pairwise(chunks, similarity_threshold as f64);
        (removed, false)
    };

    Stats {
        exact_removed: before - after_exact,
        near_removed,
        kept: chunks.len(),
        used_lsh,
    }
}

fn dedup_pairwise(chunks: &mut Vec<InputChunk>, threshold: f64) -> usize {
    let mut line_sets: Vec<Vec<u64>> = Vec::with_capacity(chunks.len());
    let mut keep: Vec<bool> = Vec::with_capacity(chunks.len());

    for c in chunks.iter() {
        let set: Vec<u64> = c
            .content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(line_fingerprint)
            .collect();

        let mut drop = false;
        for (i, prev) in line_sets.iter().enumerate() {
            if !keep[i] {
                continue;
            }
            if jaccard_sorted(&set, prev) >= threshold {
                drop = true;
                break;
            }
        }
        line_sets.push(set);
        keep.push(!drop);
    }

    let before = chunks.len();
    let mut iter = keep.iter();
    chunks.retain(|_| *iter.next().unwrap());
    before - chunks.len()
}

fn dedup_with_lsh(chunks: &mut Vec<InputChunk>, threshold: f64) -> usize {
    // Build signatures up front (parallel-safe; could rayon-ise later).
    let signatures: Vec<Signature> = chunks
        .iter()
        .map(|c| signature_of(&line_shingles(&c.content, 3)))
        .collect();

    let mut index = LshIndex::new();
    let mut keep = vec![true; chunks.len()];

    for i in 0..chunks.len() {
        let cands = index.candidates(&signatures[i]);
        let mut drop = false;
        for j in cands {
            if !keep[j] {
                continue;
            }
            if minhash::jaccard(&signatures[i], &signatures[j]) >= threshold {
                drop = true;
                break;
            }
        }
        if !drop {
            index.insert(i, &signatures[i]);
        } else {
            keep[i] = false;
        }
    }

    let before = chunks.len();
    let mut iter = keep.iter();
    chunks.retain(|_| *iter.next().unwrap());
    before - chunks.len()
}

fn jaccard_sorted(a: &[u64], b: &[u64]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut sa: Vec<u64> = a.to_vec();
    let mut sb: Vec<u64> = b.to_vec();
    sa.sort_unstable();
    sb.sort_unstable();
    sa.dedup();
    sb.dedup();

    let (mut i, mut j, mut inter, mut union) = (0, 0, 0, 0);
    while i < sa.len() && j < sb.len() {
        union += 1;
        match sa[i].cmp(&sb[j]) {
            std::cmp::Ordering::Equal => {
                inter += 1;
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    union += (sa.len() - i) + (sb.len() - j);
    if union == 0 {
        0.0
    } else {
        inter as f64 / union as f64
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
            skeleton_hint: false,
        }
    }

    #[test]
    fn removes_exact_duplicates() {
        let mut v = vec![
            c("a", "fn x() {}"),
            c("b", "fn x() {}"),
            c("c", "fn y() {}"),
        ];
        let stats = run(&mut v, 0.9);
        assert_eq!(v.len(), 2);
        assert_eq!(stats.exact_removed, 1);
    }

    #[test]
    fn whitespace_differences_are_dups() {
        let mut v = vec![c("a", "fn x() {}"), c("b", "fn   x()   {}")];
        run(&mut v, 0.9);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn near_dup_small_input_uses_pairwise() {
        let big_a = (0..30)
            .map(|i| format!("let x{i} = {i};"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut big_b = big_a.clone();
        big_b.push_str("\nlet extra = 1;");
        let mut v = vec![c("a", &big_a), c("b", &big_b)];
        let stats = run(&mut v, 0.9);
        assert_eq!(v.len(), 1);
        assert!(!stats.used_lsh);
    }

    #[test]
    fn lsh_handles_large_near_dup_batches() {
        // 100 near-copies of the same function. Each carries a unique marker
        // line so exact-hash dedup doesn't collapse them (which would skip
        // the LSH path entirely). Jaccard between any pair is ~0.92 — well
        // above the 0.85 threshold — so LSH should collapse all 100 down to
        // a small handful.
        let base_lines: Vec<String> = (0..25).map(|i| format!("let x{i} = {i};")).collect();
        let mut v: Vec<InputChunk> = (0..100)
            .map(|i| {
                let mut lines = base_lines.clone();
                lines.push(format!("// unique marker {i}"));
                c(&format!("n{i}"), &lines.join("\n"))
            })
            .collect();
        let stats = run(&mut v, 0.85);
        assert!(stats.used_lsh, "should switch to LSH path with > 64 chunks");
        assert!(v.len() < 20, "expected aggressive dedup, got {}", v.len());
    }
}
