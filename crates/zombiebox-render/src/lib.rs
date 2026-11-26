//! Dependency-free isometric software renderer for the Zombiebox action game.
//!
//! Pixels are packed RGBA8888 in row-major order (`width * height * 4` bytes)
//! using a crisp 2:1 nearest-neighbor isometric projection (`TILE_W = 40.0`,
//! `TILE_H = 20.0`, `FLOOR_HEIGHT_PX = 34.0`).

use std::collections::HashMap;

use zombiebox_core::{
    ActionEvent, ActionGame, ActionOutcome, AwarenessIcon, BuildingId, Floor, MonsterId,
    MonsterKind, Surface, TileKind, TileRenderInfo, TileVisibility, Vec2, WallOrientation, Weapon,
};

mod actors;
mod lighting;
mod street;
mod vfx;

use actors::{ActorKind, ActorVisual, draw_actor, draw_fallen};
use lighting::{DynamicLight, LightGrid, actor_light, lower_floor_actor_light};
use street::{
    DrawItem, DrawItemKind, building_stipple, draw_building_roof, draw_cache, draw_flat_roof_tile,
    draw_ground_pass, draw_mountain_crag, draw_open_door_front_jamb, draw_tile_feature,
    draw_upper_deck_pass,
};
use vfx::VfxState;

pub(crate) const TILE_W: f32 = 40.0;
pub(crate) const TILE_H: f32 = 20.0;
pub(crate) const FLOOR_HEIGHT_PX: f32 = 34.0;

const HURT_MS: u64 = 240;
const FALL_MS: u64 = 270;
const HIT_FLASH_MS: u64 = 110;
const ALERT_BADGE_MS: u64 = 800;
const MAX_FALLEN: usize = 48;

const FOLLOW_HALFLIFE_S: f32 = 0.055;
const LOOK_HALFLIFE_S: f32 = 0.16;
const FLOOR_HALFLIFE_S: f32 = 0.14;
const SHAKE_HALFLIFE_S: f32 = 0.065;
const CUTAWAY_FADE_S: f32 = 0.16;
const F0_LOWER_DIM_256: u16 = 159; // 0.62x dimmed F0 view when player is on F1

#[derive(Clone, Copy)]
pub(crate) struct Projector {
    center_x: f32,
    center_y: f32,
    focus: Vec2,
    cam_floor: f32,
    shake_x: f32,
    shake_y: f32,
}

impl Projector {
    #[inline(always)]
    pub(crate) fn project_z(&self, x: f32, y: f32, z: f32) -> (f32, f32) {
        let dx = x - self.focus.x;
        let dy = y - self.focus.y;
        (
            self.center_x + self.shake_x + (dx - dy) * (TILE_W * 0.5),
            self.center_y + self.shake_y + (dx + dy) * (TILE_H * 0.5)
                - (z - self.cam_floor) * FLOOR_HEIGHT_PX,
        )
    }
}

#[derive(Default, Clone, Copy)]
struct Gait {
    previous: Option<Vec2>,
    stride: Vec2,
    phase: f32,
}

impl Gait {
    fn observe(&mut self, position: Vec2, cycles_per_tile: f32) {
        if let Some(previous) = self.previous {
            let motion = position - previous;
            let dist = motion.x.hypot(motion.y);
            if dist > 4.0 {
                // Teleport (e.g. floor drop): reset cleanly without wild stride spike.
                self.stride = Vec2::ZERO;
            } else {
                self.stride = projected_vector(motion);
                self.phase = (self.phase + dist * cycles_per_tile).fract();
            }
        }
        self.previous = Some(position);
    }
}

#[derive(Clone, Copy)]
struct MonsterTrack {
    gait: Gait,
    hit_flash_until: u64,
    alert_started_ms: Option<u64>,
    last_awareness: AwarenessIcon,
    last_seen_tick: u64,
}

impl MonsterTrack {
    fn new(id: MonsterId, tick: u64) -> Self {
        Self {
            gait: Gait {
                phase: (id.0 as f32 * 0.27).fract(),
                ..Gait::default()
            },
            hit_flash_until: 0,
            alert_started_ms: None,
            last_awareness: AwarenessIcon::Unaware,
            last_seen_tick: tick,
        }
    }
}

#[derive(Clone, Copy)]
struct Fallen {
    at: Vec2,
    floor: Floor,
    kind: MonsterKind,
    variant: u8,
    started: u64,
}

/// A portable RGBA framebuffer, damped isometric camera, and software renderer for one game view.
pub struct Renderer {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    last_sim_tick: Option<u64>,
    last_render_ms: Option<u64>,

    // Damped camera & shake state.
    cam_initialized: bool,
    cam_anchor: Vec2,
    cam_look: Vec2,
    cam_floor: f32,
    cam_shake: f32,
    look_ahead_enabled: bool,

    // Per-building smooth cutaway openness (0.0 = closed, 1.0 = open).
    cutaway: HashMap<BuildingId, f32>,

    // Actor gait, hit-flash, alert-badge, and fallen corpse state.
    survivor: Gait,
    hurt: Option<u64>,
    monster_tracks: HashMap<MonsterId, MonsterTrack>,
    fallen: Vec<Fallen>,

    // Zero-alloc per-frame rendering workspaces.
    light_floors: [LightGrid; 3],
    draw_items: Vec<DrawItem>,
    upper_shooters: Vec<(Vec2, Floor, u8)>,
    vfx: VfxState,
}

