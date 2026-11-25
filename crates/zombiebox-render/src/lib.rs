//! Small dependency-free software renderer for the Zombiebox action game.
//!
//! Pixels are packed RGBA8888 in row-major order (`width * height * 4` bytes),
//! which can be copied directly into an Android `ARGB_8888`/RGBA bitmap after
//! accounting for platform channel order. Coordinates use a 2:1 diamond grid.

use zombiebox_core::{ActionEvent, ActionGame, ActionOutcome, Ground, Position, Vec2};

mod actors;
mod street;

use actors::{ActorKind, ActorVisual, draw_actor, draw_fallen};

const TILE_W: f32 = 40.0;
const TILE_H: f32 = 20.0;
const SHOT_MS: u64 = 155;
const HURT_MS: u64 = 240;
const LOOT_MS: u64 = 410;
const FALL_MS: u64 = 270;

type Tile = (i32, i32, Ground, f32, f32);

#[derive(Default)]
struct Gait {
    previous: Option<Vec2>,
    stride: Vec2,
    phase: f32,
}

impl Gait {
    fn observe(&mut self, position: Vec2, cycles_per_tile: f32) {
        if let Some(previous) = self.previous {
            let motion = position - previous;
            self.stride = projected_vector(motion);
            self.phase = (self.phase + motion.x.hypot(motion.y) * cycles_per_tile).fract();
        }
        self.previous = Some(position);
    }
}

struct Infected {
    variant: usize,
    gait: Gait,
}

#[derive(Clone, Copy)]
struct Shot {
    from: Vec2,
    at: Vec2,
    facing: Vec2,
    started: u64,
    hit: bool,
}

struct Fallen {
    at: Vec2,
    variant: usize,
    started: u64,
}

enum Figure {
    Survivor,
    Infected(usize),
    Fallen(usize),
}

/// A portable RGBA framebuffer and camera for one game view.
pub struct Renderer {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    last_sim_tick: Option<u64>,
    survivor: Gait,
    infected: Vec<Infected>,
    fallen: Vec<Fallen>,
    visible_tiles: Vec<Tile>,
    figures: Vec<(Vec2, Figure)>,
    shot: Option<Shot>,
    hurt: Option<u64>,
    loot: Option<(Position, u64)>,
}

impl Renderer {
    pub fn new(width: usize, height: usize) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            pixels: vec![0; width * height * 4],
            last_sim_tick: None,
            survivor: Gait::default(),
            infected: Vec::new(),
            fallen: Vec::new(),
            visible_tiles: Vec::new(),
            figures: Vec::new(),
            shot: None,
            hurt: None,
            loot: None,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn stride(&self) -> usize {
        self.width * 4
    }
    pub fn framebuffer(&self) -> &[u8] {
        &self.pixels
    }

    /// Capture poses and transient events after each simulation step, including slow frames.
    /// `render` also calls this for loops that render after each step.
    pub fn observe_events(&mut self, game: &ActionGame, elapsed_ms: u64) {
        let hud = game.hud();
        if self.last_sim_tick == Some(hud.tick) {
            return;
        }
        let player = game.player_pose();
        self.survivor.observe(player.position, 0.72);
        for event in game.events() {
            match *event {
                ActionEvent::Looted { at } => self.loot = Some((at, elapsed_ms)),
                ActionEvent::Hurt | ActionEvent::Died => self.hurt = Some(elapsed_ms),
                ActionEvent::Shot { from, at } => {
                    self.shot = Some(Shot {
                        from,
                        at,
                        facing: player.facing,
                        started: elapsed_ms,
                        hit: false,
                    });
                }
                ActionEvent::InfectedDown { index, at } => {
                    let variant = if index < self.infected.len() {
                        self.infected.remove(index).variant
                    } else {
                        index
                    };
                    self.fallen.push(Fallen {
                        at,
                        variant,
                        started: elapsed_ms,
                    });
                    if let Some(shot) = &mut self.shot {
                        shot.hit = true;
                    }
                }
                ActionEvent::Won => {}
            }
        }

        let poses = game.zombie_poses();
        if self.infected.len() != poses.len() {
            self.infected.clear();
            self.infected
                .extend((0..poses.len()).map(|variant| Infected {
                    variant,
                    gait: Gait {
                        phase: variant as f32 * 0.27,
                        ..Gait::default()
                    },
                }));
        }
        for (infected, pose) in self.infected.iter_mut().zip(poses) {
            infected.gait.observe(pose.position, 1.65);
        }
        self.last_sim_tick = Some(hud.tick);
    }

