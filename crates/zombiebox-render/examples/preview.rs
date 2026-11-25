//! Multi-scene visual showcase (`640×360` PPMs in `/tmp/`) and `800×360` p95 benchmark harness.

use std::fs;
use std::time::Instant;

use zombiebox_core::{
    ActionEvent, ActionGame, ActionInput, ActionOutcome, MonsterKind, Position, TileKind, Vec2,
    Weapon,
};
use zombiebox_render::Renderer;

const TICK_MS: u64 = 16;

fn write_ppm(path: &str, width: usize, height: usize, rgba: &[u8]) -> std::io::Result<()> {
    let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
    ppm.reserve(width * height * 3);
    for px in rgba.chunks_exact(4) {
        ppm.extend_from_slice(&px[..3]);
    }
    fs::write(path, ppm)
}

fn step_and_observe(
    game: &mut ActionGame,
    renderer: &mut Renderer,
    elapsed_ms: &mut u64,
    input: ActionInput,
) {
    game.step(input);
    *elapsed_ms += TICK_MS;
    renderer.observe_events(game, *elapsed_ms);
}

#[allow(clippy::too_many_arguments)]
fn walk_to_local(
    game: &mut ActionGame,
    renderer: &mut Renderer,
    elapsed_ms: &mut u64,
    origin: Position,
    lx: f32,
    ly: f32,
    max_ticks: usize,
    quiet: bool,
) {
    let target = Vec2::new(origin.x as f32 + lx, origin.y as f32 + ly);
    for _ in 0..max_ticks {
        let p = game.player_pose().position;
        let diff = target - p;
        let dist = diff.length();
        if dist <= 0.14 {
            break;
        }
        let dir = diff * (1.0 / dist);
        let mag = if quiet { 0.45 } else { dist.min(1.0) };
        step_and_observe(
            game,
            renderer,
            elapsed_ms,
            ActionInput {
                movement: dir * mag,
                aim: Some(dir),
                quiet_walk: quiet,
                ..ActionInput::default()
            },
        );
    }
}

fn render_and_save(
    path: &str,
    game: &ActionGame,
    renderer: &mut Renderer,
    elapsed_ms: u64,
) -> std::io::Result<()> {
    let w = renderer.width();
    let h = renderer.height();
    let rgba = renderer.render(game, elapsed_ms);
    write_ppm(path, w, h, rgba)
}

fn scene_street() -> std::io::Result<()> {
    let mut game = ActionGame::residential(42);
    let mut renderer = Renderer::new(640, 360);
    let mut ms = 0u64;
    for _ in 0..10 {
        step_and_observe(&mut game, &mut renderer, &mut ms, ActionInput::default());
    }
    render_and_save("/tmp/zombiebox-street.ppm", &game, &mut renderer, ms)?;
    println!(
        "[scene 1] /tmp/zombiebox-street.ppm  floor={} pos=({:.1},{:.1}) zombies={}",
        game.player_pose().floor,
        game.player_pose().position.x,
        game.player_pose().position.y,
        game.hud().zombie_count,
    );
    Ok(())
}

fn scene_interior() -> std::io::Result<()> {
    let mut game = ActionGame::residential(42);
    let origin = game.block_bounds().expect("block bounds").0;
    let mut renderer = Renderer::new(640, 360);
    let mut ms = 0u64;

    // 1. Grab Axe cache on front stoop (10, 24).
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        10.0,
        24.0,
        60,
        false,
    );
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            interact: true,
            ..ActionInput::default()
        },
    );

    // 2. Shatter South window (11, 23) with Axe so sparkling glass shards appear!
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        11.0,
        24.0,
        40,
        false,
    );
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            aim: Some(Vec2::new(0.0, -1.0)),
            fire: true,
            ..ActionInput::default()
        },
    );

    // 3. Enter Starter House through open front doorway (9, 23) into living room (9.0, 21.0).
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        9.0,
        24.0,
        45,
        false,
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        9.0,
        21.2,
        65,
        false,
    );

    // Settle camera & smooth 160 ms cutaway dissolve while aiming toward Pistol cache + shattered window.
    for _ in 0..18 {
        step_and_observe(
            &mut game,
            &mut renderer,
            &mut ms,
            ActionInput {
                aim: Some(Vec2::new(-0.6, -0.35)),
                ..ActionInput::default()
            },
        );
        renderer.render(&game, ms);
    }

    let win_state = game
        .tile_render_info(Position::new(origin.x + 11, origin.y + 23), 0)
        .map(|t| t.kind);
    render_and_save("/tmp/zombiebox-interior.ppm", &game, &mut renderer, ms)?;
    println!(
        "[scene 2] /tmp/zombiebox-interior.ppm floor={} indoors={} equipped={:?} south_win={:?}",
        game.player_pose().floor,
        game.hud().indoors,
        game.hud().equipped,
        win_state,
    );
    Ok(())
}

