//! Authored Phase 3 house families.
//!
//! WP 3.2 ships the first Craftsman bungalow arrangement (`porch -> living ->
//! dining/kitchen`, with a short bedroom hall). The structure is authored in
//! local blueprint space and furnished through the shared [`super::furnish`]
//! solver, so the county materializer never special-cases a house.

use crate::Position;
use crate::action::{
    BuildingStyle, CellAttachment, DoorAxis, Material, PropFacing, PropId, ReadableId, Surface,
    Vec3,
};

use super::super::RoomKind;
use super::super::furnish;
use crate::world::blueprint::{
    BlueprintAnchor, BlueprintCell, BlueprintGrid, BlueprintKind, CompoundPlanner, DoorIntent,
    DoorKind, FitError, Frontage, GridRect, LocalCell, LocalPlacementKey, OutdoorLayer,
    ParcelContext, ParcelPlan, PlannedSign, RoomMeta, SpaceId, StructureContext,
    StructureGenerator, parcel_id, place_blueprint_along_frontage,
};

/// Arrangement A: a one-floor Craftsman bungalow with a street-facing porch.
pub struct CraftsmanBungalow;

impl StructureGenerator for CraftsmanBungalow {
    fn generate(&self, context: &StructureContext) -> Result<BlueprintGrid, FitError> {
        craftsman_bungalow(context)
    }
}

/// Compound planner used by the residential fixture and later town lots.
pub struct CraftsmanParcel;

impl CompoundPlanner for CraftsmanParcel {
    fn plan(&self, context: &ParcelContext) -> Result<ParcelPlan, FitError> {
        let id = parcel_id(context.seed, context.bounds);
        let mut blueprint = CraftsmanBungalow.generate(&StructureContext {
            seed: context.seed,
            parcel: id,
            frontage: context.frontage,
        })?;
        furnish::furnish(context.seed, &mut blueprint)?;
        let structure = place_blueprint_along_frontage(
            context.seed,
            id,
            context.bounds,
            context.frontage,
            blueprint,
            "Craftsman bungalow",
            "craftsman_bungalow_a",
            BuildingStyle::WarmPutty,
        )?;
        Ok(ParcelPlan {
            id,
            bounds: context.bounds,
            frontage: context.frontage,
            zone: context.zone,
            outdoor: OutdoorLayer {
                paths: vec![(context.frontage.access, structure.front_door)],
                ..OutdoorLayer::default()
            },
            structures: vec![structure],
        })
    }
}

const LIVING: u16 = 1;
const KITCHEN: u16 = 2;
const HALL: u16 = 3;
const BEDROOM: u16 = 4;
const BATHROOM: u16 = 5;

