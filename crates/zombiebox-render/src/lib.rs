//! Small dependency-free software renderer for the Zombiebox action game.
//!
//! Pixels are packed RGBA8888 in row-major order (`width * height * 4` bytes),
//! which can be copied directly into an Android `ARGB_8888`/RGBA bitmap after
//! accounting for platform channel order. Coordinates use a 2:1 diamond grid.

use zombiebox_core::{ActionEvent, ActionGame, Ground, House, Position, Vec2};

const TILE_W: f32 = 40.0;
const TILE_H: f32 = 20.0;
const COLORS: [[u8; 3]; 8] = [
    [54, 67, 62],
    [62, 72, 66],
    [57, 70, 64],
    [68, 77, 68],
    [73, 78, 67],
    [51, 67, 62],
    [65, 73, 66],
    [58, 68, 64],
];

/// A portable RGBA framebuffer and camera for one game view.
pub struct Renderer {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    last_player: Option<Vec2>,
    last_events_tick: u64,
    hit_flash_until_ms: u64,
    shot_flash_until_ms: u64,
    shot_from: Option<Vec2>,
    shot_at: Option<Vec2>,
    walking_phase: f32,
    player_moving: bool,
    last_sim_tick: Option<u64>,
}

impl Renderer {
    pub fn new(width: usize, height: usize) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            pixels: vec![0; width * height * 4],
            last_player: None,
            last_events_tick: 0,
            hit_flash_until_ms: 0,
            shot_flash_until_ms: 0,
            shot_from: None,
            shot_at: None,
            walking_phase: 0.0,
            player_moving: false,
            last_sim_tick: None,
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

    /// Capture transient events after each simulation step, including when a frame is slow.
    /// Call once after every `ActionGame::step`; `render` also calls this for simple loops.
    pub fn observe_events(&mut self, game: &ActionGame, elapsed_ms: u64) {
        let hud = game.hud();
        if hud.tick == self.last_events_tick {
            return;
        }
        for event in game.events() {
            match event {
                ActionEvent::Hurt | ActionEvent::Died => self.hit_flash_until_ms = elapsed_ms + 260,
                ActionEvent::Shot { from, at } => {
                    self.shot_flash_until_ms = elapsed_ms + 180;
                    self.shot_from = Some(*from);
                    self.shot_at = Some(*at);
                }
                _ => {}
            }
        }
        self.last_events_tick = hud.tick;
    }

