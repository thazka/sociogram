//! Offscreen capture: render the current view into a texture and read it back.

use super::{MapStatus, Scene, SceneError};
use sociogram_gpu::GpuError;
use std::sync::{Arc, Mutex};

/// Largest side, in pixels, accepted by [`Scene::request_capture`].
pub const MAX_CAPTURE_SIDE: u32 = 8192;

/// A rendered frame read back to the CPU: tightly packed RGBA8, top row first.
#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// An offscreen capture in flight.
pub(super) struct CaptureJob {
    buffer: wgpu::Buffer,
    status: MapStatus,
    width: u32,
    height: u32,
    padded_row: u32,
    swap_red_blue: bool,
}

impl Scene {
    /// Render the current view (same camera and size as the screen) into an
    /// offscreen texture and start copying it to the CPU. The picture does not
    /// depend on when the canvas was last presented, which is what makes a
    /// screenshot reliable. Fails while another capture is in flight. Collect
    /// the result with [`Scene::poll_capture`].
    pub fn request_capture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<(), SceneError> {
        if self.capture.is_some() {
            return Err(SceneError::Gpu(GpuError::InvalidInput(
                "a capture is already in progress".into(),
            )));
        }
        let width = self.camera.viewport[0] as u32;
        let height = self.camera.viewport[1] as u32;
        if width == 0 || height == 0 || width > MAX_CAPTURE_SIDE || height > MAX_CAPTURE_SIDE {
            return Err(SceneError::Gpu(GpuError::InvalidInput(format!(
                "capture size {width}x{height} is outside 1..={MAX_CAPTURE_SIDE}"
            ))));
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sociogram capture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        // Rows in a texture-to-buffer copy must be 256-byte aligned.
        let padded_row = (width * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sociogram capture readback"),
            size: padded_row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        self.apply_link_fade();
        self.renderer.set_camera(queue, &self.camera);
        let mut encoder = device.create_command_encoder(&Default::default());
        self.renderer
            .render(&mut encoder, &view, self.options.background);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);

        let status: MapStatus = Arc::new(Mutex::new(None));
        let callback_status = Arc::clone(&status);
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            if let Ok(mut s) = callback_status.lock() {
                *s = Some(r.map_err(|e| e.to_string()));
            }
        });
        self.capture = Some(CaptureJob {
            buffer,
            status,
            width,
            height,
            padded_row,
            swap_red_blue: matches!(
                self.format,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
            ),
        });
        Ok(())
    }

    /// The finished capture, if it has arrived. An unsuccessful readback is
    /// dropped and reported as `None`; request again.
    pub fn poll_capture(&mut self) -> Option<Capture> {
        let job = self.capture.as_ref()?;
        let outcome = job.status.lock().ok().and_then(|mut s| s.take())?;
        let job = self.capture.take()?;
        outcome.ok()?;
        let mapped = job.buffer.slice(..).get_mapped_range().ok()?;
        let row = job.width as usize * 4;
        let mut rgba = Vec::with_capacity(row * job.height as usize);
        for y in 0..job.height as usize {
            let start = y * job.padded_row as usize;
            rgba.extend_from_slice(&mapped[start..start + row]);
        }
        drop(mapped);
        job.buffer.unmap();
        if job.swap_red_blue {
            for px in rgba.as_chunks_mut::<4>().0.iter_mut() {
                px.swap(0, 2);
            }
        }
        Some(Capture {
            width: job.width,
            height: job.height,
            rgba,
        })
    }
}
