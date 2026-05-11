//! `contextos install` — auto-configure Claude Code for this project.
//!
//! Writes (or merges into) `.mcp.json` in the project root so Claude Code
//! picks up ContextOS on next session load. Also writes a project-scoped
//! `.claude/settings.local.json` entry that opts into the new server for
//! users who have `enabledMcpjsonServers` gating on.
//!
//! Fully idempotent. If our entry is already correct, the write is a no-op.

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// Where the ContextOS binary lives. We resolve it as:
///   1. Canonical path of the current process (`std::env::current_exe`).
///   2. Failing that (in tests), the literal string `"contextos"` — lets the
///      user's PATH handle it.
fn binary_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "contextos".to_string())
}

#[derive(Debug, Clone)]
pub struct InstallReport {
    pub wrote_mcp_json: bool,
    pub wrote_settings_local: bool,
    pub mcp_json_path: PathBuf,
    pub settings_path: PathBuf,
    pub already_configured: bool,
}

pub fn install(root: &Path) -> Result<InstallReport> {
    let abs_root = root
        .canonicalize()
        .with_context(|| format!("resolving {}", root.display()))?;

    let bin = binary_path();
    let mcp_json_path = abs_root.join(".mcp.json");
    let settings_path = abs_root.join(".claude").join("settings.local.json");

    let (wrote_mcp, already_mcp) = upsert_mcp_json(&mcp_json_path, &bin, &abs_root)?;
    let (wrote_settings, already_settings) = upsert_settings_local(&settings_path)?;

    Ok(InstallReport {
        wrote_mcp_json: wrote_mcp,
        wrote_settings_local: wrote_settings,
        mcp_json_path,
        settings_path,
        already_configured: already_mcp && already_settings,
    })
}

/// Make sure the per-project ContextOS state files don't accidentally get
/// committed. We append to (or create) `<root>/.gitignore`, adding only the
/// entries that aren't already present. Each line is anchored with `/` so
/// it matches the repo root only — files with the same name deeper in the
/// tree (unlikely, but possible) won't be hidden by accident.
///
/// This is idempotent: running `contextos init` twice produces no
/// duplicates. Returns `Ok(true)` if any line was added, `Ok(false)` if
/// the .gitignore was already up to date.
pub fn ensure_gitignore(root: &Path) -> Result<bool> {
    let abs_root = root
        .canonicalize()
        .with_context(|| format!("resolving {}", root.display()))?;
    let path = abs_root.join(".gitignore");

    // Lines we want present. The trailing slash on the directory entries
    // makes git treat them as directories and not match same-named files.
    let want: &[&str] = &[
        "/.mcp.json",
        "/.claude/",
        "/.contextos/",
    ];

    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let already: std::collections::HashSet<&str> = existing
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    let mut to_add: Vec<&str> = Vec::new();
    for entry in want {
        // Match either the anchored form or the bare-name form a user
        // might already have written (e.g. `.contextos/`, `.mcp.json`).
        let bare = entry.trim_start_matches('/');
        if !already.contains(entry) && !already.contains(bare) {
            to_add.push(entry);
        }
    }
    if to_add.is_empty() {
        return Ok(false);
    }

    let mut out = existing.clone();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str("# ContextOS — per-project state, regenerated on demand.\n");
    for entry in to_add {
        out.push_str(entry);
        out.push('\n');
    }
    std::fs::write(&path, out)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

/// Install user-scoped Claude Code slash commands at
/// `~/.claude/commands/`. These give the user direct access to
/// ContextOS features from inside Claude Code via `/savings` etc.
///
/// User-scoped (not project-scoped) so the commands are available
/// in every workspace once installed, not per-project. Idempotent:
/// if the command file already exists with our content, we skip it.
/// We never overwrite a file the user has customised — if the
/// existing file's first line doesn't match our magic marker, we
/// leave it alone.
pub fn ensure_slash_commands() -> Result<bool> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| anyhow::anyhow!("can't resolve $HOME for slash commands"))?;
    let dir = std::path::PathBuf::from(home).join(".claude").join("commands");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating {}", dir.display()))?;

    let savings = dir.join("savings.md");
    let body = SAVINGS_SLASH_COMMAND;

    if let Ok(existing) = std::fs::read_to_string(&savings) {
        if existing == body {
            return Ok(false);
        }
        // Detect prior versions of our own file (any of the known
        // markers) so we can upgrade them. If the marker isn't there
        // anywhere, the user has customised the file and we leave it
        // alone.
        let is_ours = KNOWN_MARKERS.iter().any(|m| existing.contains(m));
        if !is_ours {
            return Ok(false);
        }
    }
    std::fs::write(&savings, body)
        .with_context(|| format!("writing {}", savings.display()))?;
    Ok(true)
}

