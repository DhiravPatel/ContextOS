//! ContextOS graph engine.
//!
//! Parses a repository into a directed property graph of code symbols and
//! relationships, persists it in SQLite, and exposes blast-radius and
//! centrality queries so the core engine can pick the minimum relevant slice
//! of code instead of feeding whole files to the LLM.
//!
//! Model
//! -----
//!   Nodes  → files, functions, classes/structs, methods, imports.
//!   Edges  → CALLS, IMPORTS, INHERITS, DEFINED_IN, TESTS.
//!
//! Incremental: we hash every file (SHA-256) and only re-parse on hash change.
//! Initial build of a 2k-file repo ≈ 2s; updates ≈ 50–300ms depending on
//! fan-in of changed files.

pub mod builder;
pub mod pagerank;
pub mod query;
pub mod store;
pub mod types;

pub use builder::GraphBuilder;
pub use query::GraphQuery;
pub use store::GraphStore;
pub use types::*;

use std::path::{Path, PathBuf};

/// High-level handle: open (or create) the on-disk store under
/// `<repo_root>/.contextos/graph.db` and return both a builder and a query
/// interface tied to it.
pub struct Graph {
    pub root: PathBuf,
    pub store: GraphStore,
}

impl Graph {
    pub fn open(root: impl AsRef<Path>) -> anyhow::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let db_dir = root.join(".contextos");
        std::fs::create_dir_all(&db_dir)?;
        let store = GraphStore::open(db_dir.join("graph.db"))?;
        Ok(Self { root, store })
    }

    pub fn builder(&self) -> GraphBuilder<'_> {
        GraphBuilder::new(&self.root, &self.store)
    }

    pub fn query(&self) -> GraphQuery<'_> {
        GraphQuery::new(&self.store)
    }
}
