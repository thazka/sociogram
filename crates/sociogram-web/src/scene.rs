use sociogram_core::{Camera, Graph, GraphData, GraphError, GridLayoutConfig, PickIndex};
use sociogram_gpu::{instances_from_graph, GpuError, GpuLayout, Renderer};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

mod capture;

use crate::lod::{adapt_lod, auto_space_half, LinkFade, LINK_FADE_AUTO_MIN_LINKS};
use capture::CaptureJob;
pub use capture::{Capture, MAX_CAPTURE_SIDE};

/// Screen-space padding (pixels) kept around the graph when fitting the view.
const FIT_PADDING_PX: f32 = 40.0;
/// Hover/click tolerance in screen pixels.
const PICK_RADIUS_PX: f32 = 6.0;

#[derive(Debug, Clone)]
pub struct SceneOptions {
    pub layout: GridLayoutConfig,
    /// Layout iterations per rendered frame (while the layout is not settled).
    pub steps_per_frame: u32,
    pub link_color: [f32; 4],
    pub link_width: f32,
    pub background: wgpu::Color,
    pub link_fade: LinkFade,
    /// Grow the layout space with the node count (see [`auto_space_half`]).
    /// Turn off to use `layout.space_half` exactly.
    pub auto_space: bool,
}

impl Default for SceneOptions {
    fn default() -> Self {
        SceneOptions {
            layout: GridLayoutConfig::default(),
            steps_per_frame: 1,
            link_color: [0.55, 0.60, 0.70, 0.35],
            link_width: 1.0,
            background: wgpu::Color {
                r: 0.043,
                g: 0.055,
                b: 0.075,
                a: 1.0,
            },
            link_fade: LinkFade::Auto,
            auto_space: true,
        }
    }
}

