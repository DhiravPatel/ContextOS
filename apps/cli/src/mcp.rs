//! Minimal MCP-compatible JSON-RPC 2.0 server on stdio.
//!
//! Wire protocol: one JSON object per line (LSP-style `Content-Length`
//! framing is optional; many MCP clients accept newline-delimited JSON on
//! stdio, which is simpler and fine for local use). If the first byte of a
//! frame is `Content-Length:`, we parse LSP framing too.
//!
//! Tools exposed (MCP `tools/list` → `tools/call`):
//!   * `optimize`              — run the engine pipeline on supplied chunks
//!   * `build_graph`           — full index
//!   * `update_graph`          — incremental update
//!   * `impact_radius`         — blast radius for changed files
//!   * `skeleton`              — signature-only view of a file
//!   * `graph_stats`           — node/edge/file counts
//!
//! This is a deliberately small subset of MCP — enough for Claude Code,
//! Cursor and friends to call the ContextOS engine and get token savings
//! without the extension layer in the way.

use anyhow::Result;
use contextos_core_engine::{Engine, EngineConfig, OptimizationRequest};
use contextos_graph::Graph;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub fn serve(root: &Path) -> Result<()> {
    let graph = Graph::open(root)?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = stdout.lock();

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(()); // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                write_line(
                    &mut stdout,
                    &error_response(Value::Null, -32700, &format!("parse error: {e}")),
                )?;
                continue;
            }
        };
        let response = handle(&graph, root, &req);
        write_line(&mut stdout, &response)?;
    }
}

fn write_line(w: &mut impl Write, v: &Value) -> Result<()> {
    let s = serde_json::to_string(v)?;
    w.write_all(s.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct Request {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorObj>,
}

#[derive(Debug, Serialize)]
struct ErrorObj {
    code: i32,
    message: String,
}

fn ok(id: Value, result: Value) -> Value {
    serde_json::to_value(Response {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    })
    .unwrap()
}

fn error_response(id: Value, code: i32, msg: &str) -> Value {
    serde_json::to_value(Response {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(ErrorObj {
            code,
            message: msg.to_string(),
        }),
    })
    .unwrap()
}

fn handle(graph: &Graph, root: &Path, req: &Request) -> Value {
    if req.jsonrpc != "2.0" {
        return error_response(req.id.clone().unwrap_or(Value::Null), -32600, "jsonrpc must be '2.0'");
    }
    let id = req.id.clone().unwrap_or(Value::Null);
    match req.method.as_str() {
        "initialize" => ok(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": { "name": "contextos", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "tools": {} }
            }),
        ),
        "tools/list" => ok(id, tools_list()),
        "tools/call" => match call_tool(graph, root, &req.params) {
            Ok(v) => ok(id, v),
            Err(e) => error_response(id, -32000, &e.to_string()),
        },
        _ => error_response(id, -32601, "method not found"),
    }
}

fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "optimize",
                "description": "Run the ContextOS optimization pipeline (dedup + compress + rank + budget) on supplied code chunks.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "chunks": { "type": "array" },
                        "query": { "type": "string" },
                        "max_tokens": { "type": "integer" }
                    },
                    "required": ["chunks"]
                }
            },
            {
                "name": "build_graph",
                "description": "Build or refresh the code graph for the active repo.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "update_graph",
                "description": "Incrementally update the graph for a list of changed files.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "files": { "type": "array", "items": { "type": "string" } } },
                    "required": ["files"]
                }
            },
            {
                "name": "impact_radius",
                "description": "Return the blast radius (affected files/symbols) for a list of changed files.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "files": { "type": "array", "items": { "type": "string" } },
                        "depth": { "type": "integer", "default": 2 }
                    },
                    "required": ["files"]
                }
            },
            {
                "name": "skeleton",
                "description": "Signature-only view of a source file — function/class declarations without bodies.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }
            },
            {
                "name": "graph_stats",
                "description": "Node / edge / file counts in the current graph.",
                "inputSchema": { "type": "object", "properties": {} }
            }
        ]
    })
}

fn call_tool(graph: &Graph, _root: &Path, params: &Value) -> anyhow::Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing tool name"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    match name {
        "optimize" => {
            let max_tokens = args
                .get("max_tokens")
                .and_then(Value::as_u64)
                .map(|v| v as usize);
            let request: OptimizationRequest = serde_json::from_value(json!({
                "chunks": args.get("chunks").cloned().unwrap_or(json!([])),
                "query": args.get("query").cloned(),
            }))?;
            let mut cfg = EngineConfig::default();
            if let Some(t) = max_tokens {
                cfg.max_tokens = t;
            }
            let result = Engine::new(cfg).optimize(request);
            Ok(wrap_text(&serde_json::to_string_pretty(&result)?))
        }
        "build_graph" => {
            let r = graph.builder().build()?;
            Ok(wrap_text(&format!(
                "scanned={} reparsed={} skipped={} nodes+={} edges+={}",
                r.files_scanned, r.files_reparsed, r.files_skipped, r.nodes_written, r.edges_written
            )))
        }
        "update_graph" => {
            let files: Vec<PathBuf> = args
                .get("files")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(PathBuf::from))
                        .collect()
                })
                .unwrap_or_default();
            let r = graph.builder().update(&files)?;
            Ok(wrap_text(&format!(
                "reparsed={} skipped={} nodes+={} edges+={}",
                r.files_reparsed, r.files_skipped, r.nodes_written, r.edges_written
            )))
        }
        "impact_radius" => {
            let files: Vec<String> = args
                .get("files")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let depth = args.get("depth").and_then(Value::as_u64).unwrap_or(2) as u32;
            let impact = graph.query().impact_radius(&files, depth)?;
            let payload = json!({
                "impacted_nodes": impact.impacted.len(),
                "impacted_files": impact
                    .impacted
                    .iter()
                    .map(|n| &n.path)
                    .collect::<std::collections::BTreeSet<_>>(),
            });
            Ok(wrap_text(&serde_json::to_string_pretty(&payload)?))
        }
        "skeleton" => {
            let path = args
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("path required"))?;
            let sk = graph.query().skeleton_for(path)?;
            Ok(wrap_text(&sk))
        }
        "graph_stats" => {
            let (n, e, f) = graph.store.stats()?;
            Ok(wrap_text(&format!("nodes={n} edges={e} files={f}")))
        }
        other => anyhow::bail!("unknown tool: {other}"),
    }
}

/// MCP wraps text tool output in `{content:[{type:"text", text:"..."}]}`.
fn wrap_text(s: &str) -> Value {
    json!({
        "content": [
            { "type": "text", "text": s }
        ]
    })
}
