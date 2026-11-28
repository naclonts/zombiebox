//! Streaming `512 × 512 × 3` procedural county runtime implementing `CombatWorld`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::spatial::action_adapter::{
    SpatialStageSource, resolve_tile_support, stage_colliders_in, stage_supports_at,
    stage_surface_quads_in,
};
use crate::spatial::{
    Aabb3, Collider, SpatialWorld, SupportSurface, SurfaceQuad, TerrainSample, Vec3,
};
use crate::{
    Building, BuildingKind, CHUNK_SIZE, CHUNKS_PER_SIDE, District, Feature, Ground, MonsterId,
    PlacedStructure, Position, PropAnchor, ReadableId, RoomKind, StructureId, WORLD_SIZE, World,
};

use super::ai::Monster;
use super::block::{
    Cell, DynamicFlash, INDOOR_AMBIENT, MAX_DYNAMIC_FLASHES, OUTDOOR_AMBIENT,
    PORCH_LIGHT_INTENSITY, PORCH_LIGHT_RADIUS, STREETLIGHT_F1_BLEED, STREETLIGHT_INTENSITY,
    STREETLIGHT_RADIUS, SupplyCache, VISIBILITY_RANGE, circle_walkable_world, damage_against,
    is_los_opaque_kind, is_room_barrier, light_cell_factor_kind, move_body_world, trace_grid_dda,
};
use super::types::{
    BuildingId, BuildingInfo, BuildingStyle, BuildingUid, COUNTY_FLOORS, CacheId, CacheKind,
    CacheLoot, CacheView, CombatWorld, CustomCellSpec, DamageKind, DoorAxis, DynamicFlashView,
    Floor, LightKind, LightSourceView, MAX_COMPACT_BUILDING_ID, MAX_COUNTY_FLOORS, Material,
    PropKind, SpatialCell, Surface, Tile3D, TileDamageResult, TileKind, TileObstruction,
    TileRenderInfo, TileVisibility, Vec2, WallOrientation, Weapon, WingRect, mix64,
    pack_chunk_slot_id,
};

pub const ACTIVE_CHUNK_RADIUS: i32 = 3;
pub const CHUNKS_PER_WINDOW: usize = (ACTIVE_CHUNK_RADIUS * 2 + 1) as usize; // 7
pub const WINDOW_SIZE: i32 = CHUNK_SIZE * CHUNKS_PER_WINDOW as i32; // 112
const NUM_CHUNKS: usize = (CHUNKS_PER_SIDE * CHUNKS_PER_SIDE) as usize; // 1024
const WINDOW_LAYER_CELLS: usize = (WINDOW_SIZE as usize) * (WINDOW_SIZE as usize);
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

type ChunkLayer = [[Cell; CHUNK_SIZE as usize]; CHUNK_SIZE as usize];
const AIR_CELL: Cell = Cell::air();

struct CountyChunk {
    floors: Vec<Box<ChunkLayer>>,
    lights: Vec<CountyLight>,
}

impl CountyChunk {
    fn with_floors(num_floors: usize) -> Self {
        let n = num_floors.clamp(1, MAX_COUNTY_FLOORS);
        let mut floors = Vec::with_capacity(n);
        for _ in 0..n {
            floors.push(Box::new(
                [[Cell::air(); CHUNK_SIZE as usize]; CHUNK_SIZE as usize],
            ));
        }
        Self {
            floors,
            lights: Vec::new(),
        }
    }

    fn ensure_floors(&mut self, num_floors: usize) {
        let n = num_floors.clamp(1, MAX_COUNTY_FLOORS);
        while self.floors.len() < n {
            self.floors.push(Box::new(
                [[Cell::air(); CHUNK_SIZE as usize]; CHUNK_SIZE as usize],
            ));
        }
    }
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
    world: World,
    wake: Position,
    chunks: Vec<Option<Box<CountyChunk>>>,
    hibernated_monsters: Vec<Vec<Monster>>,
    window_cx: i32,
    window_cy: i32,
    window_origin: Position,
    all_buildings: Vec<BuildingInfo>,
    registered_buildings: HashMap<BuildingUid, BuildingReg>,
    registered_blueprints: HashMap<StructureId, BuildingId>,
    blueprint_stair_landings: HashMap<Tile3D, (i8, i8)>,
    blueprint_props: Vec<PropAnchor>,
    blueprint_prop_uids: HashSet<u64>,
    blueprint_readables: Vec<(ReadableId, PropAnchor)>,
    blueprint_readable_uids: HashSet<u64>,
    cache_index_by_id: HashMap<CacheId, usize>,
    window_max_floors: u8,
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
    next_monster_id: u64,
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
            world: world.clone(),
            wake,
            chunks,
            hibernated_monsters,
            window_cx: wcx,
            window_cy: wcy,
            window_origin,
            all_buildings: Vec::new(),
            registered_buildings: HashMap::new(),
            registered_blueprints: HashMap::new(),
            blueprint_stair_landings: HashMap::new(),
            blueprint_props: Vec::new(),
            blueprint_prop_uids: HashSet::new(),
            blueprint_readables: Vec::new(),
            blueprint_readable_uids: HashSet::new(),
            cache_index_by_id: HashMap::new(),
            window_max_floors: 1,
            window_buildings: Vec::new(),
            caches: Vec::new(),
            flashes: Vec::new(),
            light_cache: RefCell::new(WindowLightCache {
                version: u64::MAX,
                levels: Vec::new(),
            }),
            window_rooms: Vec::new(),
            rooms_version: u64::MAX,
            currently_visible: Vec::with_capacity(512),
            terrain_version: 1,
            pending_collapses: Vec::new(),
            starter_open_door: None,
            // Generated infected use the low ID range; reserve the high range for runtime spawns.
            next_monster_id: 1u64 << 63,
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

