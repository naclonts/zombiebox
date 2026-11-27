//! Camera modes, rigs, projection math, and analog stick input mapping (`T01`).

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_6, PI, SQRT_2};

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, UVec2, Vec2, Vec3, Vec4};
use zombiebox_core::{Vec2 as SimVec2, Weapon};

use crate::coords::{
    FLOOR_H, RenderAabb, SimVec3, render_to_sim_point, render_to_sim_vec, shortest_angle_delta,
    sim_heading_from_vec2, sim_to_render_point, sim_vec2_from_heading, sim_xyz_to_render,
    wrap_angle_pi,
};

/// Exact 2:1 isometric baseline scale: `20 * sqrt(2) ≈ 28.28427125` internal pixels per view unit.
pub const ISO_PIXELS_PER_UNIT: f32 = 20.0 * SQRT_2;
pub const DEFAULT_ISO_YAW: f32 = FRAC_PI_4;
pub const DEFAULT_ISO_PITCH: f32 = FRAC_PI_6;

/// Default retro internal resolution caps matching Android `ZombieboxSurfaceView` (`360p` short side, `1200` long side).
pub const DEFAULT_INTERNAL_SHORT_SIDE: u32 = 360;
pub const DEFAULT_INTERNAL_LONG_SIDE: u32 = 1200;

/// Proven damping half-lives calibrated to the isometric reference build.
pub const DEFAULT_FOLLOW_HALFLIFE_S: f32 = 0.055;
pub const DEFAULT_ELEVATION_HALFLIFE_S: f32 = 0.14;
pub const DEFAULT_LOOK_HALFLIFE_S: f32 = 0.16;
pub const DEFAULT_SHAKE_HALFLIFE_S: f32 = 0.065;
const MOUNTED_CHASE_YAW_HALFLIFE_S: f32 = 0.11;

/// Full-deflection look rates for conventional first/third-person camera control.
pub const DEFAULT_LOOK_YAW_SPEED: f32 = 3.2;
pub const DEFAULT_LOOK_PITCH_SPEED: f32 = 2.0;
/// Non-linear stick response: precise near center, fast at full deflection.
pub const DEFAULT_LOOK_RESPONSE_EXPONENT: f32 = 1.6;
pub const DEFAULT_LOOK_DEADZONE: f32 = 0.04;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum CameraMode {
    #[default]
    Isometric = 0,
    Overhead = 1,
    Orbit = 2,
    Chase = 3,
    FirstPerson = 4,
}

impl CameraMode {
    pub const COUNT: usize = 5;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Isometric,
        Self::Overhead,
        Self::Orbit,
        Self::Chase,
        Self::FirstPerson,
    ];

    #[must_use]
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Isometric),
            1 => Some(Self::Overhead),
            2 => Some(Self::Orbit),
            3 => Some(Self::Chase),
            4 => Some(Self::FirstPerson),
            _ => None,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Isometric => "Isometric",
            Self::Overhead => "Overhead",
            Self::Orbit => "Orbit",
            Self::Chase => "Chase",
            Self::FirstPerson => "FirstPerson",
        }
    }

    #[must_use]
    pub const fn is_orthographic(self) -> bool {
        matches!(self, Self::Isometric)
    }
}

/// Computes internal low-res render target dimensions `(iw, ih)` from presentation surface `(sw, sh)`,
/// matching Android `ZombieboxSurfaceView`: `scale = min(1.0, short_cap / min(w, h), long_cap / max(w, h))`.
#[must_use]
pub fn compute_internal_resolution(
    surface_width: u32,
    surface_height: u32,
    max_short_side: u32,
    max_long_side: u32,
) -> UVec2 {
    let sw = surface_width.max(1);
    let sh = surface_height.max(1);
    let short = sw.min(sh) as f32;
    let long = sw.max(sh) as f32;
    let short_cap = max_short_side.max(1) as f32;
    let long_cap = max_long_side.max(1) as f32;
    let scale = 1.0_f32.min(short_cap / short).min(long_cap / long);
    let iw = ((sw as f32) * scale).round().max(1.0) as u32;
    let ih = ((sh as f32) * scale).round().max(1.0) as u32;
    UVec2::new(iw, ih)
}

/// Tool-weighted camera aim look-ahead distance in world units (matching the isometric reference build).
#[must_use]
pub const fn weapon_look_ahead_distance(weapon: Option<Weapon>) -> f32 {
    match weapon {
        Some(Weapon::RocketLauncher) => 1.6,
        Some(Weapon::Pistol) => 1.4,
        Some(Weapon::Shotgun) => 1.15,
        Some(Weapon::Axe) => 0.7,
        None => 0.5,
    }
}

/// Frame-rate-independent exponential smoothing factor `1 - 0.5^(dt / halflife_s)`.
#[must_use]
pub fn damp_factor(dt: f32, halflife_s: f32) -> f32 {
    if !dt.is_finite() || dt <= 0.0 {
        return 0.0;
    }
    if !halflife_s.is_finite() || halflife_s <= 0.0 {
        return 1.0;
    }
    1.0 - 0.5_f32.powf(dt / halflife_s)
}

/// Applies a deadzone-compensated exponent response curve to one look-stick axis.
#[must_use]
fn shape_look_axis(value: f32, deadzone: f32, exponent: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    let deadzone = deadzone.clamp(0.0, 0.95);
    let magnitude = value.abs();
    if magnitude <= deadzone {
        return 0.0;
    }
    let normalized = ((magnitude - deadzone) / (1.0 - deadzone)).min(1.0);
    normalized.powf(exponent.clamp(0.1, 4.0)).copysign(value)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraConfig {
    // Damping & teleport snap thresholds
    pub follow_halflife_s: f32,
    pub elevation_halflife_s: f32,
    pub look_halflife_s: f32,
    pub shake_halflife_s: f32,
    pub teleport_snap_xy: f32,
    pub teleport_snap_z: f32,
    pub look_ahead_enabled: bool,

    // Isometric preset (orthographic)
    pub iso_yaw: f32,
    pub iso_pitch: f32,
    pub iso_pixels_per_unit: f32,
    pub iso_zoom: f32,
    pub iso_focus_height: f32,
    pub iso_eye_distance: f32,
    pub iso_near: f32,
    pub iso_far: f32,

    // Overhead preset (perspective)
    pub overhead_yaw: f32,
    pub overhead_pitch: f32,
    pub overhead_distance: f32,
    pub overhead_fov_y: f32,

    // Orbit preset (perspective)
    pub orbit_default_yaw: f32,
    pub orbit_default_pitch: f32,
    pub orbit_default_distance: f32,
    pub orbit_min_pitch: f32,
    pub orbit_max_pitch: f32,
    pub orbit_min_distance: f32,
    pub orbit_max_distance: f32,
    pub orbit_focus_height: f32,
    pub orbit_fov_y: f32,

    // Chase preset (perspective, aligned to FLOOR_H = 1.388)
    pub chase_pitch: f32,
    pub chase_distance: f32,
    pub chase_focus_height: f32,
    pub chase_forward_look: f32,
    pub chase_fov_y: f32,
    pub chase_min_pitch: f32,
    pub chase_max_pitch: f32,

    // Conventional first/third-person look input
    pub look_yaw_speed: f32,
    pub look_pitch_speed: f32,
    pub look_response_exponent: f32,
    pub look_deadzone: f32,

    // First-person preset (perspective, eye at 0.92 < FLOOR_H ceiling)
    pub first_person_eye_height: f32,
    pub first_person_forward_offset: f32,
    pub first_person_default_pitch: f32,
    pub first_person_max_pitch: f32,
    pub first_person_fov_y: f32,

    // Shared perspective clip planes
    pub perspective_near: f32,
    pub perspective_far: f32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            follow_halflife_s: DEFAULT_FOLLOW_HALFLIFE_S,
            elevation_halflife_s: DEFAULT_ELEVATION_HALFLIFE_S,
            look_halflife_s: DEFAULT_LOOK_HALFLIFE_S,
            shake_halflife_s: DEFAULT_SHAKE_HALFLIFE_S,
            teleport_snap_xy: 6.0,
            teleport_snap_z: 1.5 * FLOOR_H,
            look_ahead_enabled: true,

            iso_yaw: DEFAULT_ISO_YAW,
            iso_pitch: DEFAULT_ISO_PITCH,
            iso_pixels_per_unit: ISO_PIXELS_PER_UNIT,
            iso_zoom: 1.0,
            iso_focus_height: 0.0,
            iso_eye_distance: 120.0,
            iso_near: 1.0,
            iso_far: 320.0,

            overhead_yaw: FRAC_PI_2,
            overhead_pitch: 1.35,
            overhead_distance: 22.0,
            overhead_fov_y: 45.0_f32.to_radians(),

            orbit_default_yaw: FRAC_PI_4,
            orbit_default_pitch: 0.62,
            orbit_default_distance: 16.0,
            orbit_min_pitch: 0.12,
            orbit_max_pitch: 1.48,
            orbit_min_distance: 3.0,
            orbit_max_distance: 80.0,
            orbit_focus_height: 0.75,
            orbit_fov_y: 50.0_f32.to_radians(),

            chase_pitch: 0.42,
            chase_distance: 6.5,
            chase_focus_height: 0.85,
            chase_forward_look: 1.2,
            chase_fov_y: 55.0_f32.to_radians(),
            chase_min_pitch: 0.0,
            chase_max_pitch: 1.30,

            look_yaw_speed: DEFAULT_LOOK_YAW_SPEED,
            look_pitch_speed: DEFAULT_LOOK_PITCH_SPEED,
            look_response_exponent: DEFAULT_LOOK_RESPONSE_EXPONENT,
            look_deadzone: DEFAULT_LOOK_DEADZONE,

            first_person_eye_height: 0.92,
            first_person_forward_offset: 0.12,
            first_person_default_pitch: 0.05,
            first_person_max_pitch: 1.35,
            first_person_fov_y: 70.0_f32.to_radians(),

            perspective_near: 0.10,
            perspective_far: 350.0,
        }
    }
}

