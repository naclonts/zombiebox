//! Deterministic single-player rules. One call to `step` consumes one tick.

mod world;

use std::collections::HashSet;
pub use world::{
    CHUNK_SIZE, CHUNKS_PER_SIDE, District, GENERATOR_VERSION, Ground, Position, Tile, WORLD_SIZE,
    World,
};

pub const TICKS_PER_SECOND: u32 = 20;
const ZOMBIE_MOVE_INTERVAL: u64 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    North,
    East,
    South,
    West,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Wait,
    Move(Direction),
    Scavenge,
    Attack,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    Scavenged { at: Position },
    Explosion { at: Position },
    Hurt,
    Died,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Hud {
    pub tick: u64,
    pub player: Position,
    pub health: u8,
    pub armed: bool,
    pub supplies: u16,
    pub zombie_count: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Snapshot<'a> {
    pub hud: Hud,
    pub zombies: &'a [Position],
}

pub struct Game {
    world: World,
    tick: u64,
    player: Position,
    health: u8,
    armed: bool,
    supplies: u16,
    zombies: Vec<Position>,
    searched: HashSet<Position>,
    events: Vec<Event>,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let world = World::new(seed);
        let player = world.wake_position();
        let zombies = [(4, 0), (5, 1), (-5, 1)]
            .map(|(dx, dy)| Position::new(player.x + dx, player.y + dy))
            .to_vec();
        Self {
            world,
            tick: 0,
            player,
            health: 5,
            armed: false,
            supplies: 0,
            zombies,
            searched: HashSet::new(),
            events: Vec::new(),
        }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// Events from the most recent step, valid until the next step.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn hud(&self) -> Hud {
        Hud {
            tick: self.tick,
            player: self.player,
            health: self.health,
            armed: self.armed,
            supplies: self.supplies,
            zombie_count: self.zombies.len(),
        }
    }

    pub fn snapshot(&self) -> Snapshot<'_> {
        Snapshot {
            hud: self.hud(),
            zombies: &self.zombies,
        }
    }

    pub fn has_cache(&self, p: Position) -> bool {
        self.world.tile_at(p).is_some_and(|tile| tile.cache) && !self.searched.contains(&p)
    }

    pub fn step(&mut self, command: Command) {
        self.events.clear();
        if self.health == 0 {
            return;
        }
        self.tick += 1;
        match command {
            Command::Wait => {}
            Command::Move(direction) => {
                let (dx, dy) = match direction {
                    Direction::North => (0, -1),
                    Direction::East => (1, 0),
                    Direction::South => (0, 1),
                    Direction::West => (-1, 0),
                };
                let dest = Position::new(self.player.x + dx, self.player.y + dy);
                if self.world.district_at(dest).is_some() {
                    self.player = dest;
                }
            }
            Command::Scavenge if self.has_cache(self.player) => {
                self.searched.insert(self.player);
                self.armed = true;
                self.supplies = self.supplies.saturating_add(3);
                self.events.push(Event::Scavenged { at: self.player });
            }
            Command::Scavenge => {}
            Command::Attack => self.attack(),
        }
        if self.tick.is_multiple_of(ZOMBIE_MOVE_INTERVAL) {
            self.advance_zombies();
        }
    }

    fn attack(&mut self) {
        if !self.armed || self.supplies == 0 {
            return;
        }
        let target = self
            .zombies
            .iter()
            .copied()
            .min_by_key(|z| z.distance(self.player));
        if let Some(at) = target.filter(|z| z.distance(self.player) <= 5) {
            self.supplies -= 1;
            self.zombies.retain(|z| z.distance(at) > 2);
            self.events.push(Event::Explosion { at });
        }
    }

    fn advance_zombies(&mut self) {
        for z in &mut self.zombies {
            let dx = self.player.x - z.x;
            let dy = self.player.y - z.y;
            if dx.abs() >= dy.abs() {
                z.x += dx.signum();
            } else {
                z.y += dy.signum();
            }
        }
        if self.zombies.contains(&self.player) {
            self.health -= 1;
            self.events.push(Event::Hurt);
            if self.health == 0 {
                self.events.push(Event::Died);
            }
        }
    }
}
