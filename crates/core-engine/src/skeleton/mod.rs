//! Skeleton extraction — signature-only view of a source file.
//!
//! Use case: we want the LLM to *know* a function exists (so it can reference
//! it, honour its signature, import it correctly) but we don't need the body.
//! A 200-line helper file can become 20 lines of declarations with no
//! semantic change to the LLM's answer.
//!
//! Tree-sitter walk:
//!   * For function/method decls — emit `signature {}`, drop the body.
//!   * For classes/structs — keep the head and public method signatures.
//!   * For imports — keep verbatim.
//!   * Everything else is dropped.

use crate::types::InputChunk;
use contextos_tokenizer::estimate_tokens;
use contextos_utils::Language;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser, Tree};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub chunks_skeletonised: usize,
    pub tokens_before: usize,
    pub tokens_after: usize,
}

/// Apply `skeletonise` to every chunk where `skeleton_hint == true`.
/// Leaves other chunks untouched. Returns stats for observability.
pub fn apply(chunks: &mut [InputChunk]) -> Stats {
    let mut stats = Stats::default();
    for c in chunks.iter_mut() {
        if !c.skeleton_hint {
            continue;
        }
        let before = estimate_tokens(&c.content);
        if let Some(reduced) = skeletonise(&c.content, c.language) {
            if reduced.len() < c.content.len() {
                stats.tokens_before += before;
                stats.tokens_after += estimate_tokens(&reduced);
                stats.chunks_skeletonised += 1;
                c.content = reduced;
            }
        }
    }
    stats
}

pub fn skeletonise(source: &str, lang: Language) -> Option<String> {
    let grammar = match lang {
        Language::Rust => tree_sitter_rust::language(),
        Language::TypeScript => tree_sitter_typescript::language_typescript(),
        Language::JavaScript => tree_sitter_javascript::language(),
        Language::Python => tree_sitter_python::language(),
        _ => return None,
    };
    let mut parser = Parser::new();
    parser.set_language(&grammar).ok()?;
    let tree: Tree = parser.parse(source, None)?;
    let src = source.as_bytes();

    let mut out = String::new();
    walk(tree.root_node(), src, lang, &mut out, 0);
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

fn walk(node: Node<'_>, src: &[u8], lang: Language, out: &mut String, depth: usize) {
    let kind = node.kind();

    if is_import(kind, lang) {
        push_line(out, depth, text_of(node, src).trim());
        return;
    }

    if is_function(kind, lang) {
        let sig = signature(node, src, lang);
        push_line(out, depth, &format!("{sig} {{ /* … */ }}"));
        return;
    }

    if is_container(kind, lang) {
        let head = container_head(node, src, lang);
        push_line(out, depth, &format!("{head} {{"));
        if let Some(body) = node.child_by_field_name("body") {
            let mut c = body.walk();
            for child in body.children(&mut c) {
                walk(child, src, lang, out, depth + 1);
            }
        } else {
            let mut c = node.walk();
            for child in node.children(&mut c) {
                if child.kind() != kind {
                    walk(child, src, lang, out, depth + 1);
                }
            }
        }
        push_line(out, depth, "}");
        return;
    }

    // Not an interesting node — descend looking for one.
    let mut c = node.walk();
    for child in node.children(&mut c) {
        walk(child, src, lang, out, depth);
    }
}

fn push_line(out: &mut String, depth: usize, line: &str) {
    if line.trim().is_empty() {
        return;
    }
    for _ in 0..depth {
        out.push_str("  ");
    }
    out.push_str(line);
    out.push('\n');
}

fn text_of<'a>(node: Node<'_>, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[node.start_byte()..node.end_byte()]).unwrap_or("")
}

fn signature(node: Node<'_>, src: &[u8], _lang: Language) -> String {
    // Whatever precedes the body counts as the signature.
    if let Some(body) = node.child_by_field_name("body") {
        let end = body.start_byte();
        if end > node.start_byte() {
            return std::str::from_utf8(&src[node.start_byte()..end])
                .unwrap_or("")
                .trim()
                .trim_end_matches('{')
                .trim()
                .to_string();
        }
    }
    text_of(node, src).lines().next().unwrap_or("").to_string()
}

fn container_head(node: Node<'_>, src: &[u8], _lang: Language) -> String {
    if let Some(body) = node.child_by_field_name("body") {
        let end = body.start_byte();
        if end > node.start_byte() {
            return std::str::from_utf8(&src[node.start_byte()..end])
                .unwrap_or("")
                .trim()
                .trim_end_matches('{')
                .trim()
                .to_string();
        }
    }
    text_of(node, src).lines().next().unwrap_or("").to_string()
}

fn is_import(kind: &str, lang: Language) -> bool {
    match lang {
        Language::Rust => matches!(kind, "use_declaration"),
        Language::TypeScript | Language::JavaScript => {
            matches!(kind, "import_statement" | "import_declaration")
        }
        Language::Python => matches!(kind, "import_statement" | "import_from_statement"),
        _ => false,
    }
}
fn is_function(kind: &str, lang: Language) -> bool {
    match lang {
        Language::Rust => matches!(kind, "function_item"),
        Language::TypeScript | Language::JavaScript => matches!(
            kind,
            "function_declaration" | "method_definition" | "function"
        ),
        Language::Python => matches!(kind, "function_definition"),
        _ => false,
    }
}
fn is_container(kind: &str, lang: Language) -> bool {
    match lang {
        Language::Rust => matches!(kind, "struct_item" | "enum_item" | "trait_item" | "impl_item"),
        Language::TypeScript | Language::JavaScript => {
            matches!(kind, "class_declaration" | "interface_declaration")
        }
        Language::Python => matches!(kind, "class_definition"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_function_becomes_signature_only() {
        let src = r#"
            pub fn add(a: i32, b: i32) -> i32 {
                let s = a + b;
                println!("{s}");
                s
            }
        "#;
        let sk = skeletonise(src, Language::Rust).unwrap();
        assert!(sk.contains("pub fn add"));
        assert!(!sk.contains("println"));
        assert!(!sk.contains("let s"));
        assert!(sk.len() < src.len());
    }

    #[test]
    fn typescript_class_keeps_method_signatures() {
        let src = r#"
            export class Parser {
                parse(input: string): AST {
                    const tokens = this.tokenize(input);
                    return this.build(tokens);
                }
                tokenize(input: string): Token[] {
                    return input.split(' ');
                }
            }
        "#;
        let sk = skeletonise(src, Language::TypeScript).unwrap();
        assert!(sk.contains("parse"));
        assert!(sk.contains("tokenize"));
        assert!(!sk.contains("split"));
    }
}
