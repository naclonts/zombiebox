use glam::Vec3;
use zombiebox_core::{
    BRIDGE_PARAPET_HEIGHT, BRIDGE_SLAB_THICKNESS, CHUNK_SIZE, District, Surface, WATER_SURFACE_Z,
    sample_quad_00_11,
};

use crate::coords::{ChunkCoord, sim_to_render_normal, sim_xyz_to_render};
use crate::mesh::primitives::{FaceMask, emit_box_min_max, emit_grid_quad_00_11_smooth};
use crate::mesh::{GeometryTag, MaterialFlags, MeshBuffer, MeshBuilder, VertexMeta, WorldVertex};
use crate::scene::{ChunkSnapshot, ChunkTerrainSnapshot, TileSnapshot};

/// Vertical (`+Y`) bias offsets in renderer world units, calibrated above `Depth32Float`
/// precision (`~500x` ULP margin at `far = 350`) while remaining `< 0.5 px` in 2:1 isometric.
pub const ROAD_MARKING_Y_BIAS: f32 = 0.008;
pub const SIDEWALK_Y_BIAS: f32 = 0.012;
pub const SIDEWALK_DETAIL_Y_BIAS: f32 = 0.018;
pub const WATER_RIPPLE_Y_BIAS: f32 = 0.006;
pub const BRIDGE_DETAIL_Y_BIAS: f32 = 0.008;
pub const BRIDGE_ABUTMENT_Y_BIAS: f32 = 0.010;
pub const BRIDGE_PARAPET_THICKNESS: f32 = 0.12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainMeshConfig {
    /// Emit asphalt road centerlines, edge lane lines, crosswalks, and trail wheel ruts.
    pub emit_road_markings: bool,
    /// Raise sidewalks/plazas by `SIDEWALK_Y_BIAS` with closed vertical curb skirts and joints.
    pub emit_sidewalk_curbs: bool,
    /// Emit `WATER_SURFACE_Z = 0.0` water planes (including shoreline-clipped bank extensions).
    pub emit_water_planes: bool,
    /// Emit 3D `BridgeDeck` slabs, undersides, side parapets/railings, and abutment sills.
    pub emit_bridges: bool,
    /// Emit Floor-0 interior surface (`WoodFloor`/`KitchenTile`/`Carpet`) on `indoor` tiles.
    pub emit_interior_floor_preview: bool,
}

impl Default for TerrainMeshConfig {
    fn default() -> Self {
        Self {
            emit_road_markings: true,
            emit_sidewalk_curbs: true,
            emit_water_planes: true,
            emit_bridges: true,
            emit_interior_floor_preview: true,
        }
    }
}

#[inline]
const fn hash2(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

#[inline]
fn rgba8(r: u8, g: u8, b: u8, a: u8) -> [f32; 4] {
    WorldVertex::rgba8(r, g, b, a)
}

#[inline]
fn lerp_rgba8(a: [u8; 4], b: [u8; 4], t: f32) -> [f32; 4] {
    let s = t.clamp(0.0, 1.0);
    [
        (f32::from(a[0]) + (f32::from(b[0]) - f32::from(a[0])) * s) / 255.0,
        (f32::from(a[1]) + (f32::from(b[1]) - f32::from(a[1])) * s) / 255.0,
        (f32::from(a[2]) + (f32::from(b[2]) - f32::from(a[2])) * s) / 255.0,
        (f32::from(a[3]) + (f32::from(b[3]) - f32::from(a[3])) * s) / 255.0,
    ]
}

#[inline]
fn grid_vertex_xz(coord: ChunkCoord, lx: usize, ly: usize, ox: usize, oy: usize) -> (f32, f32) {
    let gx = coord.cx * CHUNK_SIZE + (lx + ox) as i32;
    let gy = coord.cy * CHUNK_SIZE + (ly + oy) as i32;
    (gx as f32 - 0.5, gy as f32 - 0.5)
}

#[inline]
fn clean_neg_zero(v: Vec3) -> Vec3 {
    Vec3::new(
        if v.x == 0.0 { 0.0 } else { v.x },
        if v.y == 0.0 { 0.0 } else { v.y },
        if v.z == 0.0 { 0.0 } else { v.z },
    )
}

#[inline]
fn grid_vertex_pos_and_normal(
    terrain: &ChunkTerrainSnapshot,
    lx: usize,
    ly: usize,
    ox: usize,
    oy: usize,
    y_bias: f32,
) -> (Vec3, Vec3) {
    let (x, z) = grid_vertex_xz(terrain.coord, lx, ly, ox, oy);
    let (sim_z, sim_n) = terrain.vertex(lx + ox, ly + oy);
    let pos = clean_neg_zero(sim_xyz_to_render(x, z, sim_z + y_bias));
    let normal = clean_neg_zero(sim_to_render_normal(sim_n));
    (pos, normal)
}

#[inline]
fn vertex_touches_water(chunk: &ChunkSnapshot, lx: usize, ly: usize, ox: usize, oy: usize) -> bool {
    let vx = (lx + ox) as i32;
    let vy = (ly + oy) as i32;
    for dy in -1..=0 {
        for dx in -1..=0 {
            if let Some(nb) = chunk.local_tile(vx + dx, vy + dy)
                && nb.water_z.is_some()
            {
                return true;
            }
        }
    }
    false
}

#[inline]
fn mountain_vertex_color(sim_z: f32, sim_n: zombiebox_core::Vec3) -> [f32; 4] {
    let t_elev = ((sim_z - 4.0) / 14.0).clamp(0.0, 1.0);
    let slope = sim_n.xy().length() / sim_n.z.max(1e-4);
    let cliff_dark = 1.0 - 0.18 * (slope / 0.8).clamp(0.0, 1.0);
    let base = if t_elev < 0.5 {
        lerp_rgba8([84, 86, 94, 255], [122, 126, 138, 255], t_elev * 2.0)
    } else {
        lerp_rgba8(
            [122, 126, 138, 255],
            [170, 174, 186, 255],
            (t_elev - 0.5) * 2.0,
        )
    };
    [
        base[0] * cliff_dark,
        base[1] * cliff_dark,
        base[2] * cliff_dark,
        1.0,
    ]
}

#[inline]
fn riverbed_vertex_color(sim_z: f32) -> [f32; 4] {
    if sim_z >= -0.25 {
        rgba8(114, 102, 80, 255)
    } else if sim_z >= -1.0 {
        let t = (-0.25 - sim_z) / 0.75;
        lerp_rgba8([114, 102, 80, 255], [64, 68, 62, 255], t)
    } else {
        let t = ((-1.0 - sim_z) / 1.5).clamp(0.0, 1.0);
        lerp_rgba8([64, 68, 62, 255], [32, 42, 48, 255], t)
    }
}

#[inline]
fn water_surface_vertex_color(corner_z: f32) -> [f32; 4] {
    let depth = (WATER_SURFACE_Z - corner_z).max(0.0);
    if depth <= 0.28 {
        lerp_rgba8([148, 202, 218, 255], [74, 134, 160, 255], depth / 0.28)
    } else if depth <= 1.4 {
        lerp_rgba8(
            [74, 134, 160, 255],
            [38, 76, 98, 255],
            (depth - 0.28) / 1.12,
        )
    } else {
        lerp_rgba8([38, 76, 98, 255], [22, 48, 70, 255], (depth - 1.4) / 1.0)
    }
}

#[inline]
fn flat_surface_color(surface: Surface, district: District, tx: i32, ty: i32) -> [f32; 4] {
    match surface {
        Surface::Grass => {
            let v = (hash2(tx.div_euclid(3), ty.div_euclid(3)) % 9) as u8;
            rgba8(62 + v, 86 + v, 66 + v / 2, 255)
        }
        Surface::Forest => {
            let v = (hash2(tx.div_euclid(2), ty.div_euclid(2)) % 8) as u8;
            rgba8(40 + v, 66 + v, 50 + v / 2, 255)
        }
        Surface::Field => {
            let v = (hash2(tx.div_euclid(3), ty.div_euclid(3)) % 7) as u8;
            rgba8(88 + v, 86 + v, 56 + v / 2, 255)
        }
        Surface::Gravel => {
            if district == District::MilitaryBase {
                let v = (hash2(tx, ty) % 7) as u8;
                rgba8(86 + v, 92 + v, 78 + v, 255)
            } else {
                let v = (hash2(tx, ty) % 8) as u8;
                rgba8(92 + v, 98 + v, 86 + v, 255)
            }
        }
        Surface::Trail => {
            let v = (hash2(tx, ty) % 7) as u8;
            rgba8(112 + v, 94 + v, 68 + v / 2, 255)
        }
        Surface::Road => {
            let v = (hash2(tx, ty) % 6) as u8;
            rgba8(54 + v, 64 + v, 74 + v, 255)
        }
        Surface::Driveway => {
            if district == District::MilitaryBase {
                let v = (hash2(tx, ty) % 5) as u8;
                rgba8(100 + v, 106 + v, 102 + v, 255)
            } else {
                let v = (hash2(tx, ty) % 6) as u8;
                rgba8(102 + v, 106 + v, 104 + v, 255)
            }
        }
        Surface::Sidewalk => match district {
            District::City => {
                let v = (hash2(tx, ty) % 6) as u8;
                rgba8(118 + v, 122 + v, 124 + v, 255)
            }
            District::Hospital => {
                let v = (hash2(tx, ty) % 4) as u8;
                rgba8(138 + v, 146 + v, 144 + v, 255)
            }
            District::Police => {
                let v = (hash2(tx, ty) % 5) as u8;
                rgba8(116 + v, 124 + v, 134 + v, 255)
            }
            _ => {
                let v = (hash2(tx, ty) % 5) as u8;
                rgba8(124 + v, 128 + v, 122 + v, 255)
            }
        },
        Surface::Porch => {
            let v = (hash2(tx, ty) % 6) as u8;
            rgba8(144 + v, 114 + v, 84 + v, 255)
        }
        Surface::WoodFloor => {
            let v = (hash2(tx, ty) % 5) as u8;
            rgba8(134 + v, 104 + v, 74 + v, 255)
        }
        Surface::KitchenTile => rgba8(164, 168, 158, 255),
        Surface::Carpet => rgba8(98, 108, 126, 255),
        _ => rgba8(86, 84, 80, 255),
    }
}

/// Clips a convex polygon in `(u, v)` space against the half-plane `sign * (u - v) >= 0`.
fn clip_uv_halfplane(poly: &[(f32, f32)], sign: f32) -> Vec<(f32, f32)> {
    if poly.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(poly.len() + 2);
    let eval = |p: (f32, f32)| sign * (p.0 - p.1);

    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let da = eval(a);
        let db = eval(b);
        let in_a = da >= -1e-6;
        let in_b = db >= -1e-6;

        if in_a {
            out.push(a);
            if !in_b {
                let denom = da - db;
                if denom.abs() > 1e-8 {
                    let t = (da / denom).clamp(0.0, 1.0);
                    out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
                }
            }
        } else if in_b {
            let denom = da - db;
            if denom.abs() > 1e-8 {
                let t = (da / denom).clamp(0.0, 1.0);
                out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            }
        }
    }

    // Deduplicate near-identical consecutive points
    let mut dedup: Vec<(f32, f32)> = Vec::with_capacity(out.len());
    for p in out {
        if let Some(&last) = dedup.last()
            && (p.0 - last.0).abs() < 1e-6
            && (p.1 - last.1).abs() < 1e-6
        {
            continue;
        }
        dedup.push(p);
    }
    if dedup.len() >= 2 {
        let first = dedup[0];
        let last = *dedup.last().unwrap();
        if (first.0 - last.0).abs() < 1e-6 && (first.1 - last.1).abs() < 1e-6 {
            dedup.pop();
        }
    }
    dedup
}

