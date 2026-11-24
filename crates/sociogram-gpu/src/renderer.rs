use crate::instances::{LinkInstance, NodeInstance};
use sociogram_core::Camera;
use wgpu::util::DeviceExt;

/// Alpha multiplier for nodes outside the selection while a selection is active.
pub const DIM_NODE_ALPHA: f32 = 0.25;
/// Alpha multiplier for links outside the selection while a selection is active.
pub const DIM_LINK_ALPHA: f32 = 0.1;

/// Draws nodes and links. Positions come from a storage buffer
/// (`array<vec2<f32>>`) that the caller binds with [`Renderer::set_positions`]:
/// typically the buffer of a `GpuLayout`, so the layout output never leaves
/// the GPU.
pub struct Renderer {
    node_pipeline: wgpu::RenderPipeline,
    link_pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind: wgpu::BindGroup,
    positions_layout: wgpu::BindGroupLayout,
    positions_bind: Option<wgpu::BindGroup>,
    nodes: Option<(wgpu::Buffer, u32)>,
    links: Option<(wgpu::Buffer, u32)>,
    /// Screen-length range (pixels) over which links fade out; `[0, 0]` = off.
    link_fade: [f32; 2],
    selection_layout: wgpu::BindGroupLayout,
    selection_bind: wgpu::BindGroup,
    node_flags: wgpu::Buffer,
    link_flags: wgpu::Buffer,
    dim_buffer: wgpu::Buffer,
    /// Alpha multipliers for unselected nodes and links.
    dim_alpha: [f32; 2],
    /// True while a selection is active (dimming applies).
    dim_active: bool,
}

fn vertex_bgl_entry(ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn vertex_entry(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn flag_buffer(device: &wgpu::Device, flags: &[u32]) -> wgpu::Buffer {
    // Zero-sized buffers cannot be bound, so pad empty data to one word.
    let one = [0u32];
    let data: &[u32] = if flags.is_empty() { &one } else { flags };
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("sociogram selection flags"),
        contents: bytemuck::cast_slice(data),
        usage: wgpu::BufferUsages::STORAGE,
    })
}

fn selection_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    node_flags: &wgpu::Buffer,
    link_flags: &wgpu::Buffer,
    dim: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sociogram selection bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: node_flags.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: link_flags.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: dim.as_entire_binding(),
            },
        ],
    })
}

