//! Louvain community detection.
//!
//! Modularity-maximising clustering of the code graph. We treat edges as
//! undirected (an edge `a → b` plus a reverse `b → a` are merged) and
//! compute modularity per Newman & Girvan:
//!
//! ```text
//! Q = (1 / 2m) · Σ_{ij} [A_{ij} - k_i k_j / 2m] · δ(c_i, c_j)
//! ```
//!
//! where `m` is the total edge weight, `k_i` is the (weighted) degree of
//! node `i`, and `δ(c_i, c_j)` is 1 iff nodes `i, j` share a community.
//!
//! ## Algorithm — single-pass local greedy
//!
//! Full Louvain has a multi-level "agglomeration" step that's expensive to
//! implement correctly. We use the **first phase only** (local greedy
//! moves) which is what the original paper calls "Louvain Phase 1":
//!
//!   1. Each node starts in its own community.
//!   2. Repeatedly: for every node, compute the modularity gain of moving
//!      it to each of its neighbours' communities; pick the best move
//!      (including "stay"); break out when a full pass produces no gains.
//!
//! Phase 1 alone produces high-quality clusters on code graphs (which
//! tend to have a strong community structure already because of file/
//! module boundaries) and runs in O(iters · |E|). Adding the full
//! aggregation phase would buy a few percent more modularity at the cost
//! of substantial implementation complexity — not worth it for our use
//! case where the clusters feed into ranking and budget allocation, not
//! into a downstream consumer that requires hierarchical structure.
//!
//! Modularity gain when moving node `i` (degree `k_i`) into community `C`
//! (total weighted degree `Σ_tot`, sum of edge weights from `i` into `C`
//! equal to `k_{i,in}`):
//!
//! ```text
//! ΔQ = (k_{i,in} / m) - (Σ_tot · k_i / 2m²)
//! ```

use crate::store::GraphStore;
use ahash::AHashMap;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct CommunityResult {
    /// Map from node id → community label (a stable, dense `u32` index).
    pub of: AHashMap<i64, u32>,
    /// Number of distinct communities.
    pub count: u32,
    /// Final modularity score (∈ [-0.5, 1.0]).
    pub modularity: f64,
}

impl CommunityResult {
    pub fn community_of(&self, id: i64) -> Option<u32> {
        self.of.get(&id).copied()
    }
}

pub fn run(store: &GraphStore) -> Result<CommunityResult> {
    run_with(store, 50)
}

