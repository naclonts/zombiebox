//! Exact 3D ray/sphere-cast intersection primitives and 2D Minkowski circle-rect CCD (`+z` up).

use super::query::{ColliderKind, Hit3, HitSurface, QueryFilter, Ray3, SpatialWorld};
use super::{Aabb3, Vec2, Vec3};

const SEGMENT_LENGTH: f32 = 8.0;

#[inline]
fn point_in_triangle_barycentric(p: Vec3, a: Vec3, b: Vec3, c: Vec3, eps: f32) -> bool {
    let v0 = b - a;
    let v1 = c - a;
    let v2 = p - a;
    let d00 = v0.dot(v0);
    let d01 = v0.dot(v1);
    let d11 = v1.dot(v1);
    let d20 = v2.dot(v0);
    let d21 = v2.dot(v1);
    let denom = d00 * d11 - d01 * d01;
    if denom.abs() <= 1e-12 {
        return false;
    }
    let inv = 1.0 / denom;
    let v = (d11 * d20 - d01 * d21) * inv;
    let w = (d00 * d21 - d01 * d20) * inv;
    let u = 1.0 - v - w;
    u >= -eps && v >= -eps && w >= -eps
}

#[inline]
fn closest_point_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq <= 1e-12 {
        return a;
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    a + ab * t
}

fn closest_point_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;

    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }

    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return a + ab * v;
    }

    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return a + ac * w;
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + (c - b) * w;
    }

    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    a + ab * v + ac * w
}

/// Watertight Möller–Trumbore ray-vs-triangle test for `t in [0.0, max_t]`.
/// Returns `(t, point, unit_normal)` with `normal.dot(ray.dir) <= 0.0`.
pub fn intersect_ray_triangle(
    ray: Ray3,
    a: Vec3,
    b: Vec3,
    c: Vec3,
    max_t: f32,
) -> Option<(f32, Vec3, Vec3)> {
    let e1 = b - a;
    let e2 = c - a;
    let pvec = ray.dir.cross(e2);
    let det = e1.dot(pvec);
    if det.abs() <= 1e-8 {
        return None;
    }
    let inv_det = 1.0 / det;
    let tvec = ray.origin - a;
    let u = tvec.dot(pvec) * inv_det;
    if !(-1e-5..=1.0 + 1e-5).contains(&u) {
        return None;
    }
    let qvec = tvec.cross(e1);
    let v = ray.dir.dot(qvec) * inv_det;
    if v < -1e-5 || u + v > 1.0 + 1e-5 {
        return None;
    }
    let t = e2.dot(qvec) * inv_det;
    if !t.is_finite() || t < 0.0 || t > max_t {
        return None;
    }
    let mut normal = e1.cross(e2).normalized_or_up();
    if normal.dot(ray.dir) > 0.0 {
        normal = -normal;
    }
    Some((t, ray.at(t), normal))
}

/// Exact slab ray-vs-`Aabb3` intersection for `t in [0.0, max_t]`.
/// Returns `(t, point, normal)`.
pub fn intersect_ray_aabb3(ray: Ray3, bounds: Aabb3, max_t: f32) -> Option<(f32, Vec3, Vec3)> {
    let mut t_enter = f32::NEG_INFINITY;
    let mut t_exit = max_t;
    let mut enter_normal = (-ray.dir).normalized_or_up();

    let axes = [
        (ray.origin.x, ray.dir.x, bounds.min.x, bounds.max.x, Vec3::X),
        (ray.origin.y, ray.dir.y, bounds.min.y, bounds.max.y, Vec3::Y),
        (ray.origin.z, ray.dir.z, bounds.min.z, bounds.max.z, Vec3::Z),
    ];

    for (o, d, bmin, bmax, axis_n) in axes {
        if d.abs() <= 1e-8 {
            if o < bmin || o > bmax {
                return None;
            }
        } else {
            let inv_d = 1.0 / d;
            let mut t1 = (bmin - o) * inv_d;
            let mut t2 = (bmax - o) * inv_d;
            let mut n = -axis_n;
            if t1 > t2 {
                core::mem::swap(&mut t1, &mut t2);
                n = axis_n;
            }
            if t1 > t_enter {
                t_enter = t1;
                enter_normal = n;
            }
            if t2 < t_exit {
                t_exit = t2;
            }
            if t_enter > t_exit {
                return None;
            }
        }
    }

    if t_exit < 0.0 {
        return None;
    }
    if t_enter < 0.0 {
        if t_exit >= 1e-5 {
            return Some((0.0, ray.origin, (-ray.dir).normalized_or_up()));
        }
        return None;
    }
    if t_enter <= max_t {
        Some((t_enter, ray.at(t_enter), enter_normal))
    } else {
        None
    }
}

