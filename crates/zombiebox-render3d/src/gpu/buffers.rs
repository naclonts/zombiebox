//! wgpu vertex layouts, GpuMesh, ChunkMeshPool, and DynamicInstanceBuffer (`T03`).

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec3};
use std::collections::HashMap;

use crate::coords::{ChunkCoord, RenderAabb};
use crate::mesh::{MeshBuffer, VertexMeta, WorldVertex};

/// 96-byte per-instance payload (`@location(4..=9)`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct InstanceRaw {
    pub model_col0: [f32; 4],    // @location(4) Float32x4
    pub model_col1: [f32; 4],    // @location(5) Float32x4
    pub model_col2: [f32; 4],    // @location(6) Float32x4
    pub model_col3: [f32; 4],    // @location(7) Float32x4
    pub color_tint: [f32; 4],    // @location(8) Float32x4
    pub instance_meta: [u32; 4], // @location(9) Uint32x4
}

impl Default for InstanceRaw {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl InstanceRaw {
    pub const IDENTITY: Self = Self {
        model_col0: [1.0, 0.0, 0.0, 0.0],
        model_col1: [0.0, 1.0, 0.0, 0.0],
        model_col2: [0.0, 0.0, 1.0, 0.0],
        model_col3: [0.0, 0.0, 0.0, 1.0],
        color_tint: [1.0, 1.0, 1.0, 1.0],
        instance_meta: [0, 0, 0, 255],
    };

    #[must_use]
    pub fn from_transform(model: Mat4) -> Self {
        let cols = model.to_cols_array_2d();
        Self {
            model_col0: cols[0],
            model_col1: cols[1],
            model_col2: cols[2],
            model_col3: cols[3],
            ..Self::IDENTITY
        }
    }

    #[must_use]
    pub fn from_trs(translation: Vec3, rotation: Quat, scale: Vec3) -> Self {
        Self::from_transform(Mat4::from_scale_rotation_translation(
            scale,
            rotation,
            translation,
        ))
    }

    #[must_use]
    pub fn with_tint(mut self, rgba: [f32; 4]) -> Self {
        self.color_tint = rgba;
        self
    }

    #[must_use]
    pub fn with_meta_override(mut self, meta: VertexMeta) -> Self {
        let packed = meta.pack();
        self.instance_meta[0] = packed[0];
        self.instance_meta[1] = packed[1];
        self
    }

    #[must_use]
    pub fn with_dither_opacity(mut self, opacity_u8: u8) -> Self {
        self.instance_meta[3] = u32::from(opacity_u8);
        self
    }

    #[must_use]
    pub fn with_hit_flash_and_emissive(mut self, hit_flash_u8: u8, emissive_u8: u8) -> Self {
        self.instance_meta[2] = u32::from(hit_flash_u8) | (u32::from(emissive_u8) << 8);
        self
    }
}

#[must_use]
pub fn world_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: &[wgpu::VertexAttribute] = &[
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 0,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 12,
            shader_location: 1,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 24,
            shader_location: 2,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Uint32x2,
            offset: 40,
            shader_location: 3,
        },
    ];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<WorldVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: ATTRIBUTES,
    }
}

#[must_use]
pub fn instance_raw_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: &[wgpu::VertexAttribute] = &[
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 0,
            shader_location: 4,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 16,
            shader_location: 5,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 32,
            shader_location: 6,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 48,
            shader_location: 7,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 64,
            shader_location: 8,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Uint32x4,
            offset: 80,
            shader_location: 9,
        },
    ];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: ATTRIBUTES,
    }
}

#[derive(Debug)]
pub struct GpuMesh {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub vertex_count: u32,
    pub index_count: u32,
    pub aabb: RenderAabb,
}

impl GpuMesh {
    #[must_use]
    pub fn upload(device: &wgpu::Device, label: &str, mesh: &MeshBuffer) -> Option<Self> {
        if mesh.is_empty() || mesh.vertices.is_empty() {
            return None;
        }
        let aabb = mesh.compute_aabb()?;
        use wgpu::util::DeviceExt;
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label}_vertex_buffer")),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label}_index_buffer")),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Some(Self {
            vertex_buffer,
            index_buffer,
            vertex_count: mesh.vertices.len() as u32,
            index_count: mesh.indices.len() as u32,
            aabb,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PrototypeMeshId(pub u32);

#[derive(Clone, Copy, Debug)]
pub struct InstanceBatch<'a> {
    pub mesh: PrototypeMeshId,
    pub instances: &'a [InstanceRaw],
}

#[derive(Debug)]
pub struct ChunkGpuSlot {
    pub coord: ChunkCoord,
    pub revision: u64,
    pub aabb: RenderAabb,
    pub gpu_mesh: Option<GpuMesh>,
}

#[derive(Debug, Default)]
pub struct ChunkMeshPool {
    slots: HashMap<ChunkCoord, ChunkGpuSlot>,
}

impl ChunkMeshPool {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    #[must_use]
    pub fn get(&self, coord: ChunkCoord) -> Option<&ChunkGpuSlot> {
        self.slots.get(&coord)
    }

    #[must_use]
    pub fn has_revision(&self, coord: ChunkCoord, revision: u64) -> bool {
        self.slots
            .get(&coord)
            .is_some_and(|slot| slot.revision == revision)
    }

    pub fn upload_chunk(
        &mut self,
        device: &wgpu::Device,
        coord: ChunkCoord,
        revision: u64,
        aabb: RenderAabb,
        mesh: &MeshBuffer,
    ) -> bool {
        if self.has_revision(coord, revision) {
            return false;
        }
        let gpu_mesh = GpuMesh::upload(device, &format!("chunk_{}_{}", coord.cx, coord.cy), mesh);
        self.slots.insert(
            coord,
            ChunkGpuSlot {
                coord,
                revision,
                aabb,
                gpu_mesh,
            },
        );
        true
    }

    pub fn remove_chunk(&mut self, coord: ChunkCoord) -> bool {
        self.slots.remove(&coord).is_some()
    }

    pub fn retain_chunks(&mut self, mut keep: impl FnMut(ChunkCoord, &ChunkGpuSlot) -> bool) {
        self.slots.retain(|&coord, slot| keep(coord, slot));
    }

    pub fn clear(&mut self) {
        self.slots.clear();
    }

    #[must_use]
    pub fn sorted_coords(&self) -> Vec<ChunkCoord> {
        let mut coords: Vec<ChunkCoord> = self.slots.keys().copied().collect();
        coords.sort();
        coords
    }
}

#[derive(Debug, Default)]
pub struct DynamicInstanceBuffer {
    buffer: Option<wgpu::Buffer>,
    capacity: usize,
    scratch: Vec<InstanceRaw>,
}

impl DynamicInstanceBuffer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_buffer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[InstanceRaw],
        label: &str,
    ) {
        self.scratch.clear();
        self.scratch.extend_from_slice(instances);
        if self.scratch.is_empty() {
            return;
        }
        let needed = self.scratch.len();
        if needed > self.capacity || self.buffer.is_none() {
            let new_cap = needed.max(64).next_power_of_two();
            let size = (new_cap * std::mem::size_of::<InstanceRaw>()) as wgpu::BufferAddress;
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity = new_cap;
        }
        if let Some(buf) = &self.buffer {
            queue.write_buffer(buf, 0, bytemuck::cast_slice(&self.scratch));
        }
    }

    #[must_use]
    pub fn buffer(&self) -> Option<&wgpu::Buffer> {
        self.buffer.as_ref()
    }
}
