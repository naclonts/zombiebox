//! Two-storey Colonial house, fitted within a frontage lot.
use crate::action::{BuildingStyle, DoorAxis, Material, Surface};
use crate::world::RoomKind;
use crate::world::blueprint::{
    BlueprintCell, BlueprintGrid, BlueprintKind, CompoundPlanner, DoorIntent, DoorKind, FitError,
    LocalCell, OutdoorLayer, ParcelContext, ParcelPlan, RoomMeta, SpaceId, StairLink, parcel_id,
    place_blueprint_along_frontage,
};
use crate::world::{floorplan, furnish};

pub struct ColonialParcel;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Arrangement {
    CenterHall,
    SideStair,
}

const COLONIAL_LAYOUT_SALT: u64 = 0x434f_4c4f_4e49_414c;
const HALL: u16 = 1;
const LIVING: u16 = 2;
const KITCHEN: u16 = 3;
const UPSTAIRS_HALL: u16 = 4;
const BEDROOM: u16 = 5;
const SECOND_BEDROOM: u16 = 6;
const BATHROOM: u16 = 7;
const DINING: u16 = 8;

impl CompoundPlanner for ColonialParcel {
    fn plan(&self, context: &ParcelContext) -> Result<ParcelPlan, FitError> {
        let id = parcel_id(context.seed, context.bounds);
        let (available_w, available_d) =
            floorplan::usable_dimensions(context.bounds, context.frontage);
        let mut last_error = FitError::InvalidLot {
            reason: "lot too small for Colonial",
        };
        // Preferred spacious envelope, then a compact authored envelope.
        for (w, d) in [(17, 17), (15, 15)] {
            if w > available_w || d > available_d {
                continue;
            }
            let arrangements = [Arrangement::CenterHall, Arrangement::SideStair];
            for index in floorplan::ordered_arrangements(context.seed, id, COLONIAL_LAYOUT_SALT) {
                let arrangement = arrangements[index];
                let (name, archetype) = match arrangement {
                    Arrangement::CenterHall => ("Colonial A", "colonial_a"),
                    Arrangement::SideStair => ("Colonial B", "colonial_b"),
                };
                // Keep at least four interior tiles in each side bay and a
                // three-tile hall; try bounded center-biased cuts.
                for left in floorplan::midpoint_candidates(5, w - 10)
                    .into_iter()
                    .take(3)
                {
                    let candidate =
                        colonial_blueprint(w, d, arrangement, left).and_then(|mut bp| {
                            furnish::furnish(context.seed, &mut bp)?;
                            place_blueprint_along_frontage(
                                context.seed,
                                id,
                                context.bounds,
                                context.frontage,
                                bp,
                                name,
                                archetype,
                                BuildingStyle::ColonialBrick,
                            )
                        });
                    match candidate {
                        Ok(structure) => {
                            return Ok(ParcelPlan {
                                id,
                                bounds: context.bounds,
                                frontage: context.frontage,
                                zone: context.zone,
                                outdoor: OutdoorLayer {
                                    paths: floorplan::frontage_path(
                                        context.frontage,
                                        structure.front_door,
                                    ),
                                    ..OutdoorLayer::default()
                                },
                                structures: vec![structure],
                            });
                        }
                        Err(error) => last_error = error,
                    }
                }
            }
        }
        Err(last_error)
    }
}

