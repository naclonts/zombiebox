use crate::coords::sim_to_render_point;
use crate::mesh::doors::DOOR_REACH_SECONDS;
use crate::player_rig::InteractionPose;
use glam::{Quat, Vec3, Vec3Swizzles};
use std::f32::consts::{PI, TAU};
use zombiebox_core::{Tile3D, Vec3 as SimVec3, WINDOW_SILL_HEIGHT, WallOrientation};

pub(crate) fn smooth(start: f32, end: f32, x: f32) -> f32 {
    let t = ((x - start) / (end - start)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(crate) fn turn_toward(from: f32, to: f32, amount: f32) -> f32 {
    from + ((to - from + PI).rem_euclid(TAU) - PI) * amount
}

#[derive(Clone, Copy)]
pub(crate) struct DoorReach {
    pub tile: Tile3D,
    pub center: Vec3,
    pub elapsed: f32,
}

impl DoorReach {
    pub fn pose(self, position: Vec3, angle: f32, handle: Vec3) -> (Vec3, f32, InteractionPose) {
        let weight = smooth(0.0, 0.095, self.elapsed)
            * (1.0 - smooth(0.27, DOOR_REACH_SECONDS, self.elapsed));
        let toward = handle - position;
        let horizontal = Vec3::new(toward.x, 0.0, toward.z);
        let distance = horizontal.length();
        let toward = horizontal.normalize_or_zero();
        let angle = turn_toward(angle, toward.x.atan2(toward.z) - 0.24, weight);
        let position = position + toward * (distance - 0.35).clamp(0.0, 0.19) * weight;
        let target = Quat::from_rotation_y(-angle) * (handle - position);
        (
            position,
            angle,
            InteractionPose {
                hands: [Some((target, weight)), None],
                amount: weight,
                lean: 0.33 * weight,
                ..InteractionPose::default()
            },
        )
    }
}

const VAULT_SECONDS: f32 = 0.56;
const WALK_VAULT_RANGE: f32 = 0.56;

#[derive(Clone, Copy)]
pub(crate) struct WindowVault {
    pub tile: Tile3D,
    from: Vec3,
    to: Vec3,
    elapsed: f32,
}

impl WindowVault {
    pub fn new(tile: Tile3D, from: SimVec3, to: SimVec3) -> Self {
        Self {
            tile,
            from: sim_to_render_point(from),
            to: sim_to_render_point(to),
            elapsed: 0.0,
        }
    }

    pub fn is_timed(self) -> bool {
        // The interaction button moves to the tile across the opening. Walk-through
        // events instead record the small displacement of one simulation tick.
        self.from.xz().distance_squared(self.to.xz()) > 0.25
    }

    pub fn advance(&mut self, dt: f32) -> bool {
        self.elapsed += dt;
        !self.is_timed() || self.elapsed < VAULT_SECONDS
    }

    pub fn pose(
        self,
        position: Vec3,
        angle: f32,
        opening: Option<(WallOrientation, f32)>,
    ) -> Option<(Vec3, f32, InteractionPose)> {
        let travel = self.to - self.from;
        let (orientation, base) = opening.unwrap_or((
            if travel.x.abs() > travel.z.abs() {
                WallOrientation::AlongY
            } else {
                WallOrientation::AlongX
            },
            self.from.y.min(self.to.y),
        ));
        let center = Vec3::new(self.tile.x as f32, base, self.tile.y as f32);
        let normal = if orientation == WallOrientation::AlongY {
            Vec3::X
        } else {
            Vec3::Z
        };
        let forward = if (center - self.from).dot(normal) < 0.0 {
            -normal
        } else {
            normal
        };
        let facing = forward.x.atan2(forward.z);
        let side = Vec3::Y.cross(forward);
        let offset = position - center;
        let (t, mut root) = if self.is_timed() {
            let t = (self.elapsed / VAULT_SECONDS).clamp(0.0, 1.0);
            let progress = smooth(0.0, 1.0, t);
            let mut root = self.from.lerp(self.to, progress);
            let midpoint_depth = (self.from.lerp(self.to, 0.5) - center).dot(forward);
            root -= forward * (4.0 * progress * (1.0 - progress) * midpoint_depth);
            // Feed the survivor through the middle of the opening even if the
            // interaction started off to one side.
            let lateral = (root - center).dot(side);
            root -= side * lateral * (smooth(0.08, 0.36, t) - smooth(0.69, 0.94, t));
            root += (position - self.to) * smooth(0.70, 1.0, t);
            (t, root)
        } else {
            let depth = offset.dot(forward);
            if depth.abs() > WALK_VAULT_RANGE + 0.09 || offset.dot(side).abs() > 0.58 {
                return None;
            }
            let t = (0.5 + depth / (2.0 * WALK_VAULT_RANGE)).clamp(0.0, 1.0);
            (t, position)
        };

        let amount = smooth(0.06, 0.39, t) * (1.0 - smooth(0.72, 0.96, t));
        let angle = turn_toward(angle, facing, smooth(0.0, 0.27, t) - smooth(0.77, 1.0, t));
        let lift = 0.30 * smooth(0.15, 0.47, t) * (1.0 - smooth(0.65, 0.93, t));
        root.y += lift;
        let local = Quat::from_rotation_y(-angle);
        let sill = center + Vec3::Y * (WINDOW_SILL_HEIGHT + 0.035);
        let hand_weight = smooth(0.08, 0.31, t) * (1.0 - smooth(0.49, 0.71, t));
        let palm = |side_offset: f32| local * (sill + side * side_offset - forward * 0.045 - root);
        let foot = |x: f32, plant: f32, crossing: f32, land: f32| {
            let stride = smooth(plant, land, t);
            let tuck = smooth(plant, crossing, t) * (1.0 - smooth(crossing, land, t));
            let sole = 0.060 + (0.43 - lift) * tuck;
            Vec3::new(
                x,
                (sole - lift * (1.0 - tuck)).max(0.025),
                -0.15 * amount + 0.33 * stride * (1.0 - smooth(0.77, 0.96, t)),
            )
        };
        Some((
            root,
            angle,
            InteractionPose {
                hands: if hand_weight > 0.0 {
                    [
                        Some((palm(0.19), hand_weight)),
                        Some((palm(-0.19), hand_weight)),
                    ]
                } else {
                    [None, None]
                },
                ankles: (amount > 0.0).then(|| {
                    [
                        foot(0.088, 0.15, 0.46, 0.72),
                        foot(-0.088, 0.30, 0.61, 0.88),
                    ]
                }),
                amount,
                lean: 1.19 * amount,
                pelvis_drop: 0.16 * amount,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::render_to_sim_point;

    #[test]
    fn walking_vault_uses_wall_depth_in_both_directions_and_stops_outside_the_window() {
        let tile = Tile3D::new(0, 0, 0);
        for direction in [-1.0, 1.0] {
            let start = Vec3::X * (-0.51 * direction);
            let vault = WindowVault::new(
                tile,
                render_to_sim_point(start),
                render_to_sim_point(start + Vec3::X * (0.04 * direction)),
            );
            let opening = Some((WallOrientation::AlongY, 0.0));
            let (takeoff, _, _) = vault.pose(start, 0.0, opening).unwrap();
            assert_eq!(takeoff, start);
            let (middle, facing, pose) = vault.pose(Vec3::ZERO, 0.0, opening).unwrap();
            assert!(middle.y > 0.25);
            assert!((facing.sin() - direction).abs() < 1e-5);
            assert!(pose.hands.iter().all(Option::is_some));
            let [lead, trail] = pose.ankles.unwrap();
            assert!(lead.y > trail.y);
            assert!(vault.pose(Vec3::X * direction, 0.0, opening).is_none());
        }
    }
}
