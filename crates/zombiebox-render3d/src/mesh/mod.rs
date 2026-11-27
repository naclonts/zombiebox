//! CPU 3D mesh representation, WorldVertex, VertexMeta, and MeshBuilder (`T03`).

pub mod actors;
pub mod buildings;
pub(crate) mod doors;
pub mod motorcycle;
pub mod primitives;
pub mod props;
#[allow(clippy::excessive_precision)] // generated authored mesh tables
pub mod survivor;
pub mod terrain;
pub mod vehicles;
#[allow(clippy::excessive_precision)] // generated authored mesh tables
pub mod weapons;
#[allow(clippy::excessive_precision)] // generated f32 literal table
pub mod zombie;

pub use primitives::*;
pub use props::*;
pub use terrain::*;

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};

use crate::coords::{RenderAabb, is_ccw_facing};

/// Semantic geometry classification used by cutaway, shroud, and shading rules (`T04`–`T09`).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum GeometryTag {
    Terrain = 0,
    Road = 1,
    Water = 2,
    Bridge = 3,
    InteriorFloor = 4,
    WallLower = 5,
    WallUpper = 6,
    DoorOrWindow = 7,
    Roof = 8,
    #[default]
    Prop = 9,
    Actor = 10,
    Effect = 11,
    Stair = 12,
}

impl GeometryTag {
    #[must_use]
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Terrain,
            1 => Self::Road,
            2 => Self::Water,
            3 => Self::Bridge,
            4 => Self::InteriorFloor,
            5 => Self::WallLower,
            6 => Self::WallUpper,
            7 => Self::DoorOrWindow,
            8 => Self::Roof,
            9 => Self::Prop,
            10 => Self::Actor,
            11 => Self::Effect,
            12 => Self::Stair,
            _ => Self::Prop,
        }
    }
}

/// Bitflags carried in `VertexMeta::flags` (`u8`) for shader material behavior.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct MaterialFlags(pub u8);

impl MaterialFlags {
    pub const NONE: Self = Self(0);
    /// Bypasses directional/point diffuse attenuation (lamp bulbs, tracers, blasts, HUD markers).
    pub const UNLIT: Self = Self(1 << 0);
    /// Applies the Zombiebox actor readability floor (`[80, 84, 94] / 128`) so actors pop in dark rooms.
    pub const ACTOR_MIN_LIGHT: Self = Self(1 << 1);
    /// Uses moody indoor ambient baseline instead of outdoor twilight sky ambient (`T05`/`T07`).
    pub const INDOOR_AMBIENT: Self = Self(1 << 2);
    /// Exempts geometry from upper-wall/roof cutaway dissolve even when attached to a building.
    pub const CUTAWAY_EXEMPT: Self = Self(1 << 3);
    /// Keeps first-person held meshes in front of scene geometry while retaining their own depth.
    pub const FIRST_PERSON_HELD: Self = Self(1 << 4);

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for MaterialFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for MaterialFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Sentinel `VertexMeta::floor` value indicating exterior / non-floor-scoped geometry.
pub const EXTERIOR_FLOOR_SENTINEL: u8 = 0xFF;

/// Strongly-typed CPU helper that packs cleanly into `WorldVertex::meta` (`[u32; 2]`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VertexMeta {
    pub tag: GeometryTag,
    /// `0..=254` for building/bridge floor index; `EXTERIOR_FLOOR_SENTINEL` (`255`) for terrain/exterior.
    pub floor: u8,
    pub flags: MaterialFlags,
    /// Vertex ambient occlusion in `0..=255` (`255` = `1.0` fully unoccluded).
    pub ao: u8,
    /// `0` = no building; `1..=65535` = `BuildingId` + 1 (`T05`/`T07` cutaway table index).
    pub building_id: u16,
    /// `0` = exterior/default; `1..=65535` = room/visibility cell key (`T07` shroud).
    pub room_id: u16,
}

