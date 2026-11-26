//! Headless `wgpu` device initialization, offscreen render target, and smoke render.

use std::fmt;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::mpsc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::RgbaImage;

#[derive(Debug)]
pub enum GpuError {
    NoAdapter(String),
    RequestDevice(String),
    Poll(String),
    Readback(String),
    PngIo(String),
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter(msg) => write!(f, "no suitable GPU adapter: {msg}"),
            Self::RequestDevice(msg) => write!(f, "failed to request GPU device: {msg}"),
            Self::Poll(msg) => write!(f, "GPU device poll failed: {msg}"),
            Self::Readback(msg) => write!(f, "GPU readback failed: {msg}"),
            Self::PngIo(msg) => write!(f, "PNG I/O failed: {msg}"),
        }
    }
}

impl std::error::Error for GpuError {}

#[derive(Debug)]
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: wgpu::AdapterInfo,
}

impl GpuContext {
    /// Initializes a headless `wgpu` device and queue without a window/display handle.
    ///
    /// Fallback order for desktop/CI/sandbox resilience:
    /// 1. `RequestAdapterOptions` with `PowerPreference::HighPerformance`, `force_fallback_adapter: false`
    /// 2. `RequestAdapterOptions` with `force_fallback_adapter: true` (e.g. Mesa Lavapipe software Vulkan)
    /// 3. `instance.enumerate_adapters(wgpu::Backends::all())` returning the first adapter whose
    ///    `request_device(&wgpu::DeviceDescriptor::default())` succeeds.
    pub fn new_headless() -> Result<Self, GpuError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());

        let high_perf =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                ..Default::default()
            }));
        if let Ok(adapter) = high_perf
            && let Ok(ctx) = Self::from_adapter(instance.clone(), adapter)
        {
            return Ok(ctx);
        }

        let fallback = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: true,
            ..Default::default()
        }));
        if let Ok(adapter) = fallback
            && let Ok(ctx) = Self::from_adapter(instance.clone(), adapter)
        {
            return Ok(ctx);
        }

        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
        let mut last_err = String::from("no adapters enumerated across all backends");
        for adapter in adapters {
            match Self::from_adapter(instance.clone(), adapter) {
                Ok(ctx) => return Ok(ctx),
                Err(err) => last_err = err.to_string(),
            }
        }

        Err(GpuError::NoAdapter(last_err))
    }

    fn from_adapter(instance: wgpu::Instance, adapter: wgpu::Adapter) -> Result<Self, GpuError> {
        let adapter_info = adapter.get_info();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|err| GpuError::RequestDevice(err.to_string()))?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            adapter_info,
        })
    }
}

#[derive(Debug)]
pub struct OffscreenTarget {
    pub width: u32,
    pub height: u32,
    pub padded_bytes_per_row: u32,
    pub color_texture: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
    pub staging_buffer: wgpu::Buffer,
}

impl OffscreenTarget {
    pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
    pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

    #[must_use]
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        assert!(
            width > 0 && height > 0,
            "OffscreenTarget dimensions must be > 0"
        );
        let unpadded_bytes_per_row = width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen_color_texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen_depth_texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("offscreen_staging_buffer"),
            size: u64::from(padded_bytes_per_row) * u64::from(height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            width,
            height,
            padded_bytes_per_row,
            color_texture,
            color_view,
            depth_texture,
            depth_view,
            staging_buffer,
        }
    }

    /// Encodes `copy_texture_to_buffer` from `color_texture` into `staging_buffer`.
    pub fn encode_copy_to_staging(&self, encoder: &mut wgpu::CommandEncoder) {
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
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Maps `staging_buffer`, polls `ctx.device` with `PollType::wait_indefinitely()`,
    /// strips `COPY_BYTES_PER_ROW_ALIGNMENT` (256-byte) row padding, unmaps, and returns `RgbaImage`.
    pub fn readback_rgba8(&self, ctx: &GpuContext) -> Result<RgbaImage, GpuError> {
        let slice = self.staging_buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });

        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|err| GpuError::Poll(err.to_string()))?;

        rx.recv()
            .map_err(|err| GpuError::Readback(format!("map_async channel closed: {err}")))?
            .map_err(|err| GpuError::Readback(err.to_string()))?;

        let unpadded_bytes_per_row = (self.width * 4) as usize;
        let padded_bytes_per_row = self.padded_bytes_per_row as usize;
        let mut pixels = Vec::with_capacity(unpadded_bytes_per_row * self.height as usize);
        {
            let data = slice
                .get_mapped_range()
                .map_err(|err| GpuError::Readback(err.to_string()))?;
            for row in 0..self.height as usize {
                let start = row * padded_bytes_per_row;
                pixels.extend_from_slice(&data[start..start + unpadded_bytes_per_row]);
            }
        }
        self.staging_buffer.unmap();

        Ok(RgbaImage::new(self.width, self.height, pixels))
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct SmokeVertex {
    position: [f32; 3],
    color: [f32; 3],
}

