//! Core 2D/3D spatial math primitives (`+z` up, zero external dependencies).

use crate::Position;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

/// Maps a world coordinate along X or Y to the owning tile index (`[-0.5, 0.5) -> 0`).
#[inline]
pub fn world_to_tile_coord(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[inline]
    pub const fn extend(self, z: f32) -> Vec3 {
        Vec3::new(self.x, self.y, z)
    }

    #[inline]
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    #[inline]
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    #[inline]
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len > 0.0001 {
            Self::new(self.x / len, self.y / len)
        } else {
            Self::ZERO
        }
    }

    #[inline]
    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    #[inline]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    #[inline]
    pub fn perp_dot(self, other: Self) -> f32 {
        self.x * other.y - self.y * other.x
    }

    #[inline]
    pub fn rotate(self, radians: f32) -> Self {
        let (s, c) = radians.sin_cos();
        Self::new(self.x * c - self.y * s, self.x * s + self.y * c)
    }

    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
        )
    }

    #[inline]
    pub fn clamp_length(self, max_len: f32) -> Self {
        let len_sq = self.length_squared();
        if len_sq > max_len * max_len && len_sq > 0.0 {
            self * (max_len / len_sq.sqrt())
        } else {
            self
        }
    }

    #[inline]
    pub fn tile(self) -> Position {
        Position::new(self.x.round() as i32, self.y.round() as i32)
    }
}

impl Add for Vec2 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign for Vec2 {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for Vec2 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl SubAssign for Vec2 {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;
    #[inline]
    fn mul(self, rhs: Vec2) -> Vec2 {
        Vec2::new(self * rhs.x, self * rhs.y)
    }
}

impl MulAssign<f32> for Vec2 {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;
    #[inline]
    fn div(self, rhs: f32) -> Self {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

impl Neg for Vec2 {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    pub const X: Self = Self {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    pub const Y: Self = Self {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };
    pub const Z: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };
    pub const UP: Self = Self::Z;

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub const fn splat(v: f32) -> Self {
        Self { x: v, y: v, z: v }
    }

    pub const fn from_xy(xy: Vec2, z: f32) -> Self {
        Self {
            x: xy.x,
            y: xy.y,
            z,
        }
    }

    #[inline]
    pub const fn xy(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    #[inline]
    pub const fn with_z(self, z: f32) -> Self {
        Self {
            x: self.x,
            y: self.y,
            z,
        }
    }

    #[inline]
    pub fn tile(self) -> Position {
        Position::new(world_to_tile_coord(self.x), world_to_tile_coord(self.y))
    }

    #[inline]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    #[inline]
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    #[inline]
    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    #[inline]
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    #[inline]
    pub fn horizontal_length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    #[inline]
    pub fn distance_squared(self, other: Self) -> f32 {
        (self - other).length_squared()
    }

    #[inline]
    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    #[inline]
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len > 1e-6 { self / len } else { Self::ZERO }
    }

    #[inline]
    pub fn normalized_or_up(self) -> Self {
        let len = self.length();
        if len > 1e-6 { self / len } else { Self::Z }
    }

    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
            self.z + (other.z - self.z) * t,
        )
    }

    #[inline]
    pub fn min(self, other: Self) -> Self {
        Self::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
        )
    }

    #[inline]
    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
        )
    }

    #[inline]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        self.max(min).min(max)
    }

    #[inline]
    pub fn abs(self) -> Self {
        Self::new(self.x.abs(), self.y.abs(), self.z.abs())
    }

    #[inline]
    pub fn project_onto_plane(self, normal: Self) -> Self {
        self - normal * self.dot(normal)
    }

    #[inline]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl Add for Vec3 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl AddAssign for Vec3 {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

impl Sub for Vec3 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl SubAssign for Vec3 {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
        self.z -= rhs.z;
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl Mul<Vec3> for f32 {
    type Output = Vec3;
    #[inline]
    fn mul(self, rhs: Vec3) -> Vec3 {
        Vec3::new(self * rhs.x, self * rhs.y, self * rhs.z)
    }
}

impl MulAssign<f32> for Vec3 {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.x *= rhs;
        self.y *= rhs;
        self.z *= rhs;
    }
}

impl Div<f32> for Vec3 {
    type Output = Self;
    #[inline]
    fn div(self, rhs: f32) -> Self {
        Self::new(self.x / rhs, self.y / rhs, self.z / rhs)
    }
}

impl DivAssign<f32> for Vec3 {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
        self.x /= rhs;
        self.y /= rhs;
        self.z /= rhs;
    }
}

