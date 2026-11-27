use std::{
    collections::{HashMap, VecDeque},
    fmt::Write as FmtWrite,
    io::{Read, Write},
    mem,
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};
use zombiebox_core::{
    ACTION_TICKS_PER_SECOND, ActionEvent, ActionGame, ActionInput, ActionMode, ActionOutcome,
    Command, Direction, Event, FloorTransition, Game, Ground, Material, MotorcycleInput, Position,
    ProjectileKind, RideInput, Tile3D, Vec2, WORLD_SIZE, Weapon, county_survey_json,
};
use zombiebox_render3d::{CameraFrame, CameraMode, GpuContext, Presentation};

const MAX_REQUEST: usize = 8192;
const MAX_SESSIONS: usize = 256;
const MAX_ACTION_SESSIONS: usize = 24;
const MAX_CONNECTIONS: usize = 64;
const VIEW: i32 = 23;

const TICK_UNIT: u64 = 1_000_000_000;
const MAX_FRAME_NS: u64 = 50_000_000;
const ACTION_W_MIN: u16 = 160;
const ACTION_W_MAX: u16 = 640;
const ACTION_H_MIN: u16 = 120;
const ACTION_H_MAX: u16 = 400;

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
const ZB_SFX_VEHICLE_CRASH: u64 = 1 << 29;
const ZB_SFX_VEHICLE_IMPACT: u64 = 1 << 30;
const ZB_SFX_VEHICLE_SKID: u64 = 1 << 31;

const INDEX: &str = include_str!("../../../apps/web/index.html");
const ACTION_JS: &str = include_str!("../../../apps/web/action.js");
const LEGACY_HTML: &str = include_str!("../../../apps/web/legacy.html");
const JS: &str = include_str!("../../../apps/web/game.js");
const CSS: &str = include_str!("../../../apps/web/style.css");
const SCENE: &str = include_str!("../../../apps/web/scene.js");

struct Session {
    seed: u64,
    game: Game,
}

struct ActionSession {
    game: ActionGame,
    renderer: Option<Presentation>,
    last_frame: Option<u64>,
    partial_tick: u64,
    last_dt: f32,
    pending_fire_pulse: bool,
    pending_interact: bool,
    pending_reload: bool,
    pending_heal: bool,
    pending_select_weapon: Option<Weapon>,
    frame_version: u8,
}

impl ActionSession {
    fn new(game: ActionGame) -> Self {
        Self {
            game,
            renderer: None,
            last_frame: None,
            partial_tick: 0,
            last_dt: 0.0,
            pending_fire_pulse: false,
            pending_interact: false,
            pending_reload: false,
            pending_heal: false,
            pending_select_weapon: None,
            frame_version: 2,
        }
    }

    fn reset_clock_and_latches(&mut self) {
        self.last_frame = None;
        self.partial_tick = 0;
        self.last_dt = 0.0;
        self.pending_fire_pulse = false;
        self.pending_interact = false;
        self.pending_reload = false;
        self.pending_heal = false;
        self.pending_select_weapon = None;
    }

    fn ensure_renderer(&mut self, ctx: &GpuContext, width: u32, height: u32) {
        if self.renderer.is_none() {
            self.renderer = Some(Presentation::new(ctx, width, height));
        } else if let Some(renderer) = &mut self.renderer {
            renderer.camera_mut().set_display_surface(width, height);
        }
    }

    fn advance_clock(&mut self, frame_time_ns: u64) {
        let elapsed = self
            .last_frame
            .replace(frame_time_ns)
            .map_or(0, |previous| {
                frame_time_ns.saturating_sub(previous).min(MAX_FRAME_NS)
            });
        self.last_dt = elapsed as f32 / 1_000_000_000.0;
        self.partial_tick += elapsed * u64::from(ACTION_TICKS_PER_SECOND);
    }

    fn step_frame(
        &mut self,
        frame_time_ns: u64,
        mut movement: Vec2,
        mut aim: Option<Vec2>,
        action_flags: u32,
        select_weapon: Option<Weapon>,
        mut ride: Option<MotorcycleInput>,
        control_kind: u8,
    ) -> u64 {
        self.advance_clock(frame_time_ns);

        self.pending_fire_pulse |= (action_flags & ZB_INPUT_FIRE_PULSE) != 0;
        let request_mode_matches = self.frame_version != 3
            || control_kind
                == u8::from(self.game.control_mode() == zombiebox_core::ControlMode::Motorcycle);
        self.pending_interact |= (action_flags & ZB_INPUT_INTERACT) != 0 && request_mode_matches;
        self.pending_reload |= (action_flags & ZB_INPUT_RELOAD) != 0;
        self.pending_heal |= (action_flags & ZB_INPUT_HEAL) != 0;
        if let Some(weapon) = select_weapon {
            self.pending_select_weapon = Some(weapon);
        }

        let mut fire_hold = (action_flags & ZB_INPUT_FIRE_HOLD) != 0;
        let mut frame_audio = 0u64;

        while self.partial_tick >= TICK_UNIT {
            self.partial_tick -= TICK_UNIT;
            let fire = mem::take(&mut self.pending_fire_pulse) || fire_hold;
            let interact = mem::take(&mut self.pending_interact);
            let reload = mem::take(&mut self.pending_reload);
            let use_medkit = mem::take(&mut self.pending_heal);
            let select_weapon = self.pending_select_weapon.take();

            let mode_before = self.game.control_mode();
            let recovery_before = self.game.is_recovering_from_vehicle_crash();
            let input = ActionInput {
                movement,
                aim,
                fire,
                interact,
                select_weapon,
                cycle_weapon: false,
                reload,
                use_medkit,
                quiet_walk: false,
                ride: ride.map(RideInput::Motorcycle),
            };
            if self.frame_version == 3 {
                self.game.step(input);
            } else {
                self.game.step_without_vehicle_interactions(input);
            }
            let mode_after = self.game.control_mode();
            let recovery_after = self.game.is_recovering_from_vehicle_crash();
            if mode_before != mode_after || recovery_before != recovery_after {
                self.pending_fire_pulse = false;
                self.pending_interact = false;
                self.pending_reload = false;
                self.pending_heal = false;
                self.pending_select_weapon = None;
                movement = Vec2::ZERO;
                aim = None;
                fire_hold = false;
                ride = None;
            }
            frame_audio |= events_to_sfx_mask(self.game.events());
            if let Some(renderer) = &mut self.renderer {
                renderer.observe_tick(&self.game);
            }
        }
        frame_audio
    }
}

#[derive(Default)]
struct HostState {
    sessions: HashMap<u64, Session>,
    order: VecDeque<u64>,
    action_sessions: HashMap<u64, ActionSession>,
    action_order: VecDeque<u64>,
    cached_county: Option<(u64, zombiebox_core::World)>,
    gpu_context: Option<Arc<GpuContext>>,
    gpu_attempted: bool,
    gpu_error: Option<String>,
    next_id: u64,
}

impl HostState {
    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id.wrapping_add(1).max(1);
        id
    }

    fn county_world(&mut self, seed: u64) -> zombiebox_core::World {
        if let Some((cached_seed, world)) = &self.cached_county
            && *cached_seed == seed
        {
            return world.clone();
        }
        let world = zombiebox_core::World::new(seed);
        self.cached_county = Some((seed, world.clone()));
        world
    }
}

struct ConnectionGuard(Arc<AtomicUsize>);

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct ParsedRequest {
    method: String,
    path: String,
    body: Vec<u8>,
    cross_site: bool,
    keep_alive: bool,
}

