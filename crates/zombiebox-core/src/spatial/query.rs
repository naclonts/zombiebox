//! Authoritative 3D spatial query contracts (`SpatialWorld`, `SupportSurface`, `Collider`, `Ray3`, `Hit3`).

use super::{Aabb3, SupportId, TerrainSample, Vec2, Vec3};
use crate::{BuildingId, Floor, Material, Position, PropKind, Surface, Tile3D};

/// Walkable or support surface returned by [`SpatialWorld::supports_at`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SupportSurface {
    pub id: SupportId,
    /// Exact surface height (`+z` up) at the queried `xy`.
    pub z: f32,
    /// Unit upward-pointing surface normal (`normal.z > 0.0`).
    pub normal: Vec3,
    /// True if standard on-foot actors may stand/walk on this surface.
    pub walkable: bool,
    /// True for `SupportId::Stair`, exempting stair ramp grade from `MoveParams::max_walkable_slope`.
    pub is_stair: bool,
    /// Semantic floor index (`0` for terrain/bridges, `tile.floor` for floors/stairs).
    pub floor: Floor,
    /// Surface category for gameplay/SFX/footsteps.
    pub surface: Surface,
    /// `Some(WATER_SURFACE_Z)` if `xy` lies over river/lake water.
    pub water_z: Option<f32>,
}

impl SupportSurface {
    /// Exact steepest-descent grade `|grad z|` (`0.0` on horizontal surfaces).
    #[inline]
    pub fn slope(self) -> f32 {
        self.normal.xy().length() / self.normal.z.max(1e-6)
    }
}

/// Semantic category of an axis-aligned 3D collider volume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColliderKind {
    /// Solid wall segment, corner pillar, window/door lintel, jamb pier, or world-border wall.
    Wall { tile: Tile3D },
    /// Closed door leaf blocking movement, rays, and sweeps until opened or destroyed.
    ClosedDoor { tile: Tile3D },
    /// Lower wall sill beneath a window (`broken == true` permits F0 vaulting when enabled).
    WindowSill { tile: Tile3D, broken: bool },
    /// Intact glass pane inside `[WINDOW_SILL_HEIGHT, WINDOW_LINTEL_Z]`.
    WindowGlass { tile: Tile3D },
    /// Furniture, crate, fence, shed, tree trunk, or streetlight pole (`prop == None` for `Streetlight`).
    Prop {
        tile: Tile3D,
        prop: Option<PropKind>,
    },
    /// Upper-floor deck slab, roof slab, or stairwell safety railing.
    BuildingSlab {
        building_id: BuildingId,
        floor: Floor,
        is_roof: bool,
    },
    /// Structural underside/volume of a `Ground::Bridge` deck.
    BridgeDeck { tile: Position },
    /// Raised guardrail along non-road water edges of a `Ground::Bridge` tile.
    BridgeParapet { tile: Position },
    /// Deep water volume (`Surface::Water`) blocking standard on-foot wading when `MoveParams::avoid_water` is true.
    WaterHazard { tile: Position },
}

/// Solid 3D collider AABB returned by [`SpatialWorld::colliders_in`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Collider {
    pub bounds: Aabb3,
    pub kind: ColliderKind,
    pub material: Material,
}

/// 3D ray in simulation coordinates (`+z` up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray3 {
    pub origin: Vec3,
    /// Normalized direction vector (`+z` up).
    pub dir: Vec3,
}

impl Ray3 {
    #[inline]
    pub fn new(origin: Vec3, dir: Vec3) -> Self {
        Self {
            origin,
            dir: dir.normalized(),
        }
    }

    #[inline]
    pub fn from_points(from: Vec3, to: Vec3) -> Option<(Self, f32)> {
        let delta = to - from;
        let dist = delta.length();
        if !dist.is_finite() || dist <= 1e-6 {
            return None;
        }
        Some((
            Self {
                origin: from,
                dir: delta / dist,
            },
            dist,
        ))
    }

    #[inline]
    pub fn at(self, t: f32) -> Vec3 {
        self.origin + self.dir * t
    }
}

/// Surface struck by a [`SpatialWorld::raycast`] or [`SpatialWorld::sphere_cast`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HitSurface {
    Terrain { tile: Position },
    BridgeDeck { tile: Position },
    Stair { tile: Tile3D },
    Collider(ColliderKind),
}

