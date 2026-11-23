use crate::instances::{LinkInstance, NodeInstance};
use sociogram_core::Camera;
use wgpu::util::DeviceExt;

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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sociogram pipeline layout"),
            bind_group_layouts: &[Some(&camera_layout), Some(&positions_layout)],
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
        }
    }

    pub fn set_camera(&self, queue: &wgpu::Queue, camera: &Camera) {
        let mut uniform = camera.uniform();
        uniform[6] = self.link_fade[0];
        uniform[7] = self.link_fade[1];
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&uniform));
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
