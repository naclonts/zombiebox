use std::{mem, slice};

use zombiebox_core::{
    ACTION_TICKS_PER_SECOND, ActionEvent, ActionGame, ActionInput, ActionOutcome, FloorTransition,
    Material, ProjectileKind, Tile3D, Vec2, Weapon,
};
use zombiebox_render::{Renderer, screen_vector_to_world};

const TICK_UNIT: u64 = 1_000_000_000;
const MAX_FRAME_NS: u64 = 50_000_000;

const ZB_WEAPON_UNCHANGED: i32 = -1;
const ZB_WEAPON_NONE: u8 = 255;

const ZB_INPUT_FIRE_PULSE: u32 = 1 << 0;
const ZB_INPUT_FIRE_HOLD: u32 = 1 << 1;
const ZB_INPUT_INTERACT: u32 = 1 << 2;
const ZB_INPUT_RELOAD: u32 = 1 << 3;
const ZB_INPUT_HEAL: u32 = 1 << 4;
const ZB_INPUT_LOOK_AHEAD: u32 = 1 << 5;

const ZB_V2_WON: u16 = 1 << 0;
const ZB_V2_DEAD: u16 = 1 << 1;
const ZB_V2_RELOADING: u16 = 1 << 2;
const ZB_V2_CAN_RELOAD: u16 = 1 << 3;
const ZB_V2_CAN_HEAL: u16 = 1 << 4;
const ZB_V2_LOW_AMMO: u16 = 1 << 5;
const ZB_V2_INDOORS: u16 = 1 << 6;
const ZB_V2_DANGER_CLOSE: u16 = 1 << 7;

