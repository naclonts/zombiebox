//! Fixed-tick motorcycle handling prototype.
//!
//! Slice one keeps the model intentionally small: throttle, brake, drag,
//! steering, rider lean tracking, and continuous support/collision response.
//! Grip loss, impacts, wipeouts, and rider recovery extend this module in the
//! next slice.

use std::f32::consts::TAU;

use crate::spatial::{
    Aabb3, BodyShape, BodyState, MoveParams, SpatialWorld, Vec2, Vec3,
    move_kinematic_with_horizontal_vel,
};

use super::MotorcyclePhase;

/// World units per second converted to displayed miles per hour.
pub(super) const MPH_PER_UNIT_PER_SECOND: f32 = 2.24;

/// Conservative bike footprint used by the shared kinematic solver.
pub(crate) const MOTORCYCLE_SHAPE: BodyShape = BodyShape::new(0.36, 0.92);

/// Speed below which a bike counts as stopped for phase changes and a clean dismount.
pub(crate) const MOTORCYCLE_STOP_SPEED: f32 = 0.28;

/// Sanitized rider controls. Values are finite and clamped by [`sanitize`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MotorcycleInput {
    /// Handlebar steer in `[-1, 1]`, positive is rider-right.
    pub steer: f32,
    /// Throttle in `[0, 1]`.
    pub throttle: f32,
    /// Brake in `[0, 1]`. Holding it at rest pushes the bike backward at walking pace.
    pub brake: f32,
    /// Rider weight shift in `[-1, 1]`, positive is rider-right.
    pub lean: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotorcycleCrashKind {
    LowSpeedTip,
    HighSpeedEjection,
}

impl MotorcycleInput {
    pub(crate) fn sanitized(self) -> Self {
        fn unit(v: f32) -> f32 {
            if v.is_finite() {
                v.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        }
        fn positive(v: f32) -> f32 {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            }
        }
        Self {
            steer: unit(self.steer),
            throttle: positive(self.throttle),
            brake: positive(self.brake),
            lean: unit(self.lean),
        }
    }
}

/// Tunable handling curves for the prototype cruiser.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MotorcycleTuning {
    pub max_speed: f32,
    pub throttle_accel: f32,
    pub brake_decel: f32,
    pub reverse_accel: f32,
    pub reverse_max_speed: f32,
    pub rolling_resistance: f32,
    pub aero_drag: f32,
    pub wheelbase: f32,
    pub low_speed_steer: f32,
    pub high_speed_steer: f32,
    pub steer_response: f32,
    pub lean_response: f32,
    pub max_lean: f32,
    pub support_step_up: f32,
    pub support_step_down: f32,
    pub max_slope: f32,
}

impl Default for MotorcycleTuning {
    fn default() -> Self {
        Self {
            // ~40.1 mph at the top of the prototype range.
            max_speed: 17.9,
            throttle_accel: 7.2,
            brake_decel: 14.0,
            // A walking-pace push backward; the rider's legs provide this instead of the engine.
            reverse_accel: 2.0,
            reverse_max_speed: 0.92,
            rolling_resistance: 0.55,
            aero_drag: 0.055,
            wheelbase: 1.50,
            low_speed_steer: 0.62,
            high_speed_steer: 0.13,
            steer_response: 6.5,
            lean_response: 5.5,
            max_lean: 0.62,
            support_step_up: 0.28,
            support_step_down: 0.42,
            max_slope: 0.80,
        }
    }
}

/// Read-only infected candidate considered for a bike impact.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VehicleContactBody {
    pub id: u64,
    pub bounds: Aabb3,
}

/// Contact report produced by one fixed motorcycle tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct MotorcycleStep {
    pub contact_target: Option<u64>,
    pub contact_point: Vec3,
    pub contact_normal: Vec3,
    pub pre_impact_speed: f32,
    pub wipeout: Option<MotorcycleWipeout>,
    pub impact: bool,
}

#[allow(dead_code)] // populated by the crash/recovery slice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotorcycleWipeout {
    LowSpeedTip,
    HighSpeedEjection,
}

impl From<MotorcycleWipeout> for MotorcycleCrashKind {
    fn from(value: MotorcycleWipeout) -> Self {
        match value {
            MotorcycleWipeout::LowSpeedTip => Self::LowSpeedTip,
            MotorcycleWipeout::HighSpeedEjection => Self::HighSpeedEjection,
        }
    }
}