#[inline]
fn intersect_ray_sphere(
    ray: Ray3,
    center: Vec3,
    radius: f32,
    max_t: f32,
) -> Option<(f32, Vec3, Vec3, Vec3)> {
    let m = ray.origin - center;
    let b = m.dot(ray.dir);
    let c = m.length_squared() - radius * radius;
    if c <= 0.0 {
        let mut n = m.normalized();
        if n == Vec3::ZERO {
            n = (-ray.dir).normalized_or_up();
        }
        return Some((0.0, ray.origin, n, center + n * radius));
    }
    if b > 0.0 {
        return None;
    }
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()).max(0.0);
    if t > max_t {
        return None;
    }
    let center_at_hit = ray.at(t);
    let n = (center_at_hit - center).normalized_or_up();
    let contact = center + n * radius;
    Some((t, center_at_hit, n, contact))
}

/// Exact quadratic ray-vs-capsule intersection for `t in [0.0, max_t]`.
/// Returns `(t, center_at_hit, outward_normal, contact_point)`.
pub fn intersect_ray_capsule(
    ray: Ray3,
    a: Vec3,
    b: Vec3,
    radius: f32,
    max_t: f32,
) -> Option<(f32, Vec3, Vec3, Vec3)> {
    let closest0 = closest_point_on_segment(ray.origin, a, b);
    let diff0 = ray.origin - closest0;
    if diff0.length_squared() <= radius * radius {
        let n = if diff0.length_squared() > 1e-10 {
            diff0.normalized()
        } else {
            (-ray.dir).normalized_or_up()
        };
        return Some((0.0, ray.origin, n, closest0));
    }

    let mut best: Option<(f32, Vec3, Vec3, Vec3)> = None;
    let mut best_t = max_t;

    let ab = b - a;
    let ab_len = ab.length();
    if ab_len > 1e-6 {
        let u = ab / ab_len;
        let m = ray.origin - a;
        let d_perp = ray.dir - u * ray.dir.dot(u);
        let m_perp = m - u * m.dot(u);
        let qa = d_perp.length_squared();
        let qb = m_perp.dot(d_perp);
        let qc = m_perp.length_squared() - radius * radius;
        if qa > 1e-10 {
            let disc = qb * qb - qa * qc;
            if disc >= 0.0 {
                let t = (-qb - disc.sqrt()) / qa;
                if t >= 0.0 && t <= best_t {
                    let p = ray.at(t);
                    let s = (p - a).dot(u) / ab_len;
                    if (0.0..=1.0).contains(&s) {
                        let axis_pt = a + ab * s;
                        let n = (p - axis_pt).normalized_or_up();
                        best = Some((t, p, n, axis_pt));
                        best_t = t;
                    }
                }
            }
        }
    }

    for end in [a, b] {
        if let Some(hit) = intersect_ray_sphere(ray, end, radius, best_t)
            && hit.0 < best_t
        {
            best_t = hit.0;
            best = Some((hit.0, hit.1, hit.2, end));
        }
    }

    best
}

