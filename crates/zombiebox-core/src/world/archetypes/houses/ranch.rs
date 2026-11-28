use super::*;
use crate::world::blueprint::VehicleSlot;

/// Ranch A opens into the street-facing living room. Ranch B has a hall
/// entry, a front kitchen and a backyard-facing living room.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RanchArrangement {
    A,
    B,
}

/// Compound planner for the furnished ranch fixture.
pub struct RanchParcel;

impl CompoundPlanner for RanchParcel {
    fn plan(&self, context: &ParcelContext) -> Result<ParcelPlan, FitError> {
        let id = parcel_id(context.seed, context.bounds);
        let arrangement = if crate::action::mix64(context.seed ^ id.0) & 1 == 0 {
            RanchArrangement::A
        } else {
            RanchArrangement::B
        };
        let structure =
            ranch_structure(context.seed, context.bounds, context.frontage, arrangement)?;
        let garage_door = structure
            .blueprint
            .doors
            .iter()
            .find(|door| door.kind == DoorKind::Gate)
            .ok_or(FitError::AccessFailure {
                reason: "ranch garage gate is missing",
            })?
            .cell;
        let driveways = (garage_door.u - 1..=garage_door.u + 1)
            .map(|u| {
                (
                    structure
                        .transform
                        .local_to_world(LocalCell::new(u, garage_door.v - 1, 0)),
                    structure
                        .transform
                        .local_to_world(LocalCell::new(u, garage_door.v + 1, 0)),
                )
            })
            .collect();
        let slot = &structure.blueprint.vehicle_slots[0];
        let mut slot_min = Position::new(i32::MAX, i32::MAX);
        for u in slot.anchor.u..slot.anchor.u + i32::from(slot.width) {
            for v in slot.anchor.v..slot.anchor.v + i32::from(slot.length) {
                let world = structure.transform.local_to_world(LocalCell::new(u, v, 0));
                slot_min.x = slot_min.x.min(world.x);
                slot_min.y = slot_min.y.min(world.y);
            }
        }
        Ok(ParcelPlan {
            id,
            bounds: context.bounds,
            frontage: context.frontage,
            zone: context.zone,
            outdoor: OutdoorLayer {
                paths: vec![(context.frontage.access, structure.front_door)],
                driveways,
                vehicle_slots: vec![(slot_min, structure.transform.world_facing(slot.facing))],
                ..OutdoorLayer::default()
            },
            structures: vec![structure],
        })
    }
}

/// Deterministic furnished Ranch A/B preview entry point.
pub fn ranch_blueprint(
    seed: u64,
    frontage: Frontage,
    arrangement: RanchArrangement,
) -> Result<BlueprintGrid, FitError> {
    furnish_ranch(
        &StructureContext {
            seed,
            parcel: crate::world::blueprint::ParcelId(1),
            frontage,
        },
        arrangement,
    )
}

fn furnish_ranch(
    context: &StructureContext,
    arrangement: RanchArrangement,
) -> Result<BlueprintGrid, FitError> {
    let mut blueprint = ranch_house_with_width(context, arrangement, false)?;
    furnish::furnish(context.seed, &mut blueprint)?;
    Ok(blueprint)
}

/// Deterministic furnished Ranch A/B placed in an explicit lot.
pub fn ranch_structure(
    seed: u64,
    lot: GridRect,
    frontage: Frontage,
    arrangement: RanchArrangement,
) -> Result<crate::world::blueprint::PlacedStructure, FitError> {
    let parcel = parcel_id(seed, lot);
    let context = StructureContext {
        seed,
        parcel,
        frontage,
    };
    // Keep the narrow garage when the seeded wider variant cannot fit the
    // frontage, while retaining the same rooms and garage connection.
    let (available_width, _) = crate::world::floorplan::usable_dimensions(lot, frontage);
    let mut bp = ranch_house_with_width(&context, arrangement, available_width < GARAGE_LEFT + 7)?;
    furnish::furnish(seed, &mut bp)?;
    let public_door = bp
        .doors
        .iter()
        .find(|door| door.kind == DoorKind::Exterior)
        .ok_or(FitError::AccessFailure {
            reason: "ranch dwelling entry is missing",
        })?
        .cell;
    let (name, archetype) = match arrangement {
        RanchArrangement::A => ("L-wing ranch A", "ranch_l_wing_a"),
        RanchArrangement::B => ("L-wing ranch B", "ranch_l_wing_b"),
    };
    let mut structure = place_blueprint_along_frontage(
        seed,
        parcel,
        lot,
        frontage,
        bp,
        name,
        archetype,
        BuildingStyle::RanchSage,
    )?;
    structure.front_door = structure.transform.local_to_world(public_door);
    Ok(structure)
}

