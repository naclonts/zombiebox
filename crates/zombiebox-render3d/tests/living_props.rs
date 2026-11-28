use zombiebox_core::{
    ActionGame, ActionInput, DamageKind, InteractPrompt, Position, PropAnchor, PropFacing, PropId,
    PropState, Tile3D, TileKind,
};
use zombiebox_render3d::{CacheSyncMode, ChunkCoord, SceneConfig, SceneManager};
use zombiebox_render3d::mesh::props::build_chunk_props_mesh;

#[test]
fn searching_a_container_preserves_its_state_and_keeps_chunks_clean() {
    let mut game = ActionGame::county(42);
    let start = game.player_tile();
    let (at, approach) = (start.y - 5..=start.y + 5)
        .flat_map(|y| (start.x - 5..=start.x + 5).map(move |x| (x, y)))
        .find_map(|(x, y)| {
            let at = Tile3D::new(x, y, 0);
            let approach = Tile3D::new(x, y + 1, 0);
            let empty_outdoors = |tile: Tile3D| {
                game.tile_render_info(tile.pos(), 0)
                    .is_some_and(|info| info.kind == TileKind::Empty && !info.indoor)
            };
            (empty_outdoors(at)
                && empty_outdoors(approach)
                && !game.caches().iter().any(|cache| {
                    cache.pos.floor == 0 && cache.pos.center().distance(approach.center()) < 1.5
                }))
            .then_some((at, approach))
        })
        .expect("outdoor fixture site");
    let id = PropId::CardboardBox;
    let def = id.def();
    let prop = PropAnchor {
        anchor_uid: 42,
        tile: at,
        id,
        span: def.span,
        attachment: def.attachment,
        facing: PropFacing::South,
        ..Default::default()
    };
    assert!(game.add_prop_fixture(prop));
    assert_eq!(game.teleport(approach), Some(approach));

    let mut scene = SceneManager::new();
    let initial = scene.prepare_at_focus(&game, approach.pos(), 0.0);
    assert!(
        initial
            .dynamic
            .props
            .iter()
            .any(|p| { p.anchor_uid == prop.anchor_uid && p.state == PropState::Closed })
    );
    assert_eq!(game.hud().interact_prompt, InteractPrompt::Search);
    let before = game.hud().pistol_reserve;
    game.step(ActionInput {
        interact: true,
        ..Default::default()
    });
    assert_eq!(
        game.hud().pistol_reserve - before,
        def.loot.unwrap().pistol_ammo
    );

    let searched = scene.prepare_at_focus(&game, approach.pos(), 0.0);
    assert!(searched.dirty_chunks.is_empty());
    assert!(
        searched
            .dynamic
            .props
            .iter()
            .any(|p| { p.anchor_uid == prop.anchor_uid && p.state == PropState::Opened })
    );
    game.step(ActionInput {
        interact: true,
        ..Default::default()
    });
    assert_eq!(
        game.hud().pistol_reserve - before,
        def.loot.unwrap().pistol_ammo
    );

    let far = game
        .world()
        .landmarks()
        .iter()
        .map(|landmark| landmark.entrance)
        .find(|p| p.x.abs_diff(at.x).max(p.y.abs_diff(at.y)) >= 128)
        .unwrap();
    let far = game.teleport(Tile3D::from_pos(far, 0)).unwrap();
    let departed = scene.prepare_at_focus(&game, Position::new(far.x, far.y), 0.0);
    assert!(
        !departed
            .dynamic
            .props
            .iter()
            .any(|p| p.anchor_uid == prop.anchor_uid)
    );
    assert_eq!(game.teleport(approach), Some(approach));
    let revisited = scene.prepare_at_focus(&game, approach.pos(), 0.0);
    assert!(
        revisited
            .dynamic
            .props
            .iter()
            .any(|p| { p.anchor_uid == prop.anchor_uid && p.state == PropState::Opened })
    );
}

