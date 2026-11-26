//! Low-poly primitive geometry builders with exact CCW winding contracts (`T03`).

use glam::{Mat4, Quat, Vec3};
use std::f32::consts::{FRAC_PI_2, TAU};

use super::{MeshBuilder, VertexMeta, WorldVertex};
use crate::coords::{SimAabb3, SimVec3, sim_to_render_aabb, sim_to_render_point};

/// Bitmask selecting which faces of an axis-aligned box/slab to emit.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FaceMask(pub u8);

impl FaceMask {
    pub const NONE: Self = Self(0);
    pub const POS_X: Self = Self(1 << 0); // East (+X, sim +x)
    pub const NEG_X: Self = Self(1 << 1); // West (-X, sim -x)
    pub const POS_Y: Self = Self(1 << 2); // Top (+Y, sim +z)
    pub const NEG_Y: Self = Self(1 << 3); // Bot (-Y, sim -z)
    pub const POS_Z: Self = Self(1 << 4); // South (+Z, sim +y)
    pub const NEG_Z: Self = Self(1 << 5); // North (-Z, sim -y)

    pub const EAST: Self = Self::POS_X;
    pub const WEST: Self = Self::NEG_X;
    pub const TOP: Self = Self::POS_Y;
    pub const BOTTOM: Self = Self::NEG_Y;
    pub const SOUTH: Self = Self::POS_Z;
    pub const NORTH: Self = Self::NEG_Z;

    pub const SIDES: Self = Self(Self::POS_X.0 | Self::NEG_X.0 | Self::POS_Z.0 | Self::NEG_Z.0);
    pub const TOP_AND_SIDES: Self = Self(Self::POS_Y.0 | Self::SIDES.0);
    pub const ALL: Self = Self(0b0011_1111);

    #[must_use]
    pub const fn contains(self, f: Self) -> bool {
        (self.0 & f.0) == f.0
    }

    #[must_use]
    pub const fn union(self, f: Self) -> Self {
        Self(self.0 | f.0)
    }

    #[must_use]
    pub const fn without(self, f: Self) -> Self {
        Self(self.0 & !f.0)
    }
}

/// Cardinal horizontal direction in which a `ramp` climbs from `min.y` (low) to `max.y` (high).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CardinalDir {
    EastPosX,  // +X (sim +x)
    WestNegX,  // -X (sim -x)
    SouthPosZ, // +Z (sim +y)
    NorthNegZ, // -Z (sim -y)
}

/// Horizontal ridge axis for a pitched `gable_roof`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RidgeAxis {
    AlongX, // Ridge runs East-West (+X/-X), slopes face North (-Z) and South (+Z)
    AlongZ, // Ridge runs North-South (+Z/-Z), slopes face East (+X) and West (-X)
}

// --- Primitive Emission Functions ---

pub fn emit_quad(
    b: &mut MeshBuilder,
    p0: Vec3,
    p1: Vec3,
    p2: Vec3,
    p3: Vec3,
    color: [f32; 4],
    meta: VertexMeta,
) {
    b.push_quad(p0, p1, p2, p3, color, meta);
}

/// Splits cell corners `p00` (min_x, min_z), `p10` (max_x, min_z), `p11` (max_x, max_z), `p01` (min_x, max_z)
/// along the `00 -> 11` diagonal into flat-shaded CCW `+Y` triangles: `(p00, p01, p11)` and `(p00, p11, p10)`.
pub fn emit_grid_quad_00_11_flat(
    b: &mut MeshBuilder,
    p00: Vec3,
    p10: Vec3,
    p11: Vec3,
    p01: Vec3,
    color: [f32; 4],
    meta: VertexMeta,
) {
    let cross = (p01 - p00).cross(p11 - p00);
    let normal = if cross.length_squared() > 1e-12 {
        cross.normalize()
    } else {
        Vec3::Y
    };
    b.push_triangle(p00, p01, p11, color, meta);
    b.push_triangle(p00, p11, p10, color, meta);
    // Fix vertex normals for exact flat shading consistency
    let v_len = b.as_mesh().vertices.len();
    for v in &mut b.as_mesh_mut().vertices[v_len - 6..v_len] {
        v.normal = normal.to_array();
    }
}

