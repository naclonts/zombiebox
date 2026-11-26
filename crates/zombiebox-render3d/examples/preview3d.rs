//! Headless multi-primitive diorama preview CLI supporting multi-camera modes and aspect ratios (`T03`).

use glam::Vec3;
use std::path::PathBuf;
use zombiebox_render3d::coords::{heading_to_render_quat, sim_to_render_point};
use zombiebox_render3d::{
    CameraMode, CameraRig, CardinalDir, ChunkCoord, FaceMask, GeometryTag, GpuContext, GpuRenderer,
    InstanceBatch, InstanceRaw, InternalResolutionPolicy, MaterialFlags, MeshBuilder,
    PointLightGpu, RenderPipelineConfig, RidgeAxis, SceneLightingUniforms, SimVec3, VertexMeta,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut out_dir = PathBuf::from("/tmp/zombiebox-preview3d");
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--out-dir" && i + 1 < args.len() {
            out_dir = PathBuf::from(&args[i + 1]);
            i += 2;
        } else {
            i += 1;
        }
    }
    std::fs::create_dir_all(&out_dir)?;

    println!("Initializing headless GPU context for preview3d...");
    let ctx = GpuContext::new_headless()?;
    let mut renderer = GpuRenderer::new(
        &ctx,
        RenderPipelineConfig {
            clear_color_rgba: [18.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 1.0],
            resolution_policy: InternalResolutionPolicy::FromCameraFrame,
            frustum_cull_chunks: true,
        },
    );

    // Build multi-primitive diorama chunk (-1..=0, -1..=0)
    let mut world_mesh = MeshBuilder::new();

    // 1. Ground grass, sidewalks & roads
    let grass_color = WorldVertexRgba8(58, 120, 68);
    let road_color = WorldVertexRgba8(45, 48, 55);
    let stripe_color = WorldVertexRgba8(240, 220, 80);
    let water_color = WorldVertexRgba8(35, 90, 130);

    // Grass yard
    world_mesh.push_quad(
        sim_to_render_point(SimVec3::new(-16.0, -16.0, 0.0)),
        sim_to_render_point(SimVec3::new(16.0, -16.0, 0.0)),
        sim_to_render_point(SimVec3::new(16.0, 16.0, 0.0)),
        sim_to_render_point(SimVec3::new(-16.0, 16.0, 0.0)),
        grass_color,
        VertexMeta::default().with_tag(GeometryTag::Terrain),
    );

    // East-West road y in -2..=2
    world_mesh.push_quad(
        sim_to_render_point(SimVec3::new(-16.0, -2.0, 0.01)),
        sim_to_render_point(SimVec3::new(16.0, -2.0, 0.01)),
        sim_to_render_point(SimVec3::new(16.0, 2.0, 0.01)),
        sim_to_render_point(SimVec3::new(-16.0, 2.0, 0.01)),
        road_color,
        VertexMeta::default().with_tag(GeometryTag::Road),
    );

    // Yellow centerline
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

    // Sunken water river on East edge
    world_mesh.push_quad(
        sim_to_render_point(SimVec3::new(10.0, -16.0, -0.45)),
        sim_to_render_point(SimVec3::new(16.0, -16.0, -0.45)),
        sim_to_render_point(SimVec3::new(16.0, 16.0, -0.45)),
        sim_to_render_point(SimVec3::new(10.0, 16.0, -0.45)),
        water_color,
        VertexMeta::default().with_tag(GeometryTag::Water),
    );

    // Bridge over river
    zombiebox_render3d::mesh::primitives::emit_sim_box_min_max(
        &mut world_mesh,
        SimVec3::new(10.0, -2.0, 0.0),
        SimVec3::new(16.0, 2.0, 0.45),
        FaceMask::TOP_AND_SIDES,
        WorldVertexRgba8(110, 115, 125),
        VertexMeta::default().with_tag(GeometryTag::Bridge),
    );

    // Approach ramps for bridge
    zombiebox_render3d::mesh::primitives::emit_sim_ramp(
        &mut world_mesh,
        SimVec3::new(7.0, -2.0, 0.0),
        SimVec3::new(10.0, 2.0, 0.45),
        CardinalDir::EastPosX,
        true,
        WorldVertexRgba8(110, 115, 125),
        VertexMeta::default().with_tag(GeometryTag::Bridge),
    );

    // Cutaway Two-Story House (West wing)
    zombiebox_render3d::mesh::primitives::emit_sim_box_min_max(
        &mut world_mesh,
        SimVec3::new(-8.0, -8.0, 0.0),
        SimVec3::new(-2.0, -2.0, 1.388),
        FaceMask::TOP_AND_SIDES,
        WorldVertexRgba8(180, 175, 160),
        VertexMeta::default()
            .with_tag(GeometryTag::WallLower)
            .with_building(1, 1),
    );
    // 2F floor slab
    zombiebox_render3d::mesh::primitives::emit_sim_box_min_max(
        &mut world_mesh,
        SimVec3::new(-8.0, -8.0, 1.388),
        SimVec3::new(-2.0, -2.0, 1.488),
        FaceMask::TOP_AND_SIDES,
        WorldVertexRgba8(140, 110, 80),
        VertexMeta::default()
            .with_tag(GeometryTag::InteriorFloor)
            .with_building(1, 2),
    );
    // 2F walls
    zombiebox_render3d::mesh::primitives::emit_sim_box_min_max(
        &mut world_mesh,
        SimVec3::new(-8.0, -8.0, 1.488),
        SimVec3::new(-2.0, -2.0, 2.776),
        FaceMask::TOP_AND_SIDES,
        WorldVertexRgba8(190, 185, 170),
        VertexMeta::default()
            .with_tag(GeometryTag::WallUpper)
            .with_building(1, 2),
    );
    // Gable roof
    zombiebox_render3d::mesh::primitives::emit_gable_roof(
        &mut world_mesh,
        sim_to_render_point(SimVec3::new(-8.0, -8.0, 2.776)),
        sim_to_render_point(SimVec3::new(-2.0, -2.0, 3.800)),
        RidgeAxis::AlongX,
        true,
        WorldVertexRgba8(90, 95, 110),
        VertexMeta::default()
            .with_tag(GeometryTag::Roof)
            .with_building(1, 2),
    );

    // Interior stairs ramp
    zombiebox_render3d::mesh::primitives::emit_sim_ramp(
        &mut world_mesh,
        SimVec3::new(-7.0, -7.0, 0.0),
        SimVec3::new(-5.0, -5.0, 1.388),
        CardinalDir::NorthNegZ,
        false,
        WorldVertexRgba8(130, 100, 70),
        VertexMeta::default()
            .with_tag(GeometryTag::InteriorFloor)
            .with_building(1, 1),
    );

    // Streetlamps & Pine trees
    for (lamp_pos, _color) in [
        (SimVec3::new(-3.0, -3.0, 0.0), [1.0, 0.85, 0.4]),
        (SimVec3::new(3.0, 3.0, 0.0), [1.0, 0.85, 0.4]),
    ] {
        zombiebox_render3d::mesh::primitives::emit_sim_cylinder(
            &mut world_mesh,
            lamp_pos,
            0.1,
            0.1,
            2.5,
            6,
            true,
            false,
            WorldVertexRgba8(80, 85, 90),
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
            WorldVertexRgba8(255, 230, 120),
            VertexMeta::default().with_flags(MaterialFlags::UNLIT),
        );
    }

    // Pine trees (trunks + cones)
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
            WorldVertexRgba8(100, 75, 50),
            VertexMeta::default(),
        );
        zombiebox_render3d::mesh::primitives::emit_cone(
            &mut world_mesh,
            sim_to_render_point(SimVec3::new(tree_pos.x, tree_pos.y, 1.0)),
            1.2,
            2.2,
            6,
            true,
            WorldVertexRgba8(40, 95, 55),
            VertexMeta::default(),
        );
    }

    renderer.upload_chunk(
        &ctx,
        ChunkCoord::new(0, 0),
        1,
        world_mesh.as_mesh().compute_aabb().unwrap(),
        world_mesh.as_mesh(),
    );

    // Register Prototype Meshes for Instancing (Actor totem & Loot crates)
    let mut actor_mesh = MeshBuilder::new();
    // Body prism
    zombiebox_render3d::mesh::primitives::emit_cylinder(
        &mut actor_mesh,
        Vec3::new(0.0, 0.0, 0.0),
        0.35,
        0.3,
        1.1,
        6,
        true,
        true,
        WorldVertexRgba8(220, 220, 230),
        VertexMeta::default().with_tag(GeometryTag::Actor),
    );
    // Head
    zombiebox_render3d::mesh::primitives::emit_cylinder(
        &mut actor_mesh,
        Vec3::new(0.0, 1.1, 0.0),
        0.25,
        0.25,
        0.35,
        6,
        true,
        true,
        WorldVertexRgba8(240, 200, 180),
        VertexMeta::default().with_tag(GeometryTag::Actor),
    );
    let actor_proto = renderer.register_prototype(&ctx, "actor_totem", actor_mesh.as_mesh());

    let mut crate_mesh = MeshBuilder::new();
    zombiebox_render3d::mesh::primitives::emit_cuboid(
        &mut crate_mesh,
        Vec3::ZERO,
        Vec3::splat(0.4),
        FaceMask::ALL,
        WorldVertexRgba8(140, 90, 50),
        VertexMeta::default().with_tag(GeometryTag::Prop),
    );
    let crate_proto = renderer.register_prototype(&ctx, "loot_crate", crate_mesh.as_mesh());

    let survivor_instances = vec![
        InstanceRaw::from_trs(
            sim_to_render_point(SimVec3::new(0.0, 0.0, 0.0)),
            glam::Quat::IDENTITY,
            Vec3::ONE,
        )
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

    // Lighting setup with point lights at lamps
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

    // 6 Captures
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
            rig.set_viewport_and_internal_size(glam::UVec2::new(sw, sh), glam::UVec2::new(iw, ih));
        }

        if mode == CameraMode::Orbit {
            rig.orbit_rotate(0.55, 0.08);
        }

        let target = zombiebox_render3d::CameraTarget {
            position: SimVec3::new(0.0, 0.0, 0.0),
            ..Default::default()
        };
        let frame = rig.update(target, 0.016);

        let img = renderer.render_offscreen_rgba8(&ctx, &frame, &lighting, &instances, None)?;
        let path = out_dir.join(filename);
        img.save_png(&path)?;

        let stats = renderer.last_frame_stats();
        println!(
            "Saved {filename} | Mode: {:?} | Res: {}x{} -> {}x{} (Upscale: {}) | Chunks: {}/{}, Instances: {}, Triangles: {}",
            mode,
            sw,
            sh,
            iw,
            ih,
            stats.used_upscale_pass,
            stats.chunks_drawn,
            stats.chunks_resident,
            stats.instances_drawn,
            stats.triangles_submitted
        );
    }

    println!(
        "All preview3d diorama exports completed successfully in {}.",
        out_dir.display()
    );
    Ok(())
}

#[allow(non_snake_case)]
fn WorldVertexRgba8(r: u8, g: u8, b: u8) -> [f32; 4] {
    [
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        1.0,
    ]
}
