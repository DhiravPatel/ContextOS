//! Walks a repository, parses every supported source file with tree-sitter,
//! and upserts nodes/edges into the store.
//!
//! Runs in two modes:
//! * `build()` — full reindex (respects .gitignore via the `ignore` crate).
//! * `update(paths)` — only the supplied paths; skips unchanged hashes.

use crate::store::GraphStore;
use crate::types::{Edge, EdgeKind, FileRecord, Node, NodeKind};
use anyhow::{Context, Result};
use contextos_utils::Language;
use ignore::WalkBuilder;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tree_sitter::{Node as TsNode, Parser, Tree};

pub struct GraphBuilder<'a> {
    root: &'a Path,
    store: &'a GraphStore,
}

#[derive(Debug, Clone, Default)]
pub struct BuildReport {
    pub files_scanned: usize,
    pub files_reparsed: usize,
    pub files_skipped: usize,
    pub nodes_written: usize,
    pub edges_written: usize,
}

impl<'a> GraphBuilder<'a> {
    pub fn new(root: &'a Path, store: &'a GraphStore) -> Self {
        Self { root, store }
    }

    /// Full reindex. Walks the repo (honouring .gitignore + hidden files) and
    /// re-parses any file whose hash has changed since last build.
    pub fn build(&self) -> Result<BuildReport> {
        let mut report = BuildReport::default();
        let walker = WalkBuilder::new(self.root)
            .hidden(false)
            .git_ignore(true)
            .parents(true)
            .build();

        for entry in walker.flatten() {
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            let lang = Language::from_path(&path.to_string_lossy());
            if matches!(lang, Language::Unknown | Language::Json | Language::Markdown) {
                continue;
            }
            report.files_scanned += 1;
            let rel = relativise(self.root, path);
            if self.index_file(path, &rel, lang, &mut report)? {
                report.files_reparsed += 1;
            } else {
                report.files_skipped += 1;
            }
        }
        Ok(report)
    }

    /// Incremental: re-parse only the listed files and any file whose node
    /// was transitively linked to them. Callers typically pass the output of
    /// `git diff --name-only`.
    pub fn update(&self, paths: &[PathBuf]) -> Result<BuildReport> {
        let mut report = BuildReport::default();
        for path in paths {
            let abs = if path.is_absolute() {
                path.clone()
            } else {
                self.root.join(path)
            };
            if !abs.exists() {
                let rel = relativise(self.root, &abs);
                self.store.delete_file(&rel)?;
                continue;
            }
            let lang = Language::from_path(&abs.to_string_lossy());
            if matches!(lang, Language::Unknown | Language::Json | Language::Markdown) {
                continue;
            }
            report.files_scanned += 1;
            let rel = relativise(self.root, &abs);
            if self.index_file(&abs, &rel, lang, &mut report)? {
                report.files_reparsed += 1;
            } else {
                report.files_skipped += 1;
            }
        }
        Ok(report)
    }

    fn index_file(
        &self,
        abs: &Path,
        rel: &str,
        lang: Language,
        report: &mut BuildReport,
    ) -> Result<bool> {
        let bytes = std::fs::read(abs).with_context(|| format!("reading {}", abs.display()))?;
        let sha = hex_sha256(&bytes);
        if let Some(prev) = self.store.get_file_sha(rel)? {
            if prev == sha {
                return Ok(false);
            }
        }
        // Hash changed (or new file) — reset what we knew about it.
        self.store.delete_file(rel)?;

        let source = String::from_utf8_lossy(&bytes).into_owned();
        let parsed = parse(&source, lang);

        let file_node = Node {
            id: 0,
            kind: NodeKind::File,
            name: Path::new(rel)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| rel.to_string()),
            qualified: format!("{rel}::<file>"),
            path: rel.to_string(),
            language: lang,
            start_line: 1,
            end_line: source.lines().count().max(1) as u32,
            signature: None,
            body_bytes: bytes.len() as u32,
        };
        let file_id = self.store.insert_node(&file_node)?;
        report.nodes_written += 1;

        if let Some(tree) = parsed.tree {
            self.walk_symbols(
                &tree,
                source.as_bytes(),
                lang,
                rel,
                file_id,
                report,
            )?;
            self.extract_imports(&tree, source.as_bytes(), lang, rel, file_id, report)?;
        }

