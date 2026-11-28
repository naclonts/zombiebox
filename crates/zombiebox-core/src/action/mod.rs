//! Smooth, fixed-step multi-level residential action runtime (`60 Hz`).
//! The legacy 20 Hz tile-command `Game` in `lib.rs` remains unchanged.

pub mod ai;
pub mod block;
pub mod county;
pub mod prop_catalog;
pub mod props;
pub mod readables;
pub mod types;
pub mod vehicles;
pub mod weapons;

pub use ai::{MonsterSpec, monster_spec};
pub use prop_catalog::{PropCover, PropDef, PropId};
pub use props::{CellAttachment, PropAnchor, PropFacing, PropSpan, PropState, PropView};
pub use readables::{ReadableId, readable_text};
pub use types::*;
pub use vehicles::{
    ControlMode, MotorcycleCrashKind, MotorcycleInput, MotorcyclePhase, MotorcycleView, MountedHud,
    RideInput, VehicleId, VehicleView,
};
pub use weapons::{
    Inventory, Projectile, ReloadMode, WEAPON_MUZZLE_HEIGHT, WeaponSpec, weapon_spec,
};

use crate::spatial::{
    Aabb3, BodyShape, Collider, FLOOR_H, MotionState, MoveParams, QueryFilter, Ray3, SurfaceQuad,
    TerrainSample, move_kinematic_with_horizontal_vel, resolve_actor_support, resolve_tile_support,
    select_grounded_support, semantic_floor_for_body, support_normal_at,
};
use crate::{District, House, Position, World};
use ai::{AiSystem, Monster, damage_monster, propagate_noise};
use block::{ResidentialBlock, SupplyCache};
use county::CountyStage;
use vehicles::{MotorcycleTuning, VehicleContactBody, Vehicles, step_motorcycle};
use weapons::{
    compute_player_aim_3d, is_rocket_danger_close_3d, step_projectiles_3d, try_fire_or_swing_3d,
};

pub const PLAYER_MAX_SPEED: f32 = 5.2;
pub const PLAYER_QUIET_STICK_THRESHOLD: f32 = 0.52;
pub const PLAYER_DEADZONE: f32 = 0.08;
pub const AIM_DEADZONE: f32 = 0.15;
pub const PLAYER_RADIUS: f32 = 0.23;
pub const PLAYER_HEIGHT: f32 = 1.05;
/// Eye height above the player's feet for first-person/fog-of-war sight checks.
pub const PLAYER_EYE_HEIGHT: f32 = 0.92;
pub const PLAYER_SHAPE: BodyShape = BodyShape::new(PLAYER_RADIUS, PLAYER_HEIGHT);
pub const PLAYER_HURT_IFRAMES: u8 = 24;
const FOOTSTEP_INTERVAL_TICKS: u8 = 18;
const KNOCKBACK_DECAY: f32 = 0.82;
const WINDOW_VAULT_SPEED_MULT: f32 = 0.60;
const DROP_RECOVERY_TICKS: u8 = 10;
const DROP_RECOVERY_SPEED_MULT: f32 = 0.42;
const STAIR_COOLDOWN_TICKS: u8 = 20;
const INTERACT_CACHE_RADIUS: f32 = 0.95;
const INTERACT_FEATURE_RADIUS: f32 = 1.12;
const DUSK_LEVEL: f32 = 0.72;

#[derive(Clone, Copy, Debug)]
enum InteractAction {
    Search(CacheId, Tile3D),
    SearchProp(u64, Tile3D),
    Read(ReadableId, Tile3D),
    ToggleDoor(Tile3D, bool), // bool = true if currently closed (so prompt is OpenDoor)
    DropWindow(Tile3D, Tile3D),
    ClimbWindow(Tile3D, Tile3D),
    TakeStairs(Tile3D, Tile3D, (i8, i8), bool), // bool = goes_up
    Mount(VehicleId, Tile3D),
    Dismount(VehicleId, Tile3D),
    RightBike(VehicleId, Tile3D),
}

impl InteractAction {
    const fn prompt(self) -> InteractPrompt {
        match self {
            Self::Search(..) | Self::SearchProp(..) => InteractPrompt::Search,
            Self::Read(..) => InteractPrompt::Read,
            Self::ToggleDoor(_, true) => InteractPrompt::OpenDoor,
            Self::ToggleDoor(_, false) => InteractPrompt::CloseDoor,
            Self::DropWindow(..) => InteractPrompt::DropWindow,
            Self::ClimbWindow(..) => InteractPrompt::ClimbWindow,
            Self::TakeStairs(_, _, _, true) => InteractPrompt::StairsUp,
            Self::TakeStairs(_, _, _, false) => InteractPrompt::StairsDown,
            Self::Mount(..) => InteractPrompt::Mount,
            Self::Dismount(..) => InteractPrompt::Dismount,
            Self::RightBike(..) => InteractPrompt::RightBike,
        }
    }

    const fn tile(self) -> Tile3D {
        match self {
            Self::Search(_, t)
            | Self::SearchProp(_, t)
            | Self::Read(_, t)
            | Self::ToggleDoor(t, _)
            | Self::DropWindow(t, _)
            | Self::ClimbWindow(t, _)
            | Self::TakeStairs(t, _, _, _)
            | Self::Mount(_, t)
            | Self::Dismount(_, t)
            | Self::RightBike(_, t) => t,
        }
    }
}

enum Stage {
    Residential(Box<ResidentialBlock>),
    County(Box<CountyStage>),
}

impl Stage {
    const fn mode(&self) -> ActionMode {
        match self {
            Self::Residential(_) => ActionMode::BirchStreet,
            Self::County(_) => ActionMode::County,
        }
    }

    const fn is_county(&self) -> bool {
        matches!(self, Self::County(_))
    }

    pub const fn window_max_floors(&self) -> u8 {
        match self {
            Self::Residential(_) => NUM_FLOORS as u8,
            Self::County(c) => c.window_max_floors(),
        }
    }

    pub fn county_chunk_allocated_floors(&self, cx: i32, cy: i32) -> Option<usize> {
        match self {
            Self::Residential(_) => None,
            Self::County(c) => c.chunk_allocated_floors(cx, cy),
        }
    }

    pub fn stamp_custom_building(
        &mut self,
        world: &World,
        info: BuildingInfo,
        cells: &[CustomCellSpec],
        viewer_pos: Vec2,
        viewer_floor: Floor,
    ) -> BuildingId {
        match self {
            Self::Residential(_) => {
                panic!("stamp_custom_building is not supported in Residential mode")
            }
            Self::County(c) => {
                c.stamp_custom_building(world, info, cells, viewer_pos, viewer_floor)
            }
        }
    }

    fn residential(&self) -> Option<&ResidentialBlock> {
        match self {
            Self::Residential(b) => Some(b),
            Self::County(_) => None,
        }
    }

    #[cfg(test)]
    fn residential_mut(&mut self) -> Option<&mut ResidentialBlock> {
        match self {
            Self::Residential(b) => Some(b),
            Self::County(_) => None,
        }
    }

    fn circle_walkable(
        &self,
        center: Vec2,
        floor: Floor,
        radius: f32,
        can_vault_window: bool,
    ) -> bool {
        match self {
            Self::Residential(b) => b.circle_walkable(center, floor, radius, can_vault_window),
            Self::County(c) => c.circle_walkable(center, floor, radius, can_vault_window),
        }
    }

    fn tile_kind(&self, tile: Tile3D) -> TileKind {
        match self {
            Self::Residential(b) => b.tile_kind(tile),
            Self::County(c) => c.tile_kind(tile),
        }
    }

    fn tile_visibility(&self, tile: Tile3D) -> Option<TileVisibility> {
        match self {
            Self::Residential(b) => b.tile_visibility(tile),
            Self::County(c) => c.tile_visibility(tile),
        }
    }

    fn is_indoor(&self, tile: Tile3D) -> bool {
        match self {
            Self::Residential(b) => b.is_indoor(tile),
            Self::County(c) => c.is_indoor(tile),
        }
    }

    fn tile_render_info(&self, pos: Position, floor: Floor) -> Option<TileRenderInfo> {
        match self {
            Self::Residential(b) => b.tile_render_info(pos, floor),
            Self::County(c) => c.tile_render_info(pos, floor),
        }
    }

    fn buildings(&self) -> &[BuildingInfo] {
        match self {
            Self::Residential(b) => b.buildings(),
            Self::County(c) => c.buildings(),
        }
    }

    fn caches(&self) -> &[SupplyCache] {
        match self {
            Self::Residential(b) => b.caches(),
            Self::County(c) => c.caches(),
        }
    }

    fn take_cache(&mut self, id: CacheId) -> Option<CacheLoot> {
        match self {
            Self::Residential(b) => b.take_cache(id),
            Self::County(c) => c.take_cache(id),
        }
    }

    fn cache_views(
        &self,
        player_pos: Vec2,
        player_floor: Floor,
        interact_radius: f32,
    ) -> Vec<CacheView> {
        match self {
            Self::Residential(b) => b.cache_views(player_pos, player_floor, interact_radius),
            Self::County(c) => c.cache_views(player_pos, player_floor, interact_radius),
        }
    }

    fn light_sources(&self) -> Vec<LightSourceView> {
        match self {
            Self::Residential(b) => b.light_sources(),
            Self::County(c) => c.light_sources(),
        }
    }

    fn update_visibility(&mut self, viewer: Vec2, floor: Floor) {
        match self {
            Self::Residential(b) => b.update_visibility(viewer, floor),
            Self::County(c) => c.update_visibility(viewer, floor),
        }
    }

    fn tick_flashes(&mut self) {
        match self {
            Self::Residential(b) => b.tick_flashes(),
            Self::County(c) => c.tick_flashes(),
        }
    }

    fn dynamic_flashes(&self) -> Vec<DynamicFlashView> {
        match self {
            Self::Residential(b) => b.dynamic_flashes(),
            Self::County(c) => c.dynamic_flashes(),
        }
    }

    fn take_collapsed_tiles(&mut self) -> Vec<Tile3D> {
        match self {
            Self::Residential(b) => b.take_collapsed_tiles(),
            Self::County(c) => c.take_collapsed_tiles(),
        }
    }

    fn spatial_cell(&self, tile: Tile3D) -> Option<SpatialCell> {
        match self {
            Self::Residential(b) => b.spatial_cell(tile),
            Self::County(c) => c.spatial_cell(tile),
        }
    }

    fn building_by_id(&self, id: BuildingId) -> Option<&BuildingInfo> {
        match self {
            Self::Residential(b) => b.building_by_id(id),
            Self::County(c) => c.building_by_id(id),
        }
    }

    fn playable_bounds(&self) -> (Position, Position) {
        match self {
            Self::Residential(b) => b.playable_bounds(),
            Self::County(c) => c.playable_bounds(),
        }
    }

    fn floors_at(&self, pos: Position) -> Floor {
        match self {
            Self::Residential(_) => NUM_FLOORS as Floor,
            Self::County(county) => {
                crate::spatial::action_adapter::SpatialStageSource::floors_at(county.as_ref(), pos)
            }
        }
    }
}

impl CombatWorld for Stage {
    fn bounds(&self) -> (Position, Position, u8) {
        match self {
            Self::Residential(b) => b.bounds(),
            Self::County(c) => c.bounds(),
        }
    }

    fn terrain_version(&self) -> u64 {
        match self {
            Self::Residential(b) => b.terrain_version(),
            Self::County(c) => c.terrain_version(),
        }
    }

    fn obstruction(&self, tile: Tile3D) -> TileObstruction {
        match self {
            Self::Residential(b) => b.obstruction(tile),
            Self::County(c) => c.obstruction(tile),
        }
    }

    fn has_floor(&self, tile: Tile3D) -> bool {
        match self {
            Self::Residential(b) => b.has_floor(tile),
            Self::County(c) => c.has_floor(tile),
        }
    }

    fn is_walkable(&self, tile: Tile3D, can_vault_window: bool) -> bool {
        match self {
            Self::Residential(b) => b.is_walkable(tile, can_vault_window),
            Self::County(c) => c.is_walkable(tile, can_vault_window),
        }
    }

    fn building_id(&self, tile: Tile3D) -> BuildingId {
        match self {
            Self::Residential(b) => b.building_id(tile),
            Self::County(c) => c.building_id(tile),
        }
    }

    fn stair_link(&self, tile: Tile3D) -> Option<(Tile3D, (i8, i8))> {
        match self {
            Self::Residential(b) => b.stair_link(tile),
            Self::County(c) => c.stair_link(tile),
        }
    }

    fn stair_links(&self, tile: Tile3D) -> ([Option<(Tile3D, (i8, i8))>; 2], usize) {
        match self {
            Self::Residential(b) => b.stair_links(tile),
            Self::County(c) => c.stair_links(tile),
        }
    }

    fn window_drop_target(&self, tile: Tile3D) -> Option<Tile3D> {
        match self {
            Self::Residential(b) => b.window_drop_target(tile),
            Self::County(c) => c.window_drop_target(tile),
        }
    }

    fn window_vault_target(&self, from: Tile3D, window: Tile3D) -> Option<Tile3D> {
        match self {
            Self::Residential(b) => b.window_vault_target(from, window),
            Self::County(c) => c.window_vault_target(from, window),
        }
    }

    fn light_level(&self, tile: Tile3D) -> f32 {
        match self {
            Self::Residential(b) => b.light_level(tile),
            Self::County(c) => c.light_level(tile),
        }
    }

    fn damage_tile(&mut self, tile: Tile3D, kind: DamageKind) -> Option<TileDamageResult> {
        match self {
            Self::Residential(b) => b.damage_tile(tile, kind),
            Self::County(c) => c.damage_tile(tile, kind),
        }
    }

    fn toggle_door(&mut self, tile: Tile3D) -> Option<bool> {
        match self {
            Self::Residential(b) => b.toggle_door(tile),
            Self::County(c) => c.toggle_door(tile),
        }
    }

    fn register_dynamic_flash(
        &mut self,
        pos: Vec3,
        floor: Floor,
        radius: f32,
        intensity: u8,
        ttl_ticks: u8,
    ) {
        match self {
            Self::Residential(b) => {
                b.register_dynamic_flash(pos, floor, radius, intensity, ttl_ticks)
            }
            Self::County(c) => c.register_dynamic_flash(pos, floor, radius, intensity, ttl_ticks),
        }
    }
}

impl SpatialWorld for Stage {
    fn terrain_at(&self, xy: Vec2) -> Option<TerrainSample> {
        match self {
            Self::Residential(b) => b.terrain_at(xy),
            Self::County(c) => c.terrain_at(xy),
        }
    }

    fn supports_at(&self, xy: Vec2, out: &mut Vec<crate::spatial::SupportSurface>) {
        match self {
            Self::Residential(b) => b.supports_at(xy, out),
            Self::County(c) => c.supports_at(xy, out),
        }
    }

    fn colliders_in(&self, bounds: Aabb3, out: &mut Vec<Collider>) {
        match self {
            Self::Residential(b) => b.colliders_in(bounds, out),
            Self::County(c) => c.colliders_in(bounds, out),
        }
    }

    fn surface_quads_in(&self, min_tile: Position, max_tile: Position, out: &mut Vec<SurfaceQuad>) {
        match self {
            Self::Residential(b) => b.surface_quads_in(min_tile, max_tile, out),
            Self::County(c) => c.surface_quads_in(min_tile, max_tile, out),
        }
    }
}

struct ActiveBlast {
    id: u32,
    kind: BlastKind,
    position: Vec3,
    floor: Floor,
    radius: f32,
    age_ticks: u8,
    total_ticks: u8,
}

/// Combat-facing view that overlays sparse fixture props on the generated stage.
struct StageWithProps<'a> {
    stage: &'a mut Stage,
    anchors: &'a [PropAnchor],
    tile_index: &'a std::collections::HashMap<Tile3D, u64>,
    hp: &'a mut std::collections::HashMap<u64, u16>,
    terrain_version: &'a mut u64,
}

impl SpatialWorld for StageWithProps<'_> {
    fn terrain_at(&self, xy: Vec2) -> Option<TerrainSample> {
        self.stage.terrain_at(xy)
    }
    fn supports_at(&self, xy: Vec2, out: &mut Vec<crate::spatial::SupportSurface>) {
        self.stage.supports_at(xy, out)
    }
    fn colliders_in(&self, bounds: Aabb3, out: &mut Vec<Collider>) {
        self.stage.colliders_in(bounds, out);
        for a in self.anchors {
            if self.hp.get(&a.anchor_uid).is_some_and(|hp| *hp == 0) {
                continue;
            }
            let d = a.id.def();
            let (sx, sy) = if matches!(a.facing, PropFacing::East | PropFacing::West) {
                (d.collision_max.y, d.collision_max.x)
            } else {
                (d.collision_max.x, d.collision_max.y)
            };
            let z = resolve_tile_support(self.stage, a.tile).0;
            let p = a.pivot(z);
            let c = Collider {
                bounds: Aabb3::new(
                    Vec3::new(p.x - sx, p.y - sy, p.z),
                    Vec3::new(p.x + sx, p.y + sy, p.z + d.collision_max.z),
                ),
                kind: crate::spatial::ColliderKind::Prop {
                    tile: a.tile,
                    prop: None,
                },
                material: d.material,
            };
            if c.bounds.intersects(bounds) {
                out.push(c);
            }
        }
    }
    fn surface_quads_in(&self, min: Position, max: Position, out: &mut Vec<SurfaceQuad>) {
        self.stage.surface_quads_in(min, max, out)
    }
}

impl CombatWorld for StageWithProps<'_> {
    fn bounds(&self) -> (Position, Position, u8) {
        self.stage.bounds()
    }
    fn terrain_version(&self) -> u64 {
        self.stage.terrain_version()
    }
    fn obstruction(&self, tile: Tile3D) -> TileObstruction {
        if let Some(uid) = self.tile_index.get(&tile)
            && !self.hp.get(uid).is_some_and(|hp| *hp == 0)
            && let Some(a) = self.anchors.iter().find(|a| a.anchor_uid == *uid)
        {
            return match a.id.def().cover {
                PropCover::None => TileObstruction::Open,
                PropCover::Low => TileObstruction::LowProp(a.id.def().material),
                PropCover::HighBlocker => TileObstruction::Wall(a.id.def().material),
            };
        }
        self.stage.obstruction(tile)
    }
    fn has_floor(&self, tile: Tile3D) -> bool {
        self.stage.has_floor(tile)
    }
    fn is_walkable(&self, tile: Tile3D, v: bool) -> bool {
        if let Some(uid) = self.tile_index.get(&tile)
            && !self.hp.get(uid).is_some_and(|hp| *hp == 0)
            && let Some(a) = self.anchors.iter().find(|a| a.anchor_uid == *uid)
            && !matches!(a.id.def().cover, PropCover::None)
        {
            return false;
        }
        self.stage.is_walkable(tile, v)
    }
    fn building_id(&self, tile: Tile3D) -> BuildingId {
        self.stage.building_id(tile)
    }
    fn stair_link(&self, tile: Tile3D) -> Option<(Tile3D, (i8, i8))> {
        self.stage.stair_link(tile)
    }
    fn stair_links(&self, tile: Tile3D) -> ([Option<(Tile3D, (i8, i8))>; 2], usize) {
        self.stage.stair_links(tile)
    }
    fn window_drop_target(&self, tile: Tile3D) -> Option<Tile3D> {
        self.stage.window_drop_target(tile)
    }
    fn window_vault_target(&self, a: Tile3D, b: Tile3D) -> Option<Tile3D> {
        self.stage.window_vault_target(a, b)
    }
    fn light_level(&self, tile: Tile3D) -> f32 {
        self.stage.light_level(tile)
    }
    fn damage_tile(&mut self, tile: Tile3D, kind: DamageKind) -> Option<TileDamageResult> {
        let Some(uid) = self.tile_index.get(&tile) else {
            return self.stage.damage_tile(tile, kind);
        };
        let Some(a) = self.anchors.iter().find(|a| a.anchor_uid == *uid) else {
            return self.stage.damage_tile(tile, kind);
        };
        let d = a.id.def();
        let hp = self.hp.entry(a.anchor_uid).or_insert(d.durability);
        if *hp == 0 {
            return None;
        }
        let amount = block::damage_against(d.material, kind);
        if amount == 0 {
            return None;
        }
        *hp = hp.saturating_sub(amount);
        *self.terrain_version = self.terrain_version.wrapping_add(1);
        Some(TileDamageResult {
            damaged: true,
            destroyed: *hp == 0,
            material: d.material,
            old_kind: TileKind::Prop(PropKind::Crate),
            new_kind: if *hp == 0 {
                TileKind::Rubble
            } else {
                TileKind::Prop(PropKind::Crate)
            },
            sound_radius: 7.5,
            light_changed: false,
            collapsed_cells: 0,
        })
    }
    fn toggle_door(&mut self, tile: Tile3D) -> Option<bool> {
        self.stage.toggle_door(tile)
    }
    fn register_dynamic_flash(&mut self, pos: Vec3, floor: Floor, r: f32, i: u8, ttl: u8) {
        self.stage.register_dynamic_flash(pos, floor, r, i, ttl)
    }
}

