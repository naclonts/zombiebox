//! Sight- and sound-reactive monster AI, archetypes, and zero-alloc 3D A* pathfinding.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::Position;
use crate::spatial::{
    BodyShape, BodyState, FLOOR_H, MotionState, MoveParams, QueryFilter, Ray3, SpatialWorld,
    SupportId, Vec3, move_kinematic_with_horizontal_vel, resolve_actor_support,
    resolve_tile_support, semantic_floor_for_body, support_normal_at,
};

use super::PLAYER_SHAPE;
use super::types::{
    ActionEvent, AwarenessIcon, CombatWorld, DT, DamageKind, Floor, MonsterId, MonsterKind,
    MonsterView, NoiseEvent, NoiseKind, Pose, ProjectileId, ProjectileKind, Tile3D,
    TileObstruction, Vec2, mix64,
};
use super::weapons::{
    PLAYER_HURT_IFRAMES, Projectile, SHOOTER_BOLT_RADIUS, SHOOTER_BOLT_RANGE, SHOOTER_BOLT_SPEED,
    SHOOTER_MUZZLE_HEIGHT, WEAPON_MUZZLE_HEIGHT, emit_tile_damage_effects, is_blast_occluded_3d,
    semantic_floor_at_point,
};

pub const BLOATER_SLAM_RANGE: f32 = 1.18;
pub const BLOATER_SWELL_FUSE_TICKS: u16 = 18;
pub const BLOATER_BURST_RADIUS: f32 = 2.3;
pub const BLOATER_BURST_CORE_RADIUS: f32 = 1.6;
pub const BLOATER_BURST_MONSTER_DAMAGE: u8 = 3;
pub const BLOATER_BURST_NOISE_RADIUS: f32 = 11.0;

pub const SHOOTER_WINDUP_LOCK_TICKS: u16 = 11;
pub const SHOOTER_INTERRUPT_HITSTUN: u8 = 8;
pub const SHOOTER_RANGED_COOLDOWN: u8 = 75;
pub const SHOOTER_STANDOFF_MIN: f32 = 4.5;
pub const SHOOTER_STANDOFF_MAX: f32 = 7.5;
pub const SHOOTER_BACKPEDAL_RANGE: f32 = 3.2;
pub const SHOOTER_SHOT_NOISE_RADIUS: f32 = 10.0;

pub const MAX_REPATHS_PER_TICK: usize = 4;
pub const BURST_REPATHS_ON_TERRAIN_CHANGE: usize = 8;
pub const MAX_ASTAR_EXPANSIONS: usize = 384;
pub const STAIR_COOLDOWN_TICKS: u8 = 36;
pub const NOISE_BARRIER_PENALTY: f32 = 1.25;

const PROXIMITY_SENSE_RADIUS: f32 = 1.35;
const WANDER_FOV_COS: f32 = 0.4226; // cos(65°)
const ALERT_FOV_COS: f32 = 0.0872; // cos(85°)
const DIRECT_STEER_MAX_DIST: f32 = 5.0;
const COST_CARDINAL: u16 = 10;
const COST_DIAGONAL: u16 = 14;
const COST_STAIR: u16 = 12;
const COST_BROKEN_WINDOW: u16 = 22;
const COST_CLOSED_DOOR: u16 = 45;
const ALERT_LOSE_SIGHT_TICKS: u16 = 360;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonsterSpec {
    pub kind: MonsterKind,
    pub max_hp: u8,
    pub radius: f32,
    pub height: f32,
    pub wander_speed: f32,
    pub investigate_speed: f32,
    pub chase_speed: f32,
    pub sight_range: f32,
    pub attack_range: f32,
    pub windup_ticks: u16,
    pub recovery_ticks: u16,
    pub attack_damage: u8,
    pub attack_knockback: f32,
    pub kb_res: f32,
    pub bash_power: u8,
    pub can_vault_f0: bool,
}

impl MonsterSpec {
    #[inline]
    pub const fn body_shape(&self) -> BodyShape {
        BodyShape::new(self.radius, self.height)
    }
}

pub const MONSTER_SPECS: [MonsterSpec; 4] = [
    // Shambler = 0
    MonsterSpec {
        kind: MonsterKind::Shambler,
        max_hp: 2,
        radius: 0.25,
        height: 1.02,
        wander_speed: 0.75,
        investigate_speed: 1.35,
        chase_speed: 2.15,
        sight_range: 8.5,
        attack_range: 0.75,
        windup_ticks: 14,
        recovery_ticks: 34,
        attack_damage: 1,
        attack_knockback: 0.06,
        kb_res: 1.0,
        bash_power: 8,
        can_vault_f0: true,
    },
    // Brisk = 1
    MonsterSpec {
        kind: MonsterKind::Brisk,
        max_hp: 3,
        radius: 0.24,
        height: 1.00,
        wander_speed: 0.95,
        investigate_speed: 1.85,
        chase_speed: 3.15,
        sight_range: 9.5,
        attack_range: 0.78,
        windup_ticks: 10,
        recovery_ticks: 26,
        attack_damage: 1,
        attack_knockback: 0.08,
        kb_res: 1.15,
        bash_power: 10,
        can_vault_f0: true,
    },
    // Bloater = 2
    MonsterSpec {
        kind: MonsterKind::Bloater,
        max_hp: 8,
        radius: 0.36,
        height: 1.12,
        wander_speed: 0.55,
        investigate_speed: 1.00,
        chase_speed: 1.45,
        sight_range: 7.5,
        attack_range: 1.0,
        windup_ticks: 30,
        recovery_ticks: 48,
        attack_damage: 2,
        attack_knockback: 0.24,
        kb_res: 0.35,
        bash_power: 22,
        can_vault_f0: false,
    },
    // Shooter = 3
    MonsterSpec {
        kind: MonsterKind::Shooter,
        max_hp: 3,
        radius: 0.25,
        height: 1.04,
        wander_speed: 0.85,
        investigate_speed: 1.65,
        chase_speed: 2.45,
        sight_range: 11.0,
        attack_range: 8.5,
        windup_ticks: 33,
        recovery_ticks: 45,
        attack_damage: 1,
        attack_knockback: 0.08,
        kb_res: 1.0,
        bash_power: 8,
        can_vault_f0: true,
    },
];

#[inline]
pub const fn monster_spec(kind: MonsterKind) -> &'static MonsterSpec {
    &MONSTER_SPECS[kind as usize]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttackPhase {
    Windup,
    Recovery,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AwarenessState {
    Wander {
        goal: Option<Tile3D>,
        pause_ticks: u16,
    },
    Investigate {
        target: Tile3D,
        priority: u8,
        linger_ticks: u16,
    },
    Alert {
        last_seen: Tile3D,
        lose_sight_ticks: u16,
    },
    Attack {
        phase: AttackPhase,
        aim_dir: Vec2,
        aim_dir3: Vec3,
        target_floor: Floor,
        ticks_left: u16,
        total_ticks: u16,
    },
    DyingSwell {
        fuse_ticks: u16,
    },
}

#[derive(Clone, Debug)]
pub struct Monster {
    pub id: MonsterId,
    pub kind: MonsterKind,
    pub variant: u8,
    pub pos: Vec2,
    pub facing: Vec2,
    pub floor: Floor,
    pub body: BodyState,
    body_synced: bool,
    pub home: Tile3D,
    pub hp: u8,
    pub alive: bool,
    pub state: AwarenessState,
    pub hitstun_ticks: u8,
    pub knockback: Vec2,
    pub ranged_cooldown: u8,
    pub stair_cooldown: u8,
    pub bash_cooldown: u8,
    pub repath_cooldown: u8,
    pub path_dirty: bool,
    pub path: Vec<Tile3D>,
    pub has_los: bool,
}

impl Monster {
    pub fn new(id: MonsterId, kind: MonsterKind, variant: u8, spawn: Tile3D) -> Self {
        let spec = monster_spec(kind);
        let angle_idx = (mix64(id.0 as u64 ^ ((variant as u64) << 16)) & 7) as f32;
        let facing = Vec2::new(1.0, 0.0)
            .rotate(angle_idx * std::f32::consts::FRAC_PI_4)
            .normalized();
        let sup = if spawn.floor == 0 {
            SupportId::Terrain
        } else {
            SupportId::Floor(spawn)
        };
        let body =
            BodyState::from_pose_2d(spawn.center(), facing, spawn.floor as f32 * FLOOR_H, sup);
        Self {
            id,
            kind,
            variant,
            pos: spawn.center(),
            facing,
            floor: spawn.floor,
            body,
            body_synced: false,
            home: spawn,
            hp: spec.max_hp,
            alive: true,
            state: AwarenessState::Wander {
                goal: None,
                pause_ticks: 20 + ((id.0 as u16).wrapping_mul(13) % 45),
            },
            hitstun_ticks: 0,
            knockback: Vec2::ZERO,
            ranged_cooldown: 0,
            stair_cooldown: 0,
            bash_cooldown: 0,
            repath_cooldown: 0,
            path_dirty: true,
            path: Vec::new(),
            has_los: false,
        }
    }

    pub fn new_in_world(
        id: MonsterId,
        kind: MonsterKind,
        variant: u8,
        spawn: Tile3D,
        world: &dyn SpatialWorld,
    ) -> Self {
        let mut m = Self::new(id, kind, variant, spawn);
        let (z, sup, _) = resolve_tile_support(world, spawn);
        m.body = BodyState::from_pose_2d(spawn.center(), m.facing, z, sup);
        m.body_synced = true;
        m
    }

    #[inline]
    fn needs_body_sync(&self) -> bool {
        if !self.body_synced || (self.body.position.xy() - self.pos).length_squared() > 1e-7 {
            return true;
        }
        match self.body.motion {
            MotionState::Grounded(SupportId::Terrain | SupportId::Bridge(_)) => self.floor != 0,
            MotionState::Grounded(SupportId::Floor(t)) => t.floor != self.floor,
            MotionState::Grounded(SupportId::Stair(st)) => {
                self.floor != st.floor && self.floor != st.floor + 1
            }
            MotionState::Airborne => false,
        }
    }

    pub(crate) fn ensure_body_synced(&mut self, world: &dyn SpatialWorld) {
        if self.needs_body_sync() {
            let (z, sup, _) = resolve_actor_support(world, self.pos, self.floor);
            self.body = BodyState::from_pose_2d(self.pos, self.facing, z, sup);
            self.body_synced = true;
        }
    }

    pub(crate) fn resolved_body(&self, world: &dyn SpatialWorld) -> BodyState {
        if !self.needs_body_sync() {
            self.body
        } else {
            let (z, sup, _) = resolve_actor_support(world, self.pos, self.floor);
            BodyState::from_pose_2d(self.pos, self.facing, z, sup)
        }
    }

    #[inline]
    pub(crate) fn eye_pos(&self) -> Vec3 {
        self.spec().body_shape().eye_at(self.body.position)
    }

    #[inline]
    pub(crate) fn muzzle_pos(&self) -> Vec3 {
        self.body
            .position
            .with_z(self.body.position.z + SHOOTER_MUZZLE_HEIGHT)
    }

    #[inline]
    pub const fn spec(&self) -> &'static MonsterSpec {
        monster_spec(self.kind)
    }

    #[inline]
    pub const fn radius(&self) -> f32 {
        self.spec().radius
    }

    #[inline]
    pub const fn is_alive(&self) -> bool {
        self.alive
    }

    #[inline]
    pub const fn is_swelling(&self) -> bool {
        matches!(self.state, AwarenessState::DyingSwell { .. })
    }

    #[inline]
    pub const fn is_targetable(&self) -> bool {
        self.alive && self.hp > 0
    }

    pub fn invalidate_path(&mut self) {
        self.path.clear();
        self.path_dirty = true;
        self.repath_cooldown = 0;
    }

    pub(crate) fn apply_shove(
        &mut self,
        knockback: Vec2,
        hitstun_ticks: u8,
        from_pos: Vec2,
        from_floor: Floor,
        events: &mut Vec<ActionEvent>,
    ) {
        if !self.is_targetable() {
            return;
        }
        self.knockback += knockback * self.spec().kb_res;
        self.hitstun_ticks = self.hitstun_ticks.max(hitstun_ticks);
        if self.kind == MonsterKind::Shooter
            && self.hitstun_ticks >= SHOOTER_INTERRUPT_HITSTUN
            && matches!(
                self.state,
                AwarenessState::Attack {
                    phase: AttackPhase::Windup,
                    ..
                }
            )
        {
            self.state = AwarenessState::Alert {
                last_seen: Tile3D::from_vec2(from_pos, from_floor),
                lose_sight_ticks: 0,
            };
            self.ranged_cooldown = self.ranged_cooldown.max(20);
        }
        if matches!(
            self.state,
            AwarenessState::Wander { .. } | AwarenessState::Investigate { .. }
        ) {
            self.state = AwarenessState::Alert {
                last_seen: Tile3D::from_vec2(from_pos, from_floor),
                lose_sight_ticks: 0,
            };
            let to_attacker = (from_pos - self.pos).normalized();
            if to_attacker.length_squared() > 0.01 {
                self.facing = to_attacker;
            }
            self.invalidate_path();
            events.push(ActionEvent::MonsterAlerted {
                id: self.id,
                kind: self.kind,
                at: self.body.position,
                floor: self.floor,
                by_sight: false,
            });
        }
    }

    pub fn to_view(&self, world: &dyn CombatWorld) -> MonsterView {
        let body = self.resolved_body(world);
        let elevation = body.position.z;
        let eye_pos = self.spec().body_shape().eye_at(body.position);
        let telegraph_origin = body
            .position
            .with_z(body.position.z + SHOOTER_MUZZLE_HEIGHT);

        let awareness = match self.state {
            AwarenessState::Wander { .. } => AwarenessIcon::Unaware,
            AwarenessState::Investigate { .. } => AwarenessIcon::Investigating,
            AwarenessState::Alert { .. }
            | AwarenessState::Attack { .. }
            | AwarenessState::DyingSwell { .. } => AwarenessIcon::Alert,
        };

        let (
            telegraph_progress,
            telegraph_dir,
            telegraph_target,
            telegraph_dir3,
            telegraph_target3,
        ) = match self.state {
            AwarenessState::Attack {
                phase: AttackPhase::Windup,
                aim_dir,
                aim_dir3,
                ticks_left,
                total_ticks,
                ..
            } => {
                let progress = if total_ticks > 0 {
                    1.0 - (ticks_left as f32 / total_ticks as f32)
                } else {
                    1.0
                };
                if self.kind == MonsterKind::Shooter {
                    let ray = Ray3::new(telegraph_origin, aim_dir3);
                    let end3 = world
                        .raycast_filtered(ray, SHOOTER_BOLT_RANGE, QueryFilter::LINE_OF_SIGHT)
                        .map_or(telegraph_origin + aim_dir3 * SHOOTER_BOLT_RANGE, |h| {
                            h.point
                        });
                    let max_fl = world.bounds().2.saturating_sub(1);
                    let gz = world.terrain_at(end3.xy()).map_or(0.0, |t| t.z);
                    let end_fl =
                        ((end3.z - gz) / FLOOR_H).round().clamp(0.0, max_fl as f32) as Floor;
                    (
                        Some(progress.clamp(0.0, 1.0)),
                        aim_dir,
                        Some((end3.xy(), end_fl)),
                        aim_dir3,
                        Some(end3),
                    )
                } else {
                    (
                        Some(progress.clamp(0.0, 1.0)),
                        aim_dir,
                        None,
                        aim_dir3,
                        None,
                    )
                }
            }
            _ => (None, self.facing, None, self.facing.extend(0.0), None),
        };
        let ground_normal = if body.motion.is_grounded() {
            support_normal_at(world, body.position.xy(), body.motion.support(), self.floor)
        } else {
            Vec3::Z
        };

        MonsterView {
            id: self.id,
            kind: self.kind,
            variant: self.variant,
            pos: self.pos,
            facing: self.facing,
            floor: self.floor,
            body,
            ground_normal,
            elevation,
            eye_pos,
            hp: self.hp,
            max_hp: self.spec().max_hp,
            awareness,
            telegraph_progress,
            telegraph_dir,
            telegraph_target,
            telegraph_origin,
            telegraph_dir3,
            telegraph_target3,
            hitstun: self.hitstun_ticks > 0,
            swelling: self.is_swelling(),
        }
    }
}

