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
    BlueprintAnchor, BlueprintAnchor as Anchor, BlueprintCell, BlueprintGrid, BlueprintKind,
    CompoundPlanner, DoorIntent, DoorKind, FitError, Frontage, GridRect, LocalCell,
    LocalPlacementKey, OutdoorLayer, ParcelContext, ParcelPlan, PlannedProp, PlannedSign, RoomMeta,
    RuleKey, SpaceId, StructureContext, StructureGenerator, VehicleSlot, parcel_id,
    place_blueprint_along_frontage,
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

/// Street planner that chooses Craftsman A or B from stable seed/parcel data.
/// The default county fixture continues to use `CraftsmanParcel` (A).
pub struct CraftsmanStreetParcel;

impl CompoundPlanner for CraftsmanStreetParcel {
    fn plan(&self, context: &ParcelContext) -> Result<ParcelPlan, FitError> {
        let arrangement =
            if crate::action::mix64(context.seed ^ parcel_id(context.seed, context.bounds).0) & 1
                == 0
            {
                CraftsmanArrangement::A
            } else {
                CraftsmanArrangement::B
            };
        let id = parcel_id(context.seed, context.bounds);
        let mut blueprint = craftsman_authored(
            &StructureContext {
                seed: context.seed,
                parcel: id,
                frontage: context.frontage,
            },
            arrangement,
        )?;
        furnish::furnish(context.seed, &mut blueprint)?;
        let (name, archetype, style) = match arrangement {
            CraftsmanArrangement::A => (
                "Craftsman bungalow A",
                "craftsman_bungalow_a",
                BuildingStyle::WarmPutty,
            ),
            CraftsmanArrangement::B => (
                "Craftsman bungalow B",
                "craftsman_bungalow_b",
                BuildingStyle::BlueClapboard,
            ),
        };
        let structure = place_blueprint_along_frontage(
            context.seed,
            id,
            context.bounds,
            context.frontage,
            blueprint,
            name,
            archetype,
            style,
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

/// Ranch A keeps living toward the street. Ranch B puts living at the rear,
/// bedrooms along the side hall, and the public entry at the kitchen/utility
/// end of the house.
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
        let mut blueprint = ranch_house(
            &StructureContext {
                seed: context.seed,
                parcel: id,
                frontage: context.frontage,
            },
            arrangement,
        )?;
        furnish::furnish(context.seed, &mut blueprint)?;
        let archetype = match arrangement {
            RanchArrangement::A => "ranch_l_wing_a",
            RanchArrangement::B => "ranch_l_wing_b",
        };
        let name = match arrangement {
            RanchArrangement::A => "L-wing ranch A",
            RanchArrangement::B => "L-wing ranch B",
        };
        let mut structure = place_blueprint_along_frontage(
            context.seed,
            id,
            context.bounds,
            context.frontage,
            blueprint,
            name,
            archetype,
            BuildingStyle::BirchSage,
        )?;
        // Prefer the dwelling entry for pedestrian routing when the garage
        // gate is another exterior opening on the same frontage.
        let public_door = ranch_public_door(arrangement);
        structure.front_door = structure.transform.local_to_world(public_door);
        let garage_door = ranch_garage_door(arrangement);
        let driveways = (garage_door.u - 1..=garage_door.u + 1)
            .map(|u| {
                (
                    structure.transform.local_to_world(LocalCell::new(u, -1, 0)),
                    structure.transform.local_to_world(LocalCell::new(u, 1, 0)),
                )
            })
            .collect();
        let slot_u = match arrangement {
            RanchArrangement::A => 15,
            RanchArrangement::B => -6,
        };
        let mut slot_min = crate::Position::new(i32::MAX, i32::MAX);
        for u in slot_u..slot_u + 2 {
            for v in 3..8 {
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
                vehicle_slots: vec![(
                    slot_min,
                    structure.transform.world_facing(PropFacing::North),
                )],
                ..OutdoorLayer::default()
            },
            structures: vec![structure],
        })
    }
}

const GARAGE_ROOM: u16 = 20;

