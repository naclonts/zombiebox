//! Fast local-viewport dusk lighting, visibility shading, and selective light pools.
//!
//! All lighting math uses fixed-point RGB multipliers (`128 == 1.0x`) stored in a
//! reused `48×48` local viewport grid with zero per-frame heap allocations.

use zombiebox_core::{
    ActionGame, Floor, LightKind, LightSourceView, Position, TileRenderInfo, TileVisibility, Vec2,
};

use crate::{Canvas, Projector};

pub(crate) const LIGHT_GRID_SIZE: usize = 48;
const LIGHT_GRID_LEN: usize = LIGHT_GRID_SIZE * LIGHT_GRID_SIZE;

/// Cool shrouded interior silhouette color for `TileVisibility::Hidden` indoor tiles.
pub(crate) const SHROUD_COLOR: [u8; 4] = [34, 38, 48, 255];
pub(crate) const SHROUD_WALL_COLOR: [u8; 4] = [24, 28, 36, 255];
pub(crate) const SHROUD_TRIM_COLOR: [u8; 4] = [42, 47, 59, 255];

/// Actor readability floor in `128 == 1.0x` space (`0.625..0.734x`) so the survivor
/// and visible monsters always pop cleanly against dark floors.
const ACTOR_FLOOR_R: u8 = 80;
const ACTOR_FLOOR_G: u8 = 84;
const ACTOR_FLOOR_B: u8 = 94;

/// Clamped ceiling in `128 == 1.0x` space so warm highlights never blow out.
const MAX_LIGHT_R: u16 = 154;
const MAX_LIGHT_G: u16 = 146;
const MAX_LIGHT_B: u16 = 138;

/// Cool twilight blue-slate outdoor dusk ambient and moody dark indoor ambient (`128 == 1.0x`).
const OUTDOOR_AMBIENT: (u16, u16, u16) = (84, 94, 114);
const INDOOR_AMBIENT: (u16, u16, u16) = (50, 54, 68);

/// Visual pool radii (tiles) for warm sodium streetlights, porch lamps, and survivor halo.
const STREETLIGHT_VISUAL_RADIUS: f32 = 4.8;
const PORCH_VISUAL_RADIUS: f32 = 3.2;
const CACHE_VISUAL_RADIUS: f32 = 2.0;
const SURVIVOR_HALO_RADIUS: f32 = 2.6;

/// `Explored` tiles are dimmed to `0.58x` (`148 / 256`).
const EXPLORED_SCALE_256: u16 = 148;

/// Per-cell RGB light multiplier (`128 == 1.0x`) plus LOS visibility state.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CellLight {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub vis: TileVisibility,
}

impl CellLight {
    pub(crate) const OUTDOOR_DUSK: Self = Self {
        r: OUTDOOR_AMBIENT.0 as u8,
        g: OUTDOOR_AMBIENT.1 as u8,
        b: OUTDOOR_AMBIENT.2 as u8,
        vis: TileVisibility::Visible,
    };
}

/// Transient colored light contributed by muzzle flashes, rockets, blasts, or caustic bursts.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DynamicLight {
    pub pos: Vec2,
    pub floor: Floor,
    pub radius: f32,
    /// Additive intensity in `128 == 1.0x` light-grid units.
    pub r: u16,
    pub g: u16,
    pub b: u16,
    /// Optional additive screen-space glow color & strength.
    pub glow_rgb: [u8; 3],
    pub glow_strength: f32,
    pub glow_radius_px: f32,
}

/// Fast per-polygon / per-palette color tint (`128 == 1.0x`, zero divisions).
#[inline(always)]
pub(crate) fn tint(color: [u8; 4], light: CellLight) -> [u8; 4] {
    [
        ((color[0] as u16 * light.r as u16) >> 7).min(255) as u8,
        ((color[1] as u16 * light.g as u16) >> 7).min(255) as u8,
        ((color[2] as u16 * light.b as u16) >> 7).min(255) as u8,
        color[3],
    ]
}

/// Apply the actor readability floor `(80, 84, 94)` so survivor & visible monsters pop cleanly.
#[inline(always)]
pub(crate) fn actor_light(light: CellLight) -> CellLight {
    CellLight {
        r: light.r.max(ACTOR_FLOOR_R),
        g: light.g.max(ACTOR_FLOOR_G),
        b: light.b.max(ACTOR_FLOOR_B),
        vis: light.vis,
    }
}

/// Slightly dimmed actor readability floor for outdoor Floor 0 monsters viewed from Floor 1.
#[inline(always)]
pub(crate) fn lower_floor_actor_light(light: CellLight) -> CellLight {
    CellLight {
        r: light.r.max(60),
        g: light.g.max(64),
        b: light.b.max(74),
        vis: light.vis,
    }
}

