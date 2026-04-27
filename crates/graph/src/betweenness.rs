//! Approximate betweenness centrality via Brandes-Pich sampling.
//!
//! Exact Brandes is O(|V| · |E|) — fine for thousands of nodes, painful
//! beyond that. Brandes-Pich (2007) replaces the all-pairs single-source
//! shortest path (SSSP) with `k` SSSPs from uniformly-sampled pivots, and
//! rescales the partial scores by `|V| / k`. The estimator is unbiased
//! and converges as `O(1 / √k)`. For our purposes (a *prior* signal that
//! tips ranking on bridge nodes) `k = 50` is plenty, and the bottleneck
//! cost goes from `|V|` SSSPs to `k`, which is what we want.
//!
//! Why we want this: bridge nodes — the things you'd *cut* to disconnect
//! the graph — are nearly always load-bearing in code: a single function
//! that almost everything calls, an interface module the rest of the
//! tree imports through, a config struct passed everywhere. PageRank
//! captures "many things point at me", but a node can have low PR yet
//! high betweenness (a small router on a hot path) and vice versa.
//! Combining them in RRF gives the ranker two distinct centrality views.

use crate::store::GraphStore;
use crate::types::Edge;
use ahash::AHashMap;
use anyhow::Result;
use std::collections::VecDeque;

pub struct BetweennessResult {
    pub scores: AHashMap<i64, f64>,
}

impl BetweennessResult {
    pub fn get(&self, id: i64) -> f64 {
        self.scores.get(&id).copied().unwrap_or(0.0)
    }
}

/// Default sample size — `k = 50` pivots gives a usable estimator on
/// graphs up to ~100k nodes.
pub const DEFAULT_PIVOTS: usize = 50;

pub fn run(store: &GraphStore) -> Result<BetweennessResult> {
    run_with(store, DEFAULT_PIVOTS, 0xC0FFEE_42_4242)
}

/// Sampled betweenness with explicit pivot count and RNG seed.
///
/// Uses a deterministic xorshift PRNG seeded from `seed` so reruns over
/// the same graph give identical results (important for testability and
/// for cache-aware downstream consumers).
pub fn run_with(store: &GraphStore, pivots: usize, seed: u64) -> Result<BetweennessResult> {
    let nodes = store.all_node_ids()?;
    let edges = store.all_edges()?;
    let n = nodes.len();
    if n == 0 {
        return Ok(BetweennessResult {
            scores: AHashMap::new(),
        });
    }
    let mut idx: AHashMap<i64, usize> = AHashMap::new();
    for (i, id) in nodes.iter().enumerate() {
        idx.insert(*id, i);
    }
    let adj = build_undirected_adj(&edges, &idx, n);

    // Sampled set of source pivots.
    let k = pivots.min(n);
    let mut rng = XorShift64(seed.max(1));
    let mut chosen = vec![false; n];
    let mut sources: Vec<usize> = Vec::with_capacity(k);
    let mut tries = 0;
    while sources.len() < k && tries < k * 8 {
        let s = rng.next() as usize % n;
        if !chosen[s] {
            chosen[s] = true;
            sources.push(s);
        }
        tries += 1;
    }
    if sources.is_empty() {
        // Pathological: sampling failed; fall through to full Brandes
        // (which on tiny graphs is fine).
        sources = (0..n).collect();
    }

    let mut bc = vec![0.0f64; n];
    for &s in &sources {
        accumulate_brandes(s, &adj, n, &mut bc);
    }

    // Brandes-Pich rescaling: scores were summed over `k` pivots; the
    // unbiased estimator multiplies by `n / k` and (since we treated the
    // graph as undirected) divides by 2.
    let scale = n as f64 / sources.len().max(1) as f64;
    for s in bc.iter_mut() {
        *s *= scale * 0.5;
    }

    let mut out: AHashMap<i64, f64> = AHashMap::with_capacity(n);
    for (i, id) in nodes.iter().enumerate() {
        out.insert(*id, bc[i]);
    }
    Ok(BetweennessResult { scores: out })
}

