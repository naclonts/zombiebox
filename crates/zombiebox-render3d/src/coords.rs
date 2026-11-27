//! World and tile coordinate conversion helpers and bounds (`T01`).
//!
//! Simulation space (`zombiebox-core`) uses `(x, y, z)` with `+x` East, `+y` South, and `+z` Up.
//! Renderer space (`zombiebox-render3d`) uses right-handed `(X, Y, Z) = (x, z, y)` with `+X` East,
//! `+Y` Up, and `+Z` South.

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec2, Vec3};
use zombiebox_core::{CHUNK_SIZE, Floor, Position, Tile3D, Vec2 as SimVec2};

pub use zombiebox_core::{Aabb3 as SimAabb3, FLOOR_H, Vec3 as SimVec3};

/// Half-width/half-depth of a single unit tile around its integer center `(tx, ty)`.
pub const TILE_HALF_EXTENT: f32 = 0.5;

/// Convenience alias for positions/vectors in right-handed renderer `(X, Y, Z)` space (`+Y` up).
pub type RenderVec3 = Vec3;

/// Axis-aligned bounding box in right-handed renderer `(X, Y, Z)` space (`+Y` up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderAabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl RenderAabb {
    #[must_use]
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self {
            min: min.min(max),
            max: min.max(max),
        }
    }

    #[must_use]
    pub fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    #[must_use]
    pub fn half_extents(self) -> Vec3 {
        (self.max - self.min) * 0.5
    }

    #[must_use]
    pub fn contains_point(self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.min.cmple(other.max).all() && self.max.cmpge(other.min).all()
    }
}

/// Signed chunk coordinate `(cx, cy)` supporting negative and county coordinates.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChunkCoord {
    pub cx: i32,
    pub cy: i32,
}

impl ChunkCoord {
    #[must_use]
    pub const fn new(cx: i32, cy: i32) -> Self {
        Self { cx, cy }
    }

    #[must_use]
    pub const fn from_tile(tx: i32, ty: i32) -> Self {
        Self {
            cx: tx.div_euclid(CHUNK_SIZE),
            cy: ty.div_euclid(CHUNK_SIZE),
        }
    }

    #[must_use]
    pub const fn from_pos(pos: Position) -> Self {
        Self::from_tile(pos.x, pos.y)
    }

    #[must_use]
    pub fn from_sim_xy(xy: SimVec2) -> Self {
        let p = xy.tile();
        Self::from_tile(p.x, p.y)
    }

    #[must_use]
    pub const fn min_tile(self) -> Position {
        Position::new(self.cx * CHUNK_SIZE, self.cy * CHUNK_SIZE)
    }

    #[must_use]
    pub const fn max_tile_inclusive(self) -> Position {
        Position::new(
            self.cx * CHUNK_SIZE + CHUNK_SIZE - 1,
            self.cy * CHUNK_SIZE + CHUNK_SIZE - 1,
        )
    }

    /// Continuous horizontal `(min_xy, max_xy)` bounds in simulation XY space.
    #[must_use]
    pub fn sim_bounds_xy(self) -> (SimVec2, SimVec2) {
        let min = self.min_tile();
        let max = self.max_tile_inclusive();
        (
            SimVec2::new(
                min.x as f32 - TILE_HALF_EXTENT,
                min.y as f32 - TILE_HALF_EXTENT,
            ),
            SimVec2::new(
                max.x as f32 + TILE_HALF_EXTENT,
                max.y as f32 + TILE_HALF_EXTENT,
            ),
        )
    }

    /// Continuous 3D chunk AABB in simulation `(x, y, z)` space.
    #[must_use]
    pub fn sim_aabb(self, min_z: f32, max_z: f32) -> SimAabb3 {
        let (min_xy, max_xy) = self.sim_bounds_xy();
        SimAabb3::from_points(min_xy.extend(min_z), max_xy.extend(max_z))
    }

