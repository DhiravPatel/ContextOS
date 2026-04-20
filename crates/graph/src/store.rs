//! SQLite-backed graph store.
//!
//! The schema is tiny and denormalised on purpose: two tables (`nodes`,
//! `edges`) plus a `files` manifest for incremental indexing. Queries use
//! covering indexes so typical lookups stay well under 1ms even at ~100k
//! nodes.

use crate::types::{Edge, EdgeKind, FileRecord, Node, NodeKind};
use anyhow::{Context, Result};
use contextos_utils::Language;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS files (
    path        TEXT PRIMARY KEY,
    sha256      TEXT NOT NULL,
    language    TEXT NOT NULL,
    last_indexed INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS nodes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT NOT NULL,
    name        TEXT NOT NULL,
    qualified   TEXT NOT NULL UNIQUE,
    path        TEXT NOT NULL,
    language    TEXT NOT NULL,
    start_line  INTEGER NOT NULL,
    end_line    INTEGER NOT NULL,
    signature   TEXT,
    body_bytes  INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_nodes_path ON nodes(path);
CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);

CREATE TABLE IF NOT EXISTS edges (
    src         INTEGER NOT NULL,
    dst         INTEGER NOT NULL,
    kind        TEXT NOT NULL,
    confidence  REAL NOT NULL DEFAULT 1.0,
    PRIMARY KEY (src, dst, kind),
    FOREIGN KEY (src) REFERENCES nodes(id) ON DELETE CASCADE,
    FOREIGN KEY (dst) REFERENCES nodes(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_edges_src ON edges(src, kind);
CREATE INDEX IF NOT EXISTS idx_edges_dst ON edges(dst, kind);
"#;

pub struct GraphStore {
    conn: std::sync::Mutex<Connection>,
}

impl GraphStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref())
            .with_context(|| format!("opening sqlite at {}", path.as_ref().display()))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: std::sync::Mutex::new(conn),
        })
    }

    pub fn transaction<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let mut guard = self.conn.lock().unwrap();
        let tx = guard.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    // ---- file manifest --------------------------------------------------

    pub fn upsert_file(&self, rec: &FileRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO files (path, sha256, language, last_indexed) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET sha256=excluded.sha256,
                                             language=excluded.language,
                                             last_indexed=excluded.last_indexed",
            params![
                rec.path,
                rec.sha256,
                lang_to_str(rec.language),
                rec.last_indexed
            ],
        )?;
        Ok(())
    }

    pub fn get_file_sha(&self, path: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let sha: Option<String> = conn
            .query_row(
                "SELECT sha256 FROM files WHERE path = ?1",
                params![path],
                |r| r.get(0),
            )
            .optional()?;
        Ok(sha)
    }

    pub fn delete_file(&self, path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM edges WHERE src IN (SELECT id FROM nodes WHERE path = ?1)
                              OR dst IN (SELECT id FROM nodes WHERE path = ?1)",
            params![path],
        )?;
        conn.execute("DELETE FROM nodes WHERE path = ?1", params![path])?;
        conn.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        Ok(())
    }

    pub fn list_files(&self) -> Result<Vec<FileRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT path, sha256, language, last_indexed FROM files")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(FileRecord {
                    path: r.get(0)?,
                    sha256: r.get(1)?,
                    language: lang_from_str(&r.get::<_, String>(2)?),
                    last_indexed: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---- node/edge write -----------------------------------------------

    pub fn insert_node(&self, n: &Node) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO nodes
             (kind, name, qualified, path, language, start_line, end_line, signature, body_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(qualified) DO UPDATE SET
                 kind=excluded.kind,
                 name=excluded.name,
                 path=excluded.path,
                 language=excluded.language,
                 start_line=excluded.start_line,
                 end_line=excluded.end_line,
                 signature=excluded.signature,
                 body_bytes=excluded.body_bytes",
            params![
                kind_to_str(n.kind),
                n.name,
                n.qualified,
                n.path,
                lang_to_str(n.language),
                n.start_line,
                n.end_line,
                n.signature,
                n.body_bytes,
            ],
        )?;
        let id = conn.query_row(
            "SELECT id FROM nodes WHERE qualified = ?1",
            params![n.qualified],
            |r| r.get::<_, i64>(0),
        )?;
        Ok(id)
    }

    pub fn insert_edge(&self, e: &Edge) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO edges (src, dst, kind, confidence)
             VALUES (?1, ?2, ?3, ?4)",
            params![e.src, e.dst, edge_to_str(e.kind), e.confidence],
        )?;
        Ok(())
    }

    // ---- reads ----------------------------------------------------------

    pub fn find_node_by_qualified(&self, qualified: &str) -> Result<Option<Node>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, kind, name, qualified, path, language, start_line, end_line,
                        signature, body_bytes
                 FROM nodes WHERE qualified = ?1",
                params![qualified],
                row_to_node,
            )
            .optional()?;
        Ok(row)
    }

    pub fn find_node_by_name(&self, name: &str, limit: usize) -> Result<Vec<Node>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, kind, name, qualified, path, language, start_line, end_line,
                    signature, body_bytes
             FROM nodes WHERE name = ?1 LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![name, limit as i64], row_to_node)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn nodes_in_file(&self, path: &str) -> Result<Vec<Node>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, kind, name, qualified, path, language, start_line, end_line,
                    signature, body_bytes
             FROM nodes WHERE path = ?1",
        )?;
        let rows = stmt
            .query_map(params![path], row_to_node)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn node(&self, id: i64) -> Result<Option<Node>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, kind, name, qualified, path, language, start_line, end_line,
                        signature, body_bytes
                 FROM nodes WHERE id = ?1",
                params![id],
                row_to_node,
            )
            .optional()?;
        Ok(row)
    }

    pub fn neighbours(&self, node: i64, kinds: &[EdgeKind], outgoing: bool) -> Result<Vec<Edge>> {
        let conn = self.conn.lock().unwrap();
        let (src_col, dst_col) = if outgoing { ("src", "dst") } else { ("dst", "src") };
        let placeholders = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT src, dst, kind, confidence FROM edges WHERE {src_col} = ?1 AND kind IN ({placeholders})"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut bound: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::Integer(node)];
        for k in kinds {
            bound.push(rusqlite::types::Value::Text(edge_to_str(*k).to_string()));
        }
        let rows = stmt
            .query_map(rusqlite::params_from_iter(bound.iter()), |r| {
                Ok(Edge {
                    src: r.get(0)?,
                    dst: r.get(1)?,
                    kind: edge_from_str(&r.get::<_, String>(2)?),
                    confidence: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let _ = dst_col;
        Ok(rows)
    }

    pub fn all_edges(&self) -> Result<Vec<Edge>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT src, dst, kind, confidence FROM edges")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Edge {
                    src: r.get(0)?,
                    dst: r.get(1)?,
                    kind: edge_from_str(&r.get::<_, String>(2)?),
                    confidence: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn all_node_ids(&self) -> Result<Vec<i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM nodes")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn stats(&self) -> Result<(usize, usize, usize)> {
        let conn = self.conn.lock().unwrap();
        let nodes: i64 = conn.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?;
        let edges: i64 = conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
        let files: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        Ok((nodes as usize, edges as usize, files as usize))
    }
}

fn row_to_node(r: &rusqlite::Row<'_>) -> rusqlite::Result<Node> {
    Ok(Node {
        id: r.get(0)?,
        kind: kind_from_str(&r.get::<_, String>(1)?),
        name: r.get(2)?,
        qualified: r.get(3)?,
        path: r.get(4)?,
        language: lang_from_str(&r.get::<_, String>(5)?),
        start_line: r.get(6)?,
        end_line: r.get(7)?,
        signature: r.get(8)?,
        body_bytes: r.get(9)?,
    })
}

// --- small string codecs (sqlite stores text, not enums) ---------------

fn kind_to_str(k: NodeKind) -> &'static str {
    match k {
        NodeKind::File => "file",
        NodeKind::Function => "function",
        NodeKind::Method => "method",
        NodeKind::Class => "class",
        NodeKind::Import => "import",
    }
}
fn kind_from_str(s: &str) -> NodeKind {
    match s {
        "file" => NodeKind::File,
        "function" => NodeKind::Function,
        "method" => NodeKind::Method,
        "class" => NodeKind::Class,
        _ => NodeKind::Import,
    }
}
fn edge_to_str(k: EdgeKind) -> &'static str {
    match k {
        EdgeKind::Contains => "contains",
        EdgeKind::Calls => "calls",
        EdgeKind::Imports => "imports",
        EdgeKind::Inherits => "inherits",
        EdgeKind::Tests => "tests",
    }
}
fn edge_from_str(s: &str) -> EdgeKind {
    match s {
        "contains" => EdgeKind::Contains,
        "calls" => EdgeKind::Calls,
        "imports" => EdgeKind::Imports,
        "inherits" => EdgeKind::Inherits,
        _ => EdgeKind::Tests,
    }
}
fn lang_to_str(l: Language) -> &'static str {
    match l {
        Language::Rust => "rust",
        Language::TypeScript => "typescript",
        Language::JavaScript => "javascript",
        Language::Python => "python",
        Language::Json => "json",
        Language::Markdown => "markdown",
        Language::Unknown => "unknown",
    }
}
fn lang_from_str(s: &str) -> Language {
    match s {
        "rust" => Language::Rust,
        "typescript" => Language::TypeScript,
        "javascript" => Language::JavaScript,
        "python" => Language::Python,
        "json" => Language::Json,
        "markdown" => Language::Markdown,
        _ => Language::Unknown,
    }
}
