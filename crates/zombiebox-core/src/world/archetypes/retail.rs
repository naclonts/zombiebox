//! Authored open-span retail fixtures.  The grocery intentionally stays on
//! the common blueprint/materializer path so it can become a normal town
//! archetype without a renderer or action-runtime fork.

use crate::action::{
    BuildingStyle, CellAttachment, DoorAxis, Material, PropFacing, PropId, ReadableId, Surface,
    Vec3,
};
use crate::world::RoomKind;
use crate::world::blueprint::{
    BlueprintAnchor, BlueprintCell, BlueprintGrid, BlueprintKind, CompoundPlanner, DoorIntent,
    DoorKind, FitError, LocalCell, LocalPlacementKey, OutdoorLayer, ParcelContext, ParcelPlan,
    PlannedProp, PlannedSign, RoomMeta, SpaceId, parcel_id, place_blueprint_in_lot,
};

/// A compact one-storey supermarket.  Its clearance is authored first: the
/// two three-tile cross aisles, two-tile shelf aisles, entrances and the rear
/// service door are all omitted from fixture placement before props are added.
pub struct GroceryParcel;

impl CompoundPlanner for GroceryParcel {
    fn plan(&self, context: &ParcelContext) -> Result<ParcelPlan, FitError> {
        let id = parcel_id(context.seed, context.bounds);
        let structure = place_blueprint_in_lot(
            context.seed,
            id,
            context.bounds,
            context.frontage,
            grocery_blueprint(),
            "Open-span grocery",
            "grocery_open_span",
            BuildingStyle::DowntownStorefront,
        )?;
        let rear_door = structure
            .transform
            .local_to_world(LocalCell::new(14, 21, 0));
        let rear_apron = structure
            .transform
            .local_to_world(LocalCell::new(14, 22, 0));
        let rear_service = structure
            .transform
            .local_to_world(LocalCell::new(14, 23, 0));
        let side_service = structure
            .transform
            .local_to_world(LocalCell::new(21, 23, 0));
        let side_front = structure
            .transform
            .local_to_world(LocalCell::new(21, -2, 0));
        let front_apron_start = structure.transform.local_to_world(LocalCell::new(2, -2, 0));
        let front_apron_end = structure
            .transform
            .local_to_world(LocalCell::new(17, -2, 0));
        let front_apron_inner_start = structure.transform.local_to_world(LocalCell::new(2, -1, 0));
        let front_apron_inner_end = structure
            .transform
            .local_to_world(LocalCell::new(17, -1, 0));
        Ok(ParcelPlan {
            id,
            bounds: context.bounds,
            frontage: context.frontage,
            zone: context.zone,
            outdoor: OutdoorLayer {
                // The public approach deliberately enters the vestibule rather
                // than the cart bay.  The rear line remains independent.
                paths: vec![(context.frontage.access, structure.front_door)],
                driveways: vec![
                    (front_apron_start, front_apron_end),
                    (front_apron_inner_start, front_apron_inner_end),
                    (rear_door, rear_apron),
                    (rear_apron, rear_service),
                    (rear_service, side_service),
                    (side_service, side_front),
                    (side_front, front_apron_end),
                ],
                ..OutdoorLayer::default()
            },
            structures: vec![structure],
        })
    }
}

