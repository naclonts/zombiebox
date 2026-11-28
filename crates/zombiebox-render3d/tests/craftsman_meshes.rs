//! Render prototypes for the furnished Craftsman must exist and fit the
//! gameplay colliders they stand in for. A missing prototype is skipped
//! silently by the chunk builder, which hid the nightstand and TV stand.

use zombiebox_core::{PropId, PropState};
use zombiebox_render3d::PropMeshLibrary;

/// Props the Craftsman rule tables can place, plus the catalog additions.
const CRAFTSMAN_PROPS: &[PropId] = &[
    PropId::KitchenSink,
    PropId::KitchenStove,
    PropId::Fridge,
    PropId::KitchenCabinet,
    PropId::Sofa,
    PropId::TvStand,
    PropId::Television,
    PropId::BookcaseOpen,
    PropId::BedDouble,
    PropId::Nightstand,
    PropId::Bathtub,
    PropId::Toilet,
    PropId::BathroomSink,
    PropId::FirstAidKit,
];

#[test]
fn craftsman_props_have_render_prototypes() {
    let library = PropMeshLibrary::shared();
    for &id in CRAFTSMAN_PROPS {
        assert!(
            library.get(id, PropState::Closed).is_some(),
            "{id:?} has no render prototype"
        );
    }
}

#[test]
fn procedural_furniture_fits_its_collider() {
    let library = PropMeshLibrary::shared();
    const SLOP: f32 = 0.02;
    for &id in CRAFTSMAN_PROPS {
        let def = id.def();
        let (half_x, half_depth, height) = (
            def.collision_max.x,
            def.collision_max.y,
            def.collision_max.z,
        );
        let mesh = library.get(id, PropState::Closed).expect("prototype");
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for vertex in &mesh.vertices {
            for axis in 0..3 {
                lo[axis] = lo[axis].min(vertex.position[axis]);
                hi[axis] = hi[axis].max(vertex.position[axis]);
            }
        }
        assert!(
            lo[0] >= -half_x - SLOP && hi[0] <= half_x + SLOP,
            "{id:?} width {lo:?}..{hi:?}"
        );
        assert!(
            lo[2] >= -half_depth - SLOP && hi[2] <= half_depth + SLOP,
            "{id:?} depth {lo:?}..{hi:?}"
        );
        assert!(
            lo[1] >= -SLOP && hi[1] <= height + SLOP,
            "{id:?} height {lo:?}..{hi:?}"
        );
    }
}

#[test]
fn furniture_proportions_match_the_survivor_and_door() {
    use zombiebox_core::{DOOR_OPENING_HEIGHT, PLAYER_HEIGHT, WALL_HEIGHT};
    let fridge = PropId::Fridge.def().collision_max.z;
    let stove = PropId::KitchenStove.def().collision_max.z;
    let shelf = PropId::BookcaseOpen.def().collision_max.z;
    assert!(stove > PLAYER_HEIGHT * 0.4 && stove < PLAYER_HEIGHT * 0.65);
    assert!(fridge > PLAYER_HEIGHT && fridge < DOOR_OPENING_HEIGHT);
    assert!(shelf < WALL_HEIGHT && fridge < WALL_HEIGHT);
}