/// Exact swept sphere-vs-`Aabb3` test for `t in [0.0, max_t]`.
/// Returns `(t, outward_normal, contact_point)`.
pub fn sweep_sphere_aabb3(
    ray: Ray3,
    radius: f32,
    bounds: Aabb3,
    max_t: f32,
) -> Option<(f32, Vec3, Vec3)> {
    if radius <= 0.0 {
        return intersect_ray_aabb3(ray, bounds, max_t);
    }

    let (t_box, q_box, n_box) = intersect_ray_aabb3(ray, bounds.expand(radius), max_t)?;

    if bounds.distance_squared_to_point(ray.origin) <= radius * radius {
        let closest = bounds.closest_point(ray.origin);
        let diff = ray.origin - closest;
        let n = if diff.length_squared() > 1e-10 {
            diff.normalized()
        } else {
            (-ray.dir).normalized_or_up()
        };
        return Some((0.0, n, closest));
    }

    if t_box > 0.0 {
        let out_count = usize::from(q_box.x < bounds.min.x || q_box.x > bounds.max.x)
            + usize::from(q_box.y < bounds.min.y || q_box.y > bounds.max.y)
            + usize::from(q_box.z < bounds.min.z || q_box.z > bounds.max.z);
        if out_count <= 1 {
            return Some((t_box, n_box, bounds.closest_point(q_box)));
        }
    }

    // Corner/edge Voronoi region of the Minkowski rounded box: test the 3 face slabs + 12 edge capsules.
    let mut best: Option<(f32, Vec3, Vec3)> = None;
    let mut best_t = max_t;

    let face_boxes = [
        Aabb3::new(
            Vec3::new(bounds.min.x - radius, bounds.min.y, bounds.min.z),
            Vec3::new(bounds.max.x + radius, bounds.max.y, bounds.max.z),
        ),
        Aabb3::new(
            Vec3::new(bounds.min.x, bounds.min.y - radius, bounds.min.z),
            Vec3::new(bounds.max.x, bounds.max.y + radius, bounds.max.z),
        ),
        Aabb3::new(
            Vec3::new(bounds.min.x, bounds.min.y, bounds.min.z - radius),
            Vec3::new(bounds.max.x, bounds.max.y, bounds.max.z + radius),
        ),
    ];

    for fb in face_boxes {
        if let Some((t, q, n)) = intersect_ray_aabb3(ray, fb, best_t)
            && t < best_t
        {
            best_t = t;
            best = Some((t, n, bounds.closest_point(q)));
        }
    }

    let (x0, x1) = (bounds.min.x, bounds.max.x);
    let (y0, y1) = (bounds.min.y, bounds.max.y);
    let (z0, z1) = (bounds.min.z, bounds.max.z);
    let edges = [
        (Vec3::new(x0, y0, z0), Vec3::new(x1, y0, z0)),
        (Vec3::new(x0, y1, z0), Vec3::new(x1, y1, z0)),
        (Vec3::new(x0, y0, z1), Vec3::new(x1, y0, z1)),
        (Vec3::new(x0, y1, z1), Vec3::new(x1, y1, z1)),
        (Vec3::new(x0, y0, z0), Vec3::new(x0, y1, z0)),
        (Vec3::new(x1, y0, z0), Vec3::new(x1, y1, z0)),
        (Vec3::new(x0, y0, z1), Vec3::new(x0, y1, z1)),
        (Vec3::new(x1, y0, z1), Vec3::new(x1, y1, z1)),
        (Vec3::new(x0, y0, z0), Vec3::new(x0, y0, z1)),
        (Vec3::new(x1, y0, z0), Vec3::new(x1, y0, z1)),
        (Vec3::new(x0, y1, z0), Vec3::new(x0, y1, z1)),
        (Vec3::new(x1, y1, z0), Vec3::new(x1, y1, z1)),
    ];

    for (ea, eb) in edges {
        if let Some((t, _, n, contact)) = intersect_ray_capsule(ray, ea, eb, radius, best_t)
            && t < best_t
        {
            best_t = t;
            best = Some((t, n, contact));
        }
    }

    best
}

