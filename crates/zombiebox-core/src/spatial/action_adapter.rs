//! Live `O(1)` spatial query adapters for `World` and `ActionGame` (`+z` up).

use super::query::{Collider, ColliderKind, HitSurface, SpatialWorld, SupportSurface, SurfaceQuad};
use super::support::StairGeometry;
use super::{
    Aabb3, BRIDGE_PARAPET_HEIGHT, BRIDGE_SLAB_THICKNESS, DOOR_OPENING_HEIGHT, FLOOR_H,
    SLAB_THICKNESS, TerrainSample, Vec2, Vec3, WALL_HALF_THICKNESS, WALL_HEIGHT, WATER_SURFACE_Z,
    WINDOW_LINTEL_Z, WINDOW_SILL_HEIGHT, floor_elevation,
};
use crate::action::block::is_room_barrier;
use crate::action::types::SpatialCell;
use crate::{
    ActionGame, BuildingId, BuildingInfo, BuildingStyle, Floor, Ground, Material, Position,
    PropKind, Surface, Tile3D, TileKind, WORLD_SIZE, WallOrientation, World,
};

const PITCHED_ROOF_RISE: f32 = 0.95;
const STAIR_RAILING_THICKNESS: f32 = 0.08;
const STAIR_RAILING_HEIGHT: f32 = 0.55;
const STAIR_SLAB_TRIM: f32 = 0.28;

#[inline]
const fn ground_to_surface(ground: Ground) -> Surface {
    match ground {
        Ground::Bridge => Surface::Bridge,
        Ground::River | Ground::Lake => Surface::Water,
        Ground::Mountain => Surface::Mountain,
        Ground::Road | Ground::Street => Surface::Road,
        Ground::Trail => Surface::Trail,
        Ground::Pavement => Surface::Sidewalk,
        Ground::Gravel => Surface::Gravel,
        Ground::Forest => Surface::Forest,
        Ground::Field => Surface::Field,
        Ground::Grass => Surface::Grass,
    }
}

#[inline]
const fn building_wall_material(style: BuildingStyle) -> Material {
    match style {
        BuildingStyle::ColonialBrick
        | BuildingStyle::DowntownStorefront
        | BuildingStyle::DowntownOffice
        | BuildingStyle::DowntownApartments
        | BuildingStyle::HospitalClinical
        | BuildingStyle::PolicePrecinct
        | BuildingStyle::ArmyBarracks => Material::BrickMasonry,
        _ => Material::WoodSiding,
    }
}

fn roof_rise_at(b: &BuildingInfo, tx: i32, ty: i32) -> f32 {
    if !b.style.has_pitched_roof() {
        return 0.0;
    }
    let dx = (tx - b.x).min(b.x + b.width - 1 - tx).max(0) as f32;
    let dy = (ty - b.y).min(b.y + b.height - 1 - ty).max(0) as f32;
    let max_dx = ((b.width - 1) as f32 * 0.5).max(1.0);
    let max_dy = ((b.height - 1) as f32 * 0.5).max(1.0);
    let t = (dx / max_dx).min(dy / max_dy).clamp(0.0, 1.0);
    PITCHED_ROOF_RISE * t
}

#[inline]
fn push_if_intersects(out: &mut Vec<Collider>, query_bounds: Aabb3, c: Collider) {
    if c.bounds.intersects(query_bounds) {
        out.push(c);
    }
}

fn emit_bridge_colliders(world: &World, p: Position, query_bounds: Aabb3, out: &mut Vec<Collider>) {
    let Some(quad) = world.bridge_quad(p) else {
        return;
    };
    let x0 = p.x as f32 - 0.5;
    let x1 = p.x as f32 + 0.5;
    let y0 = p.y as f32 - 0.5;
    let y1 = p.y as f32 + 0.5;
    let quad_min_z = quad[0].min(quad[1]).min(quad[2]).min(quad[3]);

    push_if_intersects(
        out,
        query_bounds,
        Collider {
            bounds: Aabb3::new(
                Vec3::new(x0, y0, quad_min_z - BRIDGE_SLAB_THICKNESS),
                Vec3::new(x1, y1, quad_min_z),
            ),
            kind: ColliderKind::BridgeDeck { tile: p },
            material: Material::BrickMasonry,
        },
    );

    const PARAPET_T: f32 = 0.12;
    let edges = [
        (
            (-1, 0),
            x0,
            x0 + PARAPET_T,
            y0,
            y1,
            quad[0].min(quad[2]),
            quad[0].max(quad[2]),
        ),
        (
            (1, 0),
            x1 - PARAPET_T,
            x1,
            y0,
            y1,
            quad[1].min(quad[3]),
            quad[1].max(quad[3]),
        ),
        (
            (0, -1),
            x0,
            x1,
            y0,
            y0 + PARAPET_T,
            quad[0].min(quad[1]),
            quad[0].max(quad[1]),
        ),
        (
            (0, 1),
            x0,
            x1,
            y1 - PARAPET_T,
            y1,
            quad[2].min(quad[3]),
            quad[2].max(quad[3]),
        ),
    ];

    for ((dx, dy), px0, px1, py0, py1, ez_min, ez_max) in edges {
        let q = Position::new(p.x + dx, p.y + dy);
        if world.tile_at(q).is_none_or(|nt| !nt.ground.is_road()) {
            push_if_intersects(
                out,
                query_bounds,
                Collider {
                    bounds: Aabb3::new(
                        Vec3::new(px0, py0, ez_min),
                        Vec3::new(px1, py1, ez_max + BRIDGE_PARAPET_HEIGHT),
                    ),
                    kind: ColliderKind::BridgeParapet { tile: p },
                    material: Material::BrickMasonry,
                },
            );
        }
    }
}

