pub mod cache;
pub mod snapshot;

use std::collections::HashMap;

use zombiebox_core::{ActionEvent, ActionGame, ActionMode, BuildingId, Floor, Position, Tile3D};

use crate::camera::CameraFrame;
use crate::coords::{ChunkCoord, SimVec3, render_to_sim_point};

pub use cache::{
    CacheSyncMode, CacheSyncResult, CachedChunk, DirtyChunkDelta, SceneCache, halo_affected_chunks,
};
pub use snapshot::{
    BlastSnapshot, BlastSnapshotKind, BuildingCutawayState, BuildingSnapshot, CacheSnapshot,
    ChunkSnapshot, ChunkTerrainSnapshot, DEFAULT_VISIBILITY_HALF_EXTENT, DynamicSceneState,
    FlashSnapshot, FloorCellSnapshot, FrameSnapshot, LightSnapshot, LocalVisibilityGrid,
    PlayerSnapshot, SceneManager, TileSnapshot, VisibilityCell, build_chunk_snapshot,
    compute_chunk_fingerprint_from_game, hash_chunk_static_state,
};

pub const CUTAWAY_FADE_S: f32 = 0.16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneConfig {
    pub active_chunk_radius: i32,
    pub clamp_to_world_bounds: bool,
    pub sync_mode: CacheSyncMode,
    pub visibility_half_extent: i32,
}

impl Default for SceneConfig {
    fn default() -> Self {
        Self {
            active_chunk_radius: 3,
            clamp_to_world_bounds: false,
            sync_mode: CacheSyncMode::VerifyWindow,
            visibility_half_extent: DEFAULT_VISIBILITY_HALF_EXTENT,
        }
    }
}

#[derive(Debug)]
pub struct SceneManager {
    config: SceneConfig,
    cache: SceneCache,
    last_world_identity: Option<(ActionMode, u64)>,
    last_observed_tick: Option<u64>,
    pending_events: Vec<ActionEvent>,
    cutaways: HashMap<BuildingId, f32>,
    snap_cutaways_next_frame: bool,
}