impl Default for VertexMeta {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl VertexMeta {
    pub const DEFAULT: Self = Self {
        tag: GeometryTag::Prop,
        floor: EXTERIOR_FLOOR_SENTINEL,
        flags: MaterialFlags::NONE,
        ao: 255,
        building_id: 0,
        room_id: 0,
    };

    #[must_use]
    pub const fn with_tag(mut self, tag: GeometryTag) -> Self {
        self.tag = tag;
        self
    }

    #[must_use]
    pub const fn with_floor(mut self, floor: u8) -> Self {
        self.floor = floor;
        self
    }

    #[must_use]
    pub const fn with_flags(mut self, flags: MaterialFlags) -> Self {
        self.flags = flags;
        self
    }

    #[must_use]
    pub const fn with_ao(mut self, ao: u8) -> Self {
        self.ao = ao;
        self
    }

    #[must_use]
    pub const fn with_building(mut self, building_id: u16, room_id: u16) -> Self {
        self.building_id = building_id;
        self.room_id = room_id;
        self
    }

    #[must_use]
    pub const fn pack(self) -> [u32; 2] {
        let m0 = (self.tag as u32)
            | ((self.floor as u32) << 8)
            | ((self.flags.0 as u32) << 16)
            | ((self.ao as u32) << 24);
        let m1 = (self.building_id as u32) | ((self.room_id as u32) << 16);
        [m0, m1]
    }

    #[must_use]
    pub const fn unpack(raw: [u32; 2]) -> Self {
        Self {
            tag: GeometryTag::from_u8((raw[0] & 0xFF) as u8),
            floor: ((raw[0] >> 8) & 0xFF) as u8,
            flags: MaterialFlags(((raw[0] >> 16) & 0xFF) as u8),
            ao: ((raw[0] >> 24) & 0xFF) as u8,
            building_id: (raw[1] & 0xFFFF) as u16,
            room_id: ((raw[1] >> 16) & 0xFFFF) as u16,
        }
    }
}

/// Unified 48-byte vertex format for all static chunk and instanced 3D meshes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct WorldVertex {
    pub position: [f32; 3], // 12B @location(0) Float32x3
    pub normal: [f32; 3],   // 12B @location(1) Float32x3
    pub color: [f32; 4],    // 16B @location(2) Float32x4
    pub meta: [u32; 2],     //  8B @location(3) Uint32x2
}

impl WorldVertex {
    #[must_use]
    pub fn new(position: Vec3, normal: Vec3, color: [f32; 4]) -> Self {
        Self::with_meta(position, normal, color, VertexMeta::DEFAULT)
    }

    #[must_use]
    pub fn with_meta(position: Vec3, normal: Vec3, color: [f32; 4], meta: VertexMeta) -> Self {
        Self {
            position: position.to_array(),
            normal: normal.to_array(),
            color,
            meta: meta.pack(),
        }
    }

    #[must_use]
    pub fn rgba8(r: u8, g: u8, b: u8, a: u8) -> [f32; 4] {
        [
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            f32::from(a) / 255.0,
        ]
    }

