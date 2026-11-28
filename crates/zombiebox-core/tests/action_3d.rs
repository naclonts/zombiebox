use std::collections::{HashMap, VecDeque};

use zombiebox_core::{
    ActionEvent, ActionGame, ActionInput, BuildingKind, ColliderKind, FLOOR_H, FloorTransition,
    HitSurface, InteractPrompt, LightKind, MaterialCategory, MonsterKind, MotionState,
    PLAYER_SHAPE, Position, ProjectileKind, QueryFilter, Ray3, SpatialWorld, SupportId, Surface,
    Tile3D, TileKind, Vec2, Vec3, WORLD_SIZE, Weapon,
};

fn pos_center(p: Position) -> Vec2 {
    Vec2::new(p.x as f32, p.y as f32)
}

/// Finds a BFS path on `floor` allowing open walkable tiles and doors (opening closed doors on approach).
fn find_tile_path_on_floor(game: &ActionGame, start: Tile3D, goal: Tile3D) -> Option<Vec<Tile3D>> {
    assert_eq!(start.floor, goal.floor);
    let fl = start.floor;
    let mut visited: HashMap<Position, Position> = HashMap::new();
    let mut queue = VecDeque::new();
    visited.insert(start.pos(), start.pos());
    queue.push_back(start.pos());

    while let Some(cur) = queue.pop_front() {
        if cur == goal.pos() {
            let mut path = Vec::new();
            let mut p = cur;
            while p != start.pos() {
                path.push(Tile3D::from_pos(p, fl));
                p = visited[&p];
            }
            path.reverse();
            return Some(path);
        }
        for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            let next = Position::new(cur.x + dx, cur.y + dy);
            if visited.contains_key(&next) {
                continue;
            }
            let Some(info) = game.tile_render_info(next, fl) else {
                continue;
            };
            let passable = next == goal.pos()
                || matches!(
                    info.kind,
                    TileKind::Empty
                        | TileKind::Rubble
                        | TileKind::StairsUp
                        | TileKind::BrokenStreetlight
                        | TileKind::Door { .. }
                );
            let has_floor = info.surface != Surface::Air;
            if passable && has_floor {
                visited.insert(next, cur);
                queue.push_back(next);
            }
        }
    }
    None
}

/// Walks the player along a tile path on a single floor, opening any closed doors along the way.
fn walk_path_on_floor(game: &mut ActionGame, goal: Tile3D, max_ticks_per_tile: usize) -> bool {
    let start = game.player_tile();
    if start == goal {
        return true;
    }
    let Some(path) = find_tile_path_on_floor(game, start, goal) else {
        return false;
    };
    for wp in path {
        for _ in 0..max_ticks_per_tile {
            if game.player_pose().floor != wp.floor {
                return wp == goal;
            }
            let pos = game.player_pose().position;
            let delta = wp.center() - pos;
            if delta.length() <= 0.22 {
                break;
            }
            let interact = game.hud().interact_prompt == InteractPrompt::OpenDoor;
            game.step(ActionInput {
                movement: delta.normalized(),
                interact,
                ..ActionInput::default()
            });
        }
    }
    game.player_tile() == goal || game.player_pose().position.distance(goal.center()) <= 0.45
}

#[test]
fn birch_street_flat_ground_3d_invariants() {
    let game = ActionGame::residential(42);
    let p_body = game.player_body();
    let p_state = game.player_render_state();

    assert!(
        p_body.position.z.abs() < 1e-6,
        "Birch Street player must spawn on flat z=0.0, got {}",
        p_body.position.z
    );
    assert!((p_state.body.position.z - p_body.position.z).abs() < 1e-6);
    assert!((p_state.elevation - p_body.position.z).abs() < 1e-6);

    let monsters = game.monster_views();
    assert!(monsters.iter().any(|m| m.floor == 0));
    assert!(monsters.iter().any(|m| m.floor == 1));
    for m in &monsters {
        let expected_z = m.floor as f32 * FLOOR_H;
        assert!(
            (m.body.position.z - expected_z).abs() < 1e-5,
            "monster {:?} on F{} had z={}, expected {}",
            m.id,
            m.floor,
            m.body.position.z,
            expected_z
        );
        assert!((m.elevation - m.body.position.z).abs() < 1e-5);
        assert!(m.eye_pos.z > m.body.position.z);
    }

    let caches = game.caches();
    assert!(caches.iter().any(|c| c.pos.floor == 0));
    assert!(caches.iter().any(|c| c.pos.floor == 1));
    for c in &caches {
        let expected_z = c.pos.floor as f32 * FLOOR_H;
        assert!(
            (c.world_pos.z - expected_z).abs() < 1e-5,
            "cache {} on F{} had world_pos.z={}, expected {}",
            c.id,
            c.pos.floor,
            c.world_pos.z,
            expected_z
        );
    }

    let lights = game.light_sources();
    assert!(!lights.is_empty());
    for l in &lights {
        let expected_z = match l.kind {
            LightKind::Streetlight => 1.65,
            LightKind::Porch => l.floor as f32 * FLOOR_H + 1.10,
            LightKind::Cache => l.floor as f32 * FLOOR_H + 0.35,
        };
        assert!(
            (l.position.z - expected_z).abs() < 1e-4,
            "light {:?} on F{} had z={}, expected {}",
            l.kind,
            l.floor,
            l.position.z,
            expected_z
        );
    }
}

