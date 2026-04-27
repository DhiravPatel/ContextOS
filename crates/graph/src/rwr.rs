//! Random Walk with Restart (RWR).
//!
//! A *soft* alternative to the BFS blast-radius query in [`crate::query`].
//! Instead of "every node reachable in ≤ d hops", RWR returns a
//! probability distribution: nodes structurally close to the seeds get
//! higher mass; nodes far away decay smoothly toward zero. This is the
//! same kernel as Personalized PageRank but interpreted differently:
//! callers usually take the top-k nodes by probability, not the
//! steady-state ranking.
//!
//! The recurrence:
//!
//! ```text
//! r_{t+1} = (1 - α) · v + α · M · r_t
//! ```
//!
//! where:
//!   * `v` is the seed distribution (1/|S| on seeds, 0 elsewhere),
//!   * `α` is the propagation probability (0 < α < 1; we use 0.5 by
//!     default — sharper than PageRank's 0.85 because we want a tighter
//!     local profile, not a full repo-wide steady state),
//!   * `M` is the column-stochastic transition matrix (1 / out-degree on
//!     edges; dangling rows are routed back to the seed vector).
//!
//! Convergence: power-iterate until `||r_{t+1} - r_t||_1 < tol`. For
//! reasonable graphs and α=0.5 this converges in 15–25 iterations.

use crate::store::GraphStore;
use crate::types::Edge;
use ahash::AHashMap;
use anyhow::Result;

/// Default propagation probability. Lower than PageRank's 0.85 because RWR
/// is meant to express *local* relevance, not global centrality.
pub const DEFAULT_ALPHA: f64 = 0.5;

pub struct RwrResult {
    pub scores: AHashMap<i64, f64>,
}

impl RwrResult {
    pub fn get(&self, id: i64) -> f64 {
        self.scores.get(&id).copied().unwrap_or(0.0)
    }

    /// Top-k nodes by probability mass, descending.
    pub fn top_k(&self, k: usize) -> Vec<(i64, f64)> {
        let mut v: Vec<(i64, f64)> = self.scores.iter().map(|(id, s)| (*id, *s)).collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(k);
        v
    }
}

pub fn run(store: &GraphStore, seeds: &[i64]) -> Result<RwrResult> {
    run_with(store, seeds, DEFAULT_ALPHA, 50, 1e-6)
}

pub fn run_with(
    store: &GraphStore,
    seeds: &[i64],
    alpha: f64,
    max_iters: usize,
    tol: f64,
) -> Result<RwrResult> {
    let nodes = store.all_node_ids()?;
    let edges = store.all_edges()?;
    if nodes.is_empty() {
        return Ok(RwrResult {
            scores: AHashMap::new(),
        });
    }
    let mut idx: AHashMap<i64, usize> = AHashMap::new();
    for (i, id) in nodes.iter().enumerate() {
        idx.insert(*id, i);
    }
    let n = nodes.len();

    // Seed vector — falls back to uniform if no seeds resolve.
    let mut v = vec![0.0; n];
    let mut seed_count = 0usize;
    for s in seeds {
        if let Some(&i) = idx.get(s) {
            v[i] = 1.0;
            seed_count += 1;
        }
    }
    if seed_count == 0 {
        for slot in v.iter_mut() {
            *slot = 1.0 / n as f64;
        }
    } else {
        let inv = 1.0 / seed_count as f64;
        for slot in v.iter_mut() {
            if *slot > 0.0 {
                *slot = inv;
            }
        }
    }

    // Forward adjacency + out-degree.
    let mut out_deg = vec![0u32; n];
    let mut out_adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in &edges {
        if let (Some(&si), Some(&di)) = (idx.get(&e.src), idx.get(&e.dst)) {
            out_adj[si].push(di);
            out_deg[si] += 1;
        }
    }

    let mut r = v.clone();
    let mut next = vec![0.0; n];

    for _ in 0..max_iters {
        // Restart term: (1 − α) · v on every node.
        for i in 0..n {
            next[i] = (1.0 - alpha) * v[i];
        }
        // Dangling mass: route nodes with no out-edges back through the
        // seed distribution (otherwise the random surfer "escapes" the
        // graph and we lose probability mass each step).
        let mut dangling = 0.0;
        for i in 0..n {
            if out_deg[i] == 0 {
                dangling += r[i];
            }
        }
        for i in 0..n {
            next[i] += alpha * dangling * v[i];
        }
        // Propagation: α · M · r.
        for i in 0..n {
            if out_deg[i] == 0 {
                continue;
            }
            let share = alpha * r[i] / out_deg[i] as f64;
            for &j in &out_adj[i] {
                next[j] += share;
            }
        }

        let mut delta = 0.0;
        for i in 0..n {
            delta += (next[i] - r[i]).abs();
            r[i] = next[i];
        }
        if delta < tol {
            break;
        }
    }

    let mut scores: AHashMap<i64, f64> = AHashMap::with_capacity(n);
    for (i, id) in nodes.iter().enumerate() {
        scores.insert(*id, r[i]);
    }
    Ok(RwrResult { scores })
}

