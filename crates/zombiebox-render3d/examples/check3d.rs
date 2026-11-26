//! Reproducible headless presentation smoke run. `cargo run -p zombiebox-render3d --example check3d --features gpu`.
use std::{error::Error, path::Path, time::Instant};
use zombiebox_core::{
    ActionGame, ActionInput, DamageKind, Ground, LandmarkKind, Position, Tile3D, TileKind, Vec2,
    WORLD_SIZE, Water,
};
use zombiebox_render3d::{GpuContext, Presentation};
const OUT: &str = "/tmp/zombiebox-3d-qa";
fn capture(
    ctx: &GpuContext,
    p: &mut Presentation,
    game: &ActionGame,
    name: &str,
) -> Result<(), Box<dyn Error>> {
    let start = Instant::now();
    let img = p.render_offscreen(ctx, game, 1.0 / 60.0)?;
    let elapsed = start.elapsed();
    let unique = img
        .pixels
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect::<std::collections::HashSet<_>>()
        .len();
    if unique < 8 {
        return Err(format!("{name}: image has only {unique} colors").into());
    }
    img.save_png(Path::new(OUT).join(format!("{name}.png")))?;
    let s = p.last_frame_stats();
    println!(
        "{name}: {:.1}ms, {unique} colors, {} / {} chunks, {} instances, {} triangles",
        elapsed.as_secs_f64() * 1000.0,
        s.chunks_drawn,
        s.chunks_resident,
        s.instances_drawn,
        s.triangles_submitted
    );
    Ok(())
}
fn teleport(
    game: &mut ActionGame,
    p: &mut Presentation,
    target: Position,
    floor: u8,
) -> Result<(), Box<dyn Error>> {
    let dest = game
        .teleport(Tile3D::from_pos(target, floor))
        .ok_or("teleport failed")?;
    println!("teleport {target:?}/{floor} -> {dest:?}");
    p.notify_teleport();
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(OUT)?;
    let ctx = GpuContext::new_headless()?;
    println!("adapter: {:?}", ctx.adapter_info);
    let mut p = Presentation::new(&ctx, 640, 360);
    let mut birch = ActionGame::residential(42);
    capture(&ctx, &mut p, &birch, "birch-outdoors")?;
    let house = birch.buildings()[0];
    let mut wall = None;
    for y in house.y..house.y + house.height {
        for x in house.x..house.x + house.width {
            let pos = Position::new(x, y);
            if birch
                .tile_render_info(pos, 0)
                .is_some_and(|t| t.kind == TileKind::ExteriorWall)
            {
                wall = Some(Tile3D::from_pos(pos, 0));
                break;
            }
        }
        if wall.is_some() {
            break;
        }
    }
    let wall = wall.ok_or("no Birch wall")?;
    let before = birch.tile_render_info(wall.pos(), wall.floor).unwrap();
    for _ in 0..8 {
        let _ = birch.damage_tile(wall, DamageKind::RocketBlast);
    }
    let after = birch.tile_render_info(wall.pos(), wall.floor).unwrap();
    println!(
        "Birch damage {wall:?}: {:?} -> {:?}",
        before.kind, after.kind
    );
    if before.kind == after.kind {
        return Err("wall not destroyed".into());
    }
    p.reset();
    capture(&ctx, &mut p, &birch, "birch-damaged")?;
    // Walk toward the front entrance using real inputs. Record whether collision permits entry.
    let door = Vec2::new(house.front_door.x as f32, house.front_door.y as f32);
    let goal = Vec2::new(
        (house.x + house.width / 2) as f32,
        (house.y + house.height / 2) as f32,
    );
    for _ in 0..500 {
        let delta = goal - birch.player_pose().position;
        if (door - birch.player_pose().position).length() < 1.2 {
            birch.step(ActionInput {
                interact: true,
                ..Default::default()
            });
        }
        birch.step(ActionInput {
            movement: delta.normalized(),
            ..Default::default()
        });
        p.observe_tick(&birch);
        if birch.hud().indoors {
            break;
        }
    }
    println!(
        "Birch walk: pos {:?}, indoors {}",
        birch.player_pose().position,
        birch.hud().indoors
    );
    assert!(birch.hud().indoors, "walk never reached the Birch interior");
    capture(&ctx, &mut p, &birch, "birch-walk")?;
    let mut horde = ActionGame::residential(42);
    horde.spawn_stress_monsters(160);
    p.reset();
    capture(&ctx, &mut p, &horde, "birch-horde")?;
    assert!(
        p.last_frame_stats().instances_drawn > 100,
        "horde did not exercise batched draws"
    );
    let mut county = ActionGame::county(42);
    p.reset();
    capture(&ctx, &mut p, &county, "county-spawn")?;
    let landmarks = county.world().landmarks().to_vec();
    for (kind, name) in [
        (LandmarkKind::Downtown, "downtown"),
        (LandmarkKind::Hospital, "hospital"),
        (LandmarkKind::Cabin, "cabin"),
    ] {
        if let Some(l) = landmarks.iter().find(|l| l.kind == kind) {
            teleport(&mut county, &mut p, l.entrance, 0)?;
            capture(&ctx, &mut p, &county, name)?;
        }
    }
    let mut bridge = None;
    'outer: for y in 0..WORLD_SIZE {
        for x in 0..WORLD_SIZE {
            let pos = Position::new(x, y);
            if county
                .world()
                .tile_at(pos)
                .is_some_and(|t| t.ground == Ground::Bridge && t.water == Some(Water::River))
            {
                bridge = Some(pos);
                break 'outer;
            }
        }
    }
    if let Some(pos) = bridge {
        teleport(&mut county, &mut p, pos, 0)?;
        capture(&ctx, &mut p, &county, "river-bridge")?;
    }
    let hospital = landmarks
        .iter()
        .find(|l| l.kind == LandmarkKind::Hospital)
        .ok_or("missing hospital")?
        .entrance;
    teleport(&mut county, &mut p, hospital, 0)?;
    capture(&ctx, &mut p, &county, "hospital-revisit")?;
    let mut fresh = ActionGame::county(42);
    p.reset();
    teleport(&mut fresh, &mut p, hospital, 0)?;
    capture(&ctx, &mut p, &fresh, "hospital-reset")?;
    assert_eq!(
        std::fs::read(Path::new(OUT).join("hospital-revisit.png"))?,
        std::fs::read(Path::new(OUT).join("hospital-reset.png"))?,
        "revisited scene differs after reset"
    );
    // Sample frame times while moving across the current streaming window.
    let start = Instant::now();
    let mut max = 0.0_f64;
    let start_chunk = (
        county.player_tile().x.div_euclid(16),
        county.player_tile().y.div_euclid(16),
    );
    for _ in 0..180 {
        county.step(ActionInput {
            movement: Vec2::new(1.0, 0.0),
            ..Default::default()
        });
        p.observe_tick(&county);
        let tick = Instant::now();
        let _ = p.render_offscreen(&ctx, &county, 1.0 / 60.0)?;
        max = max.max(tick.elapsed().as_secs_f64() * 1000.0);
    }
    println!(
        "180 moving frames: mean {:.1}ms, max {:.1}ms, end {:?}",
        start.elapsed().as_secs_f64() * 1000.0 / 180.0,
        max,
        county.player_tile()
    );
    let end = county.player_tile();
    assert_ne!(
        start_chunk,
        (end.x.div_euclid(16), end.y.div_euclid(16)),
        "movement did not cross a chunk edge"
    );
    Ok(())
}