/// Simulation-space target fed into `CameraRig::update` each frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraTarget {
    /// Foot / ground-contact position in simulation `(x, y, z)` coordinates (`+z` up).
    pub position: SimVec3,
    /// Velocity in simulation `(x, y, z)` coordinates.
    pub velocity: SimVec3,
    /// Normalized horizontal aim/facing vector in simulation `(x, y)` (zero falls back to `heading`).
    pub aim: SimVec2,
    /// Horizontal body/vehicle heading in simulation radians (`0` = `+x`, `+π/2` = `+y`, `-π/2` = `-y`).
    pub heading: f32,
    /// Desired look-ahead distance along `aim` in world units (e.g. from `weapon_look_ahead_distance`).
    pub look_ahead_distance: f32,
}

impl Default for CameraTarget {
    fn default() -> Self {
        Self {
            position: SimVec3::ZERO,
            velocity: SimVec3::ZERO,
            aim: SimVec2::new(0.0, -1.0),
            heading: -FRAC_PI_2,
            look_ahead_distance: 0.0,
        }
    }
}

impl CameraTarget {
    /// Convenience constructor for ground/floor-positioned actors with an optional equipped weapon.
    #[must_use]
    pub fn from_sim_pose(position: SimVec3, facing: SimVec2, equipped: Option<Weapon>) -> Self {
        let (aim, heading) =
            if facing.x.is_finite() && facing.y.is_finite() && facing.length_squared() > 1e-8 {
                let unit = facing.normalized();
                (unit, sim_heading_from_vec2(unit))
            } else {
                (SimVec2::new(0.0, -1.0), -FRAC_PI_2)
            };
        Self {
            position,
            velocity: SimVec3::ZERO,
            aim,
            heading,
            look_ahead_distance: weapon_look_ahead_distance(equipped),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrustumPlane {
    pub normal: Vec3,
    pub d: f32,
}

impl FrustumPlane {
    #[must_use]
    pub fn signed_distance(self, point: Vec3) -> f32 {
        self.normal.dot(point) + self.d
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frustum {
    /// `[left, right, bottom, top, near, far]`, all inward-pointing.
    pub planes: [FrustumPlane; 6],
}

fn normalize_plane(row: Vec4) -> FrustumPlane {
    let normal = row.truncate();
    let len = normal.length();
    if len > 1e-12 {
        FrustumPlane {
            normal: normal / len,
            d: row.w / len,
        }
    } else {
        FrustumPlane {
            normal: Vec3::ZERO,
            d: row.w,
        }
    }
}

impl Frustum {
    /// Extracts WebGPU `[0, 1]` clip-space planes from `view_proj`.
    #[must_use]
    pub fn from_view_proj(view_proj: Mat4) -> Self {
        let r0 = view_proj.row(0);
        let r1 = view_proj.row(1);
        let r2 = view_proj.row(2);
        let r3 = view_proj.row(3);
        Self {
            planes: [
                normalize_plane(r3 + r0),
                normalize_plane(r3 - r0),
                normalize_plane(r3 + r1),
                normalize_plane(r3 - r1),
                normalize_plane(r2),
                normalize_plane(r3 - r2),
            ],
        }
    }

    #[must_use]
    pub fn contains_point(&self, point: Vec3) -> bool {
        self.planes.iter().all(|p| p.signed_distance(point) >= 0.0)
    }

    #[must_use]
    pub fn intersects_sphere(&self, center: Vec3, radius: f32) -> bool {
        self.planes
            .iter()
            .all(|p| p.signed_distance(center) >= -radius)
    }

    #[must_use]
    pub fn intersects_aabb(&self, aabb: RenderAabb) -> bool {
        self.planes.iter().all(|p| {
            let p_vertex = Vec3::new(
                if p.normal.x >= 0.0 {
                    aabb.max.x
                } else {
                    aabb.min.x
                },
                if p.normal.y >= 0.0 {
                    aabb.max.y
                } else {
                    aabb.min.y
                },
                if p.normal.z >= 0.0 {
                    aabb.max.z
                } else {
                    aabb.min.z
                },
            );
            p.signed_distance(p_vertex) >= 0.0
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenPoint {
    pub px: Vec2,
    pub depth: f32,
}

impl ScreenPoint {
    #[must_use]
    pub fn in_depth_range(self) -> bool {
        (0.0..=1.0).contains(&self.depth)
    }

    #[must_use]
    pub fn in_viewport(self, viewport_size: UVec2) -> bool {
        self.px.x >= 0.0
            && self.px.x <= viewport_size.x as f32
            && self.px.y >= 0.0
            && self.px.y <= viewport_size.y as f32
    }
}

/// Ray in right-handed renderer `(X, Y, Z)` space (`dir` is unit length).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderRay {
    pub origin: Vec3,
    pub dir: Vec3,
}

impl RenderRay {
    #[must_use]
    pub fn at(self, t: f32) -> Vec3 {
        self.origin + self.dir * t
    }

    /// Intersects horizontal plane `Y = plane_y` (`t >= 0.0`), returning the 3D hit point.
    #[must_use]
    pub fn intersect_plane_y(self, plane_y: f32) -> Option<Vec3> {
        if self.dir.y.abs() <= 1e-6 {
            return None;
        }
        let t = (plane_y - self.origin.y) / self.dir.y;
        (t >= 0.0).then(|| self.at(t))
    }
}

/// Ray in simulation `(x, y, z)` space (`+z` up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimRay {
    pub origin: SimVec3,
    pub dir: SimVec3,
}

impl SimRay {
    /// Intersects horizontal simulation plane `z = plane_z` (`t >= 0.0`), returning the 3D hit point.
    #[must_use]
    pub fn intersect_plane_z(self, plane_z: f32) -> Option<SimVec3> {
        if self.dir.z.abs() <= 1e-6 {
            return None;
        }
        let t = (plane_z - self.origin.z) / self.dir.z;
        (t >= 0.0).then(|| self.origin + self.dir * t)
    }
}

/// 16-byte-aligned WGSL uniform payload (224 bytes, zero `vec3` padding traps for `T03+`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct CameraGpuUniforms {
    pub view_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub eye_pos: [f32; 4],
    pub forward_dir: [f32; 4],
    pub right_dir: [f32; 4],
    pub up_dir: [f32; 4],
    pub viewport_and_internal: [f32; 4],
    pub clip_and_scale: [f32; 4],
    pub render_origin: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraFrame {
    pub mode: CameraMode,
    pub is_orthographic: bool,
    pub viewport_size: UVec2,
    pub internal_size: UVec2,
    pub yaw: f32,
    pub pitch: f32,
    pub near: f32,
    pub far: f32,
    /// Internal-target pixels per view-space unit at `focus` (`28.284271` in default Isometric).
    pub pixel_scale: f32,
    /// Desired eye-to-focus distance prior to obstruction clamping (`0.0` in `FirstPerson`).
    pub unobstructed_distance: f32,
    pub focus: Vec3,
    pub eye: Vec3,
    pub forward: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub view: Mat4,
    pub proj: Mat4,
    pub view_proj: Mat4,
    pub inv_view_proj: Mat4,
    pub frustum: Frustum,
}

impl CameraFrame {
    fn project_to_size(&self, render_pos: Vec3, target_size: UVec2) -> Option<ScreenPoint> {
        let clip = self.view_proj * render_pos.extend(1.0);
        if !self.is_orthographic && clip.w <= 1e-6 {
            return None;
        }
        if clip.w.abs() <= 1e-6 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        let sx = (ndc.x * 0.5 + 0.5) * (target_size.x as f32);
        let sy = (0.5 - ndc.y * 0.5) * (target_size.y as f32);
        Some(ScreenPoint {
            px: Vec2::new(sx, sy),
            depth: ndc.z,
        })
    }

    /// Projects a renderer-space point `(X, Y, Z)` onto `viewport_size` (`+x` right, `+y` down).
    /// Returns `None` if at or behind the camera in perspective mode (`clip.w <= 1e-6`).
    #[must_use]
    pub fn project_to_screen(&self, render_pos: Vec3) -> Option<ScreenPoint> {
        self.project_to_size(render_pos, self.viewport_size)
    }

    /// Projects a renderer-space point onto `internal_size` (`[0..iw, 0..ih]`).
    #[must_use]
    pub fn project_to_internal(&self, render_pos: Vec3) -> Option<ScreenPoint> {
        self.project_to_size(render_pos, self.internal_size)
    }

    /// Projects a simulation-space point `(x, y, z)` onto `viewport_size`.
    #[must_use]
    pub fn project_sim_to_screen(&self, sim_pos: SimVec3) -> Option<ScreenPoint> {
        self.project_to_screen(sim_to_render_point(sim_pos))
    }

    /// Unprojects a `viewport_size` pixel coordinate `(sx, sy)` into a world-space `RenderRay`
    /// starting on the near clip plane (`depth = 0.0`) and pointing toward `depth = 1.0`.
    #[must_use]
    pub fn screen_to_ray(&self, screen_px: Vec2) -> RenderRay {
        let vw = self.viewport_size.x.max(1) as f32;
        let vh = self.viewport_size.y.max(1) as f32;
        let ndc_x = (screen_px.x / vw) * 2.0 - 1.0;
        let ndc_y = 1.0 - (screen_px.y / vh) * 2.0;

        let near_w = self.inv_view_proj * Vec4::new(ndc_x, ndc_y, 0.0, 1.0);
        let far_w = self.inv_view_proj * Vec4::new(ndc_x, ndc_y, 1.0, 1.0);

        let origin = if near_w.w.abs() > 1e-6 {
            near_w.truncate() / near_w.w
        } else {
            self.eye
        };
        let far_pos = if far_w.w.abs() > 1e-6 {
            far_w.truncate() / far_w.w
        } else {
            origin + self.forward
        };
        let dir = (far_pos - origin).normalize_or_zero();
        RenderRay {
            origin,
            dir: if dir.length_squared() > 1e-12 {
                dir
            } else {
                self.forward
            },
        }
    }

    /// Unprojects a `viewport_size` pixel coordinate into a simulation-space `SimRay`.
    #[must_use]
    pub fn screen_to_sim_ray(&self, screen_px: Vec2) -> SimRay {
        let ray = self.screen_to_ray(screen_px);
        SimRay {
            origin: render_to_sim_point(ray.origin),
            dir: render_to_sim_vec(ray.dir),
        }
    }

    /// Unprojects `screen_px` onto the horizontal simulation plane `z = sim_z`, returning `(x, y)`.
    #[must_use]
    pub fn screen_to_sim_plane_z(&self, screen_px: Vec2, sim_z: f32) -> Option<SimVec2> {
        self.screen_to_sim_ray(screen_px)
            .intersect_plane_z(sim_z)
            .map(|hit| hit.xy())
    }

    /// Converts a screen-space virtual stick `(sx, sy)` (`+x` right, `+y` down, `-y` up)
    /// into a horizontal simulation-space vector `(vx, vy)` preserving analog magnitude `<= 1.0`.
    #[must_use]
    pub fn map_stick_to_world(&self, stick_screen: SimVec2) -> SimVec2 {
        let sx = stick_screen.x;
        let sy = stick_screen.y;
        if !sx.is_finite() || !sy.is_finite() {
            return SimVec2::ZERO;
        }
        let mag = sx.hypot(sy);
        if mag <= 1e-6 {
            return SimVec2::ZERO;
        }
        let strength = mag.min(1.0);

        let (sin_y, cos_y) = self.yaw.sin_cos();
        let right_sim = SimVec2::new(sin_y, -cos_y);
        let forward_sim = SimVec2::new(-cos_y, -sin_y);

        let raw = match self.mode {
            CameraMode::Isometric => {
                let sin_p = self.pitch.sin().abs().max(0.25);
                right_sim * sx + forward_sim * (-sy / sin_p)
            }
            CameraMode::Overhead
            | CameraMode::Orbit
            | CameraMode::Chase
            | CameraMode::FirstPerson => right_sim * sx + forward_sim * (-sy),
        };

        let raw_len = raw.length();
        if raw_len <= 1e-6 {
            SimVec2::ZERO
        } else {
            raw * (strength / raw_len)
        }
    }

    /// Horizontal origin snapped to the focus chunk; simulation coordinates remain unchanged.
    #[must_use]
    pub fn render_origin(&self) -> Vec3 {
        let size = zombiebox_core::CHUNK_SIZE as f32;
        Vec3::new(
            (self.focus.x / size).floor() * size,
            0.0,
            (self.focus.z / size).floor() * size,
        )
    }

    /// Packs camera matrices and vectors into a 16-byte-aligned `Pod` struct for `wgpu` uniform buffers.
    #[must_use]
    pub fn to_gpu_uniforms(&self) -> CameraGpuUniforms {
        let origin = self.render_origin();
        CameraGpuUniforms {
            view_proj: (self.view_proj * Mat4::from_translation(origin)).to_cols_array_2d(),
            inv_view_proj: (Mat4::from_translation(-origin) * self.inv_view_proj)
                .to_cols_array_2d(),
            eye_pos: [
                self.eye.x - origin.x,
                self.eye.y,
                self.eye.z - origin.z,
                1.0,
            ],
            forward_dir: [self.forward.x, self.forward.y, self.forward.z, 0.0],
            right_dir: [self.right.x, self.right.y, self.right.z, 0.0],
            up_dir: [self.up.x, self.up.y, self.up.z, 0.0],
            viewport_and_internal: [
                self.viewport_size.x as f32,
                self.viewport_size.y as f32,
                self.internal_size.x as f32,
                self.internal_size.y as f32,
            ],
            render_origin: origin.extend(0.0).to_array(),
            clip_and_scale: [
                self.near,
                self.far,
                self.pixel_scale,
                if self.is_orthographic { 1.0 } else { 0.0 },
            ],
        }
    }
}

/// Analytical singularity-free orthonormal camera basis `(right, up, forward)` in right-handed
/// `(X, Y, Z)` renderer space (`+Y` up).
fn basis_from_yaw_pitch(yaw: f32, pitch: f32) -> (Vec3, Vec3, Vec3) {
    let (sin_y, cos_y) = yaw.sin_cos();
    let (sin_p, cos_p) = pitch.sin_cos();
    let right = Vec3::new(sin_y, 0.0, -cos_y);
    let up = Vec3::new(-sin_p * cos_y, cos_p, -sin_p * sin_y);
    let forward = Vec3::new(-cos_p * cos_y, -sin_p, -cos_p * sin_y);
    (right, up, forward)
}

#[derive(Clone, Debug)]
pub struct CameraRig {
    mode: CameraMode,
    config: CameraConfig,
    viewport_size: UVec2,
    internal_size: UVec2,

    initialized: bool,
    anchor_sim: SimVec3,
    look_offset_sim: SimVec2,

    /// Independent view orientation for Chase/FirstPerson (renderer yaw: `heading + π`).
    view_yaw: f32,
    view_pitch: f32,
    view_yaw_initialized: bool,
    last_target_heading: f32,
    mounted_view_mode: Option<CameraMode>,

    orbit_yaw: f32,
    orbit_pitch: f32,
    orbit_distance: f32,

    obstruction_distance: Option<f32>,
    shake_amplitude: f32,
    shake_clock_s: f32,
}

impl CameraRig {
    #[must_use]
    pub fn new(viewport_width: u32, viewport_height: u32) -> Self {
        Self::with_config(viewport_width, viewport_height, CameraConfig::default())
    }

    #[must_use]
    pub fn with_config(viewport_width: u32, viewport_height: u32, config: CameraConfig) -> Self {
        let size = UVec2::new(viewport_width.max(1), viewport_height.max(1));
        Self {
            mode: CameraMode::default(),
            config,
            viewport_size: size,
            internal_size: size,
            initialized: false,
            anchor_sim: SimVec3::ZERO,
            look_offset_sim: SimVec2::ZERO,
            view_yaw: 0.0,
            view_pitch: config.first_person_default_pitch,
            view_yaw_initialized: false,
            last_target_heading: -FRAC_PI_2,
            mounted_view_mode: None,
            orbit_yaw: config.orbit_default_yaw,
            orbit_pitch: config.orbit_default_pitch,
            orbit_distance: config.orbit_default_distance,
            obstruction_distance: None,
            shake_amplitude: 0.0,
            shake_clock_s: 0.0,
        }
    }

    #[must_use]
    pub const fn mode(&self) -> CameraMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: CameraMode) {
        let changed = mode != self.mode;
        self.mode = mode;
        if changed {
            match mode {
                CameraMode::Chase => {
                    self.view_pitch = self.config.chase_pitch;
                    self.view_yaw_initialized = false;
                }
                CameraMode::FirstPerson => {
                    self.view_pitch = self.config.first_person_default_pitch;
                    self.view_yaw_initialized = false;
                }
                _ => {}
            }
        }
        self.obstruction_distance = None;
    }

    #[must_use]
    pub const fn config(&self) -> &CameraConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut CameraConfig {
        &mut self.config
    }

    pub fn set_look_ahead(&mut self, enabled: bool) {
        self.config.look_ahead_enabled = enabled;
    }

    /// Sets `viewport_size` AND `internal_size` to the same dimensions (1:1 offscreen / baseline / test mode).
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        let size = UVec2::new(width.max(1), height.max(1));
        self.viewport_size = size;
        self.internal_size = size;
    }

    /// Sets presentation `viewport_size` and computes capped retro `internal_size` (`360p` short / `1200` long).
    pub fn set_display_surface(&mut self, surface_width: u32, surface_height: u32) {
        let viewport = UVec2::new(surface_width.max(1), surface_height.max(1));
        self.viewport_size = viewport;
        self.internal_size = compute_internal_resolution(
            viewport.x,
            viewport.y,
            DEFAULT_INTERNAL_SHORT_SIDE,
            DEFAULT_INTERNAL_LONG_SIDE,
        );
    }

    /// Explicitly sets independent presentation `viewport_size` and low-res `internal_size`.
    pub fn set_viewport_and_internal_size(&mut self, viewport: UVec2, internal: UVec2) {
        self.viewport_size = UVec2::new(viewport.x.max(1), viewport.y.max(1));
        self.internal_size = UVec2::new(internal.x.max(1), internal.y.max(1));
    }

    #[must_use]
    pub const fn viewport_size(&self) -> UVec2 {
        self.viewport_size
    }

    #[must_use]
    pub const fn internal_size(&self) -> UVec2 {
        self.internal_size
    }

    /// Resets damping & shake so the next `update` snaps immediately (call after teleport or new run).
    pub fn notify_teleport(&mut self) {
        self.initialized = false;
        self.view_yaw_initialized = false;
        self.shake_amplitude = 0.0;
        self.obstruction_distance = None;
    }

    /// Snaps the Chase/FirstPerson view yaw to the actor heading (`renderer yaw = heading + π`).
    /// Call when entering a view mode; subsequent look input is integrated independently.
    pub fn initialize_view_from_heading(&mut self, heading: f32) {
        if heading.is_finite() {
            self.view_yaw = wrap_angle_pi(heading + PI);
            self.view_yaw_initialized = true;
        }
    }

    /// Integrates conventional look-stick input into the independent view orientation.
    /// Stick `+x` turns right and stick `+y` (screen-down) looks down.
    pub fn apply_look_input(&mut self, stick: SimVec2, dt: f32) {
        if !matches!(self.mode, CameraMode::Chase | CameraMode::FirstPerson) {
            return;
        }
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        if dt <= 0.0 {
            return;
        }

        let x = shape_look_axis(
            stick.x,
            self.config.look_deadzone,
            self.config.look_response_exponent,
        );
        let y = shape_look_axis(
            stick.y,
            self.config.look_deadzone,
            self.config.look_response_exponent,
        );

        self.view_yaw = wrap_angle_pi(self.view_yaw + x * self.config.look_yaw_speed * dt);
        let (min_pitch, max_pitch) = self.pitch_limits();
        self.view_pitch =
            (self.view_pitch + y * self.config.look_pitch_speed * dt).clamp(min_pitch, max_pitch);
    }

    fn pitch_limits(&self) -> (f32, f32) {
        match self.mode {
            CameraMode::Chase => (self.config.chase_min_pitch, self.config.chase_max_pitch),
            CameraMode::FirstPerson => (
                -self.config.first_person_max_pitch,
                self.config.first_person_max_pitch,
            ),
            _ => (
                -self.config.first_person_max_pitch,
                self.config.first_person_max_pitch,
            ),
        }
    }

    /// Orbit camera controls (`delta_yaw` wraps `(-π, π]`; `orbit_pitch` & `orbit_distance` clamp to config).
    pub fn orbit_rotate(&mut self, delta_yaw: f32, delta_pitch: f32) {
        if delta_yaw.is_finite() {
            self.orbit_yaw = wrap_angle_pi(self.orbit_yaw + delta_yaw);
        }
        if delta_pitch.is_finite() {
            self.orbit_pitch = (self.orbit_pitch + delta_pitch)
                .clamp(self.config.orbit_min_pitch, self.config.orbit_max_pitch);
        }
    }

    pub fn orbit_zoom_by(&mut self, distance_delta: f32) {
        if distance_delta.is_finite() {
            self.orbit_distance = (self.orbit_distance + distance_delta).clamp(
                self.config.orbit_min_distance,
                self.config.orbit_max_distance,
            );
        }
    }

    pub fn orbit_scale_distance(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.orbit_distance = (self.orbit_distance * factor).clamp(
                self.config.orbit_min_distance,
                self.config.orbit_max_distance,
            );
        }
    }

    pub fn reset_orbit(&mut self) {
        self.orbit_yaw = self.config.orbit_default_yaw;
        self.orbit_pitch = self.config.orbit_default_pitch;
        self.orbit_distance = self.config.orbit_default_distance;
    }

    /// Manual free-look deltas (used by desktop viewer mouse-look in `T09`).
    pub fn adjust_first_person_look(&mut self, delta_yaw: f32, delta_pitch: f32) {
        if delta_yaw.is_finite() {
            self.view_yaw = wrap_angle_pi(self.view_yaw + delta_yaw);
            self.view_yaw_initialized = true;
        }
        if delta_pitch.is_finite() {
            let (min_pitch, max_pitch) = self.pitch_limits();
            self.view_pitch = (self.view_pitch + delta_pitch).clamp(min_pitch, max_pitch);
        }
    }

    pub fn reset_first_person_look(&mut self) {
        self.view_yaw = wrap_angle_pi(self.last_target_heading + PI);
        self.view_yaw_initialized = true;
        self.view_pitch = self.config.first_person_default_pitch;
    }

    /// Optional camera collision distance override for `Orbit`/`Chase` (`T09` obstruction solver).
    pub fn set_obstruction_distance(&mut self, max_distance: Option<f32>) {
        self.obstruction_distance = max_distance.filter(|d| d.is_finite() && *d >= 0.0);
    }

    /// Triggers screen shake in internal-pixel units (e.g. `2.5` shotgun, `6.5` rocket blast).
    pub fn add_shake(&mut self, impulse_px: f32) {
        if impulse_px.is_finite() && impulse_px > 0.0 {
            self.shake_amplitude += impulse_px;
        }
    }

    /// Advances damped camera state by `dt` seconds (clamped internally to `0.0..=0.1`) and returns `CameraFrame`.
    #[must_use]
    pub fn update(&mut self, target: CameraTarget, dt: f32) -> CameraFrame {
        self.update_inner(target, dt, false)
    }

    /// Advances the camera while mounted. Chase follows the vehicle heading; FirstPerson tracks
    /// it directly. Call this for every mounted frame, including while another camera mode is selected.
    #[must_use]
    pub fn update_mounted(&mut self, target: CameraTarget, dt: f32) -> CameraFrame {
        self.update_inner(target, dt, true)
    }

    fn update_inner(&mut self, target: CameraTarget, dt: f32, mounted: bool) -> CameraFrame {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        let mounted_view_mode = mounted.then_some(self.mode);
        let entered_mounted_view = mounted && self.mounted_view_mode != mounted_view_mode;
        self.mounted_view_mode = mounted_view_mode;
        if target.heading.is_finite() {
            self.last_target_heading = target.heading;
        }

        let aim_dir = if target.aim.x.is_finite()
            && target.aim.y.is_finite()
            && target.aim.length_squared() > 1e-6
        {
            target.aim.normalized()
        } else {
            sim_vec2_from_heading(target.heading)
        };

        let dx = target.position.x - self.anchor_sim.x;
        let dy = target.position.y - self.anchor_sim.y;
        let xy_dist = dx.hypot(dy);
        let dz = (target.position.z - self.anchor_sim.z).abs();

        let should_snap = !self.initialized
            || !xy_dist.is_finite()
            || !dz.is_finite()
            || xy_dist > self.config.teleport_snap_xy
            || dz > self.config.teleport_snap_z;

        let mounted_chase = mounted_view_mode == Some(CameraMode::Chase);
        let mounted_first_person = mounted_view_mode == Some(CameraMode::FirstPerson);
        if matches!(self.mode, CameraMode::Chase | CameraMode::FirstPerson) {
            if !self.view_yaw_initialized
                || should_snap
                || entered_mounted_view
                || mounted_first_person
            {
                self.initialize_view_from_heading(target.heading);
            } else if mounted_chase && target.heading.is_finite() {
                let delta = shortest_angle_delta(self.view_yaw, target.heading + PI);
                self.view_yaw = wrap_angle_pi(
                    self.view_yaw + delta * damp_factor(dt, MOUNTED_CHASE_YAW_HALFLIFE_S),
                );
            }
            if mounted_chase {
                self.view_pitch = self.config.chase_pitch;
            } else if mounted_first_person {
                self.view_pitch = self.config.first_person_default_pitch;
            }
        }

        let desired_look =
            if !self.config.look_ahead_enabled || self.mode == CameraMode::FirstPerson {
                SimVec2::ZERO
            } else if self.mode == CameraMode::Chase {
                SimVec2::new(-self.view_yaw.cos(), -self.view_yaw.sin())
                    * self.config.chase_forward_look
            } else {
                aim_dir * target.look_ahead_distance
            };

        if should_snap || mounted_first_person {
            self.anchor_sim = target.position;
            self.look_offset_sim = desired_look;
            self.initialized = true;
        } else if dt > 0.0 {
            let k_xy = damp_factor(dt, self.config.follow_halflife_s);
            let k_z = damp_factor(dt, self.config.elevation_halflife_s);
            let k_look = damp_factor(dt, self.config.look_halflife_s);

            self.anchor_sim.x += (target.position.x - self.anchor_sim.x) * k_xy;
            self.anchor_sim.y += (target.position.y - self.anchor_sim.y) * k_xy;
            if dz < 0.001 {
                self.anchor_sim.z = target.position.z;
            } else {
                self.anchor_sim.z += (target.position.z - self.anchor_sim.z) * k_z;
            }
            self.look_offset_sim += (desired_look - self.look_offset_sim) * k_look;
        }
        if mounted_chase && entered_mounted_view {
            self.look_offset_sim = desired_look;
        }

        if dt > 0.0 && self.shake_amplitude > 0.0 {
            self.shake_clock_s += dt;
            let decay = if self.config.shake_halflife_s > 0.0 {
                0.5_f32.powf(dt / self.config.shake_halflife_s)
            } else {
                0.0
            };
            self.shake_amplitude *= decay;
            if self.shake_amplitude < 0.12 {
                self.shake_amplitude = 0.0;
            }
        }

        self.current_frame()
    }

    /// Recomputes `CameraFrame` at the current rig state without advancing `dt` (`dt = 0` snapshot).
    #[must_use]
    pub fn current_frame(&self) -> CameraFrame {
        let iw = self.internal_size.x.max(1) as f32;
        let ih = self.internal_size.y.max(1) as f32;
        let aspect = (iw / ih).max(1e-3);
        let look = if self.config.look_ahead_enabled && self.mode != CameraMode::FirstPerson {
            self.look_offset_sim
        } else {
            SimVec2::ZERO
        };

        let (
            yaw,
            pitch,
            right,
            up,
            forward,
            mut focus,
            mut eye,
            near,
            far,
            is_orthographic,
            pixel_scale,
            unobstructed_distance,
            proj,
        ) = match self.mode {
            CameraMode::Isometric => {
                let yaw = self.config.iso_yaw;
                let pitch = self.config.iso_pitch;
                let (right, up, forward) = basis_from_yaw_pitch(yaw, pitch);
                let focus = sim_to_render_point(
                    self.anchor_sim + SimVec3::new(look.x, look.y, self.config.iso_focus_height),
                );
                let dist = self.config.iso_eye_distance;
                let eye = focus - forward * dist;
                let pixel_scale =
                    (self.config.iso_pixels_per_unit * self.config.iso_zoom).max(1e-3);
                let half_w = iw / (2.0 * pixel_scale);
                let half_h = ih / (2.0 * pixel_scale);
                let proj = Mat4::orthographic_rh(
                    -half_w,
                    half_w,
                    -half_h,
                    half_h,
                    self.config.iso_near,
                    self.config.iso_far,
                );
                (
                    yaw,
                    pitch,
                    right,
                    up,
                    forward,
                    focus,
                    eye,
                    self.config.iso_near,
                    self.config.iso_far,
                    true,
                    pixel_scale,
                    dist,
                    proj,
                )
            }
            CameraMode::Overhead => {
                let yaw = self.config.overhead_yaw;
                let pitch = self.config.overhead_pitch;
                let (right, up, forward) = basis_from_yaw_pitch(yaw, pitch);
                let focus =
                    sim_to_render_point(self.anchor_sim + SimVec3::new(look.x, look.y, 0.0));
                let dist = self.config.overhead_distance;
                let eye = focus - forward * dist;
                let fov_y = self.config.overhead_fov_y;
                let proj = Mat4::perspective_rh(
                    fov_y,
                    aspect,
                    self.config.perspective_near,
                    self.config.perspective_far,
                );
                let ref_dist = focus.distance(eye).max(1.0);
                let pixel_scale = ih / (2.0 * ref_dist * (0.5 * fov_y).tan());
                (
                    yaw,
                    pitch,
                    right,
                    up,
                    forward,
                    focus,
                    eye,
                    self.config.perspective_near,
                    self.config.perspective_far,
                    false,
                    pixel_scale,
                    dist,
                    proj,
                )
            }
            CameraMode::Orbit => {
                let yaw = self.orbit_yaw;
                let pitch = self.orbit_pitch;
                let (right, up, forward) = basis_from_yaw_pitch(yaw, pitch);
                let focus = sim_to_render_point(
                    self.anchor_sim + SimVec3::new(look.x, look.y, self.config.orbit_focus_height),
                );
                let dist = self.orbit_distance;
                let eff_dist = self.obstruction_distance.map_or(dist, |obs| {
                    dist.min(obs.max(self.config.perspective_near * 2.0))
                });
                let eye = focus - forward * eff_dist;
                let fov_y = self.config.orbit_fov_y;
                let proj = Mat4::perspective_rh(
                    fov_y,
                    aspect,
                    self.config.perspective_near,
                    self.config.perspective_far,
                );
                let ref_dist = focus.distance(eye).max(1.0);
                let pixel_scale = ih / (2.0 * ref_dist * (0.5 * fov_y).tan());
                (
                    yaw,
                    pitch,
                    right,
                    up,
                    forward,
                    focus,
                    eye,
                    self.config.perspective_near,
                    self.config.perspective_far,
                    false,
                    pixel_scale,
                    dist,
                    proj,
                )
            }
            CameraMode::Chase => {
                let yaw = self.view_yaw;
                let pitch = self
                    .view_pitch
                    .clamp(self.config.chase_min_pitch, self.config.chase_max_pitch);
                let (right, up, forward) = basis_from_yaw_pitch(yaw, pitch);
                let focus = sim_to_render_point(
                    self.anchor_sim + SimVec3::new(look.x, look.y, self.config.chase_focus_height),
                );
                let dist = self.config.chase_distance;
                let eff_dist = self.obstruction_distance.map_or(dist, |obs| {
                    dist.min(obs.max(self.config.perspective_near * 2.0))
                });
                let eye = focus - forward * eff_dist;
                let fov_y = self.config.chase_fov_y;
                let proj = Mat4::perspective_rh(
                    fov_y,
                    aspect,
                    self.config.perspective_near,
                    self.config.perspective_far,
                );
                let ref_dist = focus.distance(eye).max(1.0);
                let pixel_scale = ih / (2.0 * ref_dist * (0.5 * fov_y).tan());
                (
                    yaw,
                    pitch,
                    right,
                    up,
                    forward,
                    focus,
                    eye,
                    self.config.perspective_near,
                    self.config.perspective_far,
                    false,
                    pixel_scale,
                    dist,
                    proj,
                )
            }
            CameraMode::FirstPerson => {
                let yaw = self.view_yaw;
                let pitch = self.view_pitch.clamp(
                    -self.config.first_person_max_pitch,
                    self.config.first_person_max_pitch,
                );
                let (right, up, forward) = basis_from_yaw_pitch(yaw, pitch);
                let horiz_forward = Vec3::new(-yaw.cos(), 0.0, -yaw.sin());
                let eye_base = sim_xyz_to_render(
                    self.anchor_sim.x,
                    self.anchor_sim.y,
                    self.anchor_sim.z + self.config.first_person_eye_height,
                );
                let eye = eye_base + horiz_forward * self.config.first_person_forward_offset;
                let focus = eye + forward;
                let fov_y = self.config.first_person_fov_y;
                let proj = Mat4::perspective_rh(
                    fov_y,
                    aspect,
                    self.config.perspective_near,
                    self.config.perspective_far,
                );
                let ref_dist = focus.distance(eye).max(1.0);
                let pixel_scale = ih / (2.0 * ref_dist * (0.5 * fov_y).tan());
                (
                    yaw,
                    pitch,
                    right,
                    up,
                    forward,
                    focus,
                    eye,
                    self.config.perspective_near,
                    self.config.perspective_far,
                    false,
                    pixel_scale,
                    0.0,
                    proj,
                )
            }
        };

        if self.shake_amplitude > 0.0 {
            let t = self.shake_clock_s * 85.0;
            let sx = (t * 1.7).sin() * self.shake_amplitude;
            let sy = (t * 2.3 + 1.1).cos() * self.shake_amplitude * 0.78;
            let offset = (-right * sx + up * sy) / pixel_scale.max(1e-3);
            eye += offset;
            focus += offset;
        }

        let view = Mat4::look_to_rh(eye, forward, up);
        let view_proj = proj * view;
        let inv_view_proj = view_proj.inverse();
        let frustum = Frustum::from_view_proj(view_proj);

        CameraFrame {
            mode: self.mode,
            is_orthographic,
            viewport_size: self.viewport_size,
            internal_size: self.internal_size,
            yaw,
            pitch,
            near,
            far,
            pixel_scale,
            unobstructed_distance,
            focus,
            eye,
            forward,
            right,
            up,
            view,
            proj,
            view_proj,
            inv_view_proj,
            frustum,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::coords::ChunkCoord;

    #[test]
    fn isometric_exact_pixel_stepping_calibration() {
        for (vw, vh) in [(640, 360), (800, 360)] {
            let mut rig = CameraRig::new(vw, vh);
            rig.set_look_ahead(false);

            let focus_sim = SimVec3::new(10.0, 20.0, 0.0);
            let frame = rig.update(
                CameraTarget {
                    position: focus_sim,
                    ..CameraTarget::default()
                },
                0.0,
            );

            assert!(frame.is_orthographic);
            assert!((frame.pixel_scale - ISO_PIXELS_PER_UNIT).abs() < 1e-5);

            let center = frame.project_sim_to_screen(focus_sim).unwrap();
            assert!((center.px.x - vw as f32 * 0.5).abs() < 1e-3);
            assert!((center.px.y - vh as f32 * 0.5).abs() < 1e-3);

            let step_x = frame
                .project_sim_to_screen(SimVec3::new(11.0, 20.0, 0.0))
                .unwrap();
            assert!(((step_x.px.x - center.px.x) - 20.0).abs() < 1e-3);
            assert!(((step_x.px.y - center.px.y) - 10.0).abs() < 1e-3);

            let step_y = frame
                .project_sim_to_screen(SimVec3::new(10.0, 21.0, 0.0))
                .unwrap();
            assert!(((step_y.px.x - center.px.x) - (-20.0)).abs() < 1e-3);
            assert!(((step_y.px.y - center.px.y) - 10.0).abs() < 1e-3);

            let step_z = frame
                .project_sim_to_screen(SimVec3::new(10.0, 20.0, FLOOR_H))
                .unwrap();
            assert!((step_z.px.x - center.px.x).abs() < 1e-3);
            assert!(((step_z.px.y - center.px.y) - (-34.0)).abs() < 0.01);

            let se_fg = frame
                .project_sim_to_screen(SimVec3::new(12.0, 22.0, 0.0))
                .unwrap();
            let nw_bg = frame
                .project_sim_to_screen(SimVec3::new(8.0, 18.0, 0.0))
                .unwrap();
            assert!(se_fg.in_depth_range());
            assert!(nw_bg.in_depth_range());
            assert!(
                se_fg.depth < nw_bg.depth,
                "Southeast foreground depth {} should be smaller than Northwest {}",
                se_fg.depth,
                nw_bg.depth
            );
        }
    }

    #[test]
    fn project_and_unproject_roundtrip_all_five_modes() {
        let ground_z = FLOOR_H;
        let target = CameraTarget {
            position: SimVec3::new(12.0, 24.0, ground_z),
            velocity: SimVec3::ZERO,
            aim: SimVec2::new(0.6, -0.8),
            heading: (-0.8_f32).atan2(0.6),
            look_ahead_distance: 1.0,
        };

        for mode in CameraMode::ALL {
            let mut rig = CameraRig::new(800, 450);
            rig.set_mode(mode);
            let frame = rig.update(target, 0.0);

            let sample_sim = match mode {
                CameraMode::FirstPerson => {
                    // Place point 5 units ahead on the same floor plane so it is in front of the eye.
                    let fwd = sim_vec2_from_heading(target.heading);
                    SimVec3::new(
                        target.position.x + fwd.x * 5.0,
                        target.position.y + fwd.y * 5.0,
                        ground_z,
                    )
                }
                _ => SimVec3::new(13.25, 22.75, ground_z),
            };

            let projected = frame
                .project_sim_to_screen(sample_sim)
                .expect("sample point should project");
            assert!(projected.in_depth_range());
            assert!(projected.in_viewport(frame.viewport_size));

            let unprojected = frame
                .screen_to_sim_plane_z(projected.px, ground_z)
                .expect("ray should hit ground_z plane");
            assert!(
                (unprojected.x - sample_sim.x).abs() < 1e-3
                    && (unprojected.y - sample_sim.y).abs() < 1e-3,
                "mode {mode:?}: expected ({}, {}), got ({}, {})",
                sample_sim.x,
                sample_sim.y,
                unprojected.x,
                unprojected.y
            );
        }

        // Exact 90-degree straight-down Overhead must produce finite matrices and valid unprojection.
        let mut overhead_rig = CameraRig::new(640, 360);
        overhead_rig.config_mut().overhead_pitch = FRAC_PI_2;
        overhead_rig.set_mode(CameraMode::Overhead);
        let oh_frame = overhead_rig.update(target, 0.0);
        assert!(oh_frame.view_proj.is_finite());
        let p = oh_frame.project_sim_to_screen(target.position).unwrap();
        let recovered = oh_frame.screen_to_sim_plane_z(p.px, ground_z).unwrap();
        assert!((recovered.x - target.position.x).abs() < 1e-3);
        assert!((recovered.y - target.position.y).abs() < 1e-3);

        // Behind-camera points in Chase and FirstPerson return None from project_to_screen.
        for mode in [CameraMode::Chase, CameraMode::FirstPerson] {
            let mut rig = CameraRig::new(640, 360);
            rig.set_mode(mode);
            let frame = rig.update(target, 0.0);
            let behind = frame.eye - frame.forward * 2.0;
            assert!(frame.project_to_screen(behind).is_none());
        }
    }

    #[test]
    fn frustum_culling_containment_and_bounds() {
        let target = CameraTarget {
            position: SimVec3::new(24.0, 24.0, 0.0),
            aim: SimVec2::new(1.0, 0.0),
            heading: 0.0,
            ..CameraTarget::default()
        };

        for mode in [
            CameraMode::Isometric,
            CameraMode::Chase,
            CameraMode::FirstPerson,
        ] {
            let mut rig = CameraRig::new(640, 360);
            rig.set_mode(mode);
            let frame = rig.update(target, 0.0);

            let in_front = if mode == CameraMode::FirstPerson {
                frame.eye + frame.forward * 5.0
            } else {
                frame.focus
            };
            assert!(
                frame.frustum.contains_point(in_front),
                "mode {mode:?}: expected point in front to be inside frustum"
            );

            // Behind eye / before near plane must be outside.
            let behind_eye = frame.eye - frame.forward * 5.0;
            assert!(
                !frame.frustum.contains_point(behind_eye),
                "mode {mode:?}: expected behind-eye point to be culled"
            );

            // Just in front of eye within near plane must also be culled.
            let before_near = frame.eye + frame.forward * (frame.near * 0.25);
            assert!(
                !frame.frustum.contains_point(before_near),
                "mode {mode:?}: expected point closer than near plane to be culled"
            );

            // Beyond far plane must be culled.
            let beyond_far = frame.eye + frame.forward * (frame.far + 20.0);
            assert!(
                !frame.frustum.contains_point(beyond_far),
                "mode {mode:?}: expected point beyond far plane to be culled"
            );

            let focus_chunk = ChunkCoord::from_sim_xy(SimVec2::new(24.0, 24.0));
            assert!(
                frame
                    .frustum
                    .intersects_aabb(focus_chunk.render_aabb(-1.0, 6.0)),
                "mode {mode:?}: focus chunk must intersect frustum"
            );

            let far_chunk = ChunkCoord::from_sim_xy(SimVec2::new(24.0, 224.0));
            assert!(
                !frame
                    .frustum
                    .intersects_aabb(far_chunk.render_aabb(-1.0, 6.0)),
                "mode {mode:?}: distant off-side chunk must be culled"
            );
        }
    }

    #[test]
    fn stick_mapping_legacy_parity_and_all_modes_screen_alignment() {
        // 1. Isometric legacy parity across 16 angles and multiple magnitudes + edge inputs.
        let mut iso_rig = CameraRig::new(640, 360);
        let iso_target = CameraTarget {
            position: SimVec3::new(10.0, 10.0, 0.0),
            ..CameraTarget::default()
        };
        let iso_frame = iso_rig.update(iso_target, 0.0);

        for i in 0..16 {
            let angle = (i as f32) * (TAU / 16.0);
            let dir = SimVec2::new(angle.cos(), angle.sin());
            for mag in [0.4_f32, 1.0_f32] {
                let stick = dir * mag;
                let mapped = iso_frame.map_stick_to_world(stick);

                let legacy_raw = SimVec2::new(
                    stick.x / 20.0 + stick.y / 10.0,
                    stick.y / 10.0 - stick.x / 20.0,
                );
                let legacy = legacy_raw.normalized() * mag.min(1.0);
                assert!(
                    (mapped.x - legacy.x).abs() < 1e-5 && (mapped.y - legacy.y).abs() < 1e-5,
                    "angle {i} mag {mag}: mapped {:?} vs legacy {:?}",
                    mapped,
                    legacy
                );
            }
        }

        // Clamping diagonal (1, 1) to unit length and non-finite/zero -> ZERO.
        let diag = iso_frame.map_stick_to_world(SimVec2::new(1.0, 1.0));
        assert!((diag.length() - 1.0).abs() < 1e-5);
        assert_eq!(iso_frame.map_stick_to_world(SimVec2::ZERO), SimVec2::ZERO);
        assert_eq!(
            iso_frame.map_stick_to_world(SimVec2::new(f32::NAN, 0.0)),
            SimVec2::ZERO
        );
        assert_eq!(
            iso_frame.map_stick_to_world(SimVec2::new(0.0, f32::INFINITY)),
            SimVec2::ZERO
        );

        // Street-angle alignment in Isometric.
        let east_stick = SimVec2::new(2.0, 1.0).normalized();
        let east_world = iso_frame.map_stick_to_world(east_stick);
        assert!((east_world.x - 1.0).abs() < 1e-5 && east_world.y.abs() < 1e-5);

        let south_stick = SimVec2::new(-2.0, 1.0).normalized();
        let south_world = iso_frame.map_stick_to_world(south_stick);
        assert!(south_world.x.abs() < 1e-5 && (south_world.y - 1.0).abs() < 1e-5);

        // 2. Directional modes: Overhead, Orbit (yaw = 1.1), Chase (heading = 0.7), FirstPerson (heading = -2.1).
        let mode_cases = [
            (CameraMode::Overhead, 0.0_f32),
            (CameraMode::Orbit, 0.0_f32),
            (CameraMode::Chase, 0.7_f32),
            (CameraMode::FirstPerson, -2.1_f32),
        ];

        for (mode, heading) in mode_cases {
            let mut rig = CameraRig::new(640, 360);
            rig.set_look_ahead(false);
            rig.set_mode(mode);
            if mode == CameraMode::Orbit {
                let delta = 1.1 - rig.config().orbit_default_yaw;
                rig.orbit_rotate(delta, 0.0);
            }
            let target = CameraTarget {
                position: SimVec3::new(30.0, 40.0, 0.0),
                heading,
                aim: sim_vec2_from_heading(heading),
                ..CameraTarget::default()
            };
            let frame = rig.update(target, 0.0);

            // Analog quiet-walk magnitude preservation.
            let quiet = frame.map_stick_to_world(SimVec2::new(0.3, -0.4));
            assert!((quiet.length() - 0.5).abs() < 1e-5);

            let ref_sim = if mode == CameraMode::FirstPerson {
                let fwd = sim_vec2_from_heading(heading);
                SimVec3::new(
                    target.position.x + fwd.x * 4.0,
                    target.position.y + fwd.y * 4.0,
                    0.0,
                )
            } else {
                target.position
            };
            let base_px = frame.project_sim_to_screen(ref_sim).unwrap().px;

            let step = 0.05;
            for (stick, expected_sign) in [
                (SimVec2::new(1.0, 0.0), (1.0_f32, 0.0_f32)),
                (SimVec2::new(-1.0, 0.0), (-1.0_f32, 0.0_f32)),
                (SimVec2::new(0.0, -1.0), (0.0_f32, -1.0_f32)),
                (SimVec2::new(0.0, 1.0), (0.0_f32, 1.0_f32)),
            ] {
                let world_dir = frame.map_stick_to_world(stick);
                let moved = SimVec3::new(
                    ref_sim.x + world_dir.x * step,
                    ref_sim.y + world_dir.y * step,
                    ref_sim.z,
                );
                let moved_px = frame.project_sim_to_screen(moved).unwrap().px;
                let ds = moved_px - base_px;
                if expected_sign.0 > 0.0 {
                    assert!(ds.x > 0.0 && ds.y.abs() < 1e-3, "{mode:?} right: {ds:?}");
                } else if expected_sign.0 < 0.0 {
                    assert!(ds.x < 0.0 && ds.y.abs() < 1e-3, "{mode:?} left: {ds:?}");
                } else if expected_sign.1 < 0.0 {
                    assert!(ds.y < 0.0 && ds.x.abs() < 1e-3, "{mode:?} up: {ds:?}");
                } else {
                    assert!(ds.y > 0.0 && ds.x.abs() < 1e-3, "{mode:?} down: {ds:?}");
                }
            }
        }
    }

    #[test]
    fn snapped_origin_preserves_projection_across_chunk_boundaries() {
        let mut rig = CameraRig::new(640, 360);
        for x in [15.9, 16.1, 512.0] {
            let frame = rig.update(
                CameraTarget {
                    position: SimVec3::new(x, 48.0, 0.0),
                    ..Default::default()
                },
                1.0,
            );
            let uniforms = frame.to_gpu_uniforms();
            let origin = frame.render_origin();
            let world = Vec3::new(x, 0.0, 48.0);
            let actual =
                Mat4::from_cols_array_2d(&uniforms.view_proj) * (world - origin).extend(1.0);
            let expected = frame.view_proj * world.extend(1.0);
            assert!((actual - expected).abs().max_element() < 0.0001);
            assert_eq!(origin.x % zombiebox_core::CHUNK_SIZE as f32, 0.0);
        }
    }

    #[test]
    fn frame_rate_independent_damping_teleport_and_display_scaling() {
        let initial = CameraTarget {
            position: SimVec3::new(10.0, 10.0, 0.0),
            ..CameraTarget::default()
        };
        let moved = CameraTarget {
            position: SimVec3::new(13.0, 8.0, 0.0),
            ..CameraTarget::default()
        };

        let mut rig_30 = CameraRig::new(640, 360);
        let mut rig_60 = CameraRig::new(640, 360);
        let mut rig_120 = CameraRig::new(640, 360);
        let _ = rig_30.update(initial, 0.0);
        let _ = rig_60.update(initial, 0.0);
        let _ = rig_120.update(initial, 0.0);

        let mut f30 = rig_30.current_frame();
        for _ in 0..30 {
            f30 = rig_30.update(moved, 1.0 / 30.0);
        }
        let mut f60 = rig_60.current_frame();
        for _ in 0..60 {
            f60 = rig_60.update(moved, 1.0 / 60.0);
        }
        let mut f120 = rig_120.current_frame();
        for _ in 0..120 {
            f120 = rig_120.update(moved, 1.0 / 120.0);
        }

        assert!((f30.focus - f60.focus).length() < 1e-4);
        assert!((f60.focus - f120.focus).length() < 1e-4);
        assert!((f30.eye - f60.eye).length() < 1e-4);
        assert!((f60.eye - f120.eye).length() < 1e-4);

        // Teleport snap via large distance (> teleport_snap_xy).
        let far = CameraTarget {
            position: SimVec3::new(25.0, 8.0, 0.0),
            ..CameraTarget::default()
        };
        let snapped = rig_60.update(far, 1.0 / 60.0);
        assert!((snapped.focus - sim_to_render_point(far.position)).length() < 1e-4);

        // Teleport snap via explicit notify_teleport() on a small delta.
        rig_60.notify_teleport();
        let tiny_hop = CameraTarget {
            position: SimVec3::new(26.0, 8.5, 0.0),
            ..CameraTarget::default()
        };
        let snapped2 = rig_60.update(tiny_hop, 1.0 / 60.0);
        assert!((snapped2.focus - sim_to_render_point(tiny_hop.position)).length() < 1e-4);

        // Display surface scaling (1920x1080 -> 640x360 internal).
        let mut scaled_rig = CameraRig::new(1920, 1080);
        scaled_rig.set_look_ahead(false);
        scaled_rig.set_display_surface(1920, 1080);
        assert_eq!(scaled_rig.viewport_size(), UVec2::new(1920, 1080));
        assert_eq!(scaled_rig.internal_size(), UVec2::new(640, 360));

        let origin = SimVec3::new(50.0, 60.0, 0.0);
        let scaled_frame = scaled_rig.update(
            CameraTarget {
                position: origin,
                ..CameraTarget::default()
            },
            0.0,
        );

        let p0_int = scaled_frame
            .project_to_internal(sim_to_render_point(origin))
            .unwrap();
        let px_int = scaled_frame
            .project_to_internal(sim_to_render_point(SimVec3::new(51.0, 60.0, 0.0)))
            .unwrap();
        assert!(((px_int.px.x - p0_int.px.x) - 20.0).abs() < 1e-3);
        assert!(((px_int.px.y - p0_int.px.y) - 10.0).abs() < 1e-3);

        let p0_win = scaled_frame.project_sim_to_screen(origin).unwrap();
        let step_pos = SimVec3::new(51.0, 60.0, 0.0);
        let px_win = scaled_frame.project_sim_to_screen(step_pos).unwrap();
        assert!(((px_win.px.x - p0_win.px.x) - 60.0).abs() < 1e-3);
        assert!(((px_win.px.y - p0_win.px.y) - 30.0).abs() < 1e-3);

        let unproj_win = scaled_frame.screen_to_sim_plane_z(px_win.px, 0.0).unwrap();
        assert!((unproj_win.x - step_pos.x).abs() < 1e-3);
        assert!((unproj_win.y - step_pos.y).abs() < 1e-3);

        // GPU uniform layout check.
        assert_eq!(std::mem::size_of::<CameraGpuUniforms>(), 240);
        let uniforms = scaled_frame.to_gpu_uniforms();
        assert_eq!(bytemuck::bytes_of(&uniforms).len(), 240);

        // Orbit, FirstPerson look, obstruction clamp, shake, and sphere frustum helpers.
        assert_eq!(CameraMode::from_index(4), Some(CameraMode::FirstPerson));
        assert_eq!(CameraMode::from_index(5), None);
        assert_eq!(CameraMode::Orbit.label(), "Orbit");

        let pose_target = CameraTarget::from_sim_pose(
            SimVec3::new(2.0, 3.0, 0.0),
            SimVec2::new(0.0, 1.0),
            Some(Weapon::RocketLauncher),
        );
        assert!((pose_target.look_ahead_distance - 1.6).abs() < 1e-6);

        let mut rig = CameraRig::new(640, 360);
        rig.set_mode(CameraMode::Orbit);
        rig.orbit_rotate(10.0, 10.0);
        rig.orbit_zoom_by(-100.0);
        rig.orbit_scale_distance(2.0);
        let orbit_clamped = rig.update(pose_target, 0.0);
        assert!((orbit_clamped.pitch - rig.config().orbit_max_pitch).abs() < 1e-6);
        assert!(
            (orbit_clamped.unobstructed_distance - rig.config().orbit_min_distance * 2.0).abs()
                < 1e-5
        );
        rig.set_obstruction_distance(Some(1.5));
        let obstructed = rig.current_frame();
        assert!((obstructed.eye.distance(obstructed.focus) - 1.5).abs() < 1e-4);
        rig.reset_orbit();
        assert!(
            (rig.current_frame().unobstructed_distance - rig.config().orbit_default_distance).abs()
                < 1e-6
        );

        rig.set_mode(CameraMode::FirstPerson);
        rig.adjust_first_person_look(0.5, 10.0);
        let fp_clamped = rig.update(pose_target, 0.0);
        assert!((fp_clamped.pitch - rig.config().first_person_max_pitch).abs() < 1e-6);
        assert!((fp_clamped.eye.y - rig.config().first_person_eye_height).abs() < 1e-5);
        assert!(fp_clamped.eye.y < FLOOR_H);
        rig.reset_first_person_look();
        assert!((rig.current_frame().pitch - rig.config().first_person_default_pitch).abs() < 1e-6);

        let base_eye = rig.current_frame().eye;
        rig.add_shake(4.0);
        let shaken = rig.update(pose_target, 1.0 / 60.0);
        assert!(shaken.eye.distance(base_eye) > 1e-4);
        rig.notify_teleport();
        let after_teleport = rig.update(pose_target, 0.0);
        assert!(after_teleport.eye.distance(base_eye) < 1e-5);

        let iso_ray = scaled_frame.screen_to_ray(p0_win.px);
        assert!(iso_ray.intersect_plane_y(0.0).is_some());
        assert!(
            scaled_frame
                .frustum
                .intersects_sphere(scaled_frame.focus, 1.0)
        );
    }

    #[test]
    fn look_stick_turns_right_and_looks_down_with_nonlinear_response() {
        let target = CameraTarget {
            position: SimVec3::new(10.0, 10.0, 0.0),
            heading: 0.0,
            aim: sim_vec2_from_heading(0.0),
            ..CameraTarget::default()
        };

        let mut rig = CameraRig::new(640, 360);
        rig.set_mode(CameraMode::Chase);
        let start = rig.update(target, 0.0);

        rig.apply_look_input(SimVec2::new(1.0, 1.0), 0.1);
        let turned = rig.current_frame();
        let yaw_delta = crate::coords::shortest_angle_delta(start.yaw, turned.yaw);
        let expected_yaw = rig.config().look_yaw_speed * 0.1;
        let expected_pitch = rig.config().look_pitch_speed * 0.1;
        assert!(
            (yaw_delta - expected_yaw).abs() < 1e-5,
            "yaw delta {yaw_delta}"
        );
        assert!(
            (turned.pitch - (start.pitch + expected_pitch)).abs() < 1e-5,
            "pitch {} -> {}",
            start.pitch,
            turned.pitch
        );

        // The response curve keeps small deflections precise at full-deflection speed.
        assert_eq!(shape_look_axis(0.0, 0.04, 1.6), 0.0);
        assert_eq!(shape_look_axis(0.03, 0.04, 1.6), 0.0);
        assert!((shape_look_axis(1.0, 0.04, 1.6) - 1.0).abs() < 1e-6);
        assert!(shape_look_axis(0.2, 0.04, 1.6) < 0.2);
        assert!(shape_look_axis(-0.2, 0.04, 1.6) < 0.0);
    }

    #[test]
    fn view_orientation_stays_independent_after_heading_changes() {
        let target = CameraTarget {
            position: SimVec3::new(10.0, 10.0, 0.0),
            heading: 0.0,
            aim: sim_vec2_from_heading(0.0),
            ..CameraTarget::default()
        };
        let mut rig = CameraRig::new(640, 360);
        rig.set_mode(CameraMode::FirstPerson);
        let start = rig.update(target, 0.0);

        rig.apply_look_input(SimVec2::new(0.4, 0.0), 0.25);
        let looked = rig.current_frame();
        assert!(crate::coords::shortest_angle_delta(start.yaw, looked.yaw) > 0.0);

        // Heading changes from movement/aim feedback must not pull the view back.
        let moved_target = CameraTarget {
            heading: 0.9,
            aim: sim_vec2_from_heading(0.9),
            ..target
        };
        let after_move = rig.update(moved_target, 1.0 / 60.0);
        assert!((after_move.yaw - looked.yaw).abs() < 1e-6);

        // Entering a view mode explicitly re-syncs yaw to the actor heading.
        rig.set_mode(CameraMode::Chase);
        let chase = rig.update(moved_target, 0.0);
        assert!(
            (crate::coords::shortest_angle_delta(moved_target.heading + PI, chase.yaw)).abs()
                < 1e-6
        );
        assert!((chase.pitch - rig.config().chase_pitch).abs() < 1e-6);
    }

    #[test]
    fn mounted_views_snap_follow_level_and_release_look_on_dismount() {
        let mut target = CameraTarget {
            position: SimVec3::new(10.0, 10.0, 0.0),
            heading: 0.0,
            aim: sim_vec2_from_heading(0.0),
            ..CameraTarget::default()
        };
        let mut rig = CameraRig::new(640, 360);
        rig.set_mode(CameraMode::Chase);
        let _ = rig.update(target, 0.0);
        rig.apply_look_input(SimVec2::new(1.0, 0.0), 0.1);
        target.heading = -0.04;
        let mounted = rig.update_mounted(target, 0.0);
        assert!(shortest_angle_delta(mounted.yaw, target.heading + PI).abs() < 1e-5);

        target.heading = 0.04;
        let dt = MOUNTED_CHASE_YAW_HALFLIFE_S / 2.0;
        let turning = rig.update_mounted(target, dt);
        assert!(shortest_angle_delta(mounted.yaw, turning.yaw) > 0.0);
        let followed = rig.update_mounted(target, dt);
        assert!(shortest_angle_delta(followed.yaw, PI).abs() < 1e-5);

        rig.set_mode(CameraMode::FirstPerson);
        let fp = rig.update_mounted(target, 0.0);
        assert!(shortest_angle_delta(fp.yaw, target.heading + PI).abs() < 1e-5);
        assert!((fp.pitch - rig.config().first_person_default_pitch).abs() < 1e-6);
        target.position.x += 2.0;
        let fp_moved = rig.update_mounted(target, 0.016);
        assert!((fp_moved.eye.x - fp.eye.x - 2.0).abs() < 1e-4);
        target.heading = FRAC_PI_2;
        target.velocity = SimVec3::new(0.0, -2.0, 0.0);
        let reversing = rig.update_mounted(target, 0.016);
        assert!(shortest_angle_delta(reversing.yaw, target.heading + PI).abs() < 1e-5);
        assert!(reversing.right.y.abs() < 1e-6);

        let foot = rig.update(target, 0.0);
        assert!(shortest_angle_delta(foot.yaw, reversing.yaw).abs() < 1e-5);
        rig.apply_look_input(SimVec2::new(1.0, 0.0), 0.1);
        assert!(shortest_angle_delta(foot.yaw, rig.current_frame().yaw) > 0.0);
    }
}
