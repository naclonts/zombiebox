use std::{mem, slice};

use zombiebox_core::{ACTION_TICKS_PER_SECOND, ActionGame, ActionInput, ActionOutcome, Vec2};
use zombiebox_render::{Renderer, screen_vector_to_world};

const TICK_UNIT: u64 = 1_000_000_000;
const MAX_FRAME_NS: u64 = 50_000_000;

pub struct ZbAction {
    game: ActionGame,
    renderer: Option<Renderer>,
    last_frame: Option<u64>,
    partial_tick: u64,
    elapsed_ns: u64,
    pending_use: bool,
}

impl ZbAction {
    fn advance(&mut self, frame_time_ns: u64, movement: Vec2, aim: Option<Vec2>, use_item: bool) {
        let elapsed = self
            .last_frame
            .replace(frame_time_ns)
            .map_or(0, |previous| {
                frame_time_ns.saturating_sub(previous).min(MAX_FRAME_NS)
            });
        self.elapsed_ns += elapsed;
        self.partial_tick += elapsed * u64::from(ACTION_TICKS_PER_SECOND);
        self.pending_use |= use_item;

        while self.partial_tick >= TICK_UNIT {
            self.partial_tick -= TICK_UNIT;
            let use_item = mem::take(&mut self.pending_use);
            let interact = use_item && self.game.near_loot();
            self.game.step(ActionInput {
                movement,
                aim,
                fire: use_item && !interact,
                interact,
            });
            if let Some(renderer) = &mut self.renderer {
                renderer.observe_events(&self.game, self.elapsed_ns / 1_000_000);
            }
        }
    }

    fn status(&self) -> i64 {
        let hud = self.game.hud();
        i64::from(hud.health)
            | i64::from(hud.ammo.min(255)) << 8
            | (hud.zombie_count.min(255) as i64) << 16
            | i64::from(hud.armed) << 24
            | i64::from(self.game.near_loot()) << 25
            | i64::from(hud.outcome == ActionOutcome::Won) << 26
            | i64::from(hud.outcome == ActionOutcome::Dead) << 27
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn zb_action_v1_create(seed: u64) -> *mut ZbAction {
    Box::into_raw(Box::new(ZbAction {
        game: ActionGame::residential(seed),
        renderer: None,
        last_frame: None,
        partial_tick: 0,
        elapsed_ns: 0,
        pending_use: false,
    }))
}

/// # Safety
/// A non-null run must come from zb_action_v1_create and have no active users.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v1_destroy(run: *mut ZbAction) {
    if !run.is_null() {
        drop(unsafe { Box::from_raw(run) });
    }
}

/// # Safety
/// A non-null run must be live and exclusively accessible for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v1_reset_clock(run: *mut ZbAction) {
    if let Some(run) = unsafe { run.as_mut() } {
        run.last_frame = None;
        run.partial_tick = 0;
        run.pending_use = false;
    }
}

/// # Safety
/// A non-null run must be live and exclusively accessible. A non-null buffer must
/// point to writable storage for stride * height bytes, disjoint from the run.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v1_frame(
    run: *mut ZbAction,
    pixels: *mut u8,
    width: u32,
    height: u32,
    stride: usize,
    frame_time_ns: u64,
    move_x: f32,
    move_y: f32,
    aim_x: f32,
    aim_y: f32,
    use_item: bool,
) -> i64 {
    let (width, height) = (width as usize, height as usize);
    let Some(buffer_len) = stride.checked_mul(height) else {
        return -1;
    };
    if pixels.is_null()
        || !(1..=2048).contains(&width)
        || !(1..=2048).contains(&height)
        || stride < width * 4
        || buffer_len > isize::MAX as usize
    {
        return -1;
    }
    let Some(run) = (unsafe { run.as_mut() }) else {
        return -1;
    };
    if run
        .renderer
        .as_ref()
        .is_none_or(|renderer| renderer.width() != width || renderer.height() != height)
    {
        run.renderer = Some(Renderer::new(width, height));
    }

    let movement = stick(move_x, move_y);
    let aim = stick(aim_x, aim_y);
    run.advance(
        frame_time_ns,
        movement,
        (aim != Vec2::ZERO).then_some(aim),
        use_item,
    );

    let renderer = run.renderer.as_mut().expect("framebuffer is initialized");
    let frame = renderer.render(&run.game, run.elapsed_ns / 1_000_000);
    let row_bytes = width * 4;
    let output = unsafe { slice::from_raw_parts_mut(pixels, buffer_len) };
    if stride == row_bytes {
        output.copy_from_slice(frame);
    } else {
        for (dst, src) in output
            .chunks_exact_mut(stride)
            .zip(frame.chunks_exact(row_bytes))
        {
            dst[..row_bytes].copy_from_slice(src);
        }
    }
    run.status()
}

fn stick(x: f32, y: f32) -> Vec2 {
    if !x.is_finite() || !y.is_finite() {
        return Vec2::ZERO;
    }
    let strength = x.hypot(y).min(1.0);
    let vector = screen_vector_to_world(x, y);
    let length = vector.x.hypot(vector.y);
    if length > 0.0 {
        vector * (strength / length)
    } else {
        Vec2::ZERO
    }
}