pub(super) const GARAGE_ROOM: u16 = 20;
pub(super) const UTILITY: u16 = 21;

const HOUSE_FRONT: i32 = 3;
const HOUSE_BACK: i32 = 13;
const HOUSE_RIGHT: i32 = 17;
const HALL_U: i32 = 7;
const PUBLIC_LEFT: i32 = HALL_U + 2;
const UTILITY_LEFT: i32 = HOUSE_RIGHT - 2;
const GARAGE_LEFT: i32 = HOUSE_RIGHT + 1;

pub(super) fn ranch_garage_door() -> LocalCell {
    LocalCell::new(GARAGE_LEFT + 2, 0, 0)
}

pub(super) fn ranch_public_door(arrangement: RanchArrangement) -> LocalCell {
    let u = match arrangement {
        RanchArrangement::A => PUBLIC_LEFT + 3,
        RanchArrangement::B => HALL_U,
    };
    LocalCell::new(u, HOUSE_FRONT, 0)
}

/// Build a wide dwelling around a full-depth hall and two bands of rooms.
/// The seed moves the transverse partition, and the two arrangements swap
/// the kitchen and living room as well as which rooms open off the entry.
#[cfg(test)]
pub(super) fn ranch_house(
    context: &StructureContext,
    arrangement: RanchArrangement,
) -> Result<BlueprintGrid, FitError> {
    ranch_house_with_width(context, arrangement, false)
}