pub fn grocery_blueprint() -> BlueprintGrid {
    use BlueprintKind::*;
    const W: i32 = 20;
    const D: i32 = 22;
    const VESTIBULE: u16 = 1;
    const SHOP: u16 = 2;
    const WC: u16 = 3;
    const STOCK: u16 = 4;
    let mut bp = BlueprintGrid::new(1);

    for v in 0..D {
        for u in 0..W {
            let boundary = u == 0 || u == W - 1 || v == 0 || v == D - 1;
            let room = if v <= 3 {
                VESTIBULE
            } else if v <= 15 {
                SHOP
            } else if u <= 5 {
                WC
            } else {
                STOCK
            };
            let mut cell = if boundary {
                BlueprintCell::new(
                    if (v == 0 && (u == 4 || u == 15)) || (u == 0 && v == 9) {
                        Window
                    } else {
                        ExteriorWall
                    },
                    Material::BrickMasonry,
                    Surface::KitchenTile,
                )
                .load_bearing()
            } else {
                BlueprintCell::new(Floor, Material::WoodLight, Surface::KitchenTile)
                    .with_room(room)
                    .indoor()
                    .with_roof()
            };
            cell.wing = 0;
            bp.set_cell(LocalCell::new(u, v, 0), cell);
        }
    }

    // Checkout lanes organize the transition; the vestibule remains visually
    // open to the sales floor from the public entrance.
    // Rear rooms: WC at the west end, stockroom to the east.  These walls are
    // also the shell for the independent loading door.
    for u in 1..W - 1 {
        bp.set_cell(
            LocalCell::new(u, 16, 0),
            BlueprintCell::new(InteriorWall, Material::Drywall, Surface::KitchenTile).indoor(),
        );
    }
    for v in 17..D - 1 {
        bp.set_cell(
            LocalCell::new(6, v, 0),
            BlueprintCell::new(InteriorWall, Material::Drywall, Surface::KitchenTile).indoor(),
        );
    }

    let doors = [
        (
            LocalCell::new(10, 0, 0),
            DoorKind::Exterior,
            (SpaceId::Exterior, SpaceId::Room(VESTIBULE)),
        ),
        (
            LocalCell::new(3, 16, 0),
            DoorKind::Interior,
            (SpaceId::Room(SHOP), SpaceId::Room(WC)),
        ),
        (
            LocalCell::new(13, 16, 0),
            DoorKind::Interior,
            (SpaceId::Room(SHOP), SpaceId::Room(STOCK)),
        ),
        (
            LocalCell::new(14, D - 1, 0),
            DoorKind::Exterior,
            (SpaceId::Exterior, SpaceId::Room(STOCK)),
        ),
    ];
    for (cell, kind, connects) in doors {
        bp.set_cell(
            cell,
            BlueprintCell::new(
                Door {
                    exterior: kind == DoorKind::Exterior,
                    gate: false,
                },
                Material::WoodDoor,
                Surface::KitchenTile,
            )
            .indoor(),
        );
        bp.add_door(DoorIntent {
            cell,
            tangent: DoorAxis::AlongX,
            kind,
            connects,
        });
    }

    // Six-tile pillar pitch.  The column cells replace shelf bays, leaving the
    // cross-aisles (v=8..10 and v=13..15) entirely open.
    for (u, v) in [(4, 6), (10, 6), (16, 6), (4, 12), (10, 12), (16, 12)] {
        bp.set_cell(
            LocalCell::new(u, v, 0),
            BlueprintCell::new(Pillar, Material::MetalPole, Surface::KitchenTile).load_bearing(),
        );
    }

    for (id, kind, cells) in [
        (VESTIBULE, RoomKind::Lobby, room_cells(&bp, VESTIBULE)),
        (SHOP, RoomKind::Shop, room_cells(&bp, SHOP)),
        (WC, RoomKind::Bathroom, room_cells(&bp, WC)),
        (STOCK, RoomKind::Storage, room_cells(&bp, STOCK)),
    ] {
        bp.rooms.push(RoomMeta {
            id,
            kind,
            unit: None,
            wing: 0,
            floor: 0,
            cells,
        });
    }

    let mut key = 1;
    // Checkout lanes leave a three-tile public route through u=9..11.  Tills
    // share their lane anchor but render on the compatible countertop layer.
    for u in [2, 6, 13, 17] {
        push_prop(
            &mut bp,
            &mut key,
            PropId::CheckoutLane,
            u,
            5,
            SHOP,
            PropFacing::South,
        );
        // The lane is two tiles long; lift and center the till on it so its
        // CounterTop attachment has physical support in the shared renderer.
        let till = PlannedProp {
            key: LocalPlacementKey(key),
            prop: PropId::CheckoutTill,
            anchor: BlueprintAnchor::new(u, 5, 0),
            facing: PropFacing::South,
            room: Some(SHOP),
            host: None,
            offset_m: Vec3::new(0.0, 0.5, PropId::CheckoutLane.def().collision_max.z),
            loot: None,
        };
        key += 1;
        bp.props.push(till);
    }
    // Gondola bays are deliberately omitted at each pillar and each cross
    // aisle.  Their two-tile spans leave at least two clear floor tiles between
    // every adjacent run.
    for (v, bays) in [(7, &[1, 6, 12, 17][..]), (12, &[1, 6, 12, 17][..])] {
        for u in bays {
            push_prop(
                &mut bp,
                &mut key,
                PropId::GondolaShelf,
                *u,
                v,
                SHOP,
                PropFacing::North,
            );
        }
    }
    push_prop(
        &mut bp,
        &mut key,
        PropId::Toilet,
        2,
        18,
        WC,
        PropFacing::North,
    );
    push_prop(
        &mut bp,
        &mut key,
        PropId::BathroomSink,
        4,
        18,
        WC,
        PropFacing::North,
    );
    for (u, v) in [(8, 18), (10, 18), (12, 18), (15, 18), (17, 18)] {
        push_prop(
            &mut bp,
            &mut key,
            PropId::CardboardBox,
            u,
            v,
            STOCK,
            PropFacing::North,
        );
    }
    push_prop(
        &mut bp,
        &mut key,
        PropId::ShoppingCart,
        2,
        2,
        VESTIBULE,
        PropFacing::South,
    );
    push_prop(
        &mut bp,
        &mut key,
        PropId::ShoppingCart,
        3,
        2,
        VESTIBULE,
        PropFacing::South,
    );

    bp.signs.extend([
        PlannedSign {
            key: LocalPlacementKey(0x7001),
            anchor: BlueprintAnchor::new(9, 0, 0),
            prop: PropId::GrocerySign,
            attachment: CellAttachment::WallMount,
            offset_m: Vec3::new(0.0, -0.52, 1.8),
            facing: PropFacing::North,
            readable: None,
            host: None,
        },
        PlannedSign {
            key: LocalPlacementKey(0x7002),
            anchor: BlueprintAnchor::new(5, 16, 0),
            prop: PropId::RestroomSign,
            attachment: CellAttachment::WallMount,
            offset_m: Vec3::new(0.0, -0.52, 1.5),
            facing: PropFacing::North,
            readable: None,
            host: None,
        },
        PlannedSign {
            key: LocalPlacementKey(0x7003),
            anchor: BlueprintAnchor::new(10, 12, 0),
            prop: PropId::AisleSign,
            attachment: CellAttachment::WallMount,
            offset_m: Vec3::new(0.52, 0.0, 0.95),
            facing: PropFacing::North,
            readable: None,
            host: None,
        },
        PlannedSign {
            key: LocalPlacementKey(0x7004),
            anchor: BlueprintAnchor::new(15, 16, 0),
            prop: PropId::Note,
            attachment: CellAttachment::WallMount,
            offset_m: Vec3::new(0.0, -0.52, 1.45),
            facing: PropFacing::North,
            readable: Some(ReadableId::GroceryNotice),
            host: None,
        },
    ]);
    bp
}