    /// Continuous 3D chunk AABB in renderer `(X, Y, Z)` space (`min_y..=max_y` vertical span).
    #[must_use]
    pub fn render_aabb(self, min_y: f32, max_y: f32) -> RenderAabb {
        let (min_xy, max_xy) = self.sim_bounds_xy();
        RenderAabb::new(
            Vec3::new(min_xy.x, min_y, min_xy.y),
            Vec3::new(max_xy.x, max_y, max_xy.y),
        )
    }
}

// --- 2D Vec2 bridges ---

#[must_use]
pub const fn sim_vec2_to_glam(v: SimVec2) -> Vec2 {
    Vec2::new(v.x, v.y)
}

#[must_use]
pub const fn glam_vec2_to_sim(v: Vec2) -> SimVec2 {
    SimVec2::new(v.x, v.y)
}

// --- 3D Point / Vector / Normal conversions: Sim (x, y, z) <-> Render (X, Y, Z) = (x, z, y) ---

#[must_use]
pub const fn sim_xyz_to_render(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, z, y)
}

#[must_use]
pub const fn sim_to_render_point(p: SimVec3) -> Vec3 {
    Vec3::new(p.x, p.z, p.y)
}

#[must_use]
pub const fn render_to_sim_point(p: Vec3) -> SimVec3 {
    SimVec3::new(p.x, p.z, p.y)
}

#[must_use]
pub const fn sim_to_render_vec(v: SimVec3) -> Vec3 {
    Vec3::new(v.x, v.z, v.y)
}

#[must_use]
pub const fn render_to_sim_vec(v: Vec3) -> SimVec3 {
    SimVec3::new(v.x, v.z, v.y)
}

#[must_use]
pub fn sim_to_render_normal(n: SimVec3) -> Vec3 {
    Vec3::new(n.x, n.z, n.y).normalize_or_zero()
}

#[must_use]
pub fn render_to_sim_normal(n: Vec3) -> SimVec3 {
    let unit = n.normalize_or_zero();
    SimVec3::new(unit.x, unit.z, unit.y)
}

// --- AABB & Tile/Floor helpers ---

#[must_use]
pub fn sim_to_render_aabb(aabb: SimAabb3) -> RenderAabb {
    RenderAabb::new(sim_to_render_point(aabb.min), sim_to_render_point(aabb.max))
}

#[must_use]
pub fn render_to_sim_aabb(aabb: RenderAabb) -> SimAabb3 {
    SimAabb3::from_points(render_to_sim_point(aabb.min), render_to_sim_point(aabb.max))
}

#[must_use]
pub const fn floor_elevation(foundation_z: f32, floor: Floor) -> f32 {
    zombiebox_core::floor_elevation(foundation_z, floor)
}

#[must_use]
pub fn sim_xy_floor_to_render(xy: SimVec2, foundation_z: f32, floor: Floor) -> Vec3 {
    sim_xyz_to_render(xy.x, xy.y, floor_elevation(foundation_z, floor))
}

#[must_use]
pub fn tile_center_render(tile: Tile3D, foundation_z: f32) -> Vec3 {
    sim_xy_floor_to_render(tile.center(), foundation_z, tile.floor)
}

#[must_use]
pub fn tile_local_in_chunk(tx: i32, ty: i32) -> (usize, usize) {
    (
        tx.rem_euclid(CHUNK_SIZE) as usize,
        ty.rem_euclid(CHUNK_SIZE) as usize,
    )
}

#[must_use]
pub fn tile_sim_bounds_xy(tx: i32, ty: i32) -> (SimVec2, SimVec2) {
    (
        SimVec2::new(tx as f32 - TILE_HALF_EXTENT, ty as f32 - TILE_HALF_EXTENT),
        SimVec2::new(tx as f32 + TILE_HALF_EXTENT, ty as f32 + TILE_HALF_EXTENT),
    )
}

