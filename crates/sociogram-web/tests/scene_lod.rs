mod common;

use common::{context, finish_capture, frozen, FORMAT, H, W};
use sociogram_core::{GraphData, LayoutConfig};
use sociogram_gpu::GpuContext;
use sociogram_web::{
    adapt_lod, auto_space_half, LinkFade, Scene, SceneOptions, LINK_FADE_AUTO_MIN_LINKS,
    LOD_MIN_SCALE, MAX_AUTO_SPACE_HALF,
};

/// Two nodes 600 world units apart joined by `links` parallel links. After
/// auto-fit in the 200 px wide viewport the link is 120 px long, and its middle
/// pixel is (100, 50).
fn long_link_graph(links: usize) -> GraphData {
    GraphData {
        node_count: 2,
        edges: vec![[0, 1]; links],
        positions: Some(vec![[-300.0, 0.0], [300.0, 0.0]]),
        sizes: Some(vec![4.0, 4.0]),
        ..Default::default()
    }
}

fn background_rgb() -> [u8; 3] {
    let b = SceneOptions::default().background;
    [
        (b.r * 255.0).round() as u8,
        (b.g * 255.0).round() as u8,
        (b.b * 255.0).round() as u8,
    ]
}

/// True when the middle of the long link was drawn (differs from the background).
fn middle_is_drawn(ctx: &GpuContext, scene: &mut Scene) -> bool {
    scene.request_capture(&ctx.device, &ctx.queue).unwrap();
    let capture = finish_capture(ctx, scene);
    let i = (50 * capture.width as usize + 100) * 4;
    [capture.rgba[i], capture.rgba[i + 1], capture.rgba[i + 2]] != background_rgb()
}

fn scene_with(ctx: &GpuContext, links: usize, fade: LinkFade) -> Scene {
    let options = SceneOptions {
        link_fade: fade,
        // 2 px wide: a 1 px link centered on y=50 has its edges exactly on the
        // pixel centers of rows 49 and 50 and may cover neither.
        link_width: 2.0,
        ..frozen()
    };
    let mut scene = Scene::new(&ctx.device, FORMAT, [W, H], options);
    scene.set_data(&ctx.device, long_link_graph(links)).unwrap();
    scene
}

#[test]
fn auto_fade_leaves_small_graphs_untouched() {
    let Some(ctx) = context() else { return };
    let mut scene = scene_with(&ctx, 1, LinkFade::Auto);
    assert!(middle_is_drawn(&ctx, &mut scene));
}

#[test]
fn auto_fade_hides_long_links_in_large_graphs() {
    let Some(ctx) = context() else { return };
    let mut scene = scene_with(&ctx, LINK_FADE_AUTO_MIN_LINKS, LinkFade::Auto);
    assert!(!middle_is_drawn(&ctx, &mut scene));
}

#[test]
fn auto_fade_starts_exactly_at_the_threshold() {
    let Some(ctx) = context() else { return };
    let mut scene = scene_with(&ctx, LINK_FADE_AUTO_MIN_LINKS - 1, LinkFade::Auto);
    assert!(middle_is_drawn(&ctx, &mut scene));
}

#[test]
fn fade_off_draws_everything_even_in_large_graphs() {
    let Some(ctx) = context() else { return };
    let mut scene = scene_with(&ctx, LINK_FADE_AUTO_MIN_LINKS, LinkFade::Off);
    assert!(middle_is_drawn(&ctx, &mut scene));
}

#[test]
fn manual_range_applies_to_any_graph_size() {
    let Some(ctx) = context() else { return };
    // The link is 120 px long: beyond a 20..60 px range, inside a 100..400 px range.
    let mut hidden = scene_with(
        &ctx,
        1,
        LinkFade::Px {
            start: 20.0,
            end: 60.0,
        },
    );
    assert!(!middle_is_drawn(&ctx, &mut hidden));
    let mut visible = scene_with(
        &ctx,
        1,
        LinkFade::Px {
            start: 200.0,
            end: 400.0,
        },
    );
    assert!(middle_is_drawn(&ctx, &mut visible));
}

