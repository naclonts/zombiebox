//! The platform adapters' C interface. Keep the declarations in include/zombiebox.h in sync.
mod action;

use zombiebox_core::{Command, Direction, Game, TICKS_PER_SECOND};

pub struct ZbGame(Game);

#[repr(C)]
pub struct ZbHud {
    tick: u64,
    player_x: i32,
    player_y: i32,
    supplies: u16,
    health: u8,
    armed: u8,
}

#[unsafe(no_mangle)]
pub extern "C" fn zb_v1_create(seed: u64) -> *mut ZbGame {
    Box::into_raw(Box::new(ZbGame(Game::new(seed))))
}

#[unsafe(no_mangle)]
pub extern "C" fn zb_v1_ticks_per_second() -> u32 {
    TICKS_PER_SECOND
}

/// # Safety
/// A non-null handle must be live and exclusively accessible during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_v1_step(game: *mut ZbGame, command: u32) -> bool {
    let Some(game) = (unsafe { game.as_mut() }) else {
        return false;
    };
    let command = match command {
        0 => Command::Wait,
        1 => Command::Move(Direction::North),
        2 => Command::Move(Direction::East),
        3 => Command::Move(Direction::South),
        4 => Command::Move(Direction::West),
        5 => Command::Scavenge,
        6 => Command::Attack,
        _ => return false,
    };
    game.0.step(command);
    true
}

/// # Safety
/// A non-null handle must be live and readable throughout the call. A non-null
/// output pointer must address separate, aligned, writable storage for one ZbHud.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_v1_read_hud(game: *const ZbGame, out: *mut ZbHud) -> bool {
    let Some(game) = (unsafe { game.as_ref() }) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    let hud = game.0.hud();
    unsafe {
        out.write(ZbHud {
            tick: hud.tick,
            player_x: hud.player.x,
            player_y: hud.player.y,
            supplies: hud.supplies,
            health: hud.health,
            armed: u8::from(hud.armed),
        })
    };
    true
}

/// # Safety
/// A non-null handle must come from zb_v1_create, have no active users and
/// never be used again after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_v1_destroy(game: *mut ZbGame) {
    if !game.is_null() {
        drop(unsafe { Box::from_raw(game) });
    }
}