#[must_use]
pub fn tile_render_aabb(tx: i32, ty: i32, min_y: f32, max_y: f32) -> RenderAabb {
    let (min_xy, max_xy) = tile_sim_bounds_xy(tx, ty);
    RenderAabb::new(
        Vec3::new(min_xy.x, min_y, min_xy.y),
        Vec3::new(max_xy.x, max_y, max_xy.y),
    )
}

// --- Angles, Headings & Winding helpers ---

/// Wraps any angle in radians into `(-π, +π]`.
#[must_use]
pub fn wrap_angle_pi(radians: f32) -> f32 {
    if !radians.is_finite() {
        return 0.0;
    }
    let wrapped = (radians + PI).rem_euclid(TAU) - PI;
    if wrapped <= -PI {
        wrapped + TAU
    } else {
        wrapped
    }
}

/// Shortest-arc angular difference `target - current` in `(-π, +π]`.
#[must_use]
pub fn shortest_angle_delta(current: f32, target: f32) -> f32 {
    wrap_angle_pi(target - current)
}

/// Heading `atan2(dy, dx)` in simulation `(x, y)` (`0` = East `+x`, `+π/2` = South `+y`, `-π/2` = North `-y`).
/// Returns `0.0` if `dir` is near-zero or non-finite.
#[must_use]
pub fn sim_heading_from_vec2(dir: SimVec2) -> f32 {
    if dir.x.is_finite() && dir.y.is_finite() && dir.length_squared() > 1e-8 {
        dir.y.atan2(dir.x)
    } else {
        0.0
    }
}

/// Unit `SimVec2` `(cos h, sin h)` for a simulation heading `h`.
#[must_use]
pub fn sim_vec2_from_heading(heading: f32) -> SimVec2 {
    let (sin_h, cos_h) = heading.sin_cos();
    SimVec2::new(cos_h, sin_h)
}

/// Right-handed renderer rotation `Quat::from_rotation_y(-heading)` mapping local `+X` `(1, 0, 0)`
/// to `(cos heading, 0, sin heading)` in renderer space while preserving CCW triangle winding.
#[must_use]
pub fn heading_to_render_quat(heading: f32) -> Quat {
    Quat::from_rotation_y(-heading)
}

/// Maximum ground-alignment tilt applied to props, actors, and vehicles.
/// Keeps near-vertical mountain geometry from flipping a model onto its side.
pub const MAX_GROUND_TILT: f32 = 40.0 * PI / 180.0;

/// Rotation from renderer up `+Y` to a simulation-space ground normal.
///
/// Horizontal headings are mapped into the terrain plane, so a yawed model keeps its
/// compass heading while its wheels/feet rest on the slope.
#[must_use]
pub fn ground_align_quat(normal: SimVec3) -> Quat {
    let up = sim_to_render_normal(normal);
    let up = if up.length_squared() < 0.5 {
        Vec3::Y
    } else if up.y < 0.0 {
        -up
    } else {
        up
    };
    let max_tilt = MAX_GROUND_TILT;
    let min_up = max_tilt.cos();
    let normal = if up.y >= min_up {
        up
    } else {
        let horizontal = up.with_y(0.0).normalize_or(Vec3::Z);
        (horizontal * max_tilt.sin() + Vec3::Y * min_up).normalize()
    };
    Quat::from_rotation_arc(Vec3::Y, normal)
}

/// Convenience wrapper converting a horizontal `SimVec2` facing vector into a renderer `Quat`
/// (returns `Quat::IDENTITY` if `facing` is near-zero).
#[must_use]
pub fn facing_to_render_quat(facing: SimVec2) -> Quat {
    if facing.x.is_finite() && facing.y.is_finite() && facing.length_squared() > 1e-8 {
        heading_to_render_quat(sim_heading_from_vec2(facing))
    } else {
        Quat::IDENTITY
    }
}

