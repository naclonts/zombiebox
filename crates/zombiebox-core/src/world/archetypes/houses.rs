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
    ParcelContext, ParcelPlan, PlannedProp, PlannedSign, RoomMeta, RuleKey, SpaceId,
    StructureContext, StructureGenerator, parcel_id, place_blueprint_along_frontage,
};
use crate::world::massing::FootprintMask;

/// Arrangement A: a one-floor Craftsman bungalow with a street-facing porch.
pub struct CraftsmanBungalow;

impl StructureGenerator for CraftsmanBungalow {
    fn generate(&self, context: &StructureContext) -> Result<BlueprintGrid, FitError> {
        craftsman_bungalow(context)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CraftsmanArrangement {
    A,
    B,
    SideHall,
}

impl CraftsmanArrangement {
    fn presentation(self) -> (&'static str, &'static str, BuildingStyle) {
        match self {
            Self::A => (
                "Craftsman bungalow A",
                "craftsman_bungalow_a",
                BuildingStyle::WarmPutty,
            ),
            Self::B => (
                "Craftsman bungalow B",
                "craftsman_bungalow_b",
                BuildingStyle::BlueClapboard,
            ),
            Self::SideHall => (
                "Side-hall Craftsman",
                "craftsman_side_hall",
                BuildingStyle::WarmPutty,
            ),
        }
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

/// Selects a stable Craftsman layout for each street lot.
pub struct CraftsmanStreetParcel;

impl CompoundPlanner for CraftsmanStreetParcel {
    fn plan(&self, context: &ParcelContext) -> Result<ParcelPlan, FitError> {
        let id = parcel_id(context.seed, context.bounds);
        let selection = crate::action::mix64(context.seed ^ id.0);
        let preferred = match selection & 3 {
            0 | 2 => CraftsmanArrangement::A,
            1 => CraftsmanArrangement::B,
            _ => CraftsmanArrangement::SideHall,
        };
        let plan = |arrangement: CraftsmanArrangement| {
            craftsman_arrangement_structure(
                context.seed,
                context.bounds,
                context.frontage,
                arrangement,
            )
        };
        // The side-hall mass is wider than the compact authored bungalow.
        // Skip it on narrow frontages before attempting its furnishing.
        let (width, _) =
            crate::world::floorplan::usable_dimensions(context.bounds, context.frontage);
        let preferred = if preferred == CraftsmanArrangement::SideHall && width < 15 {
            CraftsmanArrangement::A
        } else {
            preferred
        };
        let structure = plan(preferred).or_else(|error| {
            if preferred == CraftsmanArrangement::A {
                Err(error)
            } else {
                plan(CraftsmanArrangement::A)
            }
        })?;
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

mod ranch;
#[cfg(test)]
use ranch::{GARAGE_ROOM, UTILITY, ranch_garage_door, ranch_house, ranch_public_door};
pub use ranch::{RanchArrangement, RanchParcel, ranch_blueprint, ranch_structure};

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

    add_craftsman_porch(&mut bp, 4, 8);

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
        add_house_room(
            &mut bp,
            id,
            kind,
            GridRect::new(Position::new(min_u, min_v), Position::new(max_u, max_v)),
            surface,
            0,
        );
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

    add_craftsman_note(&mut bp);

    Ok(bp)
}

fn craftsman_bungalow_b(context: &StructureContext) -> Result<BlueprintGrid, FitError> {
    let mut bp = craftsman_bungalow(context)?;
    let mut footprint = FootprintMask::new();
    footprint.insert_rect(GridRect::new(
        crate::Position::new(1, 2),
        crate::Position::new(13, 17),
    ));
    footprint.insert_rect(GridRect::new(
        crate::Position::new(4, 0),
        crate::Position::new(8, 1),
    ));

    // The foyer occupies the center partition and opens into living and the
    // dining bay. The public door moves to the centered foyer; the kitchen
    // and bath form a rear service band and bedrooms stay off the side hall.
    let old_entry = LocalCell::new(6, 2, 0);
    bp.set_cell(
        old_entry,
        BlueprintCell::new(
            BlueprintKind::ExteriorWall,
            Material::WoodSiding,
            Surface::Porch,
        )
        .load_bearing(),
    );
    let entry = LocalCell::new(7, 2, 0);
    bp.set_cell(
        entry,
        BlueprintCell::new(
            BlueprintKind::Door {
                exterior: true,
                gate: false,
            },
            Material::WoodDoor,
            Surface::Porch,
        ),
    );

    let kitchen_front: Vec<LocalCell> = bp
        .rooms
        .iter()
        .find(|room| room.id == KITCHEN)
        .expect("Craftsman kitchen")
        .cells
        .clone();
    let bedroom_rear: Vec<LocalCell> = bp
        .rooms
        .iter()
        .find(|room| room.id == BEDROOM)
        .expect("Craftsman bedroom")
        .cells
        .clone();
    let living_front: Vec<LocalCell> = bp
        .rooms
        .iter()
        .find(|room| room.id == LIVING)
        .expect("Craftsman living room")
        .cells
        .clone();

    // Bedrooms use the front side bay; the rear bedroom bay becomes the
    // larger living room at the yard end.
    for local in &living_front {
        let cell = bp.cells.get_mut(local).unwrap();
        cell.room = Some(BEDROOM);
        cell.surface = Surface::Carpet;
    }
    for local in &bedroom_rear {
        let cell = bp.cells.get_mut(local).unwrap();
        cell.room = Some(LIVING);
        cell.surface = Surface::WoodFloor;
    }
    // Front-right becomes the dining side of the foyer. The one-cell center
    // foyer ties the public door to living, dining, and the short hall.
    let dining_id = 6;
    for local in &kitchen_front {
        let cell = bp.cells.get_mut(local).unwrap();
        cell.room = Some(dining_id);
        cell.surface = Surface::WoodFloor;
    }
    let foyer_cells: Vec<_> = (3..=7).map(|v| LocalCell::new(7, v, 0)).collect();
    for local in &foyer_cells {
        bp.set_cell(
            *local,
            BlueprintCell::new(
                BlueprintKind::Floor,
                Material::WoodLight,
                Surface::WoodFloor,
            )
            .with_room(HALL)
            .indoor()
            .with_roof(),
        );
    }

    // Retire the front kitchen and move the functional kitchen to the rear
    // right bay. Extend the rear wall by four cells to fit a real bathroom
    // rather than squeezing its required fixtures into a shallow alcove.
    let mut kitchen_cells = Vec::new();
    for v in 10..=12 {
        for u in 8..=12 {
            let local = LocalCell::new(u, v, 0);
            bp.set_cell(
                local,
                BlueprintCell::new(
                    BlueprintKind::Floor,
                    Material::BrickMasonry,
                    Surface::KitchenTile,
                )
                .with_room(KITCHEN)
                .indoor()
                .with_roof(),
            );
            kitchen_cells.push(local);
        }
    }
    let mut bathroom_cells = Vec::new();
    for v in 14..=16 {
        for u in 8..=12 {
            let local = LocalCell::new(u, v, 0);
            bp.set_cell(
                local,
                BlueprintCell::new(
                    BlueprintKind::Floor,
                    Material::BrickMasonry,
                    Surface::KitchenTile,
                )
                .with_room(BATHROOM)
                .indoor()
                .with_roof(),
            );
            bathroom_cells.push(local);
        }
    }
    // Extend the rear living room to the new exterior wall.
    let mut living_cells = bedroom_rear;
    for v in 13..=16 {
        for u in 2..=6 {
            let local = LocalCell::new(u, v, 0);
            bp.set_cell(
                local,
                BlueprintCell::new(BlueprintKind::Floor, Material::WoodLight, Surface::Carpet)
                    .with_room(LIVING)
                    .indoor()
                    .with_roof(),
            );
            living_cells.push(local);
        }
    }
    bp.rooms
        .iter_mut()
        .find(|room| room.id == KITCHEN)
        .unwrap()
        .cells = kitchen_cells;
    bp.rooms
        .iter_mut()
        .find(|room| room.id == BATHROOM)
        .unwrap()
        .cells = bathroom_cells;
    bp.rooms
        .iter_mut()
        .find(|room| room.id == LIVING)
        .unwrap()
        .cells = living_cells;
    bp.rooms
        .iter_mut()
        .find(|room| room.id == BEDROOM)
        .unwrap()
        .cells = living_front;
    bp.rooms.push(RoomMeta {
        id: dining_id,
        kind: RoomKind::Hall,
        unit: None,
        wing: 0,
        floor: 0,
        cells: kitchen_front,
    });
    bp.rooms
        .iter_mut()
        .find(|room| room.id == HALL)
        .unwrap()
        .cells
        .extend(foyer_cells);
    bp.props.push(PlannedProp {
        key: LocalPlacementKey(0x5200),
        prop: PropId::Table,
        anchor: BlueprintAnchor::new(9, 4, 0),
        facing: PropFacing::North,
        room: Some(dining_id),
        host: None,
        offset_m: Vec3::ZERO,
        loot: None,
    });

    // Rebuild the interior band and extended shell. The main hall's two side
    // doors continue into bedroom and kitchen; a rear service door joins
    // kitchen to bathroom.
    for v in 14..=17 {
        for u in [1, 13] {
            bp.set_cell(
                LocalCell::new(u, v, 0),
                BlueprintCell::new(
                    BlueprintKind::ExteriorWall,
                    Material::WoodSiding,
                    Surface::Porch,
                )
                .load_bearing(),
            );
        }
    }
    for u in 1..=13 {
        let kind = if matches!(u, 3 | 4 | 9 | 10) {
            BlueprintKind::Window
        } else {
            BlueprintKind::ExteriorWall
        };
        bp.set_cell(
            LocalCell::new(u, 17, 0),
            BlueprintCell::new(kind, Material::WoodSiding, Surface::Porch).load_bearing(),
        );
    }
    // The left bay already absorbed the former rear wall. Close the
    // right bay with a partition between the kitchen and bathroom.
    for u in 7..=12 {
        let local = LocalCell::new(u, 13, 0);
        bp.set_cell(
            local,
            BlueprintCell::new(
                BlueprintKind::InteriorWall,
                Material::Drywall,
                Surface::WoodFloor,
            )
            .indoor(),
        );
    }
    bp.cells.remove(&LocalCell::new(7, 7, 0));
    bp.set_cell(
        LocalCell::new(7, 7, 0),
        BlueprintCell::new(
            BlueprintKind::Floor,
            Material::WoodLight,
            Surface::WoodFloor,
        )
        .with_room(HALL)
        .indoor()
        .with_roof(),
    );

    bp.doors
        .retain(|door| door.cell != old_entry && door.cell != LocalCell::new(7, 5, 0));
    bp.add_door(DoorIntent {
        cell: entry,
        tangent: crate::action::DoorAxis::AlongX,
        kind: DoorKind::Exterior,
        connects: (SpaceId::Exterior, SpaceId::Room(HALL)),
    });
    if let Some(door) = bp
        .doors
        .iter_mut()
        .find(|door| door.cell == LocalCell::new(11, 7, 0))
    {
        door.connects = (SpaceId::Room(dining_id), SpaceId::Room(HALL));
    }
    if let Some(door) = bp
        .doors
        .iter_mut()
        .find(|door| door.cell == LocalCell::new(3, 7, 0))
    {
        door.connects = (SpaceId::Room(BEDROOM), SpaceId::Room(HALL));
    }
    if let Some(door) = bp
        .doors
        .iter_mut()
        .find(|door| door.cell == LocalCell::new(3, 9, 0))
    {
        door.connects = (SpaceId::Room(HALL), SpaceId::Room(LIVING));
    }
    if let Some(door) = bp
        .doors
        .iter_mut()
        .find(|door| door.cell == LocalCell::new(11, 9, 0))
    {
        door.connects = (SpaceId::Room(HALL), SpaceId::Room(KITCHEN));
    }
    add_house_door(
        &mut bp,
        LocalCell::new(11, 13, 0),
        DoorAxis::AlongX,
        DoorKind::Interior,
        (SpaceId::Room(KITCHEN), SpaceId::Room(BATHROOM)),
        0,
    );

    if footprint.len() != 13 * 16 + 5 * 2 || !footprint.contains(LocalCell::new(7, 17, 0)) {
        return Err(FitError::ImpossibleRoom {
            reason: "Craftsman B footprint composition is inconsistent",
        });
    }

    Ok(bp)
}

fn add_house_room(
    bp: &mut BlueprintGrid,
    id: u16,
    kind: RoomKind,
    bounds: GridRect,
    surface: Surface,
    wing: u8,
) {
    let material = match surface {
        Surface::KitchenTile => Material::BrickMasonry,
        _ => Material::WoodLight,
    };
    let mut cell = BlueprintCell::new(BlueprintKind::Floor, material, surface)
        .with_room(id)
        .indoor()
        .with_roof();
    cell.wing = wing;
    let mut cells = Vec::new();
    for v in bounds.min.y..=bounds.max.y {
        for u in bounds.min.x..=bounds.max.x {
            let local = LocalCell::new(u, v, 0);
            bp.set_cell(local, cell);
            cells.push(local);
        }
    }
    bp.rooms.push(RoomMeta {
        id,
        kind,
        unit: None,
        wing,
        floor: 0,
        cells,
    });
}

fn add_house_door(
    bp: &mut BlueprintGrid,
    cell: LocalCell,
    tangent: DoorAxis,
    kind: DoorKind,
    connects: (SpaceId, SpaceId),
    wing: u8,
) {
    let exterior = kind != DoorKind::Interior;
    let mut tile = BlueprintCell::new(
        BlueprintKind::Door {
            exterior,
            gate: kind == DoorKind::Gate,
        },
        Material::WoodDoor,
        if exterior {
            Surface::Porch
        } else {
            Surface::WoodFloor
        },
    );
    tile.indoor = !exterior;
    tile.load_bearing = exterior;
    tile.wing = wing;
    bp.set_cell(cell, tile);
    bp.add_door(DoorIntent {
        cell,
        tangent,
        kind,
        connects,
    });
}

fn add_craftsman_porch(bp: &mut BlueprintGrid, min_u: i32, max_u: i32) {
    for v in 0..=1 {
        for u in min_u..=max_u {
            bp.set_cell(
                LocalCell::new(u, v, 0),
                BlueprintCell::new(BlueprintKind::Floor, Material::WoodLight, Surface::Porch),
            );
        }
    }
    for u in [min_u, max_u] {
        bp.set_cell(
            LocalCell::new(u, 0, 0),
            BlueprintCell::new(BlueprintKind::Pillar, Material::MetalPole, Surface::Porch)
                .load_bearing(),
        );
    }
}

fn add_craftsman_note(bp: &mut BlueprintGrid) {
    // A readable on the bedside table. `host` ties it to the required
    // nightstand (RuleKey 31); furnishing re-anchors the sign to that prop's
    // placed anchor, facing and surface height, so the note stays supported.
    bp.signs.push(PlannedSign {
        key: LocalPlacementKey(0x6000),
        anchor: BlueprintAnchor::new(0, 0, 0),
        prop: PropId::Note,
        attachment: CellAttachment::CounterTop,
        offset_m: Vec3::new(0.0, 0.0, 1.2),
        facing: PropFacing::North,
        readable: Some(ReadableId::CraftsmanNote),
        host: Some(RuleKey(31)),
    });
}

/// A side-hall bungalow with rooms sized from the seed. The public rooms
/// span the porch frontage and the hall separates the bedroom from the wet rooms.
fn craftsman_side_hall(context: &StructureContext) -> BlueprintGrid {
    let layout = crate::action::mix64(context.seed ^ context.parcel.0 ^ 0x82c3_c65b_8c64_9471);
    let hall = 7 + (layout & 1) as i32;
    // A separate frontage choice keeps glazing independent of room dimensions.
    let front_window = 10
        + (crate::action::mix64(context.seed ^ context.parcel.0 ^ 0x4652_4f4e_5457_494e) & 1)
            as i32;
    let living_end = 6 + ((layout >> 1) & 1) as i32;
    let kitchen_end = living_end + 5 + ((layout >> 2) & 1) as i32;
    let rear = kitchen_end + 5;
    let mut bp = BlueprintGrid::new(1);

    for v in 2..=rear {
        for u in 1..=15 {
            let exterior = u == 1 || u == 15 || v == 2 || v == rear;
            let partition = v == living_end + 1
                || (v > living_end && (u == hall - 1 || u == hall + 1))
                || (u > hall && v == kitchen_end + 1);
            let cell = if exterior {
                let window = if v == 2 {
                    matches!(u, 3 | 4) || u == front_window || u == front_window + 1
                } else if v == rear {
                    matches!(u, 3 | 4 | 10 | 11)
                } else {
                    v == living_end - 1 || v == living_end + 4 || v == rear - 2
                };
                BlueprintCell::new(
                    if window {
                        BlueprintKind::Window
                    } else {
                        BlueprintKind::ExteriorWall
                    },
                    Material::WoodSiding,
                    Surface::Porch,
                )
                .load_bearing()
            } else if partition {
                BlueprintCell::new(
                    BlueprintKind::InteriorWall,
                    Material::Drywall,
                    Surface::WoodFloor,
                )
                .indoor()
            } else {
                continue;
            };
            bp.set_cell(LocalCell::new(u, v, 0), cell);
        }
    }
    add_craftsman_porch(&mut bp, 4, 8);

    for (id, kind, min, max, surface) in [
        (
            LIVING,
            RoomKind::Living,
            (2, 3),
            (14, living_end),
            Surface::WoodFloor,
        ),
        (
            HALL,
            RoomKind::Hall,
            (hall, living_end + 2),
            (hall, rear - 1),
            Surface::WoodFloor,
        ),
        (
            BEDROOM,
            RoomKind::Bedroom,
            (2, living_end + 2),
            (hall - 2, rear - 1),
            Surface::Carpet,
        ),
        (
            KITCHEN,
            RoomKind::Kitchen,
            (hall + 2, living_end + 2),
            (14, kitchen_end),
            Surface::KitchenTile,
        ),
        (
            BATHROOM,
            RoomKind::Bathroom,
            (hall + 2, kitchen_end + 2),
            (14, rear - 1),
            Surface::KitchenTile,
        ),
    ] {
        add_house_room(
            &mut bp,
            id,
            kind,
            GridRect::new(Position::new(min.0, min.1), Position::new(max.0, max.1)),
            surface,
            0,
        );
    }

    add_house_door(
        &mut bp,
        LocalCell::new(6, 2, 0),
        DoorAxis::AlongX,
        DoorKind::Exterior,
        (SpaceId::Exterior, SpaceId::Room(LIVING)),
        0,
    );
    add_house_door(
        &mut bp,
        LocalCell::new(hall, living_end + 1, 0),
        DoorAxis::AlongX,
        DoorKind::Interior,
        (SpaceId::Room(LIVING), SpaceId::Room(HALL)),
        0,
    );
    for (u, v, room) in [
        (hall - 1, living_end + 3, BEDROOM),
        (hall + 1, living_end + 4, KITCHEN),
        (hall + 1, rear - 2, BATHROOM),
    ] {
        add_house_door(
            &mut bp,
            LocalCell::new(u, v, 0),
            DoorAxis::AlongY,
            DoorKind::Interior,
            (SpaceId::Room(HALL), SpaceId::Room(room)),
            0,
        );
    }
    add_craftsman_note(&mut bp);
    bp
}

fn craftsman_authored(
    context: &StructureContext,
    arrangement: CraftsmanArrangement,
) -> Result<BlueprintGrid, FitError> {
    match arrangement {
        CraftsmanArrangement::A => CraftsmanBungalow.generate(context),
        CraftsmanArrangement::B => craftsman_bungalow_b(context),
        CraftsmanArrangement::SideHall => Ok(craftsman_side_hall(context)),
    }
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

pub fn craftsman_arrangement_blueprint(
    seed: u64,
    frontage: Frontage,
    arrangement: CraftsmanArrangement,
) -> Result<BlueprintGrid, FitError> {
    let mut bp = craftsman_authored(
        &StructureContext {
            seed,
            parcel: crate::world::blueprint::ParcelId(1),
            frontage,
        },
        arrangement,
    )?;
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

pub fn craftsman_arrangement_structure(
    seed: u64,
    lot: GridRect,
    frontage: Frontage,
    arrangement: CraftsmanArrangement,
) -> Result<crate::world::blueprint::PlacedStructure, FitError> {
    let parcel = parcel_id(seed, lot);
    let mut bp = craftsman_authored(
        &StructureContext {
            seed,
            parcel,
            frontage,
        },
        arrangement,
    )?;
    furnish::furnish(seed, &mut bp)?;
    let (name, archetype, style) = arrangement.presentation();
    place_blueprint_along_frontage(seed, parcel, lot, frontage, bp, name, archetype, style)
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
            prop.prop == PropId::Nightstand
                && prop.anchor.u == note.anchor.u
                && prop.anchor.v == note.anchor.v
                && prop.anchor.floor == note.anchor.floor
        }));
    }

    #[test]
    fn craftsman_arrangement_b_has_center_foyer_rear_kitchen_and_side_hall() {
        for side in [
            CardinalDir::North,
            CardinalDir::East,
            CardinalDir::South,
            CardinalDir::West,
        ] {
            let a =
                craftsman_arrangement_structure(42, lot(), frontage(side), CraftsmanArrangement::B)
                    .unwrap();
            let b =
                craftsman_arrangement_structure(42, lot(), frontage(side), CraftsmanArrangement::B)
                    .unwrap();
            assert_eq!(a, b, "{side:?}");
            a.blueprint.validate().unwrap();
            assert_eq!(a.archetype, "craftsman_bungalow_b");
            assert_eq!(a.blueprint.local_bounds().height(), 18);
            let public = a
                .blueprint
                .doors
                .iter()
                .find(|door| door.kind == DoorKind::Exterior)
                .unwrap();
            assert_eq!(public.cell, LocalCell::new(7, 2, 0));
            assert_eq!(public.connects.1, SpaceId::Room(HALL));
            let kitchen = a
                .blueprint
                .rooms
                .iter()
                .find(|room| room.id == KITCHEN)
                .unwrap();
            assert!(
                kitchen
                    .cells
                    .iter()
                    .all(|cell| cell.v >= 10 && cell.v <= 12)
            );
            let bathroom = a
                .blueprint
                .rooms
                .iter()
                .find(|room| room.id == BATHROOM)
                .unwrap();
            assert!(
                bathroom
                    .cells
                    .iter()
                    .all(|cell| cell.v >= 14 && cell.v <= 16)
            );
            let living = a
                .blueprint
                .rooms
                .iter()
                .find(|room| room.id == LIVING)
                .unwrap();
            assert!(living.cells.iter().all(|cell| cell.v >= 10));
            let bedroom = a
                .blueprint
                .rooms
                .iter()
                .find(|room| room.id == BEDROOM)
                .unwrap();
            assert!(bedroom.cells.iter().all(|cell| cell.v <= 6));
            assert!(a.blueprint.props.iter().any(|prop| {
                prop.key == LocalPlacementKey(0x5200)
                    && prop.prop == PropId::Table
                    && prop.anchor == BlueprintAnchor::new(9, 4, 0)
            }));
            let bedroom_entry = a
                .blueprint
                .doors
                .iter()
                .find(|door| door.cell == LocalCell::new(3, 7, 0))
                .unwrap();
            assert_eq!(bedroom_entry.connects.0, SpaceId::Room(BEDROOM));
        }
    }

    #[test]
    fn craftsman_side_hall_dimensions_vary_and_furnish() {
        use std::collections::BTreeSet;

        let mut layouts = BTreeSet::new();
        let mut facade_windows = BTreeSet::new();
        for seed in 0..64 {
            let side = [
                CardinalDir::North,
                CardinalDir::East,
                CardinalDir::South,
                CardinalDir::West,
            ][seed as usize % 4];
            let blueprint = craftsman_arrangement_structure(
                seed,
                lot(),
                frontage(side),
                CraftsmanArrangement::SideHall,
            )
            .unwrap_or_else(|error| panic!("seed {seed} {side:?}: {error:?}"))
            .blueprint;
            blueprint.validate().unwrap();
            facade_windows.insert(
                (10..=12)
                    .filter(|&u| {
                        blueprint.cells[&LocalCell::new(u, 2, 0)].kind == BlueprintKind::Window
                    })
                    .collect::<Vec<_>>(),
            );
            let hall = blueprint.rooms.iter().find(|room| room.id == HALL).unwrap();
            let door = blueprint
                .doors
                .iter()
                .find(|door| door.connects == (SpaceId::Room(LIVING), SpaceId::Room(HALL)))
                .unwrap();
            layouts.insert((
                hall.cells[0].u,
                door.cell.v,
                blueprint.local_bounds().height(),
            ));
            for room in [BEDROOM, KITCHEN, BATHROOM] {
                assert!(
                    blueprint.doors.iter().any(|door| {
                        door.connects == (SpaceId::Room(HALL), SpaceId::Room(room))
                    })
                );
            }
            assert!(
                blueprint
                    .props
                    .iter()
                    .any(|prop| prop.prop == PropId::Fridge)
            );
            assert!(
                blueprint
                    .signs
                    .iter()
                    .any(|sign| sign.readable == Some(ReadableId::CraftsmanNote))
            );
        }
        assert_eq!(layouts.len(), 8);
        assert_eq!(facade_windows.len(), 2);
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
    fn ranch_arrangements_fit_each_orientation_with_separate_garage_access() {
        for side in [
            CardinalDir::North,
            CardinalDir::East,
            CardinalDir::South,
            CardinalDir::West,
        ] {
            let frontage = match side {
                CardinalDir::North => Frontage {
                    side,
                    access: Position::new(115, 100),
                },
                CardinalDir::East => Frontage {
                    side,
                    access: Position::new(130, 115),
                },
                CardinalDir::South => Frontage {
                    side,
                    access: Position::new(115, 130),
                },
                CardinalDir::West => Frontage {
                    side,
                    access: Position::new(100, 115),
                },
            };
            for arrangement in [RanchArrangement::A, RanchArrangement::B] {
                let raw = ranch_house(
                    &StructureContext {
                        seed: 42,
                        parcel: parcel_id(42, lot()),
                        frontage,
                    },
                    arrangement,
                )
                .unwrap();
                let mut door_cells = HashSet::new();
                for door in &raw.doors {
                    assert!(
                        door_cells.insert(door.cell),
                        "duplicate raw {side:?} {arrangement:?} at {:?}: {:?}",
                        door.cell,
                        raw.doors
                    );
                }
                let a = ranch_structure(42, lot(), frontage, arrangement).unwrap();
                let b = ranch_structure(42, lot(), frontage, arrangement).unwrap();
                assert_eq!(a, b, "{side:?} {arrangement:?}");
                assert!(lot().contains(a.bounds.min) && lot().contains(a.bounds.max));
                let (width, depth) = match side {
                    CardinalDir::North | CardinalDir::South => {
                        (a.bounds.width(), a.bounds.height())
                    }
                    CardinalDir::East | CardinalDir::West => (a.bounds.height(), a.bounds.width()),
                };
                assert!(width > depth);
                let dwelling = a.blueprint.rooms.iter().filter(|room| room.wing == 0);
                let min_u = dwelling
                    .clone()
                    .flat_map(|room| &room.cells)
                    .map(|cell| cell.u)
                    .min()
                    .unwrap();
                let max_u = dwelling
                    .flat_map(|room| &room.cells)
                    .map(|cell| cell.u)
                    .max()
                    .unwrap();
                assert!(
                    max_u - min_u + 1 > 13,
                    "ranch dwelling is wider than the bungalow"
                );
                assert!(!a.blueprint.cells.contains_key(&LocalCell::new(min_u, 0, 0)));
                let public = a
                    .blueprint
                    .doors
                    .iter()
                    .find(|door| door.kind == DoorKind::Exterior)
                    .unwrap();
                assert_eq!(a.transform.local_to_world(public.cell), a.front_door);
                assert_eq!(public.cell, ranch_public_door(arrangement));
                assert!(a.blueprint.doors.iter().any(|door| {
                    door.kind == DoorKind::Gate && door.cell == ranch_garage_door()
                }));
                assert!(
                    a.blueprint
                        .vehicle_slots
                        .iter()
                        .any(|slot| { slot.width == 2 && slot.length == 5 })
                );
                for (a_id, b_id) in [(UTILITY, GARAGE_ROOM), (KITCHEN, UTILITY)] {
                    let door = a
                        .blueprint
                        .doors
                        .iter()
                        .find(|door| door.connects == (SpaceId::Room(a_id), SpaceId::Room(b_id)))
                        .expect("garage-to-kitchen route door");
                    assert_eq!(door.kind, DoorKind::Interior);
                    let left = a.blueprint.cells[&LocalCell::new(door.cell.u - 1, door.cell.v, 0)];
                    let right = a.blueprint.cells[&LocalCell::new(door.cell.u + 1, door.cell.v, 0)];
                    assert_eq!((left.room, right.room), (Some(a_id), Some(b_id)));
                }
                assert!(
                    a.blueprint
                        .rooms
                        .iter()
                        .any(|room| { room.id == GARAGE_ROOM && room.kind == RoomKind::Garage })
                );
                assert!(a.blueprint.cells.iter().any(|(cell, data)| {
                    data.wing == 1 && data.roof && cell.u != public.cell.u
                }));
                if arrangement == RanchArrangement::B {
                    assert!(a.blueprint.rooms.iter().any(|room| {
                        room.id == LIVING && room.cells.iter().all(|cell| cell.v >= 10)
                    }));
                    assert_eq!(public.connects.1, SpaceId::Room(HALL));
                }
            }
        }
    }

    #[test]
    fn ranch_parcel_keeps_driveway_and_public_approach_separate() {
        for side in [
            CardinalDir::North,
            CardinalDir::East,
            CardinalDir::South,
            CardinalDir::West,
        ] {
            let front = match side {
                CardinalDir::North => Position::new(115, 100),
                CardinalDir::East => Position::new(130, 115),
                CardinalDir::South => Position::new(115, 130),
                CardinalDir::West => Position::new(100, 115),
            };
            let frontage = Frontage {
                side,
                access: front,
            };
            let context = ParcelContext {
                seed: 42,
                bounds: lot(),
                frontage,
                zone: crate::world::blueprint::ZoneKind::Residential,
            };
            let plan = RanchParcel.plan(&context).unwrap();
            assert_eq!(
                plan.outdoor.paths,
                vec![(front, plan.structures[0].front_door)]
            );
            assert_eq!(plan.outdoor.driveways.len(), 3);
            assert_eq!(plan.outdoor.vehicle_slots.len(), 1);
            assert!(plan.outdoor.driveways.iter().all(|driveway| {
                driveway.0 != front && driveway.1 != plan.structures[0].front_door
            }));
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
