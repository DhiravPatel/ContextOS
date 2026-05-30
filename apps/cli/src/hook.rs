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

/// Threshold above which a multiline `Grep` with `output_mode=content`
/// (the verbose mode that prints whole matching lines) is blocked and
/// the model is told to either narrow the pattern, add `-A/-B/-C`
/// limits, or switch to `files_with_matches` mode. Empirically this is
/// where Grep results start dominating context windows.
const LARGE_GREP_MATCH_LINES: usize = 200;

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

/// Block oversized `Grep` calls so the model doesn't accidentally pull
/// hundreds of matching lines into the context. Allows:
///   * `output_mode=files_with_matches` (cheap — just paths)
///   * `output_mode=count` (cheap — just numbers)
///   * `head_limit` set to something reasonable (≤ LARGE_GREP_MATCH_LINES)
///   * a `path` argument that narrows the scope to one file (already
///     small)
/// Blocks the rest with a reason that tells Claude how to retry.
pub fn handle_pre_grep(stdin_json: &str) -> Value {
    let v: Value = match serde_json::from_str(stdin_json) {
        Ok(v) => v,
        Err(_) => return json!({}),
    };

    let tool = v.get("tool_name").and_then(Value::as_str).unwrap_or("");
    if tool != "Grep" {
        return json!({});
    }

    let input = match v.get("tool_input") {
        Some(i) => i,
        None => return json!({}),
    };

    // Cheap output modes never produce a flood — always allow.
    let mode = input
        .get("output_mode")
        .and_then(Value::as_str)
        .unwrap_or("files_with_matches");
    if mode == "files_with_matches" || mode == "count" {
        return json!({});
    }

    // Caller already bounded the result with `head_limit`.
    if let Some(h) = input.get("head_limit").and_then(Value::as_u64) {
        if (h as usize) <= LARGE_GREP_MATCH_LINES {
            return json!({});
        }
    }

    // Single-file Grep is bounded by file size; the pre-read hook
    // already catches the egregious cases.
    if input
        .get("path")
        .and_then(Value::as_str)
        .map(|p| !p.is_empty() && !p.ends_with('/'))
        .unwrap_or(false)
    {
        return json!({});
    }

    let pattern = input
        .get("pattern")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>");
    let reason = format!(
        "ContextOS strict mode: a repo-wide Grep with `output_mode=content` for pattern `{pattern}` is unbounded — it can return thousands of lines and burn the context window. Retry with one of:\n\
         • `output_mode=files_with_matches` to get just the file list, then `Read` (or `mcp__contextos__cx_pack_files`) the most relevant ones.\n\
         • `output_mode=content` plus `head_limit: {LARGE_GREP_MATCH_LINES}` (or smaller) to cap the result.\n\
         • Narrow with `path: \"some/dir\"` or `glob: \"*.ts\"`.\n\
         • For semantic search across multiple files, use `mcp__contextos__cx_pack_files` with `paths` + a `query`."
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

    // ---------- pre-grep tests ----------

    #[test]
    fn grep_allows_files_with_matches() {
        let out = handle_pre_grep(
            r#"{"tool_name":"Grep","tool_input":{"pattern":"foo","output_mode":"files_with_matches"}}"#,
        );
        assert!(out.as_object().unwrap().is_empty());
    }

    #[test]
    fn grep_allows_count_mode() {
        let out = handle_pre_grep(
            r#"{"tool_name":"Grep","tool_input":{"pattern":"foo","output_mode":"count"}}"#,
        );
        assert!(out.as_object().unwrap().is_empty());
    }

    #[test]
    fn grep_allows_bounded_head_limit() {
        let out = handle_pre_grep(
            r#"{"tool_name":"Grep","tool_input":{"pattern":"foo","output_mode":"content","head_limit":50}}"#,
        );
        assert!(out.as_object().unwrap().is_empty());
    }

    #[test]
    fn grep_allows_single_file_search() {
        let out = handle_pre_grep(
            r#"{"tool_name":"Grep","tool_input":{"pattern":"foo","output_mode":"content","path":"src/main.rs"}}"#,
        );
        assert!(out.as_object().unwrap().is_empty());
    }

    #[test]
    fn grep_blocks_unbounded_content_mode() {
        let out = handle_pre_grep(
            r#"{"tool_name":"Grep","tool_input":{"pattern":"TODO","output_mode":"content"}}"#,
        );
        assert_eq!(out.get("decision").and_then(Value::as_str), Some("block"));
        let reason = out.get("reason").and_then(Value::as_str).unwrap();
        assert!(reason.contains("files_with_matches"));
        assert!(reason.contains("head_limit"));
    }

    #[test]
    fn grep_blocks_head_limit_too_large() {
        let out = handle_pre_grep(
            r#"{"tool_name":"Grep","tool_input":{"pattern":"foo","output_mode":"content","head_limit":5000}}"#,
        );
        assert_eq!(out.get("decision").and_then(Value::as_str), Some("block"));
    }

    #[test]
    fn grep_ignores_non_grep_tools() {
        let out = handle_pre_grep(
            r#"{"tool_name":"Read","tool_input":{"file_path":"/foo"}}"#,
        );
        assert!(out.as_object().unwrap().is_empty());
    }
}
