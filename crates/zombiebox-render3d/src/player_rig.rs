//! Player-only rigid hierarchy posing and contact helpers.
use crate::animation::{ActorBatches, ActorInstance};
use crate::mesh::{actors::ActorPart, survivor::SurvivorNode};
use glam::{Mat4, Quat, Vec3};

#[derive(Clone, Copy, Debug)]
pub(crate) struct RiderContacts {
    pub hands: [Vec3; 2],
    pub feet: [Vec3; 2],
}

/// FK for the authored rigid hierarchy. Locomotion joints remain independent from
/// upper-body aim and action layers; gameplay continues to own root translation.
pub(crate) fn draw_survivor(
    out: &mut ActorBatches,
    root: Mat4,
    phase: f32,
    speed: f32,
    quiet: bool,
    aim: f32,
    pitch: f32,
    weapon: Option<ActorPart>,
    meta: [u32; 2],
    action: f32,
    seated: bool,
    reach: f32,
    vault: f32,
    move_yaw: f32,
    contacts: Option<RiderContacts>,
) {
    let nodes = SurvivorNode::ALL;
    let mut world = [Mat4::IDENTITY; 25];
    let gait = phase.sin()
        * (speed * if quiet { 0.24 } else { 0.34 }).clamp(0.0, if quiet { 0.36 } else { 0.55 });
    let pelvis_rot = Mat4::from_translation(Vec3::new(0.0, if quiet { -0.035 } else { 0.0 }, 0.0))
        * Mat4::from_rotation_x(if seated { -0.25 } else { gait * 0.12 });
    let torso_rot = Mat4::from_rotation_y(aim * 0.42)
        * Mat4::from_rotation_x(if seated {
            0.20
        } else {
            -pitch * 0.30 + vault * 0.32 + if quiet { 0.10 } else { 0.0 }
        });
    let torso_world = root
        * Mat4::from_translation(Vec3::from_array(SurvivorNode::Pelvis.bind_translation()))
        * pelvis_rot
        * Mat4::from_translation(Vec3::from_array(SurvivorNode::Torso.bind_translation()))
        * torso_rot;
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
            ik[slot] = a;
            ik[slot + 1] = b;
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
    for (i, node) in nodes.iter().copied().enumerate() {
        let local = match node {
            SurvivorNode::Root => Mat4::IDENTITY,
            SurvivorNode::Pelvis => pelvis_rot,
            SurvivorNode::Torso => torso_rot,
            SurvivorNode::Head => {
                Mat4::from_rotation_y(aim * 0.58) * Mat4::from_rotation_x(-pitch * 0.40)
            }
            SurvivorNode::UpperArmR => {
                if contacts.is_some() {
                    Mat4::from_quat(ik[2])
                } else {
                    Mat4::from_rotation_y(aim)
                        * Mat4::from_rotation_x(if weapon.is_some() || seated {
                            -1.15 - pitch * 0.35 + action
                        } else {
                            -gait * 0.45 + action
                        })
                }
            }
            SurvivorNode::ForearmR => {
                if contacts.is_some() {
                    Mat4::from_quat(ik[3])
                } else {
                    Mat4::from_rotation_x(if weapon.is_some() { -0.35 } else { 0.0 })
                }
            }
            SurvivorNode::UpperArmL => {
                if contacts.is_some() {
                    Mat4::from_quat(ik[0])
                } else {
                    Mat4::from_rotation_y(if seated { -0.35 } else { aim * 0.55 })
                        * Mat4::from_rotation_x(if seated || weapon.is_some() {
                            -0.95 - pitch * 0.25 - reach * 0.8
                        } else {
                            gait * 0.45 - reach * 0.8
                        })
                }
            }
            SurvivorNode::ForearmL => {
                if contacts.is_some() {
                    Mat4::from_quat(ik[1])
                } else {
                    Mat4::from_rotation_x(if seated || weapon.is_some() {
                        -0.35 + reach * 0.35
                    } else {
                        reach
                    })
                }
            }
            SurvivorNode::ThighL => {
                if contacts.is_some() {
                    Mat4::from_quat(ik[4])
                } else {
                    Mat4::from_rotation_y(move_yaw)
                        * Mat4::from_rotation_x(if seated { -1.0 } else { gait - vault * 0.9 })
                }
            }
            SurvivorNode::ThighR => {
                if contacts.is_some() {
                    Mat4::from_quat(ik[6])
                } else {
                    Mat4::from_rotation_y(move_yaw)
                        * Mat4::from_rotation_x(if seated { -1.0 } else { -gait - vault * 0.9 })
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
                    Mat4::from_rotation_x(if seated {
                        0.9
                    } else {
                        -gait.abs() * 0.65 + vault * 1.0
                    })
                }
            }
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
            out.entry(ActorPart::Survivor(node))
                .or_default()
                .push(ActorInstance {
                    model: world[i].to_cols_array_2d(),
                    color_tint: [1.0; 4],
                    meta_override: meta,
                    emissive_flash: [0.0; 4],
                });
        }
    }
    if let Some(weapon) = weapon {
        let grip = world[nodes
            .iter()
            .position(|n| *n == SurvivorNode::SocketGripR)
            .unwrap()];
        // Authored weapons point along +Z; rotate that axis into the survivor's forward aim.
        let correction = Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        out.entry(weapon).or_default().push(ActorInstance {
            model: (grip * correction).to_cols_array_2d(),
            color_tint: [1.0; 4],
            meta_override: meta,
            emissive_flash: [0.0; 4],
        });
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
