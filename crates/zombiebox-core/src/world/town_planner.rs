//! Deterministic road geometry for the highway-ribbon town fixture.
//!
//! Coordinates and distances are world tiles. Roads retain their centerlines;
//! the raster stores the originating segment so terrain can grade across the
//! road without searching the entire graph.
use std::collections::BTreeMap;

use super::frontage::FrontageSlot;
use super::{GridRect, Position, relief::NaturalTerrainSample};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TownMorphology {
    HighwayRibbon,
    Crossroads,
    SplitBank,
    Lakeshore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoadJunction {
    pub road: usize,
    pub sample: usize,
    pub parent_road: usize,
    pub parent_sample: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeSpan {
    pub first_sample: usize,
    pub last_sample: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoadClass {
    Arterial,
    Collector,
    Local,
}

#[derive(Clone, Debug)]
pub struct Road {
    pub class: RoadClass,
    pub turnaround: Option<Position>,
    pub bridge: Option<BridgeSpan>,
    /// Consecutive raster samples at most one tile apart on either world axis.
    pub centerline: Vec<Position>,
    /// Unquantized centerline samples, in world tile coordinates.
    pub continuous_centerline: Vec<[f32; 2]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoadCell {
    pub road_index: usize,
    pub segment_index: usize,
}

#[derive(Clone, Debug)]
pub struct TownPlan {
    pub morphology: TownMorphology,
    pub requested_morphology: TownMorphology,
    /// Why the requested layout fell back to the ribbon, if it did.
    pub fallback_reason: Option<&'static str>,
    pub junctions: Vec<RoadJunction>,
    pub roads: Vec<Road>,
    pub frontages: Vec<FrontageSlot>,
    pub core_anchor: Position,
    pub collector_junction: Position,
    pub local_junction: Position,
    pub turnaround: Position,
    raster: BTreeMap<(i32, i32), RoadCell>,
}

impl TownPlan {
    /// Plan on a dry envelope. Both portals are centerline positions and must
    /// accommodate a five-tile-wide road. Returns None if no connected dry
    /// configuration fits. Seeded tie breaks do not depend on query order.
    pub fn build(
        seed: u64,
        bounds: GridRect,
        portals: [Position; 2],
        sample: impl Fn(Position) -> Option<NaturalTerrainSample>,
    ) -> Option<Self> {
        if bounds.min.index().is_none()
            || bounds.max.index().is_none()
            || portals.iter().any(|&p| !bounds.contains(p))
        {
            return None;
        }
        let dx = portals[1].x - portals[0].x;
        let dy = portals[1].y - portals[0].y;
        let length = dx.abs().max(dy.abs());
        if length < 48 {
            return None;
        }
        let horizontal = dx.abs() >= dy.abs();
        let mut candidates = Vec::new();
        // Gentle sinusoidal displacement joins the portals smoothly
        // and avoids the abrupt tile-scale turns of a greedy route search.
        for (kind, amplitude) in [
            (0, -12_i32),
            (0, -8),
            (0, -4),
            (0, 4),
            (0, 8),
            (0, 12),
            (1, -16),
            (1, -10),
            (1, 10),
            (1, 16),
            // Broad arcs provide a detour when the direct shelf is interrupted.
            (0, -48),
            (0, -36),
            (0, -24),
            (0, 24),
            (0, 36),
            (0, 48),
        ] {
            let junction_index = length as usize / 2;
            let mut continuous = Vec::new();
            for i in 0..=length {
                let t = i as f32 / length as f32;
                let wave = if kind == 0 {
                    (std::f32::consts::PI * t).sin()
                } else {
                    (2.0 * std::f32::consts::PI * t).sin()
                };
                let offset = wave * amplitude as f32;
                let x = portals[0].x as f32 + dx as f32 * t + if horizontal { 0.0 } else { offset };
                let y = portals[0].y as f32 + dy as f32 * t + if horizontal { offset } else { 0.0 };
                continuous.push([x, y]);
            }
            // Snap the shared junction exactly, distributing the sub-tile
            // correction smoothly across the curve instead of introducing a kink.
            let junction = continuous[junction_index];
            let correction = [
                junction[0].round() - junction[0],
                junction[1].round() - junction[1],
            ];
            let peak = (std::f32::consts::PI * junction_index as f32 / length as f32).sin();
            for (i, p) in continuous.iter_mut().enumerate() {
                let weight = (std::f32::consts::PI * i as f32 / length as f32).sin() / peak;
                p[0] += correction[0] * weight;
                p[1] += correction[1] * weight;
            }
            continuous[junction_index] = [junction[0].round(), junction[1].round()];
            let mut points = Vec::new();
            for p in &continuous {
                let p = Position::new(p[0].round() as i32, p[1].round() as i32);
                if points.last() != Some(&p) {
                    points.push(p);
                }
            }
            if has_arterial_curvature(&continuous)
                && let Some(score) = score_path(&points, bounds, &sample)
            {
                candidates.push((
                    score
                        + amplitude.abs() as f32 * 0.0001
                        + if amplitude.abs() > 16 { 1.0 } else { 0.0 },
                    (kind * 100 + amplitude) as i64,
                    points,
                    continuous,
                ));
            }
        }
        candidates.sort_by(|a, b| {
            a.0.total_cmp(&b.0).then_with(|| {
                let ah = tie(seed, a.1 as i64 as u64);
                let bh = tie(seed, b.1 as i64 as u64);
                ah.cmp(&bh)
            })
        });
        // Try arterial candidates in order: a terrain-optimal ribbon is useful
        // only if it can also support a connected neighborhood.
        for (_, _, arterial, continuous) in candidates {
            let middle = continuous[continuous.len() / 2];
            let junction = Position::new(middle[0] as i32, middle[1] as i32);
            let (nx, ny) = if horizontal { (0, 1) } else { (1, 0) };
            let (tx, ty) = if horizontal { (1, 0) } else { (0, 1) };
            let mut branches = Vec::new();
            for side in [-1, 1] {
                for along in [-1, 1] {
                    for collector_len in [44, 42, 40] {
                        let end = Position::new(
                            junction.x + nx * side * collector_len,
                            junction.y + ny * side * collector_len,
                        );
                        let collector = line(junction, end);
                        let Some(cs) = score_path(&collector, bounds, &sample) else {
                            continue;
                        };
                        for local_len in [100, 96, 92] {
                            let tip = Position::new(
                                end.x + tx * along * local_len,
                                end.y + ty * along * local_len,
                            );
                            let local = line(end, tip);
                            let Some(ls) = score_path(&local, bounds, &sample) else {
                                continue;
                            };
                            let key =
                                ((side + 1) * 100 + (along + 1) * 10 + collector_len + local_len)
                                    as u64;
                            branches.push((
                                cs + ls - (collector_len + local_len) as f32 * 0.001,
                                tie(seed, key),
                                collector.clone(),
                                local,
                            ));
                        }
                    }
                }
            }
            branches.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            for (_, _, collector, local) in branches {
                let local_junction = collector[collector.len() - 1];
                let turnaround = local[local.len() - 1];
                let straight =
                    |points: &[Position]| points.iter().map(|p| [p.x as f32, p.y as f32]).collect();
                let roads = vec![
                    Road {
                        class: RoadClass::Arterial,
                        turnaround: None,
                        bridge: None,
                        centerline: arterial.clone(),
                        continuous_centerline: continuous.clone(),
                    },
                    Road {
                        class: RoadClass::Collector,
                        turnaround: None,
                        bridge: None,
                        continuous_centerline: straight(&collector),
                        centerline: collector,
                    },
                    Road {
                        class: RoadClass::Local,
                        turnaround: Some(turnaround),
                        bridge: None,
                        continuous_centerline: straight(&local),
                        centerline: local,
                    },
                ];
                let mut plan = Self {
                    morphology: TownMorphology::HighwayRibbon,
                    requested_morphology: TownMorphology::HighwayRibbon,
                    fallback_reason: None,
                    junctions: vec![
                        RoadJunction {
                            road: 1,
                            sample: 0,
                            parent_road: 0,
                            parent_sample: continuous.len() / 2,
                        },
                        RoadJunction {
                            road: 2,
                            sample: 0,
                            parent_road: 1,
                            parent_sample: roads[1].continuous_centerline.len() - 1,
                        },
                    ],
                    roads,
                    frontages: Vec::new(),
                    core_anchor: junction,
                    collector_junction: junction,
                    local_junction,
                    turnaround,
                    raster: BTreeMap::new(),
                };
                plan.rasterize();
                if plan.raster().all(|(p, _)| {
                    bounds.contains(p)
                        && sample(p).is_some_and(|terrain| terrain.water_distance > 0.0)
                }) {
                    return Some(plan);
                }
            }
        }
        None
    }

    fn rasterize(&mut self) {
        // A tile belongs to a road when its center lies within a capsule
        // around a continuous segment. Earlier road classes take
        // priority at intersections; within a road the closest segment wins.
        let mut distances = BTreeMap::<(i32, i32), f32>::new();
        let mut road_order: Vec<_> = (0..self.roads.len()).collect();
        road_order.sort_by_key(|&i| match self.roads[i].class {
            RoadClass::Arterial => 0,
            RoadClass::Collector => 1,
            RoadClass::Local => 2,
        });
        for road_index in road_order {
            let road = &self.roads[road_index];
            let radius: f32 = if road.class == RoadClass::Arterial {
                2.5
            } else {
                2.0
            };
            for (segment_index, segment) in road.continuous_centerline.windows(2).enumerate() {
                let (a, b) = (segment[0], segment[1]);
                let min_x = (a[0].min(b[0]) - radius).ceil() as i32;
                let max_x = (a[0].max(b[0]) + radius).floor() as i32;
                let min_y = (a[1].min(b[1]) - radius).ceil() as i32;
                let max_y = (a[1].max(b[1]) + radius).floor() as i32;
                for y in min_y..=max_y {
                    for x in min_x..=max_x {
                        let (px, py) = project_segment(a, b, x as f32, y as f32);
                        let d = (px - x as f32).powi(2) + (py - y as f32).powi(2);
                        if d > radius * radius {
                            continue;
                        }
                        let key = (x, y);
                        let cell = RoadCell {
                            road_index,
                            segment_index,
                        };
                        match self.raster.get(&key) {
                            None => {
                                self.raster.insert(key, cell);
                                distances.insert(key, d);
                            }
                            Some(old) if old.road_index == road_index && d < distances[&key] => {
                                self.raster.insert(key, cell);
                                distances.insert(key, d);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        // Each dead-end road owns its own paved turnaround.
        for (road_index, road) in self.roads.iter().enumerate() {
            let Some(turnaround) = road.turnaround else {
                continue;
            };
            let segment_index = road.continuous_centerline.len() - 2;
            for y in turnaround.y - 2..=turnaround.y + 2 {
                for x in turnaround.x - 2..=turnaround.x + 2 {
                    self.raster.entry((x, y)).or_insert(RoadCell {
                        road_index,
                        segment_index,
                    });
                }
            }
        }
    }

    /// Add a connected branch after checking its complete dry road envelope.
    pub fn add_dry_branch(
        &mut self,
        class: RoadClass,
        parent_road: usize,
        parent_sample: usize,
        end: Position,
        bounds: GridRect,
        sample: &impl Fn(Position) -> Option<NaturalTerrainSample>,
    ) -> bool {
        let start = self.roads[parent_road].continuous_centerline[parent_sample];
        if start[0].fract() != 0.0 || start[1].fract() != 0.0 {
            return false;
        }
        let start = Position::new(start[0] as i32, start[1] as i32);
        let points = line(start, end);
        if points.len() < 2 || score_path(&points, bounds, sample).is_none() {
            return false;
        }
        let road = self.roads.len();
        self.junctions.push(RoadJunction {
            road,
            sample: 0,
            parent_road,
            parent_sample,
        });
        self.roads.push(Road {
            class,
            turnaround: Some(end),
            bridge: None,
            continuous_centerline: points.iter().map(|p| [p.x as f32, p.y as f32]).collect(),
            centerline: points,
        });
        self.raster.clear();
        self.rasterize();
        true
    }

    /// Append a surveyed route whose crossings have been validated by geography.
    pub fn add_surveyed_route(
        &mut self,
        class: RoadClass,
        parent_road: usize,
        parent_sample: usize,
        points: Vec<Position>,
        bridge: Option<BridgeSpan>,
        turnaround: bool,
    ) -> bool {
        if points.len() < 2 {
            return false;
        }
        let parent = self.roads[parent_road].continuous_centerline[parent_sample];
        if parent != [points[0].x as f32, points[0].y as f32] {
            return false;
        }
        let index = self.roads.len();
        self.junctions.push(RoadJunction {
            road: index,
            sample: 0,
            parent_road,
            parent_sample,
        });
        self.roads.push(Road {
            class,
            turnaround: turnaround.then(|| *points.last().unwrap()),
            bridge,
            continuous_centerline: points.iter().map(|p| [p.x as f32, p.y as f32]).collect(),
            centerline: points,
        });
        self.raster.clear();
        self.rasterize();
        true
    }

    pub fn road_at(&self, p: Position) -> Option<RoadCell> {
        self.raster.get(&(p.x, p.y)).copied()
    }

    pub fn raster(&self) -> impl Iterator<Item = (Position, RoadCell)> + '_ {
        self.raster
            .iter()
            .map(|(&(x, y), &cell)| (Position::new(x, y), cell))
    }

    /// Project a point onto the locally indexed centerline segment.
    pub fn projection(&self, tile: Position, x: f32, y: f32) -> Option<(f32, f32)> {
        let cell = self.road_at(tile)?;
        let points = &self.roads[cell.road_index].continuous_centerline;
        let a = points[cell.segment_index];
        let b = points[cell.segment_index + 1];
        Some(project_segment(a, b, x, y))
    }

    /// Canonical FNV-1a hash of the classified road raster.
    pub fn road_hash(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        for (p, cell) in self.raster() {
            for byte in
                p.x.to_le_bytes()
                    .into_iter()
                    .chain(p.y.to_le_bytes())
                    .chain([self.roads[cell.road_index].class as u8])
            {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
        hash
    }
    /// Hash the graph, continuous geometry, anchors, raster and reserved frontage.
    /// The frontage order is canonicalized by its stable ID.
    pub fn canonical_hash(&self) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        let mut add = |bytes: &[u8]| {
            for &b in bytes {
                h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
            }
        };
        for p in [
            self.core_anchor,
            self.collector_junction,
            self.local_junction,
            self.turnaround,
        ] {
            add(&p.x.to_le_bytes());
            add(&p.y.to_le_bytes());
        }
        add(&[self.morphology as u8, self.requested_morphology as u8]);
        if let Some(reason) = self.fallback_reason {
            add(reason.as_bytes());
        }
        for j in &self.junctions {
            for n in [j.road, j.sample, j.parent_road, j.parent_sample] {
                add(&(n as u64).to_le_bytes());
            }
        }
        for road in &self.roads {
            add(&[road.class as u8]);
            if let Some(p) = road.turnaround {
                add(&[1]);
                add(&p.x.to_le_bytes());
                add(&p.y.to_le_bytes());
            } else {
                add(&[0]);
            }
            if let Some(b) = road.bridge {
                add(&[1]);
                add(&(b.first_sample as u64).to_le_bytes());
                add(&(b.last_sample as u64).to_le_bytes());
            } else {
                add(&[0]);
            }
            add(&(road.continuous_centerline.len() as u64).to_le_bytes());
            for p in &road.continuous_centerline {
                add(&p[0].to_bits().to_le_bytes());
                add(&p[1].to_bits().to_le_bytes());
            }
        }
        for (p, cell) in self.raster() {
            add(&p.x.to_le_bytes());
            add(&p.y.to_le_bytes());
            add(&(cell.road_index as u64).to_le_bytes());
            add(&(cell.segment_index as u64).to_le_bytes());
        }
        let mut frontages: Vec<_> = self.frontages.iter().collect();
        frontages.sort_by_key(|f| f.id);
        for f in frontages {
            add(&f.id.to_le_bytes());
            for p in [
                f.bounds.min,
                f.bounds.max,
                f.connector.road,
                f.connector.frontage,
            ] {
                add(&p.x.to_le_bytes());
                add(&p.y.to_le_bytes());
            }
            add(&[f.side as u8, f.road_class as u8, f.use_hint as u8]);
            add(&f.suitability.to_bits().to_le_bytes());
            add(&f.foundation_z.to_bits().to_le_bytes());
        }
        h
    }
}

fn project_segment(a: [f32; 2], b: [f32; 2], x: f32, y: f32) -> (f32, f32) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length2 = dx * dx + dy * dy;
    let t = if length2 == 0.0 {
        0.0
    } else {
        (((x - a[0]) * dx + (y - a[1]) * dy) / length2).clamp(0.0, 1.0)
    };
    (a[0] + dx * t, a[1] + dy * t)
}

fn has_arterial_curvature(line: &[[f32; 2]]) -> bool {
    line.windows(3).all(|points| {
        let [a, b, c] = [points[0], points[1], points[2]];
        let ab = (b[0] - a[0], b[1] - a[1]);
        let bc = (c[0] - b[0], c[1] - b[1]);
        let ac = (c[0] - a[0], c[1] - a[1]);
        let twice_area = (ab.0 * ac.1 - ab.1 * ac.0).abs();
        let lengths = ab.0.hypot(ab.1) * bc.0.hypot(bc.1) * ac.0.hypot(ac.1);
        // Circumcircle radius through each consecutive triple, in tiles.
        lengths >= 2.0 * twice_area * 18.0
    })
}

fn score_path(
    path: &[Position],
    bounds: GridRect,
    sample: &impl Fn(Position) -> Option<NaturalTerrainSample>,
) -> Option<f32> {
    let mut score = 0.0;
    let mut previous: Option<f32> = None;
    for (i, &p) in path.iter().enumerate() {
        // Check the entire road footprint for water, including its shoulders.
        for oy in -2..=2 {
            for ox in -2..=2 {
                let q = Position::new(p.x + ox, p.y + oy);
                if !bounds.contains(q) {
                    return None;
                }
                let s = sample(q)?;
                if s.water_distance <= 0.0 {
                    return None;
                }
                if ox == 0 && oy == 0 {
                    if let Some(z) = previous {
                        if (s.height - z).abs() > 2.0 {
                            return None;
                        }
                        score += (s.height - z).abs() * 0.2;
                    }
                    previous = Some(s.height);
                    score += s.slope * 0.1 + (1.0 - s.suitability).max(0.0) * 0.01;
                }
            }
        }
        // Normalize for small differences in rasterized diagonal lengths.
        if i > 0 {
            score += 0.001;
        }
    }
    Some(score / path.len().max(1) as f32)
}

fn line(a: Position, b: Position) -> Vec<Position> {
    let steps = (b.x - a.x).abs().max((b.y - a.y).abs());
    (0..=steps)
        .map(|i| {
            if steps == 0 {
                return a;
            }
            Position::new(a.x + (b.x - a.x) * i / steps, a.y + (b.y - a.y) * i / steps)
        })
        .collect()
}

fn tie(seed: u64, key: u64) -> u64 {
    let mut n = seed ^ key.wrapping_mul(0x9e3779b97f4a7c15);
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
    n ^ (n >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashSet, VecDeque};

    fn flat(p: Position) -> Option<NaturalTerrainSample> {
        Some(NaturalTerrainSample {
            height: 10.0,
            slope: 0.0,
            water_distance: if (130..=134).contains(&p.y) {
                0.0
            } else {
                50.0
            },
            suitability: 1.0,
        })
    }

    #[test]
    fn ribbon_is_deterministic_dry_and_connected() {
        let bounds = GridRect::new(Position::new(0, 0), Position::new(200, 200));
        let portals = [Position::new(10, 70), Position::new(190, 70)];
        let plan = TownPlan::build(42, bounds, portals, flat).unwrap();
        assert_eq!(
            plan.road_hash(),
            TownPlan::build(42, bounds, portals, flat)
                .unwrap()
                .road_hash()
        );
        assert_eq!(plan.roads[1].centerline[0], plan.collector_junction);
        let junction = [
            plan.collector_junction.x as f32,
            plan.collector_junction.y as f32,
        ];
        assert!(plan.roads[0].continuous_centerline.contains(&junction));
        assert_eq!(plan.roads[1].continuous_centerline[0], junction);
        assert_eq!(
            plan.roads[1].continuous_centerline.last(),
            plan.roads[2].continuous_centerline.first()
        );
        assert_eq!(
            plan.canonical_hash(),
            TownPlan::build(42, bounds, portals, flat)
                .unwrap()
                .canonical_hash()
        );
        let mut moved = plan.clone();
        moved.turnaround.x += 1;
        assert_ne!(plan.canonical_hash(), moved.canonical_hash());
        assert!(plan.roads[0].centerline.contains(&plan.collector_junction));
        assert_eq!(plan.roads[2].centerline[0], plan.local_junction);
        assert_eq!(
            *plan.roads[1].centerline.last().unwrap(),
            plan.local_junction
        );
        assert_eq!(*plan.roads[2].centerline.last().unwrap(), plan.turnaround);
        assert!(
            plan.roads[0]
                .continuous_centerline
                .iter()
                .any(|p| (p[1] - 70.0).abs() >= 3.0)
        );
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([portals[0]]);
        seen.insert(portals[0]);
        while let Some(p) = queue.pop_front() {
            for (dx, dy) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
                let q = Position::new(p.x + dx, p.y + dy);
                if plan.road_at(q).is_some() && seen.insert(q) {
                    queue.push_back(q);
                }
            }
        }
        assert_eq!(seen.len(), plan.raster().count());
        assert!(seen.contains(&plan.turnaround));
        for (p, cell) in plan.raster() {
            assert!(flat(p).unwrap().water_distance > 0.0);
            assert!(
                cell.segment_index + 1 < plan.roads[cell.road_index].continuous_centerline.len()
            );
            assert!(plan.projection(p, p.x as f32, p.y as f32).is_some());
        }
        for dy in -2..=2 {
            for dx in -2..=2 {
                assert!(
                    plan.road_at(Position::new(
                        plan.turnaround.x + dx,
                        plan.turnaround.y + dy
                    ))
                    .is_some()
                );
            }
        }
    }
    #[test]
    fn arterial_finds_a_dry_detour_around_a_lake() {
        let bounds = GridRect::new(Position::new(0, 0), Position::new(255, 255));
        let portals = [Position::new(10, 100), Position::new(230, 100)];
        let lake = GridRect::new(Position::new(100, 75), Position::new(120, 125));
        let sample = |p: Position| {
            Some(NaturalTerrainSample {
                height: 1.0,
                slope: 0.0,
                water_distance: if lake.contains(p) { 0.0 } else { 10.0 },
                suitability: 1.0,
            })
        };
        let plan = TownPlan::build(42, bounds, portals, sample).expect("space for a dry detour");
        assert_eq!(plan.roads[0].centerline.first(), Some(&portals[0]));
        assert_eq!(plan.roads[0].centerline.last(), Some(&portals[1]));
        assert!(plan.raster().all(|(p, _)| !lake.contains(p)));
    }
}
