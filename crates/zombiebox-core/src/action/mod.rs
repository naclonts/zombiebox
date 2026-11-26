//! Smooth, fixed-step multi-level residential action runtime (`60 Hz`).
//! The legacy 20 Hz tile-command `Game` in `lib.rs` remains unchanged.

pub mod ai;
pub mod block;
pub mod county;
pub mod types;
pub mod weapons;

pub use ai::{MonsterSpec, monster_spec};
pub use types::*;
pub use weapons::{
    Inventory, Projectile, ReloadMode, WEAPON_MUZZLE_HEIGHT, WeaponSpec, weapon_spec,
};

use crate::spatial::{
    Aabb3, BodyShape, Collider, FLOOR_H, MotionState, MoveParams, QueryFilter, Ray3, SurfaceQuad,
    TerrainSample, move_kinematic_with_horizontal_vel, resolve_actor_support, resolve_tile_support,
    select_grounded_support, semantic_floor_for_body,
};
use crate::{District, House, Position, World};
use ai::{AiSystem, Monster, damage_monster, propagate_noise};
use block::{ResidentialBlock, SupplyCache};
use county::CountyStage;
use weapons::{
    compute_player_aim_3d, is_rocket_danger_close_3d, step_projectiles_3d, try_fire_or_swing_3d,
};

pub const PLAYER_MAX_SPEED: f32 = 5.2;
pub const PLAYER_QUIET_STICK_THRESHOLD: f32 = 0.52;
pub const PLAYER_DEADZONE: f32 = 0.08;
pub const PLAYER_RADIUS: f32 = 0.23;
pub const PLAYER_HEIGHT: f32 = 1.05;
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
    ToggleDoor(Tile3D, bool), // bool = true if currently closed (so prompt is OpenDoor)
    DropWindow(Tile3D, Tile3D),
    ClimbWindow(Tile3D, Tile3D),
    TakeStairs(Tile3D, Tile3D, (i8, i8), bool), // bool = goes_up
}

impl InteractAction {
    const fn prompt(self) -> InteractPrompt {
        match self {
            Self::Search(..) => InteractPrompt::Search,
            Self::ToggleDoor(_, true) => InteractPrompt::OpenDoor,
            Self::ToggleDoor(_, false) => InteractPrompt::CloseDoor,
            Self::DropWindow(..) => InteractPrompt::DropWindow,
            Self::ClimbWindow(..) => InteractPrompt::ClimbWindow,
            Self::TakeStairs(_, _, _, true) => InteractPrompt::StairsUp,
            Self::TakeStairs(_, _, _, false) => InteractPrompt::StairsDown,
        }
    }

