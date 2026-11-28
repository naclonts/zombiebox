use zombiebox_core::{
    ActionEvent, ActionGame, ActionMode, ActionOutcome, BuildingId, BuildingInfo, BuildingStyle,
    CHUNK_SIZE, CHUNK_VERTICES, CHUNKS_PER_SIDE, CacheView, District, FLOOR_H, Feature, Floor,
    Ground, LightSourceView, Material, MonsterView, Position, ProjectileView, PropKind, PropView,
    Surface, Tile3D, TileKind, TileRenderInfo, TileVisibility, Vec2 as SimVec2, VehicleId,
    VehicleView, WORLD_SIZE, WallOrientation, World,
};

use crate::coords::{ChunkCoord, RenderAabb, SimVec3};

pub const CHUNK_HALO_SIDE: usize = CHUNK_SIZE as usize + 2; // 18
pub const CHUNK_HALO_LEN: usize = CHUNK_HALO_SIDE * CHUNK_HALO_SIDE; // 324
pub const MAX_SNAPSHOT_FLOORS: usize = zombiebox_core::MAX_COUNTY_FLOORS;
pub const DEFAULT_VISIBILITY_HALF_EXTENT: i32 = 24; // 48x48 grid matching LightGrid

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
    /// Latest visibility sampled at snapshot time (for dynamic updates use `LocalVisibilityGrid`).
    pub visibility: TileVisibility,
    /// Latest core tile light `0..=255` (excluded from `static_fingerprint`).
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
    /// Quantized visual damage tier (`0` = intact, `1` = scratched, `2` = critical).
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
    /// `0` = out-of-world void, `1` = exterior apron (`World::tile_at` fallback), `2` = BirchStreet, `3` = County.
    pub floor_count: u8,
    pub floors: [FloorCellSnapshot; MAX_SNAPSHOT_FLOORS],
}

