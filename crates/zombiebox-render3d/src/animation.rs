//! Procedural actor poses. Only currently visible monsters are emitted.
pub mod zombie;
use crate::camera::{CameraFrame, CameraMode};
use crate::coords::{ground_align_quat, sim_to_render_point};
use crate::interaction::{DoorReach, WindowVault};
use crate::mesh::doors::DOOR_REACH_SECONDS;
use crate::mesh::motorcycle::{MotorcycleLocator, motorcycle_locator};
use crate::mesh::vehicles::MotorcycleTransforms;
use crate::mesh::{
    GeometryTag, MaterialFlags, VertexMeta, actors::ActorPart, survivor::SurvivorNode,
};
use crate::player_rig::{
    CombatPose, InteractionPose, RiderContacts, draw_first_person, draw_survivor, stride_distance,
};
use crate::scene::DynamicSceneState;
use glam::{Mat4, Quat, Vec3};
use std::collections::{BTreeMap, VecDeque};
use zombiebox_core::{
    ActionEvent, MonsterId, MonsterKind, Tile3D, Vec2 as SimVec2, Vec3 as SimVec3, VehicleView,
    WallOrientation, Weapon,
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
    ground_normal: SimVec3,
    remaining: f32,
    elapsed: f32,
    pose_phase: Option<f32>,
}
#[derive(Clone, Copy)]
enum PlayerAction {
    Swing,
    Shove,
    Recoil(Weapon),
}

struct PlayerLocomotion {
    sample: Option<(u64, SimVec2)>,
    phase: f32,
    speed: f32,
    target_speed: f32,
    heading: f32,
}

impl Default for PlayerLocomotion {
    fn default() -> Self {
        Self {
            sample: None,
            phase: 0.25,
            speed: 0.0,
            target_speed: 0.0,
            heading: 0.0,
        }
    }
}

impl PlayerLocomotion {
    fn update(&mut self, tick: u64, position: SimVec2, grounded: bool, dt: f32) {
        let previous = self.sample.replace((tick, position));
        if !grounded {
            self.speed = 0.0;
            self.target_speed = 0.0;
            return;
        }

        // Render frames can share a simulation tick. Advance the stride only on new travel.
        if let Some((previous_tick, previous_position)) = previous
            && tick > previous_tick
        {
            let delta = position - previous_position;
            let distance = delta.length();
            let seconds = (tick - previous_tick) as f32 * zombiebox_core::DT;
            let speed = distance / seconds;
            if (0.035..=8.0).contains(&speed) {
                self.target_speed = speed;
                self.heading = delta.x.atan2(delta.y);
                self.phase = (self.phase + distance / stride_distance(speed)).fract();
            } else {
                self.target_speed = 0.0;
            }
        }

        let speed_step = 46.0 * dt;
        self.speed += (self.target_speed - self.speed).clamp(-speed_step, speed_step);
    }
}

