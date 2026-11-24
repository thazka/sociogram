//! Dragging, pinning, selection and link picking for a [`Scene`].
//! Pointer positions are canvas (drawing-buffer) pixels, as for `Scene::pick`.

use super::{Scene, SceneError, PICK_RADIUS_PX};
use sociogram_core::{nearest_edge, nodes_in_polygon, nodes_in_rect, SelectOp, Selection};
use sociogram_gpu::{DIM_LINK_ALPHA, DIM_NODE_ALPHA};

/// Longest index list accepted by `select_nodes` and `set_pinned` (4 times the
/// largest graph the GPU layout supports).
pub const MAX_INDEX_LIST: usize = 2_000_000;
/// Most vertices accepted for a lasso polygon. Selection cost grows with
/// nodes times vertices.
pub const MAX_POLYGON_VERTICES: usize = 5_000;

fn invalid(message: impl Into<String>) -> SceneError {
    SceneError::Gpu(sociogram_gpu::GpuError::InvalidInput(message.into()))
}

fn finite2(a: [f32; 2]) -> bool {
    a[0].is_finite() && a[1].is_finite()
}

fn too_many(len: usize) -> SceneError {
    invalid(format!(
        "too many indices ({len}, at most {MAX_INDEX_LIST})"
    ))
}

