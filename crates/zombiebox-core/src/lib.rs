//! Deterministic single-player rules. One call to `step` consumes one tick.

mod action;
pub mod spatial;
mod survey;
pub use survey::county_survey_json;
mod world;

pub use action::{
    ACTION_TICKS_PER_SECOND, ActionEvent, ActionGame, ActionHud, ActionInput, ActionMode,
    ActionOutcome, AwarenessIcon, BLOCK_HEIGHT, BLOCK_WIDTH, BlastKind, BlastView, BuildingId,
    BuildingInfo, BuildingStyle, BuildingUid, COUNTY_FLOORS, CacheId, CacheKind, CacheLoot,
    CacheView, CombatWorld, ControlMode, CustomCellSpec, DT, DamageKind, DoorAxis,
    DynamicFlashView, Floor, FloorTransition, InteractPrompt, Inventory, LightKind,
    LightSourceView, MAX_BUILDING_WINGS, MAX_COUNTY_FLOORS, Material, MaterialCategory, MonsterId,
    MonsterKind, MonsterSpec, MonsterView, MotorcycleCrashKind, MotorcycleInput, MotorcyclePhase,
    MotorcycleView, MountedHud, NUM_FLOORS, NoiseEvent, NoiseKind, ObjectiveKind, PLAYER_HEIGHT,
    PLAYER_MAX_HEALTH, PLAYER_RADIUS, PLAYER_SHAPE, PlayerRenderState, Pose, ProjectileId,
    ProjectileKind, ProjectileView, PropKind, ReloadMode, RideInput, Surface, Tile3D,
    TileDamageResult, TileKind, TileObstruction, TileRenderInfo, TileVisibility, VehicleId,
    VehicleView, WEAPON_MUZZLE_HEIGHT, WallOrientation, Weapon, WeaponSpec, WingRect, mix64,
    monster_spec, pack_chunk_slot_id, weapon_spec,
};
pub use spatial::{
    Aabb3, ActionSpatialView, AirborneSupportHit, BRIDGE_PARAPET_HEIGHT, BRIDGE_SLAB_THICKNESS,
    BodyShape, BodyState, CHUNK_VERTICES, Collider, ColliderKind, DOOR_OPENING_HEIGHT, FLOOR_H,
    Hit3, HitSurface, MAX_ROAD_GRADE, MIN_BRIDGE_CLEARANCE, MotionState, MoveParams, MoveResult,
    QueryFilter, Ray3, SLAB_THICKNESS, SpatialWorld, StairGeometry, SupportId, SupportSurface,
    SurfaceQuad, TERRAIN_GRID_SIDE, TerrainGrid, TerrainSample, TerrainTriangle, Vec2, Vec3,
    WALL_HALF_THICKNESS, WALL_HEIGHT, WALL_THICKNESS, WATER_SURFACE_Z, WINDOW_LINTEL_Z,
    WINDOW_SILL_HEIGHT, WorldSpatialView, floor_elevation, move_kinematic_with_horizontal_vel,
    raycast_world, resolve_actor_support, resolve_tile_support, sample_quad_00_11,
    select_grounded_support, semantic_floor_for_body, sphere_cast_world, step_kinematic_body,
    support_normal_at, sweep_airborne_support, world_to_tile_coord,
};

use std::collections::{HashMap, HashSet, VecDeque};
pub use world::*;

pub const TICKS_PER_SECOND: u32 = 20;
const ZOMBIE_MOVE_INTERVAL: u64 = 8;
const RESIDENTIAL_ZOMBIE_MOVE_INTERVAL: u64 = 16;
const RESIDENTIAL_AMMO: u16 = 6;
const GUN_RANGE: u32 = 5;

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
    Gunshot { from: Position, at: Position },
    Hurt,
    Died,
}

/// A small rectangular house in a residential encounter. Width and height are tile counts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct House {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// The sole walkable tile along the perimeter.
    pub door: Position,
}