        self.store.upsert_file(&FileRecord {
            path: rel.to_string(),
            sha256: sha,
            language: lang,
            last_indexed: unix_now(),
        })?;
        Ok(true)
    }

    fn walk_symbols(
        &self,
        tree: &Tree,
        src: &[u8],
        lang: Language,
        rel: &str,
        file_id: i64,
        report: &mut BuildReport,
    ) -> Result<()> {
        let mut stack: Vec<(TsNode, Option<String>, Option<i64>)> =
            vec![(tree.root_node(), None, Some(file_id))];
        while let Some((node, class_name, parent_id)) = stack.pop() {
            let kind = node.kind();

            if is_class_decl(kind, lang) {
                let name = name_of(node, src).unwrap_or_else(|| "<anonymous>".into());
                let qualified = format!("{rel}::{name}");
                let cls = Node {
                    id: 0,
                    kind: NodeKind::Class,
                    name: name.clone(),
                    qualified: qualified.clone(),
                    path: rel.to_string(),
                    language: lang,
                    start_line: (node.start_position().row + 1) as u32,
                    end_line: (node.end_position().row + 1) as u32,
                    signature: Some(first_line(node, src).to_string()),
                    body_bytes: (node.end_byte() - node.start_byte()) as u32,
                };
                let id = self.store.insert_node(&cls)?;
                report.nodes_written += 1;
                if let Some(pid) = parent_id {
                    self.store.insert_edge(&Edge {
                        src: pid,
                        dst: id,
                        kind: EdgeKind::Contains,
                        confidence: 1.0,
                    })?;
                    report.edges_written += 1;
                }
                // Inheritance edges (best effort)
                for base in bases_of(node, src, lang) {
                    if let Some(target) =
                        self.store.find_node_by_name(&base, 1)?.into_iter().next()
                    {
                        self.store.insert_edge(&Edge {
                            src: id,
                            dst: target.id,
                            kind: EdgeKind::Inherits,
                            confidence: 0.8,
                        })?;
                        report.edges_written += 1;
                    }
                }
                let mut c = node.walk();
                for child in node.children(&mut c) {
                    stack.push((child, Some(name.clone()), Some(id)));
                }
                continue;
            }

            if is_function_decl(kind, lang) {
                let name = name_of(node, src).unwrap_or_else(|| "<anonymous>".into());
                let qualified = match class_name.as_ref() {
                    Some(cls) => format!("{rel}::{cls}::{name}"),
                    None => format!("{rel}::{name}"),
                };
                let fn_node = Node {
                    id: 0,
                    kind: if class_name.is_some() {
                        NodeKind::Method
                    } else {
                        NodeKind::Function
                    },
                    name: name.clone(),
                    qualified: qualified.clone(),
                    path: rel.to_string(),
                    language: lang,
                    start_line: (node.start_position().row + 1) as u32,
                    end_line: (node.end_position().row + 1) as u32,
                    signature: Some(extract_signature(node, src, lang)),
                    body_bytes: (node.end_byte() - node.start_byte()) as u32,
                };
                let fn_id = self.store.insert_node(&fn_node)?;
                report.nodes_written += 1;
                if let Some(pid) = parent_id {
                    self.store.insert_edge(&Edge {
                        src: pid,
                        dst: fn_id,
                        kind: EdgeKind::Contains,
                        confidence: 1.0,
                    })?;
                    report.edges_written += 1;
                }
                // Call edges from function body (best effort).
                self.extract_calls(node, src, lang, fn_id, report)?;
                // Don't recurse further for call/class inspection from inside
                // a function — nested functions are uncommon enough to ignore.
                continue;
            }

            let mut c = node.walk();
            for child in node.children(&mut c) {
                stack.push((child, class_name.clone(), parent_id));
            }
        }
        Ok(())
    }

    fn extract_calls(
        &self,
        fn_node: TsNode<'_>,
        src: &[u8],
        lang: Language,
        fn_id: i64,
        report: &mut BuildReport,
    ) -> Result<()> {
        let mut stack = vec![fn_node];
        while let Some(n) = stack.pop() {
            if is_call(n.kind(), lang) {
                if let Some(callee) = callee_name(n, src, lang) {
                    if let Some(target) =
                        self.store.find_node_by_name(&callee, 1)?.into_iter().next()
                    {
                        self.store.insert_edge(&Edge {
                            src: fn_id,
                            dst: target.id,
                            kind: EdgeKind::Calls,
                            confidence: 0.7,
                        })?;
                        report.edges_written += 1;
                    }
                }
            }
            let mut c = n.walk();
            for child in n.children(&mut c) {
                stack.push(child);
            }
        }
        Ok(())
    }

    fn extract_imports(
        &self,
        tree: &Tree,
        src: &[u8],
        lang: Language,
        rel: &str,
        file_id: i64,
        report: &mut BuildReport,
    ) -> Result<()> {
        let mut stack = vec![tree.root_node()];
        while let Some(n) = stack.pop() {
            if is_import(n.kind(), lang) {
                let text = std::str::from_utf8(&src[n.start_byte()..n.end_byte()])
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if text.is_empty() {
                    continue;
                }
                let qualified = format!("{rel}::import::{}", text);
                let imp = Node {
                    id: 0,
                    kind: NodeKind::Import,
                    name: text.clone(),
                    qualified,
                    path: rel.to_string(),
                    language: lang,
                    start_line: (n.start_position().row + 1) as u32,
                    end_line: (n.end_position().row + 1) as u32,
                    signature: Some(text.clone()),
                    body_bytes: (n.end_byte() - n.start_byte()) as u32,
                };
                let iid = self.store.insert_node(&imp)?;
                report.nodes_written += 1;
                self.store.insert_edge(&Edge {
                    src: file_id,
                    dst: iid,
                    kind: EdgeKind::Imports,
                    confidence: 1.0,
                })?;
                report.edges_written += 1;
            }
            let mut c = n.walk();
            for child in n.children(&mut c) {
                stack.push(child);
            }
        }
        Ok(())
    }
}