/// Exact swept sphere-vs-triangle test for `t in [0.0, max_t]`.
/// Returns `(t, outward_normal, contact_point)`.
pub fn sweep_sphere_triangle(
    ray: Ray3,
    radius: f32,
    a: Vec3,
    b: Vec3,
    c: Vec3,
    max_t: f32,
) -> Option<(f32, Vec3, Vec3)> {
    if radius <= 0.0 {
        return intersect_ray_triangle(ray, a, b, c, max_t);
    }

    let tri_aabb = Aabb3::from_points(a, b)
        .union(Aabb3::from_points(c, c))
        .expand(radius + 1e-4);
    intersect_ray_aabb3(ray, tri_aabb, max_t)?;

    let closest0 = closest_point_on_triangle(ray.origin, a, b, c);
    let diff0 = ray.origin - closest0;
    if diff0.length_squared() <= radius * radius {
        let n = if diff0.length_squared() > 1e-10 {
            diff0.normalized()
        } else {
            let mut fn_ = (b - a).cross(c - a).normalized_or_up();
            if fn_.dot(ray.dir) > 0.0 {
                fn_ = -fn_;
            }
            fn_
        };
        return Some((0.0, n, closest0));
    }

    let mut best: Option<(f32, Vec3, Vec3)> = None;
    let mut best_t = max_t;

    let raw_n = (b - a).cross(c - a).normalized();
    if raw_n != Vec3::ZERO {
        for plane_n in [raw_n, -raw_n] {
            let denom = ray.dir.dot(plane_n);
            if denom < -1e-7 {
                let t = (radius - (ray.origin - a).dot(plane_n)) / denom;
                if t >= 0.0 && t <= best_t {
                    let center = ray.at(t);
                    let contact = center - plane_n * radius;
                    if point_in_triangle_barycentric(contact, a, b, c, 1e-5) {
                        best_t = t;
                        best = Some((t, plane_n, contact));
                    }
                }
            }
        }
    }

    for (ea, eb) in [(a, b), (b, c), (c, a)] {
        if let Some((t, _, n, contact)) = intersect_ray_capsule(ray, ea, eb, radius, best_t)
            && t < best_t
        {
            best_t = t;
            best = Some((t, n, contact));
        }
    }

    best
}