    #[must_use]
    pub const fn unpacked_meta(&self) -> VertexMeta {
        VertexMeta::unpack(self.meta)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MeshValidationError {
    NonMultipleOfThreeIndices {
        index_count: usize,
    },
    IndexOutOfBounds {
        triangle: usize,
        index: u32,
        vertex_count: usize,
    },
    NonFiniteAttribute {
        vertex: usize,
    },
    UnnormalizedNormal {
        vertex: usize,
        length: f32,
    },
    DegenerateTriangle {
        triangle: usize,
    },
    InvertedWinding {
        triangle: usize,
        dot: f32,
    },
}

impl std::fmt::Display for MeshValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonMultipleOfThreeIndices { index_count } => {
                write!(f, "Mesh index count {index_count} is not a multiple of 3")
            }
            Self::IndexOutOfBounds {
                triangle,
                index,
                vertex_count,
            } => {
                write!(
                    f,
                    "Triangle {triangle} index {index} out of bounds for vertex count {vertex_count}"
                )
            }
            Self::NonFiniteAttribute { vertex } => {
                write!(
                    f,
                    "Vertex {vertex} has non-finite position, normal, or color components"
                )
            }
            Self::UnnormalizedNormal { vertex, length } => {
                write!(
                    f,
                    "Vertex {vertex} normal length {length} is not normalized (expected ~1.0)"
                )
            }
            Self::DegenerateTriangle { triangle } => {
                write!(
                    f,
                    "Triangle {triangle} is degenerate (cross product near zero)"
                )
            }
            Self::InvertedWinding { triangle, dot } => {
                write!(
                    f,
                    "Triangle {triangle} has inverted winding or normal disagreement (dot product {dot})"
                )
            }
        }
    }
}

impl std::error::Error for MeshValidationError {}

/// CPU-side indexed triangle mesh (`PrimitiveTopology::TriangleList`, outward CCW winding).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshBuffer {
    pub vertices: Vec<WorldVertex>,
    pub indices: Vec<u32>,
}

impl MeshBuffer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_capacity(vertex_cap: usize, index_cap: usize) -> Self {
        Self {
            vertices: Vec::with_capacity(vertex_cap),
            indices: Vec::with_capacity(index_cap),
        }
    }

    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    #[must_use]
    pub fn compute_aabb(&self) -> Option<RenderAabb> {
        if self.vertices.is_empty() {
            return None;
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for v in &self.vertices {
            let p = Vec3::from_array(v.position);
            min = min.min(p);
            max = max.max(p);
        }
        Some(RenderAabb::new(min, max))
    }

    pub fn append(&mut self, other: &Self) {
        if other.is_empty() {
            return;
        }
        let base_index = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&other.vertices);
        self.indices.reserve(other.indices.len());
        for &idx in &other.indices {
            self.indices.push(idx + base_index);
        }
    }

    pub fn append_transformed(&mut self, other: &Self, transform: Mat4) {
        if other.is_empty() {
            return;
        }
        let base_index = self.vertices.len() as u32;
        let normal_matrix = Mat3::from_mat4(transform).inverse().transpose();
        for v in &other.vertices {
            let p = transform.transform_point3(Vec3::from_array(v.position));
            let n = normal_matrix
                .mul_vec3(Vec3::from_array(v.normal))
                .normalize_or_zero();
            let mut transformed_v = *v;
            transformed_v.position = p.to_array();
            transformed_v.normal = n.to_array();
            self.vertices.push(transformed_v);
        }
        self.indices.reserve(other.indices.len());
        for &idx in &other.indices {
            self.indices.push(idx + base_index);
        }
    }

    pub fn transform_in_place(&mut self, transform: Mat4) {
        let normal_matrix = Mat3::from_mat4(transform).inverse().transpose();
        for v in &mut self.vertices {
            let p = transform.transform_point3(Vec3::from_array(v.position));
            let n = normal_matrix
                .mul_vec3(Vec3::from_array(v.normal))
                .normalize_or_zero();
            v.position = p.to_array();
            v.normal = n.to_array();
        }
    }

    pub fn validate(&self) -> Result<(), MeshValidationError> {
        if !self.indices.len().is_multiple_of(3) {
            return Err(MeshValidationError::NonMultipleOfThreeIndices {
                index_count: self.indices.len(),
            });
        }
        let v_count = self.vertices.len();
        for (i, v) in self.vertices.iter().enumerate() {
            let p = Vec3::from_array(v.position);
            let n = Vec3::from_array(v.normal);
            let c = v.color;
            if !p.is_finite()
                || !n.is_finite()
                || !c[0].is_finite()
                || !c[1].is_finite()
                || !c[2].is_finite()
                || !c[3].is_finite()
            {
                return Err(MeshValidationError::NonFiniteAttribute { vertex: i });
            }
            let n_len = n.length();
            if (n_len - 1.0).abs() > 1e-2 {
                return Err(MeshValidationError::UnnormalizedNormal {
                    vertex: i,
                    length: n_len,
                });
            }
        }
        for (t_idx, chunk) in self.indices.chunks_exact(3).enumerate() {
            let i0 = chunk[0];
            let i1 = chunk[1];
            let i2 = chunk[2];
            if (i0 as usize) >= v_count {
                return Err(MeshValidationError::IndexOutOfBounds {
                    triangle: t_idx,
                    index: i0,
                    vertex_count: v_count,
                });
            }
            if (i1 as usize) >= v_count {
                return Err(MeshValidationError::IndexOutOfBounds {
                    triangle: t_idx,
                    index: i1,
                    vertex_count: v_count,
                });
            }
            if (i2 as usize) >= v_count {
                return Err(MeshValidationError::IndexOutOfBounds {
                    triangle: t_idx,
                    index: i2,
                    vertex_count: v_count,
                });
            }

            let p0 = Vec3::from_array(self.vertices[i0 as usize].position);
            let p1 = Vec3::from_array(self.vertices[i1 as usize].position);
            let p2 = Vec3::from_array(self.vertices[i2 as usize].position);

            let cross = (p1 - p0).cross(p2 - p0);
            let area_sq = cross.length_squared();
            if area_sq <= 1e-12 {
                return Err(MeshValidationError::DegenerateTriangle { triangle: t_idx });
            }
            let face_normal = cross / area_sq.sqrt();
            let n0 = Vec3::from_array(self.vertices[i0 as usize].normal);
            let n1 = Vec3::from_array(self.vertices[i1 as usize].normal);
            let n2 = Vec3::from_array(self.vertices[i2 as usize].normal);
            let avg_n = (n0 + n1 + n2).normalize_or_zero();
            let dot = face_normal.dot(avg_n);
            if dot <= -0.1 || !is_ccw_facing(p0, p1, p2, avg_n) {
                return Err(MeshValidationError::InvertedWinding {
                    triangle: t_idx,
                    dot,
                });
            }
        }
        Ok(())
    }
}