/// Unnormalized outward normal `(b - a).cross(c - a)` for a CCW triangle `(a, b, c)` in renderer space.
#[must_use]
pub fn render_triangle_normal(a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    (b - a).cross(c - a)
}

/// Returns `true` when `(a, b, c)` is non-degenerate and wound CCW with respect to `expected_outward`.
#[must_use]
pub fn is_ccw_facing(a: Vec3, b: Vec3, c: Vec3, expected_outward: Vec3) -> bool {
    let normal = render_triangle_normal(a, b, c);
    normal.length_squared() > 1e-12 && normal.dot(expected_outward) > 0.0
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    #[test]
    fn point_vec_normal_roundtrip_and_axes() {
        let east_sim = SimVec3::new(1.0, 0.0, 0.0);
        let south_sim = SimVec3::new(0.0, 1.0, 0.0);
        let up_sim = SimVec3::new(0.0, 0.0, 1.0);

        assert_eq!(sim_to_render_point(east_sim), Vec3::X);
        assert_eq!(sim_to_render_point(south_sim), Vec3::Z);
        assert_eq!(sim_to_render_point(up_sim), Vec3::Y);

        assert_eq!(render_to_sim_point(Vec3::X), east_sim);
        assert_eq!(render_to_sim_point(Vec3::Z), south_sim);
        assert_eq!(render_to_sim_point(Vec3::Y), up_sim);

        let arbitrary = SimVec3::new(3.5, -7.25, 2.5);
        assert_eq!(
            render_to_sim_point(sim_to_render_point(arbitrary)),
            arbitrary
        );
        assert_eq!(render_to_sim_vec(sim_to_render_vec(arbitrary)), arbitrary);

        let n_sim = SimVec3::new(3.0, 4.0, 0.0);
        let n_render = sim_to_render_normal(n_sim);
        assert!((n_render.length() - 1.0).abs() < 1e-6);
        assert!((n_render - Vec3::new(0.6, 0.0, 0.8)).length() < 1e-6);
        let n_back = render_to_sim_normal(n_render);
        assert!((n_back - SimVec3::new(0.6, 0.8, 0.0)).length() < 1e-6);

        assert_eq!(sim_to_render_normal(SimVec3::ZERO), Vec3::ZERO);
        assert_eq!(render_to_sim_normal(Vec3::ZERO), SimVec3::ZERO);

        assert!((floor_elevation(2.5, 2) - (2.5 + 2.0 * 1.388)).abs() < 1e-6);

        let tile = Tile3D::new(4, -3, 2);
        assert_eq!(
            tile_center_render(tile, 1.0),
            Vec3::new(4.0, 1.0 + 2.0 * FLOOR_H, -3.0)
        );
    }

    #[test]
    fn tile_and_chunk_bounds_handle_negative_coordinates() {
        assert_eq!(ChunkCoord::from_tile(0, 0), ChunkCoord::new(0, 0));
        assert_eq!(ChunkCoord::from_tile(15, 15), ChunkCoord::new(0, 0));
        assert_eq!(ChunkCoord::from_tile(16, 16), ChunkCoord::new(1, 1));

        assert_eq!(ChunkCoord::from_tile(-1, -1), ChunkCoord::new(-1, -1));
        assert_eq!(tile_local_in_chunk(-1, -1), (15, 15));
        assert_eq!(ChunkCoord::from_tile(-16, -16), ChunkCoord::new(-1, -1));
        assert_eq!(tile_local_in_chunk(-16, -16), (0, 0));

        let c0 = ChunkCoord::new(0, 0);
        assert_eq!(
            c0.sim_bounds_xy(),
            (SimVec2::new(-0.5, -0.5), SimVec2::new(15.5, 15.5))
        );

        for sample in [
            SimVec2::new(-0.49, -0.49),
            SimVec2::new(0.0, 0.0),
            SimVec2::new(7.4, 12.2),
            SimVec2::new(15.49, 15.49),
        ] {
            let p = sample.tile();
            assert!((0..=15).contains(&p.x));
            assert!((0..=15).contains(&p.y));
            assert_eq!(ChunkCoord::from_sim_xy(sample), c0);
        }

        let unsorted_sim = SimAabb3::new(SimVec3::new(5.0, 6.0, 4.0), SimVec3::new(1.0, 2.0, 0.5));
        let render_box = sim_to_render_aabb(unsorted_sim);
        assert_eq!(render_box.min, Vec3::new(1.0, 0.5, 2.0));
        assert_eq!(render_box.max, Vec3::new(5.0, 4.0, 6.0));
        assert_eq!(render_box.center(), Vec3::new(3.0, 2.25, 4.0));
        assert_eq!(render_box.half_extents(), Vec3::new(2.0, 1.75, 2.0));
        assert!(render_box.contains_point(Vec3::new(3.0, 2.0, 4.0)));

        let roundtrip_sim = render_to_sim_aabb(render_box);
        assert_eq!(roundtrip_sim.min, SimVec3::new(1.0, 2.0, 0.5));
        assert_eq!(roundtrip_sim.max, SimVec3::new(5.0, 6.0, 4.0));

        assert_eq!(
            glam_vec2_to_sim(sim_vec2_to_glam(SimVec2::new(2.5, -3.5))),
            SimVec2::new(2.5, -3.5)
        );
        let tile_box = tile_render_aabb(-1, -1, 0.0, FLOOR_H);
        assert_eq!(tile_box.min, Vec3::new(-1.5, 0.0, -1.5));
        assert_eq!(tile_box.max, Vec3::new(-0.5, FLOOR_H, -0.5));
        assert!(tile_box.intersects(c0.render_aabb(0.0, FLOOR_H)));

        let c_sim = c0.sim_aabb(-1.0, 2.0);
        assert_eq!(c_sim.min, SimVec3::new(-0.5, -0.5, -1.0));
        assert_eq!(c_sim.max, SimVec3::new(15.5, 15.5, 2.0));
    }

    #[test]
    fn heading_and_ccw_winding_orientation() {
        let east = heading_to_render_quat(0.0) * Vec3::X;
        let south = heading_to_render_quat(FRAC_PI_2) * Vec3::X;
        let north = heading_to_render_quat(-FRAC_PI_2) * Vec3::X;

        assert!((east - Vec3::X).length() < 1e-6);
        assert!((south - Vec3::Z).length() < 1e-6);
        assert!((north - (-Vec3::Z)).length() < 1e-6);

        assert_eq!(facing_to_render_quat(SimVec2::ZERO), Quat::IDENTITY);
        assert!(
            (facing_to_render_quat(SimVec2::new(0.0, 2.0)) * Vec3::X - Vec3::Z).length() < 1e-6
        );

        // Standardized 00 -> 11 terrain/floor diagonal split.
        let p00 = sim_xyz_to_render(0.0, 0.0, 0.0);
        let p10 = sim_xyz_to_render(1.0, 0.0, 0.0);
        let p11 = sim_xyz_to_render(1.0, 1.0, 0.0);
        let p01 = sim_xyz_to_render(0.0, 1.0, 0.0);

        assert!(is_ccw_facing(p00, p01, p11, Vec3::Y));
        assert!(is_ccw_facing(p00, p11, p10, Vec3::Y));
        assert!(!is_ccw_facing(p00, p11, p01, Vec3::Y));
        assert!(!is_ccw_facing(p00, p00, p11, Vec3::Y));

        let delta = shortest_angle_delta(3.0, -3.0);
        assert!((delta - (TAU - 6.0)).abs() < 1e-5);
        assert!((wrap_angle_pi(-PI) - PI).abs() < 1e-6);
        assert!((wrap_angle_pi(PI) - PI).abs() < 1e-6);
    }
}
