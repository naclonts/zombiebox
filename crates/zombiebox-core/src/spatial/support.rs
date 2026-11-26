//! Stair ramp geometry (`3 × 3` patch) and anti-snap support surface selection (`+z` up).

use super::query::{HitSurface, SupportSurface, SurfaceQuad};
use super::{FLOOR_H, SupportId, Vec2, Vec3, sample_quad_00_11};
use crate::{Material, Surface, Tile3D};

/// Unified `3 × 3` vertex height patch representing straight or L-turn winder stairs
/// from `z_low` (`tile.floor`) to `z_low + FLOOR_H` (`tile.floor + 1`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StairGeometry {
    pub tile: Tile3D,
    /// Unit cardinal direction `(dx, dy)` toward the lower-floor (`tile.floor`) entry tile.
    pub low_dir: (i8, i8),
    /// Unit cardinal direction `(dx, dy)` toward the upper-floor (`tile.floor + 1`) exit tile.
    pub high_dir: (i8, i8),
    /// `3 × 3` vertex elevations `[iv][iu]` at `ox = -0.5 + 0.5 * iu`, `oy = -0.5 + 0.5 * iv`.
    pub grid: [[f32; 3]; 3],
}

impl StairGeometry {
    /// Builds a `3 × 3` stair patch from `z_low` to `z_low + FLOOR_H`.
    pub fn new(tile: Tile3D, z_low: f32, low_dir: (i8, i8), high_dir: (i8, i8)) -> Self {
        let low_dir = if low_dir == (0, 0) { (-1, 0) } else { low_dir };
        let high_dir = if high_dir == (0, 0) || high_dir == low_dir {
            (-low_dir.0, -low_dir.1)
        } else {
            high_dir
        };

        let is_straight = high_dir == (-low_dir.0, -low_dir.1);
        let mut grid = [[z_low; 3]; 3];

        // Normalized elevation fractions for a 90-degree L-turn winder stair.
        // `ia` progresses away from `low_dir` (`0` at low entry border -> `2` opposite).
        // `ib` progresses toward `high_dir` (`0` opposite exit -> `2` at high exit border).
        const WINDER_FRAC: [[f32; 3]; 3] = [
            [0.00, 0.25, 0.50], // ib = 0 (opposite high_dir)
            [0.00, 0.50, 0.75], // ib = 1 (centerline)
            [0.50, 1.00, 1.00], // ib = 2 (high_dir exit border)
        ];

        for (iv, row) in grid.iter_mut().enumerate() {
            let oy = -0.5 + 0.5 * iv as f32;
            for (iu, cell) in row.iter_mut().enumerate() {
                let ox = -0.5 + 0.5 * iu as f32;
                let frac = if is_straight {
                    (ox * high_dir.0 as f32 + oy * high_dir.1 as f32) + 0.5
                } else {
                    let a = -(ox * low_dir.0 as f32 + oy * low_dir.1 as f32);
                    let b = ox * high_dir.0 as f32 + oy * high_dir.1 as f32;
                    let ia = ((a + 0.5) * 2.0).round().clamp(0.0, 2.0) as usize;
                    let ib = ((b + 0.5) * 2.0).round().clamp(0.0, 2.0) as usize;
                    WINDER_FRAC[ib][ia]
                };
                *cell = z_low + frac * FLOOR_H;
            }
        }

        Self {
            tile,
            low_dir,
            high_dir,
            grid,
        }
    }

    /// True if `high_dir` is directly opposite `low_dir` (planar ramp).
    #[inline]
    pub const fn is_straight(&self) -> bool {
        self.high_dir.0 == -self.low_dir.0 && self.high_dir.1 == -self.low_dir.1
    }

    /// Samples the exact `(z, unit_normal)` of the `00 -> 11` stair surface at `xy`.
    pub fn sample(&self, xy: Vec2) -> (f32, Vec3) {
        let lx = ((xy.x - self.tile.x as f32) + 0.5).clamp(0.0, 1.0) * 2.0;
        let ly = ((xy.y - self.tile.y as f32) + 0.5).clamp(0.0, 1.0) * 2.0;
        let qx = (lx.floor() as usize).min(1);
        let qy = (ly.floor() as usize).min(1);
        let u = (lx - qx as f32).clamp(0.0, 1.0);
        let v = (ly - qy as f32).clamp(0.0, 1.0);

        let (z, raw_n, _) = sample_quad_00_11(
            self.grid[qy][qx],
            self.grid[qy][qx + 1],
            self.grid[qy + 1][qx],
            self.grid[qy + 1][qx + 1],
            u,
            v,
        );
        // Each sub-quad is 0.5 × 0.5 units, so horizontal slopes are 2x sub-quad UV derivatives.
        let normal = Vec3::new(raw_n.x * 2.0, raw_n.y * 2.0, raw_n.z).normalized_or_up();
        (z, normal)
    }

