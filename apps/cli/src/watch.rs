//! `contextos watch` — long-running graph updater.
//!
//! Uses the `notify` crate to listen for filesystem events under `root`,
//! debounces (200ms), and hands the touched paths to `GraphBuilder::update`.
//! Honours .gitignore via a quick walk filter.

use anyhow::Result;
use contextos_graph::Graph;
use crossbeam_channel::{after, select, unbounded};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn run(root: &Path) -> Result<()> {
    let graph = Graph::open(root)?;
    let (tx, rx) = unbounded::<notify::Result<notify::Event>>();
    let mut watcher: RecommendedWatcher =
        notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })?;
    watcher.watch(root, RecursiveMode::Recursive)?;

    eprintln!("contextos watch: monitoring {}", root.display());

    let mut pending: HashSet<PathBuf> = HashSet::new();
    let debounce = Duration::from_millis(200);

    loop {
        select! {
            recv(rx) -> msg => {
                if let Ok(Ok(evt)) = msg {
                    if matches!(
                        evt.kind,
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                    ) {
                        for path in evt.paths {
                            if skip(&path) { continue; }
                            pending.insert(path);
                        }
                    }
                }
            }
            recv(after(debounce)) -> _ => {
                if pending.is_empty() { continue; }
                let batch: Vec<PathBuf> = pending.drain().collect();
                let n = batch.len();
                match graph.builder().update(&batch) {
                    Ok(r) => eprintln!(
                        "watch: {} events → reparsed={} skipped={} nodes+={} edges+={}",
                        n, r.files_reparsed, r.files_skipped, r.nodes_written, r.edges_written
                    ),
                    Err(e) => eprintln!("watch error: {e}"),
                }
            }
        }
    }
}

fn skip(p: &Path) -> bool {
    let s = p.to_string_lossy();
    s.contains("/.contextos/")
        || s.contains("/.git/")
        || s.contains("/node_modules/")
        || s.contains("/target/")
        || s.ends_with(".swp")
        || s.ends_with("~")
}
