//! Survival tools, firearms, melee, projectiles, and medkit system.

use super::ai::{self, Monster};
use super::types::{
    ActionEvent, CacheLoot, CombatWorld, DT, DamageKind, Floor, Material, MaterialCategory,
    NoiseEvent, NoiseKind, Pose, ProjectileId, ProjectileKind, ProjectileView, Tile3D,
    TileDamageResult, TileObstruction, Vec2, Weapon, mix64,
};

/// Cosine of half the 110° axe cleave arc (`cos(55°)`).
pub const AXE_CLEAVE_HALF_ANGLE_COS: f32 = 0.5736;

pub const UNARMED_SHOVE_RANGE: f32 = 0.95;
pub const UNARMED_SHOVE_KNOCKBACK: f32 = 0.15;
pub const UNARMED_SHOVE_COOLDOWN: u8 = 22;
pub const UNARMED_SHOVE_HITSTUN: u8 = 8;

pub const DRY_FIRE_COOLDOWN_TICKS: u8 = 18;

pub const MEDKIT_HEAL_AMOUNT: u8 = 3;
pub const MEDKIT_MAX_STACK: u8 = 5;
pub const MEDKIT_COOLDOWN_TICKS: u8 = 45;
pub const MEDKIT_ACTION_LOCK_TICKS: u8 = 12;
pub const MEDKIT_NOISE_RADIUS: f32 = 1.8;

pub const ROCKET_SPEED: f32 = 13.5;
pub const ROCKET_RADIUS: f32 = 0.16;
pub const ROCKET_BLAST_RADIUS: f32 = 2.4;
pub const ROCKET_BREACH_RADIUS: f32 = 1.65;
pub const ROCKET_SELF_CORE_RADIUS: f32 = 1.25;
pub const ROCKET_BLAST_NOISE_RADIUS: f32 = 24.0;
pub const ROCKET_MIN_DAMAGE: u8 = 4;

pub const SHOOTER_BOLT_SPEED: f32 = 10.5;
pub const SHOOTER_BOLT_RADIUS: f32 = 0.14;
pub const SHOOTER_BOLT_RANGE: f32 = 11.0;
pub const SHOOTER_BOLT_KNOCKBACK: f32 = 0.08;

pub const PLAYER_HURT_IFRAMES: u8 = 24;