fn emit_water_hazard(world: &World, p: Position, query_bounds: Aabb3, out: &mut Vec<Collider>) {
    let bed_min_z = world
        .terrain_quad(p)
        .map_or(-2.0, |q| q[0].min(q[1]).min(q[2]).min(q[3]).min(-0.5) - 0.5);
    push_if_intersects(
        out,
        query_bounds,
        Collider {
            bounds: Aabb3::new(
                Vec3::new(p.x as f32 - 0.5, p.y as f32 - 0.5, bed_min_z),
                Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.5, WATER_SURFACE_Z + 0.50),
            ),
            kind: ColliderKind::WaterHazard { tile: p },
            material: Material::None,
        },
    );
}

fn emit_border_wall(tx: i32, ty: i32, query_bounds: Aabb3, out: &mut Vec<Collider>) {
    push_if_intersects(
        out,
        query_bounds,
        Collider {
            bounds: Aabb3::new(
                Vec3::new(tx as f32 - 0.5, ty as f32 - 0.5, -10.0),
                Vec3::new(tx as f32 + 0.5, ty as f32 + 0.5, 20.0),
            ),
            kind: ColliderKind::Wall {
                tile: Tile3D::new(tx, ty, 0),
            },
            material: Material::BrickMasonry,
        },
    );
}

/// Read-only `SpatialWorld` view over a static [`World`] (terrain, bridges, water).
pub struct WorldSpatialView<'a> {
    world: &'a World,
}

impl<'a> WorldSpatialView<'a> {
    pub(crate) fn new(world: &'a World) -> Self {
        Self { world }
    }
}

impl SpatialWorld for WorldSpatialView<'_> {
    fn terrain_at(&self, xy: Vec2) -> Option<TerrainSample> {
        self.world.terrain_at(xy)
    }

    fn supports_at(&self, xy: Vec2, out: &mut Vec<SupportSurface>) {
        out.clear();
        let Some(terrain) = self.world.terrain_at(xy) else {
            return;
        };
        let tile = self.world.tile_at(terrain.tile);
        let surface = tile.map_or(Surface::Grass, |t| ground_to_surface(t.ground));
        let walkable = tile.is_none_or(|t| t.ground.walkable()) && !terrain.is_submerged();
        out.push(SupportSurface {
            id: super::SupportId::Terrain,
            z: terrain.z,
            normal: terrain.normal,
            walkable,
            is_stair: false,
            floor: 0,
            surface,
            water_z: terrain.water_z,
        });

        if let Some(deck) = self.world.bridge_deck_at(xy) {
            out.push(SupportSurface {
                id: super::SupportId::Bridge(terrain.tile),
                z: deck.z,
                normal: deck.normal,
                walkable: true,
                is_stair: false,
                floor: 0,
                surface: Surface::Bridge,
                water_z: None,
            });
        }
    }

    fn colliders_in(&self, bounds: Aabb3, out: &mut Vec<Collider>) {
        out.clear();
        let (min_tile, max_tile) = bounds.tile_bounds();
        for ty in min_tile.y..=max_tile.y {
            for tx in min_tile.x..=max_tile.x {
                if !(0..WORLD_SIZE).contains(&tx) || !(0..WORLD_SIZE).contains(&ty) {
                    emit_border_wall(tx, ty, bounds, out);
                    continue;
                }
                let p = Position::new(tx, ty);
                emit_bridge_colliders(self.world, p, bounds, out);
                if let Some(t) = self.world.tile_at(p)
                    && matches!(t.ground, Ground::River | Ground::Lake)
                {
                    emit_water_hazard(self.world, p, bounds, out);
                }
            }
        }
    }

    fn surface_quads_in(&self, min_tile: Position, max_tile: Position, out: &mut Vec<SurfaceQuad>) {
        out.clear();
        for ty in min_tile.y..=max_tile.y {
            for tx in min_tile.x..=max_tile.x {
                let p = Position::new(tx, ty);
                let x0 = tx as f32 - 0.5;
                let y0 = ty as f32 - 0.5;
                let x1 = tx as f32 + 0.5;
                let y1 = ty as f32 + 0.5;
                if let Some(corners) = self.world.terrain_quad(p) {
                    out.push(SurfaceQuad {
                        surface: HitSurface::Terrain { tile: p },
                        x0,
                        y0,
                        x1,
                        y1,
                        corners,
                        material: Material::None,
                    });
                }
                if let Some(corners) = self.world.bridge_quad(p) {
                    out.push(SurfaceQuad {
                        surface: HitSurface::BridgeDeck { tile: p },
                        x0,
                        y0,
                        x1,
                        y1,
                        corners,
                        material: Material::BrickMasonry,
                    });
                }
            }
        }
    }
}

