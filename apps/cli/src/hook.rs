//! Claude Code hook handlers.
//!
//! Each function takes the raw JSON payload Claude Code writes to the
//! hook process's stdin and returns a JSON response. The response shape
//! follows Claude Code's hook contract:
//!
//!   * `{}` (empty)                         → allow (default).
//!   * `{"decision":"block","reason":"…"}`  → deny the tool call and
//!                                            surface the reason to
//!                                            Claude as additional
//!                                            context, so it can retry.
//!
//! Hooks are advisory: any error here is converted to "allow" so a
//! buggy ContextOS install can never wedge Claude Code's tool flow.

use serde_json::{json, Value};

/// Threshold above which we redirect `Read` to `mcp__contextos__cx_pack_files`
/// or `mcp__contextos__skeleton`. Files smaller than this are cheap
/// enough that the hook overhead isn't worth it.
const LARGE_FILE_LINES: usize = 300;

pub fn handle_pre_read(stdin_json: &str) -> Value {
    // Parse defensively — any malformed payload is allowed through.
    let v: Value = match serde_json::from_str(stdin_json) {
        Ok(v) => v,
        Err(_) => return json!({}),
    };

    // Only intervene on Read tool calls; everything else slips through.
    let tool = v
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("");
    if tool != "Read" {
        return json!({});
    }

    // Pull the file path the model is trying to read.
    let path = match v
        .get("tool_input")
        .and_then(|i| i.get("file_path"))
        .and_then(Value::as_str)
    {
        Some(p) => p,
        None => return json!({}),
    };

    // A hook event with explicit `offset`/`limit` means the model is
    // already paginating — let it through; pagination is itself a
    // token-saving strategy.
    if v.get("tool_input").and_then(|i| i.get("offset")).is_some()
        || v.get("tool_input").and_then(|i| i.get("limit")).is_some()
    {
        return json!({});
    }

    // Count lines without slurping the whole file into memory.
    let line_count = match count_lines(path) {
        Ok(n) => n,
        // If we can't read it (permissions, doesn't exist, binary,
        // etc.), let Read fail with its own error message.
        Err(_) => return json!({}),
    };
    if line_count <= LARGE_FILE_LINES {
        return json!({});
    }

    // Block with a reason that gives Claude a clear next step.
    let reason = format!(
        "ContextOS strict mode: {path} is {line_count} lines. Read the full file would burn tokens unnecessarily. Use one of:\n\
         • `mcp__contextos__skeleton` (path: {path}) — for an overview of what's in the file.\n\
         • `mcp__contextos__cx_pack_files` (paths: [{path}], query: <your task>) — for a budget-trimmed view ranked against your task.\n\
         If you genuinely need the exact bytes (e.g. before an Edit), call Read again with `offset` and `limit` to fetch only the region you need."
    );
    json!({
        "decision": "block",
        "reason": reason,
    })
}

fn count_lines(path: &str) -> std::io::Result<usize> {
    use std::io::BufRead;
    let f = std::fs::File::open(path)?;
    let r = std::io::BufReader::new(f);
    let mut n = 0usize;
    for line in r.lines() {
        line?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_non_read_tools() {
        let out = handle_pre_read(r#"{"tool_name":"Edit","tool_input":{"file_path":"/dev/null"}}"#);
        assert!(out.as_object().unwrap().is_empty());
    }

    #[test]
    fn allows_paginated_reads() {
        let out = handle_pre_read(
            r#"{"tool_name":"Read","tool_input":{"file_path":"/dev/null","offset":1,"limit":50}}"#,
        );
        assert!(out.as_object().unwrap().is_empty());
    }

    #[test]
    fn allows_small_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("small.txt");
        std::fs::write(&p, "hi\n".repeat(10)).unwrap();
        let payload = format!(
            r#"{{"tool_name":"Read","tool_input":{{"file_path":"{}"}}}}"#,
            p.display()
        );
        let out = handle_pre_read(&payload);
        assert!(out.as_object().unwrap().is_empty());
    }

    #[test]
    fn blocks_large_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.txt");
        std::fs::write(&p, "x\n".repeat(500)).unwrap();
        let payload = format!(
            r#"{{"tool_name":"Read","tool_input":{{"file_path":"{}"}}}}"#,
            p.display()
        );
        let out = handle_pre_read(&payload);
        assert_eq!(out.get("decision").and_then(Value::as_str), Some("block"));
        let reason = out.get("reason").and_then(Value::as_str).unwrap();
        assert!(reason.contains("cx_pack_files"));
    }

    #[test]
    fn malformed_json_is_passthrough() {
        let out = handle_pre_read("not json");
        assert!(out.as_object().unwrap().is_empty());
    }
}