impl Neg for Vec3 {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aabb3 {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb3 {
    pub const fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    #[inline]
    pub fn from_points(a: Vec3, b: Vec3) -> Self {
        Self::new(a.min(b), a.max(b))
    }

    #[inline]
    pub fn from_center_half_extents(center: Vec3, half: Vec3) -> Self {
        Self::new(center - half, center + half)
    }

    #[inline]
    pub fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    #[inline]
    pub fn size(self) -> Vec3 {
        self.max - self.min
    }

    #[inline]
    pub fn half_extents(self) -> Vec3 {
        self.size() * 0.5
    }

    #[inline]
    pub fn contains_point(self, p: Vec3) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }

    #[inline]
    pub fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
            && self.min.z <= other.max.z
            && self.max.z >= other.min.z
    }

    #[inline]
    pub fn union(self, other: Self) -> Self {
        Self::new(self.min.min(other.min), self.max.max(other.max))
    }

    #[inline]
    pub fn expand(self, margin: f32) -> Self {
        self.expand_vec(Vec3::splat(margin))
    }

    #[inline]
    pub fn expand_vec(self, margin: Vec3) -> Self {
        Self::new(self.min - margin, self.max + margin)
    }

    #[inline]
    pub fn translate(self, offset: Vec3) -> Self {
        Self::new(self.min + offset, self.max + offset)
    }

    #[inline]
    pub fn swept(self, delta: Vec3) -> Self {
        self.union(self.translate(delta))
    }

    #[inline]
    pub fn closest_point(self, p: Vec3) -> Vec3 {
        p.clamp(self.min, self.max)
    }

    #[inline]
    pub fn distance_squared_to_point(self, p: Vec3) -> f32 {
        (p - self.closest_point(p)).length_squared()
    }

    #[inline]
    pub fn tile_bounds(self) -> (Position, Position) {
        (
            Position::new(
                world_to_tile_coord(self.min.x),
                world_to_tile_coord(self.min.y),
            ),
            Position::new(
                world_to_tile_coord(self.max.x),
                world_to_tile_coord(self.max.y),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec3_algebra_and_projection() {
        assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z);
        assert_eq!(Vec3::Y.cross(Vec3::Z), Vec3::X);
        assert_eq!(Vec3::Z.cross(Vec3::X), Vec3::Y);
        assert_eq!(Vec3::X.dot(Vec3::Y), 0.0);
        assert_eq!(Vec3::ZERO.normalized(), Vec3::ZERO);
        assert_eq!(Vec3::ZERO.normalized_or_up(), Vec3::Z);

        let v = Vec3::new(3.0, -4.0, 5.0);
        let proj = v.project_onto_plane(Vec3::Z);
        assert_eq!(proj, Vec3::new(3.0, -4.0, 0.0));
        assert!((v.horizontal_length() - 5.0).abs() < 1e-6);
    }

    #[test]
    fn aabb3_operations_and_tile_bounds() {
        let a = Aabb3::from_center_half_extents(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.4, 0.4, 1.0));
        let b = Aabb3::new(Vec3::new(0.3, 0.3, 0.5), Vec3::new(1.2, 1.2, 2.5));
        assert!(a.intersects(b));
        assert!(a.contains_point(Vec3::new(0.0, 0.0, 1.0)));
        assert!(!a.contains_point(Vec3::new(0.5, 0.0, 1.0)));

        let swept = a.swept(Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(swept.min, Vec3::new(-0.4, -0.4, 0.0));
        assert_eq!(swept.max, Vec3::new(1.4, 0.4, 2.0));

        assert_eq!(
            a.closest_point(Vec3::new(2.0, 0.0, 1.0)),
            Vec3::new(0.4, 0.0, 1.0)
        );
        assert!((a.distance_squared_to_point(Vec3::new(1.4, 0.0, 1.0)) - 1.0).abs() < 1e-6);

        let (tmin, tmax) = swept.tile_bounds();
        assert_eq!(tmin, Position::new(0, 0));
        assert_eq!(tmax, Position::new(1, 0));
    }

    #[test]
    fn world_to_tile_coord_edges() {
        assert_eq!(world_to_tile_coord(-0.5), 0);
        assert_eq!(world_to_tile_coord(0.0), 0);
        assert_eq!(world_to_tile_coord(0.4999), 0);
        assert_eq!(world_to_tile_coord(0.5), 1);
        assert_eq!(world_to_tile_coord(-0.5001), -1);
    }
}
