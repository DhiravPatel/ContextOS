//! Reachability pruning ("tree shaking" for the code graph).
//!
//! Given a set of root nodes (e.g. `main`, exported symbols, public API
//! entry points, the symbols a query mentioned), return every node
//! reachable by following edges *forward* from at least one root. Nodes
//! the graph never reaches from any root are dead — they carry no
//! information for the LLM about the requested behaviour, so the budget
//! shouldn't pay for them.
//!
//! This is the call-graph analogue of bundler tree-shaking: keep the
//! transitive closure of what's actually wired up; drop the rest.
//!
//! Two modes:
//!   * **Forward** (default) — follow `Calls`/`Imports`/`Inherits` from
//!     the roots. Answers "what does this code path execute?"
//!   * **Reverse** — same edges traversed backwards. Answers "who calls
//!     into this set?" — equivalent to the existing BFS impact radius
//!     but unbounded depth and exposed as a first-class API.

use crate::store::GraphStore;
use crate::types::{Edge, EdgeKind};
use ahash::{AHashMap, AHashSet};
use anyhow::Result;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Direction {
    Forward,
    Reverse,
}

#[derive(Debug, Clone)]
pub struct ReachableResult {
    pub reachable: Vec<i64>,
    pub roots: Vec<i64>,
}

pub fn run(store: &GraphStore, roots: &[i64], direction: Direction) -> Result<ReachableResult> {
    run_with(
        store,
        roots,
        direction,
        &[EdgeKind::Calls, EdgeKind::Imports, EdgeKind::Inherits],
    )
}

pub fn run_with(
    store: &GraphStore,
    roots: &[i64],
    direction: Direction,
    edge_kinds: &[EdgeKind],
) -> Result<ReachableResult> {
    let edges = store.all_edges()?;
    let allowed: AHashSet<EdgeKind> = edge_kinds.iter().copied().collect();

    let mut adj: AHashMap<i64, Vec<i64>> = AHashMap::new();
    for e in &edges {
        if !allowed.contains(&e.kind) {
            continue;
        }
        let (a, b) = match direction {
            Direction::Forward => (e.src, e.dst),
            Direction::Reverse => (e.dst, e.src),
        };
        adj.entry(a).or_default().push(b);
    }

    let mut visited: AHashSet<i64> = AHashSet::new();
    let mut queue: VecDeque<i64> = VecDeque::new();
    for &r in roots {
        if visited.insert(r) {
            queue.push_back(r);
        }
    }

    while let Some(v) = queue.pop_front() {
        if let Some(neighbours) = adj.get(&v) {
            for &w in neighbours {
                if visited.insert(w) {
                    queue.push_back(w);
                }
            }
        }
    }

    let mut out: Vec<i64> = visited.into_iter().collect();
    out.sort_unstable();
    Ok(ReachableResult {
        reachable: out,
        roots: roots.to_vec(),
    })
}

/// Convenience: filter an arbitrary node-id pool down to the
/// forward-reachable subset from `roots`. Useful as a candidate-set
/// pruning step before more expensive analyses.
pub fn prune_unreachable(
    store: &GraphStore,
    roots: &[i64],
    pool: &[i64],
) -> Result<Vec<i64>> {
    let r = run(store, roots, Direction::Forward)?;
    let live: AHashSet<i64> = r.reachable.into_iter().collect();
    Ok(pool.iter().copied().filter(|id| live.contains(id)).collect())
}

/// Helper used by tests and CLI for an ad-hoc reachable check.
#[allow(dead_code)]
fn _explicit_unused(_e: &Edge) {}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn forward_reaches_descendants_only() {
        let store = temp_store();
        let a = store.insert_node(&node("A")).unwrap();
        let b = store.insert_node(&node("B")).unwrap();
        let c = store.insert_node(&node("C")).unwrap();
        let dead = store.insert_node(&node("D")).unwrap();
        store
            .insert_edge(&Edge {
                src: a,
                dst: b,
                kind: EdgeKind::Calls,
                confidence: 1.0,
            })
            .unwrap();
        store
            .insert_edge(&Edge {
                src: b,
                dst: c,
                kind: EdgeKind::Calls,
                confidence: 1.0,
            })
            .unwrap();

        let r = run(&store, &[a], Direction::Forward).unwrap();
        let live: AHashSet<i64> = r.reachable.iter().copied().collect();
        assert!(live.contains(&a));
        assert!(live.contains(&b));
        assert!(live.contains(&c));
        assert!(!live.contains(&dead));
    }

    #[test]
    fn reverse_reaches_ancestors() {
        let store = temp_store();
        let a = store.insert_node(&node("A")).unwrap();
        let b = store.insert_node(&node("B")).unwrap();
        let c = store.insert_node(&node("C")).unwrap();
        store
            .insert_edge(&Edge {
                src: a,
                dst: b,
                kind: EdgeKind::Calls,
                confidence: 1.0,
            })
            .unwrap();
        store
            .insert_edge(&Edge {
                src: b,
                dst: c,
                kind: EdgeKind::Calls,
                confidence: 1.0,
            })
            .unwrap();

        let r = run(&store, &[c], Direction::Reverse).unwrap();
        let live: AHashSet<i64> = r.reachable.iter().copied().collect();
        assert!(live.contains(&a));
        assert!(live.contains(&b));
        assert!(live.contains(&c));
    }

    #[test]
    fn prune_drops_unreachable_pool_entries() {
        let store = temp_store();
        let a = store.insert_node(&node("A")).unwrap();
        let b = store.insert_node(&node("B")).unwrap();
        let dead = store.insert_node(&node("D")).unwrap();
        store
            .insert_edge(&Edge {
                src: a,
                dst: b,
                kind: EdgeKind::Calls,
                confidence: 1.0,
            })
            .unwrap();

        let pruned = prune_unreachable(&store, &[a], &[a, b, dead]).unwrap();
        let live: AHashSet<i64> = pruned.iter().copied().collect();
        assert!(live.contains(&a));
        assert!(live.contains(&b));
        assert!(!live.contains(&dead));
    }
}
