use std::collections::VecDeque;

use zombiebox_core::{
    BuildingKind, CHUNK_SIZE, CHUNKS_PER_SIDE, District, GENERATOR_VERSION, Ground, LandmarkKind,
    Position, WORLD_SIZE, World, ZoneKind, county_survey_json,
};

#[test]
fn county_survey_identifies_the_v4_cutover_and_preserves_u64_seeds() {
    let survey = county_survey_json(u64::MAX);
    assert_eq!(GENERATOR_VERSION, 4);
    assert!(survey.starts_with(&format!(
        "{{\"seed\":\"{}\",\"version\":{GENERATOR_VERSION},\"size\":512,",
        u64::MAX
    )));
}

#[test]
fn chunks_and_sites_replay_in_reverse_visit_order() {
    for seed in [42, u64::MAX] {
        let a = World::new(seed);
        let b = World::new(seed);
        assert_eq!(a.landmarks(), b.landmarks());
        let chunks: Vec<_> = (0..CHUNKS_PER_SIDE)
            .flat_map(|cy| (0..CHUNKS_PER_SIDE).map(move |cx| (cx, cy)))
            .map(|(cx, cy)| (cx, cy, a.chunk(cx, cy).unwrap()))
            .collect();
        for (cx, cy, expected) in chunks.into_iter().rev() {
            assert_eq!(Some(expected), b.chunk(cx, cy));
            for (y, row) in expected.iter().enumerate() {
                for (x, t) in row.iter().enumerate() {
                    assert_eq!(
                        b.tile_at(Position::new(
                            cx * CHUNK_SIZE + x as i32,
                            cy * CHUNK_SIZE + y as i32
                        )),
                        Some(*t)
                    );
                }
            }
        }
        assert_eq!(a.sites(), b.sites());
        assert_eq!(a.fixture_plans(), b.fixture_plans());
        assert!(a.chunk(i32::MAX, 0).is_none());
        assert!(a.tile_at(Position::new(0, WORLD_SIZE)).is_none());
        assert!(a.tile_at(Position::new(-1, 0)).is_none());
    }
}

// In each layer, compare actual generated content. A new seed that only
// relabelled sites or scattered a few caches would fail this test.
#[test]
fn seeds_change_terrain_regions_roads_and_local_plans() {
    let a = World::new(42);
    let b = World::new((1 << 42) + 42);
    assert_ne!(a.landmarks(), b.landmarks());
    let different = |layer: fn(zombiebox_core::Tile) -> u8| {
        (0..WORLD_SIZE).step_by(4).any(|y| {
            (0..WORLD_SIZE).step_by(4).any(|x| {
                let p = Position::new(x, y);
                layer(a.tile_at(p).unwrap()) != layer(b.tile_at(p).unwrap())
            })
        })
    };
    assert!(different(|t| t.elevation));
    assert!(different(|t| t.district as u8));
    assert!(different(|t| t.ground.is_road() as u8));
    assert!(different(|t| t.water.is_some() as u8));
    let local_plan = |world: &World, kind| {
        let b = world
            .sites()
            .unwrap()
            .buildings
            .iter()
            .find(|b| b.kind == kind)
            .unwrap();
        let mut shape = Vec::new();
        for f in 0..b.floors {
            for y in b.min.y..=b.max.y {
                for x in b.min.x..=b.max.x {
                    shape.push(b.feature_at(Position::new(x, y), f).unwrap());
                }
            }
        }
        (b.max.x - b.min.x, b.max.y - b.min.y, shape)
    };
    for kind in [BuildingKind::Hospital, BuildingKind::Duplex] {
        assert_ne!(local_plan(&a, kind), local_plan(&b, kind), "{kind:?}");
    }
}

