//! Shared public types for the core engine.

use contextos_utils::Language;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChunkKind {
    Code,
    Comment,
    Doc,
    Diagnostic,
    Selection,
}

impl Default for ChunkKind {
    fn default() -> Self {
        ChunkKind::Code
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputChunk {
    pub id: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default = "default_lang")]
    pub language: Language,
    pub content: String,
    #[serde(default)]
    pub kind: ChunkKind,
    /// Caller-supplied bump added to the ranker score. Use for "active file",
    /// "selected region", "file currently under cursor", etc.
    #[serde(default)]
    pub priority: i32,
    /// If true, the engine will reduce this chunk to a signature-only
    /// skeleton before dedup/rank/budget. Typical source: graph-picked
    /// "transitive dependency, relevant but not central" files.
    #[serde(default)]
    pub skeleton_hint: bool,
}

fn default_lang() -> Language {
    Language::Unknown
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationRequest {
    pub chunks: Vec<InputChunk>,
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    pub chunks: Vec<InputChunk>,
    pub original_tokens: usize,
    pub final_tokens: usize,
    pub tokens_saved: usize,
    pub reduction_pct: f64,
    pub elapsed_ms: f64,
    pub stats: crate::PipelineStats,
}
