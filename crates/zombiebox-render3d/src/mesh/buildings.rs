//! CPU building geometry derived from the same tile dimensions as spatial collision.
use super::{
    FaceMask, GeometryTag, MaterialFlags, MeshBuffer, MeshBuilder, VertexMeta, emit_box_min_max,
};
use crate::materials::MaterialRegistry;
use crate::scene::{ChunkSnapshot, FloorCellSnapshot};
use glam::Vec3;
use zombiebox_core::{
    CHUNK_SIZE, DOOR_OPENING_HEIGHT, FLOOR_H, Position, SLAB_THICKNESS, StairGeometry, Surface,
    Tile3D, TileKind, WALL_HALF_THICKNESS, WALL_HEIGHT, WINDOW_LINTEL_Z, WINDOW_SILL_HEIGHT,
    WallOrientation,
};

fn cell(s: &ChunkSnapshot, p: Position, f: u8) -> Option<&FloorCellSnapshot> {
    s.world_tile(p)?.floor(f)
}
fn barrier(k: TileKind) -> bool {
    matches!(
        k,
        TileKind::ExteriorWall
            | TileKind::InteriorWall
            | TileKind::Pillar
            | TileKind::Window
            | TileKind::BrokenWindow
            | TileKind::Door { .. }
    )
}
fn meta(tag: GeometryTag, f: u8, id: u32) -> VertexMeta {
    VertexMeta::DEFAULT
        .with_tag(tag)
        .with_floor(f)
        .with_flags(MaterialFlags::INDOOR_AMBIENT)
        .with_building(id.min(u16::MAX as u32) as u16, 0)
}
#[allow(clippy::too_many_arguments)]
fn box3(
    b: &mut MeshBuilder,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    z0: f32,
    z1: f32,
    c: [f32; 4],
    m: VertexMeta,
) {
    if x1 > x0 && y1 > y0 && z1 > z0 {
        emit_box_min_max(
            b,
            Vec3::new(x0, z0, y0),
            Vec3::new(x1, z1, y1),
            FaceMask::ALL,
            c,
            m,
        );
    }
}
#[allow(clippy::too_many_arguments)]
fn wall_box(
    b: &mut MeshBuilder,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    z0: f32,
    z1: f32,
    base: f32,
    c: [f32; 4],
    m: VertexMeta,
) {
    if m.tag != GeometryTag::WallLower {
        box3(b, x0, x1, y0, y1, z0, z1, c, m);
        return;
    }
    let cut = base + 0.48;
    box3(b, x0, x1, y0, y1, z0, z1.min(cut), c, m);
    box3(
        b,
        x0,
        x1,
        y0,
        y1,
        z0.max(cut),
        z1,
        c,
        m.with_tag(GeometryTag::WallUpper),
    );
}
fn stair_dirs(
    s: &ChunkSnapshot,
    p: Position,
    f: u8,
    c: &FloorCellSnapshot,
) -> ((i8, i8), (i8, i8)) {
    let low = if c.stair_dir == (0, 0) {
        (-1, 0)
    } else {
        c.stair_dir
    };
    let opp = (-low.0, -low.1);
    let a = (low.1, -low.0);
    let b = (-low.1, low.0);
    let (first, second) = if b.1 < a.1 { (b, a) } else { (a, b) };
    let open = |d: (i8, i8)| {
        cell(s, Position::new(p.x + d.0 as i32, p.y + d.1 as i32), f + 1).is_some_and(|v| {
            v.building_id == c.building_id && v.surface != Surface::Air && !barrier(v.kind)
        })
    };
    (
        low,
        [opp, first, second]
            .into_iter()
            .find(|&d| open(d))
            .unwrap_or(opp),
    )
}

/// Clip a support triangle against a horizontal elevation boundary.
fn clip_stair_polygon(poly: &[Vec3], threshold: f32, keep_above: bool) -> Vec<Vec3> {
    let mut result = Vec::new();
    if poly.is_empty() {
        return result;
    }
    let inside = |p: Vec3| {
        if keep_above {
            p.y >= threshold
        } else {
            p.y <= threshold
        }
    };
    let mut prev = *poly.last().unwrap();
    for &cur in poly {
        if inside(prev) != inside(cur) {
            let t = (threshold - prev.y) / (cur.y - prev.y);
            result.push(prev.lerp(cur, t));
        }
        if inside(cur) {
            result.push(cur);
        }
        prev = cur;
    }
    result
}

