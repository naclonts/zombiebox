//! Screen-space level of detail selection for articulated actors.
use glam::{Mat4, Vec3};

use crate::{CameraFrame, RenderAabb};

/// Selects a zombie mesh LOD from its projected full character height in output pixels.
/// Camera projection uses `viewport_size` (the final output surface), so adaptive internal
/// render resolution does not change the selected model.
#[must_use]
pub const fn zombie_lod_for_pixel_height(pixel_height: f32) -> u8 {
    if pixel_height >= 250.0 {
        0
    } else if pixel_height >= 100.0 {
        1
    } else if pixel_height >= 35.0 {
        2
    } else {
        3
    }
}

/// Projects the upright-axis extent of the local character bounds into output screen pixels.
/// If either endpoint is behind a perspective near plane, callers can use the highest detail
/// model after they have confirmed the transformed character bound intersects the frustum.
#[must_use]
pub fn projected_actor_height_px(
    camera: &CameraFrame,
    root: Mat4,
    bounds: RenderAabb,
) -> Option<f32> {
    // Use the upright character axis through the bounds center. Projecting the corners of a
    // T-pose AABB counts arm span as character height in an isometric view and causes premature
    // LOD0 selection. Root rotation naturally shortens a fallen actor's projected height.
    let center = (bounds.min + bounds.max) * 0.5;
    let bottom = root.transform_point3(Vec3::new(center.x, bounds.min.y, center.z));
    let top = root.transform_point3(Vec3::new(center.x, bounds.max.y, center.z));
    let bottom = camera.project_to_screen(bottom)?;
    let top = camera.project_to_screen(top)?;
    Some((top.px.y - bottom.px.y).abs())
}

/// Conservative world-space AABB obtained by transforming all eight corners of a local actor
/// bound. Use this for frustum rejection before interpreting an unprojectable height as a near
/// plane crossing.
#[must_use]
pub fn transformed_actor_bounds(root: Mat4, bounds: RenderAabb) -> RenderAabb {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for x in [bounds.min.x, bounds.max.x] {
        for y in [bounds.min.y, bounds.max.y] {
            for z in [bounds.min.z, bounds.max.z] {
                let point = root.transform_point3(Vec3::new(x, y, z));
                min = min.min(point);
                max = max.max(point);
            }
        }
    }
    RenderAabb::new(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CameraMode, CameraRig, CameraTarget};
    use glam::UVec2;

    #[test]
    fn zombie_lod_thresholds_are_inclusive() {
        assert_eq!(zombie_lod_for_pixel_height(250.0), 0);
        assert_eq!(zombie_lod_for_pixel_height(249.99), 1);
        assert_eq!(zombie_lod_for_pixel_height(100.0), 1);
        assert_eq!(zombie_lod_for_pixel_height(99.99), 2);
        assert_eq!(zombie_lod_for_pixel_height(35.0), 2);
        assert_eq!(zombie_lod_for_pixel_height(34.99), 3);
        assert_eq!(zombie_lod_for_pixel_height(0.0), 3);
    }

    #[test]
    fn projected_height_uses_output_pixels_and_scales_with_distance() {
        let make_camera = |internal: UVec2| {
            let mut rig = CameraRig::new(800, 600);
            rig.set_mode(CameraMode::Chase);
            rig.set_viewport_and_internal_size(UVec2::new(800, 600), internal);
            rig.update(CameraTarget::default(), 0.0)
        };
        let full = make_camera(UVec2::new(800, 600));
        let half = make_camera(UVec2::new(400, 300));
        let bounds = RenderAabb::new(Vec3::new(-0.2, 0.0, -0.2), Vec3::new(0.2, 1.0, 0.2));
        let root = Mat4::from_translation(Vec3::new(0.0, 0.0, -4.0));
        let full_height = projected_actor_height_px(&full, root, bounds).unwrap();
        let half_height = projected_actor_height_px(&half, root, bounds).unwrap();
        assert!((full_height - half_height).abs() < 0.01);

        let farther = Mat4::from_translation(Vec3::new(0.0, 0.0, -12.0));
        let far_height = projected_actor_height_px(&full, farther, bounds).unwrap();
        assert!(far_height < full_height);
        assert!(far_height > 0.0);
    }

    #[test]
    fn perspective_near_plane_failure_is_explicit() {
        let mut rig = CameraRig::new(800, 600);
        rig.set_mode(CameraMode::Chase);
        let camera = rig.update(CameraTarget::default(), 0.0);
        let bounds = RenderAabb::new(Vec3::new(-0.2, 0.0, -0.2), Vec3::new(0.2, 1.0, 0.2));
        let near_eye = Mat4::from_translation(camera.eye + camera.forward * 0.05);
        let world_bounds = transformed_actor_bounds(near_eye, bounds);
        assert!(camera.frustum.intersects_aabb(world_bounds));
        assert_eq!(projected_actor_height_px(&camera, near_eye, bounds), None);
    }
}
