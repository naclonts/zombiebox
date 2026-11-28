//! Deterministic motorcycle roster placement over a generated county.
//!
//! The roster is a pure function of the world seed and baseline geography. It
//! reserves slot zero for the starter bike near the walkable spawn and then
//! seeds the remaining slots from distinct roadside approaches around the
//! county landmarks. Chunk visit order never influences placement or IDs.

use std::collections::HashSet;

use super::{Feature, Ground, LandmarkKind, Position, World};
use crate::spatial::Vec2;

/// Stable motorcycle slots generated for one county run.
pub(crate) const MOTORCYCLE_SLOTS: u8 = 4;

const STARTER_SEARCH_RADIUS: i32 = 16;
const STARTER_MAX_WALK: u32 = 13;
const DISTANT_SEARCH_RADIUS: i32 = 15;
const MIN_STARTER_DISTANCE: u32 = 2;
const MIN_ROSTER_SEPARATION: u32 = 22;
const DEPARTURE_LOOKAHEAD: i32 = 12;
const BLOCKED_DEPARTURE_PENALTY: u64 = 150;

/// Baseline placement for one generated motorcycle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MotorcycleSpawn {
    /// Stable within the generated county (`0` is the starter bike).
    pub slot: u8,
    pub tile: Position,
    /// World heading in radians (`+x` is zero, positive turns toward `+y`).
    pub heading: f32,
}

#[derive(Default)]
struct LocalBlockers {
    buildings: Vec<(Position, Position)>,
    loot: Vec<Position>,
}

impl LocalBlockers {
    fn contains(&self, p: Position) -> bool {
        self.buildings
            .iter()
            .any(|(min, max)| p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y)
            || self.loot.iter().any(|q| *q == p)
    }
}

#[derive(Clone, Copy)]
struct Candidate {
    spawn: MotorcycleSpawn,
    distance: u32,
    score: u64,
}

/// Generates the four-slot motorcycle roster for `world`.
///
/// `starter_is_clear` lets the live action stage reject a starter candidate
/// blocked by materialized props or starter loot before the search moves on.
pub(crate) fn motorcycle_spawns(
    world: &World,
    start: Position,
    mut starter_is_clear: impl FnMut(&MotorcycleSpawn) -> bool,
) -> Vec<MotorcycleSpawn> {
    if world.is_flat() {
        return Vec::new();
    }
    let seed = world.seed();
    let mut spawns = Vec::with_capacity(MOTORCYCLE_SLOTS as usize);

    if let Some(spawn) = starter_spawn(world, start, seed, &mut starter_is_clear) {
        spawns.push(spawn);
    }

    let mut used_districts = HashSet::new();
    if let Some(first) = spawns.first()
        && let Some(district) = world.district_at(first.tile)
    {
        used_districts.insert(district);
    }

    let mut anchors: Vec<(u64, Position, Option<super::District>)> = world
        .landmarks()
        .iter()
        .filter(|l| {
            !matches!(
                l.kind,
                LandmarkKind::Residential | LandmarkKind::Lake | LandmarkKind::Mountain
            )
        })
        .map(|l| {
            let packed = (u64::from(l.entrance.x as u32) << 32) | u64::from(l.entrance.y as u32);
            let rank = super::noise_for_vehicles(seed, packed);
            (rank, l.entrance, world.district_at(l.entrance))
        })
        .collect();
    anchors.sort_by_key(|(rank, _, _)| *rank);

    // First pass: prefer distinct districts, giving the roster a spread from
    // downtown and civic approaches through to rural cabins.
    let mut deferred = Vec::new();
    for (rank, anchor, district) in anchors {
        if spawns.len() >= MOTORCYCLE_SLOTS as usize {
            break;
        }
        if district.is_some_and(|d| used_districts.contains(&d)) {
            deferred.push((rank, anchor, district));
            continue;
        }
        if let Some(spawn) = spawn_near_anchor(
            world,
            anchor,
            seed,
            spawns.len() as u8,
            &spawns,
            &mut starter_is_clear,
        ) {
            if let Some(d) = district.or_else(|| world.district_at(spawn.tile)) {
                used_districts.insert(d);
            }
            spawns.push(spawn);
        }
    }

    // Second pass: allow a repeat district when the county has fewer distinct
    // civic/rural anchors with usable shoulders.
    for (_, anchor, _) in deferred {
        if spawns.len() >= MOTORCYCLE_SLOTS as usize {
            break;
        }
        if let Some(spawn) = spawn_near_anchor(
            world,
            anchor,
            seed,
            spawns.len() as u8,
            &spawns,
            &mut starter_is_clear,
        ) {
            spawns.push(spawn);
        }
    }

    // A review-only bike at the drive entrance; the normal county roster is unchanged.
    if world.fixture() == super::CountyFixture::GasStationLot
        && let Some(plan) = world.fixture_plan()
    {
        let transform = plan.structures[1].transform;
        let tile = transform.local_to_world(super::LocalCell::new(8, -6, 0));
        let inward = transform.local_to_world(super::LocalCell::new(8, -5, 0));
        let heading = ((inward.y - tile.y) as f32).atan2((inward.x - tile.x) as f32);
        if let Some(last) = spawns.last_mut() {
            *last = MotorcycleSpawn {
                slot: last.slot,
                tile,
                heading,
            };
        } else {
            spawns.push(MotorcycleSpawn {
                slot: 0,
                tile,
                heading,
            });
        }
    }

    spawns
}

