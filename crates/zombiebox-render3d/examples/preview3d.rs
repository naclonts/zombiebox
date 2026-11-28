//! Headless diorama & terrain preview CLI supporting T03 diorama and T04 Birch Street / County Seed 42 scenes.

use glam::{UVec2, Vec3};
use std::path::PathBuf;
use zombiebox_core::{
    ActionGame, CountyFixture, Ground, LandmarkKind, Position, Tile3D, WORLD_SIZE, Water, World,
};
use zombiebox_render3d::coords::{heading_to_render_quat, sim_to_render_point};
use zombiebox_render3d::{
    CameraMode, CameraRig, CameraTarget, CardinalDir, ChunkCoord, FaceMask, GeometryTag,
    GpuContext, GpuRenderer, InstanceBatch, InstanceRaw, InternalResolutionPolicy, MaterialFlags,
    MeshBuilder, PointLightGpu, Presentation, RenderPipelineConfig, RidgeAxis,
    SceneLightingUniforms, SceneManager, SimVec3, TerrainMeshConfig, VertexMeta,
    sync_terrain_chunks_to_gpu,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SceneChoice {
    All,
    Diorama,
    BirchStreet,
    CountySpawn,
    CountyRiverBridge,
    CountyDowntown,
    CountyWoods,
    CountyCraftsman,
    CountyResidentialStreet,
    CountyMountains,
}

impl SceneChoice {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "all" => Ok(Self::All),
            "diorama" => Ok(Self::Diorama),
            "birch-street" => Ok(Self::BirchStreet),
            "spawn" | "county-spawn" => Ok(Self::CountySpawn),
            "river-bridge" | "county-river-bridge" => Ok(Self::CountyRiverBridge),
            "downtown" | "county-downtown" => Ok(Self::CountyDowntown),
            "woods" | "county-woods" => Ok(Self::CountyWoods),
            "craftsman" | "craftsman-lot" => Ok(Self::CountyCraftsman),
            "residential-street" => Ok(Self::CountyResidentialStreet),
            "mountains" | "county-mountains" => Ok(Self::CountyMountains),
            other => Err(format!("Unknown --scene '{other}'")),
        }
    }
}

fn parse_camera_mode(s: &str) -> Result<CameraMode, String> {
    match s {
        "iso" | "isometric" => Ok(CameraMode::Isometric),
        "orbit" => Ok(CameraMode::Orbit),
        "chase" => Ok(CameraMode::Chase),
        "overhead" => Ok(CameraMode::Overhead),
        other => Err(format!("Unknown --mode '{other}'")),
    }
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w_str, h_str) = s
        .split_once('x')
        .ok_or_else(|| format!("Invalid --size '{s}', expected WxH"))?;
    let w: u32 = w_str
        .parse()
        .map_err(|_| format!("Invalid width in --size '{s}'"))?;
    let h: u32 = h_str
        .parse()
        .map_err(|_| format!("Invalid height in --size '{s}'"))?;
    if w == 0 || h == 0 {
        return Err(format!("--size dimensions must be > 0, got '{s}'"));
    }
    Ok((w, h))
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
                Some((bc, bd, _)) => river_count > bc || (river_count == bc && dist2 < bd),
            };
            if replace {
                best = Some((river_count, dist2, pos));
            }
        }
    }
    best.expect("county has at least one river bridge").2
}

fn find_landmark(world: &World, kind: LandmarkKind) -> Position {
    world
        .landmarks()
        .iter()
        .find(|l| l.kind == kind)
        .map(|l| l.entrance)
        .unwrap_or_else(|| world.wake_position())
}