/// Cut horizontal treads and vertical risers from the collision support's
/// piecewise-linear triangles, so turns follow its exact elevation contours.
fn emit_stairs(
    out: &mut MeshBuilder,
    g: &StairGeometry,
    z: f32,
    color: [f32; 4],
    meta: VertexMeta,
) {
    const STEPS: usize = 8;
    let step = FLOOR_H / STEPS as f32;
    let x0 = g.tile.x as f32 - 0.5;
    let y0 = g.tile.y as f32 - 0.5;
    let divisions = if g.is_straight() { 1 } else { 2 };
    let size = 1.0 / divisions as f32;
    let meta = meta.with_tag(GeometryTag::Stair);
    for iy in 0..divisions {
        for ix in 0..divisions {
            let gx = ix * (2 / divisions);
            let gy = iy * (2 / divisions);
            let stride = 2 / divisions;
            let x = x0 + ix as f32 * size;
            let y = y0 + iy as f32 * size;
            let a = Vec3::new(x, g.grid[gy][gx], y);
            let b = Vec3::new(x + size, g.grid[gy][gx + stride], y);
            let d = Vec3::new(x, g.grid[gy + stride][gx], y + size);
            let e = Vec3::new(x + size, g.grid[gy + stride][gx + stride], y + size);
            for triangle in [[a, e, b], [a, d, e]] {
                let underside = triangle.map(|v| v - Vec3::Y * step);
                out.push_triangle(underside[0], underside[2], underside[1], color, meta);
                for level in 0..=STEPS {
                    let low = z + (level as f32 - 0.5) * step;
                    let high = z + (level as f32 + 0.5) * step;
                    let poly = clip_stair_polygon(&triangle, low, true);
                    let poly = clip_stair_polygon(&poly, high, false);
                    if poly.len() >= 3 {
                        let h = z + level as f32 * step;
                        let point = |v: Vec3| Vec3::new(v.x, h, v.z);
                        for i in 1..poly.len() - 1 {
                            let (a, b, c) = (point(poly[0]), point(poly[i]), point(poly[i + 1]));
                            if (b - a).cross(c - a).length_squared() > 1e-12 {
                                out.push_triangle(a, b, c, color, meta);
                            }
                        }
                        // Close only the perimeter, from the tread down to the
                        // sloping underside. Interior contour edges are risers.
                        for i in 0..poly.len() {
                            let p = poly[i];
                            let q = poly[(i + 1) % poly.len()];
                            let boundary = |a: f32, b: f32, edge: f32| {
                                (a - edge).abs() < 1e-5 && (b - edge).abs() < 1e-5
                            };
                            if !(boundary(p.x, q.x, x0)
                                || boundary(p.x, q.x, x0 + 1.0)
                                || boundary(p.z, q.z, y0)
                                || boundary(p.z, q.z, y0 + 1.0))
                                || (p - q).length_squared() < 1e-12
                            {
                                continue;
                            }
                            let v = [point(p), point(q), q - Vec3::Y * step, p - Vec3::Y * step];
                            let outward = Vec3::new(
                                (p.x + q.x) * 0.5 - (x0 + 0.5),
                                0.0,
                                (p.z + q.z) * 0.5 - (y0 + 0.5),
                            );
                            let normal = (v[1] - v[0]).cross(v[2] - v[0]);
                            if normal.dot(outward) > 0.0 {
                                out.push_quad(v[0], v[1], v[2], v[3], color, meta);
                            } else {
                                out.push_quad(v[3], v[2], v[1], v[0], color, meta);
                            }
                        }
                    }
                }
                for level in 0..STEPS {
                    let threshold = z + (level as f32 + 0.5) * step;
                    let mut crossings = Vec::new();
                    for i in 0..3 {
                        let a = triangle[i];
                        let b = triangle[(i + 1) % 3];
                        if (a.y < threshold) != (b.y < threshold) {
                            crossings.push(a.lerp(b, (threshold - a.y) / (b.y - a.y)));
                        }
                    }
                    if crossings.len() == 2 && crossings[0].distance_squared(crossings[1]) > 1e-12 {
                        let low = z + level as f32 * step;
                        let high = low + step;
                        let p = crossings[0];
                        let q = crossings[1];
                        let vertices = [
                            Vec3::new(p.x, low, p.z),
                            Vec3::new(q.x, low, q.z),
                            Vec3::new(q.x, high, q.z),
                            Vec3::new(p.x, high, p.z),
                        ];
                        let lower = triangle.iter().find(|p| p.y < threshold).unwrap();
                        let normal = (vertices[1] - vertices[0]).cross(vertices[2] - vertices[0]);
                        if normal.dot(*lower - vertices[0]) > 0.0 {
                            out.push_quad(
                                vertices[0],
                                vertices[1],
                                vertices[2],
                                vertices[3],
                                color,
                                meta,
                            );
                        } else {
                            out.push_quad(
                                vertices[3],
                                vertices[2],
                                vertices[1],
                                vertices[0],
                                color,
                                meta,
                            );
                        }
                    }
                }
            }
        }
    }
}
fn footprint(s: &ChunkSnapshot, p: Position, f: u8, id: u32) -> (f32, f32, f32, f32) {
    let same = |dx, dy| {
        cell(s, Position::new(p.x + dx, p.y + dy), f)
            .is_some_and(|c| c.building_id as u32 == id && c.surface != Surface::Air)
    };
    let x = p.x as f32;
    let y = p.y as f32;
    (
        x - if same(-1, 0) {
            0.5
        } else {
            WALL_HALF_THICKNESS
        },
        x + if same(1, 0) { 0.5 } else { WALL_HALF_THICKNESS },
        y - if same(0, -1) {
            0.5
        } else {
            WALL_HALF_THICKNESS
        },
        y + if same(0, 1) { 0.5 } else { WALL_HALF_THICKNESS },
    )
}
/// Generate building geometry only for owned tiles; the halo supplies neighbors at chunk seams.
#[must_use]
pub fn build_chunk_buildings_mesh(s: &ChunkSnapshot) -> MeshBuffer {
    let mut mesh = build_chunk_building_shell(s);
    let mut leaf_mesh = super::doors::door_leaf_mesh();
    let original_colors: Vec<_> = leaf_mesh.vertices.iter().map(|v| v.color).collect();
    for leaf in super::doors::door_leaves(s) {
        for (vertex, color) in leaf_mesh.vertices.iter_mut().zip(&original_colors) {
            vertex.meta = leaf.meta.pack();
            vertex.color = std::array::from_fn(|i| color[i] * leaf.tint()[i]);
        }
        mesh.append_transformed(&leaf_mesh, leaf.model(if leaf.opened { 1.0 } else { 0.0 }));
    }
    mesh
}

