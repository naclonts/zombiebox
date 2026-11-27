use super::{ArmPose, ArmSide, CombatPose, InteractionPose, solve_arm};
use crate::mesh::actors::ActorPart;
use crate::mesh::weapons::{WeaponModel, weapon_nodes};
use glam::{Mat4, Quat, Vec3};

pub(super) fn shoulder_turn(weapon: Option<ActorPart>, action: CombatPose) -> f32 {
    match (weapon, action) {
        (Some(ActorPart::Shotgun | ActorPart::RocketLauncher), _) => -0.36,
        (Some(ActorPart::Axe), CombatPose::Swing(t)) => axe_swing(t, 0.0).2 * 0.45,
        _ => 0.0,
    }
}

pub(super) fn weapon_stance(
    part: ActorPart,
    action: CombatPose,
    aim: Quat,
    pitch: f32,
    gait: f32,
    interaction: InteractionPose,
    arms: &mut [ArmPose; 2],
) -> Option<Mat4> {
    let lower = interaction.amount;
    let right = ArmSide::Right;
    let left = ArmSide::Left;
    let (scale, rotation) = if part == ActorPart::Axe {
        let swing = match action {
            CombatPose::Swing(t) => t,
            _ => 0.0,
        };
        let (position, rotation, _) = axe_swing(swing, gait);
        let position = position.lerp(Vec3::new(-0.154, -0.025, 0.098), lower);
        let rotation = rotation.slerp(Quat::from_rotation_x(0.15), lower);
        let hand_rotation = rotation * Quat::from_rotation_x(-0.76);
        arms[1] = solve_arm(right, position, hand_rotation, right.pole());
        (1.0, rotation)
    } else {
        let (model, grip, scale, recoil_pitch, recoil_travel) = match part {
            ActorPart::Pistol => (
                WeaponModel::Pistol,
                Vec3::new(-0.035, 0.160, 0.239),
                1.0,
                0.16,
                0.025,
            ),
            ActorPart::Shotgun => (
                WeaponModel::Shotgun,
                Vec3::new(-0.087, 0.169, 0.150),
                0.85,
                0.10,
                0.036,
            ),
            ActorPart::RocketLauncher => (
                WeaponModel::RocketLauncher,
                Vec3::new(-0.125, 0.193, 0.100),
                0.9,
                0.035,
                0.016,
            ),
            _ => return None,
        };
        let kick = match action {
            CombatPose::Recoil(amount) => amount * (1.0 - lower),
            _ => 0.0,
        };
        let mut grip =
            aim * (grip + Vec3::new(0.0, -lower * 0.10, -kick * recoil_travel - lower * 0.063));
        // The mesh's +Z is its bore, and +Y is its sight side. Give the weapon
        // its own aim frame so forearm or torso rotation cannot invert it.
        let rotation = aim
            * Quat::from_rotation_x(-pitch * (1.0 - lower) - kick * recoil_pitch + lower * 0.48);
        let right_hand = rotation * Quat::from_rotation_x(-1.19);
        let left_hand = rotation * Quat::from_rotation_y(-0.28) * Quat::from_rotation_x(-1.36);
        let support = rotation * (support_socket(model) * scale + Vec3::X * 0.021);
        grip = left.limit_grip(grip + support, left_hand) - support;
        grip = right.limit_grip(grip, right_hand);
        arms[1] = solve_arm(right, grip, right_hand, right.pole());
        arms[0] = solve_arm(left, grip + support, left_hand, left.pole());
        (scale, rotation)
    };

    // Match the actual wrist chain at the edge of its reach, then allow the
    // weapon to move onto the shoulder while both hands are on the window sill.
    let grip = arms[1].grip(right);
    let stow = interaction.hands[1].map_or(0.0, |(_, weight)| weight);
    let (stowed_grip, stowed_rotation) = if part == ActorPart::Axe {
        (Vec3::new(-0.14, 0.06, -0.14), Quat::from_rotation_z(0.26))
    } else {
        (
            Vec3::new(-0.15, 0.089, -0.15),
            Quat::from_rotation_z(-0.18) * Quat::from_rotation_x(-1.46),
        )
    };
    Some(Mat4::from_scale_rotation_translation(
        Vec3::splat(scale),
        rotation.slerp(stowed_rotation, stow),
        grip.lerp(stowed_grip, stow),
    ))
}