#[test]
fn finished_counties_connect_spawn_building_doors_and_landmarks() {
    for seed in [0, 1, 7, 42, 641, 4567, 93513, 1 << 32, u64::MAX] {
        let world = World::new(seed);
        let spawn = world.wake_position();
        let idx = |p: Position| (p.y * WORLD_SIZE + p.x) as usize;
        let mut seen = vec![false; (WORLD_SIZE * WORLD_SIZE) as usize];
        let mut queue = VecDeque::from([spawn]);
        assert_eq!(world.district_at(spawn), Some(District::Residential));
        seen[idx(spawn)] = true;
        while let Some(p) = queue.pop_front() {
            for (dx, dy) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
                let q = Position::new(p.x + dx, p.y + dy);
                if world.tile_at(q).is_some_and(|t| t.walkable()) && !seen[idx(q)] {
                    seen[idx(q)] = true;
                    queue.push_back(q);
                }
            }
        }
        for l in world.landmarks() {
            assert!(seen[idx(l.entrance)], "seed {seed}, {:?}", l.kind);
            if l.kind == LandmarkKind::Hospital {
                assert_eq!(world.tile_at(l.entrance).unwrap().ground, Ground::Road);
            }
        }
        let sites = world.sites().unwrap();
        for b in &sites.buildings {
            assert!(
                seen[idx(b.entry)],
                "seed {seed}, {:?} at {:?}",
                b.kind,
                b.entry
            );
            assert_eq!(world.district_at(b.entry), Some(b.district));
            for y in b.min.y..=b.max.y {
                for x in b.min.x..=b.max.x {
                    let t = world.tile_at(Position::new(x, y)).unwrap();
                    assert!(t.water.is_none() && !t.ground.is_road());
                }
            }
        }
        for l in &sites.loot {
            if l.floor == 0 {
                assert!(world.tile_at(l.position).unwrap().cache && seen[idx(l.position)]);
            }
        }
    }
}

#[test]
fn cross_chunk_buildings_share_floors_and_each_spawn_has_one_owner() {
    use std::collections::HashSet;
    let world = World::new(42);
    let mut loot = HashSet::new();
    let mut infected = HashSet::new();
    for cy in 0..CHUNKS_PER_SIDE {
        for cx in 0..CHUNKS_PER_SIDE {
            let c = world.chunk_sites(cx, cy).unwrap();
            for site in c.loot {
                assert_eq!(
                    (site.position.x / CHUNK_SIZE, site.position.y / CHUNK_SIZE),
                    (cx, cy)
                );
                assert!(world.walkable_at(site.position, site.floor));
                assert!(loot.insert((site.position, site.floor)));
            }
            for site in c.infected {
                assert!(world.walkable_at(site.position, site.floor));
                assert!(infected.insert((site.position, site.floor)));
            }
            for b in c.buildings {
                assert!(b.min.x / CHUNK_SIZE <= cx && b.max.x / CHUNK_SIZE >= cx);
                assert!(b.min.y / CHUNK_SIZE <= cy && b.max.y / CHUNK_SIZE >= cy);
            }
        }
    }
    let sites = world.sites().unwrap();
    assert_eq!(sites.loot.len(), loot.len());
    assert_eq!(sites.infected.len(), infected.len());
    assert!(!world.walkable_at(world.wake_position(), 1));
}

#[test]
fn counties_materialize_the_full_roster_on_connected_approaches() {
    for seed in [
        0,
        1,
        2,
        3,
        42,
        42 + (1 << 42),
        10950764489276683678,
        17904098877923710524,
        u64::MAX,
    ] {
        verify_county_roster(seed);
    }
}

#[test]
#[ignore = "extended deterministic geography audit"]
fn audit_county_geography_across_seeds() {
    for seed in (0..64).chain((1_u64..=256).map(|n| n.wrapping_mul(0x9e37_79b9_7f4a_7c15))) {
        verify_county_roster(seed);
    }
}

