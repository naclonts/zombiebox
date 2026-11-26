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

/// 1,136-byte scene lighting & atmosphere uniform buffer (`@group(0) @binding(1)`).
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
        }
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
