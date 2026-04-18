//! Shared primitives used across the ContextOS engine.
//!
//! Kept intentionally tiny: hashing, language detection, light text helpers.
//! Anything bigger belongs in a domain crate (parser, core-engine, ...).

use ahash::AHasher;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

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

/// Stable 64-bit hash used throughout the engine for fingerprinting.
pub fn fast_hash<T: Hash + ?Sized>(value: &T) -> u64 {
    let mut h = AHasher::default();
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
}
