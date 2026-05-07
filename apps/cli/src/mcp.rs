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
            },
            {
                "name": "savings",
                "description": "Show the cumulative ContextOS token-savings dashboard for this session: total tokens saved, average reduction %, per-query breakdown. Reads from the local usage log written by every optimize call.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "scope": { "type": "string", "enum": ["global", "project"], "default": "global" },
                        "top": { "type": "integer", "default": 10 }
                    }
                }
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
            let chunks_in = request.chunks.len();
            let query_for_log = request.query.clone();
            let mut cfg = EngineConfig::default();
            if let Some(t) = max_tokens {
                cfg.max_tokens = t;
            }
            let result = Engine::new(cfg).optimize(request);
            contextos_utils::record_usage(contextos_utils::UsageRecord {
                ts: 0,
                in_tokens: result.original_tokens,
                out_tokens: result.final_tokens,
                saved_tokens: result.tokens_saved,
                elapsed_ms: result.elapsed_ms,
                query: query_for_log,
                chunks_in,
                chunks_out: result.chunks.len(),
                source: "mcp".into(),
                project: Some(graph.root.to_string_lossy().into_owned()),
            });
            // Prepend a one-line human-readable summary so Claude Code's
            // tool-output panel surfaces savings without the user having
            // to ask. The full result JSON follows for the LLM to
            // consume.
            let summary = format!(
                "ContextOS: {} → {} tokens (−{:.1}%, saved {} in {:.0}ms)\n\n",
                humanize(result.original_tokens),
                humanize(result.final_tokens),
                result.reduction_pct,
                humanize(result.tokens_saved),
                result.elapsed_ms,
            );
            let body = serde_json::to_string_pretty(&result)?;
            Ok(wrap_text(&format!("{summary}{body}")))
        }
        "savings" => {
            let scope = args
                .get("scope")
                .and_then(Value::as_str)
                .unwrap_or("global");
            let top = args
                .get("top")
                .and_then(Value::as_u64)
                .map(|v| v as usize)
                .unwrap_or(10);
            let project = if scope == "project" {
                Some(graph.root.to_string_lossy().into_owned())
            } else {
                None
            };
            Ok(wrap_text(&render_savings(&graph.root, project.as_deref(), top)))
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

/// Compact number formatter: 1234 → "1.2K", 1500000 → "1.5M".
fn humanize(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Render the savings dashboard as Markdown styled to mimic Claude
/// Code's built-in "Account & Usage" modal: ACCOUNT and USAGE sections,
/// progress bars made from `█`/`░`, and a top-commands table. The
/// chat panel renders this inline; we can't trigger a real modal —
/// MCP / slash commands have no UI surface beyond returning text.
fn render_savings(_project_root: &Path, project_filter: Option<&str>, top: usize) -> String {
    let records = contextos_utils::read_usage();
    let filtered: Vec<&contextos_utils::UsageRecord> = records
        .iter()
        .filter(|r| match (project_filter, &r.project) {
            (Some(want), Some(got)) => got == want,
            (Some(_), None) => false,
            _ => true,
        })
        .collect();

    let scope_label = if project_filter.is_some() {
        "Project Scope"
    } else {
        "Global Scope"
    };

    use std::fmt::Write;
    let mut out = String::new();
    writeln!(out, "## ⚡ ContextOS — Token Savings  ·  *{scope_label}*").ok();
    writeln!(out).ok();

    // USAGE section.
    writeln!(out, "### USAGE").ok();
    writeln!(out).ok();

    if filtered.is_empty() {
        writeln!(
            out,
            "_No usage data yet. Run a few `optimize` calls (or use ContextOS via Claude Code MCP), then check back._"
        )
        .ok();
        return out;
    }

    let total_count = filtered.len();
    let total_in: usize = filtered.iter().map(|r| r.in_tokens).sum();
    let total_out: usize = filtered.iter().map(|r| r.out_tokens).sum();
    let total_saved: usize = filtered.iter().map(|r| r.saved_tokens).sum();
    let total_elapsed_ms: f64 = filtered.iter().map(|r| r.elapsed_ms).sum();
    let avg_elapsed_ms = total_elapsed_ms / total_count as f64;
    let aggregate_pct = if total_in == 0 {
        0.0
    } else {
        (total_saved as f64 / total_in as f64) * 100.0
    };
    // Per-call mean reduction: average of per-call percentages, not
    // weighted by call size. Useful when one big call would otherwise
    // dominate the aggregate number.
    let avg_call_pct: f64 = if filtered.is_empty() {
        0.0
    } else {
        let s: f64 = filtered
            .iter()
            .map(|r| {
                if r.in_tokens == 0 {
                    0.0
                } else {
                    (r.saved_tokens as f64 / r.in_tokens as f64) * 100.0
                }
            })
            .sum();
        s / filtered.len() as f64
    };

    writeln!(
        out,
        "**Tokens saved**  ·  {} of {} input  ·  output {}",
        humanize(total_saved),
        humanize(total_in),
        humanize(total_out)
    )
    .ok();
    writeln!(out, "{}", progress_bar(aggregate_pct, 36)).ok();
    writeln!(out).ok();

    writeln!(out, "**Avg reduction per call**").ok();
    writeln!(out, "{}", progress_bar(avg_call_pct, 36)).ok();
    writeln!(out).ok();

    writeln!(
        out,
        "**Total exec time**  ·  {}  (avg {} per call · {} calls)",
        format_ms(total_elapsed_ms),
        format_ms(avg_elapsed_ms),
        total_count
    )
    .ok();
    writeln!(out).ok();

    // TOP COMMANDS section.
    writeln!(out, "### TOP COMMANDS").ok();
    writeln!(out).ok();
    writeln!(out, "| # | Command | Count | Saved | Avg % | Time |").ok();
    writeln!(out, "|---|---|---:|---:|---:|---:|").ok();

    use std::collections::HashMap;
    #[derive(Default)]
    struct Agg {
        count: usize,
        saved: usize,
        in_tokens: usize,
        elapsed_ms: f64,
    }
    let mut by_query: HashMap<String, Agg> = HashMap::new();
    for r in &filtered {
        let key = r
            .query
            .as_deref()
            .map(|q| q.trim().to_string())
            .filter(|q| !q.is_empty())
            .unwrap_or_else(|| "(no query)".into());
        let entry = by_query.entry(key).or_default();
        entry.count += 1;
        entry.saved += r.saved_tokens;
        entry.in_tokens += r.in_tokens;
        entry.elapsed_ms += r.elapsed_ms;
    }
    let mut rows: Vec<(String, Agg)> = by_query.into_iter().collect();
    rows.sort_by(|a, b| b.1.saved.cmp(&a.1.saved));

    for (i, (q, a)) in rows.iter().take(top).enumerate() {
        let avg = if a.in_tokens == 0 {
            0.0
        } else {
            (a.saved as f64 / a.in_tokens as f64) * 100.0
        };
        let cmd = if q.chars().count() > 36 {
            let cut: String = q.chars().take(35).collect();
            format!("{cut}…")
        } else {
            q.clone()
        };
        writeln!(
            out,
            "| {} | {} | {} | {} | {:.1}% | {} |",
            i + 1,
            cmd,
            a.count,
            humanize(a.saved),
            avg,
            format_ms(a.elapsed_ms)
        )
        .ok();
    }
    out
}

/// Unicode progress bar with trailing percent label. Width = visible
/// glyphs; the actual byte length is larger because each glyph is
/// multi-byte UTF-8. The result is wrapped in a code span so chat
/// renderers use a monospaced font (otherwise the proportional font
/// makes the bar uneven).
fn progress_bar(pct: f64, width: usize) -> String {
    let pct_clamped = pct.clamp(0.0, 100.0);
    let filled = ((pct_clamped / 100.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("`{}{}`  **{:.1}%**", "█".repeat(filled), "░".repeat(empty), pct_clamped)
}

fn format_ms(ms: f64) -> String {
    if ms >= 1_000.0 {
        format!("{:.1}s", ms / 1_000.0)
    } else {
        format!("{:.0}ms", ms)
    }
}
