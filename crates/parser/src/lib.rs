//! Source-code aware stripping.
//!
//! Two backends:
//!   1. **Tree-sitter** (feature `tree-sitter-langs`, default) — AST-aware,
//!      never touches string/regex literals, handles nested block comments.
//!   2. **Regex fallback** — no C compiler required. Good for a 30-40%
//!      reduction; misses edge cases (comment markers inside strings) but
//!      always safe to run.
//!
//! The engine picks whichever is available at runtime; callers don't care.

use contextos_utils::Language;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StripOptions {
    pub remove_comments: bool,
    pub remove_debug_logs: bool,
    pub remove_empty_lines: bool,
    pub collapse_whitespace: bool,
}

impl Default for StripOptions {
    fn default() -> Self {
        Self {
            remove_comments: true,
            remove_debug_logs: true,
            remove_empty_lines: true,
            collapse_whitespace: true,
        }
    }
}

pub fn strip(source: &str, lang: Language, opts: StripOptions) -> String {
    #[cfg(feature = "tree-sitter-langs")]
    {
        if let Some(result) = ts::strip_with_tree_sitter(source, lang, opts) {
            return postprocess(&result, opts);
        }
    }
    let regex_stripped = regex_strip(source, lang, opts);
    postprocess(&regex_stripped, opts)
}

fn postprocess(source: &str, opts: StripOptions) -> String {
    let mut lines: Vec<&str> = source.lines().collect();

    if opts.remove_empty_lines {
        lines.retain(|l| !l.trim().is_empty());
    }

    if opts.collapse_whitespace {
        lines
            .into_iter()
            .map(|l| collapse_inline_whitespace(l))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        lines.join("\n")
    }
}