impl Renderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sociogram shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sociogram camera layout"),
            entries: &[vertex_bgl_entry(wgpu::BufferBindingType::Uniform)],
        });
        let positions_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sociogram positions layout"),
            entries: &[vertex_bgl_entry(wgpu::BufferBindingType::Storage {
                read_only: true,
            })],
        });
        let selection_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sociogram selection layout"),
            entries: &[
                vertex_entry(0, wgpu::BufferBindingType::Storage { read_only: true }),
                vertex_entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
                vertex_entry(2, wgpu::BufferBindingType::Uniform),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sociogram pipeline layout"),
            bind_group_layouts: &[
                Some(&camera_layout),
                Some(&positions_layout),
                Some(&selection_layout),
            ],
            immediate_size: 0,
        });

        let node_attrs = wgpu::vertex_attr_array![0 => Float32, 1 => Float32x4];
        let link_attrs =
            wgpu::vertex_attr_array![0 => Uint32, 1 => Uint32, 2 => Float32x4, 3 => Float32];
        let make = |vs: &str, fs: &str, stride: usize, attrs: &[wgpu::VertexAttribute]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(vs),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: stride as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: attrs,
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let node_pipeline = make(
            "vs_node",
            "fs_node",
            std::mem::size_of::<NodeInstance>(),
            &node_attrs,
        );
        let link_pipeline = make(
            "vs_link",
            "fs_link",
            std::mem::size_of::<LinkInstance>(),
            &link_attrs,
        );

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sociogram camera"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sociogram camera bind"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let node_flags = flag_buffer(device, &[]);
        let link_flags = flag_buffer(device, &[]);
        let dim_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sociogram dim"),
            contents: bytemuck::cast_slice(&[0.0f32, DIM_NODE_ALPHA, DIM_LINK_ALPHA, 0.0]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let selection_bind = selection_bind(
            device,
            &selection_layout,
            &node_flags,
            &link_flags,
            &dim_buffer,
        );

        Renderer {
            node_pipeline,
            link_pipeline,
            camera_buffer,
            camera_bind,
            positions_layout,
            positions_bind: None,
            nodes: None,
            links: None,
            link_fade: [0.0, 0.0],
            selection_layout,
            selection_bind,
            node_flags,
            link_flags,
            dim_buffer,
            dim_alpha: [DIM_NODE_ALPHA, DIM_LINK_ALPHA],
            dim_active: false,
        }
    }

    /// Camera, link fade and dimming state for the next frame. Frames and
    /// captures both call this, so both carry the current dimming.
    pub fn set_camera(&self, queue: &wgpu::Queue, camera: &Camera) {
        let mut uniform = camera.uniform();
        uniform[6] = self.link_fade[0];
        uniform[7] = self.link_fade[1];
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&uniform));
        self.write_dim(queue);
    }

    fn write_dim(&self, queue: &wgpu::Queue) {
        let v = [
            if self.dim_active { 1.0f32 } else { 0.0 },
            self.dim_alpha[0],
            self.dim_alpha[1],
            0.0,
        ];
        queue.write_buffer(&self.dim_buffer, 0, bytemuck::cast_slice(&v));
    }

    fn rebuild_selection_bind(&mut self, device: &wgpu::Device) {
        self.selection_bind = selection_bind(
            device,
            &self.selection_layout,
            &self.node_flags,
            &self.link_flags,
            &self.dim_buffer,
        );
    }

    /// Dim everything outside the selection. `node_flags` has one 0/1 word per
    /// node, `link_flags` one per link *instance* (not per edge).
    pub fn set_selection(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        node_flags: &[u32],
        link_flags: &[u32],
    ) {
        self.node_flags = flag_buffer(device, node_flags);
        self.link_flags = flag_buffer(device, link_flags);
        self.rebuild_selection_bind(device);
        self.dim_active = true;
        self.write_dim(queue);
    }

    /// No selection: draw everything normally.
    pub fn clear_selection(&mut self, queue: &wgpu::Queue) {
        self.dim_active = false;
        self.write_dim(queue);
    }

    /// New data: the old flag buffers are meaningless. Turns dimming off; the
    /// next `set_camera` writes that to the GPU.
    pub fn reset_selection_state(&mut self) {
        self.dim_active = false;
    }

    /// Alpha multipliers (0..=1) for unselected nodes and links. `1.0, 1.0`
    /// draws them normally even while a selection is active.
    pub fn set_dim_alpha(&mut self, queue: &wgpu::Queue, nodes: f32, links: f32) {
        let clamp = |v: f32| if v.is_nan() { 1.0 } else { v.clamp(0.0, 1.0) };
        self.dim_alpha = [clamp(nodes), clamp(links)];
        self.write_dim(queue);
    }

    /// Fade links out between `start_px` and `end_px` of on-screen length, and
    /// skip drawing those beyond `end_px`. Pass `end_px <= 0` to draw all links.
    /// Invalid ranges (NaN, negative start, `end < start`) turn the fade off.
    pub fn set_link_fade(&mut self, start_px: f32, end_px: f32) {
        let valid =
            start_px.is_finite() && end_px.is_finite() && start_px >= 0.0 && end_px > start_px;
        self.link_fade = if valid {
            [start_px, end_px]
        } else {
            [0.0, 0.0]
        };
    }

    /// Bind the positions buffer (`array<vec2<f32>>`, `STORAGE` usage). Nothing
    /// is drawn until this has been called. Call again whenever the buffer
    /// changes, for example after each `GpuLayout::step`.
    pub fn set_positions(&mut self, device: &wgpu::Device, positions: &wgpu::Buffer) {
        self.positions_bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sociogram positions bind"),
            layout: &self.positions_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: positions.as_entire_binding(),
            }],
        }));
    }

    /// Upload CPU positions into a new buffer usable with `set_positions`.
    pub fn upload_positions(device: &wgpu::Device, positions: &[[f32; 2]]) -> wgpu::Buffer {
        // Zero-sized buffers cannot be bound, so pad empty data to one word.
        let padded = [0u8; 4];
        let contents: &[u8] = if positions.is_empty() {
            &padded
        } else {
            bytemuck::cast_slice(positions)
        };
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sociogram positions"),
            contents,
            usage: wgpu::BufferUsages::STORAGE,
        })
    }

    pub fn set_nodes(&mut self, device: &wgpu::Device, nodes: &[NodeInstance]) {
        self.nodes = Self::upload(device, bytemuck::cast_slice(nodes), nodes.len());
    }

    pub fn set_links(&mut self, device: &wgpu::Device, links: &[LinkInstance]) {
        self.links = Self::upload(device, bytemuck::cast_slice(links), links.len());
    }

    fn upload(device: &wgpu::Device, bytes: &[u8], count: usize) -> Option<(wgpu::Buffer, u32)> {
        if count == 0 {
            return None;
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sociogram instances"),
            contents: bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });
        Some((buffer, count as u32))
    }

    /// Record a render pass: links first, then nodes on top. The pass only
    /// clears when no positions are bound.
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        clear: wgpu::Color,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sociogram pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let Some(positions) = &self.positions_bind else {
            return;
        };
        pass.set_bind_group(0, &self.camera_bind, &[]);
        pass.set_bind_group(1, positions, &[]);
        pass.set_bind_group(2, &self.selection_bind, &[]);
        if let Some((buffer, count)) = &self.links {
            pass.set_pipeline(&self.link_pipeline);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..6, 0..*count);
        }
        if let Some((buffer, count)) = &self.nodes {
            pass.set_pipeline(&self.node_pipeline);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..6, 0..*count);
        }
    }
}
