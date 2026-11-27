//! Articulated motorcycle render transforms shared by the scene renderer.
use glam::{Mat4, Quat, Vec3};
use zombiebox_core::{MotorcyclePhase, MotorcycleView};

use super::motorcycle::{MotorcyclePaint, MotorcyclePart, motorcycle_part_pivot};
use crate::coords::{ground_align_quat, heading_to_render_quat};

pub const MOTORCYCLE_SCALE: f32 = 0.60;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotorcycleTransforms {
    pub body: Mat4,
    pub steering: Mat4,
    pub front_wheel: Mat4,
    pub rear_wheel: Mat4,
}

impl MotorcycleTransforms {
    #[must_use]
    pub fn from_view(view: MotorcycleView, wheel_spin: f32) -> Self {
        let b = view.body;
        let pos = Vec3::new(b.position.x, b.position.z, b.position.y);
        // Pack forward is +Z; renderer heading orientation is +X.
        let upright =
            heading_to_render_quat(b.heading) * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let lean = match view.phase {
            MotorcyclePhase::Tipped => {
                if view.rider_lean.abs() > 0.01 {
                    view.rider_lean.signum() * 1.2
                } else {
                    1.2
                }
            }
            MotorcyclePhase::Skidding => view.rider_lean.clamp(-0.65, 0.65),
            _ => view.rider_lean.clamp(-0.5, 0.5),
        };
        let root = Mat4::from_translation(pos)
            * Mat4::from_quat(
                ground_align_quat(view.ground_normal) * upright * Quat::from_rotation_z(lean),
            )
            * Mat4::from_scale(Vec3::splat(MOTORCYCLE_SCALE));
        let s = motorcycle_part_pivot(MotorcyclePart::Steering);
        let f = motorcycle_part_pivot(MotorcyclePart::FrontWheel);
        let r = motorcycle_part_pivot(MotorcyclePart::RearWheel);
        let steer_axis = Vec3::new(0.0, 0.874791, -0.484500).normalize();
        let steer = Quat::from_axis_angle(steer_axis, -view.steer_angle.clamp(-0.8, 0.8));
        let spin = Quat::from_rotation_x(if wheel_spin.is_finite() {
            wheel_spin
        } else {
            0.0
        });
        let steering = root * Mat4::from_translation(s) * Mat4::from_quat(steer);
        Self {
            body: root,
            steering,
            front_wheel: steering * Mat4::from_translation(f - s) * Mat4::from_quat(spin),
            rear_wheel: root * Mat4::from_translation(r) * Mat4::from_quat(spin),
        }
    }

    #[must_use]
    pub fn part_transform(self, part: MotorcyclePart) -> Mat4 {
        match part {
            MotorcyclePart::Body => self.body,
            MotorcyclePart::Steering => self.steering,
            MotorcyclePart::FrontWheel => self.front_wheel,
            MotorcyclePart::RearWheel => self.rear_wheel,
        }
    }
}

pub const fn default_motorcycle_paint() -> MotorcyclePaint {
    MotorcyclePaint::GreenBlack
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::motorcycle::{MotorcycleLocator, motorcycle_locator, motorcycle_mesh};
    use zombiebox_core::spatial::BodyState;

    fn sample(heading: f32) -> MotorcycleView {
        MotorcycleView {
            body: BodyState {
                position: zombiebox_core::Vec3::new(12.0, 8.0, 0.0),
                heading,
                ..BodyState::default()
            },
            phase: MotorcyclePhase::Rolling,
            steer_angle: 0.3,
            rider_lean: 0.2,
            speed: 4.0,
            condition: 100,
            recovery_ticks: 0,
            ground_normal: zombiebox_core::Vec3::Z,
        }
    }

    #[test]
    fn generated_mesh_and_parts_validate() {
        use super::super::motorcycle::{MotorcyclePart, motorcycle_part_mesh};
        for part in [
            MotorcyclePart::Body,
            MotorcyclePart::Steering,
            MotorcyclePart::FrontWheel,
            MotorcyclePart::RearWheel,
        ] {
            motorcycle_part_mesh(part, MotorcyclePaint::GreenBlack)
                .validate()
                .unwrap();
        }
        motorcycle_mesh(MotorcyclePaint::GreenBlack)
            .validate()
            .unwrap();
    }

    #[test]
    fn mounted_parts_follow_cardinal_heading_and_pivots() {
        let local_forward = Vec3::Z;
        for (heading, expected) in [
            (0.0, Vec3::X),
            (std::f32::consts::FRAC_PI_2, Vec3::Z),
            (std::f32::consts::PI, Vec3::NEG_X),
            (-std::f32::consts::FRAC_PI_2, Vec3::NEG_Z),
        ] {
            let t = MotorcycleTransforms::from_view(sample(heading), 1.0);
            let forward = t.body.transform_vector3(local_forward).normalize();
            assert!(forward.dot(expected) > 0.99, "{heading}: {forward:?}");
            let front_pivot = t.front_wheel.transform_point3(Vec3::ZERO);
            let t2 = MotorcycleTransforms::from_view(sample(heading), 2.0);
            assert!(front_pivot.distance(t2.front_wheel.transform_point3(Vec3::ZERO)) < 1e-4);
        }
    }

    #[test]
    fn grounded_wheels_follow_terrain_slope() {
        let mut view = sample(0.0);
        let sim_normal = zombiebox_core::Vec3::new(-0.2, 0.0, 1.0).normalized();
        view.ground_normal = sim_normal;
        let t = MotorcycleTransforms::from_view(view, 0.0);
        let origin = t.body.transform_point3(Vec3::ZERO);
        let front = t.body.transform_point3(Vec3::new(0.0, 0.0, 0.835));
        let rear = t.body.transform_point3(Vec3::new(0.0, 0.0, -0.835));
        let render_normal = Vec3::new(sim_normal.x, sim_normal.z, sim_normal.y);
        assert!((front - origin).dot(render_normal).abs() < 1e-4);
        assert!((rear - origin).dot(render_normal).abs() < 1e-4);
        assert!(front.y > rear.y, "uphill front wheel should sit higher");

        let flat = MotorcycleTransforms::from_view(sample(0.0), 0.0);
        let flat_front = flat.body.transform_point3(Vec3::new(0.0, 0.0, 0.835));
        let flat_rear = flat.body.transform_point3(Vec3::new(0.0, 0.0, -0.835));
        assert!((flat_front.y - flat_rear.y).abs() < 1e-4);
    }

    #[test]
    fn grip_locators_follow_steering_pivot() {
        let t0 = MotorcycleTransforms::from_view(sample(0.0), 0.0);
        let mut changed = sample(0.0);
        changed.steer_angle = -0.3;
        let t1 = MotorcycleTransforms::from_view(changed, 0.0);
        let (part, local) = motorcycle_locator(MotorcycleLocator::GripRight);
        assert_eq!(part, MotorcyclePart::Steering);
        let before = t0.steering.transform_point3(local * MOTORCYCLE_SCALE);
        let after = t1.steering.transform_point3(local * MOTORCYCLE_SCALE);
        assert!(before.distance(after) > 0.01);
    }
}