const ZB_SFX_AXE_SWING: u64 = 1 << 0;
const ZB_SFX_AXE_HIT_FLESH: u64 = 1 << 1;
const ZB_SFX_AXE_HIT_WOOD: u64 = 1 << 2;
const ZB_SFX_PISTOL_FIRE: u64 = 1 << 3;
const ZB_SFX_SHOTGUN_FIRE: u64 = 1 << 4;
const ZB_SFX_ROCKET_LAUNCH: u64 = 1 << 5;
const ZB_SFX_EXPLOSION: u64 = 1 << 6;
const ZB_SFX_DRY_FIRE: u64 = 1 << 7;
const ZB_SFX_RELOAD_START: u64 = 1 << 8;
const ZB_SFX_RELOAD_FINISH: u64 = 1 << 9;
const ZB_SFX_GLASS_SHATTER: u64 = 1 << 10;
const ZB_SFX_WOOD_SPLINTER: u64 = 1 << 11;
const ZB_SFX_STONE_THUD: u64 = 1 << 12;
const ZB_SFX_STREETLIGHT_BREAK: u64 = 1 << 13;
const ZB_SFX_DOOR_TOGGLE: u64 = 1 << 14;
const ZB_SFX_WINDOW_CLIMB: u64 = 1 << 15;
const ZB_SFX_STAIRS_STEP: u64 = 1 << 16;
const ZB_SFX_PICKUP: u64 = 1 << 17;
const ZB_SFX_MEDKIT_HEAL: u64 = 1 << 18;
const ZB_SFX_WEAPON_SWITCH: u64 = 1 << 19;
const ZB_SFX_MONSTER_ALERT: u64 = 1 << 20;
const ZB_SFX_MONSTER_HIT: u64 = 1 << 21;
const ZB_SFX_MONSTER_DOWN: u64 = 1 << 22;
const ZB_SFX_BLOATER_BURST: u64 = 1 << 23;
const ZB_SFX_SHOOTER_FIRE: u64 = 1 << 24;
const ZB_SFX_PLAYER_HURT: u64 = 1 << 25;
const ZB_SFX_WIN: u64 = 1 << 26;
const ZB_SFX_DEATH: u64 = 1 << 27;
const ZB_SFX_STRUCTURAL_BREACH: u64 = 1 << 28;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ZbActionFrameInput {
    pub move_x: f32,
    pub move_y: f32,
    pub aim_x: f32,
    pub aim_y: f32,
    pub action_flags: u32,
    pub select_weapon: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ZbActionFrameOutput {
    pub audio_events: u64,
    pub active_reserve: u16,
    pub pistol_reserve: u16,
    pub shotgun_reserve: u16,
    pub rocket_reserve: u16,
    pub zombies: u16,
    pub zombies_total: u16,
    pub status_flags: u16,
    pub health: u8,
    pub max_health: u8,
    pub floor: u8,
    pub equipped_weapon: u8,
    pub owned_weapons_mask: u8,
    pub active_mag: u8,
    pub active_mag_cap: u8,
    pub pistol_mag: u8,
    pub shotgun_mag: u8,
    pub rocket_mag: u8,
    pub reload_progress: u8,
    pub medkits: u8,
    pub caches_found: u8,
    pub caches_total: u8,
    pub interact_kind: u8,
    pub objective_kind: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ZbActionLocation {
    pub x: i32,
    pub y: i32,
    pub floor: u8,
    pub mode: u8,
    pub district: u8,
    pub reserved: u8,
}

pub struct ZbAction {
    game: ActionGame,
    renderer: Option<Renderer>,
    last_frame: Option<u64>,
    partial_tick: u64,
    elapsed_ns: u64,
    pending_use: bool,
    pending_fire_pulse: bool,
    pending_interact: bool,
    pending_reload: bool,
    pending_heal: bool,
    pending_select_weapon: Option<Weapon>,
}

impl ZbAction {
    fn with_game(game: ActionGame) -> Self {
        Self {
            game,
            renderer: None,
            last_frame: None,
            partial_tick: 0,
            elapsed_ns: 0,
            pending_use: false,
            pending_fire_pulse: false,
            pending_interact: false,
            pending_reload: false,
            pending_heal: false,
            pending_select_weapon: None,
        }
    }

    fn new(seed: u64) -> Self {
        Self::with_game(ActionGame::residential(seed))
    }

    fn new_county(seed: u64) -> Self {
        Self::with_game(ActionGame::county(seed))
    }

    fn location_for(&self, tile: Tile3D) -> ZbActionLocation {
        ZbActionLocation {
            x: tile.x,
            y: tile.y,
            floor: tile.floor,
            mode: self.game.mode() as u8,
            district: self.game.district_code(),
            reserved: 0,
        }
    }

    fn reset_clock_and_latches(&mut self) {
        self.last_frame = None;
        self.partial_tick = 0;
        self.pending_use = false;
        self.pending_fire_pulse = false;
        self.pending_interact = false;
        self.pending_reload = false;
        self.pending_heal = false;
        self.pending_select_weapon = None;
    }

    fn ensure_renderer(&mut self, width: usize, height: usize) {
        if self
            .renderer
            .as_ref()
            .is_none_or(|renderer| renderer.width() != width || renderer.height() != height)
        {
            self.renderer = Some(Renderer::new(width, height));
        }
    }

    fn advance_clock(&mut self, frame_time_ns: u64) {
        let elapsed = self
            .last_frame
            .replace(frame_time_ns)
            .map_or(0, |previous| {
                frame_time_ns.saturating_sub(previous).min(MAX_FRAME_NS)
            });
        self.elapsed_ns += elapsed;
        self.partial_tick += elapsed * u64::from(ACTION_TICKS_PER_SECOND);
    }

    fn advance_v1(
        &mut self,
        frame_time_ns: u64,
        movement: Vec2,
        aim: Option<Vec2>,
        use_item: bool,
    ) {
        self.advance_clock(frame_time_ns);
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
                ..ActionInput::default()
            });
            if let Some(renderer) = &mut self.renderer {
                renderer.observe_events(&self.game, self.elapsed_ns / 1_000_000);
            }
        }
    }

    fn advance_v2(
        &mut self,
        frame_time_ns: u64,
        movement: Vec2,
        aim: Option<Vec2>,
        action_flags: u32,
        select_weapon: Option<Weapon>,
    ) -> u64 {
        self.advance_clock(frame_time_ns);

        self.pending_fire_pulse |= (action_flags & ZB_INPUT_FIRE_PULSE) != 0;
        self.pending_interact |= (action_flags & ZB_INPUT_INTERACT) != 0;
        self.pending_reload |= (action_flags & ZB_INPUT_RELOAD) != 0;
        self.pending_heal |= (action_flags & ZB_INPUT_HEAL) != 0;
        if let Some(weapon) = select_weapon {
            self.pending_select_weapon = Some(weapon);
        }

        let fire_hold = (action_flags & ZB_INPUT_FIRE_HOLD) != 0;
        let mut frame_audio = 0u64;

        while self.partial_tick >= TICK_UNIT {
            self.partial_tick -= TICK_UNIT;
            let fire = mem::take(&mut self.pending_fire_pulse) || fire_hold;
            let interact = mem::take(&mut self.pending_interact);
            let reload = mem::take(&mut self.pending_reload);
            let use_medkit = mem::take(&mut self.pending_heal);
            let select_weapon = self.pending_select_weapon.take();

            self.game.step(ActionInput {
                movement,
                aim,
                fire,
                interact,
                select_weapon,
                cycle_weapon: false,
                reload,
                use_medkit,
                quiet_walk: false,
            });
            frame_audio |= events_to_sfx_mask(self.game.events());
            if let Some(renderer) = &mut self.renderer {
                renderer.observe_events(&self.game, self.elapsed_ns / 1_000_000);
            }
        }
        frame_audio
    }

    fn status_v1(&self) -> i64 {
        let hud = self.game.hud();
        i64::from(hud.health)
            | i64::from(hud.ammo.min(255)) << 8
            | (hud.zombie_count.min(255) as i64) << 16
            | i64::from(hud.armed) << 24
            | i64::from(self.game.near_loot()) << 25
            | i64::from(hud.outcome == ActionOutcome::Won) << 26
            | i64::from(hud.outcome == ActionOutcome::Dead) << 27
    }

    fn output_v2(&self, audio_events: u64) -> ZbActionFrameOutput {
        let hud = self.game.hud();
        let low_ammo = matches!(
            hud.equipped,
            Some(Weapon::Pistol | Weapon::Shotgun | Weapon::RocketLauncher)
        ) && (hud.active_mag == 0
            || (hud.active_mag_cap >= 4 && hud.active_mag <= hud.active_mag_cap / 4));

        let mut status_flags = 0u16;
        if hud.outcome == ActionOutcome::Won {
            status_flags |= ZB_V2_WON;
        }
        if hud.outcome == ActionOutcome::Dead {
            status_flags |= ZB_V2_DEAD;
        }
        if hud.reloading {
            status_flags |= ZB_V2_RELOADING;
        }
        if hud.can_reload {
            status_flags |= ZB_V2_CAN_RELOAD;
        }
        if hud.can_heal {
            status_flags |= ZB_V2_CAN_HEAL;
        }
        if low_ammo {
            status_flags |= ZB_V2_LOW_AMMO;
        }
        if hud.indoors {
            status_flags |= ZB_V2_INDOORS;
        }
        if hud.rocket_danger_close {
            status_flags |= ZB_V2_DANGER_CLOSE;
        }

        ZbActionFrameOutput {
            audio_events,
            active_reserve: hud.active_reserve,
            pistol_reserve: hud.pistol_reserve,
            shotgun_reserve: hud.shotgun_reserve,
            rocket_reserve: hud.rocket_reserve,
            zombies: hud.zombie_count.min(u16::MAX as usize) as u16,
            zombies_total: hud.zombies_total.min(u16::MAX as usize) as u16,
            status_flags,
            health: hud.health,
            max_health: hud.max_health,
            floor: hud.floor,
            equipped_weapon: hud.equipped.map_or(ZB_WEAPON_NONE, |w| w as u8),
            owned_weapons_mask: hud.owned_weapons_mask,
            active_mag: hud.active_mag,
            active_mag_cap: hud.active_mag_cap,
            pistol_mag: hud.pistol_mag,
            shotgun_mag: hud.shotgun_mag,
            rocket_mag: hud.rocket_mag,
            reload_progress: hud.reload_progress,
            medkits: hud.medkits,
            caches_found: hud.caches_found,
            caches_total: hud.caches_total,
            interact_kind: hud.interact_prompt as u8,
            objective_kind: hud.objective_kind as u8,
        }
    }

    fn blit_to(&mut self, output: &mut [u8], width: usize, stride: usize) {
        let renderer = self.renderer.as_mut().expect("framebuffer is initialized");
        let frame = renderer.render(&self.game, self.elapsed_ns / 1_000_000);
        let row_bytes = width * 4;
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
    }
}

