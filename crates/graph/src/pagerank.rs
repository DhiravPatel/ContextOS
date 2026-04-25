//! Power-iteration PageRank over the full graph.
//!
//! Used to bias ranking and budgeting toward "central" symbols — the ones
//! many other things depend on. Running it offline (after each build) is
//! cheap: ~10ms for 10k nodes, ~100ms for 100k.
//!
//! Standard formulation:
//!     r(n) = (1-d)/N + d · Σ_{m → n} r(m) / out_degree(m)
//! with damping `d = 0.85` and 50 iterations (converges well before that
//! for typical codebases).
//!
//! ## Personalized PageRank
//!
//! Vanilla PageRank tells you which symbols are important *to the repo*.
//! Personalized PageRank ([`run_personalized`]) tells you which symbols are
//! important *to a specific request*. Mathematically: replace the uniform
//! teleport vector `1/N` with a sparse one that puts mass on a seed set
//! (e.g. changed files, query-matched symbols). The walk still wanders
//! through the graph, but every restart returns to the seeds — so symbols
//! structurally close to the seeds dominate the steady state.
//!
//! ```text
//! r(n) = (1-d) · v(n) + d · Σ_{m → n} r(m) / out_degree(m)
//! ```
//!
//! where `v` is a probability vector with `v(seed) = 1/|seeds|` and zero
//! elsewhere. This is the same propagation kernel; only the teleport
//! distribution changes.

use crate::store::GraphStore;
use ahash::{AHashMap, AHashSet};
use anyhow::Result;

pub struct PageRankResult {
    pub scores: AHashMap<i64, f64>,
}

impl PageRankResult {
    pub fn get(&self, id: i64) -> f64 {
        self.scores.get(&id).copied().unwrap_or(0.0)
    }
}

pub fn run(store: &GraphStore) -> Result<PageRankResult> {
    run_with(store, 0.85, 50, 1e-6)
}

/// Personalized PageRank with the teleport distribution concentrated on the
/// given seed nodes. Seeds not present in the graph are silently skipped;
/// passing an empty (or all-missing) seed set falls back to vanilla PageRank.
pub fn run_personalized(store: &GraphStore, seeds: &[i64]) -> Result<PageRankResult> {
    run_personalized_with(store, seeds, 0.85, 50, 1e-6)
}

pub fn run_personalized_with(
    store: &GraphStore,
    seeds: &[i64],
    damping: f64,
    max_iters: usize,
    tol: f64,
) -> Result<PageRankResult> {
    let nodes = store.all_node_ids()?;
    let edges = store.all_edges()?;
    if nodes.is_empty() {
        return Ok(PageRankResult {
            scores: AHashMap::new(),
        });
    }

    let mut idx: AHashMap<i64, usize> = AHashMap::new();
    for (i, id) in nodes.iter().enumerate() {
        idx.insert(*id, i);
    }
    let n = nodes.len();
    let valid_seeds: AHashSet<usize> = seeds
        .iter()
        .filter_map(|s| idx.get(s).copied())
        .collect();

    // Empty / unknown seed set → vanilla PageRank semantics, but reuse
    // already-loaded edges to avoid the extra round-trip to sqlite.
    let teleport: Vec<f64> = if valid_seeds.is_empty() {
        vec![1.0 / n as f64; n]
    } else {
        let s = valid_seeds.len() as f64;
        let mut v = vec![0.0; n];
        for &i in &valid_seeds {
            v[i] = 1.0 / s;
        }
        v
    };

    Ok(power_iterate(&nodes, &edges, &idx, &teleport, damping, max_iters, tol))
}

pub fn run_with(
    store: &GraphStore,
    damping: f64,
    max_iters: usize,
    tol: f64,
) -> Result<PageRankResult> {
    let nodes = store.all_node_ids()?;
    let edges = store.all_edges()?;
    if nodes.is_empty() {
        return Ok(PageRankResult {
            scores: AHashMap::new(),
        });
    }

    let mut idx: AHashMap<i64, usize> = AHashMap::new();
    for (i, id) in nodes.iter().enumerate() {
        idx.insert(*id, i);
    }
    let n = nodes.len();
    let teleport = vec![1.0 / n as f64; n];
    Ok(power_iterate(&nodes, &edges, &idx, &teleport, damping, max_iters, tol))
}