/// Authoritative per-bike simulation state.
pub(crate) struct MotorcycleState {
    pub body: BodyState,
    pub phase: MotorcyclePhase,
    /// `0..=100`; zero disables powered motion.
    pub condition: u8,
    pub steer_angle: f32,
    pub rider_lean: f32,
    pub traction_use: f32,
    last_throttle: f32,
    wheel_travel: f32,
    last_raw_steer: f32,
    overload_ticks: u16,
    tip_ticks: u16,
    pub recovery_ticks: u16,
    pending_crash_damage: u8,
    contact_cooldown: u8,
    contact_latched: bool,
}

impl MotorcycleState {
    pub(crate) fn parked(body: BodyState) -> Self {
        Self {
            body,
            phase: MotorcyclePhase::Parked,
            condition: 100,
            steer_angle: 0.0,
            rider_lean: 0.0,
            traction_use: 0.0,
            last_throttle: 0.0,
            wheel_travel: 0.0,
            last_raw_steer: 0.0,
            overload_ticks: 0,
            tip_ticks: 0,
            recovery_ticks: 0,
            pending_crash_damage: 0,
            contact_cooldown: 0,
            contact_latched: false,
        }
    }

    #[inline]
    pub(crate) fn speed(&self) -> f32 {
        self.body.velocity.xy().length()
    }

    #[inline]
    pub(crate) fn is_operable(&self) -> bool {
        self.condition > 0
    }

    #[inline]
    pub(crate) fn is_stopped(&self) -> bool {
        self.speed() <= MOTORCYCLE_STOP_SPEED
    }

    pub(crate) fn speed_mph_x10(&self) -> u16 {
        (self.speed() * MPH_PER_UNIT_PER_SECOND * 10.0)
            .round()
            .clamp(0.0, u16::MAX as f32) as u16
    }

    pub(crate) fn engine_load(&self) -> u8 {
        (self.last_throttle * 100.0).round().clamp(0.0, 100.0) as u8
    }

    pub(crate) fn set_parked(&mut self) {
        self.body.velocity = Vec3::ZERO;
        self.phase = MotorcyclePhase::Parked;
        self.steer_angle = 0.0;
        self.rider_lean = 0.0;
        self.traction_use = 0.0;
        self.last_throttle = 0.0;
        self.overload_ticks = 0;
        self.tip_ticks = 0;
    }

    /// Takes over a bike that may already be rolling or skidding.
    ///
    /// Momentum is deliberately preserved so a survivor can catch a moving
    /// motorcycle; only the unused rider inputs and crash timers are cleared.
    pub(crate) fn begin_ride(&mut self) {
        self.steer_angle = 0.0;
        self.rider_lean = 0.0;
        self.traction_use = 0.0;
        self.last_throttle = 0.0;
        self.last_raw_steer = 0.0;
        self.overload_ticks = 0;
        self.tip_ticks = 0;
        self.recovery_ticks = 0;
        self.pending_crash_damage = 0;
        self.contact_cooldown = 0;
        self.contact_latched = false;
        self.body.pitch = 0.0;
        self.body.roll = 0.0;
        self.phase = if self.is_stopped() {
            MotorcyclePhase::Parked
        } else {
            MotorcyclePhase::Rolling
        };
    }

    /// Releases the rider while leaving the bike's momentum intact.
    pub(crate) fn release_rider(&mut self) {
        self.begin_ride();
    }
}

