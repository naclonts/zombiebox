use std::collections::VecDeque;

use zombiebox_core::{
    BuildingKind, CHUNK_SIZE, CHUNKS_PER_SIDE, District, Ground, LandmarkKind, Position,
    WORLD_SIZE, World,
};

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
    for kind in [
        BuildingKind::Hospital,
        BuildingKind::ArmyBase,
        BuildingKind::Duplex,
    ] {
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
            if matches!(l.kind, LandmarkKind::Hospital | LandmarkKind::ArmyBase) {
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
