//! Seeded benchmark topology: a ring for guaranteed connectedness plus
//! seeded random extra edges up to the requested per-node budget.

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::collections::BTreeSet;

/// Undirected edge set over node indices, stored as ordered pairs.
#[derive(Debug)]
pub struct Topology {
    edges: Vec<(usize, usize)>,
}

fn ordered(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

impl Topology {
    /// Ring over all nodes plus, per node, `edges_per_node - 2` random
    /// extra edges (deduplicated, no self-loops). Deterministic in `seed`.
    pub fn ring_plus_random(nodes: usize, edges_per_node: usize, seed: u64) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let ring: BTreeSet<(usize, usize)> = (0..nodes)
            .filter(|i| nodes >= 2 && *i != (i + 1) % nodes)
            .map(|i| ordered(i, (i + 1) % nodes))
            .collect();
        let extra_per = edges_per_node.saturating_sub(2);
        let full = (0..nodes).fold(ring, |acc, i| {
            (0..extra_per).fold(acc, |mut acc2, _| {
                let j = rng.gen_range(0..nodes.max(1));
                if j != i {
                    acc2.insert(ordered(i, j));
                }
                acc2
            })
        });
        Self {
            edges: full.into_iter().collect(),
        }
    }

    /// All undirected edges as ordered index pairs.
    pub fn edges(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.edges.iter().copied()
    }

    /// Number of edges.
    pub fn len(&self) -> usize {
        self.edges.len()
    }

    /// True when no edges exist.
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reachable_from_zero(t: &Topology, nodes: usize) -> usize {
        let start: BTreeSet<usize> = [0usize].into_iter().collect();
        (0..nodes)
            .fold(start, |acc, _| {
                let grown: BTreeSet<usize> = t
                    .edges()
                    .filter(|(a, b)| acc.contains(a) || acc.contains(b))
                    .flat_map(|(a, b)| [a, b])
                    .chain(acc.iter().copied())
                    .collect();
                grown
            })
            .len()
    }

    #[test]
    fn topology_is_connected_and_deterministic() -> Result<(), String> {
        let n = 20;
        let t = Topology::ring_plus_random(n, 6, 42);
        let t2 = Topology::ring_plus_random(n, 6, 42);
        let same: Vec<_> = t.edges().collect();
        let same2: Vec<_> = t2.edges().collect();
        (same == same2)
            .then_some(())
            .ok_or_else(|| "same seed produced different topologies".to_string())?;
        (reachable_from_zero(&t, n) == n)
            .then_some(())
            .ok_or_else(|| "topology is not connected".to_string())
    }

    #[test]
    fn no_self_loops() -> Result<(), String> {
        let t = Topology::ring_plus_random(10, 8, 7);
        let clean = t.edges().all(|(a, b)| a != b);
        clean
            .then_some(())
            .ok_or_else(|| "self-loop found".to_string())
    }
}
