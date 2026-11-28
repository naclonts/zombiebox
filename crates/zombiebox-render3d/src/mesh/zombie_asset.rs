//! Runtime reader for the compact indexed skinned zombie mesh format (`ZBZS`).
use glam::Vec3;

use crate::animation::zombie::ZombieArchetype;

/// Bundled indexed geometry for one archetype and one of its four authored detail levels.
#[must_use]
pub fn bundled_zombie_lod(archetype: ZombieArchetype, lod: u8) -> &'static [u8] {
    let level: usize = lod.min(3).into();
    let meshes: [&'static [u8]; 4] = match archetype {
        ZombieArchetype::Walker => [
            include_bytes!("../../../../assets/zombies/converted/walker-lod0.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/walker-lod1.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/walker-lod2.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/walker-lod3.zbzs"),
        ],
        ZombieArchetype::Woman => [
            include_bytes!("../../../../assets/zombies/converted/woman-lod0.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/woman-lod1.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/woman-lod2.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/woman-lod3.zbzs"),
        ],
        ZombieArchetype::Runner => [
            include_bytes!("../../../../assets/zombies/converted/runner-lod0.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/runner-lod1.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/runner-lod2.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/runner-lod3.zbzs"),
        ],
        ZombieArchetype::Bloated => [
            include_bytes!("../../../../assets/zombies/converted/bloated-lod0.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/bloated-lod1.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/bloated-lod2.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/bloated-lod3.zbzs"),
        ],
        ZombieArchetype::Soldier => [
            include_bytes!("../../../../assets/zombies/converted/soldier-lod0.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/soldier-lod1.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/soldier-lod2.zbzs"),
            include_bytes!("../../../../assets/zombies/converted/soldier-lod3.zbzs"),
        ],
    };
    meshes[level]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkinnedZombieVertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub color: [u8; 4],
    pub joints: [u16; 4],
    /// Four normalized u16 weights. Their integer sum is expected to be 65535.
    pub weights: [u16; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct SkinnedZombieMesh {
    pub vertices: Vec<SkinnedZombieVertex>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkinnedZombieMeshError {
    Truncated,
    BadMagic,
    UnsupportedVersion(u32),
    NonZeroReserved,
    InvalidLength,
    IndexOutOfBounds,
}

impl SkinnedZombieMesh {
    #[must_use]
    pub fn compute_aabb(&self) -> Option<crate::RenderAabb> {
        let first = self.vertices.first()?;
        let mut min = first.position;
        let mut max = first.position;
        for vertex in &self.vertices[1..] {
            min = min.min(vertex.position);
            max = max.max(vertex.position);
        }
        Some(crate::RenderAabb::new(min, max))
    }

    /// Decode a version 1 little-endian `.zbzs` file. The parser validates all sizes and indices.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SkinnedZombieMeshError> {
        if bytes.len() < 20 {
            return Err(SkinnedZombieMeshError::Truncated);
        }
        if &bytes[0..4] != b"ZBZS" {
            return Err(SkinnedZombieMeshError::BadMagic);
        }
        let version = u32_at(bytes, 4)?;
        if version != 1 {
            return Err(SkinnedZombieMeshError::UnsupportedVersion(version));
        }
        let vertex_count = usize::try_from(u32_at(bytes, 8)?)
            .map_err(|_| SkinnedZombieMeshError::InvalidLength)?;
        let index_count = usize::try_from(u32_at(bytes, 12)?)
            .map_err(|_| SkinnedZombieMeshError::InvalidLength)?;
        if u32_at(bytes, 16)? != 0 {
            return Err(SkinnedZombieMeshError::NonZeroReserved);
        }
        let vertex_bytes = vertex_count
            .checked_mul(44)
            .ok_or(SkinnedZombieMeshError::InvalidLength)?;
        let index_bytes = index_count
            .checked_mul(4)
            .ok_or(SkinnedZombieMeshError::InvalidLength)?;
        let expected = 20usize
            .checked_add(vertex_bytes)
            .and_then(|n| n.checked_add(index_bytes))
            .ok_or(SkinnedZombieMeshError::InvalidLength)?;
        if bytes.len() != expected {
            return Err(SkinnedZombieMeshError::InvalidLength);
        }

        let mut vertices = Vec::with_capacity(vertex_count);
        for index in 0..vertex_count {
            let start = 20 + index * 44;
            let mut cursor = start;
            let position = Vec3::new(
                f32_at(bytes, &mut cursor)?,
                f32_at(bytes, &mut cursor)?,
                f32_at(bytes, &mut cursor)?,
            );
            let normal = Vec3::new(
                f32_at(bytes, &mut cursor)?,
                f32_at(bytes, &mut cursor)?,
                f32_at(bytes, &mut cursor)?,
            );
            let color = [
                bytes[cursor],
                bytes[cursor + 1],
                bytes[cursor + 2],
                bytes[cursor + 3],
            ];
            cursor += 4;
            let joints = [
                u16_at(bytes, &mut cursor)?,
                u16_at(bytes, &mut cursor)?,
                u16_at(bytes, &mut cursor)?,
                u16_at(bytes, &mut cursor)?,
            ];
            let weights = [
                u16_at(bytes, &mut cursor)?,
                u16_at(bytes, &mut cursor)?,
                u16_at(bytes, &mut cursor)?,
                u16_at(bytes, &mut cursor)?,
            ];
            vertices.push(SkinnedZombieVertex {
                position,
                normal,
                color,
                joints,
                weights,
            });
        }
        let indices_at = 20 + vertex_bytes;
        let mut indices = Vec::with_capacity(index_count);
        for i in 0..index_count {
            let cursor = indices_at + i * 4;
            let value = u32_at(bytes, cursor)?;
            if value as usize >= vertex_count {
                return Err(SkinnedZombieMeshError::IndexOutOfBounds);
            }
            indices.push(value);
        }
        Ok(Self { vertices, indices })
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, SkinnedZombieMeshError> {
    let raw: [u8; 4] = bytes
        .get(at..at + 4)
        .ok_or(SkinnedZombieMeshError::Truncated)?
        .try_into()
        .map_err(|_| SkinnedZombieMeshError::Truncated)?;
    Ok(u32::from_le_bytes(raw))
}

fn u16_at(bytes: &[u8], cursor: &mut usize) -> Result<u16, SkinnedZombieMeshError> {
    let raw: [u8; 2] = bytes
        .get(*cursor..*cursor + 2)
        .ok_or(SkinnedZombieMeshError::Truncated)?
        .try_into()
        .map_err(|_| SkinnedZombieMeshError::Truncated)?;
    *cursor += 2;
    Ok(u16::from_le_bytes(raw))
}

fn f32_at(bytes: &[u8], cursor: &mut usize) -> Result<f32, SkinnedZombieMeshError> {
    let raw = u32_at(bytes, *cursor)?;
    *cursor += 4;
    Ok(f32::from_bits(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"ZBZS");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        for value in [0.0f32, 1.0, 0.0, 0.0, 1.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[255, 128, 64, 255]);
        for joint in [0u16; 4] {
            bytes.extend_from_slice(&joint.to_le_bytes());
        }
        bytes.extend_from_slice(&u16::MAX.to_le_bytes());
        for _ in 0..3 {
            bytes.extend_from_slice(&0u16.to_le_bytes());
        }
        for index in [0u32, 0, 0] {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn reads_zbzs_v1_and_rejects_invalid_indices() {
        let bytes = sample();
        let mesh = SkinnedZombieMesh::from_bytes(&bytes).unwrap();
        assert_eq!(mesh.vertices.len(), 1);
        assert_eq!(mesh.indices, [0, 0, 0]);
        let mut invalid = bytes;
        *invalid.last_mut().unwrap() = 1;
        assert_eq!(
            SkinnedZombieMesh::from_bytes(&invalid),
            Err(SkinnedZombieMeshError::IndexOutOfBounds)
        );
    }

    #[test]
    fn all_bundled_archetype_lods_parse_and_have_full_height_bounds() {
        use crate::animation::zombie::ZombieArchetype;
        for archetype in [
            ZombieArchetype::Walker,
            ZombieArchetype::Woman,
            ZombieArchetype::Runner,
            ZombieArchetype::Bloated,
            ZombieArchetype::Soldier,
        ] {
            for lod in 0..4 {
                let mesh = SkinnedZombieMesh::from_bytes(bundled_zombie_lod(archetype, lod))
                    .unwrap_or_else(|error| panic!("{archetype:?} LOD {lod}: {error:?}"));
                assert!(!mesh.indices.is_empty());
                assert!(mesh.indices.len().is_multiple_of(3));
                if lod == 0 {
                    let bounds = mesh.compute_aabb().expect("nonempty LOD 0");
                    assert!(
                        (bounds.max.y - bounds.min.y - 1.0).abs() < 0.04,
                        "{archetype:?} LOD 0 height {}",
                        bounds.max.y - bounds.min.y
                    );
                }
            }
        }
    }
}