fn readable_fixtures(spawn: Tile3D, county: bool) -> Vec<(ReadableId, PropAnchor)> {
    [
        (
            if county {
                ReadableId::CountyDirectory
            } else {
                ReadableId::BirchDirectory
            },
            spawn.y - 1,
        ),
        (ReadableId::MaintenanceNote, spawn.y - 2),
        (ReadableId::SurvivorJournal, spawn.y - 3),
    ]
    .into_iter()
    .enumerate()
    .map(|(slot, (id, y))| {
        (
            id,
            PropAnchor {
                anchor_uid: 0x5244_0000_0000_0000 | slot as u64,
                tile: Tile3D::new(spawn.x, y, spawn.floor),
                id: match id {
                    ReadableId::BirchDirectory => PropId::Sign,
                    ReadableId::CountyDirectory => PropId::Sign,
                    ReadableId::MaintenanceNote => PropId::Note,
                    ReadableId::SurvivorJournal => PropId::Journal,
                    ReadableId::CraftsmanNote => PropId::Note,
                    ReadableId::GroceryNotice => PropId::Note,
                    ReadableId::FuelPrices => PropId::FuelPylon,
                },
                span: PropSpan::new(1, 1),
                attachment: CellAttachment::WallMount,
                facing: PropFacing::South,
                offset: Vec3::new(0.0, 0.0, 1.1),
            },
        )
    })
    .collect()
}

pub struct ActionGame {
    seed: u64,
    world: World,
    stage: Stage,
    tick: u64,
    player: Pose,
    player_body: BodyState,
    fall_peak_z: Option<f32>,
    player_knockback: Vec2,
    health: u8,
    max_health: u8,
    hurt_iframes: u8,
    footstep_timer: u8,
    drop_recovery_ticks: u8,
    stair_cooldown: u8,
    must_leave_stair: Option<Position>,
    last_stair_travel_up: bool,
    quiet_walking: bool,
    aim_explicit: bool,
    inventory: Inventory,
    monsters: Vec<Monster>,
    zombies_total: usize,
    next_monster_id: u64,
    projectiles: Vec<Projectile>,
    next_proj_id: u32,
    active_blasts: Vec<ActiveBlast>,
    next_blast_id: u32,
    vehicles: Vehicles,
    ai: AiSystem,
    events: Vec<ActionEvent>,
    noises: Vec<NoiseEvent>,
    outcome: ActionOutcome,
    prop_anchors: Vec<PropAnchor>,
    prop_opened: std::collections::HashSet<u64>,
    prop_hp: std::collections::HashMap<u64, u16>,
    prop_tile_index: std::collections::HashMap<Tile3D, u64>,
    prop_revision: u64,
    blueprint_prop_cursor: usize,
    blueprint_readable_cursor: usize,
    readable_fixtures: Vec<(ReadableId, PropAnchor)>,
    readable_id: ReadableId,
    read_open: bool,
}

impl ActionGame {
    pub fn new(seed: u64) -> Self {
        Self::county(seed)
    }

    pub fn county(seed: u64) -> Self {
        Self::county_with_world(World::new(seed))
    }

    pub fn county_with_world(world: World) -> Self {
        let seed = world.seed();
        let (mut county, spawn, monsters) = CountyStage::new(&world, seed);
        let player = Pose {
            position: spawn.center(),
            facing: Vec2::new(0.0, -1.0),
            floor: spawn.floor,
        };
        let (pz, psup, _) = resolve_tile_support(&county, spawn);
        let player_body = BodyState::from_pose_2d(player.position, player.facing, pz, psup);
        let zombies_total = county.total_monsters_spawned();
        county.update_visibility(player.position, player.floor);

        let vehicle_spawns = crate::world::motorcycle_spawns(&world, spawn.pos(), |spawn| {
            let pos = spawn.tile;
            if county.contains_world_tile(pos) {
                if !county.circle_walkable(Vec2::new(pos.x as f32, pos.y as f32), 0, 0.55, false) {
                    return false;
                }
                if county
                    .caches()
                    .iter()
                    .any(|c| c.pos.floor == 0 && c.pos.pos().distance(pos) <= 1)
                {
                    return false;
                }
            }
            true
        });
        let vehicles = Vehicles::from_spawns(seed, &vehicle_spawns, |tile| {
            let (z, support, _) = resolve_tile_support(&county, Tile3D::from_pos(tile, 0));
            (z, support)
        });

        let stage = Stage::County(Box::new(county));
        let readable = readable_fixtures(spawn, true);
        let prop_anchors = readable.iter().map(|(_, anchor)| *anchor).collect();
        let mut game = Self {
            seed,
            world,
            stage,
            tick: 0,
            player,
            player_body,
            fall_peak_z: None,
            player_knockback: Vec2::ZERO,
            health: PLAYER_MAX_HEALTH,
            max_health: PLAYER_MAX_HEALTH,
            hurt_iframes: 0,
            footstep_timer: 0,
            drop_recovery_ticks: 0,
            stair_cooldown: 0,
            must_leave_stair: None,
            last_stair_travel_up: false,
            quiet_walking: false,
            aim_explicit: false,
            inventory: Inventory::new(),
            monsters,
            zombies_total,
            next_monster_id: 1u64 << 63,
            projectiles: Vec::new(),
            next_proj_id: 1,
            active_blasts: Vec::new(),
            next_blast_id: 1,
            vehicles,
            ai: AiSystem::new(),
            events: Vec::new(),
            noises: Vec::new(),
            outcome: ActionOutcome::Playing,
            prop_anchors,
            prop_opened: Default::default(),
            prop_hp: Default::default(),
            prop_tile_index: Default::default(),
            prop_revision: 0,
            blueprint_prop_cursor: 0,
            blueprint_readable_cursor: 0,
            readable_fixtures: readable,
            readable_id: ReadableId::CountyDirectory,
            read_open: false,
        };
        game.sync_county_blueprint_props();
        game.sync_county_blueprint_readables();
        game
    }

    pub fn residential(seed: u64) -> Self {
        let world = World::birch_street(seed);
        let mut block = ResidentialBlock::new(&world, seed);
        let spawn = block.player_spawn();
        let player = Pose {
            position: spawn.center(),
            facing: Vec2::new(0.0, -1.0),
            floor: spawn.floor,
        };
        let (pz, psup, _) = resolve_tile_support(&block, spawn);
        let player_body = BodyState::from_pose_2d(player.position, player.facing, pz, psup);

        let mut next_monster_id = 1u64;
        let monsters: Vec<Monster> = block
            .monster_spawns()
            .iter()
            .map(|sp| {
                let id = MonsterId(next_monster_id);
                next_monster_id += 1;
                Monster::new_in_world(id, sp.kind, sp.variant, sp.pos, &block)
            })
            .collect();
        let zombies_total = monsters.len();

        block.update_visibility(player.position, player.floor);

        let stage = Stage::Residential(Box::new(block));
        let readable = readable_fixtures(spawn, false);
        let prop_anchors = readable.iter().map(|(_, anchor)| *anchor).collect();
        Self {
            seed,
            world,
            stage,
            tick: 0,
            player,
            player_body,
            fall_peak_z: None,
            player_knockback: Vec2::ZERO,
            health: PLAYER_MAX_HEALTH,
            max_health: PLAYER_MAX_HEALTH,
            hurt_iframes: 0,
            footstep_timer: 0,
            drop_recovery_ticks: 0,
            stair_cooldown: 0,
            must_leave_stair: None,
            last_stair_travel_up: false,
            quiet_walking: false,
            aim_explicit: false,
            inventory: Inventory::new(),
            monsters,
            zombies_total,
            next_monster_id,
            projectiles: Vec::new(),
            next_proj_id: 1,
            active_blasts: Vec::new(),
            next_blast_id: 1,
            vehicles: Vehicles::default(),
            ai: AiSystem::new(),
            events: Vec::new(),
            noises: Vec::new(),
            outcome: ActionOutcome::Playing,
            prop_anchors,
            prop_opened: Default::default(),
            prop_hp: Default::default(),
            prop_tile_index: Default::default(),
            prop_revision: 0,
            blueprint_prop_cursor: 0,
            blueprint_readable_cursor: 0,
            readable_fixtures: readable,
            readable_id: ReadableId::BirchDirectory,
            read_open: false,
        }
    }

    pub fn mode(&self) -> ActionMode {
        self.stage.mode()
    }

    pub const fn window_max_floors(&self) -> u8 {
        self.stage.window_max_floors()
    }

    pub fn county_chunk_allocated_floors(&self, cx: i32, cy: i32) -> Option<usize> {
        self.stage.county_chunk_allocated_floors(cx, cy)
    }

    pub fn stamp_custom_building(
        &mut self,
        info: BuildingInfo,
        cells: &[CustomCellSpec],
    ) -> BuildingId {
        self.stage.stamp_custom_building(
            &self.world,
            info,
            cells,
            self.player.position,
            self.player.floor,
        )
    }

    pub fn is_county(&self) -> bool {
        self.stage.is_county()
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Authoritative control mode from the latest fixed tick.
    pub fn control_mode(&self) -> ControlMode {
        match self.vehicles.mounted() {
            Some(_) => ControlMode::Motorcycle,
            None => ControlMode::OnFoot,
        }
    }

    /// Vehicle currently occupied by the player, if any.
    pub fn mounted_vehicle(&self) -> Option<VehicleId> {
        self.vehicles.mounted()
    }

    /// True while a crash has temporarily removed full on-foot control.
    pub fn is_recovering_from_vehicle_crash(&self) -> bool {
        self.vehicles
            .iter()
            .any(|(_, v)| v.motorcycle().is_some_and(|b| b.recovery_ticks > 0))
    }

    /// Read-only vehicle views filtered to the active streaming bounds.
    pub fn vehicle_views(&self) -> impl Iterator<Item = (VehicleId, VehicleView)> + '_ {
        let (min, max, _) = self.stage.bounds();
        self.vehicles.iter().filter_map(move |(id, state)| {
            let p = state.position();
            (p.x >= min.x as f32 - 0.5
                && p.x <= max.x as f32 + 0.5
                && p.y >= min.y as f32 - 0.5
                && p.y <= max.y as f32 + 0.5)
                .then(|| (id, state.view(&self.stage)))
        })
    }

    pub fn player_tile(&self) -> Tile3D {
        Tile3D::from_vec2(self.player.position, self.player.floor)
    }

    pub fn current_district(&self) -> District {
        if !self.is_county() {
            return District::Residential;
        }
        self.world
            .district_at(self.player_tile().pos())
            .unwrap_or(District::Farm)
    }

    pub fn district_code(&self) -> u8 {
        if !self.is_county() {
            return 0;
        }
        match self.current_district() {
            District::Residential => 1,
            District::City => 2,
            District::Hospital => 3,
            District::Police => 4,
            District::MilitaryBase => 5,
            District::Farm => 6,
            District::Woods => 7,
        }
    }

    pub fn teleport(&mut self, target: Tile3D) -> Option<Tile3D> {
        if self.mode() != ActionMode::County
            || self.outcome != ActionOutcome::Playing
            || self.control_mode() != ControlMode::OnFoot
            || self.is_recovering_from_vehicle_crash()
        {
            return None;
        }
        let current_pos = self.player_tile().pos();
        let Stage::County(county) = &mut self.stage else {
            return None;
        };
        let dest = county.find_teleport_destination(&self.world, target);
        let Some(dest) = dest else {
            county.sync_active_window(&self.world, current_pos, &mut self.monsters);
            return None;
        };

        county.sync_active_window(&self.world, dest.pos(), &mut self.monsters);
        let (dz, dsup, _) = resolve_tile_support(county.as_ref(), dest);
        self.player.position = dest.center();
        self.player.floor = dest.floor;
        self.player_body =
            BodyState::from_pose_2d(self.player.position, self.player.facing, dz, dsup);
        self.fall_peak_z = None;
        self.player_knockback = Vec2::ZERO;
        self.drop_recovery_ticks = 0;
        self.stair_cooldown = STAIR_COOLDOWN_TICKS;
        self.must_leave_stair = Some(dest.pos());
        county.update_visibility(self.player.position, self.player.floor);
        self.sync_county_blueprint_props();
        self.sync_county_blueprint_readables();
        Some(dest)
    }

    pub fn step(&mut self, input: ActionInput) {
        self.step_impl(input, true);
    }

    /// Run a fixed tick without enabling motorcycle interaction candidates.
    pub fn step_without_vehicle_interactions(&mut self, input: ActionInput) {
        self.step_impl(input, false);
    }

    fn step_impl(&mut self, input: ActionInput, vehicle_interactions: bool) {
        self.events.clear();
        self.noises.clear();
        if self.read_open && !input.interact {
            self.read_open = false;
        }
        if self.outcome != ActionOutcome::Playing {
            return;
        }
        self.tick += 1;
        let start_mode = self.control_mode();

        // 1. Tick timers, active blasts & dynamic light flashes.
        for b in &mut self.active_blasts {
            b.age_ticks = b.age_ticks.saturating_add(1);
        }
        self.active_blasts.retain(|b| b.age_ticks < b.total_ticks);
        self.stage.tick_flashes();
        self.inventory.tick_timers(&mut self.events);
        self.hurt_iframes = self.hurt_iframes.saturating_sub(1);
        self.drop_recovery_ticks = self.drop_recovery_ticks.saturating_sub(1);
        self.stair_cooldown = self.stair_cooldown.saturating_sub(1);
        self.footstep_timer = self.footstep_timer.saturating_sub(1);

        // 2. On-foot weapon switching, quick-heal, and manual reload. These
        // actions are unavailable while seated.
        if start_mode == ControlMode::OnFoot {
            if let Some(weapon) = input.select_weapon {
                self.inventory.select_weapon(weapon, &mut self.events);
            } else if input.cycle_weapon {
                self.inventory.cycle_weapon(&mut self.events);
            }
            if input.use_medkit {
                self.inventory.try_use_medkit_3d(
                    &mut self.health,
                    self.max_health,
                    self.player_body.position,
                    self.player.floor,
                    &mut self.events,
                    &mut self.noises,
                );
            }
            if input.reload {
                self.inventory.try_start_reload(&mut self.events);
            }
        }

        // 3. Route movement by the authoritative mode at the start of the tick.
        if start_mode == ControlMode::Motorcycle {
            self.step_mounted_movement(input.ride);
        } else {
            self.step_player_movement(input);
        }
        self.step_unmounted_vehicles();
        let focus = self.player_tile().pos();
        if let Stage::County(county) = &mut self.stage {
            county.sync_active_window(&self.world, focus, &mut self.monsters);
        }
        self.sync_county_blueprint_props();
        self.sync_county_blueprint_readables();

        // 4. On-foot aim / facing update (right-stick retains facing on release).
        // Vehicle heading is authoritative while mounted; carried look input is ignored.
        if start_mode == ControlMode::OnFoot {
            let aim_raw = input.aim.filter(|aim| {
                let len_sq = aim.length_squared();
                len_sq.is_finite() && len_sq > AIM_DEADZONE * AIM_DEADZONE
            });
            if let Some(aim) = aim_raw {
                let aim = aim.normalized();
                self.player.facing = aim;
                self.player_body.heading = aim.y.atan2(aim.x);
                self.aim_explicit = true;
            } else if !self.aim_explicit {
                let move_len_sq = input.movement.length_squared();
                if move_len_sq.is_finite() && move_len_sq > PLAYER_DEADZONE * PLAYER_DEADZONE {
                    let mv = input.movement.normalized();
                    self.player.facing = mv;
                    self.player_body.heading = mv.y.atan2(mv.x);
                }
            }
        }

        // 5. Contextual interaction. Mounted ticks accept only dismount; on-foot
        // ticks keep the existing cache/door/window/stair priority and then bikes.
        if input.interact {
            self.resolve_interact(vehicle_interactions);
        }

        let ev_combat_start = self.events.len();

        // 6. Firing / melee swing. A mode change in this tick suppresses carried fire.
        if start_mode == ControlMode::OnFoot
            && self.control_mode() == ControlMode::OnFoot
            && input.fire
        {
            let mut combat = StageWithProps {
                stage: &mut self.stage,
                anchors: &self.prop_anchors,
                tile_index: &self.prop_tile_index,
                hp: &mut self.prop_hp,
                terrain_version: &mut self.prop_revision,
            };
            try_fire_or_swing_3d(
                &mut combat,
                &mut self.inventory,
                self.player,
                self.player_body,
                &mut self.monsters,
                &mut self.projectiles,
                &mut self.next_proj_id,
                self.seed,
                self.tick,
                &mut self.events,
                &mut self.noises,
            );
        }

        // 7. Advance active rockets and shooter bolts on every path.
        let mut combat = StageWithProps {
            stage: &mut self.stage,
            anchors: &self.prop_anchors,
            tile_index: &self.prop_tile_index,
            hp: &mut self.prop_hp,
            terrain_version: &mut self.prop_revision,
        };
        step_projectiles_3d(
            &mut combat,
            &mut self.projectiles,
            self.player,
            self.player_body,
            PLAYER_RADIUS,
            &mut self.health,
            &mut self.hurt_iframes,
            &mut self.player_knockback,
            &mut self.monsters,
            &mut self.events,
            &mut self.noises,
        );

        // 8. Resolve any structural overhang collapses caused by breaching.
        self.process_collapses();

        // 9. Propagate player/combat noises to monsters.
        let combat = StageWithProps {
            stage: &mut self.stage,
            anchors: &self.prop_anchors,
            tile_index: &self.prop_tile_index,
            hp: &mut self.prop_hp,
            terrain_version: &mut self.prop_revision,
        };
        propagate_noise(&combat, &mut self.monsters, &self.noises, &mut self.events);

        // 10. Advance sight/sound-reactive AI, 3D pathfinding, and monster attacks.
        let mut combat = StageWithProps {
            stage: &mut self.stage,
            anchors: &self.prop_anchors,
            tile_index: &self.prop_tile_index,
            hp: &mut self.prop_hp,
            terrain_version: &mut self.prop_revision,
        };
        self.ai.step_monsters_3d(
            &mut combat,
            &mut self.monsters,
            self.player,
            self.player_body,
            PLAYER_RADIUS,
            self.quiet_walking,
            &mut self.health,
            &mut self.hurt_iframes,
            &mut self.player_knockback,
            &mut self.projectiles,
            &mut self.next_proj_id,
            self.seed,
            self.tick,
            &mut self.events,
            &mut self.noises,
        );

        // Any collapses from monster/bloater destruction.
        self.process_collapses();

        // Record any newly detonated blasts for 3D renderers.
        for ev in &self.events[ev_combat_start..] {
            match *ev {
                ActionEvent::RocketBlast { at, floor, radius } => {
                    let id = self.next_blast_id;
                    self.next_blast_id = self.next_blast_id.wrapping_add(1);
                    self.active_blasts.push(ActiveBlast {
                        id,
                        kind: BlastKind::Rocket,
                        position: at,
                        floor,
                        radius,
                        age_ticks: 0,
                        total_ticks: 23,
                    });
                }
                ActionEvent::BloaterBurst {
                    at, floor, radius, ..
                } => {
                    let id = self.next_blast_id;
                    self.next_blast_id = self.next_blast_id.wrapping_add(1);
                    self.active_blasts.push(ActiveBlast {
                        id,
                        kind: BlastKind::BloaterCaustic,
                        position: at,
                        floor,
                        radius,
                        age_ticks: 0,
                        total_ticks: 20,
                    });
                }
                _ => {}
            }
        }

        // 11. Compact dead (non-swelling) monsters and check outcome.
        self.monsters.retain(|m| m.is_alive() || m.is_swelling());

        if self.health == 0 {
            self.outcome = ActionOutcome::Dead;
            self.events.push(ActionEvent::Died);
        } else if !self.is_county() && self.monsters.is_empty() {
            self.outcome = ActionOutcome::Won;
            self.events.push(ActionEvent::Won);
        }

        // 12. Update room-based LOS visibility.
        self.stage
            .update_visibility(self.player.position, self.player.floor);
    }

