//! Cardinal development reserves next to the town road graph.

use super::{
    CardinalDir, GridRect, Lot, Position, Tile, WORLD_SIZE,
    blueprint::parcel_id,
    relief::NaturalTerrainField,
    town_planner::{RoadClass, TownPlan},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontageUse {
    Residential,
    Commercial,
}

/// Explicit connection between a road tile and the midpoint of a lot frontage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrontageConnector {
    pub road: Position,
    pub frontage: Position,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrontageSlot {
    pub id: u64,
    pub bounds: GridRect,
    pub side: CardinalDir,
    pub road_class: RoadClass,
    pub connector: FrontageConnector,
    pub use_hint: FrontageUse,
    pub suitability: f32,
    /// Suggested pad elevation in tile units for the future access ramp.
    pub foundation_z: f32,
}

impl FrontageSlot {
    /// Inclusive world-coordinate interval on the frontage edge. North and
    /// south frontages vary along X. East and west frontages vary along Y.
    pub fn usable_interval(&self) -> std::ops::RangeInclusive<i32> {
        match self.side {
            CardinalDir::North | CardinalDir::South => self.bounds.min.x..=self.bounds.max.x,
            CardinalDir::East | CardinalDir::West => self.bounds.min.y..=self.bounds.max.y,
        }
    }

    pub(super) fn connector_bounds(&self) -> GridRect {
        let FrontageConnector { road, frontage } = self.connector;
        GridRect::new(
            Position::new(road.x.min(frontage.x), road.y.min(frontage.y)),
            Position::new(road.x.max(frontage.x), road.y.max(frontage.y)),
        )
    }
}

pub(super) fn reserve(
    seed: u64,
    town: &TownPlan,
    natural: &NaturalTerrainField,
    tiles: &[Tile],
    lots: &[Lot],
) -> Vec<FrontageSlot> {
    let mut candidates = Vec::new();
    let occupied: Vec<_> = lots
        .iter()
        .map(|lot| GridRect::new(lot.min, lot.max))
        .collect();
    for (road, cell) in town.raster() {
        let class = town.roads[cell.road_index].class;
        let (use_hint, envelopes): (FrontageUse, &[(i32, i32)]) = if class == RoadClass::Arterial {
            // The gas compound needs 28×28. The 20×22 grocery also fits
            // the smaller envelope with its two-tile service setbacks.
            (FrontageUse::Commercial, &[(28, 28), (24, 26)])
        } else {
            (FrontageUse::Residential, &[(16, 18), (12, 14)])
        };
        for side in [
            CardinalDir::North,
            CardinalDir::East,
            CardinalDir::South,
            CardinalDir::West,
        ] {
            let (dx, dy) = side.inward();
            // Only consider outside edges of the raster. The slot may step
            // out alongside a curve.
            if road.x < 8 || road.y < 8 || road.x >= WORLD_SIZE - 8 || road.y >= WORLD_SIZE - 8 {
                continue;
            }
            if tiles[Position::new(road.x + dx, road.y + dy).index().unwrap()]
                .ground
                .is_road()
            {
                continue;
            }
            for (rank, &(width, depth)) in envelopes.iter().enumerate() {
                for gap in 1..=4 {
                    let frontage = Position::new(road.x + gap * dx, road.y + gap * dy);
                    let a =
                        Position::new(frontage.x - dy * (width / 2), frontage.y - dx * (width / 2));
                    let b = Position::new(
                        a.x + dy * (width - 1) + dx * (depth - 1),
                        a.y + dx * (width - 1) + dy * (depth - 1),
                    );
                    let bounds = GridRect::new(
                        Position::new(a.x.min(b.x), a.y.min(b.y)),
                        Position::new(a.x.max(b.x), a.y.max(b.y)),
                    );
                    if bounds.min.index().is_none()
                        || bounds.max.index().is_none()
                        || occupied.iter().any(|other| other.intersects(bounds))
                    {
                        continue;
                    }
                    let mut suitability = 0.0;
                    let mut natural_height = 0.0;
                    let valid = (bounds.min.y..=bounds.max.y).all(|y| {
                        (bounds.min.x..=bounds.max.x).all(|x| {
                            let p = Position::new(x, y);
                            let tile = tiles[p.index().unwrap()];
                            let sample = natural.sample(p).unwrap();
                            suitability += sample.suitability;
                            natural_height += sample.height;
                            !tile.ground.is_road()
                                && sample.water_distance >= 4.0
                                && sample.suitability > 0.0
                        })
                    });
                    if !valid {
                        continue;
                    }
                    if (1..gap).any(|step| {
                        let p = Position::new(road.x + dx * step, road.y + dy * step);
                        tiles[p.index().unwrap()].ground.is_road()
                            || natural.sample(p).unwrap().water_distance < 4.0
                            || occupied.iter().any(|bounds| bounds.contains(p))
                    }) {
                        continue;
                    }
                    candidates.push((
                        rank,
                        FrontageSlot {
                            id: parcel_id(seed, bounds).0,
                            bounds,
                            side,
                            road_class: class,
                            connector: FrontageConnector { road, frontage },
                            use_hint,
                            suitability: suitability / (width * depth) as f32,
                            foundation_z: natural_height / (width * depth) as f32,
                        },
                    ));
                }
            }
        }
    }

    // Preserve the larger envelopes where they fit before filling remaining gaps.
    candidates.sort_by(|(a_rank, a), (b_rank, b)| {
        let score = |slot: &FrontageSlot| {
            slot.suitability - slot.connector.road.distance(town.core_anchor) as f32 / 500.0
        };
        a_rank
            .cmp(b_rank)
            .then_with(|| score(b).total_cmp(&score(a)))
            .then_with(|| a.id.cmp(&b.id))
    });
    // Geography can protect an early reserve while placing civic parcels.
    let mut accepted = town.frontages.clone();
    for (_, candidate) in candidates {
        if accepted.len() >= 24 {
            break;
        }
        if accepted.iter().any(|other| {
            other.bounds.intersects(candidate.bounds)
                || other.connector_bounds().intersects(candidate.bounds)
                || candidate.connector_bounds().intersects(other.bounds)
        }) {
            continue;
        }
        accepted.push(candidate);
    }
    accepted
}