fn support_socket(model: WeaponModel) -> Vec3 {
    let nodes = weapon_nodes(model);
    let mut name = "socket_grip_L";
    let mut position = Vec3::ZERO;
    while let Some(node) = nodes.iter().find(|node| node.name == name) {
        position += Vec3::from_array(node.translation);
        match node.parent {
            Some(parent) => name = parent,
            None => break,
        }
    }
    position
}

fn axe_swing(t: f32, gait: f32) -> (Vec3, Quat, f32) {
    let carry = (
        Vec3::new(-0.143, -0.016, 0.101 + gait * 0.075),
        Quat::from_rotation_z(0.22) * Quat::from_rotation_x(0.22 + gait * 0.16),
        0.0,
    );
    let windup = (
        Vec3::new(-0.20, 0.19, 0.037),
        Quat::from_rotation_y(-0.50) * Quat::from_rotation_x(-0.60),
        -0.50,
    );
    let strike = (
        Vec3::new(-0.045, 0.104, 0.250),
        Quat::from_rotation_y(0.32) * Quat::from_rotation_x(1.43),
        0.32,
    );
    let follow_through = (
        Vec3::new(0.012, -0.055, 0.160),
        Quat::from_rotation_y(0.62) * Quat::from_rotation_x(1.69),
        0.62,
    );
    let (from, to, alpha) = if t < 0.27 {
        (carry, windup, t / 0.27)
    } else if t < 0.57 {
        (windup, strike, (t - 0.27) / 0.30)
    } else if t < 0.77 {
        (strike, follow_through, (t - 0.57) / 0.20)
    } else {
        (follow_through, carry, (t - 0.77) / 0.23)
    };
    let alpha = alpha * alpha * (3.0 - 2.0 * alpha);
    (
        from.0.lerp(to.0, alpha),
        from.1.slerp(to.1, alpha),
        from.2 + (to.2 - from.2) * alpha,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_rig::ready_arms;

    #[test]
    fn firearm_meshes_remain_upright_on_aim_and_both_hands_reach_their_grips() {
        for (part, model) in [
            (ActorPart::Pistol, WeaponModel::Pistol),
            (ActorPart::Shotgun, WeaponModel::Shotgun),
            (ActorPart::RocketLauncher, WeaponModel::RocketLauncher),
        ] {
            for pitch in [-0.5, 0.0, 0.5] {
                let aim = Quat::from_rotation_y(0.39);
                let mut arms = ready_arms(0.0, CombatPose::Ready);
                let weapon = weapon_stance(
                    part,
                    CombatPose::Ready,
                    aim,
                    pitch,
                    0.0,
                    InteractionPose::default(),
                    &mut arms,
                )
                .unwrap();
                let aimed = aim * Quat::from_rotation_x(-pitch);
                let forward = weapon.transform_vector3(Vec3::Z).normalize();
                let up = weapon.transform_vector3(Vec3::Y).normalize();
                assert!(forward.distance(aimed * Vec3::Z) < 1e-5);
                assert!(up.distance(aimed * Vec3::Y) < 1e-5);
                assert!(
                    arms[1]
                        .grip(ArmSide::Right)
                        .distance(weapon.w_axis.truncate())
                        < 1e-5
                );
                let left_grip =
                    weapon.transform_point3(support_socket(model)) + aimed * (Vec3::X * 0.021);
                assert!(
                    arms[0].grip(ArmSide::Left).distance(left_grip) < 1e-4,
                    "{part:?} {pitch}"
                );
            }
        }
    }
}