/// Local footprint:
///
/// ```text
/// u:    1 ..... 13
/// v=0    ..###..          porch front
/// v=1    ..###..
/// v=2    #############    front wall (door + windows)
/// v=3-6  # living # kitchen #
/// v=7    #########  wall
/// v=8    hall (full width)
/// v=9    #########  wall
/// v=10-12 # bedroom # bathroom #
/// v=13   #############    rear wall
/// ```
fn craftsman_bungalow(_context: &StructureContext) -> Result<BlueprintGrid, FitError> {
    let mut bp = BlueprintGrid::new(1);
    let exterior = Material::WoodSiding;
    let interior = Material::Drywall;
    let floor_mat = Material::WoodLight;

    // Main mass perimeter.
    for v in 2..=13 {
        for u in 1..=13 {
            let perimeter = u == 1 || u == 13 || v == 2 || v == 13;
            if !perimeter {
                continue;
            }
            let window = matches!(
                (u, v),
                (3, 2)
                    | (4, 2)
                    | (9, 2)
                    | (10, 2)
                    | (3, 13)
                    | (9, 13)
                    | (1, 4)
                    | (1, 11)
                    | (13, 11)
            );
            let door = (u, v) == (6, 2);
            let (kind, material, indoor) = if door {
                (
                    BlueprintKind::Door {
                        exterior: true,
                        gate: false,
                    },
                    Material::WoodDoor,
                    false,
                )
            } else if window {
                (BlueprintKind::Window, exterior, false)
            } else {
                (BlueprintKind::ExteriorWall, exterior, false)
            };
            let mut cell = BlueprintCell::new(kind, material, Surface::Porch).load_bearing();
            cell.indoor = indoor;
            bp.set_cell(LocalCell::new(u, v, 0), cell);
        }
    }

    // Porch deck and two posts at the street edge.
    for v in 0..=1 {
        for u in 4..=8 {
            bp.set_cell(
                LocalCell::new(u, v, 0),
                BlueprintCell::new(BlueprintKind::Floor, floor_mat, Surface::Porch),
            );
        }
    }
    for u in [4, 8] {
        bp.set_cell(
            LocalCell::new(u, 0, 0),
            BlueprintCell::new(BlueprintKind::Pillar, Material::MetalPole, Surface::Porch)
                .load_bearing(),
        );
    }

    // Interior wall rows between the front rooms, hall and rear rooms.
    for v in [7, 9] {
        for u in 2..=12 {
            let door =
                (u, v) == (3, 7) || (u, v) == (11, 7) || (u, v) == (3, 9) || (u, v) == (11, 9);
            let kind = if door {
                BlueprintKind::Door {
                    exterior: false,
                    gate: false,
                }
            } else {
                BlueprintKind::InteriorWall
            };
            let material = if door { Material::WoodDoor } else { interior };
            bp.set_cell(
                LocalCell::new(u, v, 0),
                BlueprintCell::new(kind, material, Surface::WoodFloor).indoor(),
            );
        }
    }

    // Living/dining and bedroom/bath partitions, with a cased opening between
    // the living room and the kitchen/dining half.
    for v in 3..=6 {
        let door = v == 5;
        bp.set_cell(
            LocalCell::new(7, v, 0),
            BlueprintCell::new(
                if door {
                    BlueprintKind::Door {
                        exterior: false,
                        gate: false,
                    }
                } else {
                    BlueprintKind::InteriorWall
                },
                if door { Material::WoodDoor } else { interior },
                Surface::WoodFloor,
            )
            .indoor(),
        );
    }
    for v in 10..=12 {
        bp.set_cell(
            LocalCell::new(7, v, 0),
            BlueprintCell::new(BlueprintKind::InteriorWall, interior, Surface::Carpet).indoor(),
        );
    }

    // Room floors.
    let rooms = [
        (LIVING, RoomKind::Living, 2, 6, 3, 6, Surface::WoodFloor),
        (
            KITCHEN,
            RoomKind::Kitchen,
            8,
            12,
            3,
            6,
            Surface::KitchenTile,
        ),
        (HALL, RoomKind::Hall, 2, 12, 8, 8, Surface::WoodFloor),
        (BEDROOM, RoomKind::Bedroom, 2, 6, 10, 12, Surface::Carpet),
        (
            BATHROOM,
            RoomKind::Bathroom,
            8,
            12,
            10,
            12,
            Surface::KitchenTile,
        ),
    ];
    for (id, kind, min_u, max_u, min_v, max_v, surface) in rooms {
        let mut cells = Vec::new();
        for v in min_v..=max_v {
            for u in min_u..=max_u {
                let local = LocalCell::new(u, v, 0);
                let material = match surface {
                    Surface::KitchenTile => Material::BrickMasonry,
                    Surface::Carpet => Material::WoodLight,
                    _ => floor_mat,
                };
                bp.set_cell(
                    local,
                    BlueprintCell::new(BlueprintKind::Floor, material, surface)
                        .with_room(id)
                        .indoor()
                        .with_roof(),
                );
                cells.push(local);
            }
        }
        bp.rooms.push(RoomMeta {
            id,
            kind,
            unit: None,
            wing: 0,
            floor: 0,
            cells,
        });
    }

    // One authoritative door intent per opening. Tangents follow the wall run.
    let doors = [
        (
            LocalCell::new(6, 2, 0),
            DoorAxis::AlongX,
            DoorKind::Exterior,
            SpaceId::Exterior,
            SpaceId::Room(LIVING),
        ),
        (
            LocalCell::new(7, 5, 0),
            DoorAxis::AlongY,
            DoorKind::Interior,
            SpaceId::Room(LIVING),
            SpaceId::Room(KITCHEN),
        ),
        (
            LocalCell::new(3, 7, 0),
            DoorAxis::AlongX,
            DoorKind::Interior,
            SpaceId::Room(LIVING),
            SpaceId::Room(HALL),
        ),
        (
            LocalCell::new(11, 7, 0),
            DoorAxis::AlongX,
            DoorKind::Interior,
            SpaceId::Room(KITCHEN),
            SpaceId::Room(HALL),
        ),
        (
            LocalCell::new(3, 9, 0),
            DoorAxis::AlongX,
            DoorKind::Interior,
            SpaceId::Room(HALL),
            SpaceId::Room(BEDROOM),
        ),
        (
            LocalCell::new(11, 9, 0),
            DoorAxis::AlongX,
            DoorKind::Interior,
            SpaceId::Room(HALL),
            SpaceId::Room(BATHROOM),
        ),
    ];
    for (cell, tangent, kind, a, b) in doors {
        bp.add_door(DoorIntent {
            cell,
            tangent,
            kind,
            connects: (a, b),
        });
    }

    // A readable on the bedside table. The furnisher always places the
    // required nightstand at this authored anchor, so the note stays supported.
    bp.signs.push(PlannedSign {
        key: LocalPlacementKey(0x6000),
        anchor: BlueprintAnchor::new(3, 12, 0),
        prop: PropId::Note,
        attachment: CellAttachment::CounterTop,
        offset_m: Vec3::new(0.0, 0.0, 1.2),
        facing: PropFacing::North,
        readable: Some(ReadableId::CraftsmanNote),
    });

    Ok(bp)
}

