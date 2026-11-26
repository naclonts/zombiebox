use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use zombiebox_core::{ActionGame, CHUNK_SIZE, CHUNKS_PER_SIDE, Position, Tile3D};

use crate::coords::{ChunkCoord, tile_local_in_chunk};
use crate::scene::snapshot::{
    ChunkSnapshot, build_chunk_snapshot, compute_chunk_fingerprint_from_game,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CacheSyncMode {
    #[default]
    VerifyWindow,
    EventDriven,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DirtyChunkDelta {
    pub coord: ChunkCoord,
    pub revision: u64,
    pub snapshot: Arc<ChunkSnapshot>,
}

#[derive(Clone, Debug)]
pub struct CachedChunk {
    pub coord: ChunkCoord,
    pub revision: u64,
    pub static_fingerprint: u64,
    pub snapshot: Arc<ChunkSnapshot>,
}

#[derive(Clone, Debug, Default)]
pub struct CacheSyncResult {
    pub active_chunks: Vec<ChunkCoord>,
    pub dirty_chunks: Vec<DirtyChunkDelta>,
    pub evicted_chunks: Vec<ChunkCoord>,
}

#[must_use]
pub fn halo_affected_chunks(pos: Position) -> ([ChunkCoord; 4], usize) {
    let owner = ChunkCoord::from_pos(pos);
    let (lx, ly) = tile_local_in_chunk(pos.x, pos.y);
    let dx: i32 = if lx == 0 {
        -1
    } else if lx == CHUNK_SIZE as usize - 1 {
        1
    } else {
        0
    };
    let dy: i32 = if ly == 0 {
        -1
    } else if ly == CHUNK_SIZE as usize - 1 {
        1
    } else {
        0
    };

    let mut out = [owner; 4];
    let mut count = 1usize;
    if dx != 0 {
        out[count] = ChunkCoord::new(owner.cx + dx, owner.cy);
        count += 1;
    }
    if dy != 0 {
        out[count] = ChunkCoord::new(owner.cx, owner.cy + dy);
        count += 1;
    }
    if dx != 0 && dy != 0 {
        out[count] = ChunkCoord::new(owner.cx + dx, owner.cy + dy);
        count += 1;
    }
    (out, count)
}

#[derive(Debug)]
pub struct SceneCache {
    active_chunk_radius: i32,
    clamp_to_world_bounds: bool,
    sync_mode: CacheSyncMode,
    next_revision: u64,
    chunks: HashMap<ChunkCoord, CachedChunk>,
    pending_invalidations: HashSet<ChunkCoord>,
}

impl SceneCache {
    #[must_use]
    pub fn new(
        active_chunk_radius: i32,
        clamp_to_world_bounds: bool,
        sync_mode: CacheSyncMode,
    ) -> Self {
        Self {
            active_chunk_radius,
            clamp_to_world_bounds,
            sync_mode,
            next_revision: 0,
            chunks: HashMap::new(),
            pending_invalidations: HashSet::new(),
        }
    }

    pub fn set_sync_mode(&mut self, mode: CacheSyncMode) {
        self.sync_mode = mode;
    }

    #[must_use]
    pub const fn sync_mode(&self) -> CacheSyncMode {
        self.sync_mode
    }

    #[must_use]
    pub const fn active_chunk_radius(&self) -> i32 {
        self.active_chunk_radius
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    #[must_use]
    pub fn get(&self, coord: ChunkCoord) -> Option<&CachedChunk> {
        self.chunks.get(&coord)
    }

    #[must_use]
    pub fn revision_of(&self, coord: ChunkCoord) -> Option<u64> {
        self.chunks.get(&coord).map(|c| c.revision)
    }

    #[must_use]
    pub fn sorted_resident_coords(&self) -> Vec<ChunkCoord> {
        let mut keys: Vec<_> = self.chunks.keys().copied().collect();
        keys.sort_unstable_by_key(|a| (a.cy, a.cx));
        keys
    }

    pub fn invalidate_chunk(&mut self, coord: ChunkCoord) {
        self.pending_invalidations.insert(coord);
    }

    pub fn invalidate_tile_with_halo(&mut self, pos: Position) {
        let (coords, count) = halo_affected_chunks(pos);
        for coord in coords.iter().take(count) {
            self.pending_invalidations.insert(*coord);
        }
    }

    pub fn invalidate_tile3d_with_halo(&mut self, tile: Tile3D) {
        self.invalidate_tile_with_halo(tile.pos());
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
        self.pending_invalidations.clear();
    }

    #[must_use]
    pub fn compute_active_window(&self, focus_chunk: ChunkCoord) -> Vec<ChunkCoord> {
        let r = self.active_chunk_radius;
        let mut coords = Vec::new();
        for cy in (focus_chunk.cy - r)..=(focus_chunk.cy + r) {
            for cx in (focus_chunk.cx - r)..=(focus_chunk.cx + r) {
                let coord = ChunkCoord::new(cx, cy);
                if self.clamp_to_world_bounds {
                    if (0..CHUNKS_PER_SIDE).contains(&cx) && (0..CHUNKS_PER_SIDE).contains(&cy) {
                        coords.push(coord);
                    }
                } else {
                    coords.push(coord);
                }
            }
        }
        coords.sort_unstable_by_key(|a| (a.cy, a.cx));
        coords
    }

    pub fn sync_window(&mut self, game: &ActionGame, focus_chunk: ChunkCoord) -> CacheSyncResult {
        let active_chunks = self.compute_active_window(focus_chunk);

        let mut evicted_chunks: Vec<ChunkCoord> = self
            .chunks
            .keys()
            .copied()
            .filter(|coord| {
                active_chunks
                    .binary_search_by_key(&(coord.cy, coord.cx), |c| (c.cy, c.cx))
                    .is_err()
            })
            .collect();
        evicted_chunks.sort_unstable_by_key(|a| (a.cy, a.cx));
        for &coord in &evicted_chunks {
            self.chunks.remove(&coord);
            self.pending_invalidations.remove(&coord);
        }

        let mut dirty_chunks = Vec::new();

        for coord in &active_chunks {
            let mut rebuild = false;
            let mut existing_rev = 0u64;

            if self.pending_invalidations.remove(coord) {
                if let Some(existing) = self.chunks.get(coord) {
                    existing_rev = existing.revision;
                }
                rebuild = true;
            } else if let Some(existing) = self.chunks.get(coord) {
                existing_rev = existing.revision;
                if self.sync_mode == CacheSyncMode::VerifyWindow {
                    let fp = compute_chunk_fingerprint_from_game(game, *coord);
                    if fp != existing.static_fingerprint {
                        rebuild = true;
                    }
                }
            } else {
                rebuild = true;
            }

            if rebuild {
                self.next_revision += 1;
                let rev = self.next_revision.max(existing_rev + 1);
                let snap = Arc::new(build_chunk_snapshot(game, *coord, rev));
                let cached = CachedChunk {
                    coord: *coord,
                    revision: rev,
                    static_fingerprint: snap.static_fingerprint,
                    snapshot: snap.clone(),
                };
                self.chunks.insert(*coord, cached);
                dirty_chunks.push(DirtyChunkDelta {
                    coord: *coord,
                    revision: rev,
                    snapshot: snap,
                });
            }
        }

        CacheSyncResult {
            active_chunks,
            dirty_chunks,
            evicted_chunks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halo_affected_chunks_and_negative_coordinates() {
        let (c_in, n_in) = halo_affected_chunks(Position::new(5, 5));
        assert_eq!(n_in, 1);
        assert_eq!(c_in[0], ChunkCoord::new(0, 0));

        for pos in [
            Position::new(0, 5),
            Position::new(15, 5),
            Position::new(5, 0),
            Position::new(5, 15),
        ] {
            let (_, n) = halo_affected_chunks(pos);
            assert_eq!(n, 2, "edge tile {pos:?}");
        }

        let (c00, n00) = halo_affected_chunks(Position::new(0, 0));
        assert_eq!(n00, 4);
        assert!(c00[..4].contains(&ChunkCoord::new(0, 0)));
        assert!(c00[..4].contains(&ChunkCoord::new(-1, 0)));
        assert!(c00[..4].contains(&ChunkCoord::new(0, -1)));
        assert!(c00[..4].contains(&ChunkCoord::new(-1, -1)));

        let (c15, n15) = halo_affected_chunks(Position::new(15, 15));
        assert_eq!(n15, 4);
        assert!(c15[..4].contains(&ChunkCoord::new(0, 0)));
        assert!(c15[..4].contains(&ChunkCoord::new(1, 0)));
        assert!(c15[..4].contains(&ChunkCoord::new(0, 1)));
        assert!(c15[..4].contains(&ChunkCoord::new(1, 1)));

        let (neg_corner, n_nc) = halo_affected_chunks(Position::new(-1, -1));
        assert_eq!(n_nc, 4);
        for expected in [
            ChunkCoord::new(-1, -1),
            ChunkCoord::new(0, -1),
            ChunkCoord::new(-1, 0),
            ChunkCoord::new(0, 0),
        ] {
            assert!(neg_corner[..4].contains(&expected));
        }

        let (neg_min, n_nm) = halo_affected_chunks(Position::new(-16, -16));
        assert_eq!(n_nm, 4);
        for expected in [
            ChunkCoord::new(-1, -1),
            ChunkCoord::new(-2, -1),
            ChunkCoord::new(-1, -2),
            ChunkCoord::new(-2, -2),
        ] {
            assert!(neg_min[..4].contains(&expected));
        }

        let (neg_edge, n_ne) = halo_affected_chunks(Position::new(-1, -8));
        assert_eq!(n_ne, 2);
        assert!(neg_edge[..2].contains(&ChunkCoord::new(-1, -1)));
        assert!(neg_edge[..2].contains(&ChunkCoord::new(0, -1)));

        // Sync with negative ChunkCoord support and verify halo invalidation at (0, 0)
        let game = ActionGame::residential(42);
        let mut cache = SceneCache::new(1, false, CacheSyncMode::EventDriven);
        let sync0 = cache.sync_window(&game, ChunkCoord::new(0, 0));
        assert_eq!(sync0.active_chunks.len(), 9);
        assert!(cache.get(ChunkCoord::new(-1, -1)).is_some());
        assert!(cache.get(ChunkCoord::new(-1, 0)).is_some());
        assert!(cache.get(ChunkCoord::new(0, -1)).is_some());

        let rev_1_0 = cache.revision_of(ChunkCoord::new(1, 0)).unwrap();
        let rev_0_1 = cache.revision_of(ChunkCoord::new(0, 1)).unwrap();
        let rev_1_1 = cache.revision_of(ChunkCoord::new(1, 1)).unwrap();

        cache.invalidate_tile_with_halo(Position::new(0, 0));
        let sync1 = cache.sync_window(&game, ChunkCoord::new(0, 0));
        let dirty_coords: Vec<_> = sync1.dirty_chunks.iter().map(|d| d.coord).collect();
        assert_eq!(dirty_coords.len(), 4);
        for expected in [
            ChunkCoord::new(0, 0),
            ChunkCoord::new(-1, 0),
            ChunkCoord::new(0, -1),
            ChunkCoord::new(-1, -1),
        ] {
            assert!(dirty_coords.contains(&expected));
        }
        assert_eq!(cache.revision_of(ChunkCoord::new(1, 0)), Some(rev_1_0));
        assert_eq!(cache.revision_of(ChunkCoord::new(0, 1)), Some(rev_0_1));
        assert_eq!(cache.revision_of(ChunkCoord::new(1, 1)), Some(rev_1_1));
    }

    #[test]
    fn streaming_window_eviction_and_partial_overlap() {
        let game = ActionGame::county(42);
        let c0 = ChunkCoord::from_pos(game.player_tile().pos());
        let mut cache = SceneCache::new(2, false, CacheSyncMode::VerifyWindow);

        let sync0 = cache.sync_window(&game, c0);
        assert_eq!(cache.len(), 25);
        assert_eq!(sync0.dirty_chunks.len(), 25);
        assert!(sync0.evicted_chunks.is_empty());

        let rev_map: HashMap<ChunkCoord, u64> = sync0
            .active_chunks
            .iter()
            .map(|&c| (c, cache.revision_of(c).unwrap()))
            .collect();

        // Shift by (1, 0): 20 overlapping chunks stay unchanged, 5 enter, 5 exit
        let c1 = ChunkCoord::new(c0.cx + 1, c0.cy);
        let sync1 = cache.sync_window(&game, c1);
        assert_eq!(cache.len(), 25);
        assert_eq!(sync1.dirty_chunks.len(), 5);
        assert_eq!(sync1.evicted_chunks.len(), 5);
        for (&coord, &old_rev) in &rev_map {
            if !sync1.evicted_chunks.contains(&coord) {
                assert_eq!(cache.revision_of(coord), Some(old_rev));
            } else {
                assert!(cache.get(coord).is_none());
            }
        }

        // Shift far away: all 25 prior chunks evicted
        let c_far = ChunkCoord::new(28, 28);
        let sync2 = cache.sync_window(&game, c_far);
        assert_eq!(cache.len(), 25);
        assert_eq!(sync2.dirty_chunks.len(), 25);
        assert_eq!(sync2.evicted_chunks.len(), 25);
    }
}