enum ReadError {
    ClosedOrTimeout,
    HttpStatus(u16),
}

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut bind: SocketAddr = "127.0.0.1:8765".parse().unwrap();
    while let Some(arg) = args.next() {
        if arg == "--bind" {
            bind = args
                .next()
                .unwrap_or_default()
                .parse()
                .expect("--bind requires a numeric SocketAddr");
        } else {
            eprintln!("usage: zombiebox-web [--bind IP:PORT]");
            std::process::exit(2);
        }
    }
    let listener = TcpListener::bind(bind)?;
    eprintln!("zombiebox web host listening on http://{bind}");
    let state = Arc::new(Mutex::new(HostState {
        next_id: 1,
        ..HostState::default()
    }));
    let active_connections = Arc::new(AtomicUsize::new(0));
    for incoming in listener.incoming() {
        match incoming {
            Ok(mut stream) => {
                let mut active = active_connections.load(Ordering::Acquire);
                let acquired = loop {
                    if active >= MAX_CONNECTIONS {
                        break false;
                    }
                    match active_connections.compare_exchange_weak(
                        active,
                        active + 1,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => break true,
                        Err(actual) => active = actual,
                    }
                };
                if !acquired {
                    let _ = stream.write_all(&response_conn(
                        503,
                        "text/plain; charset=utf-8",
                        b"too many connections\n",
                        false,
                    ));
                    continue;
                }
                let guard = ConnectionGuard(Arc::clone(&active_connections));
                let state = Arc::clone(&state);
                let _ = thread::Builder::new()
                    .name("zb-http".into())
                    .spawn(move || {
                        let _guard = guard;
                        handle(stream, &state);
                    });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, state: &Mutex<HostState>) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let mut pending = Vec::new();
    loop {
        match read_request(&mut stream, &mut pending) {
            Ok(req) => {
                let keep_alive = req.keep_alive;
                let resp = if req.method == "POST" && req.cross_site {
                    response_conn(
                        403,
                        "text/plain",
                        b"cross-site request denied\n",
                        keep_alive,
                    )
                } else if req.method == "GET" {
                    static_route_conn(&req.path, keep_alive)
                } else {
                    let mut state = state.lock().expect("host state poisoned");
                    route_conn(&req.method, &req.path, &req.body, &mut state, keep_alive)
                };
                if stream.write_all(&resp).is_err() || !keep_alive {
                    break;
                }
            }
            Err(ReadError::ClosedOrTimeout) => break,
            Err(ReadError::HttpStatus(status)) => {
                let resp =
                    response_conn(status, "text/plain; charset=utf-8", b"bad request\n", false);
                let _ = stream.write_all(&resp);
                break;
            }
        }
    }
}

fn read_request(stream: &mut TcpStream, pending: &mut Vec<u8>) -> Result<ParsedRequest, ReadError> {
    let mut buf = [0u8; 2048];
    let split = loop {
        if let Some(i) = pending.windows(4).position(|w| w == b"\r\n\r\n") {
            let end = i + 4;
            if end > MAX_REQUEST {
                return Err(ReadError::HttpStatus(413));
            }
            break end;
        }
        if pending.len() >= MAX_REQUEST {
            return Err(ReadError::HttpStatus(413));
        }
        match stream.read(&mut buf) {
            Ok(0) => {
                return Err(if pending.is_empty() {
                    ReadError::ClosedOrTimeout
                } else {
                    ReadError::HttpStatus(400)
                });
            }
            Ok(n) => pending.extend_from_slice(&buf[..n]),
            Err(_) => {
                return Err(if pending.is_empty() {
                    ReadError::ClosedOrTimeout
                } else {
                    ReadError::HttpStatus(400)
                });
            }
        }
    };

    let head = std::str::from_utf8(&pending[..split - 4])
        .map_err(|_| ReadError::HttpStatus(400))?
        .to_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines
        .next()
        .ok_or(ReadError::HttpStatus(400))?
        .split_whitespace();
    let method = first.next().ok_or(ReadError::HttpStatus(400))?.to_string();
    let path = first.next().ok_or(ReadError::HttpStatus(400))?.to_string();
    let version = first.next();
    if !matches!(version, Some("HTTP/1.1" | "HTTP/1.0")) || first.next().is_some() {
        return Err(ReadError::HttpStatus(400));
    }

    let mut len = 0usize;
    let mut saw_len = false;
    let mut cross_site = false;
    let mut conn_close = false;
    let mut conn_keep_alive = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                if saw_len {
                    return Err(ReadError::HttpStatus(400));
                }
                saw_len = true;
                len = value
                    .trim()
                    .parse()
                    .map_err(|_| ReadError::HttpStatus(400))?;
            }
            if name.eq_ignore_ascii_case("sec-fetch-site")
                && value.trim().eq_ignore_ascii_case("cross-site")
            {
                cross_site = true;
            }
            if name.eq_ignore_ascii_case("transfer-encoding") {
                return Err(ReadError::HttpStatus(400));
            }
            if name.eq_ignore_ascii_case("connection") {
                for token in value.split(',').map(str::trim) {
                    if token.eq_ignore_ascii_case("close") {
                        conn_close = true;
                    } else if token.eq_ignore_ascii_case("keep-alive") {
                        conn_keep_alive = true;
                    }
                }
            }
        }
    }

    if len > MAX_REQUEST || split + len > MAX_REQUEST {
        return Err(ReadError::HttpStatus(413));
    }
    while pending.len() < split + len {
        let n = stream
            .read(&mut buf)
            .map_err(|_| ReadError::HttpStatus(400))?;
        if n == 0 {
            return Err(ReadError::HttpStatus(400));
        }
        pending.extend_from_slice(&buf[..n]);
        if pending.len() > MAX_REQUEST + buf.len() {
            return Err(ReadError::HttpStatus(413));
        }
    }

    let body = pending[split..split + len].to_vec();
    pending.drain(..split + len);
    let keep_alive = if conn_close {
        false
    } else if version == Some("HTTP/1.1") {
        true
    } else {
        conn_keep_alive
    };

    Ok(ParsedRequest {
        method,
        path,
        body,
        cross_site,
        keep_alive,
    })
}

#[cfg(test)]
fn static_route(path: &str) -> Vec<u8> {
    static_route_conn(path, true)
}

fn static_route_conn(path: &str, keep_alive: bool) -> Vec<u8> {
    let path = path.split('?').next().unwrap_or(path);
    if let Some(value) = path.strip_prefix("/api/county/") {
        if let Some(seed) = parse_u64_decimal(value) {
            return response_conn(
                200,
                "application/json",
                county_survey_json(seed).as_bytes(),
                keep_alive,
            );
        }
        return response_conn(400, "text/plain", b"invalid seed\n", keep_alive);
    }
    let (mime, data) = match path {
        "/" | "/index.html" => ("text/html; charset=utf-8", INDEX),
        "/action.js" => ("text/javascript; charset=utf-8", ACTION_JS),
        "/legacy" | "/legacy.html" => ("text/html; charset=utf-8", LEGACY_HTML),
        "/game.js" => ("text/javascript", JS),
        "/scene.js" => ("text/javascript", SCENE),
        "/style.css" => ("text/css", CSS),
        "/county" | "/county.html" => ("text/html", include_str!("../../../apps/web/county.html")),
        "/county-map.js" => (
            "text/javascript",
            include_str!("../../../apps/web/county-map.js"),
        ),
        "/county.js" => (
            "text/javascript",
            include_str!("../../../apps/web/county.js"),
        ),
        "/county.css" => ("text/css", include_str!("../../../apps/web/county.css")),
        _ => return response_conn(404, "text/plain", b"not found\n", keep_alive),
    };
    response_conn(200, mime, data.as_bytes(), keep_alive)
}

#[cfg(test)]
fn route(method: &str, path: &str, body: &[u8], state: &mut HostState) -> Vec<u8> {
    route_conn(method, path, body, state, true)
}