    /// Appends canonical `00 -> 11` [`SurfaceQuad`]s matching [`Self::sample`] to 0 ULP.
    pub fn push_quads(&self, material: Material, out: &mut Vec<SurfaceQuad>) {
        let surface = HitSurface::Stair { tile: self.tile };
        if self.is_straight() {
            let x0 = self.tile.x as f32 - 0.5;
            let y0 = self.tile.y as f32 - 0.5;
            out.push(SurfaceQuad {
                surface,
                x0,
                y0,
                x1: x0 + 1.0,
                y1: y0 + 1.0,
                corners: [
                    self.grid[0][0],
                    self.grid[0][2],
                    self.grid[2][0],
                    self.grid[2][2],
                ],
                material,
            });
            return;
        }
        for qy in 0..2 {
            let y0 = self.tile.y as f32 - 0.5 + 0.5 * qy as f32;
            for qx in 0..2 {
                let x0 = self.tile.x as f32 - 0.5 + 0.5 * qx as f32;
                out.push(SurfaceQuad {
                    surface,
                    x0,
                    y0,
                    x1: x0 + 0.5,
                    y1: y0 + 0.5,
                    corners: [
                        self.grid[qy][qx],
                        self.grid[qy][qx + 1],
                        self.grid[qy + 1][qx],
                        self.grid[qy + 1][qx + 1],
                    ],
                    material,
                });
            }
        }
    }
}

/// Result of sweeping a falling body vertically against candidate [`SupportSurface`]s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirborneSupportHit {
    pub surface: SupportSurface,
    /// True if the body can stand grounded on this surface; false if it is an unwalkable steep slope to slide down.
    pub can_land: bool,
}

#[inline]
pub(crate) fn is_surface_standable(
    s: &SupportSurface,
    max_walkable_slope: f32,
    avoid_water: bool,
) -> bool {
    let water_ok =
        s.walkable || (!avoid_water && s.id == SupportId::Terrain && s.water_z.is_some());
    s.surface != Surface::Mountain
        && water_ok
        && (s.is_stair || s.slope() <= max_walkable_slope + 1e-4)
}

/// Selects the best grounded support within `[foot_z - max_step_down, foot_z + max_step_up]`,
/// preventing false upward snaps onto overhead bridge decks or upper-floor slabs.
pub fn select_grounded_support(
    candidates: &[SupportSurface],
    foot_z: f32,
    prev_support: Option<SupportId>,
    max_step_up: f32,
    max_step_down: f32,
    max_walkable_slope: f32,
    avoid_water: bool,
) -> Option<SupportSurface> {
    let mut best: Option<(f32, SupportSurface)> = None;

    for &s in candidates {
        if !is_surface_standable(&s, max_walkable_slope, avoid_water) {
            continue;
        }
        let dz = s.z - foot_z;
        if dz < -max_step_down - 1e-4 || dz > max_step_up + 1e-4 {
            continue;
        }
        let mut effective_dist = dz.abs();
        if Some(s.id) == prev_support {
            effective_dist -= 1e-3;
        }
        if s.is_stair {
            effective_dist -= 5e-4;
        }
        if best.is_none_or(|(best_dist, _)| effective_dist < best_dist) {
            best = Some((effective_dist, s));
        }
    }

    best.map(|(_, s)| s)
}

/// Sweeps a downward vertical motion `[z_from, z_to]` against candidate supports,
/// returning the highest encountered surface in `[z_to, z_from]`.
pub fn sweep_airborne_support(
    candidates: &[SupportSurface],
    z_from: f32,
    z_to: f32,
    max_walkable_slope: f32,
    avoid_water: bool,
) -> Option<AirborneSupportHit> {
    if z_to > z_from {
        return None;
    }

    let mut best: Option<AirborneSupportHit> = None;
    for &s in candidates {
        if s.z > z_from + 0.04 || s.z < z_to - 1e-4 {
            continue;
        }
        let can_land = is_surface_standable(&s, max_walkable_slope, avoid_water);
        let replace = match best {
            None => true,
            Some(prev) => {
                if s.z > prev.surface.z + 1e-5 {
                    true
                } else if (s.z - prev.surface.z).abs() <= 1e-5 {
                    (can_land && !prev.can_land)
                        || (can_land == prev.can_land && s.is_stair && !prev.surface.is_stair)
                } else {
                    false
                }
            }
        };
        if replace {
            best = Some(AirborneSupportHit {
                surface: s,
                can_land,
            });
        }
    }
    best
}
