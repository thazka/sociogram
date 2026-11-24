//! Picking geometry and the selection set. Pure CPU code: everything here is
//! tested without a GPU.

use crate::Graph;
use std::fmt;

/// Nodes inside the rectangle spanned by two opposite corners (edges inclusive).
pub fn nodes_in_rect(positions: &[[f32; 2]], a: [f32; 2], b: [f32; 2]) -> Vec<u32> {
    let lo = [a[0].min(b[0]), a[1].min(b[1])];
    let hi = [a[0].max(b[0]), a[1].max(b[1])];
    positions
        .iter()
        .enumerate()
        .filter(|(_, p)| p[0] >= lo[0] && p[0] <= hi[0] && p[1] >= lo[1] && p[1] <= hi[1])
        .map(|(i, _)| i as u32)
        .collect()
}

/// Nodes inside a polygon (even-odd rule). Fewer than three vertices enclose nothing.
pub fn nodes_in_polygon(positions: &[[f32; 2]], polygon: &[[f32; 2]]) -> Vec<u32> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    let (mut lo, mut hi) = (polygon[0], polygon[0]);
    for q in polygon {
        lo = [lo[0].min(q[0]), lo[1].min(q[1])];
        hi = [hi[0].max(q[0]), hi[1].max(q[1])];
    }
    positions
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p[0] >= lo[0] && p[0] <= hi[0] && p[1] >= lo[1] && p[1] <= hi[1] && inside(p, polygon)
        })
        .map(|(i, _)| i as u32)
        .collect()
}

fn inside(p: &[f32; 2], polygon: &[[f32; 2]]) -> bool {
    let mut odd = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            odd = !odd;
        }
        j = i;
    }
    odd
}

fn dist2_to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if len2 <= f32::EPSILON {
        0.0
    } else {
        ((ap[0] * ab[0] + ap[1] * ab[1]) / len2).clamp(0.0, 1.0)
    };
    let d = [p[0] - (a[0] + ab[0] * t), p[1] - (a[1] + ab[1] * t)];
    d[0] * d[0] + d[1] * d[1]
}

/// Index (into `edges`) of the link closest to `p` within `max_dist`, if any.
pub fn nearest_edge(
    positions: &[[f32; 2]],
    edges: &[[u32; 2]],
    p: [f32; 2],
    max_dist: f32,
) -> Option<usize> {
    let mut best = None;
    let mut best_d2 = max_dist * max_dist;
    for (k, &[s, t]) in edges.iter().enumerate() {
        if s == t {
            continue;
        }
        let (Some(a), Some(b)) = (positions.get(s as usize), positions.get(t as usize)) else {
            continue;
        };
        let d2 = dist2_to_segment(p, *a, *b);
        if d2 <= best_d2 {
            best_d2 = d2;
            best = Some(k);
        }
    }
    best
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectOp {
    Replace,
    Add,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionError {
    NodeOutOfRange { index: u32, len: usize },
    EdgeOutOfRange { index: u32, len: usize },
}

impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectionError::NodeOutOfRange { index, len } => {
                write!(f, "node {index} is out of range ({len} nodes)")
            }
            SelectionError::EdgeOutOfRange { index, len } => {
                write!(f, "link {index} is out of range ({len} links)")
            }
        }
    }
}

impl std::error::Error for SelectionError {}

/// Selected nodes and links. Links are indexed by edge index (the order of the
/// `edges` given to the graph, self-loops included). A selection is "active"
/// from the first select call until `clear`, even when nothing is selected
/// (an empty active selection dims everything).
#[derive(Debug, Clone)]
pub struct Selection {
    nodes: Vec<bool>,
    links: Vec<bool>,
    active: bool,
}

