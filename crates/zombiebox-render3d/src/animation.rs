//! Procedural rigid-part actor poses. Only currently visible monsters are emitted.
use crate::mesh::motorcycle::{MotorcycleLocator, motorcycle_locator};
use crate::mesh::vehicles::MotorcycleTransforms;
use crate::mesh::{
    GeometryTag, MaterialFlags, VertexMeta, actors::ActorPart, survivor::SurvivorNode,
};
use crate::player_rig::{RiderContacts, draw_survivor};
use crate::scene::DynamicSceneState;
use glam::{Mat4, Quat, Vec3};
use std::collections::{BTreeMap, VecDeque};
use zombiebox_core::{
    ActionEvent, MonsterId, MonsterKind, Vec2 as SimVec2, Vec3 as SimVec3, VehicleView, Weapon,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorInstance {
    pub model: [[f32; 4]; 4],
    pub color_tint: [f32; 4],
    pub meta_override: [u32; 2],
    pub emissive_flash: [f32; 4],
}
pub type ActorBatches = BTreeMap<ActorPart, Vec<ActorInstance>>;

const CORPSE_LIFETIME: f32 = 6.0;
const MAX_CORPSES: usize = 128;
#[derive(Clone, Copy)]
struct Corpse {
    id: MonsterId,
    kind: MonsterKind,
    at: SimVec3,
    floor: u8,
    facing: SimVec2,
    remaining: f32,
}
#[derive(Clone, Copy)]
enum PlayerAction {
    Swing,
    Shove,
    Recoil,
    DoorReach,
}

pub struct ActorAnimator {
    phase: f32,
    player_phase: f32,
    draw_player: bool,
    corpses: VecDeque<Corpse>,
    action: Option<(PlayerAction, f32)>,
    vault: Option<(SimVec3, SimVec3, f32)>,
    was_mounted: bool,
}
impl Default for ActorAnimator {
    fn default() -> Self {
        Self {
            phase: 0.0,
            player_phase: 0.0,
            draw_player: true,
            corpses: VecDeque::new(),
            action: None,
            vault: None,
            was_mounted: false,
        }
    }
}
impl ActorAnimator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_draw_player(&mut self, draw: bool) {
        self.draw_player = draw;
    }
    pub fn notify_teleport(&mut self) {
        self.phase = 0.0;
        self.player_phase = 0.0;
        self.action = None;
        self.vault = None;
    }
    /// Consume each simulation event once, including events accumulated across catch-up ticks.
    pub fn observe_events(&mut self, events: &[ActionEvent]) {
        for event in events {
            match *event {
                ActionEvent::AxeSwing { .. } => self.action = Some((PlayerAction::Swing, 0.30)),
                ActionEvent::UnarmedShove { .. } => self.action = Some((PlayerAction::Shove, 0.25)),
                ActionEvent::ShotFired { .. } => self.action = Some((PlayerAction::Recoil, 0.18)),
                ActionEvent::DoorToggled { .. } => {
                    self.action = Some((PlayerAction::DoorReach, 0.34))
                }
                ActionEvent::WindowClimbed { from, to, .. } => self.vault = Some((from, to, 0.56)),
                ActionEvent::FloorChanged { .. } => self.vault = None,
                ActionEvent::MonsterDown {
                    id,
                    kind,
                    at,
                    floor,
                    facing,
                    ..
                } => {
                    self.corpses.retain(|c| c.id != id);
                    if self.corpses.len() == MAX_CORPSES {
                        self.corpses.pop_front();
                    }
                    self.corpses.push_back(Corpse {
                        id,
                        kind,
                        at,
                        floor,
                        facing,
                        remaining: CORPSE_LIFETIME,
                    });
                }
                _ => {}
            }
        }
    }
    #[must_use]
    pub fn prepare(&mut self, scene: &DynamicSceneState, dt: f32) -> ActorBatches {
        if dt.is_finite() {
            self.phase = (self.phase + dt.clamp(0.0, 0.1) * 8.0) % std::f32::consts::TAU;
        }
        let mut out = ActorBatches::new();
        let motion = self.action.map_or(0.0, |(action, remaining)| match action {
            PlayerAction::Swing => -1.4 * (remaining / 0.30 * std::f32::consts::PI).sin(),
            PlayerAction::Shove => -1.1 * (remaining / 0.25 * std::f32::consts::PI).sin(),
            PlayerAction::Recoil => 0.5 * (remaining / 0.18).clamp(0.0, 1.0),
            PlayerAction::DoorReach => 0.0,
        });
        let reach = self.action.map_or(0.0, |(kind, remaining)| {
            if matches!(kind, PlayerAction::DoorReach) {
                (remaining / 0.34 * std::f32::consts::PI).sin().max(0.0)
            } else {
                0.0
            }
        });
        let vault_pose = self.vault.map_or(0.0, |(_, _, remaining)| {
            (std::f32::consts::PI * (1.0 - remaining / 0.56).clamp(0.0, 1.0))
                .sin()
                .max(0.0)
        });
        let p = &scene.player.render;
        let speed = (p.body.velocity.x.powi(2) + p.body.velocity.y.powi(2)).sqrt();
        if dt.is_finite() {
            self.player_phase =
                (self.player_phase + dt.clamp(0.0, 0.1) * speed * 3.5) % std::f32::consts::TAU;
        }
        let angle = p.pose.facing.x.atan2(p.pose.facing.y);
        let move_angle = if speed > 0.05 {
            p.body.velocity.x.atan2(p.body.velocity.y) - angle
        } else {
            0.0
        };
        let aim = p.aim_dir.x.atan2(p.aim_dir.y) - angle;
        let pitch = p
            .aim_dir
            .z
            .atan2((p.aim_dir.x * p.aim_dir.x + p.aim_dir.y * p.aim_dir.y).sqrt());
        let mut render_position =
            Vec3::new(p.body.position.x, p.body.position.z, p.body.position.y);
        let mut render_angle = angle;
        if let Some((from, to, remaining)) = self.vault {
            let t = (1.0 - remaining / 0.56).clamp(0.0, 1.0);
            let horizontal = Vec3::new(to.x - from.x, 0.0, to.y - from.y);
            if horizontal.length_squared() > 0.49 {
                render_position += Vec3::new(from.x - to.x, 0.0, from.y - to.y) * (1.0 - t);
                render_angle = (to.x - from.x).atan2(to.y - from.y);
            }
            render_position += Vec3::Y * (0.20 * (std::f32::consts::PI * t).sin());
        }
        let player_root =
            Mat4::from_translation(render_position) * Mat4::from_rotation_y(render_angle);
        let weapon = p.equipped.map(|w| match w {
            Weapon::Axe => ActorPart::Axe,
            Weapon::Pistol => ActorPart::Pistol,
            Weapon::Shotgun => ActorPart::Shotgun,
            Weapon::RocketLauncher => ActorPart::RocketLauncher,
        });
        let meta = actor_meta(p.pose.floor);
        let mounted = scene.mounted_vehicle.and_then(|id| {
            scene
                .vehicles
                .iter()
                .find_map(|(vehicle_id, v)| (*vehicle_id == id).then_some(*v))
        });
        if self.was_mounted != mounted.is_some() {
            self.action = None;
            self.vault = None;
            self.was_mounted = mounted.is_some();
        }
        if self.draw_player && mounted.is_none() {
            draw_survivor(
                &mut out,
                player_root,
                self.player_phase,
                speed,
                p.quiet_walking,
                aim,
                pitch,
                weapon,
                meta,
                motion,
                false,
                reach,
                vault_pose,
                move_angle,
                None,
            );
        }
        if self.draw_player
            && let Some(VehicleView::Motorcycle(moto)) = mounted
        {
            let transforms = MotorcycleTransforms::from_view(moto, 0.0);
            let (parent, local) = motorcycle_locator(MotorcycleLocator::Seat);
            let seat = transforms.part_transform(parent).transform_point3(local);
            let (_, bike_orientation, _) = transforms.body.to_scale_rotation_translation();
            let seat_local = Vec3::from_array(SurvivorNode::Pelvis.bind_translation())
                + Quat::from_rotation_x(-0.25)
                    * Vec3::from_array(SurvivorNode::SocketSeat.bind_translation());
            let rider_root = Mat4::from_translation(seat - bike_orientation * seat_local)
                * Mat4::from_quat(bike_orientation);
            let locator = |point| {
                let (parent, local) = motorcycle_locator(point);
                transforms.part_transform(parent).transform_point3(local)
            };
            let contacts = RiderContacts {
                hands: [
                    locator(MotorcycleLocator::GripLeft),
                    locator(MotorcycleLocator::GripRight),
                ],
                feet: [
                    locator(MotorcycleLocator::FootboardLeft),
                    locator(MotorcycleLocator::FootboardRight),
                ],
            };
            draw_survivor(
                &mut out,
                rider_root,
                self.phase,
                0.0,
                false,
                0.0,
                0.0,
                None,
                actor_meta(p.pose.floor),
                0.0,
                true,
                0.0,
                0.0,
                0.0,
                Some(contacts),
            );
        }
        for m in &scene.monsters {
            if !scene.local_visibility.is_actor_visible(m.pos, m.floor) {
                continue;
            }
            let (height, radius, color) = monster_shape(m.kind);
            let speed = (m.body.velocity.x.powi(2) + m.body.velocity.y.powi(2)).sqrt();
            let root = Mat4::from_translation(Vec3::new(
                m.body.position.x,
                m.body.position.z,
                m.body.position.y,
            )) * Mat4::from_rotation_y(m.facing.x.atan2(m.facing.y));
            let telegraph = m.telegraph_progress.unwrap_or(0.0).clamp(0.0, 1.0);
            let weapon = (m.kind == MonsterKind::Shooter).then_some(ActorPart::BoltLauncher);
            draw_actor(
                &mut out,
                root,
                height,
                radius,
                color,
                self.phase + (m.id.0 as f32 * 0.37),
                speed,
                false,
                m.kind == MonsterKind::Bloater,
                m.kind == MonsterKind::Shambler,
                m.hitstun,
                telegraph,
                0.0,
                0.0,
                weapon,
                actor_meta(m.floor),
                0.0,
                false,
            );
        }
        for corpse in &self.corpses {
            if scene.monsters.iter().any(|m| m.id == corpse.id)
                || !scene
                    .local_visibility
                    .is_actor_visible(corpse.at.xy(), corpse.floor)
            {
                continue;
            }
            let (height, radius, color) = monster_shape(corpse.kind);
            let root = Mat4::from_translation(Vec3::new(
                corpse.at.x,
                corpse.at.z + radius * 0.5,
                corpse.at.y,
            )) * Mat4::from_rotation_y(corpse.facing.x.atan2(corpse.facing.y))
                * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);
            draw_actor(
                &mut out,
                root,
                height,
                radius,
                color,
                0.0,
                0.0,
                false,
                corpse.kind == MonsterKind::Bloater,
                corpse.kind == MonsterKind::Shambler,
                false,
                0.0,
                0.0,
                0.0,
                None,
                actor_meta(corpse.floor),
                0.0,
                false,
            );
        }
        let elapsed = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        if let Some((_, remaining)) = &mut self.action {
            *remaining -= elapsed;
            if *remaining <= 0.0 {
                self.action = None;
            }
        }
        if let Some((_, _, remaining)) = &mut self.vault {
            *remaining -= elapsed;
            if *remaining <= 0.0 {
                self.vault = None;
            }
        }
        for corpse in &mut self.corpses {
            corpse.remaining -= elapsed;
        }
        self.corpses.retain(|c| c.remaining > 0.0);
        out
    }
}

