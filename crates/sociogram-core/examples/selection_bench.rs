//! Timing of the selection operations at 100,000 nodes and 300,000 links.
//! Run: `cargo run --release -p sociogram-core --example selection_bench`

use sociogram_core::rng::Rng;
use sociogram_core::{
    nearest_edge, nodes_in_polygon, nodes_in_rect, Graph, GraphData, SelectOp, Selection,
};
use std::time::Instant;

fn main() {
    let n = 100_000u32;
    let mut rng = Rng::new(1);
    let positions: Vec<[f32; 2]> = (0..n)
        .map(|_| {
            [
                rng.next_f32() * 2000.0 - 1000.0,
                rng.next_f32() * 2000.0 - 1000.0,
            ]
        })
        .collect();
    let edges: Vec<[u32; 2]> = (0..n * 3)
        .map(|i| [i % n, ((rng.next_f32() * n as f32) as u32).min(n - 1)])
        .collect();
    let graph = Graph::new(GraphData {
        node_count: n as usize,
        edges: edges.clone(),
        positions: Some(positions.clone()),
        ..Default::default()
    })
    .unwrap();

    let time = |label: &str, f: &mut dyn FnMut()| {
        let t = Instant::now();
        f();
        println!("{label}: {:.2} ms", t.elapsed().as_secs_f64() * 1000.0);
    };
    time("rectangle over 100k nodes", &mut || {
        let _ = nodes_in_rect(&positions, [-500.0, -500.0], [500.0, 500.0]);
    });
    let polygon: Vec<[f32; 2]> = (0..64)
        .map(|k| {
            let a = k as f32 / 64.0 * std::f32::consts::TAU;
            [a.cos() * 800.0, a.sin() * 800.0]
        })
        .collect();
    time("lasso with 64 vertices over 100k nodes", &mut || {
        let _ = nodes_in_polygon(&positions, &polygon);
    });
    for vertices in [1000usize, 5000] {
        let big: Vec<[f32; 2]> = (0..vertices)
            .map(|k| {
                let a = k as f32 / vertices as f32 * std::f32::consts::TAU;
                [a.cos() * 800.0, a.sin() * 800.0]
            })
            .collect();
        time(
            &format!("lasso with {vertices} vertices over 100k nodes"),
            &mut || {
                let _ = nodes_in_polygon(&positions, &big);
            },
        );
    }
    let first: Vec<u32> = (0..n / 2).collect();
    let more: Vec<u32> = (n / 2..n / 2 + n / 4).collect();
    let mut selection = Selection::new(graph.node_count(), graph.edge_count());
    time("select 50,000 nodes (replace)", &mut || {
        selection
            .apply_nodes(&graph, &first, SelectOp::Replace)
            .unwrap();
    });
    time("add 25,000 more nodes", &mut || {
        selection.apply_nodes(&graph, &more, SelectOp::Add).unwrap();
    });
    time("nearest link over 300,000 links", &mut || {
        let _ = nearest_edge(&positions, &edges, [0.0, 0.0], 5.0);
    });
}