/// What a click found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectHit {
    None,
    Node(usize),
    Link(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionSummary {
    pub nodes: usize,
    pub links: usize,
}

pub(super) struct InteractionState {
    pub(super) selection: Selection,
    /// For each edge, its link instance (`u32::MAX` = not drawn).
    pub(super) edge_instance: Vec<u32>,
    pub(super) pinned: Vec<u32>,
    dragging: Option<usize>,
    /// World-space vector from the pointer to the node when the drag began.
    grab_offset: [f32; 2],
    drag_heat: f32,
    dim: bool,
}

impl Default for InteractionState {
    fn default() -> Self {
        InteractionState {
            selection: Selection::new(0, 0),
            edge_instance: Vec::new(),
            pinned: Vec::new(),
            dragging: None,
            grab_offset: [0.0; 2],
            drag_heat: 0.3,
            dim: true,
        }
    }
}

impl Scene {
    /// New data: forget the drag, the pins and the selection, keep the settings.
    pub(super) fn reset_interaction(&mut self, nodes: usize, edges: usize) {
        let heat = self.interaction.drag_heat;
        let dim = self.interaction.dim;
        self.interaction = InteractionState {
            selection: Selection::new(nodes, edges),
            drag_heat: heat,
            dim,
            ..Default::default()
        };
        // The renderer's flag buffers belong to the old graph.
        self.renderer.reset_selection_state();
    }

    // ---- dragging

    pub fn dragging(&self) -> Option<usize> {
        self.interaction.dragging
    }

    /// Heat (0..=1) applied while a node is dragged; 0 does not warm the layout.
    pub fn set_drag_heat(&mut self, heat: f32) -> Result<(), SceneError> {
        if !(0.0..=1.0).contains(&heat) {
            return Err(SceneError::Gpu(sociogram_gpu::GpuError::InvalidInput(
                format!("drag heat {heat} is not in 0..=1"),
            )));
        }
        self.interaction.drag_heat = heat;
        Ok(())
    }

    /// Start dragging the node under the pointer. `None` when there is none.
    pub fn begin_drag(&mut self, x: f32, y: f32) -> Option<usize> {
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        let hit = self.pick(x, y)?;
        let node = *self.snapshot.get(hit)?;
        let pointer = self.camera.screen_to_world([x, y]);
        let heat = self.interaction.drag_heat;
        self.layout.as_mut()?.hold(hit as u32, node, heat).ok()?;
        self.interaction.grab_offset = [node[0] - pointer[0], node[1] - pointer[1]];
        self.interaction.dragging = Some(hit);
        self.auto_fit = false;
        Some(hit)
    }

    pub fn drag_to(&mut self, x: f32, y: f32) {
        if !(x.is_finite() && y.is_finite()) {
            return;
        }
        let Some(index) = self.interaction.dragging else {
            return;
        };
        let p = self.camera.screen_to_world([x, y]);
        let o = self.interaction.grab_offset;
        let heat = self.interaction.drag_heat;
        if let Some(layout) = &mut self.layout {
            let _ = layout.hold(index as u32, [p[0] + o[0], p[1] + o[1]], heat);
        }
    }

    pub fn end_drag(&mut self) {
        self.interaction.dragging = None;
        if let Some(layout) = &mut self.layout {
            layout.release();
        }
    }

    // ---- pinning

    pub fn pinned(&self) -> &[u32] {
        &self.interaction.pinned
    }

    pub fn set_pinned(&mut self, queue: &wgpu::Queue, indices: &[u32]) -> Result<(), SceneError> {
        if indices.len() > MAX_INDEX_LIST {
            return Err(too_many(indices.len()));
        }
        if let Some(layout) = &mut self.layout {
            layout.set_pinned(queue, indices)?;
        } else if !indices.is_empty() {
            return Err(SceneError::Gpu(sociogram_gpu::GpuError::InvalidInput(
                "there are no nodes to pin".into(),
            )));
        }
        let mut sorted = indices.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        self.interaction.pinned = sorted;
        Ok(())
    }

    // ---- selection

    pub fn selection(&self) -> &Selection {
        &self.interaction.selection
    }

    fn summary(&self) -> SelectionSummary {
        SelectionSummary {
            nodes: self.interaction.selection.node_count_selected(),
            links: self.interaction.selection.link_count_selected(),
        }
    }

    /// Choose whether unselected items are dimmed (default) or drawn normally.
    pub fn set_dim_unselected(&mut self, queue: &wgpu::Queue, on: bool) {
        self.interaction.dim = on;
        let (n, l) = if on {
            (DIM_NODE_ALPHA, DIM_LINK_ALPHA)
        } else {
            (1.0, 1.0)
        };
        self.renderer.set_dim_alpha(queue, n, l);
    }

    /// Upload the selection flags (indexed by node and by link instance).
    fn push_selection(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let sel = &self.interaction.selection;
        if !sel.is_active() {
            self.renderer.clear_selection(queue);
            return;
        }
        let nodes: Vec<u32> = sel.node_flags().iter().map(|&f| f as u32).collect();
        let mut links = vec![0u32; self.link_count];
        for (edge, &instance) in self.interaction.edge_instance.iter().enumerate() {
            if instance != u32::MAX && sel.link_flags()[edge] {
                links[instance as usize] = 1;
            }
        }
        self.renderer.set_selection(device, queue, &nodes, &links);
    }

    pub fn select_nodes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        indices: &[u32],
        op: SelectOp,
    ) -> Result<SelectionSummary, SceneError> {
        if indices.len() > MAX_INDEX_LIST {
            return Err(too_many(indices.len()));
        }
        let Some(graph) = &self.graph else {
            if indices.is_empty() {
                return Ok(SelectionSummary { nodes: 0, links: 0 });
            }
            return Err(SceneError::Selection(
                sociogram_core::SelectionError::NodeOutOfRange {
                    index: indices[0],
                    len: 0,
                },
            ));
        };
        self.interaction.selection.apply_nodes(graph, indices, op)?;
        self.push_selection(device, queue);
        Ok(self.summary())
    }

    pub fn select_in_rect(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        a: [f32; 2],
        b: [f32; 2],
        op: SelectOp,
    ) -> Result<SelectionSummary, SceneError> {
        if !(finite2(a) && finite2(b)) {
            return Err(invalid("rectangle corners must be finite numbers"));
        }
        let (wa, wb) = (
            self.camera.screen_to_world(a),
            self.camera.screen_to_world(b),
        );
        let found = nodes_in_rect(&self.snapshot, wa, wb);
        self.select_nodes(device, queue, &found, op)
    }

    pub fn select_in_polygon(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_polygon: &[[f32; 2]],
        op: SelectOp,
    ) -> Result<SelectionSummary, SceneError> {
        if screen_polygon.len() > MAX_POLYGON_VERTICES {
            return Err(invalid(format!(
                "polygon has {} vertices, at most {MAX_POLYGON_VERTICES}",
                screen_polygon.len()
            )));
        }
        if !screen_polygon.iter().copied().all(finite2) {
            return Err(invalid("polygon vertices must be finite numbers"));
        }
        let world: Vec<[f32; 2]> = screen_polygon
            .iter()
            .map(|&p| self.camera.screen_to_world(p))
            .collect();
        let found = nodes_in_polygon(&self.snapshot, &world);
        self.select_nodes(device, queue, &found, op)
    }

    /// Link under the pointer (edge index), by distance to its segment.
    pub fn pick_link(&self, x: f32, y: f32) -> Option<usize> {
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        let graph = self.graph.as_ref()?;
        let world = self.camera.screen_to_world([x, y]);
        let radius = PICK_RADIUS_PX / self.camera.zoom;
        nearest_edge(&self.snapshot, graph.edges(), world, radius)
    }

    /// Select what is under the pointer: a node (with its neighbours when
    /// `neighbours`), else a link with its endpoints. Nothing under the
    /// pointer leaves the selection unchanged and returns [`SelectHit::None`].
    pub fn select_at(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        x: f32,
        y: f32,
        neighbours: bool,
        op: SelectOp,
    ) -> Result<SelectHit, SceneError> {
        if !(x.is_finite() && y.is_finite()) {
            return Err(invalid("pointer position must be finite"));
        }
        if let Some(node) = self.pick(x, y) {
            let mut nodes = vec![node as u32];
            if neighbours {
                if let Some(g) = &self.graph {
                    nodes.extend_from_slice(g.neighbors_of(node));
                }
            }
            self.select_nodes(device, queue, &nodes, op)?;
            return Ok(SelectHit::Node(node));
        }
        if let Some(edge) = self.pick_link(x, y) {
            if let Some(graph) = &self.graph {
                self.interaction
                    .selection
                    .apply_link(graph, edge as u32, op, true)?;
                self.push_selection(device, queue);
            }
            return Ok(SelectHit::Link(edge));
        }
        Ok(SelectHit::None)
    }

    pub fn clear_selection(&mut self, queue: &wgpu::Queue) {
        self.interaction.selection.clear();
        self.renderer.clear_selection(queue);
    }

    /// Screen position (canvas pixels) of a node from the latest snapshot.
    pub fn node_screen_position(&self, index: usize) -> Option<[f32; 2]> {
        let p = *self.snapshot.get(index)?;
        Some(self.camera.world_to_screen(p))
    }
}
