//! Shared primitives used across the ContextOS engine.
//!
//! Kept intentionally tiny: hashing, language detection, light text helpers.
//! Anything bigger belongs in a domain crate (parser, core-engine, ...).

pub mod count_min;
pub use count_min::CountMinSketch;

use ahash::AHasher;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::hash::{BuildHasher, Hash, Hasher};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Json,
    Markdown,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "rs" => Language::Rust,
            "ts" | "tsx" => Language::TypeScript,
            "js" | "jsx" | "mjs" | "cjs" => Language::JavaScript,
            "py" | "pyi" => Language::Python,
            "json" => Language::Json,
            "md" | "markdown" => Language::Markdown,
            _ => Language::Unknown,
        }
    }

    pub fn from_path(path: &str) -> Self {
        match path.rsplit('.').next() {
            Some(ext) if !ext.is_empty() && ext.len() < path.len() => Self::from_extension(ext),
            _ => Language::Unknown,
        }
    }

    pub fn line_comment_prefixes(&self) -> &'static [&'static str] {
        match self {
            Language::Rust
            | Language::TypeScript
            | Language::JavaScript => &["//"],
            Language::Python => &["#"],
            Language::Json | Language::Markdown | Language::Unknown => &[],
        }
    }
}

/// Fast 64-bit hash used throughout the engine for fingerprinting.
///
/// Note: uses `AHasher::default()`, which seeds randomly per process. Two
/// runs of the same binary will produce different hash values for the same
/// input. That's fine for in-process tasks (dedup, MinHash signatures —
/// everything is built and consumed within a single optimize() call) but
/// unsuitable for *cross-process* stability. For that, see [`stable_hash`].
pub fn fast_hash<T: Hash + ?Sized>(value: &T) -> u64 {
    let mut h = AHasher::default();
    value.hash(&mut h);
    h.finish()
}

/// Deterministic 64-bit hash, identical across processes and runs.
///
/// Used for cache-aware ordering of pipeline output: identical inputs across
/// repeated CLI invocations must produce identical chunk orderings, so
/// downstream LLM provider prompt caches actually hit. We use ahash with
/// explicit seeds so we get aHash's speed without its per-process
/// randomisation. The seed values are fixed FNV-1a constants — there is no
/// security claim here, only stability.
pub fn stable_hash<T: Hash + ?Sized>(value: &T) -> u64 {
    use ahash::RandomState;
    let state = RandomState::with_seeds(
        0xcbf2_9ce4_8422_2325,
        0x100_0000_01b3,
        0x9E37_79B9_7F4A_7C15,
        0xC2B2_AE35_07C3_B3F5,
    );
    let mut h = state.build_hasher();
    value.hash(&mut h);
    h.finish()
}

/// Hash a normalized version of a line (trim + collapse whitespace).
pub fn line_fingerprint(line: &str) -> u64 {
    fast_hash(&normalize_whitespace(line))
}

/// Collapse any run of whitespace to a single space and trim.
pub fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Content-defined chunking using a 48-byte rolling Rabin-style fingerprint.
///
/// Produces a list of `(start, end)` byte ranges whose boundaries are derived
/// from *content*, not absolute byte offsets. The key property: inserting,
/// deleting, or modifying bytes in the middle of a stream only invalidates
/// the chunk(s) that contain the change — surrounding chunks keep the same
/// boundaries. This is the same idea as restic / rsync / IPFS chunkers.
///
/// Why we want this for token-reduction: when a user edits a single file,
/// rebuilding a chunk-level hash table only re-keys the chunks that crossed
/// the edited bytes. Caches downstream (dedup index, prompt cache) hit far
/// more often than with fixed-line chunking.
///
/// ## Algorithm
///
/// Standard rolling hash over a 48-byte window. We declare a chunk boundary
/// when the low `mask_bits` of the hash are zero, **and** the chunk is at
/// least `min_size` bytes. We force a boundary at `max_size` to bound chunk
/// length.
///
/// With `mask_bits = 13` the expected chunk size is 2^13 = 8192 bytes; we
/// clamp to `[min_size, max_size]`. These defaults work well for source code:
/// 1 KiB minimum (function-sized), 16 KiB maximum (one screen of file).
///
/// Complexity: O(N) over the input bytes, one mul + one xor + one shift per
/// byte. ~1 GB/s on a modern laptop; trivially within the engine's latency
/// budget.
pub fn rabin_chunks(bytes: &[u8]) -> Vec<(usize, usize)> {
    rabin_chunks_with(bytes, RABIN_MIN_SIZE, RABIN_MAX_SIZE, RABIN_MASK_BITS)
}

/// Lower bound on chunk size. Smaller than this and the boundary signal is
/// noisy; bigger than this and we lose the locality benefit.
pub const RABIN_MIN_SIZE: usize = 1024;
pub const RABIN_MAX_SIZE: usize = 16 * 1024;
pub const RABIN_MASK_BITS: u32 = 13; // expected chunk size = 2^13 = 8 KiB
const RABIN_WINDOW: usize = 48;

/// 256-entry table of pseudo-random 64-bit values, one per byte. Used to
/// "spread" the input bytes through the rolling hash. Computed once via
/// `once_cell`; the seed values are FNV-1a primes so the table is fully
/// deterministic across runs.
static RABIN_TABLE: Lazy<[u64; 256]> = Lazy::new(|| {
    let mut t = [0u64; 256];
    let mut x: u64 = 0xcbf2_9ce4_8422_2325;
    for slot in t.iter_mut() {
        x = x.wrapping_mul(0x100_0000_01b3);
        x ^= x >> 33;
        *slot = x;
    }
    t
});

