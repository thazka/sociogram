//! JavaScript-facing API (compiled for `wasm32` only). Framework neutral: the
//! caller owns the animation loop, the pointer events and the element
//! lifecycle; see `packages/sociogram` for the TypeScript wrapper.

use crate::lod::LinkFade;
use crate::scene::{Scene, SceneOptions};
use crate::SelectHit;
use sociogram_core::{GraphData, SelectOp};
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

    /// Start waiting for the GPU to finish everything submitted so far; check with `poll_idle`.
    pub fn request_idle(&mut self) -> Result<(), JsError> {
        let inner = self.inner_mut()?;
        inner.scene.request_idle(&inner.ctx.queue);
        Ok(())
    }

    /// True once, when the GPU has finished the work present at the last `request_idle`.
    pub fn poll_idle(&mut self) -> Result<bool, JsError> {
        Ok(self.inner_mut()?.scene.poll_idle())
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

/// Selection operation codes shared with the TypeScript layer.
fn op_from(code: u8) -> Result<SelectOp, JsError> {
    match code {
        0 => Ok(SelectOp::Replace),
        1 => Ok(SelectOp::Add),
        2 => Ok(SelectOp::Remove),
        other => Err(JsError::new(&format!(
            "unknown selection operation {other}"
        ))),
    }
}

// Dragging, pinning and selection. Input validation (non-finite numbers,
// oversized lists, unknown indices) lives in `Scene`, so every method here is
// already safe against bad input; do not duplicate the checks.
#[wasm_bindgen]
impl Sociogram {
    /// Start dragging the node under the pointer (physical pixels): its index, or -1.
    pub fn begin_drag(&mut self, x: f32, y: f32) -> Result<i32, JsError> {
        Ok(self
            .inner_mut()?
            .scene
            .begin_drag(x, y)
            .map_or(-1, |i| i as i32))
    }

    pub fn drag_to(&mut self, x: f32, y: f32) -> Result<(), JsError> {
        self.inner_mut()?.scene.drag_to(x, y);
        Ok(())
    }

    pub fn end_drag(&mut self) -> Result<(), JsError> {
        self.inner_mut()?.scene.end_drag();
        Ok(())
    }

    /// Layout temperature (0..=1) kept while a node is dragged.
    pub fn set_drag_heat(&mut self, heat: f32) -> Result<(), JsError> {
        self.inner_mut()?
            .scene
            .set_drag_heat(heat)
            .map_err(js_error)
    }

    /// Pin exactly these nodes; an empty list unpins all.
    pub fn set_pinned(&mut self, indices: &[u32]) -> Result<(), JsError> {
        let inner = self.inner_mut()?;
        inner
            .scene
            .set_pinned(&inner.ctx.queue, indices)
            .map_err(js_error)
    }

    pub fn pinned(&mut self) -> Result<Vec<u32>, JsError> {
        Ok(self.inner_mut()?.scene.pinned().to_vec())
    }

    /// `op`: 0 replace, 1 add, 2 remove.
    pub fn select_nodes(&mut self, indices: &[u32], op: u8) -> Result<(), JsError> {
        let op = op_from(op)?;
        let inner = self.inner_mut()?;
        inner
            .scene
            .select_nodes(&inner.ctx.device, &inner.ctx.queue, indices, op)
            .map(|_| ())
            .map_err(js_error)
    }

    pub fn select_rect(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        op: u8,
    ) -> Result<(), JsError> {
        let op = op_from(op)?;
        let inner = self.inner_mut()?;
        inner
            .scene
            .select_in_rect(&inner.ctx.device, &inner.ctx.queue, [x0, y0], [x1, y1], op)
            .map(|_| ())
            .map_err(js_error)
    }

    /// `xy` is flat `x, y, x, y, ...` in physical pixels.
    pub fn select_polygon(&mut self, xy: &[f32], op: u8) -> Result<(), JsError> {
        if !xy.len().is_multiple_of(2) {
            return Err(JsError::new(
                "polygon needs x, y pairs (odd number of values)",
            ));
        }
        let op = op_from(op)?;
        let polygon: Vec<[f32; 2]> = xy.as_chunks::<2>().0.to_vec();
        let inner = self.inner_mut()?;
        inner
            .scene
            .select_in_polygon(&inner.ctx.device, &inner.ctx.queue, &polygon, op)
            .map(|_| ())
            .map_err(js_error)
    }

    /// Select what is under the pointer. Returns 0 = nothing, 1 = a node, 2 = a link.
    pub fn select_at(&mut self, x: f32, y: f32, neighbours: bool, op: u8) -> Result<u8, JsError> {
        let op = op_from(op)?;
        let inner = self.inner_mut()?;
        let hit = inner
            .scene
            .select_at(&inner.ctx.device, &inner.ctx.queue, x, y, neighbours, op)
            .map_err(js_error)?;
        Ok(match hit {
            SelectHit::None => 0,
            SelectHit::Node(_) => 1,
            SelectHit::Link(_) => 2,
        })
    }

    pub fn clear_selection(&mut self) -> Result<(), JsError> {
        let inner = self.inner_mut()?;
        inner.scene.clear_selection(&inner.ctx.queue);
        Ok(())
    }

    pub fn selection_active(&mut self) -> Result<bool, JsError> {
        Ok(self.inner_mut()?.scene.selection().is_active())
    }

    pub fn selected_nodes(&mut self) -> Result<Vec<u32>, JsError> {
        Ok(self.inner_mut()?.scene.selection().selected_nodes())
    }

    pub fn selected_link_count(&mut self) -> Result<u32, JsError> {
        Ok(self.inner_mut()?.scene.selection().link_count_selected() as u32)
    }

    /// Dim everything outside the selection (default) or draw it normally.
    pub fn set_dim_unselected(&mut self, on: bool) -> Result<(), JsError> {
        let inner = self.inner_mut()?;
        inner.scene.set_dim_unselected(&inner.ctx.queue, on);
        Ok(())
    }

    /// World position under a physical-pixel screen position, as `[x, y]`.
    pub fn screen_to_world(&mut self, x: f32, y: f32) -> Result<Vec<f32>, JsError> {
        let w = self.inner_mut()?.scene.camera().screen_to_world([x, y]);
        Ok(vec![w[0], w[1]])
    }

    /// Physical-pixel position of a node, or an empty array for an unknown index.
    pub fn node_screen_position(&mut self, index: u32) -> Result<Vec<f32>, JsError> {
        Ok(self
            .inner_mut()?
            .scene
            .node_screen_position(index as usize)
            .map_or_else(Vec::new, |p| vec![p[0], p[1]]))
    }
}