fn validate_framebuffer(
    pixels: *mut u8,
    width: u32,
    height: u32,
    stride: usize,
) -> Option<(usize, usize, usize)> {
    let (width, height) = (width as usize, height as usize);
    let buffer_len = stride.checked_mul(height)?;
    if pixels.is_null()
        || !(1..=2048).contains(&width)
        || !(1..=2048).contains(&height)
        || stride < width * 4
        || buffer_len > isize::MAX as usize
    {
        return None;
    }
    Some((width, height, buffer_len))
}

fn parse_select_weapon(value: i32) -> Result<Option<Weapon>, ()> {
    if value == ZB_WEAPON_UNCHANGED {
        return Ok(None);
    }
    let slot = u8::try_from(value).map_err(|_| ())?;
    Weapon::from_slot(slot).map(Some).ok_or(())
}

fn events_to_sfx_mask(events: &[ActionEvent]) -> u64 {
    let mut mask = 0u64;
    for event in events {
        match *event {
            ActionEvent::UnarmedShove { .. } => mask |= ZB_SFX_AXE_SWING,
            ActionEvent::AxeSwing {
                hit_flesh,
                hit_solid,
                ..
            } => {
                mask |= ZB_SFX_AXE_SWING;
                if hit_flesh {
                    mask |= ZB_SFX_AXE_HIT_FLESH;
                }
                if hit_solid {
                    mask |= ZB_SFX_AXE_HIT_WOOD;
                }
            }
            ActionEvent::ShotFired { weapon, .. } => {
                mask |= match weapon {
                    Weapon::Axe => ZB_SFX_AXE_SWING,
                    Weapon::Pistol => ZB_SFX_PISTOL_FIRE,
                    Weapon::Shotgun => ZB_SFX_SHOTGUN_FIRE,
                    Weapon::RocketLauncher => ZB_SFX_ROCKET_LAUNCH,
                };
            }
            ActionEvent::BulletTrace { .. } => {}
            ActionEvent::ProjectileLaunched { kind, .. } => {
                mask |= match kind {
                    ProjectileKind::Rocket => ZB_SFX_ROCKET_LAUNCH,
                    ProjectileKind::ShooterBolt => ZB_SFX_SHOOTER_FIRE,
                };
            }
            ActionEvent::RocketBlast { .. } => mask |= ZB_SFX_EXPLOSION,
            ActionEvent::DryFire { .. } => mask |= ZB_SFX_DRY_FIRE,
            ActionEvent::MaterialHit {
                material,
                destroyed,
                light_changed,
                ..
            } => match material {
                Material::Glass => mask |= ZB_SFX_GLASS_SHATTER,
                Material::WoodLight | Material::WoodDoor | Material::TreeWood => {
                    mask |= ZB_SFX_WOOD_SPLINTER;
                }
                Material::Drywall | Material::WoodSiding | Material::BrickMasonry => {
                    mask |= ZB_SFX_STONE_THUD;
                    if destroyed {
                        mask |= ZB_SFX_STRUCTURAL_BREACH;
                    }
                }
                Material::MetalPole => {
                    if destroyed || light_changed {
                        mask |= ZB_SFX_STREETLIGHT_BREAK;
                    } else {
                        mask |= ZB_SFX_STONE_THUD;
                    }
                }
                Material::Flesh => mask |= ZB_SFX_MONSTER_HIT,
                Material::None | Material::Caustic => {}
            },
            ActionEvent::StructuralCollapse { .. } => mask |= ZB_SFX_STRUCTURAL_BREACH,
            ActionEvent::DoorToggled { .. } => mask |= ZB_SFX_DOOR_TOGGLE,
            ActionEvent::WindowClimbed { .. } => mask |= ZB_SFX_WINDOW_CLIMB,
            ActionEvent::WeaponSwitched { .. } => mask |= ZB_SFX_WEAPON_SWITCH,
            ActionEvent::ReloadStarted { .. } => mask |= ZB_SFX_RELOAD_START,
            ActionEvent::ReloadCompleted { .. } => mask |= ZB_SFX_RELOAD_FINISH,
            ActionEvent::Looted { .. } => mask |= ZB_SFX_PICKUP,
            ActionEvent::Healed { .. } => mask |= ZB_SFX_MEDKIT_HEAL,
            ActionEvent::MonsterAlerted { .. } => mask |= ZB_SFX_MONSTER_ALERT,
            ActionEvent::MonsterTelegraph { .. } => {}
            ActionEvent::MonsterHit { .. } => mask |= ZB_SFX_MONSTER_HIT,
            ActionEvent::MonsterDown { .. } => mask |= ZB_SFX_MONSTER_DOWN,
            ActionEvent::BloaterSwell { .. } => {}
            ActionEvent::BloaterBurst { .. } => mask |= ZB_SFX_BLOATER_BURST,
            ActionEvent::PlayerHurt { .. } => mask |= ZB_SFX_PLAYER_HURT,
            ActionEvent::FloorChanged { via, .. } => {
                mask |= match via {
                    FloorTransition::Stairs => ZB_SFX_STAIRS_STEP,
                    FloorTransition::WindowDrop => ZB_SFX_WINDOW_CLIMB,
                    FloorTransition::Collapse => ZB_SFX_STRUCTURAL_BREACH,
                };
            }
            ActionEvent::Died => mask |= ZB_SFX_DEATH,
            ActionEvent::Won => mask |= ZB_SFX_WIN,
        }
    }
    mask
}