fn ranch_garage_door(arrangement: RanchArrangement) -> LocalCell {
    let u = match arrangement {
        RanchArrangement::A => 17,
        RanchArrangement::B => -4,
    };
    LocalCell::new(u, 2, 0)
}

fn ranch_public_door(arrangement: RanchArrangement) -> LocalCell {
    match arrangement {
        RanchArrangement::A => LocalCell::new(6, 2, 0),
        RanchArrangement::B => LocalCell::new(7, 2, 0),
    }
}

fn ranch_house(
    context: &StructureContext,
    arrangement: RanchArrangement,
) -> Result<BlueprintGrid, FitError> {
    let mut bp = match arrangement {
        RanchArrangement::A => craftsman_bungalow(context)?,
        RanchArrangement::B => craftsman_bungalow_b(context)?,
    };
    let (garage_min_u, garage_max_u, connector_u, connector_v, garage_door_u) = match arrangement {
        RanchArrangement::A => (14, 21, 13, 8, 17),
        RanchArrangement::B => (-7, 0, 1, 8, -4),
    };
    let connector_side_u = match arrangement {
        RanchArrangement::A => garage_min_u,
        RanchArrangement::B => garage_max_u,
    };

    // Keep the union as a small explicit footprint mask. Its three disjoint
    // bounds also become three independent roof/cutaway wings at runtime.
    let mut footprint = FootprintMask::new();
    footprint.insert_rect(GridRect::new(
        crate::Position::new(1, 2),
        crate::Position::new(
            13,
            if arrangement == RanchArrangement::B {
                17
            } else {
                13
            },
        ),
    ));
    footprint.insert_rect(GridRect::new(
        crate::Position::new(4, 0),
        crate::Position::new(
            if arrangement == RanchArrangement::B {
                12
            } else {
                8
            },
            1,
        ),
    ));
    footprint.insert_rect(GridRect::new(
        crate::Position::new(garage_min_u, 2),
        crate::Position::new(garage_max_u, 9),
    ));

    // The porch deck carries its own shallow roof mass. The Craftsman's
    // authored blueprint remains untouched by this ranch-only silhouette.
    for local in [LocalCell::new(4, 0, 0), LocalCell::new(8, 1, 0)] {
        if let Some(cell) = bp.cells.get_mut(&local) {
            cell.wing = 2;
            cell.roof = true;
        }
    }
    for local in footprint.cells() {
        if local.u < 4
            || local.u
                > if arrangement == RanchArrangement::B {
                    12
                } else {
                    8
                }
            || local.v < 0
            || local.v > 1
        {
            continue;
        }
        if let Some(cell) = bp.cells.get_mut(&local) {
            cell.wing = 2;
            cell.roof = true;
        }
    }
    if arrangement == RanchArrangement::B {
        for u in 9..=12 {
            for v in 0..=1 {
                bp.set_cell(LocalCell::new(u, v, 0), {
                    let mut cell = BlueprintCell::new(
                        BlueprintKind::Floor,
                        Material::WoodLight,
                        Surface::Porch,
                    )
                    .with_roof();
                    cell.wing = 2;
                    cell
                });
            }
        }
        for u in [10, 12] {
            bp.set_cell(LocalCell::new(u, 0, 0), {
                let mut cell =
                    BlueprintCell::new(BlueprintKind::Pillar, Material::MetalPole, Surface::Porch)
                        .load_bearing()
                        .with_roof();
                cell.wing = 2;
                cell
            });
        }
    }

    // Garage shell is a distinct wing and distinct roof mass. Its front
    // opening and interior connector rotate with the same frontage transform.
    let garage = |u: i32, v: i32| {
        arrangement == RanchArrangement::A && (14..=21).contains(&u) && (2..=9).contains(&v)
            || arrangement == RanchArrangement::B && (-7..=0).contains(&u) && (2..=9).contains(&v)
    };
    for local in footprint.cells().filter(|cell| garage(cell.u, cell.v)) {
        let on_edge =
            local.u == garage_min_u || local.u == garage_max_u || local.v == 2 || local.v == 9;
        let garage_door = local.u == garage_door_u && local.v == 2;
        let cell = if garage_door {
            BlueprintCell::new(
                BlueprintKind::Door {
                    exterior: true,
                    gate: true,
                },
                Material::MetalPole,
                Surface::Porch,
            )
        } else if on_edge && !(local.u == connector_side_u && local.v == connector_v) {
            BlueprintCell::new(
                BlueprintKind::ExteriorWall,
                Material::WoodSiding,
                Surface::Porch,
            )
            .load_bearing()
        } else {
            BlueprintCell::new(
                BlueprintKind::Floor,
                Material::WoodLight,
                Surface::WoodFloor,
            )
            .with_room(GARAGE_ROOM)
            .indoor()
            .with_roof()
        };
        let mut cell = cell;
        cell.wing = 1;
        bp.set_cell(local, cell);
    }

    // Replace one main-house exterior wall cell with the indoor connection.
    let connector = LocalCell::new(connector_u, connector_v, 0);
    let mut connector_cell = BlueprintCell::new(
        BlueprintKind::Door {
            exterior: false,
            gate: false,
        },
        Material::WoodDoor,
        Surface::WoodFloor,
    )
    .indoor();
    connector_cell.wing = 0;
    bp.set_cell(connector, connector_cell);

    let garage_door = ranch_garage_door(arrangement);
    bp.add_door(DoorIntent {
        cell: garage_door,
        tangent: crate::action::DoorAxis::AlongX,
        kind: DoorKind::Gate,
        connects: (SpaceId::Exterior, SpaceId::Room(GARAGE_ROOM)),
    });
    bp.add_door(DoorIntent {
        cell: connector,
        tangent: crate::action::DoorAxis::AlongY,
        kind: DoorKind::Interior,
        connects: match arrangement {
            RanchArrangement::A => (SpaceId::Room(HALL), SpaceId::Room(GARAGE_ROOM)),
            RanchArrangement::B => (SpaceId::Room(HALL), SpaceId::Room(GARAGE_ROOM)),
        },
    });
    let mut garage_cells: Vec<LocalCell> = (garage_min_u + 1..garage_max_u)
        .flat_map(|u| (3..9).map(move |v| LocalCell::new(u, v, 0)))
        .collect();
    garage_cells.push(LocalCell::new(connector_side_u, connector_v, 0));
    bp.rooms.push(RoomMeta {
        id: GARAGE_ROOM,
        kind: RoomKind::Garage,
        unit: None,
        wing: 1,
        floor: 0,
        cells: garage_cells,
    });
    bp.vehicle_slots.push(VehicleSlot {
        anchor: Anchor::new(garage_min_u + 1, 3, 0),
        facing: PropFacing::North,
        length: 5,
        width: 2,
    });

    // Ensure each authored cell is represented by the common mask, catching
    // accidental disconnected additions while keeping ordering deterministic.
    let porch_width = if arrangement == RanchArrangement::B {
        9
    } else {
        5
    };
    let main_depth = if arrangement == RanchArrangement::B {
        16
    } else {
        12
    };
    if footprint.is_empty() || footprint.len() != 13 * main_depth + porch_width * 2 + 8 * 8 {
        return Err(FitError::ImpossibleRoom {
            reason: "ranch footprint composition is inconsistent",
        });
    }
    Ok(bp)
}