fn scene_upstairs() -> std::io::Result<()> {
    let mut game = ActionGame::residential(42);
    let origin = game.block_bounds().expect("block bounds").0;
    let mut renderer = Renderer::new(640, 360);
    let mut ms = 0u64;

    // 1. Grab Axe at (10, 24) and Pistol inside Starter House at (7, 20).
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        10.0,
        24.0,
        60,
        false,
    );
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            interact: true,
            ..ActionInput::default()
        },
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        9.0,
        23.0,
        45,
        false,
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        9.0,
        21.0,
        45,
        false,
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        7.2,
        20.5,
        55,
        false,
    );
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            interact: true,
            ..ActionInput::default()
        },
    );

    // 2. Climb StairsUp at (12, 21) to Floor 1!
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        11.0,
        21.0,
        75,
        false,
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        12.0,
        21.0,
        40,
        false,
    );

    // 3. Walk to F1 South window (11, 23) and shatter it with Axe!
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        11.0,
        22.0,
        45,
        false,
    );
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            select_weapon: Some(Weapon::Axe),
            aim: Some(Vec2::new(0.0, 1.0)),
            ..ActionInput::default()
        },
    );
    for _ in 0..20 {
        step_and_observe(
            &mut game,
            &mut renderer,
            &mut ms,
            ActionInput {
                aim: Some(Vec2::new(0.0, 1.0)),
                ..ActionInput::default()
            },
        );
    }
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            aim: Some(Vec2::new(0.0, 1.0)),
            fire: true,
            ..ActionInput::default()
        },
    );

    // 4. Switch to Pistol and fire a shot out the upstairs window to attract outdoor F0 infected below!
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            select_weapon: Some(Weapon::Pistol),
            aim: Some(Vec2::new(0.65, 0.76)),
            ..ActionInput::default()
        },
    );
    for i in 0..85 {
        step_and_observe(
            &mut game,
            &mut renderer,
            &mut ms,
            ActionInput {
                aim: Some(Vec2::new(0.62, 0.78)),
                fire: i == 25,
                ..ActionInput::default()
            },
        );
        renderer.render(&game, ms);
    }

    let f0_outdoor_visible = game
        .monster_views()
        .iter()
        .filter(|m| m.floor == 0 && m.pos.distance(game.player_pose().position) < 10.0)
        .count();
    let f1_win = game
        .tile_render_info(Position::new(origin.x + 11, origin.y + 23), 1)
        .map(|t| t.kind);
    render_and_save("/tmp/zombiebox-upstairs.ppm", &game, &mut renderer, ms)?;
    println!(
        "[scene 3] /tmp/zombiebox-upstairs.ppm floor={} f1_win={:?} prompt={:?} nearby_f0_monsters={}",
        game.player_pose().floor,
        f1_win,
        game.hud().interact_prompt,
        f0_outdoor_visible,
    );
    Ok(())
}