const SHOTGUN_PELLET_ANGLES_DEG: [f32; 5] = [-14.0, -7.0, 0.0, 7.0, 14.0];
const SHOTGUN_JITTER_DEG: f32 = 1.1;
const AXE_TILE_RAY_ANGLES_DEG: [f32; 3] = [-30.0, 0.0, 30.0];
const UPPER_WINDOW_EXTRA_RANGE: f32 = 2.5;
const PROJECTILE_MAX_SUBSTEP: f32 = 0.10;
const MEDKIT_SLOW_MULT: f32 = 0.65;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReloadMode {
    None,
    Magazine,
    SingleShell,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponSpec {
    pub weapon: Weapon,
    pub fire_cooldown: u8,
    pub mag_capacity: u8,
    pub max_reserve: u16,
    pub reload_mode: ReloadMode,
    pub reload_ticks: u16,
    pub range: f32,
    pub pellets: u8,
    pub monster_damage: u8,
    pub knockback: f32,
    pub hitstun_ticks: u8,
    pub move_speed_mult: f32,
    pub fire_noise_radius: f32,
}

pub const WEAPON_SPECS: [WeaponSpec; Weapon::COUNT] = [
    // Slot 0: Axe
    WeaponSpec {
        weapon: Weapon::Axe,
        fire_cooldown: 26,
        mag_capacity: 0,
        max_reserve: 0,
        reload_mode: ReloadMode::None,
        reload_ticks: 0,
        range: 1.45,
        pellets: 0,
        monster_damage: 2,
        knockback: 0.22,
        hitstun_ticks: 12,
        move_speed_mult: 1.0,
        fire_noise_radius: 4.0,
    },
    // Slot 1: Pistol
    WeaponSpec {
        weapon: Weapon::Pistol,
        fire_cooldown: 15,
        mag_capacity: 8,
        max_reserve: 64,
        reload_mode: ReloadMode::Magazine,
        reload_ticks: 52,
        range: 10.5,
        pellets: 1,
        monster_damage: 2,
        knockback: 0.08,
        hitstun_ticks: 5,
        move_speed_mult: 1.0,
        fire_noise_radius: 12.0,
    },
    // Slot 2: Shotgun
    WeaponSpec {
        weapon: Weapon::Shotgun,
        fire_cooldown: 38,
        mag_capacity: 4,
        max_reserve: 24,
        reload_mode: ReloadMode::SingleShell,
        reload_ticks: 22,
        range: 6.8,
        pellets: 5,
        monster_damage: 1,
        knockback: 0.06,
        hitstun_ticks: 14,
        move_speed_mult: 0.96,
        fire_noise_radius: 16.0,
    },
    // Slot 3: RocketLauncher
    WeaponSpec {
        weapon: Weapon::RocketLauncher,
        fire_cooldown: 30,
        mag_capacity: 1,
        max_reserve: 6,
        reload_mode: ReloadMode::Magazine,
        reload_ticks: 96,
        range: 14.0,
        pellets: 1,
        monster_damage: 10,
        knockback: 0.34,
        hitstun_ticks: 20,
        move_speed_mult: 0.90,
        fire_noise_radius: 10.0,
    },
];

#[inline]
pub const fn weapon_spec(weapon: Weapon) -> &'static WeaponSpec {
    &WEAPON_SPECS[weapon.slot()]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReloadState {
    pub weapon: Weapon,
    pub ticks_left: u16,
    pub ticks_total: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Inventory {
    pub owned_mask: u8,
    pub equipped: Option<Weapon>,
    pub mag: [u8; Weapon::COUNT],
    pub reserve: [u16; Weapon::COUNT],
    pub medkits: u8,
    pub medkit_cooldown: u8,
    pub action_slow_ticks: u8,
    pub reload: Option<ReloadState>,
    pub fire_cooldown: u8,
    pub dry_fire_cooldown: u8,
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new()
    }
}

impl Inventory {
    pub fn new() -> Self {
        Self {
            owned_mask: 0,
            equipped: None,
            mag: [0; Weapon::COUNT],
            reserve: [0; Weapon::COUNT],
            medkits: 0,
            medkit_cooldown: 0,
            action_slow_ticks: 0,
            reload: None,
            fire_cooldown: 0,
            dry_fire_cooldown: 0,
        }
    }

    #[inline]
    pub fn has_weapon(&self, w: Weapon) -> bool {
        (self.owned_mask & w.bit()) != 0
    }

    pub fn collect_loot(&mut self, loot: CacheLoot, events: &mut Vec<ActionEvent>) {
        let mut newly_unlocked: Option<Weapon> = None;
        if let Some(w) = loot.weapon {
            if !self.has_weapon(w) {
                newly_unlocked = Some(w);
            }
            self.owned_mask |= w.bit();
            if self.equipped.is_none() {
                self.equipped = Some(w);
                events.push(ActionEvent::WeaponSwitched { weapon: Some(w) });
            }
        }

        self.add_ammo(
            Weapon::Pistol,
            loot.pistol_ammo,
            newly_unlocked == Some(Weapon::Pistol)
                || (loot.weapon == Some(Weapon::Pistol) && self.mag[Weapon::Pistol.slot()] == 0),
        );
        self.add_ammo(
            Weapon::Shotgun,
            loot.shotgun_ammo,
            newly_unlocked == Some(Weapon::Shotgun)
                || (loot.weapon == Some(Weapon::Shotgun) && self.mag[Weapon::Shotgun.slot()] == 0),
        );
        self.add_ammo(
            Weapon::RocketLauncher,
            loot.rockets,
            newly_unlocked == Some(Weapon::RocketLauncher)
                || (loot.weapon == Some(Weapon::RocketLauncher)
                    && self.mag[Weapon::RocketLauncher.slot()] == 0),
        );

        if loot.medkits > 0 {
            self.medkits = self
                .medkits
                .saturating_add(loot.medkits)
                .min(MEDKIT_MAX_STACK);
        }
    }

    fn add_ammo(&mut self, weapon: Weapon, amount: u16, fill_mag_first: bool) {
        let spec = weapon_spec(weapon);
        let slot = weapon.slot();
        if spec.mag_capacity == 0 {
            return;
        }
        if fill_mag_first {
            // Combine any previously stockpiled reserve with new ammo so picking up
            // a firearm after looting loose ammo starts with a loaded magazine.
            let total_available = self.reserve[slot].saturating_add(amount);
            let missing_in_mag = spec.mag_capacity.saturating_sub(self.mag[slot]) as u16;
            let to_mag = total_available.min(missing_in_mag) as u8;
            self.mag[slot] += to_mag;
            let leftover = total_available.saturating_sub(to_mag as u16);
            self.reserve[slot] = leftover.min(spec.max_reserve);
        } else if amount > 0 {
            self.reserve[slot] = self.reserve[slot]
                .saturating_add(amount)
                .min(spec.max_reserve);
        }
    }

    pub fn select_weapon(&mut self, weapon: Weapon, events: &mut Vec<ActionEvent>) -> bool {
        if !self.has_weapon(weapon) || self.equipped == Some(weapon) {
            return false;
        }
        self.reload = None;
        self.equipped = Some(weapon);
        events.push(ActionEvent::WeaponSwitched {
            weapon: Some(weapon),
        });
        true
    }

    pub fn cycle_weapon(&mut self, events: &mut Vec<ActionEvent>) -> bool {
        if self.owned_mask == 0 {
            return false;
        }
        let start_slot = self.equipped.map_or(Weapon::COUNT - 1, Weapon::slot);
        for offset in 1..=Weapon::COUNT {
            let slot = (start_slot + offset) % Weapon::COUNT;
            let candidate = Weapon::ALL[slot];
            if self.has_weapon(candidate) && self.equipped != Some(candidate) {
                return self.select_weapon(candidate, events);
            }
        }
        false
    }

    pub fn can_reload(&self) -> bool {
        if self.reload.is_some() {
            return false;
        }
        let Some(weapon) = self.equipped else {
            return false;
        };
        let spec = weapon_spec(weapon);
        if spec.reload_mode == ReloadMode::None {
            return false;
        }
        let slot = weapon.slot();
        self.mag[slot] < spec.mag_capacity && self.reserve[slot] > 0
    }

    pub fn try_start_reload(&mut self, events: &mut Vec<ActionEvent>) -> bool {
        if !self.can_reload() {
            return false;
        }
        let weapon = self.equipped.expect("can_reload guarantees equipped");
        let spec = weapon_spec(weapon);
        self.reload = Some(ReloadState {
            weapon,
            ticks_left: spec.reload_ticks,
            ticks_total: spec.reload_ticks,
        });
        events.push(ActionEvent::ReloadStarted {
            weapon,
            duration_ticks: spec.reload_ticks,
        });
        true
    }

    pub fn tick_timers(&mut self, events: &mut Vec<ActionEvent>) {
        self.fire_cooldown = self.fire_cooldown.saturating_sub(1);
        self.dry_fire_cooldown = self.dry_fire_cooldown.saturating_sub(1);
        self.medkit_cooldown = self.medkit_cooldown.saturating_sub(1);
        self.action_slow_ticks = self.action_slow_ticks.saturating_sub(1);

        let Some(mut state) = self.reload else {
            return;
        };
        if self.equipped != Some(state.weapon) {
            self.reload = None;
            return;
        }
        if state.ticks_left > 1 {
            state.ticks_left -= 1;
            self.reload = Some(state);
            return;
        }

        let spec = weapon_spec(state.weapon);
        let slot = state.weapon.slot();
        match spec.reload_mode {
            ReloadMode::None => {
                self.reload = None;
            }
            ReloadMode::Magazine => {
                let needed = spec.mag_capacity.saturating_sub(self.mag[slot]) as u16;
                let loaded = needed.min(self.reserve[slot]) as u8;
                self.mag[slot] += loaded;
                self.reserve[slot] -= loaded as u16;
                self.reload = None;
                if loaded > 0 {
                    events.push(ActionEvent::ReloadCompleted {
                        weapon: state.weapon,
                        loaded,
                    });
                }
            }
            ReloadMode::SingleShell => {
                if self.mag[slot] < spec.mag_capacity && self.reserve[slot] > 0 {
                    self.mag[slot] += 1;
                    self.reserve[slot] -= 1;
                    events.push(ActionEvent::ReloadCompleted {
                        weapon: state.weapon,
                        loaded: 1,
                    });
                }
                if self.mag[slot] < spec.mag_capacity && self.reserve[slot] > 0 {
                    self.reload = Some(ReloadState {
                        weapon: state.weapon,
                        ticks_left: spec.reload_ticks,
                        ticks_total: spec.reload_ticks,
                    });
                } else {
                    self.reload = None;
                }
            }
        }
    }

    pub fn reload_progress_pct(&self) -> u8 {
        let Some(state) = self.reload else {
            return 0;
        };
        if state.ticks_total == 0 {
            return 100;
        }
        let elapsed = state.ticks_total.saturating_sub(state.ticks_left) as u32;
        ((elapsed * 100) / state.ticks_total as u32).min(100) as u8
    }

    pub fn move_speed_mult(&self) -> f32 {
        let weapon_mult = self
            .equipped
            .map_or(1.0, |w| weapon_spec(w).move_speed_mult);
        if self.action_slow_ticks > 0 {
            weapon_mult * MEDKIT_SLOW_MULT
        } else {
            weapon_mult
        }
    }

    pub fn equipped_ammo_total(&self) -> u16 {
        match self.equipped {
            None | Some(Weapon::Axe) => 0,
            Some(w) => {
                let slot = w.slot();
                self.mag[slot] as u16 + self.reserve[slot]
            }
        }
    }

    pub fn can_heal(&self, health: u8, max_health: u8) -> bool {
        self.medkits > 0 && self.medkit_cooldown == 0 && health > 0 && health < max_health
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_use_medkit(
        &mut self,
        health: &mut u8,
        max_health: u8,
        player_pos: Vec2,
        player_floor: Floor,
        events: &mut Vec<ActionEvent>,
        noises: &mut Vec<NoiseEvent>,
    ) -> bool {
        if !self.can_heal(*health, max_health) {
            return false;
        }
        let before = *health;
        *health = before.saturating_add(MEDKIT_HEAL_AMOUNT).min(max_health);
        let amount = *health - before;
        self.medkits -= 1;
        self.medkit_cooldown = MEDKIT_COOLDOWN_TICKS;
        self.action_slow_ticks = self.action_slow_ticks.max(MEDKIT_ACTION_LOCK_TICKS);
        self.fire_cooldown = self.fire_cooldown.max(MEDKIT_ACTION_LOCK_TICKS);
        events.push(ActionEvent::Healed {
            at: player_pos,
            floor: player_floor,
            amount,
            new_health: *health,
        });
        noises.push(NoiseEvent {
            origin: player_pos,
            floor: player_floor,
            radius: MEDKIT_NOISE_RADIUS,
            priority: 1,
            kind: NoiseKind::MedkitUse,
        });
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projectile {
    pub id: ProjectileId,
    pub kind: ProjectileKind,
    pub owner_is_player: bool,
    pub pos: Vec2,
    pub dir: Vec2,
    pub floor: Floor,
    pub z: f32,
    pub vz: f32,
    pub speed: f32,
    pub radius: f32,
    pub remaining_range: f32,
}

impl Projectile {
    #[allow(clippy::wrong_self_convention)]
    pub fn to_view(&self) -> ProjectileView {
        ProjectileView {
            id: self.id,
            kind: self.kind,
            pos: self.pos,
            dir: self.dir,
            floor: self.floor,
            z: self.z,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn try_fire_or_swing(
    world: &mut dyn CombatWorld,
    inventory: &mut Inventory,
    player: Pose,
    monsters: &mut [Monster],
    projectiles: &mut Vec<Projectile>,
    next_proj_id: &mut u32,
    seed: u64,
    tick: u64,
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
) {
    if inventory.fire_cooldown > 0 {
        return;
    }

    let facing = {
        let f = player.facing.normalized();
        if f.length_squared() > 0.01 {
            f
        } else {
            Vec2::new(0.0, -1.0)
        }
    };

    let Some(weapon) = inventory.equipped else {
        execute_unarmed_shove(world, inventory, player, facing, monsters, events);
        return;
    };

    if weapon == Weapon::Axe {
        execute_axe_swing(world, inventory, player, facing, monsters, events, noises);
        return;
    }

    let spec = weapon_spec(weapon);
    let slot = weapon.slot();

    // Check active reload: SingleShell can be interrupted to fire as soon as mag > 0.
    if let Some(reload) = inventory.reload {
        if reload.weapon == weapon {
            if spec.reload_mode == ReloadMode::SingleShell && inventory.mag[slot] > 0 {
                inventory.reload = None;
            } else {
                return;
            }
        } else {
            inventory.reload = None;
        }
    }

    if inventory.mag[slot] == 0 {
        if inventory.reserve[slot] > 0 {
            inventory.try_start_reload(events);
        } else if inventory.dry_fire_cooldown == 0 {
            inventory.dry_fire_cooldown = DRY_FIRE_COOLDOWN_TICKS;
            events.push(ActionEvent::DryFire { weapon });
        }
        return;
    }

    inventory.mag[slot] -= 1;
    inventory.fire_cooldown = spec.fire_cooldown;

    events.push(ActionEvent::ShotFired {
        weapon,
        from: player.position,
        floor: player.floor,
        facing,
    });

    match weapon {
        Weapon::Pistol => {
            world.register_dynamic_flash(
                player.position + facing * 0.35,
                player.floor,
                3.2,
                130,
                3,
            );
            noises.push(NoiseEvent {
                origin: player.position,
                floor: player.floor,
                radius: spec.fire_noise_radius,
                priority: 4,
                kind: NoiseKind::PistolShot,
            });
            let hit = trace_bullet_ray(
                world,
                player.position,
                player.floor,
                facing,
                spec.range,
                DamageKind::Bullet,
                monsters,
                events,
                noises,
            );
            events.push(ActionEvent::BulletTrace {
                weapon: Weapon::Pistol,
                from: player.position,
                from_z: player.floor as f32,
                to: hit.end_pos,
                to_z: hit.end_z,
                hit_material: hit.hit_material,
            });
            if let Some(idx) = hit.hit_monster {
                let _ = ai::damage_monster(
                    &mut monsters[idx],
                    spec.monster_damage,
                    facing * spec.knockback,
                    spec.hitstun_ticks,
                    false,
                    events,
                );
            }
        }
        Weapon::Shotgun => {
            world.register_dynamic_flash(player.position + facing * 0.4, player.floor, 4.5, 175, 4);
            noises.push(NoiseEvent {
                origin: player.position,
                floor: player.floor,
                radius: spec.fire_noise_radius,
                priority: 5,
                kind: NoiseKind::ShotgunBlast,
            });

            // Aggregate pellet hits per monster on the stack (at most 5 distinct targets).
            let mut hit_targets: [(usize, u8, Vec2); 5] = [(0, 0, Vec2::ZERO); 5];
            let mut hit_target_count = 0usize;

            for (pellet_idx, base_deg) in SHOTGUN_PELLET_ANGLES_DEG.iter().enumerate() {
                let h = mix64(
                    seed ^ tick.wrapping_mul(0x9e37_79b9_7f4a_7c15)
                        ^ (pellet_idx as u64).wrapping_mul(0x517c_c1b7_2722_0a95),
                );
                let unit = ((h & 0xffff) as f32 / 65535.0) * 2.0 - 1.0;
                let angle_rad = (*base_deg + unit * SHOTGUN_JITTER_DEG).to_radians();
                let dir = facing.rotate(angle_rad).normalized();

                let hit = trace_bullet_ray(
                    world,
                    player.position,
                    player.floor,
                    dir,
                    spec.range,
                    DamageKind::ShotgunPellet,
                    monsters,
                    events,
                    noises,
                );
                events.push(ActionEvent::BulletTrace {
                    weapon: Weapon::Shotgun,
                    from: player.position,
                    from_z: player.floor as f32,
                    to: hit.end_pos,
                    to_z: hit.end_z,
                    hit_material: hit.hit_material,
                });

                if let Some(m_idx) = hit.hit_monster {
                    let kb = dir * spec.knockback;
                    if let Some(entry) = hit_targets[..hit_target_count]
                        .iter_mut()
                        .find(|(idx, _, _)| *idx == m_idx)
                    {
                        entry.1 = entry.1.saturating_add(1);
                        entry.2 += kb;
                    } else if hit_target_count < hit_targets.len() {
                        hit_targets[hit_target_count] = (m_idx, 1, kb);
                        hit_target_count += 1;
                    }
                }
            }

            for &(m_idx, pellets_hit, kb_sum) in &hit_targets[..hit_target_count] {
                let total_damage = pellets_hit.saturating_mul(spec.monster_damage);
                let _ = ai::damage_monster(
                    &mut monsters[m_idx],
                    total_damage,
                    kb_sum,
                    spec.hitstun_ticks,
                    false,
                    events,
                );
            }
        }
        Weapon::RocketLauncher => {
            world.register_dynamic_flash(player.position + facing * 0.4, player.floor, 4.0, 160, 5);
            noises.push(NoiseEvent {
                origin: player.position,
                floor: player.floor,
                radius: spec.fire_noise_radius,
                priority: 4,
                kind: NoiseKind::RocketLaunch,
            });

            let vz = compute_rocket_launch_vz(world, player, facing, spec.range, monsters);
            let id = ProjectileId(*next_proj_id);
            *next_proj_id = next_proj_id.wrapping_add(1);
            projectiles.push(Projectile {
                id,
                kind: ProjectileKind::Rocket,
                owner_is_player: true,
                pos: player.position,
                dir: facing,
                floor: player.floor,
                z: player.floor as f32,
                vz,
                speed: ROCKET_SPEED,
                radius: ROCKET_RADIUS,
                remaining_range: spec.range,
            });
            events.push(ActionEvent::ProjectileLaunched {
                id,
                kind: ProjectileKind::Rocket,
                from: player.position,
                floor: player.floor,
                dir: facing,
            });
        }
        Weapon::Axe => unreachable!(),
    }
}

fn execute_unarmed_shove(
    world: &dyn CombatWorld,
    inventory: &mut Inventory,
    player: Pose,
    facing: Vec2,
    monsters: &mut [Monster],
    events: &mut Vec<ActionEvent>,
) {
    inventory.fire_cooldown = UNARMED_SHOVE_COOLDOWN;
    events.push(ActionEvent::UnarmedShove {
        from: player.position,
        floor: player.floor,
        facing,
    });
    for monster in monsters.iter_mut() {
        if !monster.is_targetable() || monster.floor != player.floor {
            continue;
        }
        let offset = monster.pos - player.position;
        let dist = offset.length();
        if dist > UNARMED_SHOVE_RANGE + monster.radius() {
            continue;
        }
        let dir = if dist > 0.001 {
            offset * (1.0 / dist)
        } else {
            facing
        };
        if dist > 0.45 && dir.dot(facing) < 0.35 {
            continue;
        }
        if ai::segment_blocked_by_solid(world, player.position, monster.pos, player.floor) {
            continue;
        }
        monster.apply_shove(
            dir * UNARMED_SHOVE_KNOCKBACK,
            UNARMED_SHOVE_HITSTUN,
            player.position,
            player.floor,
            events,
        );
    }
}

fn execute_axe_swing(
    world: &mut dyn CombatWorld,
    inventory: &mut Inventory,
    player: Pose,
    facing: Vec2,
    monsters: &mut [Monster],
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
) {
    let spec = weapon_spec(Weapon::Axe);
    inventory.fire_cooldown = spec.fire_cooldown;

    let swing_event_idx = events.len();
    events.push(ActionEvent::AxeSwing {
        from: player.position,
        floor: player.floor,
        facing,
        hit_flesh: false,
        hit_solid: false,
    });

    let mut hit_flesh = false;
    let mut hit_solid = false;

    for monster in monsters.iter_mut() {
        if !monster.is_targetable() || monster.floor != player.floor {
            continue;
        }
        let offset = monster.pos - player.position;
        let dist = offset.length();
        if dist > spec.range + monster.radius() {
            continue;
        }
        let dir = if dist > 0.001 {
            offset * (1.0 / dist)
        } else {
            facing
        };
        if dist >= 0.15 && dir.dot(facing) < AXE_CLEAVE_HALF_ANGLE_COS {
            continue;
        }
        if ai::segment_blocked_by_solid(world, player.position, monster.pos, player.floor) {
            continue;
        }
        hit_flesh = true;
        let _ = ai::damage_monster(
            monster,
            spec.monster_damage,
            dir * spec.knockback,
            spec.hitstun_ticks,
            false,
            events,
        );
    }

    // Cast 3 short rays (-30°, 0°, +30°) to damage up to 2 distinct destructible tiles.
    let mut hit_tiles: [(Tile3D, Vec2); 2] = [(Tile3D::default(), Vec2::ZERO); 2];
    let mut hit_tile_count = 0usize;

    for angle_deg in AXE_TILE_RAY_ANGLES_DEG {
        if hit_tile_count >= hit_tiles.len() {
            break;
        }
        let dir = facing.rotate(angle_deg.to_radians()).normalized();
        if let Some((tile, hit_pos)) =
            cast_axe_tile_ray(world, player.position, player.floor, dir, spec.range)
        {
            if !hit_tiles[..hit_tile_count]
                .iter()
                .any(|(existing, _)| *existing == tile)
            {
                hit_tiles[hit_tile_count] = (tile, hit_pos);
                hit_tile_count += 1;
            }
        }
    }

    for &(tile, hit_pos) in &hit_tiles[..hit_tile_count] {
        if let Some(res) = world.damage_tile(tile, DamageKind::Axe) {
            if res.damaged {
                hit_solid = true;
                emit_tile_damage_effects(tile, hit_pos, res, events, noises);
            }
        }
    }

    events[swing_event_idx] = ActionEvent::AxeSwing {
        from: player.position,
        floor: player.floor,
        facing,
        hit_flesh,
        hit_solid,
    };

    noises.push(NoiseEvent {
        origin: player.position,
        floor: player.floor,
        radius: spec.fire_noise_radius,
        priority: 2,
        kind: NoiseKind::AxeSwing,
    });
}

fn cast_axe_tile_ray(
    world: &dyn CombatWorld,
    from: Vec2,
    floor: Floor,
    dir: Vec2,
    range: f32,
) -> Option<(Tile3D, Vec2)> {
    let start_tile = Tile3D::from_vec2(from, floor);
    let steps = (range / 0.08).ceil() as usize;
    for i in 1..=steps {
        let t = (i as f32 * 0.08).min(range);
        let p = from + dir * t;
        let tile = Tile3D::from_vec2(p, floor);
        if tile == start_tile {
            continue;
        }
        if !in_world_bounds(world, tile) {
            return None;
        }
        match world.obstruction(tile) {
            TileObstruction::Open | TileObstruction::BrokenWindow => {}
            TileObstruction::IntactWindow
            | TileObstruction::DoorClosed
            | TileObstruction::LowProp(_)
            | TileObstruction::Wall(_) => return Some((tile, p)),
        }
    }
    None
}

fn compute_rocket_launch_vz(
    world: &dyn CombatWorld,
    player: Pose,
    facing: Vec2,
    range: f32,
    monsters: &[Monster],
) -> f32 {
    if player.floor == 0 {
        return 0.0;
    }
    // Check if the player is aiming out an upper-story window / open edge into exterior air.
    let start_tile = Tile3D::from_vec2(player.position, player.floor);
    let mut exits_into_air = false;
    let probe_steps = 40usize; // 4.0 tiles ahead
    for i in 1..=probe_steps {
        let t = i as f32 * 0.10;
        let p = player.position + facing * t;
        let tile = Tile3D::from_vec2(p, player.floor);
        if tile == start_tile {
            continue;
        }
        if !in_world_bounds(world, tile) {
            break;
        }
        if !world.has_floor(tile) {
            exits_into_air = true;
            break;
        }
        match world.obstruction(tile) {
            TileObstruction::Wall(_)
            | TileObstruction::DoorClosed
            | TileObstruction::LowProp(_)
            | TileObstruction::IntactWindow => break,
            TileObstruction::Open | TileObstruction::BrokenWindow => {}
        }
    }
    if !exits_into_air {
        return 0.0;
    }

    // If an outdoor Floor 0 monster lies near the aim line, slope toward its distance.
    let mut best_along: Option<f32> = None;
    for m in monsters {
        if !m.is_targetable() || m.floor != 0 {
            continue;
        }
        let offset = m.pos - player.position;
        let along = offset.dot(facing);
        if !(1.5..=range).contains(&along) {
            continue;
        }
        let side = offset.perp_dot(facing).abs();
        if side <= 1.25 && best_along.is_none_or(|best| along < best) {
            best_along = Some(along);
        }
    }

    if let Some(along) = best_along {
        let flight_t = (along / ROCKET_SPEED).max(0.15);
        (-(player.floor as f32) / flight_t).clamp(-ROCKET_SPEED * 0.45, -ROCKET_SPEED * 0.10)
    } else {
        -ROCKET_SPEED * 0.22
    }
}

struct RayHit {
    end_pos: Vec2,
    end_z: f32,
    hit_material: MaterialCategory,
    hit_monster: Option<usize>,
}

#[allow(clippy::too_many_arguments)]
fn trace_bullet_ray(
    world: &mut dyn CombatWorld,
    from: Vec2,
    from_floor: Floor,
    dir: Vec2,
    base_range: f32,
    damage_kind: DamageKind,
    monsters: &[Monster],
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
) -> RayHit {
    let sx = from.x + 0.5;
    let sy = from.y + 0.5;
    let mut tx = sx.floor() as i32;
    let mut ty = sy.floor() as i32;

    let (step_x, mut t_max_x, t_delta_x) = if dir.x > 1e-6 {
        (1, ((tx + 1) as f32 - sx) / dir.x, 1.0 / dir.x)
    } else if dir.x < -1e-6 {
        (-1, (sx - tx as f32) / -dir.x, 1.0 / -dir.x)
    } else {
        (0, f32::INFINITY, f32::INFINITY)
    };

    let (step_y, mut t_max_y, t_delta_y) = if dir.y > 1e-6 {
        (1, ((ty + 1) as f32 - sy) / dir.y, 1.0 / dir.y)
    } else if dir.y < -1e-6 {
        (-1, (sy - ty as f32) / -dir.y, 1.0 / -dir.y)
    } else {
        (0, f32::INFINITY, f32::INFINITY)
    };

    let mut cur_floor = from_floor;
    let mut max_range = base_range;
    let mut t_prev = 0.0_f32;

    for _ in 0..128 {
        let next_t = t_max_x.min(t_max_y);
        let seg_end = next_t.min(max_range);

        // Check monsters along [t_prev, seg_end] on the active floor before terrain hits.
        if let Some((m_idx, m_t)) =
            closest_monster_on_ray(monsters, cur_floor, from, dir, t_prev, seg_end)
        {
            return RayHit {
                end_pos: from + dir * m_t,
                end_z: cur_floor as f32,
                hit_material: MaterialCategory::Flesh,
                hit_monster: Some(m_idx),
            };
        }

        if next_t > max_range {
            return RayHit {
                end_pos: from + dir * max_range,
                end_z: cur_floor as f32,
                hit_material: MaterialCategory::None,
                hit_monster: None,
            };
        }

        // Corner-safe DDA step: if crossing an exact grid corner, block if either side is solid.
        if (t_max_x - t_max_y).abs() < 1e-5 && step_x != 0 && step_y != 0 {
            let side_a = Tile3D::new(tx + step_x, ty, cur_floor);
            let side_b = Tile3D::new(tx, ty + step_y, cur_floor);
            let a_solid = matches!(
                world.obstruction(side_a),
                TileObstruction::Wall(_) | TileObstruction::DoorClosed
            );
            let b_solid = matches!(
                world.obstruction(side_b),
                TileObstruction::Wall(_) | TileObstruction::DoorClosed
            );
            if a_solid || b_solid {
                let hit_tile = if a_solid { side_a } else { side_b };
                let hit_pos = from + dir * next_t;
                let mat =
                    apply_bullet_tile_hit(world, hit_tile, hit_pos, damage_kind, events, noises);
                return RayHit {
                    end_pos: hit_pos,
                    end_z: cur_floor as f32,
                    hit_material: mat,
                    hit_monster: None,
                };
            }
            tx += step_x;
            ty += step_y;
            t_max_x += t_delta_x;
            t_max_y += t_delta_y;
        } else if t_max_x < t_max_y {
            tx += step_x;
            t_max_x += t_delta_x;
        } else {
            ty += step_y;
            t_max_y += t_delta_y;
        }

        t_prev = next_t;
        let hit_pos = from + dir * next_t;
        let mut tile = Tile3D::new(tx, ty, cur_floor);

        if !in_world_bounds(world, tile) {
            return RayHit {
                end_pos: hit_pos,
                end_z: cur_floor as f32,
                hit_material: MaterialCategory::None,
                hit_monster: None,
            };
        }

        // Upper-floor shots exiting through an open/shattered upper window into exterior air
        // automatically angle downward to engage Floor 0 outdoor targets & terrain.
        if cur_floor > 0 && !world.has_floor(tile) {
            cur_floor = 0;
            max_range = base_range + UPPER_WINDOW_EXTRA_RANGE;
            tile = Tile3D::new(tx, ty, 0);
        }

        match world.obstruction(tile) {
            TileObstruction::Open | TileObstruction::BrokenWindow => {}
            TileObstruction::IntactWindow => {
                // Shatter the window and let the bullet keep traveling!
                let _ = apply_bullet_tile_hit(world, tile, hit_pos, damage_kind, events, noises);
            }
            TileObstruction::DoorClosed
            | TileObstruction::LowProp(_)
            | TileObstruction::Wall(_) => {
                let mat = apply_bullet_tile_hit(world, tile, hit_pos, damage_kind, events, noises);
                return RayHit {
                    end_pos: hit_pos,
                    end_z: cur_floor as f32,
                    hit_material: mat,
                    hit_monster: None,
                };
            }
        }
    }

    RayHit {
        end_pos: from + dir * max_range,
        end_z: cur_floor as f32,
        hit_material: MaterialCategory::None,
        hit_monster: None,
    }
}

fn apply_bullet_tile_hit(
    world: &mut dyn CombatWorld,
    tile: Tile3D,
    hit_pos: Vec2,
    damage_kind: DamageKind,
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
) -> MaterialCategory {
    let fallback_mat = match world.obstruction(tile) {
        TileObstruction::Wall(m) | TileObstruction::LowProp(m) => m.category(),
        TileObstruction::DoorClosed => MaterialCategory::Wood,
        TileObstruction::IntactWindow => MaterialCategory::Glass,
        TileObstruction::Open | TileObstruction::BrokenWindow => MaterialCategory::None,
    };
    if let Some(res) = world.damage_tile(tile, damage_kind) {
        if res.damaged {
            emit_tile_damage_effects(tile, hit_pos, res, events, noises);
        }
        let cat = res.material.category();
        if cat != MaterialCategory::None {
            return cat;
        }
    }
    fallback_mat
}

fn closest_monster_on_ray(
    monsters: &[Monster],
    floor: Floor,
    from: Vec2,
    dir: Vec2,
    t_min: f32,
    t_max: f32,
) -> Option<(usize, f32)> {
    let mut best: Option<(usize, f32)> = None;
    for (i, m) in monsters.iter().enumerate() {
        if !m.is_targetable() || m.floor != floor {
            continue;
        }
        let r = m.radius() + 0.05;
        let oc = m.pos - from;
        let proj = oc.dot(dir);
        let perp_sq = oc.length_squared() - proj * proj;
        if perp_sq > r * r {
            continue;
        }
        let half_chord = (r * r - perp_sq).max(0.0).sqrt();
        if proj + half_chord < t_min {
            continue;
        }
        let t_hit = (proj - half_chord).max(t_min);
        if t_hit <= t_max && best.is_none_or(|(_, best_t)| t_hit < best_t) {
            best = Some((i, t_hit));
        }
    }
    best
}

#[allow(clippy::too_many_arguments)]
pub fn step_projectiles(
    world: &mut dyn CombatWorld,
    projectiles: &mut Vec<Projectile>,
    player_pose: Pose,
    player_radius: f32,
    player_health: &mut u8,
    player_iframes: &mut u8,
    player_knockback: &mut Vec2,
    monsters: &mut [Monster],
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
) {
    let (_, _, num_floors) = world.bounds();
    let max_floor = num_floors.saturating_sub(1);

    let mut idx = 0usize;
    while idx < projectiles.len() {
        let mut proj = projectiles[idx];
        let total_dist = (proj.speed * DT).min(proj.remaining_range.max(0.0));
        let substeps = ((total_dist / PROJECTILE_MAX_SUBSTEP).ceil() as usize).max(1);
        let sub_dist = total_dist / substeps as f32;
        let sub_dt = if proj.speed > 0.001 {
            sub_dist / proj.speed
        } else {
            0.0
        };

        let mut remove = false;
        let mut last_shattered_window: Option<Tile3D> = None;

        for _ in 0..substeps {
            proj.pos += proj.dir * sub_dist;
            proj.z += proj.vz * sub_dt;
            proj.remaining_range -= sub_dist;

            // Ground impact check (upper-floor angled rockets / bolts hitting Floor 0).
            if proj.z <= 0.0 && proj.vz < 0.0 {
                proj.z = 0.0;
                proj.floor = 0;
                if proj.kind == ProjectileKind::Rocket {
                    detonate_rocket(
                        world,
                        proj.pos,
                        0,
                        player_pose,
                        player_health,
                        player_iframes,
                        player_knockback,
                        monsters,
                        events,
                        noises,
                    );
                }
                remove = true;
                break;
            }

            // Bolt flying well above the top floor disappears cleanly.
            if proj.kind == ProjectileKind::ShooterBolt
                && proj.vz > 0.0
                && proj.z > max_floor as f32 + 0.6
            {
                remove = true;
                break;
            }

            proj.floor = proj.z.round().clamp(0.0, max_floor as f32) as Floor;
            let tile = Tile3D::from_vec2(proj.pos, proj.floor);

            if !in_world_bounds(world, tile) {
                if proj.kind == ProjectileKind::Rocket {
                    detonate_rocket(
                        world,
                        proj.pos,
                        proj.floor,
                        player_pose,
                        player_health,
                        player_iframes,
                        player_knockback,
                        monsters,
                        events,
                        noises,
                    );
                }
                remove = true;
                break;
            }

            match proj.kind {
                ProjectileKind::Rocket => {
                    // If a horizontal upper-floor rocket exits into exterior air (e.g. through a breached wall),
                    // angle it downward toward Floor 0.
                    if proj.floor > 0 && !world.has_floor(tile) && proj.vz == 0.0 {
                        proj.vz = -ROCKET_SPEED * 0.22;
                    }

                    let hit_terrain = matches!(
                        world.obstruction(tile),
                        TileObstruction::Wall(_)
                            | TileObstruction::DoorClosed
                            | TileObstruction::LowProp(_)
                            | TileObstruction::IntactWindow
                    );
                    let hit_monster = monsters.iter().any(|m| {
                        m.is_alive()
                            && m.floor == proj.floor
                            && m.pos.distance(proj.pos) <= m.radius() + proj.radius
                    });

                    if hit_terrain || hit_monster {
                        detonate_rocket(
                            world,
                            proj.pos,
                            proj.floor,
                            player_pose,
                            player_health,
                            player_iframes,
                            player_knockback,
                            monsters,
                            events,
                            noises,
                        );
                        remove = true;
                        break;
                    }
                }
                ProjectileKind::ShooterBolt => {
                    match world.obstruction(tile) {
                        TileObstruction::Wall(_)
                        | TileObstruction::DoorClosed
                        | TileObstruction::LowProp(_) => {
                            let _ = apply_bullet_tile_hit(
                                world,
                                tile,
                                proj.pos,
                                DamageKind::Bullet,
                                events,
                                noises,
                            );
                            remove = true;
                            break;
                        }
                        TileObstruction::IntactWindow => {
                            if last_shattered_window != Some(tile) {
                                last_shattered_window = Some(tile);
                                let _ = apply_bullet_tile_hit(
                                    world,
                                    tile,
                                    proj.pos,
                                    DamageKind::Bullet,
                                    events,
                                    noises,
                                );
                            }
                        }
                        TileObstruction::Open | TileObstruction::BrokenWindow => {}
                    }

                    if !proj.owner_is_player
                        && proj.floor == player_pose.floor
                        && player_pose.position.distance(proj.pos) <= player_radius + proj.radius
                    {
                        if *player_iframes == 0 {
                            *player_health = player_health.saturating_sub(1);
                            *player_iframes = PLAYER_HURT_IFRAMES;
                            *player_knockback += proj.dir.normalized() * SHOOTER_BOLT_KNOCKBACK;
                            events.push(ActionEvent::PlayerHurt {
                                at: player_pose.position,
                                floor: player_pose.floor,
                                damage: 1,
                                remaining_hp: *player_health,
                                from: proj.pos,
                            });
                        }
                        remove = true;
                        break;
                    }
                }
            }
        }

        if !remove && proj.remaining_range <= 0.001 {
            if proj.kind == ProjectileKind::Rocket {
                detonate_rocket(
                    world,
                    proj.pos,
                    proj.floor,
                    player_pose,
                    player_health,
                    player_iframes,
                    player_knockback,
                    monsters,
                    events,
                    noises,
                );
            }
            remove = true;
        }

        if remove {
            projectiles.remove(idx);
        } else {
            projectiles[idx] = proj;
            idx += 1;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn detonate_rocket(
    world: &mut dyn CombatWorld,
    center: Vec2,
    floor: Floor,
    player_pose: Pose,
    player_health: &mut u8,
    player_iframes: &mut u8,
    player_knockback: &mut Vec2,
    monsters: &mut [Monster],
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
) {
    let spec = weapon_spec(Weapon::RocketLauncher);

    world.register_dynamic_flash(center, floor, 7.5, 220, 10);
    events.push(ActionEvent::RocketBlast {
        at: center,
        floor,
        radius: ROCKET_BLAST_RADIUS,
    });
    noises.push(NoiseEvent {
        origin: center,
        floor,
        radius: ROCKET_BLAST_NOISE_RADIUS,
        priority: 6,
        kind: NoiseKind::RocketBlast,
    });

    // Damage terrain: core breach within ROCKET_BREACH_RADIUS, splash out to ROCKET_BLAST_RADIUS.
    let cx = center.x.round() as i32;
    let cy = center.y.round() as i32;
    for dy in -3..=3 {
        for dx in -3..=3 {
            let tile = Tile3D::new(cx + dx, cy + dy, floor);
            if !in_world_bounds(world, tile) {
                continue;
            }
            let dist = center.distance(tile.center());
            if dist > ROCKET_BLAST_RADIUS {
                continue;
            }
            let kind = if dist <= ROCKET_BREACH_RADIUS {
                DamageKind::RocketBlast
            } else {
                DamageKind::RocketSplash
            };
            if let Some(res) = world.damage_tile(tile, kind) {
                if res.damaged {
                    emit_tile_damage_effects(tile, tile.center(), res, events, noises);
                }
            }
        }
    }

    // Damage monsters with smooth falloff (10 core -> 4 rim at 2.4 tiles).
    let mut chain_bloaters: Vec<usize> = Vec::new();
    for (i, m) in monsters.iter_mut().enumerate() {
        if !m.is_alive() || m.floor != floor {
            continue;
        }
        let dist = m.pos.distance(center);
        if dist > ROCKET_BLAST_RADIUS + m.radius() {
            continue;
        }
        let t = (dist / ROCKET_BLAST_RADIUS).clamp(0.0, 1.0);
        let max_dmg = spec.monster_damage as f32;
        let min_dmg = ROCKET_MIN_DAMAGE as f32;
        let dmg = (max_dmg - (max_dmg - min_dmg) * t)
            .round()
            .clamp(min_dmg, max_dmg) as u8;
        let dir = if dist > 0.001 {
            (m.pos - center) * (1.0 / dist)
        } else {
            Vec2::new(1.0, 0.0)
        };
        let kb = dir * (spec.knockback * (1.0 - 0.5 * t));
        if ai::damage_monster(m, dmg, kb, spec.hitstun_ticks, true, events) {
            chain_bloaters.push(i);
        }
    }

    // Fair player self-damage (2 core inside 1.25, 1 in 1.25..=2.4) respecting iframes.
    if player_pose.floor == floor {
        let dist = player_pose.position.distance(center);
        if dist <= ROCKET_BLAST_RADIUS {
            let dmg = if dist <= ROCKET_SELF_CORE_RADIUS {
                2
            } else {
                1
            };
            let dir = if dist > 0.001 {
                (player_pose.position - center) * (1.0 / dist)
            } else {
                Vec2::new(1.0, 0.0)
            };
            let t = (dist / ROCKET_BLAST_RADIUS).clamp(0.0, 1.0);
            *player_knockback += dir * (0.28 * (1.0 - 0.5 * t));
            if *player_iframes == 0 {
                *player_health = player_health.saturating_sub(dmg);
                *player_iframes = PLAYER_HURT_IFRAMES;
                events.push(ActionEvent::PlayerHurt {
                    at: player_pose.position,
                    floor: player_pose.floor,
                    damage: dmg,
                    remaining_hp: *player_health,
                    from: center,
                });
            }
        }
    }

    // Trigger immediate chain detonations for any explosively killed Bloaters.
    for bloater_idx in chain_bloaters {
        ai::detonate_bloater(
            world,
            monsters,
            bloater_idx,
            player_pose,
            player_health,
            player_iframes,
            player_knockback,
            events,
            noises,
            true,
        );
    }
}

pub fn is_rocket_danger_close(world: &dyn CombatWorld, player: Pose, monsters: &[Monster]) -> bool {
    let dir = player.facing.normalized();
    if dir.length_squared() <= 0.01 {
        return false;
    }

    // Any living monster intersecting the aim line within blast radius is danger close.
    for m in monsters {
        if !m.is_alive() || m.floor != player.floor {
            continue;
        }
        let r = m.radius() + ROCKET_RADIUS;
        let oc = m.pos - player.position;
        let proj = oc.dot(dir);
        let perp_sq = oc.length_squared() - proj * proj;
        if perp_sq > r * r {
            continue;
        }
        let half_chord = (r * r - perp_sq).max(0.0).sqrt();
        let t_hit = (proj - half_chord).max(0.0);
        if proj + half_chord >= 0.0 && t_hit <= ROCKET_BLAST_RADIUS {
            return true;
        }
    }

    // Any solid terrain / intact window along the aim ray within blast radius is danger close.
    let start_tile = Tile3D::from_vec2(player.position, player.floor);
    let steps = (ROCKET_BLAST_RADIUS / 0.08).ceil() as usize;
    for i in 1..=steps {
        let t = (i as f32 * 0.08).min(ROCKET_BLAST_RADIUS);
        let p = player.position + dir * t;
        let tile = Tile3D::from_vec2(p, player.floor);
        if tile == start_tile {
            continue;
        }
        if !in_world_bounds(world, tile) {
            return true;
        }
        if matches!(
            world.obstruction(tile),
            TileObstruction::Wall(_)
                | TileObstruction::DoorClosed
                | TileObstruction::LowProp(_)
                | TileObstruction::IntactWindow
        ) {
            return true;
        }
    }

    false
}

pub(crate) fn emit_tile_damage_effects(
    tile: Tile3D,
    hit_pos: Vec2,
    res: TileDamageResult,
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
) {
    events.push(ActionEvent::MaterialHit {
        at: tile,
        hit_pos,
        material: res.material,
        destroyed: res.destroyed,
        light_changed: res.light_changed,
    });
    if res.collapsed_cells > 0 {
        events.push(ActionEvent::StructuralCollapse { at: tile });
    }
    if res.sound_radius > 0.0 {
        let (kind, priority) = if res.collapsed_cells > 0 {
            (NoiseKind::Collapse, 5)
        } else if res.material == Material::Glass && res.destroyed {
            (NoiseKind::GlassShatter, 4)
        } else if res.destroyed {
            (NoiseKind::PropBreak, 3)
        } else {
            (NoiseKind::AxeSwing, 2)
        };
        noises.push(NoiseEvent {
            origin: hit_pos,
            floor: tile.floor,
            radius: res.sound_radius,
            priority,
            kind,
        });
    }
}

#[inline]
fn in_world_bounds(world: &dyn CombatWorld, tile: Tile3D) -> bool {
    let (min, max, floors) = world.bounds();
    tile.x >= min.x && tile.x <= max.x && tile.y >= min.y && tile.y <= max.y && tile.floor < floors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Position;
    use crate::action::types::{BuildingId, MonsterId, MonsterKind, TileKind};
    use std::collections::HashMap;

    struct MockWorld {
        width: i32,
        height: i32,
        floors: u8,
        version: u64,
        tiles: HashMap<Tile3D, (TileObstruction, u16)>,
        missing_floors: HashMap<Tile3D, bool>,
        flashes: Vec<(Vec2, Floor, f32, u8, u8)>,
    }

    impl MockWorld {
        fn new(width: i32, height: i32, floors: u8) -> Self {
            Self {
                width,
                height,
                floors,
                version: 0,
                tiles: HashMap::new(),
                missing_floors: HashMap::new(),
                flashes: Vec::new(),
            }
        }

        fn set_tile(&mut self, tile: Tile3D, obs: TileObstruction, hp: u16) {
            self.tiles.insert(tile, (obs, hp));
        }

        fn set_no_floor(&mut self, tile: Tile3D) {
            self.missing_floors.insert(tile, true);
        }
    }

    impl CombatWorld for MockWorld {
        fn bounds(&self) -> (Position, Position, u8) {
            (
                Position::new(0, 0),
                Position::new(self.width - 1, self.height - 1),
                self.floors,
            )
        }

        fn terrain_version(&self) -> u64 {
            self.version
        }

        fn obstruction(&self, tile: Tile3D) -> TileObstruction {
            self.tiles
                .get(&tile)
                .map_or(TileObstruction::Open, |(obs, _)| *obs)
        }

        fn has_floor(&self, tile: Tile3D) -> bool {
            !self.missing_floors.contains_key(&tile)
        }

        fn is_walkable(&self, tile: Tile3D, can_vault_window: bool) -> bool {
            if !self.has_floor(tile) {
                return false;
            }
            match self.obstruction(tile) {
                TileObstruction::Open => true,
                TileObstruction::BrokenWindow => can_vault_window && tile.floor == 0,
                _ => false,
            }
        }

        fn building_id(&self, _tile: Tile3D) -> BuildingId {
            0
        }

        fn stair_link(&self, _tile: Tile3D) -> Option<(Tile3D, (i8, i8))> {
            None
        }

        fn window_drop_target(&self, _tile: Tile3D) -> Option<Tile3D> {
            None
        }

        fn window_vault_target(&self, _from: Tile3D, _window: Tile3D) -> Option<Tile3D> {
            None
        }

        fn light_level(&self, _tile: Tile3D) -> f32 {
            1.0
        }

        fn damage_tile(&mut self, tile: Tile3D, kind: DamageKind) -> Option<TileDamageResult> {
            let (obs, hp) = *self.tiles.get(&tile)?;
            let (mat, old_kind, dmg, new_obs_on_break, new_kind_on_break, sound) = match obs {
                TileObstruction::IntactWindow => (
                    Material::Glass,
                    TileKind::Window,
                    10,
                    TileObstruction::BrokenWindow,
                    TileKind::BrokenWindow,
                    9.0,
                ),
                TileObstruction::Wall(m) => {
                    let d = match kind {
                        DamageKind::RocketBlast => 200,
                        DamageKind::RocketSplash => 40,
                        DamageKind::Axe => 1,
                        _ => 1,
                    };
                    (
                        m,
                        TileKind::ExteriorWall,
                        d,
                        TileObstruction::Open,
                        TileKind::Rubble,
                        12.0,
                    )
                }
                TileObstruction::DoorClosed => (
                    Material::WoodDoor,
                    TileKind::Door {
                        open: false,
                        exterior: true,
                    },
                    25,
                    TileObstruction::Open,
                    TileKind::Rubble,
                    7.0,
                ),
                TileObstruction::LowProp(m) => (
                    m,
                    TileKind::Prop(crate::action::types::PropKind::Crate),
                    20,
                    TileObstruction::Open,
                    TileKind::Rubble,
                    6.0,
                ),
                TileObstruction::Open | TileObstruction::BrokenWindow => return None,
            };
            let new_hp = hp.saturating_sub(dmg);
            let destroyed = new_hp == 0;
            if destroyed {
                self.tiles.insert(tile, (new_obs_on_break, 0));
                self.version += 1;
            } else {
                self.tiles.insert(tile, (obs, new_hp));
            }
            Some(TileDamageResult {
                damaged: true,
                destroyed,
                material: mat,
                old_kind,
                new_kind: if destroyed {
                    new_kind_on_break
                } else {
                    old_kind
                },
                sound_radius: if destroyed { sound } else { 2.0 },
                light_changed: false,
                collapsed_cells: 0,
            })
        }

        fn toggle_door(&mut self, _tile: Tile3D) -> Option<bool> {
            None
        }

        fn register_dynamic_flash(
            &mut self,
            pos: Vec2,
            floor: Floor,
            radius: f32,
            intensity: u8,
            ttl_ticks: u8,
        ) {
            self.flashes
                .push((pos, floor, radius, intensity, ttl_ticks));
        }
    }

    #[test]
    fn axe_cleaves_arc_shatters_glass_in_one_hit_and_applies_knockback() {
        let mut world = MockWorld::new(20, 20, 2);
        let win_tile = Tile3D::new(6, 5, 0);
        world.set_tile(win_tile, TileObstruction::IntactWindow, 1);

        let mut inv = Inventory::new();
        let mut events = Vec::new();
        let mut noises = Vec::new();
        inv.collect_loot(
            CacheLoot {
                weapon: Some(Weapon::Axe),
                ..CacheLoot::default()
            },
            &mut events,
        );
        assert_eq!(inv.equipped, Some(Weapon::Axe));

        let mut monsters = vec![
            // Inside 110° front cleave arc (slightly above center)
            Monster::new(MonsterId(1), MonsterKind::Brisk, 0, Tile3D::new(6, 4, 0)),
            // Behind player — must NOT be hit
            Monster::new(MonsterId(2), MonsterKind::Brisk, 0, Tile3D::new(4, 5, 0)),
        ];
        monsters[0].pos = Vec2::new(5.8, 4.5);
        monsters[1].pos = Vec2::new(4.2, 5.0);

        let player = Pose {
            position: Vec2::new(5.0, 5.0),
            facing: Vec2::new(1.0, 0.0),
            floor: 0,
        };
        let mut projectiles = Vec::new();
        let mut next_id = 1;
        events.clear();

        try_fire_or_swing(
            &mut world,
            &mut inv,
            player,
            &mut monsters,
            &mut projectiles,
            &mut next_id,
            42,
            1,
            &mut events,
            &mut noises,
        );

        assert_eq!(monsters[0].hp, 1, "Brisk (3 HP) takes 2 axe damage");
        assert!(
            monsters[0].knockback.length() > 0.15,
            "Axe applies strong knockback"
        );
        assert_eq!(monsters[0].hitstun_ticks, 12);
        assert_eq!(monsters[1].hp, 3, "Monster behind player is untouched");
        assert_eq!(
            world.obstruction(win_tile),
            TileObstruction::BrokenWindow,
            "Window shatters in 1 axe hit"
        );
        assert!(events.iter().any(|e| matches!(
            e,
            ActionEvent::AxeSwing {
                hit_flesh: true,
                hit_solid: true,
                ..
            }
        )));
        assert!(noises.iter().any(|n| n.kind == NoiseKind::GlassShatter));
    }

    #[test]
    fn pistol_ammo_reload_wall_block_and_glass_pierce() {
        let mut world = MockWorld::new(20, 20, 2);
        let win_tile = Tile3D::new(7, 5, 0);
        world.set_tile(win_tile, TileObstruction::IntactWindow, 1);
        let wall_tile = Tile3D::new(11, 5, 0);
        world.set_tile(
            wall_tile,
            TileObstruction::Wall(Material::BrickMasonry),
            500,
        );

        let mut inv = Inventory::new();
        let mut events = Vec::new();
        let mut noises = Vec::new();
        inv.collect_loot(
            CacheLoot {
                weapon: Some(Weapon::Pistol),
                pistol_ammo: 10,
                ..CacheLoot::default()
            },
            &mut events,
        );
        assert_eq!(inv.mag[Weapon::Pistol.slot()], 8);
        assert_eq!(inv.reserve[Weapon::Pistol.slot()], 2);

        let mut monsters = vec![
            // Behind intact window (should be hit after bullet shatters glass!)
            Monster::new(MonsterId(1), MonsterKind::Shambler, 0, Tile3D::new(9, 5, 0)),
            // Behind solid brick wall (must NOT be hit!)
            Monster::new(
                MonsterId(2),
                MonsterKind::Shambler,
                0,
                Tile3D::new(13, 5, 0),
            ),
        ];

        let player = Pose {
            position: Vec2::new(5.0, 5.0),
            facing: Vec2::new(1.0, 0.0),
            floor: 0,
        };
        let mut projectiles = Vec::new();
        let mut next_id = 1;

        // Shot 1 pierces glass and kills Shambler 1 (2 HP == 2 pistol dmg).
        events.clear();
        try_fire_or_swing(
            &mut world,
            &mut inv,
            player,
            &mut monsters,
            &mut projectiles,
            &mut next_id,
            42,
            1,
            &mut events,
            &mut noises,
        );
        assert_eq!(world.obstruction(win_tile), TileObstruction::BrokenWindow);
        assert!(!monsters[0].is_alive());
        assert_eq!(inv.mag[Weapon::Pistol.slot()], 7);

        // Cooldown expires; Shot 2 stops on brick wall so Shambler 2 is untouched.
        inv.fire_cooldown = 0;
        events.clear();
        try_fire_or_swing(
            &mut world,
            &mut inv,
            player,
            &mut monsters,
            &mut projectiles,
            &mut next_id,
            42,
            20,
            &mut events,
            &mut noises,
        );
        assert!(monsters[1].is_alive());
        assert_eq!(monsters[1].hp, 2);
        assert_eq!(inv.mag[Weapon::Pistol.slot()], 6);

        // Empty mag and trigger reload (52 ticks).
        inv.mag[Weapon::Pistol.slot()] = 0;
        inv.fire_cooldown = 0;
        events.clear();
        try_fire_or_swing(
            &mut world,
            &mut inv,
            player,
            &mut monsters,
            &mut projectiles,
            &mut next_id,
            42,
            40,
            &mut events,
            &mut noises,
        );
        assert!(inv.reload.is_some());
        assert!(events.iter().any(|e| matches!(
            e,
            ActionEvent::ReloadStarted {
                weapon: Weapon::Pistol,
                duration_ticks: 52
            }
        )));
        for _ in 0..52 {
            inv.tick_timers(&mut events);
        }
        assert!(inv.reload.is_none());
        assert_eq!(inv.mag[Weapon::Pistol.slot()], 2);
        assert_eq!(inv.reserve[Weapon::Pistol.slot()], 0);

        // Dry fire when mag == 0 and reserve == 0.
        inv.mag[Weapon::Pistol.slot()] = 0;
        inv.fire_cooldown = 0;
        events.clear();
        try_fire_or_swing(
            &mut world,
            &mut inv,
            player,
            &mut monsters,
            &mut projectiles,
            &mut next_id,
            42,
            100,
            &mut events,
            &mut noises,
        );
        assert!(events.contains(&ActionEvent::DryFire {
            weapon: Weapon::Pistol
        }));
    }

    #[test]
    fn shotgun_five_pellet_spread_and_fire_interruptible_single_shell_reload() {
        let mut world = MockWorld::new(20, 20, 2);
        let mut inv = Inventory::new();
        let mut events = Vec::new();
        let mut noises = Vec::new();
        inv.collect_loot(
            CacheLoot {
                weapon: Some(Weapon::Shotgun),
                shotgun_ammo: 8,
                ..CacheLoot::default()
            },
            &mut events,
        );
        assert_eq!(inv.mag[Weapon::Shotgun.slot()], 4);
        assert_eq!(inv.reserve[Weapon::Shotgun.slot()], 4);

        // Point-blank Bloater (8 HP) should eat all 5 pellets = 5 dmg in a single MonsterHit!
        let mut monsters = vec![Monster::new(
            MonsterId(1),
            MonsterKind::Bloater,
            0,
            Tile3D::new(6, 5, 0),
        )];
        monsters[0].pos = Vec2::new(5.85, 5.0);

        let player = Pose {
            position: Vec2::new(5.0, 5.0),
            facing: Vec2::new(1.0, 0.0),
            floor: 0,
        };
        let mut projectiles = Vec::new();
        let mut next_id = 1;
        events.clear();

        try_fire_or_swing(
            &mut world,
            &mut inv,
            player,
            &mut monsters,
            &mut projectiles,
            &mut next_id,
            7,
            1,
            &mut events,
            &mut noises,
        );

        let trace_count = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    ActionEvent::BulletTrace {
                        weapon: Weapon::Shotgun,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(trace_count, 5, "Shotgun emits 5 BulletTrace events");

        let hit_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, ActionEvent::MonsterHit { .. }))
            .collect();
        assert_eq!(
            hit_events.len(),
            1,
            "Shotgun aggregates pellets into a single MonsterHit per monster"
        );
        assert_eq!(
            monsters[0].hp, 3,
            "Point-blank Bloater takes 5 damage (8 -> 3)"
        );

        // Empty mag, start single-shell reload, load 1 shell (22 ticks), then interrupt by firing!
        inv.mag[Weapon::Shotgun.slot()] = 0;
        inv.fire_cooldown = 0;
        assert!(inv.try_start_reload(&mut events));
        for _ in 0..22 {
            inv.tick_timers(&mut events);
        }
        assert_eq!(inv.mag[Weapon::Shotgun.slot()], 1);
        assert!(
            inv.reload.is_some(),
            "SingleShell reload continues loading next shell"
        );

        // Interrupt mid-reload with a shot!
        inv.fire_cooldown = 0;
        events.clear();
        try_fire_or_swing(
            &mut world,
            &mut inv,
            player,
            &mut monsters,
            &mut projectiles,
            &mut next_id,
            7,
            60,
            &mut events,
            &mut noises,
        );
        assert!(inv.reload.is_none(), "Firing interrupts SingleShell reload");
        assert_eq!(inv.mag[Weapon::Shotgun.slot()], 0);
        assert!(events.iter().any(|e| matches!(
            e,
            ActionEvent::ShotFired {
                weapon: Weapon::Shotgun,
                ..
            }
        )));
    }

    #[test]
    fn rocket_flight_wall_breach_falloff_self_damage_and_danger_close() {
        let mut world = MockWorld::new(24, 24, 2);
        let wall = Tile3D::new(8, 5, 0);
        world.set_tile(wall, TileObstruction::Wall(Material::BrickMasonry), 150);

        let mut inv = Inventory::new();
        let mut events = Vec::new();
        let mut noises = Vec::new();
        inv.collect_loot(
            CacheLoot {
                weapon: Some(Weapon::RocketLauncher),
                rockets: 3,
                ..CacheLoot::default()
            },
            &mut events,
        );
        assert_eq!(inv.mag[Weapon::RocketLauncher.slot()], 1);
        assert_eq!(inv.reserve[Weapon::RocketLauncher.slot()], 2);

        let close_player = Pose {
            position: Vec2::new(6.8, 5.0),
            facing: Vec2::new(1.0, 0.0),
            floor: 0,
        };
        let far_player = Pose {
            position: Vec2::new(4.0, 5.0),
            facing: Vec2::new(1.0, 0.0),
            floor: 0,
        };

        assert!(
            is_rocket_danger_close(&world, close_player, &[]),
            "Wall 1.2 tiles ahead must trigger danger-close"
        );
        assert!(
            !is_rocket_danger_close(&world, far_player, &[]),
            "Wall 4 tiles ahead is safe"
        );

        let mut monsters = vec![
            // Near blast core (dist ~0.8) -> high falloff damage (kills 8-HP Bloater? Wait: use Brisk & Bloater)
            Monster::new(MonsterId(1), MonsterKind::Bloater, 0, Tile3D::new(8, 6, 0)),
            // Near blast rim (dist ~2.1) -> ~5 dmg (survives with 3 HP)
            Monster::new(MonsterId(2), MonsterKind::Bloater, 0, Tile3D::new(8, 7, 0)),
        ];
        monsters[0].pos = Vec2::new(7.6, 5.2);
        monsters[1].pos = Vec2::new(7.6, 7.1);

        let mut projectiles = Vec::new();
        let mut next_id = 1;
        let mut player_hp = 6u8;
        let mut player_iframes = 0u8;
        let mut player_kb = Vec2::ZERO;

        // Launch rocket from close_player (1.2 tiles from wall -> self core damage 2!)
        try_fire_or_swing(
            &mut world,
            &mut inv,
            close_player,
            &mut monsters,
            &mut projectiles,
            &mut next_id,
            1,
            1,
            &mut events,
            &mut noises,
        );
        assert_eq!(projectiles.len(), 1);

        for _ in 0..10 {
            if projectiles.is_empty() {
                break;
            }
            step_projectiles(
                &mut world,
                &mut projectiles,
                close_player,
                0.23,
                &mut player_hp,
                &mut player_iframes,
                &mut player_kb,
                &mut monsters,
                &mut events,
                &mut noises,
            );
        }

        assert!(projectiles.is_empty(), "Rocket detonated");
        assert_eq!(
            world.obstruction(wall),
            TileObstruction::Open,
            "Rocket breached brick wall"
        );
        assert!(
            !monsters[0].is_alive(),
            "Core Bloater killed explosively and chain-burst immediately"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ActionEvent::RocketBlast { .. })),
            "Emits RocketBlast"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ActionEvent::BloaterBurst { id, .. } if *id == MonsterId(1))),
            "Explosive kill triggers immediate Bloater chain burst"
        );
        assert!(
            player_hp <= 4,
            "Player inside core radius takes fair self-damage"
        );
        assert!(player_iframes > 0);
        assert!(
            noises
                .iter()
                .any(|n| n.kind == NoiseKind::RocketBlast && n.radius >= 24.0)
        );
    }

    #[test]
    fn upper_floor_pistol_angles_down_through_f1_window_to_hit_outdoor_f0_monster() {
        let mut world = MockWorld::new(20, 20, 2);
        // F1 window at (6, 5, 1); exterior air beyond x >= 7 on Floor 1.
        world.set_tile(Tile3D::new(6, 5, 1), TileObstruction::IntactWindow, 1);
        for x in 7..20 {
            world.set_no_floor(Tile3D::new(x, 5, 1));
        }

        let mut inv = Inventory::new();
        let mut events = Vec::new();
        let mut noises = Vec::new();
        inv.collect_loot(
            CacheLoot {
                weapon: Some(Weapon::Pistol),
                pistol_ammo: 8,
                ..CacheLoot::default()
            },
            &mut events,
        );

        let mut monsters = vec![Monster::new(
            MonsterId(1),
            MonsterKind::Shambler,
            0,
            Tile3D::new(10, 5, 0),
        )];
        let player = Pose {
            position: Vec2::new(5.0, 5.0),
            facing: Vec2::new(1.0, 0.0),
            floor: 1,
        };
        let mut projectiles = Vec::new();
        let mut next_id = 1;

        try_fire_or_swing(
            &mut world,
            &mut inv,
            player,
            &mut monsters,
            &mut projectiles,
            &mut next_id,
            42,
            1,
            &mut events,
            &mut noises,
        );

        assert_eq!(
            world.obstruction(Tile3D::new(6, 5, 1)),
            TileObstruction::BrokenWindow
        );
        assert!(
            !monsters[0].is_alive(),
            "F1 shot through window hits outdoor F0 Shambler"
        );
        assert!(events.iter().any(|e| matches!(
            e,
            ActionEvent::BulletTrace {
                from_z,
                to_z,
                hit_material: MaterialCategory::Flesh,
                ..
            } if (*from_z - 1.0).abs() < 0.01 && to_z.abs() < 0.01
        )));
    }

    #[test]
    fn medkit_quick_heal_preserves_equipped_weapon_and_caps_health() {
        let mut inv = Inventory::new();
        let mut events = Vec::new();
        let mut noises = Vec::new();
        inv.collect_loot(
            CacheLoot {
                weapon: Some(Weapon::Shotgun),
                shotgun_ammo: 4,
                medkits: 2,
                ..CacheLoot::default()
            },
            &mut events,
        );
        assert_eq!(inv.equipped, Some(Weapon::Shotgun));
        assert_eq!(inv.medkits, 2);

        let mut hp = 2u8;
        let healed =
            inv.try_use_medkit(&mut hp, 6, Vec2::new(3.0, 3.0), 0, &mut events, &mut noises);
        assert!(healed);
        assert_eq!(hp, 5);
        assert_eq!(inv.medkits, 1);
        assert_eq!(
            inv.equipped,
            Some(Weapon::Shotgun),
            "Medkit preserves currently equipped weapon"
        );
        assert!(inv.medkit_cooldown == MEDKIT_COOLDOWN_TICKS);
        assert!(inv.action_slow_ticks == MEDKIT_ACTION_LOCK_TICKS);
        assert!(inv.move_speed_mult() < weapon_spec(Weapon::Shotgun).move_speed_mult);
        assert!(events.iter().any(|e| matches!(
            e,
            ActionEvent::Healed {
                amount: 3,
                new_health: 5,
                ..
            }
        )));
        assert!(noises.iter().any(|n| n.kind == NoiseKind::MedkitUse));

        // Cooldown prevents immediate second heal.
        assert!(!inv.try_use_medkit(&mut hp, 6, Vec2::new(3.0, 3.0), 0, &mut events, &mut noises));
    }

    #[test]
    fn inventory_switching_cancels_reload_and_unarmed_shove_pushes_monsters() {
        let mut world = MockWorld::new(16, 16, 2);
        let mut inv = Inventory::new();
        let mut events = Vec::new();
        let mut noises = Vec::new();
        let mut projectiles = Vec::new();
        let mut next_id = 1u32;

        // Unarmed shove applies knockback and 0 damage.
        let mut monsters = vec![Monster::new(
            MonsterId(1),
            MonsterKind::Shambler,
            0,
            Tile3D::new(6, 5, 0),
        )];
        monsters[0].pos = Vec2::new(5.7, 5.0);
        let player = Pose {
            position: Vec2::new(5.0, 5.0),
            facing: Vec2::new(1.0, 0.0),
            floor: 0,
        };
        try_fire_or_swing(
            &mut world,
            &mut inv,
            player,
            &mut monsters,
            &mut projectiles,
            &mut next_id,
            1,
            1,
            &mut events,
            &mut noises,
        );
        assert_eq!(monsters[0].hp, 2, "Unarmed shove deals 0 damage");
        assert!(monsters[0].knockback.x > 0.1);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ActionEvent::UnarmedShove { .. }))
        );

        // Collect Axe and Pistol, start reload, verify progress and cancel on weapon switch/cycle.
        inv.collect_loot(
            CacheLoot {
                weapon: Some(Weapon::Axe),
                ..CacheLoot::default()
            },
            &mut events,
        );
        inv.collect_loot(
            CacheLoot {
                weapon: Some(Weapon::Pistol),
                pistol_ammo: 12,
                ..CacheLoot::default()
            },
            &mut events,
        );
        assert!(inv.select_weapon(Weapon::Pistol, &mut events));
        assert_eq!(inv.equipped_ammo_total(), 12);
        inv.mag[Weapon::Pistol.slot()] = 4;
        assert!(inv.try_start_reload(&mut events));
        for _ in 0..26 {
            inv.tick_timers(&mut events);
        }
        assert_eq!(inv.reload_progress_pct(), 50);

        // Cycling cancels active reload and switches back to Axe.
        assert!(inv.cycle_weapon(&mut events));
        assert_eq!(inv.equipped, Some(Weapon::Axe));
        assert!(inv.reload.is_none());
        assert_eq!(inv.equipped_ammo_total(), 0);
    }

    #[test]
    fn shooter_bolt_shatters_glass_and_hurts_player_with_iframes() {
        let mut world = MockWorld::new(20, 20, 2);
        let win = Tile3D::new(7, 5, 0);
        world.set_tile(win, TileObstruction::IntactWindow, 1);

        let proj = Projectile {
            id: ProjectileId(9),
            kind: ProjectileKind::ShooterBolt,
            owner_is_player: false,
            pos: Vec2::new(5.0, 5.0),
            dir: Vec2::new(1.0, 0.0),
            floor: 0,
            z: 0.0,
            vz: 0.0,
            speed: SHOOTER_BOLT_SPEED,
            radius: SHOOTER_BOLT_RADIUS,
            remaining_range: SHOOTER_BOLT_RANGE,
        };
        assert_eq!(proj.to_view().id, ProjectileId(9));

        let mut projectiles = vec![proj];
        let player = Pose {
            position: Vec2::new(9.0, 5.0),
            facing: Vec2::new(-1.0, 0.0),
            floor: 0,
        };
        let mut player_hp = 6u8;
        let mut player_iframes = 0u8;
        let mut player_kb = Vec2::ZERO;
        let mut monsters = Vec::new();
        let mut events = Vec::new();
        let mut noises = Vec::new();

        for _ in 0..40 {
            if projectiles.is_empty() {
                break;
            }
            step_projectiles(
                &mut world,
                &mut projectiles,
                player,
                0.23,
                &mut player_hp,
                &mut player_iframes,
                &mut player_kb,
                &mut monsters,
                &mut events,
                &mut noises,
            );
        }

        assert!(projectiles.is_empty());
        assert_eq!(world.obstruction(win), TileObstruction::BrokenWindow);
        assert_eq!(player_hp, 5);
        assert_eq!(player_iframes, PLAYER_HURT_IFRAMES);
        assert!(player_kb.x > 0.05);
        assert!(events.iter().any(|e| matches!(
            e,
            ActionEvent::PlayerHurt {
                damage: 1,
                remaining_hp: 5,
                ..
            }
        )));
    }
}
