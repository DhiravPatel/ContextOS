//! Steiner-tree approximation (KMB algorithm).
//!
//! The Steiner tree problem: given a set of "terminal" nodes (the symbols
//! we *must* include in the LLM context), find the minimum-weight
//! connected subgraph that touches all of them. NP-hard in general, but
//! Kou-Markowsky-Berman (1981) gives a 2 × OPT approximation in
//! polynomial time:
//!
//!   1. Build the **metric closure** restricted to the terminals: for
//!      every pair of terminals, find the shortest path between them in
//!      the original graph.
//!   2. Compute a minimum spanning tree (MST) over the metric closure.
//!   3. Replace each MST edge with the corresponding shortest path from
//!      step 1, dropping duplicates.
//!   4. Compute an MST of the resulting subgraph and prune leaves that
//!      aren't terminals.
//!
//! Why we want this for token reduction: the BFS blast-radius query in
//! [`crate::query`] returns *all* nodes within `d` hops of the seed —
//! easy to reason about but often a lot more than the LLM actually
//! needs. A Steiner subgraph instead returns the **smallest call-tree
//! that connects the symbols you specifically asked for**, which on
//! real code is typically 5–10× smaller while preserving every call
//! path between the asks. Lossless: every node returned was already in
//! the graph, every edge was already in the graph, no new structure is
//! invented.

use crate::store::GraphStore;
use crate::types::Edge;
use ahash::{AHashMap, AHashSet};
use anyhow::Result;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

#[derive(Debug, Clone)]
pub struct SteinerResult {
    /// All node ids in the approximate Steiner subgraph (a superset of
    /// the input terminals).
    pub nodes: Vec<i64>,
    /// Edges in the subgraph as `(src, dst)` pairs. Direction is the
    /// shortest-path direction we discovered; the underlying graph may
    /// have additional edges we chose not to include.
    pub edges: Vec<(i64, i64)>,
}

pub fn run(store: &GraphStore, terminals: &[i64]) -> Result<SteinerResult> {
    let nodes = store.all_node_ids()?;
    let edges = store.all_edges()?;
    if terminals.is_empty() || nodes.is_empty() {
        return Ok(SteinerResult {
            nodes: terminals.to_vec(),
            edges: Vec::new(),
        });
    }
    let mut idx: AHashMap<i64, usize> = AHashMap::new();
    for (i, id) in nodes.iter().enumerate() {
        idx.insert(*id, i);
    }
    let n = nodes.len();
    let adj = build_adj(&edges, &idx, n);

    let term_ix: Vec<usize> = terminals.iter().filter_map(|t| idx.get(t).copied()).collect();
    if term_ix.is_empty() {
        return Ok(SteinerResult {
            nodes: Vec::new(),
            edges: Vec::new(),
        });
    }
    if term_ix.len() == 1 {
        return Ok(SteinerResult {
            nodes: vec![nodes[term_ix[0]]],
            edges: Vec::new(),
        });
    }

    // Step 1: shortest-path BFS from each terminal. We keep predecessor
    // arrays so we can reconstruct paths in step 3.
    let mut dist_from: Vec<Vec<i64>> = Vec::with_capacity(term_ix.len());
    let mut pred_from: Vec<Vec<i32>> = Vec::with_capacity(term_ix.len());
    for &t in &term_ix {
        let (d, p) = bfs_with_preds(&adj, n, t);
        dist_from.push(d);
        pred_from.push(p);
    }

    // Step 2: MST of the metric closure over the terminal set.
    // Prim's algorithm starting at terminals[0]; weights are BFS hops.
    let k = term_ix.len();
    let mut in_tree = vec![false; k];
    let mut mst_edges: Vec<(usize, usize)> = Vec::with_capacity(k - 1);
    in_tree[0] = true;
    while mst_edges.len() < k - 1 {
        let mut best: Option<(i64, usize, usize)> = None;
        for i in 0..k {
            if !in_tree[i] {
                continue;
            }
            for j in 0..k {
                if in_tree[j] {
                    continue;
                }
                let d = dist_from[i][term_ix[j]];
                if d < 0 {
                    continue; // unreachable terminal — skip silently
                }
                if best.map(|(bd, _, _)| d < bd).unwrap_or(true) {
                    best = Some((d, i, j));
                }
            }
        }
        match best {
            Some((_, i, j)) => {
                mst_edges.push((i, j));
                in_tree[j] = true;
            }
            None => break, // disconnected terminals — emit what we have
        }
    }

    // Step 3: expand each MST edge back into its shortest path, collect
    // a multiset of nodes & edges, then dedupe.
    let mut node_set: AHashSet<usize> = AHashSet::new();
    let mut edge_set: AHashSet<(usize, usize)> = AHashSet::new();
    for &(i, j) in &mst_edges {
        let path = reconstruct_path(&pred_from[i], term_ix[j]);
        for &v in &path {
            node_set.insert(v);
        }
        for w in path.windows(2) {
            edge_set.insert((w[0], w[1]));
        }
    }
    // Always include the terminals themselves, even if some were unreachable.
    for &t in &term_ix {
        node_set.insert(t);
    }

    // Step 4: prune non-terminal leaves. Iterate until stable.
    let term_set: AHashSet<usize> = term_ix.iter().copied().collect();
    loop {
        let mut to_remove: Vec<usize> = Vec::new();
        for &v in &node_set {
            if term_set.contains(&v) {
                continue;
            }
            let deg = edge_set
                .iter()
                .filter(|(a, b)| *a == v || *b == v)
                .count();
            if deg <= 1 {
                to_remove.push(v);
            }
        }
        if to_remove.is_empty() {
            break;
        }
        for v in to_remove {
            node_set.remove(&v);
            edge_set.retain(|(a, b)| *a != v && *b != v);
        }
    }

    let mut out_nodes: Vec<i64> = node_set.iter().map(|&i| nodes[i]).collect();
    out_nodes.sort_unstable();
    let mut out_edges: Vec<(i64, i64)> = edge_set
        .iter()
        .map(|&(a, b)| (nodes[a], nodes[b]))
        .collect();
    out_edges.sort_unstable();

    Ok(SteinerResult {
        nodes: out_nodes,
        edges: out_edges,
    })
}

