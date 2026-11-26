//! Smooth, fixed-step multi-level residential action runtime (`60 Hz`).
//! The legacy 20 Hz tile-command `Game` in `lib.rs` remains unchanged.

pub mod ai;
pub mod block;
pub mod types;
pub mod weapons;

pub use types::*;
pub use weapons::{Inventory, Projectile, ReloadMode, WeaponSpec, weapon_spec};

use crate::{House, Position, World};
use ai::{AiSystem, Monster, damage_monster, propagate_noise};
use block::ResidentialBlock;
use weapons::{is_rocket_danger_close, step_projectiles, try_fire_or_swing};

pub const PLAYER_MAX_SPEED: f32 = 5.2;
pub const PLAYER_QUIET_STICK_THRESHOLD: f32 = 0.52;
pub const PLAYER_DEADZONE: f32 = 0.08;
pub const PLAYER_RADIUS: f32 = 0.23;
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
    Search(u8, Tile3D),
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

pub struct ActionGame {
    seed: u64,
    world: World,
    block: ResidentialBlock,
    tick: u64,
    player: Pose,
    player_knockback: Vec2,
    health: u8,
    max_health: u8,
    hurt_iframes: u8,
    footstep_timer: u8,
    drop_recovery_ticks: u8,
    stair_cooldown: u8,
    must_leave_stair: Option<Position>,
    quiet_walking: bool,
    aim_explicit: bool,
    inventory: Inventory,
    monsters: Vec<Monster>,
    zombies_total: usize,
    next_monster_id: u32,
    projectiles: Vec<Projectile>,
    next_proj_id: u32,
    ai: AiSystem,
    events: Vec<ActionEvent>,
    noises: Vec<NoiseEvent>,
    outcome: ActionOutcome,
}

impl ActionGame {
    pub fn residential(seed: u64) -> Self {
        let world = World::birch_street(seed);
        let mut block = ResidentialBlock::new(&world, seed);
        let spawn = block.player_spawn();
        let player = Pose {
            position: spawn.center(),
            facing: Vec2::new(0.0, -1.0),
            floor: spawn.floor,
        };

        let mut next_monster_id = 1u32;
        let monsters: Vec<Monster> = block
            .monster_spawns()
            .iter()
            .map(|sp| {
                let id = MonsterId(next_monster_id);
                next_monster_id += 1;
                Monster::new(id, sp.kind, sp.variant, sp.pos)
            })
            .collect();
        let zombies_total = monsters.len();

        block.update_visibility(player.position, player.floor);

        Self {
            seed,
            world,
            block,
            tick: 0,
            player,
            player_knockback: Vec2::ZERO,
            health: PLAYER_MAX_HEALTH,
            max_health: PLAYER_MAX_HEALTH,
            hurt_iframes: 0,
            footstep_timer: 0,
            drop_recovery_ticks: 0,
            stair_cooldown: 0,
            must_leave_stair: None,
            quiet_walking: false,
            aim_explicit: false,
            inventory: Inventory::new(),
            monsters,
            zombies_total,
            next_monster_id,
            projectiles: Vec::new(),
            next_proj_id: 1,
            ai: AiSystem::new(),
            events: Vec::new(),
            noises: Vec::new(),
            outcome: ActionOutcome::Playing,
        }
    }

