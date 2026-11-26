//! Shared 3D spatial primitives, support identifiers, body state, and terrain sampling (`+z` up).

use crate::{Floor, Position, Tile3D};

pub mod action_adapter;
pub mod collision;
pub mod kinematics;
pub mod math;
pub mod query;
pub mod support;
pub mod terrain;

pub use action_adapter::{
    ActionSpatialView, WorldSpatialView, resolve_actor_support, resolve_tile_support,
    semantic_floor_for_body,
};
pub use collision::{raycast_world, sphere_cast_world};
pub use kinematics::{
    MoveParams, MoveResult, move_kinematic_with_horizontal_vel, step_kinematic_body,
};
pub use math::{Aabb3, Vec2, Vec3, world_to_tile_coord};
pub use query::{
    Collider, ColliderKind, Hit3, HitSurface, QueryFilter, Ray3, SpatialWorld, SupportSurface,
    SurfaceQuad,
};
pub use support::{
    AirborneSupportHit, StairGeometry, select_grounded_support, sweep_airborne_support,
};
pub use terrain::{
    CHUNK_VERTICES, TERRAIN_GRID_SIDE, TerrainGrid, TerrainTriangle, sample_quad_00_11,
};

/// Vertical floor-to-floor increment in world units (~34 px in 2:1 isometric at 28.284 px/unit).
pub const FLOOR_H: f32 = 1.388;

/// Structural wall segment thickness (`[-0.08, +0.08]` around tile centerline).
pub const WALL_THICKNESS: f32 = 0.16;
pub const WALL_HALF_THICKNESS: f32 = WALL_THICKNESS * 0.5;

/// Upper-floor & roof structural slab thickness below `floor_elevation`.
pub const SLAB_THICKNESS: f32 = 0.16;

/// Clear floor-to-ceiling wall height beneath an upper floor slab (`1.388 - 0.16 = 1.228`).
pub const WALL_HEIGHT: f32 = FLOOR_H - SLAB_THICKNESS;

/// Window sill top offset above floor `z` (`0.38 > max_step_up = 0.22`).
pub const WINDOW_SILL_HEIGHT: f32 = 0.38;

/// Window lintel bottom offset above floor `z` (aperture spans `[0.38, 1.02]`).
pub const WINDOW_LINTEL_Z: f32 = 1.02;

/// Clear doorway opening height above floor `z` (`1.15 > Bloater height 1.12`).
pub const DOOR_OPENING_HEIGHT: f32 = 1.15;

/// Bridge deck slab thickness and open-edge parapet guardrail height.
pub const BRIDGE_SLAB_THICKNESS: f32 = 0.20;
pub const BRIDGE_PARAPET_HEIGHT: f32 = 0.45;

/// Unified county water-table elevation (`+z` up) shared by rivers and lakes.
pub const WATER_SURFACE_Z: f32 = 0.0;

/// Maximum allowed steepest-descent grade (`|grad z|`) on dry county roads (`<= 20%`).
pub const MAX_ROAD_GRADE: f32 = 0.20;

/// Minimum vertical clearance of bridge decks above `WATER_SURFACE_Z`.
pub const MIN_BRIDGE_CLEARANCE: f32 = 0.90;