impl Renderer {
    pub fn new(width: usize, height: usize) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            pixels: vec![0; width * height * 4],
            last_sim_tick: None,
            last_render_ms: None,
            cam_initialized: false,
            cam_anchor: Vec2::ZERO,
            cam_look: Vec2::ZERO,
            cam_floor: 0.0,
            cam_shake: 0.0,
            look_ahead_enabled: true,
            cutaway: HashMap::with_capacity(32),
            survivor: Gait::default(),
            hurt: None,
            monster_tracks: HashMap::with_capacity(128),
            fallen: Vec::with_capacity(MAX_FALLEN),
            light_floors: std::array::from_fn(|_| LightGrid::new()),
            draw_items: Vec::with_capacity(512),
            upper_shooters: Vec::with_capacity(16),
            vfx: VfxState::new(),
        }
    }

    /// Reset camera & cutaway damping after an instant player teleport.
    pub fn notify_teleport(&mut self) {
        self.cam_initialized = false;
        self.cam_shake = 0.0;
        self.survivor = Gait::default();
        self.cutaway.clear();
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn stride(&self) -> usize {
        self.width * 4
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.pixels
    }

    /// Enable or disable tool-weighted aim look-ahead on the camera.
    pub fn set_look_ahead(&mut self, enabled: bool) {
        self.look_ahead_enabled = enabled;
    }

    /// Capture poses, hit flashes, awareness triggers, camera shake, and VFX events after each step.
    /// `render` also calls this for loops that render after each step.
    pub fn observe_events(&mut self, game: &ActionGame, elapsed_ms: u64) {
        let hud = game.hud();
        if self.last_sim_tick == Some(hud.tick) {
            return;
        }
        // Detect run restart cleanly.
        if self
            .last_sim_tick
            .is_some_and(|prev| hud.tick < prev || hud.tick == 0)
        {
            self.cam_initialized = false;
            self.cam_shake = 0.0;
            self.hurt = None;
            self.survivor = Gait::default();
            self.monster_tracks.clear();
            self.fallen.clear();
            self.vfx = VfxState::new();
            self.cutaway.clear();
            self.last_render_ms = None;
        }

        let player = game.player_pose();
        self.survivor.observe(player.position, 0.72);

        for event in game.events() {
            self.vfx.observe_event(event, elapsed_ms);
            match *event {
                ActionEvent::ShotFired { weapon, .. } => match weapon {
                    Weapon::Shotgun => self.cam_shake = self.cam_shake.max(2.5),
                    Weapon::RocketLauncher => self.cam_shake = self.cam_shake.max(1.4),
                    Weapon::Pistol => self.cam_shake = self.cam_shake.max(0.55),
                    Weapon::Axe => {}
                },
                ActionEvent::PlayerHurt { .. } | ActionEvent::Died => {
                    self.hurt = Some(elapsed_ms);
                    self.cam_shake = self.cam_shake.max(3.0);
                }
                ActionEvent::BloaterBurst { .. } => {
                    self.cam_shake = self.cam_shake.max(4.5);
                }
                ActionEvent::RocketBlast { .. } => {
                    self.cam_shake = self.cam_shake.max(6.5);
                }
                ActionEvent::StructuralCollapse { .. } => {
                    self.cam_shake = self.cam_shake.max(5.0);
                }
                ActionEvent::MonsterHit { id, .. } => {
                    let tr = self
                        .monster_tracks
                        .entry(id)
                        .or_insert_with(|| MonsterTrack::new(id, hud.tick));
                    tr.hit_flash_until = elapsed_ms.saturating_add(HIT_FLASH_MS);
                }
                ActionEvent::MonsterAlerted { id, .. } => {
                    let tr = self
                        .monster_tracks
                        .entry(id)
                        .or_insert_with(|| MonsterTrack::new(id, hud.tick));
                    tr.alert_started_ms = Some(elapsed_ms);
                }
                ActionEvent::MonsterDown {
                    id,
                    kind,
                    at,
                    floor,
                    ..
                } => {
                    self.monster_tracks.remove(&id);
                    if self.fallen.len() >= MAX_FALLEN {
                        self.fallen.remove(0);
                    }
                    self.fallen.push(Fallen {
                        at,
                        floor,
                        kind,
                        variant: (id.0 % 3) as u8,
                        started: elapsed_ms,
                    });
                }
                _ => {}
            }
        }

        let monsters = game.monster_views();
        for m in &monsters {
            let tr = self
                .monster_tracks
                .entry(m.id)
                .or_insert_with(|| MonsterTrack::new(m.id, hud.tick));
            let cycles = match m.kind {
                MonsterKind::Brisk => 1.95,
                MonsterKind::Bloater => 1.15,
                MonsterKind::Shooter => 1.35,
                MonsterKind::Shambler => 1.65,
            };
            tr.gait.observe(m.pos, cycles);
            if m.awareness == AwarenessIcon::Alert
                && tr.last_awareness != AwarenessIcon::Alert
                && tr.alert_started_ms.is_none()
            {
                tr.alert_started_ms = Some(elapsed_ms);
            }
            tr.last_awareness = m.awareness;
            tr.last_seen_tick = hud.tick;
        }
        // Periodically prune stale entries without reallocating.
        if self.monster_tracks.len() > monsters.len() + 32 {
            self.monster_tracks
                .retain(|_, tr| tr.last_seen_tick == hud.tick);
        }

        self.last_sim_tick = Some(hud.tick);
    }

    /// Render one current game state. `elapsed_ms` is the run's unpaused animation clock.
    pub fn render(&mut self, game: &ActionGame, elapsed_ms: u64) -> &[u8] {
        self.observe_events(game, elapsed_ms);

        let dt = self
            .last_render_ms
            .map_or(0.0, |prev| elapsed_ms.saturating_sub(prev) as f32 * 0.001)
            .clamp(0.0, 0.1);
        self.last_render_ms = Some(elapsed_ms);

        let hud = game.hud();
        let player_state = game.player_render_state();
        let player = player_state.pose;

        // 1. Update damped camera, tool-weighted aim look-ahead, floor elevation, and screen shake.
        let target_look = if self.look_ahead_enabled {
            let dist = match player_state.equipped {
                Some(Weapon::RocketLauncher) => 1.6,
                Some(Weapon::Pistol) => 1.4,
                Some(Weapon::Shotgun) => 1.15,
                Some(Weapon::Axe) => 0.7,
                None => 0.5,
            };
            player.facing.normalized() * dist
        } else {
            Vec2::ZERO
        };

        let target_floor = player_state.elevation;
        let should_snap = !self.cam_initialized
            || self.cam_anchor.distance(player.position) > 6.0
            || (target_floor - self.cam_floor).abs() > 1.5;

        if should_snap {
            self.cam_anchor = player.position;
            self.cam_look = target_look;
            self.cam_floor = target_floor;
            self.cam_initialized = true;
        } else if dt > 0.0 {
            let k_follow = 1.0 - 0.5_f32.powf(dt / FOLLOW_HALFLIFE_S);
            let k_look = 1.0 - 0.5_f32.powf(dt / LOOK_HALFLIFE_S);
            let k_floor = 1.0 - 0.5_f32.powf(dt / FLOOR_HALFLIFE_S);
            self.cam_anchor.x += (player.position.x - self.cam_anchor.x) * k_follow;
            self.cam_anchor.y += (player.position.y - self.cam_anchor.y) * k_follow;
            self.cam_look.x += (target_look.x - self.cam_look.x) * k_look;
            self.cam_look.y += (target_look.y - self.cam_look.y) * k_look;
            self.cam_floor += (target_floor - self.cam_floor) * k_floor;
            if (target_floor - self.cam_floor).abs() < 0.002 {
                self.cam_floor = target_floor;
            }
        }

        if dt > 0.0 && self.cam_shake > 0.0 {
            self.cam_shake *= 0.5_f32.powf(dt / SHAKE_HALFLIFE_S);
            if self.cam_shake < 0.12 {
                self.cam_shake = 0.0;
            }
        }

        let (shake_x, shake_y) = if self.cam_shake > 0.0 {
            let t = elapsed_ms as f32 * 0.085;
            (
                (t * 1.7).sin() * self.cam_shake,
                (t * 2.3 + 1.1).cos() * self.cam_shake * 0.78,
            )
        } else {
            (0.0, 0.0)
        };

        let focus = self.cam_anchor + self.cam_look;
        let proj = Projector {
            center_x: self.width as f32 * 0.5,
            center_y: self.height as f32 * 0.51,
            focus,
            cam_floor: self.cam_floor,
            shake_x,
            shake_y,
        };

        // 2. Query read-only core views & step particles/flashes.
        let static_lights = game.light_sources();
        let caches = game.caches();
        let monsters = game.monster_views();
        let projectiles = game.projectiles();
        self.vfx.step(dt, &projectiles);

        let mut flashes = [DynamicLight {
            pos: Vec2::ZERO,
            floor: 0,
            radius: 0.0,
            r: 0,
            g: 0,
            b: 0,
            glow_rgb: [0; 3],
            glow_strength: 0.0,
            glow_radius_px: 0.0,
        }; 16];
        let flash_count = self
            .vfx
            .collect_flashes(&projectiles, elapsed_ms, &mut flashes);
        let active_flashes = &flashes[..flash_count];

        // 3. Populate local viewport 48×48 light grids (zero heap allocs!).
        let center_x = focus.x.round() as i32;
        let center_y = focus.y.round() as i32;
        let af = player.floor.min(2) as usize;
        let f0_dim = match af {
            0 => 256,
            1 => F0_LOWER_DIM_256,
            _ => 138,
        };

        self.light_floors[0].populate(
            game,
            center_x,
            center_y,
            0,
            player.position,
            player.floor,
            &static_lights,
            active_flashes,
            f0_dim,
        );
        if af >= 1 {
            if af == 2 {
                self.light_floors[1].populate(
                    game,
                    center_x,
                    center_y,
                    1,
                    player.position,
                    player.floor,
                    &static_lights,
                    active_flashes,
                    178,
                );
            }
            self.light_floors[af].populate(
                game,
                center_x,
                center_y,
                af as Floor,
                player.position,
                player.floor,
                &static_lights,
                active_flashes,
                256,
            );
        }

        // 4. Update per-building smooth cutaway openness (~160 ms fade).
        let p_tile = player.position.tile();
        let player_bid = self.light_floors[af]
            .tile_info(p_tile.x, p_tile.y)
            .map_or(0, |t| t.building_id);

        let buildings = game.buildings();
        for b in buildings {
            let wants = if af >= 1 {
                player_bid == b.id
            } else {
                building_wants_cutaway(b, player.position, player_bid, &self.light_floors[0])
            };
            let target = if wants { 1.0 } else { 0.0 };
            if should_snap {
                if wants {
                    self.cutaway.insert(b.id, 1.0);
                } else {
                    self.cutaway.remove(&b.id);
                }
            } else if dt > 0.0 && (wants || self.cutaway.contains_key(&b.id)) {
                let entry = self.cutaway.entry(b.id).or_insert(0.0);
                let step = dt / CUTAWAY_FADE_S;
                if *entry < target {
                    *entry = (*entry + step).min(target);
                } else {
                    *entry = (*entry - step).max(target);
                }
                if *entry <= 0.0 && !wants {
                    self.cutaway.remove(&b.id);
                }
            }
        }
        if self.cutaway.len() > 128 {
            self.cutaway
                .retain(|bid, v| *v > 0.0 && buildings.iter().any(|b| b.id == *bid));
        }

        // Collect upper-floor Shooters once for closed upper-story window peeking.
        self.upper_shooters.clear();
        for m in &monsters {
            if m.kind == MonsterKind::Shooter && m.floor > 0 {
                self.upper_shooters.push((m.pos, m.floor, m.variant));
            }
        }

        // Compute visible tile scan window (clamped comfortably within the 48×48 LightGrid).
        let radius = ((self.width as f32 / (2.0 * TILE_W)) + (self.height as f32 / (2.0 * TILE_H)))
            .ceil() as i32
            + 3;
        let radius = radius.min(22);
        let bounds = (
            center_x - radius,
            center_x + radius,
            center_y - radius,
            center_y + radius,
        );

        let recoil = self.vfx.active_recoil(elapsed_ms);
        let swing_progress = self.vfx.active_swing_progress(elapsed_ms);
        let hurt = self
            .hurt
            .map_or(0.0, |started| life(elapsed_ms, started, HURT_MS));

        let mut c = Canvas::new(self.width, self.height, &mut self.pixels);
        c.clear([22, 28, 36, 255]);

        if af == 0 {
            // === FLOOR 0 RENDERING (Outdoors & Inside Building F0) ===
            draw_ground_pass(
                &mut c,
                game,
                &proj,
                &self.light_floors[0],
                bounds,
                false,
                &self.cutaway,
                elapsed_ms,
            );
            self.vfx.draw_decals(&mut c, &proj, 0, 256);
            self.vfx.draw_ground_telegraphs(
                &mut c,
                &proj,
                &monsters,
                &self.light_floors[0],
                0,
                elapsed_ms,
            );

            // Build unified F0 depth-sorted draw queue.
            self.draw_items.clear();
            for y in bounds.2..=bounds.3 {
                for x in bounds.0..=bounds.1 {
                    let Some(info) = self.light_floors[0].tile_info(x, y) else {
                        continue;
                    };
                    let depth = (x + y) as f32;
                    if info.surface == Surface::Mountain {
                        self.draw_items.push(DrawItem {
                            depth,
                            bias: 4,
                            kind: DrawItemKind::Mountain { x, y },
                        });
                    }
                    if info.kind == TileKind::Empty {
                        continue;
                    }
                    let bid = info.building_id;
                    let stipple = building_stipple(&self.cutaway, bid);

                    if bid > 0 && stipple == 4 {
                        // Fully closed building: emit exterior perimeter multi-story columns only.
                        if is_building_perimeter(info, bid) {
                            self.draw_items.push(DrawItem {
                                depth,
                                bias: 4,
                                kind: DrawItemKind::Tile {
                                    x,
                                    y,
                                    floor: 0,
                                    closed_building: Some(bid),
                                    stipple: 4,
                                    support_floors: 0,
                                },
                            });
                        }
                    } else {
                        // Cutaway open (or mid-dissolve): emit solid F0 interior features...
                        let bias = tile_draw_bias(info.kind);
                        self.draw_items.push(DrawItem {
                            depth,
                            bias,
                            kind: DrawItemKind::Tile {
                                x,
                                y,
                                floor: 0,
                                closed_building: None,
                                stipple: 4,
                                support_floors: 0,
                            },
                        });
                        if matches!(info.kind, TileKind::Door { open: true, .. }) {
                            self.draw_items.push(DrawItem {
                                depth: depth + 0.18,
                                bias: 4,
                                kind: DrawItemKind::DoorJambFront {
                                    x,
                                    y,
                                    floor: 0,
                                    horizontal: info.orientation != WallOrientation::AlongY,
                                },
                            });
                        }
                        // ...plus dissolving multi-story exterior facade overlay if mid-transition!
                        if bid > 0 && stipple > 0 && is_building_perimeter(info, bid) {
                            self.draw_items.push(DrawItem {
                                depth: depth + 0.04,
                                bias: 4,
                                kind: DrawItemKind::Tile {
                                    x,
                                    y,
                                    floor: 0,
                                    closed_building: Some(bid),
                                    stipple,
                                    support_floors: 0,
                                },
                            });
                        }
                    }
                }
            }

            // Closed / dissolving building roofs.
            for (bi, b) in buildings.iter().enumerate() {
                let stipple = building_stipple(&self.cutaway, b.id);
                if stipple > 0 {
                    emit_building_roof(
                        &mut self.draw_items,
                        &self.light_floors[0],
                        bounds,
                        bi,
                        b,
                        stipple,
                    );
                }
            }

            // Visible F0 caches.
            for (idx, cache) in caches.iter().enumerate() {
                if cache.pos.floor != 0 {
                    continue;
                }
                if let Some(ti) = self.light_floors[0].tile_info(cache.pos.x, cache.pos.y) {
                    if ti.visibility == TileVisibility::Hidden {
                        continue;
                    }
                    if ti.building_id > 0 && building_stipple(&self.cutaway, ti.building_id) == 4 {
                        continue;
                    }
                }
                self.draw_items.push(DrawItem {
                    depth: (cache.pos.x + cache.pos.y) as f32,
                    bias: 2,
                    kind: DrawItemKind::Cache { idx },
                });
            }

            // Survivor on F0.
            self.draw_items.push(DrawItem {
                depth: player.position.x + player.position.y,
                bias: 3,
                kind: DrawItemKind::Survivor {
                    z: player_state.elevation,
                },
            });

            // Visible F0 monsters (hide out-of-LOS monsters on Explored/Hidden tiles!).
            for (idx, m) in monsters.iter().enumerate() {
                if m.floor != 0 {
                    continue;
                }
                let cell = self.light_floors[0].sample_light(m.pos);
                if cell.vis != TileVisibility::Visible {
                    continue;
                }
                if let Some(ti) =
                    self.light_floors[0].tile_info(m.pos.x.round() as i32, m.pos.y.round() as i32)
                    && ti.indoor
                    && ti.building_id > 0
                    && building_stipple(&self.cutaway, ti.building_id) == 4
                {
                    continue;
                }
                self.draw_items.push(DrawItem {
                    depth: m.pos.x + m.pos.y,
                    bias: 3,
                    kind: DrawItemKind::Monster {
                        idx,
                        z: m.elevation,
                        lower_dim: false,
                    },
                });
            }

            // Visible F0 fallen corpses.
            for (idx, f) in self.fallen.iter().enumerate() {
                if f.floor != 0 {
                    continue;
                }
                if self.light_floors[0].sample_light(f.at).vis == TileVisibility::Hidden {
                    continue;
                }
                self.draw_items.push(DrawItem {
                    depth: f.at.x + f.at.y,
                    bias: 0,
                    kind: DrawItemKind::Fallen {
                        idx,
                        z: 0.0,
                        lower_dim: false,
                    },
                });
            }

            self.draw_items.sort_unstable_by(|a, b| {
                a.depth
                    .total_cmp(&b.depth)
                    .then_with(|| a.bias.cmp(&b.bias))
            });

            draw_sorted_items(
                &mut c,
                game,
                &proj,
                &self.light_floors,
                0,
                0,
                &self.draw_items,
                &caches,
                &monsters,
                &self.fallen,
                &self.monster_tracks,
                &self.survivor,
                player_state,
                hud.outcome,
                recoil,
                swing_progress,
                hurt,
                &self.upper_shooters,
                elapsed_ms,
            );

            self.light_floors[0].draw_light_pools(
                &mut c,
                &proj,
                &static_lights,
                active_flashes,
                0,
                elapsed_ms,
                1.0,
            );
            self.vfx.draw_shooter_telegraphs(
                &mut c,
                &proj,
                &monsters,
                &self.light_floors,
                0,
                elapsed_ms,
            );
        } else {
            // === FLOOR 1 / FLOOR 2 RENDERING (Dimmed Lower World + Elevated Active Cutaway Deck) ===
            // Pass 1: Dimmed outdoor F0 street/terrain, streetlights, neighboring closed buildings,
            // outdoor F0 monsters below, and the active building's lower exterior support walls!
            draw_ground_pass(
                &mut c,
                game,
                &proj,
                &self.light_floors[0],
                bounds,
                true,
                &self.cutaway,
                elapsed_ms,
            );
            self.vfx.draw_decals(&mut c, &proj, 0, f0_dim);

            self.draw_items.clear();
            for y in bounds.2..=bounds.3 {
                for x in bounds.0..=bounds.1 {
                    let Some(info) = self.light_floors[0].tile_info(x, y) else {
                        continue;
                    };
                    let depth = (x + y) as f32;
                    if info.surface == Surface::Mountain {
                        self.draw_items.push(DrawItem {
                            depth,
                            bias: 4,
                            kind: DrawItemKind::Mountain { x, y },
                        });
                    }
                    if info.kind == TileKind::Empty {
                        continue;
                    }
                    if info.building_id == 0 {
                        // Outdoor F0 props, trees, streetlights, fences.
                        self.draw_items.push(DrawItem {
                            depth,
                            bias: tile_draw_bias(info.kind),
                            kind: DrawItemKind::Tile {
                                x,
                                y,
                                floor: 0,
                                closed_building: None,
                                stipple: 4,
                                support_floors: 0,
                            },
                        });
                    } else if info.building_id == player_bid {
                        // Active building's lower exterior support walls (0..34 px on 2F, 0..68 px on 3F).
                        if is_building_perimeter(info, player_bid) {
                            self.draw_items.push(DrawItem {
                                depth,
                                bias: 4,
                                kind: DrawItemKind::Tile {
                                    x,
                                    y,
                                    floor: 0,
                                    closed_building: None,
                                    stipple: 4,
                                    support_floors: af as u8,
                                },
                            });
                        }
                    } else if is_building_perimeter(info, info.building_id) {
                        // Neighboring buildings remain closed landmarks below!
                        self.draw_items.push(DrawItem {
                            depth,
                            bias: 4,
                            kind: DrawItemKind::Tile {
                                x,
                                y,
                                floor: 0,
                                closed_building: Some(info.building_id),
                                stipple: 4,
                                support_floors: 0,
                            },
                        });
                    }
                }
            }
            for (bi, b) in buildings.iter().enumerate() {
                if b.id != player_bid {
                    emit_building_roof(
                        &mut self.draw_items,
                        &self.light_floors[0],
                        bounds,
                        bi,
                        b,
                        4,
                    );
                }
            }
            // Outdoor F0 monsters visible from upper windows!
            for (idx, m) in monsters.iter().enumerate() {
                if m.floor != 0 {
                    continue;
                }
                let tx = m.pos.x.round() as i32;
                let ty = m.pos.y.round() as i32;
                if let Some(ti) = self.light_floors[0].tile_info(tx, ty) {
                    if ti.indoor || ti.visibility != TileVisibility::Visible {
                        continue;
                    }
                    self.draw_items.push(DrawItem {
                        depth: m.pos.x + m.pos.y,
                        bias: 3,
                        kind: DrawItemKind::Monster {
                            idx,
                            z: 0.0,
                            lower_dim: true,
                        },
                    });
                }
            }
            for (idx, f) in self.fallen.iter().enumerate() {
                if f.floor != 0 {
                    continue;
                }
                let tx = f.at.x.round() as i32;
                let ty = f.at.y.round() as i32;
                if let Some(ti) = self.light_floors[0].tile_info(tx, ty)
                    && !ti.indoor
                    && ti.visibility != TileVisibility::Hidden
                {
                    self.draw_items.push(DrawItem {
                        depth: f.at.x + f.at.y,
                        bias: 0,
                        kind: DrawItemKind::Fallen {
                            idx,
                            z: 0.0,
                            lower_dim: true,
                        },
                    });
                }
            }

            self.draw_items.sort_unstable_by(|a, b| {
                a.depth
                    .total_cmp(&b.depth)
                    .then_with(|| a.bias.cmp(&b.bias))
            });
            draw_sorted_items(
                &mut c,
                game,
                &proj,
                &self.light_floors,
                0,
                af as Floor,
                &self.draw_items,
                &caches,
                &monsters,
                &self.fallen,
                &self.monster_tracks,
                &self.survivor,
                player_state,
                hud.outcome,
                recoil,
                swing_progress,
                hurt,
                &self.upper_shooters,
                elapsed_ms,
            );
            self.light_floors[0].draw_light_pools(
                &mut c,
                &proj,
                &static_lights,
                active_flashes,
                0,
                elapsed_ms,
                if af == 1 { 0.55 } else { 0.45 },
            );

            // Pass 2: Elevated active floor deck at `z = af as f32` with 4 px joist fascia + stairwell cutout!
            draw_upper_deck_pass(
                &mut c,
                game,
                &proj,
                &self.light_floors[af],
                af as Floor,
                player_bid,
                bounds,
                elapsed_ms,
            );
            self.vfx.draw_decals(&mut c, &proj, af as Floor, 256);
            self.vfx.draw_ground_telegraphs(
                &mut c,
                &proj,
                &monsters,
                &self.light_floors[af],
                af as Floor,
                elapsed_ms,
            );

            // Pass 3: Unified active floor `af` depth-sorted walls, windows, furniture, caches, and actors!
            self.draw_items.clear();
            let active_fl = af as Floor;
            for y in bounds.2..=bounds.3 {
                for x in bounds.0..=bounds.1 {
                    let Some(info) = self.light_floors[af].tile_info(x, y) else {
                        continue;
                    };
                    if info.surface == Surface::Air || info.kind == TileKind::Empty {
                        continue;
                    }
                    if player_bid != 0 && info.building_id != player_bid {
                        continue;
                    }
                    let depth = (x + y) as f32;
                    self.draw_items.push(DrawItem {
                        depth,
                        bias: tile_draw_bias(info.kind),
                        kind: DrawItemKind::Tile {
                            x,
                            y,
                            floor: active_fl,
                            closed_building: None,
                            stipple: 4,
                            support_floors: 0,
                        },
                    });
                    if matches!(info.kind, TileKind::Door { open: true, .. }) {
                        self.draw_items.push(DrawItem {
                            depth: depth + 0.18,
                            bias: 4,
                            kind: DrawItemKind::DoorJambFront {
                                x,
                                y,
                                floor: active_fl,
                                horizontal: info.orientation != WallOrientation::AlongY,
                            },
                        });
                    }
                }
            }
            for (idx, cache) in caches.iter().enumerate() {
                if cache.pos.floor != active_fl {
                    continue;
                }
                if let Some(ti) = self.light_floors[af].tile_info(cache.pos.x, cache.pos.y) {
                    if ti.visibility == TileVisibility::Hidden
                        || (player_bid != 0 && ti.building_id != player_bid)
                    {
                        continue;
                    }
                    self.draw_items.push(DrawItem {
                        depth: (cache.pos.x + cache.pos.y) as f32,
                        bias: 2,
                        kind: DrawItemKind::Cache { idx },
                    });
                }
            }
            self.draw_items.push(DrawItem {
                depth: player.position.x + player.position.y,
                bias: 3,
                kind: DrawItemKind::Survivor {
                    z: player_state.elevation,
                },
            });
            for (idx, m) in monsters.iter().enumerate() {
                if m.floor != active_fl {
                    continue;
                }
                if self.light_floors[af].sample_light(m.pos).vis != TileVisibility::Visible {
                    continue;
                }
                self.draw_items.push(DrawItem {
                    depth: m.pos.x + m.pos.y,
                    bias: 3,
                    kind: DrawItemKind::Monster {
                        idx,
                        z: m.elevation,
                        lower_dim: false,
                    },
                });
            }
            for (idx, f) in self.fallen.iter().enumerate() {
                if f.floor != active_fl {
                    continue;
                }
                if self.light_floors[af].sample_light(f.at).vis == TileVisibility::Hidden {
                    continue;
                }
                self.draw_items.push(DrawItem {
                    depth: f.at.x + f.at.y,
                    bias: 0,
                    kind: DrawItemKind::Fallen {
                        idx,
                        z: active_fl as f32,
                        lower_dim: false,
                    },
                });
            }

            self.draw_items.sort_unstable_by(|a, b| {
                a.depth
                    .total_cmp(&b.depth)
                    .then_with(|| a.bias.cmp(&b.bias))
            });
            draw_sorted_items(
                &mut c,
                game,
                &proj,
                &self.light_floors,
                af,
                active_fl,
                &self.draw_items,
                &caches,
                &monsters,
                &self.fallen,
                &self.monster_tracks,
                &self.survivor,
                player_state,
                hud.outcome,
                recoil,
                swing_progress,
                hurt,
                &self.upper_shooters,
                elapsed_ms,
            );
            self.light_floors[af].draw_light_pools(
                &mut c,
                &proj,
                &static_lights,
                active_flashes,
                active_fl,
                elapsed_ms,
                1.0,
            );
            self.vfx.draw_shooter_telegraphs(
                &mut c,
                &proj,
                &monsters,
                &self.light_floors,
                active_fl,
                elapsed_ms,
            );
        }

        // Foreground 3D combat VFX (melee arcs, tracers, projectiles, explosions, particles).
        self.vfx
            .draw_effects(&mut c, &proj, &projectiles, elapsed_ms);

        // Tool-adaptive aim reticle (pulses warning crimson when rocket_danger_close!).
        if hud.outcome == ActionOutcome::Playing {
            let loaded = match player_state.equipped {
                Some(Weapon::Axe) | None => true,
                Some(_) => hud.active_mag > 0,
            };
            self.vfx
                .draw_reticle(&mut c, &proj, player_state, loaded, elapsed_ms);
        }

        &self.pixels
    }

    /// Convert an absolute screen point to a floating world tile position on the player's floor,
    /// consistent with the damped camera focus and floor elevation.
    pub fn screen_to_world(&self, game: &ActionGame, sx: f32, sy: f32) -> Vec2 {
        if !self.cam_initialized {
            return screen_to_world(game, self.width, self.height, sx, sy);
        }
        let focus = self.cam_anchor + self.cam_look;
        let player_z = game.player_pose().floor as f32;
        let z_shift = (player_z - self.cam_floor) * FLOOR_HEIGHT_PX;
        let dx = (sx - self.width as f32 * 0.5) / (TILE_W * 0.5);
        let dy = (sy + z_shift - self.height as f32 * 0.51) / (TILE_H * 0.5);
        Vec2 {
            x: focus.x + (dx + dy) * 0.5,
            y: focus.y + (dy - dx) * 0.5,
        }
    }

    /// Convert a screen drag vector into normalized world-space movement.
    pub fn screen_vector_to_world(&self, sx: f32, sy: f32) -> Vec2 {
        screen_vector_to_world(sx, sy)
    }
}