    /// Render one current game state. `elapsed_ms` is monotonic time for animation.
    pub fn render(&mut self, game: &ActionGame, elapsed_ms: u64) -> &[u8] {
        let player = game.player_pose();
        let p = player.position;
        let sim_tick = game.hud().tick;
        if self.last_sim_tick != Some(sim_tick) {
            self.player_moving = false;
            if let Some(prev) = self.last_player {
                let dx = p.x - prev.x;
                let dy = p.y - prev.y;
                let distance = (dx * dx + dy * dy).sqrt();
                if distance > 0.002 {
                    self.player_moving = true;
                    self.walking_phase += distance * 0.72;
                }
            }
            self.last_player = Some(p);
            self.last_sim_tick = Some(sim_tick);
        }
        self.observe_events(game, elapsed_ms);
        let hud = game.hud();

        let mut c = Canvas::new(self.width, self.height, &mut self.pixels);
        c.clear([15, 23, 24, 255]);
        let center_x = self.width as f32 * 0.5;
        let center_y = self.height as f32 * 0.51;
        let project = |x: f32, y: f32| -> (f32, f32) {
            (
                center_x + (x - p.x - y + p.y) * TILE_W * 0.5,
                center_y + (x - p.x + y - p.y) * TILE_H * 0.5,
            )
        };
        let radius = (self.width as f32 / (2.0 * TILE_W) + self.height as f32 / (2.0 * TILE_H))
            .ceil() as i32
            + 3;
        let base_x = p.x.floor() as i32;
        let base_y = p.y.floor() as i32;
        let mut tiles = Vec::new();
        for y in base_y - radius..=base_y + radius {
            for x in base_x - radius..=base_x + radius {
                if let Some(tile) = game.world().tile_at(Position::new(x, y)) {
                    let (sx, sy) = project(x as f32, y as f32);
                    if sx > -TILE_W
                        && sx < self.width as f32 + TILE_W
                        && sy > -TILE_H
                        && sy < self.height as f32 + TILE_H
                    {
                        tiles.push((x + y, x, y, tile.ground, sx, sy));
                    }
                }
            }
        }
        tiles.sort_by_key(|t| t.0);
        for (_, x, y, ground, sx, sy) in tiles {
            let mut base = ground_color(ground, x, y);
            let house = game.house();
            let in_house = house
                .is_some_and(|h| x >= h.x && x < h.x + h.width && y >= h.y && y < h.y + h.height);
            if in_house
                && !house.is_some_and(|h| {
                    x == h.x || x == h.x + h.width - 1 || y == h.y || y == h.y + h.height - 1
                })
            {
                base = [91, 76, 58];
            }
            c.diamond(
                sx,
                sy,
                TILE_W * 0.5,
                TILE_H * 0.5,
                [base[0], base[1], base[2], 255],
            );
            // Scuffed pavement seams and flecks make the repeating terrain feel hand-laid.
            let hash = hash2(x, y);
            let ox = ((hash & 15) as f32 - 7.5) * 0.82;
            let oy = (((hash >> 4) & 7) as f32 - 3.5) * 0.54;
            if hash.is_multiple_of(4) {
                c.rect(sx + ox, sy + oy, 2.2, 1.1, [117, 117, 98, 95]);
            }
            if ground == Ground::Road
                && (x.div_euclid(16) != y.div_euclid(16))
                && y.rem_euclid(4) == 0
            {
                c.line(sx - 2.0, sy, sx + 2.0, sy, [174, 157, 107, 95]);
            }
            if game.has_cache_at(Position::new(x, y)) {
                c.glow(sx, sy - 4.0, 31.0, [243, 172, 65], 92.0);
                draw_cache(&mut c, sx, sy);
            }
            // Fine dark edges provide readable separation without a grid overlay.
            c.line(
                sx - TILE_W * 0.5,
                sy,
                sx,
                sy + TILE_H * 0.5,
                [16, 27, 28, 95],
            );
            c.line(
                sx,
                sy + TILE_H * 0.5,
                sx + TILE_W * 0.5,
                sy,
                [16, 27, 28, 95],
            );
        }

        if let Some(h) = game.house() {
            draw_house(&mut c, h, &project);
        }

        // Small curbside objects sit on a stable coordinate hash, so the block feels lived in.
        for y in base_y - radius..=base_y + radius {
            for x in base_x - radius..=base_x + radius {
                let hash = hash2(x + 31, y - 7);
                if hash.is_multiple_of(43) {
                    let (sx, sy) = project(x as f32, y as f32);
                    if game
                        .world()
                        .tile_at(Position::new(x, y))
                        .is_some_and(|t| t.ground == Ground::Grass)
                        && !game.house().is_some_and(|h| {
                            x >= h.x && x < h.x + h.width && y >= h.y && y < h.y + h.height
                        })
                    {
                        draw_bush(&mut c, sx, sy, hash as usize);
                    } else {
                        draw_rubble(&mut c, sx, sy);
                    }
                }
            }
        }

        let zombies = game.zombie_poses();
        let mut actors: Vec<(f32, bool, Vec2, Vec2, usize)> = zombies
            .iter()
            .enumerate()
            .map(|(i, z)| (z.position.x + z.position.y, false, z.position, z.facing, i))
            .collect();
        actors.push((p.x + p.y, true, p, player.facing, 0));
        actors.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, is_player, pos, facing, index) in actors {
            let (sx, sy) = project(pos.x, pos.y);
            if sx < -32.
                || sx > self.width as f32 + 32.
                || sy < -32.
                || sy > self.height as f32 + 32.
            {
                continue;
            }
            c.ellipse(sx, sy + 5., 8., 3., [5, 10, 11, 110]);
            let moving = if is_player { self.player_moving } else { true };
            draw_actor(
                &mut c,
                (sx, sy),
                Vec2::new(facing.x - facing.y, (facing.x + facing.y) * 0.5),
                is_player,
                ActorAnimation {
                    moving,
                    phase: self.walking_phase,
                    elapsed_ms,
                    index,
                },
            );
        }

