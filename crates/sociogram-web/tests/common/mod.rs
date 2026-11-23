#![allow(dead_code)]

use sociogram_core::{GraphData, GridLayoutConfig, LayoutConfig};
use sociogram_gpu::GpuContext;
use sociogram_web::{Scene, SceneOptions};

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub const W: f32 = 200.0;
pub const H: f32 = 100.0;

pub fn context() -> Option<GpuContext> {
    match pollster::block_on(GpuContext::new_headless()) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("skipping GPU test: {e}");
            None
        }
    }
}

pub fn target(ctx: &GpuContext) -> wgpu::TextureView {
    ctx.device
        .create_texture(&wgpu::TextureDescriptor {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// Layout with every force off, so positions stay exactly as given.
pub fn frozen() -> SceneOptions {
    SceneOptions {
        layout: GridLayoutConfig {
            base: LayoutConfig {
                gravity: 0.0,
                repulsion: 0.0,
                link_spring: 0.0,
                alpha_decay: 0.0,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn two_nodes() -> GraphData {
    GraphData {
        node_count: 2,
        edges: vec![[0, 1]],
        positions: Some(vec![[-30.0, 0.0], [30.0, 0.0]]),
        sizes: Some(vec![4.0, 4.0]),
        ..Default::default()
    }
}

pub fn finish_capture(ctx: &GpuContext, scene: &mut Scene) -> sociogram_web::Capture {
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    scene.poll_capture().expect("capture should be ready")
}
