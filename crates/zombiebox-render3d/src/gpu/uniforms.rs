//! Scene lighting, point lights, and upscale uniform payloads (`T03`).

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

pub const MAX_POINT_LIGHTS: usize = 32;

/// Bitflags in `SceneLightingUniforms::counts_and_flags[1]`.
pub mod shading_flags {
    /// Enables 4x4 ordered Bayer screen-door dissolve using per-instance opacity (`T03`/`T07`/`T09`).
    pub const ENABLE_BAYER_DITHER: u32 = 1 << 0;
    /// Enables lower-floor dimming when `vertex.floor < active_floor` (`T07`, off by default in `T03`).
    pub const ENABLE_LOWER_FLOOR_DIM: u32 = 1 << 1;
    pub const ENABLE_WORLD_VISIBILITY: u32 = 1 << 2;
    /// Keep ceilings and walls intact when the camera is inside the world.
    pub const FIRST_PERSON: u32 = 1 << 3;
}

/// 32-byte point light (streetlamp, porch lamp, survivor halo, muzzle/explosion flash).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct PointLightGpu {
    /// `xyz` = position in renderer `(X, Y, Z)` space, `w` = falloff radius (`> 0.0`).
    pub pos_and_radius: [f32; 4],
    /// `rgb` = light color * intensity, `w` = floor index (`< 0.0` affects all floors).
    pub color_and_floor: [f32; 4],
}

impl PointLightGpu {
    #[must_use]
    pub fn new(render_pos: Vec3, radius: f32, color_rgb: [f32; 3], intensity: f32) -> Self {
        Self {
            pos_and_radius: [render_pos.x, render_pos.y, render_pos.z, radius.max(0.01)],
            color_and_floor: [
                color_rgb[0] * intensity,
                color_rgb[1] * intensity,
                color_rgb[2] * intensity,
                -1.0,
            ],
        }
    }
}

/// Scene lighting & atmosphere uniform buffer (`@group(0) @binding(1)`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct SceneLightingUniforms {
    /// `rgb` = hemisphere sky (`+Y`) ambient, `w` = indoor ambient scale (`0.60`).
    pub sky_ambient: [f32; 4],
    /// `rgb` = hemisphere ground (`-Y`) bounce ambient, `w` = actor min-light blend (`1.0`).
    pub ground_ambient: [f32; 4],
    /// `xyz` = unit vector TOWARD directional sun/moon in renderer `+Y`-up space, `w` = intensity.
    pub sun_dir_and_intensity: [f32; 4],
    /// `rgb` = directional sun/moon color, `w` = half-Lambert wrap (`0.15` for readable retro shadows).
    pub sun_color: [f32; 4],
    /// `rgb` = hidden-room shroud color (`[34, 38, 48] / 255`), `w` = lower-floor dim (`0.62`).
    pub shroud_and_dim: [f32; 4],
    /// `rgb` = horizon/clear background color (`[18, 20, 30] / 255`), `w` = linear fog density (`0.0` = off).
    pub fog_color_and_density: [f32; 4],
    /// `[point_light_count, shading_flags, active_floor_u32, _pad]`.
    pub counts_and_flags: [u32; 4],
    pub point_lights: [PointLightGpu; MAX_POINT_LIGHTS],
    pub visibility_grid: [i32; 4],
    pub player_position: [f32; 4],
}

impl Default for SceneLightingUniforms {
    fn default() -> Self {
        Self::dusk_default()
    }
}

impl SceneLightingUniforms {
    #[must_use]
    pub fn dusk_default() -> Self {
        let sun_dir = Vec3::new(-0.45, 0.78, 0.43).normalize();
        Self {
            sky_ambient: [0.46, 0.52, 0.64, 0.60],
            ground_ambient: [0.22, 0.24, 0.30, 1.0],
            sun_dir_and_intensity: [sun_dir.x, sun_dir.y, sun_dir.z, 0.62],
            sun_color: [0.96, 0.88, 0.80, 0.15],
            shroud_and_dim: [34.0 / 255.0, 38.0 / 255.0, 48.0 / 255.0, 0.62],
            fog_color_and_density: [18.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 0.0],
            counts_and_flags: [0, shading_flags::ENABLE_BAYER_DITHER, 0, 0],
            point_lights: [PointLightGpu::default(); MAX_POINT_LIGHTS],
            visibility_grid: [0; 4],
            player_position: [0.0; 4],
        }
    }

