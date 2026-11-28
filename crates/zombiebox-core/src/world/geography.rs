//! Seeded county geography, roads and buildable parcels.
use super::{
    District, Ground, Landmark, LandmarkKind, Lot, Position, Tile, WORLD_SIZE, Water, noise,
};

// Work in a local valley coordinate system, then rotate and reflect the entire
// plan. This keeps the watershed, streets and building frontages aligned.
struct Plan {
    seed: u64,
    rotation: u64,
    mirror: bool,
}
impl Plan {
    fn point(&self, mut x: i32, y: i32) -> Position {
        if self.mirror {
            x = WORLD_SIZE - 1 - x;
        }
        let n = WORLD_SIZE - 1;
        match self.rotation {
            0 => Position::new(x, y),
            1 => Position::new(n - y, x),
            2 => Position::new(n - x, n - y),
            _ => Position::new(y, n - x),
        }
    }
    fn index(&self, x: i32, y: i32) -> usize {
        let p = self.point(x, y);
        (p.y * WORLD_SIZE + p.x) as usize
    }
    fn river_x(&self, y: i32) -> i32 {
        let phase = (noise(self.seed, 2, 0, 0) % 80) as i32;
        // Interpolated coarse offsets form a continuous, winding channel.
        let k = (y + phase) / 64;
        let t = (y + phase) % 64;
        let a = (noise(self.seed, 3, k, 0) % 29) as i32 - 14;
        let b = (noise(self.seed, 3, k + 1, 0) % 29) as i32 - 14;
        355 + (a * (64 - t) + b * t) / 64
    }
    fn road(&self, tiles: &mut [Tile], x: i32, y: i32, ground: Ground) {
        if !(0..WORLD_SIZE).contains(&x) || !(0..WORLD_SIZE).contains(&y) {
            return;
        }
        let t = &mut tiles[self.index(x, y)];
        t.ground = match t.ground {
            Ground::River | Ground::Lake | Ground::Bridge => Ground::Bridge,
            _ => ground,
        };
        if t.elevation > 100 {
            t.elevation = 100;
        }
    }
    fn line(&self, tiles: &mut [Tile], a: (i32, i32), b: (i32, i32), ground: Ground) {
        let (mut x, mut y) = a;
        let dx = (b.0 - x).abs();
        let dy = (b.1 - y).abs();
        let sx = if x < b.0 { 1 } else { -1 };
        let sy = if y < b.1 { 1 } else { -1 };
        let mut err = dx - dy;
        loop {
            for oy in -2..=2 {
                for ox in -2..=2 {
                    self.road(tiles, x + ox, y + oy, ground);
                }
            }
            if (x, y) == b {
                break;
            }
            let e = 2 * err;
            if e > -dy {
                err -= dy;
                x += sx;
            }
            if e < dx {
                err += dx;
                y += sy;
            }
        }
    }
    fn lot(
        &self,
        tiles: &mut [Tile],
        lots: &mut Vec<Lot>,
        rect: (i32, i32, i32, i32),
        district: District,
        front: (i32, i32),
    ) {
        let (x0, y0, x1, y1) = rect;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let t = &mut tiles[self.index(x, y)];
                t.district = district;
                t.ground = Ground::Grass;
                let lot_rolling =
                    (((x as f32) / 14.0).sin() * 8.0) as i32 + field(self.seed, 6, x, y, 32) / 10;
                t.elevation = (80 + lot_rolling).clamp(55, 115) as u8;
            }
        }
        let corners = [
            self.point(x0, y0),
            self.point(x0, y1),
            self.point(x1, y0),
            self.point(x1, y1),
        ];
        let min = Position::new(
            corners.iter().map(|p| p.x).min().unwrap(),
            corners.iter().map(|p| p.y).min().unwrap(),
        );
        let max = Position::new(
            corners.iter().map(|p| p.x).max().unwrap(),
            corners.iter().map(|p| p.y).max().unwrap(),
        );
        lots.push(Lot {
            min,
            max,
            district,
            front: self.point(front.0, front.1),
        });
    }
    fn landmark(
        &self,
        tiles: &mut [Tile],
        kind: LandmarkKind,
        district: District,
        x: i32,
        y: i32,
    ) -> Landmark {
        tiles[self.index(x, y)].district = district;
        Landmark {
            kind,
            entrance: self.point(x, y),
        }
    }
}