fn render_diorama(
    ctx: &GpuContext,
    out_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = GpuRenderer::new(
        ctx,
        RenderPipelineConfig {
            clear_color_rgba: [18.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 1.0],
            resolution_policy: InternalResolutionPolicy::FromCameraFrame,
            frustum_cull_chunks: true,
        },
    );

    let mut world_mesh = MeshBuilder::new();
    let grass_color = rgba8(58, 120, 68);
    let road_color = rgba8(45, 48, 55);
    let stripe_color = rgba8(240, 220, 80);
    let water_color = rgba8(35, 90, 130);

    world_mesh.push_quad(
        sim_to_render_point(SimVec3::new(-16.0, -16.0, 0.0)),
        sim_to_render_point(SimVec3::new(16.0, -16.0, 0.0)),
        sim_to_render_point(SimVec3::new(16.0, 16.0, 0.0)),
        sim_to_render_point(SimVec3::new(-16.0, 16.0, 0.0)),
        grass_color,
        VertexMeta::default().with_tag(GeometryTag::Terrain),
    );
    world_mesh.push_quad(
        sim_to_render_point(SimVec3::new(-16.0, -2.0, 0.01)),
        sim_to_render_point(SimVec3::new(16.0, -2.0, 0.01)),
        sim_to_render_point(SimVec3::new(16.0, 2.0, 0.01)),
        sim_to_render_point(SimVec3::new(-16.0, 2.0, 0.01)),
        road_color,
        VertexMeta::default().with_tag(GeometryTag::Road),
    );
    world_mesh.push_quad(
        sim_to_render_point(SimVec3::new(-16.0, -0.1, 0.02)),
        sim_to_render_point(SimVec3::new(16.0, -0.1, 0.02)),
        sim_to_render_point(SimVec3::new(16.0, 0.1, 0.02)),
        sim_to_render_point(SimVec3::new(-16.0, 0.1, 0.02)),
        stripe_color,
        VertexMeta::default()
            .with_tag(GeometryTag::Road)
            .with_flags(MaterialFlags::UNLIT),
    );
    world_mesh.push_quad(
        sim_to_render_point(SimVec3::new(10.0, -16.0, -0.45)),
        sim_to_render_point(SimVec3::new(16.0, -16.0, -0.45)),
        sim_to_render_point(SimVec3::new(16.0, 16.0, -0.45)),
        sim_to_render_point(SimVec3::new(10.0, 16.0, -0.45)),
        water_color,
        VertexMeta::default().with_tag(GeometryTag::Water),
    );
    zombiebox_render3d::mesh::primitives::emit_sim_box_min_max(
        &mut world_mesh,
        SimVec3::new(10.0, -2.0, 0.0),
        SimVec3::new(16.0, 2.0, 0.45),
        FaceMask::TOP_AND_SIDES,
        rgba8(110, 115, 125),
        VertexMeta::default().with_tag(GeometryTag::Bridge),
    );
    zombiebox_render3d::mesh::primitives::emit_sim_ramp(
        &mut world_mesh,
        SimVec3::new(7.0, -2.0, 0.0),
        SimVec3::new(10.0, 2.0, 0.45),
        CardinalDir::EastPosX,
        true,
        rgba8(110, 115, 125),
        VertexMeta::default().with_tag(GeometryTag::Bridge),
    );
    zombiebox_render3d::mesh::primitives::emit_sim_box_min_max(
        &mut world_mesh,
        SimVec3::new(-8.0, -8.0, 0.0),
        SimVec3::new(-2.0, -2.0, 1.388),
        FaceMask::TOP_AND_SIDES,
        rgba8(180, 175, 160),
        VertexMeta::default()
            .with_tag(GeometryTag::WallLower)
            .with_building(1, 1),
    );
    zombiebox_render3d::mesh::primitives::emit_sim_box_min_max(
        &mut world_mesh,
        SimVec3::new(-8.0, -8.0, 1.388),
        SimVec3::new(-2.0, -2.0, 1.488),
        FaceMask::TOP_AND_SIDES,
        rgba8(140, 110, 80),
        VertexMeta::default()
            .with_tag(GeometryTag::InteriorFloor)
            .with_building(1, 2),
    );
    zombiebox_render3d::mesh::primitives::emit_sim_box_min_max(
        &mut world_mesh,
        SimVec3::new(-8.0, -8.0, 1.488),
        SimVec3::new(-2.0, -2.0, 2.776),
        FaceMask::TOP_AND_SIDES,
        rgba8(190, 185, 170),
        VertexMeta::default()
            .with_tag(GeometryTag::WallUpper)
            .with_building(1, 2),
    );
    zombiebox_render3d::mesh::primitives::emit_gable_roof(
        &mut world_mesh,
        sim_to_render_point(SimVec3::new(-8.0, -8.0, 2.776)),
        sim_to_render_point(SimVec3::new(-2.0, -2.0, 3.800)),
        RidgeAxis::AlongX,
        true,
        rgba8(90, 95, 110),
        VertexMeta::default()
            .with_tag(GeometryTag::Roof)
            .with_building(1, 2),
    );
    zombiebox_render3d::mesh::primitives::emit_sim_ramp(
        &mut world_mesh,
        SimVec3::new(-7.0, -7.0, 0.0),
        SimVec3::new(-5.0, -5.0, 1.388),
        CardinalDir::NorthNegZ,
        false,
        rgba8(130, 100, 70),
        VertexMeta::default()
            .with_tag(GeometryTag::InteriorFloor)
            .with_building(1, 1),
    );
    for lamp_pos in [SimVec3::new(-3.0, -3.0, 0.0), SimVec3::new(3.0, 3.0, 0.0)] {
        zombiebox_render3d::mesh::primitives::emit_sim_cylinder(
            &mut world_mesh,
            lamp_pos,
            0.1,
            0.1,
            2.5,
            6,
            true,
            false,
            rgba8(80, 85, 90),
            VertexMeta::default(),
        );
        zombiebox_render3d::mesh::primitives::emit_sim_cylinder(
            &mut world_mesh,
            SimVec3::new(lamp_pos.x, lamp_pos.y, 2.5),
            0.3,
            0.3,
            0.4,
            6,
            true,
            true,
            rgba8(255, 230, 120),
            VertexMeta::default().with_flags(MaterialFlags::UNLIT),
        );
    }
    for tree_pos in [
        SimVec3::new(5.0, -6.0, 0.0),
        SimVec3::new(7.0, -7.5, 0.0),
        SimVec3::new(6.0, 6.0, 0.0),
    ] {
        zombiebox_render3d::mesh::primitives::emit_sim_cylinder(
            &mut world_mesh,
            tree_pos,
            0.25,
            0.2,
            1.2,
            5,
            true,
            false,
            rgba8(100, 75, 50),
            VertexMeta::default(),
        );
        zombiebox_render3d::mesh::primitives::emit_cone(
            &mut world_mesh,
            sim_to_render_point(SimVec3::new(tree_pos.x, tree_pos.y, 1.0)),
            1.2,
            2.2,
            6,
            true,
            rgba8(40, 95, 55),
            VertexMeta::default(),
        );
    }

    renderer.upload_chunk(
        ctx,
        ChunkCoord::new(0, 0),
        1,
        world_mesh.as_mesh().compute_aabb().unwrap(),
        world_mesh.as_mesh(),
    );

    let mut actor_mesh = MeshBuilder::new();
    zombiebox_render3d::mesh::primitives::emit_cylinder(
        &mut actor_mesh,
        Vec3::ZERO,
        0.35,
        0.3,
        1.1,
        6,
        true,
        true,
        rgba8(220, 220, 230),
        VertexMeta::default().with_tag(GeometryTag::Actor),
    );
    zombiebox_render3d::mesh::primitives::emit_cylinder(
        &mut actor_mesh,
        Vec3::new(0.0, 1.1, 0.0),
        0.25,
        0.25,
        0.35,
        6,
        true,
        true,
        rgba8(240, 200, 180),
        VertexMeta::default().with_tag(GeometryTag::Actor),
    );
    let actor_proto = renderer.register_prototype(ctx, "actor_totem", actor_mesh.as_mesh());

    let mut crate_mesh = MeshBuilder::new();
    zombiebox_render3d::mesh::primitives::emit_cuboid(
        &mut crate_mesh,
        Vec3::ZERO,
        Vec3::splat(0.4),
        FaceMask::ALL,
        rgba8(140, 90, 50),
        VertexMeta::default().with_tag(GeometryTag::Prop),
    );
    let crate_proto = renderer.register_prototype(ctx, "loot_crate", crate_mesh.as_mesh());

    let survivor_instances = vec![
        InstanceRaw::from_trs(Vec3::ZERO, glam::Quat::IDENTITY, Vec3::ONE)
            .with_tint([1.0, 1.0, 1.0, 1.0]),
    ];
    let infected_instances = vec![
        InstanceRaw::from_trs(
            sim_to_render_point(SimVec3::new(3.0, 2.0, 0.0)),
            heading_to_render_quat(0.5),
            Vec3::ONE,
        )
        .with_tint([0.8, 0.3, 0.3, 1.0]),
        InstanceRaw::from_trs(
            sim_to_render_point(SimVec3::new(4.5, 3.2, 0.0)),
            heading_to_render_quat(1.2),
            Vec3::ONE,
        )
        .with_tint([0.7, 0.2, 0.2, 1.0]),
        InstanceRaw::from_trs(
            sim_to_render_point(SimVec3::new(-3.5, 4.0, 0.0)),
            heading_to_render_quat(-2.0),
            Vec3::ONE,
        )
        .with_tint([0.75, 0.25, 0.25, 1.0]),
    ];
    let crate_instances = vec![
        InstanceRaw::from_trs(
            sim_to_render_point(SimVec3::new(-2.5, 1.0, 0.0)),
            glam::Quat::IDENTITY,
            Vec3::ONE,
        ),
        InstanceRaw::from_trs(
            sim_to_render_point(SimVec3::new(-2.8, 1.5, 0.0)),
            glam::Quat::IDENTITY,
            Vec3::ONE,
        ),
        InstanceRaw::from_trs(
            sim_to_render_point(SimVec3::new(6.0, -3.0, 0.45)),
            glam::Quat::IDENTITY,
            Vec3::ONE,
        ),
    ];

    let instances = vec![
        InstanceBatch {
            mesh: actor_proto,
            instances: &survivor_instances,
        },
        InstanceBatch {
            mesh: actor_proto,
            instances: &infected_instances,
        },
        InstanceBatch {
            mesh: crate_proto,
            instances: &crate_instances,
        },
    ];

    let mut lighting = SceneLightingUniforms::dusk_default();
    lighting.set_point_lights(&[
        PointLightGpu::new(
            sim_to_render_point(SimVec3::new(-3.0, -3.0, 2.5)),
            8.0,
            [1.0, 0.85, 0.5],
            1.2,
        ),
        PointLightGpu::new(
            sim_to_render_point(SimVec3::new(3.0, 3.0, 2.5)),
            8.0,
            [1.0, 0.85, 0.5],
            1.2,
        ),
    ]);

    let captures = [
        (
            "preview-iso-16x9-native-640x360.png",
            CameraMode::Isometric,
            640,
            360,
            640,
            360,
        ),
        (
            "preview-iso-16x9-upscaled-1280x720.png",
            CameraMode::Isometric,
            1280,
            720,
            640,
            360,
        ),
        (
            "preview-iso-4x3-upscaled-960x720.png",
            CameraMode::Isometric,
            960,
            720,
            480,
            360,
        ),
        (
            "preview-iso-20x9-upscaled-1600x720.png",
            CameraMode::Isometric,
            1600,
            720,
            800,
            360,
        ),
        (
            "preview-orbit-16x9-upscaled-1280x720.png",
            CameraMode::Orbit,
            1280,
            720,
            640,
            360,
        ),
        (
            "preview-chase-20x9-upscaled-1600x720.png",
            CameraMode::Chase,
            1600,
            720,
            800,
            360,
        ),
    ];

    for (filename, mode, sw, sh, iw, ih) in captures {
        let mut rig = CameraRig::new(sw, sh);
        rig.set_mode(mode);
        if sw == iw && sh == ih {
            rig.set_viewport(sw, sh);
        } else {
            rig.set_viewport_and_internal_size(UVec2::new(sw, sh), UVec2::new(iw, ih));
        }
        if mode == CameraMode::Orbit {
            rig.orbit_rotate(0.55, 0.08);
        }
        let frame = rig.update(CameraTarget::default(), 0.016);
        let img = renderer.render_offscreen_rgba8(ctx, &frame, &lighting, &instances, None)?;
        img.save_png(out_dir.join(filename))?;
        let stats = renderer.last_frame_stats();
        println!(
            "Saved {filename} | Mode: {:?} | Chunks: {}/{} | Triangles: {}",
            mode, stats.chunks_drawn, stats.chunks_resident, stats.triangles_submitted
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_terrain_capture(
    ctx: &GpuContext,
    renderer: &mut GpuRenderer,
    mgr: &mut SceneManager,
    game: &ActionGame,
    focus_sim: SimVec3,
    mode: CameraMode,
    size: (u32, u32),
    out_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut rig = CameraRig::new(size.0, size.1);
    rig.set_mode(mode);
    if size.0 > 800 || size.1 > 450 {
        rig.set_viewport_and_internal_size(
            UVec2::new(size.0, size.1),
            UVec2::new(size.0 / 2, size.1 / 2),
        );
    } else {
        rig.set_viewport(size.0, size.1);
    }
    if mode == CameraMode::Orbit {
        rig.orbit_rotate(0.65, -0.05);
    }
    let cam_frame = rig.update(
        CameraTarget {
            position: focus_sim,
            ..Default::default()
        },
        0.016,
    );
    let scene_frame = mgr.prepare(game, &cam_frame, 0.016);
    sync_terrain_chunks_to_gpu(ctx, renderer, &scene_frame, &TerrainMeshConfig::default());

    let lighting = SceneLightingUniforms::dusk_default();
    let img = renderer.render_offscreen_rgba8(ctx, &cam_frame, &lighting, &[], None)?;
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    img.save_png(out_path)?;
    let stats = renderer.last_frame_stats();
    println!(
        "Saved {} | Mode: {:?} | {}x{} | Chunks: {}/{} | Triangles: {}",
        out_path.display(),
        mode,
        size.0,
        size.1,
        stats.chunks_drawn,
        stats.chunks_resident,
        stats.triangles_submitted
    );
    Ok(())
}

fn focus_for_county_scene(world: &World, county: &mut ActionGame, scene: SceneChoice) -> SimVec3 {
    let target_pos = match scene {
        SceneChoice::CountySpawn => county.player_tile().pos(),
        SceneChoice::CountyRiverBridge => find_river_bridge(world),
        SceneChoice::CountyDowntown => find_landmark(world, LandmarkKind::Downtown),
        SceneChoice::CountyWoods => find_landmark(world, LandmarkKind::Cabin),
        SceneChoice::CountyCraftsman | SceneChoice::CountyResidentialStreet => {
            world.fixture_plan().map_or_else(
                || county.player_tile().pos(),
                // Stand on the street-side access tile so the roof is not cut away.
                |plan| plan.frontage.access,
            )
        }
        SceneChoice::CountyMountains => find_landmark(world, LandmarkKind::Mountain),
        _ => county.player_tile().pos(),
    };
    county.teleport(Tile3D::from_pos(target_pos, 0));
    county.player_body().position
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut out_dir = PathBuf::from("/tmp/zombiebox-preview3d");
    let mut scene = SceneChoice::All;
    let mut mode = CameraMode::Isometric;
    let mut seed = 42u64;
    let mut focus_name = String::from("overview");
    let mut size: (u32, u32) = (640, 360);
    let mut positional_out: Option<PathBuf> = None;

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--out-dir" if i + 1 < args.len() => {
                out_dir = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            "--focus" if i + 1 < args.len() => {
                focus_name = args[i + 1].clone();
                i += 2;
            }
            "--seed" if i + 1 < args.len() => {
                seed = args[i + 1].parse()?;
                i += 2;
            }
            "--scene" if i + 1 < args.len() => {
                scene = SceneChoice::parse(&args[i + 1])?;
                i += 2;
            }
            "--mode" | "--camera" if i + 1 < args.len() => {
                mode = parse_camera_mode(&args[i + 1])?;
                i += 2;
            }
            "--size" if i + 1 < args.len() => {
                size = parse_size(&args[i + 1])?;
                i += 2;
            }
            arg if !arg.starts_with('-') => {
                positional_out = Some(PathBuf::from(arg));
                i += 1;
            }
            other => {
                return Err(format!("Unknown argument: {other}").into());
            }
        }
    }

    std::fs::create_dir_all(&out_dir)?;

    println!("Initializing headless GPU context for preview3d...");
    let ctx = GpuContext::new_headless()?;

    if scene == SceneChoice::Diorama || scene == SceneChoice::All {
        render_diorama(&ctx, &out_dir)?;
    }

    if scene == SceneChoice::Diorama {
        return Ok(());
    }

    let mut renderer = GpuRenderer::new(
        &ctx,
        RenderPipelineConfig {
            clear_color_rgba: [18.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 1.0],
            resolution_policy: InternalResolutionPolicy::FromCameraFrame,
            frustum_cull_chunks: true,
        },
    );

    if scene == SceneChoice::BirchStreet || scene == SceneChoice::All {
        let res = ActionGame::residential(42);
        let mut mgr = SceneManager::new();
        let out_path = if scene == SceneChoice::BirchStreet {
            positional_out
                .clone()
                .unwrap_or_else(|| out_dir.join("terrain-birch-street-640x360.png"))
        } else {
            out_dir.join("terrain-birch-street-640x360.png")
        };
        let capture_mode = if scene == SceneChoice::All {
            CameraMode::Isometric
        } else {
            mode
        };
        let capture_size = if scene == SceneChoice::All {
            (640, 360)
        } else {
            size
        };
        render_terrain_capture(
            &ctx,
            &mut renderer,
            &mut mgr,
            &res,
            res.player_body().position,
            capture_mode,
            capture_size,
            &out_path,
        )?;
        renderer.chunks_mut().clear();
    }

    if scene != SceneChoice::BirchStreet {
        let world = if scene == SceneChoice::CountyResidentialStreet {
            World::with_fixture(seed, CountyFixture::ResidentialStreet)
        } else {
            World::new(42)
        };
        let mut county = ActionGame::county_with_world(world.clone());
        let mut mgr = SceneManager::new();
        if scene == SceneChoice::CountyResidentialStreet {
            for plan in world.fixture_plans() {
                println!(
                    "parcel {:?}..{:?}, frontage {:?}, structures {:?}, slots {:?}",
                    plan.bounds.min,
                    plan.bounds.max,
                    plan.frontage,
                    plan.structures
                        .iter()
                        .map(|s| (s.name, s.front_door))
                        .collect::<Vec<_>>(),
                    plan.outdoor.vehicle_slots
                );
            }
            let plans = world.fixture_plans();
            if plans.len() != 2 {
                return Err("no compatible reference street".into());
            }
            let a = plans[0].structures[0].front_door;
            let b = plans[1].structures[0].front_door;
            let (ix, iy) = plans[0].frontage.side.inward();
            let target = match focus_name.as_str() {
                "overview" => Position::new((a.x + b.x) / 2 - ix * 8, (a.y + b.y) / 2 - iy * 8),
                "ranch" | "craftsman" => {
                    let plan = plans
                        .iter()
                        .find(|p| {
                            p.structures[0]
                                .name
                                .to_ascii_lowercase()
                                .contains(&focus_name)
                        })
                        .ok_or("requested house not found")?;
                    let structure = &plan.structures[0];
                    let door = structure.front_door;
                    let bounds = structure.bounds;
                    let (cx, cy) = (
                        (bounds.min.x + bounds.max.x) / 2,
                        (bounds.min.y + bounds.max.y) / 2,
                    );
                    if ix == 0 {
                        Position::new(cx, door.y - iy * 6)
                    } else {
                        Position::new(door.x - ix * 6, cy)
                    }
                }
                _ => return Err("--focus must be overview, ranch or craftsman".into()),
            };
            println!(
                "street focus {target:?}; teleport {:?}",
                county.teleport(Tile3D::from_pos(target, 0))
            );

            let mut presentation = Presentation::new(&ctx, size.0, size.1);
            presentation.camera_mut().set_mode(mode);
            if mode == CameraMode::Orbit {
                presentation.camera_mut().orbit_rotate(
                    (-(iy as f32)).atan2(-(ix as f32)) - std::f32::consts::FRAC_PI_4,
                    -0.15,
                );
                presentation
                    .camera_mut()
                    .orbit_scale_distance(if focus_name == "overview" { 2.5 } else { 1.3 });
            }
            let out_path = positional_out.unwrap_or_else(|| {
                out_dir.join(format!(
                    "residential-street-s{seed}-{focus_name}-{mode:?}.png"
                ))
            });
            let start = std::time::Instant::now();
            presentation
                .render_offscreen(&ctx, &county, 1.0 / 60.0)?
                .save_png(&out_path)?;
            let stats = presentation.last_frame_stats();
            println!(
                "Saved {} ({mode:?}, seed {seed}): {:?}, {} / {} chunks, {} instances, {} triangles",
                out_path.display(),
                start.elapsed(),
                stats.chunks_drawn,
                stats.chunks_resident,
                stats.instances_drawn,
                stats.triangles_submitted
            );
            return Ok(());
        }

        if scene == SceneChoice::All {
            let batch = [
                (
                    SceneChoice::CountySpawn,
                    CameraMode::Isometric,
                    (640, 360),
                    "terrain-county-s42-spawn-640x360.png",
                ),
                (
                    SceneChoice::CountyRiverBridge,
                    CameraMode::Isometric,
                    (640, 360),
                    "terrain-county-s42-river-bridge-iso-640x360.png",
                ),
                (
                    SceneChoice::CountyRiverBridge,
                    CameraMode::Orbit,
                    (1280, 720),
                    "terrain-county-s42-river-bridge-orbit-1280x720.png",
                ),
                (
                    SceneChoice::CountyDowntown,
                    CameraMode::Isometric,
                    (640, 360),
                    "terrain-county-s42-downtown-640x360.png",
                ),
                (
                    SceneChoice::CountyWoods,
                    CameraMode::Isometric,
                    (640, 360),
                    "terrain-county-s42-woods-640x360.png",
                ),
                (
                    SceneChoice::CountyMountains,
                    CameraMode::Orbit,
                    (1280, 720),
                    "terrain-county-s42-mountains-orbit-1280x720.png",
                ),
            ];
            for (sc, cam_mode, sz, filename) in batch {
                let focus = focus_for_county_scene(&world, &mut county, sc);
                mgr.notify_teleport();
                render_terrain_capture(
                    &ctx,
                    &mut renderer,
                    &mut mgr,
                    &county,
                    focus,
                    cam_mode,
                    sz,
                    &out_dir.join(filename),
                )?;
            }
        } else {
            let focus = focus_for_county_scene(&world, &mut county, scene);
            mgr.notify_teleport();
            let default_name = match scene {
                SceneChoice::CountySpawn => "terrain-county-s42-spawn.png",
                SceneChoice::CountyRiverBridge => "terrain-county-s42-river-bridge.png",
                SceneChoice::CountyDowntown => "terrain-county-s42-downtown.png",
                SceneChoice::CountyWoods => "terrain-county-s42-woods.png",
                SceneChoice::CountyCraftsman => "terrain-county-s42-craftsman.png",
                SceneChoice::CountyResidentialStreet => "terrain-county-s42-residential-street.png",
                SceneChoice::CountyMountains => "terrain-county-s42-mountains.png",
                _ => "terrain-county-s42.png",
            };
            let out_path = positional_out.unwrap_or_else(|| out_dir.join(default_name));
            render_terrain_capture(
                &ctx,
                &mut renderer,
                &mut mgr,
                &county,
                focus,
                mode,
                size,
                &out_path,
            )?;
        }
    }

    Ok(())
}

fn rgba8(r: u8, g: u8, b: u8) -> [f32; 4] {
    [
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        1.0,
    ]
}