pub(super) fn colonial_blueprint(
    w: i32,
    d: i32,
    arrangement: Arrangement,
    left: i32,
) -> Result<BlueprintGrid, FitError> {
    use BlueprintKind::*;
    let mut bp = BlueprintGrid::new(2);
    let right = left + 4;
    let side_stair = arrangement == Arrangement::SideStair;
    let split = d / 2 - i32::from(side_stair);
    let hall = (w - 1) / 2;
    let rear = floorplan::midpoint_candidates(split + 2, d - 3)[0];
    // The shorter B foyer ends after the rear kitchen door. The last two
    // floor bays open into the living room on the left.
    let hall_end = rear + 2;
    for floor in 0..2 {
        for v in 0..d {
            for u in 0..w {
                let boundary = u == 0 || u == w - 1 || v == 0 || v == d - 1;
                let shortened_hall = side_stair && floor == 0 && v >= hall_end;
                let in_hall = u > left && u < right;
                let partition = (u == left && (!shortened_hall || v == hall_end))
                    || u == right
                    || (u > right && v == split)
                    || (shortened_hall && v == hall_end && in_hall);
                let room = if shortened_hall && u < right {
                    LIVING
                } else if in_hall {
                    if floor == 0 { HALL } else { UPSTAIRS_HALL }
                } else if u < left {
                    if floor == 0 { LIVING } else { BEDROOM }
                } else if floor == 0 {
                    if v < split { DINING } else { KITCHEN }
                } else if v < split {
                    SECOND_BEDROOM
                } else {
                    BATHROOM
                };
                let cell = if boundary {
                    let front_window = v == 0 && [(w - 1) / 2 - 4, (w - 1) / 2 + 4].contains(&u);
                    let side_window = (u == 0 || u == w - 1) && (v == 3 || v == d - 4);
                    BlueprintCell::new(
                        if front_window || side_window {
                            Window
                        } else {
                            ExteriorWall
                        },
                        Material::BrickMasonry,
                        Surface::WoodFloor,
                    )
                    .load_bearing()
                } else if partition {
                    BlueprintCell::new(InteriorWall, Material::Drywall, Surface::WoodFloor).indoor()
                } else {
                    let (material, surface) = match room {
                        KITCHEN | BATHROOM => (Material::BrickMasonry, Surface::KitchenTile),
                        BEDROOM | SECOND_BEDROOM => (Material::WoodLight, Surface::Carpet),
                        _ => (Material::WoodLight, Surface::WoodFloor),
                    };
                    BlueprintCell::new(Floor, material, surface)
                        .with_room(room)
                        .indoor()
                        .with_roof()
                };
                bp.set_cell(LocalCell::new(u, v, floor), cell);
            }
        }
    }
    let (stair_u, stair_v) = if side_stair {
        (left + 1, 4)
    } else {
        (hall, d - 5)
    };
    for floor in 0..2 {
        let mut cell = BlueprintCell::new(
            Stairs {
                up: floor == 0,
                down: floor == 1,
            },
            Material::WoodLight,
            Surface::WoodFloor,
        )
        .with_room(if floor == 0 { HALL } else { UPSTAIRS_HALL })
        .indoor()
        .with_roof();
        cell.stair_dir = if floor == 0 { (0, 1) } else { (0, -1) };
        bp.set_cell(LocalCell::new(stair_u, stair_v, floor), cell);
    }
    bp.stairs.push(StairLink {
        lower: LocalCell::new(stair_u, stair_v, 0),
        upper: LocalCell::new(stair_u, stair_v, 1),
        lower_landing: LocalCell::new(stair_u, stair_v + 1, 0),
        upper_landing: LocalCell::new(stair_u, stair_v - 1, 1),
    });
    let entry = LocalCell::new(hall, 0, 0);
    bp.add_door(DoorIntent {
        cell: entry,
        tangent: DoorAxis::AlongX,
        kind: DoorKind::Exterior,
        connects: (SpaceId::Exterior, SpaceId::Room(HALL)),
    });
    let front = 2..=split - 2;
    let rear_bay = split + 2..=d - 3;
    let living = if side_stair {
        split + 2..=hall_end - 1
    } else {
        front.clone()
    };
    for (u, floor, segment, rooms) in [
        (left, 0, living, [LIVING, HALL]),
        (right, 0, front.clone(), [HALL, DINING]),
        (right, 0, rear_bay.clone(), [HALL, KITCHEN]),
        (left, 1, 2..=d - 3, [BEDROOM, UPSTAIRS_HALL]),
        (right, 1, front, [UPSTAIRS_HALL, SECOND_BEDROOM]),
        (right, 1, rear_bay, [UPSTAIRS_HALL, BATHROOM]),
    ] {
        let tangent = DoorAxis::AlongY;
        let cell = floorplan::shared_wall_opening(
            &bp,
            LocalCell::new(u, 0, floor),
            tangent,
            segment,
            rooms,
        )
        .ok_or(FitError::AccessFailure {
            reason: "no clear Colonial shared-wall opening",
        })?;
        bp.add_door(DoorIntent {
            cell,
            tangent,
            kind: DoorKind::Interior,
            connects: (SpaceId::Room(rooms[0]), SpaceId::Room(rooms[1])),
        });
    }
    for door in &bp.doors {
        let exterior = door.kind == DoorKind::Exterior;
        bp.cells.insert(
            door.cell,
            BlueprintCell::new(
                Door {
                    exterior,
                    gate: false,
                },
                Material::WoodDoor,
                Surface::WoodFloor,
            )
            .indoor(),
        );
    }
    for (id, kind, floor) in [
        (HALL, RoomKind::Hall, 0),
        (LIVING, RoomKind::Living, 0),
        (KITCHEN, RoomKind::Kitchen, 0),
        (DINING, RoomKind::Dining, 0),
        (UPSTAIRS_HALL, RoomKind::Hall, 1),
        (BEDROOM, RoomKind::Bedroom, 1),
        (SECOND_BEDROOM, RoomKind::Bedroom, 1),
        (BATHROOM, RoomKind::Bathroom, 1),
    ] {
        let cells = bp
            .cells
            .iter()
            .filter_map(|(pos, c)| (pos.floor == floor && c.room == Some(id)).then_some(*pos))
            .collect();
        bp.rooms.push(RoomMeta {
            id,
            kind,
            unit: None,
            wing: 0,
            floor,
            cells,
        });
    }
    Ok(bp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Position;
    use crate::world::blueprint::{CardinalDir, Frontage, GridRect, ZoneKind};
    #[test]
    fn both_arrangements_furnish_at_compact_size() {
        for (arrangement, stair_u) in [(Arrangement::CenterHall, 7), (Arrangement::SideStair, 6)] {
            let mut bp = colonial_blueprint(15, 15, arrangement, 5).unwrap();
            furnish::furnish(42, &mut bp).unwrap();
            bp.validate().unwrap();
            let dining = bp
                .rooms
                .iter()
                .find(|room| room.kind == RoomKind::Dining)
                .unwrap();
            let kitchen = bp
                .rooms
                .iter()
                .find(|room| room.kind == RoomKind::Kitchen)
                .unwrap();
            assert!(
                dining.cells.iter().map(|cell| cell.v).max().unwrap()
                    < kitchen.cells.iter().map(|cell| cell.v).min().unwrap()
            );
            assert!(
                bp.props
                    .iter()
                    .any(|prop| prop.room == Some(DINING)
                        && prop.prop == crate::action::PropId::Table)
            );
            assert!(bp.stairs[0].upper_landing.v < bp.stairs[0].upper.v);
            assert_eq!(bp.stairs[0].lower.u, stair_u);
        }
    }
    #[test]
    fn colonial_fits_orientations_and_sizes() {
        for seed in 0..8 {
            for side in [
                CardinalDir::North,
                CardinalDir::East,
                CardinalDir::South,
                CardinalDir::West,
            ] {
                for size in [17, 22, 31] {
                    let min = Position::new(100, 100);
                    let max = Position::new(100 + size - 1, 100 + size - 1);
                    let mid = 100 + size / 2;
                    let access = match side {
                        CardinalDir::North => Position::new(mid, min.y),
                        CardinalDir::East => Position::new(max.x, mid),
                        CardinalDir::South => Position::new(mid, max.y),
                        CardinalDir::West => Position::new(min.x, mid),
                    };
                    let context = ParcelContext {
                        seed,
                        bounds: GridRect::new(min, max),
                        frontage: Frontage { side, access },
                        zone: ZoneKind::Residential,
                    };
                    let plan = ColonialParcel
                        .plan(&context)
                        .unwrap_or_else(|e| panic!("{seed} {side:?} {size}: {e:?}"));
                    let structure = &plan.structures[0];
                    structure.blueprint.validate().unwrap();
                    let (dx, dy) = side.inward();
                    let door = structure.front_door;
                    assert_eq!((door.x - access.x) * dx + (door.y - access.y) * dy, 1);
                    assert_eq!(plan.outdoor.paths.first().unwrap().0, access);
                    assert_eq!(plan.outdoor.paths.last().unwrap().1, door);
                    assert_eq!(plan, ColonialParcel.plan(&context).unwrap());
                    if size == 17 {
                        assert_eq!(structure.blueprint.local_bounds().width(), 15);
                    }
                }
            }
        }
    }

    #[test]
    fn frontage_placement_is_independent_of_loot_profiles() {
        use crate::world::blueprint::LootProfileId;
        let bounds = GridRect::new(Position::new(0, 0), Position::new(25, 26));
        let frontage = Frontage {
            side: CardinalDir::North,
            access: Position::new(12, 0),
        };
        let parcel = parcel_id(42, bounds);
        let mut with_loot = colonial_blueprint(17, 17, Arrangement::CenterHall, 6).unwrap();
        furnish::furnish(42, &mut with_loot).unwrap();
        let mut changed_loot = with_loot.clone();
        for prop in &mut changed_loot.props {
            prop.loot = if prop.loot.is_some() {
                None
            } else {
                Some(LootProfileId(1))
            };
        }
        let place = |blueprint| {
            place_blueprint_along_frontage(
                42,
                parcel,
                bounds,
                frontage,
                blueprint,
                "Colonial A",
                "colonial_a",
                BuildingStyle::ColonialBrick,
            )
            .unwrap()
        };
        let baseline = place(with_loot);
        let changed = place(changed_loot);
        assert_ne!(baseline.blueprint.props, changed.blueprint.props);
        assert_eq!(baseline.transform, changed.transform);
        assert_eq!(baseline.front_door, changed.front_door);
        assert_eq!(baseline.blueprint.cells, changed.blueprint.cells);
        assert_eq!(baseline.blueprint.doors, changed.blueprint.doors);
        assert_eq!(baseline.blueprint.stairs, changed.blueprint.stairs);
    }
}