#[test]
fn invalid_manual_range_behaves_like_off() {
    let Some(ctx) = context() else { return };
    let mut scene = scene_with(
        &ctx,
        1,
        LinkFade::Px {
            start: 60.0,
            end: 20.0,
        },
    );
    assert!(middle_is_drawn(&ctx, &mut scene));
}

#[test]
fn the_fade_mode_can_change_at_runtime() {
    let Some(ctx) = context() else { return };
    let mut scene = scene_with(&ctx, LINK_FADE_AUTO_MIN_LINKS, LinkFade::Auto);
    assert!(!middle_is_drawn(&ctx, &mut scene));
    scene.set_link_fade(LinkFade::Off);
    assert!(middle_is_drawn(&ctx, &mut scene));
    scene.set_link_fade(LinkFade::Auto);
    assert!(!middle_is_drawn(&ctx, &mut scene));
}

#[test]
fn replacing_the_graph_updates_the_link_count_used_by_auto() {
    let Some(ctx) = context() else { return };
    let mut scene = scene_with(&ctx, LINK_FADE_AUTO_MIN_LINKS, LinkFade::Auto);
    assert!(!middle_is_drawn(&ctx, &mut scene));
    scene.set_data(&ctx.device, long_link_graph(1)).unwrap();
    assert!(middle_is_drawn(&ctx, &mut scene));
}

#[test]
fn layout_space_grows_with_the_graph_but_never_shrinks_below_the_minimum() {
    let base = LayoutConfig::default();
    let small = auto_space_half(100, &base, 1024.0);
    let medium = auto_space_half(2_000, &base, 1024.0);
    let large = auto_space_half(100_000, &base, 1024.0);
    assert_eq!(small, 1024.0, "small graphs keep the minimum");
    // 2,000 nodes settle near radius ~1,400, which does not fit in 1,024.
    assert!(medium > 1_400.0, "medium space {medium} is too small");
    assert!(large > medium);
    // Roughly proportional to sqrt(n): 50x the nodes, about 7x the space.
    let ratio = large / medium;
    assert!((6.0..8.0).contains(&ratio), "ratio {ratio}");
}

#[test]
fn layout_space_is_capped_and_ignores_missing_forces() {
    let base = LayoutConfig::default();
    assert_eq!(
        auto_space_half(usize::MAX / 2, &base, 1024.0),
        MAX_AUTO_SPACE_HALF
    );
    assert_eq!(auto_space_half(0, &base, 1024.0), 1024.0);
    let no_repulsion = LayoutConfig {
        repulsion: 0.0,
        ..base
    };
    let no_gravity = LayoutConfig {
        gravity: 0.0,
        ..base
    };
    assert_eq!(auto_space_half(100_000, &no_repulsion, 1024.0), 1024.0);
    assert_eq!(auto_space_half(100_000, &no_gravity, 1024.0), 1024.0);
    // A minimum above the cap is respected.
    assert_eq!(auto_space_half(100_000, &base, 50_000.0), 50_000.0);
}

fn feed(ms: f32, frames: usize) -> f32 {
    let (mut ema, mut scale) = (None, 1.0);
    for _ in 0..frames {
        (ema, scale) = adapt_lod(ema, scale, ms);
    }
    scale
}

#[test]
fn slow_frames_make_the_fade_stricter_down_to_a_floor() {
    assert!(feed(80.0, 5) < 0.6);
    assert_eq!(feed(80.0, 200), LOD_MIN_SCALE);
}

#[test]
fn fast_frames_never_exceed_the_unscaled_range() {
    assert_eq!(feed(8.0, 500), 1.0);
}

#[test]
fn frames_between_the_thresholds_change_nothing() {
    assert_eq!(feed(30.0, 100), 1.0);
}

