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

#[derive(Clone, Debug)]
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
        keys.sort_unstable_by(|a, b| (a.cy, a.cx).cmp(&(b.cy, b.cx)));
        keys
    }

    pub fn invalidate_chunk(&mut self, coord: ChunkCoord) {
        self.pending_invalidations.insert(coord);
    }

    pub fn invalidate_tile_with_halo(&mut self, pos: Position) {
        let (coords, count) = halo_affected_chunks(pos);
        for i in 0..count {
            self.pending_invalidations.insert(coords[i]);
        }
    }

    pub fn invalidate_tile3d_with_halo(&mut self, tile: Tile3D) {
        self.invalidate_tile_with_halo(tile.pos());
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
        self.pending_invalidations.clear();
        self.next_revision = 0;
    }

    #[must_use]
    pub fn compute_active_window(&self, focus_chunk: ChunkCoord) -> Vec<ChunkCoord> {
        let r = self.active_chunk_radius;
        let mut coords = Vec::new();
        for cy in (focus_chunk.cy - r)..=(focus_chunk.cy + r) {
            for cx in (focus_chunk.cx - r)..=(focus_chunk.cx + r) {
                let coord = ChunkCoord::new(cx, cy);
                if self.clamp_to_world_bounds {
                    if cx >= 0 && cx < CHUNKS_PER_SIDE && cy >= 0 && cy < CHUNKS_PER_SIDE {
                        coords.push(coord);
                    }
                } else {
                    coords.push(coord);
                }
            }
        }
        coords.sort_unstable_by(|a, b| (a.cy, a.cx).cmp(&(b.cy, b.cx)));
        coords
    }

    pub fn sync_window(&mut self, game: &ActionGame, focus_chunk: ChunkCoord) -> CacheSyncResult {
        let active_chunks = self.compute_active_window(focus_chunk);
        let active_set: HashSet<ChunkCoord> = active_chunks.iter().copied().collect();

        let mut evicted_chunks = Vec::new();
        let resident_coords: Vec<_> = self.chunks.keys().copied().collect();
        for coord in resident_coords {
            if !active_set.contains(&coord) {
                self.chunks.remove(&coord);
                self.pending_invalidations.remove(&coord);
                evicted_chunks.push(coord);
            }
        }
        evicted_chunks.sort_unstable_by(|a, b| (a.cy, a.cx).cmp(&(b.cy, b.cx)));

        let mut dirty_chunks = Vec::new();

        for coord in &active_chunks {
            let mut rebuild = false;
            let mut existing_rev = 0u64;

            if self.pending_invalidations.remove(coord) {
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