/// Apply damage, knockback, and hitstun to a monster.
/// Returns `true` iff a `Bloater` was killed explosively (or hit explosively while swelling)
/// and requires an immediate chain detonation via [`detonate_bloater`].
pub fn damage_monster(
    monster: &mut Monster,
    damage: u8,
    knockback: Vec2,
    hitstun_ticks: u8,
    explosive: bool,
    events: &mut Vec<ActionEvent>,
) -> bool {
    if !monster.alive {
        return false;
    }
    if monster.is_swelling() {
        return explosive;
    }

    let spec = monster.spec();
    let kb_dir = knockback.normalized();
    monster.knockback += knockback * spec.kb_res;
    monster.hitstun_ticks = monster.hitstun_ticks.max(hitstun_ticks);

    // Interrupt Shooter windup on heavy hitstun (>= 8 ticks).
    if monster.kind == MonsterKind::Shooter
        && monster.hitstun_ticks >= SHOOTER_INTERRUPT_HITSTUN
        && matches!(
            monster.state,
            AwarenessState::Attack {
                phase: AttackPhase::Windup,
                ..
            }
        )
    {
        let est_pos = if kb_dir.length_squared() > 0.01 {
            monster.pos - kb_dir * 4.0
        } else {
            monster.pos
        };
        monster.state = AwarenessState::Alert {
            last_seen: Tile3D::from_vec2(est_pos, monster.floor),
            lose_sight_ticks: 0,
        };
        monster.ranged_cooldown = monster.ranged_cooldown.max(20);
        monster.invalidate_path();
    }

    if damage == 0 {
        return false;
    }

    let prev_hp = monster.hp;
    monster.hp = prev_hp.saturating_sub(damage);
    let m_pos3 = monster.body.position;
    events.push(ActionEvent::MonsterHit {
        id: monster.id,
        kind: monster.kind,
        at: m_pos3,
        hit_pos: m_pos3.with_z(m_pos3.z + WEAPON_MUZZLE_HEIGHT),
        floor: monster.floor,
        damage,
        remaining_hp: monster.hp,
        knockback_dir: kb_dir,
    });

    if monster.hp > 0 {
        // Alert unaware or investigating monsters when hurt!
        if matches!(
            monster.state,
            AwarenessState::Wander { .. } | AwarenessState::Investigate { .. }
        ) {
            let est_pos = if kb_dir.length_squared() > 0.01 {
                monster.pos - kb_dir * 4.0
            } else {
                monster.pos
            };
            if kb_dir.length_squared() > 0.01 {
                monster.facing = Vec2::new(-kb_dir.x, -kb_dir.y);
            }
            monster.state = AwarenessState::Alert {
                last_seen: Tile3D::from_vec2(est_pos, monster.floor),
                lose_sight_ticks: 0,
            };
            monster.invalidate_path();
            events.push(ActionEvent::MonsterAlerted {
                id: monster.id,
                kind: monster.kind,
                at: m_pos3,
                floor: monster.floor,
                by_sight: false,
            });
        }
        return false;
    }

    if monster.kind == MonsterKind::Bloater {
        if explosive {
            return true;
        }
        monster.state = AwarenessState::DyingSwell {
            fuse_ticks: BLOATER_SWELL_FUSE_TICKS,
        };
        monster.invalidate_path();
        events.push(ActionEvent::BloaterSwell {
            id: monster.id,
            at: m_pos3,
            floor: monster.floor,
            fuse_ticks: BLOATER_SWELL_FUSE_TICKS,
        });
        return false;
    }

    monster.alive = false;
    monster.path.clear();
    let overkill = explosive || damage >= prev_hp.saturating_add(2);
    events.push(ActionEvent::MonsterDown {
        id: monster.id,
        kind: monster.kind,
        at: m_pos3,
        floor: monster.floor,
        facing: monster.facing,
        overkill,
    });
    false
}

