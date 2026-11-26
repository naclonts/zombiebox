//! wgpu adapter/device/queue setup (`T03`).

use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum GpuError {
    NoAdapter,
    RequestDevice(wgpu::RequestDeviceError),
    SurfaceNotSupported,
    SurfaceAcquire(&'static str),
    BufferMap,
    ReadbackNotReady,
    ReadbackFailed,
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter => write!(f, "Failed to find a suitable wgpu adapter"),
            Self::RequestDevice(e) => write!(f, "Failed to request wgpu device: {e}"),
            Self::SurfaceNotSupported => write!(f, "Surface is not supported by adapter"),
            Self::SurfaceAcquire(msg) => write!(f, "wgpu surface acquire error: {msg}"),
            Self::BufferMap => write!(f, "Failed to map staging buffer for readback"),
            Self::ReadbackNotReady => write!(f, "Staging buffer readback map failed or timed out"),
            Self::ReadbackFailed => write!(f, "Staging buffer readback failed"),
        }
    }
}

impl Error for GpuError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RequestDevice(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub adapter_info: wgpu::AdapterInfo,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl GpuContext {
    pub fn new_headless() -> Result<Self, GpuError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        pollster::block_on(Self::from_instance_and_surface(instance, None))
    }

    pub async fn from_instance_and_surface(
        instance: wgpu::Instance,
        compatible_surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, GpuError> {
        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
        {
            Ok(adapter) => adapter,
            Err(_) => instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface,
                    force_fallback_adapter: true,
                    apply_limit_buckets: false,
                })
                .await
                .map_err(|_| GpuError::NoAdapter)?,
        };

        let adapter_info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("zombiebox_render3d_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(GpuError::RequestDevice)?;

        Ok(Self {
            instance,
            adapter,
            adapter_info,
            device,
            queue,
        })
    }
}