/// Live `O(1)` `SpatialWorld` view reading [`ActionGame`] cells and building geometry directly
/// without triggering lighting rebuilds or cutaway shrouds.
pub struct ActionSpatialView<'a> {
    game: &'a ActionGame,
}

impl<'a> ActionSpatialView<'a> {
    pub(crate) fn new(game: &'a ActionGame) -> Self {
        Self { game }
    }

    fn clipped_footprint(&self, p: Position, fl: Floor, bid: BuildingId) -> (f32, f32, f32, f32) {
        let same = |nx: i32, ny: i32| -> bool {
            self.game
                .spatial_cell(Tile3D::new(nx, ny, fl))
                .is_some_and(|c| c.building_id == bid && c.surface != Surface::Air)
        };
        let rx0 = p.x as f32
            - if same(p.x - 1, p.y) {
                0.5
            } else {
                WALL_HALF_THICKNESS
            };
        let rx1 = p.x as f32
            + if same(p.x + 1, p.y) {
                0.5
            } else {
                WALL_HALF_THICKNESS
            };
        let ry0 = p.y as f32
            - if same(p.x, p.y - 1) {
                0.5
            } else {
                WALL_HALF_THICKNESS
            };
        let ry1 = p.y as f32
            + if same(p.x, p.y + 1) {
                0.5
            } else {
                WALL_HALF_THICKNESS
            };
        (rx0, rx1, ry0, ry1)
    }

    #[inline]
    fn inside_clipped_footprint(&self, p: Position, fl: Floor, bid: BuildingId, xy: Vec2) -> bool {
        let (rx0, rx1, ry0, ry1) = self.clipped_footprint(p, fl, bid);
        xy.x >= rx0 && xy.x <= rx1 && xy.y >= ry0 && xy.y <= ry1
    }

    fn resolve_stair_high_dir(&self, tile: Tile3D, cell: SpatialCell) -> ((i8, i8), (i8, i8)) {
        let low_dir = if cell.stair_dir != (0, 0) {
            cell.stair_dir
        } else {
            (-1, 0)
        };
        let opp = (-low_dir.0, -low_dir.1);
        let upper_fl = tile.floor + 1;

        let is_open_exit = |d: (i8, i8)| -> bool {
            self.game
                .spatial_cell(Tile3D::new(
                    tile.x + d.0 as i32,
                    tile.y + d.1 as i32,
                    upper_fl,
                ))
                .is_some_and(|u| {
                    u.building_id == cell.building_id
                        && u.surface != Surface::Air
                        && !is_room_barrier(u.kind)
                })
        };

        if is_open_exit(opp) {
            return (low_dir, opp);
        }

        // L-turn corner stair: check orthogonal directions (`(-low.y, low.x)` first so (-1,0) -> (0,-1)).
        let ortho_a = (low_dir.1, -low_dir.0);
        let ortho_b = (-low_dir.1, low_dir.0);
        // Prefer (0, -1) / North-like first when both are open (matches Birch Street houses).
        let (first, second) = if ortho_b.1 < ortho_a.1 {
            (ortho_b, ortho_a)
        } else {
            (ortho_a, ortho_b)
        };

        if is_open_exit(first) {
            (low_dir, first)
        } else if is_open_exit(second) {
            (low_dir, second)
        } else {
            (low_dir, opp)
        }
    }

    fn build_stair_geometry(&self, tile: Tile3D, z_low: f32, cell: SpatialCell) -> StairGeometry {
        let (low_dir, high_dir) = self.resolve_stair_high_dir(tile, cell);
        StairGeometry::new(tile, z_low, low_dir, high_dir)
    }

    #[allow(clippy::type_complexity)]
    fn emit_wall_or_fence_xy(
        &self,
        tile: Tile3D,
        cell: SpatialCell,
        half_t: f32,
        is_fence: bool,
    ) -> ([Option<(f32, f32, f32, f32)>; 2], usize) {
        let fx = tile.x as f32;
        let fy = tile.y as f32;
        match cell.orientation {
            WallOrientation::AlongX => (
                [Some((fx - 0.5, fx + 0.5, fy - half_t, fy + half_t)), None],
                1,
            ),
            WallOrientation::AlongY => (
                [Some((fx - half_t, fx + half_t, fy - 0.5, fy + 0.5)), None],
                1,
            ),
            WallOrientation::Corner => {
                let connects = |dx: i32, dy: i32| -> bool {
                    self.game
                        .spatial_cell(Tile3D::new(tile.x + dx, tile.y + dy, tile.floor))
                        .is_some_and(|n| {
                            if is_fence {
                                n.kind == TileKind::Prop(PropKind::Fence)
                            } else {
                                n.building_id == cell.building_id && is_room_barrier(n.kind)
                            }
                        })
                };
                let w = connects(-1, 0);
                let e = connects(1, 0);
                let n = connects(0, -1);
                let s = connects(0, 1);

                let mut out = [None, None];
                let mut count = 0;
                if w || e {
                    let x0 = fx - if w { 0.5 } else { half_t };
                    let x1 = fx + if e { 0.5 } else { half_t };
                    out[count] = Some((x0, x1, fy - half_t, fy + half_t));
                    count += 1;
                }
                if n || s {
                    let y0 = fy - if n { 0.5 } else { half_t };
                    let y1 = fy + if s { 0.5 } else { half_t };
                    out[count] = Some((fx - half_t, fx + half_t, y0, y1));
                    count += 1;
                }
                if count == 0 {
                    out[0] = Some((fx - 0.12, fx + 0.12, fy - 0.12, fy + 0.12));
                    count = 1;
                }
                (out, count)
            }
        }
    }