fn monster_shape(kind: MonsterKind) -> (f32, f32, [f32; 4]) {
    match kind {
        MonsterKind::Shambler => (0.98, 0.24, [0.43, 0.52, 0.39, 1.0]),
        MonsterKind::Brisk => (1.02, 0.22, [0.55, 0.43, 0.35, 1.0]),
        MonsterKind::Bloater => (1.15, 0.35, [0.55, 0.59, 0.32, 1.0]),
        MonsterKind::Shooter => (1.0, 0.23, [0.35, 0.41, 0.34, 1.0]),
    }
}
fn actor_meta(floor: u8) -> [u32; 2] {
    VertexMeta::DEFAULT
        .with_tag(GeometryTag::Actor)
        .with_floor(floor)
        .with_flags(MaterialFlags::ACTOR_MIN_LIGHT | MaterialFlags::CUTAWAY_EXEMPT)
        .pack()
}
#[allow(clippy::too_many_arguments)]
fn draw_actor(
    out: &mut ActorBatches,
    root: Mat4,
    h: f32,
    r: f32,
    color: [f32; 4],
    phase: f32,
    speed: f32,
    crouch: bool,
    bloater: bool,
    zombie: bool,
    hit: bool,
    telegraph: f32,
    aim: f32,
    pitch: f32,
    weapon: Option<ActorPart>,
    meta: [u32; 2],
    action_motion: f32,
    seated: bool,
) {
    let gait = phase.sin() * (speed * 0.3).clamp(0.0, 0.55);
    let crouch_y = if crouch { -0.10 * h } else { 0.0 };
    let root = root
        * Mat4::from_translation(Vec3::new(0.0, crouch_y, 0.0))
        * Mat4::from_rotation_x(if hit { -0.18 } else { 0.0 });
    let flash = if hit { [0.5, 0.1, 0.1, 0.5] } else { [0.0; 4] };
    // The shambler mesh carries its own palette; keep the instance tint neutral so
    // the authored vertex colours read. Other actors still use the archetype tint.
    let body_color = if zombie { [1.0; 4] } else { color };
    let head_color = if zombie {
        [1.0; 4]
    } else {
        [color[0] * 1.1, color[1] * 1.1, color[2] * 1.1, 1.0]
    };
    let body_part = if zombie {
        ActorPart::ZombieBody
    } else {
        ActorPart::Body
    };
    let head_part = if zombie {
        ActorPart::ZombieHead
    } else {
        ActorPart::Head
    };
    let mut add = |part, parent: Mat4, pos: Vec3, size: Vec3, tint, glow| {
        let model = parent * Mat4::from_translation(pos) * Mat4::from_scale(size);
        out.entry(part).or_default().push(ActorInstance {
            model: model.to_cols_array_2d(),
            color_tint: tint,
            meta_override: meta,
            emissive_flash: glow,
        });
    };
    add(
        body_part,
        root,
        Vec3::new(0.0, h * 0.60, 0.0),
        Vec3::new(r * 1.45, h * 0.38, r * 0.95),
        body_color,
        flash,
    );
    add(
        head_part,
        root,
        Vec3::new(0.0, h * 0.90, 0.0),
        Vec3::new(r * 0.85, h * 0.20, r * 0.8),
        head_color,
        flash,
    );
    for side in [-1.0_f32, 1.0] {
        let leg_part = if zombie {
            if side < 0.0 {
                ActorPart::ZombieLegL
            } else {
                ActorPart::ZombieLegR
            }
        } else {
            ActorPart::Limb
        };
        let leg = root
            * Mat4::from_translation(Vec3::new(side * r * 0.38, h * 0.43, 0.0))
            * Mat4::from_rotation_x(if seated { -0.85 * side } else { gait * side });
        add(
            leg_part,
            leg,
            Vec3::new(0.0, -h * 0.22, 0.0),
            Vec3::new(r * 0.55, h * 0.44, r * 0.65),
            body_color,
            flash,
        );
        let arm_part = if zombie {
            if side < 0.0 {
                ActorPart::ZombieArmL
            } else {
                ActorPart::ZombieArmR
            }
        } else {
            ActorPart::Limb
        };
        let arm = root
            * Mat4::from_translation(Vec3::new(side * r * 0.95, h * 0.75, 0.0))
            * Mat4::from_rotation_y(if side > 0.0 { aim } else { 0.0 })
            * Mat4::from_rotation_x(
                (if seated {
                    -1.05
                } else if side > 0.0 && weapon.is_some() {
                    -1.2 - pitch
                } else {
                    -gait * side
                }) + if side > 0.0 { action_motion } else { 0.0 },
            );
        add(
            arm_part,
            arm,
            Vec3::new(0.0, -h * 0.15, 0.0),
            Vec3::new(r * 0.40, h * 0.30, r * 0.45),
            body_color,
            flash,
        );
        if side > 0.0
            && let Some(w) = weapon
        {
            let glow = if w == ActorPart::BoltLauncher {
                [
                    0.3 * telegraph,
                    0.7 * telegraph,
                    0.15 * telegraph,
                    telegraph,
                ]
            } else {
                flash
            };
            let size = match w {
                ActorPart::Axe => Vec3::new(0.06, 0.32, 0.08),
                ActorPart::Pistol => Vec3::new(0.07, 0.14, 0.08),
                ActorPart::Shotgun => Vec3::new(0.08, 0.34, 0.08),
                _ => Vec3::new(0.13, 0.36, 0.13),
            };
            add(
                w,
                arm,
                Vec3::new(0.0, -h * 0.32, 0.0),
                size,
                [0.25, 0.23, 0.20, 1.0],
                glow,
            );
        }
        if bloater {
            let swell = 1.0 + telegraph * 0.3;
            add(
                ActorPart::Sac,
                root,
                Vec3::new(side * r * 0.58, h * 0.59, r * 0.35),
                Vec3::splat(r * 0.65 * swell),
                [0.65, 0.68, 0.28, 1.0],
                flash,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::SceneManager;
    use zombiebox_core::{ActionGame, TileVisibility};

    fn body_count(batches: &ActorBatches) -> usize {
        [
            ActorPart::Body,
            ActorPart::ZombieBody,
            ActorPart::Survivor(SurvivorNode::Pelvis),
        ]
        .into_iter()
        .filter_map(|part| batches.get(&part))
        .map(Vec::len)
        .sum()
    }

    #[test]
    fn shamblers_use_zombie_mesh_parts() {
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new()
            .prepare_at_focus(&game, game.player_tile().pos(), 0.0)
            .dynamic;
        for cell in &mut scene.local_visibility.cells {
            cell.visibility = TileVisibility::Visible;
        }
        let batches = ActorAnimator::new().prepare(&scene, 0.0);
        assert!(!batches[&ActorPart::ZombieBody].is_empty());
        assert!(!batches[&ActorPart::ZombieHead].is_empty());
        assert!(!batches[&ActorPart::ZombieLegL].is_empty());
        assert!(!batches[&ActorPart::ZombieArmR].is_empty());
        // The survivor and non-shambler archetypes keep the neutral cuboid parts.
        let non_shamblers = scene
            .monsters
            .iter()
            .filter(|monster| monster.kind != MonsterKind::Shambler)
            .count();
        assert_eq!(batches[&ActorPart::Survivor(SurvivorNode::Pelvis)].len(), 1);
        assert_eq!(batches[&ActorPart::Body].len(), non_shamblers);
    }

    #[test]
    fn hidden_monsters_never_emit_instances() {
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new()
            .prepare_at_focus(&game, game.player_tile().pos(), 0.0)
            .dynamic;
        assert!(!scene.monsters.is_empty());
        for cell in &mut scene.local_visibility.cells {
            cell.visibility = TileVisibility::Hidden;
        }
        let mut animator = ActorAnimator::new();
        let hidden = animator.prepare(&scene, 0.0);
        assert_eq!(body_count(&hidden), 1);
        for cell in &mut scene.local_visibility.cells {
            cell.visibility = TileVisibility::Visible;
        }
        let visible = animator.prepare(&scene, 0.0);
        assert!(body_count(&visible) > 1);
        animator.set_draw_player(false);
        let first_person = animator.prepare(&scene, 0.0);
        assert_eq!(body_count(&first_person) + 1, body_count(&visible));
        animator.set_draw_player(true);
        scene.local_visibility.cells.clear();
        assert_eq!(body_count(&animator.prepare(&scene, 0.0)), 1);
    }

    #[test]
    fn movement_animates_limbs_without_moving_root() {
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new()
            .prepare_at_focus(&game, game.player_tile().pos(), 0.0)
            .dynamic;
        scene.monsters.clear();
        scene.player.render.body.velocity.x = 1.0;
        let mut animator = ActorAnimator::new();
        let before = animator.prepare(&scene, 0.0);
        let after = animator.prepare(&scene, 0.1);
        assert_eq!(
            before[&ActorPart::Survivor(SurvivorNode::Pelvis)][0].model[3],
            after[&ActorPart::Survivor(SurvivorNode::Pelvis)][0].model[3]
        );
        assert_ne!(
            before[&ActorPart::Survivor(SurvivorNode::ThighL)],
            after[&ActorPart::Survivor(SurvivorNode::ThighL)]
        );
    }

    #[test]
    fn corpse_visibility_expiry_and_capacity() {
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new()
            .prepare_at_focus(&game, game.player_tile().pos(), 0.0)
            .dynamic;
        let monster = scene.monsters[0];
        scene.monsters.clear();
        let event = ActionEvent::MonsterDown {
            id: monster.id,
            kind: monster.kind,
            at: monster.body.position,
            floor: monster.floor,
            facing: monster.facing,
            overkill: false,
        };
        let mut animator = ActorAnimator::new();
        animator.observe_events(&[event]);
        for cell in &mut scene.local_visibility.cells {
            cell.visibility = TileVisibility::Visible;
        }
        assert_eq!(body_count(&animator.prepare(&scene, 0.0)), 2);
        for cell in &mut scene.local_visibility.cells {
            cell.visibility = TileVisibility::Hidden;
        }
        assert_eq!(body_count(&animator.prepare(&scene, 0.0)), 1);
        for _ in 0..61 {
            let _ = animator.prepare(&scene, 0.1);
        }
        assert!(animator.corpses.is_empty());
        for id in 0..150 {
            animator.observe_events(&[ActionEvent::MonsterDown {
                id: MonsterId(id),
                kind: monster.kind,
                at: monster.body.position,
                floor: monster.floor,
                facing: monster.facing,
                overkill: false,
            }]);
        }
        assert_eq!(animator.corpses.len(), MAX_CORPSES);
    }

    #[test]
    fn action_events_move_arm_then_expire() {
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new()
            .prepare_at_focus(&game, game.player_tile().pos(), 0.0)
            .dynamic;
        scene.monsters.clear();
        let mut animator = ActorAnimator::new();
        let idle = animator.prepare(&scene, 0.0);
        let p = scene.player.render;
        animator.observe_events(&[ActionEvent::UnarmedShove {
            from: p.body.position,
            floor: p.pose.floor,
            facing: p.pose.facing,
        }]);
        let _ = animator.prepare(&scene, 0.1);
        let active = animator.prepare(&scene, 0.0);
        assert_ne!(
            idle[&ActorPart::Survivor(SurvivorNode::UpperArmR)],
            active[&ActorPart::Survivor(SurvivorNode::UpperArmR)]
        );
        for _ in 0..4 {
            let _ = animator.prepare(&scene, 0.1);
        }
        assert_eq!(
            idle[&ActorPart::Survivor(SurvivorNode::UpperArmR)],
            animator.prepare(&scene, 0.0)[&ActorPart::Survivor(SurvivorNode::UpperArmR)]
        );
    }

    #[test]
    fn window_climb_blends_between_explicit_endpoints_and_expires() {
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new()
            .prepare_at_focus(&game, game.player_tile().pos(), 0.0)
            .dynamic;
        scene.monsters.clear();
        let from = scene.player.render.body.position;
        let to = SimVec3::new(from.x + 2.0, from.y + 1.0, from.z);
        scene.player.render.body.position = to;
        let mut animator = ActorAnimator::new();
        animator.observe_events(&[ActionEvent::WindowClimbed {
            at: game.player_tile(),
            from,
            to,
        }]);
        let start =
            animator.prepare(&scene, 0.0)[&ActorPart::Survivor(SurvivorNode::Pelvis)][0].model[3];
        assert!((start[0] - from.x).abs() < 1e-4);
        assert!((start[2] - from.y).abs() < 1e-4);
        for _ in 0..3 {
            let _ = animator.prepare(&scene, 0.1);
        }
        let mid =
            animator.prepare(&scene, 0.0)[&ActorPart::Survivor(SurvivorNode::Pelvis)][0].model[3];
        assert!(
            (from.x + 0.8..=from.x + 1.2).contains(&mid[0]),
            "mid x {} from {}",
            mid[0],
            from.x
        );
        assert!(mid[1] > from.z);
        for _ in 0..3 {
            let _ = animator.prepare(&scene, 0.1);
        }
        assert!(animator.vault.is_none());
    }
}
