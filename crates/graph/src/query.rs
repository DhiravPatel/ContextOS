//! Read-side queries on top of the store.
//!
//! Blast radius, skeletons, and centrality-based picking. This is where the
//! core-engine goes to find "the minimal slice that matters."

use crate::pagerank;
use crate::store::GraphStore;
use crate::types::{EdgeKind, Node, NodeKind};
use ahash::{AHashMap, AHashSet};
use anyhow::Result;
use std::collections::VecDeque;

pub struct GraphQuery<'a> {
    store: &'a GraphStore,
}

#[derive(Debug, Clone)]
pub struct ImpactResult {
    pub seeds: Vec<i64>,
    pub impacted: Vec<Node>,
    pub depth_of: AHashMap<i64, u32>,
}

impl<'a> GraphQuery<'a> {
    pub fn new(store: &'a GraphStore) -> Self {
        Self { store }
    }

    /// Blast radius: starting from `seed_paths`, BFS over reverse-Call and
    /// reverse-Import edges (callers / importers of the changed symbols).
    pub fn impact_radius(
        &self,
        seed_paths: &[String],
        max_depth: u32,
    ) -> Result<ImpactResult> {
        // Seed = every node in the changed files.
        let mut seeds: Vec<i64> = Vec::new();
        for p in seed_paths {
            for n in self.store.nodes_in_file(p)? {
                seeds.push(n.id);
            }
        }
        let mut visited: AHashSet<i64> = seeds.iter().copied().collect();
        let mut depth_of: AHashMap<i64, u32> =
            seeds.iter().map(|id| (*id, 0)).collect();
        let mut queue: VecDeque<(i64, u32)> =
            seeds.iter().map(|id| (*id, 0)).collect();

        while let Some((node, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            // Reverse edges: who calls / imports / inherits from me?
            let incoming = self.store.neighbours(
                node,
                &[EdgeKind::Calls, EdgeKind::Imports, EdgeKind::Inherits],
                /*outgoing=*/ false,
            )?;
            for e in incoming {
                if visited.insert(e.src) {
                    depth_of.insert(e.src, depth + 1);
                    queue.push_back((e.src, depth + 1));
                }
            }
        }

        let mut impacted = Vec::with_capacity(visited.len());
        for id in visited {
            if let Some(n) = self.store.node(id)? {
                impacted.push(n);
            }
        }
        Ok(ImpactResult {
            seeds,
            impacted,
            depth_of,
        })
    }

    /// Return a signature-only projection of all symbols in `path`.
    /// Used for "give the LLM the shape of this file, not its body."
    pub fn skeleton_for(&self, path: &str) -> Result<String> {
        let mut nodes = self.store.nodes_in_file(path)?;
        nodes.sort_by_key(|n| n.start_line);
        let mut out = String::new();
        for n in nodes {
            if matches!(n.kind, NodeKind::File) {
                continue;
            }
            if let Some(sig) = n.signature.as_ref() {
                out.push_str(sig.trim());
                if !sig.ends_with(';') {
                    out.push(';');
                }
                out.push('\n');
            }
        }
        Ok(out)
    }

    /// Find the `top_k` highest-PageRank nodes from a pool. Useful for
    /// "surface the most-connected symbols in the blast radius."
    pub fn top_central(&self, pool: &[i64], top_k: usize) -> Result<Vec<(i64, f64)>> {
        let pr = pagerank::run(self.store)?;
        let mut scored: Vec<(i64, f64)> = pool.iter().map(|&id| (id, pr.get(id))).collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        Ok(scored)
    }

    /// Lookup by textual symbol name (returns up to `limit` candidates).
    pub fn find(&self, name: &str, limit: usize) -> Result<Vec<Node>> {
        self.store.find_node_by_name(name, limit)
    }
}
