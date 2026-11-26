//! Multi-level Birch Street architecture, 4 house palettes, Bayer-dissolve cutaways,
//! destructible structures & props, stairs, streetlights, and supply caches.

use zombiebox_core::{
    ActionGame, BuildingInfo, BuildingStyle, CacheKind, CacheView, Floor, Ground, InteractPrompt,
    Material, Position, PropKind, Surface, Tile3D, TileKind, TileRenderInfo, TileVisibility, Vec2,
    WallOrientation,
};

use crate::actors::draw_peeking_shooter;
use crate::lighting::{
    CellLight, LightGrid, SHROUD_COLOR, SHROUD_TRIM_COLOR, SHROUD_WALL_COLOR, tint,
};
use crate::{Canvas, FLOOR_HEIGHT_PX, Projector, TILE_H, TILE_W, hash2};

pub(crate) const WALL_FULL_H: f32 = 30.0;
pub(crate) const WALL_STUB_H: f32 = 9.0;
const FASCIA_H: f32 = 4.0;

#[derive(Clone, Copy)]
pub(crate) enum DrawItemKind {
    Tile {
        x: i32,
        y: i32,
        floor: Floor,
        closed_building: Option<u8>,
        stipple: u8,
        f0_support_only: bool,
    },
    DoorJambFront {
        x: i32,
        y: i32,
        floor: Floor,
        horizontal: bool,
    },
    Roof {
        building_idx: usize,
        stipple: u8,
    },
    Cache {
        idx: usize,
    },
    Survivor {
        z: f32,
    },
    Monster {
        idx: usize,
        z: f32,
        lower_dim: bool,
    },
    Fallen {
        idx: usize,
        z: f32,
        lower_dim: bool,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct DrawItem {
    pub depth: f32,
    pub bias: u8,
    pub kind: DrawItemKind,
}

#[derive(Clone, Copy)]
struct HousePalette {
    wall_lit: [u8; 4],
    wall_shade: [u8; 4],
    trim: [u8; 4],
    course: [u8; 4],
    plinth: [u8; 4],
    roof_front: [u8; 4],
    roof_side: [u8; 4],
    roof_back: [u8; 4],
    roof_ridge: [u8; 4],
    roof_eave: [u8; 4],
    carpet: [u8; 4],
    carpet_trim: [u8; 4],
}

fn building_palette(style: BuildingStyle) -> HousePalette {
    match style {
        BuildingStyle::BirchSage => HousePalette {
            wall_lit: [134, 152, 134, 255],
            wall_shade: [96, 116, 104, 255],
            trim: [204, 196, 170, 255],
            course: [64, 78, 70, 145],
            plinth: [74, 78, 76, 255],
            roof_front: [72, 82, 94, 255],
            roof_side: [54, 62, 74, 255],
            roof_back: [44, 50, 62, 255],
            roof_ridge: [104, 114, 128, 255],
            roof_eave: [38, 44, 54, 255],
            carpet: [98, 108, 126, 255],
            carpet_trim: [136, 146, 162, 210],
        },
        BuildingStyle::BlueClapboard => HousePalette {
            wall_lit: [112, 138, 164, 255],
            wall_shade: [80, 102, 128, 255],
            trim: [214, 212, 200, 255],
            course: [54, 72, 92, 150],
            plinth: [82, 84, 88, 255],
            roof_front: [58, 66, 80, 255],
            roof_side: [44, 50, 64, 255],
            roof_back: [34, 40, 52, 255],
            roof_ridge: [92, 100, 118, 255],
            roof_eave: [30, 34, 46, 255],
            carpet: [118, 92, 102, 255],
            carpet_trim: [162, 132, 136, 210],
        },
        BuildingStyle::ColonialBrick => HousePalette {
            wall_lit: [160, 94, 78, 255],
            wall_shade: [120, 68, 60, 255],
            trim: [200, 190, 168, 255],
            course: [86, 52, 46, 155],
            plinth: [96, 92, 86, 255],
            roof_front: [66, 62, 72, 255],
            roof_side: [50, 46, 56, 255],
            roof_back: [40, 38, 48, 255],
            roof_ridge: [96, 92, 104, 255],
            roof_eave: [34, 32, 42, 255],
            carpet: [112, 116, 92, 255],
            carpet_trim: [152, 154, 122, 210],
        },
        BuildingStyle::WarmPutty => HousePalette {
            wall_lit: [168, 154, 130, 255],
            wall_shade: [128, 116, 98, 255],
            trim: [216, 204, 178, 255],
            course: [88, 78, 66, 145],
            plinth: [86, 82, 78, 255],
            roof_front: [82, 70, 66, 255],
            roof_side: [62, 52, 50, 255],
            roof_back: [48, 42, 42, 255],
            roof_ridge: [114, 98, 92, 255],
            roof_eave: [40, 34, 34, 255],
            carpet: [124, 104, 88, 255],
            carpet_trim: [166, 142, 118, 210],
        },
    }
}

fn find_building(game: &ActionGame, bid: u8) -> Option<&BuildingInfo> {
    if bid == 0 {
        return None;
    }
    game.buildings().iter().find(|b| b.id == bid)
}

fn style_for_building(game: &ActionGame, bid: u8) -> BuildingStyle {
    find_building(game, bid).map_or(BuildingStyle::BirchSage, |b| b.style)
}

/// Convert cutaway openness `0.0..=1.0` to 4-step 2×2 Bayer stipple opacity `0..=4`.
#[inline]
pub(crate) fn cutaway_to_stipple(cut: f32) -> u8 {
    let opacity = 1.0 - cut.clamp(0.0, 1.0);
    if opacity <= 0.05 {
        0
    } else if opacity <= 0.30 {
        1
    } else if opacity <= 0.60 {
        2
    } else if opacity <= 0.88 {
        3
    } else {
        4
    }
}

/// Draw Floor 0 ground surfaces (and county fallback outside block bounds).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_ground_pass(
    c: &mut Canvas<'_>,
    game: &ActionGame,
    proj: &Projector,
    grid: &LightGrid,
    bounds: (i32, i32, i32, i32),
    only_outdoor: bool,
    cutaway: &[f32; 8],
) {
    let (min_x, max_x, min_y, max_y) = bounds;
    let w = c.width() as f32;
    let h = c.height() as f32;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let (sx, sy) = proj.project_z(x as f32, y as f32, 0.0);
            if sx < -TILE_W || sx > w + TILE_W || sy < -TILE_H || sy > h + TILE_H {
                continue;
            }
            let light = grid.cell_light(x, y);
            if let Some(info) = grid.tile_info(x, y) {
                if only_outdoor && info.indoor {
                    continue;
                }
                // Skip interior ground under fully closed intact buildings.
                if info.indoor
                    && info.building_id > 0
                    && (info.building_id as usize) < cutaway.len()
                    && cutaway_to_stipple(cutaway[info.building_id as usize]) == 4
                    && info.visibility == TileVisibility::Hidden
                {
                    continue;
                }
                draw_surface_diamond(c, game, proj, grid, x, y, 0.0, sx, sy, info, light);
            } else if let Some(tile) = game.world().tile_at(Position::new(x, y)) {
                draw_county_fallback_tile(c, sx, sy, x, y, tile.ground, light);
            }
        }
    }
}