/// Advances one motorcycle by one fixed `DT` step.
///
/// `input == None` advances an unmanned coast/skid; a mounted bike must pass
/// sanitized neutral input instead when the client sample is missing.
pub(crate) fn step_motorcycle(
    bike: &mut MotorcycleState,
    input: Option<MotorcycleInput>,
    world: &dyn SpatialWorld,
    contacts: &[VehicleContactBody],
    tuning: &MotorcycleTuning,
) -> MotorcycleStep {
    let mounted = input.is_some();
    let input = input.unwrap_or_default().sanitized();
    let dt = crate::action::DT;
    let pre_impact_speed = bike.speed();
    let condition_before = bike.condition;
    let travel_before = bike.body.position;
    if bike.recovery_ticks > 0 {
        bike.recovery_ticks -= 1;
    }
    if bike.contact_cooldown > 0 {
        bike.contact_cooldown -= 1;
    }

    let speed = bike.speed();
    let speed_t = (speed / tuning.max_speed).clamp(0.0, 1.0);
    let steer_limit =
        tuning.low_speed_steer + (tuning.high_speed_steer - tuning.low_speed_steer) * speed_t;
    let target_steer = if bike.is_operable() && bike.recovery_ticks == 0 {
        input.steer * steer_limit
    } else {
        0.0
    };
    bike.steer_angle = approach(bike.steer_angle, target_steer, tuning.steer_response * dt);

    let target_lean = input.lean * tuning.max_lean;
    bike.rider_lean = approach(bike.rider_lean, target_lean, tuning.lean_response * dt);
    bike.last_throttle = if bike.is_operable() {
        input.throttle
    } else {
        0.0
    };

    let mut velocity = bike.body.velocity.xy();
    let heading = bike.body.facing_xy();

    // Slope gravity effect: on Seattle-level hills, gravity acts along the terrain gradient.
    const SLOPE_GRAVITY: f32 = 4.5;
    let slope_normal = world
        .terrain_at(bike.body.position.xy())
        .map_or(Vec3::Z, |t| t.normal);
    let slope_accel = if bike.body.motion.is_grounded() && slope_normal.z > 0.01 {
        Vec2::new(slope_normal.x, slope_normal.y) * (SLOPE_GRAVITY * slope_normal.z)
    } else {
        Vec2::ZERO
    };

    if bike.is_operable() && bike.recovery_ticks == 0 {
        if input.throttle > 0.0 {
            velocity += heading * (tuning.throttle_accel * input.throttle * dt);
        }
        if input.brake > 0.0 {
            let forward_speed = velocity.dot(heading);
            let at_rest = velocity.length() <= MOTORCYCLE_STOP_SPEED;
            if input.throttle <= 0.001 && (forward_speed < 0.0 || at_rest) {
                // Once stopped, holding brake becomes a walking-pace leg push.
                velocity -= heading * (tuning.reverse_accel * input.brake * dt);
            } else {
                velocity = decelerate(velocity, tuning.brake_decel * input.brake * dt);
            }
        }
    }

    // Apply slope gravity when moving or rolling unbraked.
    let holding_brake = input.brake > 0.0 && velocity.length() <= MOTORCYCLE_STOP_SPEED;
    if bike.phase != MotorcyclePhase::Parked && !holding_brake {
        velocity += slope_accel * dt;
    }

    // Forward drive and leg-powered reverse have independent speed caps.
    // Downhill momentum allows a modest speed boost beyond nominal engine cap.
    let forward_speed = velocity.dot(heading);
    let downhill_component = heading.dot(slope_accel).max(0.0);
    let effective_max_speed = tuning.max_speed + downhill_component * 0.35;
    if forward_speed > effective_max_speed {
        velocity -= heading * (forward_speed - effective_max_speed);
    } else if forward_speed < -tuning.reverse_max_speed {
        velocity -= heading * (forward_speed + tuning.reverse_max_speed);
    }
    let mut current_speed = velocity.length();
    if current_speed > tuning.max_speed {
        velocity *= tuning.max_speed / current_speed;
        current_speed = tuning.max_speed;
    }
    let drag = (tuning.aero_drag * current_speed + tuning.rolling_resistance) * dt;
    velocity = decelerate(velocity, drag);
    current_speed = velocity.length();
    if current_speed <= 0.001 {
        velocity = Vec2::ZERO;
    }

    // Yaw follows forward motion.
    // Low-speed regime (~0-8 mph): Handlebars drive yaw rate via standard bicycle kinematics.
    // High-speed regime (~15+ mph): Lean angle is primary steering driver (psi_dot = g * tan(theta) / v).
    // Transition: Smoothstep blend factor between low- and high-speed thresholds.
    const GRAVITY: f32 = 9.81;
    const LOW_SPEED_MPH: f32 = 8.0;
    const HIGH_SPEED_MPH: f32 = 15.0;

    let speed_mph = current_speed * MPH_PER_UNIT_PER_SECOND;
    let forward_speed = velocity.dot(heading);
    let yaw_rate = if current_speed > 0.02 {
        let yaw_rate_steer = forward_speed * bike.steer_angle.tan() / tuning.wheelbase;
        let v_safe = current_speed.max(0.1);
        let yaw_rate_lean = (GRAVITY * bike.rider_lean.tan() / v_safe).clamp(-1.8, 1.8);

        let t = ((speed_mph - LOW_SPEED_MPH) / (HIGH_SPEED_MPH - LOW_SPEED_MPH)).clamp(0.0, 1.0);
        let blend = t * t * (3.0 - 2.0 * t);
        if forward_speed < 0.0 {
            // Reversing steers from the handlebars; leaning cannot balance a
            // backward bicycle-style maneuver at walking pace.
            yaw_rate_steer
        } else {
            (1.0 - blend) * yaw_rate_steer + blend * yaw_rate_lean
        }
    } else {
        0.0
    };
    if current_speed > 0.02 {
        bike.body.heading = wrap_angle(bike.body.heading + yaw_rate * dt);
        let heading = bike.body.facing_xy();
        let right = Vec2::new(-heading.y, heading.x);
        let lateral = velocity.dot(right);
        let demand = (current_speed * yaw_rate.abs() / 8.0).clamp(0.0, 1.5);
        let target_lean =
            (yaw_rate.signum() * demand * 0.68).clamp(-tuning.max_lean, tuning.max_lean);
        let mismatch = (target_lean - bike.rider_lean).abs() / tuning.max_lean.max(0.01);
        bike.traction_use = (demand * (0.55 + mismatch * 0.65)).clamp(0.0, 1.0);
        // Lateral velocity follows the turn while grip is available; excess demand
        // leaves a readable, persistent slip instead of snapping the bike sideways.
        let grip = (1.0 - bike.traction_use * 0.78).clamp(0.18, 1.0);
        velocity -= right * (lateral * (1.0 - (-grip * 9.0 * dt).exp()));
        if bike.traction_use > 0.92 {
            bike.overload_ticks = bike.overload_ticks.saturating_add(1);
        } else {
            bike.overload_ticks = bike.overload_ticks.saturating_sub(2);
        }
    }
    // Fast yank ejection: the only handlebar-steering wipeout. A sudden snap of
    // the bars (most of the input range crossed in one fixed tick, e.g. digital
    // input slamming to full lock) above `EJECT_YANK_MPH` throws the rider.
    // Gradual steering and leaning stay recoverable at any road speed, however
    // much of the speed-scaled lock is used.
    const STEER_YANK_DELTA: f32 = 0.85;
    const EJECT_YANK_MPH: f32 = 35.0;
    let raw_steer = input.steer;
    let steer_yank = (raw_steer - bike.last_raw_steer).abs() > STEER_YANK_DELTA;
    bike.last_raw_steer = raw_steer;

    let high_speed_crash =
        steer_yank && speed_mph > EJECT_YANK_MPH && bike.is_operable() && bike.recovery_ticks == 0;

    if high_speed_crash {
        bike.phase = MotorcyclePhase::Skidding;
        bike.condition = bike.condition.saturating_sub(22);
        bike.pending_crash_damage = 2;
        bike.recovery_ticks = 110;
        bike.body.roll = bike.rider_lean;
        bike.last_throttle = 0.0;
        return MotorcycleStep {
            contact_target: None,
            contact_point: bike.body.position,
            contact_normal: Vec3::Z,
            pre_impact_speed,
            wipeout: Some(MotorcycleWipeout::HighSpeedEjection),
            impact: false,
        };
    }

    // Low-speed regime (~0-8 mph): Track speed-dependent maximum safe lean angle.
    // At near-zero speed, any significant sustained lean causes a tip.
    // At ~2 mph, safe lean is small; by ~8 mph centripetal force balances full max lean.
    let max_safe_lean = if speed_mph >= LOW_SPEED_MPH {
        tuning.max_lean + 0.1
    } else {
        let t_lean = (speed_mph / LOW_SPEED_MPH).clamp(0.0, 1.0);
        0.15 + (tuning.max_lean - 0.15) * t_lean
    };

    let unsafe_lean = (bike.rider_lean.abs() > max_safe_lean
        || (speed_mph < 2.5 && input.lean.abs() > 0.6))
        && bike.is_operable();

    if unsafe_lean {
        bike.tip_ticks = bike.tip_ticks.saturating_add(1);
    } else {
        bike.tip_ticks = bike.tip_ticks.saturating_sub(1);
    }
    if bike.tip_ticks >= 9
        && matches!(
            bike.phase,
            MotorcyclePhase::Parked | MotorcyclePhase::Rolling
        )
    {
        bike.phase = MotorcyclePhase::Tipped;
        bike.condition = bike.condition.saturating_sub(5);
        bike.recovery_ticks = 45;
        bike.pending_crash_damage = 1;
        bike.body.velocity = velocity.extend(0.0);
        bike.body.roll = if bike.rider_lean.abs() > 0.05 {
            bike.rider_lean.signum() * 1.2
        } else {
            1.2
        };
        return MotorcycleStep {
            contact_target: None,
            contact_point: bike.body.position,
            contact_normal: Vec3::Z,
            pre_impact_speed,
            wipeout: Some(MotorcycleWipeout::LowSpeedTip),
            impact: false,
        };
    }
    bike.body.velocity = velocity.extend(0.0);
    bike.body.roll = bike.rider_lean;
    bike.body.pitch = 0.0;
    if current_speed <= 0.02 {
        bike.traction_use = 0.0;
    }

    let params = MoveParams {
        max_step_up: tuning.support_step_up,
        max_step_down: tuning.support_step_down,
        max_walkable_slope: tuning.max_slope,
        can_vault_broken_windows: false,
        avoid_water: true,
        doorway_assist: false,
        ..MoveParams::default()
    };
    let moved = move_kinematic_with_horizontal_vel(
        world,
        MOTORCYCLE_SHAPE,
        bike.body,
        velocity,
        dt,
        &params,
    );
    bike.body = moved.state;
    let mut hit_target = None;
    for contact in contacts {
        let p = bike.body.position;
        if p.z >= contact.bounds.min.z - 0.25 && p.z <= contact.bounds.max.z + 0.25 {
            let nearest = Vec2::new(
                p.x.clamp(contact.bounds.min.x, contact.bounds.max.x),
                p.y.clamp(contact.bounds.min.y, contact.bounds.max.y),
            );
            if p.xy().distance(nearest) <= MOTORCYCLE_SHAPE.radius + 0.12 {
                hit_target = Some(hit_target.map_or(contact.id, |id: u64| id.min(contact.id)));
            }
        }
    }
    let in_contact = hit_target.is_some() || moved.blocked_xy;
    let new_contact = in_contact && !bike.contact_latched && bike.contact_cooldown == 0;
    bike.contact_latched = in_contact;
    let mut impact = false;
    if hit_target.is_some() {
        bike.body.velocity.x *= 0.48;
        bike.body.velocity.y *= 0.48;
        if new_contact {
            impact = pre_impact_speed > 1.0;
            bike.condition = bike
                .condition
                .saturating_sub((pre_impact_speed * 0.45).round().clamp(1.0, 14.0) as u8);
            bike.contact_cooldown = 14;
        } else {
            hit_target = None;
        }
    }
    if moved.blocked_xy {
        bike.body.velocity.x *= 0.24;
        bike.body.velocity.y *= 0.24;
        if new_contact && pre_impact_speed > 1.0 && hit_target.is_none() {
            impact = true;
            bike.condition = bike
                .condition
                .saturating_sub((pre_impact_speed * 0.8).round().clamp(1.0, 18.0) as u8);
            bike.contact_cooldown = 14;
        }
    }
    let mut wipeout = None;
    if condition_before > 0 && bike.condition == 0 && mounted {
        bike.phase = MotorcyclePhase::Skidding;
        bike.pending_crash_damage = 2;
        bike.recovery_ticks = 110;
        bike.last_throttle = 0.0;
        wipeout = Some(MotorcycleWipeout::HighSpeedEjection);
    }
    if !bike.body.motion.is_grounded() {
        bike.body.pitch = (bike.body.velocity.z * 0.08).clamp(-0.25, 0.25);
    }
    bike.wheel_travel += (bike.body.position - travel_before).xy().length();

    let now_speed = bike.speed();
    if matches!(
        bike.phase,
        MotorcyclePhase::Skidding | MotorcyclePhase::Tipped
    ) {
        if now_speed <= MOTORCYCLE_STOP_SPEED {
            bike.phase = MotorcyclePhase::Tipped;
            bike.body.velocity = Vec3::ZERO;
        }
    } else {
        bike.phase = if now_speed <= MOTORCYCLE_STOP_SPEED && bike.body.motion.is_grounded() {
            MotorcyclePhase::Parked
        } else {
            MotorcyclePhase::Rolling
        };
    }

    MotorcycleStep {
        contact_target: hit_target,
        contact_point: bike.body.position,
        contact_normal: Vec3::Z,
        pre_impact_speed,
        wipeout,
        impact,
    }
}

