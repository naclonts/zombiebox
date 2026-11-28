//! Seeded buildings, interior detail and district-specific encounters.
use super::{District, Ground, Lot, ParcelTiles, Position, WORLD_SIZE, noise};
use crate::action::{Material, MonsterKind};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Feature {
    #[default]
    Empty,
    Floor,
    Wall(Material),
    Door,
    Window,
    Stairs,
    Furniture,
    Fence,
    Tree,
    Streetlight,
    Gate,
    Path,
}
impl Feature {
    pub const fn symbol(self) -> char {
        match self {
            Self::Empty => '.',
            Self::Floor => '_',
            Self::Wall(Material::Drywall) => '|',
            Self::Wall(_) => '#',
            Self::Door => '+',
            Self::Window => 'w',
            Self::Stairs => '^',
            Self::Furniture => 'o',
            Self::Fence => '=',
            Self::Tree => 'T',
            Self::Streetlight => '!',
            Self::Gate => '+',
            Self::Path => '.',
        }
    }
    pub const fn walkable(self) -> bool {
        matches!(
            self,
            Self::Empty | Self::Floor | Self::Door | Self::Stairs | Self::Gate | Self::Path
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildingKind {
    Bungalow,
    Duplex,
    SplitLevel,
    Storefront,
    Office,
    Apartments,
    Hospital,
    Precinct,
    ArmyBase,
    Cabin,
    Barn,
}
impl BuildingKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bungalow => "Bungalow",
            Self::Duplex => "Duplex",
            Self::SplitLevel => "Split-level",
            Self::Storefront => "Storefront",
            Self::Office => "Office",
            Self::Apartments => "Apartments",
            Self::Hospital => "Hospital",
            Self::Precinct => "Precinct",
            Self::ArmyBase => "Army base",
            Self::Cabin => "Cabin",
            Self::Barn => "Barn",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoomKind {
    Living,
    Kitchen,
    Bedroom,
    Bathroom,
    Hall,
    Shop,
    Office,
    Apartment,
    Lobby,
    Ward,
    Pharmacy,
    Cells,
    Armory,
    Garage,
    Barracks,
    MotorPool,
    Storage,
    Dining,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Room {
    pub min: Position,
    pub max: Position,
    pub floor: u8,
    pub kind: RoomKind,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Building {
    pub min: Position,
    pub max: Position,
    pub floors: u8,
    pub district: District,
    pub entry: Position,
    pub kind: BuildingKind,
    pub material: Material,
    pub foundation_z: f32,
    pub layout: Vec<Room>,
    pub wings: Vec<crate::action::WingRect>,
    pub(crate) door_axes: Vec<(Position, u8, crate::action::DoorAxis)>,
    // Compact, row-major floor maps; ground level is also copied into tiles.
    features: Vec<Vec<Feature>>,
}
impl Building {
    pub fn feature_at(&self, p: Position, floor: u8) -> Option<Feature> {
        if p.x < self.min.x || p.y < self.min.y || p.x > self.max.x || p.y > self.max.y {
            return None;
        }
        let width = (self.max.x - self.min.x + 1) as usize;
        self.features
            .get(floor as usize)
            .map(|f| f[(p.y - self.min.y) as usize * width + (p.x - self.min.x) as usize])
    }

    pub fn door_axis_at(&self, p: Position, floor: u8) -> Option<crate::action::DoorAxis> {
        for &(pos, fl, axis) in &self.door_axes {
            if fl == floor && pos == p {
                return Some(axis);
            }
        }
        None
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LootKind {
    Household,
    Tools,
    Medkit,
    Firearm,
    Ammo,
}
impl LootKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Household => "Household",
            Self::Tools => "Tools",
            Self::Medkit => "Medkit",
            Self::Firearm => "Firearm",
            Self::Ammo => "Ammo",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LootSite {
    pub position: Position,
    pub floor: u8,
    pub kind: LootKind,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InfectedSite {
    pub position: Position,
    pub floor: u8,
    pub kind: MonsterKind,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sites {
    pub buildings: Vec<Building>,
    pub loot: Vec<LootSite>,
    pub infected: Vec<InfectedSite>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PlannedPad {
    pub min: Position,
    pub max: Position,
    pub entry: Position,
    pub front: Position,
}
impl Sites {
    /// Returns a building feature on the requested floor, if a building occupies it.
    pub fn feature_at(&self, p: Position, floor: u8) -> Option<Feature> {
        self.buildings.iter().find_map(|b| b.feature_at(p, floor))
    }
}

fn road(g: Ground) -> bool {
    matches!(
        g,
        Ground::Road | Ground::Street | Ground::Trail | Ground::Bridge
    )
}
fn kind(d: District, n: u64) -> BuildingKind {
    match d {
        District::Residential => [
            BuildingKind::Bungalow,
            BuildingKind::Duplex,
            BuildingKind::SplitLevel,
        ][(n % 3) as usize],
        District::City => [
            BuildingKind::Storefront,
            BuildingKind::Office,
            BuildingKind::Apartments,
        ][(n % 3) as usize],
        District::Hospital => BuildingKind::Hospital,
        District::Police => BuildingKind::Precinct,
        District::MilitaryBase => BuildingKind::ArmyBase,
        District::Farm => BuildingKind::Barn,
        District::Woods => BuildingKind::Cabin,
    }
}
fn room_kinds(k: BuildingKind, floor: u8) -> [RoomKind; 4] {
    use RoomKind::*;
    if floor > 0 {
        match k {
            BuildingKind::Duplex | BuildingKind::SplitLevel => {
                return [Bedroom, Living, Bedroom, Storage];
            }
            BuildingKind::Storefront => return [Office, Storage, Apartment, Apartment],
            BuildingKind::Office => return [Office, Office, Storage, Office],
            BuildingKind::Hospital => return [Ward, Ward, Pharmacy, Storage],
            BuildingKind::Precinct => return [Office, Cells, Armory, Storage],
            _ => {}
        }
    }
    match k {
        BuildingKind::Bungalow
        | BuildingKind::Duplex
        | BuildingKind::SplitLevel
        | BuildingKind::Cabin => [Living, Kitchen, Bedroom, Bedroom],
        BuildingKind::Storefront => [Shop, Storage, Office, Shop],
        BuildingKind::Office => [Lobby, Office, Office, Storage],
        BuildingKind::Apartments => [Lobby, Apartment, Apartment, Apartment],
        BuildingKind::Hospital => [Lobby, Ward, Pharmacy, Ward],
        BuildingKind::Precinct => [Lobby, Cells, Armory, Garage],
        BuildingKind::ArmyBase => [Lobby, Barracks, Armory, MotorPool],
        BuildingKind::Barn => [Storage, Storage, Garage, Storage],
    }
}
fn set(map: &mut [Feature], min: Position, width: i32, p: Position, f: Feature) {
    map[((p.y - min.y) * width + p.x - min.x) as usize] = f;
}

fn footprint(lot: &Lot, kind: BuildingKind, n: u64) -> (Position, Position) {
    let available_x = lot.max.x - lot.min.x + 1;
    let available_y = lot.max.y - lot.min.y + 1;
    let (low, high, setback) = match kind {
        BuildingKind::Bungalow | BuildingKind::Duplex | BuildingKind::SplitLevel => (10, 16, 4),
        BuildingKind::Cabin | BuildingKind::Barn => (8, 12, 4),
        BuildingKind::Storefront | BuildingKind::Office | BuildingKind::Apartments => (18, 25, 2),
        _ => (24, 32, 3),
    };
    let width = (low + (n % (high - low + 1) as u64) as i32).min(available_x - 2);
    let height = (low + ((n >> 8) % (high - low + 1) as u64) as i32).min(available_y - 2);
    let room_x = available_x - width;
    let room_y = available_y - height;
    let mut ox = 1 + ((n >> 16) % (room_x - 1).max(1) as u64) as i32;
    let mut oy = 1 + ((n >> 24) % (room_y - 1).max(1) as u64) as i32;
    if lot.front.x == lot.min.x {
        ox = setback.min(room_x - 1).max(1);
    } else if lot.front.x == lot.max.x {
        ox = room_x - setback.min(room_x - 1).max(1);
    } else if lot.front.y == lot.min.y {
        oy = setback.min(room_y - 1).max(1);
    } else {
        oy = room_y - setback.min(room_y - 1).max(1);
    }
    let min = Position::new(lot.min.x + ox, lot.min.y + oy);
    (min, Position::new(min.x + width - 1, min.y + height - 1))
}

// Open interior partitions until every room is connected to its entrance or stairs.
fn connect_floor(map: &mut [Feature], min: Position, max: Position, start: Position) {
    let w = max.x - min.x + 1;
    loop {
        let mut seen = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::from([start]);
        while let Some(p) = queue.pop_front() {
            if p.x < min.x || p.x > max.x || p.y < min.y || p.y > max.y || !seen.insert(p) {
                continue;
            }
            let f = map[((p.y - min.y) * w + p.x - min.x) as usize];
            if f == Feature::Empty || !f.walkable() {
                continue;
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                queue.push_back(Position::new(p.x + dx, p.y + dy));
            }
        }
        let reachable = |p: Position| -> bool {
            if p.x < min.x || p.x > max.x || p.y < min.y || p.y > max.y {
                return false;
            }
            seen.contains(&p)
                && matches!(
                    map[((p.y - min.y) * w + p.x - min.x) as usize],
                    Feature::Floor | Feature::Door | Feature::Stairs | Feature::Gate
                )
        };
        let mut opening = None;
        'scan: for y in min.y + 1..max.y {
            for x in min.x + 1..max.x {
                let p = Position::new(x, y);
                if !matches!(
                    map[((y - min.y) * w + x - min.x) as usize],
                    Feature::Wall(Material::Drywall | Material::MetalPole)
                ) {
                    continue;
                }
                for (dx, dy) in [(1, 0), (0, 1)] {
                    let a = Position::new(x - dx, y - dy);
                    let b = Position::new(x + dx, y + dy);
                    let inside =
                        |q: Position| q.x > min.x && q.x < max.x && q.y > min.y && q.y < max.y;
                    if inside(a) && inside(b) {
                        let af = map[((a.y - min.y) * w + a.x - min.x) as usize];
                        let bf = map[((b.y - min.y) * w + b.x - min.x) as usize];
                        if (reachable(a) && !reachable(b) && bf.walkable() && bf != Feature::Empty)
                            || (reachable(b)
                                && !reachable(a)
                                && af.walkable()
                                && af != Feature::Empty)
                        {
                            opening = Some(p);
                            break 'scan;
                        }
                    }
                }
            }
        }
        if let Some(p) = opening {
            set(map, min, w, p, Feature::Door);
        } else {
            break;
        }
    }
}

fn wall_at(map: &[Feature], min: Position, width: i32, p: Position) -> bool {
    matches!(
        map[((p.y - min.y) * width + p.x - min.x) as usize],
        Feature::Wall(_) | Feature::Window
    )
}

fn normalize_doors(
    map: &mut [Feature],
    min: Position,
    max: Position,
    floor: u8,
) -> Vec<(Position, u8, crate::action::DoorAxis)> {
    let width = max.x - min.x + 1;
    let mut axes = Vec::new();
    let doors: Vec<_> = (min.y..=max.y)
        .flat_map(|y| (min.x..=max.x).map(move |x| (x, y)))
        .filter_map(|(x, y)| {
            let feature = map[((y - min.y) * width + x - min.x) as usize];
            matches!(feature, Feature::Door | Feature::Gate).then_some((x, y, feature))
        })
        .collect();
    for (x, y, feature) in doors {
        let current = map[((y - min.y) * width + x - min.x) as usize];
        if current != feature {
            continue;
        }
        let x_walls = x > min.x
            && x < max.x
            && wall_at(map, min, width, Position::new(x - 1, y))
            && wall_at(map, min, width, Position::new(x + 1, y));
        let y_walls = y > min.y
            && y < max.y
            && wall_at(map, min, width, Position::new(x, y - 1))
            && wall_at(map, min, width, Position::new(x, y + 1));
        let axis = if feature == Feature::Gate && (x == min.x || x == max.x) {
            crate::action::DoorAxis::AlongY
        } else if feature == Feature::Gate && (y == min.y || y == max.y) {
            crate::action::DoorAxis::AlongX
        } else if x_walls && !y_walls {
            crate::action::DoorAxis::AlongX
        } else if y_walls && !x_walls {
            crate::action::DoorAxis::AlongY
        } else if x == min.x || x == max.x {
            crate::action::DoorAxis::AlongY
        } else {
            crate::action::DoorAxis::AlongX
        };
        let t = axis.tangent();
        let n = axis.normal();
        if feature == Feature::Gate {
            for (dx, dy) in [t, (-t.0, -t.1)] {
                let q = Position::new(x + dx, y + dy);
                if q.x >= min.x && q.x <= max.x && q.y >= min.y && q.y <= max.y {
                    let i = ((q.y - min.y) * width + q.x - min.x) as usize;
                    if map[i] == Feature::Door {
                        map[i] = Feature::Floor;
                    } else if !matches!(map[i], Feature::Window) {
                        map[i] = Feature::Wall(Material::Drywall);
                    }
                }
            }
            for (dx, dy) in [n, (-n.0, -n.1)] {
                let q = Position::new(x + dx, y + dy);
                if q.x > min.x && q.x < max.x && q.y > min.y && q.y < max.y {
                    let i = ((q.y - min.y) * width + q.x - min.x) as usize;
                    if matches!(
                        map[i],
                        Feature::Wall(_) | Feature::Window | Feature::Furniture
                    ) {
                        map[i] = Feature::Floor;
                    }
                }
            }
            axes.push((Position::new(x, y), floor, axis));
            continue;
        }
        for (dx, dy) in [t, (-t.0, -t.1)] {
            let q = Position::new(x + dx, y + dy);
            if q.x >= min.x && q.x <= max.x && q.y >= min.y && q.y <= max.y {
                let i = ((q.y - min.y) * width + q.x - min.x) as usize;
                if map[i] == Feature::Door {
                    map[i] = Feature::Floor;
                } else {
                    map[i] = Feature::Wall(Material::Drywall);
                }
            }
        }
        for (dx, dy) in [n, (-n.0, -n.1)] {
            let q = Position::new(x + dx, y + dy);
            if q.x > min.x && q.x < max.x && q.y > min.y && q.y < max.y {
                let i = ((q.y - min.y) * width + q.x - min.x) as usize;
                if matches!(
                    map[i],
                    Feature::Wall(_) | Feature::Window | Feature::Furniture
                ) {
                    map[i] = Feature::Floor;
                }
            }
        }
        axes.push((Position::new(x, y), floor, axis));
    }
    // Finalize anchors after all adjacent-door removals; this second pass
    // prevents a later doorway from leaving a surviving jamb as floor.
    for &(p, _, axis) in &axes {
        let tangent = axis.tangent();
        for (dx, dy) in [tangent, (-tangent.0, -tangent.1)] {
            let q = Position::new(p.x + dx, p.y + dy);
            if q.x >= min.x && q.x <= max.x && q.y >= min.y && q.y <= max.y {
                let i = ((q.y - min.y) * width + q.x - min.x) as usize;
                if map[i] != Feature::Door {
                    map[i] = Feature::Wall(Material::Drywall);
                }
            }
        }
    }
    axes
}

fn entry_for_footprint(lot: &Lot, min: Position, max: Position) -> Position {
    if lot.front.x <= lot.min.x {
        Position::new(min.x, lot.front.y.clamp(min.y + 1, max.y - 1))
    } else if lot.front.x >= lot.max.x {
        Position::new(max.x, lot.front.y.clamp(min.y + 1, max.y - 1))
    } else if lot.front.y <= lot.min.y {
        Position::new(lot.front.x.clamp(min.x + 1, max.x - 1), min.y)
    } else {
        Position::new(lot.front.x.clamp(min.x + 1, max.x - 1), max.y)
    }
}

pub(super) fn planned_pad(
    seed: u64,
    lot: &Lot,
    ordinal: u64,
    terrain: &[super::Tile],
) -> Option<PlannedPad> {
    let n = noise(seed, 0x4255494c44, lot.min.x, lot.min.y);
    let choice = ordinal + seed % 3;
    let k = kind(lot.district, choice);
    let (min, max) = footprint(lot, k, n);
    let w = max.x - min.x + 1;
    let h = max.y - min.y + 1;
    if w < 5 || h < 5 || min.x < 0 || min.y < 0 || max.x >= WORLD_SIZE || max.y >= WORLD_SIZE {
        return None;
    }
    if (min.y..=max.y).any(|y| {
        (min.x..=max.x).any(|x| {
            let tile = terrain[(y * WORLD_SIZE + x) as usize];
            road(tile.ground) || tile.feature != Feature::Empty
        })
    }) {
        return None;
    }
    Some(PlannedPad {
        min,
        max,
        entry: entry_for_footprint(lot, min, max),
        front: lot.front,
    })
}

fn floorplan(
    lot: &Lot,
    k: BuildingKind,
    n: u64,
    min: Position,
    max: Position,
    foundation_z: f32,
) -> Building {
    let w = max.x - min.x + 1;
    let h = max.y - min.y + 1;
    let floors = match k {
        BuildingKind::Bungalow
        | BuildingKind::Cabin
        | BuildingKind::Barn
        | BuildingKind::ArmyBase => 1,
        BuildingKind::Apartments => 3,
        _ => 2,
    };
    let entry = entry_for_footprint(lot, min, max);
    let mat = if matches!(
        k,
        BuildingKind::Bungalow
            | BuildingKind::Duplex
            | BuildingKind::SplitLevel
            | BuildingKind::Cabin
            | BuildingKind::Barn
    ) {
        Material::WoodSiding
    } else {
        Material::BrickMasonry
    };
    let mut features = Vec::new();
    let mut layout = Vec::new();
    let (split_x, split_y) = match k {
        BuildingKind::Bungalow | BuildingKind::Cabin => (w * 2 / 5, h * 3 / 5),
        BuildingKind::Duplex => (w / 2, h * 2 / 5),
        BuildingKind::SplitLevel => (w * 3 / 5, h * 2 / 5),
        _ => (w / 2, h / 2),
    };
    let midx = min.x + split_x;
    let midy = min.y + split_y;
    let stair = Position::new(min.x + 1, min.y + 1);
    for floor in 0..floors {
        // Upper stories keep the stair shaft and move partitions around it.
        let midx = (midx + i32::from(floor)).min(max.x - 3);
        let midy = (midy - i32::from(floor)).max(min.y + 3);
        let mut map = vec![Feature::Floor; (w * h) as usize];
        for y in min.y..=max.y {
            for x in min.x..=max.x {
                let p = Position::new(x, y);
                if x == min.x || x == max.x || y == min.y || y == max.y {
                    let window = (x + y + floor as i32 + (n % 3) as i32) % 4 == 0;
                    set(
                        &mut map,
                        min,
                        w,
                        p,
                        if window {
                            Feature::Window
                        } else {
                            Feature::Wall(mat)
                        },
                    );
                }
            }
        }
        // Crossing hallways connect four rooms; doors are openings in the partitions.
        if w >= 7 && h >= 7 {
            for y in min.y + 1..max.y {
                if y != midy && y != midy - 1 && y != midy + 1 {
                    set(
                        &mut map,
                        min,
                        w,
                        Position::new(midx, y),
                        Feature::Wall(Material::Drywall),
                    );
                }
            }
            for x in min.x + 1..max.x {
                if x != midx && x != midx - 1 && x != midx + 1 {
                    set(
                        &mut map,
                        min,
                        w,
                        Position::new(x, midy),
                        Feature::Wall(Material::Drywall),
                    );
                }
            }
        }
        // Cells and wards have separate bays opening onto the inner corridor.
        if matches!(k, BuildingKind::Precinct | BuildingKind::Hospital) && w >= 13 && h >= 13 {
            for y in ((min.y + 3)..(midy - 1)).step_by(3) {
                for x in midx + 2..max.x {
                    set(
                        &mut map,
                        min,
                        w,
                        Position::new(x, y),
                        Feature::Wall(if k == BuildingKind::Precinct {
                            Material::MetalPole
                        } else {
                            Material::Drywall
                        }),
                    );
                }
                set(&mut map, min, w, Position::new(midx + 2, y), Feature::Door);
            }
        }
        if floor == 0 {
            let mut p = entry;
            while p.x != midx || p.y != midy {
                if p.x == min.x || p.x == max.x {
                    p.x += (midx - p.x).signum();
                } else if p.y == min.y || p.y == max.y {
                    p.y += (midy - p.y).signum();
                } else if p.x != midx {
                    p.x += (midx - p.x).signum();
                } else {
                    p.y += (midy - p.y).signum();
                }
                set(&mut map, min, w, p, Feature::Floor);
            }
            set(&mut map, min, w, entry, Feature::Door);
            if k == BuildingKind::Duplex {
                // The two flats have separate entries facing the same street.
                let other = if entry.y == min.y || entry.y == max.y {
                    Position::new(
                        if entry.x <= midx {
                            max.x - 2
                        } else {
                            min.x + 2
                        },
                        entry.y,
                    )
                } else {
                    Position::new(
                        entry.x,
                        if entry.y <= midy {
                            max.y - 2
                        } else {
                            min.y + 2
                        },
                    )
                };
                set(&mut map, min, w, other, Feature::Door);
            }
        }
        if floors > 1 {
            set(&mut map, min, w, stair, Feature::Stairs);
        }
        let rooms = room_kinds(k, floor);
        let boxes = [
            (
                Position::new(min.x + 1, min.y + 1),
                Position::new(midx - 1, midy - 1),
            ),
            (
                Position::new(midx + 1, min.y + 1),
                Position::new(max.x - 1, midy - 1),
            ),
            (
                Position::new(min.x + 1, midy + 1),
                Position::new(midx - 1, max.y - 1),
            ),
            (
                Position::new(midx + 1, midy + 1),
                Position::new(max.x - 1, max.y - 1),
            ),
        ];
        for (i, (a, b)) in boxes.into_iter().enumerate() {
            if a.x <= b.x && a.y <= b.y {
                layout.push(Room {
                    min: a,
                    max: b,
                    floor,
                    kind: rooms[i],
                });
                // Furnish against an outside corner, leaving the room's doorway clear.
                let p = Position::new(
                    if i % 2 == 0 { a.x } else { b.x },
                    if i < 2 { a.y } else { b.y },
                );
                if p != stair && p.distance(entry) > 3 && a != b {
                    set(&mut map, min, w, p, Feature::Furniture);
                }
            }
        }
        // Remove a seeded recess from the facade opposite the entrance. The
        // hospital's central recess leaves two wings joined behind the lobby;
        // city buildings alternate between recessed corners and deeper courts.
        if matches!(
            k,
            BuildingKind::Hospital
                | BuildingKind::Storefront
                | BuildingKind::Office
                | BuildingKind::Apartments
        ) && w >= 13
            && h >= 13
        {
            let vertical = entry.y == min.y || entry.y == max.y;
            let depth = if vertical {
                (h / 5 + (n % 3) as i32).max(2)
            } else {
                (w / 5 + (n % 3) as i32).max(2)
            };
            let span = if vertical { w } else { h };
            let recess_width = if k == BuildingKind::Hospital {
                (span / 3).max(3)
            } else {
                (span / 4).max(2)
            };
            let offset = (span - recess_width) / 2;
            let recess = |p: Position| {
                if vertical {
                    let across = p.x - min.x;
                    let from_back = if entry.y == min.y {
                        max.y - p.y
                    } else {
                        p.y - min.y
                    };
                    across >= offset && across < offset + recess_width && from_back < depth
                } else {
                    let across = p.y - min.y;
                    let from_back = if entry.x == min.x {
                        max.x - p.x
                    } else {
                        p.x - min.x
                    };
                    across >= offset && across < offset + recess_width && from_back < depth
                }
            };
            for y in min.y..=max.y {
                for x in min.x..=max.x {
                    let p = Position::new(x, y);
                    if recess(p) {
                        set(&mut map, min, w, p, Feature::Empty);
                    } else if [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .into_iter()
                        .any(|(dx, dy)| {
                            let q = Position::new(x + dx, y + dy);
                            q.x >= min.x
                                && q.x <= max.x
                                && q.y >= min.y
                                && q.y <= max.y
                                && recess(q)
                        })
                    {
                        set(&mut map, min, w, p, Feature::Wall(mat));
                    }
                }
            }
        }
        features.push(map);
    }
    if k == BuildingKind::ArmyBase && w >= 15 && h >= 15 {
        // Three separate buildings face a shared open compound and gate.
        let map = &mut features[0];
        map.fill(Feature::Floor);
        layout.clear();
        let blocks = [
            (
                Position::new(min.x + 1, min.y + 1),
                Position::new(midx - 2, midy - 2),
                RoomKind::Barracks,
            ),
            (
                Position::new(midx + 2, min.y + 1),
                Position::new(max.x - 1, midy - 2),
                RoomKind::Armory,
            ),
            (
                Position::new(min.x + 1, midy + 2),
                Position::new(max.x - 1, max.y - 1),
                RoomKind::MotorPool,
            ),
        ];
        for (a, b, room) in blocks {
            for y in a.y..=b.y {
                for x in a.x..=b.x {
                    if x == a.x || x == b.x || y == a.y || y == b.y {
                        set(map, min, w, Position::new(x, y), Feature::Wall(mat));
                    }
                }
            }
            let door = if room == RoomKind::MotorPool {
                Position::new((a.x + b.x) / 2, a.y)
            } else {
                Position::new((a.x + b.x) / 2, b.y)
            };
            set(map, min, w, door, Feature::Door);
            set(
                map,
                min,
                w,
                Position::new(a.x + 1, a.y + 1),
                Feature::Furniture,
            );
            layout.push(Room {
                min: Position::new(a.x + 1, a.y + 1),
                max: Position::new(b.x - 1, b.y - 1),
                floor: 0,
                kind: room,
            });
        }
        set(map, min, w, entry, Feature::Gate);
    }
    let wings = vec![crate::action::WingRect::new(
        min.x, min.y, max.x, max.y, floors,
    )];
    let mut door_axes = Vec::new();
    for (floor, map) in features.iter_mut().enumerate() {
        // ArmyBase's three sub-buildings already stamp their own entrances;
        // running the generic connector through the shared compound yard can
        // add a second door directly behind a stamped door.
        if !(k == BuildingKind::ArmyBase && floor == 0) {
            connect_floor(map, min, max, if floor == 0 { entry } else { stair });
        }
        if k == BuildingKind::ArmyBase && floor == 0 {
            // The stamped sub-building fronts can meet on consecutive tiles;
            // retain the first opening and turn the duplicate into apron floor
            // so a closed neighboring leaf can never seal its passage.
            let width = max.x - min.x + 1;
            for y in min.y..=max.y {
                for x in min.x..=max.x {
                    let i = ((y - min.y) * width + x - min.x) as usize;
                    if map[i] != Feature::Door {
                        continue;
                    }
                    for (dx, dy) in [(0, 1), (1, 0)] {
                        let q = Position::new(x + dx, y + dy);
                        if q.x <= max.x && q.y <= max.y {
                            let qi = ((q.y - min.y) * width + q.x - min.x) as usize;
                            if map[qi] == Feature::Door {
                                map[qi] = Feature::Floor;
                            }
                        }
                    }
                }
            }
        }
        door_axes.extend(normalize_doors(map, min, max, floor as u8));
    }
    Building {
        min,
        max,
        floors,
        district: lot.district,
        entry,
        kind: k,
        material: mat,
        foundation_z,
        layout,
        wings,
        door_axes,
        features,
    }
}

fn place_building(lot: &Lot, building: &Building, tiles: &mut ParcelTiles) {
    let (min, max, entry, k) = (building.min, building.max, building.entry, building.kind);
    for y in min.y..=max.y {
        for x in min.x..=max.x {
            let p = Position::new(x, y);
            tiles[p].feature = building.feature_at(p, 0).unwrap();
        }
    }
    if k == BuildingKind::ArmyBase {
        for y in lot.min.y..=lot.max.y {
            for x in lot.min.x..=lot.max.x {
                if x == lot.min.x || x == lot.max.x || y == lot.min.y || y == lot.max.y {
                    let p = Position::new(x, y);
                    if !road(tiles[p].ground) {
                        // The compound's explicit entry gate is stamped on the
                        // building apron. Do not add a second lot-front gate one
                        // tile ahead of it; that external leaf can seal the
                        // entry's 3x3 passage and has no building metadata for
                        // axis lookup. Leave the frontage open for the approach.
                        if p != lot.front {
                            tiles[p].feature = Feature::Fence;
                        }
                    }
                }
            }
        }
    }
    // The short walk from lot frontage to door remains unobstructed.
    let mut p = lot.front;
    while p != entry {
        if p.x != entry.x {
            p.x += (entry.x - p.x).signum();
        } else {
            p.y += (entry.y - p.y).signum();
        }
        if p != entry
            && (0..WORLD_SIZE).contains(&p.x)
            && (0..WORLD_SIZE).contains(&p.y)
            && !road(tiles[p].ground)
        {
            tiles[p].feature = Feature::Path;
        }
    }
}

fn room_at(building: &Building, p: Position, floor: u8) -> Option<RoomKind> {
    building
        .layout
        .iter()
        .find(|r| {
            r.floor == floor
                && (r.min.x..=r.max.x).contains(&p.x)
                && (r.min.y..=r.max.y).contains(&p.y)
        })
        .map(|r| r.kind)
}

fn take_site(
    candidates: &mut Vec<(Position, u8)>,
    building: &Building,
    seed: u64,
    salt: u64,
    floor: u8,
    preferred: &[RoomKind],
) -> Option<(Position, u8)> {
    let (index, _) = candidates.iter().enumerate().min_by_key(|(_, (p, f))| {
        let role = room_at(building, *p, *f);
        (
            u8::from(*f != floor),
            u8::from(!role.is_some_and(|r| preferred.contains(&r))),
            noise(seed, salt ^ u64::from(*f), p.x, p.y),
            (*f, p.y, p.x),
        )
    })?;
    Some(candidates.swap_remove(index))
}

pub(super) fn loot_variant(seed: u64, lot: &Lot) -> u64 {
    noise(seed, 0x4c4f4f5454595045, lot.min.x, lot.min.y)
}

fn populate(
    seed: u64,
    lot: &Lot,
    guarantee_ammo: bool,
    building: &Building,
    tiles: &mut ParcelTiles,
    sites: &mut Sites,
) {
    let mut accessible = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::from([(building.entry, 0)]);
    while let Some((p, floor)) = queue.pop_front() {
        if !accessible.insert((p, floor)) {
            continue;
        }
        if building.feature_at(p, floor) == Some(Feature::Stairs) {
            for next_floor in 0..building.floors {
                if building.feature_at(p, next_floor) == Some(Feature::Stairs) {
                    queue.push_back((p, next_floor));
                }
            }
        }
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let q = Position::new(p.x + dx, p.y + dy);
            if building
                .feature_at(q, floor)
                .is_some_and(|f| f != Feature::Empty && f.walkable())
            {
                queue.push_back((q, floor));
            }
        }
    }
    let mut candidates: Vec<_> = accessible
        .into_iter()
        .filter(|&(p, floor)| {
            matches!(
                building.feature_at(p, floor),
                Some(Feature::Floor | Feature::Door)
            ) && (floor != 0 || p.distance(building.entry) > 3)
        })
        .collect();
    let mut loot_candidates = candidates.clone();
    let loot_count = match lot.district {
        District::Hospital | District::Police | District::MilitaryBase => 24,
        District::City => 10,
        _ => 2,
    };
    let variant = loot_variant(seed, lot);
    for i in 0..loot_count {
        let kind = match lot.district {
            District::Hospital => {
                if i < 20 {
                    LootKind::Medkit
                } else {
                    LootKind::Tools
                }
            }
            District::Police | District::MilitaryBase => {
                if i % 2 == 0 {
                    LootKind::Firearm
                } else {
                    LootKind::Ammo
                }
            }
            District::City => [
                LootKind::Household,
                LootKind::Tools,
                LootKind::Ammo,
                LootKind::Firearm,
                LootKind::Medkit,
            ][(i + (variant % 5) as usize) % 5],
            District::Farm | District::Woods => {
                if i == 0 {
                    LootKind::Tools
                } else if variant.is_multiple_of(4) {
                    LootKind::Firearm
                } else {
                    LootKind::Household
                }
            }
            District::Residential => {
                if i == 0 {
                    LootKind::Household
                } else if guarantee_ammo || variant.is_multiple_of(3) {
                    LootKind::Ammo
                } else {
                    LootKind::Tools
                }
            }
        };
        let preferred: &[RoomKind] = match lot.district {
            District::Hospital => &[RoomKind::Pharmacy, RoomKind::Ward],
            District::Police | District::MilitaryBase => &[RoomKind::Armory],
            District::City => &[RoomKind::Shop, RoomKind::Storage, RoomKind::Office],
            _ => &[RoomKind::Kitchen, RoomKind::Storage, RoomKind::Living],
        };
        let Some((p, floor)) = take_site(
            &mut loot_candidates,
            building,
            seed,
            0x4c4f4f54 + i as u64,
            (i % building.floors as usize) as u8,
            preferred,
        ) else {
            break;
        };
        sites.loot.push(LootSite {
            position: p,
            floor,
            kind,
        });
        if floor == 0 {
            tiles[p].cache = true;
        }
    }
    candidates.retain(|&(p, f)| !sites.loot.iter().any(|s| s.position == p && s.floor == f));
    let count = match lot.district {
        District::Hospital | District::Police | District::MilitaryBase => 24,
        District::City => 16,
        District::Farm | District::Woods => 1,
        _ => 2,
    };
    for i in 0..count {
        let kind = match lot.district {
            District::Hospital => {
                if (i / usize::from(building.floors)).is_multiple_of(2) {
                    MonsterKind::Bloater
                } else {
                    MonsterKind::Shambler
                }
            }
            District::Police | District::MilitaryBase => {
                if i * 4 < count * 3 {
                    MonsterKind::Shooter
                } else {
                    MonsterKind::Shambler
                }
            }
            District::City => [
                MonsterKind::Shambler,
                MonsterKind::Brisk,
                MonsterKind::Bloater,
                MonsterKind::Shooter,
            ][(i + (variant % 4) as usize) % 4],
            _ => {
                if (variant + i as u64).is_multiple_of(2) {
                    MonsterKind::Shambler
                } else {
                    MonsterKind::Brisk
                }
            }
        };
        let preferred: &[RoomKind] = match lot.district {
            District::Hospital => &[RoomKind::Ward, RoomKind::Pharmacy],
            District::Police | District::MilitaryBase => {
                &[RoomKind::Armory, RoomKind::Cells, RoomKind::Lobby]
            }
            District::City => &[RoomKind::Shop, RoomKind::Office, RoomKind::Apartment],
            _ => &[RoomKind::Living, RoomKind::Bedroom],
        };
        let Some((p, floor)) = take_site(
            &mut candidates,
            building,
            seed,
            0x494e46454354 + i as u64,
            (i % building.floors as usize) as u8,
            preferred,
        ) else {
            break;
        };
        sites.infected.push(InfectedSite {
            position: p,
            floor,
            kind,
        });
    }
}

pub(super) fn generate(
    seed: u64,
    lot: &Lot,
    ordinal: u64,
    guarantee_ammo: bool,
    foundation_z: f32,
    tiles: &mut ParcelTiles,
) -> Sites {
    let mut sites = Sites::default();
    let n = noise(seed, 0x4255494c44, lot.min.x, lot.min.y);
    // Cycle by the parcel's stable district ordinal, independent of visit order.
    let choice = ordinal + seed % 3;
    let k = kind(lot.district, choice);
    let (min, max) = footprint(lot, k, n);
    let w = max.x - min.x + 1;
    let h = max.y - min.y + 1;
    if w < 5 || h < 5 || min.x < 0 || min.y < 0 || max.x >= WORLD_SIZE || max.y >= WORLD_SIZE {
        return sites;
    }
    // Never place a structure over an existing road or another building.
    if (min.y..=max.y).any(|y| {
        (min.x..=max.x).any(|x| {
            road(tiles[Position::new(x, y)].ground)
                || tiles[Position::new(x, y)].feature != Feature::Empty
        })
    }) {
        return sites;
    }
    let building = floorplan(lot, k, n, min, max, foundation_z);
    place_building(lot, &building, tiles);
    populate(seed, lot, guarantee_ammo, &building, tiles, &mut sites);
    sites.buildings.push(building);
    sites
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashSet, VecDeque};

    #[test]
    fn interiors_stairs_and_sites_are_reachable() {
        for seed in 0..64 {
            let world = super::super::World::new(seed);
            let sites = world.sites().unwrap();
            for b in &sites.buildings {
                if b.floors > 1 {
                    assert!(
                        sites
                            .loot
                            .iter()
                            .any(|s| s.floor > 0 && b.feature_at(s.position, s.floor).is_some()),
                        "seed {seed} {:?} missing upper loot",
                        b.kind
                    );
                    assert!(
                        sites
                            .infected
                            .iter()
                            .any(|s| s.floor > 0 && b.feature_at(s.position, s.floor).is_some()),
                        "seed {seed} {:?} missing upper infected",
                        b.kind
                    );
                }
                let mut seen = HashSet::new();
                let mut queue = VecDeque::from([(b.entry, 0)]);
                while let Some((p, floor)) = queue.pop_front() {
                    if !seen.insert((p, floor)) {
                        continue;
                    }
                    if b.feature_at(p, floor) == Some(Feature::Stairs) {
                        for f in 0..b.floors {
                            if b.feature_at(p, f) == Some(Feature::Stairs) {
                                queue.push_back((p, f));
                            }
                        }
                    }
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let q = Position::new(p.x + dx, p.y + dy);
                        if b.feature_at(q, floor)
                            .is_some_and(|f| f != Feature::Empty && f.walkable())
                        {
                            queue.push_back((q, floor));
                        }
                    }
                }
                for floor in 0..b.floors {
                    for y in b.min.y..=b.max.y {
                        for x in b.min.x..=b.max.x {
                            let p = Position::new(x, y);
                            if matches!(
                                b.feature_at(p, floor),
                                Some(
                                    Feature::Floor
                                        | Feature::Door
                                        | Feature::Stairs
                                        | Feature::Gate
                                )
                            ) {
                                assert!(
                                    seen.contains(&(p, floor)),
                                    "seed {seed} {:?} inaccessible floor at {p:?} floor {floor}",
                                    b.kind
                                );
                            }
                        }
                    }
                }
                for room in &b.layout {
                    assert!(
                        (room.min.y..=room.max.y).any(|y| (room.min.x..=room.max.x)
                            .any(|x| seen.contains(&(Position::new(x, y), room.floor)))),
                        "seed {seed} {:?} inaccessible room {:?}",
                        b.kind,
                        room.kind
                    );
                }
                for site in sites
                    .loot
                    .iter()
                    .filter(|s| b.feature_at(s.position, s.floor).is_some())
                {
                    assert!(
                        seen.contains(&(site.position, site.floor)),
                        "seed {seed} {:?} inaccessible loot",
                        b.kind
                    );
                }
                for site in sites
                    .infected
                    .iter()
                    .filter(|s| b.feature_at(s.position, s.floor).is_some())
                {
                    assert!(
                        seen.contains(&(site.position, site.floor)),
                        "seed {seed} {:?} inaccessible infected",
                        b.kind
                    );
                }
            }
        }
    }
    #[test]
    fn generated_doors_have_explicit_axis_and_clear_three_by_three_stencil() {
        for seed in 0..64 {
            let world = super::super::World::new(seed);
            let sites = world.sites().unwrap();
            for b in &sites.buildings {
                for floor in 0..b.floors {
                    for y in b.min.y..=b.max.y {
                        for x in b.min.x..=b.max.x {
                            let p = Position::new(x, y);
                            if b.feature_at(p, floor) != Some(Feature::Door) {
                                continue;
                            }
                            let axis = b.door_axis_at(p, floor).unwrap_or_else(|| {
                                panic!("seed {seed} {:?} door {p:?} F{floor} has no axis", b.kind)
                            });
                            let (t, n) = match axis {
                                crate::action::DoorAxis::AlongX => ((1, 0), (0, 1)),
                                crate::action::DoorAxis::AlongY => ((0, 1), (1, 0)),
                            };
                            let tangent = [
                                Position::new(p.x - t.0, p.y - t.1),
                                Position::new(p.x + t.0, p.y + t.1),
                            ];
                            // Exterior doors have one anchor outside the AABB;
                            // every in-footprint anchor must remain a wall.
                            for q in tangent {
                                if q.x >= b.min.x
                                    && q.x <= b.max.x
                                    && q.y >= b.min.y
                                    && q.y <= b.max.y
                                {
                                    assert!(
                                        matches!(
                                            b.feature_at(q, floor),
                                            Some(
                                                Feature::Wall(_) | Feature::Window | Feature::Door
                                            )
                                        ),
                                        "seed {seed} {:?} door {p:?} F{floor} missing tangent anchor {q:?}",
                                        b.kind
                                    );
                                }
                            }
                            for q in [
                                Position::new(p.x - n.0, p.y - n.1),
                                Position::new(p.x + n.0, p.y + n.1),
                            ] {
                                if q.x > b.min.x && q.x < b.max.x && q.y > b.min.y && q.y < b.max.y
                                {
                                    assert!(
                                        matches!(
                                            b.feature_at(q, floor),
                                            Some(
                                                Feature::Floor
                                                    | Feature::Door
                                                    | Feature::Stairs
                                                    | Feature::Gate
                                                    | Feature::Path
                                            )
                                        ),
                                        "seed {seed} {:?} door {p:?} F{floor} blocked passage {q:?}",
                                        b.kind
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn district_ecology_is_dense_and_sites_do_not_overlap() {
        for seed in 0..64 {
            let world = super::super::World::new(seed);
            let sites = world.sites().unwrap();
            let mut occupied = HashSet::new();
            for p in sites
                .loot
                .iter()
                .map(|s| (s.position, s.floor))
                .chain(sites.infected.iter().map(|s| (s.position, s.floor)))
            {
                assert!(occupied.insert(p), "seed {seed}: overlapping entities");
                let f = sites.feature_at(p.0, p.1).unwrap();
                assert!(
                    matches!(f, Feature::Floor | Feature::Door),
                    "seed {seed}: entity on {f:?}"
                );
            }
            let residential_area: usize = sites
                .buildings
                .iter()
                .filter(|b| b.district == District::Residential)
                .map(|b| ((b.max.x - b.min.x + 1) * (b.max.y - b.min.y + 1)) as usize)
                .sum();
            let residential_infected = sites
                .infected
                .iter()
                .filter(|s| world.district_at(s.position) == Some(District::Residential))
                .count();
            let residential_loot = sites
                .loot
                .iter()
                .filter(|s| world.district_at(s.position) == Some(District::Residential))
                .count();
            for district in [District::Hospital, District::Police, District::MilitaryBase] {
                let b = sites
                    .buildings
                    .iter()
                    .find(|b| b.district == district)
                    .unwrap();
                let area = ((b.max.x - b.min.x + 1) * (b.max.y - b.min.y + 1)) as usize;
                let loot = sites
                    .loot
                    .iter()
                    .filter(|s| world.district_at(s.position) == Some(district))
                    .count();
                let infected = sites
                    .infected
                    .iter()
                    .filter(|s| world.district_at(s.position) == Some(district))
                    .count();
                assert!(
                    loot * residential_area > residential_loot * area,
                    "{district:?} supply density"
                );
                assert!(
                    infected * residential_area > residential_infected * area,
                    "{district:?} infected density"
                );
                // At least one supply and one infected per hundred footprint tiles.
                assert!(
                    loot * 100 >= area && infected * 100 >= area,
                    "{district:?}: {area} tiles, {loot} loot, {infected} infected"
                );
                if district == District::Hospital {
                    assert!(
                        sites
                            .loot
                            .iter()
                            .filter(|s| world.district_at(s.position) == Some(district)
                                && s.kind == LootKind::Medkit)
                            .count()
                            >= 8
                    );
                    assert!(
                        sites
                            .infected
                            .iter()
                            .filter(|s| world.district_at(s.position) == Some(district)
                                && s.kind == MonsterKind::Bloater)
                            .count()
                            >= 6
                    );
                } else {
                    assert!(
                        sites
                            .infected
                            .iter()
                            .filter(|s| world.district_at(s.position) == Some(district)
                                && s.kind == MonsterKind::Shooter)
                            .count()
                            >= 9
                    );
                }
            }
        }
    }
    #[test]
    fn ecology_matches_district_and_room_roles() {
        let mut rural_guns_total = 0;
        for seed in 0..16 {
            let world = super::super::World::new(seed);
            let sites = world.sites().unwrap();
            let mut civil_ammo = 0;
            let mut rural_guns = 0;
            for b in &sites.buildings {
                let loot: Vec<_> = sites
                    .loot
                    .iter()
                    .filter(|s| b.feature_at(s.position, s.floor).is_some())
                    .collect();
                let infected: Vec<_> = sites
                    .infected
                    .iter()
                    .filter(|s| b.feature_at(s.position, s.floor).is_some())
                    .collect();
                let in_roles =
                    |p, f, roles: &[RoomKind]| room_at(b, p, f).is_some_and(|r| roles.contains(&r));
                match b.district {
                    District::Hospital => {
                        assert!(loot.iter().filter(|s| s.kind == LootKind::Medkit).count() >= 20);
                        assert!(
                            infected
                                .iter()
                                .filter(|s| s.kind == MonsterKind::Bloater)
                                .count()
                                >= 12
                        );
                        assert!(
                            infected
                                .iter()
                                .filter(|s| s.kind == MonsterKind::Shambler)
                                .count()
                                >= 12
                        );
                        let medical = [RoomKind::Ward, RoomKind::Pharmacy];
                        assert!(
                            loot.iter()
                                .filter(|s| in_roles(s.position, s.floor, &medical))
                                .count()
                                * 2
                                >= loot.len()
                        );
                        assert!(
                            infected
                                .iter()
                                .filter(|s| in_roles(s.position, s.floor, &medical))
                                .count()
                                * 2
                                >= infected.len()
                        );
                    }
                    District::Police | District::MilitaryBase => {
                        assert!(loot.iter().filter(|s| s.kind == LootKind::Firearm).count() >= 12);
                        assert!(loot.iter().filter(|s| s.kind == LootKind::Ammo).count() >= 12);
                        assert!(
                            infected
                                .iter()
                                .filter(|s| s.kind == MonsterKind::Shooter)
                                .count()
                                >= 18
                        );
                        assert!(
                            loot.iter()
                                .filter(|s| in_roles(s.position, s.floor, &[RoomKind::Armory]))
                                .count()
                                * 2
                                >= loot.len()
                        );
                    }
                    District::City => {
                        assert!(loot.iter().any(|s| s.kind == LootKind::Ammo));
                        assert!(infected.iter().any(|s| s.kind == MonsterKind::Brisk));
                        assert!(infected.iter().any(|s| s.kind == MonsterKind::Shooter));
                    }
                    District::Residential => {
                        civil_ammo += loot.iter().filter(|s| s.kind == LootKind::Ammo).count()
                    }
                    District::Woods | District::Farm => {
                        rural_guns += loot.iter().filter(|s| s.kind == LootKind::Firearm).count()
                    }
                }
            }
            assert!(civil_ammo > 0, "seed {seed}: no residential ammo");
            rural_guns_total += rural_guns;
        }
        assert!(
            rural_guns_total > 0,
            "rural cabins should occasionally contain firearms"
        );
    }
}