/// Smooth-shaded grid quad from 4 pre-computed `WorldVertex`es `[v00, v10, v11, v01]`
/// emitting CCW triangles `(00, 01, 11)` and `(00, 11, 10)`.
pub fn emit_grid_quad_00_11_smooth(
    b: &mut MeshBuilder,
    v00: WorldVertex,
    v10: WorldVertex,
    v11: WorldVertex,
    v01: WorldVertex,
) {
    let base = b.push_vertex(v00);
    let i10 = b.push_vertex(v10);
    let i11 = b.push_vertex(v11);
    let i01 = b.push_vertex(v01);
    b.as_mesh_mut()
        .indices
        .extend_from_slice(&[base, i01, i11, base, i11, i10]);
}

pub fn emit_sim_grid_quad_00_11_flat(
    b: &mut MeshBuilder,
    sim_p00: SimVec3,
    sim_p10: SimVec3,
    sim_p11: SimVec3,
    sim_p01: SimVec3,
    color: [f32; 4],
    meta: VertexMeta,
) {
    emit_grid_quad_00_11_flat(
        b,
        sim_to_render_point(sim_p00),
        sim_to_render_point(sim_p10),
        sim_to_render_point(sim_p11),
        sim_to_render_point(sim_p01),
        color,
        meta,
    );
}

/// Emits an axis-aligned box/cuboid defined by `min` and `max` in renderer space for enabled `faces`.
pub fn emit_box_min_max(
    b: &mut MeshBuilder,
    min: Vec3,
    max: Vec3,
    faces: FaceMask,
    color: [f32; 4],
    meta: VertexMeta,
) {
    let lo = min.min(max);
    let hi = min.max(max);
    let (x0, y0, z0) = (lo.x, lo.y, lo.z);
    let (x1, y1, z1) = (hi.x, hi.y, hi.z);

    if faces.contains(FaceMask::POS_X) && (x1 - x0) > 1e-6 && (y1 - y0) > 1e-6 && (z1 - z0) > 1e-6 {
        b.push_quad_with_normal(
            Vec3::new(x1, y0, z1),
            Vec3::new(x1, y0, z0),
            Vec3::new(x1, y1, z0),
            Vec3::new(x1, y1, z1),
            Vec3::X,
            color,
            meta,
        );
    }
    if faces.contains(FaceMask::NEG_X) && (x1 - x0) > 1e-6 && (y1 - y0) > 1e-6 && (z1 - z0) > 1e-6 {
        b.push_quad_with_normal(
            Vec3::new(x0, y0, z0),
            Vec3::new(x0, y0, z1),
            Vec3::new(x0, y1, z1),
            Vec3::new(x0, y1, z0),
            -Vec3::X,
            color,
            meta,
        );
    }
    if faces.contains(FaceMask::POS_Y) && (x1 - x0) > 1e-6 && (y1 - y0) > 1e-6 && (z1 - z0) > 1e-6 {
        b.push_quad_with_normal(
            Vec3::new(x0, y1, z1),
            Vec3::new(x1, y1, z1),
            Vec3::new(x1, y1, z0),
            Vec3::new(x0, y1, z0),
            Vec3::Y,
            color,
            meta,
        );
    }
    if faces.contains(FaceMask::NEG_Y) && (x1 - x0) > 1e-6 && (y1 - y0) > 1e-6 && (z1 - z0) > 1e-6 {
        b.push_quad_with_normal(
            Vec3::new(x0, y0, z0),
            Vec3::new(x1, y0, z0),
            Vec3::new(x1, y0, z1),
            Vec3::new(x0, y0, z1),
            -Vec3::Y,
            color,
            meta,
        );
    }
    if faces.contains(FaceMask::POS_Z) && (x1 - x0) > 1e-6 && (y1 - y0) > 1e-6 && (z1 - z0) > 1e-6 {
        b.push_quad_with_normal(
            Vec3::new(x0, y0, z1),
            Vec3::new(x1, y0, z1),
            Vec3::new(x1, y1, z1),
            Vec3::new(x0, y1, z1),
            Vec3::Z,
            color,
            meta,
        );
    }
    if faces.contains(FaceMask::NEG_Z) && (x1 - x0) > 1e-6 && (y1 - y0) > 1e-6 && (z1 - z0) > 1e-6 {
        b.push_quad_with_normal(
            Vec3::new(x1, y0, z0),
            Vec3::new(x0, y0, z0),
            Vec3::new(x0, y1, z0),
            Vec3::new(x1, y1, z0),
            -Vec3::Z,
            color,
            meta,
        );
    }
}