fn route_conn(
    method: &str,
    path: &str,
    body: &[u8],
    state: &mut HostState,
    keep_alive: bool,
) -> Vec<u8> {
    if method != "POST" {
        return response_conn(405, "text/plain", b"method not allowed\n", keep_alive);
    }
    let path = path.split('?').next().unwrap_or(path);
    if path == "/api/runs" {
        return handle_legacy_create(body, state, keep_alive);
    }
    if let Some(id_str) = path
        .strip_prefix("/api/runs/")
        .and_then(|s| s.strip_suffix("/step"))
    {
        return handle_legacy_step(id_str, body, state, keep_alive);
    }
    if path == "/api/action/runs" {
        return handle_action_create(body, state, keep_alive);
    }
    if let Some(rest) = path.strip_prefix("/api/action/")
        && let Some((id_str, action)) = rest.split_once('/')
    {
        let Some(id) = parse_u64_decimal(id_str) else {
            return response_conn(404, "text/plain", b"not found\n", keep_alive);
        };
        return match action {
            "teleport" => handle_action_teleport(id, body, state, keep_alive),
            "reset-clock" => handle_action_reset_clock(id, state, keep_alive),
            "frame" => handle_action_frame(id, body, state, keep_alive),
            _ => response_conn(404, "text/plain", b"not found\n", keep_alive),
        };
    }
    response_conn(404, "text/plain", b"not found\n", keep_alive)
}

fn handle_legacy_create(body: &[u8], state: &mut HostState, keep_alive: bool) -> Vec<u8> {
    let Ok(seed_text) = std::str::from_utf8(body) else {
        return response_conn(400, "text/plain", b"invalid seed\n", keep_alive);
    };
    let Some(seed) = parse_u64_decimal(seed_text) else {
        return response_conn(400, "text/plain", b"invalid seed\n", keep_alive);
    };
    while state.sessions.len() >= MAX_SESSIONS {
        if let Some(oldest) = state.order.pop_front() {
            state.sessions.remove(&oldest);
        } else {
            break;
        }
    }
    let id = state.alloc_id();
    let game = Game::residential(seed);
    state.sessions.insert(id, Session { seed, game });
    state.order.push_back(id);
    match state_json(id, &state.sessions[&id], &[]) {
        Ok(s) => response_conn(201, "application/json", s.as_bytes(), keep_alive),
        Err(_) => response_conn(500, "text/plain", b"serialization error\n", keep_alive),
    }
}

fn handle_legacy_step(
    id_str: &str,
    body: &[u8],
    state: &mut HostState,
    keep_alive: bool,
) -> Vec<u8> {
    let Ok(id) = id_str.parse::<u64>() else {
        return response_conn(404, "text/plain", b"not found\n", keep_alive);
    };
    let Some(session) = state.sessions.get_mut(&id) else {
        return response_conn(404, "text/plain", b"unknown run\n", keep_alive);
    };
    let Ok(text) = std::str::from_utf8(body) else {
        return response_conn(400, "text/plain", b"invalid batch\n", keep_alive);
    };
    let commands: Option<Vec<_>> = text.bytes().map(command).collect();
    let Some(commands) = commands.filter(|c| !c.is_empty() && c.len() <= 3) else {
        return response_conn(
            400,
            "text/plain",
            b"batch must contain 1 to 3 commands\n",
            keep_alive,
        );
    };
    let mut all_events = Vec::new();
    for cmd in commands {
        session.game.step(cmd);
        let tick = session.game.hud().tick;
        all_events.extend(
            session
                .game
                .events()
                .iter()
                .copied()
                .map(|event| (tick, event)),
        );
    }
    match state_json(id, session, &all_events) {
        Ok(s) => response_conn(200, "application/json", s.as_bytes(), keep_alive),
        Err(_) => response_conn(500, "text/plain", b"serialization error\n", keep_alive),
    }
}

fn handle_action_create(body: &[u8], state: &mut HostState, keep_alive: bool) -> Vec<u8> {
    let Ok(fields) = parse_form_fields(body) else {
        return response_conn(400, "text/plain", b"invalid form body\n", keep_alive);
    };
    let mut mode = "county";
    let mut seed: Option<u64> = None;
    let mut x: Option<i32> = None;
    let mut y: Option<i32> = None;
    let mut floor: u8 = 0;

    for (k, v) in fields {
        match k {
            "mode" => {
                if !matches!(v, "county" | "birch") {
                    return response_conn(400, "text/plain", b"invalid mode\n", keep_alive);
                }
                mode = v;
            }
            "seed" => {
                let Some(parsed) = parse_u64_decimal(v) else {
                    return response_conn(400, "text/plain", b"invalid seed\n", keep_alive);
                };
                seed = Some(parsed);
            }
            "x" => {
                let Ok(parsed) = v.parse::<i32>() else {
                    return response_conn(400, "text/plain", b"invalid x\n", keep_alive);
                };
                x = Some(parsed);
            }
            "y" => {
                let Ok(parsed) = v.parse::<i32>() else {
                    return response_conn(400, "text/plain", b"invalid y\n", keep_alive);
                };
                y = Some(parsed);
            }
            "floor" => {
                let Ok(parsed) = v.parse::<u8>() else {
                    return response_conn(400, "text/plain", b"invalid floor\n", keep_alive);
                };
                floor = parsed;
            }
            _ => {}
        }
    }

    let Some(seed) = seed else {
        return response_conn(400, "text/plain", b"missing seed\n", keep_alive);
    };
    if x.is_some() != y.is_some() {
        return response_conn(400, "text/plain", b"incomplete coordinates\n", keep_alive);
    }

    let mut game = if mode == "birch" {
        ActionGame::residential(seed)
    } else {
        let world = state.county_world(seed);
        ActionGame::county_with_world(world)
    };

    let mut snapped = false;
    if mode != "birch"
        && let (Some(tx), Some(ty)) = (x, y)
    {
        let target = Tile3D::new(tx, ty, floor);
        let Some(resolved) = game.teleport(target) else {
            return response_conn(
                422,
                "application/json",
                b"{\"error\":\"impassable\"}",
                keep_alive,
            );
        };
        snapped = resolved != target;
    }

    while state.action_sessions.len() >= MAX_ACTION_SESSIONS {
        if let Some(oldest) = state.action_order.pop_front() {
            state.action_sessions.remove(&oldest);
        } else {
            break;
        }
    }

    let id = state.alloc_id();
    let json = action_run_json(id, &game, false, snapped);
    state.action_sessions.insert(id, ActionSession::new(game));
    state.action_order.push_back(id);
    response_conn(201, "application/json", json.as_bytes(), keep_alive)
}