    const fn tile(self) -> Tile3D {
        match self {
            Self::Search(_, t)
            | Self::ToggleDoor(t, _)
            | Self::DropWindow(t, _)
            | Self::ClimbWindow(t, _)
            | Self::TakeStairs(t, _, _, _) => t,
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

    const fn max_floors(&self) -> Floor {
        match self {
            Self::Residential(_) => NUM_FLOORS as Floor,
            Self::County(_) => COUNTY_FLOORS as Floor,
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
    next_monster_id: u32,
    projectiles: Vec<Projectile>,
    next_proj_id: u32,
    active_blasts: Vec<ActiveBlast>,
    next_blast_id: u32,
    ai: AiSystem,
    events: Vec<ActionEvent>,
    noises: Vec<NoiseEvent>,
    outcome: ActionOutcome,
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

        Self {
            seed,
            world,
            stage: Stage::County(Box::new(county)),
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
            next_monster_id: 1,
            projectiles: Vec::new(),
            next_proj_id: 1,
            active_blasts: Vec::new(),
            next_blast_id: 1,
            ai: AiSystem::new(),
            events: Vec::new(),
            noises: Vec::new(),
            outcome: ActionOutcome::Playing,
        }
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

        let mut next_monster_id = 1u32;
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

        Self {
            seed,
            world,
            stage: Stage::Residential(Box::new(block)),
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
            ai: AiSystem::new(),
            events: Vec::new(),
            noises: Vec::new(),
            outcome: ActionOutcome::Playing,
        }
    }

    pub fn mode(&self) -> ActionMode {
        self.stage.mode()
    }

    pub fn is_county(&self) -> bool {
        self.stage.is_county()
    }

    pub fn seed(&self) -> u64 {
        self.seed
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
        if self.mode() != ActionMode::County || self.outcome != ActionOutcome::Playing {
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
        Some(dest)
    }

    pub fn step(&mut self, input: ActionInput) {
        self.events.clear();
        self.noises.clear();
        if self.outcome != ActionOutcome::Playing {
            return;
        }
        self.tick += 1;

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

        // 2. Weapon switching, quick-heal, and manual reload.
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

        // 3. Player analog movement, quiet walk, knockback, and stair transitions.
        self.step_player_movement(input);
        let focus = self.player_tile().pos();
        if let Stage::County(county) = &mut self.stage {
            county.sync_active_window(&self.world, focus, &mut self.monsters);
        }

        // 4. Aim / facing update (right-stick retains facing on release).
        if let Some(aim) = input.aim {
            let aim = aim.normalized();
            if aim.length_squared() > 0.01 {
                self.player.facing = aim;
                self.player_body.heading = aim.y.atan2(aim.x);
                self.aim_explicit = true;
            }
        } else if !self.aim_explicit {
            let mv = input.movement.normalized();
            if mv.length_squared() > 0.01 {
                self.player.facing = mv;
                self.player_body.heading = mv.y.atan2(mv.x);
            }
        }

        // 5. Contextual interaction (cache -> door -> upper drop -> F0 vault -> stairs).
        if input.interact {
            self.resolve_interact();
        }

        let ev_combat_start = self.events.len();

        // 6. Firing / melee swing.
        if input.fire {
            try_fire_or_swing_3d(
                &mut self.stage,
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

        // 7. Advance active rockets and shooter bolts.
        step_projectiles_3d(
            &mut self.stage,
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
        propagate_noise(
            &self.stage,
            &mut self.monsters,
            &self.noises,
            &mut self.events,
        );

        // 10. Advance sight/sound-reactive AI, 3D pathfinding, and monster attacks.
        self.ai.step_monsters_3d(
            &mut self.stage,
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
                &self.stage,
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
            if self.stage.is_walkable(exit_tile, false)
                && self
                    .stage
                    .raycast_filtered(
                        Ray3::new(probe_origin, d_vec.normalized().extend(0.0)),
                        0.78,
                        QueryFilter::DEFAULT,
                    )
                    .is_none()
            {
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

    fn compute_interact_action(&self) -> Option<InteractAction> {
        let p = self.player.position;
        let floor = self.player.floor;
        let facing = self.player.facing.normalized();

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

        None
    }

    fn resolve_interact(&mut self) {
        let Some(action) = self.compute_interact_action() else {
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
        let interact = self.compute_interact_action();
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
        PlayerRenderState {
            pose: self.player,
            elevation: self.player_body.position.z,
            body: self.player_body,
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

    pub fn toggle_door(&mut self, tile: Tile3D) -> Option<bool> {
        self.stage.toggle_door(tile)
    }

    pub fn damage_tile(&mut self, tile: Tile3D, kind: DamageKind) -> Option<TileDamageResult> {
        self.stage.damage_tile(tile, kind)
    }

    pub(crate) fn spatial_cell(&self, tile: Tile3D) -> Option<SpatialCell> {
        self.stage.spatial_cell(tile)
    }

    pub(crate) fn building_by_id(&self, id: BuildingId) -> Option<&BuildingInfo> {
        self.stage.building_by_id(id)
    }

    pub(crate) fn playable_bounds(&self) -> (Position, Position) {
        self.stage.playable_bounds()
    }

    pub(crate) const fn max_floors(&self) -> Floor {
        self.stage.max_floors()
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

    pub fn has_cache_at(&self, p: Position) -> bool {
        self.stage.caches().iter().any(|c| {
            !c.opened && c.pos.x == p.x && c.pos.y == p.y && c.pos.floor == self.player.floor
        })
    }

    pub fn near_loot(&self) -> bool {
        matches!(
            self.compute_interact_action(),
            Some(InteractAction::Search(..))
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
        self.projectiles.iter().map(Projectile::to_view).collect()
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
        let interact = self.compute_interact_action();
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

        // Find a Floor 0 window in the current window and break it with the Axe.
        let (bmin, bmax) = game.block_bounds().unwrap();
        let win_tile = (bmin.y..=bmax.y)
            .flat_map(|y| (bmin.x..=bmax.x).map(move |x| Tile3D::new(x, y, 0)))
            .find(|&t| {
                game.stage.tile_kind(t) == TileKind::Window
                    && [(0, 1), (0, -1), (1, 0), (-1, 0)]
                        .into_iter()
                        .any(|(dx, dy)| {
                            game.stage
                                .is_walkable(Tile3D::new(t.x + dx, t.y + dy, 0), false)
                        })
            })
            .expect("window in starter window");
        let adj_walk = [(0, 1), (0, -1), (1, 0), (-1, 0)]
            .into_iter()
            .map(|(dx, dy)| Tile3D::new(win_tile.x + dx, win_tile.y + dy, 0))
            .find(|&t| game.stage.is_walkable(t, false))
            .unwrap();
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
        let (cliff_dmg, cliff_rec) = ActionGame::compute_fall_damage(6.5);
        assert!(cliff_dmg >= 3, "drop > 5.0 units must deal >= 3 HP damage");
        assert_eq!(cliff_rec, 26);

        let mut game = ActionGame::residential(42);
        let start_hp = game.hud().health;
        let xy = game.player_body.position.xy();
        game.player_body = BodyState::airborne(
            xy.extend(6.5),
            Vec3::new(0.0, 0.0, -4.0),
            game.player_body.heading,
        );
        game.fall_peak_z = Some(6.5);

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
}