fn collapse_inline_whitespace(line: &str) -> String {
    let leading: String = line
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    let rest = &line[leading.len()..];
    let mut out = String::with_capacity(rest.len());
    let mut prev_space = false;
    for ch in rest.chars() {
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
    let trimmed = out.trim_end();
    format!("{leading}{trimmed}")
}

// ---------- Regex backend ----------

static RE_BLOCK_COMMENT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
static RE_LINE_COMMENT_SLASH: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)//[^\n]*").unwrap());
static RE_LINE_COMMENT_HASH: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)#[^\n]*").unwrap());
static RE_PY_DOCSTRING: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?sm)^\s*(?:"""|''').*?(?:"""|''')"#).unwrap());
static RE_CONSOLE_LOG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^\s*console\.(log|debug|info|trace)\s*\([^;\n]*\)\s*;?\s*$").unwrap()
});
static RE_PRINTLN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^\s*println!\s*\([^;\n]*\)\s*;?\s*$").unwrap());
static RE_PY_PRINT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^\s*print\s*\([^)\n]*\)\s*$").unwrap());

fn regex_strip(source: &str, lang: Language, opts: StripOptions) -> String {
    let mut out = source.to_string();

    if opts.remove_comments {
        out = match lang {
            Language::Rust | Language::TypeScript | Language::JavaScript => {
                let s = RE_BLOCK_COMMENT.replace_all(&out, "").into_owned();
                RE_LINE_COMMENT_SLASH.replace_all(&s, "").into_owned()
            }
            Language::Python => {
                let s = RE_PY_DOCSTRING.replace_all(&out, "").into_owned();
                RE_LINE_COMMENT_HASH.replace_all(&s, "").into_owned()
            }
            _ => out,
        };
    }

    if opts.remove_debug_logs {
        out = match lang {
            Language::TypeScript | Language::JavaScript => {
                RE_CONSOLE_LOG.replace_all(&out, "").into_owned()
            }
            Language::Rust => RE_PRINTLN.replace_all(&out, "").into_owned(),
            Language::Python => RE_PY_PRINT.replace_all(&out, "").into_owned(),
            _ => out,
        };
    }

    out
}

// ---------- Tree-sitter backend ----------

#[cfg(feature = "tree-sitter-langs")]
mod ts {
    use super::*;
    use tree_sitter::{Node, Parser};

    pub fn strip_with_tree_sitter(
        source: &str,
        lang: Language,
        opts: StripOptions,
    ) -> Option<String> {
        let grammar = match lang {
            Language::Rust => tree_sitter_rust::language(),
            Language::TypeScript => tree_sitter_typescript::language_typescript(),
            Language::JavaScript => tree_sitter_javascript::language(),
            Language::Python => tree_sitter_python::language(),
            _ => return None,
        };

        let mut parser = Parser::new();
        parser.set_language(&grammar).ok()?;
        let tree = parser.parse(source, None)?;
        let root = tree.root_node();

        let mut removals: Vec<(usize, usize)> = Vec::new();
        collect_removals(root, source.as_bytes(), lang, opts, &mut removals);

        removals.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out = String::with_capacity(source.len());
        let mut cursor = 0usize;
        for (start, end) in removals {
            if start < cursor {
                continue; // overlap safety
            }
            out.push_str(&source[cursor..start]);
            cursor = end;
        }
        out.push_str(&source[cursor..]);
        Some(out)
    }

    fn collect_removals(
        node: Node<'_>,
        src: &[u8],
        lang: Language,
        opts: StripOptions,
        out: &mut Vec<(usize, usize)>,
    ) {
        let kind = node.kind();

        if opts.remove_comments && is_comment_kind(kind) {
            out.push((node.start_byte(), node.end_byte()));
            return;
        }

        if opts.remove_debug_logs && is_debug_log(&node, src, lang) {
            // Extend to include trailing semicolon + newline for cleanliness.
            let mut end = node.end_byte();
            while end < src.len() && matches!(src[end], b';' | b' ' | b'\t') {
                end += 1;
            }
            if end < src.len() && src[end] == b'\n' {
                end += 1;
            }
            out.push((node.start_byte(), end));
            return;
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_removals(child, src, lang, opts, out);
        }
    }

    fn is_comment_kind(kind: &str) -> bool {
        matches!(
            kind,
            "comment"
                | "line_comment"
                | "block_comment"
                | "doc_comment"
                | "documentation_comment"
        )
    }

    fn is_debug_log(node: &Node<'_>, src: &[u8], lang: Language) -> bool {
        let text = match std::str::from_utf8(&src[node.start_byte()..node.end_byte()]) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let trimmed = text.trim();
        match lang {
            Language::TypeScript | Language::JavaScript => {
                (node.kind() == "expression_statement" || node.kind() == "call_expression")
                    && (trimmed.starts_with("console.log")
                        || trimmed.starts_with("console.debug")
                        || trimmed.starts_with("console.info")
                        || trimmed.starts_with("console.trace"))
            }
            Language::Rust => {
                node.kind() == "macro_invocation"
                    && (trimmed.starts_with("println!")
                        || trimmed.starts_with("eprintln!")
                        || trimmed.starts_with("dbg!"))
            }
            Language::Python => {
                node.kind() == "call"
                    && trimmed.starts_with("print(")
                    && node
                        .parent()
                        .map(|p| p.kind() == "expression_statement")
                        .unwrap_or(false)
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_js_line_comments() {
        let src = "const x = 1; // hi\nconst y = 2;";
        let out = strip(src, Language::JavaScript, StripOptions::default());
        assert!(!out.contains("hi"));
        assert!(out.contains("const x"));
        assert!(out.contains("const y"));
    }

    #[test]
    fn strips_rust_block_comments() {
        let src = "fn a() {}\n/* block */\nfn b() {}";
        let out = strip(src, Language::Rust, StripOptions::default());
        assert!(!out.contains("block"));
        assert!(out.contains("fn a"));
        assert!(out.contains("fn b"));
    }

    #[test]
    fn removes_console_log_lines() {
        let src = "function f() {\n  console.log('x');\n  return 1;\n}";
        let out = strip(src, Language::JavaScript, StripOptions::default());
        assert!(!out.contains("console.log"));
        assert!(out.contains("return 1"));
    }

    #[test]
    fn python_hash_comments() {
        let src = "def f():\n    # pointless comment\n    return 1";
        let out = strip(src, Language::Python, StripOptions::default());
        assert!(!out.contains("pointless"));
        assert!(out.contains("return 1"));
    }

    #[test]
    fn empty_source_is_empty() {
        assert!(strip("", Language::Rust, StripOptions::default()).is_empty());
    }
}
