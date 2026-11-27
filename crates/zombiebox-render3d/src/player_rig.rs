//! Player-only rigid hierarchy posing and contact helpers.
use crate::animation::{ActorBatches, ActorInstance};
use crate::camera::CameraFrame;
use crate::mesh::{actors::ActorPart, survivor::SurvivorNode};
use glam::{Mat4, Quat, Vec3};

mod weapon_pose;
use weapon_pose::{shoulder_turn, weapon_stance};

/// Reuse the survivor's arms, weapon meshes and attack poses in viewer space.
pub(crate) fn draw_first_person(
    out: &mut ActorBatches,
    camera: &CameraFrame,
    part: ActorPart,
    action: CombatPose,
    phase: f32,
    speed: f32,
    meta: [u32; 2],
) {
    let gait = (phase * std::f32::consts::TAU).cos() * (speed / 5.2).clamp(0.0, 1.0);
    let (drop, depth) = match part {
        ActorPart::Axe => (0.215, 0.251),
        ActorPart::Pistol => (0.382, 0.153),
        ActorPart::Shotgun => (0.342, 0.183),
        ActorPart::RocketLauncher => (0.317, 0.204),
        _ => return,
    };
    // In the authored rig, -X is the survivor's right and +Z points forward.
    let depth_adjustment = (camera.near - 0.10).max(0.0);
    let camera_root = Mat4::from_cols(
        (-camera.right).extend(0.0),
        camera.up.extend(0.0),
        camera.forward.extend(0.0),
        camera.eye.extend(1.0),
    );
    let twist = Quat::from_rotation_y(shoulder_turn(Some(part), action));
    let torso_view = Mat4::from_rotation_translation(
        twist,
        Vec3::new(0.0, -drop + gait.abs() * 0.009, depth + depth_adjustment),
    );
    let torso = camera_root * torso_view;
    let to_torso = torso_view.inverse();
    let mut arms = ready_arms(gait, action);
    if let Some(weapon) = weapon_stance(
        part,
        action,
        twist.inverse(),
        0.0,
        gait * 0.32,
        InteractionPose::default(),
        &mut arms,
    ) {
        for (side, pose) in ArmSide::BOTH.into_iter().zip(arms) {
            if part == ActorPart::Axe && matches!(side, ArmSide::Left) {
                continue;
            }
            let [upper_node, forearm_node, hand_node, _] = side.nodes();
            let [shoulder_bind, upper, lower, socket] = side.bind();
            let grip = pose.grip(side);
            let hand_rotation = pose.upper * pose.forearm * pose.hand;
            let wrist = grip - hand_rotation * socket;
            // Set the shoulder just below the view so the arm enters from the
            // screen edge. Keep the same held weapon and exact wrist contact.
            let x = shoulder_bind.x.signum();
            let shoulder =
                to_torso.transform_point3(Vec3::new(x * 0.205, -0.235, 0.067 + depth_adjustment));
            let limit = upper.length() + lower.length() - 0.011;
            let shoulder = wrist + (shoulder - wrist).clamp_length_max(limit);
            let pole = to_torso.transform_vector3(Vec3::new(x * 0.60, -1.0, -0.2));
            let pose = solve_arm_at(side, shoulder, grip, hand_rotation, pole);
            let mut model = torso * Mat4::from_rotation_translation(pose.upper, shoulder);
            emit_part(out, ActorPart::Survivor(upper_node), model, meta);
            for (node, rotation) in [(forearm_node, pose.forearm), (hand_node, pose.hand)] {
                model *= Mat4::from_rotation_translation(
                    rotation,
                    Vec3::from_array(node.bind_translation()),
                );
                emit_part(out, ActorPart::Survivor(node), model, meta);
            }
        }
        emit_part(out, part, torso * weapon, meta);
    }
}