fn build_undirected_adj(
    edges: &[Edge],
    idx: &AHashMap<i64, usize>,
    n: usize,
) -> Vec<Vec<usize>> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut seen: ahash::AHashSet<(usize, usize)> = ahash::AHashSet::new();
    for e in edges {
        let (s, d) = match (idx.get(&e.src), idx.get(&e.dst)) {
            (Some(&s), Some(&d)) if s != d => (s, d),
            _ => continue,
        };
        let key = if s < d { (s, d) } else { (d, s) };
        if !seen.insert(key) {
            continue;
        }
        adj[s].push(d);
        adj[d].push(s);
    }
    adj
}

/// Single-source partial Brandes accumulator. Accumulates δ(s) values
/// into the supplied `bc` vector. Standard textbook formulation.
fn accumulate_brandes(s: usize, adj: &[Vec<usize>], n: usize, bc: &mut [f64]) {
    let mut sigma = vec![0.0f64; n]; // # shortest paths from s to v
    let mut dist = vec![-1i64; n];
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    sigma[s] = 1.0;
    dist[s] = 0;

    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut q = VecDeque::new();
    q.push_back(s);
    while let Some(v) = q.pop_front() {
        order.push(v);
        for &w in &adj[v] {
            if dist[w] < 0 {
                dist[w] = dist[v] + 1;
                q.push_back(w);
            }
            if dist[w] == dist[v] + 1 {
                sigma[w] += sigma[v];
                preds[w].push(v);
            }
        }
    }

    let mut delta = vec![0.0f64; n];
    while let Some(w) = order.pop() {
        for &v in &preds[w] {
            if sigma[w] > 0.0 {
                let coef = (sigma[v] / sigma[w]) * (1.0 + delta[w]);
                delta[v] += coef;
            }
        }
        if w != s {
            bc[w] += delta[w];
        }
    }
}

/// Cheap deterministic PRNG. We don't need cryptographic quality; we
/// just need reproducibility across runs.
struct XorShift64(u64);
impl XorShift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
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

    fn edge(src: i64, dst: i64) -> Edge {
        Edge {
            src,
            dst,
            kind: EdgeKind::Calls,
            confidence: 1.0,
        }
    }

    #[test]
    fn bridge_node_has_higher_betweenness_than_leaves() {
        // Two K3 cliques connected through a single bridge node `B`.
        // Every shortest path between the cliques goes through B, so its
        // betweenness should dominate.
        let store = temp_store();
        let l: Vec<i64> = (0..3)
            .map(|i| {
                store
                    .insert_node(&node(&format!("L{i}"), &format!("l{i}.rs")))
                    .unwrap()
            })
            .collect();
        let bridge = store.insert_node(&node("B", "b.rs")).unwrap();
        let r: Vec<i64> = (0..3)
            .map(|i| {
                store
                    .insert_node(&node(&format!("R{i}"), &format!("r{i}.rs")))
                    .unwrap()
            })
            .collect();
        for &a in &l {
            for &b in &l {
                if a != b {
                    let _ = store.insert_edge(&edge(a, b));
                }
            }
        }
        for &a in &r {
            for &b in &r {
                if a != b {
                    let _ = store.insert_edge(&edge(a, b));
                }
            }
        }
        store.insert_edge(&edge(l[0], bridge)).unwrap();
        store.insert_edge(&edge(bridge, r[0])).unwrap();

        let bc = run(&store).unwrap();
        let bridge_score = bc.get(bridge);
        let leaf_score = bc.get(l[2]);
        assert!(
            bridge_score > leaf_score * 1.5,
            "bridge={bridge_score:.2} leaf={leaf_score:.2}"
        );
    }

    #[test]
    fn empty_graph_returns_empty_scores() {
        let store = temp_store();
        let r = run(&store).unwrap();
        assert!(r.scores.is_empty());
    }
}