    /// Render one current game state. `elapsed_ms` is the run's unpaused animation clock.
    pub fn render(&mut self, game: &ActionGame, elapsed_ms: u64) -> &[u8] {
        self.observe_events(game, elapsed_ms);
        let hud = game.hud();
        let player = game.player_pose();
        let p = player.position;
        let center_x = self.width as f32 * 0.5;
        let center_y = self.height as f32 * 0.51;
        let project = |x: f32, y: f32| -> (f32, f32) {
            (
                center_x + (x - p.x - y + p.y) * TILE_W * 0.5,
                center_y + (x - p.x + y - p.y) * TILE_H * 0.5,
            )
        };
        let width = self.width as f32;
        let height = self.height as f32;
        let radius = (width / (2.0 * TILE_W) + height / (2.0 * TILE_H)).ceil() as i32 + 2;
        let base_x = p.x.floor() as i32;
        let base_y = p.y.floor() as i32;
        self.visible_tiles.clear();
        for y in base_y - radius..=base_y + radius {
            for x in base_x - radius..=base_x + radius {
                let (sx, sy) = project(x as f32, y as f32);
                if sx < -TILE_W || sx > width + TILE_W || sy < -TILE_H || sy > height + TILE_H {
                    continue;
                }
                if let Some(tile) = game.world().tile_at(Position::new(x, y)) {
                    self.visible_tiles.push((x, y, tile.ground, sx, sy));
                }
            }
        }
        self.visible_tiles
            .sort_unstable_by_key(|&(x, y, ..)| (x + y, x));

        let mut c = Canvas::new(self.width, self.height, &mut self.pixels);
        c.clear([27, 36, 43, 255]);
        street::draw_ground(&mut c, game, &project, &self.visible_tiles);
        if let Some(house) = game.house() {
            street::draw_house(&mut c, house, &project);
        }
        let cache = game.cache_position();
        if game.has_cache_at(cache) {
            let (x, y) = project(cache.x as f32, cache.y as f32);
            street::draw_cache(&mut c, x, y, game.near_loot(), elapsed_ms);
        }

        let recoil = self
            .shot
            .map_or(0., |shot| life(elapsed_ms, shot.started, 115));
        let hurt = self
            .hurt
            .map_or(0., |started| life(elapsed_ms, started, HURT_MS));
        let poses = game.zombie_poses();
        self.figures.clear();
        self.figures.push((p, Figure::Survivor));
        self.figures.extend(
            poses
                .iter()
                .enumerate()
                .map(|(index, pose)| (pose.position, Figure::Infected(index))),
        );
        self.figures.extend(
            self.fallen
                .iter()
                .enumerate()
                .map(|(index, fallen)| (fallen.at, Figure::Fallen(index))),
        );
        self.figures
            .sort_unstable_by(|(a, _), (b, _)| (a.x + a.y).total_cmp(&(b.x + b.y)));

        for &(pos, ref figure) in &self.figures {
            let (x, y) = project(pos.x, pos.y);
            if x < -30. || x > width + 30. || y < -12. || y > height + 35. {
                continue;
            }
            match *figure {
                Figure::Survivor => draw_actor(
                    &mut c,
                    x,
                    y,
                    ActorVisual {
                        kind: ActorKind::Survivor,
                        facing: screen_direction(player.facing),
                        stride: self.survivor.stride,
                        phase: self.survivor.phase,
                        armed: hud.armed,
                        recoil,
                        hurt,
                    },
                ),
                Figure::Infected(index) => {
                    let infected = &self.infected[index];
                    draw_actor(
                        &mut c,
                        x,
                        y,
                        ActorVisual {
                            kind: ActorKind::Infected(infected.variant),
                            facing: screen_direction(poses[index].facing),
                            stride: infected.gait.stride,
                            phase: infected.gait.phase,
                            armed: false,
                            recoil: 0.,
                            hurt: 0.,
                        },
                    );
                }
                Figure::Fallen(index) => {
                    let fallen = &self.fallen[index];
                    let progress = if hud.outcome == ActionOutcome::Playing {
                        1. - life(elapsed_ms, fallen.started, FALL_MS)
                    } else {
                        1.
                    };
                    draw_fallen(&mut c, x, y, fallen.variant, progress);
                }
            }
        }

        if let Some((at, started)) = self.loot {
            let remaining = life(elapsed_ms, started, LOOT_MS);
            if remaining > 0. {
                let (x, y) = project(at.x as f32, at.y as f32);
                let rise = (1. - remaining) * 19.;
                spark(
                    &mut c,
                    x,
                    y - 11. - rise,
                    3. + rise * 0.2,
                    [255, 218, 131, (225. * remaining) as u8],
                );
            }
        }
        if let Some(shot) = self.shot {
            draw_shot(&mut c, &project, shot, elapsed_ms);
        }
        if hud.armed && hud.outcome == ActionOutcome::Playing {
            let aim = p + player.facing * 2.3;
            let (x, y) = project(aim.x, aim.y);
            draw_reticle(&mut c, x.round(), (y - 13.).round(), hud.ammo > 0);
        }
        &self.pixels
    }

