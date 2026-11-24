//! Smooth, fixed-step residential action rules. The legacy tile-command runtime remains unchanged.

use crate::{Game, House, Position, World};

pub const ACTION_TICKS_PER_SECOND: u32 = 60;
const PLAYER_SPEED: f32 = 3.4;
const ZOMBIE_SPEED: f32 = 0.85;
const PLAYER_RADIUS: f32 = 0.23;
const ZOMBIE_RADIUS: f32 = 0.25;
const SHOT_RANGE: f32 = 8.0;
const INTERACT_RADIUS: f32 = 0.85;
const FIRE_COOLDOWN: u8 = 12;
const BITE_COOLDOWN: u8 = 45;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
    fn normalized(self) -> Self {
        let len = self.length();
        if len > 0.0001 {
            Self::new(self.x / len, self.y / len)
        } else {
            Self::ZERO
        }
    }
    fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }
    fn tile(self) -> Position {
        Position::new(self.x.round() as i32, self.y.round() as i32)
    }
}
impl std::ops::Add for Vec2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}
impl std::ops::Sub for Vec2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}
impl std::ops::Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vec2,
    pub facing: Vec2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActionInput {
    /// Directional movement, with analog magnitude preserved up to one.
    pub movement: Vec2,
    /// Aim direction. A zero vector or `None` preserves the prior aim direction.
    pub aim: Option<Vec2>,
    pub fire: bool,
    pub interact: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ActionEvent {
    Looted { at: Position },
    Shot { from: Vec2, at: Vec2 },
    Hurt,
    Died,
    Won,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionOutcome {
    Playing,
    Dead,
    Won,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionHud {
    pub tick: u64,
    pub health: u8,
    pub ammo: u16,
    pub armed: bool,
    pub zombie_count: usize,
    pub outcome: ActionOutcome,
}

pub struct ActionGame {
    geometry: Game,
    tick: u64,
    player: Pose,
    zombies: Vec<Pose>,
    health: u8,
    ammo: u16,
    armed: bool,
    cache_taken: bool,
    cache_position: Position,
    fire_cooldown: u8,
    bite_cooldowns: Vec<u8>,
    events: Vec<ActionEvent>,
    outcome: ActionOutcome,
    aim_explicit: bool,
}

impl ActionGame {
    pub fn residential(seed: u64) -> Self {
        let geometry = Game::residential(seed);
        let player = Vec2::new(
            geometry.hud().player.x as f32,
            geometry.hud().player.y as f32,
        );
        let zombies = geometry
            .snapshot()
            .zombies
            .iter()
            .map(|p| Pose {
                position: Vec2::new(p.x as f32, p.y as f32),
                facing: Vec2::new(-1.0, 0.0),
            })
            .collect::<Vec<_>>();
        let house = geometry.house().expect("residential geometry has a house");
        let cache_position = Position::new(house.x + 2, house.y + 3);
        let bite_cooldowns = vec![0; zombies.len()];
        Self {
            geometry,
            tick: 0,
            player: Pose {
                position: player,
                facing: Vec2::new(0.0, -1.0),
            },
            zombies,
            health: 5,
            ammo: 0,
            armed: false,
            cache_taken: false,
            cache_position,
            fire_cooldown: 0,
            bite_cooldowns,
            events: Vec::new(),
            outcome: ActionOutcome::Playing,
            aim_explicit: false,
        }
    }

    pub fn step(&mut self, input: ActionInput) {
        self.events.clear();
        if self.outcome != ActionOutcome::Playing {
            return;
        }
        self.tick += 1;
        self.fire_cooldown = self.fire_cooldown.saturating_sub(1);
        for cooldown in &mut self.bite_cooldowns {
            *cooldown = cooldown.saturating_sub(1);
        }

        let movement_len = input.movement.length();
        let movement = if movement_len > 1.0 {
            input.movement * (1.0 / movement_len)
        } else {
            input.movement
        };
        if movement.length() > 0.0 {
            Self::move_body(
                &self.geometry,
                &mut self.player.position,
                movement * (PLAYER_SPEED / ACTION_TICKS_PER_SECOND as f32),
                PLAYER_RADIUS,
            );
        }
        if let Some(aim) = input.aim {
            let aim = aim.normalized();
            if aim.length() > 0.0 {
                self.player.facing = aim;
                self.aim_explicit = true;
            }
        } else if movement.length() > 0.0 && !self.aim_explicit {
            self.player.facing = movement;
        }

        if input.interact && self.near_loot() {
            self.cache_taken = true;
            self.armed = true;
            self.ammo = 6;
            self.events.push(ActionEvent::Looted {
                at: self.cache_position,
            });
        }
        if input.fire && self.fire_cooldown == 0 && self.armed && self.ammo > 0 {
            self.fire();
        }
        self.advance_zombies();
        self.resolve_bites();
    }

    pub fn player_pose(&self) -> Pose {
        self.player
    }
    pub fn zombie_poses(&self) -> &[Pose] {
        &self.zombies
    }
    pub fn world(&self) -> &World {
        self.geometry.world()
    }
    pub fn house(&self) -> Option<House> {
        self.geometry.house()
    }
    pub fn block_bounds(&self) -> Option<(Position, Position)> {
        self.geometry.block_bounds()
    }
    pub fn has_cache_at(&self, p: Position) -> bool {
        p == self.cache_position && !self.cache_taken
    }
    /// True while the player is close enough to collect the house pistol.
    pub fn near_loot(&self) -> bool {
        !self.cache_taken
            && self.player.position.distance(Vec2::new(
                self.cache_position.x as f32,
                self.cache_position.y as f32,
            )) <= INTERACT_RADIUS
    }
    pub fn cache_position(&self) -> Position {
        self.cache_position
    }
    pub fn events(&self) -> &[ActionEvent] {
        &self.events
    }
    pub fn outcome(&self) -> ActionOutcome {
        self.outcome
    }
    pub fn hud(&self) -> ActionHud {
        ActionHud {
            tick: self.tick,
            health: self.health,
            ammo: self.ammo,
            armed: self.armed,
            zombie_count: self.zombies.len(),
            outcome: self.outcome,
        }
    }

    fn move_body(geometry: &Game, position: &mut Vec2, delta: Vec2, radius: f32) {
        let steps = ((delta.length() / 0.08).ceil() as usize).max(1);
        let part = delta * (1.0 / steps as f32);
        for _ in 0..steps {
            let candidate_x = Vec2::new(position.x + part.x, position.y);
            if Self::circle_walkable(geometry, candidate_x, radius) {
                position.x = candidate_x.x;
            }
            let candidate_y = Vec2::new(position.x, position.y + part.y);
            if Self::circle_walkable(geometry, candidate_y, radius) {
                position.y = candidate_y.y;
            }
        }
    }

    fn circle_walkable(geometry: &Game, center: Vec2, radius: f32) -> bool {
        let min_x = (center.x - radius - 0.5).ceil() as i32;
        let max_x = (center.x + radius + 0.5).floor() as i32;
        let min_y = (center.y - radius - 0.5).ceil() as i32;
        let max_y = (center.y + radius + 0.5).floor() as i32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let tile = Position::new(x, y);
                if geometry.action_is_walkable(tile) {
                    continue;
                }
                let nearest_x = center.x.clamp(x as f32 - 0.5, x as f32 + 0.5);
                let nearest_y = center.y.clamp(y as f32 - 0.5, y as f32 + 0.5);
                let dx = center.x - nearest_x;
                let dy = center.y - nearest_y;
                if dx * dx + dy * dy < radius * radius {
                    return false;
                }
            }
        }
        true
    }

    fn fire(&mut self) {
        let from = self.player.position;
        let direction = self.player.facing.normalized();
        let end = self.ray_end(from, direction);
        let max_distance = from.distance(end);
        let mut best: Option<(usize, f32)> = None;
        for (i, z) in self.zombies.iter().enumerate() {
            let offset = z.position - from;
            let along = offset.x * direction.x + offset.y * direction.y;
            if !(0.0..=max_distance).contains(&along) {
                continue;
            }
            let side = (offset.x * direction.y - offset.y * direction.x).abs();
            if side > ZOMBIE_RADIUS + 0.08 {
                continue;
            }
            if best.is_none_or(|(_, distance)| along < distance) {
                best = Some((i, along));
            }
        }
        self.ammo -= 1;
        self.fire_cooldown = FIRE_COOLDOWN;
        if let Some((i, _)) = best {
            let at = self.zombies.remove(i).position;
            self.bite_cooldowns.remove(i);
            self.events.push(ActionEvent::Shot { from, at });
            if self.zombies.is_empty() {
                self.outcome = ActionOutcome::Won;
                self.events.push(ActionEvent::Won);
            }
        } else {
            self.events.push(ActionEvent::Shot { from, at: end });
        }
    }

    fn ray_end(&self, from: Vec2, direction: Vec2) -> Vec2 {
        let samples = (SHOT_RANGE / 0.06).ceil() as usize;
        for i in 1..=samples {
            let distance = (i as f32 * 0.06).min(SHOT_RANGE);
            let p = from + direction * distance;
            if !self.geometry.action_is_walkable(p.tile()) {
                return p;
            }
        }
        from + direction * SHOT_RANGE
    }

    fn advance_zombies(&mut self) {
        let target = self.player.position.tile();
        let geometry = &self.geometry;
        for z in &mut self.zombies {
            let mut direction = (self.player.position - z.position).normalized();
            if z.position.distance(self.player.position) > 1.4 {
                if let Some(next) = geometry.action_next_path_step(z.position.tile(), target) {
                    let waypoint = Vec2::new(next.x as f32, next.y as f32);
                    direction = (waypoint - z.position).normalized();
                } else if z.position.tile() == target {
                    direction = (self.player.position - z.position).normalized();
                }
            }
            z.facing = direction;
            let delta = direction * (ZOMBIE_SPEED / ACTION_TICKS_PER_SECOND as f32);
            let mut next = z.position;
            Self::move_body(geometry, &mut next, delta, ZOMBIE_RADIUS);
            z.position = next;
        }
    }

    fn resolve_bites(&mut self) {
        for (i, z) in self.zombies.iter().enumerate() {
            if z.position.distance(self.player.position) <= PLAYER_RADIUS + ZOMBIE_RADIUS + 0.12
                && self.bite_cooldowns[i] == 0
            {
                self.health = self.health.saturating_sub(1);
                self.bite_cooldowns[i] = BITE_COOLDOWN;
                self.events.push(ActionEvent::Hurt);
                if self.health == 0 {
                    self.outcome = ActionOutcome::Dead;
                    self.events.push(ActionEvent::Died);
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analog_movement_is_smooth_normalized_and_wall_blocked() {
        let mut game = ActionGame::residential(42);
        let start = game.player_pose().position;
        game.step(ActionInput {
            movement: Vec2::new(1.0, 1.0),
            ..ActionInput::default()
        });
        let moved = game.player_pose().position;
        assert!(moved.x > start.x && moved.y > start.y);
        assert!((moved.distance(start) - PLAYER_SPEED / 60.0).abs() < 0.0001);
        let h = game.house().unwrap();
        game.player.position = Vec2::new((h.x + h.width - 2) as f32, (h.y + 3) as f32);
        for _ in 0..30 {
            game.step(ActionInput {
                movement: Vec2::new(1.0, 0.0),
                ..ActionInput::default()
            });
        }
        assert!(game.player.position.x < (h.x + h.width - 1) as f32 - PLAYER_RADIUS + 0.02);
    }

    #[test]
    fn interact_picks_up_house_pistol_and_directional_shot_kills() {
        let mut game = ActionGame::residential(42);
        let cache = game.cache_position();
        game.player.position = Vec2::new(cache.x as f32, cache.y as f32);
        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        assert_eq!(game.hud().ammo, 6);
        assert!(game.events().contains(&ActionEvent::Looted { at: cache }));
        game.zombies = vec![Pose {
            position: game.player.position + Vec2::new(2.0, 0.0),
            facing: Vec2::new(-1.0, 0.0),
        }];
        game.bite_cooldowns = vec![10];
        game.player.facing = Vec2::new(1.0, 0.0);
        game.step(ActionInput {
            fire: true,
            ..ActionInput::default()
        });
        assert_eq!(game.hud().zombie_count, 0);
        assert_eq!(game.hud().ammo, 5);
        assert_eq!(game.outcome(), ActionOutcome::Won);
    }

    #[test]
    fn wall_blocks_directional_shot_and_seed_replays() {
        let mut a = ActionGame::residential(3);
        let h = a.house().unwrap();
        a.player.position = Vec2::new((h.x - 2) as f32, h.y as f32);
        a.player.facing = Vec2::new(1.0, 0.0);
        a.armed = true;
        a.ammo = 6;
        a.zombies = vec![Pose {
            position: Vec2::new((h.x + 2) as f32, h.y as f32),
            facing: Vec2::ZERO,
        }];
        a.bite_cooldowns = vec![30];
        let mut b = ActionGame::residential(3);
        b.player = a.player;
        b.armed = true;
        b.ammo = 6;
        b.zombies = a.zombies.clone();
        b.bite_cooldowns = a.bite_cooldowns.clone();
        for tick in 0..20 {
            let input = ActionInput {
                fire: tick == 0,
                ..ActionInput::default()
            };
            a.step(input);
            b.step(input);
            if tick == 0 {
                assert!(matches!(a.events(), [ActionEvent::Shot { .. }]));
            }
        }
        assert_eq!(a.zombie_poses(), b.zombie_poses());
        assert_eq!(a.hud(), b.hud());
        assert_eq!(a.hud().ammo, 5);
        assert_eq!(a.hud().zombie_count, 1);
    }

    #[test]
    fn aim_stays_locked_after_right_stick_release_while_moving() {
        let mut game = ActionGame::residential(42);
        game.step(ActionInput {
            aim: Some(Vec2::new(1.0, 0.0)),
            ..ActionInput::default()
        });
        game.step(ActionInput {
            movement: Vec2::new(0.0, 1.0),
            ..ActionInput::default()
        });
        assert_eq!(game.player_pose().facing, Vec2::new(1.0, 0.0));
    }
}
