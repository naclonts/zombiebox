//! Multi-scene visual showcase (`640×360` PPMs in `/tmp/` or `--baselines` PNGs) and `800×360` p95 benchmark harness.

use std::fmt::Write as _;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Instant;

use zombiebox_core::{
    ActionEvent, ActionGame, ActionInput, ActionOutcome, Ground, LandmarkKind, MonsterKind,
    Position, Tile3D, TileKind, Vec2, WORLD_SIZE, Water, Weapon, World,
};
use zombiebox_render::Renderer;

const TICK_MS: u64 = 16;

enum OutputTarget<'a> {
    Ppm(&'a Path),
    Png(&'a Path),
}

struct BaselineRecord {
    file: String,
    scenario: &'static str,
    seed: u64,
    pos_x: f32,
    pos_y: f32,
    floor: u8,
    notes: String,
}

fn write_ppm(path: &Path, width: usize, height: usize, rgba: &[u8]) -> std::io::Result<()> {
    let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
    ppm.reserve(width * height * 3);
    for px in rgba.chunks_exact(4) {
        ppm.extend_from_slice(&px[..3]);
    }
    fs::write(path, ppm)
}

fn write_png_rgb(path: &Path, width: usize, height: usize, rgba: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let mut rgb = Vec::with_capacity(width * height * 3);
    for px in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }
    let file = fs::File::create(path)?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut png_writer = encoder.write_header().map_err(std::io::Error::other)?;
    png_writer
        .write_image_data(&rgb)
        .map_err(std::io::Error::other)
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
    target: OutputTarget<'_>,
    game: &ActionGame,
    renderer: &mut Renderer,
    elapsed_ms: u64,
) -> std::io::Result<()> {
    let w = renderer.width();
    let h = renderer.height();
    let rgba = renderer.render(game, elapsed_ms);
    match target {
        OutputTarget::Ppm(path) => write_ppm(path, w, h, rgba),
        OutputTarget::Png(path) => write_png_rgb(path, w, h, rgba),
    }
}

fn scene_street(target: OutputTarget<'_>) -> std::io::Result<BaselineRecord> {
    let mut game = ActionGame::residential(42);
    let mut renderer = Renderer::new(640, 360);
    let mut ms = 0u64;
    for _ in 0..10 {
        step_and_observe(&mut game, &mut renderer, &mut ms, ActionInput::default());
    }
    let pose = game.player_pose();
    let zombies = game.hud().zombie_count;
    render_and_save(target, &game, &mut renderer, ms)?;
    println!(
        "[scene 1] floor={} pos=({:.1},{:.1}) zombies={}",
        pose.floor, pose.position.x, pose.position.y, zombies,
    );
    Ok(BaselineRecord {
        file: "birch-street-exterior.png".to_string(),
        scenario: "Birch Street Exterior",
        seed: 42,
        pos_x: pose.position.x,
        pos_y: pose.position.y,
        floor: pose.floor,
        notes: format!("Dusk spawn on Birch Street, exterior houses & {zombies} infected"),
    })
}

fn scene_interior(target: OutputTarget<'_>) -> std::io::Result<BaselineRecord> {
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
    let pose = game.player_pose();
    render_and_save(target, &game, &mut renderer, ms)?;
    println!(
        "[scene 2] floor={} indoors={} equipped={:?} south_win={:?}",
        pose.floor,
        game.hud().indoors,
        game.hud().equipped,
        win_state,
    );
    Ok(BaselineRecord {
        file: "birch-street-interior.png".to_string(),
        scenario: "Birch Street Interior (F0)",
        seed: 42,
        pos_x: pose.position.x,
        pos_y: pose.position.y,
        floor: pose.floor,
        notes: format!(
            "F0 living room cutaway, shattered south window ({win_state:?}), pistol cache"
        ),
    })
}

fn scene_upstairs(target: OutputTarget<'_>) -> std::io::Result<BaselineRecord> {
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
    let pose = game.player_pose();
    render_and_save(target, &game, &mut renderer, ms)?;
    println!(
        "[scene 3] floor={} f1_win={:?} prompt={:?} nearby_f0_monsters={}",
        pose.floor,
        f1_win,
        game.hud().interact_prompt,
        f0_outdoor_visible,
    );
    Ok(BaselineRecord {
        file: "birch-street-upstairs.png".to_string(),
        scenario: "Birch Street Upstairs (F1)",
        seed: 42,
        pos_x: pose.position.x,
        pos_y: pose.position.y,
        floor: pose.floor,
        notes: format!(
            "F1 cutaway aiming out shattered window ({f1_win:?}), {f0_outdoor_visible} F0 yard threats below"
        ),
    })
}