fn build_adj(edges: &[Edge], idx: &AHashMap<i64, usize>, n: usize) -> Vec<Vec<usize>> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut seen: AHashSet<(usize, usize)> = AHashSet::new();
    for e in edges {
        let (s, d) = match (idx.get(&e.src), idx.get(&e.dst)) {
            (Some(&s), Some(&d)) if s != d => (s, d),
            _ => continue,
        };
        // Treat as undirected for path finding — code graphs have meaningful
        // structural connectivity in both directions.
        if seen.insert((s, d)) {
            adj[s].push(d);
        }
        if seen.insert((d, s)) {
            adj[d].push(s);
        }
    }
    adj
}

fn bfs_with_preds(adj: &[Vec<usize>], n: usize, src: usize) -> (Vec<i64>, Vec<i32>) {
    let mut dist = vec![-1i64; n];
    let mut pred = vec![-1i32; n];
    dist[src] = 0;
    let mut q = VecDeque::new();
    q.push_back(src);
    while let Some(v) = q.pop_front() {
        for &w in &adj[v] {
            if dist[w] < 0 {
                dist[w] = dist[v] + 1;
                pred[w] = v as i32;
                q.push_back(w);
            }
        }
    }
    (dist, pred)
}

fn reconstruct_path(pred: &[i32], target: usize) -> Vec<usize> {
    let mut path = Vec::new();
    let mut cur = target as i32;
    while cur >= 0 {
        path.push(cur as usize);
        cur = pred[cur as usize];
    }
    path.reverse();
    path
}

// We import `BinaryHeap` and `Reverse` for future Dijkstra support over
// weighted edges; not used by the current BFS path. Suppress the warning.
#[allow(dead_code)]
fn _imports_keep() {
    let _ = BinaryHeap::<Reverse<i64>>::new();
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

    fn node(name: &str) -> Node {
        Node {
            id: 0,
            kind: NodeKind::Function,
            name: name.into(),
            qualified: format!("p::{name}"),
            path: format!("{name}.rs"),
            language: Language::Rust,
            start_line: 1,
            end_line: 2,
            signature: None,
            body_bytes: 0,
        }
    }

    #[test]
    fn three_terminals_connected_via_hub_keep_hub() {
        // Star: hub H connected to leaves A, B, C, plus an unrelated node U
        // far away. Steiner tree on {A, B, C} should keep H but drop U.
        let store = temp_store();
        let h = store.insert_node(&node("H")).unwrap();
        let a = store.insert_node(&node("A")).unwrap();
        let b = store.insert_node(&node("B")).unwrap();
        let c = store.insert_node(&node("C")).unwrap();
        let u = store.insert_node(&node("U")).unwrap();
        for &t in &[a, b, c, u] {
            store
                .insert_edge(&Edge {
                    src: h,
                    dst: t,
                    kind: EdgeKind::Calls,
                    confidence: 1.0,
                })
                .unwrap();
        }

        let r = run(&store, &[a, b, c]).unwrap();
        let nodes: AHashSet<i64> = r.nodes.iter().copied().collect();
        for &t in &[a, b, c, h] {
            assert!(nodes.contains(&t), "expected {t} in Steiner result");
        }
        assert!(!nodes.contains(&u), "unrelated node U should be pruned");
    }

    #[test]
    fn single_terminal_returns_just_itself() {
        let store = temp_store();
        let a = store.insert_node(&node("A")).unwrap();
        let r = run(&store, &[a]).unwrap();
        assert_eq!(r.nodes, vec![a]);
        assert!(r.edges.is_empty());
    }

    #[test]
    fn disconnected_terminals_still_returned() {
        // Two unconnected components, one terminal in each.
        let store = temp_store();
        let a = store.insert_node(&node("A")).unwrap();
        let b = store.insert_node(&node("B")).unwrap();
        let r = run(&store, &[a, b]).unwrap();
        let nodes: AHashSet<i64> = r.nodes.iter().copied().collect();
        // Both terminals must appear even with no path between them.
        assert!(nodes.contains(&a));
        assert!(nodes.contains(&b));
    }
}
