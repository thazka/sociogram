mod common;

use common::{context, finish_capture, frozen, target, two_nodes, FORMAT, H, W};
use sociogram_core::{GraphData, GraphError};
use sociogram_gpu::GpuContext;
use sociogram_web::{Scene, SceneError, SceneOptions};

fn read_snapshot(ctx: &GpuContext, scene: &mut Scene) -> bool {
    // On native targets the "layout finished" callback only runs when the
    // device is polled; the scene does not request a snapshot while it still
    // believes the GPU is busy.
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    scene.request_snapshot(&ctx.device, &ctx.queue);
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    scene.poll_snapshot()
}

#[test]
fn invalid_data_is_rejected_and_the_previous_graph_stays() {
    let Some(ctx) = context() else { return };
    let mut scene = Scene::new(&ctx.device, FORMAT, [W, H], frozen());
    scene.set_data(&ctx.device, two_nodes()).unwrap();
    assert_eq!(scene.node_count(), 2);

    let bad = GraphData {
        node_count: 2,
        edges: vec![[0, 7]],
        ..Default::default()
    };
    match scene.set_data(&ctx.device, bad) {
        Err(SceneError::Graph(GraphError::EdgeOutOfRange { index: 7, .. })) => {}
        other => panic!("expected EdgeOutOfRange, got {other:?}"),
    }
    assert_eq!(scene.node_count(), 2, "previous graph must stay in place");
}

#[test]
fn gpu_limits_are_reported_as_errors() {
    let Some(ctx) = context() else { return };
    let mut scene = Scene::new(&ctx.device, FORMAT, [W, H], SceneOptions::default());
    let too_big = GraphData {
        node_count: sociogram_gpu::MAX_GPU_NODES + 1,
        ..Default::default()
    };
    assert!(matches!(
        scene.set_data(&ctx.device, too_big),
        Err(SceneError::Gpu(_))
    ));
}

#[test]
fn empty_graph_is_accepted_and_renders() {
    let Some(ctx) = context() else { return };
    let mut scene = Scene::new(&ctx.device, FORMAT, [W, H], SceneOptions::default());
    scene.set_data(&ctx.device, GraphData::default()).unwrap();
    assert_eq!(scene.node_count(), 0);
    assert!(scene.is_settled());
    let view = target(&ctx);
    scene.frame(&ctx.device, &ctx.queue, &view);
    scene.request_snapshot(&ctx.device, &ctx.queue);
    assert!(!scene.poll_snapshot());
    assert_eq!(scene.pick(10.0, 10.0), None);
}

#[test]
fn pick_finds_the_node_under_the_pointer() {
    let Some(ctx) = context() else { return };
    let mut scene = Scene::new(&ctx.device, FORMAT, [W, H], frozen());
    scene.set_data(&ctx.device, two_nodes()).unwrap();
    let view = target(&ctx);
    scene.frame(&ctx.device, &ctx.queue, &view);
    assert!(read_snapshot(&ctx, &mut scene));

    let screen = scene.camera().world_to_screen([30.0, 0.0]);
    assert_eq!(scene.pick(screen[0], screen[1]), Some(1));
    let screen = scene.camera().world_to_screen([-30.0, 0.0]);
    assert_eq!(scene.pick(screen[0] + 2.0, screen[1] - 2.0), Some(0));
    assert_eq!(scene.pick(5.0, 5.0), None);
}

#[test]
fn layout_moves_nodes_and_the_snapshot_follows() {
    let Some(ctx) = context() else { return };
    let mut scene = Scene::new(&ctx.device, FORMAT, [W, H], SceneOptions::default());
    let n = 20u32;
    let data = GraphData {
        node_count: n as usize,
        edges: (0..n).map(|i| [i, (i + 1) % n]).collect(),
        ..Default::default()
    };
    scene.set_data(&ctx.device, data).unwrap();
    let before = scene.snapshot().to_vec();
    let view = target(&ctx);
    for _ in 0..30 {
        scene.frame(&ctx.device, &ctx.queue, &view);
    }
    assert!(read_snapshot(&ctx, &mut scene));
    let after = scene.snapshot();
    assert_eq!(after.len(), before.len());
    assert!(after.iter().all(|p| p[0].is_finite() && p[1].is_finite()));
    assert_ne!(
        after,
        before.as_slice(),
        "layout should have moved the nodes"
    );
}

#[test]
fn a_second_readback_waits_for_the_first() {
    let Some(ctx) = context() else { return };
    let mut scene = Scene::new(&ctx.device, FORMAT, [W, H], frozen());
    scene.set_data(&ctx.device, two_nodes()).unwrap();
    scene.request_snapshot(&ctx.device, &ctx.queue);
    scene.request_snapshot(&ctx.device, &ctx.queue); // ignored while in flight
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    assert!(scene.poll_snapshot());
    assert!(!scene.poll_snapshot(), "nothing pending after the result");
}

