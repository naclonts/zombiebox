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
