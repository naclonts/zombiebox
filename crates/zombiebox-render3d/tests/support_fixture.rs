use zombiebox_core::{ActionGame, CountyFixture, LocalCell, Tile3D, World};
use zombiebox_render3d::SceneManager;

#[test]
fn occupied_fixture_wing_controls_only_its_roof_cutaway() {
    let world = World::with_fixture(42, CountyFixture::SupportLot);
    let structure = &world.fixture_plan().expect("fixture").structures[0];
    let tower = structure.transform.local_to_world(LocalCell::new(2, 3, 0));
    let wing = structure.transform.local_to_world(LocalCell::new(8, 3, 0));
    let uid = structure.id.0;
    let mut game = ActionGame::county_with_world(world);
    for (pos, floor) in [(tower, 0), (tower, 1), (wing, 0)] {
        assert_eq!(
            game.teleport(Tile3D::from_pos(pos, floor)),
            Some(Tile3D::from_pos(pos, floor))
        );
        let building = *game.buildings().iter().find(|b| b.uid.0 == uid).unwrap();
        let occupied = building.cutaway_slot_at(pos, floor);
        let other = (0..building.effective_wing_count())
            .map(|i| building.cutaway_slot(i))
            .find(|slot| *slot != occupied)
            .unwrap();
        let mut scene = SceneManager::new();
        let frame = scene.prepare_at_focus(&game, pos, 0.0);
        assert_eq!(frame.dynamic.active_floor, floor);
        let state = |slot| {
            frame
                .dynamic
                .building_cutaways
                .iter()
                .find(|state| state.building_id == slot)
                .unwrap()
        };
        assert!(state(occupied).player_inside && state(occupied).wants_cutaway);
        assert_eq!(state(occupied).dither_opacity_u8, 0);
        assert!(!state(other).player_inside && !state(other).wants_cutaway);
        assert_eq!(state(other).dither_opacity_u8, 255);
    }
}

#[test]
fn split_level_tall_and_low_wings_cut_away_independently() {
    for key in ["split_level_a", "split_level_b"] {
        let (seed, world) = (0..64)
            .map(|seed| {
                (
                    seed,
                    World::with_fixture(seed, CountyFixture::SplitLevelLot),
                )
            })
            .find(|(_, world)| {
                world
                    .fixture_plan()
                    .is_some_and(|plan| plan.structures[0].archetype == key)
            })
            .unwrap_or_else(|| panic!("no split-level seed selected {key}"));
        let plan = world.fixture_plan().unwrap();
        let structure = plan.structures[0].clone();
        let uid = structure.id.0;
        let mut game = ActionGame::county_with_world(world);
        let building = *game
            .buildings()
            .iter()
            .find(|building| building.uid.0 == uid)
            .unwrap();
        let mut samples = Vec::new();
        for room in &structure.blueprint.rooms {
            let cell = room.cells[0];
            let pos = structure.transform.local_to_world(cell);
            let wing = building.wing_index_at(pos, room.floor);
            let floors = building.wing_at(wing).floors;
            if !samples.iter().any(|(seen, _)| *seen == wing) {
                samples.push((wing, (pos, room.floor, floors)));
            }
        }
        let tall = samples
            .iter()
            .find(|(_, (_, _, floors))| *floors >= 2)
            .expect("tall rear/side wing");
        let low = samples
            .iter()
            .find(|(_, (_, _, floors))| *floors == 1)
            .expect("low street wing");
        for (selected, other) in [(tall, low), (low, tall)] {
            let (pos, floor, _) = selected.1;
            assert_eq!(
                game.teleport(Tile3D::from_pos(pos, floor)),
                Some(Tile3D::from_pos(pos, floor))
            );
            let mut scene = SceneManager::new();
            let frame = scene.prepare_at_focus(&game, pos, 0.0);
            assert_eq!(frame.dynamic.active_floor, floor, "seed {seed}, {key}");
            let state = |wing| {
                frame
                    .dynamic
                    .building_cutaways
                    .iter()
                    .find(|cutaway| cutaway.building_id == building.cutaway_slot(wing))
                    .unwrap()
            };
            assert!(state(selected.0).player_inside && state(selected.0).wants_cutaway);
            assert_eq!(state(selected.0).dither_opacity_u8, 0);
            assert!(!state(other.0).player_inside && !state(other.0).wants_cutaway);
            assert_eq!(state(other.0).dither_opacity_u8, 255);
        }
    }
}