/// Detonate a Bloater (and iteratively resolve any chained Bloater bursts).
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub fn detonate_bloater(
    world: &mut dyn CombatWorld,
    monsters: &mut [Monster],
    initial_index: usize,
    player_pose: Pose,
    player_health: &mut u8,
    player_iframes: &mut u8,
    player_knockback: &mut Vec2,
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
    overkill_initial: bool,
) {
    let (pz, _, _) = resolve_actor_support(world, player_pose.position, player_pose.floor);
    let player_foot = player_pose.position.extend(pz);
    detonate_bloater_3d(
        world,
        monsters,
        initial_index,
        player_pose,
        player_foot,
        player_health,
        player_iframes,
        player_knockback,
        events,
        noises,
        overkill_initial,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn detonate_bloater_3d(
    world: &mut dyn CombatWorld,
    monsters: &mut [Monster],
    initial_index: usize,
    player_pose: Pose,
    player_foot: Vec3,
    player_health: &mut u8,
    player_iframes: &mut u8,
    player_knockback: &mut Vec2,
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
    overkill_initial: bool,
) {
    let mut queue = vec![(initial_index, overkill_initial)];

    while let Some((idx, overkill)) = queue.pop() {
        if idx >= monsters.len() || !monsters[idx].alive {
            continue;
        }
        monsters[idx].ensure_body_synced(world);
        let id = monsters[idx].id;
        let pos = monsters[idx].pos;
        let pos3 = monsters[idx].body.position;
        let center3 = pos3.with_z(pos3.z + 0.55);
        let facing = monsters[idx].facing;
        let floor = monsters[idx].floor;

        monsters[idx].alive = false;
        monsters[idx].hp = 0;
        monsters[idx].path.clear();

        events.push(ActionEvent::BloaterBurst {
            id,
            at: pos3,
            floor,
            radius: BLOATER_BURST_RADIUS,
        });
        events.push(ActionEvent::MonsterDown {
            id,
            kind: MonsterKind::Bloater,
            at: pos3,
            floor,
            facing,
            overkill,
        });
        noises.push(NoiseEvent {
            origin: pos,
            origin_z: pos3.z,
            floor,
            radius: BLOATER_BURST_NOISE_RADIUS,
            priority: 5,
            kind: NoiseKind::BloaterBurst,
        });

        // Caustic damage to nearby windows and props.
        let cx = pos.x.round() as i32;
        let cy = pos.y.round() as i32;
        for dy in -3..=3 {
            for dx in -3..=3 {
                let tile = Tile3D::new(cx + dx, cy + dy, floor);
                if !in_world_bounds(world, tile) {
                    continue;
                }
                if pos.distance(tile.center()) > BLOATER_BURST_RADIUS {
                    continue;
                }
                if matches!(
                    world.obstruction(tile),
                    TileObstruction::IntactWindow | TileObstruction::LowProp(_)
                ) && let Some(res) = world.damage_tile(tile, DamageKind::CausticBurst)
                    && res.damaged
                {
                    let (tz, _, _) = resolve_tile_support(world, tile);
                    emit_tile_damage_effects(
                        tile,
                        tile.center().extend(tz + 0.5),
                        Vec3::Z,
                        res,
                        events,
                        noises,
                    );
                }
            }
        }

        // Splash damage to nearby monsters (3D distance + 3D occlusion; chain-triggers Bloaters).
        for (j, monster_j) in monsters.iter_mut().enumerate() {
            if j == idx || !monster_j.alive {
                continue;
            }
            monster_j.ensure_body_synced(world);
            let target_foot = monster_j.body.position;
            let target_h = monster_j.spec().height;
            let closest_z = center3
                .z
                .clamp(target_foot.z + 0.15, target_foot.z + target_h - 0.15);
            let target_pt = target_foot.with_z(closest_z);
            let dist3 = center3.distance(target_pt);
            if dist3 > BLOATER_BURST_RADIUS + monster_j.radius() {
                continue;
            }
            if is_blast_occluded_3d(world, center3, target_pt) {
                continue;
            }
            let diff_xy = monster_j.pos - pos;
            let dist_xy = diff_xy.length();
            let dir = if dist_xy > 0.001 {
                diff_xy * (1.0 / dist_xy)
            } else {
                Vec2::new(1.0, 0.0)
            };
            if damage_monster(
                monster_j,
                BLOATER_BURST_MONSTER_DAMAGE,
                dir * 0.20,
                14,
                true,
                events,
            ) {
                queue.push((j, true));
            }
        }

        // Damage & knockback player if caught in the 3D unoccluded caustic burst.
        let p_closest_z = center3.z.clamp(
            player_foot.z + 0.15,
            player_foot.z + PLAYER_SHAPE.height - 0.15,
        );
        let player_pt = player_foot.with_z(p_closest_z);
        let dist3 = center3.distance(player_pt);
        if dist3 <= BLOATER_BURST_RADIUS && !is_blast_occluded_3d(world, center3, player_pt) {
            let (dmg, kb_mag) = if dist3 <= BLOATER_BURST_CORE_RADIUS {
                (2, 0.22)
            } else {
                (1, 0.12)
            };
            let diff_xy = player_pose.position - pos;
            let dist_xy = diff_xy.length();
            let dir = if dist_xy > 0.001 {
                diff_xy * (1.0 / dist_xy)
            } else {
                Vec2::new(1.0, 0.0)
            };
            *player_knockback += dir * kb_mag;
            if *player_iframes == 0 {
                *player_health = player_health.saturating_sub(dmg);
                *player_iframes = PLAYER_HURT_IFRAMES;
                events.push(ActionEvent::PlayerHurt {
                    at: player_foot,
                    floor: player_pose.floor,
                    damage: dmg,
                    remaining_hp: *player_health,
                    from: pos3,
                });
            }
        }
    }
}

/// Propagate noise events to monsters using 3D distance + `1.25` penalty per crossed wall/closed door.
pub fn propagate_noise(
    world: &dyn CombatWorld,
    monsters: &mut [Monster],
    noises: &[NoiseEvent],
    events: &mut Vec<ActionEvent>,
) {
    if noises.is_empty() {
        return;
    }

    for noise in noises {
        let target_tile = Tile3D::from_vec2(noise.origin, noise.floor);
        let noise_pt3 = noise.origin.extend(noise.origin_z + 0.68);
        for m in monsters.iter_mut() {
            if !m.is_targetable() {
                continue;
            }
            m.ensure_body_synced(world);
            let dx = m.pos.x - noise.origin.x;
            let dy = m.pos.y - noise.origin.y;
            let dz_eff = (m.body.position.z - noise.origin_z) * 1.44;
            let dist_3d = (dx * dx + dy * dy + dz_eff * dz_eff).sqrt();
            if dist_3d > noise.radius {
                continue;
            }

            let barriers = count_solid_barriers_3d(world, noise_pt3, m.eye_pos());
            let effective_dist = dist_3d + barriers as f32 * NOISE_BARRIER_PENALTY;
            if effective_dist > noise.radius {
                continue;
            }

            match m.state {
                AwarenessState::Wander { .. } => {
                    m.state = AwarenessState::Investigate {
                        target: target_tile,
                        priority: noise.priority,
                        linger_ticks: 300,
                    };
                    m.invalidate_path();
                    events.push(ActionEvent::MonsterAlerted {
                        id: m.id,
                        kind: m.kind,
                        at: m.body.position,
                        floor: m.floor,
                        by_sight: false,
                    });
                }
                AwarenessState::Investigate {
                    target: old_target,
                    priority,
                    linger_ticks,
                } => {
                    if noise.priority > priority
                        || (noise.priority == priority
                            && target_tile != old_target
                            && linger_ticks < 180)
                    {
                        m.state = AwarenessState::Investigate {
                            target: target_tile,
                            priority: noise.priority,
                            linger_ticks: 300,
                        };
                        m.invalidate_path();
                    }
                }
                AwarenessState::Alert {
                    ref mut last_seen,
                    ref mut lose_sight_ticks,
                } => {
                    if *lose_sight_ticks > 0 {
                        *last_seen = target_tile;
                        *lose_sight_ticks = 0;
                        m.invalidate_path();
                    }
                }
                AwarenessState::Attack { .. } | AwarenessState::DyingSwell { .. } => {}
            }
        }
    }
}

/// Evaluate whether `monster` currently sees `player`, factoring in 3D eye-to-eye raycasts,
/// FOV cone, proximity sense, light level, quiet walking, and connected staircases.
#[allow(dead_code)]
pub fn can_see_player(
    world: &dyn CombatWorld,
    monster: &Monster,
    player: Pose,
    player_quiet_walking: bool,
) -> bool {
    let (pz, _, _) = resolve_actor_support(world, player.position, player.floor);
    can_see_player_3d(
        world,
        monster,
        player,
        player.position.extend(pz),
        player_quiet_walking,
    )
}

pub(crate) fn can_see_player_3d(
    world: &dyn CombatWorld,
    monster: &Monster,
    player: Pose,
    player_foot: Vec3,
    player_quiet_walking: bool,
) -> bool {
    if !monster.is_targetable() {
        return false;
    }
    let monster_foot = monster.resolved_body(world).position;
    let monster_eye = monster.spec().body_shape().eye_at(monster_foot);
    let player_eye = PLAYER_SHAPE.eye_at(player_foot);

    let player_tile = Tile3D::from_vec2(player.position, player.floor);
    let light_scale = 0.42 + 0.58 * world.light_level(player_tile).clamp(0.0, 1.0);
    let quiet_scale = if player_quiet_walking { 0.85 } else { 1.0 };
    let max_range = monster.spec().sight_range * light_scale * quiet_scale;

    let delta3 = player_eye - monster_eye;
    let dist3 = delta3.length();
    if dist3 > max_range {
        return false;
    }

    let in_fov =
        if dist3 <= PROXIMITY_SENSE_RADIUS && (player_foot.z - monster_foot.z).abs() <= 0.85 {
            true
        } else {
            let horiz_len = delta3.xy().length();
            let vert_slope_ok = delta3.z.abs() / horiz_len.max(0.25) <= 1.85;
            let horiz_dir = if horiz_len > 1e-5 {
                delta3.xy() * (1.0 / horiz_len)
            } else {
                monster.facing
            };
            let facing = monster.facing.normalized();
            let fov_cos = match monster.state {
                AwarenessState::Wander { .. } => WANDER_FOV_COS,
                _ => ALERT_FOV_COS,
            };
            vert_slope_ok && (facing.length_squared() < 0.01 || horiz_dir.dot(facing) >= fov_cos)
        };
    if !in_fov {
        return false;
    }

    let clear_3d = match Ray3::from_points(monster_eye, player_eye) {
        None => true,
        Some((ray, d)) => world
            .raycast_filtered(ray, (d - 0.04).max(0.0), QueryFilter::LINE_OF_SIGHT)
            .is_none(),
    };
    if clear_3d {
        return true;
    }

    if monster.floor != player.floor && dist3 <= max_range * 0.75 {
        return can_see_via_stair_glimpse(world, monster, monster_eye, player, player_eye);
    }

    false
}

fn can_see_via_stair_glimpse(
    world: &dyn CombatWorld,
    monster: &Monster,
    monster_eye: Vec3,
    player: Pose,
    player_eye: Vec3,
) -> bool {
    let eye_off = PLAYER_SHAPE.height * 0.85;
    let los_clear = |a: Vec3, b: Vec3| -> bool {
        match Ray3::from_points(a, b) {
            None => true,
            Some((ray, d)) => world
                .raycast_filtered(ray, (d - 0.04).max(0.0), QueryFilter::LINE_OF_SIGHT)
                .is_none(),
        }
    };

    let m_tile = Tile3D::from_vec2(monster.pos, monster.floor);
    for dy in -2..=2 {
        for dx in -2..=2 {
            let candidate = Tile3D::new(m_tile.x + dx, m_tile.y + dy, monster.floor);
            let (links, count) = world.stair_links(candidate);
            for (paired, _) in links.into_iter().take(count).flatten() {
                if paired.floor == player.floor
                    && (monster.pos.distance(candidate.center()) <= 2.2
                        || player.position.distance(paired.center()) <= 2.2)
                {
                    let (cz, _, _) = resolve_tile_support(world, candidate);
                    let (pz, _, _) = resolve_tile_support(world, paired);
                    if los_clear(monster_eye, candidate.center().extend(cz + eye_off))
                        && los_clear(paired.center().extend(pz + eye_off), player_eye)
                    {
                        return true;
                    }
                }
            }
        }
    }

    let p_tile = Tile3D::from_vec2(player.position, player.floor);
    for dy in -2..=2 {
        for dx in -2..=2 {
            let candidate = Tile3D::new(p_tile.x + dx, p_tile.y + dy, player.floor);
            let (links, count) = world.stair_links(candidate);
            for (paired, _) in links.into_iter().take(count).flatten() {
                if paired.floor == monster.floor
                    && (player.position.distance(candidate.center()) <= 2.2
                        || monster.pos.distance(paired.center()) <= 2.2)
                {
                    let (cz, _, _) = resolve_tile_support(world, candidate);
                    let (pz, _, _) = resolve_tile_support(world, paired);
                    if los_clear(player_eye, candidate.center().extend(cz + eye_off))
                        && los_clear(paired.center().extend(pz + eye_off), monster_eye)
                    {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Zero-allocation generation-stamped 3D A* pathfinder (reuses buffers across calls).
pub struct Pathfinder {
    min: Position,
    width: usize,
    height: usize,
    floors: usize,
    generation: u32,
    visited_gen: Vec<u32>,
    closed_gen: Vec<u32>,
    g_score: Vec<u16>,
    parent: Vec<u32>,
    open: BinaryHeap<Reverse<(u16, u16, u32)>>,
}

impl Default for Pathfinder {
    fn default() -> Self {
        Self::new()
    }
}

impl Pathfinder {
    pub fn new() -> Self {
        Self {
            min: Position::new(0, 0),
            width: 0,
            height: 0,
            floors: 0,
            generation: 0,
            visited_gen: Vec::new(),
            closed_gen: Vec::new(),
            g_score: Vec::new(),
            parent: Vec::new(),
            open: BinaryHeap::with_capacity(256),
        }
    }

    fn ensure_capacity(&mut self, world: &dyn CombatWorld) {
        let (min, max, floors) = world.bounds();
        let width = (max.x - min.x + 1).max(0) as usize;
        let height = (max.y - min.y + 1).max(0) as usize;
        let floors = floors as usize;
        let total = width * height * floors;
        if self.width != width
            || self.height != height
            || self.floors != floors
            || self.visited_gen.len() != total
        {
            self.min = min;
            self.width = width;
            self.height = height;
            self.floors = floors;
            self.generation = 0;
            self.visited_gen.clear();
            self.visited_gen.resize(total, 0);
            self.closed_gen.clear();
            self.closed_gen.resize(total, 0);
            self.g_score.clear();
            self.g_score.resize(total, u16::MAX);
            self.parent.clear();
            self.parent.resize(total, 0);
        } else if self.min != min {
            self.min = min;
            self.generation = self.generation.wrapping_add(1);
            if self.generation == 0 {
                self.visited_gen.fill(0);
                self.closed_gen.fill(0);
                self.generation = 1;
            }
        }
    }

    #[inline]
    fn encode(&self, tile: Tile3D) -> Option<u32> {
        let lx = tile.x - self.min.x;
        let ly = tile.y - self.min.y;
        let lf = tile.floor as usize;
        if lx < 0
            || ly < 0
            || (lx as usize) >= self.width
            || (ly as usize) >= self.height
            || lf >= self.floors
        {
            return None;
        }
        Some(((lf * self.height + ly as usize) * self.width + lx as usize) as u32)
    }

    #[inline]
    fn decode(&self, idx: u32) -> Tile3D {
        let idx = idx as usize;
        let layer = self.width * self.height;
        let floor = (idx / layer) as Floor;
        let rem = idx % layer;
        let ly = (rem / self.width) as i32;
        let lx = (rem % self.width) as i32;
        Tile3D::new(self.min.x + lx, self.min.y + ly, floor)
    }

    #[inline]
    fn heuristic(a: Tile3D, b: Tile3D) -> u16 {
        let dx = a.x.abs_diff(b.x);
        let dy = a.y.abs_diff(b.y);
        let (min_d, max_d) = if dx < dy { (dx, dy) } else { (dy, dx) };
        let h2d = max_d
            .saturating_mul(COST_CARDINAL as u32)
            .saturating_add(min_d.saturating_mul((COST_DIAGONAL - COST_CARDINAL) as u32));
        let hf = a.floor.abs_diff(b.floor) as u32 * COST_STAIR as u32;
        h2d.saturating_add(hf).min(u16::MAX as u32) as u16
    }

    /// Computes a 3D A* path from `start` to `goal` (up to `MAX_ASTAR_EXPANSIONS` node pops).
    /// Writes waypoints into `out_path` in pop order (`out_path.last()` is the immediate next step).
    /// Returns `true` if `out_path` contains at least one step toward `goal`.
    pub fn find_path(
        &mut self,
        world: &dyn CombatWorld,
        start: Tile3D,
        goal: Tile3D,
        can_vault_f0: bool,
        out_path: &mut Vec<Tile3D>,
    ) -> bool {
        out_path.clear();
        if start == goal {
            return false;
        }

        self.ensure_capacity(world);
        let (Some(start_idx), Some(goal_idx)) = (self.encode(start), self.encode(goal)) else {
            return false;
        };

        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.visited_gen.fill(0);
            self.closed_gen.fill(0);
            self.generation = 1;
        }
        let cur_gen = self.generation;
        self.open.clear();

        let start_h = Self::heuristic(start, goal);
        self.visited_gen[start_idx as usize] = cur_gen;
        self.g_score[start_idx as usize] = 0;
        self.parent[start_idx as usize] = start_idx;
        self.open.push(Reverse((start_h, start_h, start_idx)));

        let mut best_node = start_idx;
        let mut best_h = start_h;
        let mut expansions = 0usize;

        const DIRS: [(i32, i32, bool); 8] = [
            (1, 0, false),
            (-1, 0, false),
            (0, 1, false),
            (0, -1, false),
            (1, 1, true),
            (1, -1, true),
            (-1, 1, true),
            (-1, -1, true),
        ];

        while let Some(Reverse((_, cur_h, cur_idx))) = self.open.pop() {
            if self.closed_gen[cur_idx as usize] == cur_gen {
                continue;
            }
            self.closed_gen[cur_idx as usize] = cur_gen;

            if cur_h < best_h {
                best_h = cur_h;
                best_node = cur_idx;
            }

            if cur_idx == goal_idx {
                best_node = goal_idx;
                break;
            }

            expansions += 1;
            if expansions >= MAX_ASTAR_EXPANSIONS {
                break;
            }

            let cur_tile = self.decode(cur_idx);
            let cur_g = self.g_score[cur_idx as usize];

            for (dx, dy, diagonal) in DIRS {
                let next_tile = Tile3D::new(cur_tile.x + dx, cur_tile.y + dy, cur_tile.floor);
                let Some(next_idx) = self.encode(next_tile) else {
                    continue;
                };
                if self.closed_gen[next_idx as usize] == cur_gen {
                    continue;
                }

                if diagonal {
                    // Corner-safe diagonal: both adjacent cardinal tiles must be strictly open.
                    let side_x = Tile3D::new(cur_tile.x + dx, cur_tile.y, cur_tile.floor);
                    let side_y = Tile3D::new(cur_tile.x, cur_tile.y + dy, cur_tile.floor);
                    if !world.is_walkable(side_x, false)
                        || !world.is_walkable(side_y, false)
                        || world.obstruction(side_x) != TileObstruction::Open
                        || world.obstruction(side_y) != TileObstruction::Open
                    {
                        continue;
                    }
                }

                let mut step_cost = match world.obstruction(next_tile) {
                    TileObstruction::Open => {
                        if !world.is_walkable(next_tile, false) {
                            continue;
                        }
                        if diagonal {
                            COST_DIAGONAL
                        } else {
                            COST_CARDINAL
                        }
                    }
                    TileObstruction::BrokenWindow => {
                        if diagonal
                            || !can_vault_f0
                            || next_tile.floor != 0
                            || !world.is_walkable(next_tile, true)
                        {
                            continue;
                        }
                        COST_BROKEN_WINDOW
                    }
                    TileObstruction::DoorClosed => {
                        if diagonal || !world.has_floor(next_tile) {
                            continue;
                        }
                        COST_CLOSED_DOOR
                    }
                    TileObstruction::IntactWindow
                    | TileObstruction::LowProp(_)
                    | TileObstruction::Wall(_) => continue,
                };

                // Slope-aware outdoor 3D terrain grade cost & steep cliff rejection.
                if cur_tile.floor == 0
                    && next_tile.floor == 0
                    && world.stair_links(cur_tile).1 == 0
                    && world.stair_links(next_tile).1 == 0
                {
                    let (z_cur, _, _) = resolve_tile_support(world, cur_tile);
                    let (z_next, _, _) = resolve_tile_support(world, next_tile);
                    let step_horiz = if diagonal {
                        std::f32::consts::SQRT_2
                    } else {
                        1.0
                    };
                    if ((z_next - z_cur).abs() / step_horiz) > 0.85 {
                        continue;
                    }
                    let uphill = ((z_next - z_cur).max(0.0) * 12.0).round() as u16;
                    step_cost = step_cost.saturating_add(uphill);
                }

                let tentative_g = cur_g.saturating_add(step_cost);
                if self.visited_gen[next_idx as usize] != cur_gen
                    || tentative_g < self.g_score[next_idx as usize]
                {
                    self.visited_gen[next_idx as usize] = cur_gen;
                    self.g_score[next_idx as usize] = tentative_g;
                    self.parent[next_idx as usize] = cur_idx;
                    let h = Self::heuristic(next_tile, goal);
                    let f = tentative_g.saturating_add(h);
                    self.open.push(Reverse((f, h, next_idx)));
                }
            }

            // Cross-floor stair edges (supports 2-floor and 3-floor StairsBoth shafts).
            let (links, count) = world.stair_links(cur_tile);
            for (paired, _) in links.into_iter().take(count).flatten() {
                if let Some(next_idx) = self.encode(paired) {
                    if self.closed_gen[next_idx as usize] != cur_gen
                        && world.is_walkable(paired, false)
                    {
                        let tentative_g = cur_g.saturating_add(COST_STAIR);
                        if self.visited_gen[next_idx as usize] != cur_gen
                            || tentative_g < self.g_score[next_idx as usize]
                        {
                            self.visited_gen[next_idx as usize] = cur_gen;
                            self.g_score[next_idx as usize] = tentative_g;
                            self.parent[next_idx as usize] = cur_idx;
                            let h = Self::heuristic(paired, goal);
                            let f = tentative_g.saturating_add(h);
                            self.open.push(Reverse((f, h, next_idx)));
                        }
                    }
                }
            }
        }

        if best_node == start_idx {
            return false;
        }

        // Reconstruct in reverse (goal at index 0, next immediate step at `out_path.last()`).
        let mut cursor = best_node;
        let mut safety = 0usize;
        while cursor != start_idx && safety < 512 {
            out_path.push(self.decode(cursor));
            cursor = self.parent[cursor as usize];
            safety += 1;
        }
        !out_path.is_empty()
    }
}

pub struct AiSystem {
    pub pathfinder: Pathfinder,
    last_terrain_version: u64,
    initialized_terrain: bool,
}

impl Default for AiSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl AiSystem {
    pub fn new() -> Self {
        Self {
            pathfinder: Pathfinder::new(),
            last_terrain_version: 0,
            initialized_terrain: false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)]
    pub fn step_monsters(
        &mut self,
        world: &mut dyn CombatWorld,
        monsters: &mut [Monster],
        player: Pose,
        player_radius: f32,
        player_quiet_walking: bool,
        player_health: &mut u8,
        player_iframes: &mut u8,
        player_knockback: &mut Vec2,
        projectiles: &mut Vec<Projectile>,
        next_proj_id: &mut u32,
        seed: u64,
        tick: u64,
        events: &mut Vec<ActionEvent>,
        noises: &mut Vec<NoiseEvent>,
    ) {
        let (pz, psup, _) = resolve_actor_support(world, player.position, player.floor);
        let player_body = BodyState::from_pose_2d(player.position, player.facing, pz, psup);
        self.step_monsters_3d(
            world,
            monsters,
            player,
            player_body,
            player_radius,
            player_quiet_walking,
            player_health,
            player_iframes,
            player_knockback,
            projectiles,
            next_proj_id,
            seed,
            tick,
            events,
            noises,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn step_monsters_3d(
        &mut self,
        world: &mut dyn CombatWorld,
        monsters: &mut [Monster],
        player: Pose,
        player_body: BodyState,
        player_radius: f32,
        player_quiet_walking: bool,
        player_health: &mut u8,
        player_iframes: &mut u8,
        player_knockback: &mut Vec2,
        projectiles: &mut Vec<Projectile>,
        next_proj_id: &mut u32,
        seed: u64,
        tick: u64,
        events: &mut Vec<ActionEvent>,
        noises: &mut Vec<NoiseEvent>,
    ) {
        let noise_start_len = noises.len();
        let player_foot = player_body.position;

        for m in monsters.iter_mut() {
            if m.alive {
                m.ensure_body_synced(world);
            }
        }

        // Detect terrain changes (wall breach, door opened/closed, broken window) -> burst repath!
        let cur_terrain_ver = world.terrain_version();
        let terrain_changed =
            self.initialized_terrain && cur_terrain_ver != self.last_terrain_version;
        self.last_terrain_version = cur_terrain_ver;
        self.initialized_terrain = true;

        let mut repath_budget = if terrain_changed {
            for m in monsters.iter_mut() {
                if m.is_alive() {
                    m.path_dirty = true;
                    m.repath_cooldown = 0;
                }
            }
            BURST_REPATHS_ON_TERRAIN_CHANGE
        } else {
            MAX_REPATHS_PER_TICK
        };

        // Snapshot positions for same-elevation-band local separation steering.
        let mut sep_vectors: Vec<Vec2> = vec![Vec2::ZERO; monsters.len()];
        compute_separation(monsters, &mut sep_vectors);

        let mut pending_bloater_bursts: Vec<usize> = Vec::new();
        let player_tile = Tile3D::from_vec2(player.position, player.floor);

        // Pass 1: timers, swelling fuses, knockback, staggered 15 Hz vision, attacks.
        for i in 0..monsters.len() {
            if !monsters[i].alive {
                continue;
            }

            // Drop monsters standing over collapsed upper-story floors onto the floor below via gravity.
            if monsters[i].floor > 0
                && !world.has_floor(Tile3D::from_vec2(monsters[i].pos, monsters[i].floor))
            {
                let mut target_floor = monsters[i].floor - 1;
                let m_pos = Tile3D::from_vec2(monsters[i].pos, target_floor).pos();
                while target_floor > 0 && !world.has_floor(Tile3D::from_pos(m_pos, target_floor)) {
                    target_floor -= 1;
                }
                monsters[i].body.motion = MotionState::Airborne;
                monsters[i].floor = target_floor;
                monsters[i].invalidate_path();
            }

            // Bloater dying swell countdown.
            if let AwarenessState::DyingSwell { ref mut fuse_ticks } = monsters[i].state {
                if *fuse_ticks > 1 {
                    *fuse_ticks -= 1;
                } else {
                    pending_bloater_bursts.push(i);
                }
                continue;
            }

            monsters[i].ranged_cooldown = monsters[i].ranged_cooldown.saturating_sub(1);
            monsters[i].stair_cooldown = monsters[i].stair_cooldown.saturating_sub(1);
            monsters[i].bash_cooldown = monsters[i].bash_cooldown.saturating_sub(1);
            monsters[i].repath_cooldown = monsters[i].repath_cooldown.saturating_sub(1);

            // Apply residual knockback (or airborne gravity) smoothly with 3D kinematics.
            if monsters[i].knockback.length_squared() > 0.0001 {
                let step = monsters[i].knockback * 0.38;
                step_monster_kinematic(world, &mut monsters[i], step * (1.0 / DT));
                monsters[i].knockback -= step;
                if monsters[i].knockback.length_squared() < 0.0001 {
                    monsters[i].knockback = Vec2::ZERO;
                }
            } else if !monsters[i].body.motion.is_grounded() {
                step_monster_kinematic(world, &mut monsters[i], Vec2::ZERO);
            }

            // Staggered 15 Hz vision check `(tick + id) % 4 == 0`.
            if (tick + monsters[i].id.0 as u64).is_multiple_of(4) {
                let sees = can_see_player_3d(
                    world,
                    &monsters[i],
                    player,
                    player_foot,
                    player_quiet_walking,
                );
                monsters[i].has_los = sees;
                if sees {
                    match monsters[i].state {
                        AwarenessState::Wander { .. } | AwarenessState::Investigate { .. } => {
                            monsters[i].state = AwarenessState::Alert {
                                last_seen: player_tile,
                                lose_sight_ticks: 0,
                            };
                            monsters[i].invalidate_path();
                            events.push(ActionEvent::MonsterAlerted {
                                id: monsters[i].id,
                                kind: monsters[i].kind,
                                at: monsters[i].body.position,
                                floor: monsters[i].floor,
                                by_sight: true,
                            });
                        }
                        AwarenessState::Alert {
                            ref mut last_seen,
                            ref mut lose_sight_ticks,
                        } => {
                            *last_seen = player_tile;
                            *lose_sight_ticks = 0;
                        }
                        AwarenessState::Attack { .. } | AwarenessState::DyingSwell { .. } => {}
                    }
                }
            } else if monsters[i].has_los {
                if let AwarenessState::Alert {
                    ref mut last_seen,
                    ref mut lose_sight_ticks,
                } = monsters[i].state
                {
                    *last_seen = player_tile;
                    *lose_sight_ticks = 0;
                }
            }

            // Hitstun pauses voluntary action/movement.
            if monsters[i].hitstun_ticks > 0 {
                monsters[i].hitstun_ticks -= 1;
                continue;
            }

            // Active attack state machine (Windup -> Strike/Bolt -> Recovery -> Alert).
            if matches!(monsters[i].state, AwarenessState::Attack { .. }) {
                step_monster_attack(
                    world,
                    &mut monsters[i],
                    player,
                    player_foot,
                    player_health,
                    player_iframes,
                    player_knockback,
                    projectiles,
                    next_proj_id,
                    events,
                    noises,
                );
                continue;
            }

            // Alert state: lose-sight timer and attack initiation.
            if let AwarenessState::Alert {
                last_seen,
                ref mut lose_sight_ticks,
            } = monsters[i].state
            {
                if !monsters[i].has_los {
                    *lose_sight_ticks = lose_sight_ticks.saturating_add(1);
                    let reached_last = monsters[i].floor == last_seen.floor
                        && monsters[i].pos.distance(last_seen.center()) <= 0.75;
                    if *lose_sight_ticks >= ALERT_LOSE_SIGHT_TICKS
                        || (reached_last && *lose_sight_ticks >= 30)
                    {
                        monsters[i].state = AwarenessState::Investigate {
                            target: last_seen,
                            priority: 2,
                            linger_ticks: 120,
                        };
                        monsters[i].invalidate_path();
                        continue;
                    }
                }

                // Check whether to initiate an attack.
                if try_start_attack(
                    world,
                    &mut monsters[i],
                    player,
                    player_foot,
                    player_radius,
                    events,
                ) {
                    continue;
                }
            }

            // Movement & steering for Wander / Investigate / Alert.
            let sep = sep_vectors[i];
            self.step_monster_movement(
                world,
                &mut monsters[i],
                player,
                player_foot,
                sep,
                seed,
                tick,
                &mut repath_budget,
                events,
                noises,
            );
        }

        // Resolve any Bloaters whose swell fuse completed this tick.
        for bloater_idx in pending_bloater_bursts {
            detonate_bloater_3d(
                world,
                monsters,
                bloater_idx,
                player,
                player_foot,
                player_health,
                player_iframes,
                player_knockback,
                events,
                noises,
                false,
            );
        }

        // Propagate any noises emitted during AI step (Shooter shots, Bloater bursts, door bashing).
        if noises.len() > noise_start_len {
            let emitted: Vec<NoiseEvent> = noises[noise_start_len..].to_vec();
            propagate_noise(world, monsters, &emitted, events);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn step_monster_movement(
        &mut self,
        world: &mut dyn CombatWorld,
        monster: &mut Monster,
        player: Pose,
        player_foot: Vec3,
        separation: Vec2,
        seed: u64,
        tick: u64,
        repath_budget: &mut usize,
        events: &mut Vec<ActionEvent>,
        noises: &mut Vec<NoiseEvent>,
    ) {
        let spec = monster.spec();

        // Shooter standoff movement when Alert with line of sight.
        if monster.kind == MonsterKind::Shooter
            && matches!(monster.state, AwarenessState::Alert { .. })
            && monster.has_los
            && (monster.body.position.z - player_foot.z).abs() <= 0.95
        {
            let to_player = player.position - monster.pos;
            let dist = to_player.length();
            if dist > 0.01 {
                let dir_to_player = to_player * (1.0 / dist);
                monster.facing = dir_to_player;
                let desired = if dist < SHOOTER_BACKPEDAL_RANGE {
                    Vec2::new(-dir_to_player.x, -dir_to_player.y)
                } else if dist > SHOOTER_STANDOFF_MAX {
                    dir_to_player
                } else if dist >= SHOOTER_STANDOFF_MIN {
                    // Gentle strafe inside standoff band while waiting on cooldown.
                    let sign = if ((monster.id.0 as u64 + tick / 90) & 1) == 0 {
                        1.0
                    } else {
                        -1.0
                    };
                    Vec2::new(-dir_to_player.y * sign, dir_to_player.x * sign) * 0.55
                } else {
                    Vec2::ZERO
                };
                let move_dir = (desired + separation * 0.5).normalized();
                if move_dir.length_squared() > 0.01 {
                    let speed = if dist < SHOOTER_BACKPEDAL_RANGE {
                        spec.chase_speed * 0.78
                    } else {
                        spec.chase_speed
                    };
                    step_monster_kinematic(world, monster, move_dir * speed);
                }
                return;
            }
        }

        // Determine movement target tile & speed by awareness state.
        let home = monster.home;
        let floor = monster.floor;
        let id = monster.id;
        let (goal_tile, exact_goal_pos, speed) = match monster.state {
            AwarenessState::Wander {
                ref mut goal,
                ref mut pause_ticks,
            } => {
                if let Some(g) = *goal {
                    if floor == g.floor && monster.pos.distance(g.center()) <= 0.45 {
                        *goal = None;
                        *pause_ticks =
                            45 + (mix64(seed ^ tick ^ (id.0 as u64).wrapping_mul(31)) % 75) as u16;
                        monster.path.clear();
                        return;
                    }
                    (g, g.center(), spec.wander_speed)
                } else if *pause_ticks > 0 {
                    *pause_ticks -= 1;
                    return;
                } else if let Some(next_goal) = pick_wander_goal(world, home, floor, id, seed, tick)
                {
                    *goal = Some(next_goal);
                    monster.invalidate_path();
                    (next_goal, next_goal.center(), spec.wander_speed)
                } else {
                    *pause_ticks = 45;
                    return;
                }
            }
            AwarenessState::Investigate {
                target,
                ref mut linger_ticks,
                ..
            } => {
                if *linger_ticks == 0 {
                    monster.state = AwarenessState::Wander {
                        goal: None,
                        pause_ticks: 30,
                    };
                    monster.invalidate_path();
                    return;
                }
                *linger_ticks -= 1;
                if monster.floor == target.floor && monster.pos.distance(target.center()) <= 0.75 {
                    // Arrived at sound source: look around briefly before settling back to Wander.
                    *linger_ticks = (*linger_ticks).min(90);
                    monster.facing = monster.facing.rotate(0.045).normalized();
                    monster.body.heading = monster.facing.y.atan2(monster.facing.x);
                    monster.path.clear();
                    return;
                }
                (target, target.center(), spec.investigate_speed)
            }
            AwarenessState::Alert { last_seen, .. } => {
                let exact = if monster.has_los && monster.floor == player.floor {
                    player.position
                } else {
                    last_seen.center()
                };
                (last_seen, exact, spec.chase_speed)
            }
            AwarenessState::Attack { .. } | AwarenessState::DyingSwell { .. } => return,
        };

        // Direct-steering fast path: same floor, <= 5.0 tiles, unobstructed open corridor.
        if monster.floor == goal_tile.floor
            && monster.pos.distance(exact_goal_pos) <= DIRECT_STEER_MAX_DIST
            && is_corridor_clear(
                world,
                monster.pos,
                exact_goal_pos,
                monster.floor,
                spec.radius * 0.85,
            )
        {
            let desired = (exact_goal_pos - monster.pos).normalized();
            let blended = (desired + separation * 0.65).normalized();
            if desired.length_squared() > 0.01 {
                monster.facing = desired;
            }
            step_monster_kinematic(world, monster, blended * speed);
            return;
        }

        // Budgeted 3D A* repathing.
        let need_repath = monster.path_dirty
            || monster.path.is_empty()
            || (monster.repath_cooldown == 0 && monster.path.first().copied() != Some(goal_tile));
        if need_repath && *repath_budget > 0 {
            let start_tile = Tile3D::from_vec2(monster.pos, monster.floor);
            let mut tmp_path = std::mem::take(&mut monster.path);
            let found = self.pathfinder.find_path(
                world,
                start_tile,
                goal_tile,
                spec.can_vault_f0,
                &mut tmp_path,
            );
            monster.path = tmp_path;
            monster.path_dirty = false;
            monster.repath_cooldown = 14 + ((monster.id.0 as u8).wrapping_mul(3) % 7);
            *repath_budget -= 1;

            if !found && matches!(monster.state, AwarenessState::Wander { .. }) {
                monster.state = AwarenessState::Wander {
                    goal: None,
                    pause_ticks: 40,
                };
                return;
            }
        }

        // Follow waypoints along A* path.
        let Some(next_wp) = monster.path.last().copied() else {
            return;
        };

        // Pop waypoint if 3D stair kinematics already brought monster onto next_wp's floor.
        if next_wp.floor == monster.floor
            && monster.pos.distance(next_wp.center()) <= 0.55
            && monster.path.len() > 1
        {
            monster.path.pop();
            return;
        }

        // Stair floor transition: only cross stairs when the next waypoint is on the other floor
        // AND stair_cooldown == 0 (guarantees zero oscillation!).
        if next_wp.floor != monster.floor {
            let cur_tile = Tile3D::from_vec2(monster.pos, monster.floor);
            let (links, count) = world.stair_links(cur_tile);
            if let Some((paired, landing_dir)) = links
                .into_iter()
                .take(count)
                .flatten()
                .find(|(paired, _)| *paired == next_wp)
            {
                if monster.stair_cooldown == 0 {
                    let exit_dir = pick_stair_exit_dir(world, paired, landing_dir);
                    monster.floor = paired.floor;
                    let nudged = paired.center()
                        + Vec2::new(exit_dir.0 as f32 * 0.72, exit_dir.1 as f32 * 0.72);
                    let exit_tile = Tile3D::new(
                        paired.x + exit_dir.0 as i32,
                        paired.y + exit_dir.1 as i32,
                        paired.floor,
                    );
                    let can_vault = spec.can_vault_f0 && monster.floor == 0;
                    monster.pos = if world.is_walkable(exit_tile, can_vault) {
                        nudged
                    } else {
                        paired.center()
                    };
                    let (z, sup, _) = resolve_actor_support(world, monster.pos, monster.floor);
                    monster.body = BodyState::from_pose_2d(monster.pos, monster.facing, z, sup);
                    monster.body_synced = true;
                    monster.stair_cooldown = STAIR_COOLDOWN_TICKS;
                    monster.path.pop();
                }
                return;
            }
            // Monster got nudged slightly off the stair tile; steer onto the stair tile linked to next_wp.
            let (rev_links, rev_count) = world.stair_links(next_wp);
            if let Some((paired_origin, _)) = rev_links
                .into_iter()
                .take(rev_count)
                .flatten()
                .find(|(origin, _)| origin.floor == monster.floor)
            {
                let to_stair = (paired_origin.center() - monster.pos).normalized();
                if to_stair.length_squared() > 0.01 {
                    monster.facing = to_stair;
                    step_monster_kinematic(world, monster, to_stair * speed);
                }
                return;
            }
            monster.path_dirty = true;
            return;
        }

        // Closed door bashing along the path!
        if world.obstruction(next_wp) == TileObstruction::DoorClosed {
            let to_door = next_wp.center() - monster.pos;
            let dist = to_door.length();
            if dist > 0.01 {
                monster.facing = to_door * (1.0 / dist);
            }
            if dist <= 0.95 {
                if monster.bash_cooldown == 0 {
                    monster.bash_cooldown = (360 / spec.bash_power.max(1) as u16).max(12) as u8;
                    if let Some(res) = world.damage_tile(next_wp, DamageKind::MonsterMelee)
                        && res.damaged
                    {
                        let (tz, _, _) = resolve_tile_support(world, next_wp);
                        emit_tile_damage_effects(
                            next_wp,
                            next_wp.center().extend(tz + 0.5),
                            Vec3::Z,
                            res,
                            events,
                            noises,
                        );
                    }
                }
            } else {
                let dir = to_door * (1.0 / dist);
                step_monster_kinematic(world, monster, dir * speed);
            }
            return;
        }

        // Advance along same-floor waypoint.
        let wp_center = next_wp.center();
        let to_wp = wp_center - monster.pos;
        let dist_to_wp = to_wp.length();
        if dist_to_wp <= 0.28 {
            monster.path.pop();
            return;
        }

        let desired = to_wp * (1.0 / dist_to_wp);
        monster.facing = desired;

        let cur_tile = Tile3D::from_vec2(monster.pos, monster.floor);
        let speed_mult = if world.obstruction(next_wp) == TileObstruction::BrokenWindow
            || world.obstruction(cur_tile) == TileObstruction::BrokenWindow
        {
            0.65
        } else {
            1.0
        };
        let sep_weight = if world.obstruction(next_wp) == TileObstruction::BrokenWindow {
            0.15
        } else {
            0.55
        };
        let blended = (desired + separation * sep_weight).normalized();
        step_monster_kinematic(world, monster, blended * (speed * speed_mult));
    }
}

fn pick_stair_exit_dir(world: &dyn CombatWorld, paired: Tile3D, landing_dir: (i8, i8)) -> (i8, i8) {
    let (pz, _, _) = resolve_tile_support(world, paired);
    let origin = paired.center().extend(pz + 0.30);
    let is_clear = |d: (i8, i8)| -> bool {
        if d == (0, 0) {
            return false;
        }
        let target_tile = Tile3D::new(paired.x + d.0 as i32, paired.y + d.1 as i32, paired.floor);
        if !world.is_walkable(target_tile, false) {
            return false;
        }
        let ray = Ray3::new(origin, Vec3::new(d.0 as f32, d.1 as f32, 0.0));
        world
            .raycast_filtered(ray, 0.85, QueryFilter::LINE_OF_SIGHT)
            .is_none()
    };

    if is_clear(landing_dir) {
        return landing_dir;
    }
    let ortho_a = (landing_dir.1, -landing_dir.0);
    let ortho_b = (-landing_dir.1, landing_dir.0);
    let (first, second) = if ortho_b.1 < ortho_a.1 {
        (ortho_b, ortho_a)
    } else {
        (ortho_a, ortho_b)
    };
    let opp = (-landing_dir.0, -landing_dir.1);
    for d in [first, second, opp] {
        if is_clear(d) {
            return d;
        }
    }
    landing_dir
}

fn step_monster_kinematic(world: &dyn CombatWorld, monster: &mut Monster, vel_xy: Vec2) {
    monster.ensure_body_synced(world);
    let spec = monster.spec();
    let params = MoveParams {
        can_vault_broken_windows: spec.can_vault_f0 && monster.floor == 0,
        avoid_water: true,
        doorway_assist: true,
        ..MoveParams::default()
    };
    let res = move_kinematic_with_horizontal_vel(
        world,
        spec.body_shape(),
        monster.body,
        vel_xy,
        DT,
        &params,
    );
    monster.body = res.state;
    monster.pos = monster.body.position.xy();
    if monster.facing.length_squared() > 1e-6 {
        monster.body.heading = monster.facing.y.atan2(monster.facing.x);
    }
    monster.floor = semantic_floor_for_body(world, monster.body, monster.floor);
}

fn try_start_attack(
    world: &dyn CombatWorld,
    monster: &mut Monster,
    player: Pose,
    player_foot: Vec3,
    player_radius: f32,
    events: &mut Vec<ActionEvent>,
) -> bool {
    let spec = monster.spec();
    let monster_foot = monster.body.position;

    if monster.kind == MonsterKind::Shooter {
        let shooter_muzzle3 = monster.muzzle_pos();
        let player_chest3 = player_foot.with_z(player_foot.z + WEAPON_MUZZLE_HEIGHT);
        let delta3 = player_chest3 - shooter_muzzle3;
        let dist3 = delta3.length();
        if monster.ranged_cooldown > 0 || !monster.has_los || dist3 > spec.attack_range {
            return false;
        }
        let aim_dir3 = delta3.normalized();
        let aim_dir = aim_dir3.xy().normalized();
        if aim_dir.length_squared() < 0.01 {
            return false;
        }
        monster.facing = aim_dir;
        monster.body.heading = aim_dir.y.atan2(aim_dir.x);
        monster.state = AwarenessState::Attack {
            phase: AttackPhase::Windup,
            aim_dir,
            aim_dir3,
            target_floor: player.floor,
            ticks_left: spec.windup_ticks,
            total_ticks: spec.windup_ticks,
        };
        events.push(ActionEvent::MonsterTelegraph {
            id: monster.id,
            kind: monster.kind,
            at: monster_foot,
            floor: monster.floor,
            dir: aim_dir,
            dir3: aim_dir3,
            duration_ticks: spec.windup_ticks,
        });
        return true;
    }

    // Melee monsters require vertical band overlap, attack range, and unobstructed 3D chest LOS.
    if (monster_foot.z - player_foot.z).abs() > 0.78 {
        return false;
    }
    let dist_xy = monster.pos.distance(player.position);
    if dist_xy > spec.attack_range + player_radius * 0.25 {
        return false;
    }
    let m_chest = monster_foot.with_z(monster_foot.z + WEAPON_MUZZLE_HEIGHT);
    let p_chest = player_foot.with_z(player_foot.z + WEAPON_MUZZLE_HEIGHT);
    if let Some((ray, d)) = Ray3::from_points(m_chest, p_chest)
        && world
            .raycast_filtered(ray, (d - 0.05).max(0.0), QueryFilter::LINE_OF_SIGHT)
            .is_some()
    {
        return false;
    }

    let aim_dir = if dist_xy > 0.001 {
        (player.position - monster.pos) * (1.0 / dist_xy)
    } else {
        monster.facing
    };
    let aim_dir3 = (p_chest - m_chest).normalized();
    let aim_dir3 = if aim_dir3 == Vec3::ZERO {
        aim_dir.extend(0.0)
    } else {
        aim_dir3
    };
    monster.facing = aim_dir;
    monster.body.heading = aim_dir.y.atan2(aim_dir.x);
    monster.state = AwarenessState::Attack {
        phase: AttackPhase::Windup,
        aim_dir,
        aim_dir3,
        target_floor: player.floor,
        ticks_left: spec.windup_ticks,
        total_ticks: spec.windup_ticks,
    };
    events.push(ActionEvent::MonsterTelegraph {
        id: monster.id,
        kind: monster.kind,
        at: monster_foot,
        floor: monster.floor,
        dir: aim_dir,
        dir3: aim_dir3,
        duration_ticks: spec.windup_ticks,
    });
    true
}

#[allow(clippy::too_many_arguments)]
fn step_monster_attack(
    world: &mut dyn CombatWorld,
    monster: &mut Monster,
    player: Pose,
    player_foot: Vec3,
    player_health: &mut u8,
    player_iframes: &mut u8,
    player_knockback: &mut Vec2,
    projectiles: &mut Vec<Projectile>,
    next_proj_id: &mut u32,
    events: &mut Vec<ActionEvent>,
    noises: &mut Vec<NoiseEvent>,
) {
    let spec = monster.spec();
    let AwarenessState::Attack {
        phase,
        mut aim_dir,
        mut aim_dir3,
        mut target_floor,
        mut ticks_left,
        total_ticks,
    } = monster.state
    else {
        return;
    };

    let monster_foot = monster.body.position;

    match phase {
        AttackPhase::Windup => {
            // Shooter tracks player for first 22 ticks (`ticks_left > 11`), then locks aim for final 11 ticks!
            if monster.kind == MonsterKind::Shooter && ticks_left > SHOOTER_WINDUP_LOCK_TICKS {
                if can_see_player_3d(world, monster, player, player_foot, false) {
                    let shooter_muzzle3 = monster.muzzle_pos();
                    let player_chest3 = player_foot.with_z(player_foot.z + WEAPON_MUZZLE_HEIGHT);
                    let new_dir3 = (player_chest3 - shooter_muzzle3).normalized();
                    let new_dir2 = new_dir3.xy().normalized();
                    if new_dir2.length_squared() > 0.01 {
                        aim_dir = new_dir2;
                        aim_dir3 = new_dir3;
                        target_floor = player.floor;
                        monster.facing = aim_dir;
                        monster.body.heading = aim_dir.y.atan2(aim_dir.x);
                    }
                }
            } else if monster.kind != MonsterKind::Shooter
                && ticks_left > total_ticks / 2
                && (monster_foot.z - player_foot.z).abs() <= 0.78
            {
                let to_player = (player.position - monster.pos).normalized();
                if to_player.length_squared() > 0.01 {
                    aim_dir = to_player;
                    aim_dir3 = to_player.extend(0.0);
                    monster.facing = aim_dir;
                    monster.body.heading = aim_dir.y.atan2(aim_dir.x);
                }
            }

            if ticks_left > 1 {
                ticks_left -= 1;
                monster.state = AwarenessState::Attack {
                    phase: AttackPhase::Windup,
                    aim_dir,
                    aim_dir3,
                    target_floor,
                    ticks_left,
                    total_ticks,
                };
                return;
            }

            // Windup complete -> execute strike or spawn ShooterBolt!
            match monster.kind {
                MonsterKind::Shooter => {
                    let shooter_muzzle3 = monster.muzzle_pos();
                    let velocity = aim_dir3 * SHOOTER_BOLT_SPEED;
                    // Bolts can leave the muzzle above/below the shooter's nominal floor;
                    // use the authoritative 3D point for the projectile's semantic floor.
                    let bolt_floor = semantic_floor_at_point(world, shooter_muzzle3);
                    let id = ProjectileId(*next_proj_id);
                    *next_proj_id = next_proj_id.wrapping_add(1);
                    projectiles.push(Projectile {
                        id,
                        kind: ProjectileKind::ShooterBolt,
                        owner_is_player: false,
                        position: shooter_muzzle3,
                        velocity,
                        floor: bolt_floor,
                        radius: SHOOTER_BOLT_RADIUS,
                        remaining_range: SHOOTER_BOLT_RANGE,
                    });
                    events.push(ActionEvent::ProjectileLaunched {
                        id,
                        kind: ProjectileKind::ShooterBolt,
                        from: shooter_muzzle3,
                        floor: bolt_floor,
                        dir: aim_dir,
                        velocity,
                    });
                    noises.push(NoiseEvent {
                        origin: monster.pos,
                        origin_z: monster_foot.z,
                        floor: monster.floor,
                        radius: SHOOTER_SHOT_NOISE_RADIUS,
                        priority: 3,
                        kind: NoiseKind::ShooterShot,
                    });
                    monster.ranged_cooldown = SHOOTER_RANGED_COOLDOWN;
                }
                MonsterKind::Bloater => {
                    let dist = monster.pos.distance(player.position);
                    let m_chest = monster_foot.with_z(monster_foot.z + WEAPON_MUZZLE_HEIGHT);
                    let p_chest = player_foot.with_z(player_foot.z + WEAPON_MUZZLE_HEIGHT);
                    let blocked = Ray3::from_points(m_chest, p_chest).is_some_and(|(ray, d)| {
                        world
                            .raycast_filtered(ray, (d - 0.05).max(0.0), QueryFilter::LINE_OF_SIGHT)
                            .is_some()
                    });
                    if (monster_foot.z - player_foot.z).abs() <= 0.78
                        && dist <= BLOATER_SLAM_RANGE
                        && !blocked
                    {
                        let kb_dir = if dist > 0.001 {
                            (player.position - monster.pos) * (1.0 / dist)
                        } else {
                            aim_dir
                        };
                        *player_knockback += kb_dir * spec.attack_knockback;
                        if *player_iframes == 0 {
                            *player_health = player_health.saturating_sub(spec.attack_damage);
                            *player_iframes = PLAYER_HURT_IFRAMES;
                            events.push(ActionEvent::PlayerHurt {
                                at: player_foot,
                                floor: player.floor,
                                damage: spec.attack_damage,
                                remaining_hp: *player_health,
                                from: monster_foot,
                            });
                        }
                    }
                }
                MonsterKind::Shambler | MonsterKind::Brisk => {
                    let dist = monster.pos.distance(player.position);
                    let to_player = if dist > 0.001 {
                        (player.position - monster.pos) * (1.0 / dist)
                    } else {
                        aim_dir
                    };
                    let m_chest = monster_foot.with_z(monster_foot.z + WEAPON_MUZZLE_HEIGHT);
                    let p_chest = player_foot.with_z(player_foot.z + WEAPON_MUZZLE_HEIGHT);
                    let blocked = Ray3::from_points(m_chest, p_chest).is_some_and(|(ray, d)| {
                        world
                            .raycast_filtered(ray, (d - 0.05).max(0.0), QueryFilter::LINE_OF_SIGHT)
                            .is_some()
                    });
                    if (monster_foot.z - player_foot.z).abs() <= 0.78
                        && dist <= spec.attack_range + 0.18
                        && (dist <= 0.55 || to_player.dot(aim_dir) >= 0.0)
                        && !blocked
                    {
                        *player_knockback += to_player * spec.attack_knockback;
                        if *player_iframes == 0 {
                            *player_health = player_health.saturating_sub(spec.attack_damage);
                            *player_iframes = PLAYER_HURT_IFRAMES;
                            events.push(ActionEvent::PlayerHurt {
                                at: player_foot,
                                floor: player.floor,
                                damage: spec.attack_damage,
                                remaining_hp: *player_health,
                                from: monster_foot,
                            });
                        }
                    }
                }
            }

            monster.state = AwarenessState::Attack {
                phase: AttackPhase::Recovery,
                aim_dir,
                aim_dir3,
                target_floor,
                ticks_left: spec.recovery_ticks,
                total_ticks: spec.recovery_ticks,
            };
        }
        AttackPhase::Recovery => {
            if ticks_left > 1 {
                monster.state = AwarenessState::Attack {
                    phase: AttackPhase::Recovery,
                    aim_dir,
                    aim_dir3,
                    target_floor,
                    ticks_left: ticks_left - 1,
                    total_ticks,
                };
            } else {
                monster.state = AwarenessState::Alert {
                    last_seen: Tile3D::from_vec2(player.position, player.floor),
                    lose_sight_ticks: 0,
                };
            }
        }
    }
}

fn pick_wander_goal(
    world: &dyn CombatWorld,
    home: Tile3D,
    floor: Floor,
    id: MonsterId,
    seed: u64,
    tick: u64,
) -> Option<Tile3D> {
    let base_hash =
        mix64(seed ^ tick.wrapping_mul(0x9e37_79b9) ^ (id.0 as u64).wrapping_mul(0xbf58_476d));
    for attempt in 0..8u64 {
        let h = mix64(base_hash ^ attempt.wrapping_mul(0x94d0_49bb));
        let dx = ((h & 0xff) % 9) as i32 - 4;
        let dy = (((h >> 8) & 0xff) % 9) as i32 - 4;
        if dx == 0 && dy == 0 {
            continue;
        }
        let candidate = Tile3D::new(home.x + dx, home.y + dy, floor);
        if in_world_bounds(world, candidate) && world.is_walkable(candidate, false) {
            return Some(candidate);
        }
    }
    None
}

fn compute_separation(monsters: &[Monster], out: &mut [Vec2]) {
    for i in 0..monsters.len() {
        if !monsters[i].is_targetable() {
            continue;
        }
        let mut push = Vec2::ZERO;
        let r_i = monsters[i].radius();
        for j in 0..monsters.len() {
            if i == j
                || !monsters[j].is_targetable()
                || (monsters[i].body.position.z - monsters[j].body.position.z).abs() > 0.95
            {
                continue;
            }
            let min_sep = r_i + monsters[j].radius() + 0.18;
            let diff = monsters[i].pos - monsters[j].pos;
            let dist_sq = diff.length_squared();
            if dist_sq < min_sep * min_sep {
                if dist_sq > 0.0001 {
                    let dist = dist_sq.sqrt();
                    let weight = ((min_sep - dist) / min_sep) * 0.75;
                    push += diff * (weight / dist);
                } else {
                    let angle = ((i.wrapping_mul(7) + j.wrapping_mul(13)) & 7) as f32
                        * std::f32::consts::FRAC_PI_4;
                    push += Vec2::new(1.0, 0.0).rotate(angle) * 0.5;
                }
            }
        }
        out[i] = push;
    }
}

fn is_corridor_clear(
    world: &dyn CombatWorld,
    from: Vec2,
    to: Vec2,
    floor: Floor,
    half_width: f32,
) -> bool {
    let delta = to - from;
    let dist = delta.length();
    if dist <= 0.05 {
        return true;
    }
    let dir = delta * (1.0 / dist);
    let perp = Vec2::new(-dir.y, dir.x) * half_width;
    let samples = (dist / 0.25).ceil() as usize;
    let mut prev_z = resolve_actor_support(world, from, floor).0;

    for i in 1..=samples {
        let t = (i as f32 * 0.25).min(dist);
        let step_dist = if i == samples {
            (dist - (i - 1) as f32 * 0.25).max(0.05)
        } else {
            0.25
        };
        let center = from + dir * t;
        for offset in [Vec2::ZERO, perp, Vec2::new(-perp.x, -perp.y)] {
            let p = center + offset;
            let tile = Tile3D::from_vec2(p, floor);
            if !in_world_bounds(world, tile)
                || !world.is_walkable(tile, false)
                || world.obstruction(tile) != TileObstruction::Open
            {
                return false;
            }
        }
        let (cur_z, _, _) = resolve_actor_support(world, center, floor);
        if (cur_z - prev_z).abs() > 0.35 * step_dist.max(0.25) {
            return false;
        }
        prev_z = cur_z;
    }
    true
}

fn count_solid_barriers_3d(world: &dyn CombatWorld, from: Vec3, to: Vec3) -> u32 {
    let Some((ray, total_dist)) = Ray3::from_points(from, to) else {
        return 0;
    };
    let mut t_cursor = 0.0_f32;
    let mut count = 0u32;
    let mut last_tile: Option<Tile3D> = None;

    for _ in 0..8 {
        if t_cursor >= total_dist - 0.05 {
            break;
        }
        let seg_ray = Ray3 {
            origin: ray.at(t_cursor),
            dir: ray.dir,
        };
        let Some(hit) = world.raycast_filtered(
            seg_ray,
            total_dist - t_cursor - 0.04,
            QueryFilter::LINE_OF_SIGHT,
        ) else {
            break;
        };
        let hit_tile = match hit.surface {
            crate::spatial::HitSurface::Collider(
                crate::spatial::ColliderKind::Wall { tile }
                | crate::spatial::ColliderKind::ClosedDoor { tile },
            ) => Some(tile),
            _ => None,
        };
        if hit_tile.is_none() || hit_tile != last_tile {
            count += 1;
            last_tile = hit_tile;
        }
        t_cursor += hit.distance + 0.20;
    }
    count
}

#[inline]
fn in_world_bounds(world: &dyn CombatWorld, tile: Tile3D) -> bool {
    let (min, max, floors) = world.bounds();
    tile.x >= min.x && tile.x <= max.x && tile.y >= min.y && tile.y <= max.y && tile.floor < floors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Surface;
    use crate::action::types::{BuildingId, Material, TileDamageResult, TileKind};
    use crate::spatial::{
        Aabb3, Collider, ColliderKind, HitSurface, SLAB_THICKNESS, StairGeometry, SupportSurface,
        SurfaceQuad, TerrainSample, TerrainTriangle, WALL_HALF_THICKNESS, WALL_HEIGHT,
        WINDOW_LINTEL_Z, WINDOW_SILL_HEIGHT, world_to_tile_coord,
    };
    use std::collections::HashMap;

    struct TestWorld {
        width: i32,
        height: i32,
        floors: u8,
        version: u64,
        tiles: HashMap<Tile3D, (TileObstruction, u16)>,
        stairs: HashMap<Tile3D, (Tile3D, (i8, i8))>,
        lights: HashMap<Tile3D, f32>,
        default_light: f32,
    }

    impl TestWorld {
        fn new(width: i32, height: i32, floors: u8) -> Self {
            Self {
                width,
                height,
                floors,
                version: 0,
                tiles: HashMap::new(),
                stairs: HashMap::new(),
                lights: HashMap::new(),
                default_light: 1.0,
            }
        }

        fn set_tile(&mut self, tile: Tile3D, obs: TileObstruction, hp: u16) {
            self.tiles.insert(tile, (obs, hp));
            self.version += 1;
        }

        fn clear_tile(&mut self, tile: Tile3D) {
            self.tiles.remove(&tile);
            self.version += 1;
        }

        fn link_stairs(
            &mut self,
            bottom: Tile3D,
            top: Tile3D,
            up_dir: (i8, i8),
            down_dir: (i8, i8),
        ) {
            self.stairs.insert(bottom, (top, up_dir));
            self.stairs.insert(top, (bottom, down_dir));
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

    impl SpatialWorld for TestWorld {
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
            let t0 = Tile3D::from_pos(p, 0);
            if let Some(&(paired, up_dir)) = self.stairs.get(&t0)
                && paired.floor == 1
            {
                let down_dir = self
                    .stairs
                    .get(&paired)
                    .map_or((-up_dir.0, -up_dir.1), |&(_, d)| d);
                let geom = StairGeometry::new(t0, 0.0, down_dir, up_dir);
                let (z, normal) = geom.sample(xy);
                out.push(SupportSurface {
                    id: SupportId::Stair(t0),
                    z,
                    normal,
                    walkable: true,
                    is_stair: true,
                    floor: 0,
                    surface: Surface::WoodFloor,
                    water_z: None,
                });
            } else {
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
                if self.stairs.contains_key(&t) {
                    continue;
                }
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
                        if fl >= 1 && !self.stairs.contains_key(&tile) {
                            let (mut rx0, mut rx1, mut ry0, mut ry1) =
                                (fx - 0.5, fx + 0.5, fy - 0.5, fy + 0.5);
                            for (dx, dy) in [(-1_i32, 0_i32), (1, 0), (0, -1), (0, 1)] {
                                let adj_stair = Tile3D::new(tx + dx, ty + dy, fl);
                                if self.stairs.contains_key(&adj_stair) {
                                    if dx < 0 {
                                        rx0 = rx0.max(fx - 0.5 + 0.28);
                                    } else if dx > 0 {
                                        rx1 = rx1.min(fx + 0.5 - 0.28);
                                    } else if dy < 0 {
                                        ry0 = ry0.max(fy - 0.5 + 0.28);
                                    } else {
                                        ry1 = ry1.min(fy + 0.5 - 0.28);
                                    }
                                }
                            }
                            push_if(
                                out,
                                Collider {
                                    bounds: Aabb3::new(
                                        Vec3::new(rx0, ry0, z_base - SLAB_THICKNESS),
                                        Vec3::new(rx1, ry1, z_base),
                                    ),
                                    kind: ColliderKind::BuildingSlab {
                                        building_id: 1,
                                        floor: fl,
                                        is_roof: false,
                                    },
                                    material: Material::WoodLight,
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
                    out.push(SurfaceQuad {
                        surface: HitSurface::Terrain { tile: p },
                        x0: tx as f32 - 0.5,
                        y0: ty as f32 - 0.5,
                        x1: tx as f32 + 0.5,
                        y1: ty as f32 + 0.5,
                        corners: [0.0; 4],
                        material: Material::None,
                    });
                    let t0 = Tile3D::from_pos(p, 0);
                    if let Some(&(paired, up_dir)) = self.stairs.get(&t0)
                        && paired.floor == 1
                    {
                        let down_dir = self
                            .stairs
                            .get(&paired)
                            .map_or((-up_dir.0, -up_dir.1), |&(_, d)| d);
                        StairGeometry::new(t0, 0.0, down_dir, up_dir)
                            .push_quads(Material::WoodLight, out);
                    }
                }
            }
        }
    }

    impl CombatWorld for TestWorld {
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

        fn has_floor(&self, _tile: Tile3D) -> bool {
            true
        }

        fn is_walkable(&self, tile: Tile3D, can_vault_window: bool) -> bool {
            match self.obstruction(tile) {
                TileObstruction::Open => true,
                TileObstruction::BrokenWindow => can_vault_window && tile.floor == 0,
                _ => false,
            }
        }

        fn building_id(&self, _tile: Tile3D) -> BuildingId {
            0
        }

        fn stair_link(&self, tile: Tile3D) -> Option<(Tile3D, (i8, i8))> {
            self.stairs.get(&tile).copied()
        }

        fn window_drop_target(&self, _tile: Tile3D) -> Option<Tile3D> {
            None
        }

        fn window_vault_target(&self, _from: Tile3D, _window: Tile3D) -> Option<Tile3D> {
            None
        }

        fn light_level(&self, tile: Tile3D) -> f32 {
            self.lights
                .get(&tile)
                .copied()
                .unwrap_or(self.default_light)
        }

        fn damage_tile(&mut self, tile: Tile3D, _kind: DamageKind) -> Option<TileDamageResult> {
            let (obs, hp) = *self.tiles.get(&tile)?;
            let new_hp = hp.saturating_sub(10);
            let destroyed = new_hp == 0;
            if destroyed {
                let next_obs = match obs {
                    TileObstruction::IntactWindow => TileObstruction::BrokenWindow,
                    _ => TileObstruction::Open,
                };
                self.tiles.insert(tile, (next_obs, 0));
                self.version += 1;
            } else {
                self.tiles.insert(tile, (obs, new_hp));
            }
            Some(TileDamageResult {
                damaged: true,
                destroyed,
                material: Material::Glass,
                old_kind: TileKind::Window,
                new_kind: TileKind::BrokenWindow,
                sound_radius: 5.0,
                light_changed: false,
                collapsed_cells: 0,
            })
        }

        fn toggle_door(&mut self, _tile: Tile3D) -> Option<bool> {
            None
        }

        fn register_dynamic_flash(
            &mut self,
            _pos: Vec3,
            _floor: Floor,
            _radius: f32,
            _intensity: u8,
            _ttl_ticks: u8,
        ) {
        }
    }

    #[test]
    fn quiet_walk_leaves_distant_monster_wandering_while_gunshot_triggers_investigate() {
        let mut world = TestWorld::new(24, 24, 2);
        // Single wall between sound origin (2,5) and monster (10,5): dist 8 + 1.25 = 9.25.
        world.set_tile(
            Tile3D::new(6, 5, 0),
            TileObstruction::Wall(Material::BrickMasonry),
            500,
        );

        let mut monsters = vec![Monster::new(
            MonsterId(1),
            MonsterKind::Shambler,
            0,
            Tile3D::new(10, 5, 0),
        )];
        let mut events = Vec::new();

        // Quiet footstep (radius 2.0) does NOT alert distant monster.
        propagate_noise(
            &world,
            &mut monsters,
            &[NoiseEvent {
                origin: Vec2::new(2.0, 5.0),
                origin_z: 0.0,
                floor: 0,
                radius: 2.0,
                priority: 1,
                kind: NoiseKind::Footstep,
            }],
            &mut events,
        );
        assert!(matches!(monsters[0].state, AwarenessState::Wander { .. }));
        assert!(events.is_empty());

        // Loud pistol shot (radius 12.0 >= 9.25) alerts Wander -> Investigate!
        propagate_noise(
            &world,
            &mut monsters,
            &[NoiseEvent {
                origin: Vec2::new(2.0, 5.0),
                origin_z: 0.0,
                floor: 0,
                radius: 12.0,
                priority: 4,
                kind: NoiseKind::PistolShot,
            }],
            &mut events,
        );
        assert!(matches!(
            monsters[0].state,
            AwarenessState::Investigate {
                target,
                priority: 4,
                ..
            } if target == Tile3D::new(2, 5, 0)
        ));
        assert!(events.iter().any(|e| matches!(
            e,
            ActionEvent::MonsterAlerted {
                id: MonsterId(1),
                by_sight: false,
                ..
            }
        )));
    }

    #[test]
    fn sight_is_blocked_by_closed_door_and_scaled_by_darkness() {
        let mut world = TestWorld::new(24, 24, 2);
        let door = Tile3D::new(7, 5, 0);
        world.set_tile(door, TileObstruction::DoorClosed, 50);

        let mut m = Monster::new(MonsterId(1), MonsterKind::Shambler, 0, Tile3D::new(5, 5, 0));
        m.facing = Vec2::new(1.0, 0.0);

        let player = Pose {
            position: Vec2::new(10.0, 5.0), // 5.0 tiles away (Shambler base sight = 8.5)
            facing: Vec2::new(-1.0, 0.0),
            floor: 0,
        };

        assert!(
            !can_see_player(&world, &m, player, false),
            "Closed door blocks line of sight"
        );

        // Open the door -> now visible in bright light (light = 1.0 -> range 8.5 >= 5.0).
        world.clear_tile(door);
        assert!(
            can_see_player(&world, &m, player, false),
            "Open doorway allows sight in bright light"
        );

        // Pitch darkness (light = 0.0 -> range 8.5 * 0.42 = 3.57 < 5.0) hides player at 5 tiles!
        world.default_light = 0.0;
        assert!(
            !can_see_player(&world, &m, player, false),
            "Darkness scales down sight range"
        );

        // Quiet walk shrinks range even further (0.85x).
        world.default_light = 0.25; // light_scale = 0.565 -> 4.8025 normal, 4.082 quiet
        let mid_player = Pose {
            position: Vec2::new(9.5, 5.0), // 4.5 tiles away
            facing: Vec2::new(-1.0, 0.0),
            floor: 0,
        };
        assert!(can_see_player(&world, &m, mid_player, false));
        assert!(!can_see_player(&world, &m, mid_player, true));
    }

    #[test]
    fn shooter_windup_locks_aim_spawns_bolt_and_can_be_hitstun_interrupted() {
        let mut world = TestWorld::new(24, 24, 2);
        let wall_behind_player = Tile3D::new(14, 5, 0);
        world.set_tile(
            wall_behind_player,
            TileObstruction::Wall(Material::BrickMasonry),
            500,
        );

        let mut ai = AiSystem::new();
        let mut monsters = vec![Monster::new(
            MonsterId(1),
            MonsterKind::Shooter,
            0,
            Tile3D::new(5, 5, 0),
        )];
        monsters[0].facing = Vec2::new(1.0, 0.0);
        monsters[0].state = AwarenessState::Alert {
            last_seen: Tile3D::new(11, 5, 0),
            lose_sight_ticks: 0,
        };
        monsters[0].has_los = true;

        let mut player = Pose {
            position: Vec2::new(11.0, 5.0), // 6 tiles away (in standoff & attack range)
            facing: Vec2::new(-1.0, 0.0),
            floor: 0,
        };
        let mut player_hp = 6u8;
        let mut player_iframes = 0u8;
        let mut player_kb = Vec2::ZERO;
        let mut projectiles = Vec::new();
        let mut next_id = 1u32;
        let mut events = Vec::new();
        let mut noises = Vec::new();

        // Tick 0 starts 33-tick windup.
        ai.step_monsters(
            &mut world,
            &mut monsters,
            player,
            0.23,
            false,
            &mut player_hp,
            &mut player_iframes,
            &mut player_kb,
            &mut projectiles,
            &mut next_id,
            42,
            0,
            &mut events,
            &mut noises,
        );
        assert!(matches!(
            monsters[0].state,
            AwarenessState::Attack {
                phase: AttackPhase::Windup,
                ticks_left: 33,
                ..
            }
        ));

        // Verify wall-clipped telegraph target in MonsterView.
        let view = monsters[0].to_view(&world);
        assert!(view.telegraph_progress.is_some());
        let (clip_pos, clip_floor) = view.telegraph_target.expect("Shooter has telegraph target");
        assert_eq!(clip_floor, 0);
        assert!(
            clip_pos.x <= 14.0 && clip_pos.x >= 13.3,
            "Telegraph ray clips at wall x=14 (got {})",
            clip_pos.x
        );

        // Advance 22 ticks (tracking phase -> ticks_left goes 33 -> 11).
        for tick in 1..=22 {
            ai.step_monsters(
                &mut world,
                &mut monsters,
                player,
                0.23,
                false,
                &mut player_hp,
                &mut player_iframes,
                &mut player_kb,
                &mut projectiles,
                &mut next_id,
                42,
                tick,
                &mut events,
                &mut noises,
            );
        }

        // Now player sidesteps sideways during final 11 aim-locked ticks!
        player.position = Vec2::new(11.0, 8.0);
        for tick in 23..=33 {
            ai.step_monsters(
                &mut world,
                &mut monsters,
                player,
                0.23,
                false,
                &mut player_hp,
                &mut player_iframes,
                &mut player_kb,
                &mut projectiles,
                &mut next_id,
                42,
                tick,
                &mut events,
                &mut noises,
            );
        }

        assert_eq!(projectiles.len(), 1, "Shooter spawned ShooterBolt");
        assert_eq!(projectiles[0].kind, ProjectileKind::ShooterBolt);
        assert!(
            projectiles[0].velocity.y.abs() < 0.5
                && projectiles[0].velocity.x > SHOOTER_BOLT_SPEED * 0.99,
            "Bolt fires along locked aim_dir so player sidestep dodges it (got {:?})",
            projectiles[0].velocity
        );

        // Test hitstun interruption (>= 8 ticks cancels Windup!).
        monsters[0].state = AwarenessState::Attack {
            phase: AttackPhase::Windup,
            aim_dir: Vec2::new(1.0, 0.0),
            aim_dir3: Vec3::X,
            target_floor: 0,
            ticks_left: 20,
            total_ticks: 33,
        };
        let _ = damage_monster(
            &mut monsters[0],
            1,
            Vec2::new(-0.1, 0.0),
            12,
            false,
            &mut events,
        );
        assert!(
            matches!(monsters[0].state, AwarenessState::Alert { .. }),
            "Hitstun >= 8 interrupts Shooter windup"
        );
    }

    #[test]
    fn bloater_slam_swell_burst_and_explosive_instant_chain_burst() {
        let mut world = TestWorld::new(20, 20, 2);
        let win = Tile3D::new(6, 5, 0);
        world.set_tile(win, TileObstruction::IntactWindow, 1);

        let mut ai = AiSystem::new();
        let mut monsters = vec![
            Monster::new(MonsterId(1), MonsterKind::Bloater, 0, Tile3D::new(5, 5, 0)),
            Monster::new(MonsterId(2), MonsterKind::Bloater, 0, Tile3D::new(5, 6, 0)),
        ];
        monsters[1].hp = 3; // Will die explosively from Bloater 1's 3-dmg caustic burst!
        monsters[0].state = AwarenessState::Alert {
            last_seen: Tile3D::new(4, 5, 0),
            lose_sight_ticks: 0,
        };
        monsters[0].has_los = true;

        let player = Pose {
            position: Vec2::new(4.1, 5.0), // 0.9 tiles from Bloater 1 (inside 1.18 slam & 1.6 burst core!)
            facing: Vec2::new(1.0, 0.0),
            floor: 0,
        };
        let mut player_hp = 6u8;
        let mut player_iframes = 0u8;
        let mut player_kb = Vec2::ZERO;
        let mut projectiles = Vec::new();
        let mut next_id = 1u32;
        let mut events = Vec::new();
        let mut noises = Vec::new();

        // Bloater 1 starts 30-tick slam windup and slams player for 2 damage.
        for tick in 0..=30 {
            ai.step_monsters(
                &mut world,
                &mut monsters,
                player,
                0.23,
                false,
                &mut player_hp,
                &mut player_iframes,
                &mut player_kb,
                &mut projectiles,
                &mut next_id,
                1,
                tick,
                &mut events,
                &mut noises,
            );
        }
        assert_eq!(
            player_hp, 4,
            "Bloater slam deals 2 damage within 1.18 tiles"
        );

        // Non-explosive kill puts Bloater 1 into 18-tick DyingSwell (NOT immediate burst).
        player_iframes = 0;
        events.clear();
        let immediate = damage_monster(&mut monsters[0], 8, Vec2::ZERO, 0, false, &mut events);
        assert!(!immediate);
        assert!(monsters[0].is_swelling());
        assert!(monsters[0].is_alive());
        assert!(events.iter().any(|e| matches!(
            e,
            ActionEvent::BloaterSwell {
                id: MonsterId(1),
                fuse_ticks: 18,
                ..
            }
        )));

        // Step 18 ticks -> Bloater 1 bursts, shatters nearby window, hurts player, AND chain-bursts Bloater 2!
        for tick in 40..58 {
            ai.step_monsters(
                &mut world,
                &mut monsters,
                player,
                0.23,
                false,
                &mut player_hp,
                &mut player_iframes,
                &mut player_kb,
                &mut projectiles,
                &mut next_id,
                1,
                tick,
                &mut events,
                &mut noises,
            );
        }
        assert!(!monsters[0].is_alive(), "Bloater 1 finished burst");
        assert!(
            !monsters[1].is_alive(),
            "Bloater 2 chain-burst immediately from Bloater 1 caustic blast"
        );
        assert_eq!(world.obstruction(win), TileObstruction::BrokenWindow);
        assert_eq!(player_hp, 2, "Player in 1.6 core takes 2 burst damage");
        assert!(events.iter().any(|e| matches!(
            e,
            ActionEvent::BloaterBurst {
                id: MonsterId(1),
                ..
            }
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            ActionEvent::BloaterBurst {
                id: MonsterId(2),
                ..
            }
        )));
    }

    #[test]
    fn astar_3d_routes_across_stairs_and_repaths_immediately_on_wall_breach() {
        let mut world = TestWorld::new(20, 12, 2);
        // Link stairs between (6, 5, 0) and (6, 5, 1).
        world.link_stairs(Tile3D::new(6, 5, 0), Tile3D::new(6, 5, 1), (1, 0), (-1, 0));

        // Put a vertical wall at x=5 across y=1..=10 on Floor 0, leaving a detour gap at y=0.
        for y in 1..=10 {
            world.set_tile(
                Tile3D::new(5, y, 0),
                TileObstruction::Wall(Material::BrickMasonry),
                500,
            );
        }

        let mut ai = AiSystem::new();
        let mut monsters = vec![Monster::new(
            MonsterId(1),
            MonsterKind::Brisk,
            0,
            Tile3D::new(3, 5, 0),
        )];
        let player = Pose {
            position: Vec2::new(10.0, 5.0),
            facing: Vec2::new(-1.0, 0.0),
            floor: 1,
        };
        monsters[0].state = AwarenessState::Alert {
            last_seen: Tile3D::new(10, 5, 1),
            lose_sight_ticks: 0,
        };

        let mut player_hp = 6u8;
        let mut player_iframes = 0u8;
        let mut player_kb = Vec2::ZERO;
        let mut projectiles = Vec::new();
        let mut next_id = 1u32;
        let mut events = Vec::new();
        let mut noises = Vec::new();

        // Tick 1: computes 3D detour path around y=0 and up stairs to Floor 1.
        ai.step_monsters(
            &mut world,
            &mut monsters,
            player,
            0.23,
            false,
            &mut player_hp,
            &mut player_iframes,
            &mut player_kb,
            &mut projectiles,
            &mut next_id,
            42,
            1,
            &mut events,
            &mut noises,
        );
        let detour_len = monsters[0].path.len();
        assert!(
            detour_len > 10,
            "Initial path detours around wall and climbs stairs (len={detour_len})"
        );
        assert!(
            monsters[0].path.iter().any(|t| t.floor == 1),
            "3D A* path reaches Floor 1 via stairs"
        );

        // Breach the wall directly between monster and stairs at (5, 5, 0) -> terrain_version increments!
        world.clear_tile(Tile3D::new(5, 5, 0));

        // On the VERY NEXT tick, AI detects terrain_version mismatch and immediately repaths shorter!
        ai.step_monsters(
            &mut world,
            &mut monsters,
            player,
            0.23,
            false,
            &mut player_hp,
            &mut player_iframes,
            &mut player_kb,
            &mut projectiles,
            &mut next_id,
            42,
            2,
            &mut events,
            &mut noises,
        );
        assert!(
            monsters[0].path.len() < detour_len,
            "Monster immediately repathed through breached wall ({} < {})",
            monsters[0].path.len(),
            detour_len
        );

        // Simulate ticks until Brisk walks through breach, climbs stairs to Floor 1, without oscillating!
        let mut reached_f1 = false;
        for tick in 3..260 {
            ai.step_monsters(
                &mut world,
                &mut monsters,
                player,
                0.23,
                false,
                &mut player_hp,
                &mut player_iframes,
                &mut player_kb,
                &mut projectiles,
                &mut next_id,
                42,
                tick,
                &mut events,
                &mut noises,
            );
            if monsters[0].floor == 1 {
                reached_f1 = true;
            }
            if reached_f1 {
                assert_eq!(
                    monsters[0].floor, 1,
                    "Monster stays on Floor 1 after climbing stairs (no oscillation)"
                );
            }
        }
        assert!(
            reached_f1,
            "Monster successfully navigated 3D stairs to Floor 1"
        );
    }

    #[test]
    fn bloater_cannot_vault_broken_window_while_shambler_can_and_alert_refreshes_on_sound() {
        let mut world = TestWorld::new(16, 12, 2);
        // Full wall at x=6 with a single F0 BrokenWindow at (6, 5, 0).
        for y in 0..12 {
            let obs = if y == 5 {
                TileObstruction::BrokenWindow
            } else {
                TileObstruction::Wall(Material::BrickMasonry)
            };
            world.set_tile(Tile3D::new(6, y, 0), obs, 500);
        }

        let mut pf = Pathfinder::new();
        let mut shambler_path = Vec::new();
        let mut bloater_path = Vec::new();
        assert!(
            pf.find_path(
                &world,
                Tile3D::new(3, 5, 0),
                Tile3D::new(9, 5, 0),
                monster_spec(MonsterKind::Shambler).can_vault_f0,
                &mut shambler_path,
            ),
            "Shambler routes through F0 BrokenWindow"
        );
        assert!(shambler_path.contains(&Tile3D::new(6, 5, 0)));

        assert!(
            !pf.find_path(
                &world,
                Tile3D::new(3, 5, 0),
                Tile3D::new(9, 5, 0),
                monster_spec(MonsterKind::Bloater).can_vault_f0,
                &mut bloater_path,
            ) || !bloater_path.contains(&Tile3D::new(6, 5, 0)),
            "Bloater cannot route through F0 BrokenWindow"
        );

        // Alert monster that lost LOS refreshes last_seen when hearing a noise.
        let mut monsters = vec![Monster::new(
            MonsterId(7),
            MonsterKind::Shambler,
            0,
            Tile3D::new(3, 5, 0),
        )];
        monsters[0].state = AwarenessState::Alert {
            last_seen: Tile3D::new(4, 5, 0),
            lose_sight_ticks: 40,
        };
        let mut events = Vec::new();
        propagate_noise(
            &world,
            &mut monsters,
            &[NoiseEvent {
                origin: Vec2::new(8.0, 5.0),
                origin_z: 0.0,
                floor: 0,
                radius: 12.0,
                priority: 4,
                kind: NoiseKind::PistolShot,
            }],
            &mut events,
        );
        assert!(matches!(
            monsters[0].state,
            AwarenessState::Alert {
                last_seen,
                lose_sight_ticks: 0,
            } if last_seen == Tile3D::new(8, 5, 0)
        ));
    }

    #[test]
    fn closed_door_routing_leads_monster_to_bash_and_repath_when_opened() {
        let mut world = TestWorld::new(16, 12, 2);
        for y in 0..12 {
            let obs = if y == 5 {
                TileObstruction::DoorClosed
            } else {
                TileObstruction::Wall(Material::BrickMasonry)
            };
            world.set_tile(Tile3D::new(6, y, 0), obs, 20);
        }

        let mut ai = AiSystem::new();
        let mut monsters = vec![Monster::new(
            MonsterId(2),
            MonsterKind::Shambler,
            0,
            Tile3D::new(5, 5, 0),
        )];
        monsters[0].state = AwarenessState::Alert {
            last_seen: Tile3D::new(9, 5, 0),
            lose_sight_ticks: 0,
        };

        let player = Pose {
            position: Vec2::new(9.0, 5.0),
            facing: Vec2::new(-1.0, 0.0),
            floor: 0,
        };
        let mut hp = 6u8;
        let mut iframes = 0u8;
        let mut kb = Vec2::ZERO;
        let mut projectiles = Vec::new();
        let mut next_id = 1u32;
        let mut events = Vec::new();
        let mut noises = Vec::new();

        // Step until monster bashes the closed door down (2 bashes of 10 dmg each).
        for tick in 0..60 {
            ai.step_monsters(
                &mut world,
                &mut monsters,
                player,
                0.23,
                false,
                &mut hp,
                &mut iframes,
                &mut kb,
                &mut projectiles,
                &mut next_id,
                42,
                tick,
                &mut events,
                &mut noises,
            );
        }

        assert_eq!(
            world.obstruction(Tile3D::new(6, 5, 0)),
            TileObstruction::Open,
            "Monster bashed closed door open"
        );
        assert!(events.iter().any(|e| matches!(
            e,
            ActionEvent::MaterialHit {
                destroyed: true,
                ..
            }
        )));
    }
}
