mod common;

use common::{context, finish_capture, frozen, target, two_nodes, FORMAT, H, W};
use sociogram_core::{Camera, Graph, GraphData, SelectOp};
use sociogram_gpu::{instances_from_graph, GpuContext, Renderer};
use sociogram_web::{Scene, SceneError, SelectHit, MAX_INDEX_LIST, MAX_POLYGON_VERTICES};

fn render_rgba(ctx: &GpuContext, renderer: &Renderer) -> Vec<u8> {
    let texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    // Rows copied to a buffer must start on a multiple of 256 bytes.
    let padded = (W as u32 * 4).div_ceil(256) * 256;
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded * H as u32) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = ctx.device.create_command_encoder(&Default::default());
    renderer.render(&mut enc, &view, wgpu::Color::BLACK);
    enc.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(H as u32),
            },
        },
        wgpu::Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        },
    );
    ctx.queue.submit([enc.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    let mut out = Vec::with_capacity((W as usize) * (H as usize) * 4);
    for row in 0..H as usize {
        let start = row * padded as usize;
        out.extend_from_slice(&mapped[start..start + (W as usize) * 4]);
    }
    out
}

fn brightness(rgba: &[u8], x: usize, y: usize) -> u32 {
    let i = (y * W as usize + x) * 4;
    rgba[i] as u32 + rgba[i + 1] as u32 + rgba[i + 2] as u32
}

#[test]
fn unselected_nodes_are_dimmed_and_selected_ones_are_not() {
    let Some(ctx) = context() else { return };
    let graph = Graph::new(GraphData {
        node_count: 2,
        edges: vec![[0, 1]],
        positions: Some(vec![[-30.0, 0.0], [30.0, 0.0]]),
        sizes: Some(vec![8.0, 8.0]),
        colors: Some(vec![[1.0, 1.0, 1.0, 1.0]; 2]),
    })
    .unwrap();
    let (nodes, links) = instances_from_graph(&graph, [1.0, 1.0, 1.0, 1.0], 1.0);
    let mut renderer = Renderer::new(&ctx.device, FORMAT);
    renderer.set_nodes(&ctx.device, &nodes);
    renderer.set_links(&ctx.device, &links);
    let positions = Renderer::upload_positions(&ctx.device, graph.positions());
    renderer.set_positions(&ctx.device, &positions);
    let mut camera = Camera::new([W, H]);
    camera.fit_bounds([-30.0, 0.0], [30.0, 0.0], 40.0);
    renderer.set_camera(&ctx.queue, &camera);

    let at = |p: [f32; 2]| {
        let s = camera.world_to_screen(p);
        (s[0] as usize, s[1] as usize)
    };
    let (ax, ay) = at([-30.0, 0.0]);
    let (bx, by) = at([30.0, 0.0]);

    let plain = render_rgba(&ctx, &renderer);
    let (a0, b0) = (brightness(&plain, ax, ay), brightness(&plain, bx, by));
    assert!(
        a0 > 600 && b0 > 600,
        "both nodes are bright without a selection"
    );

    renderer.set_selection(&ctx.device, &ctx.queue, &[1, 0], &[0]);
    renderer.set_camera(&ctx.queue, &camera);
    let dimmed = render_rgba(&ctx, &renderer);
    assert_eq!(
        brightness(&dimmed, ax, ay),
        a0,
        "the selected node is unchanged"
    );
    assert!(
        brightness(&dimmed, bx, by) < b0 / 2,
        "the unselected node is dimmed"
    );

    renderer.set_selection(&ctx.device, &ctx.queue, &[0, 0], &[0]);
    renderer.set_camera(&ctx.queue, &camera);
    let empty = render_rgba(&ctx, &renderer);
    assert!(
        brightness(&empty, ax, ay) < a0 / 2,
        "an empty selection dims everything"
    );

    renderer.set_dim_alpha(&ctx.queue, 1.0, 1.0);
    let off = render_rgba(&ctx, &renderer);
    assert_eq!(brightness(&off, ax, ay), a0, "dimming can be switched off");

    renderer.set_dim_alpha(&ctx.queue, 0.25, 0.1);
    renderer.clear_selection(&ctx.queue);
    let cleared = render_rgba(&ctx, &renderer);
    assert_eq!(
        brightness(&cleared, bx, by),
        b0,
        "clearing the selection restores everything"
    );
}

fn snapshot_now(ctx: &GpuContext, scene: &mut Scene) {
    scene.request_snapshot(&ctx.device, &ctx.queue);
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    assert!(scene.poll_snapshot());
}

fn ready_scene(ctx: &GpuContext) -> Scene {
    let mut scene = Scene::new(&ctx.device, FORMAT, [W, H], frozen());
    scene.set_data(&ctx.device, two_nodes()).unwrap();
    let view = target(ctx);
    scene.frame(&ctx.device, &ctx.queue, &view);
    snapshot_now(ctx, &mut scene);
    scene
}

fn screen_of(scene: &Scene, world: [f32; 2]) -> [f32; 2] {
    scene.camera().world_to_screen(world)
}

#[test]
fn dragging_a_node_moves_that_node_and_empty_space_starts_no_drag() {
    let Some(ctx) = context() else { return };
    let mut scene = ready_scene(&ctx);
    let view = target(&ctx);
    assert_eq!(
        scene.begin_drag(5.0, 5.0),
        None,
        "empty space is not a drag"
    );
    assert_eq!(scene.dragging(), None);

    let s = screen_of(&scene, [30.0, 0.0]);
    assert_eq!(scene.begin_drag(s[0], s[1]), Some(1));
    assert_eq!(scene.dragging(), Some(1));
    scene.drag_to(s[0] + 20.0, s[1]);
    for _ in 0..3 {
        scene.frame(&ctx.device, &ctx.queue, &view);
    }
    snapshot_now(&ctx, &mut scene);
    let want = scene.camera().screen_to_world([s[0] + 20.0, s[1]]);
    let got = scene.snapshot()[1];
    assert!(
        (got[0] - want[0]).abs() < 0.5 && (got[1] - want[1]).abs() < 0.5,
        "node 1 at {got:?}, want {want:?}"
    );
    assert!(
        (scene.snapshot()[0][0] + 30.0).abs() < 0.5,
        "node 0 did not move"
    );

    scene.end_drag();
    assert_eq!(scene.dragging(), None);
}

#[test]
fn the_grab_offset_keeps_the_node_from_jumping_under_the_pointer() {
    let Some(ctx) = context() else { return };
    let mut scene = ready_scene(&ctx);
    let view = target(&ctx);
    let s = screen_of(&scene, [30.0, 0.0]);
    assert_eq!(scene.begin_drag(s[0] + 3.0, s[1] - 2.0), Some(1));
    scene.drag_to(s[0] + 3.0, s[1] - 2.0); // no pointer movement
    scene.frame(&ctx.device, &ctx.queue, &view);
    snapshot_now(&ctx, &mut scene);
    assert!(
        (scene.snapshot()[1][0] - 30.0).abs() < 0.1,
        "node stayed put, got {:?}",
        scene.snapshot()[1]
    );
}

#[test]
fn pinning_validates_input_and_new_data_starts_unpinned() {
    let Some(ctx) = context() else { return };
    let mut scene = ready_scene(&ctx);
    scene.set_pinned(&ctx.queue, &[0]).unwrap();
    assert_eq!(scene.pinned(), &[0]);
    assert!(matches!(
        scene.set_pinned(&ctx.queue, &[5]),
        Err(SceneError::Gpu(_))
    ));
    assert_eq!(scene.pinned(), &[0], "a rejected call changes nothing");
    scene.set_data(&ctx.device, two_nodes()).unwrap();
    assert!(scene.pinned().is_empty(), "new data starts unpinned");
}

#[test]
fn selecting_nodes_reports_counts_and_validates_indices() {
    let Some(ctx) = context() else { return };
    let mut scene = ready_scene(&ctx);
    let s = scene
        .select_nodes(&ctx.device, &ctx.queue, &[0], SelectOp::Replace)
        .unwrap();
    assert_eq!((s.nodes, s.links), (1, 0));
    let s = scene
        .select_nodes(&ctx.device, &ctx.queue, &[1], SelectOp::Add)
        .unwrap();
    assert_eq!(
        (s.nodes, s.links),
        (2, 1),
        "the link between them joins the selection"
    );
    assert!(matches!(
        scene.select_nodes(&ctx.device, &ctx.queue, &[9], SelectOp::Add),
        Err(SceneError::Selection(_))
    ));
    scene.clear_selection(&ctx.queue);
    assert!(!scene.selection().is_active());
}

#[test]
fn rectangle_and_polygon_select_by_screen_geometry() {
    let Some(ctx) = context() else { return };
    let mut scene = ready_scene(&ctx);
    let b = screen_of(&scene, [30.0, 0.0]);
    let s = scene
        .select_in_rect(
            &ctx.device,
            &ctx.queue,
            [b[0] + 10.0, b[1] + 10.0],
            [b[0] - 10.0, b[1] - 10.0],
            SelectOp::Replace,
        )
        .unwrap();
    assert_eq!(s.nodes, 1);
    assert_eq!(scene.selection().selected_nodes(), vec![1]);

    let a = screen_of(&scene, [-30.0, 0.0]);
    let tri = [
        [a[0] - 10.0, a[1] - 10.0],
        [a[0] + 10.0, a[1] - 10.0],
        [a[0], a[1] + 10.0],
    ];
    scene
        .select_in_polygon(&ctx.device, &ctx.queue, &tri, SelectOp::Replace)
        .unwrap();
    assert_eq!(scene.selection().selected_nodes(), vec![0]);
}

#[test]
fn clicking_selects_a_node_with_or_without_neighbours_and_links_by_their_segment() {
    let Some(ctx) = context() else { return };
    let mut scene = ready_scene(&ctx);
    let a = screen_of(&scene, [-30.0, 0.0]);
    let hit = scene
        .select_at(
            &ctx.device,
            &ctx.queue,
            a[0],
            a[1],
            false,
            SelectOp::Replace,
        )
        .unwrap();
    assert_eq!(hit, SelectHit::Node(0));
    assert_eq!(scene.selection().selected_nodes(), vec![0]);
    scene
        .select_at(&ctx.device, &ctx.queue, a[0], a[1], true, SelectOp::Replace)
        .unwrap();
    assert_eq!(
        scene.selection().selected_nodes(),
        vec![0, 1],
        "neighbours come along"
    );

    let mid = screen_of(&scene, [0.0, 0.0]);
    assert_eq!(scene.pick_link(mid[0], mid[1]), Some(0));
    let hit = scene
        .select_at(
            &ctx.device,
            &ctx.queue,
            mid[0],
            mid[1],
            false,
            SelectOp::Replace,
        )
        .unwrap();
    assert_eq!(hit, SelectHit::Link(0));
    assert_eq!(
        scene.selection().selected_nodes(),
        vec![0, 1],
        "a link brings its endpoints"
    );

    let hit = scene
        .select_at(&ctx.device, &ctx.queue, 2.0, 2.0, false, SelectOp::Replace)
        .unwrap();
    assert_eq!(hit, SelectHit::None);
}

#[test]
fn node_screen_position_matches_the_camera() {
    let Some(ctx) = context() else { return };
    let scene = ready_scene(&ctx);
    let want = screen_of(&scene, [30.0, 0.0]);
    let got = scene.node_screen_position(1).unwrap();
    assert!((got[0] - want[0]).abs() < 1e-3 && (got[1] - want[1]).abs() < 1e-3);
    assert_eq!(scene.node_screen_position(2), None);
}

#[test]
fn new_data_clears_the_selection() {
    let Some(ctx) = context() else { return };
    let mut scene = ready_scene(&ctx);
    scene
        .select_nodes(&ctx.device, &ctx.queue, &[0], SelectOp::Replace)
        .unwrap();
    scene.set_data(&ctx.device, two_nodes()).unwrap();
    assert!(!scene.selection().is_active());
}

#[test]
fn a_capture_shows_the_dimming() {
    let Some(ctx) = context() else { return };
    let mut scene = ready_scene(&ctx);
    let px = |scene: &mut Scene, ctx: &GpuContext, world: [f32; 2]| {
        scene.request_capture(&ctx.device, &ctx.queue).unwrap();
        let c = finish_capture(ctx, scene);
        let s = scene.camera().world_to_screen(world);
        let i = (s[1] as usize * c.width as usize + s[0] as usize) * 4;
        c.rgba[i] as u32 + c.rgba[i + 1] as u32 + c.rgba[i + 2] as u32
    };
    let before = px(&mut scene, &ctx, [30.0, 0.0]);
    scene
        .select_nodes(&ctx.device, &ctx.queue, &[0], SelectOp::Replace)
        .unwrap();
    let after = px(&mut scene, &ctx, [30.0, 0.0]);
    assert!(
        after < before,
        "the unselected node is darker in the exported image ({after} < {before})"
    );
    scene.set_dim_unselected(&ctx.queue, false);
    assert_eq!(px(&mut scene, &ctx, [30.0, 0.0]), before, "dimming off");
}

#[test]
fn non_finite_pointer_input_is_ignored_or_rejected_and_changes_nothing() {
    let Some(ctx) = context() else { return };
    let mut scene = ready_scene(&ctx);
    assert_eq!(scene.begin_drag(f32::NAN, 1.0), None);
    assert_eq!(scene.dragging(), None);
    assert_eq!(scene.pick_link(f32::INFINITY, 0.0), None);
    assert!(matches!(
        scene.select_in_rect(
            &ctx.device,
            &ctx.queue,
            [f32::NAN, 0.0],
            [1.0, 1.0],
            SelectOp::Replace
        ),
        Err(SceneError::Gpu(_))
    ));
    assert!(matches!(
        scene.select_in_polygon(
            &ctx.device,
            &ctx.queue,
            &[[0.0, 0.0], [1.0, f32::NAN], [2.0, 2.0]],
            SelectOp::Replace
        ),
        Err(SceneError::Gpu(_))
    ));
    assert!(matches!(
        scene.select_at(
            &ctx.device,
            &ctx.queue,
            f32::NAN,
            0.0,
            false,
            SelectOp::Replace
        ),
        Err(SceneError::Gpu(_))
    ));
    assert!(
        !scene.selection().is_active(),
        "nothing was selected by the rejected calls"
    );

    // A drag in progress ignores a NaN move instead of poisoning the layout.
    // (`GpuLayout::hold` and `PickIndex` already reject NaN, so this part guards
    // against regressions; the guards added for this task are proven by the
    // `select_*` asserts above.)
    let s = screen_of(&scene, [30.0, 0.0]);
    assert_eq!(scene.begin_drag(s[0], s[1]), Some(1));
    scene.drag_to(f32::NAN, f32::NAN);
    scene.end_drag();
    let view = target(&ctx);
    scene.frame(&ctx.device, &ctx.queue, &view);
    snapshot_now(&ctx, &mut scene);
    assert!(scene
        .snapshot()
        .iter()
        .all(|p| p[0].is_finite() && p[1].is_finite()));
}

#[test]
fn oversized_lists_and_polygons_are_rejected() {
    let Some(ctx) = context() else { return };
    let mut scene = ready_scene(&ctx);
    let too_many = vec![0u32; MAX_INDEX_LIST + 1];
    assert!(matches!(
        scene.select_nodes(&ctx.device, &ctx.queue, &too_many, SelectOp::Replace),
        Err(SceneError::Gpu(_))
    ));
    assert!(matches!(
        scene.set_pinned(&ctx.queue, &too_many),
        Err(SceneError::Gpu(_))
    ));
    let polygon = vec![[0.0f32, 0.0]; MAX_POLYGON_VERTICES + 1];
    assert!(matches!(
        scene.select_in_polygon(&ctx.device, &ctx.queue, &polygon, SelectOp::Replace),
        Err(SceneError::Gpu(_))
    ));
    assert!(!scene.selection().is_active());
    assert!(scene.pinned().is_empty());
}