// Bilinear interpolation avoids square-edged biome patches.
fn field(seed: u64, domain: u64, x: i32, y: i32, scale: i32) -> i32 {
    let (cx, cy) = (x / scale, y / scale);
    let (u, v) = (x % scale, y % scale);
    let n = |a, b| (noise(seed, domain, a, b) % 256) as i32;
    let a = n(cx, cy) * (scale - u) + n(cx + 1, cy) * u;
    let b = n(cx, cy + 1) * (scale - u) + n(cx + 1, cy + 1) * u;
    (a * (scale - v) + b * v) / (scale * scale)
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ValleyMeta {
    pub rotation: u64,
    pub mirror: bool,
    pub pass_y: i32,
    pub town_min: (i32, i32),
    pub town_max: (i32, i32),
    pub lake_center: (i32, i32),
    pub lake_radii: (i32, i32),
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn generate(seed: u64, tiles: &mut [Tile]) -> (Vec<Landmark>, Vec<Lot>) {
    let (landmarks, lots, _) = generate_with_meta(seed, tiles, false);
    (landmarks, lots)
}

pub(super) fn generate_with_meta(
    seed: u64,
    tiles: &mut [Tile],
    reserve_starting_houses: bool,
) -> (Vec<Landmark>, Vec<Lot>, ValleyMeta) {
    let plan = Plan {
        seed,
        rotation: noise(seed, 0, 0, 0) % 4,
        mirror: noise(seed, 1, 0, 0).is_multiple_of(2),
    };
    let lake_y = 365 + (noise(seed, 9, 0, 0) % 27) as i32;
    let lake_x = plan.river_x(lake_y);
    let rx = 34 + (noise(seed, 10, 0, 0) % 12) as i32;
    let ry = 33 + (noise(seed, 11, 0, 0) % 16) as i32;
    // The pass is a low saddle between two summits. Its eastern rim is
    // used below to land the trailhead on a dry walkable approach.
    let pass_y = 212 + (noise(seed, 33, 0, 0) % 49) as i32;
    let ridge_edge = |y: i32| {
        let spine = 25 + field(seed, 12, 0, y, 57) / 5;
        spine * ((y - pass_y).abs() - 5).clamp(0, 32) / 32
    };
    for y in 0..WORLD_SIZE {
        let mountain_edge = ridge_edge(y);
        let channel = plan.river_x(y);
        // The water surface falls toward the lake and the downstream edge.
        let water_level = 40 - y / 32;
        for x in 0..WORLD_SIZE {
            let t = &mut tiles[plan.index(x, y)];
            let ridge = x < mountain_edge;
            let bank_distance = (x - channel).abs();
            let river = bank_distance <= 4 + field(seed, 34, 0, y, 39) / 72;
            let dx = x - lake_x;
            let dy = y - lake_y;
            let local_rx = rx + (field(seed, 20, x, y, 32) - 128) / 24;
            let lake =
                dx * dx * ry * ry + dy * dy * local_rx * local_rx < local_rx * local_rx * ry * ry;
            let woodland =
                field(seed, 5, x, y, 78) + (x - 365).clamp(0, 70) + (115 - y).clamp(0, 48) > 146;
            let cultivated = y + (field(seed, 35, x, y, 75) - 128) / 2 > 320;
            t.water = if lake {
                Some(Water::Lake)
            } else if river {
                Some(Water::River)
            } else {
                None
            };
            t.ground = if ridge {
                Ground::Mountain
            } else if lake {
                Ground::Lake
            } else if river {
                Ground::River
            } else if woodland {
                Ground::Forest
            } else if cultivated {
                Ground::Field
            } else {
                Ground::Grass
            };
            t.district = if woodland {
                District::Woods
            } else {
                District::Farm
            };
            let floodplain = water_level + (bank_distance - 5).clamp(0, 36);
            let rolling = field(seed, 6, x, y, 64) / 14;
            let relief = (mountain_edge + 24 - x).max(0) * 2;
            t.elevation = if lake {
                (40 - lake_y / 32) as u8
            } else if river {
                water_level as u8
            } else {
                (floodplain + rolling * bank_distance.min(30) / 30 + relief).clamp(0, 255) as u8
            };
        }
    }
    // Independent block widths give each seed its own network and frontage.
    let mut xs = [0; 6];
    let mut ys = [0; 6];
    xs[0] = 130 + (noise(seed, 14, 0, 0) % 13) as i32;
    ys[0] = 150 + (noise(seed, 15, 0, 0) % 17) as i32;
    for i in 1..6 {
        xs[i] = xs[i - 1] + 32 + (noise(seed, 16, i as i32, 0) % 7) as i32;
        ys[i] = ys[i - 1] + 32 + (noise(seed, 17, i as i32, 0) % 7) as i32;
    }
    // The town sits on the broad shelf between the ridge and river. Civic
    // frontages keep their central neighbors as the outer city corners vary.
    // Leave both banks intact. A large southern lake can extend into the
    // proposed town apron. Slide the whole street plan up-valley to the
    // nearest dry shelf, preserving the road widths and lot frontages.
    let bank_setback = (xs[5] - 330).max(0);
    for x in &mut xs {
        *x -= bank_setback;
    }
    while (ys[0] - 2..=ys[5] + 2)
        .any(|y| (xs[0] - 2..=xs[5] + 2).any(|x| tiles[plan.index(x, y)].water.is_some()))
    {
        for y in &mut ys {
            *y -= 4;
        }
    }
    let hospital_col = if noise(seed, 24, 0, 0).is_multiple_of(2) {
        0
    } else {
        4
    };
    let police_col = 4 - hospital_col;
    let city_block = |col: usize, row: usize| {
        (1..=3).contains(&col)
            && (1..=3).contains(&row)
            && (col == 2
                || row >= 2
                || col == police_col.clamp(1, 3)
                || !noise(seed, 36, col as i32, row as i32).is_multiple_of(3))
    };
    // Roads and their adjacent blocks share the same town footprint.
    for y in ys[0] - 2..=ys[5] + 2 {
        for x in xs[0] - 2..=xs[5] + 2 {
            let col = xs.partition_point(|&v| v <= x).saturating_sub(1);
            let row = ys.partition_point(|&v| v <= y).saturating_sub(1);
            let city = city_block(col, row);
            let t = &mut tiles[plan.index(x, y)];
            t.district = if city {
                District::City
            } else {
                District::Residential
            };
            t.ground = Ground::Grass;
            let town_rolling =
                (((x as f32) / 14.0).sin() * 8.0) as i32 + field(seed, 6, x, y, 32) / 10;
            t.elevation = (80 + town_rolling).clamp(55, 115) as u8;
            debug_assert!(t.water.is_none(), "town shelf must be above water");
        }
    }
    // Selected suburban side streets end in pairs of short lanes. Lots
    // retain their cross-street entrances, and the central grid stays open.
    for (col, &x) in xs.iter().enumerate() {
        for row in 0..ys.len() - 1 {
            let (start, end) = (ys[row], ys[row + 1]);
            let lane = noise(seed, 38, col as i32, row as i32);
            if (1..5).contains(&col) && (row == 0 || row == 4) && lane.is_multiple_of(2) {
                let tip = start + 7 + (lane % 9) as i32;
                plan.line(tiles, (x, start), (x, tip), Ground::Street);
                plan.line(tiles, (x, end - 6), (x, end), Ground::Street);
            } else {
                plan.line(tiles, (x, start), (x, end), Ground::Street);
            }
        }
    }
    for &y in &ys {
        plan.line(tiles, (xs[0], y), (xs[5], y), Ground::Street);
    }
    let arterial = ys[3];
    let cabin_x = 412 + (noise(seed, 21, 0, 0) % 21) as i32;
    let base_x = 90 + (noise(seed, 22, 0, 0) % 3) as i32;
    let base_y = 330 + (noise(seed, 23, 0, 0) % 22) as i32;
    plan.line(tiles, (85, arterial), (cabin_x, arterial), Ground::Road);
    // Seed-varied rural branch ends in a quiet cul-de-sac.
    let branch_y = ys[5] + 18 + (noise(seed, 18, 0, 0) % 20) as i32;
    let branch_x = xs[1] + (noise(seed, 19, 0, 0) % ((xs[4] - xs[1]) as u64)) as i32;
    plan.line(
        tiles,
        (branch_x, ys[5]),
        (branch_x, branch_y),
        Ground::Street,
    );
    plan.line(
        tiles,
        (branch_x - 8, branch_y),
        (branch_x + 8, branch_y),
        Ground::Street,
    );
    plan.line(tiles, (cabin_x, arterial), (cabin_x, 225), Ground::Trail);
    let bend_x = 74 + (noise(seed, 25, 0, 0) % 12) as i32;
    plan.line(
        tiles,
        (85, arterial),
        (bend_x, (arterial + base_y) / 2),
        Ground::Road,
    );
    plan.line(
        tiles,
        (bend_x, (arterial + base_y) / 2),
        (85, base_y),
        Ground::Road,
    );
    plan.line(tiles, (85, base_y), (base_x + 15, base_y), Ground::Road);
    let mut shore_x = lake_x;
    while tiles[plan.index(shore_x, lake_y)].water == Some(Water::Lake) {
        shore_x += 1;
    }
    shore_x += 2;
    plan.line(tiles, (cabin_x, arterial), (cabin_x, lake_y), Ground::Trail);
    plan.line(tiles, (cabin_x, lake_y), (shore_x, lake_y), Ground::Trail);
    if noise(seed, 26, 0, 0).is_multiple_of(2) {
        let crossing_y = ys[0] - 15 - (noise(seed, 37, 0, 0) % 22) as i32;
        let east_bank = cabin_x - 12;
        plan.line(
            tiles,
            (xs[5], ys[0]),
            (xs[5] + 12, crossing_y + 5),
            Ground::Road,
        );
        plan.line(
            tiles,
            (xs[5] + 12, crossing_y + 5),
            (east_bank, crossing_y),
            Ground::Road,
        );
        plan.line(
            tiles,
            (east_bank, crossing_y),
            (cabin_x + 22, crossing_y + 28),
            Ground::Road,
        );
        plan.line(
            tiles,
            (cabin_x + 22, crossing_y + 28),
            (cabin_x + 20, 225),
            Ground::Road,
        );
        plan.line(tiles, (cabin_x + 20, 225), (cabin_x, 225), Ground::Road);
    }
    let trail_y = pass_y - 20;
    let trail_x = ridge_edge(trail_y) + 3;
    plan.line(tiles, (85, arterial), (55, pass_y), Ground::Trail);
    plan.line(tiles, (55, pass_y), (trail_x, trail_y), Ground::Trail);
    let mut lots = Vec::new();
    for row in 0..5 {
        for col in 0..5 {
            let (x, y) = (xs[col], ys[row]);
            let (right, bottom) = (xs[col + 1], ys[row + 1]);
            let district = if (row, col) == (3, hospital_col) {
                District::Hospital
            } else if (row, col) == (1, police_col) {
                District::Police
            } else if city_block(col, row) {
                District::City
            } else {
                District::Residential
            };
            // Keep the starting street's five blocks deep enough for the
            // furnished families, including the two larger footprints.
            let starter_frontage = reserve_starting_houses && row == 0;
            if district == District::Residential
                && !starter_frontage
                && (bottom - y >= 33)
                && !noise(seed, 31, col as i32, row as i32).is_multiple_of(3)
            {
                let middle = (y + bottom) / 2;
                plan.lot(
                    tiles,
                    &mut lots,
                    (x + 3, y + 3, right - 3, middle),
                    district,
                    ((x + right) / 2, y + 3),
                );
                plan.lot(
                    tiles,
                    &mut lots,
                    (x + 3, middle + 1, right - 3, bottom - 3),
                    district,
                    ((x + right) / 2, bottom - 3),
                );
            } else {
                plan.lot(
                    tiles,
                    &mut lots,
                    (x + 3, y + 3, right - 3, bottom - 3),
                    district,
                    ((x + right) / 2, y + 3),
                );
            }
        }
    }
    let base_w = 30 + (noise(seed, 8, 0, 0) % 5) as i32;
    plan.lot(
        tiles,
        &mut lots,
        (base_x, base_y + 3, base_x + base_w, base_y + 32),
        District::MilitaryBase,
        (base_x + 10, base_y + 3),
    );
    // Cabin clearings remain surrounded by actual woodland even where the
    // regional noise happens to favor grass. Additional lanes keep their
    // parcels well east of the lake and the main clearing.
    let mut cabins = vec![(cabin_x, 198, 222)];
    let extras = (noise(seed, 32, 0, 0) % 3) as usize;
    if extras > 0 {
        plan.line(tiles, (cabin_x, arterial), (480, arterial), Ground::Trail);
        plan.line(tiles, (480, arterial), (480, 77), Ground::Trail);
        plan.line(tiles, (480, 77), (460, 77), Ground::Trail);
        cabins.push((460, 50, 74));
    }
    if extras > 1 {
        plan.line(tiles, (480, arterial), (480, 327), Ground::Trail);
        plan.line(tiles, (480, 327), (460, 327), Ground::Trail);
        cabins.push((460, 300, 324));
    }
    for (cx, top, bottom) in cabins {
        let cy = (top + bottom) / 2;
        for y in cy - 22..=cy + 22 {
            for x in cx - 23..=cx + 23 {
                let t = &mut tiles[plan.index(x, y)];
                if (x - cx) * (x - cx) * 400 + (y - cy) * (y - cy) * 529 < 529 * 484
                    && t.water.is_none()
                    && !t.ground.is_road()
                    && !((cx - 13..=cx + 13).contains(&x) && (top..=bottom).contains(&y))
                {
                    t.ground = Ground::Forest;
                    t.district = District::Woods;
                }
            }
        }
        plan.lot(
            tiles,
            &mut lots,
            (cx - 13, top, cx + 13, bottom),
            District::Woods,
            (cx, bottom),
        );
    }
    let hospital_x = (xs[hospital_col] + xs[hospital_col + 1]) / 2;
    let police_x = (xs[police_col] + xs[police_col + 1]) / 2;
    let landmarks = vec![
        plan.landmark(
            tiles,
            LandmarkKind::Residential,
            District::Residential,
            (xs[0] + xs[1]) / 2,
            ys[0],
        ),
        plan.landmark(
            tiles,
            LandmarkKind::Downtown,
            District::City,
            (xs[2] + xs[3]) / 2,
            ys[2],
        ),
        plan.landmark(
            tiles,
            LandmarkKind::Hospital,
            District::Hospital,
            hospital_x,
            ys[3],
        ),
        plan.landmark(
            tiles,
            LandmarkKind::Police,
            District::Police,
            police_x,
            ys[1],
        ),
        plan.landmark(
            tiles,
            LandmarkKind::ArmyBase,
            District::MilitaryBase,
            base_x + 10,
            base_y,
        ),
        plan.landmark(tiles, LandmarkKind::Cabin, District::Woods, cabin_x, 225),
        plan.landmark(tiles, LandmarkKind::Lake, District::Woods, shore_x, lake_y),
        plan.landmark(
            tiles,
            LandmarkKind::Mountain,
            District::Farm,
            trail_x,
            trail_y,
        ),
    ];
    let meta = ValleyMeta {
        rotation: plan.rotation,
        mirror: plan.mirror,
        pass_y,
        town_min: (xs[0], ys[0]),
        town_max: (xs[5], ys[5]),
        lake_center: (lake_x, lake_y),
        lake_radii: (rx, ry),
    };
    (landmarks, lots, meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn county_roads_frontages_and_natural_landmarks_across_seeds() {
        for seed in 0..16 {
            let mut tiles = vec![Tile::default(); (WORLD_SIZE * WORLD_SIZE) as usize];
            let (landmarks, lots) = generate(seed, &mut tiles);
            let index = |p: Position| (p.y * WORLD_SIZE + p.x) as usize;
            let mut seen = vec![false; tiles.len()];
            let mut queue = VecDeque::from([landmarks[0].entrance]);
            seen[index(landmarks[0].entrance)] = true;
            while let Some(p) = queue.pop_front() {
                for (dx, dy) in [(0, 1), (0, -1), (1, 0), (-1, 0)] {
                    let q = Position::new(p.x + dx, p.y + dy);
                    if (0..WORLD_SIZE).contains(&q.x)
                        && (0..WORLD_SIZE).contains(&q.y)
                        && !seen[index(q)]
                        && tiles[index(q)].ground.is_road()
                    {
                        seen[index(q)] = true;
                        queue.push_back(q);
                    }
                }
            }
            for landmark in &landmarks {
                assert!(
                    tiles[index(landmark.entrance)].ground.is_road()
                        && seen[index(landmark.entrance)],
                    "seed {seed}: {:?}",
                    landmark.kind
                );
            }
            let gap = |a: &Lot, b: &Lot| {
                let dx = (a.min.x - b.max.x - 1).max(b.min.x - a.max.x - 1).max(0);
                let dy = (a.min.y - b.max.y - 1).max(b.min.y - a.max.y - 1).max(0);
                dx + dy
            };
            for civic in [District::Hospital, District::Police] {
                let compound = lots.iter().find(|lot| lot.district == civic).unwrap();
                for neighbor in [District::City, District::Residential] {
                    assert!(
                        lots.iter()
                            .any(|lot| lot.district == neighbor && gap(compound, lot) <= 5),
                        "seed {seed} {civic:?} away from {neighbor:?}"
                    );
                }
            }
            let base = lots
                .iter()
                .find(|lot| lot.district == District::MilitaryBase)
                .unwrap();
            assert!(
                lots.iter()
                    .filter(|lot| lot.district == District::City)
                    .all(|lot| gap(base, lot) > 16)
            );
            for (i, lot) in lots.iter().enumerate() {
                for other in &lots[i + 1..] {
                    assert!(
                        lot.max.x < other.min.x
                            || other.max.x < lot.min.x
                            || lot.max.y < other.min.y
                            || other.max.y < lot.min.y,
                        "seed {seed}: overlapping lots"
                    );
                }
                if lot.district == District::Woods {
                    let count = (lot.min.y.saturating_sub(8)..=(lot.max.y + 8).min(WORLD_SIZE - 1))
                        .flat_map(|y| {
                            (lot.min.x.saturating_sub(8)..=(lot.max.x + 8).min(WORLD_SIZE - 1))
                                .map(move |x| Position::new(x, y))
                        })
                        .filter(|p| tiles[index(*p)].ground == Ground::Forest)
                        .count();
                    assert!(count > 30, "seed {seed}: cabin lacks woodland");
                }
                let p = lot.front;
                assert!(
                    p.x == lot.min.x || p.x == lot.max.x || p.y == lot.min.y || p.y == lot.max.y
                );
                assert!(
                    [(0, 1), (0, -1), (1, 0), (-1, 0)].iter().any(|(dx, dy)| {
                        let q = Position::new(p.x + dx, p.y + dy);
                        tiles[index(q)].ground.is_road() && seen[index(q)]
                    }),
                    "seed {seed}: {:?} frontage",
                    lot.district
                );
            }
            for (kind, water) in [(LandmarkKind::Lake, true), (LandmarkKind::Mountain, false)] {
                let p = landmarks.iter().find(|l| l.kind == kind).unwrap().entrance;
                assert!(
                    (-5..=5).any(|dy| (-5..=5).any(|dx| {
                        let t = tiles[index(Position::new(p.x + dx, p.y + dy))];
                        if water {
                            t.water == Some(Water::Lake)
                        } else {
                            t.ground == Ground::Mountain
                        }
                    })),
                    "seed {seed}: {kind:?} trailhead"
                );
            }
            assert!(
                tiles
                    .iter()
                    .any(|t| t.ground == Ground::Bridge && t.water == Some(Water::River)),
                "seed {seed}: river bridge"
            );
            assert!(
                tiles.iter().all(|t| !t.ground.is_road()
                    || t.water.is_none()
                    || t.ground == Ground::Bridge)
            );
        }
    }
}