fn handle_action_teleport(
    id: u64,
    body: &[u8],
    state: &mut HostState,
    keep_alive: bool,
) -> Vec<u8> {
    if !state.action_sessions.contains_key(&id) {
        return response_conn(404, "text/plain", b"unknown run\n", keep_alive);
    }
    let Ok(fields) = parse_form_fields(body) else {
        return response_conn(400, "text/plain", b"invalid form body\n", keep_alive);
    };
    let mut seed: Option<u64> = None;
    let mut x: Option<i32> = None;
    let mut y: Option<i32> = None;
    let mut floor: u8 = 0;

    for (k, v) in fields {
        match k {
            "seed" => {
                let Some(parsed) = parse_u64_decimal(v) else {
                    return response_conn(400, "text/plain", b"invalid seed\n", keep_alive);
                };
                seed = Some(parsed);
            }
            "x" => {
                let Ok(parsed) = v.parse::<i32>() else {
                    return response_conn(400, "text/plain", b"invalid x\n", keep_alive);
                };
                x = Some(parsed);
            }
            "y" => {
                let Ok(parsed) = v.parse::<i32>() else {
                    return response_conn(400, "text/plain", b"invalid y\n", keep_alive);
                };
                y = Some(parsed);
            }
            "floor" => {
                let Ok(parsed) = v.parse::<u8>() else {
                    return response_conn(400, "text/plain", b"invalid floor\n", keep_alive);
                };
                floor = parsed;
            }
            _ => {}
        }
    }

    let (Some(seed), Some(tx), Some(ty)) = (seed, x, y) else {
        return response_conn(
            400,
            "text/plain",
            b"missing teleport parameters\n",
            keep_alive,
        );
    };
    let target = Tile3D::new(tx, ty, floor);

    let can_preserve = {
        let session = &state.action_sessions[&id];
        session.game.mode() == ActionMode::County
            && session.game.seed() == seed
            && session.game.outcome() == ActionOutcome::Playing
    };

    if can_preserve {
        let session = state
            .action_sessions
            .get_mut(&id)
            .expect("checked session existence");
        let Some(dest) = session.game.teleport(target) else {
            return response_conn(
                422,
                "application/json",
                b"{\"error\":\"impassable\"}",
                keep_alive,
            );
        };
        session.reset_clock_and_latches();
        if let Some(renderer) = &mut session.renderer {
            renderer.notify_teleport();
        }
        let json = action_run_json(id, &session.game, true, dest != target);
        return response_conn(200, "application/json", json.as_bytes(), keep_alive);
    }

    let world = state.county_world(seed);
    let mut fresh = ActionGame::county_with_world(world);
    let Some(dest) = fresh.teleport(target) else {
        return response_conn(
            422,
            "application/json",
            b"{\"error\":\"impassable\"}",
            keep_alive,
        );
    };
    let session = state
        .action_sessions
        .get_mut(&id)
        .expect("checked session existence");
    session.game = fresh;
    session.reset_clock_and_latches();
    if let Some(renderer) = &mut session.renderer {
        renderer.reset();
    }
    let json = action_run_json(id, &session.game, false, dest != target);
    response_conn(200, "application/json", json.as_bytes(), keep_alive)
}

fn handle_action_reset_clock(id: u64, state: &mut HostState, keep_alive: bool) -> Vec<u8> {
    let Some(session) = state.action_sessions.get_mut(&id) else {
        return response_conn(404, "text/plain", b"unknown run\n", keep_alive);
    };
    session.reset_clock_and_latches();
    response_conn(200, "text/plain", b"ok\n", keep_alive)
}

fn handle_action_frame(id: u64, body: &[u8], state: &mut HostState, keep_alive: bool) -> Vec<u8> {
    if !state.action_sessions.contains_key(&id) {
        return response_conn(404, "text/plain", b"unknown run\n", keep_alive);
    }
    let Ok(fields) = parse_form_fields(body) else {
        return response_conn(400, "text/plain", b"invalid form body\n", keep_alive);
    };

    let mut w: Option<u16> = None;
    let mut h: Option<u16> = None;
    let mut t_ns: u64 = 0;
    let mut mx: f32 = 0.0;
    let mut my: f32 = 0.0;
    let mut ax: f32 = 0.0;
    let mut ay: f32 = 0.0;
    let mut look_yaw: f32 = 0.0;
    let mut look_pitch: f32 = 0.0;
    let mut flags: u32 = 0;
    let mut weapon: i32 = ZB_WEAPON_UNCHANGED;
    let mut advance: u8 = 0;
    let mut camera: Option<u8> = None;
    let mut frame_version: u8 = 2;
    let mut control_kind: u8 = 0;
    let mut steer = 0.0f32;
    let mut throttle = 0.0f32;
    let mut brake = 0.0f32;
    let mut lean = 0.0f32;
    let mut mounted_isometric = false;

    for (k, v) in fields {
        match k {
            "frame_version" => match v {
                "2" => frame_version = 2,
                "3" => frame_version = 3,
                _ => {
                    return response_conn(
                        400,
                        "text/plain",
                        b"invalid frame_version\n",
                        keep_alive,
                    );
                }
            },
            "control_kind" => match v.parse::<u8>() {
                Ok(kind) if kind <= 1 => control_kind = kind,
                _ => {
                    return response_conn(400, "text/plain", b"invalid control_kind\n", keep_alive);
                }
            },
            "steer" | "throttle" | "brake" | "lean" => {
                let Ok(parsed) = v.parse::<f32>() else {
                    return response_conn(400, "text/plain", b"invalid ride axis\n", keep_alive);
                };
                match k {
                    "steer" => steer = parsed,
                    "throttle" => throttle = parsed,
                    "brake" => brake = parsed,
                    _ => lean = parsed,
                }
            }
            "mounted_isometric" => match v {
                "0" => mounted_isometric = false,
                "1" => mounted_isometric = true,
                _ => {
                    return response_conn(
                        400,
                        "text/plain",
                        b"invalid mounted_isometric\n",
                        keep_alive,
                    );
                }
            },
            "w" => {
                let Ok(parsed) = v.parse::<u16>() else {
                    return response_conn(400, "text/plain", b"invalid w\n", keep_alive);
                };
                w = Some(parsed.clamp(ACTION_W_MIN, ACTION_W_MAX));
            }
            "h" => {
                let Ok(parsed) = v.parse::<u16>() else {
                    return response_conn(400, "text/plain", b"invalid h\n", keep_alive);
                };
                h = Some(parsed.clamp(ACTION_H_MIN, ACTION_H_MAX));
            }
            "t_ns" => {
                let Some(parsed) = parse_u64_decimal(v) else {
                    return response_conn(400, "text/plain", b"invalid t_ns\n", keep_alive);
                };
                t_ns = parsed;
            }
            "mx" => {
                let Ok(parsed) = v.parse::<f32>() else {
                    return response_conn(400, "text/plain", b"invalid mx\n", keep_alive);
                };
                mx = parsed;
            }
            "my" => {
                let Ok(parsed) = v.parse::<f32>() else {
                    return response_conn(400, "text/plain", b"invalid my\n", keep_alive);
                };
                my = parsed;
            }
            "ax" => {
                let Ok(parsed) = v.parse::<f32>() else {
                    return response_conn(400, "text/plain", b"invalid ax\n", keep_alive);
                };
                ax = parsed;
            }
            "ay" => {
                let Ok(parsed) = v.parse::<f32>() else {
                    return response_conn(400, "text/plain", b"invalid ay\n", keep_alive);
                };
                ay = parsed;
            }
            "lyaw" => {
                let Ok(parsed) = v.parse::<f32>() else {
                    return response_conn(400, "text/plain", b"invalid lyaw\n", keep_alive);
                };
                look_yaw = parsed;
            }
            "lpitch" => {
                let Ok(parsed) = v.parse::<f32>() else {
                    return response_conn(400, "text/plain", b"invalid lpitch\n", keep_alive);
                };
                look_pitch = parsed;
            }
            "flags" => {
                let Ok(parsed) = v.parse::<u32>() else {
                    return response_conn(400, "text/plain", b"invalid flags\n", keep_alive);
                };
                flags = parsed;
            }
            "weapon" => {
                let Ok(parsed) = v.parse::<i32>() else {
                    return response_conn(400, "text/plain", b"invalid weapon\n", keep_alive);
                };
                weapon = parsed;
            }
            "advance" => match v {
                "0" => advance = 0,
                "1" => advance = 1,
                _ => return response_conn(400, "text/plain", b"invalid advance\n", keep_alive),
            },
            "camera" => match v.parse::<u8>() {
                Ok(mode) if mode < CameraMode::COUNT as u8 => camera = Some(mode),
                _ => return response_conn(400, "text/plain", b"invalid camera\n", keep_alive),
            },
            _ => {}
        }
    }

    let (Some(w), Some(h)) = (w, h) else {
        return response_conn(400, "text/plain", b"missing dimensions\n", keep_alive);
    };
    let Ok(select_weapon) = parse_select_weapon(weapon) else {
        return response_conn(400, "text/plain", b"invalid weapon\n", keep_alive);
    };

    if !state.gpu_attempted {
        state.gpu_attempted = true;
        match GpuContext::new_headless() {
            Ok(ctx) => state.gpu_context = Some(Arc::new(ctx)),
            Err(err) => state.gpu_error = Some(err.to_string()),
        }
    }
    let Some(gpu_context) = state.gpu_context.clone() else {
        let detail = state
            .gpu_error
            .as_deref()
            .unwrap_or("unknown adapter error");
        let message = format!("headless GPU unavailable: {detail}\n");
        return response_conn(
            503,
            "text/plain; charset=utf-8",
            message.as_bytes(),
            keep_alive,
        );
    };
    let session = state
        .action_sessions
        .get_mut(&id)
        .expect("run checked above");
    session.frame_version = frame_version;
    session.ensure_renderer(&gpu_context, u32::from(w), u32::from(h));
    let mounted_id = session.game.mounted_vehicle();
    let mounted_view = mounted_id.and_then(|id| {
        session
            .game
            .vehicle_views()
            .find_map(|(candidate, view)| (candidate == id).then_some(view))
    });
    let heading = mounted_view.map_or(session.game.player_body().heading, |v| match v {
        zombiebox_core::VehicleView::Motorcycle(m) => m.body.heading,
    });
    if let Some(renderer) = &mut session.renderer {
        renderer.set_mounted_camera_preference(mounted_isometric);
        renderer
            .camera_mut()
            .set_look_ahead((flags & ZB_INPUT_LOOK_AHEAD) != 0);
        if let Some(mode) = camera.and_then(|mode| CameraMode::from_index(mode as usize)) {
            let camera = renderer.camera_mut();
            if camera.mode() != mode {
                camera.set_mode(mode);
                if matches!(mode, CameraMode::Chase | CameraMode::FirstPerson) {
                    camera.initialize_view_from_heading(heading);
                }
            }
        }
    }

    let audio_events = if advance == 1 {
        let look_dt = session.last_dt;
        let mounted = session.game.control_mode() == zombiebox_core::ControlMode::Motorcycle;
        let camera = session
            .renderer
            .as_mut()
            .expect("renderer initialized")
            .camera_mut();
        let (movement, aim) = if mounted {
            (Vec2::ZERO, None)
        } else {
            match camera.mode() {
                CameraMode::Chase | CameraMode::FirstPerson => {
                    // Browser mouse-look arrives as a direct angular delta so a flick
                    // turns by the same amount regardless of frame pacing and stops
                    // immediately when the pointer stops. Touch keeps the stick-rate path.
                    camera.adjust_first_person_look(look_yaw, look_pitch);
                    camera.apply_look_input(Vec2::new(ax, ay), look_dt);
                    let frame = camera.current_frame();
                    let movement = stick(mx, my, &frame);
                    let aim = Vec2::new(-frame.yaw.cos(), -frame.yaw.sin());
                    (movement, Some(aim))
                }
                _ => {
                    let frame = camera.current_frame();
                    let movement = stick(mx, my, &frame);
                    let aim = stick(ax, ay, &frame);
                    (movement, (aim != Vec2::ZERO).then_some(aim))
                }
            }
        };
        let ride = (frame_version == 3 && control_kind == 1).then_some(MotorcycleInput {
            steer,
            throttle,
            brake,
            lean,
        });
        session.step_frame(
            t_ns,
            movement,
            aim,
            flags,
            select_weapon,
            ride,
            control_kind,
        )
    } else {
        session.reset_clock_and_latches();
        0
    };

    let Some(renderer) = session.renderer.as_mut() else {
        return response_conn(
            500,
            "text/plain; charset=utf-8",
            b"3D presentation setup failed\n",
            keep_alive,
        );
    };
    let pixels = match renderer.render_offscreen(&gpu_context, &session.game, session.last_dt) {
        Ok(image) => image.pixels,
        Err(err) => {
            let message = format!("GPU rendering failed: {err}\n");
            return response_conn(
                503,
                "text/plain; charset=utf-8",
                message.as_bytes(),
                keep_alive,
            );
        }
    };
    let payload = if session.frame_version == 3 {
        encode_zbf3_frame(&session.game, w, h, audio_events, &pixels)
    } else {
        encode_zbf2_frame(&session.game, w, h, audio_events, &pixels)
    };
    response_conn(200, "application/octet-stream", &payload, keep_alive)
}