fn starter_spawn(
    world: &World,
    start: Position,
    seed: u64,
    starter_is_clear: &mut impl FnMut(&MotorcycleSpawn) -> bool,
) -> Option<MotorcycleSpawn> {
    let reachable = reachable_tiles(world, start, STARTER_MAX_WALK);
    let min = Position::new(
        start.x.saturating_sub(STARTER_SEARCH_RADIUS),
        start.y.saturating_sub(STARTER_SEARCH_RADIUS),
    );
    let max = Position::new(
        (start.x + STARTER_SEARCH_RADIUS).min(super::WORLD_SIZE - 1),
        (start.y + STARTER_SEARCH_RADIUS).min(super::WORLD_SIZE - 1),
    );
    let blockers = local_blockers(world, min, max);

    let mut candidates = Vec::new();
    for y in min.y..=max.y {
        for x in min.x..=max.x {
            let p = Position::new(x, y);
            let d = p.distance(start);
            if d < MIN_STARTER_DISTANCE || !reachable.contains(&p) {
                continue;
            }
            let Some(((dx, dy), road_penalty)) = candidate_direction(world, p, &blockers) else {
                continue;
            };
            // A shoulder can face directly into a house across the road.
            // Prefer a clear starter launch even when it moves down the block.
            let blocked_departure = (1..=DEPARTURE_LOOKAHEAD).any(|distance| {
                let ahead = Position::new(p.x + dx * distance, p.y + dy * distance);
                world.tile_at(ahead).is_none_or(|tile| {
                    !tile.ground.walkable()
                        || tile.water.is_some()
                        || !matches!(tile.feature, Feature::Empty | Feature::Path)
                })
            });
            let spawn = MotorcycleSpawn {
                slot: 0,
                tile: p,
                heading: (dy as f32).atan2(dx as f32),
            };
            let score = u64::from(d) * 10
                + u64::from(road_penalty) * 25
                + u64::from(blocked_departure) * BLOCKED_DEPARTURE_PENALTY
                + super::noise_for_vehicles(seed, tile_key(p)) % 10;
            candidates.push(Candidate {
                spawn,
                distance: d,
                score,
            });
        }
    }
    candidates.sort_by_key(|c| (c.score, c.distance, tile_key(c.spawn.tile)));
    candidates
        .into_iter()
        .map(|c| c.spawn)
        .find(|spawn| starter_is_clear(spawn))
}