#[test]
fn interacting_stops_auto_fit() {
    let Some(ctx) = context() else { return };
    let mut scene = Scene::new(&ctx.device, FORMAT, [W, H], frozen());
    scene.set_data(&ctx.device, two_nodes()).unwrap();

    // Auto-fit follows the viewport.
    scene.resize(400.0, 200.0);
    let fitted = scene.camera().zoom;
    scene.resize(200.0, 100.0);
    assert_ne!(scene.camera().zoom, fitted);

    // After a zoom the user owns the camera.
    scene.zoom_at(2.0, 100.0, 50.0);
    let zoom = scene.camera().zoom;
    scene.resize(400.0, 200.0);
    assert_eq!(scene.camera().zoom, zoom);
}

#[test]
fn capture_contains_the_drawn_graph_independent_of_presentation() {
    let Some(ctx) = context() else { return };
    let mut scene = Scene::new(&ctx.device, FORMAT, [W, H], frozen());
    scene.set_data(&ctx.device, two_nodes()).unwrap();
    // Note: no frame() call. A capture renders its own frame.
    scene.request_capture(&ctx.device, &ctx.queue).unwrap();
    let capture = finish_capture(&ctx, &mut scene);

    assert_eq!((capture.width, capture.height), (W as u32, H as u32));
    assert_eq!(capture.rgba.len(), (W * H) as usize * 4);
    let background = SceneOptions::default().background;
    let bg = [
        (background.r * 255.0).round() as u8,
        (background.g * 255.0).round() as u8,
        (background.b * 255.0).round() as u8,
    ];
    let px = |x: usize, y: usize| {
        let i = (y * W as usize + x) * 4;
        [capture.rgba[i], capture.rgba[i + 1], capture.rgba[i + 2]]
    };
    assert_eq!(px(1, 1), bg, "corner is background");
    let node = scene.camera().world_to_screen([30.0, 0.0]);
    let lit = px(node[0] as usize, node[1] as usize);
    assert_ne!(lit, bg, "node center is drawn");
}

#[test]
fn only_one_capture_at_a_time_and_bad_sizes_are_rejected() {
    let Some(ctx) = context() else { return };
    let mut scene = Scene::new(&ctx.device, FORMAT, [W, H], frozen());
    scene.set_data(&ctx.device, two_nodes()).unwrap();
    scene.request_capture(&ctx.device, &ctx.queue).unwrap();
    assert!(scene.request_capture(&ctx.device, &ctx.queue).is_err());
    let _ = finish_capture(&ctx, &mut scene);
    // The slot is free again.
    assert!(scene.request_capture(&ctx.device, &ctx.queue).is_ok());
    let _ = finish_capture(&ctx, &mut scene);

    scene.resize(sociogram_web::MAX_CAPTURE_SIDE as f32 + 1.0, 100.0);
    assert!(scene.request_capture(&ctx.device, &ctx.queue).is_err());
}

#[test]
fn capture_handles_widths_that_need_row_padding() {
    let Some(ctx) = context() else { return };
    // 101 px * 4 bytes is not a multiple of 256, so rows are padded in the copy.
    let mut scene = Scene::new(&ctx.device, FORMAT, [101.0, 37.0], frozen());
    scene.set_data(&ctx.device, two_nodes()).unwrap();
    scene.request_capture(&ctx.device, &ctx.queue).unwrap();
    let capture = finish_capture(&ctx, &mut scene);
    assert_eq!((capture.width, capture.height), (101, 37));
    assert_eq!(capture.rgba.len(), 101 * 37 * 4);
}

#[test]
fn snapshots_can_be_requested_while_the_layout_is_running() {
    let Some(ctx) = context() else { return };
    let mut scene = Scene::new(&ctx.device, FORMAT, [W, H], SceneOptions::default());
    let n = 200u32;
    scene
        .set_data(
            &ctx.device,
            GraphData {
                node_count: n as usize,
                edges: (0..n).map(|i| [i, (i + 1) % n]).collect(),
                ..Default::default()
            },
        )
        .unwrap();
    let view = target(&ctx);
    scene.frame(&ctx.device, &ctx.queue, &view);
    // No device poll in between: the layout is still "busy" as far as the
    // scene knows, but a snapshot must still be accepted.
    scene.request_snapshot(&ctx.device, &ctx.queue);
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    assert!(scene.poll_snapshot());
}