#[unsafe(no_mangle)]
pub extern "C" fn zb_action_v1_create(seed: u64) -> *mut ZbAction {
    Box::into_raw(Box::new(ZbAction::new(seed)))
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
        run.reset_clock_and_latches();
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
    let Some((width, height, buffer_len)) = validate_framebuffer(pixels, width, height, stride)
    else {
        return -1;
    };
    let Some(run) = (unsafe { run.as_mut() }) else {
        return -1;
    };
    run.ensure_renderer(width, height);

    let movement = stick(move_x, move_y);
    let aim = stick(aim_x, aim_y);
    run.advance_v1(
        frame_time_ns,
        movement,
        (aim != Vec2::ZERO).then_some(aim),
        use_item,
    );

    let output = unsafe { slice::from_raw_parts_mut(pixels, buffer_len) };
    run.blit_to(output, width, stride);
    run.status_v1()
}

#[unsafe(no_mangle)]
pub extern "C" fn zb_action_v2_create(seed: u64) -> *mut ZbAction {
    Box::into_raw(Box::new(ZbAction::new(seed)))
}

#[unsafe(no_mangle)]
pub extern "C" fn zb_action_v2_create_county(seed: u64) -> *mut ZbAction {
    Box::into_raw(Box::new(ZbAction::new_county(seed)))
}