/// Exact 2D Minkowski rounded-rectangle CCD for circle `(p0, delta, radius)` (`t in [0.0, 1.0]`).
/// Returns `(t_hit, unit_normal_xy)` when moving into or colliding with `[min, max]`.
pub fn sweep_circle_rect_2d(
    p0: Vec2,
    delta: Vec2,
    radius: f32,
    min: Vec2,
    max: Vec2,
) -> Option<(f32, Vec2)> {
    if delta.length_squared() <= 1e-14 {
        return None;
    }

    // Fast reject against expanded 2D AABB.
    let exp_min = Vec2::new(min.x - radius, min.y - radius);
    let exp_max = Vec2::new(max.x + radius, max.y + radius);
    let seg_min = Vec2::new(p0.x.min(p0.x + delta.x), p0.y.min(p0.y + delta.y));
    let seg_max = Vec2::new(p0.x.max(p0.x + delta.x), p0.y.max(p0.y + delta.y));
    if seg_max.x < exp_min.x
        || seg_min.x > exp_max.x
        || seg_max.y < exp_min.y
        || seg_min.y > exp_max.y
    {
        return None;
    }

    let closest0 = Vec2::new(p0.x.clamp(min.x, max.x), p0.y.clamp(min.y, max.y));
    let diff0 = p0 - closest0;
    if diff0.length_squared() < radius * radius {
        let n = if diff0.length_squared() > 1e-12 {
            diff0.normalized()
        } else {
            (-delta).normalized()
        };
        // Only block if moving inward toward the collider; allow separating moves to exit cleanly.
        if delta.dot(n) < 0.0 {
            return Some((0.0, n));
        }
        return None;
    }

    let mut best_t = 1.0_f32;
    let mut best_n = Vec2::ZERO;
    let mut hit = false;

    // 2 flat face rectangles (Minkowski union of X-expanded and Y-expanded boxes).
    let face_rects = [
        (
            Vec2::new(min.x - radius, min.y),
            Vec2::new(max.x + radius, max.y),
        ),
        (
            Vec2::new(min.x, min.y - radius),
            Vec2::new(max.x, max.y + radius),
        ),
    ];
    for (rmin, rmax) in face_rects {
        if let Some((t, n)) = sweep_point_aabb_2d(p0, delta, rmin, rmax, best_t)
            && delta.dot(n) < 0.0
            && (!hit || t < best_t)
        {
            best_t = t;
            best_n = n;
            hit = true;
        }
    }

    // 4 corner circles of radius `radius`.
    let corners = [
        Vec2::new(min.x, min.y),
        Vec2::new(max.x, min.y),
        Vec2::new(min.x, max.y),
        Vec2::new(max.x, max.y),
    ];
    let qa = delta.length_squared();
    for c in corners {
        let m = p0 - c;
        let qb = m.dot(delta);
        if qb >= 0.0 {
            continue;
        }
        let qc = m.length_squared() - radius * radius;
        let disc = qb * qb - qa * qc;
        if disc >= 0.0 {
            let t = ((-qb - disc.sqrt()) / qa).max(0.0);
            if t <= best_t && (!hit || t < best_t) {
                let pt = p0 + delta * t;
                let n = (pt - c).normalized();
                if n != Vec2::ZERO && delta.dot(n) < 0.0 {
                    best_t = t;
                    best_n = n;
                    hit = true;
                }
            }
        }
    }

    hit.then_some((best_t, best_n))
}

#[inline]
fn sweep_point_aabb_2d(
    p0: Vec2,
    delta: Vec2,
    min: Vec2,
    max: Vec2,
    max_t: f32,
) -> Option<(f32, Vec2)> {
    let mut t_enter = 0.0_f32;
    let mut t_exit = max_t;
    let mut enter_n = Vec2::ZERO;

    for (o, d, bmin, bmax, axis_n) in [
        (p0.x, delta.x, min.x, max.x, Vec2::new(1.0, 0.0)),
        (p0.y, delta.y, min.y, max.y, Vec2::new(0.0, 1.0)),
    ] {
        if d.abs() <= 1e-10 {
            if o < bmin || o > bmax {
                return None;
            }
        } else {
            let inv_d = 1.0 / d;
            let mut t1 = (bmin - o) * inv_d;
            let mut t2 = (bmax - o) * inv_d;
            let mut n = -axis_n;
            if t1 > t2 {
                core::mem::swap(&mut t1, &mut t2);
                n = axis_n;
            }
            if t1 > t_enter {
                t_enter = t1;
                enter_n = n;
            }
            if t2 < t_exit {
                t_exit = t2;
            }
            if t_enter > t_exit {
                return None;
            }
        }
    }

    (enter_n != Vec2::ZERO && t_enter <= max_t).then_some((t_enter, enter_n))
}

/// Segmented 3D raycast against colliders and `00 -> 11` surface quads.
pub fn raycast_world<W: SpatialWorld + ?Sized>(
    world: &W,
    ray: Ray3,
    max_distance: f32,
    filter: QueryFilter,
) -> Option<Hit3> {
    cast_world_internal(world, ray, max_distance, 0.0, filter)
}

/// Segmented 3D swept sphere-cast from `from` to `to` with `radius`.
pub fn sphere_cast_world<W: SpatialWorld + ?Sized>(
    world: &W,
    from: Vec3,
    to: Vec3,
    radius: f32,
    filter: QueryFilter,
) -> Option<Hit3> {
    let (ray, max_distance) = Ray3::from_points(from, to)?;
    cast_world_internal(world, ray, max_distance, radius.max(0.0), filter)
}