    fn vehicle_contact_bodies(&self) -> Vec<VehicleContactBody> {
        self.monsters
            .iter()
            .filter(|m| m.alive && m.floor == 0)
            .map(|m| {
                let spec = m.spec();
                VehicleContactBody {
                    id: m.id.0 as u64,
                    bounds: Aabb3::from_center_half_extents(
                        m.body.position + Vec3::new(0.0, 0.0, spec.height * 0.5),
                        Vec3::new(spec.radius, spec.radius, spec.height * 0.5),
                    ),
                }
            })
            .collect()
    }

    fn step_mounted_movement(&mut self, ride: Option<RideInput>) {
        let Some(id) = self.vehicles.mounted() else {
            return;
        };
        let input = match ride {
            Some(RideInput::Motorcycle(input)) => Some(input),
            None => Some(MotorcycleInput::default()),
        };
        let tuning = MotorcycleTuning::default();
        let contacts = self.vehicle_contact_bodies();
        let (body, step, phase, condition) = {
            let stage = &self.stage;
            let Some(bike) = self.vehicles.motorcycle_mut(id) else {
                return;
            };
            let step = step_motorcycle(bike, input, stage, &contacts, &tuning);
            (bike.body, step, bike.phase, bike.condition)
        };
        if let Some(kind) = step.wipeout {
            self.events.push(ActionEvent::VehicleCrash {
                id,
                at: body.position,
                kind: kind.into(),
            });
            self.vehicles.set_mounted(None);
            if kind == vehicles::MotorcycleWipeout::HighSpeedEjection {
                self.events.push(ActionEvent::VehicleSkidStarted {
                    id,
                    at: body.position,
                    velocity: body.velocity,
                });
                let damage = if step.pre_impact_speed > 16.0 { 2 } else { 1 };
                self.health = self.health.saturating_sub(damage);
                self.hurt_iframes = 24;
                self.events.push(ActionEvent::PlayerHurt {
                    at: body.position,
                    floor: 0,
                    damage,
                    remaining_hp: self.health,
                    from: body.position - body.velocity * 0.04,
                });
                self.drop_recovery_ticks = 45;
                self.player_knockback = body.velocity.xy() * 0.55;
            } else {
                self.drop_recovery_ticks = 24;
                self.player_knockback = body.velocity.xy() * 0.1;
            }
        }
        if phase == MotorcyclePhase::Skidding && condition == 0 {
            self.vehicles.set_mounted(None);
        }
        let mut target = None;
        if let Some(target_id) = step.contact_target {
            if let Some(monster) = self
                .monsters
                .iter_mut()
                .find(|m| m.id.0 as u64 == target_id)
            {
                let damage = (step.pre_impact_speed / 3.0).round().clamp(1.0, 5.0) as u8;
                let dir = body.velocity.xy().normalized();
                let _ = damage_monster(monster, damage, dir * 5.0, 16, false, &mut self.events);
                target = Some(monster.id);
            }
        }
        if step.impact {
            if target.is_none() {
                self.apply_vehicle_terrain_damage(body, step.pre_impact_speed);
            }
            let damage = (step.pre_impact_speed * 0.7).round().clamp(1.0, 18.0) as u8;
            self.events.push(ActionEvent::VehicleImpact {
                id,
                at: body.position,
                target,
                speed: step.pre_impact_speed,
                damage,
            });
        }
        self.player.position = body.position.xy();
        self.player.floor = 0;
        self.player.facing = body.facing_xy();
        self.player_body = body;
        self.quiet_walking = false;
        if step.wipeout.is_none() {
            self.player_knockback = Vec2::ZERO;
        }
        self.aim_explicit = false;
    }

    fn apply_vehicle_terrain_damage(&mut self, body: BodyState, speed: f32) {
        let direction = if body.velocity.xy().length_squared() > 0.04 {
            body.velocity.xy().normalized()
        } else {
            body.facing_xy()
        };
        let tile = Tile3D::from_vec2(body.position.xy() + direction * 0.70, 0);
        let Some(result) = self.damage_tile(tile, DamageKind::VehicleImpact) else {
            return;
        };
        if !result.damaged {
            return;
        }
        let hit_pos = tile.center().extend(body.position.z + 0.45);
        self.events.push(ActionEvent::MaterialHit {
            at: tile,
            hit_pos,
            hit_normal: (-direction).extend(0.0),
            material: result.material,
            destroyed: result.destroyed,
            light_changed: result.light_changed,
        });
        if result.sound_radius > 0.0 {
            self.noises.push(NoiseEvent {
                origin: tile.center(),
                origin_z: body.position.z,
                floor: 0,
                radius: result.sound_radius.max(speed * 0.6),
                priority: 2,
                kind: NoiseKind::PropBreak,
            });
        }
    }

    fn step_unmounted_vehicles(&mut self) {
        let ids: Vec<_> = self
            .vehicles
            .iter()
            .filter_map(|(id, v)| {
                (Some(id) != self.vehicles.mounted()
                    && v.motorcycle()
                        .is_some_and(|b| !b.is_stopped() || b.recovery_ticks > 0))
                .then_some(id)
            })
            .collect();
        let tuning = MotorcycleTuning::default();
        for id in ids {
            let contacts = self.vehicle_contact_bodies();
            let (step, body) = {
                let Some(bike) = self.vehicles.motorcycle_mut(id) else {
                    continue;
                };
                let step = step_motorcycle(bike, None, &self.stage, &contacts, &tuning);
                (step, bike.body)
            };
            if step.impact {
                let mut target = None;
                if let Some(target_id) = step.contact_target
                    && let Some(monster) = self
                        .monsters
                        .iter_mut()
                        .find(|m| m.id.0 as u64 == target_id)
                {
                    let damage = (step.pre_impact_speed / 3.0).round().clamp(1.0, 5.0) as u8;
                    let dir = body.velocity.xy().normalized();
                    let _ = damage_monster(monster, damage, dir * 5.0, 16, false, &mut self.events);
                    target = Some(monster.id);
                }
                if target.is_none() {
                    self.apply_vehicle_terrain_damage(body, step.pre_impact_speed);
                }
                let damage = if target.is_some() {
                    (step.pre_impact_speed * 0.45).round().clamp(1.0, 14.0) as u8
                } else {
                    (step.pre_impact_speed * 0.7).round().clamp(1.0, 18.0) as u8
                };
                self.events.push(ActionEvent::VehicleImpact {
                    id,
                    at: body.position,
                    target,
                    speed: step.pre_impact_speed,
                    damage,
                });
            }
        }
    }

    fn sync_mounted_pose(&mut self, id: VehicleId) {
        let Some(body) = self.vehicles.motorcycle(id).map(|bike| bike.body) else {
            return;
        };
        self.player.position = body.position.xy();
        self.player.floor = 0;
        self.player.facing = body.facing_xy();
        self.player_body = body;
    }