fn ranch_house_with_width(
    context: &StructureContext,
    arrangement: RanchArrangement,
    compact: bool,
) -> Result<BlueprintGrid, FitError> {
    let dimensions = crate::action::mix64(context.seed ^ 0x5241_4e43_4850_4c4e);
    let split = 8 + (dimensions & 1) as i32;
    let garage_right = GARAGE_LEFT
        + 5
        + if compact {
            0
        } else {
            ((dimensions >> 1) & 1) as i32
        };
    let front = (HOUSE_FRONT + 1, split - 1);
    let rear = (split + 1, HOUSE_BACK - 1);
    let (living, kitchen, bedroom, bathroom, entry) = match arrangement {
        RanchArrangement::A => (front, rear, front, rear, LIVING),
        RanchArrangement::B => (rear, front, rear, front, HALL),
    };
    let mid = |(start, end)| (start + end) / 2;
    let utility_v = match arrangement {
        RanchArrangement::A => kitchen.0,
        RanchArrangement::B => kitchen.1,
    };
    let garage_back = (utility_v + 2).max(8);
    let mut bp = BlueprintGrid::new(1);
    shell(&mut bp, (0, HOUSE_RIGHT), (HOUSE_FRONT, HOUSE_BACK), 0);
    shell(&mut bp, (GARAGE_LEFT, garage_right), (0, garage_back), 1);

    let rooms = [
        (
            LIVING,
            RoomKind::Living,
            (PUBLIC_LEFT, HOUSE_RIGHT - 1),
            living,
            Surface::WoodFloor,
            0,
        ),
        (
            KITCHEN,
            RoomKind::Kitchen,
            (PUBLIC_LEFT, UTILITY_LEFT - 2),
            kitchen,
            Surface::KitchenTile,
            0,
        ),
        (
            UTILITY,
            RoomKind::Storage,
            (UTILITY_LEFT, HOUSE_RIGHT - 1),
            kitchen,
            Surface::KitchenTile,
            0,
        ),
        (
            HALL,
            RoomKind::Hall,
            (HALL_U, HALL_U),
            (HOUSE_FRONT + 1, HOUSE_BACK - 1),
            Surface::WoodFloor,
            0,
        ),
        (
            BEDROOM,
            RoomKind::Bedroom,
            (1, HALL_U - 2),
            bedroom,
            Surface::Carpet,
            0,
        ),
        (
            BATHROOM,
            RoomKind::Bathroom,
            (1, HALL_U - 2),
            bathroom,
            Surface::KitchenTile,
            0,
        ),
        (
            GARAGE_ROOM,
            RoomKind::Garage,
            (GARAGE_LEFT + 1, garage_right - 1),
            (1, garage_back - 1),
            Surface::Driveway,
            1,
        ),
    ];
    for (id, kind, (left, right), (front, back), surface, wing) in rooms {
        add_house_room(
            &mut bp,
            id,
            kind,
            GridRect::new(Position::new(left, front), Position::new(right, back)),
            surface,
            wing,
        );
    }

    let public_door = ranch_public_door(arrangement);
    let garage_door = ranch_garage_door();
    for (cell, tangent, kind, connects, wing) in [
        (
            public_door,
            DoorAxis::AlongX,
            DoorKind::Exterior,
            (SpaceId::Exterior, SpaceId::Room(entry)),
            0,
        ),
        (
            garage_door,
            DoorAxis::AlongX,
            DoorKind::Gate,
            (SpaceId::Exterior, SpaceId::Room(GARAGE_ROOM)),
            1,
        ),
    ] {
        add_house_door(&mut bp, cell, tangent, kind, connects, wing);
    }
    for (u, v, room_a, room_b) in [
        (HALL_U - 1, mid(bedroom), HALL, BEDROOM),
        (HALL_U - 1, mid(bathroom), HALL, BATHROOM),
        (HALL_U + 1, mid(living), HALL, LIVING),
        (HALL_U + 1, utility_v, HALL, KITCHEN),
        (UTILITY_LEFT - 1, utility_v, KITCHEN, UTILITY),
        (HOUSE_RIGHT, utility_v, UTILITY, GARAGE_ROOM),
    ] {
        add_house_door(
            &mut bp,
            LocalCell::new(u, v, 0),
            DoorAxis::AlongY,
            DoorKind::Interior,
            (SpaceId::Room(room_a), SpaceId::Room(room_b)),
            0,
        );
    }
    // Keep the street-facing gate's material and floor treatment consistent
    // with the driveway and the garage slab.
    let gate = bp.cells.get_mut(&garage_door).unwrap();
    gate.material = Material::MetalPole;
    gate.surface = Surface::Driveway;

    let garage_threshold = LocalCell::new(GARAGE_LEFT, utility_v, 0);
    bp.cells.insert(garage_threshold, {
        let mut cell = BlueprintCell::new(
            BlueprintKind::Floor,
            Material::BrickMasonry,
            Surface::Driveway,
        )
        .with_room(GARAGE_ROOM)
        .indoor()
        .with_roof();
        cell.wing = 1;
        cell
    });
    bp.rooms
        .iter_mut()
        .find(|room| room.id == GARAGE_ROOM)
        .unwrap()
        .cells
        .push(garage_threshold);

    // A paving strip marks the recessed dwelling entry. Glazing on the long
    // facade makes the main block read independently of the garage.
    for u in public_door.u - 1..=public_door.u + 1 {
        bp.set_cell(
            LocalCell::new(u, HOUSE_FRONT - 1, 0),
            BlueprintCell::new(BlueprintKind::Floor, Material::BrickMasonry, Surface::Porch),
        );
    }
    for (u, v) in [
        (3, HOUSE_FRONT),
        (4, HOUSE_FRONT),
        (10, HOUSE_FRONT),
        (13, HOUSE_FRONT),
        (3, HOUSE_BACK),
        (11, HOUSE_BACK),
        (12, HOUSE_BACK),
    ] {
        if (u, v) != (public_door.u, public_door.v) {
            bp.set_cell(
                LocalCell::new(u, v, 0),
                BlueprintCell::new(BlueprintKind::Window, Material::WoodSiding, Surface::Porch)
                    .load_bearing(),
            );
        }
    }
    bp.vehicle_slots.push(VehicleSlot {
        anchor: BlueprintAnchor::new(garage_right - 2, 2, 0),
        facing: PropFacing::North,
        length: 5,
        width: 2,
    });
    Ok(bp)
}