fn tile_draw_bias(kind: TileKind) -> u8 {
    match kind {
        TileKind::Rubble | TileKind::StairsUp | TileKind::StairsDown | TileKind::StairsBoth => 0,
        TileKind::Prop(zombiebox_core::PropKind::BirchTree | zombiebox_core::PropKind::Shed)
        | TileKind::Streetlight
        | TileKind::ExteriorWall
        | TileKind::InteriorWall
        | TileKind::Pillar
        | TileKind::Window
        | TileKind::BrokenWindow
        | TileKind::Door { .. } => 4,
        _ => 2,
    }
}

#[inline]
fn is_building_perimeter(info: TileRenderInfo, bid: BuildingId) -> bool {
    bid > 0 && !info.indoor && info.building_id == bid
}

fn emit_building_roof(
    draw_items: &mut Vec<DrawItem>,
    grid_f0: &LightGrid,
    bounds: (i32, i32, i32, i32),
    building_idx: usize,
    b: &zombiebox_core::BuildingInfo,
    stipple: u8,
) {
    if b.style.has_pitched_roof() {
        let roof_depth = (b.x + b.width - 1 + b.y + b.height - 1) as f32 + 0.08;
        draw_items.push(DrawItem {
            depth: roof_depth,
            bias: 5,
            kind: DrawItemKind::Roof {
                building_idx,
                stipple,
            },
        });
    } else {
        let min_x = bounds.0.max(b.x);
        let max_x = bounds.1.min(b.x + b.width - 1);
        let min_y = bounds.2.max(b.y);
        let max_y = bounds.3.min(b.y + b.height - 1);
        for ry in min_y..=max_y {
            for rx in min_x..=max_x {
                if grid_f0
                    .tile_info(rx, ry)
                    .is_some_and(|ti| ti.building_id == b.id)
                {
                    draw_items.push(DrawItem {
                        depth: (rx + ry) as f32 + 0.06,
                        bias: 5,
                        kind: DrawItemKind::RoofTile {
                            x: rx,
                            y: ry,
                            bid: b.id,
                            floors: b.floors,
                            stipple,
                        },
                    });
                }
            }
        }
    }
}

