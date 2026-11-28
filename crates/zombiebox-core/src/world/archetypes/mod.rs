//! Static archetype definitions and the parcel-composition registration seam.
//!
//! Phase 3 registers authored fixtures and house families through this module.
//! Later retail packages add their definitions here and resolve through the
//! same [`CompoundPlanner`] path, so the county never special-cases an
//! archetype.

use crate::Position;
use crate::action::{BuildingStyle, DoorAxis, Material, PropFacing, PropId, Surface, Vec3};

use super::RoomKind;
pub use super::blueprint::ParcelContext;
use super::blueprint::{
    BlueprintAnchor, BlueprintCell, BlueprintGrid, BlueprintKind, CardinalDir, CompoundPlanner,
    DoorIntent, DoorKind, FitError, LocalCell, LocalPlacementKey, OutdoorLayer, ParcelPlan,
    PlannedProp, RoomMeta, SpaceId, StructureContext, StructureGenerator, parcel_id,
    place_blueprint_in_lot,
};

#[path = "houses/colonial.rs"]
pub mod colonial;
pub mod houses;
pub mod support_fixture;

/// Opt-in generation fixture used while the Phase 3 rendering gates are open.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CountyFixture {
    #[default]
    None,
    /// Replace one existing county lot with a single authored blueprint room.
    BlueprintLot,
    /// Replace one existing county lot with the furnished WP 3.2 Craftsman.
    CraftsmanLot,
    /// One Craftsman, Ranch and Colonial near the normal county start.
    ResidentialStreet,
    /// Small aligned two-floor support and stairs fixture.
    SupportLot,
    /// A furnished two-floor Colonial house.
    ColonialLot,
}

/// Resolve the selected fixture through the same parcel-composition seam the
/// county uses for every authored archetype.
pub fn plan_fixture(
    fixture: CountyFixture,
    context: &ParcelContext,
) -> Result<ParcelPlan, FitError> {
    match fixture {
        CountyFixture::None => Err(FitError::InvalidLot {
            reason: "no fixture selected",
        }),
        CountyFixture::BlueprintLot => FixtureParcel.plan(context),
        CountyFixture::CraftsmanLot | CountyFixture::ResidentialStreet => {
            houses::CraftsmanParcel.plan(context)
        }
        CountyFixture::SupportLot => support_fixture::SupportFixtureParcel.plan(context),
        CountyFixture::ColonialLot => colonial::ColonialParcel.plan(context),
    }
}

pub struct FixtureRoom;

impl StructureGenerator for FixtureRoom {
    fn generate(&self, _context: &StructureContext) -> Result<BlueprintGrid, FitError> {
        Ok(fixture_blueprint())
    }
}

pub struct FixtureParcel;

impl CompoundPlanner for FixtureParcel {
    fn plan(&self, context: &ParcelContext) -> Result<ParcelPlan, FitError> {
        let id = parcel_id(context.seed, context.bounds);
        let blueprint = FixtureRoom.generate(&StructureContext {
            seed: context.seed,
            parcel: id,
            frontage: context.frontage,
        })?;
        let structure = place_blueprint_in_lot(
            context.seed,
            id,
            context.bounds,
            context.frontage,
            blueprint,
            "Blueprint fixture",
            "blueprint_fixture",
            BuildingStyle::WarmPutty,
        )?;
        let access = context.frontage.access;
        Ok(ParcelPlan {
            id,
            bounds: context.bounds,
            frontage: context.frontage,
            zone: context.zone,
            outdoor: OutdoorLayer {
                paths: vec![(access, structure.front_door)],
                ..OutdoorLayer::default()
            },
            structures: vec![structure],
        })
    }
}

/// The smallest authored blueprint that exercises every WP 3.1 contract:
/// an asymmetric footprint, a non-centered exterior door, a window, and one
/// even-span Phase 2 prop (a table, `2 × 1` in its authored frame).
pub fn fixture_blueprint() -> BlueprintGrid {
    use BlueprintKind::*;
    const W: i32 = 7;
    const D: i32 = 6;
    let mut bp = BlueprintGrid::new(1);
    let wall = Material::WoodSiding;
    let floor = Material::WoodLight;
    for v in 0..D {
        for u in 0..W {
            let perimeter = u == 0 || u == W - 1 || v == 0 || v == D - 1;
            let cell = if perimeter {
                let window = (u, v) == (5, 0) || (u, v) == (1, D - 1) || (u, v) == (0, 2);
                BlueprintCell::new(
                    if window { Window } else { ExteriorWall },
                    wall,
                    Surface::Porch,
                )
            } else {
                BlueprintCell::new(Floor, floor, Surface::WoodFloor)
                    .with_room(1)
                    .indoor()
                    .with_roof()
            };
            bp.set_cell(LocalCell::new(u, v, 0), cell);
        }
    }
    bp.set_cell(
        LocalCell::new(2, 0, 0),
        BlueprintCell::new(
            Door {
                exterior: true,
                gate: false,
            },
            Material::WoodDoor,
            Surface::Porch,
        ),
    );
    bp.add_door(DoorIntent {
        cell: LocalCell::new(2, 0, 0),
        tangent: DoorAxis::AlongX,
        kind: DoorKind::Exterior,
        connects: (SpaceId::Exterior, SpaceId::Room(1)),
    });
    bp.rooms.push(RoomMeta {
        id: 1,
        kind: RoomKind::Living,
        unit: None,
        wing: 0,
        floor: 0,
        cells: (1..W - 1)
            .flat_map(|u| (1..D - 1).map(move |v| LocalCell::new(u, v, 0)))
            .collect(),
    });
    bp.props.push(PlannedProp {
        key: LocalPlacementKey(1),
        prop: PropId::Table,
        anchor: BlueprintAnchor::new(3, 2, 0),
        facing: PropFacing::North,
        room: Some(1),
        host: None,
        offset_m: Vec3::ZERO,
        loot: None,
    });
    bp
}

/// Adapter used by the parcel dispatch. `front` is the existing lot's public
/// frontage tile; it is kept as the access point in world coordinates.
pub(super) fn frontage_side(min: Position, max: Position, front: Position) -> CardinalDir {
    if front.x <= min.x {
        CardinalDir::West
    } else if front.x >= max.x {
        CardinalDir::East
    } else if front.y <= min.y {
        CardinalDir::North
    } else {
        CardinalDir::South
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::blueprint::{Frontage, GridRect, ZoneKind};

    #[test]
    fn fixture_plan_is_stable_and_has_one_exterior_door_and_prop() {
        let context = ParcelContext {
            seed: 42,
            bounds: GridRect::new(Position::new(90, 90), Position::new(130, 130)),
            frontage: Frontage {
                side: CardinalDir::North,
                access: Position::new(110, 90),
            },
            zone: ZoneKind::Residential,
        };
        let a = FixtureParcel.plan(&context).unwrap();
        let b = FixtureParcel.plan(&context).unwrap();
        assert_eq!(a, b);
        let structure = &a.structures[0];
        let door = structure.blueprint.doors[0].cell;
        assert_eq!(
            structure.transform.local_to_world(door),
            structure.front_door
        );
        assert_eq!(structure.props_world().len(), 1);
        assert_eq!(structure.planned_props().len(), 1);
    }
}