fn scene_combat() -> std::io::Result<()> {
    let mut game = ActionGame::residential(42);
    let origin = game.block_bounds().expect("block bounds").0;
    let mut renderer = Renderer::new(640, 360);
    let mut ms = 0u64;

    // 1. Collect Axe on front stoop (10, 24).
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        10.0,
        24.0,
        60,
        false,
    );
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            interact: true,
            ..ActionInput::default()
        },
    );

    // 2. Travel north along the open Road x = 17 to streetlight (16, 12) and smash it into BrokenStreetlight!
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        17.0,
        25.0,
        120,
        true,
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        17.0,
        12.0,
        420,
        true,
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        16.9,
        12.0,
        60,
        false,
    );
    for _ in 0..4 {
        if game
            .tile_render_info(Position::new(origin.x + 16, origin.y + 12), 0)
            .is_some_and(|t| t.kind == TileKind::BrokenStreetlight)
        {
            break;
        }
        step_and_observe(
            &mut game,
            &mut renderer,
            &mut ms,
            ActionInput {
                aim: Some(Vec2::new(-1.0, 0.0)),
                fire: true,
                ..ActionInput::default()
            },
        );
        for _ in 0..28 {
            step_and_observe(&mut game, &mut renderer, &mut ms, ActionInput::default());
        }
    }

    // 3. Enter House 3 via East window (15, 7) to grab Medkit (13, 4) and upstairs Rocket Launcher (8, 4, F1)!
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        17.0,
        7.0,
        120,
        false,
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        16.0,
        7.0,
        40,
        false,
    );
    // Shatter House 3 East window (15, 7) and vault right into SE room!
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            aim: Some(Vec2::new(-1.0, 0.0)),
            fire: true,
            ..ActionInput::default()
        },
    );
    for _ in 0..10 {
        step_and_observe(&mut game, &mut renderer, &mut ms, ActionInput::default());
    }
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        14.0,
        7.0,
        50,
        false,
    );
    // Open interior door (12, 6) to reach StairsUp (14, 5) and Medkit cache (13, 4)!
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        12.0,
        7.0,
        50,
        false,
    );
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            aim: Some(Vec2::new(0.0, -1.0)),
            interact: true,
            ..ActionInput::default()
        },
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        12.0,
        5.0,
        55,
        false,
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        13.0,
        4.3,
        45,
        false,
    );
    // Grab Medkit + pistol ammo cache (13, 4, F0) just in case.
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            interact: true,
            ..ActionInput::default()
        },
    );
    // Climb StairsUp (14, 5) -> Floor 1!
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        14.0,
        5.0,
        45,
        false,
    );
    // Open F1 study/master door (11, 4, F1), grab Rocket Launcher at (8, 4, F1)!
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        12.0,
        4.0,
        45,
        false,
    );
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            aim: Some(Vec2::new(-1.0, 0.0)),
            interact: true,
            ..ActionInput::default()
        },
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        8.2,
        4.0,
        80,
        false,
    );
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            interact: true,
            ..ActionInput::default()
        },
    );
    // Heal if scratched by the upstairs Shooter while grabbing Rocket Launcher!
    if game.hud().can_heal {
        step_and_observe(
            &mut game,
            &mut renderer,
            &mut ms,
            ActionInput {
                use_medkit: true,
                ..ActionInput::default()
            },
        );
    }

    // Return down stairs (14, 5, F1) -> F0 and step out onto the intersection near broken streetlight (16, 12)
    // and NE Park Shooter (24, 6) + House 3 Bloater!
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        13.0,
        5.0,
        100,
        false,
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        14.0,
        5.0,
        45,
        false,
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        12.0,
        7.0,
        60,
        false,
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        17.5,
        7.0,
        110,
        false,
    );
    walk_to_local(
        &mut game,
        &mut renderer,
        &mut ms,
        origin,
        17.5,
        10.2,
        70,
        false,
    );

    // Equip Rocket Launcher and blast House 3's East exterior wall (15, 8) to breach wall into rubble!
    step_and_observe(
        &mut game,
        &mut renderer,
        &mut ms,
        ActionInput {
            select_weapon: Some(Weapon::RocketLauncher),
            aim: Some(Vec2::new(-0.9, -0.42).normalized()),
            ..ActionInput::default()
        },
    );
    // Wait until a Shooter is telegraphing OR fire a fresh rocket into the breached corner while monsters converge!
    let mut fired_first = false;
    let mut blast_seen_ms: Option<u64> = None;
    for tick in 0..220 {
        if game.outcome() != ActionOutcome::Playing {
            break;
        }
        let monsters = game.monster_views();
        let shooter_telegraph = monsters
            .iter()
            .any(|m| m.kind == MonsterKind::Shooter && m.telegraph_progress.is_some());
        let p = game.player_pose().position;
        let bloater_near = monsters
            .iter()
            .any(|m| m.kind == MonsterKind::Bloater && m.pos.distance(p) < 9.0);

        let fire_now = if !fired_first && tick == 10 {
            fired_first = true;
            true
        } else {
            // Fire a second rocket once reloaded when shooter telegraphs or near the end!
            tick >= 120 && game.hud().active_mag > 0 && (shooter_telegraph || tick == 165)
        };
        let aim_dir = if tick < 40 {
            Vec2::new(-0.85, -0.52).normalized()
        } else {
            // Aim slightly toward breached wall / approaching Bloater while facing park Shooter.
            Vec2::new(-0.78, -0.62).normalized()
        };
        let need_reload =
            game.hud().can_reload && !game.hud().reloading && game.hud().active_mag == 0;
        let need_heal = game.hud().health <= 3 && game.hud().can_heal;
        step_and_observe(
            &mut game,
            &mut renderer,
            &mut ms,
            ActionInput {
                aim: Some(aim_dir),
                fire: fire_now,
                reload: need_reload,
                use_medkit: need_heal,
                ..ActionInput::default()
            },
        );
        if game
            .events()
            .iter()
            .any(|e| matches!(e, ActionEvent::RocketBlast { .. }))
        {
            blast_seen_ms = Some(ms);
        }
        renderer.render(&game, ms);

        // Capture peak combat frame: right after a rocket blast (still glowing rings & particles!)
        // with shooter telegraph or bloater in frame!
        if let Some(b_ms) = blast_seen_ms {
            let age = ms.saturating_sub(b_ms);
            if (32..=160).contains(&age) && (shooter_telegraph || (bloater_near && tick > 100)) {
                break;
            }
        }
    }

    let light_state = game
        .tile_render_info(Position::new(origin.x + 16, origin.y + 12), 0)
        .map(|t| t.kind);
    let breached_walls = (2..10)
        .filter(|&ly| {
            game.tile_render_info(Position::new(origin.x + 15, origin.y + ly), 0)
                .is_some_and(|t| t.kind == TileKind::Rubble)
        })
        .count();
    let monsters = game.monster_views();
    let p = game.player_pose().position;
    let bloaters = monsters
        .iter()
        .filter(|m| m.kind == MonsterKind::Bloater && m.pos.distance(p) < 11.0)
        .count();
    let shooters = monsters
        .iter()
        .filter(|m| m.kind == MonsterKind::Shooter && m.pos.distance(p) < 12.0)
        .count();
    let telegraphing = monsters
        .iter()
        .filter(|m| m.telegraph_progress.is_some())
        .count();

    render_and_save("/tmp/zombiebox-combat.ppm", &game, &mut renderer, ms)?;
    println!(
        "[scene 4] /tmp/zombiebox-combat.ppm  equipped={:?} streetlight={:?} breached_east_wall={} bloaters_near={} shooters_near={} telegraphs={}",
        game.hud().equipped,
        light_state,
        breached_walls,
        bloaters,
        shooters,
        telegraphing,
    );
    Ok(())
}