#[allow(clippy::too_many_arguments)]
fn emit_sloped_subquad(
    b: &mut MeshBuilder,
    tx: i32,
    ty: i32,
    quad_z: [f32; 4],
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    y_bias: f32,
    color: [f32; 4],
    meta: VertexMeta,
) {
    if u1 - u0 <= 1e-5 || v1 - v0 <= 1e-5 {
        return;
    }
    // CCW order in renderer (+Y up) for (u, v): (u0,v0) -> (u0,v1) -> (u1,v1) -> (u1,v0)
    let rect = [(u0, v0), (u0, v1), (u1, v1), (u1, v0)];
    let base_x = tx as f32 - 0.5;
    let base_y = ty as f32 - 0.5;
    let [z00, z10, z01, z11] = quad_z;

    for (sign, tri_u, tri_v) in [
        (1.0_f32, 0.75_f32, 0.25_f32),
        (-1.0_f32, 0.25_f32, 0.75_f32),
    ] {
        let poly = clip_uv_halfplane(&rect, sign);
        if poly.len() < 3 {
            continue;
        }
        let (_, sim_n, _) = sample_quad_00_11(z00, z10, z01, z11, tri_u, tri_v);
        let norm = clean_neg_zero(sim_to_render_normal(sim_n));

        let to_pos = |uv: (f32, f32)| -> Vec3 {
            let (z, _, _) = sample_quad_00_11(z00, z10, z01, z11, uv.0, uv.1);
            clean_neg_zero(sim_xyz_to_render(base_x + uv.0, base_y + uv.1, z + y_bias))
        };

        let p0 = to_pos(poly[0]);
        for i in 1..(poly.len() - 1) {
            let p1 = to_pos(poly[i]);
            let p2 = to_pos(poly[i + 1]);
            let cross = (p1 - p0).cross(p2 - p0);
            if cross.length_squared() <= 1e-12 {
                continue;
            }
            let base_idx = b.push_vertex(WorldVertex::with_meta(p0, norm, color, meta));
            let i1 = b.push_vertex(WorldVertex::with_meta(p1, norm, color, meta));
            let i2 = b.push_vertex(WorldVertex::with_meta(p2, norm, color, meta));
            b.as_mesh_mut()
                .indices
                .extend_from_slice(&[base_idx, i1, i2]);
        }
    }
}

#[inline]
fn tile_terrain_quad_00_10_01_11(terrain: &ChunkTerrainSnapshot, lx: usize, ly: usize) -> [f32; 4] {
    [
        terrain.heights[ly][lx],
        terrain.heights[ly][lx + 1],
        terrain.heights[ly + 1][lx],
        terrain.heights[ly + 1][lx + 1],
    ]
}

#[inline]
fn is_road_conn(tile: Option<&TileSnapshot>) -> bool {
    tile.is_some_and(|t| matches!(t.ground_surface(), Surface::Road | Surface::Bridge))
}

#[inline]
fn is_raised_sidewalk(tile: Option<&TileSnapshot>) -> bool {
    tile.is_some_and(|t| {
        t.floor_count > 0
            && t.floors[0].surface == Surface::Sidewalk
            && t.floors[0].building_id == 0
    })
}

#[inline]
fn is_bridge_approach(tile: Option<&TileSnapshot>) -> bool {
    tile.is_some_and(|t| {
        t.bridge_quad.is_some()
            || matches!(
                t.ground_surface(),
                Surface::Road | Surface::Trail | Surface::Sidewalk
            )
    })
}

#[inline]
fn nearest_exterior_surface(chunk: &ChunkSnapshot, lx: i32, ly: i32) -> Surface {
    for (dx, dy) in [(0, 1), (1, 0), (0, -1), (-1, 0)] {
        if let Some(nb) = chunk.local_tile(lx + dx, ly + dy)
            && nb.floor_count > 0
            && nb.floors[0].building_id == 0
            && nb.floors[0].surface != Surface::Air
        {
            return nb.floors[0].surface;
        }
    }
    Surface::Grass
}

/// Emits a vertical CCW quad along one cardinal tile edge from `(za0, zb0)` up to `(za1, zb1)`.
#[allow(clippy::too_many_arguments)]
fn emit_cardinal_vertical_skirt(
    b: &mut MeshBuilder,
    coord: ChunkCoord,
    lx: usize,
    ly: usize,
    dx: i32,
    dy: i32,
    za0: f32,
    za1: f32,
    zb0: f32,
    zb1: f32,
    color: [f32; 4],
    meta: VertexMeta,
) {
    let (x0, z0) = grid_vertex_xz(coord, lx, ly, 0, 0);
    let (x1, z1) = grid_vertex_xz(coord, lx, ly, 1, 1);

    // For each edge: p0 -> p1 runs along bottom, p2 -> p3 runs along top so (p1-p0)x(p2-p0) == outward
    let (p0, p1, p2, p3) = match (dx, dy) {
        (-1, 0) => (
            Vec3::new(x0, za0, z0),
            Vec3::new(x0, zb0, z1),
            Vec3::new(x0, zb1, z1),
            Vec3::new(x0, za1, z0),
        ),
        (1, 0) => (
            Vec3::new(x1, zb0, z1),
            Vec3::new(x1, za0, z0),
            Vec3::new(x1, za1, z0),
            Vec3::new(x1, zb1, z1),
        ),
        (0, -1) => (
            Vec3::new(x1, zb0, z0),
            Vec3::new(x0, za0, z0),
            Vec3::new(x0, za1, z0),
            Vec3::new(x1, zb1, z0),
        ),
        (0, 1) => (
            Vec3::new(x0, za0, z1),
            Vec3::new(x1, zb0, z1),
            Vec3::new(x1, zb1, z1),
            Vec3::new(x0, za1, z1),
        ),
        _ => return,
    };

    b.push_quad(p0, p1, p2, p3, color, meta);
}

