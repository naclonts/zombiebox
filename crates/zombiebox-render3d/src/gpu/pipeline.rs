//! GpuRenderer pipeline, static chunk pool drawing, instancing, and nearest-neighbor upscale pass (`T03`).

use glam::{UVec2, Vec3};

use super::buffers::{
    ChunkMeshPool, GpuMesh, InstanceBatch, InstanceRaw, PrototypeMeshId, instance_raw_layout,
    world_vertex_layout,
};
use super::instance_buffer::DynamicInstanceBuffer;
use super::uniforms::{SceneLightingUniforms, UpscaleUniforms};
use super::{GpuContext, GpuError, OffscreenTarget};
use crate::RgbaImage;
use crate::camera::CameraFrame;
use crate::coords::{ChunkCoord, RenderAabb};
use crate::mesh::{MeshBuffer, WorldVertex};

/// Controls how internal low-resolution target dimensions relate to output viewport dimensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InternalResolutionPolicy {
    #[default]
    FromCameraFrame,
    AutoRetro {
        max_short_side: u32,
        max_long_side: u32,
    },
    Fixed(UVec2),
    Native1To1,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderPipelineConfig {
    pub clear_color_rgba: [f32; 4],
    pub resolution_policy: InternalResolutionPolicy,
    pub frustum_cull_chunks: bool,
}