fn spawn_near_anchor(
    world: &World,
    anchor: Position,
    seed: u64,
    slot: u8,
    existing: &[MotorcycleSpawn],
    starter_is_clear: &mut impl FnMut(&MotorcycleSpawn) -> bool,
) -> Option<MotorcycleSpawn> {
    let min = Position::new(
        anchor.x.saturating_sub(DISTANT_SEARCH_RADIUS),
        anchor.y.saturating_sub(DISTANT_SEARCH_RADIUS),
    );
    let max = Position::new(
        (anchor.x + DISTANT_SEARCH_RADIUS).min(super::WORLD_SIZE - 1),
        (anchor.y + DISTANT_SEARCH_RADIUS).min(super::WORLD_SIZE - 1),
    );
    let blockers = local_blockers(world, min, max);
    let mut candidates = Vec::new();

    for y in min.y..=max.y {
        for x in min.x..=max.x {
            let p = Position::new(x, y);
            let d = p.distance(anchor);
            if d == 0 {
                continue;
            }
            if let Some(((dx, dy), road_penalty)) = candidate_direction(world, p, &blockers) {
                let spawn = MotorcycleSpawn {
                    slot,
                    tile: p,
                    heading: (dy as f32).atan2(dx as f32),
                };
                let score = u64::from(d) * 10
                    + u64::from(road_penalty) * 25
                    + super::noise_for_vehicles(seed, tile_key(p)) % 10;
                candidates.push(Candidate {
                    spawn,
                    distance: d,
                    score,
                });
            }
        }
    }

    candidates.sort_by_key(|c| (c.score, c.distance, tile_key(c.spawn.tile)));
    candidates.into_iter().map(|c| c.spawn).find(|spawn| {
        existing
            .iter()
            .all(|used| used.tile.distance(spawn.tile) >= MIN_ROSTER_SEPARATION)
            && starter_is_clear(spawn)
    })
}

fn candidate_direction(
    world: &World,
    p: Position,
    blockers: &LocalBlockers,
) -> Option<((i32, i32), u8)> {
    let tile = world.tile_at(p)?;
    if !tile.ground.walkable() || tile.water.is_some() {
        return None;
    }
    if !matches!(tile.feature, Feature::Empty | Feature::Path) {
        return None;
    }
    if blockers.contains(p) {
        return None;
    }

    let sample = world.terrain_at(Vec2::new(p.x as f32, p.y as f32))?;
    if sample.is_submerged() || sample.slope() > 0.45 {
        return None;
    }

    let mut road = None;
    let mut standings = 0u8;
    for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
        let q = Position::new(p.x + dx, p.y + dy);
        let Some(qt) = world.tile_at(q) else {
            continue;
        };
        if qt.water.is_some() || !qt.ground.walkable() {
            continue;
        }
        if qt.ground.is_road() && qt.feature.walkable() && road.is_none() {
            road = Some((dx, dy));
        }
        if qt.feature.walkable() && !blockers.contains(q) {
            standings = standings.saturating_add(1);
        }
    }
    let direction = road?;
    if standings == 0 {
        return None;
    }
    // A shoulder or driveway is preferred; the road itself is a usable fallback.
    let road_penalty = u8::from(tile.ground.is_road());
    Some((direction, road_penalty))
}

fn local_blockers(world: &World, min: Position, max: Position) -> LocalBlockers {
    let mut blockers = LocalBlockers::default();
    let cx0 = (min.x >> 4).clamp(0, super::CHUNKS_PER_SIDE - 1);
    let cx1 = (max.x >> 4).clamp(0, super::CHUNKS_PER_SIDE - 1);
    let cy0 = (min.y >> 4).clamp(0, super::CHUNKS_PER_SIDE - 1);
    let cy1 = (max.y >> 4).clamp(0, super::CHUNKS_PER_SIDE - 1);
    for cy in cy0..=cy1 {
        for cx in cx0..=cx1 {
            let Some(sites) = world.chunk_sites(cx, cy) else {
                continue;
            };
            for b in sites.buildings {
                blockers.buildings.push((b.min, b.max));
            }
            for l in sites.loot {
                if l.floor == 0 {
                    blockers.loot.push(l.position);
                }
            }
        }
    }
    blockers
}

