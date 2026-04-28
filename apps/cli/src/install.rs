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
}
