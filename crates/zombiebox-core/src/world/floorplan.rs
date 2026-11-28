//! Bounded, deterministic residential fitting primitives.
use std::ops::RangeInclusive;

use crate::Position;
use crate::action::DoorAxis;

use super::blueprint::{
    BlueprintGrid, BlueprintKind, CardinalDir, Frontage, GridRect, LocalCell, ParcelId, RoomId,
};

/// Local frontage width and depth available to `place_blueprint_along_frontage`.
pub fn usable_dimensions(lot: GridRect, frontage: Frontage) -> (i32, i32) {
    match frontage.side {
        CardinalDir::North | CardinalDir::South => (lot.width() - 2, lot.height() - 1),
        CardinalDir::East | CardinalDir::West => (lot.height() - 2, lot.width() - 1),
    }
}

/// Follow the lot boundary to line up with the entrance before walking inward.
pub(super) fn frontage_path(frontage: Frontage, door: Position) -> Vec<(Position, Position)> {
    let access = frontage.access;
    let bend = match frontage.side {
        CardinalDir::North | CardinalDir::South => Position::new(door.x, access.y),
        CardinalDir::East | CardinalDir::West => Position::new(access.x, door.y),
    };
    [(access, bend), (bend, door)]
        .into_iter()
        .filter(|(start, end)| start != end)
        .collect()
}

/// A fixed-size, independently salted preference order; never depends on loot.
pub fn ordered_arrangements(seed: u64, parcel: ParcelId, salt: u64) -> [usize; 2] {
    if crate::action::mix64(seed ^ parcel.0 ^ salt) & 1 == 0 {
        [0, 1]
    } else {
        [1, 0]
    }
}

/// Inclusive candidate positions, closest to the midpoint first, with stable ties.
pub fn midpoint_candidates(min: i32, max: i32) -> Vec<i32> {
    if min > max {
        return Vec::new();
    }
    let middle = min + (max - min) / 2;
    let mut values: Vec<_> = (min..=max).collect();
    values.sort_by_key(|&x| ((x - middle).abs(), x));
    values
}

/// Find a one-tile opening with solid jambs and approaches in the named rooms.
pub(super) fn shared_wall_opening(
    blueprint: &BlueprintGrid,
    wall: LocalCell,
    tangent: DoorAxis,
    segment: RangeInclusive<i32>,
    rooms: [RoomId; 2],
) -> Option<LocalCell> {
    let offset = |cell: LocalCell, (u, v): (i32, i32), amount: i32| {
        LocalCell::new(cell.u + u * amount, cell.v + v * amount, cell.floor)
    };
    let along = tangent.tangent();
    let across = tangent.normal();
    midpoint_candidates(*segment.start(), *segment.end())
        .into_iter()
        .map(|at| offset(wall, along, at))
        .find(|&cell| {
            blueprint
                .cell(cell)
                .is_some_and(|center| center.kind == BlueprintKind::InteriorWall)
                && [-1, 1].into_iter().all(|side| {
                    blueprint
                        .cell(offset(cell, along, side))
                        .is_some_and(|jamb| jamb.kind.is_wall())
                })
                && [-1, 1].into_iter().zip(rooms).all(|(side, room)| {
                    blueprint
                        .cell(offset(cell, across, side))
                        .is_some_and(|approach| {
                            approach.kind == BlueprintKind::Floor && approach.room == Some(room)
                        })
                })
        })
}
