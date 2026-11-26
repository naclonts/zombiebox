//! The county fixes all frontages and template choices before a chunk is
//! visited. Neighbors sharing a building resolve the same immutable parcel.
use super::{District, Lot, Position, Sites, Tile, WORLD_SIZE, dressing, sites};
use std::ops::{Index, IndexMut};
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub(super) struct ParcelTiles {
    lot: Lot,
    width: usize,
    cells: Vec<Tile>,
}

impl ParcelTiles {
    pub(super) fn new(lot: Lot, terrain: &[Tile]) -> Self {
        let width = (lot.max.x - lot.min.x + 1) as usize;
        let mut cells = Vec::with_capacity(width * (lot.max.y - lot.min.y + 1) as usize);
        for y in lot.min.y..=lot.max.y {
            let start = (y * WORLD_SIZE + lot.min.x) as usize;
            cells.extend_from_slice(&terrain[start..start + width]);
        }
        Self { lot, width, cells }
    }

    fn offset(&self, p: Position) -> usize {
        debug_assert!((self.lot.min.x..=self.lot.max.x).contains(&p.x));
        debug_assert!((self.lot.min.y..=self.lot.max.y).contains(&p.y));
        (p.y - self.lot.min.y) as usize * self.width + (p.x - self.lot.min.x) as usize
    }
}

impl Index<Position> for ParcelTiles {
    type Output = Tile;
    fn index(&self, p: Position) -> &Self::Output {
        &self.cells[self.offset(p)]
    }
}
impl IndexMut<Position> for ParcelTiles {
    fn index_mut(&mut self, p: Position) -> &mut Self::Output {
        let i = self.offset(p);
        &mut self.cells[i]
    }
}

#[derive(Debug)]
struct Parcel {
    lot: Lot,
    template: u64,
    detail: OnceLock<(ParcelTiles, Sites)>,
}

#[derive(Debug)]
pub(super) struct Parcels {
    parcels: Vec<Parcel>,
    // Zero means open country, other values are one-based parcel indices.
    owner: Vec<u16>,
    manifest: OnceLock<Sites>,
}

impl Parcels {
    pub(super) fn new(lots: Vec<Lot>) -> Self {
        let mut owner = vec![0; (WORLD_SIZE * WORLD_SIZE) as usize];
        let mut parcels = Vec::with_capacity(lots.len());
        let (mut residential, mut city) = (0, 0);
        for lot in lots {
            let template = match lot.district {
                District::Residential => {
                    residential += 1;
                    residential - 1
                }
                District::City => {
                    city += 1;
                    city - 1
                }
                _ => 0,
            };
            let id = u16::try_from(parcels.len() + 1).expect("bounded county parcel count");
            for y in lot.min.y..=lot.max.y {
                let start = (y * WORLD_SIZE + lot.min.x) as usize;
                let row = &mut owner[start..start + (lot.max.x - lot.min.x + 1) as usize];
                debug_assert!(row.iter().all(|owner| *owner == 0), "overlapping lots");
                row.fill(id);
            }
            parcels.push(Parcel {
                lot,
                template,
                detail: OnceLock::new(),
            });
        }
        Self {
            parcels,
            owner,
            manifest: OnceLock::new(),
        }
    }

    fn detail<'a>(
        &self,
        parcel: &'a Parcel,
        seed: u64,
        terrain: &[Tile],
    ) -> &'a (ParcelTiles, Sites) {
        parcel.detail.get_or_init(|| {
            let mut cells = ParcelTiles::new(parcel.lot, terrain);
            let sites = sites::generate(seed, &parcel.lot, parcel.template, &mut cells);
            dressing::generate(seed, &parcel.lot, &sites, &mut cells);
            (cells, sites)
        })
    }

    pub(super) fn tile_at(&self, p: Position, index: usize, seed: u64, terrain: &[Tile]) -> Tile {
        match self.owner[index] {
            0 => terrain[index],
            id => {
                self.detail(&self.parcels[usize::from(id) - 1], seed, terrain)
                    .0[p]
            }
        }
    }

    pub(super) fn chunk_sites(
        &self,
        min: Position,
        max: Position,
        seed: u64,
        terrain: &[Tile],
    ) -> super::ChunkSites<'_> {
        let inside = |p: Position| (min.x..=max.x).contains(&p.x) && (min.y..=max.y).contains(&p.y);
        let intersects =
            |a: Position, b: Position| a.x <= max.x && b.x >= min.x && a.y <= max.y && b.y >= min.y;
        let mut result = super::ChunkSites::default();
        for parcel in &self.parcels {
            if intersects(parcel.lot.min, parcel.lot.max) {
                let sites = &self.detail(parcel, seed, terrain).1;
                result
                    .buildings
                    .extend(sites.buildings.iter().filter(|b| intersects(b.min, b.max)));
                result
                    .loot
                    .extend(sites.loot.iter().filter(|s| inside(s.position)));
                result
                    .infected
                    .extend(sites.infected.iter().filter(|s| inside(s.position)));
            }
        }
        result
    }

    pub(super) fn feature_at(
        &self,
        p: Position,
        index: usize,
        floor: u8,
        seed: u64,
        terrain: &[Tile],
    ) -> Option<super::Feature> {
        let id = self.owner[index].checked_sub(1)?;
        self.detail(&self.parcels[usize::from(id)], seed, terrain)
            .1
            .feature_at(p, floor)
    }

    pub(super) fn sites(&self, seed: u64, terrain: &[Tile]) -> &Sites {
        self.manifest.get_or_init(|| {
            let mut all = Sites::default();
            for parcel in &self.parcels {
                let (_, sites) = self.detail(parcel, seed, terrain);
                all.buildings.extend_from_slice(&sites.buildings);
                all.loot.extend_from_slice(&sites.loot);
                all.infected.extend_from_slice(&sites.infected);
            }
            all
        })
    }
}
