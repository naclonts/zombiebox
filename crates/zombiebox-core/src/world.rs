//! Immutable, seed-generated county baseline. A chunk is a window onto the
//! same plan regardless of the order in which a caller visits its neighbors.
mod dressing;
mod geography;
mod parcels;
use parcels::{ParcelTiles, Parcels};
mod relief;
mod sites;
pub use sites::*;

use crate::spatial::{
    CHUNK_VERTICES, TerrainGrid, TerrainSample, TerrainTriangle, Vec2, Vec3, WATER_SURFACE_Z,
    sample_quad_00_11,
};
use std::sync::Arc;

pub const CHUNK_SIZE: i32 = 16;
pub const CHUNKS_PER_SIDE: i32 = 32;
pub const WORLD_SIZE: i32 = CHUNK_SIZE * CHUNKS_PER_SIDE;
pub const GENERATOR_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

impl Position {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub(crate) fn distance(self, other: Self) -> u32 {
        self.x.abs_diff(other.x) + self.y.abs_diff(other.y)
    }

    fn index(self) -> Option<usize> {
        ((0..WORLD_SIZE).contains(&self.x) && (0..WORLD_SIZE).contains(&self.y))
            .then(|| (self.y * WORLD_SIZE + self.x) as usize)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum District {
    Residential,
    City,
    Hospital,
    Police,
    #[default]
    Farm,
    MilitaryBase,
    Woods,
}

impl District {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Residential => "Residential",
            Self::City => "Downtown",
            Self::Hospital => "Hospital",
            Self::Police => "Police",
            Self::Farm => "Countryside",
            Self::MilitaryBase => "Army Base",
            Self::Woods => "Woods",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum Ground {
    Road,
    Street,
    Trail,
    Bridge,
    #[default]
    Grass,
    Pavement,
    Field,
    Gravel,
    Forest,
    Mountain,
    River,
    Lake,
}

impl Ground {
    pub fn is_road(self) -> bool {
        matches!(self, Self::Road | Self::Street | Self::Trail | Self::Bridge)
    }

    pub fn walkable(self) -> bool {
        !matches!(self, Self::Mountain | Self::River | Self::Lake)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Water {
    River,
    Lake,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct Tile {
    pub ground: Ground,
    pub district: District,
    pub elevation: u8,
    /// Original water body, including the channel beneath bridge decks.
    pub water: Option<Water>,
    pub feature: Feature,
    /// Initial ground-floor cache. Upper floors are recorded in the site's loot.
    pub cache: bool,
}

impl Tile {
    pub fn walkable(self) -> bool {
        self.ground.walkable() && self.feature.walkable()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LandmarkKind {
    Residential,
    Downtown,
    Hospital,
    Police,
    ArmyBase,
    Cabin,
    Lake,
    Mountain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Landmark {
    pub kind: LandmarkKind,
    pub entrance: Position,
}

/// Inclusive parcel with a designated public frontage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Lot {
    min: Position,
    max: Position,
    district: District,
    front: Position,
}

/// Buildings intersecting this chunk, and spawn sites owned by it on any floor.
#[derive(Debug, Default)]
pub struct ChunkSites<'a> {
    pub buildings: Vec<&'a Building>,
    pub loot: Vec<&'a LootSite>,
    pub infected: Vec<&'a InfectedSite>,
}

#[derive(Debug)]
struct County {
    tiles: Vec<Tile>,
    landmarks: Vec<Landmark>,
    parcels: Parcels,
    terrain: TerrainGrid,
    spawn: Position,
}

#[derive(Clone, Debug)]
pub struct World {
    seed: u64,
    // The original encounter uses its authored grid as a low-cost backdrop.
    // Only the two fixed encounter constructors use this variant.
    county: Option<Arc<County>>,
}

impl World {
    pub fn new(seed: u64) -> Self {
        let mut tiles = vec![Tile::default(); (WORLD_SIZE * WORLD_SIZE) as usize];
        let (landmarks, lots, valley_meta) = geography::generate_with_meta(seed, &mut tiles);
        let mut parcels = Parcels::new(lots);
        let planned = parcels.planned_pads(seed, &tiles);
        let pads: Vec<_> = planned.iter().map(|(_, pad)| *pad).collect();
        let (terrain, foundations) = relief::generate(seed, &tiles, &valley_meta, &pads);
        for ((parcel_idx, _), foundation_z) in planned.into_iter().zip(foundations) {
            parcels.set_foundation(parcel_idx, foundation_z);
        }
        let spawn = landmarks
            .iter()
            .find(|l| l.kind == LandmarkKind::Residential)
            .expect("county has a residential entrance")
            .entrance;
        Self {
            seed,
            county: Some(Arc::new(County {
                tiles,
                landmarks,
                parcels,
                terrain,
                spawn,
            })),
        }
    }

    pub fn birch_street(seed: u64) -> Self {
        Self { seed, county: None }
    }

    #[inline]
    pub fn is_flat(&self) -> bool {
        self.county.is_none()
    }

    fn resolve_cell(xy: Vec2) -> Option<(Position, f32, f32)> {
        let max = WORLD_SIZE as f32 - 0.5;
        if !xy.x.is_finite()
            || !xy.y.is_finite()
            || xy.x < -0.5
            || xy.x > max
            || xy.y < -0.5
            || xy.y > max
        {
            return None;
        }
        let gx_f = xy.x + 0.5;
        let gy_f = xy.y + 0.5;
        let tx = (gx_f.floor() as i32).clamp(0, WORLD_SIZE - 1);
        let ty = (gy_f.floor() as i32).clamp(0, WORLD_SIZE - 1);
        let u = (gx_f - tx as f32).clamp(0.0, 1.0);
        let v = (gy_f - ty as f32).clamp(0.0, 1.0);
        Some((Position::new(tx, ty), u, v))
    }

    pub fn grid_height(&self, gx: i32, gy: i32) -> Option<f32> {
        if !(0..=WORLD_SIZE).contains(&gx) || !(0..=WORLD_SIZE).contains(&gy) {
            return None;
        }
        Some(self.county.as_ref().map_or(0.0, |c| {
            c.terrain
                .grid_height(gx, gy)
                .expect("validated grid vertex")
        }))
    }

    pub fn grid_normal(&self, gx: i32, gy: i32) -> Option<Vec3> {
        if !(0..=WORLD_SIZE).contains(&gx) || !(0..=WORLD_SIZE).contains(&gy) {
            return None;
        }
        Some(self.county.as_ref().map_or(Vec3::Z, |c| {
            c.terrain
                .grid_normal(gx, gy)
                .expect("validated grid vertex")
        }))
    }

    pub fn terrain_quad(&self, p: Position) -> Option<[f32; 4]> {
        p.index()?;
        Some(
            self.county
                .as_ref()
                .map_or([0.0; 4], |c| c.terrain.quad(p).expect("validated tile")),
        )
    }

    pub fn water_level_at(&self, p: Position) -> Option<f32> {
        let idx = p.index()?;
        let c = self.county.as_ref()?;
        c.tiles[idx].water.map(|_| WATER_SURFACE_Z)
    }

    pub fn terrain_at(&self, xy: Vec2) -> Option<TerrainSample> {
        let (tile, u, v) = Self::resolve_cell(xy)?;
        let Some(county) = &self.county else {
            let triangle = if u >= v {
                TerrainTriangle::LowerRight
            } else {
                TerrainTriangle::UpperLeft
            };
            return Some(TerrainSample {
                z: 0.0,
                normal: Vec3::Z,
                tile,
                water_z: None,
                triangle,
            });
        };
        let [z00, z10, z01, z11] = county.terrain.quad(tile)?;
        let (z, normal, triangle) = sample_quad_00_11(z00, z10, z01, z11, u, v);
        Some(TerrainSample {
            z,
            normal,
            tile,
            water_z: self.water_level_at(tile),
            triangle,
        })
    }

    #[inline]
    pub fn elevation_at(&self, xy: Vec2) -> Option<f32> {
        self.terrain_at(xy).map(|s| s.z)
    }

    #[inline]
    pub fn normal_at(&self, xy: Vec2) -> Option<Vec3> {
        self.terrain_at(xy).map(|s| s.normal)
    }

    #[inline]
    pub fn bridge_quad(&self, p: Position) -> Option<[f32; 4]> {
        self.county.as_ref()?.terrain.bridge_quad(p)
    }

    pub fn bridge_deck_at(&self, xy: Vec2) -> Option<TerrainSample> {
        let (tile, u, v) = Self::resolve_cell(xy)?;
        let [z00, z10, z01, z11] = self.bridge_quad(tile)?;
        let (z, normal, triangle) = sample_quad_00_11(z00, z10, z01, z11, u, v);
        Some(TerrainSample {
            z,
            normal,
            tile,
            water_z: self.water_level_at(tile),
            triangle,
        })
    }

    /// Row-major `17 x 17` chunk corner elevations, indexed `[ly][lx]`.
    pub fn chunk_heights(
        &self,
        cx: i32,
        cy: i32,
    ) -> Option<[[f32; CHUNK_VERTICES]; CHUNK_VERTICES]> {
        if !(0..CHUNKS_PER_SIDE).contains(&cx) || !(0..CHUNKS_PER_SIDE).contains(&cy) {
            return None;
        }
        Some(std::array::from_fn(|ly| {
            let gy = cy * CHUNK_SIZE + ly as i32;
            std::array::from_fn(|lx| {
                self.grid_height(cx * CHUNK_SIZE + lx as i32, gy)
                    .expect("validated chunk vertex")
            })
        }))
    }

    /// Row-major `17 x 17` chunk corner smooth normals, indexed `[ly][lx]`.
    pub fn chunk_normals(
        &self,
        cx: i32,
        cy: i32,
    ) -> Option<[[Vec3; CHUNK_VERTICES]; CHUNK_VERTICES]> {
        if !(0..CHUNKS_PER_SIDE).contains(&cx) || !(0..CHUNKS_PER_SIDE).contains(&cy) {
            return None;
        }
        Some(std::array::from_fn(|ly| {
            let gy = cy * CHUNK_SIZE + ly as i32;
            std::array::from_fn(|lx| {
                self.grid_normal(cx * CHUNK_SIZE + lx as i32, gy)
                    .expect("validated chunk vertex")
            })
        }))
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn spatial(&self) -> crate::spatial::WorldSpatialView<'_> {
        crate::spatial::WorldSpatialView::new(self)
    }

    pub fn wake_position(&self) -> Position {
        self.county
            .as_ref()
            .map_or_else(|| self.fixture_cache(8, 16), |c| c.spawn)
    }

    pub fn landmarks(&self) -> &[Landmark] {
        self.county.as_ref().map_or(&[], |c| &c.landmarks)
    }

    pub fn sites(&self) -> Option<&Sites> {
        self.county
            .as_ref()
            .map(|c| c.parcels.sites(self.seed, &c.tiles))
    }

    /// Resolve only the parcels intersecting this 16 × 16 chunk. Individual
    /// spawn sites belong to exactly one chunk, even when a building spans several.
    pub fn chunk_sites(&self, cx: i32, cy: i32) -> Option<ChunkSites<'_>> {
        if !(0..CHUNKS_PER_SIDE).contains(&cx) || !(0..CHUNKS_PER_SIDE).contains(&cy) {
            return None;
        }
        let min = Position::new(cx * CHUNK_SIZE, cy * CHUNK_SIZE);
        let max = Position::new(min.x + CHUNK_SIZE - 1, min.y + CHUNK_SIZE - 1);
        Some(self.county.as_ref().map_or_else(ChunkSites::default, |c| {
            c.parcels.chunk_sites(min, max, self.seed, &c.tiles)
        }))
    }

    pub fn walkable_at(&self, p: Position, floor: u8) -> bool {
        if floor == 0 {
            return self.tile_at(p).is_some_and(Tile::walkable);
        }
        p.index()
            .and_then(|i| {
                let c = self.county.as_ref()?;
                c.parcels.feature_at(p, i, floor, self.seed, &c.tiles)
            })
            .is_some_and(|f| f != Feature::Empty && f.walkable())
    }

    pub fn district_at(&self, p: Position) -> Option<District> {
        let i = p.index()?;
        Some(
            self.county
                .as_ref()
                .map_or(District::Residential, |c| c.tiles[i].district),
        )
    }

    pub fn tile_at(&self, p: Position) -> Option<Tile> {
        let index = p.index()?;
        if let Some(county) = &self.county {
            return Some(county.parcels.tile_at(p, index, self.seed, &county.tiles));
        }
        // Preserve the original residential fixture's road alignment and scatter
        // across the native camera apron as well as the headless encounter.
        let ground = if p.x % CHUNK_SIZE == 8 || p.y % CHUNK_SIZE == 8 {
            Ground::Road
        } else if fixture_hash(self.seed, p.x, p.y).is_multiple_of(8) {
            Ground::Gravel
        } else {
            Ground::Grass
        };
        Some(Tile {
            ground,
            district: District::Residential,
            cache: p == self.fixture_cache(p.x / CHUNK_SIZE, p.y / CHUNK_SIZE),
            ..Tile::default()
        })
    }

    /// Row-major baseline tiles, indexed by local y then local x.
    pub fn chunk(
        &self,
        cx: i32,
        cy: i32,
    ) -> Option<[[Tile; CHUNK_SIZE as usize]; CHUNK_SIZE as usize]> {
        if !(0..CHUNKS_PER_SIDE).contains(&cx) || !(0..CHUNKS_PER_SIDE).contains(&cy) {
            return None;
        }
        Some(std::array::from_fn(|y| {
            std::array::from_fn(|x| {
                self.tile_at(Position::new(
                    cx * CHUNK_SIZE + x as i32,
                    cy * CHUNK_SIZE + y as i32,
                ))
                .expect("validated chunk coordinates")
            })
        }))
    }

    fn fixture_cache(&self, cx: i32, cy: i32) -> Position {
        let offset = 1 + (fixture_hash(self.seed, cx, cy) % 14) as i32;
        Position::new(cx * CHUNK_SIZE + offset, cy * CHUNK_SIZE + 8)
    }
}

fn fixture_hash(seed: u64, x: i32, y: i32) -> u64 {
    let packed = (u64::from(x as u32) << 32) | u64::from(y as u32);
    crate::mix64(seed ^ packed)
}

fn noise(seed: u64, domain: u64, x: i32, y: i32) -> u64 {
    let stream = crate::mix64(domain.wrapping_add(u64::from(GENERATOR_VERSION) << 32));
    fixture_hash(seed ^ stream, x, y)
}