fn scene_combat(target: OutputTarget<'_>) -> std::io::Result<BaselineRecord> {
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

    let pose = game.player_pose();
    render_and_save(target, &game, &mut renderer, ms)?;
    println!(
        "[scene 4] equipped={:?} streetlight={:?} breached_east_wall={} bloaters_near={} shooters_near={} telegraphs={}",
        game.hud().equipped,
        light_state,
        breached_walls,
        bloaters,
        shooters,
        telegraphing,
    );
    Ok(BaselineRecord {
        file: "birch-street-combat.png".to_string(),
        scenario: "Birch Street Combat",
        seed: 42,
        pos_x: pose.position.x,
        pos_y: pose.position.y,
        floor: pose.floor,
        notes: format!(
            "Broken streetlight ({light_state:?}), {breached_walls} breached wall tiles, active rocket blast/telegraphs ({telegraphing})"
        ),
    })
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

fn find_river_bridge(world: &World) -> Position {
    let center = WORLD_SIZE / 2;
    let mut best: Option<(usize, i64, Position)> = None;
    for y in 16..(WORLD_SIZE - 16) {
        for x in 16..(WORLD_SIZE - 16) {
            let pos = Position::new(x, y);
            let Some(tile) = world.tile_at(pos) else {
                continue;
            };
            if tile.ground != Ground::Bridge || tile.water != Some(Water::River) {
                continue;
            }
            let mut river_count = 0usize;
            for dy in -3..=3 {
                for dx in -3..=3 {
                    if world
                        .tile_at(Position::new(x + dx, y + dy))
                        .is_some_and(|t| t.water == Some(Water::River))
                    {
                        river_count += 1;
                    }
                }
            }
            let dx = i64::from(x - center);
            let dy = i64::from(y - center);
            let dist2 = dx * dx + dy * dy;
            let replace = match best {
                None => true,
                Some((best_count, best_dist2, _)) => {
                    river_count > best_count || (river_count == best_count && dist2 < best_dist2)
                }
            };
            if replace {
                best = Some((river_count, dist2, pos));
            }
        }
    }
    best.expect("county has at least one river bridge").2
}

fn capture_county_scene(
    world: &World,
    out_dir: &Path,
    file: &str,
    scenario: &'static str,
    teleport_to: Option<Tile3D>,
    notes: &str,
) -> std::io::Result<BaselineRecord> {
    let mut game = ActionGame::county_with_world(world.clone());
    if let Some(target) = teleport_to {
        let dest = game.teleport(target);
        assert!(
            dest.is_some(),
            "failed to teleport to {target:?} for {file}"
        );
    }
    // Fresh Renderer after teleport so camera focus snaps immediately without panning.
    let mut renderer = Renderer::new(640, 360);
    let mut ms = 0u64;
    for _ in 0..10 {
        step_and_observe(&mut game, &mut renderer, &mut ms, ActionInput::default());
        renderer.render(&game, ms);
    }
    let out_path = out_dir.join(file);
    render_and_save(OutputTarget::Png(&out_path), &game, &mut renderer, ms)?;
    let pose = game.player_pose();
    println!(
        "[baseline] {}  floor={} pos=({:.1},{:.1}) zombies={}",
        out_path.display(),
        pose.floor,
        pose.position.x,
        pose.position.y,
        game.hud().zombie_count,
    );
    Ok(BaselineRecord {
        file: file.to_string(),
        scenario,
        seed: world.seed(),
        pos_x: pose.position.x,
        pos_y: pose.position.y,
        floor: pose.floor,
        notes: notes.to_string(),
    })
}

fn write_manifest(out_dir: &Path, records: &[BaselineRecord]) -> std::io::Result<()> {
    let mut md = String::new();
    writeln!(
        md,
        "# 2.5D Isometric Reference Baselines (`zombiebox-render`)"
    )
    .unwrap();
    writeln!(md).unwrap();
    writeln!(md, "* **Baseline Commit:** `daeff4a`").unwrap();
    writeln!(
        md,
        "* **Reproduction Command:** `cargo run --locked --offline --release -p zombiebox-render --example preview -- --baselines docs/baselines/iso-2p5d`"
    )
    .unwrap();
    writeln!(md).unwrap();
    writeln!(
        md,
        "| File | Scenario | Seed | Player Pos (x, y) | Floor | Resolution | Notes |"
    )
    .unwrap();
    writeln!(md, "|---|---|---:|---|---:|---|---|").unwrap();
    for rec in records {
        writeln!(
            md,
            "| `{}` | {} | {} | ({:.1}, {:.1}) | {} | 640×360 | {} |",
            rec.file, rec.scenario, rec.seed, rec.pos_x, rec.pos_y, rec.floor, rec.notes
        )
        .unwrap();
    }
    fs::write(out_dir.join("manifest.md"), md)
}

fn run_baselines(out_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(out_dir)?;

    let mut records = Vec::with_capacity(11);
    records.push(scene_street(OutputTarget::Png(
        &out_dir.join("birch-street-exterior.png"),
    ))?);
    records.push(scene_interior(OutputTarget::Png(
        &out_dir.join("birch-street-interior.png"),
    ))?);
    records.push(scene_upstairs(OutputTarget::Png(
        &out_dir.join("birch-street-upstairs.png"),
    ))?);
    records.push(scene_combat(OutputTarget::Png(
        &out_dir.join("birch-street-combat.png"),
    ))?);

    let world = World::new(42);
    let landmarks = world.landmarks();
    let find_landmark = |kind: LandmarkKind| -> Tile3D {
        let landmark = landmarks
            .iter()
            .find(|l| l.kind == kind)
            .unwrap_or_else(|| panic!("missing landmark {kind:?} in county seed 42"));
        Tile3D::from_pos(landmark.entrance, 0)
    };

    records.push(capture_county_scene(
        &world,
        out_dir,
        "county-s42-spawn.png",
        "County Spawn",
        None,
        "Default procedural residential spawn",
    )?);
    records.push(capture_county_scene(
        &world,
        out_dir,
        "county-s42-downtown.png",
        "County Downtown",
        Some(find_landmark(LandmarkKind::Downtown)),
        "Downtown commercial/apartment entrance",
    )?);
    records.push(capture_county_scene(
        &world,
        out_dir,
        "county-s42-hospital.png",
        "County Hospital",
        Some(find_landmark(LandmarkKind::Hospital)),
        "Hospital complex entrance",
    )?);
    records.push(capture_county_scene(
        &world,
        out_dir,
        "county-s42-police.png",
        "County Police",
        Some(find_landmark(LandmarkKind::Police)),
        "Police station entrance",
    )?);
    records.push(capture_county_scene(
        &world,
        out_dir,
        "county-s42-army-base.png",
        "County Army Base",
        Some(find_landmark(LandmarkKind::ArmyBase)),
        "Fortified army base entrance",
    )?);
    records.push(capture_county_scene(
        &world,
        out_dir,
        "county-s42-woods-cabin.png",
        "County Woods Cabin",
        Some(find_landmark(LandmarkKind::Cabin)),
        "Secluded woodland cabin entrance",
    )?);
    let bridge_pos = find_river_bridge(&world);
    records.push(capture_county_scene(
        &world,
        out_dir,
        "county-s42-river-bridge.png",
        "County River Bridge",
        Some(Tile3D::from_pos(bridge_pos, 0)),
        "River bridge crossing over open river channel",
    )?);

    write_manifest(out_dir, &records)?;
    println!(
        "[baseline] wrote {} PNGs and {}",
        records.len(),
        out_dir.join("manifest.md").display()
    );
    Ok(())
}

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    if let Some(flag) = args.next()
        && flag == "--baselines"
    {
        let out_dir = args
            .next()
            .map_or_else(|| PathBuf::from("docs/baselines/iso-2p5d"), PathBuf::from);
        return run_baselines(&out_dir);
    }

    scene_street(OutputTarget::Ppm(Path::new("/tmp/zombiebox-street.ppm")))?;
    scene_interior(OutputTarget::Ppm(Path::new("/tmp/zombiebox-interior.ppm")))?;
    scene_upstairs(OutputTarget::Ppm(Path::new("/tmp/zombiebox-upstairs.ppm")))?;
    scene_combat(OutputTarget::Ppm(Path::new("/tmp/zombiebox-combat.ppm")))?;
    benchmark_800x360("Birch Street (standard)", None);
    benchmark_800x360("Birch Street (100 stress)", Some(100));
    Ok(())
}