/// Draw elevated Floor 1 deck (`z = 1.0`) with `4 px` joist fascia border and dark stairwell cutout.
pub(crate) fn draw_f1_deck_pass(
    c: &mut Canvas<'_>,
    game: &ActionGame,
    proj: &Projector,
    grid_f1: &LightGrid,
    active_building: u8,
    bounds: (i32, i32, i32, i32),
) {
    let (min_x, max_x, min_y, max_y) = bounds;
    let w = c.width() as f32;
    let h = c.height() as f32;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let Some(info) = grid_f1.tile_info(x, y) else {
                continue;
            };
            if info.surface == Surface::Air {
                continue;
            }
            if active_building != 0 && info.building_id != active_building {
                continue;
            }
            let (sx, sy) = proj.project_z(x as f32, y as f32, 1.0);
            if sx < -TILE_W || sx > w + TILE_W || sy < -TILE_H || sy > h + TILE_H {
                continue;
            }
            let light = grid_f1.cell_light(x, y);

            // Clipped building footprint bounds so the F1 floor slab & 4 px joist fascia
            // sit 100% flush on top of the F0 exterior support walls!
            let (rx0, rx1, ry0, ry1) = if let Some(b) = find_building(game, info.building_id) {
                (
                    (x as f32 - 0.5).max(b.x as f32),
                    (x as f32 + 0.5).min((b.x + b.width - 1) as f32),
                    (y as f32 - 0.5).max(b.y as f32),
                    (y as f32 + 0.5).min((b.y + b.height - 1) as f32),
                )
            } else {
                (
                    x as f32 - 0.5,
                    x as f32 + 0.5,
                    y as f32 - 0.5,
                    y as f32 + 0.5,
                )
            };

            // 4 px joist fascia border on exposed SE / SW slab edges!
            let se_air = grid_f1
                .tile_info(x + 1, y)
                .is_none_or(|n| n.surface == Surface::Air);
            let sw_air = grid_f1
                .tile_info(x, y + 1)
                .is_none_or(|n| n.surface == Surface::Air);
            let pal = building_palette(style_for_building(game, info.building_id));
            let fascia_l = tint(pal.trim, light);
            let fascia_d = tint([136, 126, 108, 255], light);
            let joist_tick = tint([68, 56, 44, 210], light);

            if se_air && ry1 > ry0 {
                let p0 = proj.project_z(rx1, ry0, 1.0);
                let p1 = proj.project_z(rx1, ry1, 1.0);
                c.polygon(
                    &[p0, p1, (p1.0, p1.1 + FASCIA_H), (p0.0, p0.1 + FASCIA_H)],
                    fascia_d,
                );
                c.line(p0.0, p0.1, p1.0, p1.1, fascia_l);
                let mx = (p0.0 + p1.0) * 0.5;
                let my = (p0.1 + p1.1) * 0.5;
                c.line(mx, my, mx, my + FASCIA_H, joist_tick);
            }
            if sw_air && rx1 > rx0 {
                let p0 = proj.project_z(rx0, ry1, 1.0);
                let p1 = proj.project_z(rx1, ry1, 1.0);
                c.polygon(
                    &[p0, p1, (p1.0, p1.1 + FASCIA_H), (p0.0, p0.1 + FASCIA_H)],
                    fascia_l,
                );
                c.line(p0.0, p0.1 + FASCIA_H, p1.0, p1.1 + FASCIA_H, joist_tick);
                let mx = (p0.0 + p1.0) * 0.5;
                let my = (p0.1 + p1.1) * 0.5;
                c.line(mx, my, mx, my + FASCIA_H, joist_tick);
            }

            if !info.indoor && info.building_id > 0 {
                if rx1 > rx0 && ry1 > ry0 {
                    let q0 = proj.project_z(rx0, ry0, 1.0);
                    let q1 = proj.project_z(rx1, ry0, 1.0);
                    let q2 = proj.project_z(rx1, ry1, 1.0);
                    let q3 = proj.project_z(rx0, ry1, 1.0);
                    let col = if info.visibility == TileVisibility::Hidden {
                        SHROUD_COLOR
                    } else {
                        tint([134, 104, 74, 255], light)
                    };
                    c.polygon(&[q0, q1, q2, q3], col);
                }
            } else {
                draw_surface_diamond(c, game, proj, grid_f1, x, y, 1.0, sx, sy, info, light);
            }

            // Recessed dark stairwell cutout right in the F1 floor deck.
            if info.kind == TileKind::StairsDown && info.visibility != TileVisibility::Hidden {
                draw_stairs_down_cutout(c, sx, sy, info.stair_dir, light);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_surface_diamond(
    c: &mut Canvas<'_>,
    game: &ActionGame,
    proj: &Projector,
    grid: &LightGrid,
    x: i32,
    y: i32,
    z: f32,
    sx: f32,
    sy: f32,
    info: TileRenderInfo,
    light: CellLight,
) {
    if info.indoor && info.visibility == TileVisibility::Hidden {
        c.diamond(sx, sy, TILE_W * 0.5, TILE_H * 0.5, SHROUD_COLOR);
        return;
    }

    // Perimeter wall tiles: keep outdoor ground outside the wall centerline and indoor wood inside!
    if z == 0.0
        && !info.indoor
        && info.building_id > 0
        && let Some(b) = find_building(game, info.building_id)
    {
        // Fill full tile with nearest outdoor yard/porch/driveway ground so wood never bleeds outside!
        let ext_surf = [
            grid.tile_info(x, y + 1),
            grid.tile_info(x + 1, y),
            grid.tile_info(x, y - 1),
            grid.tile_info(x - 1, y),
        ]
        .into_iter()
        .flatten()
        .find(|n| n.building_id == 0)
        .map_or(Surface::Grass, |n| n.surface);
        let ext_base = match ext_surf {
            Surface::Porch => [144, 114, 84, 255],
            Surface::Sidewalk => [124, 128, 122, 255],
            Surface::Driveway => [102, 106, 104, 255],
            Surface::Gravel => [92, 98, 86, 255],
            _ => [64, 88, 66, 255],
        };
        c.diamond(sx, sy, TILE_W * 0.5, TILE_H * 0.5, tint(ext_base, light));

        // Draw clipped indoor hardwood quadrant strictly inside [b.x..=x1, b.y..=y1].
        let rx0 = (x as f32 - 0.5).max(b.x as f32);
        let rx1 = (x as f32 + 0.5).min((b.x + b.width - 1) as f32);
        let ry0 = (y as f32 - 0.5).max(b.y as f32);
        let ry1 = (y as f32 + 0.5).min((b.y + b.height - 1) as f32);
        if rx1 > rx0 && ry1 > ry0 {
            let q0 = proj.project_z(rx0, ry0, 0.0);
            let q1 = proj.project_z(rx1, ry0, 0.0);
            let q2 = proj.project_z(rx1, ry1, 0.0);
            let q3 = proj.project_z(rx0, ry1, 0.0);
            c.polygon(&[q0, q1, q2, q3], tint([134, 104, 74, 255], light));
        }
        return;
    }

    let h = hash2(x, y);
    let base = match info.surface {
        Surface::Air => return,
        Surface::Road => {
            let v = (h % 6) as u8;
            [54 + v, 64 + v, 74 + v, 255]
        }
        Surface::Sidewalk => {
            let v = (h % 5) as u8;
            [124 + v, 128 + v, 122 + v, 255]
        }
        Surface::Driveway => {
            let v = (h % 6) as u8;
            [102 + v, 106 + v, 104 + v, 255]
        }
        Surface::Gravel => {
            let v = (h % 8) as u8;
            [92 + v, 98 + v, 86 + v, 255]
        }
        Surface::Grass => {
            let v = (hash2(x.div_euclid(3), y.div_euclid(3)) % 9) as u8;
            [62 + v, 86 + v, 66 + v / 2, 255]
        }
        Surface::Porch => {
            let v = (h % 6) as u8;
            [144 + v, 114 + v, 84 + v, 255]
        }
        Surface::WoodFloor => {
            let v = (h % 8) as u8;
            [134 + v, 104 + v, 74 + v, 255]
        }
        Surface::KitchenTile => [164, 168, 158, 255],
        Surface::Carpet => building_palette(style_for_building(game, info.building_id)).carpet,
    };

    c.diamond(sx, sy, TILE_W * 0.5, TILE_H * 0.5, tint(base, light));

    match info.surface {
        Surface::Air => {}
        Surface::Road => {
            let n_road = grid
                .tile_info(x, y - 1)
                .is_some_and(|t| t.surface == Surface::Road);
            let s_road = grid
                .tile_info(x, y + 1)
                .is_some_and(|t| t.surface == Surface::Road);
            let w_road = grid
                .tile_info(x - 1, y)
                .is_some_and(|t| t.surface == Surface::Road);
            let e_road = grid
                .tile_info(x + 1, y)
                .is_some_and(|t| t.surface == Surface::Road);
            let along_x = (w_road || e_road) && !(n_road && s_road);
            let along_y = (n_road || s_road) && !(w_road && e_road);

            if (along_x && x.rem_euclid(3) < 2) || (along_y && y.rem_euclid(3) < 2) {
                let (a, b) = if along_x {
                    (
                        proj.project_z(x as f32 - 0.36, y as f32, z),
                        proj.project_z(x as f32 + 0.36, y as f32, z),
                    )
                } else {
                    (
                        proj.project_z(x as f32, y as f32 - 0.36, z),
                        proj.project_z(x as f32, y as f32 + 0.36, z),
                    )
                };
                c.line(a.0, a.1, b.0, b.1, tint([212, 182, 106, 215], light));
                c.line(
                    a.0,
                    a.1 + 1.0,
                    b.0,
                    b.1 + 1.0,
                    tint([126, 110, 76, 150], light),
                );
            }
            if h.is_multiple_of(4) {
                let crack = tint([32, 40, 48, 165], light);
                c.line(sx - 6.0, sy + 2.0, sx - 1.0, sy, crack);
                c.line(sx - 1.0, sy, sx + 3.0, sy + 2.0, crack);
            }
        }
        Surface::Sidewalk => {
            // Concrete expansion seams + pale curb lines along adjacent Road edges!
            let seam = tint([84, 88, 86, 130], light);
            let a = proj.project_z(x as f32 - 0.48, y as f32, z);
            let b = proj.project_z(x as f32 + 0.48, y as f32, z);
            if x.rem_euclid(2) == 0 {
                c.line(a.0, a.1, b.0, b.1, seam);
            }
            let curb = tint([176, 178, 164, 225], light);
            if grid
                .tile_info(x + 1, y)
                .is_some_and(|t| t.surface == Surface::Road)
            {
                let p0 = proj.project_z(x as f32 + 0.5, y as f32 - 0.5, z);
                let p1 = proj.project_z(x as f32 + 0.5, y as f32 + 0.5, z);
                c.line(p0.0, p0.1, p1.0, p1.1, curb);
            }
            if grid
                .tile_info(x - 1, y)
                .is_some_and(|t| t.surface == Surface::Road)
            {
                let p0 = proj.project_z(x as f32 - 0.5, y as f32 - 0.5, z);
                let p1 = proj.project_z(x as f32 - 0.5, y as f32 + 0.5, z);
                c.line(p0.0, p0.1, p1.0, p1.1, curb);
            }
            if grid
                .tile_info(x, y + 1)
                .is_some_and(|t| t.surface == Surface::Road)
            {
                let p0 = proj.project_z(x as f32 - 0.5, y as f32 + 0.5, z);
                let p1 = proj.project_z(x as f32 + 0.5, y as f32 + 0.5, z);
                c.line(p0.0, p0.1, p1.0, p1.1, curb);
            }
            if grid
                .tile_info(x, y - 1)
                .is_some_and(|t| t.surface == Surface::Road)
            {
                let p0 = proj.project_z(x as f32 - 0.5, y as f32 - 0.5, z);
                let p1 = proj.project_z(x as f32 + 0.5, y as f32 - 0.5, z);
                c.line(p0.0, p0.1, p1.0, p1.1, curb);
            }
        }
        Surface::Driveway => {
            let seam = tint([72, 76, 78, 145], light);
            let a = proj.project_z(x as f32 - 0.45, y as f32, z);
            let b = proj.project_z(x as f32 + 0.45, y as f32, z);
            c.line(a.0, a.1, b.0, b.1, seam);
            if h.is_multiple_of(5) {
                c.ellipse(sx + 2.0, sy, 5.0, 2.2, tint([56, 60, 64, 115], light));
            }
        }
        Surface::Gravel => {
            let light_p = tint([154, 152, 134, 165], light);
            let dark_p = tint([54, 60, 56, 165], light);
            c.rect(sx - 5.0, sy + 1.0, 2.0, 1.0, light_p);
            c.rect(sx + 4.0, sy - 2.0, 2.0, 1.0, dark_p);
            if h.is_multiple_of(2) {
                c.rect(sx + 1.0, sy + 3.0, 2.0, 1.0, light_p);
            }
        }
        Surface::Grass => {
            let tuft_l = tint([112, 140, 92, 165], light);
            let tuft_d = tint([38, 66, 50, 185], light);
            c.line(sx - 5.0, sy + 2.0, sx - 4.0, sy, tuft_l);
            c.line(sx + 5.0, sy - 1.0, sx + 6.0, sy - 3.0, tuft_d);
            if h.is_multiple_of(3) {
                c.rect(
                    sx + 2.0,
                    sy + 3.0,
                    2.0,
                    1.0,
                    tint([156, 148, 96, 155], light),
                );
            }
        }
        Surface::Porch => {
            let seam = tint([84, 62, 44, 150], light);
            for off in [-0.25_f32, 0.25] {
                let a = proj.project_z(x as f32 - 0.48, y as f32 + off, z);
                let b = proj.project_z(x as f32 + 0.48, y as f32 + off, z);
                c.line(a.0, a.1, b.0, b.1, seam);
            }
            let edge = proj.project_z(x as f32 - 0.5, y as f32 + 0.5, z);
            let edge2 = proj.project_z(x as f32 + 0.5, y as f32 + 0.5, z);
            c.line(
                edge.0,
                edge.1,
                edge2.0,
                edge2.1,
                tint([186, 154, 114, 175], light),
            );
        }
        Surface::WoodFloor => {
            let seam = tint([76, 58, 44, 125], light);
            let a = proj.project_z(x as f32 - 0.48, y as f32, z);
            let b = proj.project_z(x as f32 + 0.48, y as f32, z);
            c.line(a.0, a.1, b.0, b.1, seam);
            if h.is_multiple_of(4) {
                c.line(
                    sx - 4.0,
                    sy - 1.0,
                    sx + 3.0,
                    sy + 2.0,
                    tint([168, 134, 92, 105], light),
                );
            }
        }
        Surface::KitchenTile => {
            // Crisp 2×2 checkered linoleum sub-diamonds!
            let dark_tile = tint([104, 122, 120, 255], light);
            let p_w = proj.project_z(x as f32 - 0.25, y as f32, z);
            let p_e = proj.project_z(x as f32 + 0.25, y as f32, z);
            c.diamond(p_w.0, p_w.1, TILE_W * 0.24, TILE_H * 0.24, dark_tile);
            c.diamond(p_e.0, p_e.1, TILE_W * 0.24, TILE_H * 0.24, dark_tile);
            let grout = tint([78, 86, 86, 110], light);
            let a = proj.project_z(x as f32 - 0.5, y as f32, z);
            let b = proj.project_z(x as f32 + 0.5, y as f32, z);
            c.line(a.0, a.1, b.0, b.1, grout);
        }
        Surface::Carpet => {
            let pal = building_palette(style_for_building(game, info.building_id));
            if h.is_multiple_of(3) {
                c.rect(sx - 3.0, sy + 1.0, 3.0, 1.0, tint(pal.carpet_trim, light));
                c.rect(sx + 4.0, sy - 1.0, 2.0, 1.0, tint([44, 48, 58, 95], light));
            }
        }
    }
}

fn draw_county_fallback_tile(
    c: &mut Canvas<'_>,
    sx: f32,
    sy: f32,
    x: i32,
    y: i32,
    ground: Ground,
    light: CellLight,
) {
    let h = hash2(x, y);
    let base = match ground {
        Ground::Road | Ground::Street => [
            52 + (h % 5) as u8,
            62 + (h % 5) as u8,
            72 + (h % 5) as u8,
            255,
        ],
        Ground::Grass => [56 + (h % 6) as u8, 78 + (h % 6) as u8, 60, 255],
        Ground::Gravel => [78, 86, 76, 255],
        Ground::Pavement => [102, 106, 100, 255],
        Ground::Field => [78, 84, 60, 255],
        Ground::Bridge => [146, 123, 86, 255],
        Ground::Forest => [36, 65, 50, 255],
        Ground::Mountain => [119, 116, 121, 255],
        Ground::River | Ground::Lake => [51, 95, 113, 255],
        Ground::Trail => [119, 103, 75, 255],
    };
    c.diamond(sx, sy, TILE_W * 0.5, TILE_H * 0.5, tint(base, light));
}

/// Render any tile feature (wall, pillar, window, door, stairs, streetlight, prop, rubble).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_tile_feature(
    c: &mut Canvas<'_>,
    game: &ActionGame,
    proj: &Projector,
    grid: &LightGrid,
    grid_f1_opt: Option<&LightGrid>,
    x: i32,
    y: i32,
    floor: Floor,
    closed_building: Option<u8>,
    stipple: u8,
    f0_support_only: bool,
    player_pos: Vec2,
    near_interact: Option<Tile3D>,
    interact_prompt: InteractPrompt,
    f1_shooters: &[(Vec2, u8)],
    elapsed_ms: u64,
) {
    let Some(info) = grid.tile_info(x, y) else {
        return;
    };
    if info.kind == TileKind::Empty {
        return;
    }

    let z = floor as f32;
    let light = grid.cell_light(x, y);
    let (sx, sy) = proj.project_z(x as f32, y as f32, z);

    // Closed (or Bayer-dissolving) 2-story exterior perimeter facade!
    if let Some(bid) = closed_building {
        if stipple == 0 {
            return;
        }
        c.set_stipple(stipple);
        draw_closed_building_column(
            c,
            game,
            proj,
            grid,
            grid_f1_opt,
            x,
            y,
            bid,
            info,
            light,
            f1_shooters,
            elapsed_ms,
        );
        c.set_stipple(4);
        return;
    }

    // F0 structural support walls beneath active F1 floor when player is upstairs!
    if f0_support_only {
        let pal = building_palette(style_for_building(game, info.building_id));
        match info.kind {
            TileKind::ExteriorWall
            | TileKind::Pillar
            | TileKind::Window
            | TileKind::BrokenWindow
            | TileKind::Door { .. } => {
                draw_wall_or_opening(
                    c,
                    proj,
                    grid,
                    x,
                    y,
                    0.0,
                    FLOOR_HEIGHT_PX,
                    info,
                    pal,
                    light,
                    false,
                    false,
                    elapsed_ms,
                );
            }
            TileKind::Rubble => draw_rubble(c, sx, sy, x, y, info.material, light),
            _ => {}
        }
        return;
    }

    // Hidden indoor tiles: show clean dark shrouded interior walls only (hide props & loot).
    if info.indoor && info.visibility == TileVisibility::Hidden {
        if matches!(
            info.kind,
            TileKind::InteriorWall
                | TileKind::Door { .. }
                | TileKind::ExteriorWall
                | TileKind::Pillar
        ) {
            let h = if is_wall_stub(game, x, y, info, player_pos) {
                WALL_STUB_H
            } else {
                WALL_FULL_H
            };
            draw_shrouded_wall(c, proj, grid, x, y, z, h, info);
        }
        return;
    }

    let pal = building_palette(style_for_building(game, info.building_id));
    let is_interact_target = near_interact == Some(Tile3D::new(x, y, floor));

    match info.kind {
        TileKind::Empty => {}
        TileKind::ExteriorWall
        | TileKind::InteriorWall
        | TileKind::Pillar
        | TileKind::Window
        | TileKind::BrokenWindow
        | TileKind::Door { .. } => {
            let cut_low = is_wall_stub(game, x, y, info, player_pos);
            let height = if cut_low { WALL_STUB_H } else { WALL_FULL_H };
            let show_drop_cue = floor == 1
                && info.kind == TileKind::BrokenWindow
                && (is_interact_target || interact_prompt == InteractPrompt::DropWindow);
            draw_wall_or_opening(
                c,
                proj,
                grid,
                x,
                y,
                z,
                height,
                info,
                pal,
                light,
                cut_low,
                show_drop_cue,
                elapsed_ms,
            );
        }
        TileKind::Rubble => draw_rubble(c, sx, sy, x, y, info.material, light),
        TileKind::StairsUp => draw_stairs_up(c, sx, sy, info.stair_dir, light, elapsed_ms),
        TileKind::StairsDown => {
            draw_stairs_down_railing(c, sx, sy, info.stair_dir, light, elapsed_ms)
        }
        TileKind::Streetlight => draw_streetlight(c, sx, sy, info, light, elapsed_ms),
        TileKind::BrokenStreetlight => draw_broken_streetlight(c, sx, sy, x, y, light),
        TileKind::Prop(prop) => {
            let damaged = info.max_hp > 0 && info.hp * 2 < info.max_hp;
            draw_prop(
                c,
                proj,
                x,
                y,
                z,
                sx,
                sy,
                prop,
                info.orientation,
                damaged,
                light,
            );
        }
    }
}

fn is_wall_stub(game: &ActionGame, x: i32, y: i32, info: TileRenderInfo, player_pos: Vec2) -> bool {
    if let Some(b) = find_building(game, info.building_id) {
        let on_near_perim = x == b.x + b.width - 1 || y == b.y + b.height - 1;
        let on_far_perim = x == b.x || y == b.y;
        if on_near_perim && !on_far_perim {
            return true;
        }
        if on_near_perim && on_far_perim {
            // Cut SW, NE, and SE perimeter corners low when cutaway so near walls meet smoothly.
            return true;
        }
        if !on_far_perim {
            // Interior partitions: uniform clean 9 px stubs when in front of the player!
            return match info.orientation {
                WallOrientation::AlongX => y as f32 > player_pos.y + 0.15,
                WallOrientation::AlongY => x as f32 > player_pos.x + 0.15,
                WallOrientation::Corner => {
                    (x as f32 > player_pos.x + 0.15) || (y as f32 > player_pos.y + 0.15)
                }
            };
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn draw_closed_building_column(
    c: &mut Canvas<'_>,
    game: &ActionGame,
    proj: &Projector,
    grid_f0: &LightGrid,
    grid_f1_opt: Option<&LightGrid>,
    x: i32,
    y: i32,
    bid: u8,
    info_f0: TileRenderInfo,
    light_f0: CellLight,
    f1_shooters: &[(Vec2, u8)],
    elapsed_ms: u64,
) {
    let pal = building_palette(style_for_building(game, bid));

    // Floor 0 exterior perimeter wall (full 30 px) or breached rubble!
    if info_f0.kind == TileKind::Rubble {
        let (sx, sy) = proj.project_z(x as f32, y as f32, 0.0);
        draw_rubble(c, sx, sy, x, y, info_f0.material, light_f0);
    } else {
        draw_wall_or_opening(
            c,
            proj,
            grid_f0,
            x,
            y,
            0.0,
            WALL_FULL_H,
            info_f0,
            pal,
            light_f0,
            false,
            false,
            elapsed_ms,
        );
    }

    // Floor 1 upper facade (34..64 px) + 4 px fascia band (30..34 px) if F1 didn't collapse!
    let f1_info = grid_f1_opt
        .and_then(|g| g.tile_info(x, y))
        .or_else(|| game.tile_render_info(Position::new(x, y), 1));
    let Some(f1) = f1_info else {
        return;
    };
    if f1.surface == Surface::Air {
        return;
    }

    let f1_light = grid_f1_opt.map_or(light_f0, |g| {
        let l = g.cell_light(x, y);
        // Use brighter of F0 exterior light and F1 light so upper facades match dusk sky.
        CellLight {
            r: l.r.max(light_f0.r),
            g: l.g.max(light_f0.g),
            b: l.b.max(light_f0.b),
            vis: TileVisibility::Visible,
        }
    });

    // Draw 4 px fascia band between F0 and F1 along wall segments.
    let segs = wall_segments(proj, grid_f0, x, y, 0.0, info_f0);
    let fascia_col = tint(pal.trim, f1_light);
    let fascia_shadow = tint([56, 62, 68, 195], f1_light);
    for &(a, b, _) in &segs.items[..segs.len] {
        c.polygon(
            &[
                (a.0, a.1 - WALL_FULL_H),
                (b.0, b.1 - WALL_FULL_H),
                (b.0, b.1 - FLOOR_HEIGHT_PX),
                (a.0, a.1 - FLOOR_HEIGHT_PX),
            ],
            fascia_col,
        );
        c.line(
            a.0,
            a.1 - WALL_FULL_H,
            b.0,
            b.1 - WALL_FULL_H,
            fascia_shadow,
        );
    }

    // Draw F1 upper wall / window at z = 1.0 (`-34 px`).
    draw_wall_or_opening(
        c,
        proj,
        grid_f0,
        x,
        y,
        1.0,
        WALL_FULL_H,
        f1,
        pal,
        f1_light,
        false,
        false,
        elapsed_ms,
    );

    // If an F1 Shooter is lurking near this F1 window, draw their glowing crimson visor peeking out!
    if matches!(f1.kind, TileKind::Window | TileKind::BrokenWindow) && !f1_shooters.is_empty() {
        let win_center = Vec2::new(x as f32, y as f32);
        let has_peeker = f1_shooters
            .iter()
            .any(|&(spos, _)| spos.distance(win_center) <= 1.9);
        if has_peeker {
            let (wx, wy) = proj.project_z(x as f32, y as f32, 1.0);
            draw_peeking_shooter(c, wx, wy - 16.0, elapsed_ms);
        }
    }
}

type WallSegment = ((f32, f32), (f32, f32), bool);

struct WallSegments {
    items: [WallSegment; 2], // (a, b, is_along_x)
    len: usize,
}

fn is_wall_neighbor(grid: &LightGrid, nx: i32, ny: i32, bid: u8) -> bool {
    grid.tile_info(nx, ny).is_some_and(|n| {
        n.building_id == bid
            && matches!(
                n.kind,
                TileKind::ExteriorWall
                    | TileKind::InteriorWall
                    | TileKind::Pillar
                    | TileKind::Window
                    | TileKind::BrokenWindow
                    | TileKind::Door { .. }
            )
    })
}

fn wall_segments(
    proj: &Projector,
    grid: &LightGrid,
    x: i32,
    y: i32,
    z: f32,
    info: TileRenderInfo,
) -> WallSegments {
    let fx = x as f32;
    let fy = y as f32;
    match info.orientation {
        WallOrientation::AlongX => WallSegments {
            items: [
                (
                    proj.project_z(fx - 0.5, fy, z),
                    proj.project_z(fx + 0.5, fy, z),
                    true,
                ),
                ((0.0, 0.0), (0.0, 0.0), false),
            ],
            len: 1,
        },
        WallOrientation::AlongY => WallSegments {
            items: [
                (
                    proj.project_z(fx, fy - 0.5, z),
                    proj.project_z(fx, fy + 0.5, z),
                    false,
                ),
                ((0.0, 0.0), (0.0, 0.0), false),
            ],
            len: 1,
        },
        WallOrientation::Corner => {
            let bid = info.building_id;
            let w = is_wall_neighbor(grid, x - 1, y, bid);
            let e = is_wall_neighbor(grid, x + 1, y, bid);
            let n = is_wall_neighbor(grid, x, y - 1, bid);
            let s = is_wall_neighbor(grid, x, y + 1, bid);
            let mut items = [((0.0, 0.0), (0.0, 0.0), false); 2];
            let mut depths = [0.0_f32; 2];
            let mut len = 0usize;
            if w || e {
                let x0 = if w { fx - 0.5 } else { fx };
                let x1 = if e { fx + 0.5 } else { fx };
                items[len] = (proj.project_z(x0, fy, z), proj.project_z(x1, fy, z), true);
                depths[len] = (x0 + x1) * 0.5 + fy;
                len += 1;
            }
            if n || s {
                let y0 = if n { fy - 0.5 } else { fy };
                let y1 = if s { fy + 0.5 } else { fy };
                items[len] = (proj.project_z(fx, y0, z), proj.project_z(fx, y1, z), false);
                depths[len] = fx + (y0 + y1) * 0.5;
                len += 1;
            }
            if len == 2 && depths[0] > depths[1] {
                items.swap(0, 1);
            }
            if len == 0 {
                items[0] = (
                    proj.project_z(fx - 0.35, fy, z),
                    proj.project_z(fx + 0.35, fy, z),
                    true,
                );
                len = 1;
            }
            WallSegments { items, len }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_shrouded_wall(
    c: &mut Canvas<'_>,
    proj: &Projector,
    grid: &LightGrid,
    x: i32,
    y: i32,
    z: f32,
    height: f32,
    info: TileRenderInfo,
) {
    let segs = wall_segments(proj, grid, x, y, z, info);
    for &(a, b, _) in &segs.items[..segs.len] {
        c.polygon(
            &[a, b, (b.0, b.1 - height), (a.0, a.1 - height)],
            SHROUD_WALL_COLOR,
        );
        c.line(a.0, a.1 - height, b.0, b.1 - height, SHROUD_TRIM_COLOR);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_wall_or_opening(
    c: &mut Canvas<'_>,
    proj: &Projector,
    grid: &LightGrid,
    x: i32,
    y: i32,
    z: f32,
    height: f32,
    info: TileRenderInfo,
    pal: HousePalette,
    light: CellLight,
    cut_low: bool,
    show_drop_cue: bool,
    elapsed_ms: u64,
) {
    let segs = wall_segments(proj, grid, x, y, z, info);
    let hp_pct = if info.max_hp > 0 {
        (info.hp as u32 * 100) / info.max_hp as u32
    } else {
        100
    };
    let h_seed = hash2(x, y);

    match info.kind {
        TileKind::Door { open, exterior } => {
            let (a, b, horizontal) = segs.items[0];
            draw_door_tile(
                c, proj, x, y, z, a, b, horizontal, height, open, exterior, hp_pct, pal, light,
            );
            return;
        }
        TileKind::Window | TileKind::BrokenWindow => {
            let (a, b, horizontal) = segs.items[0];
            draw_window_tile(
                c,
                a,
                b,
                horizontal,
                height,
                info.kind == TileKind::BrokenWindow,
                hp_pct,
                cut_low,
                show_drop_cue,
                h_seed,
                pal,
                light,
                elapsed_ms,
            );
            return;
        }
        _ => {}
    }

    // Solid wall (`ExteriorWall`, `InteriorWall`, `Pillar`).
    let is_interior = info.kind == TileKind::InteriorWall;
    let is_brick = info.material == Material::BrickMasonry;

    for &(a, b, horizontal) in &segs.items[..segs.len] {
        let base_col = if is_interior {
            if horizontal {
                [164, 156, 138, 255]
            } else {
                [132, 126, 114, 255]
            }
        } else if is_brick {
            if horizontal {
                [160, 94, 78, 255]
            } else {
                [120, 68, 60, 255]
            }
        } else if horizontal {
            pal.wall_lit
        } else {
            pal.wall_shade
        };

        c.polygon(
            &[a, b, (b.0, b.1 - height), (a.0, a.1 - height)],
            tint(base_col, light),
        );
        // Top cap & baseboard / foundation plinth.
        let cap_col = if is_interior {
            [208, 200, 182, 255]
        } else {
            pal.trim
        };
        c.line(a.0, a.1 - height, b.0, b.1 - height, tint(cap_col, light));
        c.line(a.0, a.1 - 2.0, b.0, b.1 - 2.0, tint(pal.plinth, light));
        c.line(
            a.0,
            a.1 - 5.0,
            b.0,
            b.1 - 5.0,
            tint([156, 148, 132, 150], light),
        );

        // Siding courses or brick mortar lines on full walls.
        if !cut_low && height >= 24.0 {
            let course_col = if is_interior {
                [108, 102, 92, 90]
            } else {
                pal.course
            };
            for course in [10.0_f32, 16.0, 22.0, 27.0] {
                if course < height - 1.0 {
                    c.line(
                        a.0,
                        a.1 - course,
                        b.0,
                        b.1 - course,
                        tint(course_col, light),
                    );
                }
            }
            if is_brick {
                let mortar = tint([182, 168, 150, 125], light);
                for (row_i, course) in [8.0_f32, 14.0, 20.0, 26.0].iter().enumerate() {
                    if *course < height - 2.0 {
                        let t = if row_i.is_multiple_of(2) { 0.33 } else { 0.66 };
                        let mx = a.0 + (b.0 - a.0) * t;
                        let my = a.1 + (b.1 - a.1) * t - *course;
                        c.line(mx, my, mx, my + 4.0, mortar);
                    }
                }
            }
        }

        // Destructible wall cracks & exposed timber framing!
        if hp_pct < 75 {
            draw_wall_damage(c, a, b, height, hp_pct, h_seed, light);
        }
    }

    // Crisp corner vertical trim board on Corner Pillars.
    if info.kind == TileKind::Pillar {
        let (cx, cy) = proj.project_z(x as f32, y as f32, z);
        let col = if is_brick {
            tint([178, 114, 96, 255], light)
        } else {
            tint(pal.trim, light)
        };
        c.line(cx, cy, cx, cy - height, col);
        c.line(
            cx - 1.0,
            cy,
            cx - 1.0,
            cy - height,
            tint([44, 48, 56, 170], light),
        );
    }
}

fn draw_wall_damage(
    c: &mut Canvas<'_>,
    a: (f32, f32),
    b: (f32, f32),
    height: f32,
    hp_pct: u32,
    h_seed: u32,
    light: CellLight,
) {
    let t0 = 0.25 + ((h_seed & 7) as f32 * 0.04);
    let mid_x = a.0 + (b.0 - a.0) * t0;
    let mid_y = a.1 + (b.1 - a.1) * t0;
    let crack_dark = tint([30, 28, 34, 225], light);
    let chip_pale = tint([186, 178, 162, 180], light);
    let h1 = (height * 0.65).min(height - 2.0);
    let h2 = (height * 0.35).max(2.0);

    // 40-74% HP: cracks & chipped patches.
    c.line(mid_x - 2.0, mid_y - h1, mid_x + 1.0, mid_y - h2, crack_dark);
    c.line(
        mid_x + 1.0,
        mid_y - h2,
        mid_x - 1.0,
        mid_y - 2.0,
        crack_dark,
    );
    c.rect(mid_x + 2.0, mid_y - h2 - 2.0, 3.0, 2.0, chip_pale);

    // 1-39% HP: deep fractures + dark breach hole + exposed amber timber studs!
    if hp_pct < 40 {
        let timber = tint([148, 108, 68, 255], light);
        let hole_x = a.0 + (b.0 - a.0) * 0.62;
        let hole_y = a.1 + (b.1 - a.1) * 0.62;
        c.rect(
            hole_x,
            hole_y - h2 - 2.0,
            6.0,
            6.0_f32.min(height - 2.0),
            crack_dark,
        );
        c.line(
            hole_x - 2.0,
            hole_y - 1.0,
            hole_x - 2.0,
            hole_y - h1,
            timber,
        );
        c.line(
            hole_x + 2.0,
            hole_y - 1.0,
            hole_x + 2.0,
            hole_y - h1,
            timber,
        );
        c.line(
            hole_x - 3.0,
            hole_y - h1,
            hole_x + 4.0,
            hole_y - h2,
            crack_dark,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_window_tile(
    c: &mut Canvas<'_>,
    a: (f32, f32),
    b: (f32, f32),
    horizontal: bool,
    height: f32,
    broken: bool,
    hp_pct: u32,
    cut_low: bool,
    show_drop_cue: bool,
    h_seed: u32,
    pal: HousePalette,
    light: CellLight,
    elapsed_ms: u64,
) {
    let wall_col = tint(
        if horizontal {
            pal.wall_lit
        } else {
            pal.wall_shade
        },
        light,
    );
    let trim_col = tint(pal.trim, light);

    // Lower wall sill (always drawn, 9 px tall).
    let sill_h = WALL_STUB_H.min(height);
    c.polygon(&[a, b, (b.0, b.1 - sill_h), (a.0, a.1 - sill_h)], wall_col);
    c.line(a.0, a.1 - 2.0, b.0, b.1 - 2.0, tint(pal.plinth, light));

    let left = (a.0 + (b.0 - a.0) * 0.14, a.1 + (b.1 - a.1) * 0.14);
    let right = (a.0 + (b.0 - a.0) * 0.86, a.1 + (b.1 - a.1) * 0.86);
    let mid = ((left.0 + right.0) * 0.5, (left.1 + right.1) * 0.5);

    if broken {
        // Worn wooden sill highlight signaling climb/vault/drop passability!
        let sill_hi = tint([224, 196, 144, 255], light);
        c.line(left.0, left.1 - sill_h, right.0, right.1 - sill_h, sill_hi);
        c.line(
            left.0,
            left.1 - sill_h + 1.0,
            right.0,
            right.1 - sill_h + 1.0,
            tint([132, 98, 66, 255], light),
        );
    } else {
        c.line(a.0, a.1 - sill_h, b.0, b.1 - sill_h, trim_col);
    }

    if !cut_low && height >= 24.0 {
        let top_h = 24.0_f32.min(height - 3.0);
        // Side wall piers & top lintel wall above window.
        c.polygon(
            &[
                (a.0, a.1 - sill_h),
                (left.0, left.1 - sill_h),
                (left.0, left.1 - height),
                (a.0, a.1 - height),
            ],
            wall_col,
        );
        c.polygon(
            &[
                (right.0, right.1 - sill_h),
                (b.0, b.1 - sill_h),
                (b.0, b.1 - height),
                (right.0, right.1 - height),
            ],
            wall_col,
        );
        c.polygon(
            &[
                (left.0, left.1 - top_h),
                (right.0, right.1 - top_h),
                (right.0, right.1 - height),
                (left.0, left.1 - height),
            ],
            wall_col,
        );
        c.line(a.0, a.1 - height, b.0, b.1 - height, trim_col);

        if broken {
            // Empty dark frame + jagged glass teeth in corners!
            let frame_col = tint([94, 84, 72, 255], light);
            c.line(left.0, left.1 - sill_h, left.0, left.1 - top_h, frame_col);
            c.line(
                right.0,
                right.1 - sill_h,
                right.0,
                right.1 - top_h,
                frame_col,
            );
            c.line(left.0, left.1 - top_h, right.0, right.1 - top_h, trim_col);
            let tooth = tint([176, 232, 246, 230], light);
            c.polygon(
                &[
                    (left.0, left.1 - top_h),
                    (left.0 + 4.0, left.1 - top_h + 1.0),
                    (left.0 + 1.0, left.1 - top_h + 5.0),
                ],
                tooth,
            );
            c.polygon(
                &[
                    (right.0, right.1 - sill_h),
                    (right.0 - 4.0, right.1 - sill_h - 1.0),
                    (right.0 - 1.0, right.1 - sill_h - 4.0),
                ],
                tooth,
            );
            c.line(
                left.0 + 1.0,
                left.1 - sill_h,
                left.0 + 3.0,
                left.1 - sill_h - 3.0,
                [230, 250, 255, 220],
            );
        } else {
            // Intact blue-grey glass pane with diagonal glint & crisp crossbars!
            c.polygon(
                &[
                    (left.0, left.1 - sill_h),
                    (right.0, right.1 - sill_h),
                    (right.0, right.1 - top_h),
                    (left.0, left.1 - top_h),
                ],
                tint([54, 82, 98, 255], light),
            );
            // Diagonal reflective sky glint.
            c.line(
                left.0 + 2.0,
                left.1 - top_h + 4.0,
                mid.0 + 1.0,
                mid.1 - sill_h - 3.0,
                tint([146, 194, 212, 185], light),
            );
            c.line(
                mid.0 + 1.0,
                mid.1 - top_h + 3.0,
                right.0 - 2.0,
                right.1 - sill_h - 6.0,
                tint([172, 214, 228, 155], light),
            );
            // Wooden sash crossbars.
            let sash = tint([136, 154, 148, 255], light);
            let mid_h = (sill_h + top_h) * 0.5;
            c.line(left.0, left.1 - mid_h, right.0, right.1 - mid_h, sash);
            c.line(mid.0, mid.1 - sill_h, mid.0, mid.1 - top_h, sash);
            c.line(left.0, left.1 - top_h, right.0, right.1 - top_h, trim_col);
            if hp_pct < 100 {
                c.line(
                    left.0 + 2.0,
                    left.1 - sill_h - 2.0,
                    right.0 - 3.0,
                    right.1 - top_h + 3.0,
                    [220, 242, 248, 210],
                );
            }
        }
    } else if !broken {
        // Low cut window sill glint showing intact glass on near cut wall.
        c.line(
            left.0,
            left.1 - sill_h - 1.0,
            right.0,
            right.1 - sill_h - 1.0,
            tint([118, 168, 186, 225], light),
        );
    }

    // Scattered sparkling glass shards on the ground around BrokenWindow!
    if broken {
        let glint_a = [226, 248, 255, 220];
        let glint_b = tint([134, 206, 228, 205], light);
        for i in 0..5_u32 {
            let hh = h_seed.wrapping_mul(i.wrapping_mul(1664525).wrapping_add(1013904223));
            let ox = ((hh & 0x1f) as f32 - 15.5) * 0.7;
            let oy = (((hh >> 6) & 0xf) as f32 - 7.5) * 0.55;
            let px = (mid.0 + ox).round() as i32;
            let py = (mid.1 + oy + 2.0).round() as i32;
            c.put(
                px,
                py,
                if i.is_multiple_of(2) {
                    glint_a
                } else {
                    glint_b
                },
            );
            c.put(px + 1, py, glint_b);
        }
    }

    // Subtle pulsing downward drop chevron on F1 BrokenWindow when drop-ready!
    if show_drop_cue {
        let bob = (((elapsed_ms / 260) & 1) as f32) * 1.5;
        let cx = mid.0;
        let cy = mid.1 - sill_h - 5.0 + bob;
        let col = [255, 216, 114, 240];
        c.line(cx - 4.0, cy - 2.0, cx, cy + 2.0, [24, 26, 36, 220]);
        c.line(cx + 4.0, cy - 2.0, cx, cy + 2.0, [24, 26, 36, 220]);
        c.line(cx - 3.0, cy - 2.0, cx, cy + 1.0, col);
        c.line(cx + 3.0, cy - 2.0, cx, cy + 1.0, col);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_door_tile(
    c: &mut Canvas<'_>,
    proj: &Projector,
    x: i32,
    y: i32,
    z: f32,
    a: (f32, f32),
    b: (f32, f32),
    horizontal: bool,
    height: f32,
    open: bool,
    exterior: bool,
    hp_pct: u32,
    pal: HousePalette,
    light: CellLight,
) {
    let (cx, cy) = proj.project_z(x as f32, y as f32, z);
    let jamb_col = tint([110, 92, 70, 255], light);
    let jamb_hi = tint([172, 148, 108, 255], light);

    if open {
        // Lit threshold mat on floor!
        c.diamond(cx, cy, 12.0, 5.5, tint([182, 144, 92, 255], light));
        c.line(
            cx - 10.0,
            cy,
            cx,
            cy + 5.0,
            tint([226, 194, 132, 220], light),
        );
        // Back jamb post.
        let post_h = height.min(22.0);
        c.rect(a.0, a.1 - post_h * 0.5, 4.0, post_h, jamb_col);
        c.line(a.0 - 1.0, a.1, a.0 - 1.0, a.1 - post_h, jamb_hi);

        // Hinged wooden door leaf swung 90° aside!
        let leaf_end = if horizontal {
            proj.project_z(x as f32 - 0.5, y as f32 - 0.72, z)
        } else {
            proj.project_z(x as f32 - 0.72, y as f32 - 0.5, z)
        };
        let leaf_col = if exterior {
            tint([128, 84, 54, 255], light)
        } else {
            tint([148, 112, 76, 255], light)
        };
        let leaf_h = height.min(22.0);
        c.polygon(
            &[
                a,
                leaf_end,
                (leaf_end.0, leaf_end.1 - leaf_h),
                (a.0, a.1 - leaf_h),
            ],
            leaf_col,
        );
        c.line(
            a.0,
            a.1 - leaf_h,
            leaf_end.0,
            leaf_end.1 - leaf_h,
            tint([186, 146, 100, 255], light),
        );
        // Brass knob on open leaf.
        let kx = a.0 + (leaf_end.0 - a.0) * 0.78;
        let ky = a.1 + (leaf_end.1 - a.1) * 0.78 - (leaf_h * 0.45);
        c.rect(kx, ky, 2.0, 2.0, tint([234, 200, 102, 255], light));
    } else {
        // Closed wooden panel door with brass handle!
        let door_h = height.min(25.0);
        let door_base = if exterior {
            if horizontal {
                [128, 82, 54, 255]
            } else {
                [104, 66, 44, 255]
            }
        } else if horizontal {
            [148, 112, 76, 255]
        } else {
            [120, 90, 62, 255]
        };
        let panel_inset = tint([84, 54, 38, 185], light);
        c.polygon(
            &[a, b, (b.0, b.1 - door_h), (a.0, a.1 - door_h)],
            tint(door_base, light),
        );
        // Header transom if full wall height.
        if height > door_h + 1.0 {
            c.polygon(
                &[
                    (a.0, a.1 - door_h),
                    (b.0, b.1 - door_h),
                    (b.0, b.1 - height),
                    (a.0, a.1 - height),
                ],
                tint(
                    if horizontal {
                        pal.wall_lit
                    } else {
                        pal.wall_shade
                    },
                    light,
                ),
            );
            c.line(a.0, a.1 - height, b.0, b.1 - height, tint(pal.trim, light));
        }
        // Frame & recessed panel lines.
        c.line(a.0, a.1 - door_h, b.0, b.1 - door_h, jamb_hi);
        c.line(a.0, a.1, a.0, a.1 - door_h, jamb_col);
        c.line(b.0, b.1, b.0, b.1 - door_h, jamb_col);
        let p0 = (a.0 + (b.0 - a.0) * 0.22, a.1 + (b.1 - a.1) * 0.22);
        let p1 = (a.0 + (b.0 - a.0) * 0.78, a.1 + (b.1 - a.1) * 0.78);
        if door_h >= 14.0 {
            c.line(p0.0, p0.1 - 4.0, p1.0, p1.1 - 4.0, panel_inset);
            c.line(
                p0.0,
                p0.1 - (door_h - 4.0),
                p1.0,
                p1.1 - (door_h - 4.0),
                panel_inset,
            );
        }
        // Brass handle!
        let hx = a.0 + (b.0 - a.0) * 0.76;
        let hy = a.1 + (b.1 - a.1) * 0.76 - (door_h * 0.45).min(11.0);
        c.rect(hx, hy, 2.5, 2.0, tint([236, 202, 102, 255], light));

        // Splintered damage cracks if hp < max_hp!
        if hp_pct < 100 {
            let crack = tint([36, 26, 22, 220], light);
            c.line(p0.0, p0.1 - door_h * 0.7, p1.0, p1.1 - door_h * 0.3, crack);
            if hp_pct < 55 {
                c.line(p1.0, p1.1 - door_h * 0.75, p0.0, p0.1 - door_h * 0.2, crack);
            }
        }
    }

    // Small warm porch lantern beside exterior doors!
    if exterior && height >= 16.0 {
        let lx = a.0 - 2.0;
        let ly = a.1 - 16.0;
        c.rect(lx, ly, 4.0, 6.0, tint([68, 56, 46, 255], light));
        c.rect(lx, ly, 2.5, 3.5, [255, 218, 134, 255]);
    }
}

/// Front doorjamb post sorted slightly ahead of actors so actors walk between open doorposts!
pub(crate) fn draw_open_door_front_jamb(
    c: &mut Canvas<'_>,
    proj: &Projector,
    grid: &LightGrid,
    x: i32,
    y: i32,
    floor: Floor,
    horizontal: bool,
) {
    let light = grid.cell_light(x, y);
    let b = if horizontal {
        proj.project_z(x as f32 + 0.5, y as f32, floor as f32)
    } else {
        proj.project_z(x as f32, y as f32 + 0.5, floor as f32)
    };
    let jamb_col = tint([104, 88, 68, 255], light);
    let jamb_hi = tint([172, 148, 108, 255], light);
    c.rect(b.0, b.1 - 6.0, 4.0, 12.0, jamb_col);
    c.line(b.0 - 1.0, b.1, b.0 - 1.0, b.1 - 12.0, jamb_hi);
}

/// Pitched hipped dark shingle roof capping a closed (or Bayer-dissolving) 2-story building!
pub(crate) fn draw_building_roof(
    c: &mut Canvas<'_>,
    game: &ActionGame,
    proj: &Projector,
    grid_f0: &LightGrid,
    b: &BuildingInfo,
    stipple: u8,
) {
    if stipple == 0 {
        return;
    }
    let pal = building_palette(b.style);
    // Sample outdoor dusk light near front center of building for cohesive roof shading.
    let roof_light = {
        let l = grid_f0.cell_light(b.front_door.x, b.front_door.y + 1);
        CellLight {
            r: l.r.max(76),
            g: l.g.max(86),
            b: l.b.max(106),
            vis: TileVisibility::Visible,
        }
    };

    let x0 = b.x as f32 - 0.08;
    let y0 = b.y as f32 - 0.08;
    let x1 = (b.x + b.width - 1) as f32 + 0.08;
    let y1 = (b.y + b.height - 1) as f32 + 0.08;
    let eave_y = FLOOR_HEIGHT_PX + WALL_FULL_H; // 64 px
    let ridge_y = eave_y + 26.0; // 90 px peak

    let lift = |(px, py): (f32, f32), h: f32| (px, py - h);
    let nw = lift(proj.project_z(x0, y0, 0.0), eave_y);
    let ne = lift(proj.project_z(x1, y0, 0.0), eave_y);
    let sw = lift(proj.project_z(x0, y1, 0.0), eave_y);
    let se = lift(proj.project_z(x1, y1, 0.0), eave_y);

    let mid_y = (y0 + y1) * 0.5;
    let inset = ((x1 - x0) * 0.22).min(2.2);
    let rw = lift(proj.project_z(x0 + inset, mid_y, 0.0), ridge_y);
    let re = lift(proj.project_z(x1 - inset, mid_y, 0.0), ridge_y);

    c.set_stipple(stipple);
    let c_back = tint(pal.roof_back, roof_light);
    let c_side = tint(pal.roof_side, roof_light);
    let c_front = tint(pal.roof_front, roof_light);
    let c_ridge = tint(pal.roof_ridge, roof_light);
    let c_eave = tint(pal.roof_eave, roof_light);

    // 4 hipped shingle slopes.
    c.polygon(&[nw, ne, re, rw], c_back);
    c.polygon(&[nw, rw, sw], c_side);
    c.polygon(&[sw, rw, re, se], c_front);
    c.polygon(&[ne, se, re], c_side);

    // Shingle course lines on the prominent front slope (`sw..se` up to `rw..re`).
    let shingle_line = tint([46, 52, 62, 165], roof_light);
    for step in 1..=4 {
        let t = step as f32 * 0.2;
        let lx = sw.0 + (rw.0 - sw.0) * t;
        let ly = sw.1 + (rw.1 - sw.1) * t;
        let rx = se.0 + (re.0 - se.0) * t;
        let ry = se.1 + (re.1 - se.1) * t;
        c.line(lx, ly, rx, ry, shingle_line);
    }

    // Crisp ridge cap, hip ridges & dark eave overhang trim!
    c.line(rw.0, rw.1, re.0, re.1, c_ridge);
    c.line(nw.0, nw.1, rw.0, rw.1, c_ridge);
    c.line(sw.0, sw.1, rw.0, rw.1, c_ridge);
    c.line(ne.0, ne.1, re.0, re.1, c_ridge);
    c.line(se.0, se.1, re.0, re.1, c_ridge);
    c.line(sw.0, sw.1, se.0, se.1, c_eave);
    c.line(se.0, se.1, ne.0, ne.1, c_eave);
    c.line(
        sw.0,
        sw.1 + 1.0,
        se.0,
        se.1 + 1.0,
        tint(pal.trim, roof_light),
    );
    c.line(
        se.0,
        se.1 + 1.0,
        ne.0,
        ne.1 + 1.0,
        tint(pal.trim, roof_light),
    );

    // Small brick chimney near the east end of the ridge!
    let ch_x = rw.0 + (re.0 - rw.0) * 0.72;
    let ch_y = rw.1 + (re.1 - rw.1) * 0.72;
    c.rect(
        ch_x,
        ch_y - 4.0,
        7.0,
        10.0,
        tint([136, 82, 70, 255], roof_light),
    );
    c.rect(
        ch_x + 2.0,
        ch_y - 4.0,
        3.0,
        10.0,
        tint([98, 58, 52, 255], roof_light),
    );
    c.rect(
        ch_x,
        ch_y - 9.0,
        8.0,
        2.0,
        tint([182, 174, 158, 255], roof_light),
    );

    // If any F1 cells of this building collapsed, show a dark jagged collapse gap in the roof!
    for by in b.y..b.y + b.height {
        for bx in b.x..b.x + b.width {
            if game
                .tile_render_info(Position::new(bx, by), 1)
                .is_some_and(|t| t.surface == Surface::Air)
            {
                let (hx, hy) = proj.project_z(bx as f32, by as f32, 0.0);
                c.diamond(hx, hy - eave_y - 6.0, 14.0, 7.0, [24, 24, 30, 225]);
            }
        }
    }
    c.set_stipple(4);
}

fn draw_rubble(
    c: &mut Canvas<'_>,
    sx: f32,
    sy: f32,
    x: i32,
    y: i32,
    mat: Material,
    light: CellLight,
) {
    let h = hash2(x, y);
    let (main_col, hi_col, dark_col) = match mat {
        Material::BrickMasonry => ([136, 78, 66, 255], [176, 114, 96, 255], [68, 44, 42, 255]),
        Material::WoodLight | Material::WoodDoor | Material::TreeWood => {
            ([126, 92, 60, 255], [170, 130, 86, 255], [64, 48, 36, 255])
        }
        _ => (
            [116, 114, 108, 255],
            [164, 160, 148, 255],
            [58, 60, 62, 255],
        ),
    };
    let cm = tint(main_col, light);
    let ch = tint(hi_col, light);
    let cd = tint(dark_col, light);

    // Breached low foundation stump + scattered rubble chunks.
    c.ellipse(sx, sy + 2.0, 13.0, 5.0, [24, 26, 30, 125]);
    c.polygon(
        &[
            (sx - 9.0, sy + 1.0),
            (sx - 3.0, sy - 4.0),
            (sx + 6.0, sy - 3.0),
            (sx + 9.0, sy + 2.0),
            (sx, sy + 5.0),
        ],
        cm,
    );
    c.rect(sx - 5.0, sy - 2.0, 4.0, 3.0, ch);
    c.rect(sx + 4.0, sy, 5.0, 3.0, cd);
    c.rect(sx - 1.0, sy + 2.0, 3.0, 2.0, ch);
    if h.is_multiple_of(2) {
        c.line(
            sx - 7.0,
            sy - 1.0,
            sx + 4.0,
            sy - 4.0,
            tint([148, 108, 68, 255], light),
        );
    }
}

fn draw_stairs_up(
    c: &mut Canvas<'_>,
    sx: f32,
    sy: f32,
    _stair_dir: (i8, i8),
    light: CellLight,
    elapsed_ms: u64,
) {
    let wood_t = tint([164, 124, 82, 255], light);
    let wood_r = tint([106, 74, 48, 255], light);
    let rail = tint([192, 152, 102, 255], light);
    let dark = tint([46, 36, 30, 220], light);

    // 5 ascending wooden treads rising up-right!
    for i in 0..5 {
        let f = i as f32;
        let ox = sx - 10.0 + f * 4.5;
        let oy = sy + 3.0 - f * 4.2;
        c.polygon(
            &[
                (ox - 5.0, oy),
                (ox + 2.0, oy - 3.5),
                (ox + 7.0, oy - 1.0),
                (ox, oy + 2.5),
            ],
            wood_t,
        );
        c.polygon(
            &[
                (ox - 5.0, oy),
                (ox, oy + 2.5),
                (ox, oy + 5.5),
                (ox - 5.0, oy + 3.0),
            ],
            wood_r,
        );
        c.line(ox - 5.0, oy, ox, oy + 2.5, dark);
    }
    // Banister posts & rail.
    c.line(sx - 13.0, sy + 2.0, sx - 13.0, sy - 8.0, rail);
    c.line(sx + 8.0, sy - 16.0, sx + 8.0, sy - 26.0, rail);
    c.line(sx - 13.0, sy - 8.0, sx + 8.0, sy - 26.0, rail);
    c.line(sx - 13.0, sy - 7.0, sx + 8.0, sy - 25.0, dark);

    // Subtle pulsing `^` cue on lower tread.
    let bob = (((elapsed_ms / 340) & 1) as f32) * 1.0;
    let cue = [248, 224, 154, 225];
    c.line(sx - 11.0, sy + 4.0 - bob, sx - 8.0, sy + 1.0 - bob, cue);
    c.line(sx - 5.0, sy + 4.0 - bob, sx - 8.0, sy + 1.0 - bob, cue);
}

fn draw_stairs_down_cutout(
    c: &mut Canvas<'_>,
    sx: f32,
    sy: f32,
    _stair_dir: (i8, i8),
    light: CellLight,
) {
    // Dark recessed stairwell opening in the F1 floor deck.
    c.diamond(sx, sy, 14.0, 7.0, [20, 22, 28, 255]);
    let tread = tint([126, 94, 62, 255], light);
    let riser = tint([72, 52, 36, 255], light);
    for i in 0..3 {
        let f = i as f32;
        let ox = sx - 6.0 + f * 4.0;
        let oy = sy - 2.0 + f * 2.4;
        c.line(ox - 3.0, oy, ox + 4.0, oy - 3.0, tread);
        c.line(ox - 3.0, oy + 1.0, ox + 4.0, oy - 2.0, riser);
    }
    let trim = tint([182, 148, 102, 255], light);
    c.line(sx - 14.0, sy, sx, sy - 7.0, trim);
    c.line(sx, sy - 7.0, sx + 14.0, sy, trim);
}

fn draw_stairs_down_railing(
    c: &mut Canvas<'_>,
    sx: f32,
    sy: f32,
    _stair_dir: (i8, i8),
    light: CellLight,
    elapsed_ms: u64,
) {
    let rail = tint([188, 150, 102, 255], light);
    let post = tint([128, 96, 62, 255], light);
    // Safety railing along far/side edges of stairwell cutout.
    c.line(sx - 12.0, sy - 1.0, sx - 12.0, sy - 10.0, post);
    c.line(sx, sy - 7.0, sx, sy - 16.0, post);
    c.line(sx + 12.0, sy - 1.0, sx + 12.0, sy - 10.0, post);
    c.line(sx - 12.0, sy - 10.0, sx, sy - 16.0, rail);
    c.line(sx, sy - 16.0, sx + 12.0, sy - 10.0, rail);
    // Subtle downward `v` cue.
    let bob = (((elapsed_ms / 340) & 1) as f32) * 1.0;
    let cue = [242, 214, 142, 215];
    c.line(sx - 3.0, sy + 1.0 + bob, sx, sy + 4.0 + bob, cue);
    c.line(sx + 3.0, sy + 1.0 + bob, sx, sy + 4.0 + bob, cue);
}

fn draw_streetlight(
    c: &mut Canvas<'_>,
    sx: f32,
    sy: f32,
    info: TileRenderInfo,
    light: CellLight,
    elapsed_ms: u64,
) {
    let pole_d = tint([52, 60, 72, 255], light);
    let pole_l = tint([106, 118, 134, 255], light);
    let damaged = info.max_hp > 0 && info.hp < info.max_hp;

    c.ellipse(sx, sy + 2.0, 7.0, 3.0, [20, 24, 30, 155]);
    // Cast-iron plinth base + tall 42 px pole!
    c.rect(sx, sy - 2.0, 5.0, 5.0, pole_d);
    c.line(sx - 1.0, sy - 3.0, sx - 1.0, sy - 42.0, pole_d);
    c.line(sx, sy - 3.0, sx, sy - 42.0, pole_l);
    c.line(sx + 1.0, sy - 3.0, sx + 1.0, sy - 42.0, pole_d);

    // Curved cobra-head luminaire arm reaching over street.
    c.line(sx, sy - 42.0, sx + 6.0, sy - 44.0, pole_l);
    c.line(sx + 6.0, sy - 44.0, sx + 10.0, sy - 42.0, pole_d);
    c.rect(sx + 9.0, sy - 42.0, 6.0, 3.0, pole_d);

    let flicker_off = damaged && ((elapsed_ms / 55).wrapping_mul(2654435761) & 7) < 2;
    if !flicker_off {
        c.rect(sx + 9.0, sy - 40.0, 4.0, 2.0, [255, 232, 154, 255]);
        c.put((sx + 9.0) as i32, (sy - 40.0) as i32, [255, 252, 224, 255]);
    }
    if damaged && ((elapsed_ms / 80) & 1) == 0 {
        // Crackling electric yellow spark when streetlight is damaged!
        c.put((sx + 7.0) as i32, (sy - 37.0) as i32, [255, 236, 118, 255]);
        c.put((sx + 11.0) as i32, (sy - 36.0) as i32, [255, 184, 68, 240]);
    }
}

fn draw_broken_streetlight(c: &mut Canvas<'_>, sx: f32, sy: f32, x: i32, y: i32, light: CellLight) {
    let pole_d = tint([44, 50, 58, 255], light);
    let pole_l = tint([82, 92, 102, 255], light);
    let dir = if hash2(x, y).is_multiple_of(2) {
        1.0
    } else {
        -1.0
    };

    c.ellipse(sx, sy + 2.0, 7.0, 2.8, [18, 22, 26, 150]);
    // Bent snapped stump (10 px tall) + fallen dark lamp head on sidewalk, 0 light!
    c.rect(sx, sy - 2.0, 5.0, 4.0, pole_d);
    c.line(sx, sy - 3.0, sx + dir * 3.0, sy - 10.0, pole_l);
    c.line(sx - 1.0, sy - 3.0, sx + dir * 2.0, sy - 10.0, pole_d);
    c.line(sx + dir * 4.0, sy + 1.0, sx + dir * 13.0, sy + 3.0, pole_d);
    c.rect(
        sx + dir * 13.0,
        sy + 2.0,
        5.0,
        3.0,
        tint([36, 40, 48, 255], light),
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_prop(
    c: &mut Canvas<'_>,
    proj: &Projector,
    x: i32,
    y: i32,
    z: f32,
    sx: f32,
    sy: f32,
    prop: PropKind,
    orient: WallOrientation,
    damaged: bool,
    light: CellLight,
) {
    let h = hash2(x, y);
    match prop {
        PropKind::BirchTree => {
            let crown_h = 31.0 + (h % 9) as f32;
            draw_birch_tree(c, sx, sy, crown_h, damaged, light);
        }
        PropKind::Fence => {
            draw_fence(c, proj, x, y, z, orient, damaged, light);
        }
        PropKind::Mailbox => {
            c.ellipse(sx, sy + 2.0, 5.0, 2.0, [20, 28, 28, 150]);
            c.rect(sx, sy - 4.0, 2.0, 10.0, tint([106, 92, 72, 255], light));
            c.rect(sx, sy - 10.0, 9.0, 5.0, tint([96, 112, 114, 255], light));
            c.line(
                sx - 4.0,
                sy - 13.0,
                sx + 3.0,
                sy - 13.0,
                tint([172, 184, 170, 255], light),
            );
            c.rect(
                sx + 4.0,
                sy - 11.0,
                2.0,
                3.0,
                tint([204, 98, 68, 255], light),
            );
        }
        PropKind::Bench => {
            c.ellipse(sx, sy + 2.0, 13.0, 4.5, [22, 30, 28, 135]);
            let wood_l = tint([154, 112, 74, 255], light);
            let wood_d = tint([104, 72, 48, 255], light);
            let iron = tint([52, 58, 66, 255], light);
            c.line(sx - 9.0, sy + 2.0, sx - 9.0, sy - 8.0, iron);
            c.line(sx + 9.0, sy - 1.0, sx + 9.0, sy - 11.0, iron);
            c.polygon(
                &[
                    (sx - 11.0, sy - 3.0),
                    (sx + 8.0, sy - 8.0),
                    (sx + 11.0, sy - 5.0),
                    (sx - 8.0, sy),
                ],
                wood_l,
            );
            c.polygon(
                &[
                    (sx - 11.0, sy - 8.0),
                    (sx + 8.0, sy - 13.0),
                    (sx + 8.0, sy - 9.0),
                    (sx - 11.0, sy - 4.0),
                ],
                wood_d,
            );
        }
        PropKind::Crate => {
            c.ellipse(sx, sy + 2.5, 9.5, 4.0, [20, 24, 24, 150]);
            let top = tint([164, 124, 78, 255], light);
            let left = tint([126, 90, 56, 255], light);
            let right = tint([94, 66, 42, 255], light);
            let brace = tint([76, 52, 34, 220], light);
            c.polygon(
                &[
                    (sx - 9.0, sy - 7.0),
                    (sx, sy - 11.5),
                    (sx + 9.0, sy - 7.0),
                    (sx, sy - 2.5),
                ],
                top,
            );
            c.polygon(
                &[
                    (sx - 9.0, sy - 7.0),
                    (sx, sy - 2.5),
                    (sx, sy + 5.0),
                    (sx - 9.0, sy + 0.5),
                ],
                left,
            );
            c.polygon(
                &[
                    (sx, sy - 2.5),
                    (sx + 9.0, sy - 7.0),
                    (sx + 9.0, sy + 0.5),
                    (sx, sy + 5.0),
                ],
                right,
            );
            c.line(sx - 8.0, sy - 6.0, sx - 1.0, sy + 4.0, brace);
            c.line(sx + 1.0, sy + 4.0, sx + 8.0, sy - 6.0, brace);
        }
        PropKind::Table => {
            c.ellipse(sx, sy + 2.5, 12.0, 5.0, [20, 22, 26, 130]);
            let leg = tint([88, 62, 42, 255], light);
            c.line(sx - 8.0, sy - 2.0, sx - 8.0, sy + 3.0, leg);
            c.line(sx + 8.0, sy - 2.0, sx + 8.0, sy + 3.0, leg);
            c.line(sx, sy + 1.0, sx, sy + 6.0, leg);
            c.polygon(
                &[
                    (sx - 12.0, sy - 6.0),
                    (sx, sy - 12.0),
                    (sx + 12.0, sy - 6.0),
                    (sx, sy),
                ],
                tint([162, 122, 82, 255], light),
            );
            c.polygon(
                &[
                    (sx - 12.0, sy - 6.0),
                    (sx, sy),
                    (sx, sy + 2.0),
                    (sx - 12.0, sy - 4.0),
                ],
                tint([112, 82, 54, 255], light),
            );
            c.polygon(
                &[
                    (sx, sy),
                    (sx + 12.0, sy - 6.0),
                    (sx + 12.0, sy - 4.0),
                    (sx, sy + 2.0),
                ],
                tint([88, 64, 44, 255], light),
            );
            // Small tabletop mug / book detail.
            c.rect(
                sx + 2.0,
                sy - 7.0,
                3.0,
                2.0,
                tint([212, 206, 188, 255], light),
            );
        }
        PropKind::Sofa => {
            let (lit, mid, dark) = match h % 3 {
                0 => ([104, 126, 118, 255], [72, 94, 90, 255], [48, 64, 64, 255]),
                1 => ([142, 88, 86, 255], [104, 62, 64, 255], [70, 42, 46, 255]),
                _ => ([148, 122, 84, 255], [112, 88, 60, 255], [76, 60, 44, 255]),
            };
            c.ellipse(sx, sy + 3.0, 14.0, 5.5, [20, 20, 24, 135]);
            // Backrest
            c.polygon(
                &[
                    (sx - 13.0, sy - 8.0),
                    (sx - 1.0, sy - 14.0),
                    (sx + 12.0, sy - 7.5),
                    (sx, sy - 1.5),
                ],
                tint(lit, light),
            );
            // Seat & front skirt
            c.polygon(
                &[
                    (sx - 13.0, sy - 4.0),
                    (sx, sy + 2.5),
                    (sx + 12.0, sy - 3.5),
                    (sx - 1.0, sy - 9.5),
                ],
                tint(mid, light),
            );
            c.polygon(
                &[
                    (sx - 13.0, sy - 4.0),
                    (sx, sy + 2.5),
                    (sx, sy + 6.0),
                    (sx - 13.0, sy - 0.5),
                ],
                tint(dark, light),
            );
            c.polygon(
                &[
                    (sx, sy + 2.5),
                    (sx + 12.0, sy - 3.5),
                    (sx + 12.0, sy),
                    (sx, sy + 6.0),
                ],
                tint(dark, light),
            );
            c.line(
                sx - 6.0,
                sy - 6.0,
                sx + 5.0,
                sy - 0.5,
                tint([200, 194, 178, 150], light),
            );
        }
        PropKind::Counter => {
            c.ellipse(sx, sy + 2.5, 11.0, 4.5, [20, 22, 24, 130]);
            c.polygon(
                &[
                    (sx - 11.0, sy - 8.5),
                    (sx, sy - 14.0),
                    (sx + 11.0, sy - 8.5),
                    (sx, sy - 3.0),
                ],
                tint([184, 178, 162, 255], light),
            );
            c.polygon(
                &[
                    (sx - 11.0, sy - 8.5),
                    (sx, sy - 3.0),
                    (sx, sy + 5.0),
                    (sx - 11.0, sy - 0.5),
                ],
                tint([122, 92, 66, 255], light),
            );
            c.polygon(
                &[
                    (sx, sy - 3.0),
                    (sx + 11.0, sy - 8.5),
                    (sx + 11.0, sy - 0.5),
                    (sx, sy + 5.0),
                ],
                tint([92, 68, 50, 255], light),
            );
            // Drawer pulls & sink basin glint.
            c.rect(
                sx - 5.0,
                sy - 2.0,
                2.0,
                1.5,
                tint([214, 198, 146, 255], light),
            );
            c.diamond(sx, sy - 8.5, 4.5, 2.2, tint([106, 128, 134, 255], light));
        }
        PropKind::Bed => {
            let quilt = match h % 3 {
                0 => [88, 108, 138, 255],
                1 => [144, 86, 78, 255],
                _ => [92, 122, 98, 255],
            };
            c.ellipse(sx, sy + 3.0, 15.0, 6.0, [20, 20, 24, 140]);
            // Headboard
            c.polygon(
                &[
                    (sx - 12.0, sy - 8.0),
                    (sx - 1.0, sy - 13.5),
                    (sx - 1.0, sy - 18.0),
                    (sx - 12.0, sy - 12.5),
                ],
                tint([118, 84, 58, 255], light),
            );
            // Mattress & folded quilt
            c.polygon(
                &[
                    (sx - 12.0, sy - 6.0),
                    (sx, sy - 12.0),
                    (sx + 13.0, sy - 5.5),
                    (sx + 1.0, sy + 0.5),
                ],
                tint(quilt, light),
            );
            // Crisp white pillow near headboard
            c.polygon(
                &[
                    (sx - 9.0, sy - 7.5),
                    (sx - 1.0, sy - 11.5),
                    (sx + 3.0, sy - 9.5),
                    (sx - 5.0, sy - 5.5),
                ],
                tint([216, 214, 206, 255], light),
            );
            // Bed frame skirt
            c.polygon(
                &[
                    (sx - 12.0, sy - 6.0),
                    (sx + 1.0, sy + 0.5),
                    (sx + 1.0, sy + 5.0),
                    (sx - 12.0, sy - 1.5),
                ],
                tint([94, 68, 48, 255], light),
            );
            c.polygon(
                &[
                    (sx + 1.0, sy + 0.5),
                    (sx + 13.0, sy - 5.5),
                    (sx + 13.0, sy - 1.0),
                    (sx + 1.0, sy + 5.0),
                ],
                tint([74, 54, 40, 255], light),
            );
        }
        PropKind::Shed => {
            // Sturdy 24 px weathered park garden shed!
            c.ellipse(sx, sy + 3.0, 17.0, 7.5, [18, 24, 24, 150]);
            let left_w = tint([124, 108, 88, 255], light);
            let right_w = tint([94, 82, 68, 255], light);
            let roof = tint([76, 86, 94, 255], light);
            let door = tint([64, 50, 40, 255], light);
            c.polygon(
                &[
                    (sx - 15.0, sy),
                    (sx, sy + 7.5),
                    (sx, sy - 16.0),
                    (sx - 15.0, sy - 23.5),
                ],
                left_w,
            );
            c.polygon(
                &[
                    (sx, sy + 7.5),
                    (sx + 15.0, sy),
                    (sx + 15.0, sy - 20.5),
                    (sx, sy - 16.0),
                ],
                right_w,
            );
            // Slanted corrugated roof
            c.polygon(
                &[
                    (sx - 16.5, sy - 23.5),
                    (sx, sy - 31.0),
                    (sx + 16.5, sy - 20.5),
                    (sx, sy - 15.0),
                ],
                roof,
            );
            c.line(
                sx - 16.5,
                sy - 23.5,
                sx,
                sy - 15.0,
                tint([144, 154, 160, 255], light),
            );
            // Dark shed door + brass padlock!
            c.polygon(
                &[
                    (sx - 10.0, sy + 2.5),
                    (sx - 3.0, sy + 6.0),
                    (sx - 3.0, sy - 10.0),
                    (sx - 10.0, sy - 13.5),
                ],
                door,
            );
            c.rect(
                sx - 4.5,
                sy - 2.0,
                2.0,
                2.0,
                tint([224, 192, 96, 255], light),
            );
        }
    }

    if damaged {
        let crack = tint([34, 28, 26, 225], light);
        c.line(sx - 4.0, sy - 9.0, sx + 3.0, sy - 2.0, crack);
        c.line(sx + 3.0, sy - 8.0, sx - 2.0, sy - 3.0, crack);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_fence(
    c: &mut Canvas<'_>,
    proj: &Projector,
    x: i32,
    y: i32,
    z: f32,
    orient: WallOrientation,
    damaged: bool,
    light: CellLight,
) {
    let wood_l = tint([154, 128, 96, 255], light);
    let wood_d = tint([112, 90, 68, 255], light);
    let post = tint([94, 74, 54, 255], light);
    let draw_run = |c: &mut Canvas<'_>, a: (f32, f32), b: (f32, f32)| {
        // Horizontal rails
        c.line(a.0, a.1 - 4.0, b.0, b.1 - 4.0, wood_d);
        c.line(a.0, a.1 - 9.0, b.0, b.1 - 9.0, wood_l);
        // Vertical pickets
        for i in 0..=4 {
            let t = i as f32 * 0.25;
            let px = a.0 + (b.0 - a.0) * t;
            let py = a.1 + (b.1 - a.1) * t;
            let h = if damaged && i == 2 { 5.0 } else { 11.0 };
            c.line(
                px,
                py,
                px,
                py - h,
                if i == 0 || i == 4 { post } else { wood_l },
            );
        }
    };
    let fx = x as f32;
    let fy = y as f32;
    match orient {
        WallOrientation::AlongX => draw_run(
            c,
            proj.project_z(fx - 0.5, fy, z),
            proj.project_z(fx + 0.5, fy, z),
        ),
        WallOrientation::AlongY => draw_run(
            c,
            proj.project_z(fx, fy - 0.5, z),
            proj.project_z(fx, fy + 0.5, z),
        ),
        WallOrientation::Corner => {
            draw_run(
                c,
                proj.project_z(fx - 0.45, fy, z),
                proj.project_z(fx + 0.45, fy, z),
            );
            draw_run(
                c,
                proj.project_z(fx, fy - 0.45, z),
                proj.project_z(fx, fy + 0.45, z),
            );
        }
    }
}

fn draw_birch_tree(
    c: &mut Canvas<'_>,
    x: f32,
    y: f32,
    height: f32,
    damaged: bool,
    light: CellLight,
) {
    c.ellipse(x + 7.0, y + 3.0, 18.0, 6.0, [20, 32, 30, 128]);
    let bark_d = tint([114, 124, 116, 255], light);
    let bark_l = tint([192, 194, 166, 255], light);
    let nick_col = tint([56, 70, 64, 255], light);

    c.line(x, y, x - 1.0, y - height, bark_d);
    c.line(x + 1.0, y, x, y - height, bark_l);
    c.line(x, y - 13.0, x + 7.0, y - height + 4.0, bark_l);
    c.line(x, y - 10.0, x - 7.0, y - height + 11.0, bark_d);
    for nick in [4.0_f32, 9.0, 17.0, 24.0] {
        if nick < height - 4.0 {
            c.line(x - 1.0, y - nick, x + 1.0, y - nick, nick_col);
        }
    }

    let top = y - height;
    let s = if damaged { 0.78 } else { 1.0 };
    c.polygon(
        &[
            (x - 17.0 * s, top + 4.0),
            (x - 14.0 * s, top - 5.0 * s),
            (x - 5.0 * s, top - 12.0 * s),
            (x + 6.0 * s, top - 11.0 * s),
            (x + 14.0 * s, top - 6.0 * s),
            (x + 19.0 * s, top + 2.0),
            (x + 14.0 * s, top + 9.0 * s),
            (x + 4.0 * s, top + 14.0 * s),
            (x - 8.0 * s, top + 12.0 * s),
        ],
        tint([44, 72, 60, 255], light),
    );
    c.polygon(
        &[
            (x - 14.0 * s, top),
            (x - 12.0 * s, top - 6.0 * s),
            (x - 4.0 * s, top - 11.0 * s),
            (x + 5.0 * s, top - 9.0 * s),
            (x + 11.0 * s, top - 3.0 * s),
            (x + 8.0 * s, top + 5.0 * s),
            (x - 3.0 * s, top + 7.0 * s),
        ],
        tint([76, 102, 72, 255], light),
    );
    let leaf_hi = tint([124, 138, 86, 215], light);
    for (dx, dy, width) in [
        (-8.0, -5.0, 5.0),
        (1.0, -7.0, 6.0),
        (8.0, -2.0, 5.0),
        (-5.0, 1.0, 7.0),
    ] {
        c.rect(x + dx * s, top + dy * s, width * s, 2.0, leaf_hi);
    }
}

/// Draw a glowing supply cache (distinct visuals for Weapon, Ammo, Medkit, Mixed).
pub(crate) fn draw_cache(
    c: &mut Canvas<'_>,
    proj: &Projector,
    cache: &CacheView,
    light: CellLight,
    elapsed_ms: u64,
) {
    if cache.opened {
        // Subtle flat looted crate remnant.
        let (x, y) = proj.project_z(
            cache.pos.x as f32,
            cache.pos.y as f32,
            cache.pos.floor as f32,
        );
        c.diamond(x, y + 1.0, 7.0, 3.5, tint([76, 62, 48, 215], light));
        c.line(x - 6.0, y, x + 6.0, y, tint([44, 36, 30, 220], light));
        return;
    }

    let (x, y) = proj.project_z(
        cache.pos.x as f32,
        cache.pos.y as f32,
        cache.pos.floor as f32,
    );
    // Keep unopened caches cleanly readable even in dark rooms!
    let l = CellLight {
        r: light.r.max(96),
        g: light.g.max(96),
        b: light.b.max(96),
        vis: light.vis,
    };

    let (top, left, right, stripe) = match cache.kind {
        CacheKind::Weapon => (
            [222, 166, 82, 255],
            [152, 98, 48, 255],
            [116, 72, 38, 255],
            [255, 228, 140, 255],
        ),
        CacheKind::Ammo => (
            [142, 158, 98, 255],
            [94, 108, 66, 255],
            [70, 82, 52, 255],
            [246, 206, 98, 255],
        ),
        CacheKind::Medkit => (
            [226, 228, 224, 255],
            [176, 180, 178, 255],
            [138, 144, 146, 255],
            [232, 66, 62, 255],
        ),
        CacheKind::Mixed => (
            [208, 152, 84, 255],
            [144, 96, 52, 255],
            [108, 70, 40, 255],
            [252, 214, 118, 255],
        ),
    };

    let pulse = if cache.nearby {
        ((elapsed_ms as f32 * 0.007).sin() + 1.0) * 1.5
    } else {
        0.0
    };
    c.ellipse(x, y + 3.0, 9.5, 4.0, [18, 20, 22, 165]);
    c.polygon(
        &[
            (x - 8.5, y - 6.0 - pulse * 0.4),
            (x, y - 10.5 - pulse * 0.4),
            (x + 8.5, y - 6.0 - pulse * 0.4),
            (x, y - 1.5 - pulse * 0.4),
        ],
        tint(top, l),
    );
    c.polygon(
        &[
            (x - 8.5, y - 6.0),
            (x, y - 1.5),
            (x, y + 5.0),
            (x - 8.5, y + 0.5),
        ],
        tint(left, l),
    );
    c.polygon(
        &[
            (x, y - 1.5),
            (x + 8.5, y - 6.0),
            (x + 8.5, y + 0.5),
            (x, y + 5.0),
        ],
        tint(right, l),
    );

    if cache.kind == CacheKind::Medkit {
        // Crisp red medical cross on lid!
        let ix = x.round() as i32;
        let iy = (y - 6.0).round() as i32;
        c.line(x - 3.0, y - 6.0, x + 3.0, y - 6.0, stripe);
        c.line(x, y - 8.0, x, y - 4.0, stripe);
        c.put(ix, iy, stripe);
    } else {
        c.line(x - 4.5, y - 8.0, x + 4.5, y - 4.0, stripe);
        c.rect(x, y - 1.5, 3.0, 3.0, stripe);
    }

    if cache.nearby {
        // Crisp golden beacon highlight ring when ready to search!
        c.ellipse_ring(
            x,
            y + 1.0,
            12.0 + pulse,
            6.0 + pulse * 0.5,
            1.2,
            [255, 226, 134, 210],
        );
    }
}
