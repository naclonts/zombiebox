//! Deterministic CPU meshes for static county props and reusable cache geometry.
use super::{GeometryTag, MaterialFlags, MeshBuffer, MeshBuilder, VertexMeta};
use crate::coords::ground_align_quat;
use crate::materials::MaterialRegistry;
use crate::mesh::library::PropMeshLibrary;
use crate::scene::snapshot::{CacheSnapshot, ChunkSnapshot};
use glam::{Mat4, Quat, Vec3};
use zombiebox_core::{
    CHUNK_SIZE, CacheId, PropKind, TileKind, TileVisibility, WallOrientation, sample_quad_00_11,
};

const WOOD: [f32; 4] = [0.47, 0.30, 0.18, 1.];
const WOOD_L: [f32; 4] = [0.68, 0.47, 0.27, 1.];
const METAL: [f32; 4] = [0.31, 0.36, 0.39, 1.];
const LEAF: [f32; 4] = [0.16, 0.39, 0.19, 1.];
const META: VertexMeta = VertexMeta::DEFAULT.with_tag(GeometryTag::Prop);

fn bx(b: &mut MeshBuilder, a: Vec3, z: Vec3, c: [f32; 4]) {
    crate::mesh::primitives::emit_box_min_max(b, a, z, super::FaceMask::ALL, c, META);
}
fn cylinder(b: &mut MeshBuilder, at: Vec3, radius: f32, height: f32, color: [f32; 4]) {
    crate::mesh::primitives::emit_cylinder(
        b, at, radius, radius, height, 10, true, true, color, META,
    );
}
fn cone(b: &mut MeshBuilder, at: Vec3, radius: f32, height: f32, color: [f32; 4]) {
    crate::mesh::primitives::emit_cone(b, at, radius, height, 8, true, color, META);
}