fn benchmark_800x360(label: &str, stress_monsters: Option<usize>) {
    let mut game = ActionGame::residential(42);
    if let Some(total) = stress_monsters {
        game.spawn_stress_monsters(total);
    }
    let mut renderer = Renderer::new(800, 360);
    let mut ms = 0u64;

    // Warmup 10 frames.
    for _ in 0..10 {
        game.step(ActionInput {
            movement: Vec2::new(0.25, -0.5),
            ..ActionInput::default()
        });
        ms += TICK_MS;
        let _ = renderer.render(&game, ms);
    }

    let mut samples_us = Vec::with_capacity(60);
    for i in 0..60 {
        let dir = Vec2::new((i as f32 * 0.12).cos() * 0.6, (i as f32 * 0.12).sin() * 0.6);
        game.step(ActionInput {
            movement: dir,
            aim: Some(dir),
            ..ActionInput::default()
        });
        ms += TICK_MS;
        let t0 = Instant::now();
        let frame = renderer.render(&game, ms);
        std::hint::black_box(frame.len());
        samples_us.push(t0.elapsed().as_micros() as f64);
    }
    samples_us.sort_by(|a, b| a.total_cmp(b));
    let avg_ms = (samples_us.iter().sum::<f64>() / samples_us.len() as f64) / 1000.0;
    let p50_ms = samples_us[samples_us.len() / 2] / 1000.0;
    let p95_idx = ((samples_us.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(samples_us.len() - 1);
    let p95_ms = samples_us[p95_idx] / 1000.0;
    let max_ms = samples_us[samples_us.len() - 1] / 1000.0;
    println!(
        "[bench 800x360] {label:<28} monsters={:<3} avg={avg_ms:.3}ms p50={p50_ms:.3}ms p95={p95_ms:.3}ms max={max_ms:.3}ms",
        game.hud().zombie_count,
    );
}

fn main() -> std::io::Result<()> {
    scene_street()?;
    scene_interior()?;
    scene_upstairs()?;
    scene_combat()?;
    benchmark_800x360("Birch Street (standard)", None);
    benchmark_800x360("Birch Street (100 stress)", Some(100));
    Ok(())
}