fn shell(bp: &mut BlueprintGrid, (left, right): (i32, i32), (front, back): (i32, i32), wing: u8) {
    for v in front..=back {
        for u in left..=right {
            let mut cell = if u == left || u == right || v == front || v == back {
                BlueprintCell::new(
                    BlueprintKind::ExteriorWall,
                    Material::WoodSiding,
                    Surface::Porch,
                )
                .load_bearing()
            } else {
                BlueprintCell::new(
                    BlueprintKind::InteriorWall,
                    Material::Drywall,
                    Surface::WoodFloor,
                )
                .indoor()
            };
            cell.wing = wing;
            bp.set_cell(LocalCell::new(u, v, 0), cell);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::blueprint::CardinalDir;
    use std::collections::BTreeSet;

    #[test]
    fn compact_ranch_fits_narrow_frontage_in_each_orientation() {
        for side in [
            CardinalDir::North,
            CardinalDir::East,
            CardinalDir::South,
            CardinalDir::West,
        ] {
            let lot = match side {
                CardinalDir::North | CardinalDir::South => {
                    GridRect::new(Position::new(0, 0), Position::new(25, 19))
                }
                CardinalDir::East | CardinalDir::West => {
                    GridRect::new(Position::new(0, 0), Position::new(19, 25))
                }
            };
            let frontage = Frontage {
                side,
                access: match side {
                    CardinalDir::North => Position::new(12, 0),
                    CardinalDir::South => Position::new(12, 19),
                    CardinalDir::East => Position::new(19, 12),
                    CardinalDir::West => Position::new(0, 12),
                },
            };
            for seed in 0..16 {
                for arrangement in [RanchArrangement::A, RanchArrangement::B] {
                    let placed = ranch_structure(seed, lot, frontage, arrangement)
                        .unwrap_or_else(|error| panic!("{side:?} {seed} {arrangement:?}: {error}"));
                    assert_eq!(placed.blueprint.local_bounds().width(), 24);
                    assert!(lot.contains(placed.bounds.min) && lot.contains(placed.bounds.max));
                    placed.blueprint.validate().unwrap();
                }
            }
        }
    }

    #[test]
    fn seeded_ranch_dimensions_keep_both_arrangements_furnishable() {
        let frontage = Frontage {
            side: CardinalDir::North,
            access: Position::new(0, 0),
        };
        for arrangement in [RanchArrangement::A, RanchArrangement::B] {
            let mut kitchen_bounds = BTreeSet::new();
            let mut widths = BTreeSet::new();
            for seed in 0..16 {
                let bp = ranch_blueprint(seed, frontage, arrangement)
                    .unwrap_or_else(|error| panic!("seed {seed}, {arrangement:?}: {error}"));
                bp.validate().unwrap();
                let kitchen = bp
                    .rooms
                    .iter()
                    .find(|room| room.kind == RoomKind::Kitchen)
                    .unwrap();
                kitchen_bounds.insert((
                    kitchen.cells.first().unwrap().v,
                    kitchen.cells.last().unwrap().v,
                ));
                widths.insert(bp.local_bounds().width());
            }
            assert_eq!(kitchen_bounds.len(), 2, "{arrangement:?}");
            assert_eq!(widths.len(), 2, "{arrangement:?}");
        }
    }
}