pub struct PropMeshBuilder;
impl PropMeshBuilder {
    #[must_use]
    pub fn build_prop(kind: PropKind) -> MeshBuffer {
        match kind {
            PropKind::BirchTree => Self::build_tree(),
            PropKind::Fence => Self::build_fence(true),
            PropKind::Table => Self::build_furniture("table"),
            PropKind::Sofa => Self::build_furniture("sofa"),
            PropKind::Counter => Self::build_furniture("counter"),
            PropKind::Bed => Self::build_furniture("bed"),
            PropKind::Crate => Self::build_furniture("crate"),
            PropKind::Mailbox => Self::build_furniture("mailbox"),
            PropKind::Bench => Self::build_furniture("bench"),
            PropKind::Shed => Self::build_furniture("shed"),
        }
    }
    #[must_use]
    pub fn build_tree() -> MeshBuffer {
        let mut b = MeshBuilder::new();
        cylinder(&mut b, Vec3::new(0., 0., 0.), 0.13, 1.15, WOOD);
        cone(&mut b, Vec3::new(0., 0.9, 0.), 0.62, 1.0, LEAF);
        cone(
            &mut b,
            Vec3::new(0., 1.35, 0.),
            0.48,
            0.9,
            [0.22, 0.46, 0.22, 1.],
        );
        cone(
            &mut b,
            Vec3::new(0., 1.72, 0.),
            0.31,
            0.65,
            [0.32, 0.48, 0.20, 1.],
        );
        b.build()
    }
    #[must_use]
    pub fn build_streetlight() -> MeshBuffer {
        let mut b = MeshBuilder::new();
        cylinder(&mut b, Vec3::ZERO, 0.055, 2.4, METAL);
        bx(
            &mut b,
            Vec3::new(0., 2.3, -0.05),
            Vec3::new(0.52, 2.38, 0.05),
            METAL,
        );
        bx(
            &mut b,
            Vec3::new(0.35, 2.24, -0.09),
            Vec3::new(0.51, 2.31, 0.09),
            [1., 0.84, 0.48, 1.],
        );
        b.build()
    }
    #[must_use]
    pub fn build_fence(wooden: bool) -> MeshBuffer {
        let mut b = MeshBuilder::new();
        let c = if wooden { WOOD_L } else { METAL };
        let y = if wooden { 0.65 } else { 0.9 };
        bx(
            &mut b,
            Vec3::new(-0.5, y - 0.08, -0.04),
            Vec3::new(0.5, y + 0.08, 0.04),
            c,
        );
        if wooden {
            for x in [-0.45, -0.15, 0.15, 0.45] {
                bx(
                    &mut b,
                    Vec3::new(x - 0.035, 0., -0.05),
                    Vec3::new(x + 0.035, 1.0, 0.05),
                    WOOD,
                );
            }
        } else {
            for i in 0..8 {
                let x = -0.5 + i as f32 / 7.;
                bx(
                    &mut b,
                    Vec3::new(x - 0.012, 0., -0.018),
                    Vec3::new(x + 0.012, 1.8, 0.018),
                    METAL,
                );
            }
            for yy in [0.3, 0.7, 1.1, 1.5] {
                bx(
                    &mut b,
                    Vec3::new(-0.5, yy, -0.025),
                    Vec3::new(0.5, yy + 0.018, 0.025),
                    METAL,
                );
            }
        }
        b.build()
    }
    #[must_use]
    pub fn build_furniture(kind: &str) -> MeshBuffer {
        let mut b = MeshBuilder::new();
        match kind {
            "table" => {
                bx(
                    &mut b,
                    Vec3::new(-0.45, 0.58, -0.35),
                    Vec3::new(0.45, 0.72, 0.35),
                    WOOD_L,
                );
                for (x, z) in [(-0.36, -0.26), (0.36, -0.26), (-0.36, 0.26), (0.36, 0.26)] {
                    bx(
                        &mut b,
                        Vec3::new(x - 0.04, 0., z - 0.04),
                        Vec3::new(x + 0.04, 0.58, z + 0.04),
                        WOOD,
                    );
                }
            }
            "sofa" => {
                bx(
                    &mut b,
                    Vec3::new(-0.48, 0.18, -0.34),
                    Vec3::new(0.48, 0.58, 0.34),
                    [0.36, 0.25, 0.3, 1.],
                );
                bx(
                    &mut b,
                    Vec3::new(-0.48, 0.55, -0.34),
                    Vec3::new(0.48, 0.95, -0.16),
                    [0.42, 0.29, 0.33, 1.],
                );
            }
            "counter" => bx(
                &mut b,
                Vec3::new(-0.5, 0., -0.42),
                Vec3::new(0.5, 0.88, 0.42),
                WOOD_L,
            ),
            "bed" => {
                bx(
                    &mut b,
                    Vec3::new(-0.48, 0.15, -0.42),
                    Vec3::new(0.48, 0.34, 0.42),
                    WOOD,
                );
                bx(
                    &mut b,
                    Vec3::new(-0.44, 0.34, -0.4),
                    Vec3::new(0.44, 0.48, 0.4),
                    [0.68, 0.67, 0.58, 1.],
                );
                bx(
                    &mut b,
                    Vec3::new(-0.43, 0.49, -0.4),
                    Vec3::new(0.43, 0.62, -0.2),
                    [0.78, 0.75, 0.62, 1.],
                );
            }
            "crate" => {
                bx(
                    &mut b,
                    Vec3::new(-0.36, 0., -0.32),
                    Vec3::new(0.36, 0.65, 0.32),
                    WOOD,
                );
                bx(
                    &mut b,
                    Vec3::new(-0.38, 0.65, -0.34),
                    Vec3::new(0.38, 0.72, 0.34),
                    WOOD_L,
                );
            }
            "mailbox" => {
                cylinder(&mut b, Vec3::new(0., 0., 0.), 0.035, 0.58, METAL);
                bx(
                    &mut b,
                    Vec3::new(-0.2, 0.58, -0.12),
                    Vec3::new(0.2, 0.88, 0.12),
                    [0.44, 0.2, 0.16, 1.],
                );
            }
            "bench" => {
                bx(
                    &mut b,
                    Vec3::new(-0.48, 0.5, -0.22),
                    Vec3::new(0.48, 0.62, 0.22),
                    WOOD_L,
                );
                bx(
                    &mut b,
                    Vec3::new(-0.44, 0.62, 0.12),
                    Vec3::new(0.44, 0.98, 0.22),
                    WOOD,
                );
            }
            "shed" => {
                bx(
                    &mut b,
                    Vec3::new(-0.5, 0., -0.5),
                    Vec3::new(0.5, 1.35, 0.5),
                    [0.5, 0.34, 0.22, 1.],
                );
                bx(
                    &mut b,
                    Vec3::new(-0.18, 0., 0.49),
                    Vec3::new(0.18, 0.85, 0.52),
                    WOOD,
                );
            }
            _ => {
                bx(
                    &mut b,
                    Vec3::new(-0.4, 0., -0.4),
                    Vec3::new(0.4, 0.65, 0.4),
                    WOOD,
                );
            }
        }
        b.build()
    }
    #[must_use]
    pub fn build_rubble() -> MeshBuffer {
        let mut b = MeshBuilder::new();
        for (x, y, z, s) in [
            (-0.28, 0.12, -0.15, 0.24),
            (0.08, 0.18, 0.08, 0.3),
            (0.3, 0.1, -0.22, 0.18),
        ] {
            bx(
                &mut b,
                Vec3::splat(-s) + Vec3::new(x, y, z),
                Vec3::splat(s) + Vec3::new(x, y, z),
                [0.39, 0.38, 0.34, 1.],
            );
        }
        b.build()
    }
    #[must_use]
    pub fn build_loot_cache(opened: bool) -> MeshBuffer {
        let mut b = MeshBuilder::new();
        let meta = if opened {
            META.with_flags(MaterialFlags::UNLIT)
        } else {
            META
        };
        crate::mesh::primitives::emit_box_min_max(
            &mut b,
            Vec3::new(-0.38, 0., -0.3),
            Vec3::new(0.38, 0.48, 0.3),
            super::FaceMask::ALL,
            if opened {
                [0.25, 0.32, 0.27, 1.]
            } else {
                [0.35, 0.38, 0.29, 1.]
            },
            meta,
        );
        if opened {
            bx(
                &mut b,
                Vec3::new(-0.4, 0.44, -0.32),
                Vec3::new(0.4, 0.91, -0.23),
                [0.48, 0.42, 0.27, 1.],
            );
            bx(
                &mut b,
                Vec3::new(-0.27, 0.48, -0.12),
                Vec3::new(0.27, 0.55, 0.14),
                [0.68, 0.56, 0.26, 1.],
            );
        } else {
            bx(
                &mut b,
                Vec3::new(-0.4, 0.48, -0.32),
                Vec3::new(0.4, 0.58, 0.32),
                [0.48, 0.42, 0.27, 1.],
            );
        }
        b.build()
    }
}