/// Build the first residential fixture parcel for the existing county lot.
pub fn craftsman_fixture_parcel(context: &ParcelContext) -> Result<ParcelPlan, FitError> {
    CraftsmanParcel.plan(context)
}

/// Public snapshot used by previews and tests.
pub fn craftsman_blueprint(seed: u64, frontage: Frontage) -> Result<BlueprintGrid, FitError> {
    let mut bp = CraftsmanBungalow.generate(&StructureContext {
        seed,
        parcel: crate::world::blueprint::ParcelId(1),
        frontage,
    })?;
    furnish::furnish(seed, &mut bp)?;
    Ok(bp)
}

/// Convenience for tests that need a deterministic lot transform without
/// selecting a county parcel.
pub fn craftsman_structure(
    seed: u64,
    lot: GridRect,
    frontage: Frontage,
) -> Result<crate::world::blueprint::PlacedStructure, FitError> {
    let parcel = parcel_id(seed, lot);
    let mut bp = CraftsmanBungalow.generate(&StructureContext {
        seed,
        parcel,
        frontage,
    })?;
    furnish::furnish(seed, &mut bp)?;
    place_blueprint_along_frontage(
        seed,
        parcel,
        lot,
        frontage,
        bp,
        "Craftsman bungalow",
        "craftsman_bungalow_a",
        BuildingStyle::WarmPutty,
    )
}

