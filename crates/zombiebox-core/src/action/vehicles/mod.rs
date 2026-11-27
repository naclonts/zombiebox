//! Shared vehicle registry, identity, occupancy, and read-only views.
//!
//! `ActionGame` keeps one authoritative record per vehicle ID in the live run.
//! Each vehicle kind owns its own input schema, fixed-tick physics, and
//! presentation view. Chunk membership is derived from the live position.

mod motorcycle;

use std::collections::BTreeMap;

use crate::Position;
use crate::spatial::{BodyState, SupportId, Vec2};
use crate::world::MotorcycleSpawn;

pub use motorcycle::MotorcycleCrashKind;
pub use motorcycle::MotorcycleInput;
pub(crate) use motorcycle::{
    MotorcycleState, MotorcycleTuning, MotorcycleWipeout, VehicleContactBody, step_motorcycle,
};

/// Stable identity for one vehicle within a generated county run.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct VehicleId(pub u64);

/// Authoritative control mode reported by the last fixed tick.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ControlMode {
    #[default]
    OnFoot,
    Motorcycle,
}

/// Input schema for a driven vehicle. Future kinds add their own variants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RideInput {
    Motorcycle(MotorcycleInput),
}

/// Discrete motorcycle handling phase. `Skidding` and `Tipped` are populated by
/// the crash/recovery slice; the shared phase is part of the public view now.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MotorcyclePhase {
    #[default]
    Parked,
    Rolling,
    Skidding,
    Tipped,
}

/// Read-only presentation state for one motorcycle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotorcycleView {
    pub body: BodyState,
    pub phase: MotorcyclePhase,
    pub steer_angle: f32,
    pub rider_lean: f32,
    /// Forward speed in world units per second.
    pub speed: f32,
    pub condition: u8,
    pub recovery_ticks: u16,
}

/// Read-only vehicle presentation state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VehicleView {
    Motorcycle(MotorcycleView),
}

/// HUD values exposed while mounted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MountedHud {
    pub control_mode: ControlMode,
    pub speed_mph_x10: u16,
    pub grip_use: u8,
    pub engine_load: u8,
    pub condition_pct: u8,
}

pub(crate) enum VehicleState {
    Motorcycle(MotorcycleState),
}

impl VehicleState {
    pub(crate) fn body(&self) -> BodyState {
        match self {
            Self::Motorcycle(bike) => bike.body,
        }
    }

    pub(crate) fn motorcycle(&self) -> Option<&MotorcycleState> {
        match self {
            Self::Motorcycle(bike) => Some(bike),
        }
    }

    pub(crate) fn position(&self) -> Vec2 {
        self.body().position.xy()
    }

    pub(crate) fn view(&self) -> VehicleView {
        match self {
            Self::Motorcycle(bike) => VehicleView::Motorcycle(MotorcycleView {
                body: bike.body,
                phase: bike.phase,
                steer_angle: bike.steer_angle,
                rider_lean: bike.rider_lean,
                speed: bike.speed(),
                condition: bike.condition,
                recovery_ticks: bike.recovery_ticks,
            }),
        }
    }
}

/// Ordered registry of every vehicle in the live run plus the sole occupancy
/// source (`mounted`). Stable iteration by ID keeps snapshots deterministic.
pub(crate) struct Vehicles {
    by_id: BTreeMap<VehicleId, VehicleState>,
    mounted: Option<VehicleId>,
}

impl Default for Vehicles {
    fn default() -> Self {
        Self {
            by_id: BTreeMap::new(),
            mounted: None,
        }
    }
}

impl Vehicles {
    pub(crate) fn from_spawns(
        seed: u64,
        spawns: &[MotorcycleSpawn],
        mut support_at: impl FnMut(Position) -> (f32, SupportId),
    ) -> Self {
        let mut by_id = BTreeMap::new();
        for spawn in spawns {
            let id = vehicle_id(seed, spawn.slot);
            let (z, support) = support_at(spawn.tile);
            let heading = Vec2::new(spawn.heading.cos(), spawn.heading.sin());
            let body = BodyState::from_pose_2d(
                Vec2::new(spawn.tile.x as f32, spawn.tile.y as f32),
                heading,
                z,
                support,
            );
            by_id.insert(id, VehicleState::Motorcycle(MotorcycleState::parked(body)));
        }
        Self {
            by_id,
            mounted: None,
        }
    }

    #[inline]
    pub(crate) fn mounted(&self) -> Option<VehicleId> {
        self.mounted
    }

    pub(crate) fn set_mounted(&mut self, id: Option<VehicleId>) {
        self.mounted = id;
    }

    pub(crate) fn motorcycle(&self, id: VehicleId) -> Option<&MotorcycleState> {
        match self.by_id.get(&id) {
            Some(VehicleState::Motorcycle(bike)) => Some(bike),
            None => None,
        }
    }

    pub(crate) fn motorcycle_mut(&mut self, id: VehicleId) -> Option<&mut MotorcycleState> {
        match self.by_id.get_mut(&id) {
            Some(VehicleState::Motorcycle(bike)) => Some(bike),
            None => None,
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (VehicleId, &VehicleState)> {
        self.by_id.iter().map(|(id, state)| (*id, state))
    }

    pub(crate) fn mounted_hud(&self) -> Option<MountedHud> {
        let id = self.mounted?;
        let bike = self.motorcycle(id)?;
        Some(MountedHud {
            control_mode: ControlMode::Motorcycle,
            speed_mph_x10: bike.speed_mph_x10(),
            grip_use: (bike.traction_use * 100.0).round().clamp(0.0, 100.0) as u8,
            engine_load: bike.engine_load(),
            condition_pct: bike.condition,
        })
    }
}

/// Stable per-slot ID: the low byte is the slot, the rest is seed-derived.
pub(crate) fn vehicle_id(seed: u64, slot: u8) -> VehicleId {
    const VEHICLE_ID_DOMAIN: u64 = 0x7665_6869_636c_6569; // "vehiclei"
    VehicleId((crate::mix64(seed ^ VEHICLE_ID_DOMAIN) & !0xff) | u64::from(slot))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vehicle_ids_are_stable_and_unique_per_slot() {
        let ids: Vec<_> = (0..crate::world::MOTORCYCLE_SLOTS)
            .map(|slot| vehicle_id(42, slot))
            .collect();
        for (i, a) in ids.iter().enumerate() {
            assert_eq!(*a, vehicle_id(42, i as u8));
            for b in &ids[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert_ne!(ids[0], vehicle_id(43, 0));
    }

    #[test]
    fn registry_exposes_mounted_hud_from_the_live_bike() {
        let mut vehicles = Vehicles::default();
        let body = BodyState::from_pose_2d(
            Vec2::new(4.0, 5.0),
            Vec2::new(1.0, 0.0),
            0.0,
            SupportId::Terrain,
        );
        let id = VehicleId(7);
        vehicles
            .by_id
            .insert(id, VehicleState::Motorcycle(MotorcycleState::parked(body)));
        vehicles.set_mounted(Some(id));
        let hud = vehicles.mounted_hud().expect("mounted bike");
        assert_eq!(hud.control_mode, ControlMode::Motorcycle);
        assert_eq!(hud.speed_mph_x10, 0);
        assert_eq!(hud.condition_pct, 100);
    }
}
