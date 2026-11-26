//! Streaming `512 × 512 × 3` procedural county runtime implementing `CombatWorld`.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

use crate::{
    Building, BuildingKind, CHUNK_SIZE, CHUNKS_PER_SIDE, District, Feature, Ground, MonsterId,
    Position, RoomKind, WORLD_SIZE, World,
};

use super::ai::Monster;
use super::block::{
    Cell, DynamicFlash, INDOOR_AMBIENT, MAX_DYNAMIC_FLASHES, OUTDOOR_AMBIENT,
    PORCH_LIGHT_INTENSITY, PORCH_LIGHT_RADIUS, STREETLIGHT_F1_BLEED, STREETLIGHT_INTENSITY,
    STREETLIGHT_RADIUS, SupplyCache, VISIBILITY_RANGE, circle_walkable_world, damage_against,
    is_los_opaque_kind, is_room_barrier, light_cell_factor_kind, move_body_world, trace_grid_dda,
};
use super::types::{
    BuildingId, BuildingInfo, BuildingStyle, COUNTY_FLOORS, CacheId, CacheKind, CacheLoot,
    CacheView, CombatWorld, DamageKind, Floor, LightKind, LightSourceView, Material, PropKind,
    Surface, Tile3D, TileDamageResult, TileKind, TileObstruction, TileRenderInfo, TileVisibility,
    Vec2, WallOrientation, Weapon, mix64,
};

pub const ACTIVE_CHUNK_RADIUS: i32 = 3;
pub const CHUNKS_PER_WINDOW: usize = (ACTIVE_CHUNK_RADIUS * 2 + 1) as usize; // 7
pub const WINDOW_SIZE: i32 = CHUNK_SIZE * CHUNKS_PER_WINDOW as i32; // 112
const NUM_CHUNKS: usize = (CHUNKS_PER_SIDE * CHUNKS_PER_SIDE) as usize; // 1024
const WINDOW_CELLS: usize = (WINDOW_SIZE as usize) * (WINDOW_SIZE as usize) * COUNTY_FLOORS;
const PLAYER_COLLISION_RADIUS: f32 = 0.23;

#[derive(Clone, Copy, Debug)]
struct CountyLight {
    wx: i32,
    wy: i32,
    floor: Floor,
    radius: f32,
    intensity: u8,
    active: bool,
    kind: LightKind,
}

struct CountyChunk {
    cells: [[[Cell; CHUNK_SIZE as usize]; CHUNK_SIZE as usize]; COUNTY_FLOORS],
    lights: Vec<CountyLight>,
}

#[derive(Clone, Debug)]
struct WindowLightCache {
    version: u64,
    levels: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
struct ArmySubBox {
    id: BuildingId,
    min: Position,
    max: Position,
    room_kind: RoomKind,
}

#[derive(Clone, Copy, Debug)]
enum BuildingReg {
    Single(BuildingId),
    ArmySub([ArmySubBox; 3]),
}

pub struct CountyStage {
    seed: u64,
    wake: Position,
    chunks: Vec<Option<Box<CountyChunk>>>,
    hibernated_monsters: Vec<Vec<Monster>>,
    window_cx: i32,
    window_cy: i32,
    window_origin: Position,
    all_buildings: Vec<BuildingInfo>,
    registered_buildings: HashMap<(i32, i32, i32, i32), BuildingReg>,
    window_buildings: Vec<BuildingInfo>,
    caches: Vec<SupplyCache>,
    flashes: Vec<DynamicFlash>,
    light_cache: RefCell<WindowLightCache>,
    window_rooms: Vec<u16>,
    rooms_version: u64,
    currently_visible: Vec<Tile3D>,
    terrain_version: u64,
    pending_collapses: Vec<Tile3D>,
    starter_open_door: Option<Position>,
    next_monster_id: u32,
    total_monsters_spawned: usize,
}

impl CountyStage {
    pub fn new(world: &World, seed: u64) -> (Self, Tile3D, Vec<Monster>) {
        let wake = world.wake_position();
        let (wcx, wcy) = Self::window_focus_chunks(wake);
        let window_origin = Self::origin_for_chunks(wcx, wcy);

        let mut chunks = Vec::with_capacity(NUM_CHUNKS);
        chunks.resize_with(NUM_CHUNKS, || None);
        let hibernated_monsters = vec![Vec::new(); NUM_CHUNKS];

        let mut stage = Self {
            seed,
            wake,
            chunks,
            hibernated_monsters,
            window_cx: wcx,
            window_cy: wcy,
            window_origin,
            all_buildings: Vec::new(),
            registered_buildings: HashMap::new(),
            window_buildings: Vec::new(),
            caches: Vec::new(),
            flashes: Vec::new(),
            light_cache: RefCell::new(WindowLightCache {
                version: u64::MAX,
                levels: vec![0; WINDOW_CELLS],
            }),
            window_rooms: vec![0; WINDOW_CELLS],
            rooms_version: u64::MAX,
            currently_visible: Vec::with_capacity(512),
            terrain_version: 1,
            pending_collapses: Vec::new(),
            starter_open_door: None,
            // Generated infected use the low ID range; reserve the high range for runtime spawns.
            next_monster_id: 1 << 31,
            total_monsters_spawned: 0,
        };

        // Locate the nearest Residential building around wake so its front door opens cleanly.
        let mut nearest_res: Option<(u32, Building)> = None;
        for dy in -ACTIVE_CHUNK_RADIUS..=ACTIVE_CHUNK_RADIUS {
            for dx in -ACTIVE_CHUNK_RADIUS..=ACTIVE_CHUNK_RADIUS {
                if let Some(sites) = world.chunk_sites(wcx + dx, wcy + dy) {
                    for b in sites.buildings {
                        if b.district == District::Residential {
                            let d = b.entry.distance(wake);
                            if nearest_res.as_ref().is_none_or(|(bd, _)| d < *bd) {
                                nearest_res = Some((d, b.clone()));
                            }
                        }
                    }
                }
            }
        }
        stage.starter_open_door = nearest_res.as_ref().map(|(_, b)| b.entry);

        // Materialize initial 7x7 window chunks.
        stage.materialize_window_chunks(world, wcx, wcy);

        // Upgrade starter house's first cache to Pistol + 12 rounds and guarantee an outdoor Axe cache.
        if let Some((_, ref starter_b)) = nearest_res {
            stage.setup_starter_gear(starter_b, wake);
        }

        stage.rebuild_window_buildings();

        let mut active_monsters = Vec::new();
        stage.activate_window_monsters(&mut active_monsters);

        let spawn_tile = stage.find_walkable_wake(wake);
        (stage, spawn_tile, active_monsters)
    }

    #[inline]
    pub fn window_focus_chunks(p: Position) -> (i32, i32) {
        let wcx = p.x.clamp(0, WORLD_SIZE - 1).div_euclid(CHUNK_SIZE).clamp(
            ACTIVE_CHUNK_RADIUS,
            CHUNKS_PER_SIDE - 1 - ACTIVE_CHUNK_RADIUS,
        );
        let wcy = p.y.clamp(0, WORLD_SIZE - 1).div_euclid(CHUNK_SIZE).clamp(
            ACTIVE_CHUNK_RADIUS,
            CHUNKS_PER_SIDE - 1 - ACTIVE_CHUNK_RADIUS,
        );
        (wcx, wcy)
    }

    #[inline]
    const fn origin_for_chunks(wcx: i32, wcy: i32) -> Position {
        Position::new(
            (wcx - ACTIVE_CHUNK_RADIUS) * CHUNK_SIZE,
            (wcy - ACTIVE_CHUNK_RADIUS) * CHUNK_SIZE,
        )
    }

    #[inline]
    const fn chunk_idx(cx: i32, cy: i32) -> usize {
        ((cy << 5) | cx) as usize
    }

    #[inline]
    fn in_world(pos: Position) -> bool {
        (0..WORLD_SIZE).contains(&pos.x) && (0..WORLD_SIZE).contains(&pos.y)
    }

    #[inline]
    fn in_window(&self, pos: Position) -> bool {
        pos.x >= self.window_origin.x
            && pos.x < self.window_origin.x + WINDOW_SIZE
            && pos.y >= self.window_origin.y
            && pos.y < self.window_origin.y + WINDOW_SIZE
    }

    #[inline]
    fn window_cell_idx(&self, wx: i32, wy: i32, fl: usize) -> Option<usize> {
        let lx = wx - self.window_origin.x;
        let ly = wy - self.window_origin.y;
        if !(0..WINDOW_SIZE).contains(&lx) || !(0..WINDOW_SIZE).contains(&ly) || fl >= COUNTY_FLOORS
        {
            return None;
        }
        Some((fl * WINDOW_SIZE as usize + ly as usize) * WINDOW_SIZE as usize + lx as usize)
    }

    #[inline]
    fn cell_at_world(&self, tile: Tile3D) -> Option<&Cell> {
        if !Self::in_world(tile.pos()) || (tile.floor as usize) >= COUNTY_FLOORS {
            return None;
        }
        let cx = tile.x >> 4;
        let cy = tile.y >> 4;
        let lx = (tile.x & 15) as usize;
        let ly = (tile.y & 15) as usize;
        self.chunks[Self::chunk_idx(cx, cy)]
            .as_ref()
            .map(|c| &c.cells[tile.floor as usize][ly][lx])
    }

    #[inline]
    fn cell_mut_world(&mut self, tile: Tile3D) -> Option<&mut Cell> {
        if !Self::in_world(tile.pos()) || (tile.floor as usize) >= COUNTY_FLOORS {
            return None;
        }
        let cx = tile.x >> 4;
        let cy = tile.y >> 4;
        let lx = (tile.x & 15) as usize;
        let ly = (tile.y & 15) as usize;
        self.chunks[Self::chunk_idx(cx, cy)]
            .as_mut()
            .map(|c| &mut c.cells[tile.floor as usize][ly][lx])
    }

    pub fn sync_active_window(
        &mut self,
        world: &World,
        focus: Position,
        active_monsters: &mut Vec<Monster>,
    ) {
        let (new_wcx, new_wcy) = Self::window_focus_chunks(focus);
        if new_wcx == self.window_cx && new_wcy == self.window_cy {
            return;
        }
        self.window_cx = new_wcx;
        self.window_cy = new_wcy;
        self.window_origin = Self::origin_for_chunks(new_wcx, new_wcy);

        self.materialize_window_chunks(world, new_wcx, new_wcy);
        self.hibernate_outside_monsters(active_monsters);
        self.activate_window_monsters(active_monsters);
        self.rebuild_window_buildings();
        self.terrain_version += 1;
    }

    pub fn alloc_monster_id(&mut self) -> MonsterId {
        let id = MonsterId(self.next_monster_id);
        self.next_monster_id += 1;
        id
    }

    pub fn total_monsters_spawned(&self) -> usize {
        self.total_monsters_spawned
    }

    fn chunk_in_active_window(&self, cx: i32, cy: i32) -> bool {
        (self.window_cx - ACTIVE_CHUNK_RADIUS..=self.window_cx + ACTIVE_CHUNK_RADIUS).contains(&cx)
            && (self.window_cy - ACTIVE_CHUNK_RADIUS..=self.window_cy + ACTIVE_CHUNK_RADIUS)
                .contains(&cy)
    }

    fn hibernate_outside_monsters(&mut self, active_monsters: &mut Vec<Monster>) {
        let mut kept = Vec::with_capacity(active_monsters.len());
        for m in active_monsters.drain(..) {
            if !m.is_alive() && !m.is_swelling() {
                continue;
            }
            let pos = Tile3D::from_vec2(m.pos, m.floor).pos();
            let cx = pos.x.clamp(0, WORLD_SIZE - 1) >> 4;
            let cy = pos.y.clamp(0, WORLD_SIZE - 1) >> 4;
            if self.chunk_in_active_window(cx, cy) {
                kept.push(m);
            } else {
                self.hibernated_monsters[Self::chunk_idx(cx, cy)].push(m);
            }
        }
        *active_monsters = kept;
    }