/// Shared inner loop for vanilla and personalized PageRank.
///
/// `teleport` is the per-node restart distribution, expected to sum to 1.0.
/// For vanilla PR pass `[1/N; N]`; for personalized PR put mass only on the
/// seed nodes. Dangling-node mass is redistributed via the teleport vector
/// (correct handling for both cases — for vanilla it's the uniform `1/N`,
/// for personalized it concentrates dangling mass back on the seeds, which
/// is the property that makes the steady state actually personalized).
fn power_iterate(
    nodes: &[i64],
    edges: &[crate::types::Edge],
    idx: &AHashMap<i64, usize>,
    teleport: &[f64],
    damping: f64,
    max_iters: usize,
    tol: f64,
) -> PageRankResult {
    let n = nodes.len();
    let mut out_deg = vec![0u32; n];
    let mut out_adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in edges {
        if let (Some(&si), Some(&di)) = (idx.get(&e.src), idx.get(&e.dst)) {
            out_adj[si].push(di);
            out_deg[si] += 1;
        }
    }

    // Initial distribution = teleport. For vanilla that's uniform; for
    // personalized that concentrates probability on the seeds, which
    // converges substantially faster than starting from uniform.
    let mut rank = teleport.to_vec();
    let mut next = vec![0.0; n];

    for _ in 0..max_iters {
        let mut dangling = 0.0;
        for i in 0..n {
            if out_deg[i] == 0 {
                dangling += rank[i];
            }
        }
        for i in 0..n {
            next[i] = (1.0 - damping) * teleport[i] + damping * dangling * teleport[i];
        }
        for i in 0..n {
            if out_deg[i] == 0 {
                continue;
            }
            let share = damping * rank[i] / out_deg[i] as f64;
            for &j in &out_adj[i] {
                next[j] += share;
            }
        }

        let mut delta = 0.0;
        for i in 0..n {
            delta += (next[i] - rank[i]).abs();
            rank[i] = next[i];
        }
        if delta < tol {
            break;
        }
    }

    let mut scores: AHashMap<i64, f64> = AHashMap::with_capacity(n);
    for (i, id) in nodes.iter().enumerate() {
        scores.insert(*id, rank[i]);
    }
    PageRankResult { scores }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::GraphStore;
    use crate::types::{Edge, EdgeKind, Node, NodeKind};
    use contextos_utils::Language;

    fn temp_store() -> GraphStore {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("g.db");
        // leak the dir so it lives as long as the connection
        Box::leak(Box::new(dir));
        GraphStore::open(path).expect("open store")
    }

    fn node(name: &str, path: &str) -> Node {
        Node {
            id: 0,
            kind: NodeKind::Function,
            name: name.into(),
            qualified: format!("{path}::{name}"),
            path: path.into(),
            language: Language::Rust,
            start_line: 1,
            end_line: 2,
            signature: None,
            body_bytes: 0,
        }
    }

    #[test]
    fn personalized_pagerank_concentrates_mass_on_seeds() {
        let store = temp_store();
        // Three-node chain: a -> b -> c (and back-edges to keep the graph
        // strongly connected so vanilla PR is roughly uniform).
        let a = store.insert_node(&node("a", "a.rs")).unwrap();
        let b = store.insert_node(&node("b", "b.rs")).unwrap();
        let c = store.insert_node(&node("c", "c.rs")).unwrap();
        for (s, d) in [(a, b), (b, c), (c, a)] {
            store
                .insert_edge(&Edge {
                    src: s,
                    dst: d,
                    kind: EdgeKind::Calls,
                    confidence: 1.0,
                })
                .unwrap();
        }

        let vanilla = run(&store).unwrap();
        let pp = run_personalized(&store, &[a]).unwrap();

        // Personalized must allocate strictly more mass to the seed than the
        // vanilla score for the same node.
        assert!(pp.get(a) > vanilla.get(a));
        // ...and at least some other node must be lower than its vanilla
        // counterpart, since total mass is conserved.
        assert!(pp.get(b) < vanilla.get(b) || pp.get(c) < vanilla.get(c));
    }

    #[test]
    fn personalized_with_empty_seeds_matches_vanilla() {
        let store = temp_store();
        let a = store.insert_node(&node("a", "a.rs")).unwrap();
        let b = store.insert_node(&node("b", "b.rs")).unwrap();
        store
            .insert_edge(&Edge {
                src: a,
                dst: b,
                kind: EdgeKind::Calls,
                confidence: 1.0,
            })
            .unwrap();

        let v = run(&store).unwrap();
        let p = run_personalized(&store, &[]).unwrap();

        for id in [a, b] {
            assert!((v.get(id) - p.get(id)).abs() < 1e-9);
        }
    }
}
