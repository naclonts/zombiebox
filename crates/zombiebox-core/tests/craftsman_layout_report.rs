//! Diagnostic report for the furnished Craftsman (seed 42 by default).
//!
//! `cargo test -p zombiebox-core --test craftsman_layout_report -- --ignored --nocapture`

use zombiebox_core::blueprint::{BlueprintKind, LocalCell, PlannedProp, placement_id};
use zombiebox_core::{PropFacing, PropId, World};

fn glyph(id: PropId) -> char {
    match id {
        PropId::Sofa => 'S',
        PropId::TableCoffee => 'c',
        PropId::Table => 'T',
        PropId::Chair => 'h',
        PropId::Counter => 'C',
        PropId::Television => 'V',
        PropId::BookcaseOpen => 'B',
        PropId::BedDouble | PropId::Bed => 'D',
        PropId::Desk => 'd',
        PropId::ChairDesk => 'k',
        PropId::Bathtub => 't',
        PropId::Toilet => 'o',
        PropId::BathroomSink => 'z',
        PropId::FirstAidKit => 'f',
        PropId::KitchenSink => 's',
        PropId::KitchenStove => 'v',
        PropId::Fridge => 'r',
        PropId::KitchenCabinet => 'a',
        PropId::Note => 'N',
        _ => '?',
    }
}

fn covers(prop: &PlannedProp, u: i32, v: i32, floor: u8) -> bool {
    let span = prop.prop.def().span;
    let (w, h) = match prop.facing {
        PropFacing::East | PropFacing::West => (span.height as i32, span.width as i32),
        _ => (span.width as i32, span.height as i32),
    };
    floor == prop.anchor.floor
        && u >= prop.anchor.u
        && u < prop.anchor.u + w
        && v >= prop.anchor.v
        && v < prop.anchor.v + h
}

#[test]
#[ignore = "diagnostic report"]
fn craftsman_layout_report() {
    let seed = std::env::var("ZB_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(42);
    let world = World::new(seed);
    let plan = world.fixture_plan().expect("fixture plan");
    let structure = &plan.structures[0];
    let bp = &structure.blueprint;
    println!(
        "seed {seed} structure {} frontage {:?}",
        structure.name, structure.transform.frontage
    );
    println!("local grid (rows = v, cols = u); '#' wall, 'w' window, '+' door, '.' floor");
    for v in 0..=13 {
        let mut line = format!("v{v:>2} ");
        for u in 0..=14 {
            let cell = LocalCell::new(u, v, 0);
            let mut ch = match bp.cell(cell).map(|c| c.kind) {
                None => ' ',
                Some(BlueprintKind::Window) => 'w',
                Some(k) if k.is_wall() => '#',
                Some(BlueprintKind::Door { .. }) => '+',
                Some(_) => '.',
            };
            for prop in &bp.props {
                if covers(prop, u, v, 0) {
                    ch = glyph(prop.prop);
                }
            }
            line.push(ch);
        }
        println!("{line}");
    }
    println!("props:");
    let world_props = structure.props_world();
    for prop in &bp.props {
        let uid = placement_id(structure.id, prop.key).0;
        let world = world_props
            .iter()
            .find(|p| p.anchor_uid == uid)
            .map(|p| p.to_anchor());
        println!(
            "  key={:#06x} {:?} room={:?} anchor=({},{}) facing={:?} offset=({:+.2},{:+.2},{:.2}) host={:?} world_pivot={:?} layer={:?}",
            prop.key.0,
            prop.prop,
            prop.room,
            prop.anchor.u,
            prop.anchor.v,
            prop.facing,
            prop.offset_m.x,
            prop.offset_m.y,
            prop.offset_m.z,
            prop.host.map(|h| h.0),
            world.map(|a| a.pivot(structure.foundation_z)),
            prop.prop.def().attachment,
        );
    }
    for sign in &bp.signs {
        println!(
            "  sign {:?} anchor=({},{}) layer={:?} offset={:?}",
            sign.readable, sign.anchor.u, sign.anchor.v, sign.attachment, sign.offset_m
        );
    }
}