impl SmokeVertex {
    const ATTRS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct SmokeUniforms {
    mvp: [[f32; 4]; 4],
}

fn smoke_cube_geometry() -> (Vec<SmokeVertex>, Vec<u16>) {
    let h = 0.6_f32;
    let faces: [([[f32; 3]; 4], [f32; 3]); 6] = [
        // +Y top
        (
            [[-h, h, h], [h, h, h], [h, h, -h], [-h, h, -h]],
            [0.30, 0.82, 0.55],
        ),
        // +X right
        (
            [[h, -h, h], [h, -h, -h], [h, h, -h], [h, h, h]],
            [0.92, 0.45, 0.30],
        ),
        // +Z front
        (
            [[-h, -h, h], [h, -h, h], [h, h, h], [-h, h, h]],
            [0.32, 0.58, 0.92],
        ),
        // -X left
        (
            [[-h, -h, -h], [-h, -h, h], [-h, h, h], [-h, h, -h]],
            [0.65, 0.32, 0.75],
        ),
        // -Z back
        (
            [[h, -h, -h], [-h, -h, -h], [-h, h, -h], [h, h, -h]],
            [0.88, 0.76, 0.28],
        ),
        // -Y bottom
        (
            [[-h, -h, -h], [h, -h, -h], [h, -h, h], [-h, -h, h]],
            [0.25, 0.28, 0.35],
        ),
    ];

    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (quad, color) in faces {
        let base = vertices.len() as u16;
        for position in quad {
            vertices.push(SmokeVertex { position, color });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}

/// Renders a depth-tested, CCW-wound 3D colored unit cube centered at the origin in `+Y`-up
/// right-handed coordinates (`eye = (2.2, 1.8, 2.6)` looking at `Vec3::ZERO`, `[0, 1]` depth)
/// over a dusk background `rgba(18, 20, 30, 255)` and returns the readback `RgbaImage`.
pub fn render_smoke_cube(ctx: &GpuContext, width: u32, height: u32) -> Result<RgbaImage, GpuError> {
    let target = OffscreenTarget::new(&ctx.device, width, height);
    let (vertices, indices) = smoke_cube_geometry();

    let vertex_buffer = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("smoke_vertex_buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
    let index_buffer = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("smoke_index_buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

    let proj = glam::Mat4::perspective_rh(
        40.0_f32.to_radians(),
        width as f32 / height as f32,
        0.1,
        20.0,
    );
    let view = glam::Mat4::look_at_rh(
        glam::Vec3::new(2.2, 1.8, 2.6),
        glam::Vec3::ZERO,
        glam::Vec3::Y,
    );
    let uniforms = SmokeUniforms {
        mvp: (proj * view).to_cols_array_2d(),
    };
    let uniform_buffer = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("smoke_uniform_buffer"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

    let bind_group_layout = ctx
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("smoke_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("smoke_bind_group"),
        layout: &bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buffer.as_entire_binding(),
        }],
    });

    let shader = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("smoke_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/smoke.wgsl").into()),
        });

    let pipeline_layout = ctx
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("smoke_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
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
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(SmokeVertex::layout())],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: OffscreenTarget::COLOR_FORMAT,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: OffscreenTarget::DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
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
                        r: 18.0 / 255.0,
                        g: 20.0 / 255.0,
                        b: 30.0 / 255.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
    }

    target.encode_copy_to_staging(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));

    target.readback_rgba8(ctx)
}

/// Encodes an `RgbaImage` to disk as an 8-bit RGBA PNG using `png 0.18.1`.
pub fn write_rgba8_png(path: &Path, image: &RgbaImage) -> Result<(), GpuError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|err| GpuError::PngIo(err.to_string()))?;
    }
    let file = File::create(path).map_err(|err| GpuError::PngIo(err.to_string()))?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png_writer = encoder
        .write_header()
        .map_err(|err| GpuError::PngIo(err.to_string()))?;
    png_writer
        .write_image_data(&image.pixels)
        .map_err(|err| GpuError::PngIo(err.to_string()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn approx_eq_pixel(a: [u8; 4], b: [u8; 4]) -> bool {
        (0..4).all(|i| a[i].abs_diff(b[i]) <= 1)
    }

    #[test]
    fn headless_smoke_cube_renders_and_roundtrips_png() {
        let ctx = GpuContext::new_headless().expect("headless GPU context");
        let img = render_smoke_cube(&ctx, 200, 150).expect("render smoke cube");
        assert_eq!(img.width, 200);
        assert_eq!(img.height, 150);

        let bg = [18, 20, 30, 255];
        assert!(
            approx_eq_pixel(img.pixel(2, 2), bg),
            "corner pixel (2,2) was {:?}, expected ~{bg:?}",
            img.pixel(2, 2)
        );
        assert!(
            !approx_eq_pixel(img.pixel(100, 75), bg),
            "center pixel (100,75) should hit cube, got background"
        );

        let mut distinct_cube_colors: HashSet<[u8; 4]> = HashSet::new();
        for y in 0..img.height {
            for x in 0..img.width {
                let px = img.pixel(x, y);
                if !approx_eq_pixel(px, bg) {
                    distinct_cube_colors.insert(px);
                }
            }
        }
        assert!(
            distinct_cube_colors.len() >= 3,
            "expected at least 3 distinct face colors on rendered cube, found {}",
            distinct_cube_colors.len()
        );

        let tmp_path = std::env::temp_dir().join("zombiebox-render3d-smoke-test.png");
        write_rgba8_png(&tmp_path, &img).expect("write PNG");

        let file = File::open(&tmp_path).expect("open written PNG");
        let decoder = png::Decoder::new(std::io::BufReader::new(file));
        let mut reader = decoder.read_info().expect("read PNG header");
        let mut decoded = vec![0; reader.output_buffer_size().expect("PNG buffer size")];
        let info = reader.next_frame(&mut decoded).expect("decode PNG frame");
        assert_eq!(info.width, 200);
        assert_eq!(info.height, 150);
        assert_eq!(&decoded[..info.buffer_size()], img.pixels.as_slice());

        let _ = std::fs::remove_file(&tmp_path);
    }
}