/// # Safety
/// A non-null run must come from zb_action_v2_create and have no active users.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v2_destroy(run: *mut ZbAction) {
    if !run.is_null() {
        drop(unsafe { Box::from_raw(run) });
    }
}

/// # Safety
/// A non-null run must be live and exclusively accessible for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v2_reset_clock(run: *mut ZbAction) {
    if let Some(run) = unsafe { run.as_mut() } {
        run.reset_clock_and_latches();
    }
}

/// # Safety
/// Non-null pointers must be valid, properly aligned, and disjoint for this call.
/// `pixels` must point to writable storage for `stride * height` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v2_frame(
    run: *mut ZbAction,
    pixels: *mut u8,
    width: u32,
    height: u32,
    stride: usize,
    frame_time_ns: u64,
    input: *const ZbActionFrameInput,
    out: *mut ZbActionFrameOutput,
) -> bool {
    let Some((width, height, buffer_len)) = validate_framebuffer(pixels, width, height, stride)
    else {
        return false;
    };
    let Some(run) = (unsafe { run.as_mut() }) else {
        return false;
    };
    let Some(input) = (unsafe { input.as_ref() }) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    let Ok(select_weapon) = parse_select_weapon(input.select_weapon) else {
        return false;
    };

    run.ensure_renderer(width, height);
    if let Some(renderer) = &mut run.renderer {
        renderer.set_look_ahead((input.action_flags & ZB_INPUT_LOOK_AHEAD) != 0);
    }

    let movement = stick(input.move_x, input.move_y);
    let aim = stick(input.aim_x, input.aim_y);
    let audio_events = run.advance_v2(
        frame_time_ns,
        movement,
        (aim != Vec2::ZERO).then_some(aim),
        input.action_flags,
        select_weapon,
    );

    let output = unsafe { slice::from_raw_parts_mut(pixels, buffer_len) };
    run.blit_to(output, width, stride);
    unsafe { out.write(run.output_v2(audio_events)) };
    true
}

