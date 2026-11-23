#![allow(dead_code)]

use sociogram_core::Camera;
use sociogram_gpu::{GpuContext, Renderer};

pub const W: u32 = 64;
pub const H: u32 = 64;
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A GPU context, or `None` (with a note on stderr) when no adapter exists, so
/// GPU tests skip instead of failing on machines without a GPU.
pub fn context() -> Option<GpuContext> {
    pollster::block_on(sociogram_gpu::testing::test_context())
}

/// Render into an offscreen texture on a black background and return RGBA
/// pixels. `setup` configures the renderer (positions, nodes, links).
pub fn render(
    ctx: &GpuContext,
    camera: &Camera,
    setup: impl FnOnce(&mut Renderer, &wgpu::Device),
) -> Vec<u8> {
    let (device, queue) = (&ctx.device, &ctx.queue);
    let mut r = Renderer::new(device, FORMAT);
    setup(&mut r, device);
    // After `setup`: the camera uniform also carries the renderer's link-fade range.
    r.set_camera(queue, camera);

    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (W * H * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut enc = device.create_command_encoder(&Default::default());
    r.render(&mut enc, &view, wgpu::Color::BLACK);
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &out,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(W * 4),
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([enc.finish()]);

    let slice = out.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = slice.get_mapped_range().expect("mapped range");
    let data = mapped.to_vec();
    drop(mapped);
    data
}

pub fn px(data: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [data[i], data[i + 1], data[i + 2], data[i + 3]]
}