fn encode_zbf2_frame(
    game: &ActionGame,
    w: u16,
    h: u16,
    audio_events: u64,
    pixels: &[u8],
) -> Vec<u8> {
    let hud = game.hud();
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

    let zombies = hud.zombie_count.min(u16::MAX as usize) as u16;
    let zombies_total = hud.zombies_total.min(u16::MAX as usize) as u16;

    let mut header = [0u8; 64];
    header[0..4].copy_from_slice(b"ZBF2");
    header[4..6].copy_from_slice(&w.to_le_bytes());
    header[6..8].copy_from_slice(&h.to_le_bytes());
    header[8..16].copy_from_slice(&audio_events.to_le_bytes());
    header[16..24].copy_from_slice(&hud.tick.to_le_bytes());
    header[24..28].copy_from_slice(&hud.player_pos.x.to_le_bytes());
    header[28..32].copy_from_slice(&hud.player_pos.y.to_le_bytes());
    header[32..34].copy_from_slice(&hud.active_reserve.to_le_bytes());
    header[34..36].copy_from_slice(&hud.pistol_reserve.to_le_bytes());
    header[36..38].copy_from_slice(&hud.shotgun_reserve.to_le_bytes());
    header[38..40].copy_from_slice(&hud.rocket_reserve.to_le_bytes());
    header[40..42].copy_from_slice(&zombies.to_le_bytes());
    header[42..44].copy_from_slice(&zombies_total.to_le_bytes());
    header[44..46].copy_from_slice(&status_flags.to_le_bytes());
    header[46] = hud.health;
    header[47] = hud.max_health;
    header[48] = hud.floor;
    header[49] = hud.equipped.map_or(ZB_WEAPON_NONE, |w| w as u8);
    header[50] = hud.owned_weapons_mask;
    header[51] = hud.active_mag;
    header[52] = hud.active_mag_cap;
    header[53] = hud.pistol_mag;
    header[54] = hud.shotgun_mag;
    header[55] = hud.rocket_mag;
    header[56] = hud.reload_progress;
    header[57] = hud.medkits;
    header[58] = hud.caches_found;
    header[59] = hud.caches_total;
    header[60] = hud.interact_prompt as u8;
    header[61] = hud.objective_kind as u8;
    header[62] = game.district_code();
    header[63] = game.mode() as u8;

    let mut out = Vec::with_capacity(64 + pixels.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(pixels);
    out
}

fn encode_zbf3_frame(
    game: &ActionGame,
    w: u16,
    h: u16,
    audio_events: u64,
    pixels: &[u8],
) -> Vec<u8> {
    let mut out = encode_zbf2_frame(game, w, h, audio_events, pixels);
    out[0..4].copy_from_slice(b"ZBF3");
    let mounted = game.hud().mounted;
    let mut ext = [0u8; 8];
    if let Some(hud) = mounted {
        ext[0] = 1;
        ext[1] = hud.grip_use;
        ext[2..4].copy_from_slice(&hud.speed_mph_x10.to_le_bytes());
        ext[4] = hud.engine_load;
        ext[5] = 1; // occupied riding state
        ext[6] = hud.condition_pct;
    }
    ext[0] = u8::from(game.control_mode() == zombiebox_core::ControlMode::Motorcycle);
    // Preserve the v2 header and insert the v3 extension before its RGBA pixels.
    out.splice(64..64, ext);
    out
}

fn action_run_json(id: u64, game: &ActionGame, preserved: bool, snapped: bool) -> String {
    let pose = game.player_tile();
    let mode = match game.mode() {
        ActionMode::BirchStreet => "birch",
        ActionMode::County => "county",
    };
    format!(
        "{{\"id\":\"{id}\",\"seed\":\"{}\",\"mode\":\"{mode}\",\"x\":{},\"y\":{},\"floor\":{},\"district\":{},\"preserved\":{preserved},\"snapped\":{snapped}}}",
        game.seed(),
        pose.x,
        pose.y,
        pose.floor,
        game.district_code()
    )
}

fn parse_form_fields(body: &[u8]) -> Result<Vec<(&str, &str)>, ()> {
    let text = std::str::from_utf8(body).map_err(|_| ())?;
    if text.is_empty() {
        return Ok(Vec::new());
    }
    text.split('&')
        .map(|pair| pair.split_once('=').ok_or(()))
        .collect()
}

fn parse_u64_decimal(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse::<u64>().ok()
}

fn parse_select_weapon(value: i32) -> Result<Option<Weapon>, ()> {
    if value == ZB_WEAPON_UNCHANGED {
        return Ok(None);
    }
    let slot = u8::try_from(value).map_err(|_| ())?;
    Weapon::from_slot(slot).map(Some).ok_or(())
}

fn stick(x: f32, y: f32, camera: &CameraFrame) -> Vec2 {
    camera.map_stick_to_world(Vec2::new(x, y))
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
            ActionEvent::Footstep { .. } | ActionEvent::ActorLanded { .. } => {}
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
            ActionEvent::VehicleCrash { .. } => mask |= ZB_SFX_VEHICLE_CRASH,
            ActionEvent::VehicleSkidStarted { .. } => mask |= ZB_SFX_VEHICLE_SKID,
            ActionEvent::VehicleImpact { .. } => mask |= ZB_SFX_VEHICLE_IMPACT,
        }
    }
    mask
}

