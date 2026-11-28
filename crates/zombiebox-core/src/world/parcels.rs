//! The county fixes all frontages and template choices before a chunk is
//! visited. Neighbors sharing a building resolve the same immutable parcel.
use super::{
    District, Lot, ParcelPlan, Position, Sites, Tile, WORLD_SIZE,
    archetypes::{self, CountyFixture, ParcelContext, plan_fixture},
    blueprint::{BlueprintKind, CompoundPlanner, Frontage, GridRect, ZoneKind},
    dressing, sites,
};
use std::collections::VecDeque;
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
    guarantee_ammo: bool,
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
    pub(super) fn new(
        seed: u64,
        lots: Vec<Lot>,
        spawn: Position,
        fixture: CountyFixture,
        terrain: &[Tile],
        retail_lots: &[(Lot, CountyFixture)],
    ) -> Self {
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
                guarantee_ammo: false,
                foundation_z: 0.0,
                plan: None,
                planned_outdoors: false,
                detail: OnceLock::new(),
            });
        }
        let fixture_index = match fixture {
            CountyFixture::None => None,
            CountyFixture::ResidentialStreet => {
                let candidates = Self::residential_frontages(seed, &parcels, spawn, terrain);
                let starter =
                    Self::select_starting_houses(&mut parcels, &candidates).or_else(|| {
                        Self::select_fixture_parcel(
                            seed,
                            &mut parcels,
                            spawn,
                            CountyFixture::CraftsmanLot,
                        )
                    });
                Self::select_additional_houses(&mut parcels, &candidates);
                for &(lot, fixture) in retail_lots {
                    let parcel = parcels
                        .iter_mut()
                        .find(|p| p.lot.min == lot.min && p.lot.max == lot.max)
                        .expect("reserved retail lot");
                    parcel.plan = Some(
                        plan_fixture(fixture, &parcel_context(seed, lot))
                            .expect("validated retail lot"),
                    );
                    parcel.planned_outdoors = true;
                }
                // A constrained natural corridor may not have both reserve
                // envelopes. Use a suitable existing city frontage as fallback.
                for fixture in [CountyFixture::GasStationLot, CountyFixture::GroceryLot] {
                    if !retail_lots.iter().any(|(_, selected)| *selected == fixture) {
                        Self::select_fixture_parcel(seed, &mut parcels, spawn, fixture);
                    }
                }
                // Recycle the downtown families after a retail fallback takes
                // a city parcel, retaining the full legacy downtown roster.
                for (ordinal, parcel) in parcels
                    .iter_mut()
                    .filter(|p| p.lot.district == District::City && p.plan.is_none())
                    .enumerate()
                {
                    parcel.template = ordinal as u64;
                }
                // The furnished houses have their own archetypes. Cycle the legacy
                // templates across the remaining houses so selection cannot skip
                // a family when its original parcel becomes furnished.
                let mut rolled_ammo = false;
                let mut fallback = None;
                for (ordinal, parcel) in parcels
                    .iter_mut()
                    .filter(|p| p.lot.district == District::Residential && p.plan.is_none())
                    .enumerate()
                {
                    parcel.template = ordinal as u64;
                    let variant = sites::loot_variant(seed, &parcel.lot);
                    rolled_ammo |= variant.is_multiple_of(3);
                    if fallback.as_ref().is_none_or(|(best, _)| variant < *best) {
                        fallback = Some((variant, parcel));
                    }
                }
                // A smaller neighborhood can miss every independent ammo
                // roll. Only replace the fallback home's tools in that case.
                if !rolled_ammo && let Some((_, parcel)) = fallback {
                    parcel.guarantee_ammo = true;
                }
                starter
            }
            _ => Self::select_fixture_parcel(seed, &mut parcels, spawn, fixture),
        };
        Self {
            parcels,
            owner,
            fixture_index,
            manifest: OnceLock::new(),
        }
    }

    /// Rank residential frontages by their connected road distance from spawn.
    fn residential_frontages(
        seed: u64,
        parcels: &[Parcel],
        spawn: Position,
        terrain: &[Tile],
    ) -> Vec<(usize, ParcelContext)> {
        let distances = road_distances(terrain, spawn);
        let mut candidates: Vec<_> = parcels
            .iter()
            .enumerate()
            .filter_map(|(index, parcel)| {
                if parcel.lot.district != District::Residential {
                    return None;
                }
                let context = parcel_context(seed, parcel.lot);
                let access = context.frontage.access;
                let (dx, dy) = context.frontage.side.inward();
                let road = Position::new(access.x - dx, access.y - dy);
                let distance = distances[road.index()?]?;
                Some((distance, access.y, access.x, index, context))
            })
            .collect();
        candidates.sort_unstable_by_key(|&(distance, y, x, index, _)| (distance, y, x, index));
        candidates
            .into_iter()
            .map(|(_, _, _, index, context)| (index, context))
            .collect()
    }

    /// Keep viable plans until all three starter families fit on distinct lots.
    fn select_starting_houses(
        parcels: &mut [Parcel],
        candidates: &[(usize, ParcelContext)],
    ) -> Option<usize> {
        let planners: [&dyn CompoundPlanner; 3] = [
            &archetypes::houses::CraftsmanStreetParcel,
            &archetypes::houses::RanchParcel,
            &archetypes::colonial::ColonialParcel,
        ];
        let mut fits: [Vec<(usize, ParcelPlan)>; 3] = Default::default();
        for &(index, context) in candidates {
            for (planner, options) in planners.into_iter().zip(&mut fits) {
                if let Ok(plan) = planner.plan(&context) {
                    options.push((index, plan));
                }
            }
            let [craftsman, ranch, colonial] = &fits;
            for (a, craftsman) in craftsman {
                for (b, ranch) in ranch {
                    if a == b {
                        continue;
                    }
                    if let Some((c, colonial)) = colonial.iter().find(|(c, _)| a != c && b != c) {
                        for (&index, plan) in [(a, craftsman), (b, ranch), (c, colonial)] {
                            parcels[index].plan = Some(plan.clone());
                            parcels[index].planned_outdoors = true;
                        }
                        return Some(*a);
                    }
                }
            }
        }
        None
    }

    /// Give the wider duplex first choice of the remaining road-connected lots.
    fn select_additional_houses(parcels: &mut [Parcel], candidates: &[(usize, ParcelContext)]) {
        for fixture in [CountyFixture::DuplexLot, CountyFixture::SplitLevelLot] {
            for (index, context) in candidates {
                let parcel = &mut parcels[*index];
                if parcel.plan.is_none()
                    && let Ok(plan) = plan_fixture(fixture, context)
                {
                    parcel.plan = Some(plan);
                    parcel.planned_outdoors = true;
                    break;
                }
            }
        }
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
            if parcels[index].plan.is_some() {
                continue;
            }
            if fixture.furnished_lot() && lot.district != District::Residential {
                continue;
            }
            if matches!(
                fixture,
                CountyFixture::GroceryLot | CountyFixture::GasStationLot
            ) && lot.district != District::City
            {
                continue;
            }
            let context = parcel_context(seed, lot);
            let Ok(plan) = plan_fixture(fixture, &context) else {
                continue;
            };
            if fixture != CountyFixture::BlueprintLot {
                parcels[index].plan = Some(plan);
                parcels[index].planned_outdoors = fixture.furnished_lot()
                    || matches!(
                        fixture,
                        CountyFixture::GroceryLot | CountyFixture::GasStationLot
                    );
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
                if let Some(first) = plan.structures.first() {
                    // A compound shares one elevation across its buildings and
                    // forecourt; separate pads can produce a step under pumps.
                    let mut min = first.bounds.min;
                    let mut max = first.bounds.max;
                    for structure in &plan.structures[1..] {
                        min.x = min.x.min(structure.bounds.min.x);
                        min.y = min.y.min(structure.bounds.min.y);
                        max.x = max.x.max(structure.bounds.max.x);
                        max.y = max.y.max(structure.bounds.max.y);
                    }
                    pads.push((
                        idx,
                        sites::PlannedPad {
                            min,
                            max,
                            entry: first.front_door,
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
                    parcel.guarantee_ammo,
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

fn road_distances(terrain: &[Tile], spawn: Position) -> Vec<Option<u16>> {
    let mut distances = vec![None; terrain.len()];
    distances[spawn.index().expect("county spawn in bounds")] = Some(0);
    let mut queue = VecDeque::from([(spawn, 0)]);
    while let Some((pos, distance)) = queue.pop_front() {
        for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            let next = Position::new(pos.x + dx, pos.y + dy);
            if let Some(index) = next.index()
                && distances[index].is_none()
                && terrain[index].ground.is_road()
                && terrain[index].walkable()
            {
                distances[index] = Some(distance + 1);
                queue.push_back((next, distance + 1));
            }
        }
    }
    distances
}

fn parcel_context(seed: u64, lot: Lot) -> ParcelContext {
    ParcelContext {
        seed,
        bounds: GridRect::new(lot.min, lot.max),
        frontage: Frontage {
            side: archetypes::frontage_side(lot.min, lot.max, lot.front),
            access: lot.front,
        },
        zone: zone_for(lot.district),
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
    use super::road_distances;
    use crate::{CountyFixture, Position, World};

    #[test]
    fn reference_street_is_default_and_deterministic() {
        for seed in [0, 1, 2, 7, 42, 63, u64::MAX] {
            let first = World::new(seed);
            let second = World::with_fixture(seed, CountyFixture::ResidentialStreet);
            let plans = first.fixture_plans();
            assert_eq!(
                plans
                    .iter()
                    .filter(|p| p.zone == crate::ZoneKind::Residential)
                    .count(),
                5,
                "seed {seed} needs all five house families"
            );
            assert_eq!(plans, second.fixture_plans(), "seed {seed}");
            let county = first.county.as_ref().unwrap();
            let distances = road_distances(&county.tiles, first.wake_position());
            for prefix in ["craftsman_", "ranch_", "colonial_"] {
                let plan = plans
                    .iter()
                    .find(|plan| plan.structures[0].archetype.starts_with(prefix))
                    .unwrap_or_else(|| panic!("seed {seed} is missing {prefix}"));
                let access = plan.frontage.access;
                let (dx, dy) = plan.frontage.side.inward();
                let road = Position::new(access.x - dx, access.y - dy);
                let road_steps =
                    distances[road.index().unwrap()].expect("frontage connected to the spawn");
                let path_steps: u32 = plan.outdoor.paths.iter().map(|(a, b)| a.distance(*b)).sum();
                let total_steps = u32::from(road_steps) + 1 + path_steps;
                assert!(
                    total_steps <= 90,
                    "seed {seed}: {prefix} is {total_steps} tiles away"
                );
            }
        }
    }

    #[test]
    fn default_county_assigns_both_new_houses_to_connected_frontages() {
        for seed in (0..64).chain([u64::MAX]) {
            let world = World::new(seed);
            let plans = world.fixture_plans();
            assert_eq!(
                plans
                    .iter()
                    .filter(|p| p.zone == crate::ZoneKind::Residential)
                    .count(),
                5,
                "seed {seed}"
            );
            let county = world.county.as_ref().unwrap();
            let distances = road_distances(&county.tiles, world.wake_position());
            for prefix in ["split_level_", "duplex_"] {
                let plan = plans
                    .iter()
                    .find(|plan| plan.structures[0].archetype.starts_with(prefix))
                    .unwrap_or_else(|| panic!("seed {seed} is missing {prefix}"));
                let access = plan.frontage.access;
                let (dx, dy) = plan.frontage.side.inward();
                let road = Position::new(access.x - dx, access.y - dy);
                assert!(
                    distances[road.index().unwrap()].is_some(),
                    "seed {seed}: {prefix} road access is disconnected"
                );
            }
        }
    }
}