fn verify_county_roster(seed: u64) {
    use BuildingKind::*;
    let world = World::new(seed);
    let plans = world.fixture_plans();
    let houses: Vec<_> = plans
        .iter()
        .filter(|plan| plan.zone == ZoneKind::Residential)
        .collect();
    assert_eq!(houses.len(), 5, "seed {seed} furnished houses");
    for (i, plan) in plans.iter().enumerate() {
        for other in &plans[i + 1..] {
            assert!(
                !plan.bounds.intersects(other.bounds),
                "seed {seed} overlapping authored parcels {:?} and {:?}",
                plan.id,
                other.id
            );
        }
    }
    let buildings = &world.sites().unwrap().buildings;
    for kind in [
        Hospital, Precinct, Cabin, Storefront, Office, Apartments, Bungalow, Duplex, SplitLevel,
    ] {
        assert!(
            buildings.iter().any(|b| b.kind == kind),
            "seed {seed} missing {kind:?}"
        );
    }
    assert!(buildings.iter().all(|building| building.kind != ArmyBase));
    assert!(
        world
            .landmarks()
            .iter()
            .all(|landmark| landmark.kind != LandmarkKind::ArmyBase)
    );
    assert_eq!(
        buildings.iter().filter(|b| b.kind == Cabin).count(),
        world
            .landmarks()
            .iter()
            .filter(|l| l.kind == LandmarkKind::Cabin)
            .count()
    );
    let mut seen = std::collections::HashMap::from([(world.wake_position(), 0)]);
    let mut queue = VecDeque::from([world.wake_position()]);
    while let Some(p) = queue.pop_front() {
        let next_distance = seen[&p] + 1;
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let next = Position::new(p.x + dx, p.y + dy);
            if world.tile_at(next).is_some_and(|t| t.ground.is_road()) {
                if let std::collections::hash_map::Entry::Vacant(entry) = seen.entry(next) {
                    entry.insert(next_distance);
                    queue.push_back(next);
                }
            }
        }
    }
    for house in houses {
        let entrance = house.frontage.access;
        let (dx, dy) = house.frontage.side.inward();
        let approach = Position::new(entrance.x - dx, entrance.y - dy);
        let distance = *seen.get(&approach).expect("connected house approach");
        let family = house.structures[0].archetype;
        if ["craftsman_", "ranch_", "colonial_"]
            .iter()
            .any(|s| family.starts_with(s))
        {
            let path: u32 = house
                .outdoor
                .paths
                .iter()
                .map(|(a, b)| a.x.abs_diff(b.x) + a.y.abs_diff(b.y))
                .sum();
            assert!(
                distance + 1 + path <= 90,
                "seed {seed} {family} requires {} walking tiles",
                distance + 1 + path
            );
        }
    }
    for key in ["grocery_open_span", "gas_mart", "gas_canopy"] {
        let mut matching = plans.iter().filter_map(|plan| {
            plan.structures
                .iter()
                .find(|s| s.archetype == key)
                .map(|s| (plan, s))
        });
        let (plan, structure) = matching
            .next()
            .unwrap_or_else(|| panic!("seed {seed} missing {key}"));
        assert!(matching.next().is_none(), "seed {seed} duplicated {key}");
        let access = plan.frontage.access;
        let (dx, dy) = plan.frontage.side.inward();
        assert!(
            seen.contains_key(&access)
                || seen.contains_key(&Position::new(access.x - dx, access.y - dy)),
            "seed {seed}: {key} has no connected road approach at {access:?}"
        );
        for y in structure.bounds.min.y..=structure.bounds.max.y {
            for x in structure.bounds.min.x..=structure.bounds.max.x {
                let tile = world.tile_at(Position::new(x, y)).unwrap();
                assert!(
                    tile.water.is_none(),
                    "seed {seed}: wet {key} footprint ({x},{y})"
                );
            }
        }
        if key != "gas_canopy" {
            assert!(
                plan.outdoor
                    .paths
                    .iter()
                    .any(|&(_, end)| end == structure.front_door),
                "seed {seed}: no public path to the {key} front door"
            );
        }
    }
    for landmark in world.landmarks() {
        assert!(
            seen.contains_key(&landmark.entrance),
            "seed {seed} disconnected {landmark:?}"
        );
    }
}