pub fn emit_cuboid(
    b: &mut MeshBuilder,
    center: Vec3,
    half_extents: Vec3,
    faces: FaceMask,
    color: [f32; 4],
    meta: VertexMeta,
) {
    let min = center - half_extents;
    let max = center + half_extents;
    emit_box_min_max(b, min, max, faces, color, meta);
}

pub fn emit_sim_box_min_max(
    b: &mut MeshBuilder,
    sim_min: SimVec3,
    sim_max: SimVec3,
    faces: FaceMask,
    color: [f32; 4],
    meta: VertexMeta,
) {
    let render_aabb = sim_to_render_aabb(SimAabb3::new(sim_min, sim_max));
    emit_box_min_max(b, render_aabb.min, render_aabb.max, faces, color, meta);
}

/// Emits a cylinder or truncated cone / prism.
#[allow(clippy::too_many_arguments)]
pub fn emit_cylinder(
    b: &mut MeshBuilder,
    base_center: Vec3,
    radius_bottom: f32,
    radius_top: f32,
    height: f32,
    radial_segments: usize,
    include_bottom_cap: bool,
    include_top_cap: bool,
    color: [f32; 4],
    meta: VertexMeta,
) {
    if height <= 1e-6 || (radius_bottom <= 1e-6 && radius_top <= 1e-6) {
        return;
    }
    let n = radial_segments.max(3);
    let top_center = base_center + Vec3::Y * height;

    let mut angles = Vec::with_capacity(n);
    let mut u_vecs = Vec::with_capacity(n);
    for i in 0..n {
        let theta = TAU * (i as f32) / (n as f32);
        angles.push(theta);
        u_vecs.push(Vec3::new(theta.cos(), 0.0, -theta.sin()));
    }

    for i in 0..n {
        let next = (i + 1) % n;
        let u0 = u_vecs[i];
        let u1 = u_vecs[next];

        let b0 = base_center + u0 * radius_bottom;
        let b1 = base_center + u1 * radius_bottom;
        let t0 = top_center + u0 * radius_top;
        let t1 = top_center + u1 * radius_top;

        if radius_top <= 1e-6 {
            b.push_triangle(b0, b1, top_center, color, meta);
        } else if radius_bottom <= 1e-6 {
            b.push_triangle(b0, t1, t0, color, meta);
        } else {
            b.push_quad(b0, b1, t1, t0, color, meta);
        }
    }

    if include_top_cap && radius_top > 1e-6 {
        for i in 0..n {
            let next = (i + 1) % n;
            let t0 = top_center + u_vecs[i] * radius_top;
            let t1 = top_center + u_vecs[next] * radius_top;
            b.push_triangle(top_center, t0, t1, color, meta);
        }
    }

    if include_bottom_cap && radius_bottom > 1e-6 {
        for i in 0..n {
            let next = (i + 1) % n;
            let b0 = base_center + u_vecs[i] * radius_bottom;
            let b1 = base_center + u_vecs[next] * radius_bottom;
            b.push_triangle(base_center, b1, b0, color, meta);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn emit_sim_cylinder(
    b: &mut MeshBuilder,
    sim_base_center: SimVec3,
    radius_bottom: f32,
    radius_top: f32,
    height: f32,
    radial_segments: usize,
    include_bottom_cap: bool,
    include_top_cap: bool,
    color: [f32; 4],
    meta: VertexMeta,
) {
    emit_cylinder(
        b,
        sim_to_render_point(sim_base_center),
        radius_bottom,
        radius_top,
        height,
        radial_segments,
        include_bottom_cap,
        include_top_cap,
        color,
        meta,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn emit_cone(
    b: &mut MeshBuilder,
    base_center: Vec3,
    radius: f32,
    height: f32,
    radial_segments: usize,
    include_bottom_cap: bool,
    color: [f32; 4],
    meta: VertexMeta,
) {
    emit_cylinder(
        b,
        base_center,
        radius,
        0.0,
        height,
        radial_segments,
        include_bottom_cap,
        false,
        color,
        meta,
    );
}

pub fn emit_pyramid(
    b: &mut MeshBuilder,
    base_center: Vec3,
    radius: f32,
    height: f32,
    include_bottom_cap: bool,
    color: [f32; 4],
    meta: VertexMeta,
) {
    emit_cone(
        b,
        base_center,
        radius,
        height,
        4,
        include_bottom_cap,
        color,
        meta,
    );
}

pub fn emit_gable_roof(
    b: &mut MeshBuilder,
    min: Vec3,
    max: Vec3,
    axis: RidgeAxis,
    include_bottom: bool,
    color: [f32; 4],
    meta: VertexMeta,
) {
    let lo = min.min(max);
    let hi = min.max(max);
    let (x0, y0, z0) = (lo.x, lo.y, lo.z);
    let (x1, y1, z1) = (hi.x, hi.y, hi.z);

    match axis {
        RidgeAxis::AlongX => {
            let z_mid = 0.5 * (z0 + z1);
            let r_w = Vec3::new(x0, y1, z_mid);
            let r_e = Vec3::new(x1, y1, z_mid);
            // South slope (+Z)
            b.push_quad(
                Vec3::new(x0, y0, z1),
                Vec3::new(x1, y0, z1),
                r_e,
                r_w,
                color,
                meta,
            );
            // North slope (-Z)
            b.push_quad(
                Vec3::new(x1, y0, z0),
                Vec3::new(x0, y0, z0),
                r_w,
                r_e,
                color,
                meta,
            );
            // East gable (+X)
            b.push_triangle(
                Vec3::new(x1, y0, z1),
                Vec3::new(x1, y0, z0),
                r_e,
                color,
                meta,
            );
            // West gable (-X)
            b.push_triangle(
                Vec3::new(x0, y0, z0),
                Vec3::new(x0, y0, z1),
                r_w,
                color,
                meta,
            );
            if include_bottom {
                b.push_quad(
                    Vec3::new(x0, y0, z0),
                    Vec3::new(x1, y0, z0),
                    Vec3::new(x1, y0, z1),
                    Vec3::new(x0, y0, z1),
                    color,
                    meta,
                );
            }
        }
        RidgeAxis::AlongZ => {
            let x_mid = 0.5 * (x0 + x1);
            let r_n = Vec3::new(x_mid, y1, z0);
            let r_s = Vec3::new(x_mid, y1, z1);
            // East slope (+X)
            b.push_quad(
                Vec3::new(x1, y0, z1),
                Vec3::new(x1, y0, z0),
                r_n,
                r_s,
                color,
                meta,
            );
            // West slope (-X)
            b.push_quad(
                Vec3::new(x0, y0, z0),
                Vec3::new(x0, y0, z1),
                r_s,
                r_n,
                color,
                meta,
            );
            // South gable (+Z)
            b.push_triangle(
                Vec3::new(x0, y0, z1),
                Vec3::new(x1, y0, z1),
                r_s,
                color,
                meta,
            );
            // North gable (-Z)
            b.push_triangle(
                Vec3::new(x1, y0, z0),
                Vec3::new(x0, y0, z0),
                r_n,
                color,
                meta,
            );
            if include_bottom {
                b.push_quad(
                    Vec3::new(x0, y0, z0),
                    Vec3::new(x1, y0, z0),
                    Vec3::new(x1, y0, z1),
                    Vec3::new(x0, y0, z1),
                    color,
                    meta,
                );
            }
        }
    }
}

pub fn emit_ramp(
    b: &mut MeshBuilder,
    min: Vec3,
    max: Vec3,
    climb_dir: CardinalDir,
    include_bottom: bool,
    color: [f32; 4],
    meta: VertexMeta,
) {
    let lo = min.min(max);
    let hi = min.max(max);
    let dx = 0.5 * (hi.x - lo.x);
    let dz = 0.5 * (hi.z - lo.z);
    let cx = 0.5 * (lo.x + hi.x);
    let cz = 0.5 * (lo.z + hi.z);
    let y0 = lo.y;
    let y1 = hi.y;

    let (hl, hw, rot_y) = match climb_dir {
        CardinalDir::EastPosX => (dx, dz, 0.0),
        CardinalDir::NorthNegZ => (dz, dx, FRAC_PI_2),
        CardinalDir::WestNegX => (dx, dz, TAU * 0.5),
        CardinalDir::SouthPosZ => (dz, dx, -FRAC_PI_2),
    };

    let mut wedge = MeshBuilder::new();
    // Canonical EastPosX wedge
    // Slope quad
    wedge.push_quad(
        Vec3::new(-hl, y0, hw),
        Vec3::new(hl, y1, hw),
        Vec3::new(hl, y1, -hw),
        Vec3::new(-hl, y0, -hw),
        color,
        meta,
    );
    // High back wall (+X)
    wedge.push_quad(
        Vec3::new(hl, y0, hw),
        Vec3::new(hl, y0, -hw),
        Vec3::new(hl, y1, -hw),
        Vec3::new(hl, y1, hw),
        color,
        meta,
    );
    // Right cheek (+Z)
    wedge.push_triangle(
        Vec3::new(-hl, y0, hw),
        Vec3::new(hl, y0, hw),
        Vec3::new(hl, y1, hw),
        color,
        meta,
    );
    // Left cheek (-Z)
    wedge.push_triangle(
        Vec3::new(hl, y0, -hw),
        Vec3::new(-hl, y0, -hw),
        Vec3::new(hl, y1, -hw),
        color,
        meta,
    );
    if include_bottom {
        wedge.push_quad(
            Vec3::new(-hl, y0, -hw),
            Vec3::new(hl, y0, -hw),
            Vec3::new(hl, y0, hw),
            Vec3::new(-hl, y0, hw),
            color,
            meta,
        );
    }

    let transform =
        Mat4::from_rotation_translation(Quat::from_rotation_y(rot_y), Vec3::new(cx, 0.0, cz));
    b.append_transformed(wedge.as_mesh(), transform);
}

pub fn emit_sim_ramp(
    b: &mut MeshBuilder,
    sim_min: SimVec3,
    sim_max: SimVec3,
    climb_dir: CardinalDir,
    include_bottom: bool,
    color: [f32; 4],
    meta: VertexMeta,
) {
    let r_min = sim_to_render_point(sim_min);
    let r_max = sim_to_render_point(sim_max);
    emit_ramp(b, r_min, r_max, climb_dir, include_bottom, color, meta);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_primitives_pass_ccw_winding_and_bounds_validation() {
        let mut builder = MeshBuilder::new();

        // 1. Quad & Grid Quads
        emit_quad(
            &mut builder,
            Vec3::ZERO,
            Vec3::X,
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, 1.0),
            [1.0; 4],
            VertexMeta::DEFAULT,
        );
        emit_grid_quad_00_11_flat(
            &mut builder,
            Vec3::ZERO,
            Vec3::X,
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, 1.0),
            [1.0; 4],
            VertexMeta::DEFAULT,
        );

        let v0 = WorldVertex::new(Vec3::ZERO, Vec3::Y, [1.0; 4]);
        let v1 = WorldVertex::new(Vec3::X, Vec3::Y, [1.0; 4]);
        let v2 = WorldVertex::new(Vec3::new(1.0, 0.0, 1.0), Vec3::Y, [1.0; 4]);
        let v3 = WorldVertex::new(Vec3::new(0.0, 0.0, 1.0), Vec3::Y, [1.0; 4]);
        emit_grid_quad_00_11_smooth(&mut builder, v0, v1, v2, v3);

        // 2. Cuboid & Box
        emit_cuboid(
            &mut builder,
            Vec3::ZERO,
            Vec3::splat(1.0),
            FaceMask::ALL,
            [1.0; 4],
            VertexMeta::DEFAULT,
        );

        // 3. Cylinder & Prism
        emit_cylinder(
            &mut builder,
            Vec3::ZERO,
            1.0,
            0.8,
            2.0,
            8,
            true,
            true,
            [1.0; 4],
            VertexMeta::DEFAULT,
        );

        // 4. Cone & Pyramid
        emit_cone(
            &mut builder,
            Vec3::ZERO,
            1.0,
            2.0,
            6,
            true,
            [1.0; 4],
            VertexMeta::DEFAULT,
        );
        emit_pyramid(
            &mut builder,
            Vec3::ZERO,
            1.0,
            2.0,
            true,
            [1.0; 4],
            VertexMeta::DEFAULT,
        );

        // 5. Gable Roof
        emit_gable_roof(
            &mut builder,
            Vec3::ZERO,
            Vec3::new(2.0, 2.0, 2.0),
            RidgeAxis::AlongX,
            true,
            [1.0; 4],
            VertexMeta::DEFAULT,
        );
        emit_gable_roof(
            &mut builder,
            Vec3::ZERO,
            Vec3::new(2.0, 2.0, 2.0),
            RidgeAxis::AlongZ,
            true,
            [1.0; 4],
            VertexMeta::DEFAULT,
        );

        // 6. Ramps
        for dir in [
            CardinalDir::EastPosX,
            CardinalDir::NorthNegZ,
            CardinalDir::WestNegX,
            CardinalDir::SouthPosZ,
        ] {
            emit_ramp(
                &mut builder,
                Vec3::ZERO,
                Vec3::new(2.0, 1.5, 2.0),
                dir,
                true,
                [1.0; 4],
                VertexMeta::DEFAULT,
            );
        }

        let mesh = builder.build();
        mesh.validate()
            .expect("All primitives must validate successfully with outward CCW winding!");
    }

    #[test]
    fn cuboid_face_mask_omits_hidden_faces_accurately() {
        let mut b_none = MeshBuilder::new();
        emit_cuboid(
            &mut b_none,
            Vec3::ZERO,
            Vec3::ONE,
            FaceMask::NONE,
            [1.0; 4],
            VertexMeta::DEFAULT,
        );
        assert!(b_none.as_mesh().is_empty());

        let mut b_single = MeshBuilder::new();
        emit_cuboid(
            &mut b_single,
            Vec3::ZERO,
            Vec3::ONE,
            FaceMask::TOP,
            [1.0; 4],
            VertexMeta::DEFAULT,
        );
        assert_eq!(b_single.as_mesh().vertices.len(), 4);
        assert_eq!(b_single.as_mesh().indices.len(), 6);

        let mut b_top_sides = MeshBuilder::new();
        emit_cuboid(
            &mut b_top_sides,
            Vec3::ZERO,
            Vec3::ONE,
            FaceMask::TOP_AND_SIDES,
            [1.0; 4],
            VertexMeta::DEFAULT,
        );
        assert_eq!(b_top_sides.as_mesh().vertices.len(), 20);
        assert_eq!(b_top_sides.as_mesh().indices.len(), 30);
    }
}
