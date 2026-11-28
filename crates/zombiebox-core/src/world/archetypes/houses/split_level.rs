//! Stepped house with a lower roof and a supported two-storey wing.
use super::super::colonial::{Arrangement, colonial_blueprint};
use crate::action::{BuildingStyle, DoorAxis, Material, Surface};
use crate::world::blueprint::{
    BlueprintCell, BlueprintKind, CompoundPlanner, DoorIntent, DoorKind, FitError, LocalCell,
    OutdoorLayer, ParcelContext, ParcelPlan, SpaceId, parcel_id, place_blueprint_along_frontage,
};
use crate::world::{RoomKind, floorplan, furnish};

pub struct SplitLevelParcel;
impl CompoundPlanner for SplitLevelParcel {
    fn plan(&self, context: &ParcelContext) -> Result<ParcelPlan, FitError> {
        let id = parcel_id(context.seed, context.bounds);
        let (w, d) = floorplan::usable_dimensions(context.bounds, context.frontage);
        let mut error = FitError::InvalidLot {
            reason: "lot too small for split-level",
        };
        for size in [17, 15] {
            if w < size || d < size {
                continue;
            }
            for index in floorplan::ordered_arrangements(context.seed, id, 0x5350_4c49_54) {
                let result = split_blueprint(size, index == 1).and_then(|mut bp| {
                    furnish::furnish(context.seed, &mut bp)?;
                    place_blueprint_along_frontage(
                        context.seed,
                        id,
                        context.bounds,
                        context.frontage,
                        bp,
                        if index == 0 {
                            "Split-level A"
                        } else {
                            "Split-level B"
                        },
                        if index == 0 {
                            "split_level_a"
                        } else {
                            "split_level_b"
                        },
                        BuildingStyle::BlueClapboard,
                    )
                });
                match result {
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
                    Err(e) => error = e,
                }
            }
        }
        Err(error)
    }
}
fn split_blueprint(
    size: i32,
    side: bool,
) -> Result<crate::world::blueprint::BlueprintGrid, FitError> {
    let left = (size - 5) / 2;
    let mut bp = colonial_blueprint(
        size,
        size,
        if side {
            Arrangement::SideStair
        } else {
            Arrangement::CenterHall
        },
        left,
    )?;
    // The upper hall and right-hand bedroom/bath remain over the taller mass.
    // In A the front is low, so repurpose the rear left room as a bedroom.
    let split = size / 2;
    bp.cells
        .retain(|p, _| p.floor == 0 || if side { p.u >= left } else { p.v >= split });
    bp.doors.retain(|door| door.cell.floor == 0);
    bp.rooms
        .retain(|room| room.floor == 0 || if side { room.id != 5 } else { room.id != 6 });
    if !side {
        // Seal the new front wall of the raised rear wing, including the hall.
        for u in 0..size {
            bp.cells.insert(
                LocalCell::new(u, split, 1),
                BlueprintCell::new(
                    BlueprintKind::ExteriorWall,
                    Material::WoodLight,
                    Surface::WoodFloor,
                )
                .load_bearing(),
            );
        }
    }
    for room in &mut bp.rooms {
        room.wing = if room.floor == 1 { 1 } else { 0 };
        room.cells
            .retain(|p| bp.cells.get(p).is_some_and(|c| c.room == Some(room.id)));
    }
    for (p, c) in &mut bp.cells {
        if p.floor == 1 {
            c.wing = 1;
        } else if (side && p.u >= left) || (!side && p.v >= split) {
            c.wing = 1;
        }
    }
    // Room masks cannot straddle independently cut away roof wings. Split
    // common areas at the mass boundary while preserving their open floor.
    let mut extra = Vec::new();
    for room in &mut bp.rooms {
        if room.floor != 0 {
            continue;
        }
        let counts = [0u8, 1].map(|wing| {
            room.cells
                .iter()
                .filter(|p| bp.cells[p].wing == wing)
                .count()
        });
        let primary = if !side && room.kind == RoomKind::Living && counts[0] > 0 {
            0
        } else {
            u8::from(counts[1] > counts[0])
        };
        room.wing = primary;
        if counts[1 - primary as usize] == 0 {
            continue;
        }
        let new_id = room.id + 100;
        let mut other = room.clone();
        other.id = new_id;
        other.kind = RoomKind::Hall;
        other.wing = 1 - primary;
        other.cells = room
            .cells
            .iter()
            .copied()
            .filter(|p| bp.cells[p].wing == other.wing)
            .collect();
        room.cells.retain(|p| bp.cells[p].wing == primary);
        for p in &other.cells {
            bp.cells.get_mut(p).unwrap().room = Some(new_id);
        }
        extra.push(other);
    }
    bp.rooms.extend(extra);
    // Derive door ownership from its physical approaches after splitting masks.
    for door in &mut bp.doors {
        let normal = door.tangent.normal();
        let rooms: Vec<_> = [-1, 1]
            .into_iter()
            .filter_map(|sign| {
                bp.cells
                    .get(&LocalCell::new(
                        door.cell.u + normal.0 * sign,
                        door.cell.v + normal.1 * sign,
                        door.cell.floor,
                    ))
                    .and_then(|c| c.room)
                    .map(SpaceId::Room)
            })
            .collect();
        if door.kind == DoorKind::Exterior {
            if let Some(room) = rooms.first() {
                door.connects = (SpaceId::Exterior, *room);
            }
        } else if rooms.len() == 2 {
            door.connects = (rooms[0], rooms[1]);
        }
    }
    let specs: Vec<(i32, std::ops::RangeInclusive<i32>, [u16; 2])> = if side {
        vec![
            (left + 4, 2..=split - 2, [4, 6]),
            (left + 4, split + 2..=size - 3, [4, 7]),
        ]
    } else {
        vec![
            (left, split + 2..=size - 3, [5, 4]),
            (left + 4, split + 2..=size - 3, [4, 7]),
        ]
    };
    for (u, range, rooms) in specs {
        let cell = floorplan::shared_wall_opening(
            &bp,
            LocalCell::new(u, 0, 1),
            DoorAxis::AlongY,
            range,
            rooms,
        )
        .ok_or(FitError::AccessFailure {
            reason: "no split-level upper opening",
        })?;
        bp.add_door(DoorIntent {
            cell,
            tangent: DoorAxis::AlongY,
            kind: DoorKind::Interior,
            connects: (SpaceId::Room(rooms[0]), SpaceId::Room(rooms[1])),
        });
        let mut opening = BlueprintCell::new(
            BlueprintKind::Door {
                exterior: false,
                gate: false,
            },
            Material::WoodDoor,
            Surface::WoodFloor,
        )
        .indoor();
        opening.wing = 1;
        bp.cells.insert(cell, opening);
    }
    Ok(bp)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_split_arrangements_furnish() {
        for seed in 0..4 {
            for side in [false, true] {
                for size in [15, 17] {
                    let mut bp = split_blueprint(size, side)
                        .unwrap_or_else(|e| panic!("size {size} side {side}: {e:?}"));
                    furnish::furnish(seed, &mut bp)
                        .unwrap_or_else(|e| panic!("seed {seed} side {side}: {e:?}"));
                    bp.validate().unwrap();
                    if !side {
                        let living = bp
                            .rooms
                            .iter()
                            .find(|r| r.kind == RoomKind::Living)
                            .unwrap();
                        assert_eq!(living.wing, 0);
                        assert!(bp.props.iter().any(|p| p.room == Some(living.id)));
                    }
                }
            }
        }
    }
}