#[derive(Clone, Copy)]
pub(crate) enum CombatPose {
    Ready,
    Swing(f32),
    Shove(f32),
    Recoil(f32),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RiderContacts {
    pub hands: [Vec3; 2],
    pub feet: [Vec3; 2],
}

#[derive(Clone, Copy, Default)]
pub(crate) struct InteractionPose {
    pub hands: [Option<(Vec3, f32)>; 2],
    pub ankles: Option<[Vec3; 2]>,
    pub amount: f32,
    pub lean: f32,
    pub pelvis_drop: f32,
}

const SOLE_HEIGHT: f32 = 0.046703;

fn gait_pace(speed: f32) -> f32 {
    ((speed - 0.65) / 4.55).clamp(0.0, 1.0)
}

fn gait_dimensions(pace: f32) -> (f32, f32) {
    let half_stride = 0.162 + 0.128 * pace;
    let support_fraction = 0.62 - 0.22 * pace;
    (half_stride, support_fraction)
}

pub(crate) fn stride_distance(speed: f32) -> f32 {
    let (half_stride, support_fraction) = gait_dimensions(gait_pace(speed));
    let stride_scale = (speed / 1.35).clamp(0.42, 1.0);
    // Match the boot's ground sweep to travel, with a small cadence allowance.
    2.12 * half_stride * stride_scale / support_fraction
}

pub(crate) fn stance_ankles() -> [Vec3; 2] {
    [1.0, -1.0].map(|side| Vec3::new(side * 0.070143, SOLE_HEIGHT, side * 0.023))
}

#[derive(Clone, Copy)]
struct Ankle {
    position: Vec3,
    orientation: Quat,
}

fn gait_ankle(mut position: Vec3, phase: f32, weight: f32, pace: f32, direction: Vec3) -> Ankle {
    let side = position.x.signum();
    let (half_stride, support) = gait_dimensions(pace);
    let half_stride = half_stride * weight;
    let (offset, lift, pitch) = if phase < support {
        let time = phase / support;
        let heel = (1.0 - time * 6.0).max(0.0);
        let toe = ((time - 0.72) / 0.28).max(0.0);
        (
            (1.0 - 2.0 * time) * half_stride,
            0.0,
            0.32 * toe * toe - 0.18 * heel * heel,
        )
    } else {
        let time = (phase - support) / (1.0 - support);
        let swing = time * time * (3.0 - 2.0 * time);
        let clearance = (4.0 * time * (1.0 - time)).powi(2);
        (
            (2.0 * swing - 1.0) * half_stride,
            clearance * (0.045 + 0.071 * pace) * weight,
            0.32 - 0.50 * swing,
        )
    };
    let pitch = pitch * weight;
    let (sine, cosine) = pitch.sin_cos();
    // Keep the toe or heel touching the ground as the boot rolls around the ankle.
    let sole_extension = if pitch > 0.0 { 0.099451 } else { -0.042308 };
    position.x += (offset * direction.x).clamp(-0.06, 0.06);
    position.x += side * direction.x.abs() * 0.020 * weight;
    position.y = SOLE_HEIGHT * cosine + sine * sole_extension + lift;
    position.z += offset * direction.z + side * direction.x.abs() * 0.030 * weight;
    let toe_yaw = side * 0.13 + direction.x * weight * 0.48;
    Ankle {
        position,
        orientation: Quat::from_rotation_y(toe_yaw) * Quat::from_rotation_x(pitch),
    }
}

fn leg_pose(
    hip: SurvivorNode,
    shin: SurvivorNode,
    foot: SurvivorNode,
    ankle: Ankle,
    root_to_pelvis: Mat4,
) -> [Quat; 3] {
    let joint = Vec3::from_array(hip.bind_translation());
    let goal = root_to_pelvis.transform_point3(ankle.position);
    let foot_orientation = Quat::from_mat4(&root_to_pelvis) * ankle.orientation;
    let thigh_bone = Vec3::from_array(shin.bind_translation());
    let shin_bone = Vec3::from_array(foot.bind_translation());
    let (thigh, calf) = two_bone(
        joint,
        goal,
        thigh_bone.length(),
        shin_bone.length(),
        foot_orientation * Vec3::Z,
    );
    // The authored bone translations include small lateral offsets from local -Y.
    let thigh_correction = Quat::from_rotation_arc(thigh_bone.normalize(), Vec3::NEG_Y);
    let shin_correction = Quat::from_rotation_arc(shin_bone.normalize(), Vec3::NEG_Y);
    [
        thigh * thigh_correction,
        thigh_correction.inverse() * calf * shin_correction,
        (thigh * calf * shin_correction).inverse() * foot_orientation,
    ]
}

/// FK for the authored rigid hierarchy. Locomotion joints remain independent from
/// upper-body aim and action layers; gameplay continues to own root translation.
pub(crate) fn draw_survivor(
    out: &mut ActorBatches,
    root: Mat4,
    phase: f32,
    speed: f32,
    aim: f32,
    pitch: f32,
    weapon: Option<ActorPart>,
    meta: [u32; 2],
    action: CombatPose,
    seated: bool,
    interaction: InteractionPose,
    move_yaw: f32,
    contacts: Option<RiderContacts>,
) {
    let nodes = SurvivorNode::ALL;
    let mut world = [Mat4::IDENTITY; 25];
    let vault_blend = interaction.ankles.map_or(0.0, |_| interaction.amount);
    let movement_weight = (speed / 1.35).clamp(0.0, 1.0);
    let weight = movement_weight * (1.0 - vault_blend);
    let pace = gait_pace(speed);
    let cycle = phase * std::f32::consts::TAU;
    let gait = cycle.cos() * weight * (0.32 + 0.24 * pace);
    let (move_x, move_z) = move_yaw.sin_cos();
    let direction = Vec3::new(move_x, 0.0, move_z);
    let [left, right] = stance_ankles();
    let mut ankles = [
        gait_ankle(left, phase, movement_weight, pace, direction),
        gait_ankle(
            right,
            (phase + 0.5).fract(),
            movement_weight,
            pace,
            direction,
        ),
    ];
    if let Some(targets) = interaction.ankles {
        for (ankle, target) in ankles.iter_mut().zip(targets) {
            ankle.position = ankle.position.lerp(target, vault_blend);
            let toe_out = Quat::from_rotation_y(target.x.signum() * 0.13);
            ankle.orientation = ankle.orientation.slerp(toe_out, vault_blend);
        }
    }
    let pelvis_bind = Vec3::from_array(SurvivorNode::Pelvis.bind_translation());
    let pelvis_rot = if seated {
        Mat4::from_rotation_x(-0.25)
    } else {
        let roll = cycle.sin() * weight;
        let rotation = Quat::from_euler(glam::EulerRot::YXZ, -0.14 * gait, 0.0, 0.038 * roll);
        let mut position = pelvis_bind
            + Vec3::new(
                0.012 * roll,
                -0.022
                    - weight * (0.024 + 0.031 * pace + 0.009 * (2.0 * cycle).cos())
                    - interaction.pelvis_drop,
                0.0,
            );
        let reach = Vec3::from_array(SurvivorNode::ShinL.bind_translation()).length()
            + Vec3::from_array(SurvivorNode::FootL.bind_translation()).length()
            - 0.004;
        let mut reachable_height = position.y;
        for (hip, ankle) in [SurvivorNode::ThighL, SurvivorNode::ThighR]
            .into_iter()
            .zip(ankles)
        {
            let hip = rotation * Vec3::from_array(hip.bind_translation());
            let delta = ankle.position - position - hip;
            let horizontal = delta.x * delta.x + delta.z * delta.z;
            let limit = ankle.position.y - hip.y + (reach * reach - horizontal).max(0.0).sqrt();
            reachable_height = reachable_height.min(limit);
        }
        position.y += (reachable_height - position.y) * (1.0 - vault_blend);
        Mat4::from_rotation_translation(rotation, position - pelvis_bind)
    };
    let shoulder_turn = shoulder_turn(weapon, action) * (1.0 - interaction.amount);
    let torso_rot = Mat4::from_rotation_y(aim * 0.42 + shoulder_turn)
        * Mat4::from_rotation_x(if seated {
            0.20
        } else {
            -pitch * 0.30 * (1.0 - interaction.amount)
                + interaction.lean
                + weight * (0.055 + 0.12 * pace)
        });
    let torso_local =
        Mat4::from_translation(Vec3::from_array(SurvivorNode::Pelvis.bind_translation()))
            * pelvis_rot
            * Mat4::from_translation(Vec3::from_array(SurvivorNode::Torso.bind_translation()))
            * torso_rot;
    let torso_world = root * torso_local;
    let mut arms = ready_arms(gait, action);
    let held_weapon = weapon.and_then(|part| {
        let (_, torso_rotation, _) = torso_local.to_scale_rotation_translation();
        weapon_stance(
            part,
            action,
            torso_rotation.inverse() * Quat::from_rotation_y(aim),
            pitch,
            gait,
            interaction,
            &mut arms,
        )
    });
    if interaction.amount > 0.0 {
        let root_to_torso = torso_local.inverse();
        let hand_rotation = Quat::from_mat4(&root_to_torso) * Quat::from_rotation_x(-1.05);
        for (index, (side, target)) in ArmSide::BOTH.into_iter().zip(interaction.hands).enumerate()
        {
            if let Some((point, weight)) = target {
                let grip = root_to_torso.transform_point3(point);
                let reach = solve_arm(side, grip, hand_rotation, side.pole());
                arms[index] = arms[index].blend(reach, weight);
            }
        }
    }
    let mut ik = [Quat::IDENTITY; 8];
    if let Some(targets) = contacts {
        for (upper, target, slot) in [
            (SurvivorNode::UpperArmL, targets.hands[0], 0usize),
            (SurvivorNode::UpperArmR, targets.hands[1], 2),
        ] {
            let shoulder = Vec3::from_array(upper.bind_translation());
            let goal = torso_world.inverse().transform_point3(target);
            let upper_len = Vec3::from_array(SurvivorNode::ForearmL.bind_translation()).length();
            let lower_len = Vec3::from_array(SurvivorNode::HandL.bind_translation()).length()
                + Vec3::from_array(SurvivorNode::SocketGripL.bind_translation()).length();
            let (a, b) = two_bone(shoulder, goal, upper_len, lower_len, Vec3::Z);
            arms[slot / 2] = ArmPose {
                upper: a,
                forearm: b,
                hand: Quat::IDENTITY,
            };
        }
        let pelvis_world = root
            * Mat4::from_translation(Vec3::from_array(SurvivorNode::Pelvis.bind_translation()))
            * pelvis_rot;
        for (upper, target, slot) in [
            (SurvivorNode::ThighL, targets.feet[0], 4usize),
            (SurvivorNode::ThighR, targets.feet[1], 6usize),
        ] {
            let joint = Vec3::from_array(upper.bind_translation());
            let goal = pelvis_world.inverse().transform_point3(target);
            let upper_len = Vec3::from_array(SurvivorNode::ShinL.bind_translation()).length();
            let lower_len = Vec3::from_array(SurvivorNode::FootL.bind_translation()).length()
                + Vec3::from_array(SurvivorNode::SocketFootpegL.bind_translation()).length();
            let (a, b) = two_bone(joint, goal, upper_len, lower_len, Vec3::Z);
            ik[slot] = a;
            ik[slot + 1] = b;
        }
    }
    let ground_legs = if seated {
        [[Quat::IDENTITY; 3]; 2]
    } else {
        let root_to_pelvis = (Mat4::from_translation(pelvis_bind) * pelvis_rot).inverse();
        [
            leg_pose(
                SurvivorNode::ThighL,
                SurvivorNode::ShinL,
                SurvivorNode::FootL,
                ankles[0],
                root_to_pelvis,
            ),
            leg_pose(
                SurvivorNode::ThighR,
                SurvivorNode::ShinR,
                SurvivorNode::FootR,
                ankles[1],
                root_to_pelvis,
            ),
        ]
    };
    for (i, node) in nodes.iter().copied().enumerate() {
        let local = match node {
            SurvivorNode::Root => Mat4::IDENTITY,
            SurvivorNode::Pelvis => pelvis_rot,
            SurvivorNode::Torso => torso_rot,
            SurvivorNode::Head => {
                Mat4::from_rotation_y(aim * 0.58 - shoulder_turn)
                    * Mat4::from_rotation_x(-pitch * 0.40)
            }
            SurvivorNode::UpperArmL => Mat4::from_quat(arms[0].upper),
            SurvivorNode::ForearmL => Mat4::from_quat(arms[0].forearm),
            SurvivorNode::HandL => Mat4::from_quat(arms[0].hand),
            SurvivorNode::UpperArmR => Mat4::from_quat(arms[1].upper),
            SurvivorNode::ForearmR => Mat4::from_quat(arms[1].forearm),
            SurvivorNode::HandR => Mat4::from_quat(arms[1].hand),
            SurvivorNode::ThighL => {
                if contacts.is_some() {
                    Mat4::from_quat(ik[4])
                } else {
                    Mat4::from_quat(ground_legs[0][0])
                }
            }
            SurvivorNode::ThighR => {
                if contacts.is_some() {
                    Mat4::from_quat(ik[6])
                } else {
                    Mat4::from_quat(ground_legs[1][0])
                }
            }
            SurvivorNode::ShinL | SurvivorNode::ShinR => {
                if contacts.is_some() {
                    Mat4::from_quat(if node == SurvivorNode::ShinL {
                        ik[5]
                    } else {
                        ik[7]
                    })
                } else {
                    let side = usize::from(node == SurvivorNode::ShinR);
                    Mat4::from_quat(ground_legs[side][1])
                }
            }
            SurvivorNode::FootL => Mat4::from_quat(ground_legs[0][2]),
            SurvivorNode::FootR => Mat4::from_quat(ground_legs[1][2]),
            _ => Mat4::IDENTITY,
        };
        let local = Mat4::from_translation(Vec3::from_array(node.bind_translation())) * local;
        world[i] = if let Some(parent) = node.parent() {
            world[nodes.iter().position(|n| *n == parent).unwrap()] * local
        } else {
            root * local
        };
        if matches!(
            node,
            SurvivorNode::Pelvis
                | SurvivorNode::Torso
                | SurvivorNode::Head
                | SurvivorNode::UpperArmL
                | SurvivorNode::ForearmL
                | SurvivorNode::HandL
                | SurvivorNode::ThighL
                | SurvivorNode::ShinL
                | SurvivorNode::FootL
                | SurvivorNode::UpperArmR
                | SurvivorNode::ForearmR
                | SurvivorNode::HandR
                | SurvivorNode::ThighR
                | SurvivorNode::ShinR
                | SurvivorNode::FootR
        ) {
            emit_part(out, ActorPart::Survivor(node), world[i], meta);
        }
    }
    if let Some((weapon, model)) = weapon.zip(held_weapon) {
        emit_part(out, weapon, torso_world * model, meta);
    }
}

fn emit_part(out: &mut ActorBatches, part: ActorPart, model: Mat4, meta: [u32; 2]) {
    out.entry(part).or_default().push(ActorInstance {
        model: model.to_cols_array_2d(),
        color_tint: [1.0; 4],
        meta_override: meta,
        emissive_flash: [0.0; 4],
    });
}

#[derive(Clone, Copy)]
enum ArmSide {
    Left,
    Right,
}

impl ArmSide {
    const BOTH: [Self; 2] = [Self::Left, Self::Right];

