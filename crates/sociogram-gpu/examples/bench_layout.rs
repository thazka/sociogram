//! Layout throughput benchmark: `cargo run --release -p sociogram-gpu --example bench_layout`
//!
//! Optional arguments: `<nodes> <edges_per_node> <grid_dim>`.

use sociogram_core::rng::Rng;
use sociogram_core::{Graph, GraphData, GridLayoutConfig};
use sociogram_gpu::{GpuContext, GpuLayout};
use std::time::Instant;

fn arg<T: std::str::FromStr>(i: usize, default: T) -> T {
    std::env::args()
        .nth(i)
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let n: u32 = arg(1, 100_000);
    let per_node: u32 = arg(2, 5);
    let grid_dim: u32 = arg(3, 64);

    let ctx = pollster::block_on(GpuContext::new_headless()).expect("no GPU available");
    println!("adapter: {}", ctx.adapter.get_info().name);

    let mut rng = Rng::new(1);
    let edges: Vec<[u32; 2]> = (0..n * per_node)
        .map(|k| {
            let a = k / per_node;
            let b = (rng.next_u64() % n as u64) as u32;
            [a, b]
        })
        .collect();
    let graph = Graph::new(GraphData {
        node_count: n as usize,
        edges,
        ..Default::default()
    })
    .unwrap();

    let mut config = GridLayoutConfig {
        grid_dim,
        ..Default::default()
    };
    // Optional 4th argument overrides repulsion (0 skips the traversal, which
    // isolates the cost of the accumulate pass and springs).
    config.base.repulsion = arg(4, config.base.repulsion);
    let mut layout = GpuLayout::new(&ctx.device, &graph, config).unwrap();

    for _ in 0..5 {
        layout.step(&ctx.device, &ctx.queue);
    }
    layout.read_positions(&ctx.device, &ctx.queue); // sync

    let steps = 50;
    let t = Instant::now();
    for _ in 0..steps {
        layout.step(&ctx.device, &ctx.queue);
    }
    layout.read_positions(&ctx.device, &ctx.queue); // sync
    let ms = t.elapsed().as_secs_f64() * 1000.0 / steps as f64;

    println!(
        "{} nodes, {} edges, grid {}x{}: {:.2} ms/step ({:.1} steps/s)",
        graph.node_count(),
        graph.edge_count(),
        grid_dim,
        grid_dim,
        ms,
        1000.0 / ms
    );
}