/// Closest impact result from a 3D raycast or sphere-cast query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit3 {
    pub distance: f32,
    pub point: Vec3,
    pub normal: Vec3,
    pub surface: HitSurface,
    pub material: Material,
}

/// Visibility and hazard filter flags for [`SpatialWorld`] ray and sphere casts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryFilter {
    /// Pass through `ColliderKind::WindowGlass` (used for AI line-of-sight).
    pub ignore_window_glass: bool,
    /// Test against `ColliderKind::WaterHazard` (default `false` so bullets/rockets fly over water).
    pub hit_water_hazard: bool,
}

impl QueryFilter {
    pub const DEFAULT: Self = Self {
        ignore_window_glass: false,
        hit_water_hazard: false,
    };

    pub const LINE_OF_SIGHT: Self = Self {
        ignore_window_glass: true,
        hit_water_hazard: false,
    };
}

impl Default for QueryFilter {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Rectangular `[x0, x1] × [y0, y1]` surface patch with corner heights `[z00, z10, z01, z11]`
/// split along the canonical `00 -> 11` diagonal for exact `raycast` and `sphere_cast` queries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceQuad {
    pub surface: HitSurface,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub corners: [f32; 4],
    pub material: Material,
}

/// Unified 3D spatial query interface shared by kinematics, AI, weapons, and camera collision.
pub trait SpatialWorld {
    fn terrain_at(&self, xy: Vec2) -> Option<TerrainSample>;
    fn supports_at(&self, xy: Vec2, out: &mut Vec<SupportSurface>);
    fn colliders_in(&self, bounds: Aabb3, out: &mut Vec<Collider>);

    /// Emits `00 -> 11` triangulated surface quads (terrain, bridge decks, stair ramps) in `[min_tile, max_tile]`.
    /// Default implementation reconstructs terrain quads by sampling `terrain_at` at tile corners.
    fn surface_quads_in(&self, min_tile: Position, max_tile: Position, out: &mut Vec<SurfaceQuad>) {
        out.clear();
        for ty in min_tile.y..=max_tile.y {
            for tx in min_tile.x..=max_tile.x {
                let x0 = tx as f32 - 0.5;
                let y0 = ty as f32 - 0.5;
                let x1 = tx as f32 + 0.5;
                let y1 = ty as f32 + 0.5;
                // Sample slightly inside each corner (`1e-5`) so default synthetic worlds resolve owning tile cleanly.
                let (Some(s00), Some(s10), Some(s01), Some(s11)) = (
                    self.terrain_at(Vec2::new(x0 + 1e-5, y0 + 1e-5)),
                    self.terrain_at(Vec2::new(x1 - 1e-5, y0 + 1e-5)),
                    self.terrain_at(Vec2::new(x0 + 1e-5, y1 - 1e-5)),
                    self.terrain_at(Vec2::new(x1 - 1e-5, y1 - 1e-5)),
                ) else {
                    continue;
                };
                out.push(SurfaceQuad {
                    surface: HitSurface::Terrain {
                        tile: Position::new(tx, ty),
                    },
                    x0,
                    y0,
                    x1,
                    y1,
                    corners: [s00.z, s10.z, s01.z, s11.z],
                    material: Material::None,
                });
            }
        }
    }

    fn raycast(&self, ray: Ray3, max_distance: f32) -> Option<Hit3> {
        self.raycast_filtered(ray, max_distance, QueryFilter::DEFAULT)
    }

    fn sphere_cast(&self, from: Vec3, to: Vec3, radius: f32) -> Option<Hit3> {
        self.sphere_cast_filtered(from, to, radius, QueryFilter::DEFAULT)
    }

    fn raycast_filtered(&self, ray: Ray3, max_distance: f32, filter: QueryFilter) -> Option<Hit3> {
        super::collision::raycast_world(self, ray, max_distance, filter)
    }

    fn sphere_cast_filtered(
        &self,
        from: Vec3,
        to: Vec3,
        radius: f32,
        filter: QueryFilter,
    ) -> Option<Hit3> {
        super::collision::sphere_cast_world(self, from, to, radius, filter)
    }
}
