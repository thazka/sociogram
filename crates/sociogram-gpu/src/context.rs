use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum GpuError {
    NoAdapter(String),
    Device(String),
    /// The caller passed data or settings the GPU layout cannot handle.
    InvalidInput(String),
    /// The surface (canvas or window) could not be created or configured.
    Surface(String),
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuError::NoAdapter(e) => write!(f, "no suitable GPU adapter: {e}"),
            GpuError::Device(e) => write!(f, "failed to create GPU device: {e}"),
            GpuError::InvalidInput(e) => write!(f, "invalid input: {e}"),
            GpuError::Surface(e) => write!(f, "surface error: {e}"),
        }
    }
}

impl std::error::Error for GpuError {}

pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl GpuContext {
    /// Create a context without a window or surface (tests, offscreen use).
    ///
    /// Windows defaults to DX12 because the Vulkan backend crashes in
    /// `request_device` on some Intel drivers; `WGPU_BACKEND` overrides this.
    pub async fn new_headless() -> Result<Self, GpuError> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        if cfg!(windows) {
            desc.backends = wgpu::Backends::DX12;
        }
        let instance = wgpu::Instance::new(desc.with_env());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .map_err(|e| GpuError::NoAdapter(e.to_string()))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| GpuError::Device(e.to_string()))?;
        Ok(GpuContext {
            instance,
            adapter,
            device,
            queue,
        })
    }
}

impl GpuContext {
    /// Create a context that renders to a surface (a window or a browser
    /// canvas). The adapter is chosen to be compatible with the surface.
    pub async fn new_for_surface(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
    ) -> Result<(Self, wgpu::Surface<'static>), GpuError> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        if cfg!(windows) {
            desc.backends = wgpu::Backends::DX12;
        }
        let instance = wgpu::Instance::new(desc.with_env());
        let surface = instance
            .create_surface(target)
            .map_err(|e| GpuError::Surface(e.to_string()))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|e| GpuError::NoAdapter(e.to_string()))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| GpuError::Device(e.to_string()))?;
        Ok((
            GpuContext {
                instance,
                adapter,
                device,
                queue,
            },
            surface,
        ))
    }
}