#[test]
fn county_hill_los_and_terrain_hitscan_rocket_interception() {
    let mut game = ActionGame::county(42);
    let world = game.world().clone();
    let sites = world.sites().expect("county sites exist");

    let in_any_building = |p: Position| -> bool {
        sites
            .buildings
            .iter()
            .any(|b| p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y)
    };
    let is_clear_outdoor = |p: Position| -> bool {
        world.tile_at(p).is_some_and(|t| {
            t.ground.walkable()
                && t.water.is_none()
                && matches!(
                    t.feature,
                    zombiebox_core::Feature::Empty | zombiebox_core::Feature::Path
                )
        }) && !in_any_building(p)
    };
    let elev = |p: Position| -> f32 { world.elevation_at(pos_center(p)).unwrap_or(0.0) };

    // 1. Acquire starter Pistol in starter neighborhood.
    let pistol_cache = game
        .caches()
        .iter()
        .find(|c| c.loot.weapon == Some(Weapon::Pistol))
        .copied()
        .expect("starter pistol cache");
    assert!(game.teleport(pistol_cache.pos).is_some());
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    game.step(ActionInput {
        select_weapon: Some(Weapon::Pistol),
        ..ActionInput::default()
    });
    assert_eq!(game.hud().equipped, Some(Weapon::Pistol));

    // 2. Acquire RocketLauncher from ArmyBase, GunStore, or Police.
    let mut got_rocket = game.inventory().has_weapon(Weapon::RocketLauncher);
    if !got_rocket {
        for b in &sites.buildings {
            if matches!(b.kind, BuildingKind::ArmyBase | BuildingKind::Precinct) {
                let _ = game.teleport(Tile3D::from_pos(b.entry, 0));
                if let Some(rc) = game
                    .caches()
                    .iter()
                    .find(|c| !c.opened && c.loot.weapon == Some(Weapon::RocketLauncher))
                    .copied()
                {
                    assert!(game.teleport(rc.pos).is_some());
                    game.step(ActionInput {
                        interact: true,
                        ..ActionInput::default()
                    });
                    got_rocket = game.inventory().has_weapon(Weapon::RocketLauncher);
                    if got_rocket {
                        break;
                    }
                }
            }
        }
    }
    assert!(got_rocket, "expected RocketLauncher cache in County");

    // 3. Find a ridge pair occluded by terrain AND an open slope pair on the same hillside.
    let mut ridge_pair: Option<(Position, Position)> = None;
    'outer_ridge: for (dx, dy) in [(1_i32, 0_i32), (0, 1)] {
        for span in [8_i32, 12, 16, 22] {
            for y in (32..WORLD_SIZE - 32).step_by(2) {
                for x in (32..WORLD_SIZE - 32).step_by(2) {
                    let a = Position::new(x, y);
                    let b = Position::new(x + dx * span, y + dy * span);
                    if !is_clear_outdoor(a) || !is_clear_outdoor(b) {
                        continue;
                    }
                    if game.teleport(Tile3D::from_pos(a, 0)).is_none() {
                        continue;
                    }
                    let mid_clear = (0..=span).all(|k| {
                        let p = Position::new(x + dx * k, y + dy * k);
                        let mut cols = Vec::new();
                        game.spatial().colliders_in(
                            zombiebox_core::Aabb3::new(
                                Vec3::new(p.x as f32 - 0.5, p.y as f32 - 0.5, -5.0),
                                Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.5, 40.0),
                            ),
                            &mut cols,
                        );
                        world.tile_at(p).is_some_and(|t| {
                            t.water.is_none() && t.feature == zombiebox_core::Feature::Empty
                        }) && cols.is_empty()
                            && !in_any_building(p)
                    });
                    if !mid_clear {
                        continue;
                    }
                    let eye_za = PLAYER_SHAPE.eye_at(pos_center(a).extend(elev(a))).z;
                    let eye_zb = PLAYER_SHAPE.eye_at(pos_center(b).extend(elev(b))).z;
                    let has_crest = (1..span).any(|k| {
                        let p = Position::new(x + dx * k, y + dy * k);
                        let frac = k as f32 / span as f32;
                        let line_z = eye_za + (eye_zb - eye_za) * frac;
                        elev(p) > line_z + 0.25
                    });
                    if has_crest {
                        ridge_pair = Some((a, b));
                        break 'outer_ridge;
                    }
                }
            }
        }
    }
    let (ra, rb) = ridge_pair.expect("County seed 42 must have a walkable hill ridge");

    assert!(game.teleport(Tile3D::from_pos(ra, 0)).is_some());
    let eye_a = PLAYER_SHAPE.eye_at(pos_center(ra).extend(elev(ra)));
    let eye_b = PLAYER_SHAPE.eye_at(pos_center(rb).extend(elev(rb)));
    let (ray, dist) = Ray3::from_points(eye_a, eye_b).unwrap();
    let ridge_hit = game
        .spatial()
        .raycast_filtered(ray, dist, QueryFilter::LINE_OF_SIGHT);
    assert!(
        matches!(ridge_hit, Some(h) if matches!(h.surface, HitSurface::Terrain { .. })),
        "hill ridge crest must occlude 3D eye-to-eye LOS, got {:?}",
        ridge_hit
    );

    let mut slope_pair: Option<(Position, Position)> = None;
    'outer_slope: for y in (32..WORLD_SIZE - 32).step_by(2) {
        for x in (32..WORLD_SIZE - 40).step_by(2) {
            let a = Position::new(x, y);
            let b = Position::new(x + 4, y);
            if !(0..=4).all(|dx| is_clear_outdoor(Position::new(x + dx, y))) {
                continue;
            }
            if game.teleport(Tile3D::from_pos(a, 0)).is_none() {
                continue;
            }
            let za = elev(a);
            let zb = elev(b);
            if (za - zb).abs() < 0.45 {
                continue;
            }
            let eye1 = PLAYER_SHAPE.eye_at(pos_center(a).extend(za));
            let eye2 = PLAYER_SHAPE.eye_at(pos_center(b).extend(zb));
            let clear_line = (1..=3).all(|dx| {
                let p = Position::new(x + dx, y);
                let frac = dx as f32 / 4.0;
                let line_z = eye1.z + (eye2.z - eye1.z) * frac;
                elev(p) < line_z - 0.35
            });
            if clear_line {
                slope_pair = Some((a, b));
                break 'outer_slope;
            }
        }
    }
    let (sa, sb) = slope_pair.expect("County seed 42 must have an open sloped face");
    assert!(game.teleport(Tile3D::from_pos(sa, 0)).is_some());
    let eye_sa = PLAYER_SHAPE.eye_at(pos_center(sa).extend(elev(sa)));
    let eye_sb = PLAYER_SHAPE.eye_at(pos_center(sb).extend(elev(sb)));
    let (ray, dist) = Ray3::from_points(eye_sa, eye_sb).unwrap();
    let slope_hit = game
        .spatial()
        .raycast_filtered(ray, dist - 0.05, QueryFilter::LINE_OF_SIGHT);
    assert!(
        slope_hit.is_none(),
        "open same-face hillside LOS must be clear, got {:?}",
        slope_hit
    );

    // 4. Find an outdoor foothill facing a steep uphill slope for Pistol & Rocket terrain interception.
    let mut slope_shot: Option<(Position, Vec2)> = None;
    'outer_shot: for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
        for span in 3..=7_i32 {
            for y in (32..WORLD_SIZE - 32).step_by(2) {
                for x in (32..WORLD_SIZE - 32).step_by(2) {
                    let shooter = Position::new(x, y);
                    if !is_clear_outdoor(shooter) {
                        continue;
                    }
                    let zs = elev(shooter);
                    let mid_clear = (0..=span).all(|k| {
                        let p = Position::new(x + dx * k, y + dy * k);
                        let mut cols = Vec::new();
                        game.spatial().colliders_in(
                            zombiebox_core::Aabb3::new(
                                Vec3::new(p.x as f32 - 0.5, p.y as f32 - 0.5, -5.0),
                                Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.5, 40.0),
                            ),
                            &mut cols,
                        );
                        world.tile_at(p).is_some_and(|t| {
                            t.water.is_none() && t.feature == zombiebox_core::Feature::Empty
                        }) && cols.is_empty()
                            && !in_any_building(p)
                    });
                    if !mid_clear {
                        continue;
                    }
                    let peak = Position::new(x + dx * span, y + dy * span);
                    if elev(peak) >= zs + 1.8 {
                        if game.teleport(Tile3D::from_pos(shooter, 0)).is_none() {
                            continue;
                        }
                        let path_clear_of_monsters = game
                            .monster_views()
                            .iter()
                            .all(|m| m.pos.distance(pos_center(shooter)) > 7.5);
                        if path_clear_of_monsters {
                            slope_shot = Some((shooter, Vec2::new(dx as f32, dy as f32)));
                            break 'outer_shot;
                        }
                    }
                }
            }
        }
    }
    let (shooter_pos, aim_dir) =
        slope_shot.expect("County seed 42 must have a clear foothill rising into a steep slope");
    assert!(game.teleport(Tile3D::from_pos(shooter_pos, 0)).is_some());

    // Pistol shot stops on uphill Terrain.
    game.step(ActionInput {
        select_weapon: Some(Weapon::Pistol),
        aim: Some(aim_dir),
        ..ActionInput::default()
    });
    for _ in 0..30 {
        game.step(ActionInput {
            aim: Some(aim_dir),
            ..ActionInput::default()
        });
    }
    game.step(ActionInput {
        aim: Some(aim_dir),
        fire: true,
        ..ActionInput::default()
    });
    let bullet_hit = game.events().iter().find_map(|e| match *e {
        ActionEvent::BulletTrace {
            to,
            hit_normal,
            hit_material,
            ..
        } => Some((to, hit_normal, hit_material)),
        _ => None,
    });
    let (b_to, b_norm, b_mat) = bullet_hit.expect("pistol shot must emit BulletTrace");
    assert_eq!(b_mat, MaterialCategory::Stone);
    assert!(b_norm.z > 0.0, "uphill hit normal must point upward");
    let b_terrain_z = world.terrain_at(b_to.xy()).expect("on terrain").z;
    assert!(
        (b_to.z - b_terrain_z).abs() < 0.08,
        "bullet hit z={} must match terrain z={}",
        b_to.z,
        b_terrain_z
    );

    // Rocket shot detonates on uphill Terrain without tunneling.
    game.step(ActionInput {
        select_weapon: Some(Weapon::RocketLauncher),
        aim: Some(aim_dir),
        ..ActionInput::default()
    });
    for _ in 0..30 {
        game.step(ActionInput {
            aim: Some(aim_dir),
            ..ActionInput::default()
        });
    }
    assert_eq!(
        game.hud().equipped,
        Some(Weapon::RocketLauncher),
        "rocket cache must be looted before testing"
    );
    assert!(game.hud().active_mag > 0, "rocket must be loaded");
    game.step(ActionInput {
        aim: Some(aim_dir),
        fire: true,
        ..ActionInput::default()
    });
    let mut blast_at: Option<Vec3> = None;
    for _ in 0..40 {
        if let Some(at) = game.events().iter().find_map(|e| match *e {
            ActionEvent::RocketBlast { at, .. } => Some(at),
            _ => None,
        }) {
            blast_at = Some(at);
            break;
        }
        game.step(ActionInput::default());
    }
    let blast_at = blast_at.expect("rocket fired into hillside must detonate");
    assert!(
        blast_at.xy().distance(pos_center(shooter_pos)) <= 6.5,
        "rocket must detonate on the hillside, got dist={}",
        blast_at.xy().distance(pos_center(shooter_pos))
    );
    let r_terrain_z = world.terrain_at(blast_at.xy()).expect("on terrain").z;
    assert!(
        (blast_at.z - r_terrain_z).abs() < 0.45,
        "rocket blast z={} must be on terrain surface z={}",
        blast_at.z,
        r_terrain_z
    );
}