    fn nodes(self) -> [SurvivorNode; 4] {
        use SurvivorNode::*;
        match self {
            Self::Left => [UpperArmL, ForearmL, HandL, SocketGripL],
            Self::Right => [UpperArmR, ForearmR, HandR, SocketGripR],
        }
    }

    fn bind(self) -> [Vec3; 4] {
        self.nodes()
            .map(|node| Vec3::from_array(node.bind_translation()))
    }

    fn pole(self) -> Vec3 {
        let outward = match self {
            Self::Left => 0.7,
            Self::Right => -0.7,
        };
        Vec3::new(outward, -1.0, -0.28)
    }

    fn limit_grip(self, grip: Vec3, hand_rotation: Quat) -> Vec3 {
        let [shoulder, upper, lower, socket] = self.bind();
        let center = shoulder + hand_rotation * socket;
        center + (grip - center).clamp_length_max(upper.length() + lower.length() - 0.003)
    }
}

#[derive(Clone, Copy)]
struct ArmPose {
    upper: Quat,
    forearm: Quat,
    hand: Quat,
}

impl ArmPose {
    fn blend(self, other: Self, weight: f32) -> Self {
        Self {
            upper: self.upper.slerp(other.upper, weight),
            forearm: self.forearm.slerp(other.forearm, weight),
            hand: self.hand.slerp(other.hand, weight),
        }
    }