/// Static, chunk-local CPU mesh. Dynamic loot caches are intentionally excluded.
#[must_use]
pub fn build_chunk_props_mesh(chunk: &ChunkSnapshot) -> MeshBuffer {
    let mut out = MeshBuffer::new();
    let origin = chunk.coord.min_tile();
    for y in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            let Some(tile) = chunk.local_tile(x, y) else {
                continue;
            };
            for (floor, cell) in tile.floors[..usize::from(tile.floor_count)]
                .iter()
                .enumerate()
            {
                let mut mesh = match cell.kind {
                    TileKind::Prop(prop) => PropMeshBuilder::build_prop(prop),
                    TileKind::Streetlight => PropMeshBuilder::build_streetlight(),
                    TileKind::BrokenStreetlight => {
                        let mut stub = MeshBuilder::new();
                        cylinder(&mut stub, Vec3::ZERO, 0.055, 0.36, METAL);
                        stub.build()
                    }
                    TileKind::Rubble => PropMeshBuilder::build_rubble(),
                    _ => continue,
                };
                let (height, ground_normal) =
                    if let Some(building) = chunk.building(cell.building_id) {
                        (
                            building.info.foundation_z + floor as f32 * crate::FLOOR_H,
                            zombiebox_core::Vec3::Z,
                        )
                    } else {
                        let heights = &chunk.terrain.heights;
                        let z00 = heights[y as usize][x as usize];
                        let z10 = heights[y as usize][x as usize + 1];
                        let z01 = heights[y as usize + 1][x as usize];
                        let z11 = heights[y as usize + 1][x as usize + 1];
                        let (z, normal, _) = sample_quad_00_11(z00, z10, z01, z11, 0.5, 0.5);
                        (z, normal)
                    };
                let mut meta = META;
                if cell.building_id != 0 {
                    meta = meta.with_floor(floor as u8).with_building(
                        chunk
                            .building(cell.building_id)
                            .map_or(cell.building_id, |b| {
                                b.info.cutaway_slot_at(tile.pos, floor as u8)
                            }),
                        0,
                    );
                    if cell.indoor {
                        meta = meta.with_flags(MaterialFlags::INDOOR_AMBIENT);
                    }
                }
                let foliage = MaterialRegistry::for_district(tile.district).foliage;
                for vertex in &mut mesh.vertices {
                    vertex.meta = meta.pack();
                    if matches!(cell.kind, TileKind::Prop(PropKind::BirchTree))
                        && vertex.color[1] > vertex.color[0]
                    {
                        vertex.color[0] *= foliage.x / 0.22;
                        vertex.color[1] *= foliage.y / 0.40;
                        vertex.color[2] *= foliage.z / 0.22;
                    }
                }
                let slope_rotation = if cell.building_id == 0
                    && !matches!(
                        cell.kind,
                        TileKind::Prop(PropKind::BirchTree)
                            | TileKind::Streetlight
                            | TileKind::BrokenStreetlight
                    ) {
                    ground_align_quat(ground_normal)
                } else {
                    Quat::IDENTITY
                };
                let rotation = if matches!(cell.kind, TileKind::Prop(PropKind::Fence))
                    && cell.orientation == WallOrientation::AlongY
                {
                    slope_rotation * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
                } else {
                    slope_rotation
                };
                let position = Vec3::new((origin.x + x) as f32, height, (origin.y + y) as f32);
                out.append_transformed(&mesh, Mat4::from_rotation_translation(rotation, position));
            }
        }
    }
    // New catalog props are owned by the chunk snapshot. Searchable/container
    // props are omitted here and emitted as dynamic instances by Presentation.
    let library = PropMeshLibrary::shared();
    for view in &chunk.static_props {
        let Some(mesh) = library.get(view.id, view.state) else {
            continue;
        };
        let mut mesh = mesh.clone();
        let tile = chunk.world_tile(view.anchor.tile.pos());
        let mut meta = META.with_floor(view.floor);
        if let Some(cell) = tile.and_then(|t| t.floor(view.floor)) {
            if cell.building_id != 0 {
                let slot = chunk
                    .building(cell.building_id)
                    .map_or(cell.building_id, |b| {
                        b.info.cutaway_slot_at(view.anchor.tile.pos(), view.floor)
                    });
                meta = meta.with_building(slot, 0);
                if cell.indoor {
                    meta = meta.with_flags(MaterialFlags::INDOOR_AMBIENT);
                }
            }
        }
        for vertex in &mut mesh.vertices {
            vertex.meta = meta.pack();
        }
        out.append_transformed(&mesh, PropMeshLibrary::transform(view));
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CacheInstance {
    pub id: CacheId,
    pub opened: bool,
    pub transform: Mat4,
    pub tint: [f32; 4],
}
#[must_use]
pub fn cache_mesh(opened: bool) -> &'static MeshBuffer {
    static CLOSED: std::sync::OnceLock<MeshBuffer> = std::sync::OnceLock::new();
    static OPEN: std::sync::OnceLock<MeshBuffer> = std::sync::OnceLock::new();
    if opened {
        OPEN.get_or_init(|| PropMeshBuilder::build_loot_cache(true))
    } else {
        CLOSED.get_or_init(|| PropMeshBuilder::build_loot_cache(false))
    }
}
#[must_use]
pub fn cache_instances(caches: &[CacheSnapshot]) -> Vec<CacheInstance> {
    caches
        .iter()
        .filter(|c| c.visibility != TileVisibility::Hidden)
        .map(|c| CacheInstance {
            id: c.view.id,
            opened: c.view.opened,
            transform: Mat4::from_translation(Vec3::new(
                c.world_pos.x,
                c.world_pos.z,
                c.world_pos.y,
            )) * Mat4::from_quat(ground_align_quat(c.ground_normal)),
            tint: if c.visibility == TileVisibility::Explored {
                [0.55, 0.55, 0.55, 1.]
            } else {
                [1.; 4]
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn primitive_props_have_valid_geometry() {
        for m in [
            PropMeshBuilder::build_tree(),
            PropMeshBuilder::build_streetlight(),
            PropMeshBuilder::build_fence(true),
            PropMeshBuilder::build_furniture("bed"),
            PropMeshBuilder::build_rubble(),
            PropMeshBuilder::build_loot_cache(false),
        ] {
            assert!(m.validate().is_ok());
        }
        assert!(std::ptr::eq(cache_mesh(false), cache_mesh(false)));
    }
}
