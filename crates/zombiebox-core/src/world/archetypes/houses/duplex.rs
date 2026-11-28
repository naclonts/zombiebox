//! Two independently accessible dwellings sharing a solid party wall.
use crate::action::{BuildingStyle, DoorAxis, Material, Surface};
use crate::world::RoomKind;
use crate::world::blueprint::{
    BlueprintCell, BlueprintGrid, BlueprintKind, CompoundPlanner, DoorIntent, DoorKind, FitError,
    LocalCell, OutdoorLayer, ParcelContext, ParcelPlan, RoomMeta, SpaceId, parcel_id,
    place_blueprint_along_frontage,
};
use crate::world::{floorplan, furnish};

pub struct DuplexParcel;
const SALT: u64 = 0x4455_504c_4558;

impl CompoundPlanner for DuplexParcel {
    fn plan(&self, context: &ParcelContext) -> Result<ParcelPlan, FitError> {
        let id = parcel_id(context.seed, context.bounds);
        let (w, d) = floorplan::usable_dimensions(context.bounds, context.frontage);
        if w < 25 || d < 20 {
            return Err(FitError::InvalidLot {
                reason: "lot too small for duplex",
            });
        }
        let mut error = FitError::InvalidLot {
            reason: "no duplex arrangement fits",
        };
        for width in [29, 25] {
            if w < width {
                continue;
            }
            for index in floorplan::ordered_arrangements(context.seed, id, SALT) {
                let result = duplex_blueprint(width, index == 1).and_then(|mut bp| {
                    furnish::furnish(context.seed, &mut bp)?;
                    place_blueprint_along_frontage(
                        context.seed,
                        id,
                        context.bounds,
                        context.frontage,
                        bp,
                        if index == 0 { "Duplex A" } else { "Duplex B" },
                        if index == 0 { "duplex_a" } else { "duplex_b" },
                        BuildingStyle::WarmPutty,
                    )
                });
                match result {
                    Ok(structure) => {
                        let mut paths = Vec::new();
                        for door in structure
                            .blueprint
                            .doors
                            .iter()
                            .filter(|d| d.kind == DoorKind::Exterior)
                        {
                            paths.extend(floorplan::frontage_path(
                                context.frontage,
                                structure.transform.local_to_world(door.cell),
                            ));
                        }
                        return Ok(ParcelPlan {
                            id,
                            bounds: context.bounds,
                            frontage: context.frontage,
                            zone: context.zone,
                            outdoor: OutdoorLayer {
                                paths,
                                ..OutdoorLayer::default()
                            },
                            structures: vec![structure],
                        });
                    }
                    Err(e) => error = e,
                }
            }
        }
        Err(error)
    }
}

fn duplex_blueprint(width: i32, offset: bool) -> Result<BlueprintGrid, FitError> {
    use BlueprintKind::*;
    let mut bp = BlueprintGrid::new(1);
    let half = (width - 1) / 2;
    let partition_u = half / 2;
    for v in 0..19 {
        for u in 0..width {
            let unit = if u < half { 0 } else { 1 };
            let base = unit * 4 + 1;
            let local_u = if unit == 0 { u } else { u - half };
            let room = base
                + if v < 9 {
                    if local_u < partition_u { 0 } else { 1 }
                } else if local_u < partition_u {
                    2
                } else {
                    3
                };
            let boundary = u == 0 || u == width - 1 || v == 0 || v == 18;
            let partition = u == half || local_u == partition_u || v == 9;
            let mut c = if boundary {
                BlueprintCell::new(
                    if v == 0 && (u == 2 || u == width - 3) {
                        Window
                    } else {
                        ExteriorWall
                    },
                    Material::WoodLight,
                    Surface::WoodFloor,
                )
                .load_bearing()
            } else if partition {
                BlueprintCell::new(InteriorWall, Material::Drywall, Surface::WoodFloor).indoor()
            } else {
                BlueprintCell::new(
                    Floor,
                    Material::WoodLight,
                    if room == base + 1 || room == base + 3 {
                        Surface::KitchenTile
                    } else {
                        Surface::WoodFloor
                    },
                )
                .with_room(room)
                .indoor()
                .with_roof()
            };
            c.wing = unit as u8;
            bp.set_cell(LocalCell::new(u, v, 0), c);
        }
    }
    for unit in 0..2u16 {
        let x = i32::from(unit) * half;
        let base = unit * 4 + 1;
        let entry_u = if unit == 0 || offset {
            x + partition_u + 3
        } else {
            x + 3
        };
        // A pairs the doors by the party wall. B moves the right stoop outward.
        let entry_room = if unit == 0 || offset { base + 1 } else { base };
        if offset && unit == 1 {
            let mut stoop = BlueprintCell::new(Floor, Material::WoodLight, Surface::Porch);
            stoop.wing = unit as u8;
            bp.set_cell(LocalCell::new(entry_u, -1, 0), stoop);
        }
        bp.add_door(DoorIntent {
            cell: LocalCell::new(entry_u, 0, 0),
            tangent: DoorAxis::AlongX,
            kind: DoorKind::Exterior,
            connects: (SpaceId::Exterior, SpaceId::Room(entry_room)),
        });
        for (wall, tangent, range, rooms) in [
            (
                LocalCell::new(x + partition_u, 0, 0),
                DoorAxis::AlongY,
                2..=7,
                [base, base + 1],
            ),
            (
                LocalCell::new(0, 9, 0),
                DoorAxis::AlongX,
                x + 2..=x + partition_u - 2,
                [base, base + 2],
            ),
            (
                LocalCell::new(0, 9, 0),
                DoorAxis::AlongX,
                x + partition_u + 2..=x + half - 2,
                [base + 1, base + 3],
            ),
        ] {
            let cell = floorplan::shared_wall_opening(&bp, wall, tangent, range, rooms).ok_or(
                FitError::AccessFailure {
                    reason: "no duplex room opening",
                },
            )?;
            bp.add_door(DoorIntent {
                cell,
                tangent,
                kind: DoorKind::Interior,
                connects: (SpaceId::Room(rooms[0]), SpaceId::Room(rooms[1])),
            });
        }
        for (i, kind) in [
            RoomKind::Living,
            RoomKind::Kitchen,
            RoomKind::Bedroom,
            RoomKind::Bathroom,
        ]
        .into_iter()
        .enumerate()
        {
            let id = base + i as u16;
            let cells = bp
                .cells
                .iter()
                .filter_map(|(p, c)| (c.room == Some(id)).then_some(*p))
                .collect();
            bp.rooms.push(RoomMeta {
                id,
                kind,
                unit: Some(unit as u8),
                wing: unit as u8,
                floor: 0,
                cells,
            });
        }
    }
    for door in &bp.doors {
        let cell = bp
            .cells
            .get_mut(&door.cell)
            .expect("authored opening in a wall");
        cell.kind = Door {
            exterior: door.kind == DoorKind::Exterior,
            gate: false,
        };
        cell.material = Material::WoodDoor;
        cell.indoor = true;
        cell.load_bearing = false;
    }
    Ok(bp)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_duplex_arrangements_furnish() {
        for seed in 0..4 {
            for offset in [false, true] {
                let mut bp = duplex_blueprint(25, offset).unwrap();
                furnish::furnish(seed, &mut bp)
                    .unwrap_or_else(|e| panic!("seed {seed} offset {offset}: {e:?}"));
                bp.validate().unwrap();
            }
        }
    }
}