/// Emits a closed 6-face rectangular prism along a sloped cardinal edge (`along x Y == outward`).
#[allow(clippy::too_many_arguments)]
fn emit_sloped_edge_prism(
    b: &mut MeshBuilder,
    a_xz: (f32, f32),
    b_xz: (f32, f32),
    inward_xz: (f32, f32),
    thickness: f32,
    za_lo: f32,
    za_hi: f32,
    zb_lo: f32,
    zb_hi: f32,
    color: [f32; 4],
    meta: VertexMeta,
) {
    let a_out = Vec3::new(a_xz.0, 0.0, a_xz.1);
    let b_out = Vec3::new(b_xz.0, 0.0, b_xz.1);
    let in_vec = Vec3::new(inward_xz.0 * thickness, 0.0, inward_xz.1 * thickness);

    let a_out_lo = a_out + Vec3::Y * za_lo;
    let a_out_hi = a_out + Vec3::Y * za_hi;
    let b_out_lo = b_out + Vec3::Y * zb_lo;
    let b_out_hi = b_out + Vec3::Y * zb_hi;

    let a_in_lo = a_out_lo + in_vec;
    let a_in_hi = a_out_hi + in_vec;
    let b_in_lo = b_out_lo + in_vec;
    let b_in_hi = b_out_hi + in_vec;

    // Outward, inward, top, bottom, start cap, end cap
    b.push_quad(a_out_lo, b_out_lo, b_out_hi, a_out_hi, color, meta);
    b.push_quad(b_in_lo, a_in_lo, a_in_hi, b_in_hi, color, meta);
    b.push_quad(a_out_hi, b_out_hi, b_in_hi, a_in_hi, color, meta);
    b.push_quad(a_in_lo, b_in_lo, b_out_lo, a_out_lo, color, meta);
    b.push_quad(a_in_lo, a_out_lo, a_out_hi, a_in_hi, color, meta);
    b.push_quad(b_out_lo, b_in_lo, b_in_hi, b_out_hi, color, meta);
}

