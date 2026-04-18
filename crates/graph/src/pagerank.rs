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

use crate::store::GraphStore;
use ahash::AHashMap;
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

    // Build out-adjacency and out-degree maps. Count every edge regardless of
    // kind; different edge kinds all carry some "importance" signal.
    let mut idx: AHashMap<i64, usize> = AHashMap::new();
    for (i, id) in nodes.iter().enumerate() {
        idx.insert(*id, i);
    }
    let n = nodes.len();
    let mut out_deg = vec![0u32; n];
    let mut out_adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in &edges {
        if let (Some(&si), Some(&di)) = (idx.get(&e.src), idx.get(&e.dst)) {
            out_adj[si].push(di);
            out_deg[si] += 1;
        }
    }

    let mut rank = vec![1.0 / n as f64; n];
    let mut next = vec![0.0; n];
    let teleport = (1.0 - damping) / n as f64;

    for _ in 0..max_iters {
        // Dangling mass: nodes with no out-edges contribute to every node.
        let mut dangling = 0.0;
        for i in 0..n {
            if out_deg[i] == 0 {
                dangling += rank[i];
            }
        }
        let dangling_share = damping * dangling / n as f64;

        for i in 0..n {
            next[i] = teleport + dangling_share;
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
    Ok(PageRankResult { scores })
}