/// Markers we'll recognise as "this file was previously written by the
/// ContextOS installer". The current version's marker plus any earlier
/// versions whose layout we want to upgrade in place. Keep the list
/// growing; never remove an entry.
const KNOWN_MARKERS: &[&str] = &[
    "contextos:slash-command:savings v1",
];

/// Append (or refresh) a ContextOS guidance block in the project's
/// `CLAUDE.md`. This is the soft enforcement layer — Claude reads
/// `CLAUDE.md` at session start and uses it as durable instruction.
/// The block tells Claude when to prefer the ContextOS MCP tools
/// (`skeleton`, `cx_pack_files`) over raw `Read` calls so token
/// savings actually accrue during normal use.
///
/// Idempotent: the block is fenced with HTML markers, so on re-run we
/// either skip (already current) or replace in place. Never touches
/// the user's own content outside the fence.
pub fn ensure_claude_md(root: &Path) -> Result<bool> {
    let abs_root = root
        .canonicalize()
        .with_context(|| format!("resolving {}", root.display()))?;
    let path = abs_root.join("CLAUDE.md");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let block = CONTEXTOS_CLAUDE_MD_BLOCK;

    // If our current block is already verbatim in the file, no-op.
    if existing.contains(block) {
        return Ok(false);
    }

    // If an older version of our block exists (any line between the
    // OPEN and CLOSE markers), replace it. Otherwise append.
    let new_text = if let (Some(start), Some(end)) = (
        existing.find(CLAUDE_MD_OPEN_MARKER),
        existing.find(CLAUDE_MD_CLOSE_MARKER),
    ) {
        let end_full = end + CLAUDE_MD_CLOSE_MARKER.len();
        if end_full < existing.len() {
            let mut s = String::with_capacity(existing.len() + block.len());
            s.push_str(&existing[..start]);
            s.push_str(block);
            s.push_str(&existing[end_full..]);
            s
        } else {
            let mut s = String::with_capacity(existing.len() + block.len());
            s.push_str(&existing[..start]);
            s.push_str(block);
            s
        }
    } else {
        let mut s = existing.clone();
        if !s.is_empty() && !s.ends_with('\n') {
            s.push('\n');
        }
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(block);
        s
    };

    std::fs::write(&path, new_text)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

const CLAUDE_MD_OPEN_MARKER: &str = "<!-- contextos:claude-md:begin -->";
const CLAUDE_MD_CLOSE_MARKER: &str = "<!-- contextos:claude-md:end -->";

/// Strict-mode enforcement: register a `PreToolUse` hook in
/// `.claude/settings.json` that intercepts every `Read` call. The hook
/// invokes `contextos hook pre-read` with the tool input on stdin; if
/// the file is large enough to be wasteful the hook responds with a
/// `decision: block` and a reason instructing Claude to use
/// `mcp__contextos__cx_pack_files` or `skeleton` instead.
///
/// Idempotent: re-running with the same binary path is a no-op. If the
/// settings file exists with other entries, ours is merged in without
/// touching unrelated keys.
pub fn ensure_strict_hook(root: &Path) -> Result<bool> {
    let abs_root = root
        .canonicalize()
        .with_context(|| format!("resolving {}", root.display()))?;
    let path = abs_root.join(".claude").join("settings.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let bin = binary_path();
    let command = format!("{bin} hook pre-read");

    let mut doc = read_json_or_default(&path)?;
    let obj = doc
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{} is not a JSON object", path.display()))?;

    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json `hooks` is not an object"))?;

    let pretool = hooks
        .entry("PreToolUse")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json `hooks.PreToolUse` is not an array"))?;

    // Look for an existing entry that matches `Read` and contains our
    // hook command. If found, we're already configured.
    let mut already = false;
    for entry in pretool.iter() {
        if entry.get("matcher").and_then(Value::as_str) == Some("Read") {
            if let Some(arr) = entry.get("hooks").and_then(Value::as_array) {
                for h in arr {
                    if h.get("command").and_then(Value::as_str) == Some(&command) {
                        already = true;
                    }
                }
            }
        }
    }
    if already {
        return Ok(false);
    }

    pretool.push(json!({
        "matcher": "Read",
        "hooks": [
            {
                "type": "command",
                "command": command,
            }
        ]
    }));

    let pretty = serde_json::to_string_pretty(&doc)?;
    std::fs::write(&path, pretty)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

/// The CLAUDE.md fragment installed (or refreshed) by `ensure_claude_md`.
/// Bump the version suffix in the markers if the body changes materially
/// so existing installs get upgraded in place rather than appended.
const CONTEXTOS_CLAUDE_MD_BLOCK: &str = r#"<!-- contextos:claude-md:begin -->
## ContextOS — token-saving tools

This project has ContextOS wired up via MCP. Prefer the following tools
over raw file reads when they apply — they return the same information
in far fewer tokens, and the savings accrue to `/savings`.

- **`mcp__contextos__skeleton`** — signature-only view of one file
  (function and class declarations, no bodies). Use when you need to
  understand a file's *structure* but not its full implementation, e.g.
  before deciding which functions to dive into. Saves ~70-90% tokens
  vs `Read` on a typical source file.
- **`mcp__contextos__cx_pack_files`** — read N related files at once,
  ranked and budget-trimmed against a `query`. Use when you need
  context across 3+ files for a task (e.g. "how is auth wired across
  the API and middleware?") instead of issuing separate `Read` calls.
  Saves ~30-60% tokens.
- **`mcp__contextos__optimize`** — when you've already gathered code
  chunks (from grep, search, or prior reads) and want them deduped,
  ranked, and packed to a token budget before quoting them back.

Continue using `Read` directly when:
- The file is small (≤100 lines).
- You need exact, unmodified bytes (e.g. before an `Edit`).
- You need a specific narrow region you can address with `offset` /
  `limit`.

Run `/savings` at any time to see how much these tools have saved.
<!-- contextos:claude-md:end -->
"#;

/// Slash command body. The YAML frontmatter MUST be at the very top of
/// the file — Claude Code's parser scans for an opening `---` on line 1
/// and won't recognise the file as a slash command otherwise. The
/// installer marker therefore lives at the bottom as an HTML comment.
const SAVINGS_SLASH_COMMAND: &str = r#"---
description: Show the ContextOS token-savings dashboard for this session
---

Call the `savings` tool from the `contextos` MCP server with `scope: "global"`. The tool returns a Markdown dashboard with cumulative token reductions, exec time, and a per-query breakdown table. Display the dashboard exactly as returned, then add a one-line takeaway at the end summarising the headline reduction percentage and total tokens saved.

If the `savings` tool is not available, the ContextOS MCP server isn't wired in this workspace. Tell the user to run `contextos init` from this project's root directory and reopen Claude Code.

<!-- contextos:slash-command:savings v1 -->
"#;

pub fn uninstall(root: &Path) -> Result<()> {
    let abs_root = root
        .canonicalize()
        .with_context(|| format!("resolving {}", root.display()))?;
    let mcp_json_path = abs_root.join(".mcp.json");
    let settings_path = abs_root.join(".claude").join("settings.local.json");

    remove_from_mcp_json(&mcp_json_path)?;
    remove_from_settings_local(&settings_path)?;
    Ok(())
}

// ---- .mcp.json ----------------------------------------------------------

fn upsert_mcp_json(path: &Path, bin: &str, root: &Path) -> Result<(bool, bool)> {
    let desired = json!({
        "type": "stdio",
        "command": bin,
        "args": ["serve", "--root", root.to_string_lossy()]
    });

    let mut doc = read_json_or_default(path)?;
    let servers = ensure_object_key(&mut doc, "mcpServers");

    let already = servers
        .get("contextos")
        .map(|v| v == &desired)
        .unwrap_or(false);
    if already {
        return Ok((false, true));
    }

    servers.insert("contextos".to_string(), desired);
    write_json(path, &doc)?;
    Ok((true, false))
}

fn remove_from_mcp_json(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut doc = read_json_or_default(path)?;
    if let Some(servers) = doc
        .as_object_mut()
        .and_then(|o| o.get_mut("mcpServers"))
        .and_then(|v| v.as_object_mut())
    {
        servers.remove("contextos");
        if servers.is_empty() {
            if let Some(obj) = doc.as_object_mut() {
                obj.remove("mcpServers");
            }
        }
    }
    if doc.as_object().map(|o| o.is_empty()).unwrap_or(false) {
        std::fs::remove_file(path).ok();
    } else {
        write_json(path, &doc)?;
    }
    Ok(())
}

// ---- .claude/settings.local.json ---------------------------------------

fn upsert_settings_local(path: &Path) -> Result<(bool, bool)> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut doc = read_json_or_default(path)?;

    // Ensure `enabledMcpjsonServers` contains "contextos" (safe even if the
    // field doesn't exist yet; Claude Code simply picks it up).
    let list = doc
        .as_object_mut()
        .unwrap()
        .entry("enabledMcpjsonServers")
        .or_insert_with(|| Value::Array(Vec::new()));
    let arr = list.as_array_mut().context("enabledMcpjsonServers must be array")?;

    let already = arr.iter().any(|v| v.as_str() == Some("contextos"));
    if already {
        return Ok((false, true));
    }
    arr.push(Value::String("contextos".into()));
    write_json(path, &doc)?;
    Ok((true, false))
}

fn remove_from_settings_local(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut doc = read_json_or_default(path)?;
    if let Some(arr) = doc
        .as_object_mut()
        .and_then(|o| o.get_mut("enabledMcpjsonServers"))
        .and_then(|v| v.as_array_mut())
    {
        arr.retain(|v| v.as_str() != Some("contextos"));
        if arr.is_empty() {
            if let Some(obj) = doc.as_object_mut() {
                obj.remove("enabledMcpjsonServers");
            }
        }
    }
    if doc.as_object().map(|o| o.is_empty()).unwrap_or(false) {
        std::fs::remove_file(path).ok();
    } else {
        write_json(path, &doc)?;
    }
    Ok(())
}

// ---- helpers ----------------------------------------------------------

fn read_json_or_default(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_str(trimmed)
        .with_context(|| format!("parsing existing JSON at {}", path.display()))
}

fn ensure_object_key<'a>(doc: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    let obj = doc.as_object_mut().expect("top-level must be object");
    obj.entry(key)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .expect("ensure_object_key: value at key is not an object")
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let text = serde_json::to_string_pretty(value)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, format!("{text}\n"))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn install_creates_mcp_json_and_settings() {
        let tmp = setup();
        let report = install(tmp.path()).unwrap();
        assert!(report.wrote_mcp_json);
        assert!(report.wrote_settings_local);
        assert!(tmp.path().join(".mcp.json").exists());
        assert!(tmp
            .path()
            .join(".claude/settings.local.json")
            .exists());
    }

    #[test]
    fn install_is_idempotent() {
        let tmp = setup();
        let _ = install(tmp.path()).unwrap();
        let report = install(tmp.path()).unwrap();
        assert!(!report.wrote_mcp_json);
        assert!(!report.wrote_settings_local);
        assert!(report.already_configured);
    }

    #[test]
    fn install_merges_into_existing_config() {
        let tmp = setup();
        let mcp_path = tmp.path().join(".mcp.json");
        std::fs::write(
            &mcp_path,
            r#"{"mcpServers": {"other": {"command": "foo"}}}"#,
        )
        .unwrap();
        install(tmp.path()).unwrap();
        let doc: Value =
            serde_json::from_str(&std::fs::read_to_string(&mcp_path).unwrap()).unwrap();
        assert!(doc["mcpServers"]["other"].is_object());
        assert!(doc["mcpServers"]["contextos"].is_object());
    }

    #[test]
    fn uninstall_removes_only_our_entry() {
        let tmp = setup();
        let mcp_path = tmp.path().join(".mcp.json");
        std::fs::write(
            &mcp_path,
            r#"{"mcpServers": {"other": {"command": "foo"}}}"#,
        )
        .unwrap();
        install(tmp.path()).unwrap();
        uninstall(tmp.path()).unwrap();
        let doc: Value =
            serde_json::from_str(&std::fs::read_to_string(&mcp_path).unwrap()).unwrap();
        assert!(doc["mcpServers"]["other"].is_object());
        assert!(doc["mcpServers"].get("contextos").is_none());
    }

    #[test]
    fn claude_md_appended_when_missing() {
        let tmp = setup();
        let path = tmp.path().join("CLAUDE.md");
        // No CLAUDE.md yet — install should create one with our block.
        let wrote = ensure_claude_md(tmp.path()).unwrap();
        assert!(wrote);
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains(CLAUDE_MD_OPEN_MARKER));
        assert!(body.contains("cx_pack_files"));
        assert!(body.contains(CLAUDE_MD_CLOSE_MARKER));
    }

    #[test]
    fn claude_md_idempotent_second_run() {
        let tmp = setup();
        ensure_claude_md(tmp.path()).unwrap();
        let after_first = std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
        let wrote = ensure_claude_md(tmp.path()).unwrap();
        assert!(!wrote, "second run should be a no-op");
        let after_second = std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
        assert_eq!(after_first, after_second);
    }

    #[test]
    fn claude_md_preserves_user_content() {
        let tmp = setup();
        let path = tmp.path().join("CLAUDE.md");
        std::fs::write(&path, "# My project\n\nHand-written notes.\n").unwrap();
        ensure_claude_md(tmp.path()).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("# My project"));
        assert!(body.contains("Hand-written notes."));
        assert!(body.contains(CLAUDE_MD_OPEN_MARKER));
    }

    #[test]
    fn strict_hook_added_to_settings() {
        let tmp = setup();
        let added = ensure_strict_hook(tmp.path()).unwrap();
        assert!(added);
        let settings: Value = serde_json::from_str(
            &std::fs::read_to_string(tmp.path().join(".claude").join("settings.json")).unwrap(),
        )
        .unwrap();
        let pretool = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pretool.len(), 1);
        assert_eq!(pretool[0]["matcher"], "Read");
        let cmd = pretool[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.ends_with("hook pre-read"));
    }

    #[test]
    fn strict_hook_idempotent() {
        let tmp = setup();
        assert!(ensure_strict_hook(tmp.path()).unwrap());
        assert!(!ensure_strict_hook(tmp.path()).unwrap());
        let settings: Value = serde_json::from_str(
            &std::fs::read_to_string(tmp.path().join(".claude").join("settings.json")).unwrap(),
        )
        .unwrap();
        // Should still have exactly one Read entry.
        let pretool = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pretool.len(), 1);
    }

    #[test]
    fn strict_hook_preserves_unrelated_settings() {
        let tmp = setup();
        let path = tmp.path().join(".claude").join("settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"permissions":{"allow":["Bash"]},"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"echo hi"}]}]}}"#,
        )
        .unwrap();
        ensure_strict_hook(tmp.path()).unwrap();
        let settings: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // Unrelated permissions + the user's existing Bash hook are intact.
        assert_eq!(settings["permissions"]["allow"][0], "Bash");
        let pretool = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pretool.len(), 2);
        let matchers: Vec<&str> = pretool
            .iter()
            .filter_map(|e| e["matcher"].as_str())
            .collect();
        assert!(matchers.contains(&"Bash"));
        assert!(matchers.contains(&"Read"));
    }
}