fn reachable_tiles(world: &World, start: Position, max_steps: u32) -> HashSet<Position> {
    let mut seen = HashSet::new();
    if world.tile_at(start).is_none_or(|t| !t.walkable()) {
        return seen;
    }
    seen.insert(start);
    let mut frontier = vec![(start, 0u32)];
    while let Some((p, steps)) = frontier.pop() {
        if steps >= max_steps {
            continue;
        }
        for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            let q = Position::new(p.x + dx, p.y + dy);
            if seen.contains(&q) {
                continue;
            }
            let Some(tile) = world.tile_at(q) else {
                continue;
            };
            if tile.walkable() && tile.ground != Ground::Mountain {
                seen.insert(q);
                frontier.push((q, steps + 1));
            }
        }
    }
    seen
}

#[inline]
fn tile_key(p: Position) -> u64 {
    (u64::from(p.x as u32) << 32) | u64::from(p.y as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster(seed: u64) -> Vec<MotorcycleSpawn> {
        let world = World::new(seed);
        motorcycle_spawns(&world, world.wake_position(), |_| true)
    }

    #[test]
    fn roster_replays_deterministically() {
        for seed in [0, 42, 641, u64::MAX] {
            let a = roster(seed);
            let b = roster(seed);
            assert_eq!(a, b, "seed {seed}");
            assert!(!a.is_empty(), "seed {seed} should produce a starter bike");
            assert!(a.len() <= MOTORCYCLE_SLOTS as usize);
        }
    }

    #[test]
    fn starter_is_close_and_other_slots_are_spread_out() {
        for seed in [42, 641, 93513] {
            let world = World::new(seed);
            let start = world.wake_position();
            let spawns = motorcycle_spawns(&world, start, |_| true);
            let starter = spawns.first().expect("starter bike");
            assert_eq!(starter.slot, 0);
            assert!(
                starter.tile.distance(start) <= STARTER_MAX_WALK,
                "seed {seed}: starter at {:?} is too far from {start:?}",
                starter.tile
            );
            for pair in spawns.windows(2) {
                assert!(
                    pair[0].tile.distance(pair[1].tile) >= MIN_ROSTER_SEPARATION,
                    "seed {seed}: slots {} and {} are too close",
                    pair[0].slot,
                    pair[1].slot
                );
            }
            let dry = |p: Position| {
                world
                    .tile_at(p)
                    .is_some_and(|t| t.water.is_none() && t.ground.walkable())
            };
            assert!(dry(starter.tile));
            assert!(
                [(0, -1), (1, 0), (0, 1), (-1, 0)].iter().any(|(dx, dy)| {
                    world
                        .tile_at(Position::new(starter.tile.x + dx, starter.tile.y + dy))
                        .is_some_and(|t| t.ground.is_road() && t.water.is_none())
                }),
                "seed {seed}: starter must have a clear way onto a road"
            );
        }
    }

    #[test]
    fn rejecting_the_first_starter_candidate_picks_another() {
        let world = World::new(42);
        let start = world.wake_position();
        let all = motorcycle_spawns(&world, start, |_| true);
        let first = all.first().copied().expect("starter");
        let alt = motorcycle_spawns(&world, start, |spawn| spawn.tile != first.tile);
        let chosen = alt.first().copied().expect("alternate starter");
        assert_ne!(chosen.tile, first.tile);
        assert_eq!(chosen.slot, 0);
    }
}