impl House {
    fn contains(self, p: Position) -> bool {
        p.x >= self.x && p.x < self.x + self.width && p.y >= self.y && p.y < self.y + self.height
    }

    fn is_wall(self, p: Position) -> bool {
        self.contains(p)
            && p != self.door
            && (p.x == self.x
                || p.x == self.x + self.width - 1
                || p.y == self.y
                || p.y == self.y + self.height - 1)
    }
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
    house: Option<House>,
    house_cache: Option<Position>,
    zombie_move_interval: u64,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let world = World::birch_street(seed);
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
            house: None,
            house_cache: None,
            zombie_move_interval: ZOMBIE_MOVE_INTERVAL,
        }
    }

    /// Build the fixed residential-block encounter without changing the legacy fixture in `new`.
    pub fn residential(seed: u64) -> Self {
        let mut game = Self::new(seed);
        let center = game.world.wake_position();
        let house = House {
            x: center.x - 3,
            y: center.y - 3,
            width: 7,
            height: 7,
            door: Position::new(center.x, center.y + 3),
        };
        game.player = Position::new(house.door.x, house.door.y + 2);
        game.house_cache = Some(Position::new(house.x + 2, house.y + 3));
        game.zombies = [
            Position::new(house.x - 3, house.y + 3),
            Position::new(house.x + house.width + 2, house.y + 3),
            Position::new(house.x + 3, house.y + house.height + 7),
        ]
        .to_vec();
        game.house = Some(house);
        game.zombie_move_interval = RESIDENTIAL_ZOMBIE_MOVE_INTERVAL;
        game
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn house(&self) -> Option<House> {
        self.house
    }

    /// Inclusive minimum and maximum coordinates for the bounded residential play area.
    pub fn block_bounds(&self) -> Option<(Position, Position)> {
        self.house.map(|h| {
            (
                Position::new(h.x - 7, h.y - 5),
                Position::new(h.x + h.width + 6, h.y + h.height + 8),
            )
        })
    }

    /// Nearest residential zombie with a clear shot; shares the rule used by `Attack`.
    pub fn shot_target(&self) -> Option<Position> {
        if self.house.is_none() || !self.armed || self.supplies == 0 {
            return None;
        }
        self.zombies
            .iter()
            .copied()
            .filter(|z| z.distance(self.player) <= GUN_RANGE && self.clear_shot(self.player, *z))
            .min_by_key(|z| z.distance(self.player))
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
        let is_cache = if self.house.is_some() {
            self.house_cache == Some(p)
        } else {
            self.world.tile_at(p).is_some_and(|tile| tile.cache)
        };
        is_cache && !self.searched.contains(&p)
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
                if self.is_walkable(dest) {
                    self.player = dest;
                }
            }
            Command::Scavenge if self.has_cache(self.player) => {
                self.searched.insert(self.player);
                self.armed = true;
                self.supplies = self.supplies.saturating_add(if self.house.is_some() {
                    RESIDENTIAL_AMMO
                } else {
                    3
                });
                self.events.push(Event::Scavenged { at: self.player });
            }
            Command::Scavenge => {}
            Command::Attack => self.attack(),
        }
        if self.tick.is_multiple_of(self.zombie_move_interval) {
            self.advance_zombies();
        }
    }

    fn attack(&mut self) {
        if self.house.is_some() {
            if let Some(at) = self.shot_target() {
                let from = self.player;
                self.supplies -= 1;
                if let Some(i) = self.zombies.iter().position(|z| *z == at) {
                    self.zombies.remove(i);
                }
                self.events.push(Event::Gunshot { from, at });
            }
            return;
        }
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
        if self.house.is_some() {
            for index in 0..self.zombies.len() {
                let from = self.zombies[index];
                if let Some(next) = self.next_path_step(from, self.player) {
                    self.zombies[index] = next;
                }
            }
        } else {
            for z in &mut self.zombies {
                let dx = self.player.x - z.x;
                let dy = self.player.y - z.y;
                if dx.abs() >= dy.abs() {
                    z.x += dx.signum();
                } else {
                    z.y += dy.signum();
                }
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

    fn is_walkable(&self, p: Position) -> bool {
        let in_block = self.block_bounds().is_none_or(|(min, max)| {
            (min.x..=max.x).contains(&p.x) && (min.y..=max.y).contains(&p.y)
        });
        in_block && self.world.district_at(p).is_some() && !self.house.is_some_and(|h| h.is_wall(p))
    }

    fn clear_shot(&self, from: Position, to: Position) -> bool {
        let Some(house) = self.house else {
            return true;
        };
        let (mut x, mut y) = (from.x, from.y);
        let dx = (to.x - from.x).abs();
        let dy = (to.y - from.y).abs();
        let sx = (to.x - from.x).signum();
        let sy = (to.y - from.y).signum();
        let (mut ix, mut iy) = (0, 0);
        while ix < dx || iy < dy {
            let x_cross = (1 + 2 * ix) * dy;
            let y_cross = (1 + 2 * iy) * dx;
            if x_cross == y_cross {
                // A ray touching a tile corner checks both neighbors, closing thin diagonal leaks.
                if house.is_wall(Position::new(x + sx, y))
                    || house.is_wall(Position::new(x, y + sy))
                {
                    return false;
                }
                x += sx;
                y += sy;
                ix += 1;
                iy += 1;
            } else if x_cross < y_cross {
                x += sx;
                ix += 1;
            } else {
                y += sy;
                iy += 1;
            }
            if Position::new(x, y) != to && house.is_wall(Position::new(x, y)) {
                return false;
            }
        }
        true
    }

    fn next_path_step(&self, start: Position, target: Position) -> Option<Position> {
        if start == target {
            return None;
        }
        const DIRECTIONS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
        let mut queue = VecDeque::from([start]);
        let mut parent = HashMap::from([(start, start)]);
        while let Some(p) = queue.pop_front() {
            for (dx, dy) in DIRECTIONS {
                let n = Position::new(p.x + dx, p.y + dy);
                if parent.contains_key(&n) || !self.is_walkable(n) {
                    continue;
                }
                parent.insert(n, p);
                if n == target {
                    let mut step = n;
                    while parent[&step] != start {
                        step = parent[&step];
                    }
                    return Some(step);
                }
                queue.push_back(n);
            }
        }
        None
    }
}

#[cfg(test)]
mod residential_tests {
    use super::*;

    fn interior_player(game: &mut Game) {
        let h = game.house.unwrap();
        game.player = Position::new(h.door.x, h.door.y - 1);
    }

    #[test]
    fn walls_block_but_door_and_interior_are_walkable() {
        let mut game = Game::residential(42);
        let h = game.house.unwrap();
        let (min, max) = game.block_bounds().unwrap();
        assert!(game.is_walkable(min));
        assert!(game.is_walkable(max));
        assert!(!game.is_walkable(Position::new(min.x - 1, min.y)));
        assert!(!game.is_walkable(Position::new(max.x + 1, max.y)));
        let wall = Position::new(h.x + h.width - 1, h.y + 2);
        assert!(!game.is_walkable(wall));
        assert!(game.is_walkable(h.door));
        assert!(game.is_walkable(Position::new(h.x + 1, h.y + 1)));

        // The spawn is two tiles south of the door; enter, cross the room, and try the east wall.
        for command in [Command::Move(Direction::North); 3] {
            game.step(command);
        }
        for command in [Command::Move(Direction::East); 2] {
            game.step(command);
        }
        let before = game.hud().player;
        game.step(Command::Move(Direction::East));
        assert_eq!(game.hud().player, before);
        assert_eq!(
            game.hud().player,
            Position::new(h.x + h.width - 2, h.door.y - 1)
        );
    }

    #[test]
    fn marked_interior_cache_grants_gun_and_six_rounds_once() {
        let mut game = Game::residential(42);
        let cache = Position::new(game.house.unwrap().x + 2, game.house.unwrap().y + 3);
        assert!(game.has_cache(cache));
        assert!(!game.hud().armed);
        assert_eq!(game.hud().supplies, 0);
        game.player = cache;
        game.step(Command::Scavenge);
        assert_eq!(game.events(), &[Event::Scavenged { at: cache }]);
        assert!(game.hud().armed);
        assert_eq!(game.hud().supplies, RESIDENTIAL_AMMO);
        assert!(!game.has_cache(cache));
        game.step(Command::Scavenge);
        assert_eq!(game.hud().supplies, RESIDENTIAL_AMMO);
    }

    #[test]
    fn nearest_visible_shot_skips_a_closer_zombie_behind_wall() {
        let mut game = Game::residential(42);
        let h = game.house.unwrap();
        game.player = Position::new(h.door.x, h.door.y - 1);
        game.armed = true;
        game.supplies = 6;
        let blocked = Position::new(h.door.x + 4, game.player.y);
        let visible = Position::new(h.door.x, h.door.y + 4);
        game.zombies = vec![blocked, visible];
        assert_eq!(blocked.distance(game.player), 4);
        assert_eq!(visible.distance(game.player), 5);
        assert_eq!(game.shot_target(), Some(visible));
        game.step(Command::Attack);
        assert_eq!(
            game.events(),
            &[Event::Gunshot {
                from: game.player,
                at: visible
            }]
        );
        assert_eq!(game.snapshot().zombies, &[blocked]);
        assert_eq!(game.hud().supplies, 5);
    }

    #[test]
    fn line_of_sight_blocks_wall_corner_grazes() {
        let game = Game::residential(42);
        let h = game.house.unwrap();
        let from = Position::new(h.x - 2, h.y);
        let target = Position::new(h.x + 2, h.y - 1);
        assert!(!game.clear_shot(from, target));
        assert!(game.clear_shot(
            Position::new(h.door.x, h.door.y - 2),
            Position::new(h.door.x, h.door.y + 2),
        ));
    }

    #[test]
    fn blocked_shot_costs_nothing_and_one_shot_kills_one_target() {
        let mut game = Game::residential(42);
        let h = game.house.unwrap();
        game.player = Position::new(h.door.x, h.door.y - 1);
        game.armed = true;
        game.supplies = 6;
        let blocked = Position::new(h.door.x + 4, game.player.y);
        game.zombies = vec![blocked];
        game.step(Command::Attack);
        assert!(game.events().is_empty());
        assert_eq!(game.hud().supplies, 6);
        assert_eq!(game.hud().zombie_count, 1);

        game.player = Position::new(h.door.x, h.door.y + 2);
        game.zombies = vec![
            Position::new(h.door.x, h.door.y + 3),
            Position::new(h.door.x + 1, h.door.y + 3),
        ];
        for expected in [game.zombies[0], game.zombies[1]] {
            let from = game.player;
            game.step(Command::Attack);
            assert_eq!(game.events(), &[Event::Gunshot { from, at: expected }]);
        }
        assert_eq!(game.hud().zombie_count, 0);
        assert_eq!(game.hud().supplies, 4);
    }

    #[test]
    fn zombies_replay_the_same_door_path() {
        let mut game = Game::residential(42);
        let mut replay = Game::residential(42);
        let h = game.house.unwrap();
        interior_player(&mut game);
        interior_player(&mut replay);
        let outside = Position::new(h.door.x, h.door.y + 6);
        game.zombies = vec![outside];
        replay.zombies = vec![outside];
        for tick in 1..=112 {
            game.step(Command::Wait);
            replay.step(Command::Wait);
            assert_eq!(game.snapshot(), replay.snapshot());
            assert_eq!(game.events(), replay.events());
            if tick == 96 {
                assert_eq!(game.zombies[0], h.door);
            }
        }
        assert_eq!(game.zombies[0], Position::new(h.door.x, h.door.y - 1));
    }
}
