use contextos_utils::Language;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    File,
    Function,
    Method,
    Class,
    Import,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// A → B: A contains B (file → function, class → method).
    Contains,
    /// A → B: A calls B.
    Calls,
    /// A → B: A imports B.
    Imports,
    /// A → B: A extends/implements B.
    Inherits,
    /// A → B: A tests B (heuristic: test file names).
    Tests,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: i64,
    pub kind: NodeKind,
    pub name: String,
    pub qualified: String, // e.g. "src/foo.rs::Bar::baz"
    pub path: String,
    pub language: Language,
    pub start_line: u32,
    pub end_line: u32,
    /// Signature-only view (function/method header, class decl) — cached so
    /// skeleton queries are free.
    pub signature: Option<String>,
    /// Byte length of the *full* body (used for size-aware pruning).
    pub body_bytes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub src: i64,
    pub dst: i64,
    pub kind: EdgeKind,
    pub confidence: f32, // 1.0 = exact, <1 = heuristic
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub path: String,
    pub sha256: String,
    pub language: Language,
    pub last_indexed: i64, // unix seconds
}