        if elapsed_ms < self.shot_flash_until_ms {
            if let (Some(from), Some(at)) = (self.shot_from, self.shot_at) {
                let (sx, sy) = project(from.x, from.y);
                let (tx, ty) = project(at.x, at.y);
                let fade = ((self.shot_flash_until_ms - elapsed_ms) as f32 / 180.0).clamp(0.0, 1.0);
                let alpha = (205.0 * fade) as u8;
                c.line(sx, sy - 6.0, tx, ty - 6.0, [255, 214, 124, alpha]);
                c.ellipse(tx, ty - 6.0, 5.0 * fade, 4.0 * fade, [255, 206, 102, alpha]);
            }
        }
        if elapsed_ms < self.hit_flash_until_ms {
            c.rect(
                self.width as f32 * 0.5,
                self.height as f32 * 0.5,
                self.width as f32,
                self.height as f32,
                [150, 22, 27, 42],
            );
        }

        // Warm local lantern pools and cold night vignette are applied as a cheap screen-space pass.
        let (px, py) = project(p.x, p.y);
        c.lighting(px, py, elapsed_ms < self.hit_flash_until_ms);
        if game.has_cache_at(game.cache_position()) {
            let cp = game.cache_position();
            let (cx, cy) = project(cp.x as f32, cp.y as f32);
            c.glow(cx, cy - 5.0, 54.0, [246, 168, 68], 52.0);
        }
        if hud.armed {
            let f = player.facing;
            let (ax, ay) = project(p.x + f.x * 2.3, p.y + f.y * 2.3);
            let pulse = (elapsed_ms as f32 * 0.008).sin().abs();
            let radius = 4.2 + pulse * 1.8;
            c.ellipse(ax, ay - 4.0, radius + 2.0, radius + 2.0, [218, 174, 83, 75]);
            c.ellipse(ax, ay - 4.0, radius, radius, [238, 208, 136, 190]);
            c.ellipse(ax, ay - 4.0, 1.4, 1.4, [31, 35, 29, 240]);
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
        for yy in y0..y1 {
            for xx in x0..x1 {
                self.put(xx, yy, c);
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
        let y0 = (y - ry).floor() as i32;
        let y1 = (y + ry).ceil() as i32;
        for yy in y0..=y1 {
            let frac = 1.0 - ((yy as f32 - y).abs() / ry.max(0.1));
            let half = rx * frac;
            let x0 = (x - half).floor() as i32;
            let x1 = (x + half).ceil() as i32;
            for xx in x0..=x1 {
                self.put(xx, yy, c);
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
    fn lighting(&mut self, cx: f32, cy: f32, red: bool) {
        let max = ((self.w.max(self.h)) as f32 * 0.62).max(1.);
        for y in 0..self.h {
            for x in 0..self.w {
                let dx = x as f32 - cx;
                let dy = (y as f32 - cy) * 1.1;
                let d = (dx * dx + dy * dy).sqrt();
                let darkness = (d / max).clamp(0., 0.30);
                let i = (y * self.w + x) * 4;
                let warm = (1. - (d / 155.).clamp(0., 1.)) * 0.15;
                self.p[i] =
                    (self.p[i] as f32 * (1. - darkness) + self.p[i] as f32 * warm * 0.22) as u8;
                self.p[i + 1] =
                    (self.p[i + 1] as f32 * (1. - darkness) + self.p[i + 1] as f32 * warm) as u8;
                self.p[i + 2] = (self.p[i + 2] as f32 * (1. - darkness)
                    + self.p[i + 2] as f32 * warm * 0.55) as u8;
                if red {
                    self.p[i] = (self.p[i] as u16 + 12).min(255) as u8;
                }
            }
        }
    }
    fn glow(&mut self, cx: f32, cy: f32, radius: f32, color: [u8; 3], strength: f32) {
        let x0 = (cx - radius).floor().max(0.0) as i32;
        let x1 = (cx + radius).ceil().min(self.w as f32 - 1.0) as i32;
        let y0 = (cy - radius).floor().max(0.0) as i32;
        let y1 = (cy + radius).ceil().min(self.h as f32 - 1.0) as i32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let dist = (dx * dx + dy * dy).sqrt() / radius.max(1.0);
                if dist >= 1.0 {
                    continue;
                }
                let a = (strength * (1.0 - dist).powf(1.7)).clamp(0.0, 255.0) as u32;
                let i = (y as usize * self.w + x as usize) * 4;
                for k in 0..3 {
                    self.p[i + k] =
                        ((self.p[i + k] as u32 * (255 - a) + color[k] as u32 * a) / 255) as u8;
                }
            }
        }
    }
}

fn ground_color(g: Ground, x: i32, y: i32) -> [u8; 3] {
    let h = hash2(x, y) as usize;
    match g {
        Ground::Road => [58, 65, 62],
        Ground::Grass => {
            let mut c = COLORS[h % COLORS.len()];
            if h.is_multiple_of(7) {
                c = [58, 70, 51];
            }
            c
        }
        Ground::Pavement => [69 + (h % 9) as u8, 73 + (h % 8) as u8, 69 + (h % 7) as u8],
        Ground::Field => [65, 71, 48],
        Ground::Gravel => [67, 66, 55],
    }
}
fn hash2(x: i32, y: i32) -> u64 {
    let mut v = ((x as u32 as u64) << 32) | (y as u32 as u64);
    v = v.wrapping_add(0x9e3779b97f4a7c15);
    v = (v ^ (v >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94d049bb133111eb);
    v ^ (v >> 31)
}

fn draw_house(c: &mut Canvas<'_>, house: House, project: &impl Fn(f32, f32) -> (f32, f32)) {
    let House {
        x,
        y,
        width: w,
        height: h,
        door,
    } = house;
    // Low ruined masonry perimeter leaves the room and its cache visible from above.
    for yy in y..y + h {
        for xx in x..x + w {
            let wall = xx == x || xx == x + w - 1 || yy == y || yy == y + h - 1;
            if !wall {
                continue;
            }
            if xx == door.x && yy == door.y {
                continue;
            }
            let (sx, sy) = project(xx as f32, yy as f32);
            c.diamond(sx, sy - 2., TILE_W * 0.48, TILE_H * 0.44, [83, 75, 64, 255]);
            c.line(
                sx - TILE_W * 0.42,
                sy - 3.,
                sx,
                sy + TILE_H * 0.35 - 3.,
                [118, 102, 82, 255],
            );
            // Raised masonry face, brighter capstones and a top shadow sell the room's height.
            c.rect(sx, sy - 14., 25., 23., [103, 91, 73, 255]);
            c.rect(sx - 4., sy - 26., 5., 2., [155, 133, 101, 255]);
            c.rect(sx + 2., sy - 26., 6., 2., [146, 126, 95, 255]);
            c.line(sx - 11., sy - 25., sx + 11., sy - 25., [37, 38, 34, 200]);
        }
    }
    let (sx, sy) = project(door.x as f32, door.y as f32);
    c.rect(sx, sy - 8., 18., 23., [35, 27, 24, 255]);
    c.rect(sx, sy + 4., 21., 4., [181, 126, 60, 255]);
    c.rect(sx, sy - 21., 23., 3., [137, 112, 79, 255]);
}
fn draw_cache(c: &mut Canvas<'_>, x: f32, y: f32) {
    c.ellipse(x, y + 2., 9., 4., [18, 20, 18, 150]);
    c.rect(x, y - 3., 13., 9., [135, 80, 35, 255]);
    c.rect(x, y - 8., 15., 5., [194, 133, 59, 255]);
    c.rect(x - 3., y - 8., 2., 9., [82, 53, 34, 255]);
    c.rect(x + 3., y - 8., 2., 9., [82, 53, 34, 255]);
    c.rect(x, y - 9., 4., 2., [255, 225, 136, 255]);
}
fn draw_bush(c: &mut Canvas<'_>, x: f32, y: f32, variant: usize) {
    let shade = if variant.is_multiple_of(2) {
        [53, 84, 56, 255]
    } else {
        [66, 91, 58, 255]
    };
    c.ellipse(x, y + 2.0, 9.0, 3.5, [12, 19, 17, 120]);
    c.ellipse(x, y - 2.0, 8.0, 5.5, [35, 62, 42, 255]);
    c.ellipse(x - 4.0, y - 3.0, 5.0, 4.0, shade);
    c.ellipse(x + 3.0, y - 4.0, 5.5, 4.5, shade);
    c.rect(x - 2.0, y - 7.0, 3.0, 1.5, [119, 135, 78, 160]);
}
fn draw_rubble(c: &mut Canvas<'_>, x: f32, y: f32) {
    c.ellipse(x, y + 3., 9., 3., [17, 21, 21, 110]);
    c.rect(x - 3., y, 7., 5., [71, 71, 62, 255]);
    c.rect(x + 3., y - 2., 6., 5., [89, 83, 69, 255]);
    c.rect(x - 1., y - 4., 4., 3., [101, 91, 72, 255]);
}

#[derive(Clone, Copy)]
struct ActorAnimation {
    moving: bool,
    phase: f32,
    elapsed_ms: u64,
    index: usize,
}

fn draw_actor(
    c: &mut Canvas<'_>,
    point: (f32, f32),
    facing: Vec2,
    player: bool,
    animation: ActorAnimation,
) {
    let (x, y) = point;
    let ActorAnimation {
        moving,
        phase,
        elapsed_ms,
        index,
    } = animation;
    let s = if player { 1.45 } else { 1.35 };
    let bob = if moving {
        (phase * std::f32::consts::TAU).sin() * 1.6
    } else {
        (elapsed_ms as f32 * 0.003 + index as f32).sin() * 0.45
    };
    let forward_x = if facing.x.abs() > facing.y.abs() {
        facing.x.signum()
    } else {
        0.
    };
    let forward_y = if facing.y.abs() >= facing.x.abs() {
        facing.y.signum()
    } else {
        0.
    };
    let coat = if player {
        [39, 83, 77, 255]
    } else {
        [94, 104, 64, 255]
    };
    let skin = if player {
        [201, 164, 125, 255]
    } else {
        [134, 149, 105, 255]
    };
    let shoe = if player {
        [31, 35, 36, 255]
    } else {
        [53, 49, 40, 255]
    };
    let leg = if moving {
        (phase * std::f32::consts::TAU).sin() * 2.6
    } else {
        0.
    };
    c.rect(
        x + (-3. - leg) * s,
        y + (3. + bob) * s,
        4. * s,
        6. * s,
        shoe,
    );
    c.rect(x + (3. + leg) * s, y + (3. - bob) * s, 4. * s, 6. * s, shoe);
    c.rect(x, y + (-1. + bob) * s, 10. * s, 12. * s, coat);
    c.rect(x - 6. * s, y + (-1. + bob) * s, 3. * s, 7. * s, coat);
    c.rect(x + 6. * s, y + (-1. + bob) * s, 3. * s, 7. * s, coat);
    // Directional face and hair silhouettes.
    c.ellipse(
        x + forward_x * 1.5 * s,
        y + (-9. + bob + forward_y * 1.2) * s,
        5.3 * s,
        5.4 * s,
        skin,
    );
    c.rect(
        x - forward_x * 1.5 * s,
        y + (-13. + bob - forward_y) * s,
        8. * s,
        2. * s,
        if player {
            [48, 40, 34, 255]
        } else {
            [76, 77, 54, 255]
        },
    );
    if forward_x != 0. {
        c.rect(
            x + forward_x * 4. * s,
            y + (-9. + bob) * s,
            1.5 * s,
            2. * s,
            [23, 29, 27, 255],
        );
    } else {
        c.rect(
            x + (-2. + forward_x) * s,
            y + (-9. + bob + forward_y * 2.) * s,
            1.5 * s,
            1.5 * s,
            [23, 29, 27, 255],
        );
    }
    // A backpack and a short directional weapon make the survivor readable at small scale.
    if player {
        c.rect(
            x - forward_x * 4. * s,
            y + (-1. + bob) * s,
            4. * s,
            8. * s,
            [112, 68, 39, 255],
        );
        let wx = x + forward_x * 8. * s;
        let wy = y + (-4. + bob + forward_y * 5.) * s;
        c.rect(
            wx,
            wy,
            (forward_x.abs() * 7. + 2.) * s,
            2. * s,
            [47, 51, 49, 255],
        );
        c.rect(wx, wy - 1. * s, 2. * s, 2. * s, [146, 129, 94, 255]);
    } else {
        c.rect(
            x - 1. * s,
            y + (1. + bob) * s,
            2. * s,
            4. * s,
            [74, 42, 36, 255],
        );
        c.rect(
            x + forward_x * 5. * s,
            y + (-8. + bob + forward_y * 4.) * s,
            2. * s,
            2. * s,
            [190, 92, 59, 255],
        );
    }
}
