//! Survival tools, firearms, melee, projectiles, and medkit system.

use super::PLAYER_SHAPE;
use super::ai::{self, Monster};
use super::types::{
    ActionEvent, CacheLoot, CombatWorld, DT, DamageKind, Floor, Material, MaterialCategory,
    NoiseEvent, NoiseKind, Pose, ProjectileId, ProjectileKind, ProjectileView, Tile3D,
    TileDamageResult, TileObstruction, Vec2, Vec3, Weapon, mix64,
};
use crate::spatial::{
    BodyState, ColliderKind, FLOOR_H, HitSurface, QueryFilter, Ray3, WALL_HEIGHT,
    resolve_actor_support,
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

/// Default player firearm muzzle height above foot elevation.
pub const WEAPON_MUZZLE_HEIGHT: f32 = 0.68;
/// Elevated muzzle height when firing downward from an upper-story window or ledge.
pub const UPPER_LEDGE_MUZZLE_HEIGHT: f32 = 0.84;
/// Shooter monster plasma/bolt muzzle height above foot elevation.
pub const SHOOTER_MUZZLE_HEIGHT: f32 = 0.68;

const SHOTGUN_PELLET_ANGLES_DEG: [f32; 5] = [-14.0, -7.0, 0.0, 7.0, 14.0];
const SHOTGUN_JITTER_DEG: f32 = 1.1;
const AXE_TILE_RAY_ANGLES_DEG: [f32; 3] = [-30.0, 0.0, 30.0];
const UPPER_WINDOW_EXTRA_RANGE: f32 = 2.5;
#[allow(dead_code)]
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
        self.try_use_medkit_3d(
            health,
            max_health,
            player_pos.extend(player_floor as f32 * FLOOR_H),
            player_floor,
            events,
            noises,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_use_medkit_3d(
        &mut self,
        health: &mut u8,
        max_health: u8,
        player_pos: Vec3,
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
            origin: player_pos.xy(),
            origin_z: player_pos.z,
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
    pub position: Vec3,
    pub velocity: Vec3,
    pub floor: Floor,
    pub radius: f32,
    pub remaining_range: f32,
}

impl Projectile {
    #[allow(clippy::wrong_self_convention)]
    pub fn to_view(&self) -> ProjectileView {
        let dir = self.velocity.xy().normalized();
        ProjectileView {
            id: self.id,
            kind: self.kind,
            pos: self.position.xy(),
            dir,
            floor: self.floor,
            z: self.position.z,
            position: self.position,
            velocity: self.velocity,
            radius: self.radius,
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
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
    let (pz, sup, _) = resolve_actor_support(world, player.position, player.floor);
    let player_body = BodyState::from_pose_2d(player.position, player.facing, pz, sup);
    try_fire_or_swing_3d(
        world,
        inventory,
        player,
        player_body,
        monsters,
        projectiles,
        next_proj_id,
        seed,
        tick,
        events,
        noises,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn try_fire_or_swing_3d(
    world: &mut dyn CombatWorld,
    inventory: &mut Inventory,
    player: Pose,
    player_body: BodyState,
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

    for m in monsters.iter_mut() {
        if m.is_targetable() {
            m.ensure_body_synced(world);
        }
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
        execute_unarmed_shove_3d(
            world,
            inventory,
            player,
            player_body,
            facing,
            monsters,
            events,
        );
        return;
    };

    if weapon == Weapon::Axe {
        execute_axe_swing_3d(
            world,
            inventory,
            player,
            player_body,
            facing,
            monsters,
            events,
            noises,
        );
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

    let (muzzle_pos, aim_dir3) = compute_player_aim_3d(
        world,
        player_body,
        player.floor,
        facing,
        spec.range,
        monsters,
    );

    events.push(ActionEvent::ShotFired {
        weapon,
        from: player_body.position,
        muzzle: muzzle_pos,
        floor: player.floor,
        facing,
        dir: aim_dir3,
    });

    match weapon {
        Weapon::Pistol => {
            world.register_dynamic_flash(muzzle_pos + aim_dir3 * 0.35, player.floor, 3.2, 130, 3);
            noises.push(NoiseEvent {
                origin: player_body.position.xy(),
                origin_z: player_body.position.z,
                floor: player.floor,
                radius: spec.fire_noise_radius,
                priority: 4,
                kind: NoiseKind::PistolShot,
            });
            let hit = trace_bullet_ray_3d(
                world,
                muzzle_pos,
                player.floor,
                aim_dir3,
                spec.range,
                DamageKind::Bullet,
                monsters,
                events,
                noises,
            );
            events.push(ActionEvent::BulletTrace {
                weapon: Weapon::Pistol,
                from: muzzle_pos,
                to: hit.end_pos,
                hit_normal: hit.hit_normal,
                hit_material: hit.hit_material,
            });
            if let Some(idx) = hit.hit_monster {
                let kb_dir = aim_dir3.xy().normalized();
                let _ = ai::damage_monster(
                    &mut monsters[idx],
                    spec.monster_damage,
                    kb_dir * spec.knockback,
                    spec.hitstun_ticks,
                    false,
                    events,
                );
            }
        }
        Weapon::Shotgun => {
            world.register_dynamic_flash(muzzle_pos + aim_dir3 * 0.4, player.floor, 4.5, 175, 4);
            noises.push(NoiseEvent {
                origin: player_body.position.xy(),
                origin_z: player_body.position.z,
                floor: player.floor,
                radius: spec.fire_noise_radius,
                priority: 5,
                kind: NoiseKind::ShotgunBlast,
            });

            // Aggregate pellet hits per monster on the stack (at most 5 distinct targets).
            let mut hit_targets: [(usize, u8, Vec2); 5] = [(0, 0, Vec2::ZERO); 5];
            let mut hit_target_count = 0usize;

            let horiz = aim_dir3.xy();
            let horiz_len = horiz.length();
            let base_horiz = if horiz_len > 1e-4 {
                horiz * (1.0 / horiz_len)
            } else {
                facing
            };

            for (pellet_idx, base_deg) in SHOTGUN_PELLET_ANGLES_DEG.iter().enumerate() {
                let h = mix64(
                    seed ^ tick.wrapping_mul(0x9e37_79b9_7f4a_7c15)
                        ^ (pellet_idx as u64).wrapping_mul(0x517c_c1b7_2722_0a95),
                );
                let unit = ((h & 0xffff) as f32 / 65535.0) * 2.0 - 1.0;
                let angle_rad = (*base_deg + unit * SHOTGUN_JITTER_DEG).to_radians();
                let horiz_dir = base_horiz.rotate(angle_rad).normalized();
                let dir3 = Vec3::new(
                    horiz_dir.x * horiz_len.max(0.05),
                    horiz_dir.y * horiz_len.max(0.05),
                    aim_dir3.z,
                )
                .normalized();

                let hit = trace_bullet_ray_3d(
                    world,
                    muzzle_pos,
                    player.floor,
                    dir3,
                    spec.range,
                    DamageKind::ShotgunPellet,
                    monsters,
                    events,
                    noises,
                );
                events.push(ActionEvent::BulletTrace {
                    weapon: Weapon::Shotgun,
                    from: muzzle_pos,
                    to: hit.end_pos,
                    hit_normal: hit.hit_normal,
                    hit_material: hit.hit_material,
                });

                if let Some(m_idx) = hit.hit_monster {
                    let kb = horiz_dir * spec.knockback;
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
            world.register_dynamic_flash(muzzle_pos + aim_dir3 * 0.4, player.floor, 4.0, 160, 5);
            noises.push(NoiseEvent {
                origin: player_body.position.xy(),
                origin_z: player_body.position.z,
                floor: player.floor,
                radius: spec.fire_noise_radius,
                priority: 4,
                kind: NoiseKind::RocketLaunch,
            });

            let id = ProjectileId(*next_proj_id);
            *next_proj_id = next_proj_id.wrapping_add(1);
            projectiles.push(Projectile {
                id,
                kind: ProjectileKind::Rocket,
                owner_is_player: true,
                position: muzzle_pos,
                velocity: aim_dir3 * ROCKET_SPEED,
                floor: player.floor,
                radius: ROCKET_RADIUS,
                remaining_range: spec.range,
            });
            events.push(ActionEvent::ProjectileLaunched {
                id,
                kind: ProjectileKind::Rocket,
                from: muzzle_pos,
                floor: player.floor,
                dir: aim_dir3.xy(),
                velocity: aim_dir3 * ROCKET_SPEED,
            });
        }
        Weapon::Axe => unreachable!(),
    }
}

/// Computes `(muzzle_pos, aim_dir3)` with 3D target assist, upper-window downward pitch,
/// and road/stair grade alignment.
pub(crate) fn compute_player_aim_3d(
    world: &dyn CombatWorld,
    player_body: BodyState,
    player_floor: Floor,
    facing: Vec2,
    range: f32,
    monsters: &[Monster],
) -> (Vec3, Vec3) {
    let facing = {
        let f = facing.normalized();
        if f.length_squared() > 0.01 {
            f
        } else {
            Vec2::new(0.0, -1.0)
        }
    };
    let foot_z = player_body.position.z;
    let base_muzzle = player_body.position.with_z(foot_z + WEAPON_MUZZLE_HEIGHT);

    // 1. 3D target assist along the horizontal aim corridor.
    let mut best_target: Option<(f32, Vec3, Vec3)> = None;
    for m in monsters {
        if !m.is_targetable() {
            continue;
        }
        let offset_xy = m.pos - base_muzzle.xy();
        let along = offset_xy.dot(facing);
        if along < 0.30 || along > range + UPPER_WINDOW_EXTRA_RANGE {
            continue;
        }
        let perp = offset_xy.perp_dot(facing).abs();
        if perp > m.radius() + 0.55 {
            continue;
        }
        let target_pt = m
            .body
            .position
            .with_z(m.body.position.z + WEAPON_MUZZLE_HEIGHT);
        let cand_muzzle = if player_floor > 0 && target_pt.z < foot_z - 0.30 {
            player_body
                .position
                .with_z(foot_z + UPPER_LEDGE_MUZZLE_HEIGHT)
        } else {
            base_muzzle
        };
        if (target_pt.z - cand_muzzle.z).abs() / along.max(0.5) > 1.35 {
            continue;
        }
        let delta = target_pt - cand_muzzle;
        let dist = delta.length();
        if dist < 0.05 {
            continue;
        }
        let dir = delta * (1.0 / dist);
        let check_dist = (dist - m.radius() - 0.04).max(0.02);
        if world
            .raycast_filtered(
                Ray3::new(cand_muzzle, dir),
                check_dist,
                QueryFilter::LINE_OF_SIGHT,
            )
            .is_some()
        {
            continue;
        }
        if best_target.is_none_or(|(best_along, _, _)| along < best_along) {
            best_target = Some((along, cand_muzzle, dir));
        }
    }
    if let Some((_, cand_muzzle, dir)) = best_target {
        return (cand_muzzle, dir);
    }

    // 2. Upper-floor window / ledge fallback.
    if player_floor > 0 {
        let start_tile = Tile3D::from_vec2(base_muzzle.xy(), player_floor);
        for i in 1..=35 {
            let t = i as f32 * 0.10;
            let p = base_muzzle.xy() + facing * t;
            let tile = Tile3D::from_vec2(p, player_floor);
            if tile == start_tile {
                continue;
            }
            if !in_world_bounds(world, tile) {
                break;
            }
            if !world.has_floor(tile) {
                let cand_muzzle = player_body
                    .position
                    .with_z(foot_z + UPPER_LEDGE_MUZZLE_HEIGHT);
                let ahead_xy = cand_muzzle.xy() + facing * 5.5;
                let gz = world.terrain_at(ahead_xy).map_or(0.0, |tr| tr.z);
                let dir = (ahead_xy.extend(gz + 0.55) - cand_muzzle).normalized();
                return (cand_muzzle, dir);
            }
            match world.obstruction(tile) {
                TileObstruction::Wall(_)
                | TileObstruction::DoorClosed
                | TileObstruction::LowProp(_) => break,
                TileObstruction::IntactWindow
                | TileObstruction::BrokenWindow
                | TileObstruction::Open => {}
            }
        }
    }

    // 3. Terrain / stair grade alignment fallback.
    let ahead_xy = base_muzzle.xy() + facing * 2.0;
    let mut sups = Vec::with_capacity(4);
    world.supports_at(ahead_xy, &mut sups);
    let mut best_sup: Option<f32> = None;
    for s in sups {
        if !s.walkable {
            continue;
        }
        let dz = (s.z - foot_z).abs();
        if dz <= 0.70 && best_sup.is_none_or(|b| (b - foot_z).abs() > dz) {
            best_sup = Some(s.z);
        }
    }
    if let Some(sz) = best_sup {
        let dz = (sz - foot_z).clamp(-0.70, 0.70);
        return (
            base_muzzle,
            Vec3::new(facing.x * 2.0, facing.y * 2.0, dz).normalized(),
        );
    }

    (base_muzzle, facing.extend(0.0))
}

fn execute_unarmed_shove_3d(
    world: &dyn CombatWorld,
    inventory: &mut Inventory,
    player: Pose,
    player_body: BodyState,
    facing: Vec2,
    monsters: &mut [Monster],
    events: &mut Vec<ActionEvent>,
) {
    inventory.fire_cooldown = UNARMED_SHOVE_COOLDOWN;
    events.push(ActionEvent::UnarmedShove {
        from: player_body.position,
        floor: player.floor,
        facing,
    });
    let from_chest = player_body.position.with_z(player_body.position.z + 0.55);
    for monster in monsters.iter_mut() {
        if !monster.is_targetable() {
            continue;
        }
        if (monster.body.position.z - player_body.position.z).abs() > 0.78 {
            continue;
        }
        let offset = monster.pos - player_body.position.xy();
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
        let target_chest = monster.body.position.with_z(monster.body.position.z + 0.55);
        let seg = target_chest - from_chest;
        let seg_len = seg.length();
        if seg_len > 0.05
            && world
                .raycast_filtered(
                    Ray3::new(from_chest, seg * (1.0 / seg_len)),
                    (seg_len - monster.radius() * 0.5).max(0.02),
                    QueryFilter::LINE_OF_SIGHT,
                )
                .is_some()
        {
            continue;
        }
        monster.apply_shove(
            dir * UNARMED_SHOVE_KNOCKBACK,
            UNARMED_SHOVE_HITSTUN,
            player_body.position.xy(),
            player.floor,
            events,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_axe_swing_3d(
    world: &mut dyn CombatWorld,
    inventory: &mut Inventory,
    player: Pose,
    player_body: BodyState,
    facing: Vec2,
    monsters: &mut [Monster],
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
) {
    let spec = weapon_spec(Weapon::Axe);
    inventory.fire_cooldown = spec.fire_cooldown;

    let swing_event_idx = events.len();
    events.push(ActionEvent::AxeSwing {
        from: player_body.position,
        floor: player.floor,
        facing,
        hit_flesh: false,
        hit_solid: false,
    });

    let mut hit_flesh = false;
    let mut hit_solid = false;
    let from_chest = player_body.position.with_z(player_body.position.z + 0.55);

    for monster in monsters.iter_mut() {
        if !monster.is_targetable() {
            continue;
        }
        if (monster.body.position.z - player_body.position.z).abs() > 0.85 {
            continue;
        }
        let offset = monster.pos - player_body.position.xy();
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
        let target_chest = monster.body.position.with_z(monster.body.position.z + 0.55);
        let seg = target_chest - from_chest;
        let seg_len = seg.length();
        if seg_len > 0.05
            && world
                .raycast_filtered(
                    Ray3::new(from_chest, seg * (1.0 / seg_len)),
                    (seg_len - monster.radius() * 0.5).max(0.02),
                    QueryFilter::LINE_OF_SIGHT,
                )
                .is_some()
        {
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

    // Cast 3 short waist-height 3D rays (-30°, 0°, +30°) to damage up to 2 distinct destructible tiles.
    let mut hit_tiles: [(Tile3D, Vec3, Vec3); 2] = [(Tile3D::default(), Vec3::ZERO, Vec3::Z); 2];
    let mut hit_tile_count = 0usize;
    let start_tile = Tile3D::from_vec2(player_body.position.xy(), player.floor);

    for angle_deg in AXE_TILE_RAY_ANGLES_DEG {
        if hit_tile_count >= hit_tiles.len() {
            break;
        }
        let dir2 = facing.rotate(angle_deg.to_radians()).normalized();
        let ray = Ray3::new(from_chest, dir2.extend(0.0));
        if let Some(hit) = world.raycast(ray, spec.range)
            && hit.distance > 0.01
            && let HitSurface::Collider(kind) = hit.surface
        {
            let tile = match kind {
                ColliderKind::Wall { tile }
                | ColliderKind::ClosedDoor { tile }
                | ColliderKind::WindowSill { tile, .. }
                | ColliderKind::WindowGlass { tile }
                | ColliderKind::Prop { tile, .. } => Some(tile),
                _ => None,
            };
            if let Some(tile) = tile
                && tile != start_tile
                && in_world_bounds(world, tile)
                && !hit_tiles[..hit_tile_count]
                    .iter()
                    .any(|(existing, _, _)| *existing == tile)
            {
                hit_tiles[hit_tile_count] = (tile, hit.point, hit.normal);
                hit_tile_count += 1;
            }
        }
    }

    for &(tile, hit_pos, hit_normal) in &hit_tiles[..hit_tile_count] {
        if let Some(res) = world.damage_tile(tile, DamageKind::Axe)
            && res.damaged
        {
            hit_solid = true;
            emit_tile_damage_effects(tile, hit_pos, hit_normal, res, events, noises);
        }
    }

    events[swing_event_idx] = ActionEvent::AxeSwing {
        from: player_body.position,
        floor: player.floor,
        facing,
        hit_flesh,
        hit_solid,
    };

    noises.push(NoiseEvent {
        origin: player_body.position.xy(),
        origin_z: player_body.position.z,
        floor: player.floor,
        radius: spec.fire_noise_radius,
        priority: 2,
        kind: NoiseKind::AxeSwing,
    });
}

struct RayHit3D {
    end_pos: Vec3,
    hit_normal: Vec3,
    hit_material: MaterialCategory,
    hit_monster: Option<usize>,
}

#[allow(clippy::too_many_arguments)]
fn trace_bullet_ray_3d(
    world: &mut dyn CombatWorld,
    muzzle: Vec3,
    from_floor: Floor,
    dir3: Vec3,
    base_range: f32,
    damage_kind: DamageKind,
    monsters: &[Monster],
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
) -> RayHit3D {
    let dir3 = dir3.normalized();
    let max_range = if from_floor > 0 && dir3.z < -0.05 {
        base_range + UPPER_WINDOW_EXTRA_RANGE
    } else {
        base_range
    };

    let mut t_cursor = 0.0_f32;
    let mut last_shattered: Option<Tile3D> = None;

    for _ in 0..8 {
        if t_cursor >= max_range {
            break;
        }
        let seg_origin = muzzle + dir3 * t_cursor;
        let remaining = max_range - t_cursor;
        let world_hit = world.raycast(Ray3::new(seg_origin, dir3), remaining);
        let seg_end = world_hit.map_or(max_range, |h| t_cursor + h.distance);

        if let Some((m_idx, m_t, m_normal)) =
            closest_monster_on_ray_3d(monsters, muzzle, dir3, t_cursor, seg_end)
        {
            return RayHit3D {
                end_pos: muzzle + dir3 * m_t,
                hit_normal: m_normal,
                hit_material: MaterialCategory::Flesh,
                hit_monster: Some(m_idx),
            };
        }

        let Some(hit) = world_hit else {
            return RayHit3D {
                end_pos: muzzle + dir3 * max_range,
                hit_normal: -dir3,
                hit_material: MaterialCategory::None,
                hit_monster: None,
            };
        };

        match hit.surface {
            HitSurface::Collider(ColliderKind::WindowGlass { tile }) => {
                if last_shattered != Some(tile) {
                    last_shattered = Some(tile);
                    let _ = apply_bullet_tile_hit_3d(
                        world,
                        tile,
                        hit.point,
                        hit.normal,
                        damage_kind,
                        events,
                        noises,
                    );
                }
                t_cursor = seg_end + 0.03;
            }
            HitSurface::Collider(
                ColliderKind::Wall { tile }
                | ColliderKind::ClosedDoor { tile }
                | ColliderKind::WindowSill { tile, .. }
                | ColliderKind::Prop { tile, .. },
            ) => {
                let mat = apply_bullet_tile_hit_3d(
                    world,
                    tile,
                    hit.point,
                    hit.normal,
                    damage_kind,
                    events,
                    noises,
                );
                return RayHit3D {
                    end_pos: hit.point,
                    hit_normal: hit.normal,
                    hit_material: mat,
                    hit_monster: None,
                };
            }
            HitSurface::Terrain { .. }
            | HitSurface::BridgeDeck { .. }
            | HitSurface::Stair { .. }
            | HitSurface::Collider(
                ColliderKind::BuildingSlab { .. }
                | ColliderKind::BridgeParapet { .. }
                | ColliderKind::BridgeDeck { .. }
                | ColliderKind::WaterHazard { .. },
            ) => {
                return RayHit3D {
                    end_pos: hit.point,
                    hit_normal: hit.normal,
                    hit_material: MaterialCategory::Stone,
                    hit_monster: None,
                };
            }
        }
    }

    RayHit3D {
        end_pos: muzzle + dir3 * max_range,
        hit_normal: -dir3,
        hit_material: MaterialCategory::None,
        hit_monster: None,
    }
}

fn apply_bullet_tile_hit_3d(
    world: &mut dyn CombatWorld,
    tile: Tile3D,
    hit_pos: Vec3,
    hit_normal: Vec3,
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
            emit_tile_damage_effects(tile, hit_pos, hit_normal, res, events, noises);
        }
        let cat = res.material.category();
        if cat != MaterialCategory::None {
            return cat;
        }
    }
    fallback_mat
}

/// Ray vs finite vertical cylinder `(xy, radius + 0.05, [foot_z, foot_z + height])`.
fn closest_monster_on_ray_3d(
    monsters: &[Monster],
    origin: Vec3,
    dir: Vec3,
    t_min: f32,
    t_max: f32,
) -> Option<(usize, f32, Vec3)> {
    let mut best: Option<(usize, f32, Vec3)> = None;
    let d_xy = dir.xy();
    let a = d_xy.length_squared();

    for (i, m) in monsters.iter().enumerate() {
        if !m.is_targetable() {
            continue;
        }
        let c_xy = m.body.position.xy();
        let r = m.radius() + 0.05;
        let z0 = m.body.position.z;
        let z1 = z0 + m.spec().height;

        let (t_xy0, t_xy1) = if a > 1e-10 {
            let oc = origin.xy() - c_xy;
            let b = oc.dot(d_xy);
            let c = oc.length_squared() - r * r;
            let disc = b * b - a * c;
            if disc < 0.0 {
                continue;
            }
            let sqrt_d = disc.sqrt();
            ((-b - sqrt_d) / a, (-b + sqrt_d) / a)
        } else if (origin.xy() - c_xy).length_squared() <= r * r {
            (t_min, t_max)
        } else {
            continue;
        };

        let (t_z0, t_z1) = if dir.z.abs() > 1e-7 {
            let tz_a = (z0 - origin.z) / dir.z;
            let tz_b = (z1 - origin.z) / dir.z;
            (tz_a.min(tz_b), tz_a.max(tz_b))
        } else if origin.z >= z0 && origin.z <= z1 {
            (-f32::INFINITY, f32::INFINITY)
        } else {
            continue;
        };

        let t_enter = t_xy0.max(t_z0).max(t_min);
        let t_exit = t_xy1.min(t_z1).min(t_max);
        if t_enter <= t_exit && best.is_none_or(|(_, best_t, _)| t_enter < best_t) {
            let normal = if t_z0 > t_xy0 {
                Vec3::new(0.0, 0.0, if dir.z > 0.0 { -1.0 } else { 1.0 })
            } else {
                let p_xy = origin.xy() + d_xy * t_enter;
                let n_xy = (p_xy - c_xy).normalized();
                if n_xy.length_squared() > 0.01 {
                    n_xy.extend(0.0)
                } else {
                    -dir
                }
            };
            best = Some((i, t_enter, normal));
        }
    }
    best
}

/// Sweeps a sphere of radius `sphere_r` along `from -> to` against a vertical actor cylinder.
/// Returns `(u, normal)` with `u in [0.0, 1.0]` on intersection.
fn sweep_sphere_cylinder(
    from: Vec3,
    to: Vec3,
    sphere_r: f32,
    center_xy: Vec2,
    actor_r: f32,
    z_min: f32,
    z_max: f32,
) -> Option<(f32, Vec3)> {
    let d = to - from;
    let d_xy = d.xy();
    let a = d_xy.length_squared();
    let r_sum = actor_r + sphere_r;

    let (u_xy0, u_xy1) = if a > 1e-10 {
        let oc = from.xy() - center_xy;
        let b = oc.dot(d_xy);
        let c = oc.length_squared() - r_sum * r_sum;
        let disc = b * b - a * c;
        if disc < 0.0 {
            return None;
        }
        let sqrt_d = disc.sqrt();
        ((-b - sqrt_d) / a, (-b + sqrt_d) / a)
    } else if (from.xy() - center_xy).length_squared() <= r_sum * r_sum {
        (0.0, 1.0)
    } else {
        return None;
    };

    let z0 = z_min - sphere_r;
    let z1 = z_max + sphere_r;
    let (u_z0, u_z1) = if d.z.abs() > 1e-7 {
        let uz_a = (z0 - from.z) / d.z;
        let uz_b = (z1 - from.z) / d.z;
        (uz_a.min(uz_b), uz_a.max(uz_b))
    } else if from.z >= z0 && from.z <= z1 {
        (0.0, 1.0)
    } else {
        return None;
    };

    let u_enter = u_xy0.max(u_z0).max(0.0);
    let u_exit = u_xy1.min(u_z1).min(1.0);
    if u_enter > u_exit {
        return None;
    }

    let normal = if u_z0 > u_xy0 {
        Vec3::new(0.0, 0.0, if d.z > 0.0 { -1.0 } else { 1.0 })
    } else {
        let p_xy = from.xy() + d_xy * u_enter;
        let n_xy = (p_xy - center_xy).normalized();
        if n_xy.length_squared() > 0.01 {
            n_xy.extend(0.0)
        } else {
            (-d).normalized()
        }
    };
    Some((u_enter, normal))
}

#[inline]
fn semantic_floor_at_point(world: &dyn CombatWorld, pos: Vec3) -> Floor {
    let gz = world.terrain_at(pos.xy()).map_or(0.0, |t| t.z);
    let max_floor = world.bounds().2.saturating_sub(1);
    ((pos.z - gz) / FLOOR_H)
        .round()
        .clamp(0.0, max_floor as f32) as Floor
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
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
    let (pz, sup, _) = resolve_actor_support(world, player_pose.position, player_pose.floor);
    let player_body = BodyState::from_pose_2d(player_pose.position, player_pose.facing, pz, sup);
    step_projectiles_3d(
        world,
        projectiles,
        player_pose,
        player_body,
        player_radius,
        player_health,
        player_iframes,
        player_knockback,
        monsters,
        events,
        noises,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn step_projectiles_3d(
    world: &mut dyn CombatWorld,
    projectiles: &mut Vec<Projectile>,
    player_pose: Pose,
    player_body: BodyState,
    player_radius: f32,
    player_health: &mut u8,
    player_iframes: &mut u8,
    player_knockback: &mut Vec2,
    monsters: &mut [Monster],
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
) {
    for m in monsters.iter_mut() {
        if m.is_alive() {
            m.ensure_body_synced(world);
        }
    }

    let (_, _, num_floors) = world.bounds();
    let max_floor = num_floors.saturating_sub(1);

    let mut idx = 0usize;
    while idx < projectiles.len() {
        let mut proj = projectiles[idx];
        let speed = proj.velocity.length();
        let step_dist = (speed * DT).min(proj.remaining_range.max(0.0));
        let vel_dir = if speed > 1e-4 {
            proj.velocity * (1.0 / speed)
        } else {
            Vec3::X
        };
        let end_pos = proj.position + vel_dir * step_dist;
        proj.remaining_range -= step_dist;

        let mut remove = false;
        let mut seg_from = proj.position;
        let mut last_shattered_window: Option<Tile3D> = None;

        for _ in 0..4 {
            let seg_vec = end_pos - seg_from;
            let seg_len = seg_vec.length();
            if seg_len <= 1e-5 {
                break;
            }

            let world_hit = world.sphere_cast(seg_from, end_pos, proj.radius);
            let world_u =
                world_hit.map_or(f32::INFINITY, |h| (h.distance / seg_len).clamp(0.0, 1.0));

            match proj.kind {
                ProjectileKind::Rocket => {
                    let mut best_m_u = f32::INFINITY;
                    for m in monsters.iter() {
                        if !m.is_alive() {
                            continue;
                        }
                        if let Some((u, _)) = sweep_sphere_cylinder(
                            seg_from,
                            end_pos,
                            proj.radius,
                            m.body.position.xy(),
                            m.radius(),
                            m.body.position.z,
                            m.body.position.z + m.spec().height,
                        ) && u < best_m_u
                        {
                            best_m_u = u;
                        }
                    }

                    if best_m_u <= world_u && best_m_u <= 1.0 {
                        let center = seg_from.lerp(end_pos, best_m_u);
                        detonate_rocket_3d(
                            world,
                            center,
                            -vel_dir,
                            player_pose,
                            player_body,
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

                    if let Some(hit) = world_hit {
                        let center = seg_from.lerp(end_pos, world_u);
                        detonate_rocket_3d(
                            world,
                            center,
                            hit.normal,
                            player_pose,
                            player_body,
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

                    break;
                }
                ProjectileKind::ShooterBolt => {
                    let mut player_hit: Option<(f32, Vec3)> = None;
                    if !proj.owner_is_player
                        && let Some((u, n)) = sweep_sphere_cylinder(
                            seg_from,
                            end_pos,
                            proj.radius,
                            player_body.position.xy(),
                            player_radius,
                            player_body.position.z,
                            player_body.position.z + PLAYER_SHAPE.height,
                        )
                    {
                        player_hit = Some((u, n));
                    }

                    let p_u = player_hit.map_or(f32::INFINITY, |(u, _)| u);
                    if p_u <= world_u && p_u <= 1.0 {
                        let hit_pt = seg_from.lerp(end_pos, p_u);
                        if *player_iframes == 0 {
                            *player_health = player_health.saturating_sub(1);
                            *player_iframes = PLAYER_HURT_IFRAMES;
                            *player_knockback += vel_dir.xy().normalized() * SHOOTER_BOLT_KNOCKBACK;
                            events.push(ActionEvent::PlayerHurt {
                                at: player_body.position,
                                floor: player_pose.floor,
                                damage: 1,
                                remaining_hp: *player_health,
                                from: hit_pt,
                            });
                        }
                        remove = true;
                        break;
                    }

                    if let Some(hit) = world_hit {
                        match hit.surface {
                            HitSurface::Collider(ColliderKind::WindowGlass { tile }) => {
                                if last_shattered_window != Some(tile) {
                                    last_shattered_window = Some(tile);
                                    let _ = apply_bullet_tile_hit_3d(
                                        world,
                                        tile,
                                        hit.point,
                                        hit.normal,
                                        DamageKind::Bullet,
                                        events,
                                        noises,
                                    );
                                }
                                let next_u = (world_u + 0.04 / seg_len).min(1.0);
                                seg_from = seg_from.lerp(end_pos, next_u);
                                continue;
                            }
                            HitSurface::Collider(
                                ColliderKind::Wall { tile }
                                | ColliderKind::ClosedDoor { tile }
                                | ColliderKind::WindowSill { tile, .. }
                                | ColliderKind::Prop { tile, .. },
                            ) => {
                                let _ = apply_bullet_tile_hit_3d(
                                    world,
                                    tile,
                                    hit.point,
                                    hit.normal,
                                    DamageKind::Bullet,
                                    events,
                                    noises,
                                );
                                remove = true;
                                break;
                            }
                            _ => {
                                remove = true;
                                break;
                            }
                        }
                    }

                    break;
                }
            }
        }

        if !remove {
            proj.position = end_pos;
            proj.floor = semantic_floor_at_point(world, proj.position);
            let tile = Tile3D::from_vec2(proj.position.xy(), proj.floor);

            let out_of_bounds = !in_world_bounds(world, tile)
                || (proj.kind == ProjectileKind::ShooterBolt
                    && proj.velocity.z > 0.0
                    && proj.position.z > (max_floor as f32 + 0.8) * FLOOR_H);
            if out_of_bounds || proj.remaining_range <= 0.001 {
                if proj.kind == ProjectileKind::Rocket {
                    detonate_rocket_3d(
                        world,
                        proj.position,
                        -vel_dir,
                        player_pose,
                        player_body,
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
        }

        if remove {
            projectiles.remove(idx);
        } else {
            projectiles[idx] = proj;
            idx += 1;
        }
    }
}

/// Returns `true` when `QueryFilter::LINE_OF_SIGHT` encounters terrain, a floor slab,
/// bridge deck, wall, or closed door between `origin` and `target`.
pub(crate) fn is_blast_occluded_3d(world: &dyn CombatWorld, origin: Vec3, target: Vec3) -> bool {
    let delta = target - origin;
    let dist = delta.length();
    if dist <= 0.08 {
        return false;
    }
    world
        .raycast_filtered(
            Ray3::new(origin, delta * (1.0 / dist)),
            dist - 0.04,
            QueryFilter::LINE_OF_SIGHT,
        )
        .is_some()
}

#[allow(clippy::too_many_arguments)]
fn detonate_rocket_3d(
    world: &mut dyn CombatWorld,
    center: Vec3,
    blast_normal: Vec3,
    player_pose: Pose,
    player_body: BodyState,
    player_health: &mut u8,
    player_iframes: &mut u8,
    player_knockback: &mut Vec2,
    monsters: &mut [Monster],
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
) {
    let spec = weapon_spec(Weapon::RocketLauncher);
    let (_, _, num_floors) = world.bounds();
    let floor = semantic_floor_at_point(world, center);
    let ray_origin = center + blast_normal * 0.06;

    world.register_dynamic_flash(center, floor, 7.5, 220, 10);
    events.push(ActionEvent::RocketBlast {
        at: center,
        floor,
        radius: ROCKET_BLAST_RADIUS,
    });
    noises.push(NoiseEvent {
        origin: center.xy(),
        origin_z: center.z,
        floor,
        radius: ROCKET_BLAST_NOISE_RADIUS,
        priority: 6,
        kind: NoiseKind::RocketBlast,
    });

    // Step 1: Damage tiles inside the 3D blast sphere across overlapping vertical floor bands.
    let cx = center.x.round() as i32;
    let cy = center.y.round() as i32;
    let gz_center = world.terrain_at(center.xy()).map_or(0.0, |t| t.z);
    for fl in 0..num_floors {
        let z_band_base = gz_center + fl as f32 * FLOOR_H;
        if center.z + ROCKET_BLAST_RADIUS < z_band_base
            || center.z - ROCKET_BLAST_RADIUS > z_band_base + WALL_HEIGHT
        {
            continue;
        }
        for dy in -3..=3 {
            for dx in -3..=3 {
                let tile = Tile3D::new(cx + dx, cy + dy, fl);
                if !in_world_bounds(world, tile) {
                    continue;
                }
                let tile_gz = world.terrain_at(tile.center()).map_or(gz_center, |t| t.z);
                let tile_z_base = tile_gz + fl as f32 * FLOOR_H;
                let closest_z = center.z.clamp(tile_z_base, tile_z_base + FLOOR_H);
                let tile_pt = tile.center().extend(closest_z);
                let dist3 = center.distance(tile_pt);
                if dist3 > ROCKET_BLAST_RADIUS {
                    continue;
                }
                if fl != floor
                    && is_blast_occluded_3d(
                        world,
                        ray_origin,
                        tile.center().extend(tile_z_base + 0.55),
                    )
                {
                    continue;
                }
                let kind = if dist3 <= ROCKET_BREACH_RADIUS {
                    DamageKind::RocketBlast
                } else {
                    DamageKind::RocketSplash
                };
                if let Some(res) = world.damage_tile(tile, kind)
                    && res.damaged
                {
                    emit_tile_damage_effects(tile, tile_pt, Vec3::Z, res, events, noises);
                }
            }
        }
    }

    // Step 2: Damage monsters with 3D distance falloff and line-of-sight occlusion.
    let mut chain_bloaters: Vec<usize> = Vec::new();
    for (i, m) in monsters.iter_mut().enumerate() {
        if !m.is_alive() {
            continue;
        }
        let m_foot_z = m.body.position.z;
        let m_h = m.spec().height;
        let closest_z = center.z.clamp(
            m_foot_z + 0.15,
            (m_foot_z + m_h - 0.15).max(m_foot_z + 0.15),
        );
        let actor_pt = m.body.position.xy().extend(closest_z);
        let dist3 = center.distance(actor_pt);
        if dist3 > ROCKET_BLAST_RADIUS + m.radius() {
            continue;
        }
        let torso_pt = m.body.position.xy().extend(m_foot_z + m_h * 0.52);
        if dist3 > m.radius() + ROCKET_RADIUS + 0.15
            && is_blast_occluded_3d(world, ray_origin, torso_pt)
        {
            continue;
        }

        let t = (dist3 / ROCKET_BLAST_RADIUS).clamp(0.0, 1.0);
        let max_dmg = spec.monster_damage as f32;
        let min_dmg = ROCKET_MIN_DAMAGE as f32;
        let dmg = (max_dmg - (max_dmg - min_dmg) * t)
            .round()
            .clamp(min_dmg, max_dmg) as u8;
        let diff_xy = m.body.position.xy() - center.xy();
        let dist_xy = diff_xy.length();
        let dir = if dist_xy > 0.001 {
            diff_xy * (1.0 / dist_xy)
        } else {
            Vec2::new(1.0, 0.0)
        };
        let kb = dir * (spec.knockback * (1.0 - 0.5 * t));
        if ai::damage_monster(m, dmg, kb, spec.hitstun_ticks, true, events) {
            chain_bloaters.push(i);
        }
    }

    // Fair player self-damage (2 core inside 1.25, 1 in 1.25..=2.4) respecting occlusion & iframes.
    let p_foot_z = player_body.position.z;
    let p_closest_z = center.z.clamp(
        p_foot_z + 0.15,
        (p_foot_z + PLAYER_SHAPE.height - 0.15).max(p_foot_z + 0.15),
    );
    let p_pt = player_body.position.xy().extend(p_closest_z);
    let p_dist3 = center.distance(p_pt);
    let p_torso = player_body
        .position
        .xy()
        .extend(p_foot_z + PLAYER_SHAPE.height * 0.52);
    if p_dist3 <= ROCKET_BLAST_RADIUS
        && (p_dist3 <= PLAYER_SHAPE.radius + ROCKET_RADIUS + 0.15
            || !is_blast_occluded_3d(world, ray_origin, p_torso))
    {
        let dmg = if p_dist3 <= ROCKET_SELF_CORE_RADIUS {
            2
        } else {
            1
        };
        let diff_xy = player_body.position.xy() - center.xy();
        let dist_xy = diff_xy.length();
        let dir = if dist_xy > 0.001 {
            diff_xy * (1.0 / dist_xy)
        } else {
            Vec2::new(1.0, 0.0)
        };
        let t = (p_dist3 / ROCKET_BLAST_RADIUS).clamp(0.0, 1.0);
        *player_knockback += dir * (0.28 * (1.0 - 0.5 * t));
        if *player_iframes == 0 {
            *player_health = player_health.saturating_sub(dmg);
            *player_iframes = PLAYER_HURT_IFRAMES;
            events.push(ActionEvent::PlayerHurt {
                at: player_body.position,
                floor: player_pose.floor,
                damage: dmg,
                remaining_hp: *player_health,
                from: center,
            });
        }
    }

    // Trigger immediate chain detonations for any explosively killed Bloaters.
    for bloater_idx in chain_bloaters {
        ai::detonate_bloater_3d(
            world,
            monsters,
            bloater_idx,
            player_pose,
            player_body.position,
            player_health,
            player_iframes,
            player_knockback,
            events,
            noises,
            true,
        );
    }
}

#[allow(dead_code)]
pub fn is_rocket_danger_close(world: &dyn CombatWorld, player: Pose, monsters: &[Monster]) -> bool {
    let (pz, sup, _) = resolve_actor_support(world, player.position, player.floor);
    let player_body = BodyState::from_pose_2d(player.position, player.facing, pz, sup);
    is_rocket_danger_close_3d(world, player, player_body, monsters)
}

pub(crate) fn is_rocket_danger_close_3d(
    world: &dyn CombatWorld,
    player: Pose,
    player_body: BodyState,
    monsters: &[Monster],
) -> bool {
    let dir = player.facing.normalized();
    if dir.length_squared() <= 0.01 {
        return false;
    }

    let range = weapon_spec(Weapon::RocketLauncher).range;
    let (muzzle, aim_dir3) =
        compute_player_aim_3d(world, player_body, player.floor, dir, range, monsters);
    let end = muzzle + aim_dir3 * ROCKET_BLAST_RADIUS;

    // Any living monster swept within blast radius is danger close.
    for m in monsters {
        if !m.is_alive() {
            continue;
        }
        let m_body = m.resolved_body(world);
        if sweep_sphere_cylinder(
            muzzle,
            end,
            ROCKET_RADIUS,
            m_body.position.xy(),
            m.radius(),
            m_body.position.z,
            m_body.position.z + m.spec().height,
        )
        .is_some()
        {
            return true;
        }
    }

    world.sphere_cast(muzzle, end, ROCKET_RADIUS).is_some()
}

pub(crate) fn emit_tile_damage_effects(
    tile: Tile3D,
    hit_pos: Vec3,
    hit_normal: Vec3,
    res: TileDamageResult,
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
) {
    events.push(ActionEvent::MaterialHit {
        at: tile,
        hit_pos,
        hit_normal,
        material: res.material,
        destroyed: res.destroyed,
        light_changed: res.light_changed,
    });
    if res.collapsed_cells > 0 {
        events.push(ActionEvent::StructuralCollapse {
            at: tile,
            pos: hit_pos,
        });
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
            origin: hit_pos.xy(),
            origin_z: hit_pos.z,
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
    use crate::action::types::{BuildingId, MonsterId, MonsterKind, TileKind};
    use crate::spatial::{
        Aabb3, Collider, SLAB_THICKNESS, SpatialWorld, SupportId, SupportSurface, SurfaceQuad,
        TerrainSample, TerrainTriangle, WALL_HALF_THICKNESS, WINDOW_LINTEL_Z, WINDOW_SILL_HEIGHT,
        world_to_tile_coord,
    };
    use crate::{Position, Surface};
    use std::collections::HashMap;

    struct MockWorld {
        width: i32,
        height: i32,
        floors: u8,
        version: u64,
        tiles: HashMap<Tile3D, (TileObstruction, u16)>,
        missing_floors: HashMap<Tile3D, bool>,
        flashes: Vec<(Vec3, Floor, f32, u8, u8)>,
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

        fn is_wall_like(&self, tile: Tile3D) -> bool {
            matches!(
                self.obstruction(tile),
                TileObstruction::Wall(_)
                    | TileObstruction::DoorClosed
                    | TileObstruction::IntactWindow
                    | TileObstruction::BrokenWindow
            )
        }
    }

    impl SpatialWorld for MockWorld {
        fn terrain_at(&self, xy: Vec2) -> Option<TerrainSample> {
            let tx = world_to_tile_coord(xy.x);
            let ty = world_to_tile_coord(xy.y);
            if tx < 0 || tx >= self.width || ty < 0 || ty >= self.height {
                return None;
            }
            Some(TerrainSample {
                z: 0.0,
                normal: Vec3::Z,
                tile: Position::new(tx, ty),
                water_z: None,
                triangle: TerrainTriangle::LowerRight,
            })
        }

        fn supports_at(&self, xy: Vec2, out: &mut Vec<SupportSurface>) {
            out.clear();
            let Some(terrain) = self.terrain_at(xy) else {
                return;
            };
            let p = terrain.tile;
            if self.has_floor(Tile3D::from_pos(p, 0)) {
                out.push(SupportSurface {
                    id: SupportId::Terrain,
                    z: 0.0,
                    normal: Vec3::Z,
                    walkable: true,
                    is_stair: false,
                    floor: 0,
                    surface: Surface::Grass,
                    water_z: None,
                });
            }
            for fl in 1..self.floors {
                let t = Tile3D::from_pos(p, fl);
                if self.has_floor(t) {
                    out.push(SupportSurface {
                        id: SupportId::Floor(t),
                        z: fl as f32 * FLOOR_H,
                        normal: Vec3::Z,
                        walkable: true,
                        is_stair: false,
                        floor: fl,
                        surface: Surface::WoodFloor,
                        water_z: None,
                    });
                }
            }
        }

        fn colliders_in(&self, bounds: Aabb3, out: &mut Vec<Collider>) {
            out.clear();
            let (min_tile, max_tile) = bounds.tile_bounds();
            let push_if = |out: &mut Vec<Collider>, c: Collider| {
                if c.bounds.intersects(bounds) {
                    out.push(c);
                }
            };
            for ty in min_tile.y..=max_tile.y {
                for tx in min_tile.x..=max_tile.x {
                    if tx < 0 || tx >= self.width || ty < 0 || ty >= self.height {
                        push_if(
                            out,
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
                        continue;
                    }
                    let fx = tx as f32;
                    let fy = ty as f32;
                    for fl in 0..self.floors {
                        let tile = Tile3D::new(tx, ty, fl);
                        let z_base = fl as f32 * FLOOR_H;
                        let z_top = z_base
                            + if fl + 1 < self.floors {
                                WALL_HEIGHT
                            } else {
                                FLOOR_H
                            };
                        match self.obstruction(tile) {
                            TileObstruction::Wall(mat) => {
                                push_if(
                                    out,
                                    Collider {
                                        bounds: Aabb3::new(
                                            Vec3::new(fx - 0.5, fy - 0.5, z_base),
                                            Vec3::new(fx + 0.5, fy + 0.5, z_top),
                                        ),
                                        kind: ColliderKind::Wall { tile },
                                        material: mat,
                                    },
                                );
                            }
                            TileObstruction::DoorClosed => {
                                push_if(
                                    out,
                                    Collider {
                                        bounds: Aabb3::new(
                                            Vec3::new(fx - 0.5, fy - 0.5, z_base),
                                            Vec3::new(fx + 0.5, fy + 0.5, z_top),
                                        ),
                                        kind: ColliderKind::ClosedDoor { tile },
                                        material: Material::WoodDoor,
                                    },
                                );
                            }
                            TileObstruction::IntactWindow | TileObstruction::BrokenWindow => {
                                let broken =
                                    self.obstruction(tile) == TileObstruction::BrokenWindow;
                                let has_x = self.is_wall_like(Tile3D::new(tx - 1, ty, fl))
                                    || self.is_wall_like(Tile3D::new(tx + 1, ty, fl));
                                let has_y = self.is_wall_like(Tile3D::new(tx, ty - 1, fl))
                                    || self.is_wall_like(Tile3D::new(tx, ty + 1, fl));
                                let along_y = has_y || !has_x;
                                let (x0, x1, y0, y1) = if along_y {
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
                                push_if(
                                    out,
                                    Collider {
                                        bounds: Aabb3::new(
                                            Vec3::new(x0, y0, z_base),
                                            Vec3::new(x1, y1, z_base + WINDOW_SILL_HEIGHT),
                                        ),
                                        kind: ColliderKind::WindowSill { tile, broken },
                                        material: Material::WoodSiding,
                                    },
                                );
                                push_if(
                                    out,
                                    Collider {
                                        bounds: Aabb3::new(
                                            Vec3::new(x0, y0, z_base + WINDOW_LINTEL_Z),
                                            Vec3::new(x1, y1, z_top),
                                        ),
                                        kind: ColliderKind::Wall { tile },
                                        material: Material::WoodSiding,
                                    },
                                );
                                if !broken {
                                    push_if(
                                        out,
                                        Collider {
                                            bounds: Aabb3::new(
                                                Vec3::new(x0, y0, z_base + WINDOW_SILL_HEIGHT),
                                                Vec3::new(x1, y1, z_base + WINDOW_LINTEL_Z),
                                            ),
                                            kind: ColliderKind::WindowGlass { tile },
                                            material: Material::Glass,
                                        },
                                    );
                                }
                            }
                            TileObstruction::LowProp(mat) => {
                                push_if(
                                    out,
                                    Collider {
                                        bounds: Aabb3::new(
                                            Vec3::new(fx - 0.38, fy - 0.38, z_base),
                                            Vec3::new(fx + 0.38, fy + 0.38, z_base + 0.78),
                                        ),
                                        kind: ColliderKind::Prop { tile, prop: None },
                                        material: mat,
                                    },
                                );
                            }
                            TileObstruction::Open => {}
                        }
                        if fl >= 1 && self.has_floor(tile) {
                            push_if(
                                out,
                                Collider {
                                    bounds: Aabb3::new(
                                        Vec3::new(fx - 0.5, fy - 0.5, z_base - SLAB_THICKNESS),
                                        Vec3::new(fx + 0.5, fy + 0.5, z_base),
                                    ),
                                    kind: ColliderKind::BuildingSlab {
                                        floor: fl,
                                        building_id: 0,
                                        is_roof: false,
                                    },
                                    material: Material::None,
                                },
                            );
                        }
                    }
                }
            }
        }

        fn surface_quads_in(
            &self,
            min_tile: Position,
            max_tile: Position,
            out: &mut Vec<SurfaceQuad>,
        ) {
            out.clear();
            for ty in min_tile.y..=max_tile.y {
                for tx in min_tile.x..=max_tile.x {
                    if tx < 0 || tx >= self.width || ty < 0 || ty >= self.height {
                        continue;
                    }
                    let p = Position::new(tx, ty);
                    if self.has_floor(Tile3D::from_pos(p, 0)) {
                        out.push(SurfaceQuad {
                            surface: HitSurface::Terrain { tile: p },
                            x0: tx as f32 - 0.5,
                            y0: ty as f32 - 0.5,
                            x1: tx as f32 + 0.5,
                            y1: ty as f32 + 0.5,
                            corners: [0.0; 4],
                            material: Material::None,
                        });
                    }
                }
            }
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
            pos: Vec3,
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
                from,
                to,
                hit_material: MaterialCategory::Flesh,
                ..
            } if from.z > FLOOR_H && to.z < FLOOR_H && from.z > to.z
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
            position: Vec3::new(5.0, 5.0, SHOOTER_MUZZLE_HEIGHT),
            velocity: Vec3::new(SHOOTER_BOLT_SPEED, 0.0, 0.0),
            floor: 0,
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
