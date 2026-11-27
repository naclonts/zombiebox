use super::{
    FaceMask, GeometryTag, MaterialFlags, MeshBuffer, MeshBuilder, VertexMeta, emit_box_min_max,
};
use crate::scene::ChunkSnapshot;
use glam::{Mat4, Vec3};
use std::f32::consts::FRAC_PI_2;
#[cfg(feature = "gpu")]
use zombiebox_core::Tile3D;
use zombiebox_core::{CHUNK_SIZE, DOOR_OPENING_HEIGHT, FLOOR_H, TileKind, WallOrientation};

const HALF_WIDTH: f32 = 0.45;
const HANDLE_X: f32 = 0.80;
const HANDLE_Y: f32 = 0.59;
const PANEL_HALF_DEPTH: f32 = 0.023;
const HANDLE_DEPTH: f32 = 0.045;

pub(crate) const DOOR_REACH_SECONDS: f32 = 0.44;

#[cfg(feature = "gpu")]
pub(crate) struct AnimatedDoor {
    pub leaf: DoorLeaf,
    from: f32,
    elapsed: f32,
}

#[cfg(feature = "gpu")]
impl AnimatedDoor {
    pub fn new(leaf: DoorLeaf, previous: Option<&Self>, toggled: bool) -> Self {
        let target = f32::from(leaf.opened);
        let (from, elapsed) = match previous {
            Some(door) if door.leaf.opened == leaf.opened => (door.from, door.elapsed),
            Some(door) => (door.openness(), 0.0),
            None if toggled => (1.0 - target, 0.0),
            None => (target, DOOR_REACH_SECONDS),
        };
        Self {
            leaf,
            from,
            elapsed,
        }
    }

    pub fn openness(&self) -> f32 {
        let t = ((self.elapsed - 0.075) / 0.25).clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        self.from + (f32::from(self.leaf.opened) - self.from) * eased
    }

    pub fn advance(&mut self, dt: f32) {
        self.elapsed = (self.elapsed + dt).min(DOOR_REACH_SECONDS);
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DoorLeaf {
    #[cfg(feature = "gpu")]
    pub tile: Tile3D,
    pub opened: bool,
    pub meta: VertexMeta,
    tint: f32,
    hinge: Vec3,
    yaw: f32,
    swing: f32,
}

impl DoorLeaf {
    pub fn model(self, openness: f32) -> Mat4 {
        Mat4::from_translation(self.hinge) * Mat4::from_rotation_y(self.yaw + self.swing * openness)
    }

    pub fn tint(self) -> [f32; 4] {
        [self.tint, self.tint, self.tint, 1.0]
    }

    #[cfg(feature = "gpu")]
    pub fn handle(self, openness: f32, player: Vec3) -> Vec3 {
        let model = self.model(openness);
        let normal = model.transform_vector3(Vec3::Z);
        let side = if (player - self.hinge).dot(normal) < 0.0 {
            -1.0
        } else {
            1.0
        };
        model.transform_point3(Vec3::new(HANDLE_X, HANDLE_Y, side * HANDLE_DEPTH))
    }
}

pub(crate) fn door_leaves(chunk: &ChunkSnapshot) -> impl Iterator<Item = DoorLeaf> + '_ {
    (0..CHUNK_SIZE).flat_map(move |y| {
        (0..CHUNK_SIZE).flat_map(move |x| {
            let tile = chunk.local_tile(x, y).unwrap();
            (0..tile.floor_count).filter_map(move |floor| {
                let c = tile.floor(floor)?;
                let TileKind::Door { open, exterior } = c.kind else {
                    return None;
                };
                let info = chunk.building(c.building_id)?.info;
                let along_y = c.orientation == WallOrientation::AlongY;
                let base = Vec3::new(
                    tile.pos.x as f32,
                    info.foundation_z + floor as f32 * FLOOR_H,
                    tile.pos.y as f32,
                );
                let along = if along_y { Vec3::Z } else { Vec3::X };
                let normal = if along_y { Vec3::NEG_X } else { Vec3::Z };
                let center = Vec3::new(
                    info.x as f32 + (info.width - 1) as f32 * 0.5,
                    base.y,
                    info.y as f32 + (info.height - 1) as f32 * 0.5,
                );
                let inward = if exterior && (center - base).dot(normal) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                Some(DoorLeaf {
                    #[cfg(feature = "gpu")]
                    tile: Tile3D::from_pos(tile.pos, floor),
                    opened: open,
                    meta: VertexMeta::DEFAULT
                        .with_tag(GeometryTag::DoorOrWindow)
                        .with_floor(floor)
                        .with_flags(MaterialFlags::INDOOR_AMBIENT)
                        .with_building(c.building_id, 0),
                    tint: match c.damage_tier() {
                        1 => 0.85,
                        2 => 0.68,
                        _ => 1.0,
                    },
                    hinge: base - along * HALF_WIDTH,
                    yaw: if along_y { -FRAC_PI_2 } else { 0.0 },
                    swing: -FRAC_PI_2 * inward,
                })
            })
        })
    })
}

pub(crate) fn door_leaf_mesh() -> MeshBuffer {
    let mut b = MeshBuilder::new();
    let mut part = |min, max, color| {
        emit_box_min_max(&mut b, min, max, FaceMask::ALL, color, VertexMeta::DEFAULT);
    };
    part(
        Vec3::new(0.0, 0.012, -PANEL_HALF_DEPTH),
        Vec3::new(
            HALF_WIDTH * 2.0,
            DOOR_OPENING_HEIGHT - 0.014,
            PANEL_HALF_DEPTH,
        ),
        [0.36, 0.24, 0.16, 1.0],
    );
    part(
        Vec3::new(HANDLE_X - 0.029, HANDLE_Y - 0.019, -HANDLE_DEPTH),
        Vec3::new(HANDLE_X + 0.021, HANDLE_Y + 0.019, HANDLE_DEPTH),
        [0.58, 0.46, 0.27, 1.0],
    );
    b.build()
}

#[cfg(all(test, feature = "gpu"))]
mod tests {
    use super::*;

    #[test]
    fn a_door_hinges_through_intermediate_positions_and_reverses_smoothly() {
        let leaf = DoorLeaf {
            tile: Tile3D::new(5, 6, 0),
            opened: false,
            meta: VertexMeta::DEFAULT,
            tint: 1.0,
            hinge: Vec3::new(4.55, 0.0, 6.0),
            yaw: 0.0,
            swing: -FRAC_PI_2,
        };
        let closed = AnimatedDoor::new(leaf, None, false);
        let open_leaf = DoorLeaf {
            opened: true,
            ..leaf
        };
        let mut opening = AnimatedDoor::new(open_leaf, Some(&closed), true);
        assert_eq!(opening.openness(), 0.0);
        opening.advance(0.20);
        assert!((opening.openness() - 0.5).abs() < 1e-5);
        let mid = leaf.handle(opening.openness(), Vec3::new(5.0, 0.0, 6.6));
        assert!(mid.x > leaf.hinge.x && mid.z > leaf.hinge.z);
        assert_eq!(
            leaf.model(opening.openness()).transform_point3(Vec3::ZERO),
            leaf.hinge
        );
        let mut closing = AnimatedDoor::new(leaf, Some(&opening), true);
        assert_eq!(closing.openness(), opening.openness());
        closing.advance(DOOR_REACH_SECONDS);
        assert_eq!(closing.openness(), 0.0);
    }
}
