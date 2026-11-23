//! JavaScript-facing API (compiled for `wasm32` only). Framework neutral: the
//! caller owns the animation loop, the pointer events and the element
//! lifecycle; see `packages/sociogram` for the TypeScript wrapper.

use crate::lod::LinkFade;
use crate::scene::{Scene, SceneOptions};
use sociogram_core::GraphData;
use sociogram_gpu::GpuContext;
use std::fmt::Display;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

fn js_error(e: impl Display) -> JsError {
    JsError::new(&e.to_string())
}

struct Inner {
    ctx: GpuContext,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    scene: Scene,
}

/// A graph view bound to one canvas.
#[wasm_bindgen]
pub struct Sociogram {
    inner: Option<Inner>,
}

fn pairs<const N: usize, T: Copy>(
    values: &[T],
    what: &str,
    node_count: usize,
) -> Result<Vec<[T; N]>, JsError> {
    if values.len() != N * node_count {
        return Err(JsError::new(&format!(
            "invalid graph: {what} must have {N} numbers per node ({} expected, got {})",
            N * node_count,
            values.len()
        )));
    }
    Ok(values.as_chunks::<N>().0.to_vec())
}

#[wasm_bindgen]
impl Sociogram {
    /// Create a view on `canvas`. Rejects when WebGPU is unavailable.
    pub async fn create(canvas: HtmlCanvasElement) -> Result<Sociogram, JsError> {
        console_error_panic_hook::set_once();
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));
        let (ctx, surface) = GpuContext::new_for_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .await
            .map_err(js_error)?;

        let caps = surface.get_capabilities(&ctx.adapter);
        // Prefer a non-sRGB format: colors are written as given, not re-encoded.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| JsError::new("the canvas surface supports no texture format"))?;
        let mut config = surface
            .get_default_config(&ctx.adapter, width, height)
            .ok_or_else(|| JsError::new("the adapter cannot present to this canvas"))?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&ctx.device, &config);

        let scene = Scene::new(
            &ctx.device,
            format,
            [width as f32, height as f32],
            SceneOptions::default(),
        );
        Ok(Sociogram {
            inner: Some(Inner {
                ctx,
                surface,
                config,
                scene,
            }),
        })
    }

    fn inner_mut(&mut self) -> Result<&mut Inner, JsError> {
        self.inner
            .as_mut()
            .ok_or_else(|| JsError::new("this Sociogram instance was destroyed"))
    }

    /// Replace the graph. `edges` is a flat `[src, dst, src, dst, ...]` array;
    /// optional `positions` (2 per node), `sizes` (1 per node) and `colors`
    /// (RGBA, 4 per node) follow the node order. On error the previous graph
    /// stays in place.
    pub fn set_data(
        &mut self,
        node_count: u32,
        edges: &[u32],
        positions: Option<Vec<f32>>,
        sizes: Option<Vec<f32>>,
        colors: Option<Vec<f32>>,
    ) -> Result<(), JsError> {
        let n = node_count as usize;
        if !edges.len().is_multiple_of(2) {
            return Err(JsError::new(
                "invalid graph: edges must contain a source and a target per edge",
            ));
        }
        let data = GraphData {
            node_count: n,
            edges: edges.as_chunks::<2>().0.to_vec(),
            positions: positions
                .map(|p| pairs::<2, f32>(&p, "positions", n))
                .transpose()?,
            sizes,
            colors: colors
                .map(|c| pairs::<4, f32>(&c, "colors", n))
                .transpose()?,
        };
        let inner = self.inner_mut()?;
        inner
            .scene
            .set_data(&inner.ctx.device, data)
            .map_err(js_error)
    }

    /// Resize the drawing buffer (physical pixels).
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), JsError> {
        let inner = self.inner_mut()?;
        inner.config.width = width.max(1);
        inner.config.height = height.max(1);
        inner.surface.configure(&inner.ctx.device, &inner.config);
        inner
            .scene
            .resize(inner.config.width as f32, inner.config.height as f32);
        Ok(())
    }

    /// Run layout steps and draw one frame.
    pub fn frame(&mut self) -> Result<(), JsError> {
        let inner = self.inner_mut()?;
        match inner.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                let view = texture.texture.create_view(&Default::default());
                inner
                    .scene
                    .frame(&inner.ctx.device, &inner.ctx.queue, &view);
                inner.ctx.queue.present(texture);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                inner.surface.configure(&inner.ctx.device, &inner.config);
            }
            // Timeout, occluded, validation errors: skip this frame.
            _ => {}
        }
        Ok(())
    }

    pub fn pan_by(&mut self, dx: f32, dy: f32) -> Result<(), JsError> {
        self.inner_mut()?.scene.pan_by_pixels(dx, dy);
        Ok(())
    }

    pub fn zoom_at(&mut self, factor: f32, x: f32, y: f32) -> Result<(), JsError> {
        self.inner_mut()?.scene.zoom_at(factor, x, y);
        Ok(())
    }

    pub fn fit_view(&mut self) -> Result<(), JsError> {
        self.inner_mut()?.scene.fit_view();
        Ok(())
    }

    /// Fade links out between `start_px` and `end_px` of on-screen length
    /// (drawing-buffer pixels); longer links are not drawn. Invalid ranges
    /// (NaN, negative start, `end <= start`) turn the fade off.
    pub fn set_link_fade(&mut self, start_px: f32, end_px: f32) -> Result<(), JsError> {
        self.inner_mut()?.scene.set_link_fade(LinkFade::Px {
            start: start_px,
            end: end_px,
        });
        Ok(())
    }

    /// Report the time between presented frames (milliseconds). In the default
    /// link-fade mode large graphs adapt their link detail to keep frames fast.
    pub fn report_frame_time(&mut self, ms: f32) -> Result<(), JsError> {
        self.inner_mut()?.scene.report_frame_time(ms);
        Ok(())
    }

    /// Draw every link at full strength.
    pub fn set_link_fade_off(&mut self) -> Result<(), JsError> {
        self.inner_mut()?.scene.set_link_fade(LinkFade::Off);
        Ok(())
    }

    /// Default: fade long links only for large graphs (50,000 links or more).
    pub fn set_link_fade_auto(&mut self) -> Result<(), JsError> {
        self.inner_mut()?.scene.set_link_fade(LinkFade::Auto);
        Ok(())
    }

    /// Restart the layout so the graph moves again.
    pub fn reheat(&mut self) -> Result<(), JsError> {
        self.inner_mut()?.scene.reheat();
        Ok(())
    }

    /// Start copying positions to the CPU (for `pick`); apply with `poll_snapshot`.
    pub fn request_snapshot(&mut self) -> Result<(), JsError> {
        let inner = self.inner_mut()?;
        inner
            .scene
            .request_snapshot(&inner.ctx.device, &inner.ctx.queue);
        Ok(())
    }

    /// Returns true when a requested snapshot arrived and was applied.
    pub fn poll_snapshot(&mut self) -> Result<bool, JsError> {
        Ok(self.inner_mut()?.scene.poll_snapshot())
    }

    /// Index of the node under the pointer (physical pixels) or -1.
    pub fn pick(&self, x: f32, y: f32) -> i32 {
        self.inner
            .as_ref()
            .and_then(|i| i.scene.pick(x, y))
            .map_or(-1, |n| n as i32)
    }

    pub fn node_count(&self) -> u32 {
        self.inner
            .as_ref()
            .map_or(0, |i| i.scene.node_count() as u32)
    }

    pub fn is_settled(&self) -> bool {
        self.inner.as_ref().is_none_or(|i| i.scene.is_settled())
    }

    /// `[center_x, center_y, zoom]`.
    pub fn camera(&self) -> Vec<f32> {
        self.inner.as_ref().map_or_else(Vec::new, |i| {
            let c = i.scene.camera();
            vec![c.center[0], c.center[1], c.zoom]
        })
    }

    /// Release the GPU resources. Safe to call more than once.
    pub fn destroy(&mut self) {
        self.inner = None;
    }
}

/// A rendered picture: tightly packed RGBA8, top row first.
#[wasm_bindgen]
pub struct ImageCapture {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[wasm_bindgen]
impl ImageCapture {
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// RGBA pixels (a copy).
    #[wasm_bindgen(getter)]
    pub fn rgba(&self) -> Vec<u8> {
        self.rgba.clone()
    }
}

#[wasm_bindgen]
impl Sociogram {
    /// Render the current view offscreen and start copying it to the CPU.
    /// Collect it with `poll_capture`. Throws if a capture is already running.
    pub fn request_capture(&mut self) -> Result<(), JsError> {
        let inner = self.inner_mut()?;
        inner
            .scene
            .request_capture(&inner.ctx.device, &inner.ctx.queue)
            .map_err(js_error)
    }

    /// The finished capture, or `undefined` while it is still in flight.
    pub fn poll_capture(&mut self) -> Result<Option<ImageCapture>, JsError> {
        Ok(self
            .inner_mut()?
            .scene
            .poll_capture()
            .map(|c| ImageCapture {
                width: c.width,
                height: c.height,
                rgba: c.rgba,
            }))
    }
}
