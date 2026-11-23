use crate::context::GpuError;
use bytemuck::{Pod, Zeroable};
use sociogram_core::grid_layout::total_cells;
use sociogram_core::{Graph, GridLayoutConfig};
use wgpu::util::DeviceExt;

/// Per-cell fixed-point sums are `i32`; this many nodes in a single cell
/// (`FIXED_SCALE` = 4096 per node) is the most that cannot overflow.
pub const MAX_GPU_NODES: usize = 500_000;
const WORKGROUP_SIZE: u32 = 64;
/// One `vec4<i32>` per cell: count, sum_x, sum_y, unused.
const CELL_BYTES: usize = 16;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    counts: [u32; 4],
    a: [f32; 4],
    b: [f32; 4],
}

/// Force-directed layout running on the GPU. Positions are double buffered:
/// each step reads one buffer and writes the other, so results do not depend
/// on thread scheduling.
pub struct GpuLayout {
    config: GridLayoutConfig,
    node_count: u32,
    alpha: f32,
    params: wgpu::Buffer,
    positions: [wgpu::Buffer; 2],
    cells: wgpu::Buffer,
    binds: [wgpu::BindGroup; 2],
    cells_acc_bind: wgpu::BindGroup,
    cells_ro_bind: wgpu::BindGroup,
    accumulate: wgpu::ComputePipeline,
    step: wgpu::ComputePipeline,
    /// Index into `positions` of the buffer holding the latest positions.
    current: usize,
}

fn storage_init(
    device: &wgpu::Device,
    label: &str,
    bytes: &[u8],
    extra: wgpu::BufferUsages,
) -> wgpu::Buffer {
    // Zero-sized buffers cannot be bound, so pad empty data to one word.
    let padded;
    let contents = if bytes.is_empty() {
        padded = [0u8; 4];
        &padded[..]
    } else {
        bytes
    };
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents,
        usage: wgpu::BufferUsages::STORAGE | extra,
    })
}