    fn emit_tile_floor_colliders(
        &self,
        tile: Tile3D,
        cell: SpatialCell,
        z_base: f32,
        b_info: Option<&BuildingInfo>,
        query_bounds: Aabb3,
        out: &mut Vec<Collider>,
    ) {
        let fx = tile.x as f32;
        let fy = tile.y as f32;

        let has_slab_above = b_info.is_some_and(|b| {
            let next_fl = tile.floor + 1;
            if next_fl < b.floors {
                self.game
                    .spatial_cell(Tile3D::new(tile.x, tile.y, next_fl))
                    .is_some_and(|u| {
                        u.surface != Surface::Air
                            && !matches!(u.kind, TileKind::StairsDown | TileKind::StairsBoth)
                    })
            } else {
                next_fl == b.floors && cell.surface != Surface::Air
            }
        });
        let z_wall_top = z_base + if has_slab_above { WALL_HEIGHT } else { FLOOR_H };

        let wall_mat = if !matches!(cell.material, Material::None | Material::Glass) {
            cell.material
        } else {
            b_info.map_or(Material::WoodSiding, |b| building_wall_material(b.style))
        };

        match cell.kind {
            TileKind::ExteriorWall | TileKind::InteriorWall | TileKind::Pillar => {
                let (rects, count) =
                    self.emit_wall_or_fence_xy(tile, cell, WALL_HALF_THICKNESS, false);
                for rect in rects.into_iter().take(count).flatten() {
                    push_if_intersects(
                        out,
                        query_bounds,
                        Collider {
                            bounds: Aabb3::new(
                                Vec3::new(rect.0, rect.2, z_base),
                                Vec3::new(rect.1, rect.3, z_wall_top),
                            ),
                            kind: ColliderKind::Wall { tile },
                            material: wall_mat,
                        },
                    );
                }
            }
            TileKind::Door { open, .. } => {
                let along_y = cell.orientation == WallOrientation::AlongY;
                let (full_x0, full_x1, full_y0, full_y1) = if along_y {
                    (
                        fx - WALL_HALF_THICKNESS,
                        fx + WALL_HALF_THICKNESS,
                        fy - 0.5,
                        fy + 0.5,
                    )
                } else {
                    (
                        fx - 0.5,
                        fx + 0.5,
                        fy - WALL_HALF_THICKNESS,
                        fy + WALL_HALF_THICKNESS,
                    )
                };

                // Upper transom above doorway opening.
                push_if_intersects(
                    out,
                    query_bounds,
                    Collider {
                        bounds: Aabb3::new(
                            Vec3::new(full_x0, full_y0, z_base + DOOR_OPENING_HEIGHT),
                            Vec3::new(full_x1, full_y1, z_wall_top),
                        ),
                        kind: ColliderKind::Wall { tile },
                        material: wall_mat,
                    },
                );

                if !open {
                    push_if_intersects(
                        out,
                        query_bounds,
                        Collider {
                            bounds: Aabb3::new(
                                Vec3::new(full_x0, full_y0, z_base),
                                Vec3::new(full_x1, full_y1, z_base + DOOR_OPENING_HEIGHT),
                            ),
                            kind: ColliderKind::ClosedDoor { tile },
                            material: Material::WoodDoor,
                        },
                    );
                } else {
                    // Two slim 0.05 jamb posts framing the 0.90-wide open doorway.
                    for (a0, a1) in [(-0.5_f32, -0.45_f32), (0.45, 0.5)] {
                        let (jx0, jx1, jy0, jy1) = if along_y {
                            (
                                fx - WALL_HALF_THICKNESS,
                                fx + WALL_HALF_THICKNESS,
                                fy + a0,
                                fy + a1,
                            )
                        } else {
                            (
                                fx + a0,
                                fx + a1,
                                fy - WALL_HALF_THICKNESS,
                                fy + WALL_HALF_THICKNESS,
                            )
                        };
                        push_if_intersects(
                            out,
                            query_bounds,
                            Collider {
                                bounds: Aabb3::new(
                                    Vec3::new(jx0, jy0, z_base),
                                    Vec3::new(jx1, jy1, z_base + DOOR_OPENING_HEIGHT),
                                ),
                                kind: ColliderKind::Wall { tile },
                                material: wall_mat,
                            },
                        );
                    }
                }
            }
            TileKind::Window | TileKind::BrokenWindow => {
                let broken = cell.kind == TileKind::BrokenWindow;
                let along_y = cell.orientation == WallOrientation::AlongY;
                let (full_x0, full_x1, full_y0, full_y1) = if along_y {
                    (
                        fx - WALL_HALF_THICKNESS,
                        fx + WALL_HALF_THICKNESS,
                        fy - 0.5,
                        fy + 0.5,
                    )
                } else {
                    (
                        fx - 0.5,
                        fx + 0.5,
                        fy - WALL_HALF_THICKNESS,
                        fy + WALL_HALF_THICKNESS,
                    )
                };

                // Lower sill.
                push_if_intersects(
                    out,
                    query_bounds,
                    Collider {
                        bounds: Aabb3::new(
                            Vec3::new(full_x0, full_y0, z_base),
                            Vec3::new(full_x1, full_y1, z_base + WINDOW_SILL_HEIGHT),
                        ),
                        kind: ColliderKind::WindowSill { tile, broken },
                        material: wall_mat,
                    },
                );
                // Upper lintel.
                push_if_intersects(
                    out,
                    query_bounds,
                    Collider {
                        bounds: Aabb3::new(
                            Vec3::new(full_x0, full_y0, z_base + WINDOW_LINTEL_Z),
                            Vec3::new(full_x1, full_y1, z_wall_top),
                        ),
                        kind: ColliderKind::Wall { tile },
                        material: wall_mat,
                    },
                );
                // Left & right 0.12 jamb piers leaving a 0.76-wide central opening.
                for (a0, a1) in [(-0.5_f32, -0.38_f32), (0.38, 0.5)] {
                    let (jx0, jx1, jy0, jy1) = if along_y {
                        (
                            fx - WALL_HALF_THICKNESS,
                            fx + WALL_HALF_THICKNESS,
                            fy + a0,
                            fy + a1,
                        )
                    } else {
                        (
                            fx + a0,
                            fx + a1,
                            fy - WALL_HALF_THICKNESS,
                            fy + WALL_HALF_THICKNESS,
                        )
                    };
                    push_if_intersects(
                        out,
                        query_bounds,
                        Collider {
                            bounds: Aabb3::new(
                                Vec3::new(jx0, jy0, z_base + WINDOW_SILL_HEIGHT),
                                Vec3::new(jx1, jy1, z_base + WINDOW_LINTEL_Z),
                            ),
                            kind: ColliderKind::Wall { tile },
                            material: wall_mat,
                        },
                    );
                }
                // Intact glass pane inside `[WINDOW_SILL_HEIGHT, WINDOW_LINTEL_Z]`.
                if !broken {
                    let (gx0, gx1, gy0, gy1) = if along_y {
                        (fx - 0.02, fx + 0.02, fy - 0.38, fy + 0.38)
                    } else {
                        (fx - 0.38, fx + 0.38, fy - 0.02, fy + 0.02)
                    };
                    push_if_intersects(
                        out,
                        query_bounds,
                        Collider {
                            bounds: Aabb3::new(
                                Vec3::new(gx0, gy0, z_base + WINDOW_SILL_HEIGHT),
                                Vec3::new(gx1, gy1, z_base + WINDOW_LINTEL_Z),
                            ),
                            kind: ColliderKind::WindowGlass { tile },
                            material: Material::Glass,
                        },
                    );
                }
            }
            TileKind::Streetlight => {
                push_if_intersects(
                    out,
                    query_bounds,
                    Collider {
                        bounds: Aabb3::new(
                            Vec3::new(fx - 0.12, fy - 0.12, z_base),
                            Vec3::new(fx + 0.12, fy + 0.12, z_base + 1.75),
                        ),
                        kind: ColliderKind::Prop { tile, prop: None },
                        material: Material::MetalPole,
                    },
                );
            }
            TileKind::Prop(PropKind::Fence) => {
                let (rects, count) = self.emit_wall_or_fence_xy(tile, cell, 0.07, true);
                for rect in rects.into_iter().take(count).flatten() {
                    push_if_intersects(
                        out,
                        query_bounds,
                        Collider {
                            bounds: Aabb3::new(
                                Vec3::new(rect.0, rect.2, z_base),
                                Vec3::new(rect.1, rect.3, z_base + 0.78),
                            ),
                            kind: ColliderKind::Prop {
                                tile,
                                prop: Some(PropKind::Fence),
                            },
                            material: Material::WoodLight,
                        },
                    );
                }
            }
            TileKind::Prop(prop) => {
                let (hx, hy, hz, mat) = match prop {
                    PropKind::Shed => (0.46, 0.46, 1.30, Material::WoodLight),
                    PropKind::BirchTree => (0.22, 0.22, 2.40, Material::TreeWood),
                    PropKind::Counter | PropKind::Crate => (0.38, 0.38, 0.78, Material::WoodLight),
                    PropKind::Table | PropKind::Sofa => (0.36, 0.36, 0.72, Material::WoodLight),
                    PropKind::Bed | PropKind::Bench => (0.38, 0.32, 0.58, Material::WoodLight),
                    PropKind::Mailbox => (0.16, 0.16, 0.75, Material::WoodLight),
                    PropKind::Fence => unreachable!(),
                };
                push_if_intersects(
                    out,
                    query_bounds,
                    Collider {
                        bounds: Aabb3::new(
                            Vec3::new(fx - hx, fy - hy, z_base),
                            Vec3::new(fx + hx, fy + hy, z_base + hz),
                        ),
                        kind: ColliderKind::Prop {
                            tile,
                            prop: Some(prop),
                        },
                        material: mat,
                    },
                );
            }
            _ => {}
        }
    }