pub struct ActorAnimator {
    phase: f32,
    player_locomotion: PlayerLocomotion,
    draw_player: bool,
    first_person: Option<CameraFrame>,
    corpses: VecDeque<Corpse>,
    action: Option<(PlayerAction, f32)>,
    door: Option<DoorReach>,
    vault: Option<WindowVault>,
    was_mounted: bool,
}
impl Default for ActorAnimator {
    fn default() -> Self {
        Self {
            phase: 0.0,
            player_locomotion: PlayerLocomotion::default(),
            draw_player: true,
            first_person: None,
            corpses: VecDeque::new(),
            action: None,
            door: None,
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
        self.first_person = None;
    }
    pub fn set_player_camera(&mut self, camera: CameraFrame) {
        self.first_person = (camera.mode == CameraMode::FirstPerson).then_some(camera);
        self.draw_player = self.first_person.is_none();
    }
    pub fn notify_teleport(&mut self) {
        self.phase = 0.0;
        self.player_locomotion = PlayerLocomotion::default();
        self.action = None;
        self.door = None;
        self.vault = None;
    }
    /// Consume each simulation event once, including events accumulated across catch-up ticks.
    pub fn observe_events(&mut self, events: &[ActionEvent]) {
        self.observe_events_with_ground(events, |_, _| SimVec3::Z);
    }

    /// Consume events while sampling the ground normal for corpse placement.
    pub fn observe_events_with_ground(
        &mut self,
        events: &[ActionEvent],
        ground_normal: impl Fn(SimVec2, u8) -> SimVec3,
    ) {
        for event in events {
            match *event {
                ActionEvent::AxeSwing { .. } => self.action = Some((PlayerAction::Swing, 0.30)),
                ActionEvent::UnarmedShove { .. } => self.action = Some((PlayerAction::Shove, 0.25)),
                ActionEvent::ShotFired { weapon, .. } => {
                    self.action = Some((PlayerAction::Recoil(weapon), 0.18))
                }
                ActionEvent::DoorToggled { at, pos, .. } => {
                    self.door = Some(DoorReach {
                        tile: at,
                        center: sim_to_render_point(pos),
                        elapsed: 0.0,
                    });
                }
                ActionEvent::WindowClimbed { at, from, to } => {
                    self.door = None;
                    self.vault = Some(WindowVault::new(at, from, to));
                }
                ActionEvent::FloorChanged { .. } => {
                    self.door = None;
                    self.vault = None;
                }
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
                        ground_normal: ground_normal(at.xy(), floor),
                        remaining: CORPSE_LIFETIME,
                        elapsed: 0.0,
                        pose_phase: None,
                    });
                }
                _ => {}
            }
        }
    }

    /// Build visible skinned records for living zombies and their brief collapse.
    /// This presentation path does not modify gameplay state or collision bodies.
    pub fn prepare_zombie_records(
        &mut self,
        scene: &DynamicSceneState,
        skeletons: &[(zombie::ZombieArchetype, zombie::ZombieSkeleton)],
    ) -> Vec<zombie::ZombieDrawRecord> {
        let mut records = zombie::visible_zombie_records(scene, skeletons);
        for corpse in &mut self.corpses {
            if scene.monsters.iter().any(|m| m.id == corpse.id)
                || !scene
                    .local_visibility
                    .is_actor_visible(corpse.at.xy(), corpse.floor)
            {
                continue;
            }
            let archetype = zombie::zombie_archetype(corpse.kind, corpse.id);
            let Some((_, rig)) = skeletons.iter().find(|(kind, _)| *kind == archetype) else {
                continue;
            };
            let phase = *corpse.pose_phase.get_or_insert_with(|| {
                scene.sim_tick as f32 * zombiebox_core::DT * 7.0
                    + (corpse.id.0 % 997) as f32 * 0.021
            });
            let death = (corpse.elapsed / 0.32).clamp(0.0, 1.0);
            let height = match corpse.kind {
                MonsterKind::Shambler => 0.98,
                MonsterKind::Brisk => 1.02,
                MonsterKind::Bloater => 1.15,
                MonsterKind::Shooter => 1.0,
            };
            let motion = zombie::ZombieMotion {
                kind: corpse.kind,
                phase,
                speed: 0.0,
                hitstun: false,
                telegraph: 0.0,
                swelling: false,
                death,
            };
            let root = Mat4::from_translation(Vec3::new(corpse.at.x, corpse.at.z, corpse.at.y))
                * Mat4::from_quat(ground_align_quat(corpse.ground_normal))
                * Mat4::from_rotation_y(corpse.facing.x.atan2(corpse.facing.y));
            let collapse = root
                * Mat4::from_translation(Vec3::Y * (-height * 0.42 * death))
                * Mat4::from_translation(Vec3::Y * (height * 0.5))
                * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2 * death)
                * Mat4::from_translation(Vec3::Y * (-height * 0.5));
            records.push(zombie::ZombieDrawRecord {
                id: corpse.id,
                kind: corpse.kind,
                archetype,
                root: collapse * Mat4::from_scale(Vec3::splat(height)),
                skin_palette: zombie::skin_palette(rig, motion),
                color_tint: [1.0; 4],
                meta_override: actor_meta(corpse.floor),
                emissive_flash: [0.0; 4],
                height,
                telegraph_progress: 0.0,
                shooter_weapon: None,
            });
        }
        records
    }
    #[must_use]
    pub fn prepare(&mut self, scene: &DynamicSceneState, dt: f32) -> ActorBatches {
        self.prepare_with_openings(scene, dt, |_, _| None, |_| None)
    }

    pub(crate) fn prepare_with_openings(
        &mut self,
        scene: &DynamicSceneState,
        dt: f32,
        door_handle: impl FnOnce(Tile3D, Vec3) -> Option<Vec3>,
        window_opening: impl FnOnce(Tile3D) -> Option<(WallOrientation, f32)>,
    ) -> ActorBatches {
        let elapsed = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        self.phase = (self.phase + elapsed * 8.0) % std::f32::consts::TAU;
        let mut out = ActorBatches::new();
        let equipped = scene.player.render.equipped;
        let motion = match self.action {
            Some((PlayerAction::Swing, remaining)) if equipped == Some(Weapon::Axe) => {
                CombatPose::Swing((1.0 - remaining / 0.30).clamp(0.0, 1.0))
            }
            Some((PlayerAction::Shove, remaining)) => {
                CombatPose::Shove((remaining / 0.25 * std::f32::consts::PI).sin().max(0.0))
            }
            Some((PlayerAction::Recoil(weapon), remaining)) if equipped == Some(weapon) => {
                CombatPose::Recoil((remaining / 0.18).clamp(0.0, 1.0).powi(3))
            }
            _ => CombatPose::Ready,
        };
        let p = &scene.player.render;
        let angle = p.pose.facing.x.atan2(p.pose.facing.y);
        self.player_locomotion.update(
            scene.sim_tick,
            p.body.position.xy(),
            p.body.motion.is_grounded() && scene.mounted_vehicle.is_none(),
            elapsed,
        );
        let pitch = p
            .aim_dir
            .z
            .atan2((p.aim_dir.x * p.aim_dir.x + p.aim_dir.y * p.aim_dir.y).sqrt());
        let mut render_position =
            Vec3::new(p.body.position.x, p.body.position.z, p.body.position.y);
        let mut render_angle = angle;
        let mut interaction = InteractionPose::default();
        if let Some(vault) = self.vault {
            if let Some(pose) = vault.pose(render_position, angle, window_opening(vault.tile)) {
                (render_position, render_angle, interaction) = pose;
            } else {
                self.vault = None;
            }
        } else if let Some(door) = self.door {
            let handle =
                door_handle(door.tile, render_position).unwrap_or(door.center + Vec3::Y * 0.59);
            if p.pose.floor == door.tile.floor && render_position.distance(handle) < 1.38 {
                (render_position, render_angle, interaction) =
                    door.pose(render_position, angle, handle);
            }
        }
        let move_angle = self.player_locomotion.heading - render_angle;
        let aim = ((p.aim_dir.x.atan2(p.aim_dir.y) - render_angle + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI)
            * (1.0 - interaction.amount);
        let player_root = Mat4::from_translation(render_position)
            * Mat4::from_quat(
                ground_align_quat(p.ground_normal) * Quat::from_rotation_y(render_angle),
            );
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
            self.door = None;
            self.vault = None;
            self.was_mounted = mounted.is_some();
        }
        if self.draw_player && mounted.is_none() {
            draw_survivor(
                &mut out,
                player_root,
                self.player_locomotion.phase,
                self.player_locomotion.speed,
                aim,
                pitch,
                weapon,
                meta,
                motion,
                false,
                interaction,
                move_angle,
                None,
            );
        }
        if let (Some(camera), Some(part), None) = (&self.first_person, weapon, mounted) {
            let view_meta = VertexMeta::DEFAULT
                .with_tag(GeometryTag::Actor)
                .with_floor(p.pose.floor)
                .with_flags(MaterialFlags::ACTOR_MIN_LIGHT | MaterialFlags::FIRST_PERSON_HELD)
                .pack();
            draw_first_person(
                &mut out,
                camera,
                part,
                motion,
                self.player_locomotion.phase,
                self.player_locomotion.speed,
                view_meta,
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
                0.0,
                0.0,
                None,
                actor_meta(p.pose.floor),
                CombatPose::Ready,
                true,
                InteractionPose::default(),
                0.0,
                Some(contacts),
            );
        }
        if let Some((_, remaining)) = &mut self.action {
            *remaining -= elapsed;
            if *remaining <= 0.0 {
                self.action = None;
            }
        }
        if let Some(door) = &mut self.door {
            door.elapsed += elapsed;
            if door.elapsed >= DOOR_REACH_SECONDS {
                self.door = None;
            }
        }
        if let Some(vault) = &mut self.vault
            && !vault.advance(elapsed)
        {
            self.vault = None;
        }
        for corpse in &mut self.corpses {
            corpse.remaining -= elapsed;
            corpse.elapsed += elapsed;
        }
        self.corpses.retain(|c| c.remaining > 0.0);
        out
    }
}

