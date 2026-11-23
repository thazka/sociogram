use sociogram_core::rng::Rng;
use sociogram_core::{Graph, GraphData, GridLayout, GridLayoutConfig, LayoutConfig};
use sociogram_gpu::{GpuContext, GpuError, GpuLayout, MAX_GPU_NODES};

fn context() -> Option<GpuContext> {
    match pollster::block_on(GpuContext::new_headless()) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("skipping GPU test: {e}");
            None
        }
    }
}

fn pure(spring: f32, gravity: f32, repulsion: f32) -> GridLayoutConfig {
    GridLayoutConfig {
        base: LayoutConfig {
            gravity,
            repulsion,
            link_spring: spring,
            link_distance: 30.0,
            friction: 0.2,
            alpha_decay: 0.0,
            alpha_min: 0.001,
        },
        ..Default::default()
    }
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Random graph with positions snapped to multiples of 0.25, so cell
/// assignment is exact in f32 on both CPU and GPU (cell size 32 is a power of two).
fn snapped_graph(n: u32) -> Graph {
    let mut rng = Rng::new(7);
    let positions: Vec<[f32; 2]> = (0..n)
        .map(|_| {
            let x = ((rng.next_f32() * 400.0 - 200.0) * 4.0).round() / 4.0;
            let y = ((rng.next_f32() * 400.0 - 200.0) * 4.0).round() / 4.0;
            [x, y]
        })
        .collect();
    let mut edges: Vec<[u32; 2]> = (0..n).map(|i| [i, (i + 1) % n]).collect();
    edges.extend((0..n).map(|i| [i, (i * 5 + 3) % n]));
    Graph::new(GraphData {
        node_count: n as usize,
        edges,
        positions: Some(positions),
        ..Default::default()
    })
    .unwrap()
}

fn max_abs_diff(a: &[[f32; 2]], b: &[[f32; 2]]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(p, q)| (p[0] - q[0]).abs().max((p[1] - q[1]).abs()))
        .fold(0.0, f32::max)
}

fn compare_with_oracle(steps: usize, tolerance: f32) {
    let Some(ctx) = context() else { return };
    let graph = snapped_graph(200);
    let config = GridLayoutConfig::default();
    let mut gpu = GpuLayout::new(&ctx.device, &graph, config).unwrap();
    let mut cpu = GridLayout::new(&graph, config).unwrap();
    for _ in 0..steps {
        gpu.step(&ctx.device, &ctx.queue);
        cpu.step(&graph);
    }
    let got = gpu.read_positions(&ctx.device, &ctx.queue);
    let diff = max_abs_diff(&got, cpu.positions());
    assert!(
        diff < tolerance,
        "GPU and CPU oracle differ by {diff} after {steps} step(s)"
    );
}

#[test]
fn one_step_matches_cpu_oracle() {
    compare_with_oracle(1, 1e-3);
}

#[test]
fn five_steps_match_cpu_oracle() {
    compare_with_oracle(5, 1e-2);
}

#[test]
fn spring_converges_to_link_distance_on_gpu() {
    let Some(ctx) = context() else { return };
    let graph = Graph::new(GraphData {
        node_count: 2,
        edges: vec![[0, 1]],
        positions: Some(vec![[0.0, 0.0], [1.0, 0.0]]),
        ..Default::default()
    })
    .unwrap();
    let mut gpu = GpuLayout::new(&ctx.device, &graph, pure(0.1, 0.0, 0.0)).unwrap();
    for _ in 0..500 {
        gpu.step(&ctx.device, &ctx.queue);
    }
    let p = gpu.read_positions(&ctx.device, &ctx.queue);
    assert!((dist(p[0], p[1]) - 30.0).abs() < 0.01);
}

#[test]
fn positions_stay_inside_the_space() {
    let Some(ctx) = context() else { return };
    let graph = Graph::new(GraphData {
        node_count: 2,
        positions: Some(vec![[1023.0, 0.0], [1023.5, 0.0]]),
        ..Default::default()
    })
    .unwrap();
    let mut gpu = GpuLayout::new(&ctx.device, &graph, pure(0.0, 0.0, 1.0e6)).unwrap();
    for _ in 0..20 {
        gpu.step(&ctx.device, &ctx.queue);
    }
    for p in gpu.read_positions(&ctx.device, &ctx.queue) {
        assert!(p[0].is_finite() && p[1].is_finite());
        assert!(p[0].abs() <= 1024.0 && p[1].abs() <= 1024.0);
    }
}

#[test]
fn empty_graph_steps_without_panicking() {
    let Some(ctx) = context() else { return };
    let graph = Graph::new(GraphData::default()).unwrap();
    let mut gpu = GpuLayout::new(&ctx.device, &graph, GridLayoutConfig::default()).unwrap();
    gpu.step(&ctx.device, &ctx.queue);
    assert!(gpu.read_positions(&ctx.device, &ctx.queue).is_empty());
}

#[test]
fn graph_without_edges_works() {
    let Some(ctx) = context() else { return };
    let graph = Graph::new(GraphData {
        node_count: 10,
        ..Default::default()
    })
    .unwrap();
    let mut gpu = GpuLayout::new(&ctx.device, &graph, GridLayoutConfig::default()).unwrap();
    gpu.step(&ctx.device, &ctx.queue);
    let p = gpu.read_positions(&ctx.device, &ctx.queue);
    assert_eq!(p.len(), 10);
    assert!(p.iter().all(|v| v[0].is_finite() && v[1].is_finite()));
}

#[test]
fn rejects_too_many_nodes_and_bad_settings() {
    let Some(ctx) = context() else { return };
    let big = Graph::new(GraphData {
        node_count: MAX_GPU_NODES + 1,
        ..Default::default()
    })
    .unwrap();
    assert!(matches!(
        GpuLayout::new(&ctx.device, &big, GridLayoutConfig::default()),
        Err(GpuError::InvalidInput(_))
    ));

    let small = Graph::new(GraphData {
        node_count: 1,
        ..Default::default()
    })
    .unwrap();
    for bad in [
        GridLayoutConfig {
            grid_dim: 0,
            ..Default::default()
        },
        GridLayoutConfig {
            grid_dim: 2,
            ..Default::default()
        },
        GridLayoutConfig {
            grid_dim: 100,
            ..Default::default()
        },
        GridLayoutConfig {
            grid_dim: 512,
            ..Default::default()
        },
        GridLayoutConfig {
            space_half: f32::NAN,
            ..Default::default()
        },
        GridLayoutConfig {
            space_half: -1.0,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            GpuLayout::new(&ctx.device, &small, bad),
            Err(GpuError::InvalidInput(_))
        ));
    }
}
