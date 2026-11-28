//! Small-scale outdoor details, applied after buildings and their routes exist.
use super::{District, Feature, Ground, Lot, ParcelTiles, Position, Sites, noise};

fn inside_building(p: Position, sites: &Sites) -> bool {
    sites
        .buildings
        .iter()
        .any(|b| p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y)
}

fn near_entry(p: Position, sites: &Sites) -> bool {
    sites
        .buildings
        .iter()
        .any(|b| p.x.abs_diff(b.entry.x) + p.y.abs_diff(b.entry.y) <= 3)
}

/// Dressing never changes roads, water, buildings, or a site's existing paths.
pub(super) fn generate(seed: u64, lot: &Lot, sites: &Sites, tiles: &mut ParcelTiles) {
    let front_is_x = lot.front.x == lot.min.x || lot.front.x == lot.max.x;
    let depth = |p: Position| -> i32 {
        if front_is_x {
            (p.x - lot.front.x).abs()
        } else {
            (p.y - lot.front.y).abs()
        }
    };
    let along = |p: Position| -> i32 {
        if front_is_x {
            (p.y - lot.front.y).abs()
        } else {
            (p.x - lot.front.x).abs()
        }
    };
    for y in lot.min.y..=lot.max.y {
        for x in lot.min.x..=lot.max.x {
            let p = Position::new(x, y);
            let t = &mut tiles[p];
            if t.water.is_some() || t.ground.is_road() || inside_building(p, sites) {
                continue;
            }
            // Preserve the paths laid out by the site generator. Their
            // material still contributes to the appearance of an approach.
            if t.feature == Feature::Path {
                t.ground = match lot.district {
                    District::City | District::Hospital | District::Police => Ground::Pavement,
                    _ => Ground::Gravel,
                };
                continue;
            }
            if t.feature != Feature::Empty {
                continue;
            }
            let d = depth(p);
            let a = along(p);
            let texture = noise(seed, 301, x / 4, y / 4);
            // Open, walkable forecourts and yards, with irregular edges.
            match lot.district {
                District::City if d <= 4 || (d <= 8 && a <= 5) => t.ground = Ground::Pavement,
                District::Hospital if d <= 8 && a <= 9 + (texture % 3) as i32 => {
                    t.ground = Ground::Pavement
                }
                District::Police if d <= 7 && a <= 7 => t.ground = Ground::Pavement,
                District::MilitaryBase if d <= 10 && a <= 10 + (texture % 3) as i32 => {
                    t.ground = Ground::Gravel
                }
                District::Residential if d <= 4 && a <= 2 => t.ground = Ground::Gravel,
                District::Woods if d <= 7 && a <= 2 => t.ground = Ground::Gravel,
                _ => {}
            }
            // A short, broken rear fence suggests a yard without enclosing
            // the parcel or blocking access to neighboring lots.
            let rear = if front_is_x {
                p.x == if lot.front.x == lot.min.x {
                    lot.max.x
                } else {
                    lot.min.x
                }
            } else {
                p.y == if lot.front.y == lot.min.y {
                    lot.max.y
                } else {
                    lot.min.y
                }
            };
            if rear
                && lot.district == District::Residential
                && a > 4
                && a % 8 < 5
                && !near_entry(p, sites)
            {
                t.feature = Feature::Fence;
            }
            // Lamps and trees sit off routes and away from doors; unlike
            // perimeter fences they occupy only isolated yard tiles.
            let corner = (x == lot.min.x + 2 || x == lot.max.x - 2)
                && (y == lot.min.y + 2 || y == lot.max.y - 2);
            if corner && d > 3 && a > 4 && !near_entry(p, sites) {
                let choice = noise(seed, 302, x, y) % 5;
                match lot.district {
                    District::Residential | District::City if choice == 0 => {
                        t.feature = Feature::Streetlight
                    }
                    District::Residential | District::Woods if choice == 1 => {
                        t.feature = Feature::Tree
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{geography, sites};

    #[test]
    fn dressing_preserves_site_routes_and_road_water_and_adds_detail() {
        let mut lamps = 0;
        let mut trees = 0;
        let mut paving = 0;
        for seed in 0..4 {
            use crate::world::{Tile, WORLD_SIZE};
            let mut terrain = vec![Tile::default(); (WORLD_SIZE * WORLD_SIZE) as usize];
            let (_, lots) = geography::generate(seed, &mut terrain);
            for (i, lot) in lots.iter().enumerate() {
                let mut tiles = ParcelTiles::new(*lot, &terrain);
                let sites = sites::generate(seed, lot, i as u64, false, 0.0, &mut tiles);
                let before = tiles.clone();
                generate(seed, lot, &sites, &mut tiles);
                for y in lot.min.y..=lot.max.y {
                    for x in lot.min.x..=lot.max.x {
                        let p = Position::new(x, y);
                        let (old, new) = (before[p], tiles[p]);
                        if old.ground.is_road() || old.water.is_some() || inside_building(p, &sites)
                        {
                            assert_eq!(
                                old, new,
                                "protected site or terrain changed for seed {seed}"
                            );
                        }
                        if old.feature != Feature::Empty {
                            assert_eq!(old.feature, new.feature);
                        }
                        lamps += usize::from(new.feature == Feature::Streetlight);
                        trees += usize::from(new.feature == Feature::Tree);
                        paving += usize::from(old.ground != new.ground);
                    }
                }
            }
        }
        assert!(
            lamps > 0 && trees > 0 && paving > 0,
            "lamps={lamps}, trees={trees}, paving={paving}"
        );
    }
}
