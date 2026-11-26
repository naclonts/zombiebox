use std::sync::Arc;

use zombiebox_core::{
    ActionGame, BuildingId, BuildingInfo, CHUNK_SIZE, CHUNK_VERTICES, CacheView, District, Floor,
    LightSourceView, Material, MonsterView, Position, ProjectileView, Surface, Tile3D, TileKind,
    TileRenderInfo, TileVisibility, WallOrientation, World,
};

use crate::coords::{ChunkCoord, RenderAabb, SimVec2, SimVec3};

pub const CHUNK_HALO_SIDE: usize = CHUNK_SIZE as usize + 2; // 18
pub const CHUNK_HALO_LEN: usize = CHUNK_HALO_SIDE * CHUNK_HALO_SIDE; // 324
pub const MAX_SNAPSHOT_FLOORS: usize = 3;
pub const DEFAULT_VISIBILITY_HALF_EXTENT: i32 = 24;

/// Per-(tile, floor) static + dynamic metadata sampled from `ActionGame`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloorCellSnapshot {
    pub surface: Surface,
    pub kind: TileKind,
    pub material: Material,
    pub hp: u16,
    pub max_hp: u16,
    pub building_id: BuildingId,
    pub indoor: bool,
    pub orientation: WallOrientation,
    pub stair_dir: (i8, i8),
    pub visibility: TileVisibility,
    pub core_light: u8,
}

impl Default for FloorCellSnapshot {
    fn default() -> Self {
        Self {
            surface: Surface::Air,
            kind: TileKind::Empty,
            material: Material::None,
            hp: 0,
            max_hp: 0,
            building_id: 0,
            indoor: false,
            orientation: WallOrientation::AlongX,
            stair_dir: (0, 0),
            visibility: TileVisibility::Hidden,
            core_light: 0,
        }
    }
}

impl FloorCellSnapshot {
    #[must_use]
    pub const fn damage_tier(self) -> u8 {
        if self.max_hp == 0 || self.hp >= self.max_hp {
            0
        } else if self.hp.saturating_mul(2) >= self.max_hp {
            1
        } else {
            2
        }
    }

    #[must_use]
    pub const fn from_render_info(info: TileRenderInfo) -> Self {
        Self {
            surface: info.surface,
            kind: info.kind,
            material: info.material,
            hp: info.hp,
            max_hp: info.max_hp,
            building_id: info.building_id,
            indoor: info.indoor,
            orientation: info.orientation,
            stair_dir: info.stair_dir,
            visibility: info.visibility,
            core_light: info.light,
        }
    }
}

/// One horizontal `(x, y)` column inside a chunk or its 1-tile perimeter halo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileSnapshot {
    pub pos: Position,
    pub in_playable_bounds: bool,
    pub district: District,
    pub water_z: Option<f32>,
    pub bridge_quad: Option<[f32; 4]>,
    pub floor_count: u8,
    pub floors: [FloorCellSnapshot; MAX_SNAPSHOT_FLOORS],
}

impl TileSnapshot {
    #[must_use]
    pub fn void(pos: Position) -> Self {
        Self {
            pos,
            in_playable_bounds: false,
            district: District::Rural,
            water_z: None,
            bridge_quad: None,
            floor_count: 0,
            floors: [FloorCellSnapshot::default(); MAX_SNAPSHOT_FLOORS],
        }
    }

    #[must_use]
    pub fn floor(&self, fl: Floor) -> Option<&FloorCellSnapshot> {
        ((fl as usize) < self.floor_count as usize).then(|| &self.floors[fl as usize])
    }

    #[must_use]
    pub fn ground_surface(&self) -> Surface {
        if self.floor_count > 0 {
            self.floors[0].surface
        } else {
            Surface::Air
        }
    }
}