    /// Convert an absolute screen point to a floating world tile position.
    pub fn screen_to_world(&self, game: &ActionGame, sx: f32, sy: f32) -> Vec2 {
        screen_to_world(game, self.width, self.height, sx, sy)
    }

    /// Convert a screen drag vector into world-space movement and normalize it.
    pub fn screen_vector_to_world(&self, sx: f32, sy: f32) -> Vec2 {
        screen_vector_to_world(sx, sy)
    }
}

fn projected_vector(v: Vec2) -> Vec2 {
    Vec2::new((v.x - v.y) * TILE_W * 0.5, (v.x + v.y) * TILE_H * 0.5)
}

fn screen_direction(v: Vec2) -> Vec2 {
    let v = projected_vector(v);
    let length = v.x.hypot(v.y);
    if length > 0. {
        v * (1. / length)
    } else {
        Vec2::ZERO
    }
}

fn life(now: u64, started: u64, duration: u64) -> f32 {
    duration.saturating_sub(now.saturating_sub(started)) as f32 / duration as f32
}

fn draw_shot(c: &mut Canvas<'_>, project: &impl Fn(f32, f32) -> (f32, f32), shot: Shot, now: u64) {
    let remaining = life(now, shot.started, SHOT_MS);
    if remaining <= 0. {
        return;
    }

    let (mut x, mut y) = project(shot.from.x, shot.from.y);
    let (tx, ty) = project(shot.at.x, shot.at.y);
    let direction = screen_direction(shot.facing);
    let muzzle = (tx - x).hypot(ty - y).min(12.);
    x += direction.x * muzzle;
    y += direction.y * muzzle - 15.;
    let impact_y = ty - 15.;
    let trace = life(now, shot.started, 78);
    if trace > 0. {
        c.glow(x, y, 17., [255, 168, 76], 105. * trace);
        c.line(x, y, tx, impact_y, [255, 210, 132, (185. * trace) as u8]);
        spark(
            c,
            x,
            y,
            5. * trace + 1.,
            [255, 236, 173, (255. * trace) as u8],
        );
    }
    let impact = if shot.hit {
        [255, 164, 101]
    } else {
        [206, 192, 152]
    };
    let alpha = (222. * remaining) as u8;
    spark(
        c,
        tx,
        impact_y,
        3. + (1. - remaining) * 5.,
        [impact[0], impact[1], impact[2], alpha],
    );
}

fn spark(c: &mut Canvas<'_>, x: f32, y: f32, size: f32, ink: [u8; 4]) {
    c.diamond(x, y, size * 0.66, size, ink);
    c.line(x - size, y, x + size, y, ink);
}

fn draw_reticle(c: &mut Canvas<'_>, x: f32, y: f32, loaded: bool) {
    let ink = if loaded {
        [255, 219, 138, 255]
    } else {
        [197, 181, 164, 215]
    };
    c.diamond(x, y, 6., 4., [25, 28, 37, 185]);
    c.line(x - 6., y, x - 3., y, ink);
    c.line(x + 3., y, x + 6., y, ink);
    c.line(x, y - 4., x, y - 3., ink);
    c.line(x, y + 3., x, y + 4., ink);
    c.put(x as i32, y as i32, ink);
}

/// Map a screen point to world coordinates using the renderer's 2:1 diamond projection.
pub fn screen_to_world(game: &ActionGame, width: usize, height: usize, sx: f32, sy: f32) -> Vec2 {
    let p = game.player_pose().position;
    let dx = (sx - width as f32 * 0.5) / (TILE_W * 0.5);
    let dy = (sy - height as f32 * 0.51) / (TILE_H * 0.5);
    Vec2 {
        x: p.x + (dx + dy) * 0.5,
        y: p.y + (dy - dx) * 0.5,
    }
}

/// Convert a screen drag vector into normalized world-space movement.
pub fn screen_vector_to_world(sx: f32, sy: f32) -> Vec2 {
    let dx = sx / (TILE_W * 0.5);
    let dy = sy / (TILE_H * 0.5);
    let mut out = Vec2 {
        x: (dx + dy) * 0.5,
        y: (dy - dx) * 0.5,
    };
    let length = (out.x * out.x + out.y * out.y).sqrt();
    if length > 1.0 {
        out.x /= length;
        out.y /= length;
    }
    out
}

