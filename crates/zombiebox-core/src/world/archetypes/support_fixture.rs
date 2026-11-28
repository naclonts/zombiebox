//! Compact stepped two-floor fixture for exercising the shared runtime path.
use super::super::RoomKind;
use super::super::blueprint::{
    BlueprintCell, BlueprintGrid, BlueprintKind, CompoundPlanner, DoorIntent, DoorKind, FitError,
    LocalCell, OutdoorLayer, ParcelContext, ParcelPlan, RoomMeta, SpaceId, StairLink, parcel_id,
    place_blueprint_in_lot,
};
use crate::action::{BuildingStyle, DoorAxis, Material, Surface};

pub struct SupportFixtureParcel;
impl CompoundPlanner for SupportFixtureParcel {
    fn plan(&self, context: &ParcelContext) -> Result<ParcelPlan, FitError> {
        let id = parcel_id(context.seed, context.bounds);
        let structure = place_blueprint_in_lot(
            context.seed,
            id,
            context.bounds,
            context.frontage,
            support_fixture_blueprint(),
            "Support fixture",
            "support_fixture",
            BuildingStyle::WarmPutty,
        )?;
        let outdoor = OutdoorLayer {
            paths: vec![(context.frontage.access, structure.front_door)],
            ..OutdoorLayer::default()
        };
        Ok(ParcelPlan {
            id,
            bounds: context.bounds,
            frontage: context.frontage,
            zone: context.zone,
            structures: vec![structure],
            outdoor,
        })
    }
}

pub fn support_fixture_blueprint() -> BlueprintGrid {
    use BlueprintKind::*;
    let mut bp = BlueprintGrid::new(2);
    // Two rectangular wings meet at u=5/6. The western wing supports an
    // upper floor; the eastern wing carries its own lower roof.
    for floor in 0..2 {
        let width = if floor == 0 { 10 } else { 6 };
        for v in 0..7 {
            for u in 0..width {
                let boundary = u == 0 || u == width - 1 || v == 0 || v == 6;
                let mut cell = if boundary {
                    BlueprintCell::new(ExteriorWall, Material::WoodSiding, Surface::WoodFloor)
                        .load_bearing()
                } else {
                    BlueprintCell::new(Floor, Material::WoodLight, Surface::WoodFloor)
                        .with_room(if floor == 0 { 1 } else { 2 })
                        .indoor()
                        .with_roof()
                };
                cell.wing = if u < 6 { 0 } else { 1 };
                bp.set_cell(LocalCell::new(u, v, floor), cell);
            }
        }
    }
    // Open the shared wall to connect the two ground-floor wings.
    for v in 1..6 {
        let mut cell = BlueprintCell::new(Floor, Material::WoodLight, Surface::WoodFloor)
            .with_room(1)
            .indoor()
            .with_roof();
        cell.wing = 0;
        bp.set_cell(LocalCell::new(5, v, 0), cell);
    }
    bp.set_cell(
        LocalCell::new(2, 0, 0),
        BlueprintCell::new(
            Door {
                exterior: true,
                gate: false,
            },
            Material::WoodDoor,
            Surface::WoodFloor,
        ),
    );
    bp.add_door(DoorIntent {
        cell: LocalCell::new(2, 0, 0),
        tangent: DoorAxis::AlongX,
        kind: DoorKind::Exterior,
        connects: (SpaceId::Exterior, SpaceId::Room(1)),
    });
    for floor in 0u8..2 {
        let mut cell = BlueprintCell::new(
            Stairs {
                up: floor == 0,
                down: floor == 1,
            },
            Material::WoodLight,
            Surface::WoodFloor,
        )
        .with_room(u16::from(floor) + 1)
        .indoor()
        .with_roof();
        cell.stair_dir = if floor == 0 { (1, 0) } else { (-1, 0) };
        bp.set_cell(LocalCell::new(3, 3, floor), cell);
        bp.rooms.push(RoomMeta {
            id: u16::from(floor) + 1,
            kind: RoomKind::Hall,
            unit: None,
            wing: 0,
            floor,
            cells: bp
                .cells
                .iter()
                .filter_map(|(pos, cell)| {
                    (pos.floor == floor && cell.room == Some(u16::from(floor) + 1)).then_some(*pos)
                })
                .collect(),
        });
    }
    bp.stairs.push(StairLink {
        lower: LocalCell::new(3, 3, 0),
        upper: LocalCell::new(3, 3, 1),
        lower_landing: LocalCell::new(4, 3, 0),
        upper_landing: LocalCell::new(2, 3, 1),
    });
    bp
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_has_valid_supported_aligned_stairs() {
        let bp = support_fixture_blueprint();
        bp.validate().unwrap();
        assert_eq!(bp.stairs.len(), 1);
        assert_eq!(bp.cell(bp.stairs[0].lower).unwrap().stair_dir, (1, 0));
    }
}