pub fn rabin_chunks_with(
    bytes: &[u8],
    min_size: usize,
    max_size: usize,
    mask_bits: u32,
) -> Vec<(usize, usize)> {
    let n = bytes.len();
    if n == 0 {
        return Vec::new();
    }
    if n <= min_size {
        return vec![(0, n)];
    }
    let mask: u64 = (1u64 << mask_bits) - 1;
    let table = &*RABIN_TABLE;

    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut h: u64 = 0;

    while i < n {
        let entering = table[bytes[i] as usize];
        h = h.rotate_left(1) ^ entering;
        if i >= RABIN_WINDOW {
            let leaving = table[bytes[i - RABIN_WINDOW] as usize];
            // Roll the leaving byte out; rotating its initial entry forward
            // by `RABIN_WINDOW` positions matches the rotate_left above.
            h ^= leaving.rotate_left(RABIN_WINDOW as u32);
        }

        let chunk_len = i - start + 1;
        let boundary = chunk_len >= min_size && (h & mask) == 0;
        let forced = chunk_len >= max_size;

        if boundary || forced {
            out.push((start, i + 1));
            start = i + 1;
            h = 0;
        }
        i += 1;
    }
    if start < n {
        out.push((start, n));
    }
    out
}

/// Very cheap alphanumeric tokenizer for ranking/scoring.
pub fn tokenize_words(text: &str) -> Vec<String> {
    static STOP: Lazy<ahash::AHashSet<&'static str>> = Lazy::new(|| {
        let stop = [
            "the", "a", "an", "and", "or", "but", "if", "then", "else", "for", "while",
            "in", "of", "to", "is", "are", "was", "were", "be", "been", "being",
            "do", "does", "did", "done", "it", "this", "that", "these", "those",
            "as", "at", "by", "from", "on", "with",
        ];
        stop.into_iter().collect()
    });

    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty() && w.len() > 1)
        .map(|w| w.to_ascii_lowercase())
        .filter(|w| !STOP.contains(w.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_from_extension() {
        assert_eq!(Language::from_extension("rs"), Language::Rust);
        assert_eq!(Language::from_extension(".ts"), Language::TypeScript);
        assert_eq!(Language::from_extension("PY"), Language::Python);
        assert_eq!(Language::from_extension("xyz"), Language::Unknown);
    }

    #[test]
    fn normalize_whitespace_collapses() {
        assert_eq!(normalize_whitespace("  hello   world  "), "hello world");
    }

    #[test]
    fn identical_lines_hash_identically() {
        let a = line_fingerprint("  let x = 1;");
        let b = line_fingerprint("let x = 1;");
        assert_eq!(a, b);
    }

    #[test]
    fn tokenize_drops_stopwords() {
        let t = tokenize_words("The quick brown fox jumps over");
        assert!(!t.contains(&"the".to_string()));
        assert!(t.contains(&"quick".to_string()));
    }

    #[test]
    fn stable_hash_is_deterministic_within_process() {
        // Sanity check: same input → same output. (Full cross-process
        // determinism is verified by the integration test that re-invokes
        // the CLI and compares chunk orderings.)
        let a = stable_hash("alpha");
        let b = stable_hash("alpha");
        let c = stable_hash("beta");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn rabin_chunks_short_input_emits_one_chunk() {
        let chunks = rabin_chunks(b"hello world");
        assert_eq!(chunks, vec![(0, 11)]);
    }

    #[test]
    fn rabin_chunks_partition_full_input() {
        let payload: Vec<u8> = (0..200_000u32).flat_map(|i| i.to_le_bytes()).collect();
        let chunks = rabin_chunks(&payload);
        assert!(chunks.len() > 1, "expected multiple chunks, got {}", chunks.len());

        // Every chunk respects the size bounds (last chunk may be smaller).
        for (a, b) in chunks.iter().take(chunks.len() - 1) {
            let len = b - a;
            assert!(len >= RABIN_MIN_SIZE);
            assert!(len <= RABIN_MAX_SIZE);
        }
        // Chunks form a contiguous, non-overlapping cover of the input.
        assert_eq!(chunks.first().unwrap().0, 0);
        assert_eq!(chunks.last().unwrap().1, payload.len());
        for w in chunks.windows(2) {
            assert_eq!(w[0].1, w[1].0);
        }
    }

    #[test]
    fn rabin_local_edit_only_invalidates_local_chunks() {
        // Build a payload long enough to produce many chunks; insert a few
        // bytes deep in the middle and confirm that the *prefix* boundaries
        // before the edit are stable.
        let mut a: Vec<u8> = Vec::with_capacity(60_000);
        for i in 0..60_000u32 {
            a.extend_from_slice(&i.to_le_bytes());
        }
        let mut b = a.clone();
        let edit_pos = 30_000;
        b.splice(edit_pos..edit_pos, b"local change here".iter().copied());

        let chunks_a = rabin_chunks(&a);
        let chunks_b = rabin_chunks(&b);

        let stable = chunks_a
            .iter()
            .zip(chunks_b.iter())
            .take_while(|(x, y)| x == y && x.1 < edit_pos)
            .count();
        assert!(
            stable >= 2,
            "expected at least 2 stable prefix chunks, got {stable}"
        );
    }
}