#[inline]
pub const fn floor_elevation(foundation_z: f32, floor: Floor) -> f32 {
    foundation_z + (floor as f32) * FLOOR_H
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SupportId {
    Terrain,
    Floor(Tile3D),
    Stair(Tile3D),
    Bridge(Position),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MotionState {
    Grounded(SupportId),
    Airborne,
}

impl Default for MotionState {
    fn default() -> Self {
        Self::Grounded(SupportId::Terrain)
    }
}

impl MotionState {
    #[inline]
    pub const fn is_grounded(self) -> bool {
        matches!(self, Self::Grounded(_))
    }

    #[inline]
    pub const fn support(self) -> Option<SupportId> {
        match self {
            Self::Grounded(id) => Some(id),
            Self::Airborne => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyShape {
    pub radius: f32,
    pub height: f32,
}

impl BodyShape {
    pub const fn new(radius: f32, height: f32) -> Self {
        Self { radius, height }
    }

    #[inline]
    pub fn aabb_at(self, foot_pos: Vec3) -> Aabb3 {
        Aabb3::new(
            Vec3::new(
                foot_pos.x - self.radius,
                foot_pos.y - self.radius,
                foot_pos.z,
            ),
            Vec3::new(
                foot_pos.x + self.radius,
                foot_pos.y + self.radius,
                foot_pos.z + self.height,
            ),
        )
    }

    #[inline]
    pub fn swept_aabb(self, from_foot: Vec3, to_foot: Vec3) -> Aabb3 {
        self.aabb_at(from_foot).union(self.aabb_at(to_foot))
    }

    #[inline]
    pub fn center_at(self, foot_pos: Vec3) -> Vec3 {
        foot_pos.with_z(foot_pos.z + self.height * 0.5)
    }

    #[inline]
    pub fn eye_at(self, foot_pos: Vec3) -> Vec3 {
        foot_pos.with_z(foot_pos.z + self.height * 0.85)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyState {
    /// Ground contact / foot position in simulation space (`+z` up).
    pub position: Vec3,
    pub velocity: Vec3,
    /// Horizontal yaw in radians (`atan2(facing.y, facing.x)`).
    pub heading: f32,
    pub pitch: f32,
    pub roll: f32,
    pub motion: MotionState,
}

impl BodyState {
    pub const fn grounded(position: Vec3, heading: f32, support: SupportId) -> Self {
        Self {
            position,
            velocity: Vec3::ZERO,
            heading,
            pitch: 0.0,
            roll: 0.0,
            motion: MotionState::Grounded(support),
        }
    }

    pub const fn airborne(position: Vec3, velocity: Vec3, heading: f32) -> Self {
        Self {
            position,
            velocity,
            heading,
            pitch: 0.0,
            roll: 0.0,
            motion: MotionState::Airborne,
        }
    }

    #[inline]
    pub fn from_pose_2d(pos: Vec2, facing: Vec2, z: f32, support: SupportId) -> Self {
        let heading = if facing.length_squared() > 1e-8 {
            facing.y.atan2(facing.x)
        } else {
            0.0
        };
        Self::grounded(pos.extend(z), heading, support)
    }

    #[inline]
    pub fn facing_xy(self) -> Vec2 {
        Vec2::new(self.heading.cos(), self.heading.sin())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainSample {
    /// Sampled surface/bed height (`+z` up) from the `00 -> 11` triangle mesh.
    pub z: f32,
    /// Unit upward-pointing analytical surface normal of the sampled triangle (`normal.z > 0.0`).
    pub normal: Vec3,
    /// Tile index `(tx, ty)` whose `[tx - 0.5, tx + 0.5] x [ty - 0.5, ty + 0.5]` cell contains the query.
    pub tile: Position,
    /// Water surface plane (`Some(WATER_SURFACE_Z)`) if `tile` has `water.is_some()` (including beneath bridges).
    pub water_z: Option<f32>,
    /// Which triangle of the `00 -> 11` split contained the sample.
    pub triangle: TerrainTriangle,
}

impl TerrainSample {
    #[inline]
    pub fn is_submerged(self) -> bool {
        self.water_z.is_some_and(|wz| self.z < wz)
    }

    #[inline]
    pub fn water_depth(self) -> f32 {
        self.water_z.map_or(0.0, |wz| (wz - self.z).max(0.0))
    }

    /// Exact steepest-descent grade `|grad z| = tan(acos(normal.z))` (`0.0` on flat ground).
    #[inline]
    pub fn slope(self) -> f32 {
        self.normal.xy().length() / self.normal.z.max(1e-6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_shape_and_floor_elevation() {
        assert!((floor_elevation(2.5, 2) - (2.5 + 2.0 * 1.388)).abs() < 1e-6);
        let shape = BodyShape::new(0.25, 1.8);
        let a = shape.aabb_at(Vec3::new(1.0, 2.0, 0.5));
        assert_eq!(a.min, Vec3::new(0.75, 1.75, 0.5));
        assert_eq!(a.max, Vec3::new(1.25, 2.25, 2.3));

        let swept = shape.swept_aabb(Vec3::new(1.0, 2.0, 0.5), Vec3::new(3.0, 2.0, 1.5));
        assert_eq!(swept.min, Vec3::new(0.75, 1.75, 0.5));
        assert_eq!(swept.max, Vec3::new(3.25, 2.25, 3.3));
    }

    #[test]
    fn body_state_pose_roundtrip() {
        let s = BodyState::from_pose_2d(
            Vec2::new(4.0, 5.0),
            Vec2::new(0.0, 1.0),
            1.25,
            SupportId::Terrain,
        );
        assert!(s.motion.is_grounded());
        assert_eq!(s.motion.support(), Some(SupportId::Terrain));
        assert!((s.facing_xy().x).abs() < 1e-6);
        assert!((s.facing_xy().y - 1.0).abs() < 1e-6);
    }
}