fn approach(current: f32, target: f32, max_delta: f32) -> f32 {
    let delta = (target - current).clamp(-max_delta, max_delta);
    current + delta
}

fn decelerate(velocity: Vec2, amount: f32) -> Vec2 {
    let speed = velocity.length();
    if speed <= amount {
        Vec2::ZERO
    } else {
        velocity * ((speed - amount) / speed)
    }
}

fn wrap_angle(angle: f32) -> f32 {
    let mut a = angle % TAU;
    if a > std::f32::consts::PI {
        a -= TAU;
    } else if a < -std::f32::consts::PI {
        a += TAU;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spatial::{
        Collider, ColliderKind, SupportId, SupportSurface, TerrainSample, TerrainTriangle,
    };
    use crate::{Material, Position, Surface, Tile3D};

    struct TestWorld {
        z: f32,
        water_z: Option<f32>,
        supports: Vec<SupportSurface>,
        colliders: Vec<Collider>,
    }

    impl SpatialWorld for TestWorld {
        fn terrain_at(&self, xy: Vec2) -> Option<TerrainSample> {
            Some(TerrainSample {
                z: self.z,
                normal: Vec3::Z,
                tile: Position::new(xy.x.floor() as i32, xy.y.floor() as i32),
                water_z: self.water_z,
                triangle: TerrainTriangle::UpperLeft,
            })
        }

        fn supports_at(&self, _xy: Vec2, out: &mut Vec<SupportSurface>) {
            out.clear();
            out.extend_from_slice(&self.supports);
        }

        fn colliders_in(&self, bounds: Aabb3, out: &mut Vec<Collider>) {
            out.clear();
            out.extend(
                self.colliders
                    .iter()
                    .copied()
                    .filter(|c| c.bounds.intersects(bounds)),
            );
        }
    }

    fn support(
        id: SupportId,
        z: f32,
        walkable: bool,
        surface: Surface,
        water_z: Option<f32>,
    ) -> SupportSurface {
        SupportSurface {
            id,
            z,
            normal: Vec3::Z,
            walkable,
            is_stair: false,
            floor: 0,
            surface,
            water_z,
        }
    }

    #[test]
    fn closer_solid_wall_prevents_contact_with_infected_behind_it() {
        let wall_tile = Tile3D::new(2, 0, 0);
        let world = TestWorld {
            z: 0.0,
            water_z: None,
            supports: vec![support(SupportId::Terrain, 0.0, true, Surface::Road, None)],
            colliders: vec![Collider {
                bounds: Aabb3::new(Vec3::new(1.5, -1.0, 0.0), Vec3::new(1.65, 1.0, 2.5)),
                kind: ColliderKind::Wall { tile: wall_tile },
                material: Material::BrickMasonry,
            }],
        };
        let mut bike = MotorcycleState::parked(BodyState::from_pose_2d(
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 0.0),
            0.0,
            SupportId::Terrain,
        ));
        bike.body.velocity = Vec3::new(17.0, 0.0, 0.0);
        let infected = VehicleContactBody {
            id: 7,
            bounds: Aabb3::from_center_half_extents(
                Vec3::new(2.65, 0.0, 0.8),
                Vec3::new(0.25, 0.25, 0.8),
            ),
        };

        let result = step_motorcycle(
            &mut bike,
            Some(MotorcycleInput::default()),
            &world,
            &[infected],
            &MotorcycleTuning::default(),
        );

        assert!(result.impact, "bike should hit the nearer wall");
        assert_eq!(
            result.contact_target, None,
            "wall occludes the infected contact"
        );
        assert!(
            bike.body.position.x < 1.2,
            "sweep stops the bike before the wall: {:?}",
            bike.body.position
        );
    }

    #[test]
    fn holding_brake_at_a_stop_pushes_backward_at_walking_pace() {
        let world = TestWorld {
            z: 0.0,
            water_z: None,
            supports: vec![support(SupportId::Terrain, 0.0, true, Surface::Road, None)],
            colliders: Vec::new(),
        };
        let mut bike = MotorcycleState::parked(BodyState::from_pose_2d(
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            0.0,
            SupportId::Terrain,
        ));
        let tuning = MotorcycleTuning::default();
        let mut top_speed = 0.0_f32;

        for _ in 0..120 {
            let step = step_motorcycle(
                &mut bike,
                Some(MotorcycleInput {
                    brake: 1.0,
                    ..MotorcycleInput::default()
                }),
                &world,
                &[],
                &tuning,
            );
            assert!(step.wipeout.is_none());
            top_speed = top_speed.max(bike.speed());
        }

        assert!(
            bike.body.position.x < -0.8,
            "holding brake should push the bike backward: {:?}",
            bike.body.position
        );
        assert!(
            top_speed <= tuning.reverse_max_speed + 0.02,
            "reverse should stay at pushing pace: {top_speed}"
        );
        assert!((top_speed * MPH_PER_UNIT_PER_SECOND) < 2.5);
        assert_eq!(bike.condition, 100);
    }

    #[test]
    fn reverse_handlebar_steering_uses_bicycle_kinematics() {
        let world = TestWorld {
            z: 0.0,
            water_z: None,
            supports: vec![support(SupportId::Terrain, 0.0, true, Surface::Road, None)],
            colliders: Vec::new(),
        };
        let mut bike = MotorcycleState::parked(BodyState::from_pose_2d(
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            0.0,
            SupportId::Terrain,
        ));
        let tuning = MotorcycleTuning::default();

        for _ in 0..90 {
            let step = step_motorcycle(
                &mut bike,
                Some(MotorcycleInput {
                    steer: 1.0,
                    brake: 1.0,
                    ..MotorcycleInput::default()
                }),
                &world,
                &[],
                &tuning,
            );
            assert!(step.wipeout.is_none());
        }

        assert!(
            bike.body.heading < -0.2,
            "a right handlebar input should swing the nose left while backing: {}",
            bike.body.heading
        );
        assert!(bike.body.position.x < -0.4);
    }

    #[test]
    fn motorcycle_supports_bridge_deck_over_submerged_bed_and_stops_at_deep_water() {
        let bridge = Position::new(10, 10);
        let bridge_world = TestWorld {
            z: -2.0,
            water_z: Some(-0.1),
            supports: vec![
                support(SupportId::Terrain, -2.0, false, Surface::Water, Some(-0.1)),
                support(
                    SupportId::Bridge(bridge),
                    0.95,
                    true,
                    Surface::Bridge,
                    Some(-0.1),
                ),
            ],
            colliders: vec![Collider {
                bounds: Aabb3::new(Vec3::new(-5.0, -5.0, -2.0), Vec3::new(15.0, 15.0, -0.1)),
                kind: ColliderKind::WaterHazard { tile: bridge },
                material: Material::None,
            }],
        };
        let mut bridge_bike = MotorcycleState::parked(BodyState::from_pose_2d(
            Vec2::new(bridge.x as f32, bridge.y as f32),
            Vec2::new(1.0, 0.0),
            0.95,
            SupportId::Bridge(bridge),
        ));
        bridge_bike.body.velocity = Vec3::new(3.0, 0.0, 0.0);
        step_motorcycle(
            &mut bridge_bike,
            Some(MotorcycleInput::default()),
            &bridge_world,
            &[],
            &MotorcycleTuning::default(),
        );
        assert!((bridge_bike.body.position.z - 0.95).abs() < 0.02);
        assert_eq!(
            bridge_bike.body.motion,
            crate::spatial::MotionState::Grounded(SupportId::Bridge(bridge))
        );

        let water = Position::new(1, 0);
        let water_world = TestWorld {
            z: 0.0,
            water_z: Some(-0.1),
            supports: vec![support(SupportId::Terrain, 0.0, true, Surface::Road, None)],
            colliders: vec![Collider {
                bounds: Aabb3::new(Vec3::new(1.5, -2.0, -2.0), Vec3::new(3.0, 2.0, 1.8)),
                kind: ColliderKind::WaterHazard { tile: water },
                material: Material::None,
            }],
        };
        let mut water_bike = MotorcycleState::parked(BodyState::from_pose_2d(
            Vec2::new(0.9, 0.0),
            Vec2::new(1.0, 0.0),
            0.0,
            SupportId::Terrain,
        ));
        water_bike.body.velocity = Vec3::new(MotorcycleTuning::default().max_speed, 0.0, 0.0);
        let blocked = step_motorcycle(
            &mut water_bike,
            Some(MotorcycleInput::default()),
            &water_world,
            &[],
            &MotorcycleTuning::default(),
        );
        assert!(blocked.impact, "deep-water hazard should block the bike");
        assert!(
            water_bike.body.position.x < 1.2,
            "bike must remain on the dry side: {:?}",
            water_bike.body.position
        );
    }
}
