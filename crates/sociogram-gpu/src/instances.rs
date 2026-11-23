use bytemuck::{Pod, Zeroable};
use sociogram_core::Graph;

/// Per-node instance data. Node `i` is drawn at `positions[i]` of the bound
/// positions buffer. Field order and sizes must match `NodeIn` in
/// `shader.wgsl` and the attributes in `renderer.rs`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct NodeInstance {
    /// Radius in pixels at zoom 1; rendered radius is `size * sqrt(zoom)` pixels (min 1 px).
    pub size: f32,
    pub color: [f32; 4],
}

/// Per-link instance data. `src` and `dst` index the bound positions buffer;
/// `width` is in pixels.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct LinkInstance {
    pub src: u32,
    pub dst: u32,
    pub color: [f32; 4],
    pub width: f32,
}

pub fn instances_from_graph(
    graph: &Graph,
    link_color: [f32; 4],
    link_width: f32,
) -> (Vec<NodeInstance>, Vec<LinkInstance>) {
    let nodes = (0..graph.node_count())
        .map(|i| NodeInstance {
            size: graph.sizes()[i],
            color: graph.colors()[i],
        })
        .collect();
    let links = graph
        .edges()
        .iter()
        .filter(|e| e[0] != e[1])
        .map(|&[src, dst]| LinkInstance {
            src,
            dst,
            color: link_color,
            width: link_width,
        })
        .collect();
    (nodes, links)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sociogram_core::{Graph, GraphData};

    #[test]
    fn instance_layouts_have_no_hidden_padding() {
        // vertex_attr_array! packs attributes back to back: 4 + 16.
        assert_eq!(std::mem::size_of::<NodeInstance>(), 20);
        // 4 + 4 + 16 + 4.
        assert_eq!(std::mem::size_of::<LinkInstance>(), 28);
    }

    #[test]
    fn builds_one_node_per_node_and_one_link_per_edge() {
        let g = Graph::new(GraphData {
            node_count: 3,
            edges: vec![[0, 1], [1, 2]],
            sizes: Some(vec![1.0, 2.0, 3.0]),
            ..Default::default()
        })
        .unwrap();
        let (nodes, links) = instances_from_graph(&g, [0.5, 0.5, 0.5, 1.0], 2.0);
        assert_eq!(nodes.len(), 3);
        assert_eq!(links.len(), 2);
        assert_eq!(nodes[2].size, 3.0);
        assert_eq!((links[1].src, links[1].dst), (1, 2));
        assert_eq!(links[0].width, 2.0);
    }

    #[test]
    fn self_loops_produce_no_link() {
        let g = Graph::new(GraphData {
            node_count: 2,
            edges: vec![[0, 0], [0, 1]],
            ..Default::default()
        })
        .unwrap();
        let (_, links) = instances_from_graph(&g, [1.0; 4], 1.0);
        assert_eq!(links.len(), 1);
    }
}