/// Ergonomic stateful builder wrapping a `MeshBuffer`.
#[derive(Clone, Debug, Default)]
pub struct MeshBuilder {
    mesh: MeshBuffer,
    current_meta: VertexMeta,
    current_color: [f32; 4],
}

impl MeshBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            mesh: MeshBuffer::new(),
            current_meta: VertexMeta::DEFAULT,
            current_color: [1.0; 4],
        }
    }

    #[must_use]
    pub fn with_capacity(vertex_cap: usize, index_cap: usize) -> Self {
        Self {
            mesh: MeshBuffer::with_capacity(vertex_cap, index_cap),
            current_meta: VertexMeta::DEFAULT,
            current_color: [1.0; 4],
        }
    }

    pub fn set_meta(&mut self, meta: VertexMeta) -> &mut Self {
        self.current_meta = meta;
        self
    }

    pub fn set_color(&mut self, color: [f32; 4]) -> &mut Self {
        self.current_color = color;
        self
    }

    #[must_use]
    pub const fn current_meta(&self) -> VertexMeta {
        self.current_meta
    }

    #[must_use]
    pub const fn current_color(&self) -> [f32; 4] {
        self.current_color
    }

    pub fn push_vertex(&mut self, v: WorldVertex) -> u32 {
        let idx = self.mesh.vertices.len() as u32;
        self.mesh.vertices.push(v);
        idx
    }

    pub fn push_triangle(
        &mut self,
        p0: Vec3,
        p1: Vec3,
        p2: Vec3,
        color: [f32; 4],
        meta: VertexMeta,
    ) {
        let cross = (p1 - p0).cross(p2 - p0);
        let normal = if cross.length_squared() > 1e-12 {
            cross.normalize()
        } else {
            Vec3::Y
        };
        let base = self.mesh.vertices.len() as u32;
        self.mesh
            .vertices
            .push(WorldVertex::with_meta(p0, normal, color, meta));
        self.mesh
            .vertices
            .push(WorldVertex::with_meta(p1, normal, color, meta));
        self.mesh
            .vertices
            .push(WorldVertex::with_meta(p2, normal, color, meta));
        self.mesh
            .indices
            .extend_from_slice(&[base, base + 1, base + 2]);
    }

    pub fn push_quad(
        &mut self,
        p0: Vec3,
        p1: Vec3,
        p2: Vec3,
        p3: Vec3,
        color: [f32; 4],
        meta: VertexMeta,
    ) {
        let cross = (p1 - p0).cross(p2 - p0);
        let normal = if cross.length_squared() > 1e-12 {
            cross.normalize()
        } else {
            Vec3::Y
        };
        self.push_quad_with_normal(p0, p1, p2, p3, normal, color, meta);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push_quad_with_normal(
        &mut self,
        p0: Vec3,
        p1: Vec3,
        p2: Vec3,
        p3: Vec3,
        normal: Vec3,
        color: [f32; 4],
        meta: VertexMeta,
    ) {
        let base = self.mesh.vertices.len() as u32;
        self.mesh
            .vertices
            .push(WorldVertex::with_meta(p0, normal, color, meta));
        self.mesh
            .vertices
            .push(WorldVertex::with_meta(p1, normal, color, meta));
        self.mesh
            .vertices
            .push(WorldVertex::with_meta(p2, normal, color, meta));
        self.mesh
            .vertices
            .push(WorldVertex::with_meta(p3, normal, color, meta));
        self.mesh
            .indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    pub fn append_mesh(&mut self, other: &MeshBuffer) -> &mut Self {
        self.mesh.append(other);
        self
    }

    pub fn append_transformed(&mut self, other: &MeshBuffer, transform: Mat4) -> &mut Self {
        self.mesh.append_transformed(other, transform);
        self
    }

    #[must_use]
    pub fn as_mesh(&self) -> &MeshBuffer {
        &self.mesh
    }

    #[must_use]
    pub fn as_mesh_mut(&mut self) -> &mut MeshBuffer {
        &mut self.mesh
    }

    #[must_use]
    pub fn build(self) -> MeshBuffer {
        self.mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_vertex_and_meta_bitpacking_roundtrip() {
        assert_eq!(std::mem::size_of::<WorldVertex>(), 48);

        for tag in [
            GeometryTag::Terrain,
            GeometryTag::Road,
            GeometryTag::Water,
            GeometryTag::Bridge,
            GeometryTag::InteriorFloor,
            GeometryTag::WallLower,
            GeometryTag::WallUpper,
            GeometryTag::DoorOrWindow,
            GeometryTag::Roof,
            GeometryTag::Prop,
            GeometryTag::Actor,
            GeometryTag::Effect,
        ] {
            for floor in [0_u8, 2_u8, EXTERIOR_FLOOR_SENTINEL] {
                for flags in [
                    MaterialFlags::NONE,
                    MaterialFlags::UNLIT,
                    MaterialFlags::ACTOR_MIN_LIGHT,
                    MaterialFlags::INDOOR_AMBIENT | MaterialFlags::CUTAWAY_EXEMPT,
                ] {
                    for ao in [0_u8, 128_u8, 255_u8] {
                        for building_id in [0_u16, 42_u16, 65535_u16] {
                            for room_id in [0_u16, 7_u16, 4242_u16] {
                                let meta = VertexMeta {
                                    tag,
                                    floor,
                                    flags,
                                    ao,
                                    building_id,
                                    room_id,
                                };
                                let packed = meta.pack();
                                let unpacked = VertexMeta::unpack(packed);
                                assert_eq!(unpacked, meta);
                            }
                        }
                    }
                }
            }
        }
    }
}