    fn grip(self, side: ArmSide) -> Vec3 {
        let [shoulder, upper, lower, socket] = side.bind();
        let forearm = self.upper * self.forearm;
        shoulder + self.upper * upper + forearm * (lower + self.hand * socket)
    }
}

fn ready_arms(gait: f32, action: CombatPose) -> [ArmPose; 2] {
    let mut arms = [1.0, -1.0].map(|side| ArmPose {
        upper: Quat::from_rotation_z(side * 0.10) * Quat::from_rotation_x(gait * side * 0.55),
        forearm: Quat::from_rotation_x(-0.12),
        hand: Quat::IDENTITY,
    });
    if let CombatPose::Shove(amount) = action {
        let side = ArmSide::Right;
        let shove = solve_arm(
            side,
            Vec3::new(-0.085, 0.16, 0.309),
            Quat::from_rotation_x(-1.05),
            side.pole(),
        );
        arms[1] = arms[1].blend(shove, amount);
    }
    arms
}

/// Aim the wrist so its authored grip socket lands on the contact point.
fn solve_arm(side: ArmSide, grip: Vec3, hand_rotation: Quat, pole: Vec3) -> ArmPose {
    solve_arm_at(side, side.bind()[0], grip, hand_rotation, pole)
}

fn solve_arm_at(
    side: ArmSide,
    shoulder: Vec3,
    grip: Vec3,
    hand_rotation: Quat,
    pole: Vec3,
) -> ArmPose {
    let [_, upper, lower, socket] = side.bind();
    let wrist = grip - hand_rotation * socket;
    let (a, b) = two_bone(shoulder, wrist, upper.length(), lower.length(), pole);
    let upper_in_torso = a * Quat::from_rotation_arc(upper.normalize(), Vec3::NEG_Y);
    let lower_in_torso = a * b * Quat::from_rotation_arc(lower.normalize(), Vec3::NEG_Y);
    ArmPose {
        upper: upper_in_torso,
        forearm: upper_in_torso.inverse() * lower_in_torso,
        hand: lower_in_torso.inverse() * hand_rotation,
    }
}

/// Return parent-local rotations for two rigid bones extending down local -Y.
fn two_bone(joint: Vec3, target: Vec3, upper_len: f32, lower_len: f32, pole: Vec3) -> (Quat, Quat) {
    let delta = target - joint;
    let raw_distance = delta.length();
    let direction = if raw_distance > 1e-5 {
        delta / raw_distance
    } else {
        Vec3::NEG_Y
    };
    let distance = raw_distance.clamp(0.02, upper_len + lower_len - 0.002);
    let endpoint = joint + direction * distance;
    let along = ((upper_len * upper_len - lower_len * lower_len + distance * distance)
        / (2.0 * distance))
        .clamp(0.0, upper_len);
    let perpendicular = pole - direction * pole.dot(direction);
    let perpendicular = if perpendicular.length_squared() > 1e-6 {
        perpendicular.normalize()
    } else {
        let fallback = if direction.x.abs() < 0.8 {
            Vec3::X
        } else {
            Vec3::Z
        };
        (fallback - direction * fallback.dot(direction)).normalize_or_zero()
    };
    let height = (upper_len * upper_len - along * along).max(0.0).sqrt();
    let elbow = joint + direction * along + perpendicular * height;
    let upper_direction = (elbow - joint).normalize_or_zero();
    let lower_direction = (endpoint - elbow).normalize_or_zero();
    let upper = Quat::from_rotation_arc(
        Vec3::NEG_Y,
        if upper_direction.length_squared() > 0.5 {
            upper_direction
        } else {
            Vec3::NEG_Y
        },
    );
    let lower_world = Quat::from_rotation_arc(
        Vec3::NEG_Y,
        if lower_direction.length_squared() > 0.5 {
            lower_direction
        } else {
            Vec3::NEG_Y
        },
    );
    (upper, upper.inverse() * lower_world)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{CameraMode, CameraRig, CameraTarget};

    #[test]
    fn first_person_axe_grip_is_visible_during_carry_and_swing() {
        let mut camera = CameraRig::new(640, 360);
        camera.set_mode(CameraMode::FirstPerson);
        let frame = camera.update(CameraTarget::default(), 0.0);
        let mut previous = None;
        for pose in [
            CombatPose::Ready,
            CombatPose::Swing(0.27),
            CombatPose::Swing(0.57),
        ] {
            let mut instances = ActorBatches::new();
            draw_first_person(
                &mut instances,
                &frame,
                ActorPart::Axe,
                pose,
                0.0,
                0.0,
                [0; 2],
            );
            assert_eq!(instances.len(), 4);
            assert!(instances.contains_key(&ActorPart::Survivor(SurvivorNode::ForearmR)));
            let axe = Mat4::from_cols_array_2d(&instances[&ActorPart::Axe][0].model);
            let hand = Mat4::from_cols_array_2d(
                &instances[&ActorPart::Survivor(SurvivorNode::HandR)][0].model,
            );
            let grip = hand.transform_point3(Vec3::from_array(
                SurvivorNode::SocketGripR.bind_translation(),
            ));
            assert!(axe.w_axis.truncate().distance(grip) < 1e-5);
            let point = frame.project_to_screen(grip).unwrap();
            assert!(point.depth > 0.0 && point.depth < 1.0);
            assert!((0.0..640.0).contains(&point.px.x));
            assert!((0.0..360.0).contains(&point.px.y));
            assert_ne!(previous, Some(axe));
            previous = Some(axe);
        }
    }

    #[test]
    fn ik_reaches_named_socket_distance_and_handles_degenerate_poles() {
        let joint = Vec3::new(0.13, 0.8, 0.0);
        let target = Vec3::new(0.31, 0.62, 0.12);
        let upper_len = Vec3::from_array(SurvivorNode::ForearmL.bind_translation()).length();
        let lower_len = Vec3::from_array(SurvivorNode::HandL.bind_translation()).length()
            + Vec3::from_array(SurvivorNode::SocketGripL.bind_translation()).length();
        let (upper, lower) = two_bone(joint, target, upper_len, lower_len, Vec3::Z);
        let end =
            joint + upper * Vec3::NEG_Y * upper_len + (upper * lower) * Vec3::NEG_Y * lower_len;
        assert!(end.distance(target) < 1e-4);
        let (a, b) = two_bone(joint, joint, upper_len, lower_len, Vec3::NEG_Y);
        assert!(a.is_finite() && b.is_finite());
        assert!(a.length_squared() > 0.99 && b.length_squared() > 0.99);
    }
}