#[allow(clippy::too_many_arguments)]
fn emit_road_overlays(
    b: &mut MeshBuilder,
    chunk: &ChunkSnapshot,
    lx: usize,
    ly: usize,
    tx: i32,
    ty: i32,
    quad_z: [f32; 4],
    district: District,
    meta: VertexMeta,
) {
    let lxi = lx as i32;
    let lyi = ly as i32;
    let n_road = is_road_conn(chunk.local_tile(lxi, lyi - 1));
    let s_road = is_road_conn(chunk.local_tile(lxi, lyi + 1));
    let w_road = is_road_conn(chunk.local_tile(lxi - 1, lyi));
    let e_road = is_road_conn(chunk.local_tile(lxi + 1, lyi));

    let along_x = (w_road || e_road) && !(n_road && s_road);
    let along_y = (n_road || s_road) && !(w_road && e_road);

    let yellow = rgba8(212, 182, 106, 255);
    if along_x && tx.rem_euclid(3) < 2 {
        emit_sloped_subquad(
            b,
            tx,
            ty,
            quad_z,
            0.14,
            0.45,
            0.86,
            0.55,
            ROAD_MARKING_Y_BIAS,
            yellow,
            meta,
        );
    }
    if along_y && ty.rem_euclid(3) < 2 {
        emit_sloped_subquad(
            b,
            tx,
            ty,
            quad_z,
            0.45,
            0.14,
            0.55,
            0.86,
            ROAD_MARKING_Y_BIAS,
            yellow,
            meta,
        );
    }

    // Multi-tile County street / highway edge lane stripes
    let is_urban = matches!(
        district,
        District::Residential | District::City | District::Hospital | District::Police
    );
    let edge_color = if is_urban {
        rgba8(146, 150, 148, 255)
    } else {
        rgba8(168, 156, 112, 255)
    };
    if w_road && e_road {
        if !n_road {
            emit_sloped_subquad(
                b,
                tx,
                ty,
                quad_z,
                0.0,
                0.06,
                1.0,
                0.13,
                ROAD_MARKING_Y_BIAS,
                edge_color,
                meta,
            );
        }
        if !s_road {
            emit_sloped_subquad(
                b,
                tx,
                ty,
                quad_z,
                0.0,
                0.87,
                1.0,
                0.94,
                ROAD_MARKING_Y_BIAS,
                edge_color,
                meta,
            );
        }
    }
    if n_road && s_road {
        if !w_road {
            emit_sloped_subquad(
                b,
                tx,
                ty,
                quad_z,
                0.06,
                0.0,
                0.13,
                1.0,
                ROAD_MARKING_Y_BIAS,
                edge_color,
                meta,
            );
        }
        if !e_road {
            emit_sloped_subquad(
                b,
                tx,
                ty,
                quad_z,
                0.87,
                0.0,
                0.94,
                1.0,
                ROAD_MARKING_Y_BIAS,
                edge_color,
                meta,
            );
        }
    }

    // Halo-safe white zebra crosswalks
    let white = rgba8(204, 206, 202, 255);
    if along_x && !n_road && !s_road {
        if w_road
            && (is_road_conn(chunk.local_tile(lxi - 1, lyi - 1))
                || is_road_conn(chunk.local_tile(lxi - 1, lyi + 1)))
        {
            emit_crosswalk(b, tx, ty, quad_z, true, 0.08, 0.38, white, meta);
        }
        if e_road
            && (is_road_conn(chunk.local_tile(lxi + 1, lyi - 1))
                || is_road_conn(chunk.local_tile(lxi + 1, lyi + 1)))
        {
            emit_crosswalk(b, tx, ty, quad_z, true, 0.62, 0.92, white, meta);
        }
    } else if along_y && !w_road && !e_road {
        if n_road
            && (is_road_conn(chunk.local_tile(lxi - 1, lyi - 1))
                || is_road_conn(chunk.local_tile(lxi + 1, lyi - 1)))
        {
            emit_crosswalk(b, tx, ty, quad_z, false, 0.08, 0.38, white, meta);
        }
        if s_road
            && (is_road_conn(chunk.local_tile(lxi - 1, lyi + 1))
                || is_road_conn(chunk.local_tile(lxi + 1, lyi + 1)))
        {
            emit_crosswalk(b, tx, ty, quad_z, false, 0.62, 0.92, white, meta);
        }
    } else if is_urban {
        if w_road && e_road && (!n_road || !s_road) {
            let check_dy = if !n_road { -1 } else { 1 };
            if is_road_conn(chunk.local_tile(lxi - 1, lyi + check_dy)) {
                emit_crosswalk(b, tx, ty, quad_z, true, 0.08, 0.38, white, meta);
            }
            if is_road_conn(chunk.local_tile(lxi + 1, lyi + check_dy)) {
                emit_crosswalk(b, tx, ty, quad_z, true, 0.62, 0.92, white, meta);
            }
        } else if n_road && s_road && (!w_road || !e_road) {
            let check_dx = if !w_road { -1 } else { 1 };
            if is_road_conn(chunk.local_tile(lxi + check_dx, lyi - 1)) {
                emit_crosswalk(b, tx, ty, quad_z, false, 0.08, 0.38, white, meta);
            }
            if is_road_conn(chunk.local_tile(lxi + check_dx, lyi + 1)) {
                emit_crosswalk(b, tx, ty, quad_z, false, 0.62, 0.92, white, meta);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_crosswalk(
    b: &mut MeshBuilder,
    tx: i32,
    ty: i32,
    quad_z: [f32; 4],
    bars_along_x: bool,
    band_start: f32,
    band_end: f32,
    color: [f32; 4],
    meta: VertexMeta,
) {
    for (s0, s1) in [(0.12_f32, 0.26_f32), (0.43, 0.57), (0.74, 0.88)] {
        if bars_along_x {
            emit_sloped_subquad(
                b,
                tx,
                ty,
                quad_z,
                band_start,
                s0,
                band_end,
                s1,
                ROAD_MARKING_Y_BIAS,
                color,
                meta,
            );
        } else {
            emit_sloped_subquad(
                b,
                tx,
                ty,
                quad_z,
                s0,
                band_start,
                s1,
                band_end,
                ROAD_MARKING_Y_BIAS,
                color,
                meta,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_sidewalk_tile(
    b: &mut MeshBuilder,
    chunk: &ChunkSnapshot,
    lx: usize,
    ly: usize,
    tx: i32,
    ty: i32,
    quad_z: [f32; 4],
    district: District,
) {
    let terrain = &chunk.terrain;
    let meta = VertexMeta::DEFAULT.with_tag(GeometryTag::Road);
    let top_color = flat_surface_color(Surface::Sidewalk, district, tx, ty);

    let make_top = |ox: usize, oy: usize| -> WorldVertex {
        let (pos, norm) = grid_vertex_pos_and_normal(terrain, lx, ly, ox, oy, SIDEWALK_Y_BIAS);
        WorldVertex::with_meta(pos, norm, top_color, meta)
    };
    emit_grid_quad_00_11_smooth(
        b,
        make_top(0, 0),
        make_top(1, 0),
        make_top(1, 1),
        make_top(0, 1),
    );

    let lxi = lx as i32;
    let lyi = ly as i32;
    let [z00, z10, z01, z11] = quad_z;
    let curb_lip_color = rgba8(176, 178, 164, 255);

    for (dx, dy, za, zb) in [
        (-1, 0, z00, z01),
        (1, 0, z10, z11),
        (0, -1, z10, z00),
        (0, 1, z01, z11),
    ] {
        let nb = chunk.local_tile(lxi + dx, lyi + dy);
        if !is_raised_sidewalk(nb) {
            let nb_is_road = nb.is_some_and(|n| n.ground_surface() == Surface::Road);
            let skirt_color = if nb_is_road {
                rgba8(112, 116, 112, 255)
            } else {
                rgba8(90, 94, 92, 255)
            };
            emit_cardinal_vertical_skirt(
                b,
                chunk.coord,
                lx,
                ly,
                dx,
                dy,
                za,
                za + SIDEWALK_Y_BIAS,
                zb,
                zb + SIDEWALK_Y_BIAS,
                skirt_color,
                meta,
            );
            if nb_is_road {
                let (u0, v0, u1, v1) = match (dx, dy) {
                    (-1, 0) => (0.0, 0.0, 0.09, 1.0),
                    (1, 0) => (0.91, 0.0, 1.0, 1.0),
                    (0, -1) => (0.0, 0.0, 1.0, 0.09),
                    _ => (0.0, 0.91, 1.0, 1.0),
                };
                emit_sloped_subquad(
                    b,
                    tx,
                    ty,
                    quad_z,
                    u0,
                    v0,
                    u1,
                    v1,
                    SIDEWALK_DETAIL_Y_BIAS,
                    curb_lip_color,
                    meta,
                );
            }
        }
    }

    // Expansion joint seams & district plaza borders
    if matches!(district, District::Hospital) {
        emit_sloped_subquad(
            b,
            tx,
            ty,
            quad_z,
            0.0,
            0.0,
            1.0,
            0.04,
            SIDEWALK_DETAIL_Y_BIAS,
            rgba8(96, 134, 132, 255),
            meta,
        );
    } else if tx.rem_euclid(2) == 0 || matches!(district, District::City | District::Police) {
        emit_sloped_subquad(
            b,
            tx,
            ty,
            quad_z,
            0.48,
            0.04,
            0.52,
            0.96,
            SIDEWALK_DETAIL_Y_BIAS,
            rgba8(84, 88, 86, 255),
            meta,
        );
    }
}

fn emit_bridge_tile(
    b: &mut MeshBuilder,
    chunk: &ChunkSnapshot,
    lx: usize,
    ly: usize,
    tx: i32,
    ty: i32,
    bq: [f32; 4],
) {
    let meta = VertexMeta::DEFAULT.with_tag(GeometryTag::Bridge);
    let [z00, z10, z01, z11] = bq;
    let (x0, y0) = grid_vertex_xz(chunk.coord, lx, ly, 0, 0);
    let (x1, y1) = grid_vertex_xz(chunk.coord, lx, ly, 1, 1);

    let p00 = sim_xyz_to_render(x0, y0, z00);
    let p10 = sim_xyz_to_render(x1, y0, z10);
    let p11 = sim_xyz_to_render(x1, y1, z11);
    let p01 = sim_xyz_to_render(x0, y1, z01);

    let v = (hash2(tx, ty) % 6) as u8;
    let deck_color = rgba8(132 + v, 106 + v, 74 + v, 255);

    let (_, sim_n_ul, _) = sample_quad_00_11(z00, z10, z01, z11, 0.25, 0.75);
    let (_, sim_n_lr, _) = sample_quad_00_11(z00, z10, z01, z11, 0.75, 0.25);
    let n_ul = sim_to_render_normal(sim_n_ul);
    let n_lr = sim_to_render_normal(sim_n_lr);

    // Top deck: 00 -> 11 diagonal split
    let push_tri = |b: &mut MeshBuilder, a: Vec3, c: Vec3, d: Vec3, n: Vec3, col: [f32; 4]| {
        let i0 = b.push_vertex(WorldVertex::with_meta(a, n, col, meta));
        let i1 = b.push_vertex(WorldVertex::with_meta(c, n, col, meta));
        let i2 = b.push_vertex(WorldVertex::with_meta(d, n, col, meta));
        b.as_mesh_mut().indices.extend_from_slice(&[i0, i1, i2]);
    };
    push_tri(b, p00, p01, p11, n_ul, deck_color);
    push_tri(b, p00, p11, p10, n_lr, deck_color);

    // Underside (-Y)
    let bot_color = rgba8(72, 58, 44, 255);
    let t = BRIDGE_SLAB_THICKNESS;
    let b00 = sim_xyz_to_render(x0, y0, z00 - t);
    let b10 = sim_xyz_to_render(x1, y0, z10 - t);
    let b11 = sim_xyz_to_render(x1, y1, z11 - t);
    let b01 = sim_xyz_to_render(x0, y1, z01 - t);
    push_tri(b, b00, b11, b01, -n_ul, bot_color);
    push_tri(b, b00, b10, b11, -n_lr, bot_color);

    // Deck plank seams oriented perpendicular to bridge span
    let lxi = lx as i32;
    let lyi = ly as i32;
    let n_span = is_road_conn(chunk.local_tile(lxi, lyi - 1));
    let s_span = is_road_conn(chunk.local_tile(lxi, lyi + 1));
    let span_y = n_span || s_span;
    let plank_color = rgba8(78, 60, 42, 255);
    for c in [0.16_f32, 0.50, 0.84] {
        if span_y {
            emit_sloped_subquad(
                b,
                tx,
                ty,
                bq,
                0.06,
                c - 0.025,
                0.94,
                c + 0.025,
                BRIDGE_DETAIL_Y_BIAS,
                plank_color,
                meta,
            );
        } else {
            emit_sloped_subquad(
                b,
                tx,
                ty,
                bq,
                c - 0.025,
                0.06,
                c + 0.025,
                0.94,
                BRIDGE_DETAIL_Y_BIAS,
                plank_color,
                meta,
            );
        }
    }

    // Perimeter slab fascia + guardrails / abutment strips
    let post_color = rgba8(94, 74, 54, 255);
    let mid_rail_color = rgba8(124, 98, 68, 255);
    let top_rail_color = rgba8(172, 144, 108, 255);
    let abutment_color = rgba8(114, 116, 122, 255);

    // Cardinal edges ordered so along x Y == outward
    let edges = [
        (-1, 0, (x0, y0), (x0, y1), (1.0_f32, 0.0_f32), z00, z01),
        (1, 0, (x1, y1), (x1, y0), (-1.0_f32, 0.0_f32), z11, z10),
        (0, -1, (x1, y0), (x0, y0), (0.0_f32, 1.0_f32), z10, z00),
        (0, 1, (x0, y1), (x1, y1), (0.0_f32, -1.0_f32), z01, z11),
    ];

    for (dx, dy, a_xz, b_xz, inward_xz, za, zb) in edges {
        let nb = chunk.local_tile(lxi + dx, lyi + dy);
        if nb.is_some_and(|n| n.bridge_quad.is_some()) {
            continue;
        }

        let is_approach = is_bridge_approach(nb);
        let fascia_color = if is_approach {
            rgba8(96, 98, 104, 255)
        } else {
            rgba8(86, 68, 50, 255)
        };
        emit_cardinal_vertical_skirt(
            b,
            chunk.coord,
            lx,
            ly,
            dx,
            dy,
            za - t,
            za,
            zb - t,
            zb,
            fascia_color,
            meta,
        );

        if is_approach {
            let (u0, v0, u1, v1) = match (dx, dy) {
                (-1, 0) => (0.0, 0.0, 0.14, 1.0),
                (1, 0) => (0.86, 0.0, 1.0, 1.0),
                (0, -1) => (0.0, 0.0, 1.0, 0.14),
                _ => (0.0, 0.86, 1.0, 1.0),
            };
            emit_sloped_subquad(
                b,
                tx,
                ty,
                bq,
                u0,
                v0,
                u1,
                v1,
                BRIDGE_ABUTMENT_Y_BIAS,
                abutment_color,
                meta,
            );
        } else {
            // Curb sill
            emit_sloped_edge_prism(
                b,
                a_xz,
                b_xz,
                inward_xz,
                BRIDGE_PARAPET_THICKNESS,
                za,
                za + 0.09,
                zb,
                zb + 0.09,
                post_color,
                meta,
            );
            // Mid rail
            emit_sloped_edge_prism(
                b,
                a_xz,
                b_xz,
                inward_xz,
                0.06,
                za + 0.21,
                za + 0.27,
                zb + 0.21,
                zb + 0.27,
                mid_rail_color,
                meta,
            );
            // Top rail
            emit_sloped_edge_prism(
                b,
                a_xz,
                b_xz,
                inward_xz,
                0.10,
                za + 0.38,
                za + BRIDGE_PARAPET_HEIGHT,
                zb + 0.38,
                zb + BRIDGE_PARAPET_HEIGHT,
                top_rail_color,
                meta,
            );
            // 3 vertical guardrail posts at t = 0.10, 0.50, 0.90
            for t_pos in [0.10_f32, 0.50, 0.90] {
                let px = a_xz.0 + (b_xz.0 - a_xz.0) * t_pos + inward_xz.0 * 0.045;
                let pz = a_xz.1 + (b_xz.1 - a_xz.1) * t_pos + inward_xz.1 * 0.045;
                let pz_base = za + (zb - za) * t_pos;
                emit_box_min_max(
                    b,
                    Vec3::new(px - 0.045, pz_base, pz - 0.045),
                    Vec3::new(px + 0.045, pz_base + BRIDGE_PARAPET_HEIGHT, pz + 0.045),
                    FaceMask::ALL,
                    post_color,
                    meta,
                );
            }
        }
    }
}

#[must_use]
pub fn build_chunk_terrain_mesh(chunk: &ChunkSnapshot) -> MeshBuffer {
    build_chunk_terrain_mesh_with_config(chunk, &TerrainMeshConfig::default())
}

#[must_use]
pub fn build_chunk_terrain_mesh_with_config(
    chunk: &ChunkSnapshot,
    config: &TerrainMeshConfig,
) -> MeshBuffer {
    let mut builder = MeshBuilder::with_capacity(2048, 4096);
    let terrain = &chunk.terrain;

    for ly in 0..16 {
        for lx in 0..16 {
            let Some(tile) = chunk.local_tile(lx as i32, ly as i32) else {
                continue;
            };
            if tile.floor_count == 0 || tile.ground_surface() == Surface::Air {
                continue;
            }

            let tx = chunk.coord.cx * CHUNK_SIZE + lx as i32;
            let ty = chunk.coord.cy * CHUNK_SIZE + ly as i32;
            let cell = tile.floors[0];
            let raw_surface = cell.surface;
            let quad_z = tile_terrain_quad_00_10_01_11(terrain, lx, ly);

            // Resolve Case 1 (building perimeter wall) vs Case 2..5
            let is_perimeter_wall =
                cell.building_id > 0 && !cell.indoor && raw_surface != Surface::Driveway;
            let is_interior = cell.building_id > 0 && cell.indoor;
            let interior_meta = VertexMeta::DEFAULT
                .with_tag(GeometryTag::InteriorFloor)
                .with_floor(0)
                .with_flags(MaterialFlags::INDOOR_AMBIENT)
                .with_building(cell.building_id, 0);

            let effective_surface = if is_perimeter_wall {
                nearest_exterior_surface(chunk, lx as i32, ly as i32)
            } else {
                raw_surface
            };

            let (v00_pos, v00_norm) = grid_vertex_pos_and_normal(terrain, lx, ly, 0, 0, 0.0);
            let (v10_pos, v10_norm) = grid_vertex_pos_and_normal(terrain, lx, ly, 1, 0, 0.0);
            let (v11_pos, v11_norm) = grid_vertex_pos_and_normal(terrain, lx, ly, 1, 1, 0.0);
            let (v01_pos, v01_norm) = grid_vertex_pos_and_normal(terrain, lx, ly, 0, 1, 0.0);

            // Case 3: Submerged riverbed / lakebed under Water or Bridge
            if tile.water_z.is_some()
                || matches!(effective_surface, Surface::Water | Surface::Bridge)
            {
                let bed_meta = VertexMeta::DEFAULT.with_tag(GeometryTag::Terrain);
                let make_bed = |ox: usize, oy: usize, pos: Vec3, norm: Vec3| -> WorldVertex {
                    let (sim_z, _) = terrain.vertex(lx + ox, ly + oy);
                    WorldVertex::with_meta(pos, norm, riverbed_vertex_color(sim_z), bed_meta)
                };
                emit_grid_quad_00_11_smooth(
                    &mut builder,
                    make_bed(0, 0, v00_pos, v00_norm),
                    make_bed(1, 0, v10_pos, v10_norm),
                    make_bed(1, 1, v11_pos, v11_norm),
                    make_bed(0, 1, v01_pos, v01_norm),
                );
            } else if effective_surface == Surface::Sidewalk
                && !is_perimeter_wall
                && config.emit_sidewalk_curbs
            {
                // Case 4: Raised Sidewalk / Plaza
                emit_sidewalk_tile(&mut builder, chunk, lx, ly, tx, ty, quad_z, tile.district);
            } else if is_interior
                && effective_surface == Surface::KitchenTile
                && config.emit_interior_floor_preview
            {
                // Case 2: 2x2 checkered linoleum KitchenTile preview
                let meta = interior_meta;
                let light_c = rgba8(164, 168, 158, 255);
                let dark_c = rgba8(112, 128, 126, 255);
                for su in 0..2 {
                    for sv in 0..2 {
                        let col = if (su + sv) % 2 == 0 { light_c } else { dark_c };
                        emit_sloped_subquad(
                            &mut builder,
                            tx,
                            ty,
                            quad_z,
                            su as f32 * 0.5,
                            sv as f32 * 0.5,
                            (su + 1) as f32 * 0.5,
                            (sv + 1) as f32 * 0.5,
                            0.0,
                            col,
                            meta,
                        );
                    }
                }
            } else {
                // Case 1, Case 2, Case 5: Dry terrain / road / mountain / building pad
                let tag = match effective_surface {
                    Surface::Road | Surface::Trail | Surface::Driveway => GeometryTag::Road,
                    Surface::Sidewalk if is_perimeter_wall => GeometryTag::Road,
                    _ => GeometryTag::Terrain,
                };
                let meta = if is_interior {
                    interior_meta
                } else {
                    VertexMeta::DEFAULT.with_tag(tag)
                };

                if effective_surface == Surface::Mountain {
                    let make_m = |ox: usize, oy: usize, pos: Vec3, norm: Vec3| -> WorldVertex {
                        let (sim_z, sim_n) = terrain.vertex(lx + ox, ly + oy);
                        WorldVertex::with_meta(pos, norm, mountain_vertex_color(sim_z, sim_n), meta)
                    };
                    emit_grid_quad_00_11_smooth(
                        &mut builder,
                        make_m(0, 0, v00_pos, v00_norm),
                        make_m(1, 0, v10_pos, v10_norm),
                        make_m(1, 1, v11_pos, v11_norm),
                        make_m(0, 1, v01_pos, v01_norm),
                    );
                } else {
                    let base_color = if is_interior && !config.emit_interior_floor_preview {
                        rgba8(86, 84, 80, 255)
                    } else {
                        flat_surface_color(effective_surface, tile.district, tx, ty)
                    };

                    // Shoreline bank tint for dry corners touching water
                    let make_dry = |ox: usize, oy: usize, pos: Vec3, norm: Vec3| -> WorldVertex {
                        let (sim_z, _) = terrain.vertex(lx + ox, ly + oy);
                        let col = if sim_z < 0.28
                            && !matches!(effective_surface, Surface::Road | Surface::Driveway)
                            && vertex_touches_water(chunk, lx, ly, ox, oy)
                        {
                            let t = ((0.28 - sim_z) / 0.44).clamp(0.0, 1.0);
                            let bank = rgba8(104, 96, 76, 255);
                            [
                                base_color[0] + (bank[0] - base_color[0]) * t,
                                base_color[1] + (bank[1] - base_color[1]) * t,
                                base_color[2] + (bank[2] - base_color[2]) * t,
                                1.0,
                            ]
                        } else {
                            base_color
                        };
                        WorldVertex::with_meta(pos, norm, col, meta)
                    };
                    emit_grid_quad_00_11_smooth(
                        &mut builder,
                        make_dry(0, 0, v00_pos, v00_norm),
                        make_dry(1, 0, v10_pos, v10_norm),
                        make_dry(1, 1, v11_pos, v11_norm),
                        make_dry(0, 1, v01_pos, v01_norm),
                    );
                }

                // Surface detail overlays
                if !is_perimeter_wall && !is_interior {
                    if config.emit_road_markings && effective_surface == Surface::Road {
                        emit_road_overlays(
                            &mut builder,
                            chunk,
                            lx,
                            ly,
                            tx,
                            ty,
                            quad_z,
                            tile.district,
                            meta,
                        );
                    } else if config.emit_road_markings && effective_surface == Surface::Trail {
                        let lxi = lx as i32;
                        let lyi = ly as i32;
                        let n_tr = is_road_conn(chunk.local_tile(lxi, lyi - 1))
                            || chunk
                                .local_tile(lxi, lyi - 1)
                                .is_some_and(|t| t.ground_surface() == Surface::Trail);
                        let s_tr = is_road_conn(chunk.local_tile(lxi, lyi + 1))
                            || chunk
                                .local_tile(lxi, lyi + 1)
                                .is_some_and(|t| t.ground_surface() == Surface::Trail);
                        let rut = rgba8(84, 70, 52, 255);
                        if n_tr || s_tr {
                            emit_sloped_subquad(
                                &mut builder,
                                tx,
                                ty,
                                quad_z,
                                0.25,
                                0.0,
                                0.34,
                                1.0,
                                ROAD_MARKING_Y_BIAS,
                                rut,
                                meta,
                            );
                            emit_sloped_subquad(
                                &mut builder,
                                tx,
                                ty,
                                quad_z,
                                0.66,
                                0.0,
                                0.75,
                                1.0,
                                ROAD_MARKING_Y_BIAS,
                                rut,
                                meta,
                            );
                        } else {
                            emit_sloped_subquad(
                                &mut builder,
                                tx,
                                ty,
                                quad_z,
                                0.0,
                                0.25,
                                1.0,
                                0.34,
                                ROAD_MARKING_Y_BIAS,
                                rut,
                                meta,
                            );
                            emit_sloped_subquad(
                                &mut builder,
                                tx,
                                ty,
                                quad_z,
                                0.0,
                                0.66,
                                1.0,
                                0.75,
                                ROAD_MARKING_Y_BIAS,
                                rut,
                                meta,
                            );
                        }
                    } else if config.emit_road_markings && effective_surface == Surface::Driveway {
                        let seam = rgba8(76, 80, 82, 255);
                        emit_sloped_subquad(
                            &mut builder,
                            tx,
                            ty,
                            quad_z,
                            0.0,
                            0.48,
                            1.0,
                            0.52,
                            ROAD_MARKING_Y_BIAS,
                            seam,
                            meta,
                        );
                    } else if effective_surface == Surface::Field && ty.rem_euclid(2) == 0 {
                        let furrow = rgba8(64, 60, 42, 255);
                        emit_sloped_subquad(
                            &mut builder,
                            tx,
                            ty,
                            quad_z,
                            0.0,
                            0.35,
                            1.0,
                            0.55,
                            0.006,
                            furrow,
                            meta,
                        );
                    } else if effective_surface == Surface::Porch {
                        let plank = rgba8(92, 68, 48, 255);
                        for v_c in [0.25_f32, 0.75] {
                            emit_sloped_subquad(
                                &mut builder,
                                tx,
                                ty,
                                quad_z,
                                0.04,
                                v_c - 0.025,
                                0.96,
                                v_c + 0.025,
                                ROAD_MARKING_Y_BIAS,
                                plank,
                                meta,
                            );
                        }
                    }
                }
            }

            // Water surface planes at WATER_SURFACE_Z = 0.0 (including shoreline bank extensions)
            let min_corner_z = quad_z[0].min(quad_z[1]).min(quad_z[2]).min(quad_z[3]);
            if config.emit_water_planes
                && (tile.water_z.is_some() || min_corner_z < WATER_SURFACE_Z)
            {
                let w_meta = VertexMeta::DEFAULT.with_tag(GeometryTag::Water);
                let make_w = |ox: usize, oy: usize, pos: Vec3| -> WorldVertex {
                    let (corner_z, _) = terrain.vertex(lx + ox, ly + oy);
                    WorldVertex::with_meta(
                        Vec3::new(pos.x, WATER_SURFACE_Z, pos.z),
                        Vec3::Y,
                        water_surface_vertex_color(corner_z),
                        w_meta,
                    )
                };
                emit_grid_quad_00_11_smooth(
                    &mut builder,
                    make_w(0, 0, v00_pos),
                    make_w(1, 0, v10_pos),
                    make_w(1, 1, v11_pos),
                    make_w(0, 1, v01_pos),
                );

                // Subtle static shimmer streaks on deeper interior water tiles
                if tile.water_z.is_some()
                    && min_corner_z <= -0.35
                    && hash2(tx, ty).is_multiple_of(3)
                {
                    let u_start = 0.18 + ((hash2(tx + 7, ty) % 5) as f32) * 0.08;
                    let v_start = 0.25 + ((hash2(tx, ty + 11) % 5) as f32) * 0.10;
                    let shimmer = if min_corner_z > -0.75 {
                        rgba8(132, 186, 204, 255)
                    } else {
                        rgba8(94, 152, 182, 255)
                    };
                    let x0 = tx as f32 - 0.5 + u_start;
                    let z0 = ty as f32 - 0.5 + v_start;
                    let y = WATER_SURFACE_Z + WATER_RIPPLE_Y_BIAS;
                    builder.push_quad_with_normal(
                        Vec3::new(x0, y, z0),
                        Vec3::new(x0, y, z0 + 0.06),
                        Vec3::new(x0 + 0.28, y, z0 + 0.06),
                        Vec3::new(x0 + 0.28, y, z0),
                        Vec3::Y,
                        shimmer,
                        w_meta,
                    );
                }
            }

            // 3D Bridge decks, undersides, fascia, guardrails & approach abutments
            if config.emit_bridges
                && let Some(bq) = tile.bridge_quad
            {
                emit_bridge_tile(&mut builder, chunk, lx, ly, tx, ty, bq);
            }
        }
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::MeshBuffer;
    use crate::scene::SceneManager;
    use std::collections::HashMap;
    use zombiebox_core::{
        ActionGame, CHUNK_SIZE, CountyFixture, Ground, LandmarkKind, LocalCell, Position, Tile3D,
        WORLD_SIZE, Water, World,
    };

    fn find_river_bridge(world: &World) -> Position {
        let center = WORLD_SIZE / 2;
        let mut best: Option<(usize, i64, Position)> = None;
        for y in 16..(WORLD_SIZE - 16) {
            for x in 16..(WORLD_SIZE - 16) {
                let pos = Position::new(x, y);
                let Some(tile) = world.tile_at(pos) else {
                    continue;
                };
                if tile.ground != Ground::Bridge || tile.water != Some(Water::River) {
                    continue;
                }
                let mut river_count = 0usize;
                for dy in -3..=3 {
                    for dx in -3..=3 {
                        if world
                            .tile_at(Position::new(x + dx, y + dy))
                            .is_some_and(|t| t.water == Some(Water::River))
                        {
                            river_count += 1;
                        }
                    }
                }
                let dx = i64::from(x - center);
                let dy = i64::from(y - center);
                let dist2 = dx * dx + dy * dy;
                let replace = match best {
                    None => true,
                    Some((bc, bd, _)) => river_count > bc || (river_count == bc && dist2 < bd),
                };
                if replace {
                    best = Some((river_count, dist2, pos));
                }
            }
        }
        best.expect("county has a river bridge").2
    }

    type SeamVertex = ([u32; 3], [u32; 3], [u32; 4]);
    type SeamVertexMap = HashMap<(u32, u32), Vec<SeamVertex>>;

    fn collect_seam_vertices(
        mesh: &MeshBuffer,
        is_east_west: bool,
        seam_coord: f32,
    ) -> SeamVertexMap {
        let mut map: SeamVertexMap = HashMap::new();
        for v in &mesh.vertices {
            let meta = v.unpacked_meta();
            if !matches!(
                meta.tag,
                GeometryTag::Terrain
                    | GeometryTag::InteriorFloor
                    | GeometryTag::Road
                    | GeometryTag::Water
                    | GeometryTag::Bridge
            ) {
                continue;
            }
            let coord_val = if is_east_west {
                v.position[0]
            } else {
                v.position[2]
            };
            if (coord_val - seam_coord).abs() > 1e-6 {
                continue;
            }
            let along = if is_east_west {
                v.position[2]
            } else {
                v.position[0]
            };
            let key = (meta.tag as u32, along.to_bits());
            let entry = (
                [
                    v.position[0].to_bits(),
                    v.position[1].to_bits(),
                    v.position[2].to_bits(),
                ],
                [
                    v.normal[0].to_bits(),
                    v.normal[1].to_bits(),
                    v.normal[2].to_bits(),
                ],
                [
                    v.color[0].to_bits(),
                    v.color[1].to_bits(),
                    v.color[2].to_bits(),
                    v.color[3].to_bits(),
                ],
            );
            map.entry(key).or_default().push(entry);
        }
        map
    }

    #[test]
    fn chunk_seam_positions_and_normals_are_zero_ulp_identical() {
        let world = World::new(42);
        let mut game = ActionGame::county_with_world(world.clone());
        let mut mgr = SceneManager::new();

        let bridge_pos = find_river_bridge(&world);
        let mountain_pos = world
            .landmarks()
            .iter()
            .find(|l| l.kind == LandmarkKind::Mountain)
            .unwrap()
            .entrance;
        let downtown_pos = world
            .landmarks()
            .iter()
            .find(|l| l.kind == LandmarkKind::Downtown)
            .unwrap()
            .entrance;

        let regions = [
            game.player_tile().pos(),
            bridge_pos,
            downtown_pos,
            mountain_pos,
        ];

        for focus in regions {
            game.teleport(Tile3D::from_pos(focus, 0));
            let snap = mgr.prepare_at_focus(&game, focus, 0.016);
            let fc = snap.focus_chunk;

            let a = mgr.cache().get(fc).unwrap().snapshot.clone();
            let b_east = mgr
                .cache()
                .get(ChunkCoord::new(fc.cx + 1, fc.cy))
                .unwrap()
                .snapshot
                .clone();
            let b_south = mgr
                .cache()
                .get(ChunkCoord::new(fc.cx, fc.cy + 1))
                .unwrap()
                .snapshot
                .clone();

            let mesh_a = build_chunk_terrain_mesh(&a);
            let mesh_e = build_chunk_terrain_mesh(&b_east);
            let mesh_s = build_chunk_terrain_mesh(&b_south);

            mesh_a.validate().expect("mesh_a valid");
            mesh_e.validate().expect("mesh_e valid");
            mesh_s.validate().expect("mesh_s valid");

            // Raised sidewalk and tiled interior previews replace portions of the
            // base grid. Compare the full grid separately across material tags.
            let base_config = TerrainMeshConfig {
                emit_sidewalk_curbs: false,
                emit_interior_floor_preview: false,
                ..TerrainMeshConfig::default()
            };
            let base_a = build_chunk_terrain_mesh_with_config(&a, &base_config);
            let base_e = build_chunk_terrain_mesh_with_config(&b_east, &base_config);
            let base_s = build_chunk_terrain_mesh_with_config(&b_south, &base_config);
            let assert_base_edge = |other: &MeshBuffer, lx, ly| {
                let (position, normal) = grid_vertex_pos_and_normal(&a.terrain, lx, ly, 0, 0, 0.0);
                for mesh in [&base_a, other] {
                    assert!(
                        mesh.vertices.iter().any(|vertex| {
                            vertex.position.map(f32::to_bits)
                                == position.to_array().map(f32::to_bits)
                                && vertex.normal.map(f32::to_bits)
                                    == normal.to_array().map(f32::to_bits)
                        }),
                        "missing terrain seam corner at {position:?} with normal {normal:?} near {focus:?}"
                    );
                }
            };
            let edge = CHUNK_SIZE as usize;
            for along in 0..=edge {
                assert_base_edge(&base_e, edge, along);
                assert_base_edge(&base_s, along, edge);
            }

            let seam_x = (fc.cx + 1) as f32 * CHUNK_SIZE as f32 - 0.5;
            let map_a_e = collect_seam_vertices(&mesh_a, true, seam_x);
            let map_e = collect_seam_vertices(&mesh_e, true, seam_x);
            for (k, verts_a) in &map_a_e {
                let along = f32::from_bits(k.1);
                let is_grid_corner = (along + 0.5).fract().abs() < 1e-5;
                if let Some(verts_e) = map_e.get(k) {
                    for va in verts_a {
                        let norm_y = f32::from_bits(va.1[1]);
                        if norm_y.abs() < 1e-3 {
                            continue; // vertical curb/skirt face
                        }
                        if is_grid_corner {
                            let ly_opt = ((along + 0.5).round() as i32 - fc.cy * CHUNK_SIZE)
                                .try_into()
                                .ok()
                                .filter(|&ly: &usize| ly < zombiebox_core::CHUNK_VERTICES);
                            let y_val = f32::from_bits(va.0[1]);
                            let is_base_grid = ly_opt.is_some_and(|ly| {
                                let gz = a.terrain.heights[ly][CHUNK_SIZE as usize];
                                (y_val - gz).abs() < 1e-5
                                    || (y_val - (gz + SIDEWALK_Y_BIAS)).abs() < 1e-5
                            });
                            if is_base_grid {
                                assert!(
                                    verts_e.iter().any(|ve| ve.0 == va.0 && ve.1 == va.1),
                                    "E-W grid seam mismatch at key {k:?}: {va:?} vs {verts_e:?}"
                                );
                                continue;
                            }
                        }
                        assert!(
                            verts_e.iter().any(|ve| ve.0 == va.0),
                            "E-W overlay seam pos mismatch at key {k:?}: {va:?} vs {verts_e:?}"
                        );
                    }
                }
            }

            let seam_z = (fc.cy + 1) as f32 * CHUNK_SIZE as f32 - 0.5;
            let map_a_s = collect_seam_vertices(&mesh_a, false, seam_z);
            let map_s = collect_seam_vertices(&mesh_s, false, seam_z);
            for (k, verts_a) in &map_a_s {
                let along = f32::from_bits(k.1);
                let is_grid_corner = (along + 0.5).fract().abs() < 1e-5;
                if let Some(verts_s) = map_s.get(k) {
                    for va in verts_a {
                        let norm_y = f32::from_bits(va.1[1]);
                        if norm_y.abs() < 1e-3 {
                            continue;
                        }
                        if is_grid_corner {
                            let lx_opt = ((along + 0.5).round() as i32 - fc.cx * CHUNK_SIZE)
                                .try_into()
                                .ok()
                                .filter(|&lx: &usize| lx < zombiebox_core::CHUNK_VERTICES);
                            let y_val = f32::from_bits(va.0[1]);
                            let is_base_grid = lx_opt.is_some_and(|lx| {
                                let gz = a.terrain.heights[CHUNK_SIZE as usize][lx];
                                (y_val - gz).abs() < 1e-5
                                    || (y_val - (gz + SIDEWALK_Y_BIAS)).abs() < 1e-5
                            });
                            if is_base_grid {
                                assert!(
                                    verts_s.iter().any(|vs| vs.0 == va.0 && vs.1 == va.1),
                                    "N-S grid seam mismatch at key {k:?}: {va:?} vs {verts_s:?}"
                                );
                                continue;
                            }
                        }
                        assert!(
                            verts_s.iter().any(|vs| vs.0 == va.0),
                            "N-S overlay seam pos mismatch at key {k:?}: {va:?} vs {verts_s:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn birch_street_flat_terrain_and_negative_void_chunks_validate() {
        let game = ActionGame::residential(42);
        let mut mgr = SceneManager::new();
        let snap = mgr.prepare_at_focus(&game, game.player_tile().pos(), 0.016);

        let mut saw_marking = false;
        let mut saw_sidewalk = false;

        for delta in &snap.dirty_chunks {
            let mesh = build_chunk_terrain_mesh(&delta.snapshot);
            mesh.validate().expect("birch street chunk mesh valid");
            for v in &mesh.vertices {
                let meta = v.unpacked_meta();
                assert_ne!(meta.tag, GeometryTag::Water);
                assert_ne!(meta.tag, GeometryTag::Bridge);
                if meta.tag == GeometryTag::Terrain {
                    assert!(
                        (v.position[1] - 0.0).abs() < 1e-6
                            || (v.position[1] - ROAD_MARKING_Y_BIAS).abs() < 1e-6
                    );
                } else if meta.tag == GeometryTag::Road {
                    if (v.position[1] - ROAD_MARKING_Y_BIAS).abs() < 1e-6 {
                        saw_marking = true;
                    } else if (v.position[1] - SIDEWALK_Y_BIAS).abs() < 1e-6 {
                        saw_sidewalk = true;
                    }
                }
            }
        }
        assert!(saw_marking);
        assert!(saw_sidewalk);

        // Negative out-of-bounds chunk (-1, -1) produces empty void cleanly
        let neg_snap = mgr
            .prepare_at_focus(&game, Position::new(-8, -8), 0.016)
            .dirty_chunks
            .into_iter()
            .find(|d| d.coord == ChunkCoord::new(-1, -1))
            .unwrap()
            .snapshot;
        let neg_mesh = build_chunk_terrain_mesh(&neg_snap);
        neg_mesh.validate().expect("negative void chunk valid");
        assert!(neg_mesh.is_empty());
    }

    #[test]
    fn gas_canopy_keeps_driveway_surface_under_the_roof() {
        let world = World::with_fixture(42, CountyFixture::GasStationLot);
        let canopy = &world.fixture_plan().unwrap().structures[1];
        let pos = canopy.transform.local_to_world(LocalCell::new(8, 4, 0));
        let mut game = ActionGame::county_with_world(world);
        assert_eq!(
            game.teleport(Tile3D::from_pos(pos, 0)),
            Some(Tile3D::from_pos(pos, 0))
        );

        let mut scene = SceneManager::new();
        let frame = scene.prepare_at_focus(&game, pos, 0.016);
        let chunk = frame
            .dirty_chunks
            .iter()
            .find(|delta| delta.coord == ChunkCoord::from_pos(pos))
            .unwrap();
        let mesh = build_chunk_terrain_mesh(&chunk.snapshot);
        assert!(mesh.indices.chunks_exact(3).any(|triangle| {
            let vertices =
                [triangle[0], triangle[1], triangle[2]].map(|index| &mesh.vertices[index as usize]);
            let centroid = vertices
                .iter()
                .map(|vertex| Vec3::from_array(vertex.position))
                .sum::<Vec3>()
                / 3.0;
            (centroid.x - pos.x as f32).abs() < 0.5
                && (centroid.z - pos.y as f32).abs() < 0.5
                && vertices[0].unpacked_meta().tag == GeometryTag::Road
        }));
    }

    #[test]
    fn bridge_deck_and_terrain_mesh_match_world_spatial_queries() {
        let world = World::new(42);
        let bridge_pos = find_river_bridge(&world);
        let mut game = ActionGame::county_with_world(world.clone());
        game.teleport(Tile3D::from_pos(bridge_pos, 0));

        let mut mgr = SceneManager::new();
        let snap = mgr.prepare_at_focus(&game, bridge_pos, 0.016);
        let bridge_chunk = ChunkCoord::from_pos(bridge_pos);
        let chunk_snap = &mgr.cache().get(bridge_chunk).unwrap().snapshot;
        let mesh = build_chunk_terrain_mesh(chunk_snap);
        mesh.validate().expect("bridge chunk mesh valid");

        let bq = world.bridge_quad(bridge_pos).expect("tile has bridge_quad");
        let min_deck_z = bq[0].min(bq[1]).min(bq[2]).min(bq[3]);
        let max_deck_z = bq[0].max(bq[1]).max(bq[2]).max(bq[3]);

        let mut saw_deck_top = false;
        let mut saw_deck_under = false;
        let mut saw_parapet_top = false;
        let mut shallow_r_sum = 0.0f32;
        let mut shallow_count = 0usize;
        let mut deep_r_sum = 0.0f32;
        let mut deep_count = 0usize;

        for v in &mesh.vertices {
            let meta = v.unpacked_meta();
            if meta.tag == GeometryTag::Bridge {
                let dx = (v.position[0] - bridge_pos.x as f32).abs();
                let dz = (v.position[2] - bridge_pos.y as f32).abs();
                if dx <= 0.501 && dz <= 0.501 {
                    if (v.position[1] - bq[0]).abs() < 1e-5
                        || (v.position[1] - bq[1]).abs() < 1e-5
                        || (v.position[1] - bq[2]).abs() < 1e-5
                        || (v.position[1] - bq[3]).abs() < 1e-5
                    {
                        saw_deck_top = true;
                    }
                    if (v.position[1] - (min_deck_z - BRIDGE_SLAB_THICKNESS)).abs() < 0.08 {
                        saw_deck_under = true;
                    }
                    if v.position[1] >= min_deck_z + BRIDGE_PARAPET_HEIGHT - 0.05
                        && v.position[1] <= max_deck_z + BRIDGE_PARAPET_HEIGHT + 0.05
                    {
                        saw_parapet_top = true;
                    }
                }
            } else if meta.tag == GeometryTag::Water
                && (v.position[1] - WATER_SURFACE_Z).abs() < 1e-6
            {
                let sim_z = world
                    .elevation_at(zombiebox_core::Vec2::new(
                        v.position[0].clamp(-0.5, WORLD_SIZE as f32 - 0.5),
                        v.position[2].clamp(-0.5, WORLD_SIZE as f32 - 0.5),
                    ))
                    .unwrap_or(0.0);
                let depth = (WATER_SURFACE_Z - sim_z).max(0.0);
                if depth <= 0.25 {
                    shallow_r_sum += v.color[0];
                    shallow_count += 1;
                } else if depth >= 1.2 {
                    deep_r_sum += v.color[0];
                    deep_count += 1;
                }
            }
        }
        assert!(!snap.active_chunks.is_empty());
        assert!(saw_deck_top);
        assert!(saw_deck_under);
        assert!(saw_parapet_top);
        assert!(shallow_count > 0 && deep_count > 0);
        assert!(
            shallow_r_sum / shallow_count as f32 > deep_r_sum / deep_count as f32,
            "shoreline water must be brighter than deep channel"
        );
    }

    #[test]
    fn sloped_road_markings_conform_without_sinking_or_floating() {
        let world = World::new(42);
        let mut game = ActionGame::county(42);
        // Find a sloped Road tile in county that has road markings (e.g. non-2 mod 3 along a road)
        let mut found: Option<Position> = None;
        for y in 16..(WORLD_SIZE - 16) {
            for x in 16..(WORLD_SIZE - 16) {
                let p = Position::new(x, y);
                if let Some(t) = world.tile_at(p)
                    && t.ground == Ground::Road
                    && x.rem_euclid(3) < 2
                    && y.rem_euclid(3) < 2
                    && let Some(q) = world.terrain_quad(p)
                {
                    let dz = q.into_iter().fold(f32::NEG_INFINITY, f32::max)
                        - q.into_iter().fold(f32::INFINITY, f32::min);
                    if dz >= 0.08 {
                        found = Some(p);
                        break;
                    }
                }
            }
            if found.is_some() {
                break;
            }
        }
        let road_pos = found.expect("county has sloped road tiles");
        game.teleport(Tile3D::from_pos(road_pos, 0));
        let mut mgr = SceneManager::new();
        let _ = mgr.prepare_at_focus(&game, road_pos, 0.016);
        let chunk_snap = &mgr
            .cache()
            .get(ChunkCoord::from_pos(road_pos))
            .unwrap()
            .snapshot;
        let mesh = build_chunk_terrain_mesh(chunk_snap);
        mesh.validate().expect("sloped road mesh valid");

        let mut checked = 0usize;
        for v in &mesh.vertices {
            let meta = v.unpacked_meta();
            if meta.tag == GeometryTag::Road {
                let x = v.position[0];
                let z = v.position[2];
                if let Some(ground_z) = world.elevation_at(zombiebox_core::Vec2::new(x, z)) {
                    let dy = v.position[1] - ground_z;
                    if dy > 0.002 {
                        assert!(
                            (dy - ROAD_MARKING_Y_BIAS).abs() < 1e-4,
                            "marking drifted from slope at ({x}, {z}): dy={dy}, expected={ROAD_MARKING_Y_BIAS}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert!(
            checked > 0,
            "expected sloped road marking vertices near {road_pos:?}"
        );
    }
}
