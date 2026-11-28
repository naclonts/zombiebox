//! The county fixes all frontages and template choices before a chunk is
//! visited. Neighbors sharing a building resolve the same immutable parcel.
use super::{
    District, Lot, ParcelPlan, Position, Sites, Tile, WORLD_SIZE,
    archetypes::{self, CountyFixture, ParcelContext, plan_fixture},
    blueprint::{BlueprintKind, CardinalDir, CompoundPlanner, Frontage, GridRect, ZoneKind},
    dressing, sites,
};
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
    foundation_z: f32,
    plan: Option<ParcelPlan>,
    planned_outdoors: bool,
    detail: OnceLock<(ParcelTiles, Sites)>,
}

#[derive(Debug)]
pub(super) struct Parcels {
    parcels: Vec<Parcel>,
    // Zero means open country, other values are one-based parcel indices.
    owner: Vec<u16>,
    fixture_index: Option<usize>,
    manifest: OnceLock<Sites>,
}

impl Parcels {
    pub(super) fn new(seed: u64, lots: Vec<Lot>, spawn: Position, fixture: CountyFixture) -> Self {
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
                foundation_z: 0.0,
                plan: None,
                planned_outdoors: false,
                detail: OnceLock::new(),
            });
        }
        let mut fixture_index = None;
        if fixture != CountyFixture::None {
            fixture_index = if fixture == CountyFixture::ResidentialStreet {
                Self::select_reference_street(seed, &mut parcels, spawn).or_else(|| {
                    Self::select_fixture_parcel(
                        seed,
                        &mut parcels,
                        spawn,
                        CountyFixture::CraftsmanLot,
                    )
                })
            } else {
                Self::select_fixture_parcel(seed, &mut parcels, spawn, fixture)
            };
        }
        Self {
            parcels,
            owner,
            fixture_index,
            manifest: OnceLock::new(),
        }
    }

    // Only compare existing lots along the same road-facing line. The bounded
    // pair list is sorted independently of geography emission or chunk order.
    fn select_reference_street(
        seed: u64,
        parcels: &mut [Parcel],
        spawn: Position,
    ) -> Option<usize> {
        let mut pairs = Vec::new();
        for (a, first) in parcels.iter().enumerate() {
            if first.lot.district != District::Residential {
                continue;
            }
            let side = archetypes::frontage_side(first.lot.min, first.lot.max, first.lot.front);
            for (b, second) in parcels.iter().enumerate() {
                if a == b
                    || second.lot.district != District::Residential
                    || side
                        != archetypes::frontage_side(
                            second.lot.min,
                            second.lot.max,
                            second.lot.front,
                        )
                {
                    continue;
                }
                let x_front = matches!(side, CardinalDir::East | CardinalDir::West);
                let (same_line, gap) = if x_front {
                    (
                        first.lot.front.x == second.lot.front.x,
                        (first.lot.min.y - second.lot.max.y - 1)
                            .max(second.lot.min.y - first.lot.max.y - 1),
                    )
                } else {
                    (
                        first.lot.front.y == second.lot.front.y,
                        (first.lot.min.x - second.lot.max.x - 1)
                            .max(second.lot.min.x - first.lot.max.x - 1),
                    )
                };
                if !same_line || !(0..=6).contains(&gap) {
                    continue;
                }
                let distance = first.lot.front.distance(spawn) + second.lot.front.distance(spawn);
                pairs.push((
                    distance,
                    first.lot.min.y,
                    first.lot.min.x,
                    second.lot.min.y,
                    second.lot.min.x,
                    a,
                    b,
                ));
            }
        }
        pairs.sort_unstable();
        for (_, _, _, _, _, a, b) in pairs {
            let context = |lot: Lot| ParcelContext {
                seed,
                bounds: GridRect::new(lot.min, lot.max),
                frontage: Frontage {
                    side: archetypes::frontage_side(lot.min, lot.max, lot.front),
                    access: lot.front,
                },
                zone: zone_for(lot.district),
            };
            let Ok(craftsman) =
                archetypes::houses::CraftsmanStreetParcel.plan(&context(parcels[a].lot))
            else {
                continue;
            };
            let Ok(ranch) = archetypes::houses::RanchParcel.plan(&context(parcels[b].lot)) else {
                continue;
            };
            parcels[a].planned_outdoors = true;
            parcels[b].planned_outdoors = true;
            parcels[a].plan = Some(craftsman);
            parcels[b].plan = Some(ranch);
            return Some(a);
        }
        None
    }

    fn select_fixture_parcel(
        seed: u64,
        parcels: &mut [Parcel],
        spawn: Position,
        fixture: CountyFixture,
    ) -> Option<usize> {
        let mut candidates: Vec<usize> = parcels
            .iter()
            .enumerate()
            .filter(|(_, parcel)| {
                matches!(
                    parcel.lot.district,
                    District::Residential | District::City | District::Woods
                ) && parcel.lot.max.x - parcel.lot.min.x >= 14
                    && parcel.lot.max.y - parcel.lot.min.y >= 14
            })
            .map(|(index, _)| index)
            .collect();
        candidates.sort_by_key(|&index| {
            let lot = parcels[index].lot;
            let cx = (lot.min.x + lot.max.x) / 2;
            let cy = (lot.min.y + lot.max.y) / 2;
            let distance = Position::new(cx, cy).distance(spawn);
            let district_rank = match lot.district {
                District::Residential => 0,
                District::City => 1,
                _ => 2,
            };
            (distance, district_rank, lot.min.y, lot.min.x)
        });
        let mut fallback = None;
        for index in candidates {
            let lot = parcels[index].lot;
            if fixture == CountyFixture::ColonialLot && lot.district != District::Residential {
                continue;
            }
            let side = archetypes::frontage_side(lot.min, lot.max, lot.front);
            let context = ParcelContext {
                seed,
                bounds: GridRect::new(lot.min, lot.max),
                frontage: Frontage {
                    side,
                    access: lot.front,
                },
                zone: zone_for(lot.district),
            };
            let Ok(plan) = plan_fixture(fixture, &context) else {
                continue;
            };
            if fixture != CountyFixture::BlueprintLot {
                parcels[index].plan = Some(plan);
                parcels[index].planned_outdoors = fixture == CountyFixture::ColonialLot;
                return Some(index);
            }
            let crosses_seam = plan.structures.iter().any(|structure| {
                structure.props_world().iter().any(|prop| {
                    let tiles: Vec<_> = prop.span.covered_tiles(prop.tile).collect();
                    tiles.iter().any(|a| {
                        tiles
                            .iter()
                            .any(|b| a.x >> 4 != b.x >> 4 || a.y >> 4 != b.y >> 4)
                    })
                })
            });
            if fallback.is_none() {
                fallback = Some((index, plan.clone()));
            }
            if crosses_seam {
                parcels[index].plan = Some(plan);
                return Some(index);
            }
        }
        if let Some((index, plan)) = fallback {
            parcels[index].plan = Some(plan);
            return Some(index);
        }
        None
    }

    pub(super) fn planned_pads(
        &self,
        seed: u64,
        terrain: &[Tile],
    ) -> Vec<(usize, sites::PlannedPad)> {
        let mut pads = Vec::new();
        for (idx, parcel) in self.parcels.iter().enumerate() {
            if let Some(plan) = &parcel.plan {
                for structure in &plan.structures {
                    pads.push((
                        idx,
                        sites::PlannedPad {
                            min: structure.bounds.min,
                            max: structure.bounds.max,
                            entry: structure.front_door,
                            front: plan.frontage.access,
                        },
                    ));
                }
            } else if let Some(pad) =
                sites::planned_pad(seed, &parcel.lot, parcel.template, terrain)
            {
                pads.push((idx, pad));
            }
        }
        pads
    }

    pub(super) fn set_foundation(&mut self, parcel_idx: usize, foundation_z: f32) {
        let parcel = &mut self.parcels[parcel_idx];
        parcel.foundation_z = foundation_z;
        if let Some(plan) = &mut parcel.plan {
            for structure in &mut plan.structures {
                structure.set_foundation(foundation_z);
            }
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
            let sites = if let Some(plan) = &parcel.plan {
                apply_plan_ground(plan, &mut cells, parcel.planned_outdoors);
                Sites::default()
            } else {
                sites::generate(
                    seed,
                    &parcel.lot,
                    parcel.template,
                    parcel.foundation_z,
                    &mut cells,
                )
            };
            if !parcel.planned_outdoors {
                dressing::generate(seed, &parcel.lot, &sites, &mut cells);
            }
            (cells, sites)
        })
    }

    pub(super) fn fixture_plan(&self) -> Option<&ParcelPlan> {
        let index = self.fixture_index?;
        self.parcels[index].plan.as_ref()
    }

    pub(super) fn fixture_plans(&self) -> impl Iterator<Item = &ParcelPlan> {
        self.parcels
            .iter()
            .filter_map(|parcel| parcel.plan.as_ref())
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
                if let Some(plan) = &parcel.plan {
                    result.structures.extend(
                        plan.structures
                            .iter()
                            .filter(|s| intersects(s.bounds.min, s.bounds.max)),
                    );
                } else {
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
        let parcel = &self.parcels[usize::from(id)];
        if let Some(plan) = &parcel.plan {
            return plan.structures.iter().find_map(|structure| {
                structure
                    .cell_at_world(p, floor)
                    .map(|(cell, _)| cell.kind.to_feature(cell.material))
            });
        }
        self.detail(parcel, seed, terrain).1.feature_at(p, floor)
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

fn zone_for(district: District) -> ZoneKind {
    match district {
        District::Residential | District::Woods => ZoneKind::Residential,
        District::City => ZoneKind::Commercial,
        District::Hospital | District::Police => ZoneKind::Civic,
        District::MilitaryBase => ZoneKind::Service,
        District::Farm => ZoneKind::Rural,
    }
}

fn apply_plan_ground(plan: &ParcelPlan, cells: &mut ParcelTiles, planned_outdoors: bool) {
    // Paths are established before structures, which remain authoritative.
    for (&(start, end), ground) in plan
        .outdoor
        .paths
        .iter()
        .map(|p| (p, super::Ground::Gravel))
        .chain(
            plan.outdoor
                .driveways
                .iter()
                .map(|p| (p, super::Ground::Pavement)),
        )
        .filter(|_| planned_outdoors)
    {
        let mut p = start;
        loop {
            if p.x >= plan.bounds.min.x
                && p.x <= plan.bounds.max.x
                && p.y >= plan.bounds.min.y
                && p.y <= plan.bounds.max.y
            {
                let tile = &mut cells[p];
                if tile.feature == super::Feature::Empty
                    && !tile.ground.is_road()
                    && tile.water.is_none()
                {
                    tile.feature = super::Feature::Path;
                    tile.ground = ground;
                }
            }
            if p == end {
                break;
            }
            if p.x != end.x {
                p.x += (end.x - p.x).signum();
            } else {
                p.y += (end.y - p.y).signum();
            }
        }
    }
    for structure in &plan.structures {
        for (local, world, cell) in structure.world_cells() {
            if local.floor != 0 || cell.kind == BlueprintKind::Empty {
                continue;
            }
            cells[world].feature = cell.kind.to_feature(cell.material);
        }
    }
}

#[cfg(test)]
mod reference_street_tests {
    use crate::{CountyFixture, World};

    #[test]
    fn reference_street_is_default_and_deterministic() {
        for seed in [0, 1, 2, 7, 42, 63, u64::MAX] {
            let first = World::with_fixture(seed, CountyFixture::ResidentialStreet);
            let second = World::with_fixture(seed, CountyFixture::ResidentialStreet);
            let plans = first.fixture_plans();
            assert_eq!(plans.len(), 2, "seed {seed} has no compatible street");
            assert_eq!(plans, second.fixture_plans(), "seed {seed}");
            assert!(plans.iter().all(|p| !p.outdoor.paths.is_empty()));
            assert_eq!(World::new(seed).fixture_plans(), plans);
            assert_eq!(World::legacy(seed).fixture(), CountyFixture::None);
        }
    }
}