impl TileSnapshot {
    #[must_use]
    pub fn void(pos: Position) -> Self {
        Self {
            pos,
            in_playable_bounds: false,
            district: District::Farm,
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
    /// `true` when chunk lies inside `[0..CHUNKS_PER_SIDE)^2` of `World`.
    pub has_terrain: bool,
    /// Row-major `[ly][lx]` vertex heights (`lx, ly in 0..17`), world XY = `(cx*16 + lx - 0.5, cy*16 + ly - 0.5)`.
    pub heights: [[f32; CHUNK_VERTICES]; CHUNK_VERTICES],
    /// Row-major `[ly][lx]` upward unit normals in simulation `(x, y, z)` (`+z` up).
    pub normals: [[SimVec3; CHUNK_VERTICES]; CHUNK_VERTICES],
    pub min_z: f32,
    pub max_z: f32,
}

impl ChunkTerrainSnapshot {
    #[must_use]
    pub fn sample_from_world(world: &World, coord: ChunkCoord) -> Self {
        let has_terrain =
            (0..CHUNKS_PER_SIDE).contains(&coord.cx) && (0..CHUNKS_PER_SIDE).contains(&coord.cy);
        let mut heights = [[0.0f32; CHUNK_VERTICES]; CHUNK_VERTICES];
        let mut normals = [[SimVec3::Z; CHUNK_VERTICES]; CHUNK_VERTICES];

        if !has_terrain {
            return Self {
                coord,
                has_terrain: false,
                heights,
                normals,
                min_z: 0.0,
                max_z: 0.0,
            };
        }

        if world.is_flat() {
            return Self {
                coord,
                has_terrain: true,
                heights,
                normals,
                min_z: 0.0,
                max_z: 0.0,
            };
        }

        let c_min = coord.min_tile();
        let mut min_z = f32::INFINITY;
        let mut max_z = f32::NEG_INFINITY;

        for ly in 0..CHUNK_VERTICES {
            for lx in 0..CHUNK_VERTICES {
                let gx = c_min.x + lx as i32;
                let gy = c_min.y + ly as i32;
                let z = world.grid_height(gx, gy).unwrap_or(0.0);
                heights[ly][lx] = z;
                min_z = min_z.min(z);
                max_z = max_z.max(z);
                normals[ly][lx] = world.grid_normal(gx, gy).unwrap_or(SimVec3::Z);
            }
        }

        if !min_z.is_finite() {
            min_z = 0.0;
            max_z = 0.0;
        }

        Self {
            coord,
            has_terrain: true,
            heights,
            normals,
            min_z,
            max_z,
        }
    }

    /// Returns `(z, normal)` at local vertex `(lx, ly)` (`0..17`).
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
    /// Row-major `18 × 18` tile columns covering `lx in -1..=16, ly in -1..=16`.
    pub tiles: Box<[TileSnapshot; CHUNK_HALO_LEN]>,
    /// Buildings whose horizontal footprint intersects this chunk's `18 × 18` halo, sorted by `info.id`.
    pub buildings: Vec<BuildingSnapshot>,
    /// Non-searchable props owned by this chunk. Searchable/container props
    /// stay dynamic and are intentionally excluded from static fingerprints.
    pub static_props: Vec<PropView>,
    /// `true` if any core `16 × 16` tile has `floor_count > 0` or `terrain.has_terrain`.
    pub has_renderable_content: bool,
    pub min_sim_z: f32,
    pub max_sim_z: f32,
}

impl ChunkSnapshot {
    /// Access tile at chunk-local `(lx, ly)` where `0..=15` is interior and `-1` / `16` is the 1-tile halo.
    #[must_use]
    pub fn local_tile(&self, lx: i32, ly: i32) -> Option<&TileSnapshot> {
        if !(-1..=CHUNK_SIZE).contains(&lx) || !(-1..=CHUNK_SIZE).contains(&ly) {
            return None;
        }
        let idx = (ly + 1) as usize * CHUNK_HALO_SIDE + (lx + 1) as usize;
        Some(&self.tiles[idx])
    }

    /// Access tile by world `Position` if within this chunk's `18 × 18` footprint.
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

    /// Renderer-space AABB `(X, Y, Z)` (`+Y` up) for `Frustum::intersects_aabb` and `ChunkMeshPool`.
    #[must_use]
    pub fn bounds_render_aabb(&self) -> RenderAabb {
        self.coord.render_aabb(self.min_sim_z, self.max_sim_z)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerSnapshot {
    pub render: zombiebox_core::PlayerRenderState,
    /// Guaranteed 3D foot position (`+z` up).
    pub foot_pos: SimVec3,
    /// Guaranteed 3D unit aim direction (`+z` up).
    pub aim_dir3: SimVec3,
    /// Guaranteed 3D muzzle origin (`+z` up).
    pub muzzle_pos3: SimVec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CacheSnapshot {
    pub view: CacheView,
    /// Authoritative 3D ground/floor support position (`+z` up).
    pub world_pos: SimVec3,
    /// Upward ground normal at `world_pos` (simulation `+z` up).
    pub ground_normal: SimVec3,
    pub visibility: TileVisibility,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightSnapshot {
    pub view: LightSourceView,
    /// Authoritative 3D emitter position (`+z` up).
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

/// Smooth per-building cutaway state (`T07`) updated every frame with zero static mesh rebuilds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildingCutawayState {
    pub building_id: BuildingId,
    pub floors: u8,
    pub foundation_z: f32,
    pub player_inside: bool,
    pub wants_cutaway: bool,
    /// Smooth `0.0` (closed) -> `1.0` (cut away) over `CUTAWAY_FADE_S = 0.16`.
    pub openness: f32,
    /// Quantized 4-step `0..=4` stipple matching the isometric reference build (`0` = open, `4` = closed).
    pub stipple_step: u8,
    /// Smooth `0..=255` Bayer dither opacity (`255` = opaque roof/upper walls, `0` = cut away) for `InstanceRaw`/`T07`.
    pub dither_opacity_u8: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VisibilityCell {
    pub visibility: TileVisibility,
}

/// Compact local-window room visibility for shrouding and actor culling.
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
        if tile.floor >= self.floor_count {
            return None;
        }
        let dx = tile.x - self.origin.x;
        let dy = tile.y - self.origin.y;
        if dx < 0 || dy < 0 || dx >= self.side as i32 || dy >= self.side as i32 {
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

/// Complete dynamic frame state consumed by `T06`–`T09` without touching static chunk meshes.
#[derive(Clone, Debug, PartialEq)]
pub struct DynamicSceneState {
    pub sim_tick: u64,
    pub mode: ActionMode,
    pub seed: u64,
    pub outcome: ActionOutcome,
    pub dusk_level: f32,
    pub district: District,
    pub active_floor: Floor,
    pub player_building_id: BuildingId,
    pub player: PlayerSnapshot,
    pub vehicles: Vec<(VehicleId, VehicleView)>,
    pub mounted_vehicle: Option<VehicleId>,
    pub monsters: Vec<MonsterView>,
    pub projectiles: Vec<ProjectileView>,
    pub caches: Vec<CacheSnapshot>,
    /// Persistent furnishing/retail props. These remain dynamic instances so
    /// search/open state never invalidates resident chunk meshes.
    pub props: Vec<PropView>,
    pub lights: Vec<LightSnapshot>,
    pub dynamic_flashes: Vec<FlashSnapshot>,
    pub blasts: Vec<BlastSnapshot>,
    pub building_cutaways: Vec<BuildingCutawayState>,
    pub local_visibility: LocalVisibilityGrid,
}

/// Output of `SceneManager::prepare(...)` for one render frame.
#[derive(Clone, Debug)]
pub struct FrameSnapshot {
    pub sim_tick: u64,
    pub focus_chunk: ChunkCoord,
    /// Sorted list of all chunks currently inside the active streaming window.
    pub active_chunks: Vec<ChunkCoord>,
    /// Newly materialized or invalidated chunks whose `(coord, revision)` must be meshed & uploaded.
    pub dirty_chunks: Vec<crate::scene::cache::DirtyChunkDelta>,
    /// Chunks evicted from the active window that should be dropped from `ChunkMeshPool`.
    pub evicted_chunks: Vec<ChunkCoord>,
    pub dynamic: DynamicSceneState,
    /// Transient simulation events accumulated via `observe_tick` since the previous frame.
    pub events: Vec<ActionEvent>,
}

// --- Deterministic 64-bit FNV-1a Fingerprinting ---

struct Fnv1a64(u64);

impl Fnv1a64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;

    #[inline]
    const fn new() -> Self {
        Self(Self::OFFSET)
    }

    #[inline]
    fn write_u8(&mut self, v: u8) {
        self.0 ^= u64::from(v);
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    #[inline]
    fn write_bool(&mut self, v: bool) {
        self.write_u8(u8::from(v));
    }

    #[inline]
    fn write_i8(&mut self, v: i8) {
        self.write_u8(v as u8);
    }

    #[inline]
    fn write_u16(&mut self, v: u16) {
        for b in v.to_le_bytes() {
            self.write_u8(b);
        }
    }

    #[inline]
    fn write_u32(&mut self, v: u32) {
        for b in v.to_le_bytes() {
            self.write_u8(b);
        }
    }
    #[inline]
    fn write_u64(&mut self, v: u64) {
        self.write_u32(v as u32);
        self.write_u32((v >> 32) as u32);
    }

    #[inline]
    fn write_i32(&mut self, v: i32) {
        for b in v.to_le_bytes() {
            self.write_u8(b);
        }
    }

    #[inline]
    fn write_f32(&mut self, v: f32) {
        let norm = if v == 0.0 { 0.0 } else { v };
        self.write_u32(norm.to_bits());
    }

    #[inline]
    const fn finish(self) -> u64 {
        self.0
    }
}

#[inline]
const fn surface_discriminant(s: Surface) -> u8 {
    match s {
        Surface::Air => 0,
        Surface::Grass => 1,
        Surface::Forest => 2,
        Surface::Field => 3,
        Surface::Trail => 4,
        Surface::Road => 5,
        Surface::Sidewalk => 6,
        Surface::Driveway => 7,
        Surface::Gravel => 8,
        Surface::Bridge => 9,
        Surface::Water => 10,
        Surface::Mountain => 11,
        Surface::Porch => 12,
        Surface::WoodFloor => 13,
        Surface::KitchenTile => 14,
        Surface::Carpet => 15,
    }
}

#[inline]
const fn prop_kind_discriminant(p: PropKind) -> u8 {
    match p {
        PropKind::Table => 0,
        PropKind::Sofa => 1,
        PropKind::Counter => 2,
        PropKind::Bed => 3,
        PropKind::Crate => 4,
        PropKind::Fence => 5,
        PropKind::Mailbox => 6,
        PropKind::Bench => 7,
        PropKind::Shed => 8,
        PropKind::BirchTree => 9,
    }
}

#[inline]
fn hash_tile_kind(h: &mut Fnv1a64, kind: TileKind) {
    match kind {
        TileKind::Empty => h.write_u8(0),
        TileKind::ExteriorWall => h.write_u8(1),
        TileKind::InteriorWall => h.write_u8(2),
        TileKind::Pillar => h.write_u8(3),
        TileKind::Window => h.write_u8(4),
        TileKind::BrokenWindow => h.write_u8(5),
        TileKind::Door { open, exterior } => {
            h.write_u8(6);
            h.write_bool(open);
            h.write_bool(exterior);
        }
        TileKind::GarageDoor { open, segment } => {
            h.write_u8(14);
            h.write_bool(open);
            h.write_u8(segment);
        }
        TileKind::StairsUp => h.write_u8(7),
        TileKind::StairsDown => h.write_u8(8),
        TileKind::StairsBoth => h.write_u8(9),
        TileKind::Streetlight => h.write_u8(10),
        TileKind::BrokenStreetlight => h.write_u8(11),
        TileKind::Prop(p) => {
            h.write_u8(12);
            h.write_u8(prop_kind_discriminant(p));
        }
        TileKind::Rubble => h.write_u8(13),
    }
}

#[inline]
const fn material_discriminant(m: Material) -> u8 {
    match m {
        Material::None => 0,
        Material::Glass => 1,
        Material::WoodLight => 2,
        Material::WoodDoor => 3,
        Material::Drywall => 4,
        Material::WoodSiding => 5,
        Material::BrickMasonry => 6,
        Material::MetalPole => 7,
        Material::TreeWood => 8,
        Material::Flesh => 9,
        Material::Caustic => 10,
    }
}

#[inline]
const fn orientation_discriminant(o: WallOrientation) -> u8 {
    match o {
        WallOrientation::AlongX => 0,
        WallOrientation::AlongY => 1,
        WallOrientation::Corner => 2,
    }
}

#[inline]
const fn district_discriminant(d: District) -> u8 {
    match d {
        District::Residential => 0,
        District::City => 1,
        District::Hospital => 2,
        District::Police => 3,
        District::Farm => 4,
        District::MilitaryBase => 5,
        District::Woods => 6,
    }
}

#[inline]
const fn building_style_discriminant(s: BuildingStyle) -> u8 {
    match s {
        BuildingStyle::BirchSage => 0,
        BuildingStyle::BlueClapboard => 1,
        BuildingStyle::ColonialBrick => 2,
        BuildingStyle::WarmPutty => 3,
        BuildingStyle::DowntownStorefront => 4,
        BuildingStyle::DowntownOffice => 5,
        BuildingStyle::DowntownApartments => 6,
        BuildingStyle::HospitalClinical => 7,
        BuildingStyle::PolicePrecinct => 8,
        BuildingStyle::ArmyBarracks => 9,
        BuildingStyle::WoodlandCabin => 10,
        BuildingStyle::RuralBarn => 11,
        BuildingStyle::RanchSage => 12,
    }
}

#[inline]
fn hash_floor_cell(h: &mut Fnv1a64, cell: &FloorCellSnapshot) {
    h.write_u8(surface_discriminant(cell.surface));
    hash_tile_kind(h, cell.kind);
    h.write_u8(material_discriminant(cell.material));
    h.write_u8(cell.damage_tier());
    h.write_u16(cell.building_id);
    h.write_bool(cell.indoor);
    h.write_u8(orientation_discriminant(cell.orientation));
    h.write_i8(cell.stair_dir.0);
    h.write_i8(cell.stair_dir.1);
}

#[inline]
fn hash_tile_column(h: &mut Fnv1a64, tile: &TileSnapshot) {
    h.write_bool(tile.in_playable_bounds);
    h.write_u8(district_discriminant(tile.district));
    match tile.water_z {
        None => h.write_u8(0),
        Some(wz) => {
            h.write_u8(1);
            h.write_f32(wz);
        }
    }
    match tile.bridge_quad {
        None => h.write_u8(0),
        Some(bq) => {
            h.write_u8(1);
            for z in bq {
                h.write_f32(z);
            }
        }
    }
    h.write_u8(tile.floor_count);
    for cell in tile.floors.iter().take(tile.floor_count as usize) {
        hash_floor_cell(h, cell);
    }
}

#[inline]
fn hash_building(h: &mut Fnv1a64, b: &BuildingSnapshot) {
    h.write_u16(b.info.id);
    h.write_u32(b.info.uid.0 as u32);
    h.write_u32((b.info.uid.0 >> 32) as u32);
    h.write_i32(b.info.x);
    h.write_i32(b.info.y);
    h.write_i32(b.info.width);
    h.write_i32(b.info.height);
    h.write_u8(b.info.floors);
    h.write_u8(building_style_discriminant(b.info.style));
    h.write_i32(b.info.front_door.x);
    h.write_i32(b.info.front_door.y);
    h.write_f32(b.info.foundation_z);
    h.write_bool(b.has_pitched_roof);
    h.write_u8(b.info.wing_count);
    for w in &b.info.wings {
        h.write_i32(w.min_x);
        h.write_i32(w.min_y);
        h.write_i32(w.max_x);
        h.write_i32(w.max_y);
        h.write_u8(w.floors);
    }
}

#[inline]
const fn ground_to_surface(ground: Ground) -> Surface {
    match ground {
        Ground::Bridge => Surface::Bridge,
        Ground::River | Ground::Lake => Surface::Water,
        Ground::Mountain => Surface::Mountain,
        Ground::Street | Ground::Road => Surface::Road,
        Ground::Pavement => Surface::Sidewalk,
        Ground::Trail => Surface::Trail,
        Ground::Gravel => Surface::Gravel,
        Ground::Forest => Surface::Forest,
        Ground::Field => Surface::Field,
        Ground::Grass => Surface::Grass,
    }
}

#[inline]
fn sample_tile_snapshot(game: &ActionGame, pos: Position, max_floors: u8) -> TileSnapshot {
    if !(0..WORLD_SIZE).contains(&pos.x) || !(0..WORLD_SIZE).contains(&pos.y) {
        return TileSnapshot::void(pos);
    }

    let world = game.world();
    let district = world.district_at(pos).unwrap_or_default();
    let water_z = world.water_level_at(pos);
    let bridge_quad = world.bridge_quad(pos);

    if let Some(info0) = game.tile_render_info(pos, 0) {
        let mut floors = [FloorCellSnapshot::default(); MAX_SNAPSHOT_FLOORS];
        floors[0] = FloorCellSnapshot::from_render_info(info0);
        let mut floor_count = 1u8;
        let limit = max_floors.min(MAX_SNAPSHOT_FLOORS as u8);
        for fl in 1..limit {
            if let Some(info) = game.tile_render_info(pos, fl) {
                floors[fl as usize] = FloorCellSnapshot::from_render_info(info);
                floor_count = fl + 1;
            }
        }
        return TileSnapshot {
            pos,
            in_playable_bounds: true,
            district,
            water_z,
            bridge_quad,
            floor_count,
            floors,
        };
    }

    // Exterior apron fallback via World::tile_at
    if let Some(wt) = world.tile_at(pos) {
        let surface = ground_to_surface(wt.ground);
        let (kind, material) = match wt.feature {
            Feature::Streetlight => (TileKind::Streetlight, Material::MetalPole),
            Feature::Tree => (TileKind::Prop(PropKind::BirchTree), Material::TreeWood),
            Feature::Fence => (TileKind::Prop(PropKind::Fence), Material::WoodLight),
            Feature::Gate => (
                TileKind::Door {
                    open: true,
                    exterior: true,
                },
                Material::WoodDoor,
            ),
            _ => (TileKind::Empty, Material::None),
        };
        let mut floors = [FloorCellSnapshot::default(); MAX_SNAPSHOT_FLOORS];
        floors[0] = FloorCellSnapshot {
            surface,
            kind,
            material,
            ..FloorCellSnapshot::default()
        };
        return TileSnapshot {
            pos,
            in_playable_bounds: false,
            district,
            water_z,
            bridge_quad,
            floor_count: 1,
            floors,
        };
    }

    TileSnapshot::void(pos)
}

#[inline]
const fn building_intersects_halo(b: &BuildingInfo, c_min: Position) -> bool {
    let halo_min_x = c_min.x - 1;
    let halo_max_x = c_min.x + CHUNK_SIZE;
    let halo_min_y = c_min.y - 1;
    let halo_max_y = c_min.y + CHUNK_SIZE;
    b.x <= halo_max_x
        && b.x + b.width > halo_min_x
        && b.y <= halo_max_y
        && b.y + b.height > halo_min_y
}

#[must_use]
pub fn hash_chunk_static_state(snapshot: &ChunkSnapshot) -> u64 {
    let mut h = Fnv1a64::new();
    h.write_i32(snapshot.coord.cx);
    h.write_i32(snapshot.coord.cy);
    h.write_bool(snapshot.terrain.has_terrain);
    if snapshot.terrain.has_terrain {
        for ly in 0..CHUNK_VERTICES {
            for lx in 0..CHUNK_VERTICES {
                h.write_f32(snapshot.terrain.heights[ly][lx]);
                let n = snapshot.terrain.normals[ly][lx];
                h.write_f32(n.x);
                h.write_f32(n.y);
                h.write_f32(n.z);
            }
        }
    }
    for tile in snapshot.tiles.iter() {
        hash_tile_column(&mut h, tile);
    }
    h.write_u32(snapshot.buildings.len() as u32);
    for b in &snapshot.buildings {
        hash_building(&mut h, b);
    }
    h.write_u32(snapshot.static_props.len() as u32);
    for p in &snapshot.static_props {
        hash_static_prop(&mut h, p);
    }
    h.finish()
}

fn hash_static_prop(h: &mut Fnv1a64, p: &PropView) {
    h.write_u64(p.anchor_uid);
    h.write_u16(p.id as u16);
    h.write_i32(p.anchor.tile.x);
    h.write_i32(p.anchor.tile.y);
    h.write_u8(p.floor);
    h.write_u8(p.anchor.span.width);
    h.write_u8(p.anchor.span.height);
    h.write_u8(p.facing as u8);
    h.write_f32(p.world_pivot.x);
    h.write_f32(p.world_pivot.y);
    h.write_f32(p.world_pivot.z);
}

/// Streams chunk static state directly from `ActionGame` into FNV-1a with zero heap allocations.
#[must_use]
pub fn compute_chunk_fingerprint_from_game(game: &ActionGame, coord: ChunkCoord) -> u64 {
    let mut h = Fnv1a64::new();
    h.write_i32(coord.cx);
    h.write_i32(coord.cy);

    let world = game.world();
    let has_terrain =
        (0..CHUNKS_PER_SIDE).contains(&coord.cx) && (0..CHUNKS_PER_SIDE).contains(&coord.cy);
    h.write_bool(has_terrain);

    let c_min = coord.min_tile();
    if has_terrain {
        if world.is_flat() {
            for _ in 0..(CHUNK_VERTICES * CHUNK_VERTICES) {
                h.write_f32(0.0);
                h.write_f32(0.0);
                h.write_f32(0.0);
                h.write_f32(1.0);
            }
        } else {
            for ly in 0..CHUNK_VERTICES {
                for lx in 0..CHUNK_VERTICES {
                    let gx = c_min.x + lx as i32;
                    let gy = c_min.y + ly as i32;
                    let z = world.grid_height(gx, gy).unwrap_or(0.0);
                    let n = world.grid_normal(gx, gy).unwrap_or(SimVec3::Z);
                    h.write_f32(z);
                    h.write_f32(n.x);
                    h.write_f32(n.y);
                    h.write_f32(n.z);
                }
            }
        }
    }

    let max_floors = game.window_max_floors().min(MAX_SNAPSHOT_FLOORS as u8);

    for ly in -1..=CHUNK_SIZE {
        for lx in -1..=CHUNK_SIZE {
            let pos = Position::new(c_min.x + lx, c_min.y + ly);
            let tile = sample_tile_snapshot(game, pos, max_floors);
            hash_tile_column(&mut h, &tile);
        }
    }

    // Count & iterate intersecting buildings sorted by id with zero heap allocation
    let all_buildings = game.buildings();
    let mut count = 0u32;
    for b in all_buildings {
        if building_intersects_halo(b, c_min) {
            count += 1;
        }
    }
    h.write_u32(count);

    let mut min_next_id = 0u16;
    for _ in 0..count {
        let mut chosen: Option<&BuildingInfo> = None;
        for b in all_buildings {
            if b.id > min_next_id
                && building_intersects_halo(b, c_min)
                && chosen.is_none_or(|c| b.id < c.id)
            {
                chosen = Some(b);
            }
        }
        if let Some(b) = chosen {
            let snap = BuildingSnapshot {
                info: *b,
                has_pitched_roof: b.style.has_pitched_roof(),
            };
            hash_building(&mut h, &snap);
            min_next_id = b.id;
        }
    }

    let mut static_props: Vec<PropView> = game
        .prop_views()
        .into_iter()
        // A seam-spanning prototype belongs to its anchor's core chunk.  The
        // halo is for sampling surrounding terrain/buildings, never ownership.
        .filter(|p| p.id.def().loot.is_none() && ChunkCoord::from_pos(p.anchor.tile.pos()) == coord)
        .collect();
    static_props.sort_by_key(|p| p.anchor_uid);
    h.write_u32(static_props.len() as u32);
    for p in &static_props {
        hash_static_prop(&mut h, p);
    }

    h.finish()
}

#[must_use]
pub fn build_chunk_snapshot(game: &ActionGame, coord: ChunkCoord, revision: u64) -> ChunkSnapshot {
    let world = game.world();
    let terrain = ChunkTerrainSnapshot::sample_from_world(world, coord);
    let c_min = coord.min_tile();
    let max_floors = game.window_max_floors().min(MAX_SNAPSHOT_FLOORS as u8);

    let mut tiles = Vec::with_capacity(CHUNK_HALO_LEN);
    let mut has_renderable_content = terrain.has_terrain;
    let mut min_sim_z = terrain.min_z;
    let mut max_sim_z = terrain.max_z;

    for ly in -1..=CHUNK_SIZE {
        for lx in -1..=CHUNK_SIZE {
            let pos = Position::new(c_min.x + lx, c_min.y + ly);
            let tile = sample_tile_snapshot(game, pos, max_floors);

            let in_core = (0..CHUNK_SIZE).contains(&lx) && (0..CHUNK_SIZE).contains(&ly);
            if in_core && tile.floor_count > 0 {
                has_renderable_content = true;
            }
            if let Some(bq) = tile.bridge_quad {
                for z in bq {
                    min_sim_z = min_sim_z.min(z - 0.5);
                    max_sim_z = max_sim_z.max(z + 0.5);
                }
            }

            tiles.push(tile);
        }
    }

    let mut buildings: Vec<BuildingSnapshot> = game
        .buildings()
        .iter()
        .filter(|b| building_intersects_halo(b, c_min))
        .map(|b| BuildingSnapshot {
            info: *b,
            has_pitched_roof: b.style.has_pitched_roof(),
        })
        .collect();
    buildings.sort_by_key(|b| b.info.id);

    for b in &buildings {
        let roof_extra = if b.has_pitched_roof { 1.5 } else { 0.5 };
        let top_z = b.info.foundation_z + f32::from(b.info.floors) * FLOOR_H + roof_extra;
        min_sim_z = min_sim_z.min(b.info.foundation_z);
        max_sim_z = max_sim_z.max(top_z);
    }

    let mut static_props: Vec<PropView> = game
        .prop_views()
        .into_iter()
        .filter(|p| p.id.def().loot.is_none() && ChunkCoord::from_pos(p.anchor.tile.pos()) == coord)
        .collect();
    static_props.sort_by_key(|p| p.anchor_uid);

    let mut snap = ChunkSnapshot {
        coord,
        revision,
        static_fingerprint: 0,
        max_floors,
        terrain,
        tiles: tiles.into_boxed_slice().try_into().unwrap(),
        buildings,
        static_props,
        has_renderable_content,
        min_sim_z: min_sim_z - 0.5,
        max_sim_z: max_sim_z + 1.0,
    };
    snap.static_fingerprint = hash_chunk_static_state(&snap);
    snap
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::ChunkCoord;

    #[test]
    fn fingerprint_stream_matches_snapshot_fingerprint() {
        let res = ActionGame::residential(42);
        let p_res = ChunkCoord::from_pos(res.player_tile().pos());
        for coord in [
            p_res,
            ChunkCoord::new(0, 0),
            ChunkCoord::new(-1, -1),
            ChunkCoord::new(p_res.cx + 1, p_res.cy),
        ] {
            let snap = build_chunk_snapshot(&res, coord, 1);
            let fp = compute_chunk_fingerprint_from_game(&res, coord);
            assert_eq!(
                snap.static_fingerprint, fp,
                "residential mismatch at {coord:?}"
            );
        }

        let county = ActionGame::county(42);
        let p_county = ChunkCoord::from_pos(county.player_tile().pos());
        for coord in [
            p_county,
            ChunkCoord::new(p_county.cx - 1, p_county.cy),
            ChunkCoord::new(p_county.cx, p_county.cy + 1),
            ChunkCoord::new(-1, -1),
        ] {
            let snap = build_chunk_snapshot(&county, coord, 1);
            let fp = compute_chunk_fingerprint_from_game(&county, coord);
            assert_eq!(snap.static_fingerprint, fp, "county mismatch at {coord:?}");
        }

        // Changing visibility or core_light must NOT change static_fingerprint;
        // changing Door { open }, TileKind, damage_tier, heights, or BuildingInfo MUST change it.
        let mut snap = build_chunk_snapshot(&res, p_res, 1);
        let base_fp = snap.static_fingerprint;

        snap.tiles[0].floors[0].visibility = TileVisibility::Visible;
        snap.tiles[0].floors[0].core_light = 250;
        assert_eq!(hash_chunk_static_state(&snap), base_fp);

        let mut snap_door = snap.clone();
        snap_door.tiles[100].floors[0].kind = TileKind::Door {
            open: true,
            exterior: false,
        };
        assert_ne!(hash_chunk_static_state(&snap_door), base_fp);

        let mut snap_dmg = snap.clone();
        snap_dmg.tiles[100].floors[0].max_hp = 100;
        snap_dmg.tiles[100].floors[0].hp = 40;
        assert_ne!(hash_chunk_static_state(&snap_dmg), base_fp);

        let mut snap_h = snap.clone();
        snap_h.terrain.heights[3][4] += 0.25;
        assert_ne!(hash_chunk_static_state(&snap_h), base_fp);

        if !snap.buildings.is_empty() {
            let mut snap_b = snap.clone();
            snap_b.buildings[0].info.foundation_z += 0.5;
            assert_ne!(hash_chunk_static_state(&snap_b), base_fp);
        }
    }
}