fn bgl_entry(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

impl GpuLayout {
    pub fn new(
        device: &wgpu::Device,
        graph: &Graph,
        config: GridLayoutConfig,
    ) -> Result<Self, GpuError> {
        let n = graph.node_count();
        if n > MAX_GPU_NODES {
            return Err(GpuError::InvalidInput(format!(
                "{n} nodes exceeds the GPU layout limit of {MAX_GPU_NODES}"
            )));
        }
        config.validate().map_err(|e| GpuError::InvalidInput(e.0))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sociogram layout shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("layout.wgsl").into()),
        });

        use wgpu::BufferBindingType::{Storage, Uniform};
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sociogram layout bgl"),
            entries: &[
                bgl_entry(0, Uniform),
                bgl_entry(1, Storage { read_only: true }),
                bgl_entry(2, Storage { read_only: false }),
                bgl_entry(3, Storage { read_only: false }),
                bgl_entry(4, Storage { read_only: true }),
                bgl_entry(5, Storage { read_only: true }),
            ],
        });
        // The cell buffer is bound twice: atomics for accumulate (group 1) and
        // plain read-only data for step (group 2).
        let bgl_acc = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sociogram cells (atomic)"),
            entries: &[bgl_entry(0, Storage { read_only: false })],
        });
        let bgl_ro = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sociogram cells (read-only)"),
            entries: &[bgl_entry(0, Storage { read_only: true })],
        });
        let acc_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sociogram accumulate layout"),
            bind_group_layouts: &[Some(&bgl), Some(&bgl_acc)],
            immediate_size: 0,
        });
        let step_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sociogram step layout"),
            bind_group_layouts: &[Some(&bgl), None, Some(&bgl_ro)],
            immediate_size: 0,
        });
        let make = |entry: &str, layout: &wgpu::PipelineLayout| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let accumulate = make("accumulate", &acc_layout);
        let step = make("step", &step_layout);

        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sociogram layout params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let pos_bytes: &[u8] = bytemuck::cast_slice(graph.positions());
        let both = wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
        let positions = [
            storage_init(device, "positions A", pos_bytes, both),
            storage_init(device, "positions B", pos_bytes, both),
        ];
        let vel = storage_init(
            device,
            "velocities",
            &vec![0u8; n * std::mem::size_of::<[f32; 2]>()],
            wgpu::BufferUsages::empty(),
        );
        let cells = storage_init(
            device,
            "cells",
            &vec![0u8; total_cells(config.grid_dim) * CELL_BYTES],
            wgpu::BufferUsages::COPY_DST,
        );
        let offsets = storage_init(
            device,
            "csr offsets",
            bytemuck::cast_slice(graph.offsets()),
            wgpu::BufferUsages::empty(),
        );
        let neighbors = storage_init(
            device,
            "csr neighbors",
            bytemuck::cast_slice(graph.neighbors()),
            wgpu::BufferUsages::empty(),
        );

        let bind = |pos_in: &wgpu::Buffer, pos_out: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sociogram layout bind"),
                layout: &bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: pos_in.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: pos_out.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: vel.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: offsets.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: neighbors.as_entire_binding(),
                    },
                ],
            })
        };
        let binds = [
            bind(&positions[0], &positions[1]),
            bind(&positions[1], &positions[0]),
        ];
        let cells_bind = |layout: &wgpu::BindGroupLayout| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sociogram cells bind"),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: cells.as_entire_binding(),
                }],
            })
        };
        let cells_acc_bind = cells_bind(&bgl_acc);
        let cells_ro_bind = cells_bind(&bgl_ro);

        Ok(GpuLayout {
            config,
            node_count: n as u32,
            alpha: 1.0,
            params,
            positions,
            cells,
            binds,
            cells_acc_bind,
            cells_ro_bind,
            accumulate,
            step,
            current: 0,
        })
    }

    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    pub fn is_settled(&self) -> bool {
        self.alpha < self.config.base.alpha_min
    }

    pub fn node_count(&self) -> usize {
        self.node_count as usize
    }

    /// Buffer holding the latest positions (`array<vec2<f32>>`), usable as a
    /// storage buffer by the renderer. It changes after every `step`.
    pub fn positions_buffer(&self) -> &wgpu::Buffer {
        &self.positions[self.current]
    }

    /// Run one layout iteration and submit it. Each call uploads its own
    /// parameters, so several steps per frame are several calls.
    pub fn step(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let c = self.config.base;
        if self.node_count > 0 {
            let params = Params {
                counts: [self.node_count, self.config.grid_dim, 0, 0],
                a: [c.gravity, c.repulsion, c.link_spring, c.link_distance],
                b: [c.friction, self.alpha, self.config.space_half, 0.0],
            };
            queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));

            let groups = self.node_count.div_ceil(WORKGROUP_SIZE);
            let mut enc = device.create_command_encoder(&Default::default());
            enc.clear_buffer(&self.cells, 0, None);
            // Two passes: the cell buffer is written with atomics in the first
            // and read as plain data in the second, which must not share a
            // usage scope.
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("sociogram accumulate"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.accumulate);
                pass.set_bind_group(0, &self.binds[self.current], &[]);
                pass.set_bind_group(1, &self.cells_acc_bind, &[]);
                pass.dispatch_workgroups(groups, 1, 1);
            }
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("sociogram step"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.step);
                pass.set_bind_group(0, &self.binds[self.current], &[]);
                pass.set_bind_group(2, &self.cells_ro_bind, &[]);
                pass.dispatch_workgroups(groups, 1, 1);
            }
            queue.submit([enc.finish()]);
            self.current = 1 - self.current;
        }
        self.alpha *= 1.0 - c.alpha_decay;
    }

    /// Block until the GPU is idle and copy the latest positions back.
    /// Native only (it polls the device); intended for tests and tooling.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_positions(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<[f32; 2]> {
        let size = self.node_count as u64 * std::mem::size_of::<[f32; 2]>() as u64;
        if size == 0 {
            return Vec::new();
        }
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("positions readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(self.positions_buffer(), 0, &staging, 0, size);
        queue.submit([enc.finish()]);

        let slice = staging.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device poll failed");
        let mapped = slice.get_mapped_range().expect("mapped range");
        let out = bytemuck::pod_collect_to_vec::<u8, [f32; 2]>(&mapped);
        drop(mapped);
        out
    }
}

impl GpuLayout {
    /// Restart the cooling schedule so the layout moves again (for example
    /// after the data or the settings changed).
    pub fn reheat(&mut self) {
        self.alpha = 1.0;
    }
}