/// Internal helper: power-iterate over an in-memory graph. Used by
/// [`run_with`] above and by other modules ([`crate::betweenness`]) that
/// have already loaded `nodes`/`edges`.
#[allow(dead_code)]
pub(crate) fn iterate(
    nodes: &[i64],
    edges: &[Edge],
    seeds: &AHashMap<usize, f64>,
    alpha: f64,
    max_iters: usize,
    tol: f64,
) -> Vec<f64> {
    let n = nodes.len();
    if n == 0 {
        return Vec::new();
    }
    let mut idx: AHashMap<i64, usize> = AHashMap::new();
    for (i, id) in nodes.iter().enumerate() {
        idx.insert(*id, i);
    }
    let mut v = vec![0.0; n];
    let mut total = 0.0;
    for (&i, &w) in seeds {
        if i < n {
            v[i] = w;
            total += w;
        }
    }
    if total <= 0.0 {
        for slot in v.iter_mut() {
            *slot = 1.0 / n as f64;
        }
    } else {
        for slot in v.iter_mut() {
            *slot /= total;
        }
    }

    let mut out_deg = vec![0u32; n];
    let mut out_adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in edges {
        if let (Some(&si), Some(&di)) = (idx.get(&e.src), idx.get(&e.dst)) {
            out_adj[si].push(di);
            out_deg[si] += 1;
        }
    }

    let mut r = v.clone();
    let mut next = vec![0.0; n];
    for _ in 0..max_iters {
        for i in 0..n {
            next[i] = (1.0 - alpha) * v[i];
        }
        let mut dangling = 0.0;
        for i in 0..n {
            if out_deg[i] == 0 {
                dangling += r[i];
            }
        }
        for i in 0..n {
            next[i] += alpha * dangling * v[i];
        }
        for i in 0..n {
            if out_deg[i] == 0 {
                continue;
            }
            let share = alpha * r[i] / out_deg[i] as f64;
            for &j in &out_adj[i] {
                next[j] += share;
            }
        }
        let mut delta = 0.0;
        for i in 0..n {
            delta += (next[i] - r[i]).abs();
            r[i] = next[i];
        }
        if delta < tol {
            break;
        }
    }
    r
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
    fn rwr_concentrates_mass_near_seeds() {
        // Chain: a -> b -> c -> d -> e, with a self-loop on a so it's not dangling.
        let store = temp_store();
        let ids: Vec<i64> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|n| store.insert_node(&node(n, &format!("{n}.rs"))).unwrap())
            .collect();
        for w in ids.windows(2) {
            store
                .insert_edge(&Edge {
                    src: w[0],
                    dst: w[1],
                    kind: EdgeKind::Calls,
                    confidence: 1.0,
                })
                .unwrap();
        }

        let r = run(&store, &[ids[0]]).unwrap();
        // a (the seed) should outscore e (4 hops away).
        assert!(r.get(ids[0]) > r.get(ids[4]));
        // and the gradient should be monotone-ish along the chain.
        assert!(r.get(ids[0]) >= r.get(ids[1]));
        assert!(r.get(ids[2]) >= r.get(ids[4]));
    }

    #[test]
    fn rwr_unknown_seeds_falls_back_to_uniform() {
        let store = temp_store();
        let a = store.insert_node(&node("a", "a.rs")).unwrap();
        let b = store.insert_node(&node("b", "b.rs")).unwrap();
        let r = run(&store, &[/* nothing in graph */ 99_999]).unwrap();
        // With no resolvable seeds, uniform restart → both nodes get
        // similar mass.
        assert!((r.get(a) - r.get(b)).abs() < 1e-6);
    }

    #[test]
    fn rwr_top_k_returns_in_descending_order() {
        let store = temp_store();
        let ids: Vec<i64> = (0..4)
            .map(|i| {
                store
                    .insert_node(&node(&format!("n{i}"), &format!("n{i}.rs")))
                    .unwrap()
            })
            .collect();
        for w in ids.windows(2) {
            store
                .insert_edge(&Edge {
                    src: w[0],
                    dst: w[1],
                    kind: EdgeKind::Calls,
                    confidence: 1.0,
                })
                .unwrap();
        }
        let r = run(&store, &[ids[0]]).unwrap();
        let top = r.top_k(3);
        assert!(top.len() <= 3);
        for w in top.windows(2) {
            assert!(w[0].1 >= w[1].1);
        }
    }
}