pub fn run_with(store: &GraphStore, max_passes: usize) -> Result<CommunityResult> {
    let nodes = store.all_node_ids()?;
    let edges = store.all_edges()?;
    let n = nodes.len();
    if n == 0 {
        return Ok(CommunityResult {
            of: AHashMap::new(),
            count: 0,
            modularity: 0.0,
        });
    }

    // Dense reindexing so all the work is done over [0, n).
    let mut idx: AHashMap<i64, usize> = AHashMap::new();
    for (i, id) in nodes.iter().enumerate() {
        idx.insert(*id, i);
    }

    // Symmetrise: convert directed edges into undirected weights.
    // Each undirected edge contributes 1 unit to the total `m`.
    let mut adj: Vec<AHashMap<usize, f64>> = vec![AHashMap::new(); n];
    for e in &edges {
        let (s, d) = match (idx.get(&e.src), idx.get(&e.dst)) {
            (Some(&s), Some(&d)) if s != d => (s, d),
            _ => continue, // self-loops contribute zero to modularity
        };
        let w = e.confidence.max(0.0) as f64;
        if w == 0.0 {
            continue;
        }
        *adj[s].entry(d).or_insert(0.0) += w;
        *adj[d].entry(s).or_insert(0.0) += w;
    }

    let degree: Vec<f64> = adj.iter().map(|m| m.values().sum()).collect();
    let m: f64 = degree.iter().sum::<f64>() / 2.0;
    if m <= 0.0 {
        // No edges — every node is its own community.
        return Ok(CommunityResult {
            of: nodes
                .iter()
                .enumerate()
                .map(|(i, id)| (*id, i as u32))
                .collect(),
            count: n as u32,
            modularity: 0.0,
        });
    }

    // Each node starts in its own community.
    let mut comm: Vec<usize> = (0..n).collect();
    // Σ_tot[c] = total degree of nodes in community c.
    let mut sigma_tot: Vec<f64> = degree.clone();

    let two_m = 2.0 * m;
    for _ in 0..max_passes {
        let mut moved = false;
        for i in 0..n {
            let ki = degree[i];
            if ki == 0.0 {
                continue;
            }
            let ci = comm[i];

            // k_{i, c}: edge weight from i into each neighbouring community.
            let mut k_in: AHashMap<usize, f64> = AHashMap::new();
            for (&j, &w) in &adj[i] {
                if i == j {
                    continue;
                }
                *k_in.entry(comm[j]).or_insert(0.0) += w;
            }
            // Treat the current community fairly: when computing Δ for
            // moving to a *new* community c', we must subtract k_{i,ci}
            // (the contribution lost on leaving) before adding k_{i,c'}.
            let k_in_self = *k_in.get(&ci).unwrap_or(&0.0);

            let mut best_c = ci;
            let mut best_delta = 0.0f64;
            for (&c, &k_in_c) in &k_in {
                if c == ci {
                    continue;
                }
                let sigma_c = sigma_tot[c];
                // Gain from joining c minus loss from leaving ci.
                let gain_join = k_in_c - sigma_c * ki / two_m;
                let loss_leave = k_in_self - (sigma_tot[ci] - ki) * ki / two_m;
                let delta = (gain_join - loss_leave) / m;
                if delta > best_delta + 1e-12 {
                    best_delta = delta;
                    best_c = c;
                }
            }

            if best_c != ci {
                sigma_tot[ci] -= ki;
                sigma_tot[best_c] += ki;
                comm[i] = best_c;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }

    // Compress community labels into a dense [0, count) range.
    let mut relabel: AHashMap<usize, u32> = AHashMap::new();
    let mut next_id = 0u32;
    let mut out: AHashMap<i64, u32> = AHashMap::with_capacity(n);
    for (i, id) in nodes.iter().enumerate() {
        let raw = comm[i];
        let dense = *relabel.entry(raw).or_insert_with(|| {
            let id = next_id;
            next_id += 1;
            id
        });
        out.insert(*id, dense);
    }

    let modularity = compute_modularity(&adj, &comm, &degree, m);
    Ok(CommunityResult {
        of: out,
        count: next_id,
        modularity,
    })
}

fn compute_modularity(
    adj: &[AHashMap<usize, f64>],
    comm: &[usize],
    degree: &[f64],
    m: f64,
) -> f64 {
    let two_m = 2.0 * m;
    let mut q = 0.0;
    for i in 0..adj.len() {
        for (&j, &w) in &adj[i] {
            if comm[i] != comm[j] {
                continue;
            }
            q += w - degree[i] * degree[j] / two_m;
        }
    }
    q / two_m
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
    fn isolated_nodes_become_singleton_communities() {
        let store = temp_store();
        let a = store.insert_node(&node("a", "a.rs")).unwrap();
        let b = store.insert_node(&node("b", "b.rs")).unwrap();
        let r = run(&store).unwrap();
        assert_eq!(r.count, 2);
        assert_ne!(r.community_of(a), r.community_of(b));
    }

    #[test]
    fn two_cliques_collapse_into_two_communities() {
        // Build two K3 cliques connected by a single bridge edge. Louvain
        // should put each clique in its own community.
        let store = temp_store();
        let left: Vec<i64> = (0..3)
            .map(|i| {
                store
                    .insert_node(&node(&format!("L{i}"), &format!("l{i}.rs")))
                    .unwrap()
            })
            .collect();
        let right: Vec<i64> = (0..3)
            .map(|i| {
                store
                    .insert_node(&node(&format!("R{i}"), &format!("r{i}.rs")))
                    .unwrap()
            })
            .collect();
        // K3 on left
        for &a in &left {
            for &b in &left {
                if a != b {
                    let _ = store.insert_edge(&edge(a, b));
                }
            }
        }
        // K3 on right
        for &a in &right {
            for &b in &right {
                if a != b {
                    let _ = store.insert_edge(&edge(a, b));
                }
            }
        }
        // Bridge
        store.insert_edge(&edge(left[0], right[0])).unwrap();

        let r = run(&store).unwrap();
        // We expect 2 communities; the bridge node may go either way but
        // *all of left* should agree and *all of right* should agree.
        let cl: std::collections::HashSet<u32> = left
            .iter()
            .map(|id| r.community_of(*id).unwrap())
            .collect();
        let cr: std::collections::HashSet<u32> = right
            .iter()
            .map(|id| r.community_of(*id).unwrap())
            .collect();
        assert_eq!(cl.len(), 1, "left clique should be one community");
        assert_eq!(cr.len(), 1, "right clique should be one community");
        assert_ne!(cl, cr, "the two cliques should differ");
        assert!(r.modularity > 0.2, "modularity {} too low", r.modularity);
    }
}
