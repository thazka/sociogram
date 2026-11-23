use crate::error::GraphError;
use crate::rng::Rng;

pub const DEFAULT_SIZE: f32 = 5.0;
pub const DEFAULT_COLOR: [f32; 4] = [0.40, 0.60, 1.00, 1.00];
const INIT_SPREAD: f32 = 50.0;
const INIT_SEED: u64 = 42;

/// Raw, unvalidated input. Convert with [`Graph::new`].
#[derive(Debug, Clone, Default)]
pub struct GraphData {
    pub node_count: usize,
    pub edges: Vec<[u32; 2]>,
    pub sizes: Option<Vec<f32>>,
    pub colors: Option<Vec<[f32; 4]>>,
    pub positions: Option<Vec<[f32; 2]>>,
}

/// Validated graph. Adjacency is undirected CSR; self-loops are kept in
/// `edges` but excluded from neighbor lists.
#[derive(Debug, Clone)]
pub struct Graph {
    node_count: usize,
    edges: Vec<[u32; 2]>,
    offsets: Vec<u32>,
    neighbors: Vec<u32>,
    sizes: Vec<f32>,
    colors: Vec<[f32; 4]>,
    positions: Vec<[f32; 2]>,
}

fn check_len(
    field: &'static str,
    expected: usize,
    actual: Option<usize>,
) -> Result<(), GraphError> {
    match actual {
        Some(a) if a != expected => Err(GraphError::LengthMismatch {
            field,
            expected,
            actual: a,
        }),
        _ => Ok(()),
    }
}

impl Graph {
    pub fn new(data: GraphData) -> Result<Self, GraphError> {
        let n = data.node_count;

        // Indices and CSR offsets are u32; reject sizes that would overflow
        // before any allocation proportional to the input happens.
        if n > u32::MAX as usize {
            return Err(GraphError::TooLarge {
                field: "nodes",
                count: n,
            });
        }
        if data.edges.len().saturating_mul(2) > u32::MAX as usize {
            return Err(GraphError::TooLarge {
                field: "edges",
                count: data.edges.len(),
            });
        }

        check_len("sizes", n, data.sizes.as_ref().map(|v| v.len()))?;
        check_len("colors", n, data.colors.as_ref().map(|v| v.len()))?;
        check_len("positions", n, data.positions.as_ref().map(|v| v.len()))?;

        for (i, e) in data.edges.iter().enumerate() {
            for &idx in e {
                if idx as usize >= n {
                    return Err(GraphError::EdgeOutOfRange {
                        edge: i,
                        index: idx,
                        node_count: n,
                    });
                }
            }
        }
        if let Some(s) = &data.sizes {
            if let Some(i) = s.iter().position(|v| !v.is_finite()) {
                return Err(GraphError::NonFinite {
                    field: "sizes",
                    index: i,
                });
            }
        }
        if let Some(c) = &data.colors {
            if let Some(i) = c.iter().position(|v| v.iter().any(|x| !x.is_finite())) {
                return Err(GraphError::NonFinite {
                    field: "colors",
                    index: i,
                });
            }
        }
        if let Some(p) = &data.positions {
            if let Some(i) = p
                .iter()
                .position(|v| !v[0].is_finite() || !v[1].is_finite())
            {
                return Err(GraphError::NonFinite {
                    field: "positions",
                    index: i,
                });
            }
        }

        let mut offsets = vec![0u32; n + 1];
        for &[a, b] in &data.edges {
            if a != b {
                offsets[a as usize + 1] += 1;
                offsets[b as usize + 1] += 1;
            }
        }
        for i in 0..n {
            offsets[i + 1] += offsets[i];
        }
        let mut cursor = offsets.clone();
        let mut neighbors = vec![0u32; offsets[n] as usize];
        for &[a, b] in &data.edges {
            if a != b {
                neighbors[cursor[a as usize] as usize] = b;
                cursor[a as usize] += 1;
                neighbors[cursor[b as usize] as usize] = a;
                cursor[b as usize] += 1;
            }
        }

        let positions = data.positions.unwrap_or_else(|| {
            let mut rng = Rng::new(INIT_SEED);
            (0..n)
                .map(|_| {
                    [
                        (rng.next_f32() * 2.0 - 1.0) * INIT_SPREAD,
                        (rng.next_f32() * 2.0 - 1.0) * INIT_SPREAD,
                    ]
                })
                .collect()
        });

        Ok(Graph {
            node_count: n,
            edges: data.edges,
            offsets,
            neighbors,
            sizes: data.sizes.unwrap_or_else(|| vec![DEFAULT_SIZE; n]),
            colors: data.colors.unwrap_or_else(|| vec![DEFAULT_COLOR; n]),
            positions,
        })
    }

