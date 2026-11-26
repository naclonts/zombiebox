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
                let wallmeta = meta(GeometryTag::WallLower, f, c.building_id as u32);
                let floormeta = meta(GeometryTag::InteriorFloor, f, c.building_id as u32);
                let roofmeta = meta(GeometryTag::Roof, info.floors, c.building_id as u32);
                let openingmeta = meta(GeometryTag::DoorOrWindow, f, c.building_id as u32);
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
                    for iy in 0..2 {
                        for ix in 0..2 {
                            let px = x - 0.5 + ix as f32 * 0.5;
                            let py = y - 0.5 + iy as f32 * 0.5;
                            let a = Vec3::new(px, g.grid[iy][ix], py);
                            let b = Vec3::new(px + 0.5, g.grid[iy][ix + 1], py);
                            let d = Vec3::new(px, g.grid[iy + 1][ix], py + 0.5);
                            let e = Vec3::new(px + 0.5, g.grid[iy + 1][ix + 1], py + 0.5);
                            out.push_triangle(a, e, b, palette.carpet_trim.to_array(), floormeta);
                            out.push_triangle(a, d, e, palette.carpet_trim.to_array(), floormeta);
                        }
                    }
                }
                if f + 1 == info.floors {
                    let (rx0, rx1, ry0, ry1) = footprint(s, p, f, c.building_id as u32);
                    let eave = info.foundation_z + info.floors as f32 * FLOOR_H;
                    if bs.has_pitched_roof {
                        // Shared vertex heights make roof slopes continuous across tiles/chunks.
                        let rise = |vx: f32, vy: f32| {
                            let left = info.x as f32;
                            let right = (info.x + info.width - 1) as f32;
                            let near = info.y as f32;
                            let far = (info.y + info.height - 1) as f32;
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
                    TileKind::Door { open, .. } => {
                        part(
                            -0.5,
                            0.5,
                            z + DOOR_OPENING_HEIGHT,
                            top,
                            palette.trim.to_array(),
                            wallmeta,
                        );
                        if open {
                            for (a, b) in [(-0.5, -0.45), (0.45, 0.5)] {
                                part(a, b, z, z + DOOR_OPENING_HEIGHT, color, wallmeta)
                            }
                        } else {
                            part(
                                -0.5,
                                0.5,
                                z,
                                z + DOOR_OPENING_HEIGHT,
                                [0.36, 0.24, 0.16, 1.0],
                                openingmeta,
                            )
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