/// # Safety
/// A non-null run must be live and readable throughout the call. A non-null
/// output pointer must address aligned, writable storage for one `ZbActionLocation`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v2_location(
    run: *const ZbAction,
    out: *mut ZbActionLocation,
) -> bool {
    let Some(run) = (unsafe { run.as_ref() }) else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    let loc = run.location_for(run.game.player_tile());
    unsafe { out.write(loc) };
    true
}

/// # Safety
/// A non-null run must be live and exclusively accessible for this call.
/// If `out_resolved` is non-null, it must address aligned, writable storage
/// for one `ZbActionLocation`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zb_action_v2_teleport(
    run: *mut ZbAction,
    x: i32,
    y: i32,
    floor: u8,
    out_resolved: *mut ZbActionLocation,
) -> bool {
    let Some(run) = (unsafe { run.as_mut() }) else {
        return false;
    };
    let Some(dest) = run.game.teleport(Tile3D::new(x, y, floor)) else {
        return false;
    };
    run.reset_clock_and_latches();
    if let Some(renderer) = &mut run.renderer {
        renderer.notify_teleport();
    }
    if !out_resolved.is_null() {
        unsafe { out_resolved.write(run.location_for(dest)) };
    }
    true
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

#[cfg(test)]
mod tests {
    use super::*;
    use zombiebox_core::{CacheLoot, MonsterId, MonsterKind, ProjectileId};

    #[test]
    fn parse_select_weapon_validates_bounds() {
        assert_eq!(parse_select_weapon(ZB_WEAPON_UNCHANGED), Ok(None));
        assert_eq!(parse_select_weapon(0), Ok(Some(Weapon::Axe)));
        assert_eq!(parse_select_weapon(1), Ok(Some(Weapon::Pistol)));
        assert_eq!(parse_select_weapon(2), Ok(Some(Weapon::Shotgun)));
        assert_eq!(parse_select_weapon(3), Ok(Some(Weapon::RocketLauncher)));
        assert!(parse_select_weapon(-2).is_err());
        assert!(parse_select_weapon(4).is_err());
    }

    #[test]
    fn events_to_sfx_mask_maps_all_categories() {
        let events = [
            ActionEvent::AxeSwing {
                from: Vec2::ZERO,
                floor: 0,
                facing: Vec2::new(1.0, 0.0),
                hit_flesh: true,
                hit_solid: true,
            },
            ActionEvent::ShotFired {
                weapon: Weapon::Shotgun,
                from: Vec2::ZERO,
                floor: 0,
                facing: Vec2::new(1.0, 0.0),
            },
            ActionEvent::ProjectileLaunched {
                id: ProjectileId(1),
                kind: ProjectileKind::ShooterBolt,
                from: Vec2::ZERO,
                floor: 0,
                dir: Vec2::new(-1.0, 0.0),
            },
            ActionEvent::RocketBlast {
                at: Vec2::ZERO,
                floor: 0,
                radius: 2.4,
            },
            ActionEvent::MaterialHit {
                at: Tile3D::default(),
                hit_pos: Vec2::ZERO,
                material: Material::Glass,
                destroyed: true,
                light_changed: false,
            },
            ActionEvent::MaterialHit {
                at: Tile3D::default(),
                hit_pos: Vec2::ZERO,
                material: Material::MetalPole,
                destroyed: true,
                light_changed: true,
            },
            ActionEvent::MaterialHit {
                at: Tile3D::default(),
                hit_pos: Vec2::ZERO,
                material: Material::BrickMasonry,
                destroyed: true,
                light_changed: false,
            },
            ActionEvent::Looted {
                at: Tile3D::default(),
                loot: CacheLoot::default(),
            },
            ActionEvent::Healed {
                at: Vec2::ZERO,
                floor: 0,
                amount: 3,
                new_health: 6,
            },
            ActionEvent::MonsterDown {
                id: MonsterId(1),
                kind: MonsterKind::Shambler,
                at: Vec2::ZERO,
                floor: 0,
                overkill: false,
            },
            ActionEvent::FloorChanged {
                from_floor: 0,
                to_floor: 1,
                at: Tile3D::default(),
                via: FloorTransition::Stairs,
            },
            ActionEvent::Won,
        ];
        let mask = events_to_sfx_mask(&events);
        assert_ne!(mask & ZB_SFX_AXE_SWING, 0);
        assert_ne!(mask & ZB_SFX_AXE_HIT_FLESH, 0);
        assert_ne!(mask & ZB_SFX_AXE_HIT_WOOD, 0);
        assert_ne!(mask & ZB_SFX_SHOTGUN_FIRE, 0);
        assert_ne!(mask & ZB_SFX_SHOOTER_FIRE, 0);
        assert_ne!(mask & ZB_SFX_EXPLOSION, 0);
        assert_ne!(mask & ZB_SFX_GLASS_SHATTER, 0);
        assert_ne!(mask & ZB_SFX_STREETLIGHT_BREAK, 0);
        assert_ne!(mask & ZB_SFX_STONE_THUD, 0);
        assert_ne!(mask & ZB_SFX_STRUCTURAL_BREACH, 0);
        assert_ne!(mask & ZB_SFX_PICKUP, 0);
        assert_ne!(mask & ZB_SFX_MEDKIT_HEAL, 0);
        assert_ne!(mask & ZB_SFX_MONSTER_DOWN, 0);
        assert_ne!(mask & ZB_SFX_STAIRS_STEP, 0);
        assert_ne!(mask & ZB_SFX_WIN, 0);
    }
}
