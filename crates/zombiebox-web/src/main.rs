use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};
use zombiebox_core::{
    Command, Direction, Event, Game, Ground, Position, WORLD_SIZE, county_survey_json,
};

const MAX_REQUEST: usize = 4096;
const MAX_SESSIONS: usize = 256;
const VIEW: i32 = 23;
const INDEX: &str = include_str!("../../../apps/web/index.html");
const JS: &str = include_str!("../../../apps/web/game.js");
const CSS: &str = include_str!("../../../apps/web/style.css");
const SCENE: &str = include_str!("../../../apps/web/scene.js");

struct Session {
    seed: u64,
    game: Game,
}

#[derive(Default)]
struct HostState {
    sessions: HashMap<u64, Session>,
    order: VecDeque<u64>,
    next_id: u64,
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
    let (tx, rx) = mpsc::sync_channel::<TcpStream>(32);
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..8 {
        let rx = Arc::clone(&rx);
        let state = Arc::clone(&state);
        thread::spawn(move || {
            loop {
                let stream = match rx.lock().expect("worker receiver poisoned").recv() {
                    Ok(s) => s,
                    Err(_) => break,
                };
                handle(stream, &state);
            }
        });
    }
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                if tx.send(stream).is_err() {
                    break;
                }
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, state: &Mutex<HostState>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let response = match read_request(&mut stream) {
        Ok((method, path, body, cross_site)) => {
            if method == "POST" && cross_site {
                response(403, "text/plain", b"cross-site request denied\n")
            } else if method == "GET" {
                static_route(&path)
            } else {
                let mut state = state.lock().expect("host state poisoned");
                route(&method, &path, &body, &mut state)
            }
        }
        Err(status) => response(status, "text/plain; charset=utf-8", b"bad request\n"),
    };
    let _ = stream.write_all(&response);
}

fn read_request(stream: &mut TcpStream) -> Result<(String, String, Vec<u8>, bool), u16> {
    let mut bytes = Vec::new();
    let mut buf = [0u8; 1024];
    let split;
    loop {
        if bytes.len() >= MAX_REQUEST {
            return Err(413);
        }
        let n = stream.read(&mut buf).map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        bytes.extend_from_slice(&buf[..n]);
        if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            split = i + 4;
            break;
        }
    }
    let head = std::str::from_utf8(&bytes[..split - 4]).map_err(|_| 400u16)?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().ok_or(400u16)?.split_whitespace();
    let method = first.next().ok_or(400u16)?.to_string();
    let path = first.next().ok_or(400u16)?.to_string();
    let version = first.next();
    if !matches!(version, Some("HTTP/1.1" | "HTTP/1.0")) || first.next().is_some() {
        return Err(400);
    }
    let mut len = 0usize;
    let mut saw_len = false;
    let mut cross_site = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                if saw_len {
                    return Err(400);
                }
                saw_len = true;
                len = value.trim().parse().map_err(|_| 400u16)?;
            }
            if name.eq_ignore_ascii_case("sec-fetch-site")
                && value.trim().eq_ignore_ascii_case("cross-site")
            {
                cross_site = true;
            }
            if name.eq_ignore_ascii_case("transfer-encoding") {
                return Err(400);
            }
        }
    }
    if len > MAX_REQUEST || split + len > MAX_REQUEST {
        return Err(413);
    }
    while bytes.len() < split + len {
        let n = stream.read(&mut buf).map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        bytes.extend_from_slice(&buf[..n]);
        if bytes.len() > MAX_REQUEST {
            return Err(413);
        }
    }
    Ok((method, path, bytes[split..split + len].to_vec(), cross_site))
}

fn static_route(path: &str) -> Vec<u8> {
    let path = path.split('?').next().unwrap_or(path);
    if let Some(value) = path.strip_prefix("/api/county/") {
        if let Some(seed) = value
            .parse::<u64>()
            .ok()
            .filter(|_| value.bytes().all(|b| b.is_ascii_digit()))
        {
            return response(200, "application/json", county_survey_json(seed).as_bytes());
        }
        return response(400, "text/plain", b"invalid seed\n");
    }
    let (mime, data) = match path {
        "/" | "/index.html" => ("text/html", INDEX),
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
        _ => return response(404, "text/plain", b"not found\n"),
    };
    response(200, mime, data.as_bytes())
}

fn route(method: &str, path: &str, body: &[u8], state: &mut HostState) -> Vec<u8> {
    if method != "POST" {
        return response(405, "text/plain", b"method not allowed\n");
    }
    if path == "/api/runs" {
        let Ok(seed_text) = std::str::from_utf8(body) else {
            return response(400, "text/plain", b"invalid seed\n");
        };
        if seed_text.is_empty() || !seed_text.bytes().all(|b| b.is_ascii_digit()) {
            return response(400, "text/plain", b"invalid seed\n");
        }
        let Ok(seed) = seed_text.parse::<u64>() else {
            return response(400, "text/plain", b"invalid seed\n");
        };
        while state.sessions.len() >= MAX_SESSIONS {
            if let Some(oldest) = state.order.pop_front() {
                state.sessions.remove(&oldest);
            }
        }
        let id = state.next_id;
        state.next_id = state.next_id.wrapping_add(1).max(1);
        let game = Game::residential(seed);
        state.sessions.insert(id, Session { seed, game });
        state.order.push_back(id);
        return match state_json(id, &state.sessions[&id], &[]) {
            Ok(s) => response(201, "application/json", s.as_bytes()),
            Err(_) => response(500, "text/plain", b"serialization error\n"),
        };
    }
    let Some(id_str) = path
        .strip_prefix("/api/runs/")
        .and_then(|s| s.strip_suffix("/step"))
    else {
        return response(404, "text/plain", b"not found\n");
    };
    let Ok(id) = id_str.parse::<u64>() else {
        return response(404, "text/plain", b"not found\n");
    };
    let Some(session) = state.sessions.get_mut(&id) else {
        return response(404, "text/plain", b"unknown run\n");
    };
    let Ok(text) = std::str::from_utf8(body) else {
        return response(400, "text/plain", b"invalid batch\n");
    };
    let commands: Option<Vec<_>> = text.bytes().map(command).collect();
    let Some(commands) = commands.filter(|c| !c.is_empty() && c.len() <= 3) else {
        return response(400, "text/plain", b"batch must contain 1 to 3 commands\n");
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
        Ok(s) => response(200, "application/json", s.as_bytes()),
        Err(_) => response(500, "text/plain", b"serialization error\n"),
    }
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
    use std::fmt::Write as FmtWrite;
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

fn response(status: u16, mime: &str, body: &[u8]) -> Vec<u8> {
    let label = match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let mut out=format!("HTTP/1.1 {status} {label}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\r\n",body.len()).into_bytes();
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