#[test]
fn event_driven_static_props_dirty_only_their_owner_chunk() {
    let mut game = ActionGame::county(42);
    let start = game.player_tile();
    let at = (start.y - 24..=start.y + 24)
        .flat_map(|y| (start.x - 24..=start.x + 24).map(move |x| (x, y)))
        .find(|&x_y| {
            let (x, y) = x_y;
            x.rem_euclid(16) == 15
                && [Tile3D::new(x, y, 0), Tile3D::new(x + 1, y, 0)]
                    .into_iter()
                    .all(|tile| {
                        game.tile_render_info(tile.pos(), 0)
                            .is_some_and(|info| info.kind == TileKind::Empty && !info.indoor)
                    })
        })
        .map(|(x, y)| Tile3D::new(x, y, 0))
        .expect("seam-crossing outdoor fixture site");
    let id = PropId::Table;
    let def = id.def();
    let prop = PropAnchor {
        anchor_uid: 0x5ea1,
        tile: at,
        id,
        span: def.span,
        attachment: def.attachment,
        facing: PropFacing::South,
        ..Default::default()
    };
    let mut scene = SceneManager::with_config(SceneConfig {
        active_chunk_radius: 2,
        sync_mode: CacheSyncMode::EventDriven,
        ..SceneConfig::default()
    });
    let _ = scene.prepare_at_focus(&game, start.pos(), 0.0);
    let owner = ChunkCoord::from_pos(at.pos());

    assert!(game.add_prop_fixture(prop));
    let inserted = scene.prepare_at_focus(&game, start.pos(), 0.0);
    assert_eq!(
        inserted
            .dirty_chunks
            .iter()
            .map(|chunk| chunk.coord)
            .collect::<Vec<_>>(),
        vec![owner],
    );
    assert!(
        inserted.dirty_chunks[0]
            .snapshot
            .static_props
            .iter()
            .any(|view| view.anchor_uid == prop.anchor_uid)
    );

    assert!(game.damage_tile(at, DamageKind::RocketBlast).is_some());
    let destroyed = scene.prepare_at_focus(&game, start.pos(), 0.0);
    assert_eq!(
        destroyed
            .dirty_chunks
            .iter()
            .map(|chunk| chunk.coord)
            .collect::<Vec<_>>(),
        vec![owner],
    );
    assert!(destroyed.dirty_chunks[0].snapshot.static_props.is_empty());
}

fn furnished_chunk(prop_count: usize) -> (ActionGame, Position) {
    let mut game = ActionGame::county(42);
    let start = game.player_tile();
    let mut placed = 0;
    'tiles: for y in start.y - 48..=start.y + 48 {
        for x in start.x - 48..=start.x + 48 {
            let at = Tile3D::new(x, y, 0);
            if ChunkCoord::from_pos(at.pos()) != ChunkCoord::from_pos(start.pos())
                || x.rem_euclid(16) > 13
                || ![at, Tile3D::new(x + 1, y, 0)].into_iter().all(|tile| {
                    game.tile_render_info(tile.pos(), 0).is_some_and(|info| {
                        info.kind == TileKind::Empty && !info.indoor
                    })
                })
            {
                continue;
            }
            let id = PropId::Table;
            let def = id.def();
            if game.add_prop_fixture(PropAnchor {
                anchor_uid: 0x7000 + placed as u64,
                tile: at,
                id,
                span: def.span,
                attachment: def.attachment,
                facing: PropFacing::South,
                ..Default::default()
            }) {
                placed += 1;
                if placed == prop_count {
                    break 'tiles;
                }
            }
        }
    }
    assert_eq!(placed, prop_count, "fixture chunk has enough clear floor");
    (game, start.pos())
}

#[test]
fn static_prop_mesh_benchmark_reports_modest_and_dense_fixture_costs() {
    for (label, count) in [("modest", 6), ("dense", 32)] {
        let (game, focus) = furnished_chunk(count);
        let mut scene = SceneManager::with_config(SceneConfig {
            active_chunk_radius: 0,
            sync_mode: CacheSyncMode::EventDriven,
            ..SceneConfig::default()
        });
        let frame = scene.prepare_at_focus(&game, focus, 0.0);
        let snapshot = &frame.dirty_chunks[0].snapshot;
        let mut samples = Vec::with_capacity(101);
        for _ in 0..101 {
            let started = std::time::Instant::now();
            let mesh = build_chunk_props_mesh(snapshot);
            assert!(!mesh.vertices.is_empty());
            samples.push(started.elapsed().as_micros());
        }
        samples.sort_unstable();
        println!(
            "static-prop {label}: {count} fixtures, {} tris, p50={}us p95={}us max={}us",
            build_chunk_props_mesh(snapshot).indices.len() / 3,
            samples[50],
            samples[95],
            samples[100],
        );
    }
}
