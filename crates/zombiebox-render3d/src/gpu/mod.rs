//! wgpu rendering context, offscreen targets, and T00 smoke rendering compatibility (`T03`).

pub mod buffers;
pub mod pipeline;
pub mod uniforms;

pub use buffers::*;
pub use pipeline::*;
pub use uniforms::*;

use std::error::Error;
use std::fmt;

use crate::RgbaImage;

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
                required_limits: wgpu::Limits::downlevel_defaults(),
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

#[derive(Debug)]
pub struct OffscreenTarget {
    pub width: u32,
    pub height: u32,
    pub color_texture: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
    pub staging_buffer: wgpu::Buffer,
}

impl OffscreenTarget {
    #[must_use]
    pub fn new(device: &wgpu::Device, width: u32, height: u32, label: &str) -> Self {
        let w = width.max(1);
        let h = height.max(1);

        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("{label}_color")),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("{label}_depth")),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[wgpu::TextureFormat::Depth32Float],
        });
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let unpadded_bytes_per_row = w * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
        let staging_buffer_size = (padded_bytes_per_row * h) as wgpu::BufferAddress;

        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("{label}_staging")),
            size: staging_buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            width: w,
            height: h,
            color_texture,
            color_view,
            depth_texture,
            depth_view,
            staging_buffer,
        }
    }

    pub fn download_rgba8(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<RgbaImage, GpuError> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("offscreen_download_encoder"),
        });

        let unpadded_bytes_per_row = self.width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.color_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        queue.submit(Some(encoder.finish()));

        let buffer_slice = self.staging_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|_| GpuError::ReadbackFailed)?;

        match receiver.recv() {
            Ok(Ok(())) => {
                let data = buffer_slice
                    .get_mapped_range()
                    .map_err(|_| GpuError::BufferMap)?;
                let mut pixels = Vec::with_capacity((self.width * self.height * 4) as usize);
                if padded_bytes_per_row == unpadded_bytes_per_row {
                    pixels.extend_from_slice(&data);
                } else {
                    for row in 0..self.height {
                        let start = (row * padded_bytes_per_row) as usize;
                        let end = start + (self.width * 4) as usize;
                        pixels.extend_from_slice(&data[start..end]);
                    }
                }
                drop(data);
                self.staging_buffer.unmap();
                Ok(RgbaImage::new(self.width, self.height, pixels))
            }
            _ => Err(GpuError::ReadbackNotReady),
        }
    }
}

/// Preserved T00 smoke rendering helper for backwards compatibility.
pub fn render_smoke_cube(
    ctx: &GpuContext,
    target_width: u32,
    target_height: u32,
    time_secs: f32,
) -> Result<RgbaImage, GpuError> {
    let target = OffscreenTarget::new(&ctx.device, target_width, target_height, "smoke_target");

    let shader = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("smoke_shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../shaders/smoke.wgsl"
            ))),
        });

    let pipeline_layout = ctx
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("smoke_pipeline_layout"),
            bind_group_layouts: &[None],
            immediate_size: 0,
        });

    let pipeline = ctx
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("smoke_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("smoke_encoder"),
        });

    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("smoke_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.05,
                        g: 0.05,
                        b: 0.08,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.draw(0..3, 0..1);
    }

    ctx.queue.submit(Some(encoder.finish()));
    let _ = time_secs; // Used by smoke shader in T00 if needed
    target.download_rgba8(&ctx.device, &ctx.queue)
}

#[must_use]
pub fn select_surface_format(caps: &wgpu::SurfaceCapabilities) -> Option<wgpu::TextureFormat> {
    for fmt in &caps.formats {
        if matches!(
            fmt,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
        ) {
            return Some(*fmt);
        }
    }
    caps.formats.first().copied()
}

/// Writes an [`RgbaImage`] to `path` as an 8-bit RGBA PNG file.
pub fn write_rgba8_png(
    path: impl AsRef<std::path::Path>,
    image: &RgbaImage,
) -> std::io::Result<()> {
    image.save_png(path)
}