    fn activate_window_monsters(&mut self, active_monsters: &mut Vec<Monster>) {
        for cy in (self.window_cy - ACTIVE_CHUNK_RADIUS)..=(self.window_cy + ACTIVE_CHUNK_RADIUS) {
            for cx in
                (self.window_cx - ACTIVE_CHUNK_RADIUS)..=(self.window_cx + ACTIVE_CHUNK_RADIUS)
            {
                let cidx = Self::chunk_idx(cx, cy);
                if !self.hibernated_monsters[cidx].is_empty() {
                    active_monsters.append(&mut self.hibernated_monsters[cidx]);
                }
            }
        }
    }

    fn materialize_window_chunks(&mut self, world: &World, wcx: i32, wcy: i32) {
        for cy in (wcy - ACTIVE_CHUNK_RADIUS)..=(wcy + ACTIVE_CHUNK_RADIUS) {
            for cx in (wcx - ACTIVE_CHUNK_RADIUS)..=(wcx + ACTIVE_CHUNK_RADIUS) {
                self.materialize_chunk(world, cx, cy);
            }
        }
    }

    fn rebuild_window_buildings(&mut self) {
        self.window_buildings.clear();
        let wx0 = self.window_origin.x;
        let wy0 = self.window_origin.y;
        let wx1 = wx0 + WINDOW_SIZE - 1;
        let wy1 = wy0 + WINDOW_SIZE - 1;
        for b in &self.all_buildings {
            let bx1 = b.x + b.width - 1;
            let by1 = b.y + b.height - 1;
            if b.x <= wx1 && bx1 >= wx0 && b.y <= wy1 && by1 >= wy0 {
                self.window_buildings.push(*b);
            }
        }
    }

    fn register_building(&mut self, b: &Building) -> BuildingReg {
        let key = (b.min.x, b.min.y, b.max.x, b.max.y);
        if let Some(&reg) = self.registered_buildings.get(&key) {
            return reg;
        }

        let reg = if b.kind == BuildingKind::ArmyBase && b.layout.len() == 3 {
            let mut boxes = [ArmySubBox {
                id: 0,
                min: Position::new(0, 0),
                max: Position::new(0, 0),
                room_kind: RoomKind::Barracks,
            }; 3];
            for (idx, room) in b.layout.iter().enumerate() {
                let sub_min = Position::new(room.min.x - 1, room.min.y - 1);
                let sub_max = Position::new(room.max.x + 1, room.max.y + 1);
                let door = find_subbox_door(b, sub_min, sub_max);
                let id = (self.all_buildings.len() + 1) as BuildingId;
                let name = match room.kind {
                    RoomKind::Barracks => "Army Barracks",
                    RoomKind::Armory => "Army Armory",
                    RoomKind::MotorPool => "Motor Pool",
                    _ => "Army Building",
                };
                self.all_buildings.push(BuildingInfo {
                    id,
                    name,
                    x: sub_min.x,
                    y: sub_min.y,
                    width: sub_max.x - sub_min.x + 1,
                    height: sub_max.y - sub_min.y + 1,
                    floors: 1,
                    style: BuildingStyle::ArmyBarracks,
                    front_door: door,
                    foundation_z: b.foundation_z,
                });
                boxes[idx] = ArmySubBox {
                    id,
                    min: sub_min,
                    max: sub_max,
                    room_kind: room.kind,
                };
            }
            BuildingReg::ArmySub(boxes)
        } else {
            let id = (self.all_buildings.len() + 1) as BuildingId;
            let (style, name) = building_style_and_name(b, self.seed);
            self.all_buildings.push(BuildingInfo {
                id,
                name,
                x: b.min.x,
                y: b.min.y,
                width: b.max.x - b.min.x + 1,
                height: b.max.y - b.min.y + 1,
                floors: b.floors.min(COUNTY_FLOORS as u8),
                style,
                front_door: b.entry,
                foundation_z: b.foundation_z,
            });
            BuildingReg::Single(id)
        };

        self.registered_buildings.insert(key, reg);
        reg
    }

