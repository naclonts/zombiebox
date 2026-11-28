//! Lightweight procedural poses for the imported zombie skeletons.
//!
//! Gameplay remains authoritative for movement and collision. This module only turns
//! those movement and combat cues into local joint motion and a skinning palette.
use crate::{coords::ground_align_quat, scene::DynamicSceneState};
use glam::{Mat4, Quat, Vec3};
use zombiebox_core::{MonsterId, MonsterKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZombieArchetype {
    Walker,
    Woman,
    Runner,
    Bloated,
    Soldier,
}

#[must_use]
pub fn zombie_archetype(kind: MonsterKind, id: MonsterId) -> ZombieArchetype {
    match kind {
        MonsterKind::Shambler if id.0 & 1 == 1 => ZombieArchetype::Woman,
        MonsterKind::Shambler => ZombieArchetype::Walker,
        MonsterKind::Brisk => ZombieArchetype::Runner,
        MonsterKind::Bloater => ZombieArchetype::Bloated,
        MonsterKind::Shooter => ZombieArchetype::Soldier,
    }
}

/// Skeleton metadata in the same order as the skinned mesh's joint indices.
#[derive(Clone, Debug)]
pub struct ZombieSkeleton {
    pub joints: Vec<ZombieJoint>,
}

#[derive(Clone, Debug)]
pub struct ZombieJoint {
    pub name: String,
    pub parent: Option<usize>,
    pub bind_local: Mat4,
    pub inverse_bind: Mat4,
}

/// Inputs sampled from the authoritative monster view. Phase is stable per actor.
#[derive(Clone, Copy, Debug)]
pub struct ZombieMotion {
    pub kind: MonsterKind,
    pub phase: f32,
    pub speed: f32,
    pub hitstun: bool,
    pub telegraph: f32,
    pub swelling: bool,
    /// 0 while alive, then ramps from 0 to 1 for the corpse collapse.
    pub death: f32,
}

/// Per-actor render data consumed by the skinned-mesh renderer.
#[derive(Clone, Debug)]
pub struct ZombieDrawRecord {
    pub id: MonsterId,
    pub kind: MonsterKind,
    pub archetype: ZombieArchetype,
    pub root: Mat4,
    pub skin_palette: Vec<Mat4>,
    pub color_tint: [f32; 4],
    pub meta_override: [u32; 2],
    pub emissive_flash: [f32; 4],
    pub height: f32,
    pub telegraph_progress: f32,
    /// Optional Shooter launcher transform in render-world space.
    pub shooter_weapon: Option<Mat4>,
}

/// Builds visible zombie skinning records without changing simulation state.
/// Motion phase derives from fixed simulation ticks and actor ID, so render FPS does not affect poses.
#[must_use]
pub fn visible_zombie_records(
    scene: &DynamicSceneState,
    skeletons: &[(ZombieArchetype, ZombieSkeleton)],
) -> Vec<ZombieDrawRecord> {
    let mut records = Vec::new();
    for monster in &scene.monsters {
        if !scene
            .local_visibility
            .is_actor_visible(monster.pos, monster.floor)
        {
            continue;
        }
        let archetype = zombie_archetype(monster.kind, monster.id);
        let Some((_, rig)) = skeletons.iter().find(|(kind, _)| *kind == archetype) else {
            continue;
        };
        let speed = monster.body.velocity.xy().length();
        let phase = scene.sim_tick as f32
            * zombiebox_core::DT
            * (if monster.kind == MonsterKind::Brisk {
                11.5
            } else {
                7.0
            })
            + (monster.id.0 % 997) as f32 * 0.021;
        let root = Mat4::from_translation(Vec3::new(
            monster.body.position.x,
            monster.body.position.z,
            monster.body.position.y,
        )) * Mat4::from_quat(ground_align_quat(monster.ground_normal))
            * Mat4::from_rotation_y(monster.facing.x.atan2(monster.facing.y));
        let height = match monster.kind {
            MonsterKind::Shambler => 0.98,
            MonsterKind::Brisk => 1.02,
            MonsterKind::Bloater => 1.15,
            MonsterKind::Shooter => 1.0,
        };
        let telegraph = monster.telegraph_progress.unwrap_or(0.0).clamp(0.0, 1.0);
        let motion = ZombieMotion {
            kind: monster.kind,
            phase,
            speed,
            hitstun: monster.hitstun,
            telegraph,
            swelling: monster.swelling,
            death: 0.0,
        };
        let meta = crate::mesh::VertexMeta::DEFAULT
            .with_tag(crate::mesh::GeometryTag::Actor)
            .with_floor(monster.floor)
            .with_flags(
                crate::mesh::MaterialFlags::ACTOR_MIN_LIGHT
                    | crate::mesh::MaterialFlags::CUTAWAY_EXEMPT,
            )
            .pack();
        let (skin_palette, globals) = pose_matrices(rig, motion);
        let shooter_weapon = if monster.kind == MonsterKind::Shooter {
            rig.joints
                .iter()
                .position(|joint| joint.name.to_ascii_lowercase().ends_with("righthand"))
                .map(|hand| {
                    root * Mat4::from_scale(Vec3::splat(height))
                        * globals[hand]
                        * Mat4::from_translation(Vec3::new(0.0, 0.0, 0.20))
                })
        } else {
            None
        };
        records.push(ZombieDrawRecord {
            id: monster.id,
            kind: monster.kind,
            archetype,
            root: root * Mat4::from_scale(Vec3::splat(height)),
            skin_palette,
            color_tint: [1.0; 4],
            meta_override: meta,
            emissive_flash: if monster.hitstun {
                [0.5, 0.1, 0.1, 0.5]
            } else {
                [0.0; 4]
            },
            height,
            telegraph_progress: telegraph,
            shooter_weapon,
        });
    }
    records
}

/// Mesh-space matrices (`joint_global * inverse_bind`) in source joint order.
#[must_use]
pub fn skin_palette(rig: &ZombieSkeleton, motion: ZombieMotion) -> Vec<Mat4> {
    pose_matrices(rig, motion).0
}

fn pose_matrices(rig: &ZombieSkeleton, motion: ZombieMotion) -> (Vec<Mat4>, Vec<Mat4>) {
    let mut globals = Vec::with_capacity(rig.joints.len());
    let mut palette = Vec::with_capacity(rig.joints.len());
    let speed = motion.speed.clamp(0.0, 5.0);
    let gait = motion.phase.sin() * (speed * 0.38).clamp(0.0, 0.8);
    let brisk = motion.kind == MonsterKind::Brisk;
    let shambler = motion.kind == MonsterKind::Shambler;
    let dying = motion.death.clamp(0.0, 1.0);
    let hit = if motion.hitstun { 1.0 } else { 0.0 };

    for joint in &rig.joints {
        let (bind_scale, bind_rotation, bind_translation) =
            joint.bind_local.to_scale_rotation_translation();
        // The generated skeletons use fixed Mixamo names. Strip the armature prefix
        // and compare case-insensitively without allocating on each rendered actor.
        let n = joint.name.strip_prefix("mixamorig:").unwrap_or(&joint.name);
        let mut delta = Quat::IDENTITY;
        let mut translation = Vec3::ZERO;
        if bone_ends_with(n, "hips") || bone_ends_with(n, "pelvis") {
            translation.y += if dying > 0.0 {
                0.0
            } else {
                (motion.phase * 2.0).sin() * speed * 0.002
            };
            delta *= Quat::from_rotation_z(hit * -0.16 + dying * 0.85);
        } else if bone_ends_with(n, "spine")
            || bone_ends_with(n, "spine1")
            || bone_ends_with(n, "spine2")
            || bone_ends_with(n, "chest")
        {
            let weight = if bone_ends_with(n, "spine") {
                0.45
            } else if bone_ends_with(n, "spine1") {
                0.7
            } else {
                1.0
            };
            let lean = if shambler {
                0.22
            } else if brisk {
                0.08
            } else {
                0.0
            };
            delta *= Quat::from_rotation_x(
                lean * weight + gait * 0.08 - hit * 0.2 + dying * 0.65 * weight,
            );
            delta *= Quat::from_rotation_z(-gait * 0.07 * weight);
        } else if bone_ends_with(n, "head") || bone_ends_with(n, "neck") {
            let head_weight = if bone_ends_with(n, "neck") { 0.35 } else { 1.0 };
            delta *=
                Quat::from_rotation_x(-0.13 * head_weight - gait * 0.06 + hit * 0.2 - dying * 0.38);
            delta *= Quat::from_rotation_z((motion.phase * 0.7).sin() * 0.06 * head_weight);
        } else if bone_ends_with(n, "upleg") || bone_ends_with(n, "thigh") {
            let side = if bone_starts_with(n, "left") {
                -1.0
            } else {
                1.0
            };
            let walk = gait * side * if brisk { 1.35 } else { 1.0 };
            delta *= Quat::from_rotation_x(walk + dying * side * 0.45);
        } else if bone_ends_with(n, "leg") && !bone_ends_with(n, "upleg") {
            let side = if bone_starts_with(n, "left") {
                -1.0
            } else {
                1.0
            };
            // Bend the trailing knee, alternating with the planted leg.
            delta *= Quat::from_rotation_x((gait * side).max(0.0) * 0.65 - dying * 0.25);
        } else if bone_ends_with(n, "foot") || bone_ends_with(n, "toe") {
            delta *= Quat::from_rotation_x(-gait * 0.12 + dying * 0.1);
        } else if bone_ends_with(n, "arm")
            || bone_ends_with(n, "forearm")
            || bone_ends_with(n, "shoulder")
        {
            let left = bone_starts_with(n, "left");
            let side = if left { -1.0 } else { 1.0 };
            let joint_weight = if bone_ends_with(n, "shoulder") {
                0.20
            } else if bone_ends_with(n, "forearm") {
                0.15
            } else if bone_ends_with(n, "arm") || bone_ends_with(n, "upperarm") {
                0.65
            } else {
                0.0
            };
            let reach = if shambler {
                0.72
            } else if brisk {
                0.20
            } else {
                0.16
            };
            let arm_swing = gait * side * 0.45;
            let attack = if left {
                0.0
            } else {
                motion.telegraph.clamp(0.0, 1.0) * 0.62
            };
            // Mixamo arms extend along local X: rotate about Y to swing toward
            // model-forward (+Z), and about Z to lower the horizontal T-pose.
            delta *= Quat::from_rotation_y(
                side * (reach + arm_swing + attack + hit * 0.25) * joint_weight,
            );
            let arm_drop = if shambler { 0.25 } else { 0.55 };
            delta *= Quat::from_rotation_z(
                (side * (arm_drop + gait * 0.08) + dying * 0.35) * joint_weight,
            );
        }
        // Bloater swelling is deliberately subtle; the mesh silhouette carries the archetype.
        if motion.kind == MonsterKind::Bloater
            && (bone_ends_with(n, "spine2") || bone_ends_with(n, "chest"))
        {
            let swell = if motion.swelling { 1.08 } else { 1.0 };
            let local = Mat4::from_scale_rotation_translation(
                bind_scale * Vec3::new(swell, swell, swell),
                bind_rotation * delta,
                bind_translation + translation,
            );
            let global = joint.parent.map_or(local, |parent| globals[parent] * local);
            palette.push(global * joint.inverse_bind);
            globals.push(global);
            continue;
        }
        let local = Mat4::from_scale_rotation_translation(
            bind_scale,
            bind_rotation * delta,
            bind_translation + translation,
        );
        let global = joint.parent.map_or(local, |parent| globals[parent] * local);
        palette.push(global * joint.inverse_bind);
        globals.push(global);
    }
    (palette, globals)
}

fn bone_ends_with(name: &str, suffix: &str) -> bool {
    name.len() >= suffix.len() && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

fn bone_starts_with(name: &str, prefix: &str) -> bool {
    name.len() >= prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_respects_bind_pose_and_joint_order() {
        let rig = ZombieSkeleton {
            joints: vec![
                ZombieJoint {
                    name: "hips".into(),
                    parent: None,
                    bind_local: Mat4::IDENTITY,
                    inverse_bind: Mat4::IDENTITY,
                },
                ZombieJoint {
                    name: "unknown".into(),
                    parent: Some(0),
                    bind_local: Mat4::from_translation(Vec3::Y),
                    inverse_bind: Mat4::from_translation(-Vec3::Y),
                },
            ],
        };
        let palette = skin_palette(
            &rig,
            ZombieMotion {
                kind: MonsterKind::Shambler,
                phase: 0.0,
                speed: 0.0,
                hitstun: false,
                telegraph: 0.0,
                swelling: false,
                death: 0.0,
            },
        );
        assert_eq!(palette.len(), 2);
        assert!(palette[0].abs_diff_eq(Mat4::IDENTITY, 1e-5));
        assert!(palette[1].abs_diff_eq(Mat4::IDENTITY, 1e-5));
    }

    #[test]
    fn gait_hit_and_death_drive_joint_palette() {
        let rig = ZombieSkeleton {
            joints: ["hips", "leftUpLeg", "leftArm", "spine"]
                .into_iter()
                .enumerate()
                .map(|(i, name)| ZombieJoint {
                    name: name.into(),
                    parent: (i > 0).then_some(0),
                    bind_local: Mat4::IDENTITY,
                    inverse_bind: Mat4::IDENTITY,
                })
                .collect(),
        };
        let pose = |phase, hitstun, death| {
            skin_palette(
                &rig,
                ZombieMotion {
                    kind: MonsterKind::Shambler,
                    phase,
                    speed: 2.0,
                    hitstun,
                    telegraph: 0.0,
                    swelling: false,
                    death,
                },
            )
        };
        let idle = pose(0.0, false, 0.0);
        let gait = pose(1.0, false, 0.0);
        let hit = pose(1.0, true, 0.0);
        let death = pose(1.0, false, 1.0);
        assert_ne!(idle, gait);
        assert_ne!(gait, hit);
        assert_ne!(gait, death);
    }

    #[test]
    fn bundled_archetype_skeletons_produce_finite_palettes() {
        for (_, rig) in crate::mesh::zombie_skeletons::all_zombie_skeletons() {
            assert_eq!(rig.joints.len(), 66);
            let palette = skin_palette(
                &rig,
                ZombieMotion {
                    kind: MonsterKind::Shambler,
                    phase: 0.7,
                    speed: 2.0,
                    hitstun: false,
                    telegraph: 0.0,
                    swelling: false,
                    death: 0.0,
                },
            );
            assert_eq!(palette.len(), rig.joints.len());
            assert!(
                palette
                    .iter()
                    .all(|matrix| matrix.to_cols_array().iter().all(|v| v.is_finite()))
            );
        }
    }

    #[test]
    fn mixamo_shambler_pose_lowers_and_reaches_forward_without_lifting_support_foot() {
        let (_, rig) = crate::mesh::zombie_skeletons::all_zombie_skeletons()
            .into_iter()
            .find(|(name, _)| *name == "walker")
            .unwrap();
        let bind = bind_globals(&rig);
        let (_, posed) = pose_matrices(
            &rig,
            ZombieMotion {
                kind: MonsterKind::Shambler,
                phase: std::f32::consts::FRAC_PI_2,
                speed: 1.0,
                hitstun: false,
                telegraph: 0.0,
                swelling: false,
                death: 0.0,
            },
        );
        let joint_pos = |globals: &[Mat4], suffix: &str| {
            let index = rig
                .joints
                .iter()
                .position(|joint| joint.name.to_ascii_lowercase().ends_with(suffix))
                .unwrap();
            globals[index].transform_point3(Vec3::ZERO)
        };
        for hand in ["lefthand", "righthand"] {
            let before = joint_pos(&bind, hand);
            let after = joint_pos(&posed, hand);
            assert!(after.z > before.z, "{hand} should reach toward +Z");
            assert!(after.y < before.y, "{hand} should lower from the T-pose");
        }
        let left_foot_before = joint_pos(&bind, "leftfoot");
        let left_foot_after = joint_pos(&posed, "leftfoot");
        assert!(
            (left_foot_after.y - left_foot_before.y).abs() < 0.05,
            "support foot moved vertically: {left_foot_before:?} -> {left_foot_after:?}"
        );
    }

    fn bind_globals(rig: &ZombieSkeleton) -> Vec<Mat4> {
        let mut globals = Vec::with_capacity(rig.joints.len());
        for joint in &rig.joints {
            let global = joint.parent.map_or(joint.bind_local, |parent| {
                globals[parent] * joint.bind_local
            });
            globals.push(global);
        }
        globals
    }
}