impl SceneManager {
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(SceneConfig::default())
    }

    #[must_use]
    pub fn with_config(config: SceneConfig) -> Self {
        let cache = SceneCache::new(
            config.active_chunk_radius,
            config.clamp_to_world_bounds,
            config.sync_mode,
        );
        Self {
            config,
            cache,
            last_world_identity: None,
            last_observed_tick: None,
            pending_events: Vec::new(),
            cutaways: HashMap::new(),
            snap_cutaways_next_frame: true,
        }
    }

    #[must_use]
    pub const fn cache(&self) -> &SceneCache {
        &self.cache
    }

    pub fn cache_mut(&mut self) -> &mut SceneCache {
        &mut self.cache
    }

    pub fn reset(&mut self) {
        self.cache.clear();
        self.pending_events.clear();
        self.cutaways.clear();
        self.last_world_identity = None;
        self.last_observed_tick = None;
        self.snap_cutaways_next_frame = true;
    }

    pub fn notify_teleport(&mut self) {
        self.cutaways.clear();
        self.snap_cutaways_next_frame = true;
    }

    pub fn observe_tick(&mut self, game: &ActionGame) -> bool {
        let hud = game.hud();
        let world_id = (game.mode(), game.seed());

        if self
            .last_world_identity
            .is_some_and(|prev| prev != world_id)
            || self
                .last_observed_tick
                .is_some_and(|prev| hud.tick < prev || (hud.tick == 0 && prev > 0))
        {
            self.reset();
        }

        if self.last_observed_tick == Some(hud.tick) {
            return false;
        }

        for ev in game.events() {
            self.pending_events.push(*ev);
            match ev {
                ActionEvent::DoorToggled { at, .. }
                | ActionEvent::MaterialHit { at, .. }
                | ActionEvent::StructuralCollapse { at, .. } => {
                    self.cache.invalidate_tile3d_with_halo(*at);
                }
                _ => {}
            }
        }

        self.last_world_identity = Some(world_id);
        self.last_observed_tick = Some(hud.tick);
        true
    }

    #[must_use]
    pub fn prepare(&mut self, game: &ActionGame, camera: &CameraFrame, dt: f32) -> FrameSnapshot {
        let focus_sim = render_to_sim_point(camera.focus);
        let focus_pos = Position::new(focus_sim.x.round() as i32, focus_sim.y.round() as i32);
        self.prepare_at_focus(game, focus_pos, dt)
    }

    #[must_use]
    pub fn prepare_at_focus(
        &mut self,
        game: &ActionGame,
        focus_pos: Position,
        dt: f32,
    ) -> FrameSnapshot {
        let _ = self.observe_tick(game);
        let hud = game.hud();
        let focus_chunk = ChunkCoord::from_pos(focus_pos);

        let sync = self.cache.sync_window(game, focus_chunk);

        // Build local visibility grid
        let half = self.config.visibility_half_extent;
        let side = (half * 2 + 1) as usize;
        let origin = Position::new(focus_pos.x - half, focus_pos.y - half);
        let world = game.world();
        let (_, _, world_floors) = world.bounds();
        let floor_count = world_floors as u8;

        let mut visibility_cells = Vec::with_capacity(side * side * floor_count as usize);
        for fl in 0..floor_count {
            for dy in 0..side {
                for dx in 0..side {
                    let tx = origin.x + dx as i32;
                    let ty = origin.y + dy as i32;
                    let t3d = Tile3D::new(tx, ty, fl);
                    let mut cell = VisibilityCell::default();
                    if let Some(info) = world.tile_render_info(t3d) {
                        cell.visibility = info.visibility;
                        cell.core_light = info.light;
                        cell.building_id = info.building_id;
                        cell.indoor = info.indoor;
                    }
                    visibility_cells.push(cell);
                }
            }
        }

        let local_visibility = LocalVisibilityGrid {
            origin,
            side,
            floor_count,
            cells: visibility_cells,
        };

        // Player building check
        let player_pos = game.player_tile().pos();
        let player_building_id = world.building_id(Tile3D::from_pos(player_pos, hud.floor));

        // Cutaways
        let mut building_cutaways = Vec::new();
        let dt_clamped = dt.clamp(0.0, 0.1);
        for b in game.buildings() {
            let inside = player_building_id == b.id;
            let wants = inside; // simplified for T02
            let target_openness = if wants { 1.0 } else { 0.0 };
            let current = self.cutaways.entry(b.id).or_insert(target_openness);
            if self.snap_cutaways_next_frame {
                *current = target_openness;
            } else if *current != target_openness {
                let step = dt_clamped / CUTAWAY_FADE_S;
                if target_openness > *current {
                    *current = (*current + step).min(1.0);
                } else {
                    *current = (*current - step).max(0.0);
                }
            }

            let openness = *current;
            let stipple_step = ((1.0 - openness) * 4.0).round().clamp(0.0, 4.0) as u8;
            let dither_opacity_u8 = (openness * 255.0).round().clamp(0.0, 255.0) as u8;

            building_cutaways.push(BuildingCutawayState {
                building_id: b.id,
                floors: b.floors,
                foundation_z: b.foundation_z,
                player_inside: inside,
                wants_cutaway: wants,
                openness,
                stipple_step,
                dither_opacity_u8,
            });
        }
        building_cutaways.sort_by_key(|c| c.building_id);

        let ps = game.player_render_state();
        let p_z = world.elevation_at(ps.pose.position).unwrap_or(0.0)
            + (hud.floor as f32) * zombiebox_core::FLOOR_H;
        let foot_pos = SimVec3::new(ps.pose.position.x, ps.pose.position.y, p_z);
        let aim_dir3 = SimVec3::new(ps.pose.facing.x, ps.pose.facing.y, 0.0).normalize_or_zero();
        let muzzle_pos3 = foot_pos.with_z(foot_pos.z + 0.68);

        let player = PlayerSnapshot {
            render: ps,
            foot_pos,
            aim_dir3,
            muzzle_pos3,
        };

        let caches = game
            .caches()
            .iter()
            .map(|c| {
                let wp = c.world_pos;
                CacheSnapshot {
                    view: *c,
                    world_pos: wp,
                    visibility: local_visibility.visibility_at(c.pos),
                }
            })
            .collect();

        let lights = game
            .light_sources()
            .iter()
            .map(|l| LightSnapshot {
                view: *l,
                position: l.position,
            })
            .collect();

        let dynamic_flashes = game
            .dynamic_flashes()
            .iter()
            .map(|f| FlashSnapshot {
                position: f.position,
                floor: f.floor,
                radius: f.radius,
                intensity: f.intensity,
                ttl_ticks: f.ttl_ticks,
            })
            .collect();

        let blasts = game
            .blasts()
            .iter()
            .map(|b| {
                let kind = match b.kind {
                    zombiebox_core::BlastKind::Rocket => BlastSnapshotKind::Rocket,
                    zombiebox_core::BlastKind::BloaterCaustic => BlastSnapshotKind::BloaterCaustic,
                };
                BlastSnapshot {
                    id: b.id,
                    kind,
                    position: b.position,
                    floor: b.floor,
                    radius: b.radius,
                    progress: b.progress,
                }
            })
            .collect();

        let dynamic = DynamicSceneState {
            sim_tick: hud.tick,
            mode: game.mode(),
            seed: game.seed(),
            outcome: game.outcome(),
            dusk_level: 0.0,
            district: game
                .world()
                .district_at(focus_pos)
                .unwrap_or(zombiebox_core::District::Residential),
            active_floor: hud.floor,
            player_building_id,
            player,
            monsters: game.monster_views().to_vec(),
            projectiles: game.projectiles().to_vec(),
            caches,
            lights,
            dynamic_flashes,
            blasts,
            building_cutaways,
            local_visibility,
        };

        let events = std::mem::take(&mut self.pending_events);
        self.snap_cutaways_next_frame = false;

        FrameSnapshot {
            sim_tick: hud.tick,
            focus_chunk,
            active_chunks: sync.active_chunks,
            dirty_chunks: sync.dirty_chunks,
            evicted_chunks: sync.evicted_chunks,
            dynamic,
            events,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::CameraRig;

    #[test]
    fn fingerprint_stream_matches_snapshot_fingerprint() {
        let game = ActionGame::residential(42);
        let coord = ChunkCoord::new(0, 0);
        let snap = build_chunk_snapshot(&game, coord, 1);
        let fp = compute_chunk_fingerprint_from_game(&game, coord);
        assert_eq!(snap.static_fingerprint, fp);
    }

    #[test]
    fn halo_affected_chunks_and_negative_coordinates() {
        let (c_in, n_in) = halo_affected_chunks(Position::new(5, 5));
        assert_eq!(n_in, 1);
        assert_eq!(c_in[0], ChunkCoord::new(0, 0));

        let (c_edge, n_edge) = halo_affected_chunks(Position::new(0, 5));
        assert_eq!(n_edge, 2);
        assert!(c_edge[0..2].contains(&ChunkCoord::new(0, 0)));
        assert!(c_edge[0..2].contains(&ChunkCoord::new(-1, 0)));

        let (c_neg, n_neg) = halo_affected_chunks(Position::new(-1, -1));
        assert_eq!(n_neg, 4);
        assert!(c_neg[0..4].contains(&ChunkCoord::new(-1, -1)));
        assert!(c_neg[0..4].contains(&ChunkCoord::new(0, -1)));
        assert!(c_neg[0..4].contains(&ChunkCoord::new(-1, 0)));
        assert!(c_neg[0..4].contains(&ChunkCoord::new(0, 0)));
    }

    #[test]
    fn camera_only_movement_triggers_zero_rebuilds() {
        let game = ActionGame::residential(42);
        let mut manager = SceneManager::new();
        let rig = CameraRig::new(640, 360);
        let frame0 = manager.prepare(&game, &rig.current_frame(), 0.016);
        assert!(!frame0.dirty_chunks.is_empty());

        let frame1 = manager.prepare(&game, &rig.current_frame(), 0.016);
        assert!(frame1.dirty_chunks.is_empty());
        assert!(frame1.evicted_chunks.is_empty());
    }

    #[test]
    fn observe_tick_handles_zero_tick_and_multi_tick_catchup() {
        let mut game = ActionGame::residential(42);
        let mut manager = SceneManager::new();
        assert!(manager.observe_tick(&game));
        assert!(!manager.observe_tick(&game)); // 0-tick deduplication

        game.step(Default::default());
        assert!(manager.observe_tick(&game));
    }
}