impl Default for RenderPipelineConfig {
    fn default() -> Self {
        Self {
            clear_color_rgba: [18.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 1.0],
            resolution_policy: InternalResolutionPolicy::FromCameraFrame,
            frustum_cull_chunks: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    pub internal_size: UVec2,
    pub output_size: UVec2,
    pub used_upscale_pass: bool,
    pub chunks_resident: u32,
    pub chunks_drawn: u32,
    pub chunks_culled: u32,
    pub prototype_batches_drawn: u32,
    pub instances_drawn: u32,
    pub triangles_submitted: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceFrameOutcome {
    Presented { suboptimal: bool, stats: FrameStats },
    SkippedTimeout,
    SkippedOccluded,
    NeedsReconfigure,
    SurfaceLost,
}

struct PrototypeRecord {
    mesh: GpuMesh,
}

pub struct GpuRenderer {
    config: RenderPipelineConfig,
    world_pipeline: wgpu::RenderPipeline,
    upscale_pipeline_cache: std::collections::HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    world_bind_group: wgpu::BindGroup,
    visibility: super::visibility::VisibilityTexture,
    upscale_bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,

    camera_uniform_buffer: wgpu::Buffer,
    lighting_uniform_buffer: wgpu::Buffer,
    upscale_uniform_buffer: wgpu::Buffer,

    identity_instance_buffer: wgpu::Buffer,
    chunk_pool: ChunkMeshPool,
    prototypes: Vec<PrototypeRecord>,
    dynamic_instance_buffer: DynamicInstanceBuffer,

    internal_target: Option<OffscreenTarget>,
    upscale_readback_target: Option<UpscaleReadbackTarget>,
    last_stats: FrameStats,
}

struct UpscaleReadbackTarget {
    width: u32,
    height: u32,
    color_texture: wgpu::Texture,
    color_view: wgpu::TextureView,
    staging_buffer: wgpu::Buffer,
}

impl UpscaleReadbackTarget {
    fn new(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("upscale_readback_color"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[format],
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let unpadded = w * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("upscale_readback_staging"),
            size: (padded * h) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            width: w,
            height: h,
            color_texture,
            color_view,
            staging_buffer,
        }
    }

    fn download(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<RgbaImage, GpuError> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("upscale_download_encoder"),
        });
        let unpadded = self.width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;

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
                    bytes_per_row: Some(padded),
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

        let slice = self.staging_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = sender.send(res);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|_| GpuError::ReadbackFailed)?;

        match receiver.recv() {
            Ok(Ok(())) => {
                let data = slice.get_mapped_range().map_err(|_| GpuError::BufferMap)?;
                let mut pixels = Vec::with_capacity((self.width * self.height * 4) as usize);
                if padded == unpadded {
                    pixels.extend_from_slice(&data);
                } else {
                    for r in 0..self.height {
                        let start = (r * padded) as usize;
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

impl GpuRenderer {
    #[must_use]
    pub fn new(ctx: &GpuContext, config: RenderPipelineConfig) -> Self {
        let bind_group_layout =
            ctx.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("world_bind_group_layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: Some(
                                    wgpu::BufferSize::new(std::mem::size_of::<
                                        crate::camera::CameraGpuUniforms,
                                    >()
                                        as u64)
                                    .unwrap(),
                                ),
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: Some(
                                    wgpu::BufferSize::new(
                                        std::mem::size_of::<SceneLightingUniforms>() as u64,
                                    )
                                    .unwrap(),
                                ),
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Uint,
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                    ],
                });

        let upscale_bind_group_layout =
            ctx.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("upscale_bind_group_layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: Some(
                                    wgpu::BufferSize::new(
                                        std::mem::size_of::<UpscaleUniforms>() as u64
                                    )
                                    .unwrap(),
                                ),
                            },
                            count: None,
                        },
                    ],
                });

        let pipeline_layout = ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("world_pipeline_layout"),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            });

        let world_shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("world_shader"),
                source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                    "../shaders/world.wgsl"
                ))),
            });

        let vertex_buffers = [Some(world_vertex_layout()), Some(instance_raw_layout())];

        let world_pipeline = ctx
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("world_render_pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &world_shader,
                    entry_point: Some("vs_main"),
                    buffers: &vertex_buffers,
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &world_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview_mask: None,
                cache: None,
            });

        let sampler = ctx.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        use wgpu::util::DeviceExt;
        let camera_uniform_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera_uniform_buffer"),
            size: std::mem::size_of::<crate::camera::CameraGpuUniforms>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let lighting_uniform_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lighting_uniform_buffer"),
            size: std::mem::size_of::<SceneLightingUniforms>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let upscale_uniform_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("upscale_uniform_buffer"),
            size: std::mem::size_of::<UpscaleUniforms>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let identity_instance_buffer =
            ctx.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("identity_instance_buffer"),
                    contents: bytemuck::cast_slice(&[InstanceRaw::IDENTITY]),
                    usage: wgpu::BufferUsages::VERTEX,
                });

        let visibility = super::visibility::VisibilityTexture::new(&ctx.device);
        let world_bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: lighting_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&visibility.view),
                },
            ],
        });

        Self {
            config,
            world_pipeline,
            upscale_pipeline_cache: std::collections::HashMap::new(),
            world_bind_group,
            visibility,
            upscale_bind_group_layout,
            sampler,
            camera_uniform_buffer,
            lighting_uniform_buffer,
            upscale_uniform_buffer,
            identity_instance_buffer,
            chunk_pool: ChunkMeshPool::new(),
            prototypes: Vec::new(),
            dynamic_instance_buffer: DynamicInstanceBuffer::new(),
            internal_target: None,
            upscale_readback_target: None,
            last_stats: FrameStats::default(),
        }
    }

    #[must_use]
    pub const fn config(&self) -> &RenderPipelineConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut RenderPipelineConfig {
        &mut self.config
    }

    #[must_use]
    pub const fn last_frame_stats(&self) -> FrameStats {
        self.last_stats
    }

    #[must_use]
    pub const fn chunks(&self) -> &ChunkMeshPool {
        &self.chunk_pool
    }

    pub fn chunks_mut(&mut self) -> &mut ChunkMeshPool {
        &mut self.chunk_pool
    }

    pub(crate) fn update_visibility(
        &mut self,
        ctx: &GpuContext,
        scene: &crate::scene::DynamicSceneState,
    ) {
        self.visibility.update(&ctx.queue, scene);
    }

    pub fn upload_chunk(
        &mut self,
        ctx: &GpuContext,
        coord: ChunkCoord,
        revision: u64,
        aabb: RenderAabb,
        mesh: &MeshBuffer,
    ) -> bool {
        self.chunk_pool
            .upload_chunk(&ctx.device, coord, revision, aabb, mesh)
    }

    pub fn register_prototype(
        &mut self,
        ctx: &GpuContext,
        label: &str,
        mesh: &MeshBuffer,
    ) -> PrototypeMeshId {
        let gpu_mesh = GpuMesh::upload(&ctx.device, label, mesh).unwrap_or_else(|| {
            // Fallback degenerate mesh if empty
            let empty_mesh = MeshBuffer {
                vertices: vec![WorldVertex::new(Vec3::ZERO, Vec3::Y, [1.0; 4])],
                indices: vec![0, 0, 0],
            };
            GpuMesh::upload(&ctx.device, label, &empty_mesh).unwrap()
        });
        let id = self.prototypes.len() as u32;
        self.prototypes.push(PrototypeRecord { mesh: gpu_mesh });
        PrototypeMeshId(id)
    }

    pub fn update_prototype(
        &mut self,
        ctx: &GpuContext,
        id: PrototypeMeshId,
        label: &str,
        mesh: &MeshBuffer,
    ) {
        if let Some(record) = self.prototypes.get_mut(id.0 as usize) {
            if let Some(gpu_mesh) = GpuMesh::upload(&ctx.device, label, mesh) {
                record.mesh = gpu_mesh;
            }
        }
    }

    fn get_or_create_upscale_pipeline(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> &wgpu::RenderPipeline {
        self.upscale_pipeline_cache
            .entry(format)
            .or_insert_with(|| {
                let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("upscale_pipeline_layout"),
                    bind_group_layouts: &[Some(&self.upscale_bind_group_layout)],
                    immediate_size: 0,
                });
                let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("upscale_shader"),
                    source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                        "../shaders/upscale.wgsl"
                    ))),
                });
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("upscale_render_pipeline"),
                    layout: Some(&layout),
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
                            format,
                            blend: Some(wgpu::BlendState::REPLACE),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
            })
    }

    fn resolve_dimensions(&self, camera: &CameraFrame) -> (UVec2, UVec2) {
        let output_size = camera.viewport_size;
        let internal_size = match self.config.resolution_policy {
            InternalResolutionPolicy::FromCameraFrame => camera.internal_size,
            InternalResolutionPolicy::AutoRetro {
                max_short_side,
                max_long_side,
            } => crate::camera::compute_internal_resolution(
                output_size.x,
                output_size.y,
                max_short_side,
                max_long_side,
            ),
            InternalResolutionPolicy::Fixed(size) => size,
            InternalResolutionPolicy::Native1To1 => output_size,
        };
        (internal_size, output_size)
    }

    pub fn render_offscreen_rgba8(
        &mut self,
        ctx: &GpuContext,
        camera: &CameraFrame,
        lighting: &SceneLightingUniforms,
        instances: &[InstanceBatch<'_>],
        transient_mesh: Option<&MeshBuffer>,
    ) -> Result<RgbaImage, GpuError> {
        let (internal_size, output_size) = self.resolve_dimensions(camera);

        if self
            .internal_target
            .as_ref()
            .is_none_or(|t| t.width != internal_size.x || t.height != internal_size.y)
        {
            self.internal_target = Some(OffscreenTarget::new(
                &ctx.device,
                internal_size.x,
                internal_size.y,
                "internal_render_target",
            ));
        }

        let needs_upscale = internal_size != output_size;
        if needs_upscale {
            if self
                .upscale_readback_target
                .as_ref()
                .is_none_or(|t| t.width != output_size.x || t.height != output_size.y)
            {
                self.upscale_readback_target = Some(UpscaleReadbackTarget::new(
                    &ctx.device,
                    output_size.x,
                    output_size.y,
                    wgpu::TextureFormat::Rgba8Unorm,
                ));
            }
            let _ =
                self.get_or_create_upscale_pipeline(&ctx.device, wgpu::TextureFormat::Rgba8Unorm);
        }

        // Write uniform buffers
        let cam_uniforms = camera.to_gpu_uniforms();
        ctx.queue.write_buffer(
            &self.camera_uniform_buffer,
            0,
            bytemuck::bytes_of(&cam_uniforms),
        );
        let local_lighting = lighting.relative_to(camera.render_origin());
        ctx.queue.write_buffer(
            &self.lighting_uniform_buffer,
            0,
            bytemuck::bytes_of(&local_lighting),
        );

        let target = self.internal_target.as_ref().unwrap();
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render_encoder"),
            });

        // Pack every instanced batch into one upload up front. Each draw call
        // below gets its own stable range, because command encoding does not
        // snapshot the buffer contents.
        let prepared_batches = self.dynamic_instance_buffer.prepare_batches(
            &ctx.device,
            &ctx.queue,
            instances,
            "instance_buffer",
        );

        let mut chunks_drawn = 0_u32;
        let mut chunks_culled = 0_u32;
        let mut prototype_batches_drawn = 0_u32;
        let mut instances_drawn = 0_u32;
        let mut triangles_submitted = 0_u64;

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.config.clear_color_rgba[0] as f64,
                            g: self.config.clear_color_rgba[1] as f64,
                            b: self.config.clear_color_rgba[2] as f64,
                            a: self.config.clear_color_rgba[3] as f64,
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
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.world_pipeline);
            pass.set_bind_group(0, &self.world_bind_group, &[]);

            // Draw chunks
            let sorted_coords = self.chunk_pool.sorted_coords();
            let chunks_resident = sorted_coords.len() as u32;

            for coord in sorted_coords {
                if let Some(slot) = self.chunk_pool.get(coord) {
                    if self.config.frustum_cull_chunks && !camera.frustum.intersects_aabb(slot.aabb)
                    {
                        chunks_culled += 1;
                        continue;
                    }
                    if let Some(mesh) = &slot.gpu_mesh {
                        pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                        pass.set_vertex_buffer(1, slot.instance_buffer.slice(..));
                        pass.set_index_buffer(
                            mesh.index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                        chunks_drawn += 1;
                        triangles_submitted += (mesh.index_count / 3) as u64;
                    }
                }
            }

            // Draw transient mesh if present
            if let Some(tm) = transient_mesh {
                if !tm.is_empty() {
                    if let Some(gpu_tm) = GpuMesh::upload(&ctx.device, "transient_mesh", tm) {
                        pass.set_vertex_buffer(0, gpu_tm.vertex_buffer.slice(..));
                        pass.set_vertex_buffer(1, self.identity_instance_buffer.slice(..));
                        pass.set_index_buffer(
                            gpu_tm.index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        pass.draw_indexed(0..gpu_tm.index_count, 0, 0..1);
                        triangles_submitted += (gpu_tm.index_count / 3) as u64;
                    }
                }
            }

            // Draw instanced batches from their own slices of the shared buffer
            if let Some(inst_buf) = self.dynamic_instance_buffer.buffer() {
                pass.set_vertex_buffer(1, inst_buf.slice(..));
                for batch in &prepared_batches {
                    let Some(proto) = self.prototypes.get(batch.mesh.0 as usize) else {
                        continue;
                    };
                    pass.set_vertex_buffer(0, proto.mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(
                        proto.mesh.index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    let inst_count = batch.instance_range.end - batch.instance_range.start;
                    pass.draw_indexed(0..proto.mesh.index_count, 0, batch.instance_range.clone());
                    prototype_batches_drawn += 1;
                    instances_drawn += inst_count;
                    triangles_submitted +=
                        ((proto.mesh.index_count / 3) as u64) * (inst_count as u64);
                }
            }

            self.last_stats = FrameStats {
                internal_size,
                output_size,
                used_upscale_pass: needs_upscale,
                chunks_resident,
                chunks_drawn,
                chunks_culled,
                prototype_batches_drawn,
                instances_drawn,
                triangles_submitted,
            };
        }

        if needs_upscale {
            let upscale_target = self.upscale_readback_target.as_ref().unwrap();
            let upscale_uniforms = UpscaleUniforms {
                output_is_srgb: 0,
                _pad: [0; 3],
            };
            ctx.queue.write_buffer(
                &self.upscale_uniform_buffer,
                0,
                bytemuck::bytes_of(&upscale_uniforms),
            );

            let bg = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("upscale_bind_group"),
                layout: &self.upscale_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&target.color_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.upscale_uniform_buffer.as_entire_binding(),
                    },
                ],
            });

            let pipeline = &self.upscale_pipeline_cache[&wgpu::TextureFormat::Rgba8Unorm];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("upscale_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &upscale_target.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.draw(0..3, 0..1);
            drop(pass);

            ctx.queue.submit(Some(encoder.finish()));
            upscale_target.download(&ctx.device, &ctx.queue)
        } else {
            ctx.queue.submit(Some(encoder.finish()));
            target.download_rgba8(&ctx.device, &ctx.queue)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_to_surface(
        &mut self,
        ctx: &GpuContext,
        surface: &wgpu::Surface<'_>,
        surface_config: &wgpu::SurfaceConfiguration,
        camera: &CameraFrame,
        lighting: &SceneLightingUniforms,
        instances: &[InstanceBatch<'_>],
        transient_mesh: Option<&MeshBuffer>,
    ) -> Result<SurfaceFrameOutcome, GpuError> {
        let (surface_texture, suboptimal) = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(tex) => (tex, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(tex) => (tex, true),
            wgpu::CurrentSurfaceTexture::Timeout => return Ok(SurfaceFrameOutcome::SkippedTimeout),
            wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(SurfaceFrameOutcome::SkippedOccluded);
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                return Ok(SurfaceFrameOutcome::NeedsReconfigure);
            }
            wgpu::CurrentSurfaceTexture::Lost => return Ok(SurfaceFrameOutcome::SurfaceLost),
            wgpu::CurrentSurfaceTexture::Validation => return Ok(SurfaceFrameOutcome::SurfaceLost),
        };

        let (internal_size, output_size) = self.resolve_dimensions(camera);
        if self
            .internal_target
            .as_ref()
            .is_none_or(|t| t.width != internal_size.x || t.height != internal_size.y)
        {
            self.internal_target = Some(OffscreenTarget::new(
                &ctx.device,
                internal_size.x,
                internal_size.y,
                "internal_render_target",
            ));
        }
        let _ = self.get_or_create_upscale_pipeline(&ctx.device, surface_config.format);

        let cam_uniforms = camera.to_gpu_uniforms();
        ctx.queue.write_buffer(
            &self.camera_uniform_buffer,
            0,
            bytemuck::bytes_of(&cam_uniforms),
        );
        let local_lighting = lighting.relative_to(camera.render_origin());
        ctx.queue.write_buffer(
            &self.lighting_uniform_buffer,
            0,
            bytemuck::bytes_of(&local_lighting),
        );

        let target = self.internal_target.as_ref().unwrap();
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("surface_render_encoder"),
            });

        // Same single-upload scheme as the offscreen path: every batch draws from
        // its own range of the shared instance buffer.
        let prepared_batches = self.dynamic_instance_buffer.prepare_batches(
            &ctx.device,
            &ctx.queue,
            instances,
            "instance_buffer",
        );

        let mut chunks_drawn = 0_u32;
        let mut chunks_culled = 0_u32;
        let mut prototype_batches_drawn = 0_u32;
        let mut instances_drawn = 0_u32;
        let mut triangles_submitted = 0_u64;

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.config.clear_color_rgba[0] as f64,
                            g: self.config.clear_color_rgba[1] as f64,
                            b: self.config.clear_color_rgba[2] as f64,
                            a: self.config.clear_color_rgba[3] as f64,
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
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.world_pipeline);
            pass.set_bind_group(0, &self.world_bind_group, &[]);

            let sorted_coords = self.chunk_pool.sorted_coords();
            let chunks_resident = sorted_coords.len() as u32;

            for coord in sorted_coords {
                if let Some(slot) = self.chunk_pool.get(coord) {
                    if self.config.frustum_cull_chunks && !camera.frustum.intersects_aabb(slot.aabb)
                    {
                        chunks_culled += 1;
                        continue;
                    }
                    if let Some(mesh) = &slot.gpu_mesh {
                        pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                        pass.set_vertex_buffer(1, slot.instance_buffer.slice(..));
                        pass.set_index_buffer(
                            mesh.index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                        chunks_drawn += 1;
                        triangles_submitted += (mesh.index_count / 3) as u64;
                    }
                }
            }

            if let Some(tm) = transient_mesh {
                if !tm.is_empty() {
                    if let Some(gpu_tm) = GpuMesh::upload(&ctx.device, "transient_mesh", tm) {
                        pass.set_vertex_buffer(0, gpu_tm.vertex_buffer.slice(..));
                        pass.set_vertex_buffer(1, self.identity_instance_buffer.slice(..));
                        pass.set_index_buffer(
                            gpu_tm.index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        pass.draw_indexed(0..gpu_tm.index_count, 0, 0..1);
                        triangles_submitted += (gpu_tm.index_count / 3) as u64;
                    }
                }
            }

            if let Some(inst_buf) = self.dynamic_instance_buffer.buffer() {
                pass.set_vertex_buffer(1, inst_buf.slice(..));
                for batch in &prepared_batches {
                    let Some(proto) = self.prototypes.get(batch.mesh.0 as usize) else {
                        continue;
                    };
                    pass.set_vertex_buffer(0, proto.mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(
                        proto.mesh.index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    let inst_count = batch.instance_range.end - batch.instance_range.start;
                    pass.draw_indexed(0..proto.mesh.index_count, 0, batch.instance_range.clone());
                    prototype_batches_drawn += 1;
                    instances_drawn += inst_count;
                    triangles_submitted +=
                        ((proto.mesh.index_count / 3) as u64) * (inst_count as u64);
                }
            }

            self.last_stats = FrameStats {
                internal_size,
                output_size,
                used_upscale_pass: internal_size != output_size,
                chunks_resident,
                chunks_drawn,
                chunks_culled,
                prototype_batches_drawn,
                instances_drawn,
                triangles_submitted,
            };
        }

        // Upscale pass into surface texture view
        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let output_is_srgb = if surface_config.format.is_srgb() {
            1
        } else {
            0
        };
        let upscale_uniforms = UpscaleUniforms {
            output_is_srgb,
            _pad: [0; 3],
        };
        ctx.queue.write_buffer(
            &self.upscale_uniform_buffer,
            0,
            bytemuck::bytes_of(&upscale_uniforms),
        );

        let bg = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("surface_upscale_bind_group"),
            layout: &self.upscale_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&target.color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.upscale_uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let pipeline = &self.upscale_pipeline_cache[&surface_config.format];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("surface_upscale_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &surface_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);

        ctx.queue.submit(Some(encoder.finish()));
        ctx.queue.present(surface_texture);

        Ok(SurfaceFrameOutcome::Presented {
            suboptimal,
            stats: self.last_stats,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::CameraRig;
    use crate::gpu::uniforms::SceneLightingUniforms;
    use crate::mesh::{MeshBuilder, VertexMeta};

    #[test]
    fn depth_occlusion_front_wins_regardless_of_draw_order() {
        let ctx = GpuContext::new_headless().expect("GpuContext init failed");
        let mut renderer = GpuRenderer::new(&ctx, RenderPipelineConfig::default());

        let mut rig = CameraRig::new(100, 100);
        rig.set_mode(crate::camera::CameraMode::Overhead);
        rig.set_viewport(100, 100);

        let red = [1.0, 0.1, 0.1, 1.0];
        let green = [0.1, 1.0, 0.1, 1.0];

        // M_red HIGHER (Y = 2.0, closer to overhead camera!), m_green LOWER (Y = 0.5, farther).
        let mut m_red = MeshBuilder::new();
        m_red.push_quad(
            Vec3::new(-1.0, 2.0, -1.0),
            Vec3::new(-1.0, 2.0, 1.0),
            Vec3::new(1.0, 2.0, 1.0),
            Vec3::new(1.0, 2.0, -1.0),
            red,
            VertexMeta::DEFAULT
                .with_tag(crate::mesh::GeometryTag::Terrain)
                .with_flags(crate::mesh::MaterialFlags::UNLIT),
        );

        let mut m_green = MeshBuilder::new();
        m_green.push_quad(
            Vec3::new(-1.0, 0.5, -1.0),
            Vec3::new(-1.0, 0.5, 1.0),
            Vec3::new(1.0, 0.5, 1.0),
            Vec3::new(1.0, 0.5, -1.0),
            green,
            VertexMeta::DEFAULT
                .with_tag(crate::mesh::GeometryTag::Terrain)
                .with_flags(crate::mesh::MaterialFlags::UNLIT),
        );

        let lighting = SceneLightingUniforms::unlit_test();

        // Case A: Red in chunk (0,0), Green in chunk (1,0)
        renderer.upload_chunk(
            &ctx,
            ChunkCoord::new(0, 0),
            1,
            m_red.as_mesh().compute_aabb().unwrap(),
            m_red.as_mesh(),
        );
        renderer.upload_chunk(
            &ctx,
            ChunkCoord::new(1, 0),
            1,
            m_green.as_mesh().compute_aabb().unwrap(),
            m_green.as_mesh(),
        );

        let frame = rig.update(crate::camera::CameraTarget::default(), 0.016);
        let img = renderer
            .render_offscreen_rgba8(&ctx, &frame, &lighting, &[], None)
            .unwrap();

        // Center pixel should be red
        let center_px = img.pixel(50, 50);
        assert!(
            center_px[0] > 200 && center_px[1] < 50,
            "Center pixel should be red, got {center_px:?}"
        );

        // Recenter the GPU frame without moving the view. Resident chunks must
        // stay put across a focus-chunk transition, including their depth.
        let mut recentered = frame;
        recentered.focus.x += 32.0;
        let img_recentered = renderer
            .render_offscreen_rgba8(&ctx, &recentered, &lighting, &[], None)
            .unwrap();
        assert_eq!(img_recentered.pixel(50, 50), center_px);

        // Case B: Green in chunk (0,0), Red in chunk (1,0) - Red should still win via Depth32Float LessEqual
        renderer.upload_chunk(
            &ctx,
            ChunkCoord::new(0, 0),
            2,
            m_green.as_mesh().compute_aabb().unwrap(),
            m_green.as_mesh(),
        );
        renderer.upload_chunk(
            &ctx,
            ChunkCoord::new(1, 0),
            2,
            m_red.as_mesh().compute_aabb().unwrap(),
            m_red.as_mesh(),
        );

        let img_b = renderer
            .render_offscreen_rgba8(&ctx, &frame, &lighting, &[], None)
            .unwrap();
        let center_px_b = img_b.pixel(50, 50);
        assert!(
            center_px_b[0] > 200 && center_px_b[1] < 50,
            "Center pixel should still be red due to depth test, got {center_px_b:?}"
        );
    }

    #[test]
    fn cutaway_removes_shrouded_upper_floor_before_depth_write() {
        use crate::mesh::{GeometryTag, MaterialFlags};
        use crate::scene::{BuildingCutawayState, SceneManager};
        use zombiebox_core::{ActionGame, Position};

        let ctx = GpuContext::new_headless().expect("GPU setup");
        let mut renderer = GpuRenderer::new(&ctx, RenderPipelineConfig::default());
        let mut rig = CameraRig::new(100, 100);
        rig.set_mode(crate::CameraMode::Overhead);
        let camera = rig.update(crate::CameraTarget::default(), 0.0);
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new()
            .prepare_at_focus(&game, Position::new(0, 0), 0.0)
            .dynamic;
        scene.building_cutaways = vec![BuildingCutawayState {
            building_id: 1,
            floors: 2,
            foundation_z: 0.0,
            player_inside: true,
            wants_cutaway: true,
            openness: 1.0,
            stipple_step: 0,
            dither_opacity_u8: 0,
        }];
        let lighting = SceneLightingUniforms::for_scene(&scene, &camera);
        let mut mesh = MeshBuilder::new();
        let mut floor = |height, color, meta| {
            mesh.push_quad(
                Vec3::new(-2.0, height, -2.0),
                Vec3::new(-2.0, height, 2.0),
                Vec3::new(2.0, height, 2.0),
                Vec3::new(2.0, height, -2.0),
                color,
                meta,
            );
        };
        floor(
            0.0,
            [0.1, 1.0, 0.2, 1.0],
            VertexMeta::DEFAULT
                .with_tag(GeometryTag::Terrain)
                .with_flags(MaterialFlags::UNLIT),
        );
        floor(
            crate::FLOOR_H,
            [1.0, 0.1, 0.2, 1.0],
            VertexMeta::DEFAULT
                .with_tag(GeometryTag::InteriorFloor)
                .with_floor(1)
                .with_building(1, 0)
                .with_flags(MaterialFlags::UNLIT),
        );
        let mesh = mesh.build();

        scene.building_cutaways[0].dither_opacity_u8 = 255;
        renderer.update_visibility(&ctx, &scene);
        let closed = renderer
            .render_offscreen_rgba8(&ctx, &camera, &lighting, &[], Some(&mesh))
            .unwrap()
            .pixel(50, 50);
        assert!(
            closed[0] < 30 && closed[1] < 30,
            "shroud leaked floor color {closed:?}"
        );

        scene.building_cutaways[0].dither_opacity_u8 = 0;
        renderer.update_visibility(&ctx, &scene);
        let open = renderer
            .render_offscreen_rgba8(&ctx, &camera, &lighting, &[], Some(&mesh))
            .unwrap()
            .pixel(50, 50);
        assert!(
            open[0] < 50 && open[1] > 200,
            "shroud blocked cutaway {open:?}"
        );

        let mut first_person_rig = CameraRig::new(100, 100);
        first_person_rig.set_mode(crate::CameraMode::FirstPerson);
        let first_person = first_person_rig.update(crate::CameraTarget::default(), 0.0);
        let first_person_lighting = SceneLightingUniforms::for_scene(&scene, &first_person);
        assert_ne!(
            first_person_lighting.counts_and_flags[1]
                & super::super::uniforms::shading_flags::FIRST_PERSON,
            0
        );
        // Keep the overhead camera for this pixel comparison while exercising the
        // first-person visibility policy against the same fully open cutaway.
        let closed_in_first_person = renderer
            .render_offscreen_rgba8(&ctx, &camera, &first_person_lighting, &[], Some(&mesh))
            .unwrap()
            .pixel(50, 50);
        assert!(
            closed_in_first_person[0] < 30 && closed_in_first_person[1] < 30,
            "first-person cutaway or shroud leaked floor color {closed_in_first_person:?}"
        );

        // The underside belongs to the visible room below even if the floor
        // above has never been discovered.
        let grid = &mut scene.local_visibility;
        for (index, cell) in grid.cells.iter_mut().enumerate() {
            cell.visibility = if index < grid.side * grid.side {
                zombiebox_core::TileVisibility::Visible
            } else {
                zombiebox_core::TileVisibility::Hidden
            };
        }
        renderer.update_visibility(&ctx, &scene);
        let mut ceiling = MeshBuilder::new();
        ceiling.push_quad(
            Vec3::new(-2.0, crate::FLOOR_H, -2.0),
            Vec3::new(2.0, crate::FLOOR_H, -2.0),
            Vec3::new(2.0, crate::FLOOR_H, 2.0),
            Vec3::new(-2.0, crate::FLOOR_H, 2.0),
            [1.0, 0.1, 0.2, 1.0],
            VertexMeta::DEFAULT
                .with_tag(GeometryTag::InteriorFloor)
                .with_floor(1)
                .with_building(1, 0)
                .with_flags(MaterialFlags::UNLIT),
        );
        first_person_rig.adjust_first_person_look(0.0, -1.3);
        let upward_camera = first_person_rig.update(crate::CameraTarget::default(), 0.0);
        let ceiling_color = renderer
            .render_offscreen_rgba8(
                &ctx,
                &upward_camera,
                &first_person_lighting,
                &[],
                Some(&ceiling.build()),
            )
            .unwrap()
            .pixel(50, 50);
        assert!(
            ceiling_color[0] > 200 && ceiling_color[1] < 50,
            "visible room ceiling was shrouded: {ceiling_color:?}"
        );
    }

    #[test]
    fn high_floor_roofs_and_eighth_visibility_layer_use_the_active_floor() {
        use crate::mesh::{GeometryTag, MaterialFlags};
        use crate::scene::{BuildingCutawayState, SceneManager};
        use zombiebox_core::{ActionGame, Position, TileVisibility};

        let ctx = GpuContext::new_headless().expect("GPU setup");
        let mut renderer = GpuRenderer::new(&ctx, RenderPipelineConfig::default());
        let mut rig = CameraRig::new(100, 100);
        rig.set_mode(crate::CameraMode::Overhead);
        let camera = rig.update(crate::CameraTarget::default(), 0.0);
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new()
            .prepare_at_focus(&game, Position::new(0, 0), 0.0)
            .dynamic;
        scene.active_floor = 5;
        scene.local_visibility.floor_count = 8;
        let area = scene.local_visibility.side * scene.local_visibility.side;
        let visible = scene.local_visibility.cells[0];
        scene.local_visibility.cells.resize(area * 8, visible);
        for (index, cell) in scene.local_visibility.cells.iter_mut().enumerate() {
            cell.visibility = if index / area == 7 {
                TileVisibility::Visible
            } else {
                TileVisibility::Hidden
            };
        }
        scene.building_cutaways = vec![BuildingCutawayState {
            building_id: 1,
            floors: 6,
            foundation_z: 0.0,
            player_inside: true,
            wants_cutaway: true,
            openness: 1.0,
            stipple_step: 0,
            dither_opacity_u8: 0,
        }];
        renderer.update_visibility(&ctx, &scene);
        let lighting = SceneLightingUniforms::for_scene(&scene, &camera);
        let render = |renderer: &mut GpuRenderer, tag, floor| {
            let mut mesh = MeshBuilder::new();
            for (height, color, meta) in [
                (
                    0.0,
                    [0.1, 1.0, 0.2, 1.0],
                    VertexMeta::DEFAULT
                        .with_tag(GeometryTag::Terrain)
                        .with_flags(MaterialFlags::UNLIT),
                ),
                (
                    1.0,
                    [1.0, 0.1, 0.2, 1.0],
                    VertexMeta::DEFAULT
                        .with_tag(tag)
                        .with_floor(floor)
                        .with_building(1, 0)
                        .with_flags(MaterialFlags::UNLIT),
                ),
            ] {
                mesh.push_quad(
                    Vec3::new(-2.0, height, -2.0),
                    Vec3::new(-2.0, height, 2.0),
                    Vec3::new(2.0, height, 2.0),
                    Vec3::new(2.0, height, -2.0),
                    color,
                    meta,
                );
            }
            renderer
                .render_offscreen_rgba8(&ctx, &camera, &lighting, &[], Some(&mesh.build()))
                .unwrap()
                .pixel(50, 50)
        };
        let tower = render(&mut renderer, GeometryTag::Roof, 6);
        assert!(
            tower[1] > 200 && tower[0] < 50,
            "sixth floor roof should dissolve: {tower:?}"
        );
        let podium = render(&mut renderer, GeometryTag::Roof, 2);
        assert!(
            podium[0] > 130 && podium[1] < 50,
            "lower podium roof should remain: {podium:?}"
        );
        // Disable cutaway to inspect the eighth visibility page directly.
        scene.building_cutaways[0].dither_opacity_u8 = 255;
        renderer.update_visibility(&ctx, &scene);
        let eighth = render(&mut renderer, GeometryTag::InteriorFloor, 7);
        assert!(
            eighth[0] > 200 && eighth[1] < 50,
            "eighth-floor visibility page: {eighth:?}"
        );
        let hidden = render(&mut renderer, GeometryTag::InteriorFloor, 6);
        assert!(
            hidden[0] < 100 && hidden[1] < 100,
            "hidden seventh-floor visibility page: {hidden:?}"
        );
    }

    #[test]
    fn headless_terrain_renders_birch_street_and_county_scenes_seam_free() {
        use std::collections::HashSet;
        use zombiebox_core::{
            ActionGame, CHUNK_SIZE, Ground, LandmarkKind, Position, Tile3D, WATER_SURFACE_Z,
            WORLD_SIZE, Water, World,
        };

        let ctx = GpuContext::new_headless().expect("GpuContext init failed");
        let mut renderer = GpuRenderer::new(&ctx, RenderPipelineConfig::default());
        let lighting = SceneLightingUniforms::dusk_default();
        let mesh_cfg = crate::TerrainMeshConfig::default();

        // 1. Birch Street (640x360 Isometric)
        {
            let res_game = ActionGame::residential(42);
            let mut mgr = crate::SceneManager::new();
            let mut rig = CameraRig::new(640, 360);
            rig.set_mode(crate::camera::CameraMode::Isometric);
            let target = crate::camera::CameraTarget {
                position: res_game.player_body().position,
                ..Default::default()
            };
            let cam_frame = rig.update(target, 0.016);
            let scene_frame = mgr.prepare(&res_game, &cam_frame, 0.016);
            crate::sync_terrain_chunks_to_gpu(&ctx, &mut renderer, &scene_frame, &mesh_cfg);
            let img = renderer
                .render_offscreen_rgba8(&ctx, &cam_frame, &lighting, &[], None)
                .unwrap();
            let stats = renderer.last_frame_stats();
            assert!(stats.chunks_drawn >= 1);
            let colors: HashSet<[u8; 3]> = img
                .pixels
                .chunks_exact(4)
                .map(|c| [c[0], c[1], c[2]])
                .collect();
            assert!(
                colors.len() >= 8,
                "Birch Street expected >= 8 colors, got {}",
                colors.len()
            );
        }

        // 2. County Seed 42 scenes
        let world = World::new(42);
        let mut county = ActionGame::county_with_world(world.clone());
        let mut mgr = crate::SceneManager::new();

        let bridge_pos = {
            let center = WORLD_SIZE / 2;
            let mut best: Option<(usize, i64, Position)> = None;
            for y in 16..(WORLD_SIZE - 16) {
                for x in 16..(WORLD_SIZE - 16) {
                    let pos = Position::new(x, y);
                    if let Some(t) = world.tile_at(pos)
                        && t.ground == Ground::Bridge
                        && t.water == Some(Water::River)
                    {
                        let mut rc = 0usize;
                        for dy in -3..=3 {
                            for dx in -3..=3 {
                                if world
                                    .tile_at(Position::new(x + dx, y + dy))
                                    .is_some_and(|nt| nt.water == Some(Water::River))
                                {
                                    rc += 1;
                                }
                            }
                        }
                        let dist2 = i64::from(x - center).pow(2) + i64::from(y - center).pow(2);
                        if best.is_none_or(|(bc, bd, _)| rc > bc || (rc == bc && dist2 < bd)) {
                            best = Some((rc, dist2, pos));
                        }
                    }
                }
            }
            best.expect("county has river bridge").2
        };
        let downtown_pos = world
            .landmarks()
            .iter()
            .find(|l| l.kind == LandmarkKind::Downtown)
            .unwrap()
            .entrance;
        let mountain_pos = world
            .landmarks()
            .iter()
            .find(|l| l.kind == LandmarkKind::Mountain)
            .unwrap()
            .entrance;

        for (label, pos, mode) in [
            (
                "spawn",
                county.player_tile().pos(),
                crate::camera::CameraMode::Isometric,
            ),
            (
                "river-bridge-iso",
                bridge_pos,
                crate::camera::CameraMode::Isometric,
            ),
            (
                "river-bridge-orbit",
                bridge_pos,
                crate::camera::CameraMode::Orbit,
            ),
            (
                "downtown",
                downtown_pos,
                crate::camera::CameraMode::Isometric,
            ),
            (
                "mountains",
                mountain_pos,
                crate::camera::CameraMode::Isometric,
            ),
        ] {
            county.teleport(Tile3D::from_pos(pos, 0));
            mgr.notify_teleport();
            let mut rig = CameraRig::new(640, 360);
            rig.set_mode(mode);
            if mode == crate::camera::CameraMode::Orbit {
                rig.orbit_rotate(0.65, -0.05);
            }
            let target = crate::camera::CameraTarget {
                position: county.player_body().position,
                ..Default::default()
            };
            let cam_frame = rig.update(target, 0.016);
            let scene_frame = mgr.prepare(&county, &cam_frame, 0.016);
            crate::sync_terrain_chunks_to_gpu(&ctx, &mut renderer, &scene_frame, &mesh_cfg);
            let img = renderer
                .render_offscreen_rgba8(&ctx, &cam_frame, &lighting, &[], None)
                .unwrap();
            let stats = renderer.last_frame_stats();
            assert!(stats.chunks_drawn >= 1, "{label} drew 0 chunks");
            let colors: HashSet<[u8; 3]> = img
                .pixels
                .chunks_exact(4)
                .map(|c| [c[0], c[1], c[2]])
                .collect();
            assert!(
                colors.len() >= 8,
                "{label} expected >= 8 distinct colors, got {}",
                colors.len()
            );

            if label == "river-bridge-iso" {
                let bq = world.bridge_quad(bridge_pos).unwrap();
                let bz = (bq[0] + bq[1] + bq[2] + bq[3]) * 0.25;
                let sp_bridge = cam_frame
                    .project_sim_to_screen(zombiebox_core::Vec3::new(
                        bridge_pos.x as f32,
                        bridge_pos.y as f32,
                        bz,
                    ))
                    .unwrap();
                let bpx = img.pixel(
                    (sp_bridge.px.x as u32).min(639),
                    (sp_bridge.px.y as u32).min(359),
                );
                assert!(
                    bpx[0] > bpx[2],
                    "projected bridge pixel should be warm timber, got {bpx:?}"
                );

                // Find adjacent open river water tile
                let mut water_tile = None;
                for dy in -4..=4 {
                    for dx in -4..=4 {
                        let p = Position::new(bridge_pos.x + dx, bridge_pos.y + dy);
                        if world.tile_at(p).is_some_and(|t| {
                            t.ground == Ground::River && t.water == Some(Water::River)
                        }) {
                            water_tile = Some(p);
                            break;
                        }
                    }
                    if water_tile.is_some() {
                        break;
                    }
                }
                let wp = water_tile.expect("open river tile near bridge");
                let sp_water = cam_frame
                    .project_sim_to_screen(zombiebox_core::Vec3::new(
                        wp.x as f32,
                        wp.y as f32,
                        WATER_SURFACE_Z,
                    ))
                    .unwrap();
                let wpx = img.pixel(
                    (sp_water.px.x as u32).min(639),
                    (sp_water.px.y as u32).min(359),
                );
                assert!(
                    wpx[2] > wpx[0],
                    "projected water pixel should be blue, got {wpx:?}"
                );
            }
        }

        // 3. Zero chunk-seam crack test with bright magenta sentinel clear color
        let mut crack_renderer = GpuRenderer::new(
            &ctx,
            RenderPipelineConfig {
                clear_color_rgba: [1.0, 0.0, 1.0, 1.0],
                ..Default::default()
            },
        );
        county.teleport(Tile3D::from_pos(downtown_pos, 0));
        mgr.notify_teleport();
        let fc = ChunkCoord::from_pos(downtown_pos);
        let seam_xy = zombiebox_core::Vec2::new(
            (fc.cx + 1) as f32 * CHUNK_SIZE as f32 - 0.5,
            (fc.cy + 1) as f32 * CHUNK_SIZE as f32 - 0.5,
        );
        let seam_z = world.elevation_at(seam_xy).unwrap_or(0.0);
        let mut rig = CameraRig::new(640, 360);
        rig.set_mode(crate::camera::CameraMode::Isometric);
        let cam_frame = rig.update(
            crate::camera::CameraTarget {
                position: seam_xy.extend(seam_z),
                ..Default::default()
            },
            0.016,
        );
        let scene_frame = mgr.prepare(&county, &cam_frame, 0.016);
        crate::sync_terrain_chunks_to_gpu(&ctx, &mut crack_renderer, &scene_frame, &mesh_cfg);
        let crack_img = crack_renderer
            .render_offscreen_rgba8(&ctx, &cam_frame, &lighting, &[], None)
            .unwrap();
        let x_lo = 640 / 8;
        let x_hi = 640 - x_lo;
        let y_lo = 360 / 8;
        let y_hi = 360 - y_lo;
        let mut magenta_hits = 0usize;
        for y in y_lo..y_hi {
            for x in x_lo..x_hi {
                let px = crack_img.pixel(x, y);
                if px[0] > 230 && px[1] < 25 && px[2] > 230 {
                    magenta_hits += 1;
                }
            }
        }
        assert_eq!(
            magenta_hits, 0,
            "detected {magenta_hits} magenta background crack pixels along chunk boundary!"
        );
    }
}
