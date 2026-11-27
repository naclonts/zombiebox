pub mod cache;
pub mod snapshot;

use std::collections::HashMap;

use zombiebox_core::{
    ActionEvent, ActionGame, ActionMode, BuildingId, BuildingInfo, Position, Tile3D, TileKind,
    TileVisibility, Vec2 as SimVec2,
};

use crate::camera::CameraFrame;
use crate::coords::{ChunkCoord, render_to_sim_point};

pub use cache::{
    CacheSyncMode, CacheSyncResult, CachedChunk, DirtyChunkDelta, SceneCache, halo_affected_chunks,
};
pub use snapshot::{
    BlastSnapshot, BlastSnapshotKind, BuildingCutawayState, BuildingSnapshot, CHUNK_HALO_LEN,
    CHUNK_HALO_SIDE, CacheSnapshot, ChunkSnapshot, ChunkTerrainSnapshot,
    DEFAULT_VISIBILITY_HALF_EXTENT, DynamicSceneState, FlashSnapshot, FloorCellSnapshot,
    FrameSnapshot, LightSnapshot, LocalVisibilityGrid, MAX_SNAPSHOT_FLOORS, PlayerSnapshot,
    TileSnapshot, VisibilityCell, build_chunk_snapshot, compute_chunk_fingerprint_from_game,
    hash_chunk_static_state,
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

impl Default for SceneManager {
    fn default() -> Self {
        Self::new()
    }
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
        let tick = game.tick_count();
        let world_id = (game.mode(), game.seed());

        if self
            .last_world_identity
            .is_some_and(|prev| prev != world_id)
            || self
                .last_observed_tick
                .is_some_and(|prev| tick < prev || (tick == 0 && prev > 0))
        {
            self.reset();
        }

        if self.last_observed_tick == Some(tick) {
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
        self.last_observed_tick = Some(tick);
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
        let tick = game.tick_count();
        let focus_chunk = ChunkCoord::from_pos(focus_pos);

        let sync = self.cache.sync_window(game, focus_chunk);

        // Build local visibility grid
        let half = self.config.visibility_half_extent;
        let side = (half * 2 + 1) as usize;
        let origin = Position::new(focus_pos.x - half, focus_pos.y - half);
        let floor_count = game.window_max_floors().min(MAX_SNAPSHOT_FLOORS as u8);

        let mut visibility_cells = Vec::with_capacity(side * side * floor_count as usize);
        for fl in 0..floor_count {
            for dy in 0..side {
                for dx in 0..side {
                    let tx = origin.x + dx as i32;
                    let ty = origin.y + dy as i32;
                    visibility_cells.push(VisibilityCell {
                        visibility: game
                            .tile_visibility(Tile3D::new(tx, ty, fl))
                            .unwrap_or(TileVisibility::Hidden),
                    });
                }
            }
        }

        let local_visibility = LocalVisibilityGrid {
            origin,
            side,
            floor_count,
            cells: visibility_cells,
        };

        let ps = game.player_render_state();
        let player_pos = ps.pose.position;
        let player_tile = game.player_tile().pos();
        let floor = ps.pose.floor;
        let player_building_id = game
            .tile_render_info(player_tile, floor)
            .map_or(0, |ti| ti.building_id);

        // Cutaways
        let active_buildings = game.buildings();
        if self.cutaways.len() > 128 {
            self.cutaways.retain(|id, _| {
                active_buildings
                    .iter()
                    .any(|b| (0..b.effective_wing_count()).any(|i| b.cutaway_slot(i) == *id))
            });
        }

        let mut building_cutaways = Vec::with_capacity(active_buildings.len());
        let dt_clamped = dt.clamp(0.0, 0.1);
        let mut nearby_openings = Vec::new();
        for dy in -2..=2 {
            for dx in -2..=2 {
                if let Some(tile) = game
                    .tile_render_info(Position::new(player_tile.x + dx, player_tile.y + dy), floor)
                    && matches!(
                        tile.kind,
                        TileKind::Window | TileKind::BrokenWindow | TileKind::Door { .. }
                    )
                {
                    nearby_openings.push((
                        tile.building_id,
                        Position::new(player_tile.x + dx, player_tile.y + dy),
                    ));
                }
            }
        }
        for b in active_buildings {
            for wing_index in 0..b.effective_wing_count() {
                let wing = b.wing_at(wing_index);
                let slot = b.cutaway_slot(wing_index);
                let inside =
                    player_building_id == b.id && b.wing_index_at(player_tile, floor) == wing_index;
                let wants = building_wants_cutaway(
                    b,
                    wing_index,
                    player_pos,
                    floor,
                    inside,
                    &nearby_openings,
                );
                let target_openness = if wants { 1.0 } else { 0.0 };
                let current = self.cutaways.entry(slot).or_insert(target_openness);
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
                let dither_opacity_u8 = ((1.0 - openness) * 255.0).round().clamp(0.0, 255.0) as u8;

                building_cutaways.push(BuildingCutawayState {
                    building_id: slot,
                    floors: wing.floors,
                    foundation_z: b.foundation_z,
                    player_inside: inside,
                    wants_cutaway: wants,
                    openness,
                    stipple_step,
                    dither_opacity_u8,
                });
            }
        }
        building_cutaways.sort_by_key(|c| c.building_id);

        let player = PlayerSnapshot {
            render: ps,
            foot_pos: ps.body.position,
            aim_dir3: ps.aim_dir,
            muzzle_pos3: ps.muzzle_pos,
        };

        let caches = game
            .caches()
            .iter()
            .map(|c| CacheSnapshot {
                view: *c,
                world_pos: c.world_pos,
                ground_normal: game.ground_normal_at(c.world_pos.xy(), c.pos.floor),
                visibility: local_visibility.visibility_at(c.pos),
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
            sim_tick: tick,
            mode: game.mode(),
            seed: game.seed(),
            outcome: game.outcome(),
            dusk_level: game.dusk_level(),
            district: game
                .world()
                .district_at(focus_pos)
                .unwrap_or(zombiebox_core::District::Residential),
            active_floor: floor,
            player_building_id,
            player,
            vehicles: game.vehicle_views().collect(),
            mounted_vehicle: game.mounted_vehicle(),
            monsters: game.monster_views(),
            projectiles: game.projectiles(),
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
            sim_tick: tick,
            focus_chunk,
            active_chunks: sync.active_chunks,
            dirty_chunks: sync.dirty_chunks,
            evicted_chunks: sync.evicted_chunks,
            dynamic,
            events,
        }
    }
}

fn building_wants_cutaway(
    b: &BuildingInfo,
    wing_index: usize,
    p: SimVec2,
    floor: u8,
    inside: bool,
    nearby_openings: &[(BuildingId, Position)],
) -> bool {
    let wing = b.wing_at(wing_index);
    if floor >= wing.floors {
        return false;
    }
    if inside {
        return true;
    }
    nearby_openings.iter().any(|&(id, pos)| {
        id == b.id
            && b.wing_index_at(pos, floor) == wing_index
            && p.distance(SimVec2::new(pos.x as f32, pos.y as f32)) <= 1.3
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{CameraMode, CameraRig, CameraTarget};
    use crate::coords::tile_local_in_chunk;
    use zombiebox_core::{ActionInput, DamageKind, Weapon};

    #[test]
    fn cutaways_follow_occupied_wings_and_floor_height() {
        let game = ActionGame::residential(42);
        let mut b = game.buildings()[0];
        b.x = 0;
        b.y = 0;
        b.width = 10;
        b.height = 10;
        b.floors = 6;
        b.wings[0] = zombiebox_core::WingRect::new(0, 0, 2, 9, 6);
        b.wings[1] = zombiebox_core::WingRect::new(7, 0, 9, 9, 2);
        b.wing_count = 2;
        assert!(!building_wants_cutaway(
            &b,
            0,
            SimVec2::new(5.0, 5.0),
            0,
            false,
            &[]
        ));
        assert!(!building_wants_cutaway(
            &b,
            1,
            SimVec2::new(5.0, 5.0),
            0,
            false,
            &[]
        ));
        assert!(building_wants_cutaway(
            &b,
            1,
            SimVec2::new(8.0, 5.0),
            1,
            true,
            &[]
        ));
        assert!(!building_wants_cutaway(
            &b,
            1,
            SimVec2::new(8.0, 5.0),
            2,
            false,
            &[]
        ));
        assert!(building_wants_cutaway(
            &b,
            0,
            SimVec2::new(1.0, 5.0),
            5,
            true,
            &[]
        ));
    }

    #[test]
    fn camera_only_movement_triggers_zero_rebuilds() {
        let game = ActionGame::residential(42);
        let mut manager = SceneManager::new();
        let mut rig = CameraRig::new(640, 360);
        let target = CameraTarget {
            position: game.player_body().position,
            ..Default::default()
        };

        let frame0 = rig.update(target, 0.016);
        let snap0 = manager.prepare(&game, &frame0, 0.016);
        assert!(!snap0.dirty_chunks.is_empty());

        for mode in [
            CameraMode::Isometric,
            CameraMode::Orbit,
            CameraMode::Chase,
            CameraMode::Overhead,
            CameraMode::FirstPerson,
        ] {
            rig.set_mode(mode);
            if mode == CameraMode::Orbit {
                rig.orbit_rotate(0.4, 0.1);
                rig.orbit_zoom_by(-0.2);
            }
            let cam_frame = rig.update(target, 0.016);
            let snap = manager.prepare(&game, &cam_frame, 0.016);
            assert!(
                snap.dirty_chunks.is_empty(),
                "mode {mode:?} unexpectedly rebuilt chunks"
            );
            assert!(
                snap.evicted_chunks.is_empty(),
                "mode {mode:?} unexpectedly evicted chunks"
            );
        }
    }

    #[test]
    fn door_toggle_and_wall_breach_dirty_only_affected_chunk_and_halo() {
        // 1. Birch Street interior door + wall breach
        let mut game = ActionGame::residential(42);
        let mut manager = SceneManager::new();
        let focus = game.player_tile().pos();
        let frame0 = manager.prepare_at_focus(&game, focus, 0.016);
        assert!(!frame0.dirty_chunks.is_empty());

        let (bmin, bmax) = game.block_bounds().unwrap();
        let mut interior_door = None;
        let mut interior_wall = None;
        for y in bmin.y..=bmax.y {
            for x in bmin.x..=bmax.x {
                let (lx, ly) = tile_local_in_chunk(x, y);
                if !(1..=14).contains(&lx) || !(1..=14).contains(&ly) {
                    continue;
                }
                let p = Position::new(x, y);
                if let Some(ti) = game.tile_render_info(p, 0) {
                    if interior_door.is_none() && matches!(ti.kind, TileKind::Door { .. }) {
                        interior_door = Some(Tile3D::from_pos(p, 0));
                    }
                    if interior_wall.is_none()
                        && matches!(ti.kind, TileKind::ExteriorWall | TileKind::InteriorWall)
                    {
                        interior_wall = Some(Tile3D::from_pos(p, 0));
                    }
                }
            }
        }

        let door_tile = interior_door.expect("starter block has an interior door");
        let door_chunk = ChunkCoord::from_pos(door_tile.pos());
        let before_revs: HashMap<ChunkCoord, u64> = frame0
            .active_chunks
            .iter()
            .map(|&c| (c, manager.cache().revision_of(c).unwrap()))
            .collect();

        assert!(game.toggle_door(door_tile).is_some());
        let frame_door = manager.prepare_at_focus(&game, focus, 0.016);
        assert_eq!(frame_door.dirty_chunks.len(), 1);
        assert_eq!(frame_door.dirty_chunks[0].coord, door_chunk);
        for (&c, &rev) in &before_revs {
            if c != door_chunk {
                assert_eq!(manager.cache().revision_of(c), Some(rev));
            }
        }

        let wall_tile = interior_wall.expect("starter block has an interior wall");
        let revs_before_wall: HashMap<ChunkCoord, u64> = frame_door
            .active_chunks
            .iter()
            .map(|&c| (c, manager.cache().revision_of(c).unwrap()))
            .collect();
        let dmg = game
            .damage_tile(wall_tile, DamageKind::RocketBlast)
            .expect("wall is damageable");
        assert!(dmg.damaged);
        let frame_wall = manager.prepare_at_focus(&game, focus, 0.016);
        assert!(!frame_wall.dirty_chunks.is_empty());
        let wall_chunk = ChunkCoord::from_pos(wall_tile.pos());
        assert!(
            frame_wall
                .dirty_chunks
                .iter()
                .any(|d| d.coord == wall_chunk)
        );
        for d in &frame_wall.dirty_chunks {
            assert!(
                (d.coord.cx - wall_chunk.cx).abs() <= 1 && (d.coord.cy - wall_chunk.cy).abs() <= 1
            );
        }
        for (&c, &rev) in &revs_before_wall {
            if !frame_wall.dirty_chunks.iter().any(|d| d.coord == c) {
                assert_eq!(manager.cache().revision_of(c), Some(rev));
            }
        }

        // 2. County boundary tile test (or chunk-edge door/wall)
        let mut county = ActionGame::county(42);
        let mut c_mgr = SceneManager::new();
        let c_focus = county.player_tile().pos();
        let c_frame0 = c_mgr.prepare_at_focus(&county, c_focus, 0.016);

        let mut edge_target: Option<Tile3D> = None;
        for &chunk in &c_frame0.active_chunks {
            let min = chunk.min_tile();
            for ly in 0..16 {
                for lx in 0..16 {
                    if lx != 0 && lx != 15 && ly != 0 && ly != 15 {
                        continue;
                    }
                    let p = Position::new(min.x + lx, min.y + ly);
                    if let Some(ti) = county.tile_render_info(p, 0)
                        && matches!(
                            ti.kind,
                            TileKind::Door { .. }
                                | TileKind::ExteriorWall
                                | TileKind::InteriorWall
                                | TileKind::Prop(_)
                        )
                    {
                        edge_target = Some(Tile3D::from_pos(p, 0));
                        break;
                    }
                }
                if edge_target.is_some() {
                    break;
                }
            }
            if edge_target.is_some() {
                break;
            }
        }
        let target =
            edge_target.expect("county window has a destructible/toggleable chunk-edge tile");
        let (expected_chunks, expected_count) = halo_affected_chunks(target.pos());
        assert!(expected_count >= 2);
        let c_revs_before: HashMap<ChunkCoord, u64> = c_frame0
            .active_chunks
            .iter()
            .map(|&c| (c, c_mgr.cache().revision_of(c).unwrap()))
            .collect();

        if matches!(
            county.tile_render_info(target.pos(), 0).unwrap().kind,
            TileKind::Door { .. }
        ) {
            county.toggle_door(target);
        } else {
            county.damage_tile(target, DamageKind::RocketBlast);
        }
        let c_frame1 = c_mgr.prepare_at_focus(&county, c_focus, 0.016);
        assert!(!c_frame1.dirty_chunks.is_empty());
        for d in &c_frame1.dirty_chunks {
            assert!(
                expected_chunks[..expected_count].contains(&d.coord)
                    || (d.coord.cx - expected_chunks[0].cx).abs() <= 1
                        && (d.coord.cy - expected_chunks[0].cy).abs() <= 1,
                "unrelated chunk {:?} dirtied for edge tile {:?}",
                d.coord,
                target
            );
        }
        for (&c, &rev) in &c_revs_before {
            if !c_frame1.dirty_chunks.iter().any(|d| d.coord == c) {
                assert_eq!(c_mgr.cache().revision_of(c), Some(rev));
            }
        }
    }

    #[test]
    fn cache_opening_and_walking_visibility_trigger_zero_chunk_rebuilds() {
        let mut game = ActionGame::residential(42);
        let mut manager = SceneManager::new();
        let focus = game.player_tile().pos();
        let frame0 = manager.prepare_at_focus(&game, focus, 0.016);
        assert!(!frame0.dirty_chunks.is_empty());

        let starter_cache = game
            .caches()
            .into_iter()
            .find(|c| !c.opened)
            .expect("residential has an unopened cache");

        // Walk toward starter cache
        for _ in 0..90 {
            let p = game.player_body().position.xy();
            let delta = starter_cache.pos.center() - p;
            if delta.length() < 0.6 {
                break;
            }
            game.step(ActionInput {
                movement: delta.normalized(),
                ..Default::default()
            });
            manager.observe_tick(&game);
            let frame = manager.prepare_at_focus(&game, focus, 0.016);
            assert!(
                frame.dirty_chunks.is_empty(),
                "walking visibility/cutaway caused rebuilds"
            );
        }

        // Interact to loot the cache
        game.step(ActionInput {
            interact: true,
            ..Default::default()
        });
        manager.observe_tick(&game);
        let frame_loot = manager.prepare_at_focus(&game, focus, 0.016);
        assert!(frame_loot.dirty_chunks.is_empty());
        let looted = frame_loot
            .dynamic
            .caches
            .iter()
            .find(|c| c.view.id == starter_cache.id)
            .expect("starter cache present in dynamic state");
        assert!(looted.view.opened);
    }

    #[test]
    fn observe_tick_handles_zero_tick_and_multi_tick_catchup_without_duplication() {
        let mut game = ActionGame::residential(42);
        let mut manager = SceneManager::new();
        let focus = game.player_tile().pos();

        assert!(manager.observe_tick(&game));
        assert!(!manager.observe_tick(&game)); // 0-tick dedup

        let _ = manager.prepare_at_focus(&game, focus, 0.016);

        // Tick 1
        game.step(ActionInput {
            select_weapon: Some(Weapon::Axe),
            fire: true,
            movement: SimVec2::new(1.0, 0.0),
            ..Default::default()
        });
        let ev_t1 = game.events().len();
        assert!(ev_t1 > 0);
        assert!(manager.observe_tick(&game));

        // Tick 2 (no prepare in between)
        game.step(ActionInput {
            movement: SimVec2::new(1.0, 0.0),
            fire: true,
            ..Default::default()
        });
        let ev_t2 = game.events().len();
        assert!(manager.observe_tick(&game));

        let snap = manager.prepare_at_focus(&game, focus, 0.016);
        assert_eq!(snap.events.len(), ev_t1 + ev_t2);

        // 0-tick follow-up
        assert!(!manager.observe_tick(&game));
        let snap_idle = manager.prepare_at_focus(&game, focus, 0.016);
        assert!(snap_idle.events.is_empty());

        // Restart resets cleanly
        let fresh = ActionGame::residential(99);
        assert!(manager.observe_tick(&fresh));
        let snap_fresh = manager.prepare_at_focus(&fresh, fresh.player_tile().pos(), 0.016);
        assert!(!snap_fresh.dirty_chunks.is_empty());
        assert_eq!(snap_fresh.dynamic.seed, 99);
    }
}