    fn materialize_chunk(&mut self, world: &World, cx: i32, cy: i32) {
        if !(0..CHUNKS_PER_SIDE).contains(&cx) || !(0..CHUNKS_PER_SIDE).contains(&cy) {
            return;
        }
        let cidx = Self::chunk_idx(cx, cy);
        if self.chunks[cidx].is_some() {
            return;
        }
        let Some(sites) = world.chunk_sites(cx, cy) else {
            return;
        };

        let mut regs = Vec::with_capacity(sites.buildings.len());
        for b in &sites.buildings {
            regs.push((*b, self.register_building(b)));
        }

        let mut chunk = Box::new(CountyChunk {
            cells: [[[Cell::air(); CHUNK_SIZE as usize]; CHUNK_SIZE as usize]; COUNTY_FLOORS],
            lights: Vec::new(),
        });

        let base_x = cx * CHUNK_SIZE;
        let base_y = cy * CHUNK_SIZE;

        // 1. Outdoor Ground & Feature baseline on Floor 0.
        for ly in 0..CHUNK_SIZE as usize {
            let wy = base_y + ly as i32;
            for lx in 0..CHUNK_SIZE as usize {
                let wx = base_x + lx as i32;
                let p = Position::new(wx, wy);
                let tile = world.tile_at(p).expect("validated chunk world coords");

                let mut surface = match tile.ground {
                    Ground::Bridge => Surface::Bridge,
                    Ground::River | Ground::Lake => Surface::Water,
                    Ground::Mountain => Surface::Mountain,
                    Ground::Street => {
                        let town = matches!(
                            tile.district,
                            District::Residential
                                | District::City
                                | District::Hospital
                                | District::Police
                        );
                        if town
                            && tile.feature == Feature::Empty
                            && any_cardinal_neighbor(p, |q| {
                                world.tile_at(q).is_some_and(|n| !n.ground.is_road())
                            })
                        {
                            Surface::Sidewalk
                        } else {
                            Surface::Road
                        }
                    }
                    Ground::Road => Surface::Road,
                    Ground::Pavement => Surface::Sidewalk,
                    Ground::Trail => Surface::Trail,
                    Ground::Gravel => Surface::Gravel,
                    Ground::Forest => Surface::Forest,
                    Ground::Field => Surface::Field,
                    Ground::Grass => Surface::Grass,
                };

                let kind = match tile.feature {
                    Feature::Streetlight => {
                        chunk.lights.push(CountyLight {
                            wx,
                            wy,
                            floor: 0,
                            radius: STREETLIGHT_RADIUS,
                            intensity: STREETLIGHT_INTENSITY,
                            active: true,
                            kind: LightKind::Streetlight,
                        });
                        TileKind::Streetlight
                    }
                    Feature::Tree => TileKind::Prop(PropKind::BirchTree),
                    Feature::Fence => TileKind::Prop(PropKind::Fence),
                    Feature::Gate => {
                        surface = Surface::Driveway;
                        TileKind::Door {
                            open: true,
                            exterior: true,
                        }
                    }
                    Feature::Path => {
                        if matches!(tile.district, District::Residential | District::Woods)
                            && any_cardinal_neighbor(p, |q| {
                                world
                                    .tile_at(q)
                                    .is_some_and(|nt| nt.feature == Feature::Door)
                            })
                        {
                            surface = Surface::Porch;
                        } else if tile.ground == Ground::Pavement {
                            surface = Surface::Sidewalk;
                        } else {
                            surface = Surface::Gravel;
                        }
                        TileKind::Empty
                    }
                    Feature::Empty => {
                        if p != self.wake && p.distance(self.wake) > 1 {
                            if surface == Surface::Sidewalk
                                && !tile.cache
                                && matches!(
                                    tile.district,
                                    District::Residential
                                        | District::City
                                        | District::Hospital
                                        | District::Police
                                )
                                && ((wx.rem_euclid(16) == 0 && wy.rem_euclid(8) == 0)
                                    || (wy.rem_euclid(16) == 0 && wx.rem_euclid(8) == 0))
                                && !any_8_neighbor(p, |q| {
                                    world.tile_at(q).is_some_and(|nt| {
                                        matches!(
                                            nt.feature,
                                            Feature::Door
                                                | Feature::Gate
                                                | Feature::Path
                                                | Feature::Streetlight
                                        )
                                    })
                                })
                            {
                                chunk.lights.push(CountyLight {
                                    wx,
                                    wy,
                                    floor: 0,
                                    radius: STREETLIGHT_RADIUS,
                                    intensity: STREETLIGHT_INTENSITY,
                                    active: true,
                                    kind: LightKind::Streetlight,
                                });
                                TileKind::Streetlight
                            } else if tile.ground == Ground::Forest
                                && !tile.cache
                                && wx.rem_euclid(3) == 1
                                && wy.rem_euclid(3) == 1
                                && !mix64(
                                    self.seed
                                        ^ ((wx as u64 as u32 as u64) << 32)
                                        ^ (wy as u64 as u32 as u64),
                                )
                                .is_multiple_of(3)
                                && all_cardinal_neighbors(p, |q| {
                                    world.tile_at(q).is_some_and(|nt| {
                                        nt.ground == Ground::Forest && nt.feature == Feature::Empty
                                    })
                                })
                            {
                                TileKind::Prop(PropKind::BirchTree)
                            } else {
                                TileKind::Empty
                            }
                        } else {
                            TileKind::Empty
                        }
                    }
                    _ => TileKind::Empty,
                };

                let cell = &mut chunk.cells[0][ly][lx];
                cell.surface = surface;
                cell.set_kind(kind, Material::None);
            }
        }

        // 2. Stamp buildings intersecting this chunk.
        for (b, reg) in regs {
            let x0 = b.min.x.max(base_x);
            let x1 = b.max.x.min(base_x + CHUNK_SIZE - 1);
            let y0 = b.min.y.max(base_y);
            let y1 = b.max.y.min(base_y + CHUNK_SIZE - 1);

            match reg {
                BuildingReg::ArmySub(sub_boxes) => {
                    for wy in y0..=y1 {
                        let ly = (wy - base_y) as usize;
                        for wx in x0..=x1 {
                            let lx = (wx - base_x) as usize;
                            let p = Position::new(wx, wy);
                            let feat = b.feature_at(p, 0).unwrap_or(Feature::Empty);
                            if let Some(sub) = sub_boxes.iter().find(|s| {
                                p.x >= s.min.x && p.x <= s.max.x && p.y >= s.min.y && p.y <= s.max.y
                            }) {
                                let on_x = p.x == sub.min.x || p.x == sub.max.x;
                                let on_y = p.y == sub.min.y || p.y == sub.max.y;
                                let on_perimeter = on_x || on_y;
                                let is_corner = on_x && on_y;
                                let (kind, wall_mat) = match feat {
                                    Feature::Wall(_) => (
                                        if is_corner {
                                            TileKind::Pillar
                                        } else if on_perimeter {
                                            TileKind::ExteriorWall
                                        } else {
                                            TileKind::InteriorWall
                                        },
                                        b.material,
                                    ),
                                    Feature::Window => (
                                        if is_corner {
                                            TileKind::Pillar
                                        } else {
                                            TileKind::Window
                                        },
                                        b.material,
                                    ),
                                    Feature::Door => {
                                        if on_perimeter {
                                            chunk.lights.push(CountyLight {
                                                wx,
                                                wy,
                                                floor: 0,
                                                radius: PORCH_LIGHT_RADIUS,
                                                intensity: PORCH_LIGHT_INTENSITY,
                                                active: true,
                                                kind: LightKind::Porch,
                                            });
                                        }
                                        (
                                            TileKind::Door {
                                                open: false,
                                                exterior: on_perimeter,
                                            },
                                            Material::WoodDoor,
                                        )
                                    }
                                    Feature::Furniture => (
                                        TileKind::Prop(prop_for_room(Some(sub.room_kind), p)),
                                        Material::None,
                                    ),
                                    _ => (TileKind::Empty, Material::None),
                                };
                                let cell = &mut chunk.cells[0][ly][lx];
                                cell.building_id = sub.id;
                                cell.indoor = !on_perimeter;
                                cell.surface = Surface::Driveway;
                                cell.set_kind(kind, wall_mat);
                                cell.load_bearing = false;
                            } else {
                                // Open ArmyBase compound yard.
                                let on_compound_perim = wx == b.min.x
                                    || wx == b.max.x
                                    || wy == b.min.y
                                    || wy == b.max.y;
                                let has_site =
                                    sites.loot.iter().any(|s| s.floor == 0 && s.position == p)
                                        || sites
                                            .infected
                                            .iter()
                                            .any(|s| s.floor == 0 && s.position == p);
                                let cell = &mut chunk.cells[0][ly][lx];
                                cell.building_id = 0;
                                cell.indoor = false;
                                if p == b.entry || feat == Feature::Gate {
                                    cell.surface = Surface::Driveway;
                                    cell.set_kind(
                                        TileKind::Door {
                                            open: true,
                                            exterior: true,
                                        },
                                        Material::WoodDoor,
                                    );
                                    chunk.lights.push(CountyLight {
                                        wx,
                                        wy,
                                        floor: 0,
                                        radius: PORCH_LIGHT_RADIUS,
                                        intensity: PORCH_LIGHT_INTENSITY,
                                        active: true,
                                        kind: LightKind::Porch,
                                    });
                                } else if on_compound_perim && !has_site {
                                    cell.surface = Surface::Gravel;
                                    cell.set_kind(TileKind::Prop(PropKind::Fence), Material::None);
                                } else {
                                    cell.surface = if wx == b.entry.x || wy == b.entry.y {
                                        Surface::Driveway
                                    } else {
                                        Surface::Gravel
                                    };
                                    cell.set_kind(TileKind::Empty, Material::None);
                                }
                            }
                        }
                    }
                }
                BuildingReg::Single(bid) => {
                    let max_fl = b.floors.min(COUNTY_FLOORS as u8);
                    for floor in 0..max_fl {
                        let fl_idx = floor as usize;
                        for wy in y0..=y1 {
                            let ly = (wy - base_y) as usize;
                            for wx in x0..=x1 {
                                let lx = (wx - base_x) as usize;
                                let p = Position::new(wx, wy);
                                let feat = b.feature_at(p, floor).unwrap_or(Feature::Empty);
                                if feat == Feature::Empty {
                                    continue;
                                }

                                let w_out = b
                                    .feature_at(Position::new(wx - 1, wy), floor)
                                    .unwrap_or(Feature::Empty)
                                    == Feature::Empty;
                                let e_out = b
                                    .feature_at(Position::new(wx + 1, wy), floor)
                                    .unwrap_or(Feature::Empty)
                                    == Feature::Empty;
                                let n_out = b
                                    .feature_at(Position::new(wx, wy - 1), floor)
                                    .unwrap_or(Feature::Empty)
                                    == Feature::Empty;
                                let s_out = b
                                    .feature_at(Position::new(wx, wy + 1), floor)
                                    .unwrap_or(Feature::Empty)
                                    == Feature::Empty;
                                let on_x = w_out || e_out;
                                let on_y = n_out || s_out;
                                let on_perimeter = on_x || on_y;
                                let is_corner = on_x && on_y;

                                let room = room_at_building(b, p, floor);
                                let surface = surface_for_room(room);
                                let supports_above = floor + 1 < b.floors;

                                let (kind, wall_mat, load_bearing, stair_dir) = match feat {
                                    Feature::Wall(mat) => {
                                        if on_perimeter {
                                            (
                                                if is_corner {
                                                    TileKind::Pillar
                                                } else {
                                                    TileKind::ExteriorWall
                                                },
                                                b.material,
                                                supports_above,
                                                (0, 0),
                                            )
                                        } else {
                                            let imat = if mat == Material::MetalPole {
                                                Material::MetalPole
                                            } else {
                                                Material::Drywall
                                            };
                                            (TileKind::InteriorWall, imat, false, (0, 0))
                                        }
                                    }
                                    Feature::Window => {
                                        if is_corner {
                                            (TileKind::Pillar, b.material, supports_above, (0, 0))
                                        } else {
                                            (TileKind::Window, b.material, false, (0, 0))
                                        }
                                    }
                                    Feature::Door => {
                                        if floor == 0 && p == b.entry {
                                            chunk.lights.push(CountyLight {
                                                wx,
                                                wy,
                                                floor: 0,
                                                radius: PORCH_LIGHT_RADIUS,
                                                intensity: PORCH_LIGHT_INTENSITY,
                                                active: true,
                                                kind: LightKind::Porch,
                                            });
                                        }
                                        let open = floor == 0 && self.starter_open_door == Some(p);
                                        (
                                            TileKind::Door {
                                                open,
                                                exterior: on_perimeter,
                                            },
                                            Material::WoodDoor,
                                            false,
                                            (0, 0),
                                        )
                                    }
                                    Feature::Stairs => {
                                        let st_kind = match (floor, b.floors) {
                                            (0, _) => TileKind::StairsUp,
                                            (1, f) if f >= 3 => TileKind::StairsBoth,
                                            _ => TileKind::StairsDown,
                                        };
                                        let dir = compute_stair_dir(b, p, floor);
                                        (st_kind, Material::None, false, dir)
                                    }
                                    Feature::Furniture => (
                                        TileKind::Prop(prop_for_room(room, p)),
                                        Material::None,
                                        false,
                                        (0, 0),
                                    ),
                                    _ => (TileKind::Empty, Material::None, false, (0, 0)),
                                };

                                let cell = &mut chunk.cells[fl_idx][ly][lx];
                                cell.surface = surface;
                                cell.set_kind(kind, wall_mat);
                                cell.building_id = bid;
                                cell.indoor = !on_perimeter;
                                cell.load_bearing = load_bearing;
                                cell.stair_dir = stair_dir;
                            }
                        }
                    }
                }
            }
        }

        self.chunks[cidx] = Some(chunk);

        // 3. Materialize chunk-owned loot caches.
        for site in sites.loot {
            let district = world
                .tile_at(site.position)
                .map_or(District::Farm, |t| t.district);
            let (kind, loot) =
                roll_cache_loot(self.seed, site.position, site.floor, site.kind, district);
            let id = self.caches.len() as CacheId;
            self.caches.push(SupplyCache {
                id,
                pos: Tile3D::from_pos(site.position, site.floor),
                kind,
                loot,
                opened: false,
            });
        }

        // 4. Materialize chunk-owned infected spawns (skipping within 4.5 tiles of wake_position).
        let wake_vec = Vec2::new(self.wake.x as f32, self.wake.y as f32);
        for (spawn_index, site) in sites.infected.into_iter().enumerate() {
            let pos_vec = Vec2::new(site.position.x as f32, site.position.y as f32);
            if pos_vec.distance(wake_vec) < 4.5 {
                continue;
            }
            // Chunk and spawn index are stable regardless of the route taken through the county.
            // A chunk contains at most 3 * 16 * 16 spawn sites, well within this slot.
            let id = MonsterId(1 + (cidx as u32) * 1024 + spawn_index as u32);
            let h = mix64(
                self.seed
                    ^ (id.0 as u64)
                    ^ ((site.position.x as u64 as u32 as u64) << 16)
                    ^ (site.position.y as u64 as u32 as u64),
            );
            let variant = (h % 4) as u8;
            let spawn = Tile3D::from_pos(site.position, site.floor);
            self.hibernated_monsters[cidx].push(Monster::new(id, site.kind, variant, spawn));
            self.total_monsters_spawned += 1;
        }
    }

    fn setup_starter_gear(&mut self, starter_b: &Building, wake: Position) {
        // Upgrade the nearest Residential house's first cache to Pistol + 12 rounds.
        let best_idx = self
            .caches
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.pos.x >= starter_b.min.x
                    && c.pos.x <= starter_b.max.x
                    && c.pos.y >= starter_b.min.y
                    && c.pos.y <= starter_b.max.y
            })
            .min_by_key(|(_, c)| (c.pos.floor, c.id))
            .map(|(i, _)| i);

        if let Some(idx) = best_idx {
            self.caches[idx].kind = CacheKind::Weapon;
            self.caches[idx].loot = CacheLoot {
                weapon: Some(Weapon::Pistol),
                pistol_ammo: 12,
                ..CacheLoot::default()
            };
        }

        // Guarantee a starter Axe cache on a walkable approach tile outside starter_b.entry.
        let wake_vec = Vec2::new(wake.x as f32, wake.y as f32);
        let has_nearby_axe = self.caches.iter().any(|c| {
            c.pos.floor == 0
                && c.loot.weapon == Some(Weapon::Axe)
                && c.pos.center().distance(wake_vec) <= 8.0
        });
        if has_nearby_axe {
            return;
        }

