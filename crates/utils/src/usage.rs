//! Usage telemetry — local-only, append-only JSONL log.
//!
//! Every successful `optimize` call appends one line to
//! `~/.contextos/usage.jsonl`. This drives the `contextos savings`
//! dashboard. We deliberately keep the file:
//!
//!   * **append-only JSONL** — easy to inspect with `cat` / `jq`,
//!     resilient to partial writes, no migrations.
//!   * **local-only** — never sent off the machine.
//!   * **content-free** — only token counts, the query string (which
//!     the user typed), and the project root. No source bytes.
//!
//! Disable globally with `CONTEXTOS_NO_USAGE=1`.
//!
//! Rotation: not implemented yet; the file grows linearly. A typical
//! event is ~150 bytes, so 100k optimisations is ~15 MB. We can add
//! periodic compaction later (keep last N days, summarise older
//! entries) if it becomes a problem.

use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Unix epoch seconds.
    pub ts: u64,
    /// Tokens in the original input.
    pub in_tokens: usize,
    /// Tokens in the optimised output.
    pub out_tokens: usize,
    /// Pre-computed saving (in_tokens - out_tokens). Stored explicitly
    /// so consumers don't recompute.
    pub saved_tokens: usize,
    /// Wall-clock time spent in `Engine::optimize`, milliseconds.
    pub elapsed_ms: f64,
    /// User's query, if any. Truncated to 200 chars.
    #[serde(default)]
    pub query: Option<String>,
    /// Number of input chunks (before pipeline).
    #[serde(default)]
    pub chunks_in: usize,
    /// Number of output chunks (after pipeline).
    #[serde(default)]
    pub chunks_out: usize,
    /// Origin: "cli", "mcp", "extension", etc. Helpful for aggregating
    /// by surface.
    #[serde(default)]
    pub source: String,
    /// Absolute path of the project root (whatever the caller said is
    /// "the project"). May be `None` for graph-free CLI invocations.
    #[serde(default)]
    pub project: Option<String>,
}

/// Default global log path: `~/.contextos/usage.jsonl`.
pub fn default_log_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let mut p = PathBuf::from(home);
    p.push(".contextos");
    p.push("usage.jsonl");
    Some(p)
}

/// Append a single record to the global usage log. Silently does
/// nothing if `CONTEXTOS_NO_USAGE=1` or the log path can't be resolved
/// (rare — we're not in someone's `$HOME`). I/O errors during write
/// are logged to stderr but not propagated; usage telemetry must never
/// break the actual pipeline.
pub fn record(mut rec: UsageRecord) {
    if std::env::var_os("CONTEXTOS_NO_USAGE").is_some() {
        return;
    }
    if rec.ts == 0 {
        rec.ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
    }
    if let Some(q) = rec.query.as_mut() {
        if q.len() > 200 {
            q.truncate(200);
        }
    }
    if let Err(e) = write_record(&rec) {
        eprintln!("contextos: usage log write failed: {e}");
    }
}

fn write_record(rec: &UsageRecord) -> std::io::Result<()> {
    let path = default_log_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no $HOME for usage log")
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    let line = serde_json::to_string(rec).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    f.write_all(line.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(())
}

/// Read every record from the default log, in chronological order
/// (which is the order they were appended). Malformed lines are
/// skipped silently — partial writes from a crashed process shouldn't
/// kill the dashboard.
pub fn read_all() -> Vec<UsageRecord> {
    read_from(default_log_path())
}

pub fn read_from(path: Option<PathBuf>) -> Vec<UsageRecord> {
    let path = match path {
        Some(p) => p,
        None => return Vec::new(),
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let text = match std::str::from_utf8(&bytes) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<UsageRecord>(l).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_roundtrips_via_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.jsonl");
        let r = UsageRecord {
            ts: 1234,
            in_tokens: 100,
            out_tokens: 30,
            saved_tokens: 70,
            elapsed_ms: 12.5,
            query: Some("auth".into()),
            chunks_in: 3,
            chunks_out: 2,
            source: "cli".into(),
            project: Some("/tmp/x".into()),
        };
        // Manual write to bypass HOME resolution
        std::fs::write(&path, format!("{}\n", serde_json::to_string(&r).unwrap())).unwrap();
        let back = read_from(Some(path));
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].saved_tokens, 70);
        assert_eq!(back[0].query.as_deref(), Some("auth"));
    }

    #[test]
    fn read_skips_malformed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.jsonl");
        let good = serde_json::to_string(&UsageRecord {
            ts: 1,
            in_tokens: 1,
            out_tokens: 1,
            saved_tokens: 0,
            elapsed_ms: 0.0,
            query: None,
            chunks_in: 0,
            chunks_out: 0,
            source: String::new(),
            project: None,
        })
        .unwrap();
        std::fs::write(&path, format!("{good}\n{{ not json\n{good}\n")).unwrap();
        let back = read_from(Some(path));
        assert_eq!(back.len(), 2);
    }
}