        // Starter supplies follow the furnished starter house when it replaces
        // the legacy parcel, whose cache sites are no longer generated.
        if world.fixture() == crate::CountyFixture::ResidentialStreet
            && let Some(plan) = world.fixture_plan()
            && let Some(starter) = plan.structures.first()
        {
            let entry = starter.front_door;
            let (dx, dy) = plan.frontage.side.inward();
            let pistol = Position::new(entry.x + dx, entry.y + dy);
            stage.add_starter_cache(
                (1 << 63) | 1,
                pistol,
                CacheLoot {
                    weapon: Some(Weapon::Pistol),
                    pistol_ammo: 12,
                    ..CacheLoot::default()
                },
            );
            stage.ensure_starter_axe(entry, wake);
        } else if let Some((_, ref starter_b)) = nearest_res {
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

    /// True when `pos` currently has materialized collision and support cells.
    #[inline]
    pub(crate) fn contains_world_tile(&self, pos: Position) -> bool {
        Self::in_world(pos) && self.in_window(pos)
    }

    #[inline]
    fn window_cell_idx(&self, wx: i32, wy: i32, fl: usize) -> Option<usize> {
        let lx = wx - self.window_origin.x;
        let ly = wy - self.window_origin.y;
        if !(0..WINDOW_SIZE).contains(&lx)
            || !(0..WINDOW_SIZE).contains(&ly)
            || fl >= usize::from(self.window_max_floors)
        {
            return None;
        }
        Some((fl * WINDOW_SIZE as usize + ly as usize) * WINDOW_SIZE as usize + lx as usize)
    }

    #[inline]
    fn cell_at_world(&self, tile: Tile3D) -> Option<&Cell> {
        if !Self::in_world(tile.pos()) || (tile.floor as usize) >= MAX_COUNTY_FLOORS {
            return None;
        }
        let cx = tile.x >> 4;
        let cy = tile.y >> 4;
        let lx = (tile.x & 15) as usize;
        let ly = (tile.y & 15) as usize;
        let chunk = self.chunks[Self::chunk_idx(cx, cy)].as_ref()?;
        if (tile.floor as usize) < chunk.floors.len() {
            Some(&chunk.floors[tile.floor as usize][ly][lx])
        } else {
            Some(&AIR_CELL)
        }
    }

    #[inline]
    fn cell_mut_world(&mut self, tile: Tile3D) -> Option<&mut Cell> {
        if !Self::in_world(tile.pos()) || (tile.floor as usize) >= MAX_COUNTY_FLOORS {
            return None;
        }
        let cx = tile.x >> 4;
        let cy = tile.y >> 4;
        let lx = (tile.x & 15) as usize;
        let ly = (tile.y & 15) as usize;
        let chunk = self.chunks[Self::chunk_idx(cx, cy)].as_mut()?;
        chunk
            .floors
            .get_mut(tile.floor as usize)
            .map(|layer| &mut layer[ly][lx])
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

    fn next_building_id(&self) -> BuildingId {
        let id = self.all_buildings.len() + 1;
        assert!(
            id <= usize::from(MAX_COMPACT_BUILDING_ID),
            "county building slots exhausted"
        );
        id as BuildingId
    }

    pub const fn window_max_floors(&self) -> u8 {
        self.window_max_floors
    }

    pub fn chunk_allocated_floors(&self, cx: i32, cy: i32) -> Option<usize> {
        if !(0..CHUNKS_PER_SIDE).contains(&cx) || !(0..CHUNKS_PER_SIDE).contains(&cy) {
            return None;
        }
        self.chunks[Self::chunk_idx(cx, cy)]
            .as_ref()
            .map(|c| c.floors.len())
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
        let mut max_fl = 1u8;
        for cy in (self.window_cy - ACTIVE_CHUNK_RADIUS)..=(self.window_cy + ACTIVE_CHUNK_RADIUS) {
            for cx in
                (self.window_cx - ACTIVE_CHUNK_RADIUS)..=(self.window_cx + ACTIVE_CHUNK_RADIUS)
            {
                if (0..CHUNKS_PER_SIDE).contains(&cx)
                    && (0..CHUNKS_PER_SIDE).contains(&cy)
                    && let Some(c) = &self.chunks[Self::chunk_idx(cx, cy)]
                {
                    max_fl = max_fl.max(c.floors.len() as u8);
                }
            }
        }
        self.window_max_floors = max_fl.min(MAX_COUNTY_FLOORS as u8);
    }

    fn register_building(&mut self, b: &Building) -> BuildingReg {
        let uid_key = BuildingUid::from_coords(b.min.x, b.min.y, 0);
        if let Some(&reg) = self.registered_buildings.get(&uid_key) {
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
                let id = self.next_building_id();
                let uid = BuildingUid::from_coords(sub_min.x, sub_min.y, idx as u16 + 1);
                let (wings, wing_count) = BuildingInfo::single_wing_array(
                    sub_min.x,
                    sub_min.y,
                    sub_max.x - sub_min.x + 1,
                    sub_max.y - sub_min.y + 1,
                    1,
                );
                let name = match room.kind {
                    RoomKind::Barracks => "Army Barracks",
                    RoomKind::Armory => "Army Armory",
                    RoomKind::MotorPool => "Motor Pool",
                    _ => "Army Building",
                };
                self.all_buildings.push(BuildingInfo {
                    id,
                    uid,
                    name,
                    x: sub_min.x,
                    y: sub_min.y,
                    width: sub_max.x - sub_min.x + 1,
                    height: sub_max.y - sub_min.y + 1,
                    floors: 1,
                    style: BuildingStyle::ArmyBarracks,
                    front_door: door,
                    foundation_z: b.foundation_z,
                    wings,
                    wing_count,
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
            let id = self.next_building_id();
            let uid = uid_key;
            let floors = b.floors.min(MAX_COUNTY_FLOORS as u8);
            let (wings, wing_count) = BuildingInfo::from_wings_slice(
                &b.wings,
                WingRect::new(b.min.x, b.min.y, b.max.x, b.max.y, floors),
            );
            let (style, name) = building_style_and_name(b, self.seed);
            self.all_buildings.push(BuildingInfo {
                id,
                uid,
                name,
                x: b.min.x,
                y: b.min.y,
                width: b.max.x - b.min.x + 1,
                height: b.max.y - b.min.y + 1,
                floors,
                style,
                front_door: b.entry,
                foundation_z: b.foundation_z,
                wings,
                wing_count,
            });
            BuildingReg::Single(id)
        };

        self.registered_buildings.insert(uid_key, reg);
        reg
    }

    fn register_blueprint_building(&mut self, structure: &PlacedStructure) -> BuildingId {
        if let Some(&id) = self.registered_blueprints.get(&structure.id) {
            return id;
        }
        let id = self.next_building_id();
        let floors = structure.floors().min(MAX_COUNTY_FLOORS as u8);
        // Blueprint wing IDs identify independently roofed masses. Derive their
        // world rectangles from the authored cells after applying the lot basis.
        let mut wing_bounds = std::collections::BTreeMap::new();
        for (local, pos, cell) in structure.world_cells() {
            let wing = wing_bounds
                .entry(cell.wing)
                .or_insert_with(|| WingRect::new(pos.x, pos.y, pos.x, pos.y, local.floor + 1));
            wing.min_x = wing.min_x.min(pos.x);
            wing.min_y = wing.min_y.min(pos.y);
            wing.max_x = wing.max_x.max(pos.x);
            wing.max_y = wing.max_y.max(pos.y);
            wing.floors = wing.floors.max(local.floor + 1);
        }
        let authored_wings: Vec<_> = wing_bounds.into_values().collect();
        let (wings, wing_count) = BuildingInfo::from_wings_slice(
            &authored_wings,
            WingRect::new(
                structure.bounds.min.x,
                structure.bounds.min.y,
                structure.bounds.max.x,
                structure.bounds.max.y,
                floors,
            ),
        );
        self.all_buildings.push(BuildingInfo {
            id,
            uid: BuildingUid(structure.id.0),
            name: structure.name,
            x: structure.bounds.min.x,
            y: structure.bounds.min.y,
            width: structure.bounds.width(),
            height: structure.bounds.height(),
            floors,
            style: structure.style,
            front_door: structure.front_door,
            foundation_z: structure.foundation_z,
            wings,
            wing_count,
        });
        self.registered_blueprints.insert(structure.id, id);
        for stair in &structure.blueprint.stairs {
            for (endpoint, landing) in [
                (stair.lower, stair.lower_landing),
                (stair.upper, stair.upper_landing),
            ] {
                let tile =
                    Tile3D::from_pos(structure.transform.local_to_world(endpoint), endpoint.floor);
                let direction = (
                    (landing.u - endpoint.u) as i8,
                    (landing.v - endpoint.v) as i8,
                );
                self.blueprint_stair_landings
                    .insert(tile, structure.transform.world_stair_dir(direction));
            }
        }
        for anchor in structure.planned_props() {
            if self.blueprint_prop_uids.insert(anchor.anchor_uid) {
                self.blueprint_props.push(anchor);
            }
        }
        for sign in structure.signs_world() {
            if self.blueprint_prop_uids.insert(sign.anchor.anchor_uid) {
                self.blueprint_props.push(sign.anchor);
            }
            if let Some(readable) = sign.readable
                && self.blueprint_readable_uids.insert(sign.anchor.anchor_uid)
            {
                self.blueprint_readables.push((readable, sign.anchor));
            }
        }
        id
    }

    /// Persistent prop anchors produced by authored blueprints. The action
    /// layer ingests these through the existing `PropAnchor` registry.
    pub fn blueprint_prop_anchors(&self) -> &[PropAnchor] {
        &self.blueprint_props
    }

    pub(crate) fn blueprint_stair_landing(&self, tile: Tile3D) -> Option<(i8, i8)> {
        self.blueprint_stair_landings.get(&tile).copied()
    }

    /// Readable signs/notes authored inside blueprint structures.
    pub fn blueprint_readables(&self) -> &[(ReadableId, PropAnchor)] {
        &self.blueprint_readables
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

        // Outdoor parked vehicles use the same stable anchor registry as interior
        // props and are registered by the chunk containing their minimum corner.
        for plan in world.fixture_plans_iter() {
            for (slot, &(pos, facing)) in plan.outdoor.vehicle_slots.iter().enumerate() {
                if pos.x / CHUNK_SIZE != cx || pos.y / CHUNK_SIZE != cy {
                    continue;
                }
                let span = crate::world::blueprint::world_span_for_facing(
                    super::PropId::Car.def().span,
                    facing,
                );
                let anchor = PropAnchor {
                    anchor_uid: mix64(plan.id.0 ^ 0x7374_7265_6574_6361 ^ slot as u64),
                    tile: Tile3D::from_pos(pos, 0),
                    id: super::PropId::Car,
                    span,
                    attachment: super::CellAttachment::Floor,
                    facing,
                    offset: Vec3::ZERO,
                };
                if self.blueprint_prop_uids.insert(anchor.anchor_uid) {
                    self.blueprint_props.push(anchor);
                }
            }
        }

        let mut regs = Vec::with_capacity(sites.buildings.len());
        for b in &sites.buildings {
            regs.push((*b, self.register_building(b)));
        }
        let structures = sites.structures.clone();
        let mut blueprint_regs = Vec::with_capacity(structures.len());
        for structure in structures {
            let id = self.register_blueprint_building(structure);
            blueprint_regs.push((structure, id));
        }

        let mut chunk = Box::new(CountyChunk::with_floors(1));

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

                let cell = &mut chunk.floors[0][ly][lx];
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
                                let (kind, wall_mat, door_axis) = match feat {
                                    Feature::Wall(_) => (
                                        if is_corner {
                                            TileKind::Pillar
                                        } else if on_perimeter {
                                            TileKind::ExteriorWall
                                        } else {
                                            TileKind::InteriorWall
                                        },
                                        b.material,
                                        None,
                                    ),
                                    Feature::Window => (
                                        if is_corner {
                                            TileKind::Pillar
                                        } else {
                                            TileKind::Window
                                        },
                                        b.material,
                                        None,
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
                                            b.door_axis_at(p, 0).or(Some(if on_y {
                                                DoorAxis::AlongX
                                            } else {
                                                DoorAxis::AlongY
                                            })),
                                        )
                                    }
                                    Feature::Furniture => (
                                        TileKind::Prop(prop_for_room(Some(sub.room_kind), p)),
                                        Material::None,
                                        None,
                                    ),
                                    _ => (TileKind::Empty, Material::None, None),
                                };
                                let cell = &mut chunk.floors[0][ly][lx];
                                cell.building_id = sub.id;
                                cell.indoor = !on_perimeter;
                                cell.surface = Surface::Driveway;
                                cell.set_kind(kind, wall_mat);
                                cell.door_axis = door_axis;
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
                                let cell = &mut chunk.floors[0][ly][lx];
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
                                    cell.door_axis = b.door_axis_at(p, 0).or(Some(
                                        if wy == b.min.y || wy == b.max.y {
                                            DoorAxis::AlongX
                                        } else {
                                            DoorAxis::AlongY
                                        },
                                    ));
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
                    let max_fl = (b.floors as usize).clamp(1, MAX_COUNTY_FLOORS);
                    for floor in 0..max_fl {
                        for wy in y0..=y1 {
                            let ly = (wy - base_y) as usize;
                            for wx in x0..=x1 {
                                let lx = (wx - base_x) as usize;
                                let p = Position::new(wx, wy);
                                let feat =
                                    b.feature_at(p, floor as Floor).unwrap_or(Feature::Empty);
                                if feat == Feature::Empty {
                                    continue;
                                }
                                if floor >= chunk.floors.len() {
                                    chunk.ensure_floors(floor + 1);
                                }

                                let w_out = b
                                    .feature_at(Position::new(wx - 1, wy), floor as Floor)
                                    .unwrap_or(Feature::Empty)
                                    == Feature::Empty;
                                let e_out = b
                                    .feature_at(Position::new(wx + 1, wy), floor as Floor)
                                    .unwrap_or(Feature::Empty)
                                    == Feature::Empty;
                                let n_out = b
                                    .feature_at(Position::new(wx, wy - 1), floor as Floor)
                                    .unwrap_or(Feature::Empty)
                                    == Feature::Empty;
                                let s_out = b
                                    .feature_at(Position::new(wx, wy + 1), floor as Floor)
                                    .unwrap_or(Feature::Empty)
                                    == Feature::Empty;
                                let on_x = w_out || e_out;
                                let on_y = n_out || s_out;
                                let on_perimeter = on_x || on_y;
                                let is_corner = on_x && on_y;

                                let room = room_at_building(b, p, floor as Floor);
                                let surface = surface_for_room(room);
                                let supports_above = b
                                    .feature_at(p, floor as Floor + 1)
                                    .is_some_and(|above| above != Feature::Empty);

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
                                        let st_kind = match (floor, max_fl) {
                                            (0, _) => TileKind::StairsUp,
                                            (f, mf) if f > 0 && f + 1 < mf => TileKind::StairsBoth,
                                            _ => TileKind::StairsDown,
                                        };
                                        let dir = compute_stair_dir(b, p, floor as Floor);
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

                                let cell = &mut chunk.floors[floor][ly][lx];
                                cell.surface = surface;
                                cell.set_kind(kind, wall_mat);
                                cell.building_id = bid;
                                cell.indoor = !on_perimeter;
                                cell.load_bearing = load_bearing;
                                cell.stair_dir = stair_dir;
                                cell.door_axis = b.door_axis_at(p, floor as Floor);
                            }
                        }
                    }
                }
            }
        }

        self.chunks[cidx] = Some(chunk);

        // 2b. Stamp authored blueprint structures. They share the same empty
        // chunk and building registry, but their authoritative cell data comes
        // from the sparse blueprint instead of the legacy site template.
        if let Some(chunk) = self.chunks[cidx].as_mut() {
            for (structure, bid) in blueprint_regs {
                let floors = usize::from(structure.floors()).clamp(1, MAX_COUNTY_FLOORS);
                if chunk.floors.len() < floors {
                    chunk.ensure_floors(floors);
                }
                for (local, pos, blueprint_cell) in structure.world_cells() {
                    let lx = pos.x - base_x;
                    let ly = pos.y - base_y;
                    if !(0..CHUNK_SIZE).contains(&lx) || !(0..CHUNK_SIZE).contains(&ly) {
                        continue;
                    }
                    let floor = local.floor as usize;
                    let target = &mut chunk.floors[floor][ly as usize][lx as usize];
                    target.surface = blueprint_cell.surface;
                    target.building_id = bid;
                    target.indoor = blueprint_cell.indoor;
                    target.load_bearing = blueprint_cell.load_bearing;
                    target.stair_dir = structure
                        .transform
                        .world_stair_dir(blueprint_cell.stair_dir);
                    target.door_axis =
                        if matches!(blueprint_cell.kind, crate::BlueprintKind::Door { .. }) {
                            structure.door_axis_at(local)
                        } else {
                            None
                        };
                    target.set_kind(blueprint_cell.kind.to_tile_kind(), blueprint_cell.material);
                }
            }
        }

        // 3. Materialize chunk-owned loot caches.
        for (cache_slot, site) in sites.loot.into_iter().enumerate() {
            let district = world
                .tile_at(site.position)
                .map_or(District::Farm, |t| t.district);
            let (kind, loot) =
                roll_cache_loot(self.seed, site.position, site.floor, site.kind, district);
            let id = pack_chunk_slot_id(cx, cy, cache_slot as u16);
            self.cache_index_by_id.insert(id, self.caches.len());
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
            let id = MonsterId::from_chunk_slot(cx, cy, spawn_index as u16);
            let h = mix64(
                self.seed
                    ^ id.0
                    ^ ((site.position.x as u64 as u32 as u64) << 16)
                    ^ (site.position.y as u64 as u32 as u64),
            );
            let variant = (h % 4) as u8;
            let spawn = Tile3D::from_pos(site.position, site.floor);
            let monster = Monster::new_in_world(id, site.kind, variant, spawn, self);
            self.hibernated_monsters[cidx].push(monster);
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
            .min_by_key(|(_, c)| (c.pos.floor, c.pos.y >> 4, c.id))
            .map(|(i, _)| i);

        if let Some(idx) = best_idx {
            self.caches[idx].kind = CacheKind::Weapon;
            self.caches[idx].loot = CacheLoot {
                weapon: Some(Weapon::Pistol),
                pistol_ammo: 12,
                ..CacheLoot::default()
            };
        }

        self.ensure_starter_axe(starter_b.entry, wake);
    }

    fn ensure_starter_axe(&mut self, entry: Position, wake: Position) {
        let wake_vec = Vec2::new(wake.x as f32, wake.y as f32);
        let has_nearby_axe = self.caches.iter().any(|c| {
            c.pos.floor == 0
                && c.loot.weapon == Some(Weapon::Axe)
                && c.pos.center().distance(wake_vec) <= 8.0
        });
        if has_nearby_axe {
            return;
        }

        if let Some(axe_pos) = self.find_starter_axe_tile(entry, wake) {
            self.add_starter_cache(
                1 << 63,
                axe_pos,
                CacheLoot {
                    weapon: Some(Weapon::Axe),
                    ..CacheLoot::default()
                },
            );
        }
    }

    fn add_starter_cache(&mut self, id: CacheId, pos: Position, loot: CacheLoot) {
        // Run-owned IDs live outside the generated chunk-slot space.
        self.cache_index_by_id.insert(id, self.caches.len());
        self.caches.push(SupplyCache {
            id,
            pos: Tile3D::from_pos(pos, 0),
            kind: CacheKind::Weapon,
            loot,
            opened: false,
        });
    }

    fn find_starter_axe_tile(&self, entry: Position, wake: Position) -> Option<Position> {
        let mut candidates: Vec<(u32, u32, Position)> = Vec::new();
        for dy in -5..=5_i32 {
            for dx in -5..=5_i32 {
                let p = Position::new(entry.x + dx, entry.y + dy);
                if p == entry || p == wake {
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
                let d_entry = p.distance(entry);
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

    pub(crate) fn spatial_cell(&self, tile: Tile3D) -> Option<SpatialCell> {
        if !Self::in_world(tile.pos()) || (tile.floor as usize) >= COUNTY_FLOORS {
            return None;
        }
        if let Some(cell) = self.cell_at_world(tile) {
            return Some(SpatialCell {
                surface: cell.surface,
                kind: cell.kind,
                material: cell.material,
                building_id: cell.building_id,
                orientation: self.wall_orientation(tile.x, tile.y, tile.floor),
                stair_dir: cell.stair_dir,
            });
        }
        if tile.floor == 0 {
            let wt = self.world.tile_at(tile.pos())?;
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
                    self.world
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
            return Some(SpatialCell {
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
        let cache = &mut self.caches[*self.cache_index_by_id.get(&id)?];
        if cache.opened {
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
            .map(|c| {
                let (z, _, _) = resolve_tile_support(self, c.pos);
                CacheView {
                    id: c.id,
                    pos: c.pos,
                    world_pos: c.pos.center().extend(z),
                    kind: c.kind,
                    loot: c.loot,
                    opened: c.opened,
                    nearby: !c.opened
                        && c.pos.floor == player_floor
                        && c.pos.center().distance(player_pos) <= interact_radius,
                }
            })
            .collect()
    }

    pub(super) fn tile_visibility(&self, tile: Tile3D) -> Option<TileVisibility> {
        if !self.in_window(tile.pos()) {
            return None;
        }
        self.cell_at_world(tile).map(|cell| cell.visibility)
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
                        let tile = Tile3D::new(light.wx, light.wy, light.floor);
                        let (surface_z, _, _) = resolve_tile_support(self, tile);
                        let bulb_offset = match light.kind {
                            LightKind::Streetlight => 1.65,
                            LightKind::Porch => 1.10,
                            LightKind::Cache => 0.35,
                        };
                        views.push(LightSourceView {
                            pos: Vec2::new(light.wx as f32, light.wy as f32),
                            position: tile.center().extend(surface_z + bulb_offset),
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
                let (surface_z, _, _) = resolve_tile_support(self, cache.pos);
                views.push(LightSourceView {
                    pos: cache.pos.center(),
                    position: cache.pos.center().extend(surface_z + 0.35),
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

    pub fn dynamic_flashes(&self) -> Vec<DynamicFlashView> {
        self.flashes
            .iter()
            .map(|f| DynamicFlashView {
                position: f.pos,
                floor: f.floor,
                radius: f.radius,
                intensity: f.intensity,
                ttl_ticks: f.ttl_ticks,
            })
            .collect()
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

    #[allow(dead_code)]
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
            let dist = center.distance(flash.pos.xy());
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
        if matches!(cell.kind, TileKind::Door { .. })
            && let Some(axis) = cell.door_axis
        {
            return axis.to_wall_orientation();
        }
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
        let mut accum = vec![0.0_f32; WINDOW_LAYER_CELLS * usize::from(self.window_max_floors)];
        let ox = self.window_origin.x;
        let oy = self.window_origin.y;
        let w = WINDOW_SIZE as usize;

        for fl in 0..usize::from(self.window_max_floors) {
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
                                && self.window_max_floors > 1
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

        cache.levels.resize(accum.len(), 0);
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
        self.window_rooms.clear();
        self.window_rooms
            .resize(WINDOW_LAYER_CELLS * usize::from(self.window_max_floors), 0);
        let ox = self.window_origin.x;
        let oy = self.window_origin.y;
        let w = WINDOW_SIZE as usize;
        let mut next_room = 1u16;
        let mut queue = VecDeque::new();
        const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

        for fl in 0..usize::from(self.window_max_floors) {
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
        if floor >= self.window_max_floors {
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

    /// A breach drops unsupported floor cells and the occupied layers above them onto the damaged floor.
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
        for upper_fl in (from_fl + 1)..self.window_max_floors {
            let upper = Tile3D::new(wx, wy, upper_fl);
            if self.cell_at_world(upper).is_some_and(|c| {
                c.surface != Surface::Air
                    && !matches!(c.kind, TileKind::StairsDown | TileKind::StairsBoth)
            }) {
                self.clear_collapsed_cell(wx, wy, upper_fl, landing_fl, false);
            }
        }
    }

    pub fn stamp_custom_building(
        &mut self,
        world: &World,
        mut info: BuildingInfo,
        cells: &[CustomCellSpec],
        viewer_pos: Vec2,
        viewer_floor: Floor,
    ) -> BuildingId {
        assert!((1..=MAX_COUNTY_FLOORS).contains(&usize::from(info.floors)));
        assert!(cells.iter().all(|spec| {
            Self::in_world(spec.pos)
                && spec.floor < info.floors
                && (info.x..info.x + info.width).contains(&spec.pos.x)
                && (info.y..info.y + info.height).contains(&spec.pos.y)
        }));
        for spec in cells {
            self.materialize_chunk(world, spec.pos.x >> 4, spec.pos.y >> 4);
        }

        let id = self.next_building_id();
        info.id = id;
        if info.uid.0 == 0 {
            info.uid = BuildingUid::from_coords(info.x, info.y, (1 << 15) - 1);
        }
        assert!(self.all_buildings.iter().all(|b| b.uid != info.uid));
        if info.wing_count == 0 {
            let (wings, wing_count) = BuildingInfo::single_wing_array(
                info.x,
                info.y,
                info.width,
                info.height,
                info.floors,
            );
            info.wings = wings;
            info.wing_count = wing_count;
        }
        self.all_buildings.push(info);

        for spec in cells {
            let cx = spec.pos.x >> 4;
            let cy = spec.pos.y >> 4;
            let lx = (spec.pos.x & 15) as usize;
            let ly = (spec.pos.y & 15) as usize;
            if let Some(chunk) = self.chunks[Self::chunk_idx(cx, cy)].as_mut() {
                chunk.ensure_floors(usize::from(spec.floor) + 1);
                let mut cell = Cell::air();
                cell.surface = spec.surface;
                cell.set_kind(spec.kind, spec.material);
                cell.indoor = spec.indoor;
                cell.load_bearing = spec.load_bearing;
                cell.building_id = id;
                cell.door_axis = spec.door_axis;
                cell.stair_dir = spec.stair_dir;
                chunk.floors[usize::from(spec.floor)][ly][lx] = cell;
            }
        }

        self.rebuild_window_buildings();
        self.terrain_version += 1;
        self.update_visibility(viewer_pos, viewer_floor);
        id
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

impl SpatialStageSource for CountyStage {
    fn world(&self) -> &World {
        &self.world
    }

    fn is_county(&self) -> bool {
        true
    }

    fn floors_at(&self, pos: Position) -> Floor {
        self.chunk_allocated_floors(pos.x >> 4, pos.y >> 4)
            .unwrap_or(1) as Floor
    }

    fn playable_bounds(&self) -> (Position, Position) {
        self.playable_bounds()
    }

    fn spatial_cell(&self, tile: Tile3D) -> Option<SpatialCell> {
        self.spatial_cell(tile)
    }

    fn building_by_id(&self, id: BuildingId) -> Option<&BuildingInfo> {
        self.building_by_id(id)
    }

    fn authored_stair_landing(&self, tile: Tile3D) -> Option<(i8, i8)> {
        self.blueprint_stair_landing(tile)
    }
}

impl SpatialWorld for CountyStage {
    fn terrain_at(&self, xy: Vec2) -> Option<TerrainSample> {
        self.world.terrain_at(xy)
    }

    fn supports_at(&self, xy: Vec2, out: &mut Vec<SupportSurface>) {
        stage_supports_at(self, xy, out);
    }

    fn colliders_in(&self, bounds: Aabb3, out: &mut Vec<Collider>) {
        stage_colliders_in(self, bounds, out);
    }

    fn surface_quads_in(&self, min_tile: Position, max_tile: Position, out: &mut Vec<SurfaceQuad>) {
        stage_surface_quads_in(self, min_tile, max_tile, out);
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
        let landing_at = |dest| self.blueprint_stair_landing(dest).unwrap_or(cell.stair_dir);

        match cell.kind {
            TileKind::StairsUp => {
                if tile.floor + 1 < COUNTY_FLOORS as u8 {
                    let above = Tile3D::new(tile.x, tile.y, tile.floor + 1);
                    if self.has_floor(above)
                        && self.cell_at_world(above).is_some_and(|c| {
                            matches!(c.kind, TileKind::StairsDown | TileKind::StairsBoth)
                        })
                    {
                        out[count] = Some((above, landing_at(above)));
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
                        out[count] = Some((below, landing_at(below)));
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
                        out[count] = Some((above, landing_at(above)));
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
                        out[count] = Some((below, landing_at(below)));
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
        pos: Vec3,
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
        Some(RoomKind::Kitchen | RoomKind::Bathroom | RoomKind::Pharmacy | RoomKind::Ward) => {
            Surface::KitchenTile
        }
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
        Some(RoomKind::Living | RoomKind::Hall | RoomKind::Shop | RoomKind::Dining) | None => {
            Surface::WoodFloor
        }
    }
}

fn prop_for_room(room: Option<RoomKind>, p: Position) -> PropKind {
    match room {
        Some(RoomKind::Bedroom | RoomKind::Barracks | RoomKind::Ward) => PropKind::Bed,
        Some(RoomKind::Kitchen | RoomKind::Bathroom | RoomKind::Pharmacy | RoomKind::Shop) => {
            PropKind::Counter
        }
        Some(RoomKind::Living | RoomKind::Apartment | RoomKind::Lobby) => {
            if (p.x + p.y).rem_euclid(2) == 0 {
                PropKind::Sofa
            } else {
                PropKind::Table
            }
        }
        Some(RoomKind::Office | RoomKind::Hall | RoomKind::Dining) | None => PropKind::Table,
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
    use crate::CountyFixture;
    use crate::spatial::{Aabb3, ColliderKind, FLOOR_H, Vec3};

    #[test]
    fn generated_doors_and_gates_pass_runtime_bilateral_capsule_sweep() {
        use crate::spatial::SpatialWorld;
        use crate::spatial::collision::sweep_circle_rect_2d;
        let mut failures = Vec::new();
        for seed in 0..64 {
            let world = World::new(seed);
            let (mut stage, _, _) = CountyStage::new(&world, seed);
            for building in &world.sites().unwrap().buildings {
                for floor in 0..building.floors {
                    for y in building.min.y..=building.max.y {
                        for x in building.min.x..=building.max.x {
                            let pos = Position::new(x, y);
                            if !matches!(
                                building.feature_at(pos, floor),
                                Some(Feature::Door | Feature::Gate)
                            ) {
                                continue;
                            }
                            let axis = building
                                .door_axis_at(pos, floor)
                                .expect("generated door axis");
                            for cy in ((y - 2) >> 4)..=((y + 2) >> 4) {
                                for cx in ((x - 2) >> 4)..=((x + 2) >> 4) {
                                    if (0..CHUNKS_PER_SIDE).contains(&cx)
                                        && (0..CHUNKS_PER_SIDE).contains(&cy)
                                    {
                                        stage.materialize_chunk(&world, cx, cy);
                                    }
                                }
                            }
                            let tangent = axis.tangent();
                            for sign in [-1, 1] {
                                let anchor_pos =
                                    Position::new(x + sign * tangent.0, y + sign * tangent.1);
                                let anchor = building.feature_at(anchor_pos, floor);
                                if !matches!(anchor, Some(Feature::Wall(_)) | Some(Feature::Fence))
                                    && failures.len() < 30
                                {
                                    failures.push(format!("seed {seed} {:?} {pos:?} F{floor} {axis:?}: missing jamb anchor {anchor_pos:?}: {anchor:?}", building.kind));
                                }
                            }
                            let tile = Tile3D::from_pos(pos, floor);
                            let cell = stage.cell_at_world(tile).expect("door cell");
                            let TileKind::Door { exterior, .. } = cell.kind else {
                                panic!(
                                    "seed {seed} {:?} {tile:?}: generated door became {:?}",
                                    building.kind, cell.kind
                                );
                            };
                            assert_eq!(
                                cell.door_axis,
                                Some(axis),
                                "seed {seed} {:?} {tile:?}",
                                building.kind
                            );
                            let base = stage.building_by_id(cell.building_id).map_or_else(
                                || world.elevation_at(tile.center()).unwrap_or(0.0),
                                |info| info.foundation_z + floor as f32 * FLOOR_H,
                            );
                            let normal = axis.normal();
                            let direction = Vec2::new(normal.0 as f32, normal.1 as f32);
                            let center = tile.center();
                            for open in [true, false] {
                                stage.cell_mut_world(tile).unwrap().set_kind(
                                    TileKind::Door { open, exterior },
                                    Material::WoodDoor,
                                );
                                let query = Aabb3::new(
                                    center.extend(base) - Vec3::new(1.5, 1.5, 0.01),
                                    center.extend(base + super::super::PLAYER_HEIGHT)
                                        + Vec3::new(1.5, 1.5, 0.01),
                                );
                                let mut colliders = Vec::new();
                                stage.colliders_in(query, &mut colliders);
                                for sign in [-1.0, 1.0] {
                                    let start = center - direction * (sign * 0.8);
                                    let delta = direction * (sign * 1.6);
                                    let hit = colliders.iter().find(|c| {
                                        c.bounds.max.z > base + 0.001
                                            && c.bounds.min.z
                                                < base + super::super::PLAYER_HEIGHT - 0.001
                                            && sweep_circle_rect_2d(
                                                start,
                                                delta,
                                                super::super::PLAYER_RADIUS,
                                                c.bounds.min.xy(),
                                                c.bounds.max.xy(),
                                            )
                                            .is_some()
                                    });
                                    let valid = if open {
                                        hit.is_none()
                                    } else {
                                        hit.is_some_and(|c| matches!(c.kind, ColliderKind::ClosedDoor { tile: t } if t == tile))
                                    };
                                    if !valid && failures.len() < 30 {
                                        failures.push(format!("seed {seed} {:?} {tile:?} {axis:?} open={open} direction {sign}: {hit:?}", building.kind));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let plans = world.fixture_plans();
            assert!(
                !plans.is_empty(),
                "seed {seed}: default county fixtures must be enumerated"
            );
            for plan in &plans {
                for structure in &plan.structures {
                    for (cx, cy) in fixture_chunks(structure) {
                        stage.materialize_chunk(&world, cx, cy);
                    }
                    assert_blueprint_door_sweep(&mut stage, structure);
                }
            }
        }
        assert!(
            failures.is_empty(),
            "runtime doorway failures (first 30):\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn six_floor_structure_allocates_sparse_wings_and_collapses_through_the_tower() {
        let world = World::new(42);
        let (mut stage, _, _) = CountyStage::new(&world, 42);
        let (cx, cy) = (stage.window_cx, stage.window_cy);
        let adjacent = cx + 1;
        let tower = Position::new(adjacent * CHUNK_SIZE - 2, cy * CHUNK_SIZE + 4);
        let podium = Position::new(adjacent * CHUNK_SIZE, tower.y);
        assert_eq!(stage.chunk_allocated_floors(cx, cy), Some(1));
        assert_eq!(stage.chunk_allocated_floors(adjacent, cy), Some(1));
        let (wings, wing_count) = BuildingInfo::from_wings_slice(
            &[
                WingRect::new(tower.x, tower.y, tower.x + 1, tower.y, 6),
                WingRect::new(podium.x, podium.y, podium.x, podium.y, 2),
            ],
            WingRect::default(),
        );
        let info = BuildingInfo {
            id: 0,
            uid: BuildingUid::default(),
            name: "Stepped tower",
            x: tower.x,
            y: tower.y,
            width: 3,
            height: 1,
            floors: 6,
            style: BuildingStyle::HospitalClinical,
            front_door: tower,
            foundation_z: 0.0,
            wings,
            wing_count,
        };
        let mut cells = Vec::new();
        for (pos, height) in [
            (tower, 6),
            (Position::new(tower.x + 1, tower.y), 6),
            (podium, 2),
        ] {
            for floor in 0..height {
                let support = pos == tower && floor == 0;
                cells.push(CustomCellSpec {
                    pos,
                    floor,
                    surface: Surface::WoodFloor,
                    kind: if support {
                        TileKind::InteriorWall
                    } else {
                        TileKind::Empty
                    },
                    material: if support {
                        Material::Drywall
                    } else {
                        Material::None
                    },
                    indoor: true,
                    load_bearing: support,
                    stair_dir: (0, 0),
                    door_axis: None,
                });
            }
        }
        let id = stage.stamp_custom_building(
            &world,
            info,
            &cells,
            Tile3D::from_pos(tower, 0).center(),
            0,
        );
        assert_eq!(stage.window_max_floors(), 6);
        assert_eq!(stage.chunk_allocated_floors(cx, cy), Some(6));
        assert_eq!(stage.chunk_allocated_floors(adjacent, cy), Some(2));
        assert_eq!(
            stage.tile_render_info(podium, 2).unwrap().surface,
            Surface::Air
        );

        let roofs_at = |stage: &CountyStage, pos| {
            let xy = Tile3D::from_pos(pos, 0).center();
            let mut colliders = Vec::new();
            stage.colliders_in(
                Aabb3::new(
                    xy.extend(0.0) - Vec3::new(0.1, 0.1, 0.0),
                    xy.extend(8.0 * FLOOR_H) + Vec3::new(0.1, 0.1, 0.0),
                ),
                &mut colliders,
            );
            colliders
                .into_iter()
                .filter_map(|c| match c.kind {
                    ColliderKind::BuildingSlab {
                        building_id,
                        floor,
                        is_roof: true,
                    } if building_id == id => Some((floor, c.bounds.max.z)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(roofs_at(&stage, tower), vec![(6, 6.0 * FLOOR_H)]);
        assert_eq!(roofs_at(&stage, podium), vec![(2, 2.0 * FLOOR_H)]);

        let base = Tile3D::from_pos(tower, 0);
        let damage = stage.damage_tile(base, DamageKind::RocketBlast).unwrap();
        assert!(damage.destroyed);
        assert!(damage.collapsed_cells > 0);
        for floor in 1..6 {
            let cell = stage.cell_at_world(Tile3D::from_pos(tower, floor)).unwrap();
            assert!(cell.collapsed);
            assert_eq!(cell.surface, Surface::Air);
        }
        assert!(roofs_at(&stage, tower).is_empty());
        assert_eq!(roofs_at(&stage, podium), vec![(2, 2.0 * FLOOR_H)]);
    }

    #[test]
    fn building_uid_and_cache_id_are_independent_of_chunk_visit_order() {
        let world = World::new(42);
        let (mut first, _, _) = CountyStage::new(&world, 42);
        let (mut second, _, _) = CountyStage::new(&world, 42);
        let chunks: Vec<_> = (0..CHUNKS_PER_SIDE)
            .flat_map(|cy| (0..CHUNKS_PER_SIDE).map(move |cx| (cx, cy)))
            .filter(|&(cx, cy)| {
                first.chunks[CountyStage::chunk_idx(cx, cy)].is_none()
                    && world
                        .chunk_sites(cx, cy)
                        .is_some_and(|sites| !sites.buildings.is_empty() && !sites.loot.is_empty())
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
        let buildings = |stage: &CountyStage| {
            let mut buildings: Vec<_> = stage
                .all_buildings
                .iter()
                .map(|b| (b.uid, b.x, b.y))
                .collect();
            buildings.sort_unstable();
            buildings
        };
        let caches = |stage: &CountyStage| {
            let mut caches: Vec<_> = stage.caches.iter().map(|c| (c.id, c.pos)).collect();
            caches.sort_unstable_by_key(|&(id, _)| id);
            caches
        };
        assert_eq!(buildings(&first), buildings(&second));
        assert_eq!(caches(&first), caches(&second));
        for cache in first.caches.clone() {
            assert_eq!(first.take_cache(cache.id), Some(cache.loot));
            assert_eq!(first.take_cache(cache.id), None);
        }
    }

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

    fn fixture_chunks(structure: &PlacedStructure) -> Vec<(i32, i32)> {
        let bounds = structure.bounds;
        let mut chunks = Vec::new();
        for cy in (bounds.min.y >> 4)..=(bounds.max.y >> 4) {
            for cx in (bounds.min.x >> 4)..=(bounds.max.x >> 4) {
                if (0..CHUNKS_PER_SIDE).contains(&cx) && (0..CHUNKS_PER_SIDE).contains(&cy) {
                    chunks.push((cx, cy));
                }
            }
        }
        chunks
    }

    fn blueprint_snapshot(
        stage: &CountyStage,
        structure: &PlacedStructure,
    ) -> (
        BuildingInfo,
        Vec<(
            Position,
            Floor,
            BuildingUid,
            TileKind,
            Option<DoorAxis>,
            Surface,
            bool,
            bool,
        )>,
        Vec<PropAnchor>,
    ) {
        let mut info = stage
            .all_buildings
            .iter()
            .find(|b| b.uid == BuildingUid(structure.id.0))
            .copied()
            .expect("registered blueprint building");
        // The live numeric slot is assigned as chunks stream in. The UID is stable.
        info.id = 0;
        let mut cells = Vec::new();
        for (local, pos, _) in structure.world_cells() {
            let Some(cell) = stage.cell_at_world(Tile3D::from_pos(pos, local.floor)) else {
                continue;
            };
            let owner = stage
                .building_by_id(cell.building_id)
                .map_or(BuildingUid::default(), |b| b.uid);
            cells.push((
                pos,
                local.floor,
                owner,
                cell.kind,
                cell.door_axis,
                cell.surface,
                cell.indoor,
                cell.load_bearing,
            ));
        }
        cells.sort_by_key(|cell| (cell.0.x, cell.0.y, cell.1));
        let expected_props: std::collections::HashSet<_> = structure
            .props_world()
            .into_iter()
            .map(|prop| prop.anchor_uid)
            .collect();
        let mut props: Vec<_> = stage
            .blueprint_prop_anchors()
            .iter()
            .copied()
            .filter(|anchor| expected_props.contains(&anchor.anchor_uid))
            .collect();
        props.sort_by_key(|anchor| anchor.anchor_uid);
        (info, cells, props)
    }

    #[test]
    fn default_county_entry_uses_reference_street_and_legacy_is_explicit() {
        let seed = 42;
        let world = World::new(seed);
        assert_eq!(world.fixture(), CountyFixture::ResidentialStreet);
        assert_eq!(world.fixture_plans().len(), 5);
        let plan = world.fixture_plan().expect("default fixture plan");
        let fridge = plan.structures[0]
            .props_world()
            .into_iter()
            .find(|prop| prop.id == crate::action::PropId::Fridge)
            .expect("Craftsman fixture furnishes a fridge");
        assert_eq!(
            World::with_fixture(seed, CountyFixture::BlueprintLot).fixture(),
            CountyFixture::BlueprintLot
        );
        assert_eq!(World::legacy(seed).fixture(), CountyFixture::None);
        assert!(World::legacy(seed).fixture_plan().is_none());

        let mut game = crate::action::ActionGame::county(seed);
        game.teleport(Tile3D::from_pos(plan.frontage.access, 0))
            .expect("fixture access");
        assert!(
            game.prop_anchors()
                .iter()
                .any(|anchor| anchor.anchor_uid == fridge.anchor_uid),
            "default county run should stream the Craftsman furniture"
        );
        let ranch = world
            .fixture_plans()
            .into_iter()
            .find(|plan| !plan.outdoor.vehicle_slots.is_empty())
            .expect("default county includes the ranch parcel");
        game.teleport(Tile3D::from_pos(ranch.frontage.access, 0))
            .expect("ranch access");
        let car = ranch.outdoor.vehicle_slots[0].0;
        assert!(
            game.prop_anchors()
                .iter()
                .any(|anchor| anchor.id == crate::action::PropId::Car && anchor.tile.pos() == car),
            "default county run should stream the ranch garage car"
        );
    }

    fn assert_blueprint_door_sweep(stage: &mut CountyStage, structure: &PlacedStructure) {
        use crate::spatial::SpatialWorld;
        use crate::spatial::collision::sweep_circle_rect_2d;
        for door in &structure.blueprint.doors {
            let door_pos = structure.transform.local_to_world(door.cell);
            let axis = structure
                .door_axis_at(door.cell)
                .expect("world door axis for authored intent");
            let tile = Tile3D::from_pos(door_pos, door.cell.floor);
            let cell = *stage.cell_at_world(tile).expect("materialized door cell");
            let TileKind::Door { exterior, .. } = cell.kind else {
                panic!("authored door at {door_pos:?} became {:?}", cell.kind);
            };
            assert_eq!(cell.door_axis, Some(axis));
            let info = stage.building_by_id(cell.building_id).expect("door owner");
            assert_eq!(info.uid, BuildingUid(structure.id.0));
            let base = info.foundation_z + f32::from(door.cell.floor) * FLOOR_H;
            let normal = axis.normal();
            let direction = Vec2::new(normal.0 as f32, normal.1 as f32);
            let center = tile.center();
            for open in [true, false] {
                stage
                    .cell_mut_world(tile)
                    .unwrap()
                    .set_kind(TileKind::Door { open, exterior }, Material::WoodDoor);
                let query = Aabb3::new(
                    center.extend(base) - Vec3::new(1.5, 1.5, 0.01),
                    center.extend(base + crate::action::PLAYER_HEIGHT) + Vec3::new(1.5, 1.5, 0.01),
                );
                let mut colliders = Vec::new();
                stage.colliders_in(query, &mut colliders);
                for sign in [-1.0, 1.0] {
                    let start = center - direction * (sign * 0.8);
                    let delta = direction * (sign * 1.6);
                    let hit = colliders.iter().find(|c| {
                        c.bounds.max.z > base + 0.001
                            && c.bounds.min.z < base + crate::action::PLAYER_HEIGHT - 0.001
                            && sweep_circle_rect_2d(
                                start,
                                delta,
                                crate::action::PLAYER_RADIUS,
                                c.bounds.min.xy(),
                                c.bounds.max.xy(),
                            )
                            .is_some()
                    });
                    let valid = if open {
                        hit.is_none()
                    } else {
                        hit.is_some_and(
                            |c| matches!(c.kind, ColliderKind::ClosedDoor { tile: t } if t == tile),
                        )
                    };
                    assert!(
                        valid,
                        "door at {door_pos:?} open={open} sign={sign} hit={hit:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn craftsman_fixture_selection_works_across_representative_seeds() {
        for seed in [0, 1, 2, 7, 42, 63, u64::MAX] {
            let world = World::with_fixture(seed, CountyFixture::CraftsmanLot);
            assert_eq!(world.fixture(), CountyFixture::CraftsmanLot, "seed {seed}");
            let plan = world
                .fixture_plan()
                .unwrap_or_else(|| panic!("seed {seed} selected no Craftsman fixture"));
            let structure = &plan.structures[0];
            assert_eq!(plan.structures.len(), 1);
            assert!(plan.bounds.contains(structure.bounds.min));
            assert!(plan.bounds.contains(structure.bounds.max));
            assert_eq!(structure.floors(), 1);
            assert!(
                !structure.blueprint.doors.is_empty(),
                "seed {seed} house must have doors"
            );
        }
    }

    #[test]
    fn blueprint_fixture_streams_across_seams_in_any_chunk_order() {
        // u64::MAX places the fixture outside the initial 7x7 window, so the
        // test actually exercises first-time streaming order.
        let seed = u64::MAX;
        let world = World::with_fixture(seed, CountyFixture::BlueprintLot);
        let plan = world.fixture_plan().expect("fixture plan");
        let structure = &plan.structures[0];
        assert!(plan.bounds.contains(structure.bounds.min));
        let props = structure.props_world();
        assert_eq!(props.len(), 1);
        let prop_tiles: Vec<_> = props[0].span.covered_tiles(props[0].tile).collect();
        assert!(
            prop_tiles.iter().any(|a| prop_tiles
                .iter()
                .any(|b| { a.x >> 4 != b.x >> 4 || a.y >> 4 != b.y >> 4 })),
            "fixture prop must straddle a chunk seam"
        );

        let chunks = fixture_chunks(structure);
        let (mut forward, _, _) = CountyStage::new(&world, seed);
        for &(cx, cy) in &chunks {
            forward.materialize_chunk(&world, cx, cy);
        }
        let (mut reverse, _, _) = CountyStage::new(&world, seed);
        for &(cx, cy) in chunks.iter().rev() {
            reverse.materialize_chunk(&world, cx, cy);
        }

        let a = blueprint_snapshot(&forward, structure);
        let b = blueprint_snapshot(&reverse, structure);
        assert_eq!(a.0.uid, BuildingUid(structure.id.0));
        assert_eq!(a.0.x, structure.bounds.min.x);
        assert_eq!(a.0.width, structure.bounds.width());
        assert_eq!(a.0.floors, structure.floors());
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, b.1);
        assert_eq!(a.1.len(), structure.blueprint.cells.len());
        assert_eq!(a.2, b.2);
        assert_eq!(a.2.len(), 1, "exactly one fixture prop anchor");
        assert!(a.1.iter().all(|cell| cell.2 == BuildingUid(structure.id.0)));
        assert_eq!(a.2[0].tile, props[0].tile);
        assert_eq!(a.2[0].span, props[0].span);
        assert_eq!(a.2[0].id, props[0].id);
    }

    #[test]
    fn blueprint_fixture_door_passes_runtime_bilateral_capsule_sweep() {
        use crate::spatial::SpatialWorld;
        use crate::spatial::collision::sweep_circle_rect_2d;
        let seed = 42;
        let world = World::with_fixture(seed, CountyFixture::BlueprintLot);
        let plan = world.fixture_plan().expect("fixture plan");
        let structure = &plan.structures[0];
        let door = structure.blueprint.doors[0].cell;
        let door_pos = structure.transform.local_to_world(door);
        let axis = structure.door_axis_at(door).expect("world door axis");
        let (mut stage, _, _) = CountyStage::new(&world, seed);
        for cy in ((door_pos.y - 2) >> 4)..=((door_pos.y + 2) >> 4) {
            for cx in ((door_pos.x - 2) >> 4)..=((door_pos.x + 2) >> 4) {
                if (0..CHUNKS_PER_SIDE).contains(&cx) && (0..CHUNKS_PER_SIDE).contains(&cy) {
                    stage.materialize_chunk(&world, cx, cy);
                }
            }
        }
        let tile = Tile3D::from_pos(door_pos, 0);
        let cell = stage.cell_at_world(tile).expect("door cell");
        let TileKind::Door { exterior, .. } = cell.kind else {
            panic!("fixture door became {:?}", cell.kind);
        };
        assert_eq!(cell.door_axis, Some(axis));
        let info = stage.building_by_id(cell.building_id).expect("door owner");
        assert_eq!(info.uid, BuildingUid(structure.id.0));
        let base = info.foundation_z;
        let normal = axis.normal();
        let direction = Vec2::new(normal.0 as f32, normal.1 as f32);
        let center = tile.center();
        for open in [true, false] {
            stage
                .cell_mut_world(tile)
                .unwrap()
                .set_kind(TileKind::Door { open, exterior }, Material::WoodDoor);
            let query = Aabb3::new(
                center.extend(base) - Vec3::new(1.5, 1.5, 0.01),
                center.extend(base + crate::action::PLAYER_HEIGHT) + Vec3::new(1.5, 1.5, 0.01),
            );
            let mut colliders = Vec::new();
            stage.colliders_in(query, &mut colliders);
            for sign in [-1.0, 1.0] {
                let start = center - direction * (sign * 0.8);
                let delta = direction * (sign * 1.6);
                let hit = colliders.iter().find(|c| {
                    c.bounds.max.z > base + 0.001
                        && c.bounds.min.z < base + crate::action::PLAYER_HEIGHT - 0.001
                        && sweep_circle_rect_2d(
                            start,
                            delta,
                            crate::action::PLAYER_RADIUS,
                            c.bounds.min.xy(),
                            c.bounds.max.xy(),
                        )
                        .is_some()
                });
                let valid = if open {
                    hit.is_none()
                } else {
                    hit.is_some_and(
                        |c| matches!(c.kind, ColliderKind::ClosedDoor { tile: t } if t == tile),
                    )
                };
                assert!(valid, "open={open} sign={sign} hit={hit:?}");
            }
        }
    }

    #[test]
    fn craftsman_fixture_streams_stable_cells_and_every_door_passes_capsule_sweep() {
        let seed = 42;
        let world = World::with_fixture(seed, CountyFixture::CraftsmanLot);
        let plan = world.fixture_plan().expect("Craftsman fixture plan");
        let structure = &plan.structures[0];
        assert_eq!(structure.floors(), 1);
        let ids: Vec<crate::action::PropId> = structure
            .props_world()
            .into_iter()
            .map(|prop| prop.id)
            .collect();
        for required in [
            crate::action::PropId::KitchenSink,
            crate::action::PropId::KitchenStove,
            crate::action::PropId::Fridge,
            crate::action::PropId::Sofa,
            crate::action::PropId::Toilet,
            crate::action::PropId::BathroomSink,
            crate::action::PropId::Bathtub,
        ] {
            assert!(ids.contains(&required), "missing {required:?}");
        }
        assert!(!structure.blueprint.signs.is_empty());

        let chunks = fixture_chunks(structure);
        let (mut forward, _, _) = CountyStage::new(&world, seed);
        for &(cx, cy) in &chunks {
            forward.materialize_chunk(&world, cx, cy);
        }
        let (mut reverse, _, _) = CountyStage::new(&world, seed);
        for &(cx, cy) in chunks.iter().rev() {
            reverse.materialize_chunk(&world, cx, cy);
        }
        let a = blueprint_snapshot(&forward, structure);
        let b = blueprint_snapshot(&reverse, structure);
        assert_eq!(a.0.uid, BuildingUid(structure.id.0));
        assert_eq!(a.0.floors, 1);
        assert_eq!(a.1.len(), structure.blueprint.cells.len());
        assert_eq!(a, b, "chunk visitation order must not change the plan");
        assert!(a.1.iter().all(|cell| cell.2 == BuildingUid(structure.id.0)));

        assert_blueprint_door_sweep(&mut forward, structure);
        assert_blueprint_door_sweep(&mut reverse, structure);
    }

    #[test]
    fn reference_street_doors_pass_capsule_sweeps_in_both_chunk_orders() {
        for seed in [42, 63] {
            let world = World::new(seed);
            let plans = world.fixture_plans();
            assert_eq!(plans.len(), 5);
            let mut chunks: Vec<_> = plans
                .iter()
                .flat_map(|plan| plan.structures.iter())
                .flat_map(fixture_chunks)
                .collect();
            chunks.sort_unstable();
            chunks.dedup();
            let (mut forward, _, _) = CountyStage::new(&world, seed);
            let (mut reverse, _, _) = CountyStage::new(&world, seed);
            for &(cx, cy) in &chunks {
                forward.materialize_chunk(&world, cx, cy);
            }
            for &(cx, cy) in chunks.iter().rev() {
                reverse.materialize_chunk(&world, cx, cy);
            }
            for structure in plans.iter().flat_map(|plan| &plan.structures) {
                assert_eq!(
                    blueprint_snapshot(&forward, structure),
                    blueprint_snapshot(&reverse, structure)
                );
                assert_blueprint_door_sweep(&mut forward, structure);
                assert_blueprint_door_sweep(&mut reverse, structure);
            }
        }
    }

    #[test]
    fn colonial_lot_streams_two_floors_in_both_chunk_orders() {
        for seed in [42, u64::MAX] {
            let world = World::with_fixture(seed, CountyFixture::ColonialLot);
            let plan = world.fixture_plan().expect("Colonial review lot");
            let structure = &plan.structures[0];
            assert_eq!(structure.blueprint.floors(), 2);
            assert!(!structure.blueprint.doors.is_empty());
            assert!(
                structure
                    .blueprint
                    .doors
                    .iter()
                    .any(|door| door.cell.floor == 1),
                "the sweep must include upstairs doors"
            );
            let chunks = fixture_chunks(structure);
            let (mut forward, _, _) = CountyStage::new(&world, seed);
            let (mut reverse, _, _) = CountyStage::new(&world, seed);
            for &(cx, cy) in &chunks {
                forward.materialize_chunk(&world, cx, cy);
            }
            for &(cx, cy) in chunks.iter().rev() {
                reverse.materialize_chunk(&world, cx, cy);
            }
            assert_eq!(
                blueprint_snapshot(&forward, structure),
                blueprint_snapshot(&reverse, structure)
            );
            assert_blueprint_door_sweep(&mut forward, structure);
            assert_blueprint_door_sweep(&mut reverse, structure);
        }
    }

    #[test]
    fn wp35_split_level_and_duplex_materialize_and_replay_live_contracts() {
        use crate::world::blueprint::{BlueprintKind, DoorKind, SpaceId};
        use std::collections::{BTreeSet, VecDeque};

        fn reachable(
            stage: &CountyStage,
            start: Tile3D,
            allowed: &BTreeSet<Tile3D>,
        ) -> BTreeSet<Tile3D> {
            let mut found = BTreeSet::from([start]);
            let mut queue = VecDeque::from([start]);
            while let Some(tile) = queue.pop_front() {
                for next in [
                    Tile3D::new(tile.x - 1, tile.y, tile.floor),
                    Tile3D::new(tile.x + 1, tile.y, tile.floor),
                    Tile3D::new(tile.x, tile.y - 1, tile.floor),
                    Tile3D::new(tile.x, tile.y + 1, tile.floor),
                ] {
                    if allowed.contains(&next)
                        && stage.is_walkable_world(next, false)
                        && found.insert(next)
                    {
                        queue.push_back(next);
                    }
                }
            }
            found
        }

        for (fixture, expected) in [
            (
                CountyFixture::SplitLevelLot,
                ["split_level_a", "split_level_b"],
            ),
            (CountyFixture::DuplexLot, ["duplex_a", "duplex_b"]),
        ] {
            let mut seen = BTreeSet::new();
            let mut seam_seen = BTreeSet::new();
            for seed in 0..64 {
                let world = World::with_fixture(seed, fixture);
                let plan = world.fixture_plan().expect("WP 3.5 fixture plan");
                assert_eq!(plan.structures.len(), 1, "seed {seed} {fixture:?}");
                let structure = &plan.structures[0];
                let first_layout = seen.insert(structure.archetype);
                let crossing_prop = structure.props_world().into_iter().find(|prop| {
                    let tiles: Vec<_> = prop.span.covered_tiles(prop.tile).collect();
                    tiles.iter().any(|a| {
                        tiles
                            .iter()
                            .any(|b| a.x >> 4 != b.x >> 4 || a.y >> 4 != b.y >> 4)
                    })
                });
                if let Some(prop) = crossing_prop {
                    seam_seen.insert(structure.archetype);
                    let mut game = crate::action::ActionGame::county_with_world(world.clone());
                    game.teleport(Tile3D::from_pos(prop.tile.pos(), prop.tile.floor))
                        .expect("seam prop streams");
                    let first = game
                        .prop_anchors()
                        .iter()
                        .filter(|anchor| anchor.anchor_uid == prop.anchor_uid)
                        .count();
                    assert_eq!(
                        first, 1,
                        "seed {seed} crossing prop has one registered anchor"
                    );
                    let prop_pos = prop.tile.pos();
                    let far_corner = [
                        Position::new(32, 32),
                        Position::new(480, 32),
                        Position::new(32, 480),
                        Position::new(480, 480),
                    ]
                    .into_iter()
                    .max_by_key(|corner| {
                        (corner.x - prop_pos.x).abs() + (corner.y - prop_pos.y).abs()
                    })
                    .unwrap();
                    let away = game
                        .teleport(Tile3D::from_pos(far_corner, 0))
                        .expect("stream away from seam prop");
                    assert!(
                        away.pos().distance(prop_pos) > 30,
                        "seam revisit must leave active streaming range"
                    );
                    game.teleport(Tile3D::from_pos(prop.tile.pos(), prop.tile.floor))
                        .expect("revisit seam prop");
                    let revisited = game
                        .prop_anchors()
                        .iter()
                        .filter(|anchor| anchor.anchor_uid == prop.anchor_uid)
                        .count();
                    assert_eq!(
                        revisited, 1,
                        "seed {seed} revisit must not duplicate crossing prop"
                    );
                }
                let chunks = fixture_chunks(structure);
                let (mut forward, _, _) = CountyStage::new(&world, seed);
                let (mut reverse, _, _) = CountyStage::new(&world, seed);
                for &(cx, cy) in &chunks {
                    forward.materialize_chunk(&world, cx, cy);
                }
                for &(cx, cy) in chunks.iter().rev() {
                    reverse.materialize_chunk(&world, cx, cy);
                }
                let a = blueprint_snapshot(&forward, structure);
                let b = blueprint_snapshot(&reverse, structure);
                assert_eq!(a, b, "fixture replay differs by chunk order, seed {seed}");
                assert!(a.1.iter().all(|cell| cell.2 == BuildingUid(structure.id.0)));
                for door in &structure.blueprint.doors {
                    let pos = structure.transform.local_to_world(door.cell);
                    if let Some(cell) =
                        forward.cell_mut_world(Tile3D::from_pos(pos, door.cell.floor))
                    {
                        let exterior = matches!(cell.kind, TileKind::Door { exterior: true, .. });
                        cell.set_kind(
                            TileKind::Door {
                                open: true,
                                exterior,
                            },
                            Material::WoodDoor,
                        );
                    }
                }
                assert_blueprint_door_sweep(&mut forward, structure);
                assert_blueprint_door_sweep(&mut reverse, structure);

                if fixture == CountyFixture::SplitLevelLot {
                    assert!(!structure.blueprint.stairs.is_empty(), "seed {seed}");
                    for link in &structure.blueprint.stairs {
                        let lower = structure.transform.local_to_world(link.lower);
                        let upper = structure.transform.local_to_world(link.upper);
                        assert_eq!(lower, upper);
                        let low_tile = Tile3D::from_pos(lower, link.lower.floor);
                        let high_tile = Tile3D::from_pos(upper, link.upper.floor);
                        assert_eq!(forward.stair_link(low_tile).unwrap().0, high_tile);
                        assert_eq!(forward.stair_link(high_tile).unwrap().0, low_tile);
                    }
                    let info = forward
                        .all_buildings
                        .iter()
                        .find(|b| b.uid == BuildingUid(structure.id.0))
                        .unwrap();
                    let wings: Vec<_> = structure
                        .blueprint
                        .rooms
                        .iter()
                        .map(|room| {
                            let p = structure.transform.local_to_world(room.cells[0]);
                            let index = info.wing_index_at(p, room.floor);
                            (index, info.wing_at(index).floors)
                        })
                        .collect();
                    assert!(
                        wings.iter().any(|(_, floors)| *floors == 1),
                        "seed {seed} {} wings {wings:?}",
                        structure.archetype
                    );
                    assert!(
                        wings.iter().any(|(_, floors)| *floors >= 2),
                        "seed {seed} {} wings {wings:?}",
                        structure.archetype
                    );
                    if first_layout {
                        let link = structure.blueprint.stairs[0];
                        let lower_landing = structure.transform.local_to_world(link.lower_landing);
                        let mut game = crate::action::ActionGame::county_with_world(world.clone());
                        assert_eq!(
                            game.teleport(Tile3D::from_pos(lower_landing, link.lower.floor))
                                .map(Tile3D::pos),
                            Some(lower_landing)
                        );
                        for _ in 0..40 {
                            game.step(crate::action::ActionInput::default());
                        }
                        game.step(crate::action::ActionInput {
                            interact: true,
                            ..Default::default()
                        });
                        assert_eq!(
                            game.player_pose().floor,
                            link.upper.floor,
                            "seed {seed} stair up"
                        );
                        for _ in 0..40 {
                            game.step(crate::action::ActionInput::default());
                        }
                        game.step(crate::action::ActionInput {
                            interact: true,
                            ..Default::default()
                        });
                        assert_eq!(
                            game.player_pose().floor,
                            link.lower.floor,
                            "seed {seed} stair down"
                        );
                    }
                } else {
                    for door in &structure.blueprint.doors {
                        let pos = structure.transform.local_to_world(door.cell);
                        if let Some(cell) =
                            forward.cell_mut_world(Tile3D::from_pos(pos, door.cell.floor))
                        {
                            let exterior =
                                matches!(cell.kind, TileKind::Door { exterior: true, .. });
                            cell.set_kind(
                                TileKind::Door {
                                    open: true,
                                    exterior,
                                },
                                Material::WoodDoor,
                            );
                        }
                    }
                    let exterior: Vec<_> = structure
                        .blueprint
                        .doors
                        .iter()
                        .filter(|door| door.kind == DoorKind::Exterior)
                        .collect();
                    assert_eq!(
                        exterior.len(),
                        2,
                        "seed {seed} must have both unit entrances"
                    );
                    let units: BTreeSet<_> = exterior
                        .iter()
                        .map(|door| {
                            let room_id = match door.connects.0 {
                                SpaceId::Room(id) => id,
                                _ => match door.connects.1 {
                                    SpaceId::Room(id) => id,
                                    _ => panic!("entry door must name a room"),
                                },
                            };
                            structure
                                .blueprint
                                .rooms
                                .iter()
                                .find(|room| room.id == room_id)
                                .and_then(|room| room.unit)
                                .expect("entry room has unit ownership")
                        })
                        .collect();
                    assert_eq!(
                        units.len(),
                        2,
                        "seed {seed} has distinct unit entry ownership"
                    );
                    let room_units: BTreeSet<_> = structure
                        .blueprint
                        .rooms
                        .iter()
                        .filter_map(|room| room.unit)
                        .collect();
                    assert_eq!(room_units, units, "seed {seed} room ownership");
                    let mut party_wall = None;
                    for (&local, cell) in &structure.blueprint.cells {
                        if cell.kind != BlueprintKind::InteriorWall {
                            continue;
                        }
                        for (du, dv) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            let neighbor =
                                crate::LocalCell::new(local.u + du, local.v + dv, local.floor);
                            let Some(other) = structure.blueprint.cells.get(&neighbor) else {
                                continue;
                            };
                            let room_a = structure
                                .blueprint
                                .rooms
                                .iter()
                                .find(|r| r.cells.contains(&neighbor));
                            let room_b = structure.blueprint.rooms.iter().find(|r| {
                                r.cells.contains(&crate::LocalCell::new(
                                    local.u - du,
                                    local.v - dv,
                                    local.floor,
                                ))
                            });
                            if let (Some(a), Some(b)) = (room_a, room_b) {
                                if a.unit != b.unit
                                    && a.unit.is_some()
                                    && b.unit.is_some()
                                    && other.kind == BlueprintKind::Floor
                                {
                                    party_wall = Some(local);
                                }
                            }
                        }
                    }
                    let party_wall = party_wall.expect("seed must retain a solid party wall");
                    let wall_world = structure.transform.local_to_world(party_wall);
                    assert_eq!(
                        forward
                            .cell_at_world(Tile3D::from_pos(wall_world, party_wall.floor))
                            .unwrap()
                            .kind,
                        TileKind::InteriorWall,
                        "seed {seed} party wall must materialize intact"
                    );
                    let mut unit_reachable = BTreeSet::new();
                    for door in exterior {
                        let entry_room = match door.connects.0 {
                            SpaceId::Room(id) => id,
                            _ => match door.connects.1 {
                                SpaceId::Room(id) => id,
                                _ => panic!("entry door must name its room"),
                            },
                        };
                        let room = structure
                            .blueprint
                            .rooms
                            .iter()
                            .find(|room| room.id == entry_room)
                            .expect("entry room");
                        let unit = room.unit.expect("unit-owned entry room");
                        let mut allowed: BTreeSet<_> = structure
                            .blueprint
                            .rooms
                            .iter()
                            .filter(|candidate| candidate.unit == Some(unit))
                            .flat_map(|candidate| candidate.cells.iter())
                            .map(|cell| {
                                Tile3D::from_pos(
                                    structure.transform.local_to_world(*cell),
                                    cell.floor,
                                )
                            })
                            .collect();
                        for intent in &structure.blueprint.doors {
                            let joins_unit =
                                [intent.connects.0, intent.connects.1].iter().any(|space| {
                                    match space {
                                        SpaceId::Room(id) => structure
                                            .blueprint
                                            .rooms
                                            .iter()
                                            .find(|r| r.id == *id)
                                            .is_some_and(|r| r.unit == Some(unit)),
                                        SpaceId::Unit(id) => *id == unit,
                                        SpaceId::Exterior => false,
                                    }
                                });
                            if joins_unit {
                                let pos = structure.transform.local_to_world(intent.cell);
                                allowed.insert(Tile3D::from_pos(pos, intent.cell.floor));
                            }
                        }
                        let door_pos = structure.transform.local_to_world(door.cell);
                        let (dx, dy) = plan.frontage.side.inward();
                        let entry_room_cells: BTreeSet<_> = room
                            .cells
                            .iter()
                            .map(|cell| {
                                Tile3D::from_pos(
                                    structure.transform.local_to_world(*cell),
                                    cell.floor,
                                )
                            })
                            .collect();
                        let start = [
                            Tile3D::new(door_pos.x - 1, door_pos.y, door.cell.floor),
                            Tile3D::new(door_pos.x + 1, door_pos.y, door.cell.floor),
                            Tile3D::new(door_pos.x, door_pos.y - 1, door.cell.floor),
                            Tile3D::new(door_pos.x, door_pos.y + 1, door.cell.floor),
                        ]
                        .into_iter()
                        .find(|candidate| {
                            entry_room_cells.contains(candidate)
                                && forward.is_walkable_world(*candidate, false)
                        })
                        .unwrap_or_else(|| {
                            Tile3D::from_pos(
                                Position::new(door_pos.x + dx, door_pos.y + dy),
                                door.cell.floor,
                            )
                        });
                        let reached = reachable(&forward, start, &allowed);
                        assert!(
                            reached.contains(&start),
                            "seed {seed} unit entry not walkable"
                        );
                        for target_room in structure
                            .blueprint
                            .rooms
                            .iter()
                            .filter(|candidate| candidate.unit == Some(unit))
                        {
                            assert!(
                                target_room.cells.iter().any(|cell| reached.contains(
                                    &Tile3D::from_pos(
                                        structure.transform.local_to_world(*cell),
                                        cell.floor
                                    )
                                )),
                                "seed {seed} unit {unit} cannot reach room {}",
                                target_room.id
                            );
                        }
                        unit_reachable.insert(unit);
                    }
                    assert_eq!(
                        unit_reachable, units,
                        "both duplex homes must be reachable from their doors"
                    );
                }
                if expected
                    .iter()
                    .all(|key| seen.contains(key) && seam_seen.contains(key))
                {
                    break;
                }
            }
            for key in expected {
                assert!(seen.contains(key), "missing layout {key} for {fixture:?}");
                assert!(
                    seam_seen.contains(key),
                    "missing chunk-crossing prop for {key}"
                );
            }
        }
    }

    #[test]
    fn support_fixture_streams_stairs_wings_and_collapses_after_breach() {
        let world = World::with_fixture(42, CountyFixture::SupportLot);
        let structure = &world.fixture_plan().expect("support fixture").structures[0];
        let (mut stage, _, _) = CountyStage::new(&world, 42);
        for (cx, cy) in fixture_chunks(structure) {
            stage.materialize_chunk(&world, cx, cy);
        }
        let info = stage
            .all_buildings
            .iter()
            .find(|b| b.uid == BuildingUid(structure.id.0))
            .unwrap();
        assert_eq!(info.wing_count, 2);
        let tower = structure
            .transform
            .local_to_world(crate::LocalCell::new(2, 3, 0));
        let wing = structure
            .transform
            .local_to_world(crate::LocalCell::new(8, 3, 0));
        assert_eq!(info.wing_at(info.wing_index_at(tower, 0)).floors, 2);
        assert_eq!(info.wing_at(info.wing_index_at(wing, 0)).floors, 1);
        assert_ne!(
            info.cutaway_slot_at(tower, 0),
            info.cutaway_slot_at(wing, 0)
        );
        let id = info.id;
        for (pos, expected_floor) in [(tower, 2), (wing, 1)] {
            let center = Tile3D::from_pos(pos, 0).center();
            let mut colliders = Vec::new();
            stage.colliders_in(
                Aabb3::new(
                    center.extend(-100.0) - Vec3::new(0.1, 0.1, 0.0),
                    center.extend(100.0) + Vec3::new(0.1, 0.1, 0.0),
                ),
                &mut colliders,
            );
            assert!(colliders.iter().any(|c| matches!(c.kind, ColliderKind::BuildingSlab { building_id, floor, is_roof: true } if building_id == id && floor == expected_floor)));
        }
        let stair = structure
            .transform
            .local_to_world(structure.blueprint.stairs[0].lower);
        for floor in 0..2 {
            let tile = Tile3D::from_pos(stair, floor);
            let (dest, direction) = stage.stair_link(tile).expect("live stair link");
            assert_eq!(dest.floor, 1 - floor);
            let landing_dir = if floor == 0 { (-1, 0) } else { (1, 0) };
            assert_eq!(direction, structure.transform.world_stair_dir(landing_dir));
        }
        let adjacent = structure
            .transform
            .local_to_world(crate::LocalCell::new(0, 2, 0));
        let _ = stage.damage_tile(Tile3D::from_pos(adjacent, 0), DamageKind::RocketBlast);
        let wall = structure
            .transform
            .local_to_world(crate::LocalCell::new(0, 3, 0));
        let damage = stage
            .damage_tile(Tile3D::from_pos(wall, 0), DamageKind::RocketBlast)
            .unwrap();
        assert!(damage.destroyed && damage.collapsed_cells > 0);
        assert!(
            stage
                .cell_at_world(Tile3D::from_pos(wall, 1))
                .unwrap()
                .collapsed
        );
    }

    #[test]
    fn blueprint_fixture_registers_roof_owner_and_single_prop_after_revisit() {
        use crate::spatial::SpatialWorld;
        let seed = u64::MAX;
        let world = World::with_fixture(seed, CountyFixture::BlueprintLot);
        let plan = world.fixture_plan().expect("fixture plan");
        let structure = &plan.structures[0];
        let (mut stage, _, _) = CountyStage::new(&world, seed);
        for &(cx, cy) in &fixture_chunks(structure) {
            stage.materialize_chunk(&world, cx, cy);
        }
        let info = stage
            .all_buildings
            .iter()
            .find(|b| b.uid == BuildingUid(structure.id.0))
            .copied()
            .unwrap();
        let center = Tile3D::from_pos(structure.bounds.min, 0).center();
        let mut colliders = Vec::new();
        stage.colliders_in(
            Aabb3::new(
                center.extend(info.foundation_z - 0.1)
                    - Vec3::new(
                        structure.bounds.width() as f32,
                        structure.bounds.height() as f32,
                        0.0,
                    ),
                center.extend(info.foundation_z + FLOOR_H + 1.0)
                    + Vec3::new(
                        structure.bounds.width() as f32,
                        structure.bounds.height() as f32,
                        0.0,
                    ),
            ),
            &mut colliders,
        );
        assert!(colliders.iter().any(|c| matches!(
            c.kind,
            ColliderKind::BuildingSlab {
                building_id,
                is_roof: true,
                ..
            } if building_id == info.id
        )));

        let prop = structure.props_world()[0];
        let spawn = world.wake_position();
        let mut game = crate::action::ActionGame::county_with_world(world.clone());
        game.teleport(Tile3D::from_pos(prop.tile.pos(), 0))
            .expect("teleport to fixture");
        let count = |game: &crate::action::ActionGame| {
            game.prop_anchors()
                .iter()
                .filter(|a| a.anchor_uid == prop.anchor_uid)
                .count()
        };
        assert_eq!(count(&game), 1);
        game.teleport(Tile3D::from_pos(spawn, 0))
            .expect("teleport away");
        game.teleport(Tile3D::from_pos(prop.tile.pos(), 0))
            .expect("teleport back");
        assert_eq!(count(&game), 1);
    }
}