    #[must_use]
    pub fn unlit_test() -> Self {
        Self {
            sky_ambient: [1.0, 1.0, 1.0, 1.0],
            ground_ambient: [1.0, 1.0, 1.0, 1.0],
            sun_dir_and_intensity: [0.0, 1.0, 0.0, 0.0],
            sun_color: [0.0, 0.0, 0.0, 0.0],
            shroud_and_dim: [0.0, 0.0, 0.0, 1.0],
            fog_color_and_density: [18.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 0.0],
            counts_and_flags: [0, 0, 0, 0],
            point_lights: [PointLightGpu::default(); MAX_POINT_LIGHTS],
            visibility_grid: [0; 4],
            player_position: [0.0; 4],
        }
    }

    #[must_use]
    pub fn for_scene(scene: &crate::scene::DynamicSceneState, camera: &crate::CameraFrame) -> Self {
        use crate::coords::sim_to_render_point;
        use zombiebox_core::LightKind;

        let mut result = Self::dusk_default();
        let daylight = 1.0 - scene.dusk_level.clamp(0.0, 1.0);
        result.sun_dir_and_intensity[3] += daylight * 0.18;
        let grid = &scene.local_visibility;
        result.visibility_grid = [
            grid.origin.x,
            grid.origin.y,
            grid.side as i32,
            i32::from(grid.floor_count),
        ];
        result.player_position = sim_to_render_point(scene.player.foot_pos)
            .extend(1.0)
            .to_array();
        result.counts_and_flags[1] |=
            shading_flags::ENABLE_WORLD_VISIBILITY | shading_flags::ENABLE_LOWER_FLOOR_DIM;
        if camera.mode == crate::CameraMode::FirstPerson {
            result.counts_and_flags[1] |= shading_flags::FIRST_PERSON;
        }
        result.counts_and_flags[2] = u32::from(scene.active_floor);
        let mut candidates = Vec::with_capacity(scene.lights.len() + scene.dynamic_flashes.len());
        for light in &scene.lights {
            if !light.view.active {
                continue;
            }
            let color = match light.view.kind {
                LightKind::Streetlight => [1.0, 0.79, 0.49],
                LightKind::Porch => [1.0, 0.69, 0.40],
                LightKind::Cache => [0.62, 0.91, 0.71],
            };
            let mut point = PointLightGpu::new(
                sim_to_render_point(light.position),
                light.view.radius,
                color,
                f32::from(light.view.intensity) / 72.0,
            );
            if light.view.kind != LightKind::Streetlight {
                point.color_and_floor[3] = f32::from(light.view.floor);
            }
            candidates.push(point);
        }
        for flash in &scene.dynamic_flashes {
            let mut point = PointLightGpu::new(
                sim_to_render_point(flash.position),
                flash.radius,
                [1.0, 0.66, 0.35],
                f32::from(flash.intensity) / 60.0,
            );
            point.color_and_floor[3] = f32::from(flash.floor);
            candidates.push(point);
        }
        let score = |light: &PointLightGpu| {
            let [x, y, z, radius] = light.pos_and_radius;
            let position = Vec3::new(x, y, z);
            let distance = camera.focus.distance(position);
            let luminance = light.color_and_floor[0].max(light.color_and_floor[1]);
            luminance * radius / (1.0 + distance)
        };
        candidates.retain(|light| {
            let [x, y, z, radius] = light.pos_and_radius;
            camera.focus.distance_squared(Vec3::new(x, y, z)) <= (18.0 + radius).powi(2)
        });
        candidates.sort_by(|a, b| score(b).total_cmp(&score(a)));
        result.set_point_lights(&candidates);
        result
    }

    pub fn set_point_lights(&mut self, lights: &[PointLightGpu]) {
        let count = lights.len().min(MAX_POINT_LIGHTS);
        self.point_lights[..count].copy_from_slice(&lights[..count]);
        self.counts_and_flags[0] = count as u32;
    }
}

/// 16-byte uniform for `upscale.wgsl` (`@group(0) @binding(2)`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct UpscaleUniforms {
    pub output_is_srgb: u32,
    pub _pad: [u32; 3],
}