/// `17 × 17` continuous terrain elevation & smooth normal grid for one `16 × 16` chunk.
#[derive(Clone, Debug, PartialEq)]
pub struct ChunkTerrainSnapshot {
    pub coord: ChunkCoord,
    pub has_terrain: bool,
    pub heights: [[f32; CHUNK_VERTICES]; CHUNK_VERTICES],
    pub normals: [[SimVec3; CHUNK_VERTICES]; CHUNK_VERTICES],
    pub min_z: f32,
    pub max_z: f32,
}

impl ChunkTerrainSnapshot {
    #[must_use]
    pub fn sample_from_world(world: &World, coord: ChunkCoord) -> Self {
        let (min_p, max_p, floors) = world.bounds();
        let _ = floors;
        let c_min = coord.min_tile();
        let mut has_terrain = false;
        let mut heights = [[0.0f32; CHUNK_VERTICES]; CHUNK_VERTICES];
        let mut normals = [[SimVec3::Z; CHUNK_VERTICES]; CHUNK_VERTICES];
        let mut min_z = f32::MAX;
        let mut max_z = f32::MIN;

        for ly in 0..CHUNK_VERTICES {
            for lx in 0..CHUNK_VERTICES {
                let gx = c_min.x + lx as i32;
                let gy = c_min.y + ly as i32;
                let in_bounds = gx >= min_p.x && gx <= max_p.x && gy >= min_p.y && gy <= max_p.y;
                if in_bounds {
                    has_terrain = true;
                }
                let xy = SimVec2::new(gx as f32 - 0.5, gy as f32 - 0.5);
                let z = world.elevation_at(xy).unwrap_or(0.0);
                heights[ly][lx] = z;
                if z < min_z {
                    min_z = z;
                }
                if z > max_z {
                    max_z = z;
                }

                let eps = 0.5;
                let z_l = world
                    .elevation_at(xy + SimVec2::new(-eps, 0.0))
                    .unwrap_or(z);
                let z_r = world.elevation_at(xy + SimVec2::new(eps, 0.0)).unwrap_or(z);
                let z_d = world
                    .elevation_at(xy + SimVec2::new(0.0, -eps))
                    .unwrap_or(z);
                let z_u = world.elevation_at(xy + SimVec2::new(0.0, eps)).unwrap_or(z);
                let tangent_x = SimVec3::new(2.0 * eps, 0.0, z_r - z_l);
                let tangent_y = SimVec3::new(0.0, 2.0 * eps, z_u - z_d);
                let norm = tangent_y.cross(tangent_x).normalize_or_zero();
                normals[ly][lx] = if norm.z.is_finite() && norm.length_squared() > 0.01 {
                    norm
                } else {
                    SimVec3::Z
                };
            }
        }

        if !has_terrain {
            min_z = 0.0;
            max_z = 0.0;
        }

        Self {
            coord,
            has_terrain,
            heights,
            normals,
            min_z,
            max_z,
        }
    }