#[derive(Debug)]
pub enum SceneError {
    Graph(GraphError),
    Gpu(GpuError),
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SceneError::Graph(e) => write!(f, "invalid graph: {e}"),
            SceneError::Gpu(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SceneError {}

impl From<GraphError> for SceneError {
    fn from(e: GraphError) -> Self {
        SceneError::Graph(e)
    }
}

impl From<GpuError> for SceneError {
    fn from(e: GpuError) -> Self {
        SceneError::Gpu(e)
    }
}

type MapStatus = Arc<Mutex<Option<Result<(), String>>>>;

/// A position readback in flight.
struct Readback {
    buffer: wgpu::Buffer,
    status: MapStatus,
}

/// Everything needed to show one graph: data, GPU layout, renderer, camera and
/// a CPU snapshot of the positions for picking. Independent of any window or
/// canvas, so it can be tested with an offscreen texture.
pub struct Scene {
    options: SceneOptions,
    link_fade: LinkFade,
    link_count: usize,
    /// Smoothed frame time reported by the host, and the fade scale derived from it.
    frame_ema: Option<f32>,
    lod_scale: f32,
    format: wgpu::TextureFormat,
    capture: Option<CaptureJob>,
    camera: Camera,
    renderer: Renderer,
    layout: Option<GpuLayout>,
    max_size: f32,
    snapshot: Vec<[f32; 2]>,
    pick: PickIndex,
    readback: Option<Readback>,
    auto_fit: bool,
    /// True while a batch of layout steps is still executing on the GPU.
    layout_busy: Arc<AtomicBool>,
    /// Set by the GPU when everything submitted before `request_idle` has finished.
    idle_done: Arc<AtomicBool>,
    idle_waiting: bool,
}

fn bounds(points: &[[f32; 2]]) -> Option<([f32; 2], [f32; 2])> {
    let first = *points.first()?;
    let (mut lo, mut hi) = (first, first);
    for p in points {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    Some((lo, hi))
}

impl Scene {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        viewport: [f32; 2],
        options: SceneOptions,
    ) -> Self {
        Scene {
            link_fade: options.link_fade,
            link_count: 0,
            frame_ema: None,
            lod_scale: 1.0,
            options,
            format,
            capture: None,
            camera: Camera::new(viewport),
            renderer: Renderer::new(device, format),
            layout: None,
            max_size: 0.0,
            snapshot: Vec::new(),
            pick: PickIndex::build(&[], 1.0),
            readback: None,
            auto_fit: true,
            layout_busy: Arc::new(AtomicBool::new(false)),
            idle_done: Arc::new(AtomicBool::new(false)),
            idle_waiting: false,
        }
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn node_count(&self) -> usize {
        self.layout.as_ref().map_or(0, |l| l.node_count())
    }

    pub fn is_settled(&self) -> bool {
        self.layout.as_ref().is_none_or(|l| l.is_settled())
    }

    /// Replace the graph. On error the previous graph stays in place.
    pub fn set_data(&mut self, device: &wgpu::Device, data: GraphData) -> Result<(), SceneError> {
        let graph = Graph::new(data)?;
        if graph.node_count() == 0 {
            self.layout = None;
            self.link_count = 0;
            self.renderer.set_nodes(device, &[]);
            self.renderer.set_links(device, &[]);
            self.reset_snapshot(Vec::new(), 0.0);
            return Ok(());
        }
        let mut layout_config = self.options.layout;
        if self.options.auto_space {
            layout_config.space_half = auto_space_half(
                graph.node_count(),
                &layout_config.base,
                layout_config.space_half,
            );
        }
        let layout = GpuLayout::new(device, &graph, layout_config)?;
        let (nodes, links) =
            instances_from_graph(&graph, self.options.link_color, self.options.link_width);
        self.link_count = links.len();
        self.frame_ema = None;
        self.lod_scale = 1.0;
        self.renderer.set_nodes(device, &nodes);
        self.renderer.set_links(device, &links);
        self.renderer
            .set_positions(device, layout.positions_buffer());
        let max_size = graph.sizes().iter().copied().fold(0.0, f32::max);
        self.reset_snapshot(graph.positions().to_vec(), max_size);
        self.layout = Some(layout);
        Ok(())
    }

    fn reset_snapshot(&mut self, positions: Vec<[f32; 2]>, max_size: f32) {
        self.max_size = max_size;
        self.pick = PickIndex::build(&positions, 16.0);
        self.snapshot = positions;
        self.readback = None;
        self.auto_fit = true;
        self.fit_to_snapshot();
    }

    fn fit_to_snapshot(&mut self) {
        if let Some((lo, hi)) = bounds(&self.snapshot) {
            self.camera.fit_bounds(lo, hi, FIT_PADDING_PX);
        }
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.camera.viewport = [width.max(1.0), height.max(1.0)];
        if self.auto_fit {
            self.fit_to_snapshot();
        }
    }

    pub fn pan_by_pixels(&mut self, dx: f32, dy: f32) {
        self.auto_fit = false;
        self.camera.pan_by_pixels(dx, dy);
    }

    pub fn zoom_at(&mut self, factor: f32, x: f32, y: f32) {
        self.auto_fit = false;
        self.camera.zoom_at(factor, [x, y]);
    }

    /// Fit the view to the latest position snapshot and stop auto-fitting.
    pub fn fit_view(&mut self) {
        self.auto_fit = false;
        self.fit_to_snapshot();
    }

    /// Tell the scene how long the last frame took (milliseconds between
    /// presented frames). In [`LinkFade::Auto`] mode large graphs get a stricter
    /// link fade while frames are slow and a more generous one when there is
    /// headroom, so the page stays interactive on weaker GPUs.
    pub fn report_frame_time(&mut self, ms: f32) {
        (self.frame_ema, self.lod_scale) = adapt_lod(self.frame_ema, self.lod_scale, ms);
    }

    /// Current adaptive fade scale (1.0 = unscaled, never below [`LOD_MIN_SCALE`]).
    pub fn link_lod_scale(&self) -> f32 {
        self.lod_scale
    }

    /// Choose how links fade out by on-screen length; see [`LinkFade`].
    pub fn set_link_fade(&mut self, mode: LinkFade) {
        self.link_fade = mode;
    }

    /// Push the effective fade range to the renderer (depends on the viewport
    /// and, in `Auto` mode, on the number of links).
    fn apply_link_fade(&mut self) {
        let (start, end) = match self.link_fade {
            LinkFade::Off => (0.0, 0.0),
            LinkFade::Px { start, end } => (start, end),
            LinkFade::Auto if self.link_count >= LINK_FADE_AUTO_MIN_LINKS => {
                let side = self.camera.viewport[0].max(self.camera.viewport[1]);
                (side * 0.125 * self.lod_scale, side * 0.375 * self.lod_scale)
            }
            LinkFade::Auto => (0.0, 0.0),
        };
        self.renderer.set_link_fade(start, end);
    }

    /// Ask to be told when the GPU has finished everything submitted so far
    /// (pipelines compiled, buffers uploaded, the first layout steps and frame).
    /// Check with [`Scene::poll_idle`]; on native targets the caller must also
    /// poll the device. Asking again before the answer arrives starts over.
    pub fn request_idle(&mut self, queue: &wgpu::Queue) {
        self.idle_done.store(false, Ordering::Release);
        let done = Arc::clone(&self.idle_done);
        queue.on_submitted_work_done(move || done.store(true, Ordering::Release));
        self.idle_waiting = true;
    }

    /// True once, when the GPU has finished the work present at the last
    /// [`Scene::request_idle`]. False while still waiting and when nothing was asked.
    pub fn poll_idle(&mut self) -> bool {
        if self.idle_waiting && self.idle_done.load(Ordering::Acquire) {
            self.idle_waiting = false;
            return true;
        }
        false
    }

    /// Restart the layout cooling so the graph moves again.
    pub fn reheat(&mut self) {
        if let Some(l) = &mut self.layout {
            l.reheat();
        }
    }

    /// Run layout steps and draw one frame into `view`.
    pub fn frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView) {
        self.apply_link_fade();
        self.renderer.set_camera(queue, &self.camera);
        // Back-pressure: never queue a new batch of layout steps while the
        // previous one is still running. Without this the CPU submits work
        // faster than the GPU finishes it, the queue grows without bound and a
        // single frame can stall for many seconds on large graphs.
        if let Some(layout) = &mut self.layout {
            if !self.layout_busy.load(Ordering::Acquire) {
                let mut stepped = false;
                for _ in 0..self.options.steps_per_frame {
                    if layout.is_settled() {
                        break;
                    }
                    layout.step(device, queue);
                    stepped = true;
                }
                if stepped {
                    self.renderer
                        .set_positions(device, layout.positions_buffer());
                    self.layout_busy.store(true, Ordering::Release);
                    let busy = Arc::clone(&self.layout_busy);
                    queue.on_submitted_work_done(move || busy.store(false, Ordering::Release));
                }
            }
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        self.renderer
            .render(&mut encoder, view, self.options.background);
        queue.submit([encoder.finish()]);
    }

    /// Start copying the current positions to the CPU (for picking and fitting).
    /// Does nothing while a previous readback is still in flight. The result is
    /// picked up by [`Scene::poll_snapshot`]; on native targets the caller must
    /// also poll the device for the copy to complete.
    pub fn request_snapshot(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let Some(layout) = &self.layout else { return };
        // One readback at a time. It queues behind the layout steps already
        // submitted, so a snapshot arrives within a frame or two even while the
        // layout is running; the camera must keep following a growing graph.
        if self.readback.is_some() {
            return;
        }
        let size = layout.node_count() as u64 * std::mem::size_of::<[f32; 2]>() as u64;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sociogram snapshot"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(layout.positions_buffer(), 0, &buffer, 0, size);
        queue.submit([encoder.finish()]);

        let status: MapStatus = Arc::new(Mutex::new(None));
        let callback_status = Arc::clone(&status);
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            if let Ok(mut s) = callback_status.lock() {
                *s = Some(r.map_err(|e| e.to_string()));
            }
        });
        self.readback = Some(Readback { buffer, status });
    }

    /// Apply a finished readback. Returns true when the snapshot was updated.
    pub fn poll_snapshot(&mut self) -> bool {
        let Some(readback) = &self.readback else {
            return false;
        };
        let outcome = readback.status.lock().ok().and_then(|mut s| s.take());
        let Some(outcome) = outcome else { return false };
        let updated = if outcome.is_ok() {
            let mapped = readback
                .buffer
                .slice(..)
                .get_mapped_range()
                .expect("mapped range");
            let positions = bytemuck_positions(&mapped);
            drop(mapped);
            readback.buffer.unmap();
            self.pick = PickIndex::build(&positions, 16.0);
            self.snapshot = positions;
            if self.auto_fit {
                self.fit_to_snapshot();
            }
            true
        } else {
            false
        };
        self.readback = None;
        updated
    }

    /// Node under the screen position (physical pixels), if any, using the
    /// latest snapshot.
    pub fn pick(&self, x: f32, y: f32) -> Option<usize> {
        let world = self.camera.screen_to_world([x, y]);
        let radius = (PICK_RADIUS_PX / self.camera.zoom).max(self.max_size);
        self.pick.nearest_within(world, radius)
    }

    /// Latest CPU copy of the node positions.
    pub fn snapshot(&self) -> &[[f32; 2]] {
        &self.snapshot
    }
}

fn bytemuck_positions(bytes: &[u8]) -> Vec<[f32; 2]> {
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| {
            [
                f32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                f32::from_le_bytes([c[4], c[5], c[6], c[7]]),
            ]
        })
        .collect()
}