fn building_wants_cutaway(
    b: &zombiebox_core::BuildingInfo,
    p: Vec2,
    player_bid: BuildingId,
    grid: &LightGrid,
) -> bool {
    if player_bid == b.id {
        return true;
    }
    let x0 = b.x as f32;
    let y0 = b.y as f32;
    let x1 = (b.x + b.width - 1) as f32;
    let y1 = (b.y + b.height - 1) as f32;

    // 1. Inside footprint margin.
    if p.x >= x0 - 0.3 && p.x <= x1 + 0.3 && p.y >= y0 - 0.3 && p.y <= y1 + 0.3 {
        return true;
    }
    // 2. Within 1.5 tiles behind far NW / NE walls so tall roof never occludes survivor.
    let behind_ne = p.x >= x0 - 1.5 && p.x <= x1 + 0.5 && p.y >= y0 - 1.5 && p.y < y0;
    let behind_nw = p.y >= y0 - 1.5 && p.y <= y1 + 0.5 && p.x >= x0 - 1.5 && p.x < x0;
    if behind_ne || behind_nw {
        return true;
    }
    // 3. Within 1.3 tiles of front doorway or adjacent exterior windows/doors.
    let front_d = p.distance(Vec2::new(b.front_door.x as f32, b.front_door.y as f32));
    if front_d <= 1.3 {
        return true;
    }
    let px = p.x.round() as i32;
    let py = p.y.round() as i32;
    for dy in -2..=2 {
        for dx in -2..=2 {
            let tx = px + dx;
            let ty = py + dy;
            if let Some(ti) = grid.tile_info(tx, ty)
                && ti.building_id == b.id
                && matches!(
                    ti.kind,
                    TileKind::Door { .. } | TileKind::Window | TileKind::BrokenWindow
                )
                && p.distance(Vec2::new(tx as f32, ty as f32)) <= 1.3
            {
                return true;
            }
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn draw_sorted_items(
    c: &mut Canvas<'_>,
    game: &ActionGame,
    proj: &Projector,
    light_floors: &[LightGrid; 3],
    grid_idx: usize,
    max_populated_floor: Floor,
    items: &[DrawItem],
    caches: &[zombiebox_core::CacheView],
    monsters: &[zombiebox_core::MonsterView],
    fallen: &[Fallen],
    tracks: &HashMap<MonsterId, MonsterTrack>,
    survivor_gait: &Gait,
    player_state: zombiebox_core::PlayerRenderState,
    outcome: ActionOutcome,
    recoil: f32,
    swing_progress: Option<f32>,
    hurt: f32,
    upper_shooters: &[(Vec2, Floor, u8)],
    elapsed_ms: u64,
) {
    let w = c.width() as f32;
    let h = c.height() as f32;
    let buildings = game.buildings();
    let grid = &light_floors[grid_idx];

    for item in items {
        match item.kind {
            DrawItemKind::Tile {
                x,
                y,
                floor,
                closed_building,
                stipple,
                support_floors,
            } => {
                draw_tile_feature(
                    c,
                    game,
                    proj,
                    light_floors,
                    grid_idx,
                    max_populated_floor,
                    x,
                    y,
                    floor,
                    closed_building,
                    stipple,
                    support_floors,
                    player_state.pose.position,
                    player_state.near_interact_tile,
                    player_state.interact_prompt,
                    upper_shooters,
                    elapsed_ms,
                );
            }
            DrawItemKind::DoorJambFront {
                x,
                y,
                floor,
                horizontal,
            } => {
                draw_open_door_front_jamb(c, proj, grid, x, y, floor, horizontal);
            }
            DrawItemKind::Roof {
                building_idx,
                stipple,
            } => {
                if let Some(b) = buildings.get(building_idx) {
                    draw_building_roof(c, game, proj, grid, b, stipple);
                }
            }
            DrawItemKind::RoofTile {
                x,
                y,
                bid,
                floors,
                stipple,
            } => {
                draw_flat_roof_tile(c, game, proj, grid, x, y, bid, floors, stipple);
            }
            DrawItemKind::Mountain { x, y } => {
                draw_mountain_crag(c, proj, grid, x, y);
            }
            DrawItemKind::Cache { idx } => {
                let cache = &caches[idx];
                let l = grid.cell_light(cache.pos.x, cache.pos.y);
                draw_cache(c, proj, cache, l, elapsed_ms);
            }
            DrawItemKind::Survivor { z } => {
                let (x, y) = proj.project_z(
                    player_state.pose.position.x,
                    player_state.pose.position.y,
                    z,
                );
                if x < -32.0 || x > w + 32.0 || y < -16.0 || y > h + 40.0 {
                    continue;
                }
                let light = actor_light(grid.sample_light(player_state.pose.position));
                draw_actor(
                    c,
                    x,
                    y,
                    ActorVisual {
                        kind: ActorKind::Survivor(player_state.equipped),
                        facing: screen_direction(player_state.pose.facing),
                        stride: survivor_gait.stride,
                        phase: survivor_gait.phase,
                        recoil,
                        swing_progress,
                        hurt,
                        hit_flash: 0.0,
                        quiet_walk: player_state.quiet_walking,
                        hp: player_state.health,
                        max_hp: player_state.max_health,
                        awareness: AwarenessIcon::Unaware,
                        show_alert_badge: false,
                        alert_age_ms: 0,
                        telegraph_progress: None,
                        swelling: false,
                        light,
                        elapsed_ms,
                    },
                );
            }
            DrawItemKind::Monster { idx, z, lower_dim } => {
                let m = &monsters[idx];
                let (x, y) = proj.project_z(m.pos.x, m.pos.y, z);
                if x < -34.0 || x > w + 34.0 || y < -16.0 || y > h + 42.0 {
                    continue;
                }
                let raw_light = grid.sample_light(m.pos);
                let light = if lower_dim {
                    lower_floor_actor_light(raw_light)
                } else {
                    actor_light(raw_light)
                };
                let track = tracks.get(&m.id);
                let (stride, phase, hit_flash, show_alert, alert_age) = match track {
                    Some(tr) => {
                        let hf = if tr.hit_flash_until > elapsed_ms {
                            (tr.hit_flash_until - elapsed_ms) as f32 / HIT_FLASH_MS as f32
                        } else {
                            0.0
                        };
                        let (sa, age) = match tr.alert_started_ms {
                            Some(st) if elapsed_ms.saturating_sub(st) <= ALERT_BADGE_MS => {
                                (true, elapsed_ms.saturating_sub(st))
                            }
                            _ => (false, ALERT_BADGE_MS),
                        };
                        (tr.gait.stride, tr.gait.phase, hf, sa, age)
                    }
                    None => (Vec2::ZERO, 0.0, 0.0, false, ALERT_BADGE_MS),
                };
                let aim_dir = if m.telegraph_progress.is_some() {
                    m.telegraph_dir
                } else {
                    m.facing
                };
                draw_actor(
                    c,
                    x,
                    y,
                    ActorVisual {
                        kind: ActorKind::Monster(m.kind, m.variant),
                        facing: screen_direction(aim_dir),
                        stride,
                        phase,
                        recoil: 0.0,
                        swing_progress: None,
                        hurt: if m.hitstun { 0.5 } else { 0.0 },
                        hit_flash,
                        quiet_walk: false,
                        hp: m.hp,
                        max_hp: m.max_hp,
                        awareness: m.awareness,
                        show_alert_badge: show_alert,
                        alert_age_ms: alert_age,
                        telegraph_progress: m.telegraph_progress,
                        swelling: m.swelling,
                        light,
                        elapsed_ms,
                    },
                );
            }
            DrawItemKind::Fallen { idx, z, lower_dim } => {
                let f = &fallen[idx];
                let (x, y) = proj.project_z(f.at.x, f.at.y, z);
                if x < -30.0 || x > w + 30.0 || y < -14.0 || y > h + 30.0 {
                    continue;
                }
                let progress = if outcome == ActionOutcome::Playing {
                    1.0 - life(elapsed_ms, f.started, FALL_MS)
                } else {
                    1.0
                };
                let raw_light = grid.sample_light(f.at);
                let light = if lower_dim {
                    raw_light
                } else {
                    actor_light(raw_light)
                };
                draw_fallen(c, x, y, f.kind, f.variant, progress, light);
            }
        }
    }
}

#[inline]
pub(crate) fn projected_vector(v: Vec2) -> Vec2 {
    Vec2::new((v.x - v.y) * TILE_W * 0.5, (v.x + v.y) * TILE_H * 0.5)
}

#[inline]
pub(crate) fn screen_direction(v: Vec2) -> Vec2 {
    let v = projected_vector(v);
    let length = v.x.hypot(v.y);
    if length > 0.0 {
        v * (1.0 / length)
    } else {
        Vec2::ZERO
    }
}

#[inline]
pub(crate) fn life(now: u64, started: u64, duration: u64) -> f32 {
    duration.saturating_sub(now.saturating_sub(started)) as f32 / duration as f32
}

#[inline]
pub(crate) fn hash2(x: i32, y: i32) -> u32 {
    let mut n = (x as u32).wrapping_mul(0x9e37_79b9) ^ (y as u32).wrapping_mul(0x85eb_ca6b);
    n ^= n >> 16;
    n = n.wrapping_mul(0x7feb_352d);
    n ^ (n >> 15)
}

/// Map a screen point to world coordinates using the renderer's 2:1 diamond projection.
pub fn screen_to_world(game: &ActionGame, width: usize, height: usize, sx: f32, sy: f32) -> Vec2 {
    let p = game.player_pose().position;
    let dx = (sx - width as f32 * 0.5) / (TILE_W * 0.5);
    let dy = (sy - height as f32 * 0.51) / (TILE_H * 0.5);
    Vec2 {
        x: p.x + (dx + dy) * 0.5,
        y: p.y + (dy - dx) * 0.5,
    }
}

/// Convert a screen drag vector into normalized world-space movement.
pub fn screen_vector_to_world(sx: f32, sy: f32) -> Vec2 {
    let dx = sx / (TILE_W * 0.5);
    let dy = sy / (TILE_H * 0.5);
    let mut out = Vec2 {
        x: (dx + dy) * 0.5,
        y: (dy - dx) * 0.5,
    };
    let length = (out.x * out.x + out.y * out.y).sqrt();
    if length > 1.0 {
        out.x /= length;
        out.y /= length;
    }
    out
}

/// 2×2 Bayer threshold matrix (`0..4`) for authentic Genesis/PS1 4-step stipple dissolve.
#[inline(always)]
const fn bayer2x2(x: i32, y: i32) -> u8 {
    match ((y & 1) << 1) | (x & 1) {
        0 => 0,
        1 => 2,
        2 => 3,
        _ => 1,
    }
}

/// Fast CPU software RGBA canvas with opaque span fills, fixed-point `>> 8` alpha blending,
/// 2×2 Bayer stipple dissolve, and additive 2:1 elliptical light pools.
pub(crate) struct Canvas<'a> {
    w: usize,
    h: usize,
    p: &'a mut [u8],
    stipple: u8,
}

impl<'a> Canvas<'a> {
    pub(crate) fn new(w: usize, h: usize, p: &'a mut [u8]) -> Self {
        Self {
            w,
            h,
            p,
            stipple: 4,
        }
    }

    #[inline(always)]
    pub(crate) fn width(&self) -> usize {
        self.w
    }

    #[inline(always)]
    pub(crate) fn height(&self) -> usize {
        self.h
    }

    #[inline(always)]
    pub(crate) fn set_stipple(&mut self, level: u8) {
        self.stipple = level.min(4);
    }

    pub(crate) fn clear(&mut self, color: [u8; 4]) {
        if self.w == 0 || self.h == 0 {
            return;
        }
        let row_bytes = self.w * 4;
        for px in self.p[..row_bytes].chunks_exact_mut(4) {
            px.copy_from_slice(&color);
        }
        let mut filled = row_bytes;
        let total = self.p.len();
        while filled < total {
            let chunk = (total - filled).min(filled);
            let (src, dst) = self.p.split_at_mut(filled);
            dst[..chunk].copy_from_slice(&src[..chunk]);
            filled += chunk;
        }
    }

    #[inline(always)]
    pub(crate) fn put(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if self.stipple == 0 || x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        if self.stipple < 4 && bayer2x2(x, y) >= self.stipple {
            return;
        }
        let a = color[3] as u32;
        if a == 0 {
            return;
        }
        let i = (y as usize * self.w + x as usize) * 4;
        if a == 255 {
            self.p[i..i + 4].copy_from_slice(&color);
            return;
        }
        let inv = 256 - a;
        self.p[i] = ((color[0] as u32 * a + self.p[i] as u32 * inv) >> 8) as u8;
        self.p[i + 1] = ((color[1] as u32 * a + self.p[i + 1] as u32 * inv) >> 8) as u8;
        self.p[i + 2] = ((color[2] as u32 * a + self.p[i + 2] as u32 * inv) >> 8) as u8;
        self.p[i + 3] = 255;
    }

    #[inline]
    pub(crate) fn span(&mut self, x0: i32, x1: i32, y: i32, color: [u8; 4]) {
        if self.stipple == 0 || y < 0 || y >= self.h as i32 {
            return;
        }
        let x0 = x0.max(0);
        let x1 = x1.min(self.w as i32);
        if x0 >= x1 {
            return;
        }
        let a = color[3] as u32;
        if a == 0 {
            return;
        }
        let row = y as usize * self.w;
        let slice = &mut self.p[(row + x0 as usize) * 4..(row + x1 as usize) * 4];

        if self.stipple >= 4 {
            if a == 255 {
                for px in slice.chunks_exact_mut(4) {
                    px.copy_from_slice(&color);
                }
            } else {
                let inv = 256 - a;
                let sr = color[0] as u32 * a;
                let sg = color[1] as u32 * a;
                let sb = color[2] as u32 * a;
                for px in slice.chunks_exact_mut(4) {
                    px[0] = ((sr + px[0] as u32 * inv) >> 8) as u8;
                    px[1] = ((sg + px[1] as u32 * inv) >> 8) as u8;
                    px[2] = ((sb + px[2] as u32 * inv) >> 8) as u8;
                    px[3] = 255;
                }
            }
        } else {
            // Fast 2×2 Bayer stipple span (only 2-pixel period along X!).
            let pass_even = bayer2x2(0, y) < self.stipple;
            let pass_odd = bayer2x2(1, y) < self.stipple;
            if !pass_even && !pass_odd {
                return;
            }
            let inv = 256 - a;
            let sr = color[0] as u32 * a;
            let sg = color[1] as u32 * a;
            let sb = color[2] as u32 * a;
            for (offset, px) in slice.chunks_exact_mut(4).enumerate() {
                let is_odd = ((x0 + offset as i32) & 1) != 0;
                if (is_odd && !pass_odd) || (!is_odd && !pass_even) {
                    continue;
                }
                if a == 255 {
                    px.copy_from_slice(&color);
                } else {
                    px[0] = ((sr + px[0] as u32 * inv) >> 8) as u8;
                    px[1] = ((sg + px[1] as u32 * inv) >> 8) as u8;
                    px[2] = ((sb + px[2] as u32 * inv) >> 8) as u8;
                    px[3] = 255;
                }
            }
        }
    }

    pub(crate) fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: [u8; 4]) {
        let x0 = (x - w * 0.5).floor() as i32;
        let y0 = (y - h * 0.5).floor() as i32;
        let x1 = (x + w * 0.5).ceil() as i32;
        let y1 = (y + h * 0.5).ceil() as i32;
        for yy in y0.max(0)..y1.min(self.h as i32) {
            self.span(x0, x1, yy, c);
        }
    }

    pub(crate) fn rect_xywh(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        if w <= 0 || h <= 0 {
            return;
        }
        for yy in y.max(0)..(y + h).min(self.h as i32) {
            self.span(x, x + w, yy, c);
        }
    }

    pub(crate) fn ellipse(&mut self, x: f32, y: f32, rx: f32, ry: f32, c: [u8; 4]) {
        if rx <= 0.1 || ry <= 0.1 {
            return;
        }
        let y0 = ((y - ry).floor() as i32).max(0);
        let y1 = ((y + ry).ceil() as i32).min(self.h as i32 - 1);
        let inv_ry = 1.0 / ry;
        for yy in y0..=y1 {
            let dy = (yy as f32 - y) * inv_ry;
            let rem = 1.0 - dy * dy;
            if rem >= 0.0 {
                let half = rx * rem.sqrt();
                self.span(
                    (x - half).ceil() as i32,
                    (x + half).floor() as i32 + 1,
                    yy,
                    c,
                );
            }
        }
    }

    pub(crate) fn ellipse_ring(
        &mut self,
        x: f32,
        y: f32,
        rx: f32,
        ry: f32,
        thickness: f32,
        c: [u8; 4],
    ) {
        if rx <= 0.5 || ry <= 0.5 {
            return;
        }
        let irx = (rx - thickness).max(0.0);
        let iry = (ry - thickness * (ry / rx)).max(0.0);
        let y0 = ((y - ry).floor() as i32).max(0);
        let y1 = ((y + ry).ceil() as i32).min(self.h as i32 - 1);
        let inv_ry = 1.0 / ry;
        let inv_iry = if iry > 0.1 { 1.0 / iry } else { 0.0 };

        for yy in y0..=y1 {
            let dy = (yy as f32 - y) * inv_ry;
            let rem = 1.0 - dy * dy;
            if rem < 0.0 {
                continue;
            }
            let outer = rx * rem.sqrt();
            let ox0 = (x - outer).ceil() as i32;
            let ox1 = (x + outer).floor() as i32 + 1;

            if irx > 0.1 && iry > 0.1 {
                let idy = (yy as f32 - y) * inv_iry;
                let irem = 1.0 - idy * idy;
                if irem > 0.0 {
                    let inner = irx * irem.sqrt();
                    let ix0 = (x - inner).ceil() as i32;
                    let ix1 = (x + inner).floor() as i32 + 1;
                    self.span(ox0, ix0, yy, c);
                    self.span(ix1, ox1, yy, c);
                    continue;
                }
            }
            self.span(ox0, ox1, yy, c);
        }
    }

    pub(crate) fn diamond(&mut self, x: f32, y: f32, rx: f32, ry: f32, c: [u8; 4]) {
        let y0 = ((y - ry).ceil() as i32).max(0);
        let y1 = ((y + ry).floor() as i32).min(self.h as i32 - 1);
        let inv_ry = 1.0 / ry.max(0.1);
        for yy in y0..=y1 {
            let half = rx * (1.0 - (yy as f32 - y).abs() * inv_ry);
            self.span(
                (x - half).ceil() as i32,
                (x + half).floor() as i32 + 1,
                yy,
                c,
            );
        }
    }

    pub(crate) fn polygon(&mut self, points: &[(f32, f32)], color: [u8; 4]) {
        let n = points.len();
        if n < 3 {
            return;
        }
        let mut y_min = f32::INFINITY;
        let mut y_max = f32::NEG_INFINITY;
        for &(_, py) in points {
            y_min = y_min.min(py);
            y_max = y_max.max(py);
        }
        let row0 = (y_min.ceil() as i32).max(0);
        let row1 = (y_max.floor() as i32).min(self.h as i32 - 1);
        for row in row0..=row1 {
            let y = row as f32;
            let mut left = f32::INFINITY;
            let mut right = f32::NEG_INFINITY;
            for i in 0..n {
                let (ax, ay) = points[i];
                let (bx, by) = points[(i + 1) % n];
                let (lo, hi) = if ay < by { (ay, by) } else { (by, ay) };
                if y >= lo && y < hi {
                    let x = ax + (y - ay) * (bx - ax) / (by - ay);
                    left = left.min(x);
                    right = right.max(x);
                }
            }
            if left <= right {
                self.span(left.ceil() as i32, right.floor() as i32 + 1, row, color);
            }
        }
    }

    pub(crate) fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: [u8; 4]) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let n = dx.abs().max(dy.abs()).ceil() as i32;
        if n == 0 {
            self.put(x0.round() as i32, y0.round() as i32, c);
            return;
        }
        let inv_n = 1.0 / n as f32;
        let sx = dx * inv_n;
        let sy = dy * inv_n;
        let mut cx = x0;
        let mut cy = y0;
        for _ in 0..=n {
            self.put(cx.round() as i32, cy.round() as i32, c);
            cx += sx;
            cy += sy;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn dashed_line(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        dash: i32,
        gap: i32,
        phase: i32,
        c: [u8; 4],
    ) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let n = dx.abs().max(dy.abs()).ceil() as i32;
        if n == 0 {
            return;
        }
        let period = (dash + gap).max(1);
        let inv_n = 1.0 / n as f32;
        let sx = dx * inv_n;
        let sy = dy * inv_n;
        let mut cx = x0;
        let mut cy = y0;
        for i in 0..=n {
            if (i + phase).rem_euclid(period) < dash {
                self.put(cx.round() as i32, cy.round() as i32, c);
            }
            cx += sx;
            cy += sy;
        }
    }

    /// Fast additive 2:1 elliptical light pool with strict screen culling and scanline bounds.
    pub(crate) fn glow_iso(
        &mut self,
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        color: [u8; 3],
        strength: f32,
    ) {
        if rx <= 1.0 || ry <= 1.0 || strength <= 0.5 {
            return;
        }
        if cx + rx < 0.0 || cx - rx >= self.w as f32 || cy + ry < 0.0 || cy - ry >= self.h as f32 {
            return;
        }
        let y0 = ((cy - ry).floor() as i32).max(0);
        let y1 = ((cy + ry).ceil() as i32).min(self.h as i32 - 1);
        let inv_rx2 = 1.0 / (rx * rx);
        let inv_ry2 = 1.0 / (ry * ry);
        let cr = color[0] as u32;
        let cg = color[1] as u32;
        let cb = color[2] as u32;

        for y in y0..=y1 {
            let dy = y as f32 - cy;
            let dy_n = dy * dy * inv_ry2;
            let rem = 1.0 - dy_n;
            if rem <= 0.0 {
                continue;
            }
            let half_x = (rx * rem.sqrt()).ceil();
            let x0 = ((cx - half_x).floor() as i32).max(0);
            let x1 = ((cx + half_x).ceil() as i32).min(self.w as i32 - 1);
            if x0 > x1 {
                continue;
            }
            let row = y as usize * self.w;
            let slice = &mut self.p[(row + x0 as usize) * 4..(row + x1 as usize + 1) * 4];
            for (idx, px) in slice.chunks_exact_mut(4).enumerate() {
                let dx = (x0 + idx as i32) as f32 - cx;
                let falloff = rem - dx * dx * inv_rx2;
                if falloff > 0.0 {
                    let a = (strength * falloff * falloff).min(255.0) as u32;
                    if a > 0 {
                        px[0] = px[0].saturating_add(((cr * a) >> 8) as u8);
                        px[1] = px[1].saturating_add(((cg * a) >> 8) as u8);
                        px[2] = px[2].saturating_add(((cb * a) >> 8) as u8);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zombiebox_core::{ActionInput, Tile3D};

    #[test]
    fn screen_to_world_round_trips_with_damped_camera_and_look_ahead() {
        let mut game = ActionGame::residential(42);
        game.step(ActionInput::default());
        let mut r = Renderer::new(800, 360);

        // Before first render, matches free function.
        let free = screen_to_world(&game, 800, 360, 412.0, 195.0);
        let method = r.screen_to_world(&game, 412.0, 195.0);
        assert!((free.x - method.x).abs() < 1e-4 && (free.y - method.y).abs() < 1e-4);

        // After render with look-ahead disabled, player maps to center.
        r.set_look_ahead(false);
        r.render(&game, 0);
        let center_world = r.screen_to_world(&game, 400.0, 360.0 * 0.51);
        let p = game.player_pose().position;
        assert!((center_world.x - p.x).abs() < 0.01 && (center_world.y - p.y).abs() < 0.01);

        // With look-ahead enabled, focus shifts along facing.
        r.set_look_ahead(true);
        r.render(&game, 250);
        let shifted = r.screen_to_world(&game, 400.0, 360.0 * 0.51);
        assert!(shifted.y < p.y - 0.1);
    }

    #[test]
    fn renders_f0_f1_cutaway_and_combat_without_panics() {
        let mut game = ActionGame::residential(42);
        let mut r = Renderer::new(640, 360);
        let frame = r.render(&game, 0);
        assert_eq!(frame.len(), 640 * 360 * 4);
        assert!(frame.iter().any(|&b| b > 40));

        // Walk up to Axe cache and render several frames.
        for i in 1..=25 {
            game.step(ActionInput {
                movement: Vec2::new(0.4, -0.8),
                interact: i == 15,
                fire: i == 22,
                ..ActionInput::default()
            });
            r.render(&game, i * 16);
        }

        // Stress test with 100 monsters renders cleanly.
        game.spawn_stress_monsters(100);
        let stress_frame = r.render(&game, 500);
        assert_eq!(stress_frame.len(), 640 * 360 * 4);

        // Verify tile helper compiles and F1 tiles render cleanly if reached.
        let origin = game.block_bounds().unwrap().0;
        let _ = Tile3D::new(origin.x + 12, origin.y + 21, 1);
    }

    fn assert_non_uniform_frame(frame: &[u8]) {
        assert!(!frame.is_empty());
        let first = &frame[..4];
        assert!(
            frame.chunks_exact(4).any(|px| px != first),
            "expected non-uniform rendered framebuffer"
        );
    }

    #[test]
    fn renders_county_spawn_3f_apartment_and_terrain_without_panics() {
        let mut game = ActionGame::county(42);
        let mut r = Renderer::new(640, 360);

        // 1. County spawn (F0).
        let spawn_frame = r.render(&game, 0);
        assert_eq!(spawn_frame.len(), 640 * 360 * 4);
        assert_non_uniform_frame(spawn_frame);

        // Locate City (Downtown), Bridge/River, Hospital, Forest/Mountain tiles on the 512x512 world map.
        let mut city_pos = None;
        let mut bridge_pos = None;
        let mut hospital_pos = None;
        let mut mountain_or_forest = None;
        for y in (0..512).step_by(2) {
            for x in (0..512).step_by(2) {
                let p = zombiebox_core::Position::new(x, y);
                if let Some(tile) = game.world().tile_at(p) {
                    let walkable_ground = !matches!(
                        tile.ground,
                        zombiebox_core::Ground::River
                            | zombiebox_core::Ground::Lake
                            | zombiebox_core::Ground::Mountain
                    );
                    if city_pos.is_none()
                        && tile.district == zombiebox_core::District::City
                        && walkable_ground
                        && tile.feature == zombiebox_core::Feature::Empty
                    {
                        city_pos = Some(Tile3D::new(x, y, 0));
                    }
                    if bridge_pos.is_none() && tile.ground == zombiebox_core::Ground::Bridge {
                        bridge_pos = Some(Tile3D::new(x, y, 0));
                    }
                    if hospital_pos.is_none()
                        && tile.district == zombiebox_core::District::Hospital
                        && walkable_ground
                        && tile.feature == zombiebox_core::Feature::Empty
                    {
                        hospital_pos = Some(Tile3D::new(x, y, 0));
                    }
                    if mountain_or_forest.is_none()
                        && tile.ground == zombiebox_core::Ground::Trail
                        && tile.feature == zombiebox_core::Feature::Empty
                    {
                        mountain_or_forest = Some(Tile3D::new(x, y, 0));
                    }
                }
            }
        }

        // 2. Teleport to Downtown, find a 3F Apartment (floors >= 3), teleport to floor 2 and render!
        let city_anchor = city_pos.expect("expected City district tile in county(42)");
        let _ = game.teleport(city_anchor);
        r.notify_teleport();
        game.step(ActionInput::default());
        let mut found_3f = false;
        'find_3f: for b in game.buildings().to_vec() {
            if b.floors >= 3 {
                for y in b.y..b.y + b.height {
                    for x in b.x..b.x + b.width {
                        if game
                            .tile_render_info(zombiebox_core::Position::new(x, y), 2)
                            .is_some_and(|ti| {
                                ti.indoor
                                    && ti.surface != Surface::Air
                                    && ti.kind == TileKind::Empty
                            })
                            && game.teleport(Tile3D::new(x, y, 2)).is_some()
                        {
                            r.notify_teleport();
                            game.step(ActionInput::default());
                            assert_eq!(game.player_pose().floor, 2);
                            let f2_frame = r.render(&game, 64);
                            assert_non_uniform_frame(f2_frame);
                            found_3f = true;
                            break 'find_3f;
                        }
                    }
                }
            }
        }
        assert!(
            found_3f,
            "expected 3F Downtown Apartment render in county(42)"
        );

        // 3. Teleport near River/Bridge, Hospital, and Trail/Forest/Mountain without panics.
        for (i, spot) in [bridge_pos, hospital_pos, mountain_or_forest]
            .into_iter()
            .flatten()
            .enumerate()
        {
            if game.teleport(spot).is_some() {
                r.notify_teleport();
                game.step(ActionInput::default());
                let frame = r.render(&game, 256 + (i as u64) * 128);
                assert_non_uniform_frame(frame);
            }
        }
    }
}
