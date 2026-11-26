//! Instanced-draw regression coverage: batches share one instance buffer.
#![cfg(feature = "gpu")]

use glam::Vec3;
use zombiebox_render3d::{
    CameraMode, CameraRig, CameraTarget, GeometryTag, GpuContext, GpuRenderer, InstanceBatch,
    InstanceRaw, MaterialFlags, MeshBuilder, RenderPipelineConfig, RgbaImage,
    SceneLightingUniforms, VertexMeta,
};

fn red_mask(image: &RgbaImage) -> Vec<bool> {
    image
        .pixels
        .chunks_exact(4)
        .map(|px| px[0] > 150 && px[1] < 80 && px[2] < 80)
        .collect()
}

/// Regression: every instanced batch must draw from its own slice of the shared
/// instance buffer. Encoding a draw call does not snapshot buffer contents, so
/// uploading each batch to offset zero made every batch draw the last batch's
/// instances, leaving survivors, infected, and caches partly or wholly invisible.
#[test]
fn instanced_batches_keep_their_own_instance_ranges() {
    let Ok(ctx) = GpuContext::new_headless() else {
        eprintln!("skipping: no headless GPU adapter");
        return;
    };
    let mut renderer = GpuRenderer::new(&ctx, RenderPipelineConfig::default());
    let lighting = SceneLightingUniforms::unlit_test();
    let mut rig = CameraRig::new(100, 100);
    rig.set_mode(CameraMode::Isometric);
    let camera = rig.update(CameraTarget::default(), 0.0);

    let mut quad = MeshBuilder::new();
    quad.push_quad(
        Vec3::new(-0.5, 0.02, -0.5),
        Vec3::new(-0.5, 0.02, 0.5),
        Vec3::new(0.5, 0.02, 0.5),
        Vec3::new(0.5, 0.02, -0.5),
        [1.0, 0.05, 0.05, 1.0],
        VertexMeta::DEFAULT
            .with_tag(GeometryTag::Terrain)
            .with_flags(MaterialFlags::UNLIT),
    );
    let quad = renderer.register_prototype(&ctx, "range_test_quad", quad.as_mesh());

    let left = [InstanceRaw::from_transform(glam::Mat4::from_translation(
        Vec3::new(-1.5, 0.0, 0.0),
    ))];
    let right = [InstanceRaw::from_transform(glam::Mat4::from_translation(
        Vec3::new(1.5, 0.0, 0.0),
    ))];
    let left_only = [InstanceBatch {
        mesh: quad,
        instances: &left,
    }];
    let right_only = [InstanceBatch {
        mesh: quad,
        instances: &right,
    }];
    let both = [
        InstanceBatch {
            mesh: quad,
            instances: &left,
        },
        InstanceBatch {
            mesh: quad,
            instances: &right,
        },
    ];

    let single_left = renderer
        .render_offscreen_rgba8(&ctx, &camera, &lighting, &left_only, None)
        .unwrap();
    let single_right = renderer
        .render_offscreen_rgba8(&ctx, &camera, &lighting, &right_only, None)
        .unwrap();
    let combined = renderer
        .render_offscreen_rgba8(&ctx, &camera, &lighting, &both, None)
        .unwrap();

    let stats = renderer.last_frame_stats();
    assert_eq!(stats.prototype_batches_drawn, 2, "both batches submitted");
    assert_eq!(stats.instances_drawn, 2, "all instances counted");

    let left_mask = red_mask(&single_left);
    let right_mask = red_mask(&single_right);
    let combined_mask = red_mask(&combined);

    assert!(
        left_mask.iter().any(|&hit| hit),
        "left instance never drawn"
    );
    assert!(
        right_mask.iter().any(|&hit| hit),
        "right instance never drawn"
    );

    for (index, (&l, &r)) in left_mask.iter().zip(right_mask.iter()).enumerate() {
        if l || r {
            assert!(
                combined_mask[index],
                "pixel {index} vanished when both batches shared the instance buffer"
            );
        }
    }
}
