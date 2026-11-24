//! Baseline geography can be rebuilt from coordinates in any order.

pub const CHUNK_SIZE: i32 = 16;
pub const CHUNKS_PER_SIDE: i32 = 32;
pub const WORLD_SIZE: i32 = CHUNK_SIZE * CHUNKS_PER_SIDE;
pub const GENERATOR_VERSION: u32 = 1;

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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum District {
    Residential,
    City,
    Hospital,
    Farm,
    MilitaryBase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ground {
    Road,
    Grass,
    Pavement,
    Field,
    Gravel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tile {
    pub ground: Ground,
    /// Initial cache site. Query the game for whether the contents remain.
    pub cache: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct World {
    seed: u64,
}

impl World {
    pub const fn new(seed: u64) -> Self {
        Self { seed }
    }

    pub fn wake_position(&self) -> Position {
        self.cache_position(8, 16)
    }

    pub fn district_at(&self, p: Position) -> Option<District> {
        if !(0..WORLD_SIZE).contains(&p.x) || !(0..WORLD_SIZE).contains(&p.y) {
            return None;
        }
        let district = match (p.x / CHUNK_SIZE, p.y / CHUNK_SIZE) {
            (16..=18, 12..=13) => District::Hospital,
            (25..=28, 3..=6) => District::MilitaryBase,
            (12..=21, 10..=21) => District::City,
            (22.., _) | (_, 23..) => District::Farm,
            _ => District::Residential,
        };
        Some(district)
    }

    pub fn tile_at(&self, p: Position) -> Option<Tile> {
        let district = self.district_at(p)?;
        let variant = self.hash(p.x, p.y) % 8;
        let ground = if p.x % CHUNK_SIZE == 8 || p.y % CHUNK_SIZE == 8 {
            Ground::Road
        } else {
            match (district, variant) {
                (District::Farm, 0) | (District::Residential, 0) => Ground::Gravel,
                (District::Farm, _) => Ground::Field,
                (District::Residential, _) | (District::Hospital, 0..=2) => Ground::Grass,
                (District::MilitaryBase, 0..=5) => Ground::Gravel,
                _ => Ground::Pavement,
            }
        };
        Some(Tile {
            ground,
            cache: p == self.cache_position(p.x / CHUNK_SIZE, p.y / CHUNK_SIZE),
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
                let p = Position::new(cx * CHUNK_SIZE + x as i32, cy * CHUNK_SIZE + y as i32);
                self.tile_at(p).expect("validated chunk coordinates")
            })
        }))
    }

    fn cache_position(&self, cx: i32, cy: i32) -> Position {
        let offset = 1 + (self.hash(cx, cy) % 14) as i32;
        Position::new(cx * CHUNK_SIZE + offset, cy * CHUNK_SIZE + 8)
    }

    fn hash(&self, x: i32, y: i32) -> u64 {
        // SplitMix64 finalizer over the seed and packed coordinates.
        let coordinates = (u64::from(x as u32) << 32) | u64::from(y as u32);
        let mut n = (self.seed ^ coordinates).wrapping_add(0x9e37_79b9_7f4a_7c15);
        n = (n ^ (n >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        n = (n ^ (n >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        n ^ (n >> 31)
    }
}