        if let Some(axe_pos) = self.find_starter_axe_tile(starter_b, wake) {
            let id = self.caches.len() as CacheId;
            self.caches.push(SupplyCache {
                id,
                pos: Tile3D::from_pos(axe_pos, 0),
                kind: CacheKind::Weapon,
                loot: CacheLoot {
                    weapon: Some(Weapon::Axe),
                    ..CacheLoot::default()
                },
                opened: false,
            });
        }
    }

    fn find_starter_axe_tile(&self, starter_b: &Building, wake: Position) -> Option<Position> {
        let mut candidates: Vec<(u32, u32, Position)> = Vec::new();
        for dy in -5..=5_i32 {
            for dx in -5..=5_i32 {
                let p = Position::new(starter_b.entry.x + dx, starter_b.entry.y + dy);
                if p == starter_b.entry || p == wake {
                    continue;
                }
                let t = Tile3D::from_pos(p, 0);
                let Some(cell) = self.cell_at_world(t) else {
                    continue;
                };
                if cell.building_id != 0 {
                    continue;
                }
                if !self.is_walkable_world(t, false)
                    || !self.circle_walkable_world_pos(t.center(), 0, PLAYER_COLLISION_RADIUS)
                {
                    continue;
                }
                if self.caches.iter().any(|c| c.pos == t) {
                    continue;
                }
                let d_entry = p.distance(starter_b.entry);
                let d_wake = p.distance(wake);
                let surface_bias = u32::from(!matches!(
                    cell.surface,
                    Surface::Porch | Surface::Sidewalk | Surface::Gravel
                ));
                candidates.push((surface_bias * 10 + d_entry, d_wake, p));
            }
        }
        candidates.sort_by_key(|&(score, d_wake, p)| (score, d_wake, p.y, p.x));
        candidates.first().map(|&(_, _, p)| p)
    }

    fn find_walkable_wake(&self, wake: Position) -> Tile3D {
        let t0 = Tile3D::from_pos(wake, 0);
        if self.is_walkable_world(t0, false)
            && self.circle_walkable_world_pos(t0.center(), 0, PLAYER_COLLISION_RADIUS)
        {
            return t0;
        }
        for r in 1..=6_i32 {
            let mut ring: Vec<(i32, Position)> = Vec::new();
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) != r {
                        continue;
                    }
                    let p = Position::new(wake.x + dx, wake.y + dy);
                    let t = Tile3D::from_pos(p, 0);
                    if self.is_walkable_world(t, false)
                        && self.circle_walkable_world_pos(t.center(), 0, PLAYER_COLLISION_RADIUS)
                    {
                        ring.push((dx * dx + dy * dy, p));
                    }
                }
            }
            ring.sort_by_key(|&(d2, p)| (d2, p.y, p.x));
            if let Some(&(_, p)) = ring.first() {
                return Tile3D::from_pos(p, 0);
            }
        }
        t0
    }

    pub fn find_teleport_destination(&mut self, world: &World, target: Tile3D) -> Option<Tile3D> {
        let tx = target.x.clamp(0, WORLD_SIZE - 1);
        let ty = target.y.clamp(0, WORLD_SIZE - 1);
        let tfloor = target.floor.min((COUNTY_FLOORS - 1) as u8);
        let clamped = Tile3D::new(tx, ty, tfloor);

        // Ensure target chunk and immediate neighbor chunks are materialized without disturbing active_monsters.
        let tcx = tx >> 4;
        let tcy = ty >> 4;
        for dy in -2..=2 {
            for dx in -2..=2 {
                self.materialize_chunk(world, tcx + dx, tcy + dy);
            }
        }

        // 1. Exact tile check.
        if self.is_walkable_world(clamped, false)
            && self.circle_walkable_world_pos(clamped.center(), tfloor, PLAYER_COLLISION_RADIUS)
        {
            return Some(clamped);
        }

        // 2. Check if (tx, ty) falls inside a registered building's bounding box.
        let matching_building = self
            .all_buildings
            .iter()
            .find(|b| tx >= b.x && tx < b.x + b.width && ty >= b.y && ty < b.y + b.height)
            .copied();

        if let Some(b) = matching_building {
            let f = tfloor.min(b.floors.saturating_sub(1));
            let mut inside_cands: Vec<(i32, Position)> = Vec::new();
            for dy in -12..=12_i32 {
                for dx in -12..=12_i32 {
                    if dx * dx + dy * dy > 144 {
                        continue;
                    }
                    let px = tx + dx;
                    let py = ty + dy;
                    if px < b.x || px >= b.x + b.width || py < b.y || py >= b.y + b.height {
                        continue;
                    }
                    let cand = Tile3D::new(px, py, f);
                    if self.is_walkable_world(cand, false)
                        && self.circle_walkable_world_pos(cand.center(), f, PLAYER_COLLISION_RADIUS)
                    {
                        inside_cands.push((dx * dx + dy * dy, Position::new(px, py)));
                    }
                }
            }
            inside_cands.sort_by_key(|&(d2, p)| (d2, p.y, p.x));
            if let Some(&(_, p)) = inside_cands.first() {
                return Some(Tile3D::from_pos(p, f));
            }

            // Fallback to building front_door on floor 0 or r = 1..=4 around front_door.
            let door_t = Tile3D::from_pos(b.front_door, 0);
            if self.is_walkable_world(door_t, false)
                && self.circle_walkable_world_pos(door_t.center(), 0, PLAYER_COLLISION_RADIUS)
            {
                return Some(door_t);
            }
            let mut door_cands: Vec<(i32, Position)> = Vec::new();
            for dy in -4..=4_i32 {
                for dx in -4..=4_i32 {
                    let cand = Tile3D::new(b.front_door.x + dx, b.front_door.y + dy, 0);
                    if self.is_walkable_world(cand, false)
                        && self.circle_walkable_world_pos(cand.center(), 0, PLAYER_COLLISION_RADIUS)
                    {
                        door_cands.push((dx * dx + dy * dy, cand.pos()));
                    }
                }
            }
            door_cands.sort_by_key(|&(d2, p)| (d2, p.y, p.x));
            if let Some(&(_, p)) = door_cands.first() {
                return Some(Tile3D::from_pos(p, 0));
            }
        }

        // 3. Concentric r = 1..=8 on floor 0.
        for r in 1..=8_i32 {
            let mut ring: Vec<(i32, Position)> = Vec::new();
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) != r {
                        continue;
                    }
                    let cand = Tile3D::new(tx + dx, ty + dy, 0);
                    if self.is_walkable_world(cand, false)
                        && self.circle_walkable_world_pos(cand.center(), 0, PLAYER_COLLISION_RADIUS)
                    {
                        ring.push((dx * dx + dy * dy, cand.pos()));
                    }
                }
            }
            ring.sort_by_key(|&(d2, p)| (d2, p.y, p.x));
            if let Some(&(_, p)) = ring.first() {
                return Some(Tile3D::from_pos(p, 0));
            }
        }

        None
    }

    fn is_walkable_world(&self, tile: Tile3D, can_vault_window: bool) -> bool {
        let Some(cell) = self.cell_at_world(tile) else {
            return false;
        };
        if cell.surface == Surface::Air
            || matches!(cell.surface, Surface::Water | Surface::Mountain)
        {
            return false;
        }
        match cell.kind {
            TileKind::Empty
            | TileKind::Rubble
            | TileKind::StairsUp
            | TileKind::StairsDown
            | TileKind::StairsBoth
            | TileKind::BrokenStreetlight
            | TileKind::Door { open: true, .. } => true,
            TileKind::BrokenWindow => can_vault_window && tile.floor == 0,
            _ => false,
        }
    }

    fn circle_walkable_world_pos(&self, center: Vec2, floor: Floor, radius: f32) -> bool {
        let min_x = (center.x - radius - 0.5).floor() as i32;
        let max_x = (center.x + radius + 0.5).ceil() as i32;
        let min_y = (center.y - radius - 0.5).floor() as i32;
        let max_y = (center.y + radius + 0.5).ceil() as i32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let tile = Tile3D::new(x, y, floor);
                if self.is_walkable_world(tile, false) {
                    continue;
                }
                let nearest_x = center.x.clamp(x as f32 - 0.5, x as f32 + 0.5);
                let nearest_y = center.y.clamp(y as f32 - 0.5, y as f32 + 0.5);
                let dx = center.x - nearest_x;
                let dy = center.y - nearest_y;
                if dx * dx + dy * dy < radius * radius {
                    return false;
                }
            }
        }
        true
    }

    // --- Public Query & Render Helpers ---

    pub fn buildings(&self) -> &[BuildingInfo] {
        &self.window_buildings
    }

    pub(crate) fn building_by_id(&self, id: BuildingId) -> Option<&BuildingInfo> {
        self.all_buildings.get(id.checked_sub(1)? as usize)
    }

    pub(crate) fn playable_bounds(&self) -> (Position, Position) {
        (
            Position::new(0, 0),
            Position::new(WORLD_SIZE - 1, WORLD_SIZE - 1),
        )
    }

    pub(crate) fn spatial_cell(
        &self,
        tile: Tile3D,
        world: &World,
    ) -> Option<super::types::SpatialCell> {
        if !Self::in_world(tile.pos()) || (tile.floor as usize) >= COUNTY_FLOORS {
            return None;
        }
        if let Some(cell) = self.cell_at_world(tile) {
            return Some(super::types::SpatialCell {
                surface: cell.surface,
                kind: cell.kind,
                material: cell.material,
                building_id: cell.building_id,
                orientation: self.wall_orientation(tile.x, tile.y, tile.floor),
                stair_dir: cell.stair_dir,
            });
        }
        if tile.floor == 0 {
            let wt = world.tile_at(tile.pos())?;
            let surface = match wt.ground {
                Ground::Bridge => Surface::Bridge,
                Ground::River | Ground::Lake => Surface::Water,
                Ground::Mountain => Surface::Mountain,
                Ground::Street | Ground::Road => Surface::Road,
                Ground::Pavement => Surface::Sidewalk,
                Ground::Trail => Surface::Trail,
                Ground::Gravel => Surface::Gravel,
                Ground::Forest => Surface::Forest,
                Ground::Field => Surface::Field,
                Ground::Grass => Surface::Grass,
            };
            let kind = match wt.feature {
                Feature::Streetlight => TileKind::Streetlight,
                Feature::Tree => TileKind::Prop(PropKind::BirchTree),
                Feature::Fence => TileKind::Prop(PropKind::Fence),
                Feature::Gate => TileKind::Door {
                    open: true,
                    exterior: true,
                },
                _ => TileKind::Empty,
            };
            let (material, _) = super::block::tile_spec(kind, Material::None);
            let orientation = if kind == TileKind::Prop(PropKind::Fence) {
                let is_fence = |nx: i32, ny: i32| -> bool {
                    world
                        .tile_at(Position::new(nx, ny))
                        .is_some_and(|nt| nt.feature == Feature::Fence)
                };
                let has_x = is_fence(tile.x - 1, tile.y) || is_fence(tile.x + 1, tile.y);
                let has_y = is_fence(tile.x, tile.y - 1) || is_fence(tile.x, tile.y + 1);
                match (has_x, has_y) {
                    (true, true) => WallOrientation::Corner,
                    (false, true) => WallOrientation::AlongY,
                    _ => WallOrientation::AlongX,
                }
            } else {
                WallOrientation::AlongX
            };
            return Some(super::types::SpatialCell {
                surface,
                kind,
                material,
                building_id: 0,
                orientation,
                stair_dir: (0, 0),
            });
        }
        None
    }

    pub fn caches(&self) -> &[SupplyCache] {
        &self.caches
    }

    pub fn take_cache(&mut self, id: CacheId) -> Option<CacheLoot> {
        let cache = self.caches.get_mut(id as usize)?;
        if cache.id != id || cache.opened {
            return None;
        }
        cache.opened = true;
        Some(cache.loot)
    }

    pub fn cache_views(
        &self,
        player_pos: Vec2,
        player_floor: Floor,
        interact_radius: f32,
    ) -> Vec<CacheView> {
        self.caches
            .iter()
            .filter(|c| self.in_window(c.pos.pos()))
            .map(|c| CacheView {
                id: c.id,
                pos: c.pos,
                kind: c.kind,
                loot: c.loot,
                opened: c.opened,
                nearby: !c.opened
                    && c.pos.floor == player_floor
                    && c.pos.center().distance(player_pos) <= interact_radius,
            })
            .collect()
    }

    pub fn tile_render_info(&self, pos: Position, floor: Floor) -> Option<TileRenderInfo> {
        if !self.in_window(pos) {
            return None;
        }
        let tile = Tile3D::from_pos(pos, floor);
        let cell = self.cell_at_world(tile)?;
        let light = self.effective_light_u8(tile);
        let orientation = self.wall_orientation(pos.x, pos.y, floor);
        Some(TileRenderInfo {
            surface: cell.surface,
            kind: cell.kind,
            material: cell.material,
            hp: cell.hp,
            max_hp: cell.max_hp,
            building_id: cell.building_id,
            indoor: cell.indoor,
            light,
            visibility: cell.visibility,
            orientation,
            stair_dir: cell.stair_dir,
        })
    }

    pub fn light_sources(&self) -> Vec<LightSourceView> {
        let mut views = Vec::new();
        for cy in (self.window_cy - ACTIVE_CHUNK_RADIUS)..=(self.window_cy + ACTIVE_CHUNK_RADIUS) {
            for cx in
                (self.window_cx - ACTIVE_CHUNK_RADIUS)..=(self.window_cx + ACTIVE_CHUNK_RADIUS)
            {
                if let Some(chunk) = &self.chunks[Self::chunk_idx(cx, cy)] {
                    for light in &chunk.lights {
                        views.push(LightSourceView {
                            pos: Vec2::new(light.wx as f32, light.wy as f32),
                            floor: light.floor,
                            radius: light.radius,
                            intensity: light.intensity,
                            active: light.active,
                            kind: light.kind,
                        });
                    }
                }
            }
        }
        for cache in &self.caches {
            if self.in_window(cache.pos.pos()) {
                views.push(LightSourceView {
                    pos: cache.pos.center(),
                    floor: cache.pos.floor,
                    radius: 1.8,
                    intensity: 48,
                    active: !cache.opened,
                    kind: LightKind::Cache,
                });
            }
        }
        views
    }

    pub fn is_indoor(&self, tile: Tile3D) -> bool {
        self.cell_at_world(tile).is_some_and(|c| c.indoor)
    }

    pub fn tile_kind(&self, tile: Tile3D) -> TileKind {
        self.cell_at_world(tile)
            .map_or(TileKind::ExteriorWall, |c| c.kind)
    }

    pub fn tick_flashes(&mut self) {
        for flash in &mut self.flashes {
            flash.ttl_ticks = flash.ttl_ticks.saturating_sub(1);
        }
        self.flashes.retain(|flash| flash.ttl_ticks > 0);
    }

    pub fn take_collapsed_tiles(&mut self) -> Vec<Tile3D> {
        std::mem::take(&mut self.pending_collapses)
    }

    pub fn circle_walkable(
        &self,
        center: Vec2,
        floor: Floor,
        radius: f32,
        can_vault_window: bool,
    ) -> bool {
        circle_walkable_world(self, center, floor, radius, can_vault_window)
    }

    pub fn move_body(
        &self,
        position: &mut Vec2,
        floor: Floor,
        delta: Vec2,
        radius: f32,
        can_vault_window: bool,
        doorway_assist: bool,
    ) {
        move_body_world(
            self,
            position,
            floor,
            delta,
            radius,
            can_vault_window,
            doorway_assist,
        );
    }

    pub fn effective_light_u8(&self, tile: Tile3D) -> u8 {
        let Some(idx) = self.window_cell_idx(tile.x, tile.y, tile.floor as usize) else {
            return OUTDOOR_AMBIENT as u8;
        };
        self.ensure_static_light();
        let base = self.light_cache.borrow().levels[idx] as u16;
        if self.flashes.is_empty() {
            return base as u8;
        }
        let center = tile.center();
        let mut extra = 0.0_f32;
        for flash in &self.flashes {
            if flash.ttl_ticks == 0 || flash.radius <= 0.0 {
                continue;
            }
            let floor_scale = if flash.floor == tile.floor { 1.0 } else { 0.25 };
            let dist = center.distance(flash.pos);
            if dist < flash.radius {
                let att = (1.0 - dist / flash.radius) * floor_scale;
                extra += flash.intensity as f32 * att;
            }
        }
        (base + extra.round() as u16).min(255) as u8
    }

    fn wall_orientation(&self, wx: i32, wy: i32, fl: Floor) -> WallOrientation {
        let Some(cell) = self.cell_at_world(Tile3D::new(wx, wy, fl)) else {
            return WallOrientation::AlongX;
        };
        match cell.kind {
            TileKind::Pillar => WallOrientation::Corner,
            TileKind::ExteriorWall
            | TileKind::InteriorWall
            | TileKind::Window
            | TileKind::BrokenWindow
            | TileKind::Door { .. } => {
                let same_wall = |nx: i32, ny: i32| -> bool {
                    self.cell_at_world(Tile3D::new(nx, ny, fl))
                        .is_some_and(|other| {
                            other.building_id == cell.building_id && is_room_barrier(other.kind)
                        })
                };
                let has_x = same_wall(wx - 1, wy) || same_wall(wx + 1, wy);
                let has_y = same_wall(wx, wy - 1) || same_wall(wx, wy + 1);
                match (has_x, has_y) {
                    (true, true) => WallOrientation::Corner,
                    (false, true) => WallOrientation::AlongY,
                    _ => WallOrientation::AlongX,
                }
            }
            TileKind::Prop(PropKind::Fence) => {
                let is_fence = |nx: i32, ny: i32| -> bool {
                    self.cell_at_world(Tile3D::new(nx, ny, fl))
                        .is_some_and(|c| c.kind == TileKind::Prop(PropKind::Fence))
                };
                let has_x = is_fence(wx - 1, wy) || is_fence(wx + 1, wy);
                let has_y = is_fence(wx, wy - 1) || is_fence(wx, wy + 1);
                match (has_x, has_y) {
                    (true, true) => WallOrientation::Corner,
                    (false, true) => WallOrientation::AlongY,
                    _ => WallOrientation::AlongX,
                }
            }
            _ => WallOrientation::AlongX,
        }
    }

    fn ensure_static_light(&self) {
        let mut cache = self.light_cache.borrow_mut();
        if cache.version == self.terrain_version {
            return;
        }
        let mut accum = vec![0.0_f32; WINDOW_CELLS];
        let ox = self.window_origin.x;
        let oy = self.window_origin.y;
        let w = WINDOW_SIZE as usize;

        for fl in 0..COUNTY_FLOORS {
            for ly in 0..w {
                let wy = oy + ly as i32;
                for lx in 0..w {
                    let wx = ox + lx as i32;
                    let idx = (fl * w + ly) * w + lx;
                    let indoor = self
                        .cell_at_world(Tile3D::new(wx, wy, fl as Floor))
                        .is_some_and(|c| c.indoor);
                    accum[idx] = if indoor {
                        INDOOR_AMBIENT
                    } else {
                        OUTDOOR_AMBIENT
                    };
                }
            }
        }

        let max_wx = ox + WINDOW_SIZE - 1;
        let max_wy = oy + WINDOW_SIZE - 1;

        for cy in (self.window_cy - ACTIVE_CHUNK_RADIUS)..=(self.window_cy + ACTIVE_CHUNK_RADIUS) {
            for cx in
                (self.window_cx - ACTIVE_CHUNK_RADIUS)..=(self.window_cx + ACTIVE_CHUNK_RADIUS)
            {
                let Some(chunk) = &self.chunks[Self::chunk_idx(cx, cy)] else {
                    continue;
                };
                for light in &chunk.lights {
                    if !light.active || light.intensity == 0 || light.radius <= 0.0 {
                        continue;
                    }
                    let r_ceil = light.radius.ceil() as i32;
                    let min_x = (light.wx - r_ceil).max(ox);
                    let max_x = (light.wx + r_ceil).min(max_wx);
                    let min_y = (light.wy - r_ceil).max(oy);
                    let max_y = (light.wy + r_ceil).min(max_wy);
                    let fl = light.floor;
                    let src_closed_door = self
                        .cell_at_world(Tile3D::new(light.wx, light.wy, fl))
                        .is_some_and(|c| matches!(c.kind, TileKind::Door { open: false, .. }));

                    for wy in min_y..=max_y {
                        for wx in min_x..=max_x {
                            let dx = (wx - light.wx) as f32;
                            let dy = (wy - light.wy) as f32;
                            let dist = (dx * dx + dy * dy).sqrt();
                            if dist >= light.radius {
                                continue;
                            }
                            let falloff = (1.0 - dist / light.radius).powf(1.35);

                            if let Some(target_f0) = self.cell_at_world(Tile3D::new(wx, wy, fl))
                                && !(src_closed_door && target_f0.indoor)
                            {
                                let transmit =
                                    self.light_transmittance(light.wx, light.wy, wx, wy, fl);
                                if transmit > 0.0
                                    && let Some(idx) = self.window_cell_idx(wx, wy, fl as usize)
                                {
                                    accum[idx] += light.intensity as f32 * falloff * transmit;
                                }
                            }

                            if light.kind == LightKind::Streetlight
                                && fl == 0
                                && let Some(f1_cell) = self.cell_at_world(Tile3D::new(wx, wy, 1))
                            {
                                let eligible = matches!(
                                    f1_cell.kind,
                                    TileKind::Window | TileKind::BrokenWindow
                                ) || f1_cell.surface == Surface::Air
                                    || f1_cell.indoor;
                                if eligible {
                                    let transmit_f1 =
                                        self.light_transmittance(light.wx, light.wy, wx, wy, 1);
                                    if transmit_f1 > 0.0
                                        && let Some(idx) = self.window_cell_idx(wx, wy, 1)
                                    {
                                        accum[idx] += light.intensity as f32
                                            * STREETLIGHT_F1_BLEED
                                            * falloff
                                            * transmit_f1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        for (out, val) in cache.levels.iter_mut().zip(accum) {
            *out = val.round().clamp(0.0, 255.0) as u8;
        }
        cache.version = self.terrain_version;
    }

    fn light_transmittance(&self, sx: i32, sy: i32, tx: i32, ty: i32, fl: Floor) -> f32 {
        if sx == tx && sy == ty {
            return 1.0;
        }
        let direct = self.trace_light_ray(sx, sy, tx, ty, fl);
        if direct > 0.0 {
            return direct;
        }
        if self
            .cell_at_world(Tile3D::new(tx, ty, fl))
            .is_some_and(|c| matches!(c.kind, TileKind::Window | TileKind::BrokenWindow))
        {
            const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
            let ldx = sx - tx;
            let ldy = sy - ty;
            let mut best = 0.0_f32;
            for (nx, ny) in DIRS {
                if nx * ldx + ny * ldy <= 0 {
                    continue;
                }
                let ox = tx + nx;
                let oy = ty + ny;
                if !self.in_window(Position::new(ox, oy)) {
                    continue;
                }
                if self
                    .cell_at_world(Tile3D::new(ox, oy, fl))
                    .is_some_and(|c| c.building_id == 0)
                    && self.light_cell_factor(ox, oy, fl) > 0.0
                {
                    best = best.max(self.trace_light_ray(sx, sy, ox, oy, fl));
                }
            }
            return best;
        }
        0.0
    }

    fn trace_light_ray(&self, sx: i32, sy: i32, tx: i32, ty: i32, fl: Floor) -> f32 {
        if sx == tx && sy == ty {
            return 1.0;
        }
        let mut transmittance = 1.0_f32;
        let from = Vec2::new(sx as f32, sy as f32);
        let to = Vec2::new(tx as f32, ty as f32);
        let clear = trace_grid_dda(
            from,
            to,
            (WINDOW_SIZE as usize) * 4,
            |cx, cy, corner_a, corner_b| {
                if (cx == sx && cy == sy) || (cx == tx && cy == ty) {
                    return true;
                }
                if let (Some((ax, ay)), Some((bx, by))) = (corner_a, corner_b)
                    && self.light_cell_factor(ax, ay, fl) <= 0.0
                    && self.light_cell_factor(bx, by, fl) <= 0.0
                {
                    transmittance = 0.0;
                    return false;
                }
                let f = self.light_cell_factor(cx, cy, fl);
                transmittance *= f;
                transmittance > 0.001
            },
        );
        if clear { transmittance } else { 0.0 }
    }

    fn light_cell_factor(&self, wx: i32, wy: i32, fl: Floor) -> f32 {
        let Some(cell) = self.cell_at_world(Tile3D::new(wx, wy, fl)) else {
            return 0.0;
        };
        if cell.surface == Surface::Mountain {
            return 0.0;
        }
        light_cell_factor_kind(cell.kind)
    }

    fn ensure_window_rooms(&mut self) {
        if self.rooms_version == self.terrain_version {
            return;
        }
        self.window_rooms.fill(0);
        let ox = self.window_origin.x;
        let oy = self.window_origin.y;
        let w = WINDOW_SIZE as usize;
        let mut next_room = 1u16;
        let mut queue = VecDeque::new();
        const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

        for fl in 0..COUNTY_FLOORS {
            for ly in 0..w {
                let wy = oy + ly as i32;
                for lx in 0..w {
                    let wx = ox + lx as i32;
                    let idx = (fl * w + ly) * w + lx;
                    if self.window_rooms[idx] != 0 {
                        continue;
                    }
                    let Some(&start) = self.cell_at_world(Tile3D::new(wx, wy, fl as Floor)) else {
                        continue;
                    };
                    if !start.indoor || start.building_id == 0 || is_room_barrier(start.kind) {
                        continue;
                    }
                    let room_id = next_room;
                    next_room = next_room.saturating_add(1);
                    self.window_rooms[idx] = room_id;
                    queue.push_back((lx as i32, ly as i32));

                    while let Some((cx, cy)) = queue.pop_front() {
                        for (dx, dy) in DIRS {
                            let nlx = cx + dx;
                            let nly = cy + dy;
                            if !(0..WINDOW_SIZE).contains(&nlx) || !(0..WINDOW_SIZE).contains(&nly)
                            {
                                continue;
                            }
                            let nidx = (fl * w + nly as usize) * w + nlx as usize;
                            if self.window_rooms[nidx] != 0 {
                                continue;
                            }
                            let Some(&ncell) =
                                self.cell_at_world(Tile3D::new(ox + nlx, oy + nly, fl as Floor))
                            else {
                                continue;
                            };
                            if ncell.indoor
                                && ncell.building_id == start.building_id
                                && !is_room_barrier(ncell.kind)
                            {
                                self.window_rooms[nidx] = room_id;
                                queue.push_back((nlx, nly));
                            }
                        }
                    }
                }
            }
        }
        self.rooms_version = self.terrain_version;
    }

    pub fn update_visibility(&mut self, viewer: Vec2, floor: Floor) {
        if (floor as usize) >= COUNTY_FLOORS {
            return;
        }
        self.ensure_window_rooms();

        let mut visible_buf = std::mem::take(&mut self.currently_visible);
        for tile in visible_buf.drain(..) {
            if let Some(cell) = self.cell_mut_world(tile)
                && cell.visibility == TileVisibility::Visible
            {
                cell.visibility = TileVisibility::Explored;
            }
        }

        let vx = viewer.x.round() as i32;
        let vy = viewer.y.round() as i32;
        let fl = floor as usize;

        let mut revealed_rooms: Vec<(u16, BuildingId)> = Vec::new();
        let ox = self.window_origin.x;
        let oy = self.window_origin.y;
        let w = WINDOW_SIZE as usize;

        if let Some(room_idx) = self.window_cell_idx(vx, vy, fl) {
            let room_id = self.window_rooms[room_idx];
            let v_tile = Tile3D::new(vx, vy, floor);
            if let Some(cell) = self.cell_mut_world(v_tile) {
                let bid = cell.building_id;
                if cell.visibility != TileVisibility::Visible {
                    cell.visibility = TileVisibility::Visible;
                    visible_buf.push(v_tile);
                }
                if room_id != 0
                    && !revealed_rooms
                        .iter()
                        .any(|&(r, b)| r == room_id && b == bid)
                {
                    revealed_rooms.push((room_id, bid));
                }
            }
        }

        let r_ceil = VISIBILITY_RANGE.ceil() as i32;
        let min_x = (vx - r_ceil).max(ox);
        let max_x = (vx + r_ceil).min(ox + WINDOW_SIZE - 1);
        let min_y = (vy - r_ceil).max(oy);
        let max_y = (vy + r_ceil).min(oy + WINDOW_SIZE - 1);
        let range_sq = VISIBILITY_RANGE * VISIBILITY_RANGE;

        for wy in min_y..=max_y {
            for wx in min_x..=max_x {
                let dx = wx as f32 - viewer.x;
                let dy = wy as f32 - viewer.y;
                if dx * dx + dy * dy > range_sq {
                    continue;
                }
                if self.los_reaches(viewer, wx, wy, floor) {
                    let t = Tile3D::new(wx, wy, floor);
                    let room_id = self
                        .window_cell_idx(wx, wy, fl)
                        .map_or(0, |i| self.window_rooms[i]);
                    if let Some(cell) = self.cell_mut_world(t) {
                        let bid = cell.building_id;
                        if cell.visibility != TileVisibility::Visible {
                            cell.visibility = TileVisibility::Visible;
                            visible_buf.push(t);
                        }
                        if room_id != 0
                            && !revealed_rooms
                                .iter()
                                .any(|&(r, b)| r == room_id && b == bid)
                        {
                            revealed_rooms.push((room_id, bid));
                        }
                    }
                }
            }
        }

        // Full room reveal when standing in or peeking into any room via window/door/breach.
        if !revealed_rooms.is_empty() {
            let mut room_tiles: Vec<(i32, i32, BuildingId)> = Vec::new();
            for bi in 0..self.window_buildings.len() {
                let b = self.window_buildings[bi];
                if !revealed_rooms.iter().any(|&(_, bid)| bid == b.id) {
                    continue;
                }
                let bx0 = b.x.max(ox);
                let bx1 = (b.x + b.width - 1).min(ox + WINDOW_SIZE - 1);
                let by0 = b.y.max(oy);
                let by1 = (b.y + b.height - 1).min(oy + WINDOW_SIZE - 1);
                for wy in by0..=by1 {
                    for wx in bx0..=bx1 {
                        let idx = (fl * w + (wy - oy) as usize) * w + (wx - ox) as usize;
                        let rid = self.window_rooms[idx];
                        if rid != 0
                            && revealed_rooms
                                .iter()
                                .any(|&(r, bid)| r == rid && bid == b.id)
                        {
                            let t = Tile3D::new(wx, wy, floor);
                            if let Some(cell) = self.cell_mut_world(t) {
                                let bid = cell.building_id;
                                if cell.visibility != TileVisibility::Visible {
                                    cell.visibility = TileVisibility::Visible;
                                    visible_buf.push(t);
                                }
                                room_tiles.push((wx, wy, bid));
                            }
                        }
                    }
                }
            }

            for (wx, wy, bid) in room_tiles {
                for dy in -1..=1_i32 {
                    for dx in -1..=1_i32 {
                        let nt = Tile3D::new(wx + dx, wy + dy, floor);
                        if !self.in_window(nt.pos()) {
                            continue;
                        }
                        if let Some(ncell) = self.cell_mut_world(nt)
                            && ncell.building_id == bid
                            && is_room_barrier(ncell.kind)
                            && ncell.visibility != TileVisibility::Visible
                        {
                            ncell.visibility = TileVisibility::Visible;
                            visible_buf.push(nt);
                        }
                    }
                }
            }
        }

        // Upper-floor sighting of outdoor Floor 0 ground through Surface::Air.
        if floor >= 1 {
            for wy in min_y..=max_y {
                for wx in min_x..=max_x {
                    let upper_vis_air =
                        self.cell_at_world(Tile3D::new(wx, wy, floor))
                            .is_some_and(|c| {
                                c.visibility == TileVisibility::Visible && c.surface == Surface::Air
                            });
                    if upper_vis_air {
                        let f0_t = Tile3D::new(wx, wy, 0);
                        if let Some(f0) = self.cell_mut_world(f0_t)
                            && !f0.indoor
                            && f0.visibility != TileVisibility::Visible
                        {
                            f0.visibility = TileVisibility::Visible;
                            visible_buf.push(f0_t);
                        }
                    }
                }
            }
        }

        self.currently_visible = visible_buf;
    }

    fn los_reaches(&self, viewer: Vec2, tx: i32, ty: i32, fl: Floor) -> bool {
        let vx = viewer.x.round() as i32;
        let vy = viewer.y.round() as i32;
        if vx == tx && vy == ty {
            return true;
        }
        let to = Vec2::new(tx as f32, ty as f32);
        trace_grid_dda(
            viewer,
            to,
            (WINDOW_SIZE as usize) * 4,
            |cx, cy, corner_a, corner_b| {
                if let (Some((ax, ay)), Some((bx, by))) = (corner_a, corner_b)
                    && self.is_los_opaque(ax, ay, fl)
                    && self.is_los_opaque(bx, by, fl)
                {
                    return false;
                }
                if cx == tx && cy == ty {
                    return true;
                }
                if cx == vx && cy == vy {
                    return true;
                }
                !self.is_los_opaque(cx, cy, fl)
            },
        )
    }

    fn is_los_opaque(&self, wx: i32, wy: i32, fl: Floor) -> bool {
        if !self.in_window(Position::new(wx, wy)) {
            return true;
        }
        let Some(cell) = self.cell_at_world(Tile3D::new(wx, wy, fl)) else {
            return true;
        };
        if cell.surface == Surface::Mountain {
            return true;
        }
        is_los_opaque_kind(cell.kind)
    }

    fn count_load_bearing_3x3(&self, wx: i32, wy: i32, fl: Floor) -> u8 {
        let mut count = 0u8;
        for dy in -1..=1_i32 {
            for dx in -1..=1_i32 {
                if self
                    .cell_at_world(Tile3D::new(wx + dx, wy + dy, fl))
                    .is_some_and(|c| c.load_bearing)
                {
                    count += 1;
                }
            }
        }
        count
    }

    /// 3-floor structural collapse:
    /// breaching F0 load_bearing collapses unsupported F1 cells (and any F2 cells above them) onto F0;
    /// breaching F1 load_bearing collapses unsupported F2 cells onto F1.
    fn resolve_structural_collapse(
        &mut self,
        breached_wx: i32,
        breached_wy: i32,
        breached_fl: Floor,
    ) -> u8 {
        let target_fl = breached_fl + 1;
        if (target_fl as usize) >= COUNTY_FLOORS {
            return 0;
        }
        const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
        let candidates = [
            (breached_wx, breached_wy),
            (breached_wx, breached_wy - 1),
            (breached_wx + 1, breached_wy),
            (breached_wx, breached_wy + 1),
            (breached_wx - 1, breached_wy),
        ];

        let mut pass1: Vec<(i32, i32)> = Vec::new();
        for (cx, cy) in candidates {
            if !self.in_window(Position::new(cx, cy)) {
                continue;
            }
            let Some(above) = self.cell_at_world(Tile3D::new(cx, cy, target_fl)) else {
                continue;
            };
            if above.surface == Surface::Air
                || matches!(above.kind, TileKind::StairsDown | TileKind::StairsBoth)
            {
                continue;
            }
            if self.count_load_bearing_3x3(cx, cy, breached_fl) < 2 {
                pass1.push((cx, cy));
            }
        }

        if pass1.is_empty() {
            return 0;
        }

        for &(ux, uy) in &pass1 {
            self.collapse_stack_from(ux, uy, target_fl, breached_fl);
        }

        let mut pass2: Vec<(i32, i32)> = Vec::new();
        for &(ux, uy) in &pass1 {
            for (dx, dy) in DIRS {
                let nx = ux + dx;
                let ny = uy + dy;
                if !self.in_window(Position::new(nx, ny)) || pass2.contains(&(nx, ny)) {
                    continue;
                }
                let Some(above) = self.cell_at_world(Tile3D::new(nx, ny, target_fl)) else {
                    continue;
                };
                if above.surface == Surface::Air
                    || matches!(above.kind, TileKind::StairsDown | TileKind::StairsBoth)
                {
                    continue;
                }
                let mut collapsed_neighbors = 0u8;
                for (adx, ady) in DIRS {
                    if self
                        .cell_at_world(Tile3D::new(nx + adx, ny + ady, target_fl))
                        .is_some_and(|c| c.collapsed)
                    {
                        collapsed_neighbors += 1;
                    }
                }
                if collapsed_neighbors >= 2 {
                    pass2.push((nx, ny));
                }
            }
        }

        for &(ux, uy) in &pass2 {
            self.collapse_stack_from(ux, uy, target_fl, breached_fl);
        }

        (pass1.len() + pass2.len()).min(u8::MAX as usize) as u8
    }

    fn collapse_stack_from(&mut self, wx: i32, wy: i32, from_fl: Floor, landing_fl: Floor) {
        self.clear_collapsed_cell(wx, wy, from_fl, landing_fl, true);
        if from_fl == 1 {
            let f2 = Tile3D::new(wx, wy, 2);
            if self.cell_at_world(f2).is_some_and(|c| {
                c.surface != Surface::Air
                    && !matches!(c.kind, TileKind::StairsDown | TileKind::StairsBoth)
            }) {
                self.clear_collapsed_cell(wx, wy, 2, 0, false);
            }
        }
    }

    fn clear_collapsed_cell(
        &mut self,
        wx: i32,
        wy: i32,
        from_fl: Floor,
        landing_fl: Floor,
        place_rubble_and_notify: bool,
    ) {
        let from_tile = Tile3D::new(wx, wy, from_fl);
        if let Some(cell) = self.cell_mut_world(from_tile) {
            cell.surface = Surface::Air;
            cell.set_kind(TileKind::Empty, Material::None);
            cell.indoor = false;
            cell.load_bearing = false;
            cell.collapsed = true;
            cell.room_id = 0;
        }

        let landing_tile = Tile3D::new(wx, wy, landing_fl);
        if place_rubble_and_notify {
            if let Some(landing) = self.cell_mut_world(landing_tile)
                && !landing.load_bearing
                && !matches!(
                    landing.kind,
                    TileKind::StairsUp
                        | TileKind::StairsDown
                        | TileKind::StairsBoth
                        | TileKind::Door { .. }
                )
            {
                landing.set_kind(TileKind::Rubble, Material::None);
            }
            self.pending_collapses.push(landing_tile);
        }

        for cache in &mut self.caches {
            if !cache.opened && cache.pos == from_tile {
                cache.pos = landing_tile;
            }
        }
    }
}

impl CombatWorld for CountyStage {
    fn bounds(&self) -> (Position, Position, u8) {
        (
            self.window_origin,
            Position::new(
                self.window_origin.x + WINDOW_SIZE - 1,
                self.window_origin.y + WINDOW_SIZE - 1,
            ),
            COUNTY_FLOORS as u8,
        )
    }

    fn terrain_version(&self) -> u64 {
        self.terrain_version
    }

    fn obstruction(&self, tile: Tile3D) -> TileObstruction {
        if !self.in_window(tile.pos()) {
            return TileObstruction::Wall(Material::BrickMasonry);
        }
        let Some(cell) = self.cell_at_world(tile) else {
            return TileObstruction::Wall(Material::BrickMasonry);
        };
        if cell.surface == Surface::Mountain {
            return TileObstruction::Wall(Material::BrickMasonry);
        }
        match cell.kind {
            TileKind::Empty
            | TileKind::Rubble
            | TileKind::StairsUp
            | TileKind::StairsDown
            | TileKind::StairsBoth
            | TileKind::BrokenStreetlight
            | TileKind::Door { open: true, .. } => TileObstruction::Open,
            TileKind::BrokenWindow => TileObstruction::BrokenWindow,
            TileKind::Window => TileObstruction::IntactWindow,
            TileKind::Door { open: false, .. } => TileObstruction::DoorClosed,
            TileKind::Streetlight => TileObstruction::LowProp(cell.material),
            TileKind::Prop(PropKind::Shed | PropKind::BirchTree) => {
                TileObstruction::Wall(cell.material)
            }
            TileKind::Prop(_) => TileObstruction::LowProp(cell.material),
            TileKind::ExteriorWall | TileKind::InteriorWall | TileKind::Pillar => {
                TileObstruction::Wall(cell.material)
            }
        }
    }

    fn has_floor(&self, tile: Tile3D) -> bool {
        self.in_window(tile.pos())
            && self
                .cell_at_world(tile)
                .is_some_and(|c| c.surface != Surface::Air)
    }

    fn is_walkable(&self, tile: Tile3D, can_vault_window: bool) -> bool {
        if !self.in_window(tile.pos()) {
            return false;
        }
        self.is_walkable_world(tile, can_vault_window)
    }

    fn building_id(&self, tile: Tile3D) -> BuildingId {
        if !self.in_window(tile.pos()) {
            return 0;
        }
        self.cell_at_world(tile).map_or(0, |c| c.building_id)
    }

    fn stair_link(&self, tile: Tile3D) -> Option<(Tile3D, (i8, i8))> {
        let (links, count) = self.stair_links(tile);
        if count > 0 { links[0] } else { None }
    }

    fn stair_links(&self, tile: Tile3D) -> ([Option<(Tile3D, (i8, i8))>; 2], usize) {
        if !self.in_window(tile.pos()) {
            return ([None, None], 0);
        }
        let Some(cell) = self.cell_at_world(tile) else {
            return ([None, None], 0);
        };
        let mut out = [None, None];
        let mut count = 0usize;

        match cell.kind {
            TileKind::StairsUp => {
                if tile.floor + 1 < COUNTY_FLOORS as u8 {
                    let above = Tile3D::new(tile.x, tile.y, tile.floor + 1);
                    if self.has_floor(above)
                        && self.cell_at_world(above).is_some_and(|c| {
                            matches!(c.kind, TileKind::StairsDown | TileKind::StairsBoth)
                        })
                    {
                        out[count] = Some((above, cell.stair_dir));
                        count += 1;
                    }
                }
            }
            TileKind::StairsDown => {
                if tile.floor > 0 {
                    let below = Tile3D::new(tile.x, tile.y, tile.floor - 1);
                    if self.has_floor(below)
                        && self.cell_at_world(below).is_some_and(|c| {
                            matches!(c.kind, TileKind::StairsUp | TileKind::StairsBoth)
                        })
                    {
                        out[count] = Some((below, cell.stair_dir));
                        count += 1;
                    }
                }
            }
            TileKind::StairsBoth => {
                if tile.floor + 1 < COUNTY_FLOORS as u8 {
                    let above = Tile3D::new(tile.x, tile.y, tile.floor + 1);
                    if self.has_floor(above)
                        && self.cell_at_world(above).is_some_and(|c| {
                            matches!(c.kind, TileKind::StairsDown | TileKind::StairsBoth)
                        })
                    {
                        out[count] = Some((above, cell.stair_dir));
                        count += 1;
                    }
                }
                if tile.floor > 0 {
                    let below = Tile3D::new(tile.x, tile.y, tile.floor - 1);
                    if self.has_floor(below)
                        && self.cell_at_world(below).is_some_and(|c| {
                            matches!(c.kind, TileKind::StairsUp | TileKind::StairsBoth)
                        })
                    {
                        out[count] = Some((below, cell.stair_dir));
                        count += 1;
                    }
                }
            }
            _ => {}
        }

        (out, count)
    }

    fn window_drop_target(&self, tile: Tile3D) -> Option<Tile3D> {
        if tile.floor == 0 || !self.in_window(tile.pos()) {
            return None;
        }
        let cell = self.cell_at_world(tile)?;

        if cell.collapsed {
            let mut drop_fl = tile.floor - 1;
            while drop_fl > 0 && !self.has_floor(Tile3D::new(tile.x, tile.y, drop_fl)) {
                drop_fl -= 1;
            }
            let below = Tile3D::new(tile.x, tile.y, drop_fl);
            return self.is_walkable(below, false).then_some(below);
        }

        if matches!(cell.kind, TileKind::BrokenWindow | TileKind::Rubble) {
            const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
            for (dx, dy) in DIRS {
                let adj_upper = Tile3D::new(tile.x + dx, tile.y + dy, tile.floor);
                if !self.has_floor(adj_upper) && self.building_id(adj_upper) == 0 {
                    let landing = Tile3D::new(tile.x + dx, tile.y + dy, 0);
                    if self.is_walkable(landing, false) {
                        return Some(landing);
                    }
                }
            }
        }
        None
    }

    fn window_vault_target(&self, from: Tile3D, window: Tile3D) -> Option<Tile3D> {
        if from.floor != 0 || window.floor != 0 || !self.in_window(window.pos()) {
            return None;
        }
        if self.cell_at_world(window)?.kind != TileKind::BrokenWindow {
            return None;
        }
        let dx = window.x - from.x;
        let dy = window.y - from.y;
        if dx.abs() + dy.abs() != 1 {
            return None;
        }
        let dest = Tile3D::new(window.x + dx, window.y + dy, 0);
        self.is_walkable(dest, false).then_some(dest)
    }

    fn light_level(&self, tile: Tile3D) -> f32 {
        self.effective_light_u8(tile) as f32 / 255.0
    }

    fn damage_tile(&mut self, tile: Tile3D, kind: DamageKind) -> Option<TileDamageResult> {
        if !self.in_window(tile.pos()) {
            return None;
        }
        let cell = *self.cell_at_world(tile)?;
        if cell.hp == 0 || cell.material == Material::None {
            return None;
        }
        let dmg = damage_against(cell.material, kind);
        if dmg == 0 {
            return None;
        }

        let old_kind = cell.kind;
        let material = cell.material;
        let remaining = cell.hp.saturating_sub(dmg);
        self.cell_mut_world(tile)?.hp = remaining;

        if remaining > 0 {
            let sound_radius = match old_kind {
                TileKind::ExteriorWall | TileKind::InteriorWall | TileKind::Pillar => 4.5,
                _ => 3.5,
            };
            return Some(TileDamageResult {
                damaged: true,
                destroyed: false,
                material,
                old_kind,
                new_kind: old_kind,
                sound_radius,
                light_changed: false,
                collapsed_cells: 0,
            });
        }

        let was_load_bearing = cell.load_bearing;
        let (new_kind, sound_radius, light_changed) = match old_kind {
            TileKind::Window => (TileKind::BrokenWindow, 8.5, false),
            TileKind::Streetlight => (TileKind::BrokenStreetlight, 9.5, true),
            TileKind::Door { open, .. } => (TileKind::Rubble, 7.5, !open),
            TileKind::Prop(prop) => (
                TileKind::Rubble,
                7.5,
                matches!(prop, PropKind::Shed | PropKind::BirchTree),
            ),
            TileKind::InteriorWall | TileKind::ExteriorWall | TileKind::Pillar => {
                (TileKind::Rubble, 14.0, true)
            }
            other => (other, 0.0, false),
        };

        if let Some(target) = self.cell_mut_world(tile) {
            target.set_kind(new_kind, Material::None);
            target.load_bearing = false;
        }

        if old_kind == TileKind::Streetlight {
            let cx = tile.x >> 4;
            let cy = tile.y >> 4;
            if let Some(chunk) = &mut self.chunks[Self::chunk_idx(cx, cy)] {
                for light in &mut chunk.lights {
                    if light.kind == LightKind::Streetlight
                        && light.wx == tile.x
                        && light.wy == tile.y
                        && light.floor == tile.floor
                    {
                        light.active = false;
                    }
                }
            }
        }

        let mut collapsed_cells = 0u8;
        if was_load_bearing && tile.floor < (COUNTY_FLOORS - 1) as u8 {
            collapsed_cells = self.resolve_structural_collapse(tile.x, tile.y, tile.floor);
        }

        self.terrain_version += 1;

        Some(TileDamageResult {
            damaged: true,
            destroyed: true,
            material,
            old_kind,
            new_kind,
            sound_radius,
            light_changed: light_changed || collapsed_cells > 0,
            collapsed_cells,
        })
    }

    fn toggle_door(&mut self, tile: Tile3D) -> Option<bool> {
        if !self.in_window(tile.pos()) {
            return None;
        }
        let cell = self.cell_mut_world(tile)?;
        let TileKind::Door { open, exterior } = cell.kind else {
            return None;
        };
        let now_open = !open;
        cell.kind = TileKind::Door {
            open: now_open,
            exterior,
        };
        self.terrain_version += 1;
        Some(now_open)
    }

    fn register_dynamic_flash(
        &mut self,
        pos: Vec2,
        floor: Floor,
        radius: f32,
        intensity: u8,
        ttl_ticks: u8,
    ) {
        if ttl_ticks == 0 || intensity == 0 || radius <= 0.0 {
            return;
        }
        if self.flashes.len() >= MAX_DYNAMIC_FLASHES {
            self.flashes.remove(0);
        }
        self.flashes.push(DynamicFlash {
            pos,
            floor,
            radius,
            intensity,
            ttl_ticks,
        });
    }
}

fn any_cardinal_neighbor<F: FnMut(Position) -> bool>(p: Position, mut f: F) -> bool {
    [(0, -1), (1, 0), (0, 1), (-1, 0)]
        .into_iter()
        .any(|(dx, dy)| f(Position::new(p.x + dx, p.y + dy)))
}

fn all_cardinal_neighbors<F: FnMut(Position) -> bool>(p: Position, mut f: F) -> bool {
    [(0, -1), (1, 0), (0, 1), (-1, 0)]
        .into_iter()
        .all(|(dx, dy)| f(Position::new(p.x + dx, p.y + dy)))
}

fn any_8_neighbor<F: FnMut(Position) -> bool>(p: Position, mut f: F) -> bool {
    for dy in -1..=1 {
        for dx in -1..=1 {
            if (dx != 0 || dy != 0) && f(Position::new(p.x + dx, p.y + dy)) {
                return true;
            }
        }
    }
    false
}

fn find_subbox_door(b: &Building, sub_min: Position, sub_max: Position) -> Position {
    for y in sub_min.y..=sub_max.y {
        for x in sub_min.x..=sub_max.x {
            if x == sub_min.x || x == sub_max.x || y == sub_min.y || y == sub_max.y {
                let p = Position::new(x, y);
                if b.feature_at(p, 0) == Some(Feature::Door) {
                    return p;
                }
            }
        }
    }
    Position::new((sub_min.x + sub_max.x) / 2, sub_max.y)
}

fn building_style_and_name(b: &Building, seed: u64) -> (BuildingStyle, &'static str) {
    match b.kind {
        BuildingKind::Bungalow => {
            let h = mix64(seed ^ ((b.min.x as u64) << 16) ^ (b.min.y as u64));
            let style = if h.is_multiple_of(2) {
                BuildingStyle::BirchSage
            } else {
                BuildingStyle::WarmPutty
            };
            (style, "Bungalow")
        }
        BuildingKind::Duplex => (BuildingStyle::ColonialBrick, "Duplex"),
        BuildingKind::SplitLevel => (BuildingStyle::BlueClapboard, "Split-Level"),
        BuildingKind::Storefront => (BuildingStyle::DowntownStorefront, "Storefront"),
        BuildingKind::Office => (BuildingStyle::DowntownOffice, "Office"),
        BuildingKind::Apartments => (BuildingStyle::DowntownApartments, "Apartments"),
        BuildingKind::Hospital => (BuildingStyle::HospitalClinical, "Hospital"),
        BuildingKind::Precinct => (BuildingStyle::PolicePrecinct, "Police Precinct"),
        BuildingKind::ArmyBase => (BuildingStyle::ArmyBarracks, "Army Base"),
        BuildingKind::Cabin => (BuildingStyle::WoodlandCabin, "Woodland Cabin"),
        BuildingKind::Barn => (BuildingStyle::RuralBarn, "Rural Barn"),
    }
}

fn room_at_building(b: &Building, p: Position, floor: Floor) -> Option<RoomKind> {
    b.layout
        .iter()
        .find(|r| {
            r.floor == floor
                && (r.min.x..=r.max.x).contains(&p.x)
                && (r.min.y..=r.max.y).contains(&p.y)
        })
        .map(|r| r.kind)
}

fn surface_for_room(room: Option<RoomKind>) -> Surface {
    match room {
        Some(RoomKind::Kitchen | RoomKind::Pharmacy | RoomKind::Ward) => Surface::KitchenTile,
        Some(RoomKind::Bedroom | RoomKind::Office | RoomKind::Apartment | RoomKind::Lobby) => {
            Surface::Carpet
        }
        Some(
            RoomKind::Garage
            | RoomKind::MotorPool
            | RoomKind::Cells
            | RoomKind::Armory
            | RoomKind::Storage
            | RoomKind::Barracks,
        ) => Surface::Driveway,
        Some(RoomKind::Living | RoomKind::Shop) | None => Surface::WoodFloor,
    }
}

fn prop_for_room(room: Option<RoomKind>, p: Position) -> PropKind {
    match room {
        Some(RoomKind::Bedroom | RoomKind::Barracks | RoomKind::Ward) => PropKind::Bed,
        Some(RoomKind::Kitchen | RoomKind::Pharmacy | RoomKind::Shop) => PropKind::Counter,
        Some(RoomKind::Living | RoomKind::Apartment | RoomKind::Lobby) => {
            if (p.x + p.y).rem_euclid(2) == 0 {
                PropKind::Sofa
            } else {
                PropKind::Table
            }
        }
        Some(RoomKind::Office) | None => PropKind::Table,
        Some(RoomKind::Cells) => PropKind::Bench,
        Some(RoomKind::Armory | RoomKind::Storage | RoomKind::Garage | RoomKind::MotorPool) => {
            PropKind::Crate
        }
    }
}

fn compute_stair_dir(b: &Building, p: Position, floor: Floor) -> (i8, i8) {
    for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
        let q = Position::new(p.x + dx, p.y + dy);
        if matches!(b.feature_at(q, floor), Some(Feature::Floor | Feature::Door)) {
            return (dx as i8, dy as i8);
        }
    }
    (1, 0)
}

fn roll_cache_loot(
    seed: u64,
    pos: Position,
    floor: Floor,
    loot_kind: crate::LootKind,
    district: District,
) -> (CacheKind, CacheLoot) {
    let h = mix64(
        seed ^ ((pos.x as u64 as u32 as u64) << 32)
            ^ (pos.y as u64 as u32 as u64)
            ^ ((floor as u64) << 48)
            ^ 0xca7c_4e01,
    );
    match loot_kind {
        crate::LootKind::Medkit => {
            let medkits = if district == District::Hospital && h.is_multiple_of(3) {
                2
            } else {
                1
            };
            (
                CacheKind::Medkit,
                CacheLoot {
                    medkits,
                    ..CacheLoot::default()
                },
            )
        }
        crate::LootKind::Firearm => {
            let loot = match district {
                District::Police | District::MilitaryBase => match h % 10 {
                    0..=1 => CacheLoot {
                        weapon: Some(Weapon::Pistol),
                        pistol_ammo: 12,
                        ..CacheLoot::default()
                    },
                    2..=6 => CacheLoot {
                        weapon: Some(Weapon::Shotgun),
                        shotgun_ammo: 8,
                        ..CacheLoot::default()
                    },
                    _ => CacheLoot {
                        weapon: Some(Weapon::RocketLauncher),
                        rockets: 3,
                        ..CacheLoot::default()
                    },
                },
                District::City => match h % 10 {
                    0..=4 => CacheLoot {
                        weapon: Some(Weapon::Pistol),
                        pistol_ammo: 10,
                        ..CacheLoot::default()
                    },
                    5..=8 => CacheLoot {
                        weapon: Some(Weapon::Shotgun),
                        shotgun_ammo: 6,
                        ..CacheLoot::default()
                    },
                    _ => CacheLoot {
                        weapon: Some(Weapon::RocketLauncher),
                        rockets: 2,
                        ..CacheLoot::default()
                    },
                },
                _ => match h % 6 {
                    0..=3 => CacheLoot {
                        weapon: Some(Weapon::Pistol),
                        pistol_ammo: 8,
                        ..CacheLoot::default()
                    },
                    4 => CacheLoot {
                        weapon: Some(Weapon::Shotgun),
                        shotgun_ammo: 6,
                        ..CacheLoot::default()
                    },
                    _ => CacheLoot {
                        weapon: Some(Weapon::Axe),
                        ..CacheLoot::default()
                    },
                },
            };
            (CacheKind::Weapon, loot)
        }
        crate::LootKind::Ammo => {
            let loot = match district {
                District::Police | District::MilitaryBase => match h % 5 {
                    0..=1 => CacheLoot {
                        pistol_ammo: 16,
                        ..CacheLoot::default()
                    },
                    2..=3 => CacheLoot {
                        shotgun_ammo: 10,
                        ..CacheLoot::default()
                    },
                    _ => CacheLoot {
                        rockets: 2,
                        pistol_ammo: 8,
                        ..CacheLoot::default()
                    },
                },
                _ => match h % 3 {
                    0 => CacheLoot {
                        pistol_ammo: 10,
                        ..CacheLoot::default()
                    },
                    1 => CacheLoot {
                        shotgun_ammo: 6,
                        ..CacheLoot::default()
                    },
                    _ => CacheLoot {
                        pistol_ammo: 6,
                        shotgun_ammo: 4,
                        ..CacheLoot::default()
                    },
                },
            };
            (CacheKind::Ammo, loot)
        }
        crate::LootKind::Tools => {
            if h.is_multiple_of(3) {
                (
                    CacheKind::Weapon,
                    CacheLoot {
                        weapon: Some(Weapon::Axe),
                        medkits: u8::from(h.is_multiple_of(6)),
                        ..CacheLoot::default()
                    },
                )
            } else {
                (
                    CacheKind::Mixed,
                    CacheLoot {
                        medkits: 1,
                        pistol_ammo: 4,
                        ..CacheLoot::default()
                    },
                )
            }
        }
        crate::LootKind::Household => {
            let loot = match h % 4 {
                0 => CacheLoot {
                    medkits: 1,
                    pistol_ammo: 4,
                    ..CacheLoot::default()
                },
                1 => CacheLoot {
                    pistol_ammo: 8,
                    ..CacheLoot::default()
                },
                2 => CacheLoot {
                    shotgun_ammo: 4,
                    medkits: 1,
                    ..CacheLoot::default()
                },
                _ => CacheLoot {
                    medkits: 1,
                    ..CacheLoot::default()
                },
            };
            (CacheKind::Mixed, loot)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infected_identity_is_independent_of_chunk_visit_order() {
        let world = World::new(42);
        let (mut first, _, _) = CountyStage::new(&world, 42);
        let (mut second, _, _) = CountyStage::new(&world, 42);
        let chunks: Vec<_> = (0..CHUNKS_PER_SIDE)
            .flat_map(|cy| (0..CHUNKS_PER_SIDE).map(move |cx| (cx, cy)))
            .filter(|&(cx, cy)| {
                first.chunks[CountyStage::chunk_idx(cx, cy)].is_none()
                    && world
                        .chunk_sites(cx, cy)
                        .is_some_and(|sites| !sites.infected.is_empty())
            })
            .take(2)
            .collect();
        assert_eq!(chunks.len(), 2);
        for &(cx, cy) in &chunks {
            first.materialize_chunk(&world, cx, cy);
        }
        for &(cx, cy) in chunks.iter().rev() {
            second.materialize_chunk(&world, cx, cy);
        }
        for (cx, cy) in chunks {
            let idx = CountyStage::chunk_idx(cx, cy);
            let signature = |stage: &CountyStage| {
                stage.hibernated_monsters[idx]
                    .iter()
                    .map(|m| (m.id, m.home, m.variant, m.facing))
                    .collect::<Vec<_>>()
            };
            assert_eq!(signature(&first), signature(&second));
        }
    }
}
