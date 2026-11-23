mod common;

use common::{context, px, render, H, W};
use sociogram_core::Camera;
use sociogram_gpu::{LinkInstance, NodeInstance, Renderer};

/// positions[0] is the node, positions[1] and [2] are the link endpoints.
const POSITIONS: [[f32; 2]; 3] = [[0.0, 0.0], [-20.0, 15.0], [20.0, 15.0]];

fn red_node() -> NodeInstance {
    NodeInstance {
        size: 10.0,
        color: [1.0, 0.0, 0.0, 1.0],
    }
}

fn green_link() -> LinkInstance {
    LinkInstance {
        src: 1,
        dst: 2,
        color: [0.0, 1.0, 0.0, 1.0],
        width: 4.0,
    }
}

#[test]
fn draws_node_link_and_background() {
    let Some(ctx) = context() else { return };
    let cam = Camera::new([W as f32, H as f32]);
    let data = render(&ctx, &cam, |r, device| {
        let positions = Renderer::upload_positions(device, &POSITIONS);
        r.set_positions(device, &positions);
        r.set_nodes(device, &[red_node()]);
        r.set_links(device, &[green_link()]);
    });
    assert_eq!(px(&data, 32, 32), [255, 0, 0, 255], "node center is red");
    assert_eq!(px(&data, 2, 2), [0, 0, 0, 255], "background is black");
    assert_eq!(
        px(&data, 32, 17),
        [0, 255, 0, 255],
        "link (15 world units up) is green"
    );
}

#[test]
fn node_is_circular_not_square() {
    let Some(ctx) = context() else { return };
    let cam = Camera::new([W as f32, H as f32]);
    let data = render(&ctx, &cam, |r, device| {
        let positions = Renderer::upload_positions(device, &POSITIONS);
        r.set_positions(device, &positions);
        r.set_nodes(device, &[red_node()]);
    });
    // Radius 10: the quad corner (dx=dy=9 -> distance ~12.7) must stay empty.
    assert_eq!(px(&data, 41, 23), [0, 0, 0, 255]);
    assert_eq!(px(&data, 38, 32)[0], 255);
}

#[test]
fn zoom_scales_node_radius() {
    let Some(ctx) = context() else { return };
    let mut cam = Camera::new([W as f32, H as f32]);
    let setup = |r: &mut Renderer, device: &wgpu::Device| {
        let positions = Renderer::upload_positions(device, &POSITIONS);
        r.set_positions(device, &positions);
        r.set_nodes(device, &[red_node()]);
    };
    // Radius grows with sqrt(zoom): 10 px at zoom 1, 20 px at zoom 4.
    // Pixel x=47 is 15 px from the center: outside radius 10, inside radius 20.
    let data = render(&ctx, &cam, setup);
    assert_eq!(px(&data, 47, 32), [0, 0, 0, 255]);
    cam.zoom = 4.0;
    let data = render(&ctx, &cam, setup);
    assert_eq!(px(&data, 47, 32), [255, 0, 0, 255]);
    // Distinguishes sqrt from linear scaling: x=62 is 30 px from the center,
    // outside radius 20 (sqrt) but inside radius 40 (linear).
    assert_eq!(px(&data, 62, 32), [0, 0, 0, 255]);
}

#[test]
fn nothing_is_drawn_until_positions_are_bound() {
    let Some(ctx) = context() else { return };
    let cam = Camera::new([W as f32, H as f32]);
    let data = render(&ctx, &cam, |r, device| {
        r.set_nodes(device, &[red_node()]);
        r.set_links(device, &[green_link()]);
    });
    assert!(data.chunks(4).all(|p| p == [0, 0, 0, 255]));
}

#[test]
fn empty_scene_renders_only_background() {
    let Some(ctx) = context() else { return };
    let cam = Camera::new([W as f32, H as f32]);
    let data = render(&ctx, &cam, |r, device| {
        let positions = Renderer::upload_positions(device, &[]);
        r.set_positions(device, &positions);
    });
    assert!(data.chunks(4).all(|p| p == [0, 0, 0, 255]));
}

/// The link in `POSITIONS` is 40 px long at zoom 1; its middle pixel is (32, 17).
fn link_pixel(fade: Option<(f32, f32)>) -> [u8; 4] {
    let Some(ctx) = context() else {
        return [0, 255, 0, 255];
    };
    let cam = Camera::new([W as f32, H as f32]);
    let data = render(&ctx, &cam, |r, device| {
        let positions = Renderer::upload_positions(device, &POSITIONS);
        r.set_positions(device, &positions);
        r.set_links(device, &[green_link()]);
        if let Some((start, end)) = fade {
            r.set_link_fade(start, end);
        }
    });
    px(&data, 32, 17)
}

#[test]
fn links_longer_than_the_fade_range_are_not_drawn() {
    // 40 px link, fade range 10..30 px: fully faded.
    assert_eq!(link_pixel(Some((10.0, 30.0))), [0, 0, 0, 255]);
}

#[test]
fn links_shorter_than_the_fade_range_stay_fully_visible() {
    assert_eq!(link_pixel(Some((50.0, 100.0))), [0, 255, 0, 255]);
}

#[test]
fn links_inside_the_fade_range_are_partly_transparent() {
    // 40 px link, range 20..60 px: visibility 0.5 over a black background.
    let p = link_pixel(Some((20.0, 60.0)));
    assert!(
        p[1] > 100 && p[1] < 160,
        "green channel {} not near half",
        p[1]
    );
    assert_eq!((p[0], p[2]), (0, 0));
}

#[test]
fn invalid_fade_ranges_are_ignored() {
    for (start, end) in [
        (f32::NAN, 100.0),
        (10.0, f32::INFINITY),
        (-5.0, 100.0),
        (50.0, 20.0),
        (30.0, 30.0),
        (0.0, 0.0),
    ] {
        assert_eq!(
            link_pixel(Some((start, end))),
            [0, 255, 0, 255],
            "range ({start}, {end}) should be ignored"
        );
    }
}