#[test]
fn multi_floor_3d_shooting_slab_occlusion_and_fall_injury_curve() {
    // Part A: Birch Street F1 shooting, slab occlusion, and 1-story safe window drop.
    let mut game = ActionGame::residential(42);
    let h1 = game.house().expect("House 1");

    // Grab Axe cache on stoop and Pistol cache inside House 1.
    let axe_cache = game.caches()[0].pos;
    assert!(walk_path_on_floor(&mut game, axe_cache, 45));
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert_eq!(game.hud().equipped, Some(Weapon::Axe));

    let pistol_cache = game.caches()[1].pos;
    assert!(walk_path_on_floor(&mut game, pistol_cache, 45));
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    game.step(ActionInput {
        select_weapon: Some(Weapon::Pistol),
        ..ActionInput::default()
    });
    assert_eq!(game.hud().equipped, Some(Weapon::Pistol));

    // Verify F1 intact floor slab blocks 3D LOS between F1 interior and F0 directly below.
    let slab_test_p = Position::new(h1.x + 2, h1.y + 3);
    let f1_eye = Vec3::new(slab_test_p.x as f32, slab_test_p.y as f32, FLOOR_H + 0.55);
    let f0_eye = Vec3::new(slab_test_p.x as f32, slab_test_p.y as f32, 0.55);
    let (ray, dist) = Ray3::from_points(f1_eye, f0_eye).unwrap();
    let slab_hit = game
        .spatial()
        .raycast_filtered(ray, dist, QueryFilter::LINE_OF_SIGHT);
    assert!(
        matches!(
            slab_hit,
            Some(h) if matches!(h.surface, HitSurface::Collider(ColliderKind::BuildingSlab { floor: 1, is_roof: false, .. }))
        ),
        "intact F1 slab must occlude vertical LOS between F1 and F0, got {:?}",
        slab_hit
    );

    // Climb House 1 stairs to F1.
    let block_origin = game.block_bounds().expect("block bounds").0;
    let stair_f0 = Tile3D::new(block_origin.x + 12, block_origin.y + 21, 0);
    assert!(walk_path_on_floor(&mut game, stair_f0, 45));
    for _ in 0..30 {
        if game.player_pose().floor == 1 {
            break;
        }
        game.step(ActionInput::default());
    }
    assert_eq!(game.player_pose().floor, 1);

    // Walk next to House 1 F1 south window and shoot downward out onto Floor 0 yard.
    let f1_win = Tile3D::new(h1.x + 2, h1.y + h1.height - 1, 1);
    let f1_win_inside = Tile3D::new(f1_win.x, f1_win.y - 2, 1);
    assert!(
        walk_path_on_floor(&mut game, f1_win_inside, 45),
        "player {:?}, target {:?}, outcome {:?}",
        game.player_tile(),
        f1_win_inside,
        game.outcome()
    );

    // Step slightly closer toward the window so DropWindow is within reach after shattering.
    for _ in 0..6 {
        game.step(ActionInput {
            movement: Vec2::new(0.0, 1.0),
            aim: Some(Vec2::new(0.0, 1.0)),
            ..ActionInput::default()
        });
    }
    for _ in 0..25 {
        game.step(ActionInput {
            aim: Some(Vec2::new(0.0, 1.0)),
            ..ActionInput::default()
        });
    }
    game.step(ActionInput {
        aim: Some(Vec2::new(0.0, 1.0)),
        fire: true,
        ..ActionInput::default()
    });
    let downward_trace = game.events().iter().any(|e| {
        matches!(
            e,
            ActionEvent::BulletTrace { from, to, .. } if from.z > FLOOR_H && to.z < FLOOR_H && from.z > to.z
        )
    });
    assert!(
        downward_trace,
        "F1 pistol shot through south window must angle downward into Floor 0"
    );

    // 1-story hang-drop from F1 broken window lands safely on F0 with 0 damage.
    let hp_before_f1_drop = game.hud().health;
    for _ in 0..20 {
        if game.hud().interact_prompt == InteractPrompt::DropWindow {
            break;
        }
        game.step(ActionInput {
            movement: Vec2::new(0.0, 1.0),
            aim: Some(Vec2::new(0.0, 1.0)),
            ..ActionInput::default()
        });
    }
    assert_eq!(
        game.hud().interact_prompt,
        InteractPrompt::DropWindow,
        "player {:?}, window {:?}, kind {:?}, outcome {:?}",
        game.player_body().position,
        f1_win,
        game.tile_render_info(f1_win.pos(), 1).map(|v| v.kind),
        game.outcome()
    );
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert_eq!(game.player_pose().floor, 0);
    assert!(game.player_body().position.z.abs() < 0.05);
    assert_eq!(
        game.hud().health,
        hp_before_f1_drop,
        "1-story hang-drop must deal 0 fall damage"
    );
    assert!(game.events().iter().any(|e| matches!(
        e,
        ActionEvent::FloorChanged {
            from_floor: 1,
            to_floor: 0,
            via: FloorTransition::WindowDrop,
            ..
        }
    )));

    // Part B: 2-story drop (3F -> 1F, 2.776u) in County 3F Apartments deals 1 HP fall damage.
    let mut county = ActionGame::county(42);
    let world = county.world().clone();
    let sites = world.sites().expect("county sites");
    let apt = sites
        .buildings
        .iter()
        .find(|b| b.kind == BuildingKind::Apartments)
        .expect("3F Apartments");

    let axe_c = county
        .caches()
        .iter()
        .find(|c| c.loot.weapon == Some(Weapon::Axe))
        .copied()
        .expect("county starter Axe");
    assert!(county.teleport(axe_c.pos).is_some());
    county.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    assert!(county.teleport(Tile3D::from_pos(apt.entry, 0)).is_some());

    // Find an F2 exterior window adjacent to an indoor F2 tile and outdoor F0 landing tile.
    let mut f2_drop_setup: Option<(Tile3D, Tile3D)> = None;
    for y in apt.min.y..=apt.max.y {
        for x in apt.min.x..=apt.max.x {
            let win = Tile3D::new(x, y, 2);
            if county.tile_render_info(win.pos(), 2).map(|i| i.kind) != Some(TileKind::Window) {
                continue;
            }
            for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
                let inside = Tile3D::new(x + dx, y + dy, 2);
                let outside_upper = Tile3D::new(x - dx, y - dy, 2);
                let outside_f0 = Tile3D::new(x - dx, y - dy, 0);
                let inside_ok = county
                    .tile_render_info(inside.pos(), 2)
                    .is_some_and(|i| i.kind == TileKind::Empty && i.surface != Surface::Air);
                let outside_air = county
                    .tile_render_info(outside_upper.pos(), 2)
                    .is_none_or(|i| i.surface == Surface::Air && i.building_id == 0);
                let f0_ok = county
                    .tile_render_info(outside_f0.pos(), 0)
                    .is_some_and(|i| i.kind == TileKind::Empty && i.building_id == 0);
                if inside_ok && outside_air && f0_ok {
                    f2_drop_setup = Some((inside, win));
                    break;
                }
            }
            if f2_drop_setup.is_some() {
                break;
            }
        }
        if f2_drop_setup.is_some() {
            break;
        }
    }
    let (f2_inside, f2_win) = f2_drop_setup.expect("3F Apartments must have a droppable F2 window");
    assert!(county.teleport(f2_inside).is_some());
    assert_eq!(county.player_pose().floor, 2);

    // Face F2 window, shatter it with Axe, and interact to drop 2 stories (2.776u).
    let to_win = (f2_win.center() - county.player_pose().position).normalized();
    for _ in 0..30 {
        county.step(ActionInput {
            aim: Some(to_win),
            ..ActionInput::default()
        });
    }
    county.step(ActionInput {
        aim: Some(to_win),
        fire: true,
        ..ActionInput::default()
    });
    assert_eq!(
        county.tile_render_info(f2_win.pos(), 2).map(|i| i.kind),
        Some(TileKind::BrokenWindow)
    );
    assert_eq!(county.hud().interact_prompt, InteractPrompt::DropWindow);

    let hp_before_3f_drop = county.hud().health;
    county.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    let mut got_fall_hurt = false;
    let mut got_landed_dmg = false;
    for _ in 0..90 {
        if county
            .events()
            .iter()
            .any(|e| matches!(e, ActionEvent::PlayerHurt { damage: 1, .. }))
        {
            got_fall_hurt = true;
        }
        if county
            .events()
            .iter()
            .any(|e| matches!(e, ActionEvent::ActorLanded { damage: 1, .. }))
        {
            got_landed_dmg = true;
        }
        if county.player_body().motion.is_grounded() && got_landed_dmg {
            break;
        }
        county.step(ActionInput::default());
    }
    assert!(
        got_fall_hurt,
        "3F -> 1F drop must emit PlayerHurt(damage: 1)"
    );
    assert!(
        got_landed_dmg,
        "3F -> 1F drop must emit ActorLanded(damage: 1)"
    );
    assert_eq!(county.player_pose().floor, 0);
    assert_eq!(county.hud().health, hp_before_3f_drop - 1);
}

