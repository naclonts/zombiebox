//! Continuous 513x513 terrain height grid, bridge deck quads, and canonical `00 -> 11` triangle math.

use super::Vec3;
use crate::{CHUNK_SIZE, Position, WORLD_SIZE};
use std::collections::HashMap;

pub const TERRAIN_GRID_SIDE: usize = (WORLD_SIZE + 1) as usize; // 513
pub const CHUNK_VERTICES: usize = (CHUNK_SIZE + 1) as usize; // 17

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerrainTriangle {
    /// `u >= v`, vertices `(00, 10, 11)`.
    /// Note for `T04`: in renderer space `(X, Y, Z) = (x, z, y)`, CCW upward winding is `(00, 11, 10)`.
    LowerRight,
    /// `u < v`, vertices `(00, 11, 01)`.
    /// Note for `T04`: in renderer space `(X, Y, Z) = (x, z, y)`, CCW upward winding is `(00, 01, 11)`.
    UpperLeft,
}

#[inline]
pub fn sample_quad_00_11(
    z00: f32,
    z10: f32,
    z01: f32,
    z11: f32,
    u: f32,
    v: f32,
) -> (f32, Vec3, TerrainTriangle) {
    let u = u.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let tri = if u >= v {
        TerrainTriangle::LowerRight
    } else {
        TerrainTriangle::UpperLeft
    };
    // Exact zero-ULP fast path for flat worlds (Birch Street) and flat building pads.
    if z00 == z10 && z10 == z01 && z01 == z11 {
        return (z00, Vec3::Z, tri);
    }
    match tri {
        TerrainTriangle::LowerRight => {
            // Barycentric weights on (00, 10, 11): w00 = 1 - u, w10 = u - v, w11 = v.
            // Boundary-exact in IEEE-754 f32 along v = 0, u = 1, and u = v.
            let z = (1.0 - u) * z00 + (u - v) * z10 + v * z11;
            let dz_du = z10 - z00;
            let dz_dv = z11 - z10;
            let normal = Vec3::new(-dz_du, -dz_dv, 1.0).normalized_or_up();
            (z, normal, tri)
        }
        TerrainTriangle::UpperLeft => {
            // Barycentric weights on (00, 11, 01): w00 = 1 - v, w01 = v - u, w11 = u.
            // Boundary-exact in IEEE-754 f32 along u = 0, v = 1, and u = v.
            let z = (1.0 - v) * z00 + (v - u) * z01 + u * z11;
            let dz_du = z11 - z01;
            let dz_dv = z01 - z00;
            let normal = Vec3::new(-dz_du, -dz_dv, 1.0).normalized_or_up();
            (z, normal, tri)
        }
    }
}

#[derive(Debug, Clone)]
pub struct TerrainGrid {
    /// Row-major `513 x 513` vertex elevations (`gy * 513 + gx`).
    heights: Box<[f32]>,
    /// Elevated bridge deck corner quads `[z00, z10, z01, z11]` for `Ground::Bridge` tiles.
    bridges: HashMap<Position, [f32; 4]>,
}

impl TerrainGrid {
    pub(crate) fn new(heights: Box<[f32]>, bridges: HashMap<Position, [f32; 4]>) -> Self {
        debug_assert_eq!(heights.len(), TERRAIN_GRID_SIDE * TERRAIN_GRID_SIDE);
        Self { heights, bridges }
    }

    #[inline]
    fn height_unchecked(&self, gx: i32, gy: i32) -> f32 {
        self.heights[gy as usize * TERRAIN_GRID_SIDE + gx as usize]
    }

    #[inline]
    pub fn grid_height(&self, gx: i32, gy: i32) -> Option<f32> {
        ((0..=WORLD_SIZE).contains(&gx) && (0..=WORLD_SIZE).contains(&gy))
            .then(|| self.height_unchecked(gx, gy))
    }