fn command(b: u8) -> Option<Command> {
    Some(match b {
        b'.' => Command::Wait,
        b'N' => Command::Move(Direction::North),
        b'E' => Command::Move(Direction::East),
        b'S' => Command::Move(Direction::South),
        b'W' => Command::Move(Direction::West),
        b'L' => Command::Scavenge,
        b'A' => Command::Attack,
        _ => return None,
    })
}

fn state_json(id: u64, s: &Session, events: &[(u64, Event)]) -> Result<String, std::fmt::Error> {
    let hud = s.game.hud();
    // Stable viewport centered on the player; clip coordinates at world edges.
    let x0 = (hud.player.x - VIEW / 2).max(0);
    let y0 = (hud.player.y - VIEW / 2).max(0);
    let x1 = (x0 + VIEW).min(WORLD_SIZE);
    let y1 = (y0 + VIEW).min(WORLD_SIZE);
    let w = x1 - x0;
    let h = y1 - y0;
    let mut out = String::new();
    write!(
        out,
        "{{\"id\":\"{id}\",\"seed\":\"{}\",\"tick\":{},\"player\":{{\"x\":{},\"y\":{}}},\"health\":{},\"armed\":{},\"supplies\":{}",
        s.seed, hud.tick, hud.player.x, hud.player.y, hud.health, hud.armed, hud.supplies
    )?;
    if let Some(house) = s.game.house() {
        write!(
            out,
            ",\"house\":{{\"x\":{},\"y\":{},\"width\":{},\"height\":{},\"door\":{{\"x\":{},\"y\":{}}}}}",
            house.x, house.y, house.width, house.height, house.door.x, house.door.y
        )?;
    }
    if let Some((min, max)) = s.game.block_bounds() {
        write!(
            out,
            ",\"block\":{{\"min\":{{\"x\":{},\"y\":{}}},\"max\":{{\"x\":{},\"y\":{}}}}}",
            min.x, min.y, max.x, max.y
        )?;
    }
    out.push_str(",\"aim\":");
    if let Some(aim) = s.game.shot_target() {
        write!(out, "{{\"x\":{},\"y\":{}}}", aim.x, aim.y)?;
    } else {
        out.push_str("null");
    }
    out.push_str(",\"zombies\":[");
    for (i, z) in s.game.snapshot().zombies.iter().enumerate() {
        if i > 0 {
            out.push(',')
        }
        write!(out, "{{\"x\":{},\"y\":{}}}", z.x, z.y)?;
    }
    write!(
        out,
        "],\"tiles\":{{\"x\":{x0},\"y\":{y0},\"width\":{w},\"height\":{h},\"ground\":\""
    )?;
    for y in y0..y1 {
        for x in x0..x1 {
            let p = Position::new(x, y);
            let tile = s.game.world().tile_at(p).expect("viewport in world");
            out.push(match tile.ground {
                Ground::Road | Ground::Street | Ground::Bridge => 'r',
                Ground::Grass | Ground::Forest => 'g',
                Ground::Pavement => 'p',
                Ground::Field => 'f',
                Ground::Gravel | Ground::Trail | Ground::Mountain => 'v',
                Ground::River | Ground::Lake => 'w',
            });
        }
    }
    out.push_str("\",\"caches\":[");
    let mut first = true;
    for y in y0..y1 {
        for x in x0..x1 {
            let p = Position::new(x, y);
            if s.game.has_cache(p) {
                if !first {
                    out.push(',')
                }
                first = false;
                write!(out, "{{\"x\":{x},\"y\":{y}}}")?;
            }
        }
    }
    out.push_str("]},\"events\":[");
    for (i, (tick, e)) in events.iter().enumerate() {
        if i > 0 {
            out.push(',')
        }
        match e {
            Event::Scavenged { at } => write!(
                out,
                "{{\"type\":\"scavenged\",\"tick\":{tick},\"x\":{},\"y\":{}}}",
                at.x, at.y
            )?,
            Event::Explosion { at } => write!(
                out,
                "{{\"type\":\"explosion\",\"tick\":{tick},\"x\":{},\"y\":{}}}",
                at.x, at.y
            )?,
            Event::Gunshot { from, at } => write!(
                out,
                "{{\"type\":\"gunshot\",\"tick\":{tick},\"from\":{{\"x\":{},\"y\":{}}},\"at\":{{\"x\":{},\"y\":{}}}}}",
                from.x, from.y, at.x, at.y
            )?,
            Event::Hurt => write!(out, "{{\"type\":\"hurt\",\"tick\":{tick}}}")?,
            Event::Died => write!(out, "{{\"type\":\"died\",\"tick\":{tick}}}")?,
        }
    }
    out.push_str("]}");
    Ok(out)
}