#[test]
fn monster_3d_stair_pursuit_and_uneven_county_combat() {
    // Part A: Birch Street F0 zombie climbs House 1 stairs to pursue F1 player.
    let mut game = ActionGame::residential(42);
    let f0_ids: Vec<_> = game
        .monster_views()
        .into_iter()
        .filter(|m| m.floor == 0)
        .map(|m| m.id)
        .collect();

    // Pick up stoop Axe so Search doesn't shadow House 1's front door, then loot Pistol and climb stairs to F1.
    let axe_cache = game.caches()[0].pos;
    assert!(walk_path_on_floor(&mut game, axe_cache, 45));
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    let pistol_cache = game.caches()[1].pos;
    assert!(walk_path_on_floor(&mut game, pistol_cache, 45));
    game.step(ActionInput {
        interact: true,
        ..ActionInput::default()
    });
    game.step(ActionInput {
        select_weapon: Some(Weapon::Pistol),
        ..ActionInput::default()
    });

    let b_origin = game.block_bounds().expect("block bounds").0;
    let stair_f0 = Tile3D::new(b_origin.x + 12, b_origin.y + 21, 0);
    assert!(walk_path_on_floor(&mut game, stair_f0, 45));
    for _ in 0..30 {
        if game.player_pose().floor == 1 {
            break;
        }
        game.step(ActionInput::default());
    }
    assert_eq!(game.player_pose().floor, 1);

    // Step aside from the stairwell exit so ascending F0 monsters have an unobstructed F1 landing.
    let landing_side = Tile3D::new(stair_f0.x - 2, stair_f0.y - 1, 1);
    let _ = walk_path_on_floor(&mut game, landing_side, 20);

    let mut climbed_to_f1 = false;
    for t in 0..900 {
        let should_fire = t % 120 == 10 && game.hud().active_mag > 1;
        game.step(ActionInput {
            aim: Some(Vec2::new(1.0, 0.0)),
            fire: should_fire,
            ..ActionInput::default()
        });
        if game.monster_views().iter().any(|m| {
            f0_ids.contains(&m.id) && m.floor == 1 && (m.body.position.z - FLOOR_H).abs() < 0.15
        }) {
            climbed_to_f1 = true;
            break;
        }
    }
    assert!(
        climbed_to_f1,
        "at least one Birch Street F0 monster must climb stairs to F1 (z ~= FLOOR_H)"
    );

    // Part B: County road -> bridge -> road elevation tracking, slope aim_dir, and Shooter 3D targeting.
    let mut county = ActionGame::county(42);
    let world = county.world().clone();
    let sites = world.sites().expect("county sites");
    let in_any_building = |p: Position| -> bool {
        sites
            .buildings
            .iter()
            .any(|b| p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y)
    };

    let mut bridge_run: Option<(Position, Vec2, usize)> = None;
    'find_bridge: for (dx, dy) in [(1_i32, 0_i32), (0, 1)] {
        for y in 16..WORLD_SIZE - 24 {
            for x in 16..WORLD_SIZE - 24 {
                let p = Position::new(x, y);
                let prev = Position::new(x - dx, y - dy);
                if world.bridge_deck_at(pos_center(p)).is_none()
                    || world.bridge_deck_at(pos_center(prev)).is_some()
                {
                    continue;
                }
                let mut span = 0_i32;
                while span < 20
                    && world
                        .bridge_deck_at(pos_center(Position::new(x + dx * span, y + dy * span)))
                        .is_some()
                {
                    span += 1;
                }
                if span == 0 || span >= 20 {
                    continue;
                }
                let exit = Position::new(x + dx * span, y + dy * span);
                if ![prev, exit].into_iter().all(|q| {
                    world
                        .tile_at(q)
                        .is_some_and(|t| t.ground.is_road() && t.water.is_none())
                }) {
                    continue;
                }
                let ok = (-2..=span + 2).all(|k| {
                    let q = Position::new(x + dx * k, y + dy * k);
                    let mut cols = Vec::new();
                    county.spatial().colliders_in(
                        zombiebox_core::Aabb3::new(
                            Vec3::new(q.x as f32 - 0.5, q.y as f32 - 0.5, -5.0),
                            Vec3::new(q.x as f32 + 0.5, q.y as f32 + 0.5, 20.0),
                        ),
                        &mut cols,
                    );
                    let no_props = !cols.iter().any(|c| {
                        matches!(
                            c.kind,
                            zombiebox_core::ColliderKind::Prop { .. }
                                | zombiebox_core::ColliderKind::Wall { .. }
                                | zombiebox_core::ColliderKind::ClosedDoor { .. }
                        ) || matches!(c.kind, zombiebox_core::ColliderKind::BridgeParapet { .. })
                            && if dx != 0 {
                                c.bounds.min.y < q.y as f32 + PLAYER_SHAPE.radius
                                    && c.bounds.max.y > q.y as f32 - PLAYER_SHAPE.radius
                            } else {
                                c.bounds.min.x < q.x as f32 + PLAYER_SHAPE.radius
                                    && c.bounds.max.x > q.x as f32 - PLAYER_SHAPE.radius
                            }
                    });
                    world.tile_at(q).is_some_and(|t| {
                        (t.ground.walkable() || world.bridge_deck_at(pos_center(q)).is_some())
                            && t.feature == zombiebox_core::Feature::Empty
                    }) && no_props
                        && !in_any_building(q)
                });
                if ok {
                    bridge_run = Some((
                        Position::new(x, y),
                        Vec2::new(dx as f32, dy as f32),
                        span as usize,
                    ));
                    break 'find_bridge;
                }
            }
        }
    }
    let (bridge_start, bridge_dir, bridge_tiles) =
        bridge_run.expect("County seed 42 must have a walkable road-bridge-road crossing");
    let approach = Position::new(
        bridge_start.x - bridge_dir.x as i32,
        bridge_start.y - bridge_dir.y as i32,
    );
    assert_eq!(
        county.teleport(Tile3D::from_pos(approach, 0)),
        Some(Tile3D::from_pos(approach, 0))
    );
    assert!(matches!(
        county.player_body().motion,
        MotionState::Grounded(SupportId::Terrain)
    ));

    let mut saw_bridge = false;
    let mut saw_terrain_after_bridge = false;
    let total_walk_ticks = ((bridge_tiles as f32 / 4.0) * 60.0).ceil() as usize + 600;
    for _ in 0..total_walk_ticks {
        county.step(ActionInput {
            movement: bridge_dir,
            ..ActionInput::default()
        });
        match county.player_body().motion {
            MotionState::Grounded(SupportId::Bridge(_)) => saw_bridge = true,
            MotionState::Grounded(SupportId::Terrain) => {
                if saw_bridge {
                    saw_terrain_after_bridge = true;
                    break;
                }
            }
            _ => {
                // If monster or blockage or whatever pushes us, or if we keep walking
            }
        }
    }
    assert!(saw_bridge, "player must walk onto SupportId::Bridge");
    assert!(
        saw_terrain_after_bridge,
        "player must walk cleanly off bridge back onto SupportId::Terrain"
    );
    // Continue until the whole body clears the slab, not merely its center.
    let exit_pos = county.player_body().position.xy();
    for _ in 0..15 {
        county.step(ActionInput {
            movement: bridge_dir,
            ..ActionInput::default()
        });
    }
    assert!(
        (county.player_body().position.xy() - exit_pos).dot(bridge_dir) > PLAYER_SHAPE.radius,
        "player must continue past the bridge edge after returning to terrain"
    );

    // Sloped road aim_dir.z alignment (uphill > 0, downhill < 0).
    let mut sloped_road: Option<(Position, Vec2)> = None;
    'find_slope: for y in 24..WORLD_SIZE - 24 {
        for x in 24..WORLD_SIZE - 24 {
            let p = Position::new(x, y);
            let Some(t) = world.tile_at(p) else {
                continue;
            };
            if !t.ground.walkable() || t.water.is_some() || in_any_building(p) {
                continue;
            }
            let z0 = world.elevation_at(pos_center(p)).unwrap_or(0.0);
            let p_east = Position::new(x + 2, y);
            let p_west = Position::new(x - 2, y);
            if !in_any_building(p_east)
                && !in_any_building(p_west)
                && world
                    .tile_at(p_east)
                    .is_some_and(|te| te.ground.walkable() && te.water.is_none())
                && world
                    .tile_at(p_west)
                    .is_some_and(|tw| tw.ground.walkable() && tw.water.is_none())
            {
                let dz_e = world.elevation_at(pos_center(p_east)).unwrap_or(z0) - z0;
                let dz_w = z0 - world.elevation_at(pos_center(p_west)).unwrap_or(z0);
                if dz_e >= 0.20 && dz_w >= 0.20 {
                    sloped_road = Some((p, Vec2::new(1.0, 0.0)));
                    break 'find_slope;
                }
            }
        }
    }
    let (slope_pos, uphill_dir) = sloped_road.expect("County has sloped road/ground");
    assert!(county.teleport(Tile3D::from_pos(slope_pos, 0)).is_some());
    county.step(ActionInput {
        aim: Some(uphill_dir),
        ..ActionInput::default()
    });
    let up_aim = county.player_render_state().aim_dir;
    assert!(
        up_aim.z > 0.02,
        "aiming uphill must angle aim_dir upward, got {:?}",
        up_aim
    );
    county.step(ActionInput {
        aim: Some(-uphill_dir),
        ..ActionInput::default()
    });
    let down_aim = county.player_render_state().aim_dir;
    assert!(
        down_aim.z < -0.02,
        "aiming downhill must angle aim_dir downward, got {:?}",
        down_aim
    );

    // Shooter 3D targeting & bolt launch in County.
    let mut shooter_found = false;
    for b in &sites.buildings {
        let _ = county.teleport(Tile3D::from_pos(b.entry, 0));
        if let Some(sh) = county
            .monster_views()
            .into_iter()
            .find(|m| m.kind == MonsterKind::Shooter && m.floor == 0)
        {
            let sh_pos = sh.pos;
            for (dx, dy) in [(5, 0), (-5, 0), (0, 5), (0, -5), (3, 3)] {
                let stand = Tile3D::new(
                    sh_pos.x.round() as i32 + dx,
                    sh_pos.y.round() as i32 + dy,
                    0,
                );
                if county.teleport(stand).is_some() {
                    for _ in 0..180 {
                        county.step(ActionInput {
                            fire: true,
                            ..ActionInput::default()
                        });
                        if let Some((from, dir)) = county.events().iter().find_map(|e| match *e {
                            ActionEvent::ProjectileLaunched {
                                kind: ProjectileKind::ShooterBolt,
                                from,
                                dir,
                                ..
                            } => Some((from, dir)),
                            _ => None,
                        }) {
                            let gz = world.terrain_at(from.xy()).map_or(0.0, |t| t.z);
                            assert!(
                                from.z > gz + 0.3,
                                "ShooterBolt must spawn at 3D muzzle height above ground (from.z={}, gz={})",
                                from.z,
                                gz
                            );
                            assert!((dir.length() - 1.0).abs() < 0.02);
                            shooter_found = true;
                            break;
                        }
                    }
                    if shooter_found {
                        break;
                    }
                }
            }
            if shooter_found {
                break;
            }
        }
    }
    assert!(
        shooter_found,
        "expected a County Shooter to fire a 3D ShooterBolt"
    );
}