    pub fn node_count(&self) -> usize {
        self.node_count
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
    pub fn edges(&self) -> &[[u32; 2]] {
        &self.edges
    }
    pub fn neighbors_of(&self, node: usize) -> &[u32] {
        &self.neighbors[self.offsets[node] as usize..self.offsets[node + 1] as usize]
    }
    /// CSR row offsets (`node_count + 1` entries) into [`Graph::neighbors`].
    pub fn offsets(&self) -> &[u32] {
        &self.offsets
    }
    /// All adjacency lists back to back (CSR column indices).
    pub fn neighbors(&self) -> &[u32] {
        &self.neighbors
    }
    pub fn degree(&self, node: usize) -> usize {
        (self.offsets[node + 1] - self.offsets[node]) as usize
    }
    pub fn sizes(&self) -> &[f32] {
        &self.sizes
    }
    pub fn colors(&self) -> &[[f32; 4]] {
        &self.colors
    }
    pub fn positions(&self) -> &[[f32; 2]] {
        &self.positions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle() -> GraphData {
        GraphData {
            node_count: 3,
            edges: vec![[0, 1], [1, 2], [2, 0]],
            ..Default::default()
        }
    }

    #[test]
    fn builds_csr_for_triangle() {
        let g = Graph::new(triangle()).unwrap();
        assert_eq!(g.node_count(), 3);
        assert_eq!(g.edge_count(), 3);
        for i in 0..3 {
            assert_eq!(g.degree(i), 2);
            let mut n = g.neighbors_of(i).to_vec();
            n.sort();
            let expected: Vec<u32> = (0..3u32).filter(|&x| x != i as u32).collect();
            assert_eq!(n, expected);
        }
    }

    #[test]
    fn self_loops_are_kept_as_edges_but_not_neighbors() {
        let g = Graph::new(GraphData {
            node_count: 2,
            edges: vec![[0, 0], [0, 1]],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(g.edge_count(), 2);
        assert_eq!(g.neighbors_of(0), &[1]);
        assert_eq!(g.degree(1), 1);
    }

    #[test]
    fn rejects_edge_out_of_range() {
        let err = Graph::new(GraphData {
            node_count: 2,
            edges: vec![[0, 1], [1, 5]],
            ..Default::default()
        })
        .unwrap_err();
        assert_eq!(
            err,
            GraphError::EdgeOutOfRange {
                edge: 1,
                index: 5,
                node_count: 2
            }
        );
    }

    #[test]
    fn rejects_length_mismatch() {
        let err = Graph::new(GraphData {
            node_count: 3,
            sizes: Some(vec![1.0, 2.0]),
            ..Default::default()
        })
        .unwrap_err();
        assert_eq!(
            err,
            GraphError::LengthMismatch {
                field: "sizes",
                expected: 3,
                actual: 2
            }
        );
    }

    #[test]
    fn rejects_non_finite_position() {
        let err = Graph::new(GraphData {
            node_count: 2,
            positions: Some(vec![[0.0, 0.0], [f32::NAN, 1.0]]),
            ..Default::default()
        })
        .unwrap_err();
        assert_eq!(
            err,
            GraphError::NonFinite {
                field: "positions",
                index: 1
            }
        );
    }

    #[test]
    fn rejects_node_count_beyond_u32_before_allocating() {
        let err = Graph::new(GraphData {
            node_count: u32::MAX as usize + 1,
            ..Default::default()
        })
        .unwrap_err();
        assert_eq!(
            err,
            GraphError::TooLarge {
                field: "nodes",
                count: u32::MAX as usize + 1
            }
        );
    }

    #[test]
    fn fills_defaults_and_deterministic_positions() {
        let a = Graph::new(triangle()).unwrap();
        let b = Graph::new(triangle()).unwrap();
        assert_eq!(a.sizes(), &[DEFAULT_SIZE; 3]);
        assert_eq!(a.colors()[0], DEFAULT_COLOR);
        assert_eq!(a.positions(), b.positions());
        for p in a.positions() {
            assert!(p[0].abs() <= 50.0 && p[1].abs() <= 50.0);
        }
    }

    #[test]
    fn empty_graph_is_valid() {
        let g = Graph::new(GraphData::default()).unwrap();
        assert_eq!(g.node_count(), 0);
        assert_eq!(g.edge_count(), 0);
    }
}