#[test]
fn the_fade_backs_off_quickly_and_recovers_slowly_and_monotonically() {
    // Frames until the scale hits the floor when every frame is slow.
    let (mut ema, mut scale) = (None, 1.0);
    let mut back_off_frames = 0;
    while scale > LOD_MIN_SCALE {
        (ema, scale) = adapt_lod(ema, scale, 90.0);
        back_off_frames += 1;
        assert!(back_off_frames < 1_000, "never reached the floor");
    }
    // Frames until it is back to the unscaled range when every frame is fast.
    let mut recover_frames = 0;
    let mut last = scale;
    while scale < 1.0 {
        (ema, scale) = adapt_lod(ema, scale, 8.0);
        recover_frames += 1;
        assert!(scale >= last, "recovery must be monotonic");
        last = scale;
        assert!(recover_frames < 10_000, "never recovered");
    }
    assert!(
        recover_frames > 3 * back_off_frames,
        "recovery ({recover_frames} frames) should be much slower than backing off ({back_off_frames})"
    );
}

#[test]
fn invalid_and_outlier_frame_times_are_handled() {
    let (ema, scale) = adapt_lod(Some(10.0), 0.7, f32::NAN);
    assert_eq!((ema, scale), (Some(10.0), 0.7));
    assert_eq!(adapt_lod(Some(10.0), 0.7, -5.0), (Some(10.0), 0.7));
    assert_eq!(adapt_lod(Some(10.0), 0.7, f32::INFINITY), (Some(10.0), 0.7));
    // One enormous hitch (hidden tab) is clamped and, smoothed with healthy
    // history, does not slam the scale to the floor.
    let (_, scale) = adapt_lod(Some(10.0), 1.0, 60_000.0);
    assert!(scale > 0.8, "a single hitch moved the scale to {scale}");
}

#[test]
fn a_new_graph_starts_from_the_generous_setting() {
    let Some(ctx) = context() else { return };
    let mut scene = scene_with(&ctx, 1, LinkFade::Auto);
    for _ in 0..50 {
        scene.report_frame_time(90.0);
    }
    assert!(scene.link_lod_scale() < 0.2);
    scene.set_data(&ctx.device, long_link_graph(1)).unwrap();
    assert_eq!(scene.link_lod_scale(), 1.0);
}

#[test]
fn adaptive_scale_tightens_the_auto_fade_for_large_graphs() {
    let Some(ctx) = context() else { return };
    // 1000 px wide viewport: the unscaled Auto range is 125..375 px. Two far
    // away anchor nodes (+-230) make auto-fit zoom 2, so the 60 unit link is
    // 120 px long: fully visible at scale 1, hidden once the range shrinks.
    let options = SceneOptions {
        link_width: 2.0,
        ..frozen()
    };
    let mut scene = Scene::new(&ctx.device, FORMAT, [1000.0, 100.0], options);
    scene
        .set_data(
            &ctx.device,
            GraphData {
                node_count: 4,
                edges: vec![[0, 1]; LINK_FADE_AUTO_MIN_LINKS],
                positions: Some(vec![[-30.0, 0.0], [30.0, 0.0], [-230.0, 0.0], [230.0, 0.0]]),
                sizes: Some(vec![4.0; 4]),
                ..Default::default()
            },
        )
        .unwrap();
    let middle = |scene: &mut Scene| {
        scene.request_capture(&ctx.device, &ctx.queue).unwrap();
        let c = finish_capture(&ctx, scene);
        let i = (50 * c.width as usize + 500) * 4;
        [c.rgba[i], c.rgba[i + 1], c.rgba[i + 2]] != background_rgb()
    };
    let length_px = scene.camera().zoom * 60.0;
    assert!(
        (110.0..125.0).contains(&length_px),
        "link is {length_px} px"
    );
    assert!(middle(&mut scene), "visible at the unscaled range");
    for _ in 0..60 {
        scene.report_frame_time(90.0);
    }
    assert!(
        !middle(&mut scene),
        "hidden once the controller tightened the range"
    );
}