fn cast_world_internal<W: SpatialWorld + ?Sized>(
    world: &W,
    ray: Ray3,
    max_distance: f32,
    radius: f32,
    filter: QueryFilter,
) -> Option<Hit3> {
    if !max_distance.is_finite() || max_distance <= 0.0 || !ray.dir.is_finite() {
        return None;
    }
    let dir = ray.dir.normalized();
    if dir == Vec3::ZERO {
        return None;
    }
    let ray = Ray3 {
        origin: ray.origin,
        dir,
    };

    let mut best: Option<Hit3> = None;
    let mut best_t = max_distance;
    let mut seg_start = 0.0_f32;
    let mut colliders = Vec::new();
    let mut quads = Vec::new();

    while seg_start < best_t {
        let seg_end = (seg_start + SEGMENT_LENGTH).min(best_t);
        if seg_end <= seg_start {
            break;
        }
        let seg_aabb = Aabb3::from_points(ray.at(seg_start), ray.at(seg_end)).expand(radius + 0.05);

        world.colliders_in(seg_aabb, &mut colliders);
        for c in &colliders {
            if filter.ignore_window_glass && matches!(c.kind, ColliderKind::WindowGlass { .. }) {
                continue;
            }
            if !filter.hit_water_hazard && matches!(c.kind, ColliderKind::WaterHazard { .. }) {
                continue;
            }
            let hit = if radius > 0.0 {
                sweep_sphere_aabb3(ray, radius, c.bounds, best_t)
            } else {
                intersect_ray_aabb3(ray, c.bounds, best_t)
            };
            if let Some((t, p_or_n, c_or_p)) = hit
                && t < best_t
            {
                let (point, normal) = if radius > 0.0 {
                    (c_or_p, p_or_n)
                } else {
                    (p_or_n, c_or_p)
                };
                best_t = t;
                best = Some(Hit3 {
                    distance: t,
                    point,
                    normal,
                    surface: HitSurface::Collider(c.kind),
                    material: c.material,
                });
            }
        }

        let (tmin, tmax) = seg_aabb.tile_bounds();
        world.surface_quads_in(tmin, tmax, &mut quads);
        for q in &quads {
            let q_min_z = q.corners[0]
                .min(q.corners[1])
                .min(q.corners[2])
                .min(q.corners[3]);
            let q_max_z = q.corners[0]
                .max(q.corners[1])
                .max(q.corners[2])
                .max(q.corners[3]);
            if q_max_z + radius < seg_aabb.min.z || q_min_z - radius > seg_aabb.max.z {
                continue;
            }
            let p00 = Vec3::new(q.x0, q.y0, q.corners[0]);
            let p10 = Vec3::new(q.x1, q.y0, q.corners[1]);
            let p01 = Vec3::new(q.x0, q.y1, q.corners[2]);
            let p11 = Vec3::new(q.x1, q.y1, q.corners[3]);

            for (a, b, c) in [(p00, p10, p11), (p00, p11, p01)] {
                let hit = if radius > 0.0 {
                    sweep_sphere_triangle(ray, radius, a, b, c, best_t)
                } else {
                    intersect_ray_triangle(ray, a, b, c, best_t)
                };
                if let Some((t, p_or_n, c_or_p)) = hit
                    && t < best_t
                {
                    let (point, normal) = if radius > 0.0 {
                        (c_or_p, p_or_n)
                    } else {
                        (p_or_n, c_or_p)
                    };
                    best_t = t;
                    best = Some(Hit3 {
                        distance: t,
                        point,
                        normal,
                        surface: q.surface,
                        material: q.material,
                    });
                }
            }
        }

        seg_start = seg_end;
    }

    best
}