// ---------------- tree-sitter helpers ----------------

struct Parsed {
    tree: Option<Tree>,
}

fn parse(source: &str, lang: Language) -> Parsed {
    let grammar = match lang {
        Language::Rust => tree_sitter_rust::language(),
        Language::TypeScript => tree_sitter_typescript::language_typescript(),
        Language::JavaScript => tree_sitter_javascript::language(),
        Language::Python => tree_sitter_python::language(),
        _ => return Parsed { tree: None },
    };
    let mut parser = Parser::new();
    if parser.set_language(&grammar).is_err() {
        return Parsed { tree: None };
    }
    Parsed {
        tree: parser.parse(source, None),
    }
}

fn is_class_decl(kind: &str, lang: Language) -> bool {
    match lang {
        Language::Rust => matches!(kind, "struct_item" | "enum_item" | "trait_item" | "impl_item"),
        Language::TypeScript | Language::JavaScript => {
            matches!(kind, "class_declaration" | "interface_declaration")
        }
        Language::Python => matches!(kind, "class_definition"),
        _ => false,
    }
}
fn is_function_decl(kind: &str, lang: Language) -> bool {
    match lang {
        Language::Rust => matches!(kind, "function_item"),
        Language::TypeScript | Language::JavaScript => matches!(
            kind,
            "function_declaration" | "method_definition" | "function" | "arrow_function"
        ),
        Language::Python => matches!(kind, "function_definition"),
        _ => false,
    }
}
fn is_call(kind: &str, lang: Language) -> bool {
    match lang {
        Language::Rust => matches!(kind, "call_expression" | "macro_invocation"),
        Language::TypeScript | Language::JavaScript => matches!(kind, "call_expression"),
        Language::Python => matches!(kind, "call"),
        _ => false,
    }
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

fn name_of(node: TsNode<'_>, src: &[u8]) -> Option<String> {
    for field in ["name", "identifier"] {
        if let Some(child) = node.child_by_field_name(field) {
            if let Ok(s) = std::str::from_utf8(&src[child.start_byte()..child.end_byte()]) {
                return Some(s.to_string());
            }
        }
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if child.kind() == "identifier" || child.kind() == "type_identifier" {
            if let Ok(s) = std::str::from_utf8(&src[child.start_byte()..child.end_byte()]) {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn callee_name(node: TsNode<'_>, src: &[u8], _lang: Language) -> Option<String> {
    if let Some(child) = node
        .child_by_field_name("function")
        .or_else(|| node.child_by_field_name("macro"))
    {
        // Strip `foo.bar()` → `bar`, `a::b::c()` → `c`.
        let raw = std::str::from_utf8(&src[child.start_byte()..child.end_byte()]).ok()?;
        return Some(
            raw.rsplit(|c: char| c == '.' || c == ':')
                .next()
                .unwrap_or(raw)
                .trim()
                .to_string(),
        );
    }
    None
}

fn bases_of(node: TsNode<'_>, src: &[u8], lang: Language) -> Vec<String> {
    let mut out = Vec::new();
    match lang {
        Language::TypeScript | Language::JavaScript => {
            if let Some(clause) = node.child_by_field_name("superclass") {
                if let Ok(s) = std::str::from_utf8(&src[clause.start_byte()..clause.end_byte()]) {
                    out.push(s.trim().to_string());
                }
            }
        }
        Language::Python => {
            if let Some(args) = node.child_by_field_name("superclasses") {
                if let Ok(s) = std::str::from_utf8(&src[args.start_byte()..args.end_byte()]) {
                    for part in s.trim_matches(|c| c == '(' || c == ')').split(',') {
                        let p = part.trim();
                        if !p.is_empty() {
                            out.push(p.to_string());
                        }
                    }
                }
            }
        }
        _ => {}
    }
    out
}

fn extract_signature(node: TsNode<'_>, src: &[u8], lang: Language) -> String {
    // Signature = everything up to the body opener. Fall back to first line.
    let body_field = match lang {
        Language::Rust | Language::JavaScript | Language::TypeScript => "body",
        Language::Python => "body",
        _ => "body",
    };
    if let Some(body) = node.child_by_field_name(body_field) {
        let end = body.start_byte();
        if end > node.start_byte() {
            if let Ok(s) = std::str::from_utf8(&src[node.start_byte()..end]) {
                return s.trim().trim_end_matches('{').trim().to_string();
            }
        }
    }
    first_line(node, src).to_string()
}

fn first_line<'a>(node: TsNode<'_>, src: &'a [u8]) -> &'a str {
    let s = std::str::from_utf8(&src[node.start_byte()..node.end_byte()]).unwrap_or("");
    s.lines().next().unwrap_or("")
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn relativise(root: &Path, abs: &Path) -> String {
    abs.strip_prefix(root)
        .unwrap_or(abs)
        .to_string_lossy()
        .into_owned()
}