impl Selection {
    pub fn new(node_count: usize, edge_count: usize) -> Self {
        Selection {
            nodes: vec![false; node_count],
            links: vec![false; edge_count],
            active: false,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }
    pub fn node_flags(&self) -> &[bool] {
        &self.nodes
    }
    pub fn link_flags(&self) -> &[bool] {
        &self.links
    }
    pub fn node_count_selected(&self) -> usize {
        self.nodes.iter().filter(|&&f| f).count()
    }
    /// Selected links, self-loops included (they are never drawn).
    pub fn link_count_selected(&self) -> usize {
        self.links.iter().filter(|&&f| f).count()
    }
    pub fn selected_nodes(&self) -> Vec<u32> {
        (0..self.nodes.len() as u32)
            .filter(|&i| self.nodes[i as usize])
            .collect()
    }

    pub fn clear(&mut self) {
        self.nodes.fill(false);
        self.links.fill(false);
        self.active = false;
    }

    pub fn apply_nodes(
        &mut self,
        graph: &Graph,
        indices: &[u32],
        op: SelectOp,
    ) -> Result<(), SelectionError> {
        let n = self.nodes.len();
        if let Some(&index) = indices.iter().find(|&&i| i as usize >= n) {
            return Err(SelectionError::NodeOutOfRange { index, len: n });
        }
        match op {
            SelectOp::Replace => {
                self.nodes.fill(false);
                self.links.fill(false);
                self.add_nodes(graph, indices);
            }
            SelectOp::Add => self.add_nodes(graph, indices),
            SelectOp::Remove => {
                if !self.active {
                    return Ok(());
                }
                let mut gone = vec![false; n];
                for &i in indices {
                    gone[i as usize] = true;
                    self.nodes[i as usize] = false;
                }
                for (k, e) in graph.edges().iter().enumerate() {
                    if gone[e[0] as usize] || gone[e[1] as usize] {
                        self.links[k] = false;
                    }
                }
            }
        }
        self.active = true;
        Ok(())
    }

    fn add_nodes(&mut self, graph: &Graph, indices: &[u32]) {
        let before = self.nodes.clone();
        let mut fresh = vec![false; self.nodes.len()];
        for &i in indices {
            fresh[i as usize] = true;
            self.nodes[i as usize] = true;
        }
        for (k, e) in graph.edges().iter().enumerate() {
            let (a, b) = (e[0] as usize, e[1] as usize);
            if (fresh[a] && (fresh[b] || before[b])) || (fresh[b] && before[a]) {
                self.links[k] = true;
            }
        }
    }

    /// Select, add or remove one link. With `with_endpoints` the two end nodes go
    /// along; removing then also removes every link attached to those nodes.
    pub fn apply_link(
        &mut self,
        graph: &Graph,
        edge: u32,
        op: SelectOp,
        with_endpoints: bool,
    ) -> Result<(), SelectionError> {
        let m = self.links.len();
        if edge as usize >= m {
            return Err(SelectionError::EdgeOutOfRange {
                index: edge,
                len: m,
            });
        }
        let [a, b] = graph.edges()[edge as usize];
        match op {
            SelectOp::Replace | SelectOp::Add => {
                if op == SelectOp::Replace {
                    self.nodes.fill(false);
                    self.links.fill(false);
                }
                self.links[edge as usize] = true;
                if with_endpoints {
                    // Same rule as selecting nodes: links among the endpoints and
                    // links bridging them to the earlier selection come along.
                    self.add_nodes(graph, &[a, b]);
                }
            }
            SelectOp::Remove => {
                if !self.active {
                    return Ok(());
                }
                self.links[edge as usize] = false;
                if with_endpoints {
                    self.nodes[a as usize] = false;
                    self.nodes[b as usize] = false;
                    for (k, e) in graph.edges().iter().enumerate() {
                        if [a, b].contains(&e[0]) || [a, b].contains(&e[1]) {
                            self.links[k] = false;
                        }
                    }
                }
            }
        }
        self.active = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Graph, GraphData};

    fn path4() -> Graph {
        Graph::new(GraphData {
            node_count: 4,
            edges: vec![[0, 1], [1, 2], [2, 3]],
            ..Default::default()
        })
        .unwrap()
    }

    fn links(s: &Selection) -> Vec<usize> {
        s.link_flags()
            .iter()
            .enumerate()
            .filter(|(_, &f)| f)
            .map(|(i, _)| i)
            .collect()
    }

    #[test]
    fn rect_is_inclusive_and_corner_order_does_not_matter() {
        let p = [[0.0, 0.0], [10.0, 10.0], [5.0, 5.0], [11.0, 5.0]];
        assert_eq!(nodes_in_rect(&p, [0.0, 0.0], [10.0, 10.0]), vec![0, 1, 2]);
        assert_eq!(nodes_in_rect(&p, [10.0, 10.0], [0.0, 0.0]), vec![0, 1, 2]);
        assert!(nodes_in_rect(&[], [0.0, 0.0], [1.0, 1.0]).is_empty());
    }

    #[test]
    fn polygon_handles_concave_shapes_and_degenerate_input() {
        // An L shape: the notch (7, 7) is outside.
        let l = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 5.0],
            [5.0, 5.0],
            [5.0, 10.0],
            [0.0, 10.0],
        ];
        let pts = [[2.0, 2.0], [2.0, 8.0], [8.0, 2.0], [7.0, 7.0], [20.0, 20.0]];
        assert_eq!(nodes_in_polygon(&pts, &l), vec![0, 1, 2]);
        assert!(
            nodes_in_polygon(&pts, &l[..2]).is_empty(),
            "two vertices enclose nothing"
        );
        assert!(nodes_in_polygon(&pts, &[]).is_empty());
    }

    #[test]
    fn nearest_edge_picks_the_closest_within_range_and_skips_loops() {
        let pos = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        let edges = [[0, 1], [1, 2], [2, 2], [0, 9]];
        assert_eq!(nearest_edge(&pos, &edges, [5.0, 1.0], 2.0), Some(0));
        assert_eq!(nearest_edge(&pos, &edges, [9.0, 5.0], 2.0), Some(1));
        assert_eq!(
            nearest_edge(&pos, &edges, [5.0, 5.0], 2.0),
            None,
            "out of range"
        );
        assert_eq!(
            nearest_edge(&pos, &edges, [10.0, 10.0], 0.5),
            Some(1),
            "loop 2-2 is ignored"
        );
    }

    #[test]
    fn replace_selects_nodes_and_the_links_between_them() {
        let g = path4();
        let mut s = Selection::new(4, 3);
        assert!(!s.is_active());
        s.apply_nodes(&g, &[0, 1], SelectOp::Replace).unwrap();
        assert!(s.is_active());
        assert_eq!(s.selected_nodes(), vec![0, 1]);
        assert_eq!(links(&s), vec![0]);
    }

    #[test]
    fn add_also_picks_up_links_bridging_to_the_previous_selection() {
        let g = path4();
        let mut s = Selection::new(4, 3);
        s.apply_nodes(&g, &[0, 1], SelectOp::Replace).unwrap();
        s.apply_nodes(&g, &[3], SelectOp::Add).unwrap();
        assert_eq!(links(&s), vec![0], "3 is not adjacent to the old selection");
        s.apply_nodes(&g, &[2], SelectOp::Add).unwrap();
        assert_eq!(s.selected_nodes(), vec![0, 1, 2, 3]);
        assert_eq!(links(&s), vec![0, 1, 2], "2 bridges to both 1 and 3");
    }

    #[test]
    fn remove_drops_the_nodes_and_every_link_attached_to_them() {
        let g = path4();
        let mut s = Selection::new(4, 3);
        s.apply_nodes(&g, &[0, 1, 2, 3], SelectOp::Replace).unwrap();
        s.apply_nodes(&g, &[1], SelectOp::Remove).unwrap();
        assert_eq!(s.selected_nodes(), vec![0, 2, 3]);
        assert_eq!(links(&s), vec![2]);
    }

    #[test]
    fn an_empty_selection_is_active_and_clear_makes_it_inactive() {
        let g = path4();
        let mut s = Selection::new(4, 3);
        s.apply_nodes(&g, &[], SelectOp::Replace).unwrap();
        assert!(s.is_active());
        assert_eq!(s.node_count_selected(), 0);
        s.clear();
        assert!(!s.is_active());
    }

    #[test]
    fn remove_without_a_selection_does_nothing() {
        let g = path4();
        let mut s = Selection::new(4, 3);
        s.apply_nodes(&g, &[1], SelectOp::Remove).unwrap();
        assert!(!s.is_active());
    }

    #[test]
    fn selecting_a_link_selects_its_endpoints_unless_single() {
        let g = path4();
        let mut s = Selection::new(4, 3);
        s.apply_link(&g, 1, SelectOp::Replace, true).unwrap();
        assert_eq!(s.selected_nodes(), vec![1, 2]);
        assert_eq!(links(&s), vec![1]);
        s.apply_link(&g, 1, SelectOp::Replace, false).unwrap();
        assert!(s.selected_nodes().is_empty());
        assert_eq!(links(&s), vec![1]);
    }

    #[test]
    fn unselecting_a_link_removes_endpoints_and_the_links_attached_to_them() {
        let g = path4();
        let mut s = Selection::new(4, 3);
        s.apply_nodes(&g, &[0, 1, 2, 3], SelectOp::Replace).unwrap();
        s.apply_link(&g, 1, SelectOp::Remove, true).unwrap();
        assert_eq!(s.selected_nodes(), vec![0, 3]);
        assert!(
            links(&s).is_empty(),
            "links 0, 1 and 2 all touch node 1 or 2"
        );
    }

    #[test]
    fn out_of_range_input_is_an_error_and_changes_nothing() {
        let g = path4();
        let mut s = Selection::new(4, 3);
        s.apply_nodes(&g, &[1], SelectOp::Replace).unwrap();
        assert_eq!(
            s.apply_nodes(&g, &[0, 4], SelectOp::Replace),
            Err(SelectionError::NodeOutOfRange { index: 4, len: 4 })
        );
        assert_eq!(
            s.apply_link(&g, 3, SelectOp::Add, true),
            Err(SelectionError::EdgeOutOfRange { index: 3, len: 3 })
        );
        assert_eq!(s.selected_nodes(), vec![1]);
    }
}
