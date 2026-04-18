//! Deduplication pass.
//!
//! Two levels:
//! 1. **Exact chunk dedup** — drop any chunk whose whitespace-normalized
//!    content hash has already been seen. O(n).
//! 2. **Near-duplicate dedup** — for chunks that survive (1), compare
//!    line-multiset similarity (Jaccard over line fingerprints) to earlier
//!    chunks. If similarity ≥ threshold, drop. O(n·avg_lines).
//!
//! Both are in-place mutations on the chunk vector.

use crate::types::InputChunk;
use ahash::AHashMap;
use contextos_utils::{fast_hash, line_fingerprint, normalize_whitespace};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub exact_removed: usize,
    pub near_removed: usize,
    pub kept: usize,
}

pub fn run(chunks: &mut Vec<InputChunk>, similarity_threshold: f32) -> Stats {
    let before = chunks.len();
    let mut seen_exact: AHashMap<u64, ()> = AHashMap::new();
    chunks.retain(|c| {
        let key = fast_hash(&normalize_whitespace(&c.content));
        seen_exact.insert(key, ()).is_none()
    });
    let after_exact = chunks.len();

    // Near-dup pass
    let mut line_sets: Vec<Vec<u64>> = Vec::with_capacity(chunks.len());
    let mut keep_flags: Vec<bool> = Vec::with_capacity(chunks.len());

    for c in chunks.iter() {
        let set: Vec<u64> = c
            .content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(line_fingerprint)
            .collect();

        let mut drop = false;
        for (i, prev) in line_sets.iter().enumerate() {
            if !keep_flags[i] {
                continue;
            }
            if jaccard_sorted(&set, prev) >= similarity_threshold as f64 {
                drop = true;
                break;
            }
        }
        line_sets.push(set);
        keep_flags.push(!drop);
    }

    let mut iter = keep_flags.iter();
    chunks.retain(|_| *iter.next().unwrap());

    let after_all = chunks.len();
    Stats {
        exact_removed: before - after_exact,
        near_removed: after_exact - after_all,
        kept: after_all,
    }
}

/// Jaccard similarity over two line-fingerprint multisets.
/// Returns 1.0 for identical, 0.0 for disjoint.
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
    fn whitespace_only_differences_are_dups() {
        let mut v = vec![c("a", "fn x() {}"), c("b", "fn   x()   {}")];
        run(&mut v, 0.9);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn near_dup_catches_small_edits() {
        let big_a = (0..30)
            .map(|i| format!("let x{i} = {i};"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut big_b = big_a.clone();
        big_b.push_str("\nlet extra = 1;");
        let mut v = vec![c("a", &big_a), c("b", &big_b)];
        run(&mut v, 0.9);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn disjoint_chunks_both_kept() {
        let mut v = vec![c("a", "fn apple() {}"), c("b", "struct Banana;")];
        run(&mut v, 0.9);
        assert_eq!(v.len(), 2);
    }
}