    pub fn step(&mut self, input: ActionInput) {
        self.events.clear();
        self.noises.clear();
        if self.outcome != ActionOutcome::Playing {
            return;
        }
        self.tick += 1;

        // 1. Tick timers & dynamic light flashes.
        self.block.tick_flashes();
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
            self.inventory.try_use_medkit(
                &mut self.health,
                self.max_health,
                self.player.position,
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

        // 4. Aim / facing update (right-stick retains facing on release).
        if let Some(aim) = input.aim {
            let aim = aim.normalized();
            if aim.length_squared() > 0.01 {
                self.player.facing = aim;
                self.aim_explicit = true;
            }
        } else if !self.aim_explicit {
            let mv = input.movement.normalized();
            if mv.length_squared() > 0.01 {
                self.player.facing = mv;
            }
        }

        // 5. Contextual interaction (cache -> door -> F1 drop -> F0 vault -> stairs).
        if input.interact {
            self.resolve_interact();
        }

        // 6. Firing / melee swing.
        if input.fire {
            try_fire_or_swing(
                &mut self.block,
                &mut self.inventory,
                self.player,
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
        step_projectiles(
            &mut self.block,
            &mut self.projectiles,
            self.player,
            PLAYER_RADIUS,
            &mut self.health,
            &mut self.hurt_iframes,
            &mut self.player_knockback,
            &mut self.monsters,
            &mut self.events,
            &mut self.noises,
        );

        // 8. Resolve any structural F0 -> F1 overhang collapses caused by breaching.
        self.process_collapses();

        // 9. Propagate player/combat noises to monsters.
        propagate_noise(
            &self.block,
            &mut self.monsters,
            &self.noises,
            &mut self.events,
        );

        // 10. Advance sight/sound-reactive AI, 3D pathfinding, and monster attacks.
        self.ai.step_monsters(
            &mut self.block,
            &mut self.monsters,
            self.player,
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

        // 11. Compact dead (non-swelling) monsters and check outcome.
        self.monsters.retain(|m| m.is_alive() || m.is_swelling());

        if self.health == 0 {
            self.outcome = ActionOutcome::Dead;
            self.events.push(ActionEvent::Died);
        } else if self.monsters.is_empty() {
            self.outcome = ActionOutcome::Won;
            self.events.push(ActionEvent::Won);
        }

        // 12. Update room-based LOS visibility (cached by tile + floor + terrain_version).
        self.block
            .update_visibility(self.player.position, self.player.floor);
    }

    fn step_player_movement(&mut self, input: ActionInput) {
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
        if self.block.obstruction(cur_tile) == TileObstruction::BrokenWindow {
            speed_mult *= WINDOW_VAULT_SPEED_MULT;
        }
        if self.drop_recovery_ticks > 0 {
            speed_mult *= DROP_RECOVERY_SPEED_MULT;
        }

        let move_delta = dir * (mag * PLAYER_MAX_SPEED * speed_mult * DT) + self.player_knockback;
        if move_delta.length_squared() > 1e-8 {
            self.block.move_body(
                &mut self.player.position,
                self.player.floor,
                move_delta,
                PLAYER_RADIUS,
                self.player.floor == 0,
                true,
            );
        }

        self.player_knockback = self.player_knockback * KNOCKBACK_DECAY;
        if self.player_knockback.length_squared() < 1e-6 {
            self.player_knockback = Vec2::ZERO;
        }

        // Footstep noise only when moving briskly above quiet-walk threshold.
        if mag > PLAYER_QUIET_STICK_THRESHOLD && self.footstep_timer == 0 {
            self.footstep_timer = FOOTSTEP_INTERVAL_TICKS;
            let radius = 2.2
                + 1.2
                    * ((mag - PLAYER_QUIET_STICK_THRESHOLD) / (1.0 - PLAYER_QUIET_STICK_THRESHOLD));
            self.noises.push(NoiseEvent {
                origin: self.player.position,
                floor: self.player.floor,
                radius,
                priority: 1,
                kind: NoiseKind::Footstep,
            });
        }

        // Safety drop if standing on an F1 tile that lacks flooring (e.g. collapsed).
        let now_tile = Tile3D::from_vec2(self.player.position, self.player.floor);
        if self.player.floor > 0 && !self.block.has_floor(now_tile) {
            let from_floor = self.player.floor;
            self.player.floor = 0;
            self.events.push(ActionEvent::FloorChanged {
                from_floor,
                to_floor: 0,
                at: Tile3D::new(now_tile.x, now_tile.y, 0),
                via: FloorTransition::Collapse,
            });
        }

        // Anti-oscillation automatic stair transitions.
        if let Some(stair_pos) = self.must_leave_stair {
            let stair_center = Vec2::new(stair_pos.x as f32, stair_pos.y as f32);
            if self.player.position.distance(stair_center) > 0.62 {
                self.must_leave_stair = None;
            }
        }
        if self.stair_cooldown == 0 && self.must_leave_stair.is_none() {
            let p_tile = Tile3D::from_vec2(self.player.position, self.player.floor);
            if let Some((dest, landing)) = self.block.stair_link(p_tile)
                && self.player.position.distance(p_tile.center()) <= 0.38
            {
                self.take_stairs(p_tile, dest, landing);
            }
        }
    }

    fn take_stairs(&mut self, stair_tile: Tile3D, dest: Tile3D, landing: (i8, i8)) {
        let from_floor = self.player.floor;
        self.player.floor = dest.floor;
        self.player.position = Vec2::new(
            dest.x as f32 + landing.0 as f32 * 0.42,
            dest.y as f32 + landing.1 as f32 * 0.42,
        );
        self.stair_cooldown = STAIR_COOLDOWN_TICKS;
        self.must_leave_stair = Some(stair_tile.pos());
        self.events.push(ActionEvent::FloorChanged {
            from_floor,
            to_floor: dest.floor,
            at: dest,
            via: FloorTransition::Stairs,
        });
    }

    fn compute_interact_action(&self) -> Option<InteractAction> {
        let p = self.player.position;
        let floor = self.player.floor;
        let facing = self.player.facing.normalized();

        // Priority 1: Unopened supply cache within reach.
        let mut best_cache: Option<(f32, u8, Tile3D)> = None;
        for c in self.block.caches() {
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
                if let TileKind::Door { open, .. } = self.block.tile_kind(t) {
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

        // Priority 3: Floor 1 broken window / collapsed exterior edge -> Drop down.
        if floor == 1 {
            let mut best_drop: Option<(f32, Tile3D, Tile3D)> = None;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let t = Tile3D::new(center_tile.x + dx, center_tile.y + dy, 1);
                    let d = p.distance(t.center());
                    if d <= INTERACT_FEATURE_RADIUS
                        && let Some(landing) = self.block.window_drop_target(t)
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
                        && self.block.tile_kind(t) == TileKind::BrokenWindow
                        && let Some(dest) = self.block.window_vault_target(center_tile, t)
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
                    && let Some((dest, landing)) = self.block.stair_link(t)
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
                if let Some(loot) = self.block.take_cache(id) {
                    self.inventory.collect_loot(loot, &mut self.events);
                    self.events.push(ActionEvent::Looted { at, loot });
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
                if let Some(opened) = self.block.toggle_door(tile) {
                    self.events
                        .push(ActionEvent::DoorToggled { at: tile, opened });
                    self.noises.push(NoiseEvent {
                        origin: tile.center(),
                        floor: tile.floor,
                        radius: 3.0,
                        priority: 2,
                        kind: NoiseKind::DoorToggle,
                    });
                }
            }
            InteractAction::DropWindow(_win, landing) => {
                let from_floor = self.player.floor;
                self.player.floor = landing.floor;
                self.player.position = landing.center();
                self.drop_recovery_ticks = DROP_RECOVERY_TICKS;
                self.stair_cooldown = 12;
                self.events.push(ActionEvent::FloorChanged {
                    from_floor,
                    to_floor: landing.floor,
                    at: landing,
                    via: FloorTransition::WindowDrop,
                });
                self.noises.push(NoiseEvent {
                    origin: landing.center(),
                    floor: landing.floor,
                    radius: 6.0,
                    priority: 2,
                    kind: NoiseKind::WindowDrop,
                });
            }
            InteractAction::ClimbWindow(win, dest) => {
                if self
                    .block
                    .circle_walkable(dest.center(), dest.floor, PLAYER_RADIUS, true)
                {
                    self.player.position = dest.center();
                    self.events.push(ActionEvent::WindowClimbed { at: win });
                    self.noises.push(NoiseEvent {
                        origin: win.center(),
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
        let collapsed = self.block.take_collapsed_tiles();
        if collapsed.is_empty() {
            return;
        }
        for c in collapsed {
            let center = c.center();
            self.events.push(ActionEvent::StructuralCollapse { at: c });
            self.noises.push(NoiseEvent {
                origin: center,
                floor: 0,
                radius: 16.0,
                priority: 4,
                kind: NoiseKind::Collapse,
            });

            // Player caught above or under collapsed overhang.
            if self.player.position.distance(center) <= 0.72 {
                if self.player.floor == 1 {
                    self.player.floor = 0;
                    self.events.push(ActionEvent::FloorChanged {
                        from_floor: 1,
                        to_floor: 0,
                        at: c,
                        via: FloorTransition::Collapse,
                    });
                }
                if self.hurt_iframes == 0 && self.health > 0 {
                    self.health = self.health.saturating_sub(1);
                    self.hurt_iframes = PLAYER_HURT_IFRAMES;
                    self.events.push(ActionEvent::PlayerHurt {
                        at: self.player.position,
                        floor: self.player.floor,
                        damage: 1,
                        remaining_hp: self.health,
                        from: center,
                    });
                }
            }

            // Monsters caught above or under collapse.
            for m in &mut self.monsters {
                if !m.is_alive() {
                    continue;
                }
                if m.pos.distance(center) <= 0.85 {
                    if m.floor == 1 {
                        m.floor = 0;
                        m.invalidate_path();
                    }
                    let _ = damage_monster(m, 2, Vec2::ZERO, 12, false, &mut self.events);
                }
            }
        }
    }

    // --- Public Query & Render API ---

    pub fn player_pose(&self) -> Pose {
        self.player
    }

    pub fn player_render_state(&self) -> PlayerRenderState {
        let interact = self.compute_interact_action();
        let p_tile = Tile3D::from_vec2(self.player.position, self.player.floor);
        PlayerRenderState {
            pose: self.player,
            elevation: self.player.floor as f32,
            equipped: self.inventory.equipped,
            ammo_in_equipped: self.inventory.equipped_ammo_total(),
            health: self.health,
            max_health: self.max_health,
            indoors: self.block.is_indoor(p_tile),
            quiet_walking: self.quiet_walking,
            near_interact_tile: interact.map(InteractAction::tile),
            interact_prompt: interact.map_or(InteractPrompt::None, InteractAction::prompt),
            rocket_danger_close: self.inventory.equipped == Some(Weapon::RocketLauncher)
                && is_rocket_danger_close(&self.block, self.player, &self.monsters),
        }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn block(&self) -> &ResidentialBlock {
        &self.block
    }

    pub fn house(&self) -> Option<House> {
        Some(self.block.starter_house())
    }

    pub fn buildings(&self) -> &[BuildingInfo] {
        self.block.buildings()
    }

    pub fn block_bounds(&self) -> Option<(Position, Position)> {
        let (min, max, _) = self.block.bounds();
        Some((min, max))
    }

    pub fn tile_render_info(&self, pos: Position, floor: Floor) -> Option<TileRenderInfo> {
        self.block.tile_render_info(pos, floor)
    }

    pub fn light_sources(&self) -> Vec<LightSourceView> {
        self.block.light_sources()
    }

    pub fn caches(&self) -> Vec<CacheView> {
        self.block.cache_views(
            self.player.position,
            self.player.floor,
            INTERACT_CACHE_RADIUS,
        )
    }

    pub fn has_cache_at(&self, p: Position) -> bool {
        self.block.caches().iter().any(|c| {
            !c.opened && c.pos.x == p.x && c.pos.y == p.y && c.pos.floor == self.player.floor
        })
    }

    pub fn near_loot(&self) -> bool {
        matches!(
            self.compute_interact_action(),
            Some(InteractAction::Search(..))
        )
    }

    /// Starter pistol cache position inside House 1.
    pub fn cache_position(&self) -> Position {
        self.block
            .caches()
            .get(1)
            .map_or_else(|| self.block.player_spawn().pos(), |c| c.pos.pos())
    }

    pub fn monster_views(&self) -> Vec<MonsterView> {
        self.monsters
            .iter()
            .filter(|m| m.is_alive() || m.is_swelling())
            .map(|m| m.to_view(&self.block))
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
        let caches_total = self.block.caches().len() as u8;
        let caches_found = self.block.caches().iter().filter(|c| c.opened).count() as u8;
        let objective_kind = match self.outcome {
            ActionOutcome::Won => ObjectiveKind::Won,
            ActionOutcome::Dead => ObjectiveKind::Dead,
            ActionOutcome::Playing => {
                if self.inventory.owned_mask == 0 {
                    ObjectiveKind::Scavenge
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
                && is_rocket_danger_close(&self.block, self.player, &self.monsters),
            indoors: self.block.is_indoor(p_tile),
            outcome: self.outcome,
        }
    }

    /// Spawn additional varied infected on walkable street/yard tiles for performance benchmarking.
    pub fn spawn_stress_monsters(&mut self, target_total: usize) {
        let (min, max, _) = self.block.bounds();
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
            if !self.block.is_walkable(tile, false) {
                continue;
            }
            if tile.center().distance(self.player.position) < 2.2 {
                continue;
            }
            let kind = kinds[(cursor as usize) % kinds.len()];
            let id = MonsterId(self.next_monster_id);
            self.next_monster_id += 1;
            self.monsters
                .push(Monster::new(id, kind, (cursor % 3) as u8, tile));
        }
        self.zombies_total = self.monsters.len().max(self.zombies_total);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let axe_pos = game.block.caches()[0].pos;
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
        assert_eq!(game.block.tile_kind(win_tile), TileKind::Window);
        game.player.position = Vec2::new(win_tile.x as f32, win_tile.y as f32 + 0.9);
        game.player.facing = Vec2::new(0.0, -1.0);
        game.step(ActionInput {
            fire: true,
            ..ActionInput::default()
        });
        assert_eq!(game.block.tile_kind(win_tile), TileKind::BrokenWindow);

        // Collect Pistol cache inside Starter House, switch to Pistol, kill last monster -> Won.
        let pistol_pos = game.block.caches()[1].pos;
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
        let origin = game.block.origin();
        // Starter House stairs are at local (12, 21).
        let stair_f0 = Tile3D::new(origin.x + 12, origin.y + 21, 0);
        assert_eq!(game.block.tile_kind(stair_f0), TileKind::StairsUp);

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
        assert_eq!(game.block.tile_kind(f1_win), TileKind::Window);
        let _ = game.block.damage_tile(f1_win, DamageKind::Axe);
        assert_eq!(game.block.tile_kind(f1_win), TileKind::BrokenWindow);

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
}