    #[must_use]
    pub fn vertex(&self, lx: usize, ly: usize) -> (f32, SimVec3) {
        (self.heights[ly][lx], self.normals[ly][lx])
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildingSnapshot {
    pub info: BuildingInfo,
    pub has_pitched_roof: bool,
}

/// Self-contained static snapshot of a single `16 × 16` chunk + 1-tile perimeter halo.
#[derive(Clone, Debug, PartialEq)]
pub struct ChunkSnapshot {
    pub coord: ChunkCoord,
    pub revision: u64,
    pub static_fingerprint: u64,
    pub max_floors: u8,
    pub terrain: ChunkTerrainSnapshot,
    pub tiles: Box<[TileSnapshot; CHUNK_HALO_LEN]>,
    pub buildings: Vec<BuildingSnapshot>,
    pub has_renderable_content: bool,
    pub min_sim_z: f32,
    pub max_sim_z: f32,
}

impl ChunkSnapshot {
    #[must_use]
    pub fn local_tile(&self, lx: i32, ly: i32) -> Option<&TileSnapshot> {
        if !(-1..=CHUNK_SIZE).contains(&lx) || !(-1..=CHUNK_SIZE).contains(&ly) {
            return None;
        }
        let idx = (ly + 1) as usize * CHUNK_HALO_SIDE + (lx + 1) as usize;
        Some(&self.tiles[idx])
    }

    #[must_use]
    pub fn world_tile(&self, pos: Position) -> Option<&TileSnapshot> {
        let min = self.coord.min_tile();
        self.local_tile(pos.x - min.x, pos.y - min.y)
    }

    #[must_use]
    pub fn building(&self, id: BuildingId) -> Option<&BuildingSnapshot> {
        if id == 0 {
            return None;
        }
        self.buildings
            .binary_search_by_key(&id, |b| b.info.id)
            .ok()
            .map(|i| &self.buildings[i])
    }

    #[must_use]
    pub fn bounds_render_aabb(&self) -> RenderAabb {
        self.coord.render_aabb(self.min_sim_z, self.max_sim_z)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerSnapshot {
    pub render: zombiebox_core::PlayerRenderState,
    pub foot_pos: SimVec3,
    pub aim_dir3: SimVec3,
    pub muzzle_pos3: SimVec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CacheSnapshot {
    pub view: CacheView,
    pub world_pos: SimVec3,
    pub visibility: TileVisibility,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightSnapshot {
    pub view: LightSourceView,
    pub position: SimVec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlashSnapshot {
    pub position: SimVec3,
    pub floor: Floor,
    pub radius: f32,
    pub intensity: u8,
    pub ttl_ticks: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlastSnapshotKind {
    Rocket,
    BloaterCaustic,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlastSnapshot {
    pub id: u32,
    pub kind: BlastSnapshotKind,
    pub position: SimVec3,
    pub floor: Floor,
    pub radius: f32,
    pub progress: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildingCutawayState {
    pub building_id: BuildingId,
    pub floors: u8,
    pub foundation_z: f32,
    pub player_inside: bool,
    pub wants_cutaway: bool,
    pub openness: f32,
    pub stipple_step: u8,
    pub dither_opacity_u8: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VisibilityCell {
    pub visibility: TileVisibility,
    pub core_light: u8,
    pub building_id: BuildingId,
    pub indoor: bool,
}

impl Default for VisibilityCell {
    fn default() -> Self {
        Self {
            visibility: TileVisibility::Hidden,
            core_light: 0,
            building_id: 0,
            indoor: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalVisibilityGrid {
    pub origin: Position,
    pub side: usize,
    pub floor_count: u8,
    pub cells: Vec<VisibilityCell>,
}

impl LocalVisibilityGrid {
    #[must_use]
    pub fn sample(&self, tile: Tile3D) -> Option<VisibilityCell> {
        let dx = tile.x - self.origin.x;
        let dy = tile.y - self.origin.y;
        if dx < 0 || dy < 0 || (dx as usize) >= self.side || (dy as usize) >= self.side {
            return None;
        }
        if (tile.floor as usize) >= self.floor_count as usize {
            return None;
        }
        let idx = (tile.floor as usize) * (self.side * self.side)
            + (dy as usize) * self.side
            + (dx as usize);
        self.cells.get(idx).copied()
    }

    #[must_use]
    pub fn visibility_at(&self, tile: Tile3D) -> TileVisibility {
        self.sample(tile)
            .map_or(TileVisibility::Hidden, |c| c.visibility)
    }

    #[must_use]
    pub fn is_actor_visible(&self, xy: SimVec2, floor: Floor) -> bool {
        let t = Tile3D::from_vec2(xy, floor);
        self.visibility_at(t) == TileVisibility::Visible
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DynamicSceneState {
    pub sim_tick: u64,
    pub mode: zombiebox_core::ActionMode,
    pub seed: u64,
    pub outcome: zombiebox_core::ActionOutcome,
    pub dusk_level: f32,
    pub district: District,
    pub active_floor: Floor,
    pub player_building_id: BuildingId,
    pub player: PlayerSnapshot,
    pub monsters: Vec<MonsterView>,
    pub projectiles: Vec<ProjectileView>,
    pub caches: Vec<CacheSnapshot>,
    pub lights: Vec<LightSnapshot>,
    pub dynamic_flashes: Vec<FlashSnapshot>,
    pub blasts: Vec<BlastSnapshot>,
    pub building_cutaways: Vec<BuildingCutawayState>,
    pub local_visibility: LocalVisibilityGrid,
}

#[derive(Clone, Debug)]
pub struct FrameSnapshot {
    pub sim_tick: u64,
    pub focus_chunk: ChunkCoord,
    pub active_chunks: Vec<ChunkCoord>,
    pub dirty_chunks: Vec<DirtyChunkDelta>,
    pub evicted_chunks: Vec<ChunkCoord>,
    pub dynamic: DynamicSceneState,
    pub events: Vec<zombiebox_core::ActionEvent>,
}

use crate::scene::cache::DirtyChunkDelta;

pub struct Fnv1a64 {
    state: u64,
}

impl Fnv1a64 {
    pub const OFFSET: u64 = 0xcbf29ce484222325;
    pub const PRIME: u64 = 0x100000001b3;

    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: Self::OFFSET,
        }
    }

    pub const fn write_u8(mut self, b: u8) -> Self {
        self.state ^= b as u64;
        self.state = self.state.wrapping_mul(Self::PRIME);
        self
    }

    pub const fn write_u64(mut self, v: u64) -> Self {
        let bytes = v.to_le_bytes();
        let mut i = 0;
        while i < 8 {
            self = self.write_u8(bytes[i]);
            i += 1;
        }
        self
    }

    pub const fn write_f32(self, v: f32) -> Self {
        let norm = if v == 0.0 { 0.0 } else { v };
        self.write_u64(norm.to_bits() as u64)
    }

    #[must_use]
    pub const fn finish(self) -> u64 {
        self.state
    }
}

#[must_use]
pub fn hash_chunk_static_state(snapshot: &ChunkSnapshot) -> u64 {
    let mut h = Fnv1a64::new();
    h = h.write_u64(snapshot.coord.cx as u64);
    h = h.write_u64(snapshot.coord.cy as u64);
    h = h.write_u64(snapshot.terrain.has_terrain as u64);
    for row in &snapshot.terrain.heights {
        for &z in row {
            h = h.write_f32(z);
        }
    }
    for tile in snapshot.tiles.iter() {
        h = h.write_u64(tile.pos.x as u64);
        h = h.write_u64(tile.pos.y as u64);
        h = h.write_u64(tile.floor_count as u64);
        for fl in &tile.floors {
            h = h.write_u8(fl.surface as u8);
            h = h.write_u8(fl.kind as u8);
            h = h.write_u8(fl.material as u8);
            h = h.write_u64(fl.hp as u64);
            h = h.write_u64(fl.max_hp as u64);
            h = h.write_u64(fl.building_id as u64);
            h = h.write_u64(fl.indoor as u64);
            h = h.write_u8(fl.orientation as u8);
            h = h.write_u8(fl.stair_dir.0 as u8);
            h = h.write_u8(fl.stair_dir.1 as u8);
        }
    }
    for b in &snapshot.buildings {
        h = h.write_u64(b.info.id as u64);
        h = h.write_u64(b.info.x as u64);
        h = h.write_u64(b.info.y as u64);
        h = h.write_u64(b.info.width as u64);
        h = h.write_u64(b.info.height as u64);
        h = h.write_u8(b.info.floors);
        h = h.write_u8(b.has_pitched_roof as u8);
    }
    h.finish()
}

#[must_use]
pub fn compute_chunk_fingerprint_from_game(game: &ActionGame, coord: ChunkCoord) -> u64 {
    let snap = build_chunk_snapshot(game, coord, 1);
    snap.static_fingerprint
}

#[must_use]
pub fn build_chunk_snapshot(game: &ActionGame, coord: ChunkCoord, revision: u64) -> ChunkSnapshot {
    let world = game.world();
    let terrain = ChunkTerrainSnapshot::sample_from_world(world, coord);
    let c_min = coord.min_tile();

    let mut tiles = Vec::with_capacity(CHUNK_HALO_LEN);
    let mut max_floors = 0u8;
    let mut has_renderable_content = terrain.has_terrain;
    let mut min_sim_z = terrain.min_z;
    let mut max_sim_z = terrain.max_z;

    for ly in -1..=CHUNK_SIZE {
        for lx in -1..=CHUNK_SIZE {
            let tx = c_min.x + lx;
            let ty = c_min.y + ly;
            let pos = Position::new(tx, ty);
            let (min_p, max_p, world_floors) = world.bounds();
            let in_bounds = tx >= min_p.x && tx <= max_p.x && ty >= min_p.y && ty <= max_p.y;

            if !in_bounds {
                tiles.push(TileSnapshot::void(pos));
                continue;
            }

            let district = world.district_at(pos);
            let water_z = world.water_elevation_at(pos);
            let bridge_quad = world.bridge_quad_at(pos);
            let fc = world_floors as u8;

            let mut floors = [FloorCellSnapshot::default(); MAX_SNAPSHOT_FLOORS];
            let mut floor_count = 0u8;
            for fl in 0..(fc.min(MAX_SNAPSHOT_FLOORS as u8)) {
                let tile3d = Tile3D::new(tx, ty, fl);
                if let Some(info) = world.tile_render_info(tile3d) {
                    let cell = FloorCellSnapshot::from_render_info(info);
                    floors[fl as usize] = cell;
                    floor_count = fl + 1;
                    if cell.surface != Surface::Air || cell.kind != TileKind::Empty {
                        if lx >= 0 && lx < CHUNK_SIZE && ly >= 0 && ly < CHUNK_SIZE {
                            has_renderable_content = true;
                        }
                    }
                }
            }

            if floor_count > max_floors {
                max_floors = floor_count;
            }

            tiles.push(TileSnapshot {
                pos,
                in_playable_bounds: in_bounds,
                district,
                water_z,
                bridge_quad,
                floor_count,
                floors,
            });
        }
    }

    let mut buildings = Vec::new();
    for &b in game.buildings() {
        let b_min_x = b.x;
        let b_min_y = b.y;
        let b_max_x = b.x + b.width - 1;
        let b_max_y = b.y + b.height - 1;

        let c_bounds_min = c_min;
        let c_bounds_max = coord.max_tile_inclusive();

        if b_max_x >= c_bounds_min.x
            && b_min_x <= c_bounds_max.x
            && b_max_y >= c_bounds_min.y
            && b_min_y <= c_bounds_max.y
        {
            buildings.push(BuildingSnapshot {
                info: b,
                has_pitched_roof: b.style.has_pitched_roof(),
            });
            let top_z = b.foundation_z + (b.floors as f32) * zombiebox_core::FLOOR_H + 2.0;
            if top_z > max_sim_z {
                max_sim_z = top_z;
            }
            if b.foundation_z < min_sim_z {
                min_sim_z = b.foundation_z;
            }
        }
    }
    buildings.sort_by_key(|b| b.info.id);

    let mut dummy = ChunkSnapshot {
        coord,
        revision,
        static_fingerprint: 0,
        max_floors,
        terrain,
        tiles: tiles.into_boxed_slice().try_into().unwrap(),
        buildings,
        has_renderable_content,
        min_sim_z,
        max_sim_z,
    };
    dummy.static_fingerprint = hash_chunk_static_state(&dummy);
    dummy
}
