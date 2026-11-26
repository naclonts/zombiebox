//! Shared field atlas payload used by the local web host and offline Android adapter.
use crate::{District, GENERATOR_VERSION, Ground, LandmarkKind, Position, WORLD_SIZE, World};
use std::fmt::Write;

fn district_key(district: District) -> (char, &'static str) {
    match district {
        District::Residential => ('R', "Residential"),
        District::City => ('C', "Downtown"),
        District::Hospital => ('H', "Hospital"),
        District::Police => ('P', "Police"),
        District::MilitaryBase => ('A', "Army base"),
        District::Woods => ('W', "Woods"),
        District::Farm => ('F', "Countryside"),
    }
}

fn ground_key(ground: Ground) -> char {
    match ground {
        Ground::Grass => 'g',
        Ground::Field => 'f',
        Ground::Forest => 't',
        Ground::Mountain => 'm',
        Ground::River => 'r',
        Ground::Lake => 'l',
        Ground::Road => 'A',
        Ground::Street => 's',
        Ground::Trail => 'd',
        Ground::Bridge => 'b',
        Ground::Gravel => 'v',
        Ground::Pavement => 'p',
    }
}

fn landmark_name(kind: LandmarkKind) -> &'static str {
    match kind {
        LandmarkKind::Residential => "Residential",
        LandmarkKind::Downtown => "Downtown",
        LandmarkKind::Hospital => "Hospital",
        LandmarkKind::Police => "Police station",
        LandmarkKind::ArmyBase => "Army base",
        LandmarkKind::Cabin => "Woodland cabin",
        LandmarkKind::Lake => "Lakeshore",
        LandmarkKind::Mountain => "Mountain pass",
    }
}

/// Generate an ASCII JSON survey for a full unsigned 64-bit seed. Maps use
/// one character per tile, in row-major order, to keep phone memory use small.
pub fn county_survey_json(seed: u64) -> String {
    let world = World::new(seed);
    let sites = world.sites().expect("generated county");
    let size = (WORLD_SIZE * WORLD_SIZE) as usize;
    let mut ground = String::with_capacity(size);
    let mut district = String::with_capacity(size);
    let mut elevation = String::with_capacity(size);
    let mut features = String::with_capacity(size);
    for y in 0..WORLD_SIZE {
        for x in 0..WORLD_SIZE {
            let t = world.tile_at(Position::new(x, y)).expect("in bounds");
            ground.push(ground_key(t.ground));
            district.push(district_key(t.district).0);
            let altitude = char::from(33 + t.elevation / 4);
            if matches!(altitude, '"' | '\\') {
                elevation.push('\\');
            }
            elevation.push(altitude);
            features.push(t.feature.symbol());
        }
    }

    let spawn = world.wake_position();
    let mut json = format!(
        r#"{{"seed":"{seed}","version":{GENERATOR_VERSION},"size":{WORLD_SIZE},"spawn":[{},{}],"ground":"{ground}","district":"{district}","elevation":"{elevation}","features":"{features}","landmarks":["#,
        spawn.x, spawn.y
    );
    for (i, l) in world.landmarks().iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        write!(
            json,
            r#"{{"name":"{}","x":{},"y":{}}}"#,
            landmark_name(l.kind),
            l.entrance.x,
            l.entrance.y
        )
        .unwrap();
    }
    json.push_str(r#"],"buildings":["#);
    for (i, b) in sites.buildings.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        write!(json, r#"{{"name":"{}","district":"{}","x":{},"y":{},"w":{},"h":{},"floors":{},"entry":[{},{}],"material":"{:?}","upper":["#,
            b.kind.name(),district_key(b.district).1,b.min.x,b.min.y,b.max.x-b.min.x+1,b.max.y-b.min.y+1,b.floors,b.entry.x,b.entry.y,b.material).unwrap();
        for floor in 1..b.floors {
            if floor > 1 {
                json.push(',');
            }
            json.push('"');
            for y in b.min.y..=b.max.y {
                for x in b.min.x..=b.max.x {
                    json.push(
                        b.feature_at(Position::new(x, y), floor)
                            .map_or('.', |f| f.symbol()),
                    );
                }
            }
            json.push('"');
        }
        json.push_str(r#"],"rooms":["#);
        for (i, room) in b.layout.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            write!(
                json,
                r#"{{"name":"{:?}","x":{},"y":{},"w":{},"h":{},"floor":{}}}"#,
                room.kind,
                room.min.x,
                room.min.y,
                room.max.x - room.min.x + 1,
                room.max.y - room.min.y + 1,
                room.floor
            )
            .unwrap();
        }
        json.push_str("]}");
    }
    json.push_str(r#"],"loot":["#);
    for (i, s) in sites.loot.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        write!(
            json,
            r#"{{"name":"{}","x":{},"y":{},"floor":{}}}"#,
            s.kind.name(),
            s.position.x,
            s.position.y,
            s.floor
        )
        .unwrap();
    }
    json.push_str(r#"],"infected":["#);
    for (i, s) in sites.infected.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        write!(
            json,
            r#"{{"name":"{:?}","x":{},"y":{},"floor":{}}}"#,
            s.kind, s.position.x, s.position.y, s.floor
        )
        .unwrap();
    }
    json.push_str("]}");
    json
}