    fn emit_stairwell_railings(
        &self,
        tile: Tile3D,
        cell: SpatialCell,
        z_top: f32,
        query_bounds: Aabb3,
        out: &mut Vec<Collider>,
    ) {
        if tile.floor == 0 {
            return;
        }
        // Check `low_dir` and `high_dir` of flight `(tile.floor - 1 -> tile.floor)` if stairs come from below.
        let (lower_low_dir, lower_high_dir) = self
            .game
            .spatial_cell(Tile3D::new(tile.x, tile.y, tile.floor - 1))
            .filter(|c| matches!(c.kind, TileKind::StairsUp | TileKind::StairsBoth))
            .map(|c| {
                let (ld, hd) =
                    self.resolve_stair_high_dir(Tile3D::new(tile.x, tile.y, tile.floor - 1), c);
                (Some(ld), Some(hd))
            })
            .unwrap_or((None, None));
        // Check `low_dir` of flight `(tile.floor -> tile.floor + 1)` if `StairsBoth`.
        let upper_low_dir = (cell.kind == TileKind::StairsBoth).then(|| {
            if cell.stair_dir != (0, 0) {
                cell.stair_dir
            } else {
                (-1, 0)
            }
        });

        let fx = tile.x as f32;
        let fy = tile.y as f32;

        for (dx, dy) in [(-1_i8, 0_i8), (1, 0), (0, -1), (0, 1)] {
            if Some((dx, dy)) == lower_high_dir || Some((dx, dy)) == upper_low_dir {
                continue;
            }
            let neighbor = self.game.spatial_cell(Tile3D::new(
                tile.x + dx as i32,
                tile.y + dy as i32,
                tile.floor,
            ));
            let walkable_neighbor = neighbor.is_some_and(|n| {
                n.building_id == cell.building_id
                    && n.surface != Surface::Air
                    && !is_room_barrier(n.kind)
            });
            if !walkable_neighbor {
                continue;
            }
            // Set `low_dir` railing back flush with the trimmed `BuildingSlab` edge so ascending/descending
            // actors have unobstructed headroom across the entire stair width.
            let setback = if Some((dx, dy)) == lower_low_dir {
                STAIR_SLAB_TRIM
            } else {
                0.0
            };
            let (rx0, rx1, ry0, ry1) = if dx < 0 {
                let edge_x = fx - 0.5 - setback;
                (edge_x - STAIR_RAILING_THICKNESS, edge_x, fy - 0.5, fy + 0.5)
            } else if dx > 0 {
                let edge_x = fx + 0.5 + setback;
                (edge_x, edge_x + STAIR_RAILING_THICKNESS, fy - 0.5, fy + 0.5)
            } else if dy < 0 {
                let edge_y = fy - 0.5 - setback;
                (fx - 0.5, fx + 0.5, edge_y - STAIR_RAILING_THICKNESS, edge_y)
            } else {
                let edge_y = fy + 0.5 + setback;
                (fx - 0.5, fx + 0.5, edge_y, edge_y + STAIR_RAILING_THICKNESS)
            };
            push_if_intersects(
                out,
                query_bounds,
                Collider {
                    bounds: Aabb3::new(
                        Vec3::new(rx0, ry0, z_top),
                        Vec3::new(rx1, ry1, z_top + STAIR_RAILING_HEIGHT),
                    ),
                    kind: ColliderKind::BuildingSlab {
                        building_id: cell.building_id,
                        floor: tile.floor,
                        is_roof: false,
                    },
                    material: Material::WoodLight,
                },
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn trim_slab_around_stairs(
        &self,
        p: Position,
        fl: Floor,
        bid: BuildingId,
        rx0: &mut f32,
        rx1: &mut f32,
        ry0: &mut f32,
        ry1: &mut f32,
    ) {
        if fl == 0 {
            return;
        }
        for (dx, dy) in [(-1_i32, 0_i32), (1, 0), (0, -1), (0, 1)] {
            let stair_cur = Tile3D::new(p.x + dx, p.y + dy, fl);
            let Some(sc) = self.game.spatial_cell(stair_cur) else {
                continue;
            };
            if sc.building_id != bid
                || !matches!(sc.kind, TileKind::StairsDown | TileKind::StairsBoth)
            {
                continue;
            }
            let stair_below = Tile3D::new(p.x + dx, p.y + dy, fl - 1);
            let Some(sb) = self.game.spatial_cell(stair_below) else {
                continue;
            };
            if sb.building_id != bid
                || !matches!(sb.kind, TileKind::StairsUp | TileKind::StairsBoth)
            {
                continue;
            }
            let (low_dir, high_dir) = self.resolve_stair_high_dir(stair_below, sb);
            let from_stair = (-dx as i8, -dy as i8);
            if from_stair == low_dir || from_stair == high_dir {
                if dx < 0 {
                    *rx0 = (*rx0).max(p.x as f32 - 0.5 + STAIR_SLAB_TRIM);
                } else if dx > 0 {
                    *rx1 = (*rx1).min(p.x as f32 + 0.5 - STAIR_SLAB_TRIM);
                } else if dy < 0 {
                    *ry0 = (*ry0).max(p.y as f32 - 0.5 + STAIR_SLAB_TRIM);
                } else {
                    *ry1 = (*ry1).min(p.y as f32 + 0.5 - STAIR_SLAB_TRIM);
                }
            }
        }
    }
}

impl SpatialWorld for ActionSpatialView<'_> {
    fn terrain_at(&self, xy: Vec2) -> Option<TerrainSample> {
        self.game.world().terrain_at(xy)
    }

    fn supports_at(&self, xy: Vec2, out: &mut Vec<SupportSurface>) {
        out.clear();
        let Some(terrain) = self.terrain_at(xy) else {
            return;
        };
        let p = terrain.tile;
        let f0 = self.game.spatial_cell(Tile3D::from_pos(p, 0));
        let f0_surface = f0.map_or(Surface::Grass, |c| c.surface);

        if let Some(c0) = f0
            && c0.building_id > 0
            && c0.surface != Surface::Air
        {
            let foundation_z = self
                .game
                .building_by_id(c0.building_id)
                .map_or(0.0, |b| b.foundation_z);
            if matches!(c0.kind, TileKind::StairsUp | TileKind::StairsBoth)
                && self.game.max_floors() > 1
            {
                let stair_tile = Tile3D::from_pos(p, 0);
                let geom = self.build_stair_geometry(stair_tile, foundation_z, c0);
                let (z, normal) = geom.sample(xy);
                out.push(SupportSurface {
                    id: super::SupportId::Stair(stair_tile),
                    z,
                    normal,
                    walkable: true,
                    is_stair: true,
                    floor: 0,
                    surface: c0.surface,
                    water_z: None,
                });
            } else if self.inside_clipped_footprint(p, 0, c0.building_id, xy) {
                out.push(SupportSurface {
                    id: super::SupportId::Floor(Tile3D::from_pos(p, 0)),
                    z: foundation_z,
                    normal: Vec3::Z,
                    walkable: true,
                    is_stair: false,
                    floor: 0,
                    surface: c0.surface,
                    water_z: None,
                });
            } else {
                out.push(SupportSurface {
                    id: super::SupportId::Terrain,
                    z: terrain.z,
                    normal: terrain.normal,
                    walkable: !matches!(f0_surface, Surface::Water | Surface::Mountain)
                        && !terrain.is_submerged(),
                    is_stair: false,
                    floor: 0,
                    surface: f0_surface,
                    water_z: terrain.water_z,
                });
            }
        } else {
            out.push(SupportSurface {
                id: super::SupportId::Terrain,
                z: terrain.z,
                normal: terrain.normal,
                walkable: !matches!(f0_surface, Surface::Water | Surface::Mountain)
                    && !terrain.is_submerged(),
                is_stair: false,
                floor: 0,
                surface: f0_surface,
                water_z: terrain.water_z,
            });
        }

        if self.game.is_county()
            && let Some(deck) = self.game.world().bridge_deck_at(xy)
        {
            out.push(SupportSurface {
                id: super::SupportId::Bridge(p),
                z: deck.z,
                normal: deck.normal,
                walkable: true,
                is_stair: false,
                floor: 0,
                surface: Surface::Bridge,
                water_z: None,
            });
        }

        let max_floors = self.game.max_floors();
        for fl in 1..max_floors {
            let Some(cell) = self.game.spatial_cell(Tile3D::from_pos(p, fl)) else {
                continue;
            };
            if cell.surface == Surface::Air || cell.building_id == 0 {
                continue;
            }
            let foundation_z = self
                .game
                .building_by_id(cell.building_id)
                .map_or(0.0, |b| b.foundation_z);
            let z_fl = floor_elevation(foundation_z, fl);

            if cell.kind == TileKind::StairsBoth && fl + 1 < max_floors {
                let stair_tile = Tile3D::from_pos(p, fl);
                let geom = self.build_stair_geometry(stair_tile, z_fl, cell);
                let (z, normal) = geom.sample(xy);
                out.push(SupportSurface {
                    id: super::SupportId::Stair(stair_tile),
                    z,
                    normal,
                    walkable: true,
                    is_stair: true,
                    floor: fl,
                    surface: cell.surface,
                    water_z: None,
                });
            } else if !matches!(cell.kind, TileKind::StairsDown | TileKind::StairsBoth)
                && self.inside_clipped_footprint(p, fl, cell.building_id, xy)
            {
                out.push(SupportSurface {
                    id: super::SupportId::Floor(Tile3D::from_pos(p, fl)),
                    z: z_fl,
                    normal: Vec3::Z,
                    walkable: true,
                    is_stair: false,
                    floor: fl,
                    surface: cell.surface,
                    water_z: None,
                });
            }
        }
    }

    fn colliders_in(&self, bounds: Aabb3, out: &mut Vec<Collider>) {
        out.clear();
        let (play_min, play_max) = self.game.playable_bounds();
        let (min_tile, max_tile) = bounds.tile_bounds();
        let max_floors = self.game.max_floors();
        let world = self.game.world();

        for ty in min_tile.y..=max_tile.y {
            for tx in min_tile.x..=max_tile.x {
                if tx < play_min.x || tx > play_max.x || ty < play_min.y || ty > play_max.y {
                    emit_border_wall(tx, ty, bounds, out);
                    continue;
                }

                let p = Position::new(tx, ty);
                if self.game.is_county() {
                    emit_bridge_colliders(world, p, bounds, out);
                }

                let f0 = self.game.spatial_cell(Tile3D::from_pos(p, 0));
                if let Some(c0) = f0
                    && c0.surface == Surface::Water
                {
                    emit_water_hazard(world, p, bounds, out);
                }

                let terrain_z = world
                    .terrain_quad(p)
                    .map_or(0.0, |q| (q[0] + q[1] + q[2] + q[3]) * 0.25);

                for fl in 0..max_floors {
                    let tile = Tile3D::from_pos(p, fl);
                    let Some(cell) = self.game.spatial_cell(tile) else {
                        continue;
                    };
                    if cell.surface == Surface::Air {
                        continue;
                    }

                    let b_info = (cell.building_id > 0)
                        .then(|| self.game.building_by_id(cell.building_id))
                        .flatten();
                    let z_base = match b_info {
                        Some(b) => floor_elevation(b.foundation_z, fl),
                        None => terrain_z,
                    };

                    self.emit_tile_floor_colliders(tile, cell, z_base, b_info, bounds, out);

                    if let Some(b) = b_info {
                        if fl >= 1
                            && !matches!(cell.kind, TileKind::StairsDown | TileKind::StairsBoth)
                        {
                            let (mut rx0, mut rx1, mut ry0, mut ry1) =
                                self.clipped_footprint(p, fl, cell.building_id);
                            self.trim_slab_around_stairs(
                                p,
                                fl,
                                cell.building_id,
                                &mut rx0,
                                &mut rx1,
                                &mut ry0,
                                &mut ry1,
                            );
                            if rx0 < rx1 && ry0 < ry1 {
                                push_if_intersects(
                                    out,
                                    bounds,
                                    Collider {
                                        bounds: Aabb3::new(
                                            Vec3::new(rx0, ry0, z_base - SLAB_THICKNESS),
                                            Vec3::new(rx1, ry1, z_base),
                                        ),
                                        kind: ColliderKind::BuildingSlab {
                                            building_id: cell.building_id,
                                            floor: fl,
                                            is_roof: false,
                                        },
                                        material: Material::WoodLight,
                                    },
                                );
                            }
                        }

                        if matches!(cell.kind, TileKind::StairsDown | TileKind::StairsBoth) {
                            self.emit_stairwell_railings(tile, cell, z_base, bounds, out);
                        }

                        if fl + 1 == b.floors {
                            let z_eave = floor_elevation(b.foundation_z, b.floors);
                            let z_roof_top = z_eave + roof_rise_at(b, tx, ty);
                            let (rx0, rx1, ry0, ry1) =
                                self.clipped_footprint(p, fl, cell.building_id);
                            push_if_intersects(
                                out,
                                bounds,
                                Collider {
                                    bounds: Aabb3::new(
                                        Vec3::new(rx0, ry0, z_eave - SLAB_THICKNESS),
                                        Vec3::new(rx1, ry1, z_roof_top),
                                    ),
                                    kind: ColliderKind::BuildingSlab {
                                        building_id: cell.building_id,
                                        floor: b.floors,
                                        is_roof: true,
                                    },
                                    material: building_wall_material(b.style),
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    fn surface_quads_in(&self, min_tile: Position, max_tile: Position, out: &mut Vec<SurfaceQuad>) {
        out.clear();
        let world = self.game.world();
        let max_floors = self.game.max_floors();

        for ty in min_tile.y..=max_tile.y {
            for tx in min_tile.x..=max_tile.x {
                let p = Position::new(tx, ty);
                let x0 = tx as f32 - 0.5;
                let y0 = ty as f32 - 0.5;
                let x1 = tx as f32 + 0.5;
                let y1 = ty as f32 + 0.5;

                if let Some(corners) = world.terrain_quad(p) {
                    out.push(SurfaceQuad {
                        surface: HitSurface::Terrain { tile: p },
                        x0,
                        y0,
                        x1,
                        y1,
                        corners,
                        material: Material::None,
                    });
                }
                if self.game.is_county()
                    && let Some(corners) = world.bridge_quad(p)
                {
                    out.push(SurfaceQuad {
                        surface: HitSurface::BridgeDeck { tile: p },
                        x0,
                        y0,
                        x1,
                        y1,
                        corners,
                        material: Material::BrickMasonry,
                    });
                }

                for fl in 0..max_floors {
                    if fl + 1 >= max_floors {
                        continue;
                    }
                    let tile = Tile3D::from_pos(p, fl);
                    let Some(cell) = self.game.spatial_cell(tile) else {
                        continue;
                    };
                    if cell.building_id > 0
                        && cell.surface != Surface::Air
                        && matches!(cell.kind, TileKind::StairsUp | TileKind::StairsBoth)
                    {
                        let foundation_z = self
                            .game
                            .building_by_id(cell.building_id)
                            .map_or(0.0, |b| b.foundation_z);
                        let geom = self.build_stair_geometry(
                            tile,
                            floor_elevation(foundation_z, fl),
                            cell,
                        );
                        geom.push_quads(Material::WoodLight, out);
                    }
                }
            }
        }
    }
}
