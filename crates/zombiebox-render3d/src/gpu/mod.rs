//! wgpu rendering context, offscreen targets, and T00 smoke rendering compatibility (`T03`).

pub mod buffers;
pub mod context;
pub mod instance_buffer;
pub mod offscreen;
pub mod pipeline;
pub mod uniforms;
mod visibility;

pub use buffers::*;
pub use context::*;
pub use instance_buffer::*;
pub use offscreen::*;
pub use pipeline::*;
pub use uniforms::*;

use crate::RgbaImage;
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