fn actor_meta(floor: u8) -> [u32; 2] {
    VertexMeta::DEFAULT
        .with_tag(GeometryTag::Actor)
        .with_floor(floor)
        .with_flags(MaterialFlags::ACTOR_MIN_LIGHT | MaterialFlags::CUTAWAY_EXEMPT)
        .pack()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::SceneManager;
    use zombiebox_core::{ActionGame, TileVisibility};

    fn zombie_skeletons() -> Vec<(zombie::ZombieArchetype, zombie::ZombieSkeleton)> {
        crate::mesh::zombie_skeletons::all_zombie_skeletons()
            .into_iter()
            .map(|(name, rig)| {
                let archetype = match name {
                    "walker" => zombie::ZombieArchetype::Walker,
                    "woman" => zombie::ZombieArchetype::Woman,
                    "runner" => zombie::ZombieArchetype::Runner,
                    "bloated" => zombie::ZombieArchetype::Bloated,
                    "soldier" => zombie::ZombieArchetype::Soldier,
                    _ => unreachable!(),
                };
                (archetype, rig)
            })
            .collect()
    }

    #[test]
    fn skeletal_records_use_archetype_meshes_and_preserve_player_batches() {
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new()
            .prepare_at_focus(&game, game.player_tile().pos(), 0.0)
            .dynamic;
        for cell in &mut scene.local_visibility.cells {
            cell.visibility = TileVisibility::Visible;
        }
        let mut animator = ActorAnimator::new();
        let records = animator.prepare_zombie_records(&scene, &zombie_skeletons());
        assert_eq!(records.len(), scene.monsters.len());
        assert!(records.iter().all(|record| record.skin_palette.len() == 66));
        assert!(records.iter().all(|record| {
            record.archetype == zombie::zombie_archetype(record.kind, record.id)
        }));
        let batches = animator.prepare(&scene, 0.0);
        assert_eq!(batches[&ActorPart::Survivor(SurvivorNode::Pelvis)].len(), 1);
        for removed in [
            ActorPart::Body,
            ActorPart::Head,
            ActorPart::Limb,
            ActorPart::ZombieBody,
            ActorPart::ZombieHead,
            ActorPart::ZombieArmL,
            ActorPart::ZombieArmR,
            ActorPart::ZombieLegL,
            ActorPart::ZombieLegR,
            ActorPart::BoltLauncher,
        ] {
            assert!(
                !batches.contains_key(&removed),
                "legacy actor {removed:?} emitted"
            );
        }
    }

    #[test]
    fn sloped_ground_normal_tilts_actor_roots() {
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new()
            .prepare_at_focus(&game, game.player_tile().pos(), 0.0)
            .dynamic;
        for cell in &mut scene.local_visibility.cells {
            cell.visibility = TileVisibility::Visible;
        }
        let mut flat_animator = ActorAnimator::new();
        let flat = flat_animator.prepare(&scene, 0.0);
        let flat_player = flat[&ActorPart::Survivor(SurvivorNode::Pelvis)][0].model;
        let flat_monster =
            flat_animator.prepare_zombie_records(&scene, &zombie_skeletons())[0].root;

        let slope = zombiebox_core::Vec3::new(-0.35, 0.0, 1.0).normalized();
        scene.player.render.ground_normal = slope;
        for monster in &mut scene.monsters {
            monster.ground_normal = slope;
        }
        let mut sloped_animator = ActorAnimator::new();
        let sloped = sloped_animator.prepare(&scene, 0.0);
        let sloped_monster =
            sloped_animator.prepare_zombie_records(&scene, &zombie_skeletons())[0].root;
        assert_ne!(
            flat_player,
            sloped[&ActorPart::Survivor(SurvivorNode::Pelvis)][0].model
        );
        assert_ne!(flat_monster, sloped_monster);
    }

    #[test]
    fn hidden_monsters_never_emit_skeletal_records() {
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new()
            .prepare_at_focus(&game, game.player_tile().pos(), 0.0)
            .dynamic;
        assert!(!scene.monsters.is_empty());
        for cell in &mut scene.local_visibility.cells {
            cell.visibility = TileVisibility::Hidden;
        }
        let rigs = zombie_skeletons();
        let mut animator = ActorAnimator::new();
        assert!(animator.prepare_zombie_records(&scene, &rigs).is_empty());
        for cell in &mut scene.local_visibility.cells {
            cell.visibility = TileVisibility::Visible;
        }
        let visible = animator.prepare_zombie_records(&scene, &rigs);
        assert_eq!(visible.len(), scene.monsters.len());
        scene.local_visibility.cells.clear();
        assert!(animator.prepare_zombie_records(&scene, &rigs).is_empty());
    }

    #[test]
    fn gait_follows_travel_and_settles_with_stale_velocity() {
        let game = ActionGame::residential(42);
        let mut scene = SceneManager::new()
            .prepare_at_focus(&game, game.player_tile().pos(), 0.0)
            .dynamic;
        scene.monsters.clear();
        scene.player.render.body.velocity.x = 3.0;
        let mut animator = ActorAnimator::new();
        let left_boot = ActorPart::Survivor(SurvivorNode::FootL);
        let right_boot = ActorPart::Survivor(SurvivorNode::FootR);
        let idle = animator.prepare(&scene, 0.0);
        let idle_height = idle[&left_boot][0].model[3][1];
        assert!((idle_height - idle[&right_boot][0].model[3][1]).abs() < 1e-5);

        let dt = zombiebox_core::DT;
        let mut lifted = [false; 2];
        for _ in 0..30 {
            scene.sim_tick += 1;
            scene.player.render.body.position.x += 3.0 * dt;
            let moving = animator.prepare(&scene, dt);
            for (boot, lifted) in [left_boot, right_boot].into_iter().zip(&mut lifted) {
                *lifted |= moving[&boot][0].model[3][1] > idle_height + 0.05;
            }
        }
        assert_eq!(lifted, [true; 2]);

        let phase_at_stop = animator.player_locomotion.phase;
        for _ in 0..10 {
            scene.sim_tick += 1;
            let _ = animator.prepare(&scene, dt);
        }
        assert_eq!(animator.player_locomotion.phase, phase_at_stop);
        let stopped = animator.prepare(&scene, dt);
        assert!((stopped[&left_boot][0].model[3][1] - idle_height).abs() < 1e-5);
        assert!((stopped[&right_boot][0].model[3][1] - idle_height).abs() < 1e-5);
        scene.sim_tick += 1;
        let later = animator.prepare(&scene, dt);
        assert_eq!(stopped[&left_boot], later[&left_boot]);
        assert_eq!(stopped[&right_boot], later[&right_boot]);
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
        let rigs = zombie_skeletons();
        for cell in &mut scene.local_visibility.cells {
            cell.visibility = TileVisibility::Visible;
        }
        let visible = animator.prepare_zombie_records(&scene, &rigs);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, monster.id);
        assert!(visible[0].root.w_axis.y < monster.body.position.z + 0.2);
        for _ in 0..4 {
            let _ = animator.prepare(&scene, 0.1);
        }
        let fallen = animator.prepare_zombie_records(&scene, &rigs);
        assert_eq!(fallen.len(), 1);
        assert!(fallen[0].root.w_axis.y < monster.body.position.z + 0.15);
        for cell in &mut scene.local_visibility.cells {
            cell.visibility = TileVisibility::Hidden;
        }
        assert!(animator.prepare_zombie_records(&scene, &rigs).is_empty());
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
        let to = SimVec3::new(from.x + 2.0, from.y, from.z);
        let window = Tile3D::new(game.player_tile().x + 1, game.player_tile().y, 0);
        scene.player.render.body.position = to;
        let mut animator = ActorAnimator::new();
        animator.observe_events(&[ActionEvent::WindowClimbed {
            at: window,
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