/// Deterministic furnished Ranch A/B preview entry point.
pub fn ranch_blueprint(
    seed: u64,
    frontage: Frontage,
    arrangement: RanchArrangement,
) -> Result<BlueprintGrid, FitError> {
    let mut bp = ranch_house(
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

/// Deterministic furnished Ranch A/B placed in an explicit lot.
pub fn ranch_structure(
    seed: u64,
    lot: GridRect,
    frontage: Frontage,
    arrangement: RanchArrangement,
) -> Result<crate::world::blueprint::PlacedStructure, FitError> {
    let parcel = parcel_id(seed, lot);
    let mut bp = ranch_house(
        &StructureContext {
            seed,
            parcel,
            frontage,
        },
        arrangement,
    )?;
    furnish::furnish(seed, &mut bp)?;
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
        BuildingStyle::BirchSage,
    )?;
    structure.front_door = structure
        .transform
        .local_to_world(ranch_public_door(arrangement));
    Ok(structure)
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
    // Extend the bedroom into the rear-left strip while preserving its side
    // hall entry and private dead-end circulation.
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
    // Former rear wall cells are reused for bedroom, kitchen, or the kitchen
    // to bathroom dividing wall.
    for u in 2..=12 {
        let local = LocalCell::new(u, 13, 0);
        if (8..=12).contains(&u) {
            let kind = if u == 11 {
                BlueprintKind::Door {
                    exterior: false,
                    gate: false,
                }
            } else {
                BlueprintKind::InteriorWall
            };
            bp.set_cell(
                local,
                BlueprintCell::new(
                    kind,
                    if u == 11 {
                        Material::WoodDoor
                    } else {
                        Material::Drywall
                    },
                    Surface::WoodFloor,
                )
                .indoor(),
            );
        } else if (2..=6).contains(&u) {
            bp.set_cell(
                local,
                BlueprintCell::new(BlueprintKind::Floor, Material::WoodLight, Surface::Carpet)
                    .with_room(BEDROOM)
                    .indoor()
                    .with_roof(),
            );
        } else if u == 7 {
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
    bp.add_door(DoorIntent {
        cell: LocalCell::new(11, 13, 0),
        tangent: crate::action::DoorAxis::AlongX,
        kind: DoorKind::Interior,
        connects: (SpaceId::Room(KITCHEN), SpaceId::Room(BATHROOM)),
    });

    if footprint.len() != 13 * 16 + 5 * 2 || !footprint.contains(LocalCell::new(7, 17, 0)) {
        return Err(FitError::ImpossibleRoom {
            reason: "Craftsman B footprint composition is inconsistent",
        });
    }

    Ok(bp)
}

fn craftsman_authored(
    context: &StructureContext,
    arrangement: CraftsmanArrangement,
) -> Result<BlueprintGrid, FitError> {
    match arrangement {
        CraftsmanArrangement::A => CraftsmanBungalow.generate(context),
        CraftsmanArrangement::B => craftsman_bungalow_b(context),
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
    let (name, archetype, style) = match arrangement {
        CraftsmanArrangement::A => (
            "Craftsman bungalow A",
            "craftsman_bungalow_a",
            BuildingStyle::WarmPutty,
        ),
        CraftsmanArrangement::B => (
            "Craftsman bungalow B",
            "craftsman_bungalow_b",
            BuildingStyle::BlueClapboard,
        ),
    };
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
                match side {
                    CardinalDir::North | CardinalDir::South => {
                        assert_eq!(
                            (a.bounds.width(), a.bounds.height()),
                            (
                                21,
                                if arrangement == RanchArrangement::A {
                                    14
                                } else {
                                    18
                                }
                            ),
                        );
                    }
                    CardinalDir::East | CardinalDir::West => {
                        assert_eq!(
                            (a.bounds.width(), a.bounds.height()),
                            (
                                if arrangement == RanchArrangement::A {
                                    14
                                } else {
                                    18
                                },
                                21
                            ),
                        );
                    }
                }
                let public = a
                    .blueprint
                    .doors
                    .iter()
                    .find(|door| door.kind == DoorKind::Exterior)
                    .unwrap();
                assert_eq!(a.transform.local_to_world(public.cell), a.front_door);
                assert_eq!(public.cell, ranch_public_door(arrangement));
                assert!(a.blueprint.doors.iter().any(|door| {
                    door.kind == DoorKind::Gate && door.cell == ranch_garage_door(arrangement)
                }));
                assert!(
                    a.blueprint
                        .vehicle_slots
                        .iter()
                        .any(|slot| { slot.width == 2 && slot.length == 5 })
                );
                let garage_connector = a
                    .blueprint
                    .doors
                    .iter()
                    .find(|door| door.connects == (SpaceId::Room(HALL), SpaceId::Room(GARAGE_ROOM)))
                    .unwrap();
                assert_eq!(
                    garage_connector.cell,
                    LocalCell::new(
                        if arrangement == RanchArrangement::A {
                            13
                        } else {
                            1
                        },
                        8,
                        0,
                    )
                );
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