fn room_cells(bp: &BlueprintGrid, room: u16) -> Vec<LocalCell> {
    bp.cells
        .iter()
        .filter_map(|(&cell, data)| (data.room == Some(room)).then_some(cell))
        .collect()
}

fn push_prop(
    bp: &mut BlueprintGrid,
    key: &mut u32,
    prop: PropId,
    u: i32,
    v: i32,
    room: u16,
    facing: PropFacing,
) {
    bp.props.push(PlannedProp {
        key: LocalPlacementKey(*key),
        prop,
        anchor: BlueprintAnchor::new(u, v, 0),
        facing,
        room: Some(room),
        host: None,
        offset_m: Vec3::ZERO,
        loot: None,
    });
    *key += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Position;
    use crate::world::blueprint::{CardinalDir, Frontage, GridRect, ZoneKind};

    #[test]
    fn grocery_reserves_open_aisles_and_supported_sales_floor() {
        let bp = grocery_blueprint();
        bp.validate().unwrap();
        for v in 8..=10 {
            for u in 1..19 {
                assert!(bp.cell(LocalCell::new(u, v, 0)).unwrap().kind.walkable());
            }
        }
        assert_eq!(
            bp.rooms.iter().find(|room| room.id == 2).unwrap().kind,
            RoomKind::Shop
        );
        assert_eq!(
            bp.props
                .iter()
                .filter(|p| p.prop == PropId::CheckoutTill)
                .count(),
            4
        );
        for cell in [
            LocalCell::new(4, 6, 0),
            LocalCell::new(10, 6, 0),
            LocalCell::new(16, 6, 0),
            LocalCell::new(4, 12, 0),
            LocalCell::new(10, 12, 0),
            LocalCell::new(16, 12, 0),
        ] {
            let pillar = bp.cell(cell).unwrap();
            assert_eq!(pillar.kind, BlueprintKind::Pillar);
            assert!(pillar.load_bearing);
        }
        assert_eq!(
            PropId::GondolaShelf.def().cover,
            crate::action::PropCover::HighBlocker
        );
    }

    #[test]
    fn grocery_plan_replays_with_stable_placement_ids_and_routes() {
        let context = ParcelContext {
            seed: 42,
            bounds: GridRect::new(Position::new(80, 80), Position::new(111, 111)),
            frontage: Frontage {
                side: CardinalDir::North,
                access: Position::new(96, 80),
            },
            zone: ZoneKind::Commercial,
        };
        let first = GroceryParcel.plan(&context).unwrap();
        let second = GroceryParcel.plan(&context).unwrap();
        assert_eq!(first, second);
        assert!(!first.outdoor.paths.is_empty() && !first.outdoor.driveways.is_empty());
        let props = first.structures[0].props_world();
        assert!(
            props
                .iter()
                .any(|prop| prop.id == PropId::CardboardBox && prop.loot.is_none())
        );
        assert_eq!(
            props
                .iter()
                .filter(|prop| prop.id == PropId::GondolaShelf)
                .count(),
            8
        );
    }

    #[test]
    fn grocery_floor_footprints_do_not_overlap_and_leave_cross_aisles_clear() {
        let structure = GroceryParcel
            .plan(&ParcelContext {
                seed: 42,
                bounds: GridRect::new(Position::new(80, 80), Position::new(111, 111)),
                frontage: Frontage {
                    side: CardinalDir::North,
                    access: Position::new(96, 80),
                },
                zone: ZoneKind::Commercial,
            })
            .unwrap()
            .structures
            .remove(0);
        let mut occupied = std::collections::BTreeSet::new();
        for prop in structure
            .props_world()
            .into_iter()
            .filter(|prop| prop.attachment == CellAttachment::Floor)
        {
            for tile in prop.span.covered_tiles(prop.tile) {
                assert!(
                    occupied.insert(tile),
                    "overlap at {tile:?} from {:?}",
                    prop.id
                );
            }
        }
        for v in 8..=10 {
            for u in 1..19 {
                assert!(
                    structure
                        .blueprint
                        .cell(LocalCell::new(u, v, 0))
                        .unwrap()
                        .kind
                        .walkable()
                );
            }
        }
    }

    #[test]
    fn grocery_live_shelf_occludes_while_adjacent_aisle_stays_clear() {
        let world = crate::World::with_fixture(42, crate::CountyFixture::GroceryLot);
        let structure = &world.fixture_plan().expect("grocery fixture").structures[0];
        let local = |u, v| {
            crate::Tile3D::from_pos(
                structure.transform.local_to_world(LocalCell::new(u, v, 0)),
                0,
            )
        };
        let pillar = local(4, 6);
        let blocked_origin = local(1, 5);
        let blocked_target = local(1, 9);
        let clear_origin = local(8, 9);
        let clear_target = local(12, 9);
        let mut game = crate::action::ActionGame::county_with_world(world);
        assert_eq!(
            game.tile_render_info(pillar.pos(), 0).unwrap().kind,
            crate::action::TileKind::Pillar
        );
        assert_eq!(game.teleport(blocked_origin), Some(blocked_origin));
        let sight_z = game.player_body().position.z + 0.60;
        assert!(
            !game.has_line_of_sight_to_point(blocked_target.center().extend(sight_z)),
            "high gondola shelf must occlude the direct sales-floor ray"
        );
        assert_eq!(game.teleport(clear_origin), Some(clear_origin));
        assert!(
            game.has_line_of_sight_to_point(clear_target.center().extend(sight_z)),
            "adjacent reserved aisle must remain clear"
        );
    }

    #[test]
    fn grocery_fixture_fits_a_real_road_front_city_lot() {
        for seed in [0, 1, 7, 42, 63] {
            let world = crate::World::with_fixture(seed, crate::CountyFixture::GroceryLot);
            let plan = world
                .fixture_plan()
                .unwrap_or_else(|| panic!("seed {seed} has grocery lot"));
            assert_eq!(plan.zone, ZoneKind::Commercial);
            assert!(
                plan.outdoor
                    .paths
                    .iter()
                    .any(|(_, end)| *end == plan.structures[0].front_door)
            );
        }
    }
}
