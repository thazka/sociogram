mod common;

use common::{context, px, render, H, W};
use sociogram_core::{Camera, Graph, GraphData, GridLayoutConfig, LayoutConfig};
use sociogram_gpu::{instances_from_graph, GpuLayout};

/// The renderer draws straight from the layout's GPU buffer, including after
/// the layout has ping-ponged between its two position buffers.
#[test]
fn renderer_reads_positions_from_the_layout_buffer() {
    let Some(ctx) = context() else { return };
    let graph = Graph::new(GraphData {
        node_count: 2,
        edges: vec![[0, 1]],
        positions: Some(vec![[-20.0, 0.0], [20.0, 0.0]]),
        sizes: Some(vec![6.0, 6.0]),
        colors: Some(vec![[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]]),
    })
    .unwrap();

    // All forces off: positions must stay exactly where they started.
    let config = GridLayoutConfig {
        base: LayoutConfig {
            gravity: 0.0,
            repulsion: 0.0,
            link_spring: 0.0,
            alpha_decay: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut layout = GpuLayout::new(&ctx.device, &graph, config).unwrap();
    // An odd number of steps leaves the result in the second buffer.
    for _ in 0..3 {
        layout.step(&ctx.device, &ctx.queue);
    }

    let (nodes, links) = instances_from_graph(&graph, [0.0, 1.0, 0.0, 1.0], 2.0);
    let cam = Camera::new([W as f32, H as f32]);
    let data = render(&ctx, &cam, |r, device| {
        r.set_positions(device, layout.positions_buffer());
        r.set_nodes(device, &nodes);
        r.set_links(device, &links);
    });

    assert_eq!(px(&data, 12, 32), [255, 0, 0, 255], "node 0 at x=-20");
    assert_eq!(px(&data, 52, 32), [0, 0, 255, 255], "node 1 at x=+20");
    assert_eq!(px(&data, 32, 32), [0, 255, 0, 255], "link between them");
    assert_eq!(px(&data, 32, 5), [0, 0, 0, 255], "background");
    let _ = H;
}