    pub fn grid_normal(&self, gx: i32, gy: i32) -> Option<Vec3> {
        if !(0..=WORLD_SIZE).contains(&gx) || !(0..=WORLD_SIZE).contains(&gy) {
            return None;
        }
        let x0 = (gx - 1).max(0);
        let x1 = (gx + 1).min(WORLD_SIZE);
        let y0 = (gy - 1).max(0);
        let y1 = (gy + 1).min(WORLD_SIZE);
        let dz_dx =
            (self.height_unchecked(x1, gy) - self.height_unchecked(x0, gy)) / ((x1 - x0) as f32);
        let dz_dy =
            (self.height_unchecked(gx, y1) - self.height_unchecked(gx, y0)) / ((y1 - y0) as f32);
        Some(Vec3::new(-dz_dx, -dz_dy, 1.0).normalized_or_up())
    }

    #[inline]
    pub fn quad(&self, p: Position) -> Option<[f32; 4]> {
        if !(0..WORLD_SIZE).contains(&p.x) || !(0..WORLD_SIZE).contains(&p.y) {
            return None;
        }
        let z00 = self.height_unchecked(p.x, p.y);
        let z10 = self.height_unchecked(p.x + 1, p.y);
        let z01 = self.height_unchecked(p.x, p.y + 1);
        let z11 = self.height_unchecked(p.x + 1, p.y + 1);
        Some([z00, z10, z01, z11])
    }

    #[inline]
    pub fn bridge_quad(&self, p: Position) -> Option<[f32; 4]> {
        self.bridges.get(&p).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_quad_00_11_corners_diagonal_and_normals() {
        let (z00, z10, z01, z11) = (1.2, 2.7, -0.4, 3.1);

        let (c00, _, t00) = sample_quad_00_11(z00, z10, z01, z11, 0.0, 0.0);
        assert_eq!(c00, z00);
        assert_eq!(t00, TerrainTriangle::LowerRight);

        let (c10, _, t10) = sample_quad_00_11(z00, z10, z01, z11, 1.0, 0.0);
        assert_eq!(c10, z10);
        assert_eq!(t10, TerrainTriangle::LowerRight);

        let (c01, _, t01) = sample_quad_00_11(z00, z10, z01, z11, 0.0, 1.0);
        assert_eq!(c01, z01);
        assert_eq!(t01, TerrainTriangle::UpperLeft);

        let (c11, _, t11) = sample_quad_00_11(z00, z10, z01, z11, 1.0, 1.0);
        assert_eq!(c11, z11);
        assert_eq!(t11, TerrainTriangle::LowerRight);

        // Diagonal invariance regardless of z10/z01 and continuity across u == v.
        for t in [0.1, 0.35, 0.5, 0.85] {
            let (zd, _, _) = sample_quad_00_11(z00, z10, z01, z11, t, t);
            let expected = (1.0 - t) * z00 + t * z11;
            assert!((zd - expected).abs() < 1e-6);

            let (za, _, ta) = sample_quad_00_11(z00, z10, z01, z11, t + 1e-6, t);
            let (zb, _, tb) = sample_quad_00_11(z00, z10, z01, z11, t, t + 1e-6);
            assert_eq!(ta, TerrainTriangle::LowerRight);
            assert_eq!(tb, TerrainTriangle::UpperLeft);
            assert!((za - zb).abs() < 1e-4);
        }

        // Analytical normal vs finite differences inside both triangles.
        let eps = 1e-3;
        for (u, v, expected_tri) in [
            (0.7, 0.25, TerrainTriangle::LowerRight),
            (0.25, 0.7, TerrainTriangle::UpperLeft),
        ] {
            let (_, n, tri) = sample_quad_00_11(z00, z10, z01, z11, u, v);
            assert_eq!(tri, expected_tri);
            assert!((n.length() - 1.0).abs() < 1e-6);
            assert!(n.z > 0.0);
            let dz_du = (sample_quad_00_11(z00, z10, z01, z11, u + eps, v).0
                - sample_quad_00_11(z00, z10, z01, z11, u - eps, v).0)
                / (2.0 * eps);
            let dz_dv = (sample_quad_00_11(z00, z10, z01, z11, u, v + eps).0
                - sample_quad_00_11(z00, z10, z01, z11, u, v - eps).0)
                / (2.0 * eps);
            assert!((-n.x / n.z - dz_du).abs() < 1e-4);
            assert!((-n.y / n.z - dz_dv).abs() < 1e-4);
        }
    }
}