struct Canvas<'a> {
    w: usize,
    h: usize,
    p: &'a mut [u8],
}
impl<'a> Canvas<'a> {
    fn new(w: usize, h: usize, p: &'a mut [u8]) -> Self {
        Self { w, h, p }
    }
    fn clear(&mut self, color: [u8; 4]) {
        for px in self.p.chunks_exact_mut(4) {
            px.copy_from_slice(&color);
        }
    }
    fn put(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = (y as usize * self.w + x as usize) * 4;
        let a = color[3] as u32;
        if a == 255 {
            self.p[i..i + 4].copy_from_slice(&color);
            return;
        }
        if a == 0 {
            return;
        }
        let inv = 255 - a;
        for k in 0..3 {
            self.p[i + k] = ((color[k] as u32 * a + self.p[i + k] as u32 * inv) / 255) as u8;
        }
        self.p[i + 3] = 255;
    }
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: [u8; 4]) {
        let x0 = (x - w * 0.5).floor() as i32;
        let y0 = (y - h * 0.5).floor() as i32;
        let x1 = (x + w * 0.5).ceil() as i32;
        let y1 = (y + h * 0.5).ceil() as i32;
        for yy in y0.max(0)..y1.min(self.h as i32) {
            self.span(x0, x1, yy, c);
        }
    }
    fn span(&mut self, x0: i32, x1: i32, y: i32, color: [u8; 4]) {
        let x0 = x0.max(0);
        let x1 = x1.min(self.w as i32);
        if x0 >= x1 {
            return;
        }
        if color[3] == 255 {
            let row = y as usize * self.w;
            for pixel in
                self.p[(row + x0 as usize) * 4..(row + x1 as usize) * 4].chunks_exact_mut(4)
            {
                pixel.copy_from_slice(&color);
            }
        } else {
            for x in x0..x1 {
                self.put(x, y, color);
            }
        }
    }
    fn ellipse(&mut self, x: f32, y: f32, rx: f32, ry: f32, c: [u8; 4]) {
        let x0 = (x - rx).floor() as i32;
        let x1 = (x + rx).ceil() as i32;
        let y0 = (y - ry).floor() as i32;
        let y1 = (y + ry).ceil() as i32;
        for yy in y0..=y1 {
            for xx in x0..=x1 {
                let dx = (xx as f32 - x) / rx.max(0.1);
                let dy = (yy as f32 - y) / ry.max(0.1);
                if dx * dx + dy * dy <= 1. {
                    self.put(xx, yy, c);
                }
            }
        }
    }
    fn diamond(&mut self, x: f32, y: f32, rx: f32, ry: f32, c: [u8; 4]) {
        let y0 = ((y - ry).ceil() as i32).max(0);
        let y1 = ((y + ry).floor() as i32).min(self.h as i32 - 1);
        for yy in y0..=y1 {
            let half = rx * (1.0 - (yy as f32 - y).abs() / ry.max(0.1));
            self.span(
                (x - half).ceil() as i32,
                (x + half).floor() as i32 + 1,
                yy,
                c,
            );
        }
    }
    // The scene uses convex polygons for wall faces, fixtures and prop silhouettes.
    fn polygon(&mut self, points: &[(f32, f32)], color: [u8; 4]) {
        let y0 = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
        let y1 = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
        let edges = || points.iter().zip(points.iter().cycle().skip(1));
        for row in (y0.ceil() as i32).max(0)..=(y1.floor() as i32).min(self.h as i32 - 1) {
            let y = row as f32;
            let mut left = f32::INFINITY;
            let mut right = f32::NEG_INFINITY;
            for (&(ax, ay), &(bx, by)) in edges() {
                if y >= ay.min(by) && y < ay.max(by) {
                    let x = ax + (y - ay) * (bx - ax) / (by - ay);
                    left = left.min(x);
                    right = right.max(x);
                }
            }
            if left <= right {
                self.span(left.ceil() as i32, right.floor() as i32 + 1, row, color);
            }
        }
    }
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: [u8; 4]) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let n = dx.abs().max(dy.abs()).ceil() as i32;
        if n == 0 {
            self.put(x0 as i32, y0 as i32, c);
            return;
        }
        for i in 0..=n {
            let t = i as f32 / n as f32;
            self.put(
                (x0 + dx * t).round() as i32,
                (y0 + dy * t).round() as i32,
                c,
            );
        }
    }
    fn glow(&mut self, cx: f32, cy: f32, radius: f32, color: [u8; 3], strength: f32) {
        let x0 = (cx - radius).floor().max(0.0) as i32;
        let x1 = (cx + radius).ceil().min(self.w as f32 - 1.0) as i32;
        let y0 = (cy - radius).floor().max(0.0) as i32;
        let y1 = (cy + radius).ceil().min(self.h as f32 - 1.0) as i32;
        let inv_radius_squared = 1. / (radius * radius);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let falloff = 1. - (dx * dx + dy * dy) * inv_radius_squared;
                if falloff > 0. {
                    let a = (strength * falloff * falloff) as u8;
                    self.put(x, y, [color[0], color[1], color[2], a]);
                }
            }
        }
    }
}