/// Reused `48×48` local viewport lighting & tile-info cache (zero per-frame heap allocations).
pub(crate) struct LightGrid {
    origin_x: i32,
    origin_y: i32,
    tiles: Box<[Option<TileRenderInfo>; LIGHT_GRID_LEN]>,
    lights: Box<[CellLight; LIGHT_GRID_LEN]>,
    accum_r: Box<[u16; LIGHT_GRID_LEN]>,
    accum_g: Box<[u16; LIGHT_GRID_LEN]>,
    accum_b: Box<[u16; LIGHT_GRID_LEN]>,
}

impl LightGrid {
    pub(crate) fn new() -> Self {
        Self {
            origin_x: 0,
            origin_y: 0,
            tiles: Box::new([None; LIGHT_GRID_LEN]),
            lights: Box::new([CellLight::OUTDOOR_DUSK; LIGHT_GRID_LEN]),
            accum_r: Box::new([0; LIGHT_GRID_LEN]),
            accum_g: Box::new([0; LIGHT_GRID_LEN]),
            accum_b: Box::new([0; LIGHT_GRID_LEN]),
        }
    }

    #[inline(always)]
    fn index(&self, wx: i32, wy: i32) -> Option<usize> {
        let lx = wx - self.origin_x;
        let ly = wy - self.origin_y;
        if (0..LIGHT_GRID_SIZE as i32).contains(&lx) && (0..LIGHT_GRID_SIZE as i32).contains(&ly) {
            Some(ly as usize * LIGHT_GRID_SIZE + lx as usize)
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) fn tile_info(&self, wx: i32, wy: i32) -> Option<TileRenderInfo> {
        self.index(wx, wy).and_then(|i| self.tiles[i])
    }

    #[inline(always)]
    pub(crate) fn cell_light(&self, wx: i32, wy: i32) -> CellLight {
        self.index(wx, wy)
            .map_or(CellLight::OUTDOOR_DUSK, |i| self.lights[i])
    }

    #[inline(always)]
    pub(crate) fn sample_light(&self, pos: Vec2) -> CellLight {
        self.cell_light(pos.x.round() as i32, pos.y.round() as i32)
    }

    /// Populate the `48×48` local viewport grid around `(center_x, center_y)` for `floor`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn populate(
        &mut self,
        game: &ActionGame,
        center_x: i32,
        center_y: i32,
        floor: Floor,
        player_pos: Vec2,
        player_floor: Floor,
        static_lights: &[LightSourceView],
        flashes: &[DynamicLight],
        dim_scale_256: u16,
    ) {
        self.origin_x = center_x - (LIGHT_GRID_SIZE as i32 / 2);
        self.origin_y = center_y - (LIGHT_GRID_SIZE as i32 / 2);

        // Slight twilight variation controlled by core dusk_level (0.72 default).
        let dusk = game.dusk_level().clamp(0.35, 1.0);
        let dusk_mult = (1.08 - 0.11 * dusk).clamp(0.92, 1.08);
        let out_r = (OUTDOOR_AMBIENT.0 as f32 * dusk_mult) as u16;
        let out_g = (OUTDOOR_AMBIENT.1 as f32 * dusk_mult) as u16;
        let out_b = (OUTDOOR_AMBIENT.2 as f32 * dusk_mult) as u16;

        // Pass 1: Sample tile_render_info once per cell and seed base ambient + core wall-occluded light.
        for ly in 0..LIGHT_GRID_SIZE {
            let wy = self.origin_y + ly as i32;
            let row = ly * LIGHT_GRID_SIZE;
            for lx in 0..LIGHT_GRID_SIZE {
                let wx = self.origin_x + lx as i32;
                let idx = row + lx;
                let info = game.tile_render_info(Position::new(wx, wy), floor);
                self.tiles[idx] = info;

                if let Some(ti) = info {
                    let (base_r, base_g, base_b, core_base) = if ti.indoor {
                        (INDOOR_AMBIENT.0, INDOOR_AMBIENT.1, INDOOR_AMBIENT.2, 42u16)
                    } else {
                        (out_r, out_g, out_b, 92u16)
                    };
                    // Core `ti.light` already accounts for raycasted wall & closed-door occlusion.
                    // Translate excess core light into warm spill (e.g. light pouring through windows/open doors).
                    let excess = (ti.light as u16).saturating_sub(core_base);
                    let spill_r = (excess * 5) >> 4;
                    let spill_g = (excess * 4) >> 4;
                    let spill_b = (excess * 2) >> 4;

                    self.accum_r[idx] = base_r + spill_r;
                    self.accum_g[idx] = base_g + spill_g;
                    self.accum_b[idx] = base_b + spill_b;
                } else {
                    // Outside residential block bounds: cool twilight county edge.
                    self.accum_r[idx] = (out_r * 7) >> 3;
                    self.accum_g[idx] = (out_g * 7) >> 3;
                    self.accum_b[idx] = (out_b * 7) >> 3;
                }
            }
        }

        // Pass 2: Inject warm sodium streetlights, porch lamps, and visible cache glows.
        for src in static_lights {
            if !src.active {
                continue;
            }
            let (radius, add_r, add_g, add_b, floor_scale) = match src.kind {
                LightKind::Streetlight => {
                    let fs = if src.floor == floor {
                        1.0
                    } else if src.floor == 0 && floor == 1 {
                        0.45
                    } else {
                        0.0
                    };
                    (STREETLIGHT_VISUAL_RADIUS, 62.0, 44.0, 14.0, fs)
                }
                LightKind::Porch => {
                    if src.floor != floor {
                        continue;
                    }
                    (PORCH_VISUAL_RADIUS, 54.0, 38.0, 14.0, 1.0)
                }
                LightKind::Cache => {
                    if src.floor != floor {
                        continue;
                    }
                    // Unrevealed caches in hidden rooms never emit visible light.
                    let cx = src.pos.x.round() as i32;
                    let cy = src.pos.y.round() as i32;
                    if let Some(ti) = self.tile_info(cx, cy)
                        && ti.visibility == TileVisibility::Hidden
                    {
                        continue;
                    }
                    (CACHE_VISUAL_RADIUS, 34.0, 26.0, 8.0, 1.0)
                }
            };
            if floor_scale <= 0.0 {
                continue;
            }
            self.splat_light(
                src.pos,
                radius,
                add_r * floor_scale,
                add_g * floor_scale,
                add_b * floor_scale,
                src.kind != LightKind::Cache,
                false,
            );
        }

        // Pass 3: Subtle survivor personal halo (r = 2.6, same floor, visible tiles only).
        if player_floor == floor {
            self.splat_light(
                player_pos,
                SURVIVOR_HALO_RADIUS,
                38.0,
                38.0,
                34.0,
                false,
                true,
            );
        }

        // Pass 4: Transient combat flashes (muzzle flash, rocket glow, rocket blast, bloater burst).
        for flash in flashes {
            let floor_scale = if flash.floor == floor { 1.0 } else { 0.3 };
            if flash.radius <= 0.0 {
                continue;
            }
            self.splat_light(
                flash.pos,
                flash.radius,
                flash.r as f32 * floor_scale,
                flash.g as f32 * floor_scale,
                flash.b as f32 * floor_scale,
                false,
                false,
            );
        }

        // Pass 5: Clamp ceiling and apply TileVisibility (`Visible`, `Explored` 0.58x, `Hidden`).
        for idx in 0..LIGHT_GRID_LEN {
            let mut r = self.accum_r[idx].min(MAX_LIGHT_R);
            let mut g = self.accum_g[idx].min(MAX_LIGHT_G);
            let mut b = self.accum_b[idx].min(MAX_LIGHT_B);

            let vis = self.tiles[idx].map_or(TileVisibility::Explored, |ti| ti.visibility);
            let indoor = self.tiles[idx].is_some_and(|ti| ti.indoor);

            match vis {
                TileVisibility::Visible => {}
                TileVisibility::Explored => {
                    r = (r * EXPLORED_SCALE_256) >> 8;
                    g = (g * EXPLORED_SCALE_256) >> 8;
                    b = (b * EXPLORED_SCALE_256) >> 8;
                }
                TileVisibility::Hidden => {
                    if indoor {
                        // Shrouded interior silhouette handled directly in street renderer;
                        // keep a dark cool fallback tint for any tinted elements.
                        r = 24;
                        g = 27;
                        b = 35;
                    } else {
                        // Distant or building-shadowed outdoor twilight fog.
                        r = (r * 176) >> 8;
                        g = (g * 182) >> 8;
                        b = (b * 194) >> 8;
                    }
                }
            }

            if dim_scale_256 != 256 {
                r = (r * dim_scale_256) >> 8;
                g = (g * dim_scale_256) >> 8;
                b = (b * dim_scale_256) >> 8;
            }

            self.lights[idx] = CellLight {
                r: r.min(255) as u8,
                g: g.min(255) as u8,
                b: b.min(255) as u8,
                vis,
            };
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn splat_light(
        &mut self,
        pos: Vec2,
        radius: f32,
        add_r: f32,
        add_g: f32,
        add_b: f32,
        respect_wall_occlusion: bool,
        visible_only: bool,
    ) {
        let r_ceil = radius.ceil() as i32;
        let min_lx = (pos.x.floor() as i32 - r_ceil - self.origin_x).max(0) as usize;
        let max_lx = (pos.x.ceil() as i32 + r_ceil - self.origin_x).min(LIGHT_GRID_SIZE as i32 - 1);
        let min_ly = (pos.y.floor() as i32 - r_ceil - self.origin_y).max(0) as usize;
        let max_ly = (pos.y.ceil() as i32 + r_ceil - self.origin_y).min(LIGHT_GRID_SIZE as i32 - 1);
        if max_lx < min_lx as i32 || max_ly < min_ly as i32 {
            return;
        }
        let max_lx = max_lx as usize;
        let max_ly = max_ly as usize;
        let inv_r = 1.0 / radius;
        let r_sq = radius * radius;

        for ly in min_ly..=max_ly {
            let wy = (self.origin_y + ly as i32) as f32;
            let dy = wy - pos.y;
            let dy_sq = dy * dy;
            if dy_sq >= r_sq {
                continue;
            }
            let row = ly * LIGHT_GRID_SIZE;
            for lx in min_lx..=max_lx {
                let wx = (self.origin_x + lx as i32) as f32;
                let dx = wx - pos.x;
                let dist_sq = dx * dx + dy_sq;
                if dist_sq >= r_sq {
                    continue;
                }
                let idx = row + lx;
                let mut atten = 1.0;
                if let Some(ti) = self.tiles[idx] {
                    if visible_only && ti.visibility != TileVisibility::Visible {
                        continue;
                    }
                    // Prevent outdoor streetlights/porch lights from bleeding through solid walls
                    // into closed unlit rooms (`core light <= 46`).
                    if respect_wall_occlusion && ti.indoor && ti.light <= 46 {
                        continue;
                    }
                    if respect_wall_occlusion && ti.indoor {
                        atten = ((ti.light.saturating_sub(42)) as f32 / 55.0).clamp(0.15, 0.85);
                    }
                } else if visible_only {
                    continue;
                }

                let t = 1.0 - dist_sq.sqrt() * inv_r;
                let falloff = t * t * atten;
                self.accum_r[idx] += (add_r * falloff) as u16;
                self.accum_g[idx] += (add_g * falloff) as u16;
                self.accum_b[idx] += (add_b * falloff) as u16;
            }
        }
    }

    /// Selective additive 2:1 elliptical `Canvas::glow_iso` pools ONLY for on-screen
    /// working streetlights, porch lamps, visible caches, and active combat flashes.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_light_pools(
        &self,
        c: &mut Canvas<'_>,
        proj: &Projector,
        static_lights: &[LightSourceView],
        flashes: &[DynamicLight],
        floor: Floor,
        elapsed_ms: u64,
        dim_scale: f32,
    ) {
        let z = floor as f32;
        for src in static_lights {
            if !src.active || src.floor != floor {
                continue;
            }
            let tx = src.pos.x.round() as i32;
            let ty = src.pos.y.round() as i32;
            if let Some(ti) = self.tile_info(tx, ty)
                && ti.visibility == TileVisibility::Hidden
                && src.kind == LightKind::Cache
            {
                continue;
            }

            let (sx, sy) = proj.project_z(src.pos.x, src.pos.y, z);
            match src.kind {
                LightKind::Streetlight => {
                    // Subtle flicker if streetlight pole is damaged.
                    let mut strength = 38.0 * dim_scale;
                    if let Some(ti) = self.tile_info(tx, ty)
                        && ti.max_hp > 0
                        && ti.hp < ti.max_hp
                    {
                        let flicker = ((elapsed_ms / 55).wrapping_mul(2654435761) & 7) as f32;
                        if flicker < 2.0 {
                            strength *= 0.35;
                        }
                    }
                    // Warm sodium ground pool (2:1 isometric ellipse) + lamp head halo.
                    c.glow_iso(sx, sy + 2.0, 74.0, 37.0, [255, 192, 92], strength);
                    c.glow_iso(
                        sx + 4.0,
                        sy - 38.0,
                        22.0,
                        16.0,
                        [255, 222, 138],
                        52.0 * dim_scale,
                    );
                }
                LightKind::Porch => {
                    c.glow_iso(sx, sy + 4.0, 44.0, 22.0, [255, 184, 88], 34.0 * dim_scale);
                }
                LightKind::Cache => {
                    let pulse = ((elapsed_ms as f32 * 0.0045).sin() + 1.0) * 6.0;
                    c.glow_iso(
                        sx,
                        sy - 4.0,
                        24.0,
                        13.0,
                        [248, 178, 72],
                        (34.0 + pulse) * dim_scale,
                    );
                }
            }
        }

        for flash in flashes {
            if flash.floor != floor || flash.glow_strength <= 0.5 {
                continue;
            }
            let (sx, sy) = proj.project_z(flash.pos.x, flash.pos.y, z);
            c.glow_iso(
                sx,
                sy - 8.0,
                flash.glow_radius_px,
                flash.glow_radius_px * 0.52,
                flash.glow_rgb,
                flash.glow_strength * dim_scale,
            );
        }
    }
}