/// Building geometry for a presentation that draws the hinged leaves as instances.
pub(crate) fn build_chunk_building_shell(s: &ChunkSnapshot) -> MeshBuffer {
    let mut out = MeshBuilder::new();
    for ly in 0..CHUNK_SIZE {
        for lx in 0..CHUNK_SIZE {
            let t = s.local_tile(lx, ly).unwrap();
            let p = t.pos;
            let x = p.x as f32;
            let y = p.y as f32;
            for f in 0..t.floor_count {
                let Some(c) = t.floor(f) else { continue };
                if c.building_id == 0 || c.surface == Surface::Air {
                    continue;
                }
                let Some(bs) = s.building(c.building_id) else {
                    continue;
                };
                let info = &bs.info;
                let z = info.foundation_z + f as f32 * FLOOR_H;
                let palette = MaterialRegistry::for_building_style(info.style);
                let mut color = palette.wall_lit.to_array();
                let tint = match c.damage_tier() {
                    1 => 0.85,
                    2 => 0.68,
                    _ => 1.0,
                };
                for v in &mut color[..3] {
                    *v *= tint
                }
                let slot = info.cutaway_slot_at(p, f) as u32;
                let wallmeta = meta(GeometryTag::WallLower, f, slot);
                let floormeta = meta(GeometryTag::InteriorFloor, f, slot);
                let roofmeta = meta(GeometryTag::Roof, f + 1, slot);
                let openingmeta = meta(GeometryTag::DoorOrWindow, f, slot);
                let (mut x0, mut x1, mut y0, mut y1) = footprint(s, p, f, c.building_id as u32);
                if f > 0 && !matches!(c.kind, TileKind::StairsDown | TileKind::StairsBoth) {
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let q = Position::new(p.x + dx, p.y + dy);
                        if let (Some(sc), Some(sb)) = (cell(s, q, f), cell(s, q, f - 1)) {
                            if sc.building_id == c.building_id
                                && sb.building_id == c.building_id
                                && matches!(sc.kind, TileKind::StairsDown | TileKind::StairsBoth)
                                && matches!(sb.kind, TileKind::StairsUp | TileKind::StairsBoth)
                            {
                                let (lo, hi) = stair_dirs(s, q, f - 1, sb);
                                let d = (-dx as i8, -dy as i8);
                                if d == lo || d == hi {
                                    if dx < 0 {
                                        x0 = x0.max(x - 0.22)
                                    } else if dx > 0 {
                                        x1 = x1.min(x + 0.22)
                                    } else if dy < 0 {
                                        y0 = y0.max(y - 0.22)
                                    } else {
                                        y1 = y1.min(y + 0.22)
                                    }
                                }
                            }
                        }
                    }
                    box3(
                        &mut out,
                        x0,
                        x1,
                        y0,
                        y1,
                        z - SLAB_THICKNESS,
                        z,
                        palette.carpet.to_array(),
                        floormeta,
                    );
                }
                if matches!(c.kind, TileKind::StairsUp | TileKind::StairsBoth)
                    && f + 1 < info.floors
                {
                    let (lo, hi) = stair_dirs(s, p, f, c);
                    let g = StairGeometry::new(Tile3D::from_pos(p, f), z, lo, hi);
                    emit_stairs(&mut out, &g, z, palette.carpet_trim.to_array(), floormeta);
                }
                if !cell(s, p, f + 1).is_some_and(|above| above.building_id == c.building_id) {
                    let (rx0, rx1, ry0, ry1) = footprint(s, p, f, c.building_id as u32);
                    let eave = info.foundation_z + (f + 1) as f32 * FLOOR_H;
                    let wing = info.wing_at(info.wing_index_at(p, f));
                    if bs.has_pitched_roof {
                        // Shared vertex heights make roof slopes continuous across tiles/chunks.
                        let rise = |vx: f32, vy: f32| {
                            let left = wing.min_x as f32;
                            let right = wing.max_x as f32;
                            let near = wing.min_y as f32;
                            let far = wing.max_y as f32;
                            let dx = ((vx - left).min(right - vx)
                                / ((right - left) * 0.5).max(1.0))
                            .clamp(0.0, 1.0);
                            let dy = ((vy - near).min(far - vy) / ((far - near) * 0.5).max(1.0))
                                .clamp(0.0, 1.0);
                            0.95 * dx.min(dy)
                        };
                        let a = Vec3::new(rx0, eave + rise(rx0, ry0), ry0);
                        let b = Vec3::new(rx1, eave + rise(rx1, ry0), ry0);
                        let d = Vec3::new(rx0, eave + rise(rx0, ry1), ry1);
                        let e = Vec3::new(rx1, eave + rise(rx1, ry1), ry1);
                        out.push_triangle(a, e, b, palette.roof_front.to_array(), roofmeta);
                        out.push_triangle(a, d, e, palette.roof_front.to_array(), roofmeta);
                        // Face the room and meet the wall tops at the eaves.
                        let thickness = Vec3::Y * SLAB_THICKNESS;
                        let (a, b, d, e) =
                            (a - thickness, b - thickness, d - thickness, e - thickness);
                        out.push_triangle(a, b, e, palette.roof_side.to_array(), roofmeta);
                        out.push_triangle(a, e, d, palette.roof_side.to_array(), roofmeta);
                    } else {
                        box3(
                            &mut out,
                            rx0,
                            rx1,
                            ry0,
                            ry1,
                            eave - SLAB_THICKNESS,
                            eave,
                            palette.roof_side.to_array(),
                            roofmeta,
                        );
                    }
                }
                let above = if f + 1 < info.floors {
                    cell(s, p, f + 1).is_some_and(|v| {
                        v.surface != Surface::Air
                            && !matches!(v.kind, TileKind::StairsDown | TileKind::StairsBoth)
                    })
                } else {
                    true
                };
                let top = z + if above { WALL_HEIGHT } else { FLOOR_H };
                // Close the space beneath a pitched roof where an irregular wing's
                // exterior wall lies inside the wing bounding rectangle.
                if bs.has_pitched_roof
                    && !cell(s, p, f + 1).is_some_and(|v| v.building_id == c.building_id)
                    && matches!(
                        c.kind,
                        TileKind::ExteriorWall
                            | TileKind::Window
                            | TileKind::BrokenWindow
                            | TileKind::Door { .. }
                    )
                {
                    let wing = info.wing_at(info.wing_index_at(p, f));
                    let rise = |vx: f32, vy: f32| {
                        let (left, right) = (wing.min_x as f32, wing.max_x as f32);
                        let (near, far) = (wing.min_y as f32, wing.max_y as f32);
                        let dx = ((vx - left).min(right - vx) / ((right - left) * 0.5).max(1.0))
                            .clamp(0.0, 1.0);
                        let dy = ((vy - near).min(far - vy) / ((far - near) * 0.5).max(1.0))
                            .clamp(0.0, 1.0);
                        0.95 * dx.min(dy)
                    };
                    let segments: &[(f32, f32)] = match c.orientation {
                        WallOrientation::AlongY => &[(0.0, 1.0)],
                        WallOrientation::AlongX => &[(1.0, 0.0)],
                        WallOrientation::Corner => &[(1.0, 0.0), (0.0, 1.0)],
                    };
                    for &(dx, dy) in segments {
                        let a = Vec3::new(x - dx * 0.5, top, y - dy * 0.5);
                        let b = Vec3::new(x + dx * 0.5, top, y + dy * 0.5);
                        let c = a + Vec3::Y * rise(a.x, a.z);
                        let d = b + Vec3::Y * rise(b.x, b.z);
                        let m = wallmeta.with_tag(GeometryTag::WallUpper);
                        if d.y > top + 1e-5 {
                            out.push_triangle(a, b, d, color, m);
                            out.push_triangle(a, d, b, color, m);
                        }
                        if c.y > top + 1e-5 {
                            out.push_triangle(a, d, c, color, m);
                            out.push_triangle(a, c, d, color, m);
                        }
                    }
                }
                let along_y = c.orientation == WallOrientation::AlongY;
                let rect = |a0: f32, a1: f32| {
                    if along_y {
                        (
                            x - WALL_HALF_THICKNESS,
                            x + WALL_HALF_THICKNESS,
                            y + a0,
                            y + a1,
                        )
                    } else {
                        (
                            x + a0,
                            x + a1,
                            y - WALL_HALF_THICKNESS,
                            y + WALL_HALF_THICKNESS,
                        )
                    }
                };
                let mut part = |a0, a1, z0, z1, col, m| {
                    let (a, b, c, d) = rect(a0, a1);
                    wall_box(&mut out, a, b, c, d, z0, z1, z, col, m)
                };
                match c.kind {
                    TileKind::ExteriorWall | TileKind::InteriorWall | TileKind::Pillar => {
                        if c.orientation != WallOrientation::Corner {
                            part(-0.5, 0.5, z, top, color, wallmeta)
                        } else {
                            let connected = |dx, dy| {
                                cell(s, Position::new(p.x + dx, p.y + dy), f).is_some_and(|n| {
                                    n.building_id == c.building_id && barrier(n.kind)
                                })
                            };
                            let (w, e, n, so) = (
                                connected(-1, 0),
                                connected(1, 0),
                                connected(0, -1),
                                connected(0, 1),
                            );
                            if w || e {
                                wall_box(
                                    &mut out,
                                    x - if w { 0.5 } else { WALL_HALF_THICKNESS },
                                    x + if e { 0.5 } else { WALL_HALF_THICKNESS },
                                    y - WALL_HALF_THICKNESS,
                                    y + WALL_HALF_THICKNESS,
                                    z,
                                    top,
                                    z,
                                    color,
                                    wallmeta,
                                )
                            }
                            if n || so {
                                wall_box(
                                    &mut out,
                                    x - WALL_HALF_THICKNESS,
                                    x + WALL_HALF_THICKNESS,
                                    y - if n { 0.5 } else { WALL_HALF_THICKNESS },
                                    y + if so { 0.5 } else { WALL_HALF_THICKNESS },
                                    z,
                                    top,
                                    z,
                                    color,
                                    wallmeta,
                                )
                            }
                            if !(w || e || n || so) {
                                wall_box(
                                    &mut out,
                                    x - 0.12,
                                    x + 0.12,
                                    y - 0.12,
                                    y + 0.12,
                                    z,
                                    top,
                                    z,
                                    color,
                                    wallmeta,
                                )
                            }
                        }
                    }
                    TileKind::Door { .. } => {
                        part(
                            -0.5,
                            0.5,
                            z + DOOR_OPENING_HEIGHT,
                            top,
                            palette.trim.to_array(),
                            wallmeta,
                        );
                        for (a, b) in [(-0.5, -0.45), (0.45, 0.5)] {
                            part(a, b, z, z + DOOR_OPENING_HEIGHT, color, wallmeta)
                        }
                    }
                    TileKind::Window | TileKind::BrokenWindow => {
                        part(-0.5, 0.5, z, z + WINDOW_SILL_HEIGHT, color, wallmeta);
                        part(
                            -0.5,
                            0.5,
                            z + WINDOW_LINTEL_Z,
                            top,
                            palette.trim.to_array(),
                            wallmeta,
                        );
                        for (a, b) in [(-0.5, -0.38), (0.38, 0.5)] {
                            part(
                                a,
                                b,
                                z + WINDOW_SILL_HEIGHT,
                                z + WINDOW_LINTEL_Z,
                                color,
                                wallmeta,
                            )
                        }
                        if c.kind == TileKind::Window {
                            part(
                                -0.38,
                                0.38,
                                z + WINDOW_SILL_HEIGHT,
                                z + WINDOW_LINTEL_Z,
                                [0.37, 0.57, 0.60, 1.0],
                                openingmeta,
                            )
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{coords::ChunkCoord, scene::build_chunk_snapshot};
    use zombiebox_core::{ActionGame, Tile3D};

    #[test]
    fn stair_mesh_has_level_treads_and_vertical_risers_for_each_orientation() {
        for low in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            for high in [(-low.0, -low.1), (low.1, -low.0), (-low.1, low.0)] {
                let stair = StairGeometry::new(Tile3D::new(0, 0, 0), 0.0, low, high);
                let mut builder = MeshBuilder::new();
                emit_stairs(&mut builder, &stair, 0.0, [1.0; 4], VertexMeta::DEFAULT);
                let mesh = builder.build();
                mesh.validate().unwrap();
                assert!(mesh.vertices.iter().any(|v| v.normal[1] > 0.99));
                assert!(mesh.vertices.iter().any(|v| v.normal[1].abs() < 0.01));
                assert!(
                    mesh.vertices
                        .iter()
                        .all(|v| v.normal[1] < 0.01 || v.normal[1] > 0.99)
                );
                assert!(mesh.vertices.iter().any(|v| v.normal[1] < -0.01));
                assert!(
                    mesh.vertices
                        .iter()
                        .any(|v| v.normal[1].abs() < 0.01 && v.position[1] < -1e-5)
                );
                assert!(mesh.vertices.iter().any(|v| v.position[1].abs() < 1e-5));
                assert!(
                    mesh.vertices
                        .iter()
                        .any(|v| (v.position[1] - FLOOR_H).abs() < 1e-5)
                );
            }
        }
    }

    #[test]
    fn pitched_roof_has_interior_facing_triangles() {
        let game = ActionGame::residential(42);
        let building = game
            .buildings()
            .iter()
            .find(|b| b.style.has_pitched_roof())
            .unwrap();
        let coord = ChunkCoord::from_pos(Position::new(building.x, building.y));
        let mesh = build_chunk_buildings_mesh(&build_chunk_snapshot(&game, coord, 1));
        let roof: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| v.unpacked_meta().tag == GeometryTag::Roof)
            .collect();
        assert!(roof.iter().any(|v| v.normal[1] > 0.0));
        assert!(roof.iter().any(|v| v.normal[1] < 0.0));
    }

    #[test]
    fn stepped_six_floor_wings_roof_each_exposed_cell_and_keep_collapses_open() {
        let game = ActionGame::residential(42);
        let mut snapshot =
            build_chunk_snapshot(&game, ChunkCoord::from_pos(Position::new(0, 0)), 1);
        let mut info = game.buildings()[0];
        info.x = 2;
        info.y = 2;
        info.width = 2;
        info.height = 1;
        info.floors = 6;
        info.wings[0] = zombiebox_core::WingRect::new(2, 2, 2, 2, 2);
        info.wings[1] = zombiebox_core::WingRect::new(3, 2, 3, 2, 6);
        info.wing_count = 2;
        snapshot.buildings = vec![crate::scene::BuildingSnapshot {
            info,
            has_pitched_roof: false,
        }];
        for tile in snapshot.tiles.iter_mut() {
            tile.floor_count = 8;
            tile.floors = [FloorCellSnapshot::default(); crate::scene::MAX_SNAPSHOT_FLOORS];
            let height = match (tile.pos.x, tile.pos.y) {
                (2, 2) => 2,
                (3, 2) => 6,
                _ => 0,
            };
            for floor in 0..height {
                tile.floors[floor].building_id = info.id;
                tile.floors[floor].surface = Surface::WoodFloor;
            }
        }
        let mesh = build_chunk_buildings_mesh(&snapshot);
        mesh.validate().unwrap();
        let roof: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| v.unpacked_meta().tag == GeometryTag::Roof)
            .collect();
        assert!(roof.iter().any(|v| v.unpacked_meta().floor == 2
            && v.unpacked_meta().building_id == info.cutaway_slot(0)));
        assert!(roof.iter().any(|v| v.unpacked_meta().floor == 6
            && v.unpacked_meta().building_id == info.cutaway_slot(1)));
        assert!(
            !roof
                .iter()
                .any(|v| (3..6).contains(&v.unpacked_meta().floor))
        );
        let tile = snapshot
            .tiles
            .iter_mut()
            .find(|t| t.pos == Position::new(3, 2))
            .unwrap();
        tile.floors[5].surface = Surface::Air;
        let mesh = build_chunk_buildings_mesh(&snapshot);
        assert!(!mesh.vertices.iter().any(|v| v.unpacked_meta().tag == GeometryTag::Roof && v.unpacked_meta().floor == 5));
    }

    #[test]
    fn residential_mesh_is_valid_and_tracks_openings_and_damage() {
        let game = ActionGame::residential(42);
        let door = game
            .buildings()
            .iter()
            .find_map(|b| {
                (b.y..b.y + b.height)
                    .flat_map(|y| (b.x..b.x + b.width).map(move |x| Position::new(x, y)))
                    .find(|&p| {
                        game.tile_render_info(p, 0)
                            .is_some_and(|c| matches!(c.kind, TileKind::Door { open: false, .. }))
                    })
            })
            .expect("residential door");
        let coord = ChunkCoord::from_pos(door);
        let mut snapshot = build_chunk_snapshot(&game, coord, 1);
        let original = build_chunk_buildings_mesh(&snapshot);
        assert!(!original.is_empty());
        original.validate().unwrap();
        assert!(
            original
                .vertices
                .iter()
                .any(|v| v.unpacked_meta().tag == GeometryTag::Roof)
        );
        let min = coord.min_tile();
        let idx = (door.y - min.y + 1) as usize * crate::scene::CHUNK_HALO_SIDE
            + (door.x - min.x + 1) as usize;
        let c = &mut snapshot.tiles[idx].floors[0];
        c.kind = TileKind::Door {
            open: true,
            exterior: true,
        };
        let opened = build_chunk_buildings_mesh(&snapshot);
        opened.validate().unwrap();
        assert_ne!(original, opened);
        let _ = Tile3D::from_pos(door, 0);
    }
}