/// Helper used by parcel fixture selection to keep the access point explicit.
pub fn frontage_for_lot(min: Position, max: Position, front: Position) -> Frontage {
    Frontage {
        side: super::frontage_side(min, max, front),
        access: front,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::ReadableId;
    use std::collections::HashSet;

    use crate::world::blueprint::CardinalDir;

    fn lot() -> GridRect {
        GridRect::new(Position::new(100, 100), Position::new(130, 130))
    }

    fn frontage(side: CardinalDir) -> Frontage {
        Frontage {
            side,
            access: Position::new(115, 100),
        }
    }

    #[test]
    fn arrangement_a_has_required_rooms_fixtures_and_readable() {
        let structure = craftsman_structure(42, lot(), frontage(CardinalDir::North)).unwrap();
        let bp = &structure.blueprint;
        bp.validate().unwrap();
        for kind in [
            RoomKind::Living,
            RoomKind::Kitchen,
            RoomKind::Bedroom,
            RoomKind::Bathroom,
        ] {
            assert!(
                bp.rooms.iter().any(|room| room.kind == kind),
                "missing room {kind:?}"
            );
        }
        let ids: Vec<PropId> = bp.props.iter().map(|prop| prop.prop).collect();
        for required in [
            PropId::KitchenSink,
            PropId::KitchenStove,
            PropId::Fridge,
            PropId::Sofa,
            PropId::Toilet,
            PropId::BathroomSink,
            PropId::Bathtub,
        ] {
            assert!(ids.contains(&required), "missing fixture {required:?}");
        }
        assert!(
            ids.iter()
                .any(|id| matches!(id, PropId::BedDouble | PropId::Bed))
        );
        assert!(
            ids.contains(&PropId::Fridge) && PropId::Fridge.def().loot.is_some(),
            "the Craftsman must ship one supported searchable container"
        );
        assert_eq!(
            bp.doors
                .iter()
                .filter(|door| door.kind == DoorKind::Exterior)
                .count(),
            1
        );
        let note = bp
            .signs
            .iter()
            .find(|sign| sign.readable == Some(ReadableId::CraftsmanNote))
            .expect("Craftsman note");
        assert_eq!(note.attachment, CellAttachment::CounterTop);
        // The note anchor must sit on the required nightstand anchor so the
        // readable is physically supported in the live scene.
        assert!(bp.props.iter().any(|prop| {
            prop.prop == PropId::Counter
                && prop.anchor.u == note.anchor.u
                && prop.anchor.v == note.anchor.v
                && prop.anchor.floor == note.anchor.floor
        }));
    }

    #[test]
    fn craftsman_plan_is_stable_and_fits_all_four_orientations() {
        for side in [
            CardinalDir::North,
            CardinalDir::East,
            CardinalDir::South,
            CardinalDir::West,
        ] {
            let a = craftsman_structure(7, lot(), frontage(side)).unwrap();
            let b = craftsman_structure(7, lot(), frontage(side)).unwrap();
            assert_eq!(a, b, "{side:?}");
            assert!(lot().contains(a.bounds.min) && lot().contains(a.bounds.max));
            match side {
                CardinalDir::North | CardinalDir::South => {
                    assert_eq!((a.bounds.width(), a.bounds.height()), (13, 14));
                }
                CardinalDir::East | CardinalDir::West => {
                    assert_eq!((a.bounds.width(), a.bounds.height()), (14, 13));
                }
            }
            let door = a
                .blueprint
                .doors
                .iter()
                .find(|d| d.kind == DoorKind::Exterior)
                .unwrap()
                .cell;
            assert_eq!(a.transform.local_to_world(door), a.front_door);
            let axis = a.door_axis_at(door).unwrap();
            match side {
                CardinalDir::North | CardinalDir::South => {
                    assert_eq!(axis, DoorAxis::AlongX);
                    let expected_y = if side == CardinalDir::North {
                        a.bounds.min.y + 2
                    } else {
                        a.bounds.max.y - 2
                    };
                    assert_eq!(a.front_door.y, expected_y, "{side:?}");
                }
                CardinalDir::East | CardinalDir::West => {
                    assert_eq!(axis, DoorAxis::AlongY);
                    let expected_x = if side == CardinalDir::West {
                        a.bounds.min.x + 2
                    } else {
                        a.bounds.max.x - 2
                    };
                    assert_eq!(a.front_door.x, expected_x, "{side:?}");
                }
            }
            // Furnished footprints must not overlap on the same attachment layer.
            let mut floor_cells = HashSet::new();
            for prop in a.props_world() {
                if prop.attachment != CellAttachment::Floor {
                    continue;
                }
                for cell in prop.span.covered_tiles(prop.tile) {
                    assert!(floor_cells.insert(cell), "{side:?} overlap at {cell:?}");
                }
            }
        }
    }

    #[test]
    fn furnishing_rejects_a_lot_that_is_too_small_for_the_house() {
        let small = GridRect::new(Position::new(100, 100), Position::new(110, 110));
        assert!(matches!(
            craftsman_structure(1, small, frontage(CardinalDir::North)),
            Err(FitError::InvalidLot { .. })
        ));
    }
}