    fn mount_candidate(&self) -> Option<VehicleId> {
        if self.control_mode() != ControlMode::OnFoot
            || self.player.floor != 0
            || self.is_recovering_from_vehicle_crash()
        {
            return None;
        }
        let eye = self.player_eye_position();
        let mut best: Option<(f32, VehicleId)> = None;
        for (id, state) in self.vehicles.iter() {
            let Some(bike) = state.motorcycle() else {
                continue;
            };
            if !bike.is_operable()
                || !bike.body.motion.is_grounded()
                || !matches!(
                    bike.phase,
                    MotorcyclePhase::Parked | MotorcyclePhase::Rolling | MotorcyclePhase::Skidding
                )
            {
                continue;
            }
            let d = bike.body.position.xy().distance(self.player.position);
            if d > 1.35 || (bike.body.position.z - self.player_body.position.z).abs() > 0.75 {
                continue;
            }
            let seat = bike.body.position.with_z(bike.body.position.z + 0.55);
            if let Some((ray, dist)) = Ray3::from_points(eye, seat)
                && dist > 0.1
                && self
                    .spatial()
                    .raycast_filtered(ray, dist - 0.08, QueryFilter::LINE_OF_SIGHT)
                    .is_some()
            {
                continue;
            }
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, id));
            }
        }
        best.map(|(_, id)| id)
    }

    fn dismount_candidate(&self) -> Option<VehicleId> {
        if self.control_mode() != ControlMode::Motorcycle {
            return None;
        }
        let id = self.vehicles.mounted()?;
        let bike = self.vehicles.motorcycle(id)?;
        if !bike.body.motion.is_grounded() {
            return None;
        }
        self.dismount_spot(id).map(|_| id)
    }

    fn right_bike_candidate(&self) -> Option<VehicleId> {
        if self.control_mode() != ControlMode::OnFoot || self.player.floor != 0 {
            return None;
        }
        let mut best: Option<(f32, VehicleId)> = None;
        for (id, state) in self.vehicles.iter() {
            let Some(bike) = state.motorcycle() else {
                continue;
            };
            if bike.phase != MotorcyclePhase::Tipped || !bike.is_operable() {
                continue;
            }
            if bike.recovery_ticks > 0 {
                continue;
            }
            let d = bike.body.position.xy().distance(self.player.position);
            if d <= 1.35 && (bike.body.position.z - self.player_body.position.z).abs() <= 0.75 {
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, id));
                }
            }
        }
        best.map(|(_, id)| id)
    }

    fn dismount_spot(&self, id: VehicleId) -> Option<(Vec2, f32, crate::spatial::SupportId)> {
        let bike = self.vehicles.motorcycle(id)?;
        let center = bike.body.position.xy();
        let forward = bike.body.facing_xy();
        let right = Vec2::new(-forward.y, forward.x);
        for offset in [right, -right, -forward] {
            let spot = center + offset * 0.9;
            if !self.stage.circle_walkable(spot, 0, PLAYER_RADIUS, false) {
                continue;
            }
            let (z, support, surface) = resolve_actor_support(&self.stage, spot, 0);
            if matches!(surface, Surface::Water | Surface::Mountain) {
                continue;
            }
            if (z - bike.body.position.z).abs() > 0.45 {
                continue;
            }
            return Some((spot, z, support));
        }
        None
    }

    fn try_mount(&mut self, id: VehicleId) {
        if self.control_mode() != ControlMode::OnFoot || self.mount_candidate() != Some(id) {
            return;
        }
        if self.vehicles.motorcycle(id).is_none() {
            return;
        }
        self.vehicles.set_mounted(Some(id));
        if let Some(bike) = self.vehicles.motorcycle_mut(id) {
            bike.begin_ride();
        }
        self.sync_mounted_pose(id);
        self.fall_peak_z = None;
        self.player_knockback = Vec2::ZERO;
        self.drop_recovery_ticks = 0;
        self.aim_explicit = false;
        self.quiet_walking = false;
    }

    fn try_dismount(&mut self, id: VehicleId) {
        if self.dismount_candidate() != Some(id) {
            return;
        }
        let Some((spot, z, support)) = self.dismount_spot(id) else {
            return;
        };
        let moving = self
            .vehicles
            .motorcycle(id)
            .is_some_and(|bike| !bike.is_stopped());
        let facing = self.player.facing;
        self.vehicles.set_mounted(None);
        if let Some(bike) = self.vehicles.motorcycle_mut(id) {
            if moving {
                bike.release_rider();
            } else {
                bike.set_parked();
            }
        }
        self.player.position = spot;
        self.player.floor = 0;
        self.player_body = BodyState::from_pose_2d(spot, facing, z, support);
        self.fall_peak_z = None;
        self.player_knockback = Vec2::ZERO;
        self.drop_recovery_ticks = 0;
        self.aim_explicit = false;
    }

    fn try_right_bike(&mut self, id: VehicleId) {
        if self.right_bike_candidate() != Some(id) {
            return;
        }
        if let Some(bike) = self.vehicles.motorcycle_mut(id) {
            bike.phase = MotorcyclePhase::Parked;
            bike.body.velocity = Vec3::ZERO;
            bike.body.pitch = 0.0;
            bike.body.roll = 0.0;
            bike.steer_angle = 0.0;
            bike.rider_lean = 0.0;
        }
    }

    fn sync_player_if_overridden(&mut self) {
        let xy_changed =
            (self.player.position - self.player_body.position.xy()).length_squared() > 1e-7;
        let semantic_fl = semantic_floor_for_body(&self.stage, self.player_body, self.player.floor);
        let floor_changed =
            self.player_body.motion.is_grounded() && self.player.floor != semantic_fl;
        if xy_changed || floor_changed {
            let mut sups = Vec::with_capacity(4);
            self.stage.supports_at(self.player.position, &mut sups);
            let gz = self
                .stage
                .terrain_at(self.player.position)
                .map_or(0.0, |t| t.z);
            let desired_z = gz + self.player.floor as f32 * FLOOR_H;
            let (z, sup) = if let Some(s) =
                select_grounded_support(&sups, desired_z, None, 0.22, 0.30, 0.85, true)
                && (self.player.floor == 0 || s.floor == self.player.floor)
            {
                (s.z, s.id)
            } else {
                let tile = Tile3D::from_vec2(self.player.position, self.player.floor);
                let (rz, rsup, _) = resolve_tile_support(&self.stage, tile);
                (rz, rsup)
            };
            self.player_body =
                BodyState::from_pose_2d(self.player.position, self.player.facing, z, sup);
            self.fall_peak_z = None;
        } else if self.player.facing.length_squared() > 1e-6 {
            self.player_body.heading = self.player.facing.y.atan2(self.player.facing.x);
        }
    }

    fn compute_fall_damage(drop_h: f32) -> (u8, u8) {
        if drop_h <= FLOOR_H * 1.35 {
            let rec = if drop_h >= 0.90 {
                DROP_RECOVERY_TICKS
            } else {
                0
            };
            (0, rec)
        } else if drop_h <= FLOOR_H * 2.45 {
            (1, 16)
        } else if drop_h <= FLOOR_H * 3.60 {
            (2, 22)
        } else {
            let dmg = (3 + ((drop_h - 5.0).max(0.0) * 0.5).floor() as u8).min(PLAYER_MAX_HEALTH);
            (dmg, 26)
        }
    }

    fn step_player_movement(&mut self, input: ActionInput) {
        self.sync_player_if_overridden();

        let raw_len = input.movement.length();
        let mut mag = if raw_len <= PLAYER_DEADZONE {
            0.0
        } else {
            raw_len.min(1.0)
        };
        if input.quiet_walk && mag > 0.48 {
            mag = 0.48;
        }
        let dir = if raw_len > PLAYER_DEADZONE {
            input.movement * (1.0 / raw_len)
        } else {
            Vec2::ZERO
        };

        self.quiet_walking = mag <= PLAYER_QUIET_STICK_THRESHOLD;

        let cur_tile = Tile3D::from_vec2(self.player.position, self.player.floor);
        let mut speed_mult = self.inventory.move_speed_mult();
        if self.stage.obstruction(cur_tile) == TileObstruction::BrokenWindow {
            speed_mult *= WINDOW_VAULT_SPEED_MULT;
        }
        if self.drop_recovery_ticks > 0 {
            speed_mult *= DROP_RECOVERY_SPEED_MULT;
        }

        // Hill slope effect on walking: climbing uphill slightly reduces walking speed,
        // descending downhill slightly increases walking speed.
        if mag > 0.0 && self.player_body.motion.is_grounded() && self.player.floor == 0 {
            let normal = self
                .stage
                .terrain_at(self.player.position)
                .map_or(Vec3::Z, |t| t.normal);
            let slope_grade = -dir.dot(Vec2::new(normal.x, normal.y)) / normal.z.max(0.1);
            let slope_mult = if slope_grade > 0.0 {
                1.0 - (slope_grade * 0.65).clamp(0.0, 0.25)
            } else {
                1.0 + ((-slope_grade) * 0.30).clamp(0.0, 0.10)
            };
            speed_mult *= slope_mult;
        }

        let horiz_vel =
            dir * (mag * PLAYER_MAX_SPEED * speed_mult) + self.player_knockback * (1.0 / DT);
        let params = MoveParams {
            can_vault_broken_windows: self.player.floor == 0,
            avoid_water: true,
            doorway_assist: true,
            ..MoveParams::default()
        };

        let prev_pos = self.player_body.position;
        let prev_motion = self.player_body.motion;
        let prev_floor = self.player.floor;
        let prev_tile = Tile3D::from_vec2(prev_pos.xy(), prev_floor);

        let (landed_impact_speed, ground_surface) = if horiz_vel.length_squared() > 1e-8
            || !self.player_body.motion.is_grounded()
            || !self.stage.has_floor(prev_tile)
        {
            let move_res = move_kinematic_with_horizontal_vel(
                &self.spatial(),
                PLAYER_SHAPE,
                self.player_body,
                horiz_vel,
                DT,
                &params,
            );
            self.player_body = move_res.state;
            self.player.position = self.player_body.position.xy();
            (
                move_res.landed_impact_speed,
                resolve_actor_support(
                    &self.stage,
                    self.player.position,
                    semantic_floor_for_body(&self.stage, self.player_body, prev_floor),
                )
                .2,
            )
        } else {
            let (_, _, surf) = resolve_actor_support(&self.stage, self.player.position, prev_floor);
            (0.0_f32, surf)
        };

        self.player_knockback *= KNOCKBACK_DECAY;
        if self.player_knockback.length_squared() < 1e-6 {
            self.player_knockback = Vec2::ZERO;
        }

        // 7. Airborne peak tracking, landing & fall injury curve.
        if !self.player_body.motion.is_grounded() {
            let cur_max = prev_pos.z.max(self.player_body.position.z);
            self.fall_peak_z = Some(self.fall_peak_z.map_or(cur_max, |p| p.max(cur_max)));
        } else if !prev_motion.is_grounded() {
            let peak_z = self.fall_peak_z.take().unwrap_or(prev_pos.z);
            let drop_h = (peak_z - self.player_body.position.z).max(0.0);
            let landed_floor = semantic_floor_for_body(&self.stage, self.player_body, prev_floor);
            if landed_floor != prev_floor {
                self.player.floor = landed_floor;
                self.events.push(ActionEvent::FloorChanged {
                    from_floor: prev_floor,
                    to_floor: landed_floor,
                    at: Tile3D::from_vec2(self.player.position, landed_floor),
                    pos: self.player_body.position,
                    via: FloorTransition::WindowDrop,
                });
            }
            let (dmg, rec) = Self::compute_fall_damage(drop_h);
            if rec > 0 {
                self.drop_recovery_ticks = self.drop_recovery_ticks.max(rec);
            }
            let mut applied_dmg = 0u8;
            if dmg > 0 && self.hurt_iframes == 0 && self.health > 0 {
                self.health = self.health.saturating_sub(dmg);
                self.hurt_iframes = PLAYER_HURT_IFRAMES;
                applied_dmg = dmg;
                self.events.push(ActionEvent::PlayerHurt {
                    at: self.player_body.position,
                    floor: self.player.floor,
                    damage: dmg,
                    remaining_hp: self.health,
                    from: self.player_body.position,
                });
            }
            if landed_impact_speed > 1.5 {
                self.events.push(ActionEvent::ActorLanded {
                    at: self.player_body.position,
                    floor: self.player.floor,
                    impact_speed: landed_impact_speed,
                    damage: applied_dmg,
                });
                if drop_h >= 0.90 {
                    self.noises.push(NoiseEvent {
                        origin: self.player.position,
                        origin_z: self.player_body.position.z,
                        floor: self.player.floor,
                        radius: 6.0,
                        priority: 2,
                        kind: NoiseKind::WindowDrop,
                    });
                }
            }
        }

        // 8. Walk-through window vault event & footsteps.
        if self.player.floor == 0 {
            let cur_t = Tile3D::from_vec2(self.player.position, 0);
            if self.stage.tile_kind(cur_t) == TileKind::BrokenWindow && prev_tile != cur_t {
                self.events.push(ActionEvent::WindowClimbed {
                    at: cur_t,
                    from: prev_pos,
                    to: self.player_body.position,
                });
                self.noises.push(NoiseEvent {
                    origin: cur_t.center(),
                    origin_z: self.player_body.position.z,
                    floor: 0,
                    radius: 2.8,
                    priority: 1,
                    kind: NoiseKind::WindowVault,
                });
            }
        }

        if self.player_body.motion.is_grounded()
            && mag > 0.12
            && (self.player_body.position.xy() - prev_pos.xy()).length_squared() > 1e-6
            && self.footstep_timer == 0
        {
            self.footstep_timer = if self.quiet_walking {
                24
            } else {
                FOOTSTEP_INTERVAL_TICKS
            };
            self.events.push(ActionEvent::Footstep {
                at: self.player_body.position,
                floor: self.player.floor,
                surface: ground_surface,
                quiet: self.quiet_walking,
            });
            if !self.quiet_walking {
                let radius = 2.2
                    + 1.2
                        * ((mag - PLAYER_QUIET_STICK_THRESHOLD)
                            / (1.0 - PLAYER_QUIET_STICK_THRESHOLD));
                self.noises.push(NoiseEvent {
                    origin: self.player.position,
                    origin_z: self.player_body.position.z,
                    floor: self.player.floor,
                    radius,
                    priority: 1,
                    kind: NoiseKind::Footstep,
                });
            }
        }

        // 9. 3D stair floor sync + switchback / stationary stair completion.
        if self.player_body.motion.is_grounded() {
            let new_fl = semantic_floor_for_body(&self.stage, self.player_body, self.player.floor);
            if new_fl != self.player.floor {
                let from_floor = self.player.floor;
                self.last_stair_travel_up = new_fl > from_floor;
                self.player.floor = new_fl;
                self.stair_cooldown = STAIR_COOLDOWN_TICKS;
                self.must_leave_stair = Some(Tile3D::from_vec2(self.player.position, new_fl).pos());
                self.events.push(ActionEvent::FloorChanged {
                    from_floor,
                    to_floor: new_fl,
                    at: Tile3D::from_vec2(self.player.position, new_fl),
                    pos: self.player_body.position,
                    via: FloorTransition::Stairs,
                });
            }
        }

        if let Some(stair_pos) = self.must_leave_stair {
            let stair_center = Vec2::new(stair_pos.x as f32, stair_pos.y as f32);
            if self.player.position.distance(stair_center) > 0.62 {
                self.must_leave_stair = None;
            }
        }
        if self.stair_cooldown == 0
            && self.must_leave_stair.is_none()
            && self.player_body.motion.is_grounded()
        {
            let p_tile = Tile3D::from_vec2(self.player.position, self.player.floor);
            if let Some((dest, landing)) = self.pick_stair_link(p_tile)
                && self.player.position.distance(p_tile.center()) <= 0.38
            {
                let landing_dir = Vec2::new(landing.0 as f32, landing.1 as f32);
                let step_xy = (self.player_body.position.xy() - prev_pos.xy()).length();
                let climbing_smoothly =
                    mag > 0.05 && dir.dot(landing_dir) > 0.35 && step_xy > 0.015;
                if !climbing_smoothly {
                    self.take_stairs(p_tile, dest, landing);
                }
            }
        }
    }

    fn pick_stair_link(&self, tile: Tile3D) -> Option<(Tile3D, (i8, i8))> {
        let (links, count) = self.stage.stair_links(tile);
        match count {
            0 => None,
            1 => links[0],
            _ => {
                let want_up = self.last_stair_travel_up;
                links
                    .into_iter()
                    .take(count)
                    .flatten()
                    .find(|(dest, _)| (dest.floor > tile.floor) == want_up)
                    .or(links[0])
            }
        }
    }

    fn take_stairs(&mut self, stair_tile: Tile3D, dest: Tile3D, landing: (i8, i8)) {
        let from_floor = self.player.floor;
        self.last_stair_travel_up = dest.floor > from_floor;
        self.player.floor = dest.floor;

        let gz = self.stage.terrain_at(dest.center()).map_or(0.0, |t| t.z);
        let probe_origin = dest
            .center()
            .extend(gz + dest.floor as f32 * FLOOR_H + 0.30);
        let authored_floor_z = self.blueprint_stair_landing(dest).and_then(|_| {
            self.stage
                .building_by_id(self.stage.building_id(dest))
                .map(|building| building.foundation_z + f32::from(dest.floor) * FLOOR_H)
        });
        let candidate_dirs = [
            landing,
            (-landing.0, -landing.1),
            (landing.1, -landing.0),
            (-landing.1, landing.0),
        ];
        let mut chosen_pos: Option<Vec2> = None;
        for d in candidate_dirs {
            if d == (0, 0) {
                continue;
            }
            let exit_tile = Tile3D::new(dest.x + d.0 as i32, dest.y + d.1 as i32, dest.floor);
            let d_vec = Vec2::new(d.0 as f32, d.1 as f32);
            let clear = if let Some(z) = authored_floor_z {
                // A horizontal ray from the stair center meets the descending
                // flight itself. Snap landings need clearance from solid volumes.
                let start = dest.center();
                let delta = d_vec * 0.78;
                let end = start + delta;
                let mut colliders = Vec::new();
                self.spatial().colliders_in(
                    Aabb3::from_points(
                        start.extend(z + 0.01),
                        end.extend(z + PLAYER_HEIGHT - 0.01),
                    )
                    .expand_vec(Vec3::new(PLAYER_RADIUS, PLAYER_RADIUS, 0.0)),
                    &mut colliders,
                );
                colliders.iter().all(|collider| {
                    crate::spatial::collision::sweep_circle_rect_2d(
                        start,
                        delta,
                        PLAYER_RADIUS,
                        collider.bounds.min.xy(),
                        collider.bounds.max.xy(),
                    )
                    .is_none()
                })
            } else {
                self.stage
                    .raycast_filtered(
                        Ray3::new(probe_origin, d_vec.normalized().extend(0.0)),
                        0.78,
                        QueryFilter::DEFAULT,
                    )
                    .is_none()
            };
            if self.stage.is_walkable(exit_tile, false) && clear {
                chosen_pos = Some(dest.center() + d_vec * 0.78);
                break;
            }
        }

        self.player.position = chosen_pos.unwrap_or_else(|| {
            Vec2::new(
                dest.x as f32 + landing.0 as f32 * 0.42,
                dest.y as f32 + landing.1 as f32 * 0.42,
            )
        });
        let (pz, psup, _) = resolve_actor_support(&self.stage, self.player.position, dest.floor);
        self.player_body =
            BodyState::from_pose_2d(self.player.position, self.player.facing, pz, psup);
        self.fall_peak_z = None;
        self.stair_cooldown = STAIR_COOLDOWN_TICKS;
        self.must_leave_stair = Some(stair_tile.pos());
        self.events.push(ActionEvent::FloorChanged {
            from_floor,
            to_floor: dest.floor,
            at: dest,
            pos: self.player_body.position,
            via: FloorTransition::Stairs,
        });
    }

    fn compute_interact_action(&self, vehicle_interactions: bool) -> Option<InteractAction> {
        if self.control_mode() == ControlMode::Motorcycle {
            if !vehicle_interactions {
                return None;
            }
            let id = self.dismount_candidate()?;
            let tile = self
                .vehicles
                .motorcycle(id)
                .map(|bike| Tile3D::from_vec2(bike.body.position.xy(), 0))?;
            return Some(InteractAction::Dismount(id, tile));
        }

        let p = self.player.position;
        let floor = self.player.floor;
        let facing = self.player.facing.normalized();

        // Read fixtures are immediately discoverable at the seeded spawn.
        for (id, anchor) in &self.readable_fixtures {
            let read_center = anchor.tile.center();
            let read_delta = read_center - p;
            let read_dist = read_delta.length();
            let front = match anchor.facing {
                PropFacing::North => Vec2::new(0.0, -1.0),
                PropFacing::East => Vec2::new(1.0, 0.0),
                PropFacing::South => Vec2::new(0.0, 1.0),
                PropFacing::West => Vec2::new(-1.0, 0.0),
            };
            let (support_z, _, _) = resolve_tile_support(&self.stage, anchor.tile);
            // The readable's own collider sits on its pivot, so sight lines aim
            // at its front face. The 0.04 m ray margin then ignores that face.
            let def = anchor.id.def();
            let depth = if matches!(anchor.facing, PropFacing::East | PropFacing::West) {
                def.collision_max.x
            } else {
                def.collision_max.y
            };
            let sight = anchor.pivot(support_z) + Vec3::new(front.x * depth, front.y * depth, 0.0);
            if anchor.tile.floor == floor
                && read_dist <= INTERACT_FEATURE_RADIUS
                && read_dist > 0.05
                && read_delta.normalized().dot(facing) >= 0.20
                && facing.dot(front) <= -0.20
                && self.has_line_of_sight_to_point(sight)
            {
                return Some(InteractAction::Read(*id, anchor.tile));
            }
        }

        // Priority 1: Unopened supply cache within reach.
        let mut best_cache: Option<(f32, CacheId, Tile3D)> = None;
        for c in self.stage.caches() {
            if c.opened || c.pos.floor != floor {
                continue;
            }
            let d = p.distance(c.pos.center());
            if d <= INTERACT_CACHE_RADIUS && best_cache.is_none_or(|(bd, _, _)| d < bd) {
                best_cache = Some((d, c.id, c.pos));
            }
        }
        if let Some((_, id, pos)) = best_cache {
            return Some(InteractAction::Search(id, pos));
        }

        if let Some(prop) = self.searchable_prop() {
            return Some(InteractAction::SearchProp(
                prop.anchor_uid,
                prop.anchor.tile,
            ));
        }

        let center_tile = Tile3D::from_vec2(p, floor);

        // Priority 2: Door (preferring facing half-space).
        let mut best_door: Option<(f32, Tile3D, bool)> = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let t = Tile3D::new(center_tile.x + dx, center_tile.y + dy, floor);
                if let TileKind::Door { open, .. } = self.stage.tile_kind(t) {
                    let d = p.distance(t.center());
                    if d <= INTERACT_FEATURE_RADIUS {
                        let dir_bias = if d > 0.05 {
                            ((t.center() - p) * (1.0 / d)).dot(facing) * 0.25
                        } else {
                            0.0
                        };
                        let score = d - dir_bias;
                        if best_door.is_none_or(|(bs, _, _)| score < bs) {
                            best_door = Some((score, t, !open));
                        }
                    }
                }
            }
        }
        if let Some((_, t, is_closed)) = best_door {
            return Some(InteractAction::ToggleDoor(t, is_closed));
        }

        // Priority 3: Upper-floor broken window / collapsed exterior edge -> Drop down.
        if floor >= 1 {
            let mut best_drop: Option<(f32, Tile3D, Tile3D)> = None;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let t = Tile3D::new(center_tile.x + dx, center_tile.y + dy, floor);
                    let d = p.distance(t.center());
                    if d <= INTERACT_FEATURE_RADIUS
                        && let Some(landing) = self.stage.window_drop_target(t)
                        && best_drop.is_none_or(|(bd, _, _)| d < bd)
                    {
                        best_drop = Some((d, t, landing));
                    }
                }
            }
            if let Some((_, win, landing)) = best_drop {
                return Some(InteractAction::DropWindow(win, landing));
            }
        }

        // Priority 4: Floor 0 broken window -> Vault across.
        if floor == 0 {
            let mut best_vault: Option<(f32, Tile3D, Tile3D)> = None;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let t = Tile3D::new(center_tile.x + dx, center_tile.y + dy, 0);
                    let d = p.distance(t.center());
                    if d <= 1.05
                        && self.stage.tile_kind(t) == TileKind::BrokenWindow
                        && let Some(dest) = self.stage.window_vault_target(center_tile, t)
                        && best_vault.is_none_or(|(bd, _, _)| d < bd)
                    {
                        best_vault = Some((d, t, dest));
                    }
                }
            }
            if let Some((_, win, dest)) = best_vault {
                return Some(InteractAction::ClimbWindow(win, dest));
            }
        }

        // Priority 5: Adjacent stairs.
        let mut best_stair: Option<(f32, InteractAction)> = None;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let t = Tile3D::new(center_tile.x + dx, center_tile.y + dy, floor);
                let d = p.distance(t.center());
                if d <= 1.05
                    && let Some((dest, landing)) = self.pick_stair_link(t)
                    && best_stair.is_none_or(|(bd, _)| d < bd)
                {
                    best_stair = Some((
                        d,
                        InteractAction::TakeStairs(t, dest, landing, dest.floor > floor),
                    ));
                }
            }
        }
        if let Some((_, action)) = best_stair {
            return Some(action);
        }

        // Vehicles are considered only after the on-foot prompts have no candidate.
        if vehicle_interactions && let Some(id) = self.right_bike_candidate() {
            let tile = self
                .vehicles
                .motorcycle(id)
                .map(|bike| Tile3D::from_vec2(bike.body.position.xy(), 0))?;
            return Some(InteractAction::RightBike(id, tile));
        }
        if vehicle_interactions && let Some(id) = self.mount_candidate() {
            let tile = self
                .vehicles
                .motorcycle(id)
                .map(|bike| Tile3D::from_vec2(bike.body.position.xy(), 0))?;
            return Some(InteractAction::Mount(id, tile));
        }

        None
    }

    fn searchable_prop(&self) -> Option<PropView> {
        let eye = self.player_eye_position();
        self.prop_views()
            .into_iter()
            .filter_map(|prop| {
                if prop.floor != self.player.floor
                    || prop.visibility == TileVisibility::Hidden
                    || prop.state == PropState::Opened
                    || prop.id.def().loot.is_none()
                {
                    return None;
                }
                let extent = prop.id.def().collision_max;
                let (width, depth) = match prop.facing {
                    PropFacing::North | PropFacing::South => (extent.x, extent.y),
                    PropFacing::East | PropFacing::West => (extent.y, extent.x),
                };
                let local = eye - prop.world_pivot;
                let face = prop.world_pivot
                    + Vec3::new(
                        local.x.clamp(-width, width),
                        local.y.clamp(-depth, depth),
                        local.z.clamp(0.0, extent.z),
                    );
                let dist = (face - eye).length();
                if dist > INTERACT_FEATURE_RADIUS || !self.has_line_of_sight_from(eye, face) {
                    return None;
                }
                Some((dist, prop))
            })
            .min_by(|(a, lhs), (b, rhs)| a.total_cmp(b).then(lhs.anchor_uid.cmp(&rhs.anchor_uid)))
            .map(|(_, prop)| prop)
    }

    fn resolve_interact(&mut self, vehicle_interactions: bool) {
        let Some(action) = self.compute_interact_action(vehicle_interactions) else {
            return;
        };
        match action {
            InteractAction::Search(id, at) => {
                if let Some(loot) = self.stage.take_cache(id) {
                    let (lz, _, _) = resolve_tile_support(&self.stage, at);
                    self.inventory.collect_loot(loot, &mut self.events);
                    self.events.push(ActionEvent::Looted {
                        at,
                        pos: at.center().extend(lz),
                        loot,
                    });
                }
            }
            InteractAction::Read(id, _) => {
                self.read_open = true;
                self.readable_id = id;
            }
            InteractAction::SearchProp(uid, at) => {
                let Some(prop) = self
                    .prop_views()
                    .into_iter()
                    .find(|prop| prop.anchor_uid == uid)
                else {
                    return;
                };
                if let Some(loot) = prop.id.def().loot
                    && self.prop_opened.insert(uid)
                {
                    self.inventory.collect_loot(loot, &mut self.events);
                    self.events.push(ActionEvent::Looted {
                        at,
                        pos: prop.world_pivot,
                        loot,
                    });
                }
            }
            InteractAction::ToggleDoor(tile, will_open) => {
                if !will_open {
                    // Refuse closing a door while player or any living monster overlaps its tile.
                    if self.actor_overlaps_tile(
                        self.player.position,
                        self.player.floor,
                        PLAYER_RADIUS,
                        tile,
                    ) {
                        return;
                    }
                    if self.monsters.iter().any(|m| {
                        m.is_alive() && self.actor_overlaps_tile(m.pos, m.floor, m.radius(), tile)
                    }) {
                        return;
                    }
                }
                if let Some(opened) = self.stage.toggle_door(tile) {
                    let (dz, _, _) = resolve_tile_support(&self.stage, tile);
                    self.events.push(ActionEvent::DoorToggled {
                        at: tile,
                        pos: tile.center().extend(dz),
                        opened,
                    });
                    self.noises.push(NoiseEvent {
                        origin: tile.center(),
                        origin_z: dz,
                        floor: tile.floor,
                        radius: 3.0,
                        priority: 2,
                        kind: NoiseKind::DoorToggle,
                    });
                }
            }
            InteractAction::DropWindow(_win, landing) => {
                let from_floor = self.player.floor;
                let prev_pos = self.player_body.position;
                let (landing_z, landing_sup, _) = resolve_tile_support(&self.stage, landing);
                self.player.floor = landing.floor;
                self.player.position = landing.center();
                self.drop_recovery_ticks = DROP_RECOVERY_TICKS;
                self.stair_cooldown = 12;

                if from_floor.saturating_sub(landing.floor) >= 2 {
                    self.player_body = BodyState::airborne(
                        landing
                            .center()
                            .extend((prev_pos.z - 0.35).max(landing_z + 0.1)),
                        Vec3::new(0.0, 0.0, -2.0),
                        self.player_body.heading,
                    );
                    self.fall_peak_z = Some(prev_pos.z);
                } else {
                    self.player_body = BodyState::from_pose_2d(
                        landing.center(),
                        self.player.facing,
                        landing_z,
                        landing_sup,
                    );
                    self.fall_peak_z = None;
                    self.events.push(ActionEvent::ActorLanded {
                        at: self.player_body.position,
                        floor: landing.floor,
                        impact_speed: 4.5,
                        damage: 0,
                    });
                }

                self.events.push(ActionEvent::FloorChanged {
                    from_floor,
                    to_floor: landing.floor,
                    at: landing,
                    pos: landing.center().extend(landing_z),
                    via: FloorTransition::WindowDrop,
                });
                self.noises.push(NoiseEvent {
                    origin: landing.center(),
                    origin_z: landing_z,
                    floor: landing.floor,
                    radius: 6.0,
                    priority: 2,
                    kind: NoiseKind::WindowDrop,
                });
            }
            InteractAction::ClimbWindow(win, dest) => {
                if self
                    .stage
                    .circle_walkable(dest.center(), dest.floor, PLAYER_RADIUS, true)
                {
                    let prev_pos = self.player_body.position;
                    let (dest_z, dest_sup, _) = resolve_tile_support(&self.stage, dest);
                    self.player.position = dest.center();
                    self.player_body = BodyState::from_pose_2d(
                        dest.center(),
                        self.player.facing,
                        dest_z,
                        dest_sup,
                    );
                    self.events.push(ActionEvent::WindowClimbed {
                        at: win,
                        from: prev_pos,
                        to: self.player_body.position,
                    });
                    self.noises.push(NoiseEvent {
                        origin: win.center(),
                        origin_z: dest_z,
                        floor: win.floor,
                        radius: 2.8,
                        priority: 1,
                        kind: NoiseKind::WindowVault,
                    });
                }
            }
            InteractAction::TakeStairs(stair_t, dest, landing, _) => {
                self.take_stairs(stair_t, dest, landing);
            }
            InteractAction::Mount(id, _) => self.try_mount(id),
            InteractAction::Dismount(id, _) => self.try_dismount(id),
            InteractAction::RightBike(id, _) => self.try_right_bike(id),
        }
    }

    fn actor_overlaps_tile(&self, center: Vec2, floor: Floor, radius: f32, tile: Tile3D) -> bool {
        if floor != tile.floor {
            return false;
        }
        let nx = center.x.clamp(tile.x as f32 - 0.5, tile.x as f32 + 0.5);
        let ny = center.y.clamp(tile.y as f32 - 0.5, tile.y as f32 + 0.5);
        let dx = center.x - nx;
        let dy = center.y - ny;
        dx * dx + dy * dy < radius * radius
    }

    fn process_collapses(&mut self) {
        let collapsed = self.stage.take_collapsed_tiles();
        if collapsed.is_empty() {
            return;
        }
        for c in collapsed {
            let center = c.center();
            let (c_z, _, _) = resolve_tile_support(&self.stage, c);
            let c_pos = center.extend(c_z);
            self.events
                .push(ActionEvent::StructuralCollapse { at: c, pos: c_pos });
            self.noises.push(NoiseEvent {
                origin: center,
                origin_z: c_z,
                floor: c.floor,
                radius: 16.0,
                priority: 4,
                kind: NoiseKind::Collapse,
            });

            // Player caught above or under collapsed overhang.
            if self.player.position.distance(center) <= 0.72 {
                let mut fell = false;
                if self.player.floor > c.floor
                    && !self
                        .stage
                        .has_floor(Tile3D::from_vec2(self.player.position, self.player.floor))
                {
                    let from_floor = self.player.floor;
                    self.player_body.motion = MotionState::Airborne;
                    self.fall_peak_z = Some(self.player_body.position.z);
                    self.player.floor = c.floor;
                    fell = true;
                    self.events.push(ActionEvent::FloorChanged {
                        from_floor,
                        to_floor: c.floor,
                        at: c,
                        pos: self.player_body.position,
                        via: FloorTransition::Collapse,
                    });
                }
                if (fell || self.player.floor == c.floor)
                    && self.hurt_iframes == 0
                    && self.health > 0
                {
                    self.health = self.health.saturating_sub(1);
                    self.hurt_iframes = PLAYER_HURT_IFRAMES;
                    self.events.push(ActionEvent::PlayerHurt {
                        at: self.player_body.position,
                        floor: self.player.floor,
                        damage: 1,
                        remaining_hp: self.health,
                        from: c_pos,
                    });
                }
            }

            // Monsters caught above or under collapse.
            for m in &mut self.monsters {
                if !m.is_alive() {
                    continue;
                }
                if m.pos.distance(center) <= 0.85 {
                    let mut fell = false;
                    if m.floor > c.floor && !self.stage.has_floor(Tile3D::from_vec2(m.pos, m.floor))
                    {
                        m.body.motion = MotionState::Airborne;
                        m.floor = c.floor;
                        m.invalidate_path();
                        fell = true;
                    }
                    if fell || m.floor == c.floor {
                        let _ = damage_monster(m, 2, Vec2::ZERO, 12, false, &mut self.events);
                    }
                }
            }
        }
    }

    // --- Public Query & Render API ---

    pub fn player_pose(&self) -> Pose {
        self.player
    }

    pub fn player_body(&self) -> BodyState {
        self.player_body
    }

    pub fn blasts(&self) -> Vec<BlastView> {
        self.active_blasts
            .iter()
            .map(|b| BlastView {
                id: b.id,
                kind: b.kind,
                position: b.position,
                floor: b.floor,
                radius: b.radius,
                progress: (b.age_ticks as f32 / b.total_ticks.max(1) as f32).clamp(0.0, 1.0),
            })
            .collect()
    }

    pub fn dynamic_flashes(&self) -> Vec<DynamicFlashView> {
        self.stage.dynamic_flashes()
    }

    pub fn player_render_state(&self) -> PlayerRenderState {
        let interact = self.compute_interact_action(true);
        let p_tile = Tile3D::from_vec2(self.player.position, self.player.floor);
        let range = self
            .inventory
            .equipped
            .map_or(1.45, |w| weapon_spec(w).range);
        let (muzzle_pos, aim_dir) = compute_player_aim_3d(
            &self.stage,
            self.player_body,
            self.player.floor,
            self.player.facing,
            range,
            &self.monsters,
        );
        let ground_normal = if self.player_body.motion.is_grounded() {
            support_normal_at(
                &self.stage,
                self.player_body.position.xy(),
                self.player_body.motion.support(),
                self.player.floor,
            )
        } else {
            Vec3::Z
        };
        PlayerRenderState {
            pose: self.player,
            elevation: self.player_body.position.z,
            body: self.player_body,
            ground_normal,
            aim_dir,
            muzzle_pos,
            equipped: self.inventory.equipped,
            ammo_in_equipped: self.inventory.equipped_ammo_total(),
            health: self.health,
            max_health: self.max_health,
            indoors: self.stage.is_indoor(p_tile),
            quiet_walking: self.quiet_walking,
            near_interact_tile: interact.map(InteractAction::tile),
            interact_prompt: interact.map_or(InteractPrompt::None, InteractAction::prompt),
            rocket_danger_close: self.inventory.equipped == Some(Weapon::RocketLauncher)
                && is_rocket_danger_close_3d(
                    &self.stage,
                    self.player,
                    self.player_body,
                    &self.monsters,
                ),
        }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn spatial(&self) -> crate::spatial::ActionSpatialView<'_> {
        crate::spatial::ActionSpatialView::new(self)
    }

    /// Upward support normal at an `(xy, floor)` point (`+z` up).
    /// Terrain and bridge decks tilt; floors, stairs, and empty space return `Vec3::Z`.
    pub fn ground_normal_at(&self, xy: Vec2, floor: Floor) -> Vec3 {
        support_normal_at(&self.stage, xy, None, floor)
    }

    pub fn toggle_door(&mut self, tile: Tile3D) -> Option<bool> {
        self.stage.toggle_door(tile)
    }

    pub fn damage_tile(&mut self, tile: Tile3D, kind: DamageKind) -> Option<TileDamageResult> {
        if let Some(anchor) = self.prop_anchor_at(tile) {
            let def = anchor.id.def();
            let hp = self
                .prop_hp
                .entry(anchor.anchor_uid)
                .or_insert(def.durability);
            if *hp == 0 {
                return None;
            }
            let dmg = block::damage_against(def.material, kind);
            if dmg == 0 {
                return None;
            }
            *hp = hp.saturating_sub(dmg);
            self.prop_revision = self.prop_revision.wrapping_add(1);
            return Some(TileDamageResult {
                damaged: true,
                destroyed: *hp == 0,
                material: def.material,
                old_kind: TileKind::Prop(crate::action::types::PropKind::Crate),
                new_kind: if *hp == 0 {
                    TileKind::Rubble
                } else {
                    TileKind::Prop(crate::action::types::PropKind::Crate)
                },
                sound_radius: if *hp == 0 { 7.5 } else { 3.5 },
                light_changed: false,
                collapsed_cells: 0,
            });
        }
        self.stage.damage_tile(tile, kind)
    }

    pub const fn prop_revision(&self) -> u64 {
        self.prop_revision
    }

    pub(crate) fn spatial_cell(&self, tile: Tile3D) -> Option<SpatialCell> {
        self.stage.spatial_cell(tile)
    }

    pub(crate) fn blueprint_stair_landing(&self, tile: Tile3D) -> Option<(i8, i8)> {
        match &self.stage {
            Stage::County(stage) => stage.blueprint_stair_landing(tile),
            Stage::Residential(_) => None,
        }
    }

    pub(crate) fn building_by_id(&self, id: BuildingId) -> Option<&BuildingInfo> {
        self.stage.building_by_id(id)
    }

    pub(crate) fn playable_bounds(&self) -> (Position, Position) {
        self.stage.playable_bounds()
    }

    pub(crate) fn floors_at(&self, pos: Position) -> Floor {
        self.stage.floors_at(pos)
    }

    pub fn block(&self) -> Option<&ResidentialBlock> {
        self.stage.residential()
    }

    pub fn house(&self) -> Option<House> {
        self.stage
            .residential()
            .map(ResidentialBlock::starter_house)
    }

    pub fn buildings(&self) -> &[BuildingInfo] {
        self.stage.buildings()
    }

    pub fn block_bounds(&self) -> Option<(Position, Position)> {
        let (min, max, _) = self.stage.bounds();
        Some((min, max))
    }

    pub fn tile_render_info(&self, pos: Position, floor: Floor) -> Option<TileRenderInfo> {
        self.stage.tile_render_info(pos, floor)
    }

    /// Read room discovery without sampling tile lighting or wall geometry.
    pub fn tile_visibility(&self, tile: Tile3D) -> Option<TileVisibility> {
        match &self.stage {
            Stage::Residential(block) => block.tile_visibility(tile),
            Stage::County(county) => county.tile_visibility(tile),
        }
    }

    pub const fn tick_count(&self) -> u64 {
        self.tick
    }

    pub fn light_sources(&self) -> Vec<LightSourceView> {
        self.stage.light_sources()
    }

    pub fn caches(&self) -> Vec<CacheView> {
        self.stage.cache_views(
            self.player.position,
            self.player.floor,
            INTERACT_CACHE_RADIUS,
        )
    }

    /// Snapshot of persistent prop anchors in the active stage. Visibility is read from
    /// the anchor tile, so a multi-cell prop remains seam-safe and room-shroud-safe.
    pub fn prop_views(&self) -> Vec<PropView> {
        self.prop_anchors
            .iter()
            .filter_map(|anchor| {
                if self
                    .prop_hp
                    .get(&anchor.anchor_uid)
                    .is_some_and(|hp| *hp == 0)
                {
                    return None;
                }
                let visibility = self.stage.tile_visibility(anchor.tile)?;
                let (z, _, _) = resolve_tile_support(&self.stage, anchor.tile);
                Some(PropView {
                    id: anchor.id,
                    anchor: *anchor,
                    world_pivot: anchor.pivot(z),
                    floor: anchor.tile.floor,
                    facing: anchor.facing,
                    state: if self.prop_opened.contains(&anchor.anchor_uid) {
                        PropState::Opened
                    } else {
                        PropState::Closed
                    },
                    anchor_uid: anchor.anchor_uid,
                    visibility,
                })
            })
            .collect()
    }

    /// Ingest persistent blueprint props from the county stage. Re-running this
    /// after streaming is idempotent, so revisiting a parcel cannot duplicate a
    /// prop instance.
    fn sync_county_blueprint_props(&mut self) {
        let anchors = match &self.stage {
            Stage::County(county) => {
                let all = county.blueprint_prop_anchors();
                let start = self.blueprint_prop_cursor.min(all.len());
                if start == all.len() {
                    return;
                }
                all[start..].to_vec()
            }
            Stage::Residential(_) => return,
        };
        for anchor in anchors {
            if self
                .prop_anchors
                .iter()
                .any(|existing| existing.anchor_uid == anchor.anchor_uid)
            {
                continue;
            }
            if !self.add_prop_fixture(anchor) {
                // The anchor's chunk window is not fully materialized yet.
                // Retry the same tail after the next streaming update.
                return;
            }
        }
        if let Stage::County(county) = &self.stage {
            self.blueprint_prop_cursor = county.blueprint_prop_anchors().len();
        }
    }

    /// Ingest readable signs/notes authored inside blueprint structures. This
    /// runs after the prop sync so the readable's wall/floor anchor already
    /// exists in the shared prop registry.
    fn sync_county_blueprint_readables(&mut self) {
        let pending = match &self.stage {
            Stage::County(county) => {
                let all = county.blueprint_readables();
                let start = self.blueprint_readable_cursor.min(all.len());
                if start == all.len() {
                    return;
                }
                all[start..].to_vec()
            }
            Stage::Residential(_) => return,
        };
        for (id, anchor) in pending {
            if !self
                .prop_anchors
                .iter()
                .any(|existing| existing.anchor_uid == anchor.anchor_uid)
            {
                // The anchor's chunk window is not fully materialized yet.
                return;
            }
            if !self
                .readable_fixtures
                .iter()
                .any(|(_, existing)| existing.anchor_uid == anchor.anchor_uid)
            {
                self.readable_fixtures.push((id, anchor));
            }
            self.blueprint_readable_cursor += 1;
        }
    }

    /// Persistent prop anchors in stable placement order.
    pub fn prop_anchors(&self) -> &[PropAnchor] {
        &self.prop_anchors
    }

    /// Adds a deliberately controlled prop fixture to the snapshot registry. This is
    /// intended for gallery/reference scenes; procedural generation does not call it.
    pub fn add_prop_fixture(&mut self, anchor: PropAnchor) -> bool {
        if self
            .prop_anchors
            .iter()
            .any(|a| a.anchor_uid == anchor.anchor_uid)
        {
            return false;
        }
        let def = anchor.id.def();
        let rotated = matches!(anchor.facing, PropFacing::East | PropFacing::West);
        let expected = if rotated {
            PropSpan::new(def.span.height, def.span.width)
        } else {
            def.span
        };
        if anchor.span != expected {
            return false;
        }
        if anchor
            .span
            .covered_tiles(anchor.tile)
            .any(|t| self.prop_tile_index.contains_key(&t))
        {
            let same_layer = anchor.span.covered_tiles(anchor.tile).any(|t| {
                self.prop_anchor_at(t)
                    .is_some_and(|a| a.attachment == anchor.attachment)
            });
            if same_layer {
                return false;
            }
        }
        let covered = anchor.span.covered_tiles(anchor.tile).collect::<Vec<_>>();
        if anchor.attachment == CellAttachment::WallMount {
            if !covered.iter().any(|tile| {
                matches!(
                    self.stage.tile_kind(*tile),
                    TileKind::ExteriorWall
                        | TileKind::InteriorWall
                        | TileKind::Window
                        | TileKind::Pillar
                )
            }) {
                return false;
            }
        } else if covered.iter().any(|t| {
            !self.stage.is_walkable(*t, false)
                || matches!(
                    self.stage.tile_kind(*t),
                    TileKind::Door { .. }
                        | TileKind::StairsUp
                        | TileKind::StairsDown
                        | TileKind::StairsBoth
                )
        }) {
            return false;
        }
        self.prop_anchors.push(anchor);
        for tile in anchor.span.covered_tiles(anchor.tile) {
            self.prop_tile_index.insert(tile, anchor.anchor_uid);
        }
        self.prop_revision = self.prop_revision.wrapping_add(1);
        true
    }

    /// Returns the sparse anchor owning a covered tile, including anchors that cross
    /// chunk boundaries. The registry intentionally contains fixtures only.
    pub fn prop_anchor_at(&self, tile: Tile3D) -> Option<PropAnchor> {
        let uid = *self.prop_tile_index.get(&tile)?;
        self.prop_anchors
            .iter()
            .copied()
            .find(|a| a.anchor_uid == uid)
    }

    pub(crate) fn prop_colliders_in(&self, bounds: Aabb3, out: &mut Vec<Collider>) {
        for anchor in &self.prop_anchors {
            if self
                .prop_hp
                .get(&anchor.anchor_uid)
                .is_some_and(|hp| *hp == 0)
            {
                continue;
            }
            let def = anchor.id.def();
            let (sx, sy) = if matches!(anchor.facing, PropFacing::East | PropFacing::West) {
                (def.collision_max.y, def.collision_max.x)
            } else {
                (def.collision_max.x, def.collision_max.y)
            };
            let pivot = anchor.pivot(resolve_tile_support(&self.stage, anchor.tile).0);
            let c = Collider {
                bounds: Aabb3::new(
                    Vec3::new(pivot.x - sx, pivot.y - sy, pivot.z + def.collision_min.z),
                    Vec3::new(pivot.x + sx, pivot.y + sy, pivot.z + def.collision_max.z),
                ),
                kind: crate::spatial::ColliderKind::Prop {
                    tile: anchor.tile,
                    prop: None,
                },
                material: def.material,
            };
            if c.bounds.intersects(bounds) {
                out.push(c);
            }
        }
    }

    pub fn has_cache_at(&self, p: Position) -> bool {
        self.stage.caches().iter().any(|c| {
            !c.opened && c.pos.x == p.x && c.pos.y == p.y && c.pos.floor == self.player.floor
        })
    }

    pub fn near_loot(&self) -> bool {
        matches!(
            self.compute_interact_action(true),
            Some(InteractAction::Search(..) | InteractAction::SearchProp(..))
        )
    }

    /// Starter pistol cache position inside House 1 (or starter Residential house in County mode).
    pub fn cache_position(&self) -> Position {
        if self.is_county() {
            return self
                .stage
                .caches()
                .iter()
                .find(|c| c.loot.weapon == Some(Weapon::Pistol))
                .or_else(|| self.stage.caches().get(1))
                .map_or_else(|| self.player_tile().pos(), |c| c.pos.pos());
        }
        self.stage
            .caches()
            .get(1)
            .map_or_else(|| self.player_tile().pos(), |c| c.pos.pos())
    }

    pub fn monster_views(&self) -> Vec<MonsterView> {
        self.monsters
            .iter()
            .filter(|m| m.is_alive() || m.is_swelling())
            .map(|m| m.to_view(&self.stage))
            .collect()
    }

    pub fn zombie_poses(&self) -> Vec<Pose> {
        self.monsters
            .iter()
            .filter(|m| m.is_alive())
            .map(|m| Pose {
                position: m.pos,
                facing: m.facing,
                floor: m.floor,
            })
            .collect()
    }

    pub fn projectiles(&self) -> Vec<ProjectileView> {
        let eye = self.player_eye_position();
        self.projectiles
            .iter()
            .map(|p| p.to_view(self.has_line_of_sight_from(eye, p.position)))
            .collect()
    }

    /// Authoritative player eye point (`+z` up), used by 3D visibility policies.
    #[must_use]
    pub fn player_eye_position(&self) -> Vec3 {
        self.player_body
            .position
            .with_z(self.player_body.position.z + PLAYER_EYE_HEIGHT)
    }

    /// Geometric 3D line of sight from the player's eye to `point`.
    ///
    /// This is intentionally independent of floor-tile fog-of-war state. Used to keep
    /// transient gameplay actors (projectiles) secret only when solid geometry blocks
    /// them, rather than whenever the tile underneath them is not currently visible.
    #[must_use]
    pub fn has_line_of_sight_to_point(&self, point: Vec3) -> bool {
        self.has_line_of_sight_from(self.player_eye_position(), point)
    }

    fn has_line_of_sight_from(&self, from: Vec3, to: Vec3) -> bool {
        let Some((ray, dist)) = Ray3::from_points(from, to) else {
            return true;
        };
        if dist <= 0.08 {
            return true;
        }
        self.spatial()
            .raycast_filtered(ray, dist - 0.04, QueryFilter::LINE_OF_SIGHT)
            .is_none()
    }

    pub fn dusk_level(&self) -> f32 {
        DUSK_LEVEL
    }

    pub fn events(&self) -> &[ActionEvent] {
        &self.events
    }

    pub fn outcome(&self) -> ActionOutcome {
        self.outcome
    }

    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    pub fn hud(&self) -> ActionHud {
        let interact = self.compute_interact_action(true);
        let interact_prompt = interact.map_or(InteractPrompt::None, InteractAction::prompt);
        let equipped = self.inventory.equipped;
        let (active_mag, active_mag_cap, active_reserve) = match equipped {
            Some(w) => {
                let s = w.slot();
                (
                    self.inventory.mag[s],
                    weapon_spec(w).mag_capacity,
                    self.inventory.reserve[s],
                )
            }
            None => (0, 0, 0),
        };
        let caches_total = self.stage.caches().len().min(u8::MAX as usize) as u8;
        let caches_found = self
            .stage
            .caches()
            .iter()
            .filter(|c| c.opened)
            .count()
            .min(u8::MAX as usize) as u8;
        let objective_kind = match self.outcome {
            ActionOutcome::Won => ObjectiveKind::Won,
            ActionOutcome::Dead => ObjectiveKind::Dead,
            ActionOutcome::Playing => {
                if self.inventory.owned_mask == 0 {
                    ObjectiveKind::Scavenge
                } else if self.is_county() {
                    ObjectiveKind::Explore
                } else {
                    ObjectiveKind::ClearBlock
                }
            }
        };
        let p_tile = Tile3D::from_vec2(self.player.position, self.player.floor);

        ActionHud {
            tick: self.tick,
            health: self.health,
            max_health: self.max_health,
            floor: self.player.floor,
            ammo: self.inventory.equipped_ammo_total(),
            armed: self.inventory.owned_mask != 0,
            equipped,
            owned_weapons_mask: self.inventory.owned_mask,
            active_mag,
            active_mag_cap,
            active_reserve,
            pistol_mag: self.inventory.mag[Weapon::Pistol.slot()],
            pistol_reserve: self.inventory.reserve[Weapon::Pistol.slot()],
            shotgun_mag: self.inventory.mag[Weapon::Shotgun.slot()],
            shotgun_reserve: self.inventory.reserve[Weapon::Shotgun.slot()],
            rocket_mag: self.inventory.mag[Weapon::RocketLauncher.slot()],
            rocket_reserve: self.inventory.reserve[Weapon::RocketLauncher.slot()],
            reloading: self.inventory.reload.is_some(),
            can_reload: self.inventory.can_reload(),
            reload_progress: self.inventory.reload_progress_pct(),
            medkits: self.inventory.medkits,
            can_heal: self.inventory.can_heal(self.health, self.max_health),
            zombie_count: self.monsters.iter().filter(|m| m.is_alive()).count(),
            zombies_total: self.zombies_total,
            caches_found,
            caches_total,
            interact_prompt,
            read_open: self.read_open,
            read_id: if self.read_open {
                self.readable_id as u8
            } else {
                0
            },
            objective_kind,
            rocket_danger_close: equipped == Some(Weapon::RocketLauncher)
                && is_rocket_danger_close_3d(
                    &self.stage,
                    self.player,
                    self.player_body,
                    &self.monsters,
                ),
            indoors: self.stage.is_indoor(p_tile),
            outcome: self.outcome,
            player_pos: p_tile.pos(),
            district: self.current_district(),
            mounted: self.vehicles.mounted_hud(),
        }
    }

    /// Spawn additional varied infected on walkable street/yard tiles for performance benchmarking.
    pub fn spawn_stress_monsters(&mut self, target_total: usize) {
        let (min, max, _) = self.stage.bounds();
        let kinds = [
            MonsterKind::Shambler,
            MonsterKind::Brisk,
            MonsterKind::Bloater,
            MonsterKind::Shooter,
        ];
        let mut cursor = 0u64;
        while self.monsters.len() < target_total && cursor < 20_000 {
            cursor += 1;
            let h = mix64(self.seed ^ cursor.wrapping_mul(0x517c_c1b7_2722_0a95));
            let x = min.x + (h % (max.x - min.x + 1) as u64) as i32;
            let y = min.y + ((h >> 16) % (max.y - min.y + 1) as u64) as i32;
            let tile = Tile3D::new(x, y, 0);
            if !self.stage.is_walkable(tile, false) {
                continue;
            }
            if tile.center().distance(self.player.position) < 2.2 {
                continue;
            }
            let kind = kinds[(cursor as usize) % kinds.len()];
            let id = match &mut self.stage {
                Stage::Residential(_) => {
                    let id = MonsterId(self.next_monster_id);
                    self.next_monster_id += 1;
                    id
                }
                Stage::County(county) => county.alloc_monster_id(),
            };
            self.monsters.push(Monster::new_in_world(
                id,
                kind,
                (cursor % 3) as u8,
                tile,
                &self.stage,
            ));
        }
        self.zombies_total = self.monsters.len().max(self.zombies_total);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WORLD_SIZE;

    #[test]
    fn disabled_motorcycle_skid_remains_in_registry_across_streaming_revisit() {
        let mut game = ActionGame::county(42);
        let (id, spawn, _) = game
            .vehicles
            .iter()
            .find_map(|(id, state)| {
                state.motorcycle().map(|bike| {
                    (
                        id,
                        Tile3D::from_vec2(bike.body.position.xy(), 0),
                        bike.condition,
                    )
                })
            })
            .expect("seeded bike");
        let bike = game.vehicles.motorcycle_mut(id).unwrap();
        bike.condition = 0;
        bike.phase = MotorcyclePhase::Skidding;
        bike.body.velocity = Vec3::new(6.0, 0.0, 0.0);
        bike.recovery_ticks = 0;
        game.step(ActionInput::default());
        let moved = game.vehicles.motorcycle(id).unwrap().body.position;
        assert!(
            moved.xy().distance(spawn.center()) > 0.02,
            "disabled bike should finish its skid"
        );
        assert_eq!(game.vehicles.motorcycle(id).unwrap().condition, 0);
        assert!(
            game.teleport(Tile3D::new(spawn.x + 100, spawn.y, 0))
                .is_some()
        );
        assert!(game.vehicle_views().all(|(view_id, _)| view_id != id));
        assert!(game.teleport(spawn).is_some());
        let restored = game
            .vehicle_views()
            .find(|(view_id, _)| *view_id == id)
            .expect("bike restored by live position")
            .1;
        assert!(
            matches!(restored, VehicleView::Motorcycle(bike) if bike.condition == 0 && bike.phase == MotorcyclePhase::Skidding)
        );
    }

    #[test]
    fn an_operative_skidding_motorcycle_can_be_remounted() {
        let mut game = ActionGame::county(42);
        let (id, spawn) = game
            .vehicles
            .iter()
            .find_map(|(id, state)| {
                state
                    .motorcycle()
                    .map(|bike| (id, Tile3D::from_vec2(bike.body.position.xy(), 0)))
            })
            .expect("seeded bike");
        assert!(game.teleport(spawn).is_some());
        let bike = game.vehicles.motorcycle_mut(id).unwrap();
        bike.body.velocity = bike.body.facing_xy().extend(0.0) * 3.0;
        bike.phase = MotorcyclePhase::Skidding;
        bike.recovery_ticks = 0;

        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });

        assert_eq!(game.control_mode(), ControlMode::Motorcycle);
        assert_eq!(game.mounted_vehicle(), Some(id));
        let bike = game.vehicles.motorcycle(id).unwrap();
        assert_eq!(bike.phase, MotorcyclePhase::Rolling);
        assert!(
            bike.speed() > 2.5,
            "remount should keep the skid's momentum: {}",
            bike.speed()
        );
    }

    #[test]
    fn mounted_bike_disables_and_releases_rider_when_contact_uses_last_condition() {
        let mut game = ActionGame::county(42);
        let (id, state) = game.vehicles.iter().next().expect("seeded bike");
        let bike = state.motorcycle().unwrap();
        let bike_pos = bike.body.position;
        let forward = bike.body.facing_xy();
        let target_pos = bike_pos + forward.extend(0.0) * 0.25 + Vec3::new(0.0, 0.0, 0.8);
        let monster = game
            .monsters
            .iter_mut()
            .find(|m| m.alive && m.floor == 0)
            .expect("active infected");
        monster.body.position = target_pos - Vec3::new(0.0, 0.0, 0.8);
        monster.pos = target_pos.xy();
        let bike = game.vehicles.motorcycle_mut(id).unwrap();
        bike.condition = 1;
        bike.body.velocity = forward.extend(0.0) * 2.0;
        bike.phase = MotorcyclePhase::Rolling;
        game.vehicles.set_mounted(Some(id));
        game.sync_mounted_pose(id);

        game.step(ActionInput {
            ride: Some(RideInput::Motorcycle(MotorcycleInput::default())),
            ..ActionInput::default()
        });

        assert_eq!(game.vehicles.motorcycle(id).unwrap().condition, 0);
        assert_eq!(game.control_mode(), ControlMode::OnFoot);
        assert!(
            game.events()
                .iter()
                .any(|e| matches!(e, ActionEvent::VehicleCrash { id: got, .. } if *got == id))
        );
    }

    #[test]
    fn riderless_skid_hits_and_damages_an_infected() {
        let mut game = ActionGame::county(42);
        let (id, state) = game.vehicles.iter().next().expect("seeded bike");
        let pos = state.body().position;
        let forward = state.body().facing_xy();
        let target_pos = pos + forward.extend(0.0) * 0.25 + Vec3::new(0.0, 0.0, 0.8);
        let monster = game
            .monsters
            .iter_mut()
            .find(|m| m.alive && m.floor == 0)
            .expect("active infected");
        monster.body.position = target_pos - Vec3::new(0.0, 0.0, 0.8);
        monster.pos = target_pos.xy();
        let monster_id = monster.id;
        let monster_hp = monster.hp;
        let bike = game.vehicles.motorcycle_mut(id).unwrap();
        bike.phase = MotorcyclePhase::Skidding;
        bike.body.velocity = forward.extend(0.0) * 2.0;

        game.step(ActionInput::default());

        assert!(
            game.monsters
                .iter()
                .find(|m| m.id == monster_id)
                .unwrap()
                .hp
                < monster_hp
        );
        assert!(game.events().iter().any(|e| matches!(e, ActionEvent::VehicleImpact { id: got, target: Some(target), .. } if *got == id && *target == monster_id)));
    }

    #[test]
    fn projectile_views_use_3d_los_not_floor_visibility() {
        let mut game = ActionGame::residential(42);
        let house = game.house().unwrap();
        let south_y = house.y + house.height - 1;
        let wall_x = (house.x..house.x + house.width)
            .find(|x| game.stage.tile_kind(Tile3D::new(*x, south_y, 0)) == TileKind::ExteriorWall)
            .expect("starter house should have a solid south wall");
        let outside = Vec2::new(wall_x as f32, south_y as f32 + 1.0);
        let (z, support, _) = resolve_actor_support(&game.stage, outside, 0);
        game.player.position = outside;
        game.player.floor = 0;
        game.player_body = BodyState::from_pose_2d(outside, Vec2::new(0.0, -1.0), z, support);

        let eye = game.player_eye_position();
        let behind_wall = Vec3::new(wall_x as f32, south_y as f32 - 0.6, eye.z);
        assert!(
            !game.has_line_of_sight_to_point(behind_wall),
            "a solid exterior wall must occlude the point behind it"
        );
        let open_air = eye.with_z(eye.z + 5.0);
        assert!(
            game.has_line_of_sight_to_point(open_air),
            "open air above the player must be visible"
        );

        let base = Projectile {
            id: ProjectileId(1),
            kind: ProjectileKind::ShooterBolt,
            owner_is_player: false,
            position: behind_wall,
            velocity: Vec3::new(1.0, 0.0, 0.0),
            floor: 0,
            radius: 0.1,
            remaining_range: 10.0,
        };
        game.projectiles.clear();
        game.projectiles.push(base);
        game.projectiles.push(Projectile {
            id: ProjectileId(2),
            position: open_air,
            ..base
        });

        let views = game.projectiles();
        assert!(
            !views[0].visible_to_player,
            "projectile behind a solid wall stays secret"
        );
        assert!(
            views[1].visible_to_player,
            "projectile in open 3D space is visible regardless of its floor tile"
        );
    }

    #[test]
    fn brisk_analog_movement_respects_quiet_walk_and_walls() {
        let mut game = ActionGame::residential(42);
        let start = game.player_pose().position;
        game.step(ActionInput {
            movement: Vec2::new(1.0, 1.0),
            ..ActionInput::default()
        });
        let moved = game.player_pose().position;
        assert!(moved.x > start.x && moved.y > start.y);
        assert!((moved.distance(start) - PLAYER_MAX_SPEED / 60.0).abs() < 0.001);
        assert!(!game.player_render_state().quiet_walking);

        // Slow analog tilt enters quiet walk (zero footstep noise).
        game.step(ActionInput {
            movement: Vec2::new(0.4, 0.0),
            ..ActionInput::default()
        });
        assert!(game.player_render_state().quiet_walking);

        // Moving north into Starter House exterior wall stops cleanly.
        let h = game.house().unwrap();
        game.player.position = Vec2::new((h.x + 1) as f32, (h.y + h.height) as f32);
        for _ in 0..30 {
            game.step(ActionInput {
                movement: Vec2::new(0.0, -1.0),
                ..ActionInput::default()
            });
        }
        assert!(game.player.position.y > (h.y + h.height - 1) as f32 + PLAYER_RADIUS);
    }

    #[test]
    fn starter_axe_shatters_window_and_pistol_kills_infected() {
        let mut game = ActionGame::residential(42);
        assert_eq!(game.hud().health, PLAYER_MAX_HEALTH);
        assert_eq!(game.hud().zombie_count, 12);
        assert!(!game.hud().armed);

        // Collect starter Axe on front stoop (cache 0).
        let axe_pos = game.stage.caches()[0].pos;
        game.player.position = axe_pos.center();
        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        assert_eq!(game.hud().equipped, Some(Weapon::Axe));
        assert!(game.hud().armed);

        // Aim north at Starter House south window and shatter it in 1 axe swing!
        let h = game.house().unwrap();
        let win_tile = Tile3D::new(h.x + 2, h.y + h.height - 1, 0);
        assert_eq!(game.stage.tile_kind(win_tile), TileKind::Window);
        game.player.position = Vec2::new(win_tile.x as f32, win_tile.y as f32 + 0.9);
        game.player.facing = Vec2::new(0.0, -1.0);
        game.step(ActionInput {
            fire: true,
            ..ActionInput::default()
        });
        assert_eq!(game.stage.tile_kind(win_tile), TileKind::BrokenWindow);

        // Collect Pistol cache inside Starter House, switch to Pistol, kill last monster -> Won.
        let pistol_pos = game.stage.caches()[1].pos;
        game.player.position = pistol_pos.center();
        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        game.step(ActionInput {
            select_weapon: Some(Weapon::Pistol),
            ..ActionInput::default()
        });
        assert_eq!(game.hud().equipped, Some(Weapon::Pistol));
        assert_eq!(game.hud().active_mag, 8);
        assert_eq!(game.hud().active_reserve, 4);

        game.monsters.truncate(1);
        game.monsters[0] = Monster::new(
            MonsterId(99),
            MonsterKind::Shambler,
            0,
            Tile3D::new(pistol_pos.x + 2, pistol_pos.y, 0),
        );
        game.player.facing = Vec2::new(1.0, 0.0);
        // Clear fire cooldown before firing.
        for _ in 0..30 {
            game.step(ActionInput::default());
        }
        game.monsters[0].pos = game.player.position + Vec2::new(2.0, 0.0);
        game.step(ActionInput {
            fire: true,
            ..ActionInput::default()
        });
        assert_eq!(game.hud().zombie_count, 0);
        assert_eq!(game.outcome(), ActionOutcome::Won);
        assert!(game.events().contains(&ActionEvent::Won));
    }

    #[test]
    fn readable_spawn_fixture_requires_facing_and_opens_authoritative_text() {
        let mut game = ActionGame::residential(42);
        assert_eq!(game.hud().interact_prompt, InteractPrompt::Read);
        let sign = game.readable_fixtures[0].1.tile;
        game.player.position = Vec2::new(sign.x as f32, sign.y as f32 + 0.72);
        game.player.facing = Vec2::new(0.0, -1.0);
        assert_eq!(game.hud().interact_prompt, InteractPrompt::Read);
        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        let hud = game.hud();
        assert!(hud.read_open);
        assert_eq!(hud.read_id, ReadableId::BirchDirectory as u8);
        game.player.facing = Vec2::new(0.0, 1.0);
        assert_ne!(game.hud().interact_prompt, InteractPrompt::Read);
    }

    #[test]
    fn craftsman_fridge_searches_without_chunk_rebuild_and_persists() {
        let mut game = ActionGame::county(42);
        let structure = game
            .world
            .fixture_plan()
            .expect("Craftsman fixture")
            .structures[0]
            .clone();
        let fridge = structure
            .props_world()
            .into_iter()
            .find(|prop| prop.id == PropId::Fridge)
            .expect("Craftsman fridge");
        let approach = match fridge.facing {
            PropFacing::North => Tile3D::new(fridge.tile.x, fridge.tile.y - 1, 0),
            PropFacing::South => Tile3D::new(
                fridge.tile.x,
                fridge.tile.y + i32::from(fridge.span.height),
                0,
            ),
            PropFacing::East => Tile3D::new(
                fridge.tile.x + i32::from(fridge.span.width),
                fridge.tile.y,
                0,
            ),
            PropFacing::West => Tile3D::new(fridge.tile.x - 1, fridge.tile.y, 0),
        };
        game.teleport(approach).expect("fridge approach streams");
        // Stand just outside the fridge collider, as a walking player would.
        let pivot = fridge.to_anchor().pivot(0.0);
        let front = match fridge.facing {
            PropFacing::North => Vec2::new(0.0, -1.0),
            PropFacing::East => Vec2::new(1.0, 0.0),
            PropFacing::South => Vec2::new(0.0, 1.0),
            PropFacing::West => Vec2::new(-1.0, 0.0),
        };
        game.player.position = Vec2::new(pivot.x + front.x * 0.9, pivot.y + front.y * 0.9);
        let terrain_before = game.stage.terrain_version();
        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        assert!(
            game.prop_opened.contains(&fridge.anchor_uid),
            "the supported fridge container should open"
        );
        assert!(
            game.prop_views().iter().any(|view| {
                view.anchor_uid == fridge.anchor_uid && view.state == PropState::Opened
            }),
            "the opened prototype is exposed to the renderer"
        );
        assert_eq!(
            game.stage.terrain_version(),
            terrain_before,
            "container state must not rebuild static chunks"
        );

        let wake = game.world.wake_position();
        game.teleport(Tile3D::from_pos(wake, 0))
            .expect("teleport away from the Craftsman");
        game.teleport(approach)
            .expect("teleport back to the Craftsman");
        assert_eq!(
            game.prop_anchors()
                .iter()
                .filter(|anchor| anchor.anchor_uid == fridge.anchor_uid)
                .count(),
            1,
            "revisiting must not duplicate the fridge anchor"
        );
        assert!(game.prop_opened.contains(&fridge.anchor_uid));
    }

    #[test]
    fn craftsman_note_is_readable_and_wall_breach_persists() {
        let mut game = ActionGame::county(42);
        let (readable, anchor) = *game
            .readable_fixtures
            .iter()
            .find(|(id, _)| *id == ReadableId::CraftsmanNote)
            .expect("Craftsman note is synced from the blueprint");
        let front = match anchor.facing {
            PropFacing::North => Vec2::new(0.0, -1.0),
            PropFacing::East => Vec2::new(1.0, 0.0),
            PropFacing::South => Vec2::new(0.0, 1.0),
            PropFacing::West => Vec2::new(-1.0, 0.0),
        };
        let approach = Tile3D::new(
            anchor.tile.x + front.x as i32,
            anchor.tile.y + front.y as i32,
            0,
        );
        game.teleport(approach).expect("note approach streams");
        game.player.facing = -front;
        assert_eq!(game.hud().interact_prompt, InteractPrompt::Read);
        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        let hud = game.hud();
        assert!(hud.read_open);
        assert_eq!(hud.read_id, readable as u8);

        let structure = game.world.fixture_plan().unwrap().structures[0].clone();
        let (wall_local, _) = structure
            .blueprint
            .cells
            .iter()
            .find(|(local, cell)| {
                local.floor == 0 && cell.kind == crate::BlueprintKind::ExteriorWall
            })
            .expect("Craftsman exterior wall");
        let wall = Tile3D::from_pos(structure.transform.local_to_world(*wall_local), 0);
        let inside = Tile3D::from_pos(
            structure.transform.local_to_world(crate::LocalCell::new(
                wall_local.u,
                wall_local.v + 1,
                0,
            )),
            0,
        );
        game.teleport(inside).expect("interior approach streams");
        let result = game
            .damage_tile(wall, DamageKind::RocketBlast)
            .expect("wall takes rocket damage");
        assert!(result.destroyed);
        assert_eq!(game.stage.tile_kind(wall), TileKind::Rubble);
        let wake = game.world.wake_position();
        game.teleport(Tile3D::from_pos(wake, 0))
            .expect("teleport away from the breach");
        game.teleport(inside).expect("teleport back to the breach");
        assert_eq!(
            game.stage.tile_kind(wall),
            TileKind::Rubble,
            "breached walls stay breached on revisit"
        );
    }

    #[test]
    fn reference_street_player_can_walk_from_spawn_to_all_three_entries() {
        use std::collections::{HashMap, VecDeque};

        for seed in [42, u64::MAX] {
            let world = World::new(seed);
            let plans: Vec<_> = world
                .fixture_plans()
                .into_iter()
                .filter(|plan| {
                    let family = plan.structures[0].archetype;
                    ["craftsman_", "ranch_", "colonial_"]
                        .iter()
                        .any(|prefix| family.starts_with(prefix))
                })
                .collect();
            assert_eq!(plans.len(), 3);
            let spawn = ActionGame::county_with_world(world.clone())
                .player_tile()
                .pos();
            let mut previous = HashMap::from([(spawn, spawn)]);
            let mut queue = VecDeque::from([spawn]);
            while let Some(pos) = queue.pop_front() {
                for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
                    let next = Position::new(pos.x + dx, pos.y + dy);
                    if let std::collections::hash_map::Entry::Vacant(entry) = previous.entry(next)
                        && world
                            .tile_at(next)
                            .is_some_and(|tile| tile.ground.is_road() && tile.walkable())
                    {
                        entry.insert(pos);
                        queue.push_back(next);
                    }
                }
            }
            for plan in plans {
                let access = plan.frontage.access;
                let (dx, dy) = plan.frontage.side.inward();
                let mut pos = Position::new(access.x - dx, access.y - dy);
                let mut route = Vec::new();
                while pos != spawn {
                    route.push(pos);
                    pos = *previous.get(&pos).expect("street connects to spawn");
                }
                route.reverse();
                route.push(access);
                let name = plan.structures[0].name;
                let door = plan.structures[0].front_door;
                for &(mut start, end) in &plan.outdoor.paths {
                    assert_eq!(route.last(), Some(&start));
                    assert!(start.x == end.x || start.y == end.y);
                    while start != end {
                        start.x += (end.x - start.x).signum();
                        start.y += (end.y - start.y).signum();
                        route.push(start);
                    }
                }
                assert_eq!(route.last(), Some(&door));
                route.push(Position::new(door.x + dx, door.y + dy));

                let mut game = ActionGame::county_with_world(world.clone());
                game.monsters.clear();
                for pos in route {
                    let destination = Tile3D::from_pos(pos, 0).center();
                    let mut opened = false;
                    for _ in 0..90 {
                        let delta = destination - game.player.position;
                        if delta.length() < 0.22 {
                            break;
                        }
                        let interact = pos == door && !opened && delta.length() < 0.9;
                        opened |= interact;
                        game.step(ActionInput {
                            interact,
                            movement: if interact {
                                Vec2::ZERO
                            } else {
                                delta.normalized()
                            },
                            ..ActionInput::default()
                        });
                    }
                    assert!(
                        game.player.position.distance(destination) < 0.22,
                        "seed {seed} path to {name} blocked at {pos:?}, player {:?}",
                        game.player.position
                    );
                }
            }
        }
    }

    #[test]
    fn gas_station_motorcycle_drives_in_and_reverses_out() {
        let world = World::with_fixture(42, crate::CountyFixture::GasStationLot);
        let transform = world.fixture_plan().unwrap().structures[1].transform;
        let start_tile =
            Tile3D::from_pos(transform.local_to_world(crate::LocalCell::new(8, -6, 0)), 0);
        assert!(world.tile_at(start_tile.pos()).unwrap().ground.is_road());
        let pump = transform.local_to_world(crate::LocalCell::new(5, 3, 0));
        let (ix, iy) = transform.inward();
        let inward = Vec2::new(ix as f32, iy as f32);
        let mut game = ActionGame::county_with_world(world);
        assert_eq!(game.teleport(start_tile), Some(start_tile));
        game.monsters.clear();
        game.step(ActionInput {
            interact: true,
            ..Default::default()
        });
        assert_eq!(game.control_mode(), ControlMode::Motorcycle);
        let initial = game.player.position;
        let input = |throttle, brake| ActionInput {
            ride: Some(RideInput::Motorcycle(MotorcycleInput {
                throttle,
                brake,
                steer: 0.0,
                lean: 0.0,
            })),
            ..Default::default()
        };
        for _ in 0..170 {
            game.step(input(0.45, 0.0));
        }
        let inside = game.player.position;
        assert!(
            inside.distance(initial) > 8.5,
            "drive in {initial:?} → {inside:?}"
        );
        assert!((inside - Tile3D::from_pos(pump, 0).center()).dot(inward) > 0.0);
        for _ in 0..90 {
            if game.hud().mounted.unwrap().speed_mph_x10 == 0 {
                break;
            }
            game.step(input(0.0, 1.0));
        }
        for _ in 0..1100 {
            game.step(input(0.0, 1.0));
            if game.player.position.distance(initial) < 0.3 {
                break;
            }
        }
        let exit = game.player.position;
        eprintln!("gas motorcycle start {initial:?}, inside {inside:?}, exit {exit:?}");
        assert!(
            exit.distance(initial) < 0.3,
            "reverse out {inside:?} → {exit:?}"
        );
        assert_eq!(game.control_mode(), ControlMode::Motorcycle);
        assert_eq!(game.hud().mounted.unwrap().condition_pct, 100);
    }

    #[test]
    fn gas_mart_public_wc_and_till_are_reachable() {
        let world = World::with_fixture(42, crate::CountyFixture::GasStationLot);
        let plan = world.fixture_plan().unwrap();
        let access = Tile3D::from_pos(plan.frontage.access, 0);
        let transform = plan.structures[0].transform;
        let till_uid = plan.structures[0]
            .props_world()
            .into_iter()
            .find(|p| p.id == PropId::CheckoutTill)
            .unwrap()
            .anchor_uid;
        let tile =
            |u, v| Tile3D::from_pos(transform.local_to_world(crate::LocalCell::new(u, v, 0)), 0);
        let mut game = ActionGame::county_with_world(world);
        assert_eq!(game.teleport(access), Some(access));
        game.monsters.clear();
        let walk = |game: &mut ActionGame, u, v| {
            let target = tile(u, v).center();
            for _ in 0..480 {
                let delta = target - game.player.position;
                if delta.length() < 0.12 {
                    return;
                }
                game.step(ActionInput {
                    movement: delta.normalized(),
                    ..Default::default()
                });
            }
            panic!(
                "gas route blocked at ({u},{v}), target {target:?}, player {:?}",
                game.player.position
            );
        };
        for (u, v) in [(18, -12), (18, -1), (10, -1)] {
            walk(&mut game, u, v);
        }
        assert_eq!(game.hud().interact_prompt, InteractPrompt::OpenDoor);
        game.step(ActionInput {
            interact: true,
            ..Default::default()
        });
        for (u, v) in [(10, 1), (8, 2)] {
            walk(&mut game, u, v);
        }
        assert_eq!(game.hud().interact_prompt, InteractPrompt::Search);
        game.step(ActionInput {
            interact: true,
            ..Default::default()
        });
        assert!(game.prop_opened.contains(&till_uid));
        for (u, v) in [(9, 4), (3, 4), (3, 5)] {
            walk(&mut game, u, v);
        }
        assert_eq!(game.hud().interact_prompt, InteractPrompt::OpenDoor);
        game.step(ActionInput {
            interact: true,
            ..Default::default()
        });
        walk(&mut game, 3, 7);
    }

    #[test]
    fn grocery_public_and_service_routes_are_walkable_in_live_action() {
        let world = World::with_fixture(42, crate::CountyFixture::GroceryLot);
        let plan = world.fixture_plan().expect("grocery fixture");
        let access = Tile3D::from_pos(plan.frontage.access, 0);
        let transform = plan.structures[0].transform;
        let tile =
            |u, v| Tile3D::from_pos(transform.local_to_world(crate::LocalCell::new(u, v, 0)), 0);
        let mut game = ActionGame::county_with_world(world);

        let walk = |game: &mut ActionGame, u, v| {
            let target = tile(u, v).center();
            for _ in 0..480 {
                let delta = target - game.player.position;
                if delta.length() < 0.08 {
                    return;
                }
                game.step(ActionInput {
                    movement: delta.normalized(),
                    ..ActionInput::default()
                });
            }
            panic!(
                "grocery route blocked at local ({u}, {v}) / {target:?}, player {:?} {:?}",
                game.player.position,
                game.outcome()
            );
        };
        let open = |game: &mut ActionGame, u, v| {
            assert_eq!(
                game.hud().interact_prompt,
                InteractPrompt::OpenDoor,
                "grocery door at local ({u}, {v}), player {:?}",
                game.player.position
            );
            game.step(ActionInput {
                interact: true,
                ..ActionInput::default()
            });
            assert!(matches!(
                game.stage.tile_kind(tile(u, v)),
                TileKind::Door { open: true, .. }
            ));
        };

        assert_eq!(game.teleport(access), Some(access));
        game.monsters.clear();
        walk(&mut game, 2, -2);
        walk(&mut game, 10, -2);
        walk(&mut game, 10, -1);
        open(&mut game, 10, 0);
        for (u, v) in [(10, 2), (9, 4), (9, 10), (3, 10), (3, 15)] {
            walk(&mut game, u, v);
        }
        open(&mut game, 3, 16);
        walk(&mut game, 3, 17);

        let apron = tile(17, -2);
        assert_eq!(game.teleport(apron), Some(apron));
        game.monsters.clear();
        for (u, v) in [(21, -2), (21, 23), (14, 23), (14, 22)] {
            walk(&mut game, u, v);
        }
        open(&mut game, 14, 21);
        walk(&mut game, 14, 20);
    }

    #[test]
    fn ranch_garage_connection_is_walkable_in_live_action() {
        use crate::world::RoomKind;
        use crate::world::blueprint::SpaceId;

        let world = World::new(42);
        let structure = world
            .fixture_plans()
            .into_iter()
            .flat_map(|p| &p.structures)
            .find(|s| s.archetype.starts_with("ranch_l_wing"))
            .expect("reference street ranch");
        let room_id = |kind| {
            SpaceId::Room(
                structure
                    .blueprint
                    .rooms
                    .iter()
                    .find(|room| room.kind == kind)
                    .expect("ranch room")
                    .id,
            )
        };
        let garage = room_id(RoomKind::Garage);
        let utility = room_id(RoomKind::Storage);
        let kitchen = room_id(RoomKind::Kitchen);
        let mut crossings = Vec::new();
        for (from, to) in [(kitchen, utility), (utility, garage)] {
            let door = structure
                .blueprint
                .doors
                .iter()
                .find(|door| {
                    door.kind == crate::DoorKind::Interior
                        && (door.connects == (from, to) || door.connects == (to, from))
                })
                .expect("interior door along the garage to kitchen route");
            let (du, dv) = door.tangent.normal();
            let adjacent =
                |sign| crate::LocalCell::new(door.cell.u + du * sign, door.cell.v + dv * sign, 0);
            let sign = [-1, 1]
                .into_iter()
                .find(|&sign| {
                    structure
                        .blueprint
                        .cell(adjacent(sign))
                        .and_then(|cell| cell.room)
                        .map(SpaceId::Room)
                        == Some(to)
                })
                .expect("door opens onto the destination room");
            let transform = structure.transform;
            crossings.push((
                transform.local_to_world(adjacent(-sign)),
                transform.local_to_world(door.cell),
                transform.local_to_world(adjacent(sign)),
                to == garage,
            ));
        }

        let mut game = ActionGame::county_with_world(world);
        for (start, door, target, enters_garage) in crossings {
            let start_tile = Tile3D::from_pos(start, 0);
            let door_tile = Tile3D::from_pos(door, 0);
            let target_tile = Tile3D::from_pos(target, 0);
            assert_eq!(game.teleport(start_tile), Some(start_tile));
            game.monsters.clear();
            game.step(ActionInput {
                interact: true,
                ..ActionInput::default()
            });
            assert!(matches!(
                game.stage.tile_kind(door_tile),
                TileKind::Door { open: true, .. }
            ));
            for _ in 0..120 {
                let delta = target_tile.center() - game.player.position;
                if delta.length() < 0.22 {
                    break;
                }
                game.step(ActionInput {
                    movement: delta.normalized(),
                    ..ActionInput::default()
                });
            }
            assert!(
                game.player.position.distance(target_tile.center()) < 0.22,
                "interior crossing from {start:?} to {target:?} blocked"
            );
            if enters_garage {
                let info = game
                    .stage
                    .building_by_id(game.stage.building_id(target_tile))
                    .unwrap();
                assert_ne!(
                    info.cutaway_slot_at(start, 0),
                    info.cutaway_slot_at(target, 0)
                );
            }
        }
    }

    #[test]
    fn reference_street_parked_car_leaves_dwelling_entries_clear() {
        use crate::spatial::collision::sweep_circle_rect_2d;
        let world = World::with_fixture(42, crate::CountyFixture::ResidentialStreet);
        let doors: Vec<_> = world
            .fixture_plans()
            .iter()
            .flat_map(|p| &p.structures)
            .filter(|s| {
                ["craftsman_", "ranch_", "colonial_"]
                    .iter()
                    .any(|prefix| s.archetype.starts_with(prefix))
            })
            .flat_map(|structure| {
                structure
                    .blueprint
                    .doors
                    .iter()
                    .filter(|d| d.kind == crate::DoorKind::Exterior)
                    .map(|d| {
                        (
                            structure.transform.local_to_world(d.cell),
                            structure.door_axis_at(d.cell).unwrap(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(doors.len(), 3);
        let mut game = ActionGame::county_with_world(world);
        for (pos, axis) in doors {
            game.teleport(Tile3D::from_pos(pos, 0))
                .expect("stream entrance");
            let cars: Vec<_> = game
                .prop_anchors
                .iter()
                .filter(|a| a.id == PropId::Car)
                .collect();
            assert!(
                !cars.is_empty(),
                "reference street must materialize a parked car"
            );
            let center = Tile3D::from_pos(pos, 0).center();
            let base = resolve_tile_support(&game.stage, Tile3D::from_pos(pos, 0)).0;
            let mut colliders = Vec::new();
            game.prop_colliders_in(
                Aabb3::new(
                    center.extend(base - 0.1) - Vec3::new(2.0, 2.0, 0.0),
                    center.extend(base + PLAYER_HEIGHT) + Vec3::new(2.0, 2.0, 0.1),
                ),
                &mut colliders,
            );
            let normal = axis.normal();
            let dir = Vec2::new(normal.0 as f32, normal.1 as f32);
            for sign in [-1.0, 1.0] {
                assert!(
                    colliders.iter().all(|c| sweep_circle_rect_2d(
                        center - dir * (sign * 0.8),
                        dir * (sign * 1.6),
                        PLAYER_RADIUS,
                        c.bounds.min.xy(),
                        c.bounds.max.xy()
                    )
                    .is_none()),
                    "prop blocks entrance at {pos:?}"
                );
            }
        }
    }

    #[test]
    fn support_fixture_stairs_transition_in_both_directions() {
        let world = World::with_fixture(42, crate::CountyFixture::SupportLot);
        let structure = &world.fixture_plan().expect("support fixture").structures[0];
        let stair = structure
            .transform
            .local_to_world(structure.blueprint.stairs[0].lower);
        let lower_landing = structure
            .transform
            .local_to_world(structure.blueprint.stairs[0].lower_landing);
        let mut game = ActionGame::county_with_world(world);
        assert_eq!(
            game.teleport(Tile3D::from_pos(lower_landing, 0))
                .unwrap()
                .pos(),
            lower_landing
        );
        for _ in 0..40 {
            game.step(ActionInput::default());
        }
        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        assert_eq!(game.player_pose().floor, 1);
        assert!(game.events().iter().any(|event| matches!(
            event,
            ActionEvent::FloorChanged {
                from_floor: 0,
                to_floor: 1,
                via: FloorTransition::Stairs,
                ..
            }
        )));
        // Leave the stair and allow the transition cooldown to expire before returning.
        let landing = game.player.position;
        for _ in 0..40 {
            game.step(ActionInput::default());
        }
        assert_ne!(landing, Tile3D::from_pos(stair, 1).center());
        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        assert_eq!(game.player_pose().floor, 0);
        assert!(game.events().iter().any(|event| matches!(
            event,
            ActionEvent::FloorChanged {
                from_floor: 1,
                to_floor: 0,
                via: FloorTransition::Stairs,
                ..
            }
        )));
    }

    #[test]
    fn colonial_approach_and_two_way_stairs_work_in_live_action() {
        for seed in [42, u64::MAX] {
            let world = World::with_fixture(seed, crate::CountyFixture::ColonialLot);
            let plan = world.fixture_plan().expect("Colonial review lot");
            assert_eq!(plan.zone, crate::world::blueprint::ZoneKind::Residential);
            let structure = &plan.structures[0];
            let access = plan.frontage.access;
            let front_door = structure.front_door;
            let paths = plan.outdoor.paths.clone();
            let (dx, dy) = plan.frontage.side.inward();
            let inside = Position::new(front_door.x + dx, front_door.y + dy);
            let stairs = structure.blueprint.stairs[0];
            let lower =
                Tile3D::from_pos(structure.transform.local_to_world(stairs.lower_landing), 0);
            let upper =
                Tile3D::from_pos(structure.transform.local_to_world(stairs.upper_landing), 1);
            let route_to_stairs = {
                use crate::world::blueprint::BlueprintKind;
                use std::collections::{HashMap, HashSet, VecDeque};
                let blueprint = &structure.blueprint;
                let hall_id = blueprint.cell(stairs.lower_landing).unwrap().room.unwrap();
                let hall: HashSet<_> = blueprint
                    .cells
                    .iter()
                    .filter(|(local, cell)| {
                        local.floor == 0
                            && cell.room == Some(hall_id)
                            && cell.kind == BlueprintKind::Floor
                    })
                    .map(|(&local, _)| structure.transform.local_to_world(local))
                    .collect();
                assert!(hall.contains(&inside));
                let mut pending = VecDeque::from([inside]);
                let mut previous = HashMap::from([(inside, inside)]);
                while let Some(pos) = pending.pop_front() {
                    if pos == lower.pos() {
                        break;
                    }
                    for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                        let next = Position::new(pos.x + dx, pos.y + dy);
                        if hall.contains(&next)
                            && let std::collections::hash_map::Entry::Vacant(entry) =
                                previous.entry(next)
                        {
                            entry.insert(pos);
                            pending.push_back(next);
                        }
                    }
                }
                let mut route = Vec::new();
                let mut pos = lower.pos();
                while pos != inside {
                    route.push(pos);
                    pos = *previous
                        .get(&pos)
                        .expect("stairs reachable through the entry hall");
                }
                route
            };
            let road = Position::new(access.x - dx, access.y - dy);
            assert!(
                world
                    .tile_at(road)
                    .is_some_and(|tile| tile.ground.is_road())
            );
            let mut game = ActionGame::county_with_world(world);
            let start = Tile3D::from_pos(road, 0);
            assert_eq!(game.teleport(start), Some(start));
            game.monsters.clear();
            let walk_to = |game: &mut ActionGame, pos: Position, door: bool| {
                let destination = Tile3D::from_pos(pos, 0).center();
                let mut opened = false;
                for _ in 0..90 {
                    let delta = destination - game.player.position;
                    if delta.length() < 0.19 {
                        break;
                    }
                    let interact = door && !opened && delta.length() < 0.9;
                    opened |= interact;
                    game.step(ActionInput {
                        movement: if interact {
                            Vec2::ZERO
                        } else {
                            delta.normalized()
                        },
                        interact,
                        ..ActionInput::default()
                    });
                }
                assert!(
                    game.player.position.distance(destination) < 0.19,
                    "seed {seed} cannot reach {pos:?} from {:?}",
                    game.player.position
                );
            };
            walk_to(&mut game, access, false);
            let mut last = access;
            assert!(!paths.is_empty());
            for (start, end) in paths {
                assert_eq!(start, last);
                assert!(start.x == end.x || start.y == end.y);
                let mut pos = start;
                while pos != end {
                    pos.x += (end.x - pos.x).signum();
                    pos.y += (end.y - pos.y).signum();
                    walk_to(&mut game, pos, pos == front_door);
                }
                last = end;
            }
            assert_eq!(last, front_door);
            walk_to(&mut game, inside, false);
            for pos in route_to_stairs.into_iter().rev() {
                walk_to(&mut game, pos, false);
            }
            for arrival in [upper, lower] {
                for _ in 0..40 {
                    game.step(ActionInput::default());
                }
                game.step(ActionInput {
                    interact: true,
                    ..ActionInput::default()
                });
                assert_eq!(game.player_tile(), arrival, "seed {seed}");
                assert!(game.events().iter().any(|event| matches!(
                    event,
                    ActionEvent::FloorChanged {
                        to_floor,
                        via: FloorTransition::Stairs,
                        ..
                    } if *to_floor == arrival.floor
                )));
            }
        }
    }

    #[test]
    fn stairs_transition_to_floor_1_and_f1_window_drop_returns_to_floor_0() {
        let mut game = ActionGame::residential(42);
        let origin = game.stage.residential().unwrap().origin();
        // Starter House stairs are at local (12, 21).
        let stair_f0 = Tile3D::new(origin.x + 12, origin.y + 21, 0);
        assert_eq!(game.stage.tile_kind(stair_f0), TileKind::StairsUp);

        game.player.position = stair_f0.center();
        game.step(ActionInput::default());
        assert_eq!(game.player_pose().floor, 1);
        assert!(game.events().iter().any(|e| matches!(
            e,
            ActionEvent::FloorChanged {
                from_floor: 0,
                to_floor: 1,
                via: FloorTransition::Stairs,
                ..
            }
        )));

        // Staying still does not oscillate back to Floor 0.
        for _ in 0..25 {
            game.step(ActionInput::default());
        }
        assert_eq!(game.player_pose().floor, 1);

        // Shatter a Floor 1 south window and interact to drop safely to Floor 0 exterior.
        let h = game.house().unwrap();
        let f1_win = Tile3D::new(h.x + 2, h.y + h.height - 1, 1);
        assert_eq!(game.stage.tile_kind(f1_win), TileKind::Window);
        let _ = game
            .stage
            .residential_mut()
            .unwrap()
            .damage_tile(f1_win, DamageKind::Axe);
        assert_eq!(game.stage.tile_kind(f1_win), TileKind::BrokenWindow);

        game.player.position = Vec2::new(f1_win.x as f32, (f1_win.y - 1) as f32);
        assert_eq!(game.hud().interact_prompt, InteractPrompt::DropWindow);
        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        assert_eq!(game.player_pose().floor, 0);
        assert_eq!(game.player_pose().position.y.round() as i32, f1_win.y + 1);
        assert!(game.events().iter().any(|e| matches!(
            e,
            ActionEvent::FloorChanged {
                from_floor: 1,
                to_floor: 0,
                via: FloorTransition::WindowDrop,
                ..
            }
        )));
    }

    #[test]
    fn aim_stays_locked_after_right_stick_release_while_moving() {
        let mut game = ActionGame::residential(42);
        game.step(ActionInput {
            aim: Some(Vec2::new(1.0, 0.0)),
            ..ActionInput::default()
        });
        game.step(ActionInput {
            movement: Vec2::new(0.0, 1.0),
            ..ActionInput::default()
        });
        assert_eq!(game.player_pose().facing, Vec2::new(1.0, 0.0));
    }

    #[test]
    fn aim_inside_deadzone_does_not_override_movement_heading() {
        let mut game = ActionGame::residential(42);
        game.step(ActionInput {
            movement: Vec2::new(1.0, 0.0),
            aim: Some(Vec2::new(0.0, AIM_DEADZONE * 0.5)),
            ..ActionInput::default()
        });
        let facing = game.player_pose().facing;
        assert!(
            (facing.x - 1.0).abs() < 1e-5 && facing.y.abs() < 1e-5,
            "tiny aim stick must not override movement heading, got {facing:?}"
        );
    }

    #[test]
    fn movement_inside_deadzone_does_not_change_heading() {
        let mut game = ActionGame::residential(42);
        let start = game.player_pose().facing;
        game.step(ActionInput {
            movement: Vec2::new(PLAYER_DEADZONE * 0.5, 0.0),
            ..ActionInput::default()
        });
        let facing = game.player_pose().facing;
        assert!(
            (facing - start).length() < 1e-5,
            "movement below the deadzone must not rotate the survivor, got {facing:?}"
        );
    }

    #[test]
    fn seed_replay_is_fully_deterministic() {
        let mut a = ActionGame::residential(42);
        let mut b = ActionGame::residential(42);
        for tick in 0..120 {
            let input = ActionInput {
                movement: if tick < 40 {
                    Vec2::new(0.4, -0.9)
                } else {
                    Vec2::new(-0.6, 0.2)
                },
                aim: (tick % 15 == 0).then_some(Vec2::new(1.0, -0.3)),
                interact: tick == 10 || tick == 55,
                fire: tick == 65 || tick == 90,
                ..ActionInput::default()
            };
            a.step(input);
            b.step(input);
            assert_eq!(a.hud(), b.hud());
            assert_eq!(a.player_pose(), b.player_pose());
            assert_eq!(a.zombie_poses(), b.zombie_poses());
        }
    }

    #[test]
    fn county_spawn_streams_chunks_and_does_not_auto_win() {
        let mut game = ActionGame::county(42);
        assert!(game.is_county());
        assert_eq!(game.mode(), ActionMode::County);
        assert_eq!(game.current_district(), District::Residential);
        assert_eq!(game.district_code(), 1);
        assert!(game.house().is_none());

        let spawn_tile = game.player_tile();
        assert!(game.stage.is_walkable(spawn_tile, false));
        assert!(!game.caches().is_empty());
        assert!(
            game.stage
                .caches()
                .iter()
                .any(|c| c.loot.weapon == Some(Weapon::Axe)),
            "starter Axe cache must be spawned"
        );
        assert!(
            game.stage
                .caches()
                .iter()
                .any(|c| c.loot.weapon == Some(Weapon::Pistol) && c.loot.pistol_ammo >= 12),
            "starter house must have Pistol + 12 rounds"
        );
        assert!(
            !game.monsters.is_empty(),
            "county window must contain infected"
        );
        for m in &game.monsters {
            assert!(
                m.pos.distance(game.player_pose().position) >= 4.4,
                "no monster should spawn right on top of wake_position"
            );
        }

        // Clearing local active monsters must NOT end a County run.
        game.monsters.clear();
        game.step(ActionInput::default());
        assert_eq!(game.outcome(), ActionOutcome::Playing);

        // Walking > 32 tiles across chunk seams via step() shifts the active 7x7 window seamlessly.
        let initial_bounds = game.block_bounds().unwrap();
        let mut dir = Vec2::new(1.0, 0.0);
        let mut walked_far = false;
        for step_idx in 0..800 {
            let p = game.player_tile().pos();
            if p.distance(spawn_tile.pos()) >= 34 {
                walked_far = true;
                break;
            }
            // Pick an unobstructed walkable cardinal neighbor moving away from spawn when stuck.
            let ahead = Position::new(
                (game.player_pose().position.x + dir.x * 0.9).round() as i32,
                (game.player_pose().position.y + dir.y * 0.9).round() as i32,
            );
            if !game.stage.is_walkable(Tile3D::from_pos(ahead, 0), false) || step_idx % 45 == 44 {
                let mut best_dir = dir;
                let mut best_score = i32::MIN;
                for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                    let cand = Position::new(p.x + dx, p.y + dy);
                    let cand2 = Position::new(p.x + dx * 2, p.y + dy * 2);
                    if game.stage.is_walkable(Tile3D::from_pos(cand, 0), false)
                        && game.stage.is_walkable(Tile3D::from_pos(cand2, 0), false)
                    {
                        let d_spawn = cand.distance(spawn_tile.pos()) as i32;
                        let road_bonus = if game
                            .world()
                            .tile_at(cand)
                            .is_some_and(|t| t.ground.is_road())
                        {
                            20
                        } else {
                            0
                        };
                        let score = d_spawn + road_bonus;
                        if score > best_score {
                            best_score = score;
                            best_dir = Vec2::new(dx as f32, dy as f32);
                        }
                    }
                }
                dir = best_dir;
            }
            game.step(ActionInput {
                movement: dir,
                ..ActionInput::default()
            });
        }
        assert!(walked_far, "player walked > 32 tiles across chunk seams");
        let after_bounds = game.block_bounds().unwrap();
        assert_ne!(initial_bounds, after_bounds);
    }

    #[test]
    fn county_army_base_subbuildings_and_3f_collapse_work() {
        let mut game = ActionGame::county(42);
        let world = game.world().clone();
        let sites = world.sites().unwrap();

        // ArmyBase splits into 3 sub-buildings while keeping the compound yard outdoors.
        let army = sites
            .buildings
            .iter()
            .find(|b| b.kind == crate::BuildingKind::ArmyBase)
            .expect("army base exists");
        assert!(game.teleport(Tile3D::from_pos(army.entry, 0)).is_some());
        assert_eq!(game.current_district(), District::MilitaryBase);
        let sub_buildings: Vec<_> = game
            .buildings()
            .iter()
            .filter(|b| b.style == BuildingStyle::ArmyBarracks)
            .collect();
        assert_eq!(sub_buildings.len(), 3);
        for sb in sub_buildings {
            assert_eq!(sb.floors, 1);
        }

        // Breaching an F0 load-bearing wall in 3F Apartments collapses F1 and F2 overhang onto F0.
        let apt = sites
            .buildings
            .iter()
            .find(|b| b.kind == crate::BuildingKind::Apartments)
            .expect("apartments exists");
        assert!(game.teleport(Tile3D::from_pos(apt.entry, 0)).is_some());
        let corner = Tile3D::new(apt.max.x, apt.max.y, 0);
        let adj_wall = Tile3D::new(apt.max.x - 1, apt.max.y, 0);
        let Stage::County(county) = &mut game.stage else {
            unreachable!();
        };
        let _ = county.damage_tile(adj_wall, DamageKind::RocketBlast);
        let res = county
            .damage_tile(corner, DamageKind::RocketBlast)
            .expect("breach corner pillar");
        assert!(res.collapsed_cells > 0);
        assert!(!county.has_floor(Tile3D::new(apt.max.x, apt.max.y, 1)));
        assert!(!county.has_floor(Tile3D::new(apt.max.x, apt.max.y, 2)));
    }

    #[test]
    fn county_teleport_persists_state_across_3f_apartments_and_hospital() {
        let mut game = ActionGame::county(42);
        let world = game.world().clone();
        let sites = world.sites().unwrap();

        // Find starter Axe cache, collect it, and shatter a nearby window in starter neighborhood.
        let axe_cache = game
            .stage
            .caches()
            .iter()
            .find(|c| c.loot.weapon == Some(Weapon::Axe))
            .copied()
            .expect("starter Axe cache");
        let axe_pos = axe_cache.pos;
        assert!(game.teleport(axe_pos).is_some());
        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        assert_eq!(game.hud().equipped, Some(Weapon::Axe));
        assert_eq!(game.hud().objective_kind, ObjectiveKind::Explore);

        // Use a window on the starter house's street wall. This gives the
        // axe a perpendicular approach on the same foundation.
        let plan = world.fixture_plan().unwrap();
        let house = &plan.structures[0];
        let front = house
            .blueprint
            .doors
            .iter()
            .find(|door| door.kind == crate::DoorKind::Exterior)
            .unwrap()
            .cell
            .v;
        let (win_tile, adj_walk) = house
            .blueprint
            .cells
            .iter()
            .filter(|(tile, cell)| {
                tile.floor == 0 && tile.v == front && cell.kind == crate::BlueprintKind::Window
            })
            .find_map(|(&window, _)| {
                let stand = crate::LocalCell::new(window.u, window.v - 1, 0);
                house
                    .blueprint
                    .cell(stand)
                    .filter(|cell| cell.surface == Surface::Porch && cell.kind.walkable())?;
                Some((
                    Tile3D::from_pos(house.transform.local_to_world(window), 0),
                    Tile3D::from_pos(house.transform.local_to_world(stand), 0),
                ))
            })
            .expect("a front window reachable from the starter porch");
        assert!(game.teleport(adj_walk).is_some());
        game.player.facing = (win_tile.center() - game.player.position).normalized();
        game.step(ActionInput {
            fire: true,
            ..ActionInput::default()
        });
        assert_eq!(game.stage.tile_kind(win_tile), TileKind::BrokenWindow);

        // Teleport to Hospital, then to 3F Apartments and traverse 3F stairs (0 -> 1 -> 2 -> 1 -> 0).
        let hospital = sites
            .buildings
            .iter()
            .find(|b| b.kind == crate::BuildingKind::Hospital)
            .expect("hospital exists");
        assert!(game.teleport(Tile3D::from_pos(hospital.entry, 0)).is_some());
        assert_eq!(game.current_district(), District::Hospital);
        assert_eq!(game.district_code(), 3);

        let apt = sites
            .buildings
            .iter()
            .find(|b| b.kind == crate::BuildingKind::Apartments)
            .expect("3F apartments exists");
        let stair_pos = (apt.min.y..=apt.max.y)
            .flat_map(|y| (apt.min.x..=apt.max.x).map(move |x| Position::new(x, y)))
            .find(|&p| apt.feature_at(p, 0) == Some(crate::Feature::Stairs))
            .expect("apartments stair shaft");
        let stair_f0 = Tile3D::from_pos(stair_pos, 0);
        assert!(game.teleport(stair_f0).is_some());
        assert_eq!(game.stage.tile_kind(stair_f0), TileKind::StairsUp);
        assert_eq!(
            game.stage.tile_kind(Tile3D::from_pos(stair_pos, 1)),
            TileKind::StairsBoth
        );
        assert_eq!(
            game.stage.tile_kind(Tile3D::from_pos(stair_pos, 2)),
            TileKind::StairsDown
        );

        // Step off stair, then traverse 0 -> 1 -> 2 -> 1 -> 0.
        game.must_leave_stair = None;
        game.stair_cooldown = 0;
        game.player.position = stair_f0.center();
        game.step(ActionInput::default());
        assert_eq!(game.player_pose().floor, 1);

        game.must_leave_stair = None;
        game.stair_cooldown = 0;
        game.player.position = Tile3D::from_pos(stair_pos, 1).center();
        game.step(ActionInput::default());
        assert_eq!(game.player_pose().floor, 2);

        game.must_leave_stair = None;
        game.stair_cooldown = 0;
        game.player.position = Tile3D::from_pos(stair_pos, 2).center();
        game.step(ActionInput::default());
        assert_eq!(game.player_pose().floor, 1);

        game.must_leave_stair = None;
        game.stair_cooldown = 0;
        game.player.position = Tile3D::from_pos(stair_pos, 1).center();
        game.step(ActionInput::default());
        assert_eq!(game.player_pose().floor, 0);

        // Teleport back to chunk A and verify Axe, health, opened cache, and broken window persist!
        assert!(game.teleport(adj_walk).is_some());
        assert_eq!(game.hud().equipped, Some(Weapon::Axe));
        assert_eq!(game.hud().health, PLAYER_MAX_HEALTH);
        assert_eq!(game.stage.tile_kind(win_tile), TileKind::BrokenWindow);
        assert!(
            game.stage
                .caches()
                .iter()
                .find(|c| c.id == axe_cache.id)
                .is_some_and(|c| c.opened)
        );
    }

    #[test]
    fn county_teleport_rejects_deep_water_or_mountain_and_snaps_wall_to_walkable() {
        let mut game = ActionGame::county(42);
        let world = game.world().clone();

        // Find a deep water/mountain tile whose r <= 8 neighborhood has NO walkable tiles.
        let impassable_core = (0..WORLD_SIZE)
            .flat_map(|y| (0..WORLD_SIZE).map(move |x| Position::new(x, y)))
            .find(|&p| {
                (-8..=8_i32).all(|dy| {
                    (-8..=8_i32).all(|dx| {
                        let q = Position::new(p.x + dx, p.y + dy);
                        world.tile_at(q).is_some_and(|t| !t.ground.walkable())
                    })
                })
            })
            .expect("county has deep water or mountain core");

        let before_pose = game.player_pose();
        let before_bounds = game.block_bounds();
        assert_eq!(game.teleport(Tile3D::from_pos(impassable_core, 0)), None);
        assert_eq!(game.player_pose(), before_pose);
        assert_eq!(game.block_bounds(), before_bounds);

        // Teleporting onto a building wall snaps cleanly to a walkable interior/entry tile.
        let sites = world.sites().unwrap();
        let b = sites
            .buildings
            .iter()
            .find(|b| b.kind != crate::BuildingKind::ArmyBase)
            .expect("non-army building");
        let wall_tile = Tile3D::new(b.min.x, b.min.y, 0);
        let snapped = game
            .teleport(wall_tile)
            .expect("wall teleport snaps to walkable tile");
        assert!(game.stage.is_walkable(snapped, false));
        assert!(
            game.stage
                .circle_walkable(snapped.center(), snapped.floor, PLAYER_RADIUS, false)
        );
    }

    #[test]
    fn fall_injury_curve_handles_one_story_two_story_and_cliff_drops() {
        assert_eq!(ActionGame::compute_fall_damage(0.5), (0, 0));
        assert_eq!(
            ActionGame::compute_fall_damage(FLOOR_H),
            (0, DROP_RECOVERY_TICKS)
        );
        assert_eq!(ActionGame::compute_fall_damage(FLOOR_H * 2.0), (1, 16));
        assert_eq!(ActionGame::compute_fall_damage(FLOOR_H * 3.0), (2, 22));
        let (cliff_dmg, cliff_rec) = ActionGame::compute_fall_damage(10.0);
        assert!(cliff_dmg >= 3, "a four-story drop must deal >= 3 HP damage");
        assert_eq!(cliff_rec, 26);

        let mut game = ActionGame::residential(42);
        let start_hp = game.hud().health;
        let xy = game.player_body.position.xy();
        game.player_body = BodyState::airborne(
            xy.extend(10.0),
            Vec3::new(0.0, 0.0, -4.0),
            game.player_body.heading,
        );
        game.fall_peak_z = Some(10.0);

        let mut landed_ev = None;
        for _ in 0..120 {
            game.step(ActionInput::default());
            if let Some(ev) = game
                .events()
                .iter()
                .find(|e| matches!(e, ActionEvent::ActorLanded { .. }))
            {
                landed_ev = Some(*ev);
                break;
            }
        }
        let Some(ActionEvent::ActorLanded { damage, .. }) = landed_ev else {
            panic!("expected ActorLanded after high airborne drop");
        };
        assert!(
            damage >= 3,
            "expected >= 3 damage from 6.5u drop, got {damage}"
        );
        assert_eq!(game.hud().health, start_hp.saturating_sub(damage));
    }

    #[test]
    fn fixture_prop_collides_and_damage_is_uid_owned_across_span() {
        let mut game = ActionGame::residential(7);
        let tile = Tile3D::from_vec2(game.player.position, game.player.floor);
        let anchor = PropAnchor {
            anchor_uid: 0xfeed,
            tile,
            id: PropId::Sofa,
            span: PropSpan::new(2, 1),
            attachment: CellAttachment::Floor,
            facing: PropFacing::North,
            offset: Vec3::ZERO,
        };
        assert!(game.add_prop_fixture(anchor));
        assert!(
            game.prop_anchor_at(Tile3D::new(tile.x + 1, tile.y, tile.floor))
                .is_some()
        );
        let hp = anchor.id.def().durability;
        let first = game.damage_tile(tile, DamageKind::Axe).unwrap();
        assert!(first.damaged && !first.destroyed);
        let second = game
            .damage_tile(
                Tile3D::new(tile.x + 1, tile.y, tile.floor),
                DamageKind::RocketBlast,
            )
            .unwrap();
        assert!(second.destroyed || hp > 0);
    }
}