fn response_conn(status: u16, mime: &str, body: &[u8], keep_alive: bool) -> Vec<u8> {
    let label = match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let conn = if keep_alive { "keep-alive" } else { "close" };
    let mut out = format!(
        "HTTP/1.1 {status} {label}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: {conn}\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game_at_starter_motorcycle() -> ActionGame {
        let mut game = ActionGame::county(42);
        let (_, bike_pos) = game
            .vehicle_views()
            .find_map(|(id, view)| {
                (id.0 & 0xff == 0).then(|| match view {
                    zombiebox_core::VehicleView::Motorcycle(bike) => (id, bike.body.position.xy()),
                })
            })
            .expect("seed 42 starter motorcycle");
        assert!(game.teleport(Tile3D::from_vec2(bike_pos, 0)).is_some());
        game
    }

    fn game_after_low_speed_tip() -> ActionGame {
        let mut game = game_at_starter_motorcycle();
        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        for _ in 0..20 {
            game.step(ActionInput {
                ride: Some(RideInput::Motorcycle(MotorcycleInput {
                    lean: 1.0,
                    ..MotorcycleInput::default()
                })),
                ..ActionInput::default()
            });
            if game.control_mode() == zombiebox_core::ControlMode::OnFoot {
                break;
            }
        }
        assert!(game.is_recovering_from_vehicle_crash());
        game
    }

    fn split_http_response(resp: &[u8]) -> (&str, &[u8]) {
        let idx = resp
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("valid HTTP response");
        let head = std::str::from_utf8(&resp[..idx]).expect("utf8 headers");
        (head, &resp[idx + 4..])
    }

    #[test]
    fn batch_is_validated_before_any_step() {
        let mut state = HostState {
            next_id: 1,
            ..HostState::default()
        };
        route("POST", "/api/runs", b"42", &mut state);
        assert_eq!(state.sessions[&1].game.hud().tick, 0);
        for invalid in [&b""[..], b"....", b".x", b"N\n"] {
            let response = route("POST", "/api/runs/1/step", invalid, &mut state);
            assert!(response.starts_with(b"HTTP/1.1 400"));
            assert_eq!(state.sessions[&1].game.hud().tick, 0);
        }
        route("POST", "/api/runs/1/step", b".EW", &mut state);
        assert_eq!(state.sessions[&1].game.hud().tick, 3);
    }

    #[test]
    fn command_parser_accepts_only_documented_ascii_commands() {
        assert!(b".NESWLA".iter().all(|b| command(*b).is_some()));
        assert!(b" neswla\n".iter().all(|b| command(*b).is_none()));
    }

    #[test]
    fn v3_interact_requires_matching_authoritative_control_kind() {
        let mut game = game_at_starter_motorcycle();
        game.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        assert_eq!(game.control_mode(), zombiebox_core::ControlMode::Motorcycle);

        let mut session = ActionSession::new(game);
        session.frame_version = 3;
        session.partial_tick = TICK_UNIT;
        session.step_frame(0, Vec2::ZERO, None, ZB_INPUT_INTERACT, None, None, 0);
        assert_eq!(
            session.game.control_mode(),
            zombiebox_core::ControlMode::Motorcycle,
            "a stale on-foot Interact must not dismount a rider"
        );

        session.partial_tick = TICK_UNIT;
        session.step_frame(0, Vec2::ZERO, None, ZB_INPUT_INTERACT, None, None, 1);
        assert_eq!(
            session.game.control_mode(),
            zombiebox_core::ControlMode::OnFoot,
            "a matching mounted Interact still dismounts at rest"
        );
    }

    #[test]
    fn v3_mount_interact_is_consumed_once_across_elapsed_ticks() {
        let mut session = ActionSession::new(game_at_starter_motorcycle());
        session.frame_version = 3;
        session.partial_tick = 2 * TICK_UNIT;
        session.step_frame(0, Vec2::ZERO, None, ZB_INPUT_INTERACT, None, None, 0);
        assert_eq!(
            session.game.control_mode(),
            zombiebox_core::ControlMode::Motorcycle,
            "the mount pulse must not be replayed as a dismount on the next tick"
        );
        assert!(!session.pending_interact);
    }

    #[test]
    fn v3_recovery_expiry_clears_cached_movement_for_remaining_ticks() {
        let mut batched = ActionSession::new(game_after_low_speed_tip());
        let mut split = ActionSession::new(game_after_low_speed_tip());
        assert_eq!(batched.game.hud().tick, split.game.hud().tick);
        batched.frame_version = 3;
        split.frame_version = 3;
        let movement = Vec2::new(0.0, -1.0);

        batched.partial_tick = 46 * TICK_UNIT;
        batched.step_frame(0, movement, None, 0, None, None, 0);

        split.partial_tick = 45 * TICK_UNIT;
        split.step_frame(0, movement, None, 0, None, None, 0);
        assert!(!split.game.is_recovering_from_vehicle_crash());
        split.step_frame(
            TICK_UNIT.div_ceil(u64::from(ACTION_TICKS_PER_SECOND)),
            Vec2::ZERO,
            None,
            0,
            None,
            None,
            0,
        );

        assert_eq!(batched.game.hud().tick, split.game.hud().tick);
        assert_eq!(
            batched.game.player_pose(),
            split.game.player_pose(),
            "the final same-request tick should use the neutralized input after recovery expires"
        );
    }

    #[test]
    fn action_county_gpu_protocol_renders_and_switches_camera() {
        let mut state = HostState {
            next_id: 1,
            ..HostState::default()
        };
        let create_resp = route(
            "POST",
            "/api/action/runs",
            b"mode=county&seed=42",
            &mut state,
        );
        let (create_head, create_body) = split_http_response(&create_resp);
        assert!(create_head.starts_with("HTTP/1.1 201 Created"));
        assert!(create_head.contains("Connection: keep-alive"));
        let create_json = std::str::from_utf8(create_body).unwrap();
        assert!(create_json.contains("\"id\":\"1\""));
        assert!(create_json.contains("\"seed\":\"42\""));
        assert!(create_json.contains("\"mode\":\"county\""));

        let f0 = route(
            "POST",
            "/api/action/1/frame",
            b"w=320&h=180&t_ns=1000000000&mx=0&my=0&ax=0&ay=0&flags=32&weapon=-1&advance=1",
            &mut state,
        );
        let (f0_head, f0_body) = split_http_response(&f0);
        assert!(f0_head.starts_with("HTTP/1.1 200 OK"));
        assert_eq!(f0_body.len(), 64 + 320 * 180 * 4);
        assert_eq!(&f0_body[0..4], b"ZBF2");
        assert_eq!(u16::from_le_bytes([f0_body[4], f0_body[5]]), 320);
        assert_eq!(u16::from_le_bytes([f0_body[6], f0_body[7]]), 180);
        assert_eq!(f0_body[63], ActionMode::County as u8);

        let f1 = route(
            "POST",
            "/api/action/1/frame",
            b"w=320&h=180&t_ns=1050000000&mx=1.0&my=0&ax=0&ay=0&flags=32&weapon=-1&advance=1",
            &mut state,
        );
        let (f1_head, f1_body) = split_http_response(&f1);
        assert!(f1_head.starts_with("HTTP/1.1 200 OK"));
        assert_eq!(f1_body.len(), 64 + 320 * 180 * 4);
        let tick = u64::from_le_bytes(f1_body[16..24].try_into().unwrap());
        assert_eq!(tick, 3);

        let context = state
            .gpu_context
            .as_ref()
            .expect("headless GPU initialized");
        assert!(!context.adapter_info.name.is_empty());
        let f2 = route(
            "POST",
            "/api/action/1/frame",
            b"w=320&h=180&t_ns=1050000000&mx=0&my=0&ax=0&ay=0&flags=32&weapon=-1&advance=0&camera=1",
            &mut state,
        );
        let (f2_head, f2_body) = split_http_response(&f2);
        assert!(f2_head.starts_with("HTTP/1.1 200 OK"));
        assert_eq!(&f2_body[0..4], b"ZBF2");
        assert_ne!(
            &f1_body[64..],
            &f2_body[64..],
            "camera mode must change the rendered image"
        );
        assert_eq!(
            state.action_sessions[&1]
                .renderer
                .as_ref()
                .unwrap()
                .camera()
                .mode(),
            CameraMode::Overhead
        );

        // Browser mouse-look uses direct angular deltas in Chase/First Person.
        let f3 = route(
            "POST",
            "/api/action/1/frame",
            b"w=320&h=180&t_ns=1050000000&mx=0&my=0&ax=0&ay=0&lyaw=0&lpitch=0&flags=32&weapon=-1&advance=1&camera=4",
            &mut state,
        );
        let (f3_head, f3_body) = split_http_response(&f3);
        assert!(f3_head.starts_with("HTTP/1.1 200 OK"));
        let f4 = route(
            "POST",
            "/api/action/1/frame",
            b"w=320&h=180&t_ns=1050000000&mx=0&my=0&ax=0&ay=0&lyaw=0.35&lpitch=0.1&flags=32&weapon=-1&advance=1&camera=4",
            &mut state,
        );
        let (f4_head, f4_body) = split_http_response(&f4);
        assert!(f4_head.starts_with("HTTP/1.1 200 OK"));
        assert_ne!(
            &f3_body[64..],
            &f4_body[64..],
            "mouse look delta must change the rendered view"
        );
        assert_eq!(
            state.action_sessions[&1]
                .renderer
                .as_ref()
                .unwrap()
                .camera()
                .mode(),
            CameraMode::FirstPerson
        );

        let v3 = route(
            "POST",
            "/api/action/1/frame",
            b"w=320&h=180&t_ns=1050000000&mx=0&my=0&ax=0&ay=0&flags=0&weapon=-1&advance=0&frame_version=3&control_kind=0&steer=0&throttle=0&brake=0&lean=0&mounted_isometric=0",
            &mut state,
        );
        let (_, v3_body) = split_http_response(&v3);
        assert_eq!(&v3_body[0..4], b"ZBF3");
        assert_eq!(v3_body.len(), 72 + 320 * 180 * 4);
        assert_eq!(v3_body[64], 0, "on-foot control kind is authoritative");

        let legacy = route(
            "POST",
            "/api/action/1/frame",
            b"w=320&h=180&t_ns=1050000000&mx=0&my=0&ax=0&ay=0&flags=0&weapon=-1&advance=0",
            &mut state,
        );
        let (_, legacy_body) = split_http_response(&legacy);
        assert_eq!(&legacy_body[0..4], b"ZBF2");
        assert_eq!(legacy_body.len(), 64 + 320 * 180 * 4);
    }

    #[test]
    fn action_county_gpu_protocol_mounted_respects_all_camera_modes() {
        let mut state = HostState {
            next_id: 1,
            ..HostState::default()
        };
        let mut starter = game_at_starter_motorcycle();
        starter.step(ActionInput {
            interact: true,
            ..ActionInput::default()
        });
        assert_eq!(starter.control_mode(), zombiebox_core::ControlMode::Motorcycle);
        state.action_sessions.insert(1, ActionSession::new(starter));

        // First frame initializes GPU renderer and establishes baseline frame time
        let init_req = route(
            "POST",
            "/api/action/1/frame",
            b"w=320&h=180&t_ns=1000000000&mx=0&my=0&ax=0&ay=0&flags=0&weapon=-1&advance=0&frame_version=3&control_kind=1&steer=0&throttle=0&brake=0&lean=0&camera=0",
            &mut state,
        );
        let (_, init_body) = split_http_response(&init_req);
        assert_eq!(&init_body[0..4], b"ZBF3");
        assert_eq!(init_body[64], 1, "session starts mounted on motorcycle");

        // Verify each camera mode can be selected and is respected while riding
        for mode_idx in 0..CameraMode::COUNT {
            let body_str = format!(
                "w=320&h=180&t_ns=1000000000&mx=0&my=0&ax=0&ay=0&flags=0&weapon=-1&advance=0&frame_version=3&control_kind=1&steer=0&throttle=0&brake=0&lean=0&camera={mode_idx}"
            );
            let resp = route("POST", "/api/action/1/frame", body_str.as_bytes(), &mut state);
            let (head, body) = split_http_response(&resp);
            assert!(head.starts_with("HTTP/1.1 200 OK"));
            assert_eq!(&body[0..4], b"ZBF3");
            assert_eq!(body[64], 1, "still mounted");
            let expected_mode = CameraMode::from_index(mode_idx).unwrap();
            assert_eq!(
                state.action_sessions[&1]
                    .renderer
                    .as_ref()
                    .unwrap()
                    .camera()
                    .mode(),
                expected_mode,
                "renderer camera mode must match selected camera mode while mounted"
            );
        }

        // Advance clock baseline then send a tick with interact (flags=4) to dismount at rest
        let _ = route(
            "POST",
            "/api/action/1/frame",
            b"w=320&h=180&t_ns=1000000000&mx=0&my=0&ax=0&ay=0&flags=0&weapon=-1&advance=1&frame_version=3&control_kind=1&steer=0&throttle=0&brake=0&lean=0",
            &mut state,
        );
        let dismount_req = route(
            "POST",
            "/api/action/1/frame",
            b"w=320&h=180&t_ns=1050000000&mx=0&my=0&ax=0&ay=0&flags=4&weapon=-1&advance=1&frame_version=3&control_kind=1&steer=0&throttle=0&brake=0&lean=0",
            &mut state,
        );
        let (_, dismount_body) = split_http_response(&dismount_req);
        assert_eq!(&dismount_body[0..4], b"ZBF3");
        assert_eq!(dismount_body[64], 0, "now dismounted");
        assert_eq!(
            state.action_sessions[&1]
                .renderer
                .as_ref()
                .unwrap()
                .camera()
                .mode(),
            CameraMode::FirstPerson,
            "camera mode preserved after dismount"
        );
    }

    #[test]
    fn action_teleport_preserves_same_seed_and_resets_different_seed() {
        let mut state = HostState {
            next_id: 1,
            ..HostState::default()
        };
        let create_resp = route(
            "POST",
            "/api/action/runs",
            b"mode=county&seed=42",
            &mut state,
        );
        assert!(create_resp.starts_with(b"HTTP/1.1 201"));

        // Step a few ticks so tick > 0 on session 1.
        let _ = route(
            "POST",
            "/api/action/1/frame",
            b"w=320&h=180&t_ns=1000000000&mx=0&my=0&ax=0&ay=0&flags=32&weapon=-1&advance=1",
            &mut state,
        );
        let _ = route(
            "POST",
            "/api/action/1/frame",
            b"w=320&h=180&t_ns=1050000000&mx=0&my=0&ax=0&ay=0&flags=32&weapon=-1&advance=1",
            &mut state,
        );
        assert_eq!(state.action_sessions[&1].game.hud().tick, 3);

        // Find a valid walkable destination on seed 42 (the spawn position itself is guaranteed walkable).
        let dest = state.action_sessions[&1].game.player_tile();
        let tp_same_body = format!("seed=42&x={}&y={}&floor={}", dest.x, dest.y, dest.floor);
        let tp_same = route(
            "POST",
            "/api/action/1/teleport",
            tp_same_body.as_bytes(),
            &mut state,
        );
        let (same_head, same_body) = split_http_response(&tp_same);
        assert!(same_head.starts_with("HTTP/1.1 200 OK"));
        let same_json = std::str::from_utf8(same_body).unwrap();
        assert!(same_json.contains("\"preserved\":true"));
        assert_eq!(state.action_sessions[&1].game.hud().tick, 3);

        // Teleporting with a different seed creates a fresh run (preserved == false).
        let fresh_world = state.county_world(99);
        let fresh_spawn = fresh_world.wake_position();
        let tp_diff_body = format!("seed=99&x={}&y={}&floor=0", fresh_spawn.x, fresh_spawn.y);
        let tp_diff = route(
            "POST",
            "/api/action/1/teleport",
            tp_diff_body.as_bytes(),
            &mut state,
        );
        let (diff_head, diff_body) = split_http_response(&tp_diff);
        assert!(diff_head.starts_with("HTTP/1.1 200 OK"));
        let diff_json = std::str::from_utf8(diff_body).unwrap();
        assert!(diff_json.contains("\"preserved\":false"));
        assert!(diff_json.contains("\"seed\":\"99\""));
        assert_eq!(state.action_sessions[&1].game.seed(), 99);
        assert_eq!(state.action_sessions[&1].game.hud().tick, 0);
    }

    #[test]
    fn static_routes_serve_action_and_legacy_assets() {
        for (path, mime_substr) in [
            ("/", "text/html"),
            ("/index.html", "text/html"),
            ("/action.js", "text/javascript"),
            ("/legacy", "text/html"),
            ("/legacy.html", "text/html"),
            ("/county.html", "text/html"),
        ] {
            let resp = static_route(path);
            let (head, body) = split_http_response(&resp);
            assert!(head.starts_with("HTTP/1.1 200 OK"), "{path}");
            assert!(head.contains(mime_substr), "{path}: {head}");
            assert!(head.contains("Connection: keep-alive"), "{path}");
            assert!(!body.is_empty(), "{path}");
        }
    }
}
