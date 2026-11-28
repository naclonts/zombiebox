//! Deterministic room furnishing for authored Phase 3 blueprints.
//!
//! Furnishing runs after doors and stairs are authored. It reserves circulation
//! first, then places required fixtures in dependency/priority order and only
//! afterwards fills optional clutter. Every placement is checked against the
//! room mask, the reserved circulation spine, prop span rotation, window
//! sightlines and real interaction approaches. The final pass reruns
//! reachability over the furnished walkable mask, so optional clutter cannot
//! close a room or trap a fixture.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use crate::action::{CellAttachment, PropCover, PropFacing, PropId, PropSpan, Vec3, mix64};

use super::RoomKind;
use super::blueprint::{
    BlueprintAnchor, BlueprintGrid, BlueprintKind, FitError, LocalCell, LocalPlacementKey,
    LocalPropOffset, LootProfileId, PlacementStrategy, PlannedProp, RoomMeta, RuleKey,
    WallPreference,
};

const FURNISH_SALT: u64 = 0x4655_524e_4953_4831; // "FURNISH1"

const NEIGHBORS: [(i32, i32); 4] = [(0, 1), (-1, 0), (1, 0), (0, -1)];

/// One authored furnishing rule. Alternative prop IDs are tried in order, so a
/// double bed may fall back to a single bed when the room is narrow.
#[derive(Clone, Copy, Debug)]
pub struct PropPlacementRule {
    pub key: RuleKey,
    pub alternatives: &'static [PropId],
    pub strategy: PlacementStrategy,
    /// Minimum number of copies. Failing this fails the layout candidate.
    pub required: u8,
    pub maximum: u8,
    /// Chance to add each optional copy beyond `required`.
    pub extra_chance: f32,
    pub priority: u8,
    pub loot: Option<LootProfileId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FurnishReport {
    pub placed: u16,
    pub optional_removed: u16,
}

#[derive(Clone, Copy, Debug)]
struct PlacedProp {
    prop: PropId,
    anchor: BlueprintAnchor,
    facing: PropFacing,
    required: bool,
}

#[derive(Default)]
struct Occupancy {
    floor: BTreeSet<LocalCell>,
    attachments: HashSet<(LocalCell, CellAttachment)>,
}

impl Occupancy {
    fn blocks(&self, cells: &[LocalCell], attachment: CellAttachment) -> bool {
        cells.iter().any(|cell| match attachment {
            CellAttachment::Floor => self.floor.contains(cell),
            layer => self.attachments.contains(&(*cell, layer)),
        })
    }

    fn insert(&mut self, cells: &[LocalCell], attachment: CellAttachment) {
        for cell in cells {
            match attachment {
                CellAttachment::Floor => {
                    self.floor.insert(*cell);
                }
                layer => {
                    self.attachments.insert((*cell, layer));
                }
            }
        }
    }
}

/// Furnish every room in `blueprint` with its authored room grammar.
pub fn furnish(seed: u64, blueprint: &mut BlueprintGrid) -> Result<FurnishReport, FitError> {
    let mut reserved = reserve_circulation(blueprint);
    let mut occupancy = Occupancy::default();
    let mut placed_by_rule: BTreeMap<RuleKey, Vec<PlacedProp>> = BTreeMap::new();
    let mut placed_props: Vec<PlannedProp> = Vec::new();
    let mut required_keys = BTreeSet::new();

    for room in blueprint.rooms.clone() {
        let rules = rules_for(room.kind);
        if rules.is_empty() {
            continue;
        }
        let room_cells = room_cell_set(blueprint, &room);
        if room_cells.is_empty() {
            continue;
        }
        let ordered = order_rules(rules)?;
        for (rule, _depth) in ordered {
            let copies = planned_copies(seed, &room, rule);
            for copy in 0..copies {
                if let Some(placed) = try_place_rule(
                    seed,
                    blueprint,
                    &room,
                    &room_cells,
                    &mut reserved,
                    &mut occupancy,
                    &placed_by_rule,
                    rule,
                    copy,
                )? {
                    let key = placement_key(rule.key, copy);
                    let offset_m = match rule.strategy {
                        PlacementStrategy::Attachment { host, .. } => placed_by_rule
                            .get(&host)
                            .and_then(|props| props.first())
                            .map_or(Vec3::ZERO, |host| {
                                Vec3::new(0.0, 0.0, host.prop.def().collision_max.z)
                            }),
                        _ => Vec3::ZERO,
                    };
                    let planned = PlannedProp {
                        key,
                        prop: placed.prop,
                        anchor: placed.anchor,
                        facing: placed.facing,
                        room: Some(room.id),
                        host: host_key(rule),
                        offset_m,
                        loot: rule.loot,
                    };
                    placed_props.push(planned);
                    if placed.required {
                        required_keys.insert(key);
                    }
                    placed_by_rule.entry(rule.key).or_default().push(placed);
                }
            }
        }
    }

    blueprint.props.extend(placed_props);
    let removed = prune_unreachable(blueprint, &required_keys)?;
    Ok(FurnishReport {
        placed: blueprint.props.len() as u16,
        optional_removed: removed,
    })
}

fn placement_key(rule: RuleKey, copy: u8) -> LocalPlacementKey {
    LocalPlacementKey((rule.0 << 8) | u32::from(copy))
}

fn host_key(rule: &PropPlacementRule) -> Option<LocalPlacementKey> {
    match rule.strategy {
        PlacementStrategy::RelativeTo { parent, .. }
        | PlacementStrategy::Attachment { host: parent, .. } => Some(placement_key(parent, 0)),
        _ => None,
    }
}

fn planned_copies(seed: u64, room: &RoomMeta, rule: &PropPlacementRule) -> u8 {
    let mut desired = rule.required.min(rule.maximum);
    while desired < rule.maximum {
        let roll = mix64(
            seed ^ FURNISH_SALT
                ^ ((rule.key.0 as u64) << 32)
                ^ (u64::from(room.id) << 16)
                ^ u64::from(desired),
        );
        let chance = (f64::from(rule.extra_chance).clamp(0.0, 1.0) * 10_000.0) as u64;
        if rule.extra_chance >= 1.0 || roll % 10_000 < chance {
            desired += 1;
        } else {
            break;
        }
    }
    desired
}

fn room_cell_set(blueprint: &BlueprintGrid, room: &RoomMeta) -> HashSet<LocalCell> {
    room.cells
        .iter()
        .copied()
        .filter(|cell| {
            blueprint
                .cell(*cell)
                .is_some_and(|cell| cell.kind == BlueprintKind::Floor)
        })
        .collect()
}

/// Reserve doorway aprons, stair landings and one-tile circulation spines
/// between the portals of every room.
fn reserve_circulation(blueprint: &BlueprintGrid) -> HashSet<LocalCell> {
    let mut reserved = HashSet::new();
    for door in &blueprint.doors {
        let normal = door.tangent.normal();
        for sign in [-1, 1] {
            let cell = LocalCell::new(
                door.cell.u + normal.0 * sign,
                door.cell.v + normal.1 * sign,
                door.cell.floor,
            );
            if blueprint
                .cell(cell)
                .is_some_and(|cell| cell.kind.walkable())
            {
                reserved.insert(cell);
            }
        }
    }
    for stair in &blueprint.stairs {
        for cell in [
            stair.lower,
            stair.upper,
            stair.lower_landing,
            stair.upper_landing,
        ] {
            if blueprint
                .cell(cell)
                .is_some_and(|cell| cell.kind.walkable())
            {
                reserved.insert(cell);
            }
        }
    }

    for room in &blueprint.rooms {
        let cells = room_cell_set(blueprint, room);
        if cells.len() < 2 {
            continue;
        }
        let mut portals: Vec<LocalCell> = Vec::new();
        for door in &blueprint.doors {
            if door.cell.floor != room.floor {
                continue;
            }
            let normal = door.tangent.normal();
            for sign in [-1, 1] {
                let cell = LocalCell::new(
                    door.cell.u + normal.0 * sign,
                    door.cell.v + normal.1 * sign,
                    door.cell.floor,
                );
                if cells.contains(&cell) {
                    portals.push(cell);
                }
            }
        }
        portals.sort_by_key(|cell| (cell.v, cell.u));
        portals.dedup();
        let Some(&start) = portals.first() else {
            continue;
        };
        for &target in &portals[1..] {
            if let Some(path) = bfs_path(&cells, start, target) {
                reserved.extend(path);
            }
        }
    }
    reserved
}

fn bfs_path(
    cells: &HashSet<LocalCell>,
    start: LocalCell,
    target: LocalCell,
) -> Option<Vec<LocalCell>> {
    if start == target {
        return Some(vec![start]);
    }
    let mut queue = VecDeque::new();
    let mut parent: BTreeMap<LocalCell, LocalCell> = BTreeMap::new();
    queue.push_back(start);
    parent.insert(start, start);
    while let Some(cell) = queue.pop_front() {
        for (du, dv) in NEIGHBORS {
            let next = LocalCell::new(cell.u + du, cell.v + dv, cell.floor);
            if !cells.contains(&next) || parent.contains_key(&next) {
                continue;
            }
            parent.insert(next, cell);
            if next == target {
                let mut path = vec![target];
                let mut cursor = target;
                while cursor != start {
                    cursor = parent[&cursor];
                    path.push(cursor);
                }
                path.reverse();
                return Some(path);
            }
            queue.push_back(next);
        }
    }
    None
}

fn order_rules(
    rules: &'static [PropPlacementRule],
) -> Result<Vec<(&'static PropPlacementRule, u8)>, FitError> {
    let keys: BTreeMap<RuleKey, &PropPlacementRule> =
        rules.iter().map(|rule| (rule.key, rule)).collect();
    if keys.len() != rules.len() {
        return Err(FitError::ImpossibleRoom {
            reason: "duplicate furnishing rule key",
        });
    }
    let mut ordered = Vec::with_capacity(rules.len());
    for rule in rules {
        ordered.push((rule, dependency_depth(rule, &keys, 0)?));
    }
    ordered.sort_by_key(|(rule, depth)| {
        (
            u8::from(rule.required == 0),
            *depth,
            std::cmp::Reverse(rule.priority),
            rule.key,
        )
    });
    Ok(ordered)
}

fn dependency_depth(
    rule: &PropPlacementRule,
    keys: &BTreeMap<RuleKey, &PropPlacementRule>,
    depth: u8,
) -> Result<u8, FitError> {
    if depth > 8 {
        return Err(FitError::ImpossibleRoom {
            reason: "cyclic furnishing dependencies",
        });
    }
    let parent = match rule.strategy {
        PlacementStrategy::RelativeTo { parent, .. }
        | PlacementStrategy::Attachment { host: parent, .. } => Some(parent),
        _ => None,
    };
    match parent {
        Some(parent) => {
            let Some(parent_rule) = keys.get(&parent) else {
                return Err(FitError::ImpossibleRoom {
                    reason: "furnishing rule references a missing parent",
                });
            };
            dependency_depth(parent_rule, keys, depth + 1).map(|d| d.saturating_add(1))
        }
        None => Ok(0),
    }
}

#[allow(clippy::too_many_arguments)]
fn try_place_rule(
    seed: u64,
    blueprint: &BlueprintGrid,
    room: &RoomMeta,
    room_cells: &HashSet<LocalCell>,
    reserved: &mut HashSet<LocalCell>,
    occupancy: &mut Occupancy,
    placed_by_rule: &BTreeMap<RuleKey, Vec<PlacedProp>>,
    rule: &PropPlacementRule,
    copy: u8,
) -> Result<Option<PlacedProp>, FitError> {
    let candidates = candidates_for(
        seed,
        blueprint,
        room,
        room_cells,
        reserved,
        occupancy,
        placed_by_rule,
        rule,
    );
    for (prop, anchor, facing) in candidates {
        let def = prop.def();
        let cells = footprint(def.span, facing, anchor);
        let blocked_reserved = cells.iter().any(|c| reserved.contains(c));
        let window = cells
            .iter()
            .any(|cell| below_is_window(blueprint, *cell, def.cover));
        if cells.is_empty() || occupancy.blocks(&cells, def.attachment) || window {
            continue;
        }
        if def.attachment == CellAttachment::Floor && blocked_reserved {
            continue;
        }
        let required = copy < rule.required;
        if required
            && def.attachment == CellAttachment::Floor
            && !reserve_interaction_approach(
                blueprint, room_cells, reserved, occupancy, &cells, facing,
            )
        {
            continue;
        }
        occupancy.insert(&cells, def.attachment);
        return Ok(Some(PlacedProp {
            prop,
            anchor,
            facing,
            required,
        }));
    }
    if copy < rule.required {
        return Err(FitError::ImpossibleRoom {
            reason: "required furnishing fixture did not fit",
        });
    }
    Ok(None)
}

/// Reserve an adjacent tile the player can stand on to interact with a
/// required fixture. Already-reserved circulation counts as an approach.
fn reserve_interaction_approach(
    blueprint: &BlueprintGrid,
    room_cells: &HashSet<LocalCell>,
    reserved: &mut HashSet<LocalCell>,
    occupancy: &Occupancy,
    cells: &[LocalCell],
    facing: PropFacing,
) -> bool {
    let front = facing_offset(facing);
    let mut directions = vec![
        front,
        (-front.0, -front.1),
        (-front.1, front.0),
        (front.1, -front.0),
    ];
    directions.dedup();
    let mut fallback = None;
    for (du, dv) in directions {
        for cell in cells {
            let candidate = LocalCell::new(cell.u + du, cell.v + dv, cell.floor);
            if cells.contains(&candidate)
                || occupancy.floor.contains(&candidate)
                || !blueprint
                    .cell(candidate)
                    .is_some_and(|cell| cell.kind.walkable())
            {
                continue;
            }
            if reserved.contains(&candidate) {
                return true;
            }
            if room_cells.contains(&candidate) {
                if let Some(path) = bfs_to_reserved(room_cells, reserved, occupancy, candidate) {
                    reserved.extend(path);
                    return true;
                }
            } else {
                fallback.get_or_insert(candidate);
            }
        }
    }
    // A prop against a tight wall may only have a walkable tile outside its
    // room mask. Accept that tile if it already belongs to circulation.
    if let Some(candidate) = fallback {
        reserved.insert(candidate);
        return true;
    }
    false
}

/// Connect a fixture's interaction tile to the reserved circulation spine.
fn bfs_to_reserved(
    room_cells: &HashSet<LocalCell>,
    reserved: &HashSet<LocalCell>,
    occupancy: &Occupancy,
    start: LocalCell,
) -> Option<Vec<LocalCell>> {
    let mut queue = VecDeque::new();
    let mut parent: BTreeMap<LocalCell, LocalCell> = BTreeMap::new();
    queue.push_back(start);
    parent.insert(start, start);
    while let Some(cell) = queue.pop_front() {
        for (du, dv) in NEIGHBORS {
            let next = LocalCell::new(cell.u + du, cell.v + dv, cell.floor);
            if !room_cells.contains(&next)
                || occupancy.floor.contains(&next)
                || parent.contains_key(&next)
            {
                continue;
            }
            if reserved.contains(&next) {
                let mut path = vec![next];
                let mut cursor = cell;
                path.push(cursor);
                while cursor != start {
                    cursor = parent[&cursor];
                    path.push(cursor);
                }
                path.reverse();
                return Some(path);
            }
            parent.insert(next, cell);
            queue.push_back(next);
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn candidates_for(
    seed: u64,
    blueprint: &BlueprintGrid,
    room: &RoomMeta,
    room_cells: &HashSet<LocalCell>,
    reserved: &HashSet<LocalCell>,
    occupancy: &Occupancy,
    placed_by_rule: &BTreeMap<RuleKey, Vec<PlacedProp>>,
    rule: &PropPlacementRule,
) -> Vec<(PropId, BlueprintAnchor, PropFacing)> {
    let mut out = Vec::new();
    for &prop in rule.alternatives {
        let def = prop.def();
        let mut candidates = match rule.strategy {
            PlacementStrategy::WallRun { preference, .. } => {
                wall_run_candidates(blueprint, room, def, preference)
            }
            PlacementStrategy::FloorOpen {
                min_wall_clearance,
                prefer_corner,
            } => floor_open_candidates(
                room,
                room_cells,
                reserved,
                occupancy,
                def,
                min_wall_clearance,
                prefer_corner,
            ),
            PlacementStrategy::RelativeTo { parent, offsets } => relative_candidates(
                room_cells,
                reserved,
                occupancy,
                placed_by_rule,
                parent,
                offsets,
            ),
            PlacementStrategy::Attachment { layer, host } => {
                attachment_candidates(room_cells, reserved, occupancy, placed_by_rule, host, layer)
            }
            PlacementStrategy::AisleRun { clear_gap, .. } => {
                floor_open_candidates(room, room_cells, reserved, occupancy, def, clear_gap, false)
            }
        };
        candidates.retain(|(anchor, facing)| {
            let cells = footprint(def.span, *facing, *anchor);
            !cells.is_empty()
                && cells.iter().all(|cell| room_cells.contains(cell))
                && !occupancy.blocks(&cells, def.attachment)
                && (def.attachment != CellAttachment::Floor
                    || !cells.iter().any(|cell| reserved.contains(cell)))
        });
        if let PlacementStrategy::WallRun { preference, .. } = rule.strategy {
            candidates.sort_by_key(|(anchor, facing)| {
                let cells = footprint(def.span, *facing, *anchor);
                let (window_rank, run) = back_wall_score(blueprint, &cells, *facing, preference);
                let center = room_center(room);
                let (au, av) = (anchor.u, anchor.v);
                (
                    u8::from(preference == WallPreference::WindowThenAny && window_rank == 0),
                    u32::MAX - run,
                    (au - center.0).abs() + (av - center.1).abs(),
                    mix64(seed ^ FURNISH_SALT ^ u64::from(au as u32) << 8 ^ u64::from(av as u32)),
                    av,
                    au,
                )
            });
        } else if let PlacementStrategy::FloorOpen { prefer_corner, .. } = rule.strategy {
            let center = room_center(room);
            candidates.sort_by_key(|(anchor, _)| {
                let score = if prefer_corner {
                    corner_distance(room, *anchor)
                } else {
                    (anchor.u - center.0).abs() + (anchor.v - center.1).abs()
                };
                (score, anchor.v, anchor.u)
            });
        }
        out.extend(
            candidates
                .into_iter()
                .map(|(anchor, facing)| (prop, anchor, facing)),
        );
    }
    out
}

fn room_center(room: &RoomMeta) -> (i32, i32) {
    let min_u = room.cells.iter().map(|c| c.u).min().unwrap_or(0);
    let max_u = room.cells.iter().map(|c| c.u).max().unwrap_or(0);
    let min_v = room.cells.iter().map(|c| c.v).min().unwrap_or(0);
    let max_v = room.cells.iter().map(|c| c.v).max().unwrap_or(0);
    ((min_u + max_u) / 2, (min_v + max_v) / 2)
}

fn corner_distance(room: &RoomMeta, anchor: BlueprintAnchor) -> i32 {
    let min_u = room.cells.iter().map(|c| c.u).min().unwrap_or(0);
    let max_u = room.cells.iter().map(|c| c.u).max().unwrap_or(0);
    let min_v = room.cells.iter().map(|c| c.v).min().unwrap_or(0);
    let max_v = room.cells.iter().map(|c| c.v).max().unwrap_or(0);
    [
        (min_u, min_v),
        (min_u, max_v),
        (max_u, min_v),
        (max_u, max_v),
    ]
    .into_iter()
    .map(|(u, v)| (anchor.u - u).abs() + (anchor.v - v).abs())
    .min()
    .unwrap_or(0)
}

fn room_bbox(room: &RoomMeta) -> (i32, i32, i32, i32) {
    (
        room.cells.iter().map(|c| c.u).min().unwrap_or(0),
        room.cells.iter().map(|c| c.u).max().unwrap_or(0),
        room.cells.iter().map(|c| c.v).min().unwrap_or(0),
        room.cells.iter().map(|c| c.v).max().unwrap_or(0),
    )
}

fn footprint(span: PropSpan, facing: PropFacing, anchor: BlueprintAnchor) -> Vec<LocalCell> {
    let span = if matches!(facing, PropFacing::East | PropFacing::West) {
        PropSpan::new(span.height, span.width)
    } else {
        span
    };
    (0..span.height as i32)
        .flat_map(|v| {
            (0..span.width as i32)
                .map(move |u| LocalCell::new(anchor.u + u, anchor.v + v, anchor.floor))
        })
        .collect()
}

fn wall_run_candidates(
    blueprint: &BlueprintGrid,
    room: &RoomMeta,
    def: crate::action::PropDef,
    preference: WallPreference,
) -> Vec<(BlueprintAnchor, PropFacing)> {
    let (min_u, max_u, min_v, max_v) = room_bbox(room);
    let floor = room.floor;
    let mut out = Vec::new();
    for facing in [
        PropFacing::North,
        PropFacing::East,
        PropFacing::South,
        PropFacing::West,
    ] {
        let span = if matches!(facing, PropFacing::East | PropFacing::West) {
            PropSpan::new(def.span.height, def.span.width)
        } else {
            def.span
        };
        for v in min_v..=max_v - span.height as i32 + 1 {
            for u in min_u..=max_u - span.width as i32 + 1 {
                let anchor = BlueprintAnchor::new(u, v, floor);
                let cells = footprint(def.span, facing, anchor);
                if cells.is_empty() || !back_wall_matches(blueprint, &cells, facing, preference) {
                    continue;
                }
                out.push((anchor, facing));
            }
        }
    }
    out
}

fn back_wall_score(
    blueprint: &BlueprintGrid,
    cells: &[LocalCell],
    facing: PropFacing,
    preference: WallPreference,
) -> (u8, u32) {
    let mut window = 0;
    let mut run = 0;
    for cell in back_edge(cells, facing) {
        let (du, dv) = back_normal(facing);
        let wall = LocalCell::new(cell.u + du, cell.v + dv, cell.floor);
        if let Some(cell) = blueprint.cell(wall) {
            if cell.kind == BlueprintKind::Window {
                window = 1;
            }
            if wall_matches(cell.kind, preference) {
                run += 1;
            }
        }
    }
    (window, run)
}

fn back_wall_matches(
    blueprint: &BlueprintGrid,
    cells: &[LocalCell],
    facing: PropFacing,
    preference: WallPreference,
) -> bool {
    let mut any_window = false;
    for cell in back_edge(cells, facing) {
        let (du, dv) = back_normal(facing);
        let wall = LocalCell::new(cell.u + du, cell.v + dv, cell.floor);
        let Some(wall) = blueprint.cell(wall) else {
            return false;
        };
        if wall.kind == BlueprintKind::Window {
            any_window = true;
        }
        let ok = match preference {
            WallPreference::Window => wall.kind == BlueprintKind::Window,
            WallPreference::WindowThenAny => wall_matches(wall.kind, WallPreference::Any),
            other => wall_matches(wall.kind, other),
        };
        if !ok {
            return false;
        }
    }
    preference != WallPreference::Window || any_window
}

fn wall_matches(kind: BlueprintKind, preference: WallPreference) -> bool {
    match preference {
        WallPreference::Any | WallPreference::WindowThenAny => matches!(
            kind,
            BlueprintKind::ExteriorWall
                | BlueprintKind::InteriorWall
                | BlueprintKind::Window
                | BlueprintKind::Pillar
        ),
        WallPreference::Interior => kind == BlueprintKind::InteriorWall,
        WallPreference::Exterior => matches!(
            kind,
            BlueprintKind::ExteriorWall | BlueprintKind::Window | BlueprintKind::Pillar
        ),
        WallPreference::Window => kind == BlueprintKind::Window,
    }
}

fn back_edge(cells: &[LocalCell], facing: PropFacing) -> Vec<&LocalCell> {
    let min_v = cells.iter().map(|c| c.v).min().unwrap_or(0);
    let max_v = cells.iter().map(|c| c.v).max().unwrap_or(0);
    let min_u = cells.iter().map(|c| c.u).min().unwrap_or(0);
    let max_u = cells.iter().map(|c| c.u).max().unwrap_or(0);
    match facing {
        PropFacing::North => cells.iter().filter(|c| c.v == max_v).collect(),
        PropFacing::South => cells.iter().filter(|c| c.v == min_v).collect(),
        PropFacing::East => cells.iter().filter(|c| c.u == min_u).collect(),
        PropFacing::West => cells.iter().filter(|c| c.u == max_u).collect(),
    }
}

/// Direction from the room into the wall behind a prop facing `facing`.
fn back_normal(facing: PropFacing) -> (i32, i32) {
    match facing {
        PropFacing::North => (0, 1),
        PropFacing::South => (0, -1),
        PropFacing::East => (-1, 0),
        PropFacing::West => (1, 0),
    }
}

fn facing_offset(facing: PropFacing) -> (i32, i32) {
    match facing {
        PropFacing::North => (0, -1),
        PropFacing::East => (1, 0),
        PropFacing::South => (0, 1),
        PropFacing::West => (-1, 0),
    }
}

fn floor_open_candidates(
    room: &RoomMeta,
    room_cells: &HashSet<LocalCell>,
    reserved: &HashSet<LocalCell>,
    occupancy: &Occupancy,
    def: crate::action::PropDef,
    _min_wall_clearance: u8,
    _prefer_corner: bool,
) -> Vec<(BlueprintAnchor, PropFacing)> {
    let (min_u, max_u, min_v, max_v) = room_bbox(room);
    let floor = room.floor;
    let mut out = Vec::new();
    for facing in [
        PropFacing::North,
        PropFacing::East,
        PropFacing::South,
        PropFacing::West,
    ] {
        let span = if matches!(facing, PropFacing::East | PropFacing::West) {
            PropSpan::new(def.span.height, def.span.width)
        } else {
            def.span
        };
        for v in min_v..=max_v - span.height as i32 + 1 {
            for u in min_u..=max_u - span.width as i32 + 1 {
                let anchor = BlueprintAnchor::new(u, v, floor);
                let cells = footprint(def.span, facing, anchor);
                if cells.is_empty()
                    || !cells.iter().all(|cell| room_cells.contains(cell))
                    || occupancy.blocks(&cells, def.attachment)
                    || (def.attachment == CellAttachment::Floor
                        && cells.iter().any(|cell| reserved.contains(cell)))
                {
                    continue;
                }
                out.push((anchor, facing));
            }
        }
    }
    out
}

fn relative_candidates(
    room_cells: &HashSet<LocalCell>,
    reserved: &HashSet<LocalCell>,
    occupancy: &Occupancy,
    placed_by_rule: &BTreeMap<RuleKey, Vec<PlacedProp>>,
    parent: RuleKey,
    offsets: &'static [LocalPropOffset],
) -> Vec<(BlueprintAnchor, PropFacing)> {
    let Some(parent) = placed_by_rule.get(&parent).and_then(|props| props.first()) else {
        return Vec::new();
    };
    offsets
        .iter()
        .filter_map(|offset| {
            let anchor = BlueprintAnchor::new(
                parent.anchor.u + offset.du,
                parent.anchor.v + offset.dv,
                parent.anchor.floor,
            );
            (room_cells.contains(&anchor.local())
                && !reserved.contains(&anchor.local())
                && !occupancy.floor.contains(&anchor.local()))
            .then_some((anchor, offset.facing))
        })
        .collect()
}

fn attachment_candidates(
    room_cells: &HashSet<LocalCell>,
    reserved: &HashSet<LocalCell>,
    occupancy: &Occupancy,
    placed_by_rule: &BTreeMap<RuleKey, Vec<PlacedProp>>,
    host: RuleKey,
    layer: CellAttachment,
) -> Vec<(BlueprintAnchor, PropFacing)> {
    let Some(host) = placed_by_rule.get(&host).and_then(|props| props.first()) else {
        return Vec::new();
    };
    let anchor = host.anchor;
    let cells = footprint(host.prop.def().span, host.facing, anchor);
    if !cells.iter().all(|cell| room_cells.contains(cell))
        || occupancy.blocks(&cells, layer)
        || cells.iter().any(|cell| reserved.contains(cell))
    {
        return Vec::new();
    }
    vec![(anchor, host.facing)]
}

fn below_is_window(blueprint: &BlueprintGrid, cell: LocalCell, cover: PropCover) -> bool {
    if cover != PropCover::HighBlocker {
        return false;
    }
    for (du, dv) in NEIGHBORS {
        let neighbor = LocalCell::new(cell.u + du, cell.v + dv, cell.floor);
        if blueprint
            .cell(neighbor)
            .is_some_and(|cell| cell.kind == BlueprintKind::Window)
        {
            return true;
        }
    }
    false
}

fn prune_unreachable(
    blueprint: &mut BlueprintGrid,
    required_keys: &BTreeSet<LocalPlacementKey>,
) -> Result<u16, FitError> {
    let mut removed = 0u16;
    loop {
        let passable = furnished_passable(blueprint);
        let mut door_approaches: BTreeMap<u8, Vec<LocalCell>> = BTreeMap::new();
        for door in &blueprint.doors {
            let normal = door.tangent.normal();
            for sign in [-1, 1] {
                let cell = LocalCell::new(
                    door.cell.u + normal.0 * sign,
                    door.cell.v + normal.1 * sign,
                    door.cell.floor,
                );
                if passable.contains(&cell) {
                    door_approaches.entry(cell.floor).or_default().push(cell);
                }
            }
        }
        let mut reached: HashSet<LocalCell> = HashSet::new();
        let mut blocked_rooms: BTreeSet<u16> = BTreeSet::new();
        let mut disconnected = false;
        for cells in door_approaches.values() {
            if cells.is_empty() {
                continue;
            }
            let root = *cells.iter().min().unwrap();
            let component = bfs_component(&passable, root);
            for cell in cells {
                if !component.contains(cell) {
                    disconnected = true;
                    if let Some(room) = room_at_cell(blueprint, *cell) {
                        blocked_rooms.insert(room);
                    }
                }
            }
            reached.extend(component);
        }

        let mut blocked_required_rooms: BTreeSet<u16> = BTreeSet::new();
        let mut remove = BTreeSet::new();
        for prop in &blueprint.props {
            if prop.prop.def().attachment != CellAttachment::Floor {
                continue;
            }
            let cells = footprint(prop.prop.def().span, prop.facing, prop.anchor);
            let approach = NEIGHBORS.iter().any(|(du, dv)| {
                cells.iter().any(|cell| {
                    let neighbor = LocalCell::new(cell.u + du, cell.v + dv, cell.floor);
                    !cells.contains(&neighbor) && reached.contains(&neighbor)
                })
            });
            if approach {
                continue;
            }
            if required_keys.contains(&prop.key) {
                if let Some(room) = prop.room {
                    blocked_required_rooms.insert(room);
                } else {
                    return Err(FitError::AccessFailure {
                        reason: "required fixture has no reachable interaction approach",
                    });
                }
            } else {
                remove.insert(prop.key);
            }
        }

        if !disconnected && blocked_required_rooms.is_empty() {
            if remove.is_empty() {
                return Ok(removed);
            }
            let before = blueprint.props.len();
            blueprint.props.retain(|prop| !remove.contains(&prop.key));
            removed += (before - blueprint.props.len()) as u16;
            continue;
        }

        // Optional clutter closed a required route. Drop optional props in the
        // affected room(s) and retry the full reachability pass. This is the
        // deterministic last resort the authored fallback expects.
        let mut rooms = blocked_required_rooms.clone();
        rooms.extend(blocked_rooms.clone());
        if rooms.is_empty() {
            rooms.extend(blueprint.rooms.iter().map(|room| room.id));
        }
        let before = blueprint.props.len();
        blueprint.props.retain(|prop| {
            required_keys.contains(&prop.key)
                || !prop.room.is_some_and(|room| rooms.contains(&room))
        });
        let removed_now = before - blueprint.props.len();
        if removed_now == 0 {
            return Err(FitError::AccessFailure {
                reason: "required fixture has no reachable interaction approach",
            });
        }
        removed += removed_now as u16;
    }
}

fn furnished_passable(blueprint: &BlueprintGrid) -> HashSet<LocalCell> {
    let mut passable: HashSet<LocalCell> = blueprint
        .cells
        .iter()
        .filter(|(_, cell)| cell.kind.walkable())
        .map(|(cell, _)| *cell)
        .collect();
    for prop in &blueprint.props {
        if prop.prop.def().attachment != CellAttachment::Floor {
            continue;
        }
        for cell in footprint(prop.prop.def().span, prop.facing, prop.anchor) {
            passable.remove(&cell);
        }
    }
    passable
}

fn room_at_cell(blueprint: &BlueprintGrid, cell: LocalCell) -> Option<u16> {
    blueprint
        .rooms
        .iter()
        .find(|room| room.cells.contains(&cell))
        .map(|room| room.id)
}

fn bfs_component(passable: &HashSet<LocalCell>, root: LocalCell) -> HashSet<LocalCell> {
    let mut component = HashSet::new();
    let mut queue = VecDeque::new();
    if !passable.contains(&root) {
        return component;
    }
    component.insert(root);
    queue.push_back(root);
    while let Some(cell) = queue.pop_front() {
        for (du, dv) in NEIGHBORS {
            let next = LocalCell::new(cell.u + du, cell.v + dv, cell.floor);
            if passable.contains(&next) && component.insert(next) {
                queue.push_back(next);
            }
        }
    }
    component
}

fn rules_for(kind: RoomKind) -> &'static [PropPlacementRule] {
    match kind {
        RoomKind::Kitchen => KITCHEN_RULES,
        RoomKind::Living => LIVING_RULES,
        RoomKind::Bedroom => BEDROOM_RULES,
        RoomKind::Bathroom => BATHROOM_RULES,
        _ => &[],
    }
}

const CHAIR_OFFSETS: &[LocalPropOffset] = &[
    LocalPropOffset {
        du: -1,
        dv: 0,
        facing: PropFacing::East,
    },
    LocalPropOffset {
        du: 2,
        dv: 0,
        facing: PropFacing::West,
    },
    LocalPropOffset {
        du: 0,
        dv: 1,
        facing: PropFacing::North,
    },
    LocalPropOffset {
        du: 0,
        dv: -1,
        facing: PropFacing::South,
    },
];

const NIGHTSTAND_OFFSETS: &[LocalPropOffset] = &[
    LocalPropOffset {
        du: -1,
        dv: 2,
        facing: PropFacing::East,
    },
    LocalPropOffset {
        du: 2,
        dv: 2,
        facing: PropFacing::West,
    },
];

const DESK_CHAIR_OFFSETS: &[LocalPropOffset] = &[
    LocalPropOffset {
        du: 2,
        dv: 0,
        facing: PropFacing::West,
    },
    LocalPropOffset {
        du: 0,
        dv: 2,
        facing: PropFacing::North,
    },
];

const TOILET_OFFSETS: &[LocalPropOffset] = &[
    LocalPropOffset {
        du: -2,
        dv: -1,
        facing: PropFacing::East,
    },
    LocalPropOffset {
        du: -2,
        dv: 0,
        facing: PropFacing::East,
    },
];

const SINK_OFFSETS: &[LocalPropOffset] = &[
    LocalPropOffset {
        du: -2,
        dv: 0,
        facing: PropFacing::East,
    },
    LocalPropOffset {
        du: -2,
        dv: 1,
        facing: PropFacing::East,
    },
];

const KITCHEN_RULES: &[PropPlacementRule] = &[
    PropPlacementRule {
        key: RuleKey(10),
        alternatives: &[PropId::KitchenSink],
        strategy: PlacementStrategy::WallRun {
            preference: WallPreference::WindowThenAny,
            segments: (1, 4),
        },
        required: 1,
        maximum: 1,
        extra_chance: 0.0,
        priority: 100,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(11),
        alternatives: &[PropId::KitchenStove],
        strategy: PlacementStrategy::WallRun {
            preference: WallPreference::WindowThenAny,
            segments: (1, 4),
        },
        required: 1,
        maximum: 1,
        extra_chance: 0.0,
        priority: 95,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(12),
        alternatives: &[PropId::Fridge],
        strategy: PlacementStrategy::WallRun {
            preference: WallPreference::Any,
            segments: (1, 4),
        },
        required: 1,
        maximum: 1,
        extra_chance: 0.0,
        priority: 90,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(13),
        alternatives: &[PropId::KitchenCabinet],
        strategy: PlacementStrategy::WallRun {
            preference: WallPreference::Any,
            segments: (1, 4),
        },
        required: 0,
        maximum: 3,
        extra_chance: 0.8,
        priority: 40,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(14),
        alternatives: &[PropId::Table],
        strategy: PlacementStrategy::FloorOpen {
            min_wall_clearance: 0,
            prefer_corner: false,
        },
        required: 0,
        maximum: 1,
        extra_chance: 1.0,
        priority: 30,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(15),
        alternatives: &[PropId::Chair],
        strategy: PlacementStrategy::RelativeTo {
            parent: RuleKey(14),
            offsets: CHAIR_OFFSETS,
        },
        required: 0,
        maximum: 3,
        extra_chance: 1.0,
        priority: 20,
        loot: None,
    },
];

const LIVING_RULES: &[PropPlacementRule] = &[
    PropPlacementRule {
        key: RuleKey(20),
        alternatives: &[PropId::Sofa],
        strategy: PlacementStrategy::WallRun {
            preference: WallPreference::Any,
            segments: (1, 4),
        },
        required: 1,
        maximum: 1,
        extra_chance: 0.0,
        priority: 100,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(21),
        alternatives: &[PropId::Counter],
        strategy: PlacementStrategy::WallRun {
            preference: WallPreference::Any,
            segments: (1, 4),
        },
        required: 0,
        maximum: 1,
        extra_chance: 1.0,
        priority: 70,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(22),
        alternatives: &[PropId::Television],
        strategy: PlacementStrategy::Attachment {
            layer: CellAttachment::CounterTop,
            host: RuleKey(21),
        },
        required: 0,
        maximum: 1,
        extra_chance: 1.0,
        priority: 60,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(23),
        alternatives: &[PropId::TableCoffee],
        strategy: PlacementStrategy::FloorOpen {
            min_wall_clearance: 0,
            prefer_corner: false,
        },
        required: 0,
        maximum: 1,
        extra_chance: 1.0,
        priority: 50,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(24),
        alternatives: &[PropId::BookcaseOpen],
        strategy: PlacementStrategy::WallRun {
            preference: WallPreference::Any,
            segments: (1, 4),
        },
        required: 0,
        maximum: 2,
        extra_chance: 0.4,
        priority: 20,
        loot: None,
    },
];

const BEDROOM_RULES: &[PropPlacementRule] = &[
    PropPlacementRule {
        key: RuleKey(30),
        alternatives: &[PropId::BedDouble, PropId::Bed],
        strategy: PlacementStrategy::WallRun {
            preference: WallPreference::Exterior,
            segments: (1, 4),
        },
        required: 1,
        maximum: 1,
        extra_chance: 0.0,
        priority: 100,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(31),
        alternatives: &[PropId::Counter],
        strategy: PlacementStrategy::RelativeTo {
            parent: RuleKey(30),
            offsets: NIGHTSTAND_OFFSETS,
        },
        required: 1,
        maximum: 2,
        extra_chance: 0.8,
        priority: 60,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(32),
        alternatives: &[PropId::Desk],
        strategy: PlacementStrategy::FloorOpen {
            min_wall_clearance: 0,
            prefer_corner: true,
        },
        required: 0,
        maximum: 1,
        extra_chance: 0.7,
        priority: 50,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(33),
        alternatives: &[PropId::ChairDesk],
        strategy: PlacementStrategy::RelativeTo {
            parent: RuleKey(32),
            offsets: DESK_CHAIR_OFFSETS,
        },
        required: 0,
        maximum: 1,
        extra_chance: 1.0,
        priority: 40,
        loot: None,
    },
];

const BATHROOM_RULES: &[PropPlacementRule] = &[
    PropPlacementRule {
        key: RuleKey(40),
        alternatives: &[PropId::Bathtub],
        strategy: PlacementStrategy::WallRun {
            preference: WallPreference::Any,
            segments: (1, 4),
        },
        required: 1,
        maximum: 1,
        extra_chance: 0.0,
        priority: 100,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(41),
        alternatives: &[PropId::Toilet],
        strategy: PlacementStrategy::RelativeTo {
            parent: RuleKey(40),
            offsets: TOILET_OFFSETS,
        },
        required: 1,
        maximum: 1,
        extra_chance: 0.0,
        priority: 90,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(42),
        alternatives: &[PropId::BathroomSink],
        strategy: PlacementStrategy::RelativeTo {
            parent: RuleKey(40),
            offsets: SINK_OFFSETS,
        },
        required: 1,
        maximum: 1,
        extra_chance: 0.0,
        priority: 80,
        loot: None,
    },
    PropPlacementRule {
        key: RuleKey(43),
        alternatives: &[PropId::FirstAidKit],
        strategy: PlacementStrategy::Attachment {
            layer: CellAttachment::CounterTop,
            host: RuleKey(42),
        },
        required: 0,
        maximum: 1,
        extra_chance: 1.0,
        priority: 40,
        loot: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_keys_are_stable_per_rule_and_copy() {
        assert_eq!(placement_key(RuleKey(10), 0), LocalPlacementKey(0x0a00));
        assert_eq!(placement_key(RuleKey(10), 1), LocalPlacementKey(0x0a01));
        assert_eq!(placement_key(RuleKey(11), 0), LocalPlacementKey(0x0b00));
    }

    #[test]
    fn cyclic_rules_are_rejected() {
        static CYCLIC: &[PropPlacementRule] = &[
            PropPlacementRule {
                key: RuleKey(1),
                alternatives: &[PropId::Chair],
                strategy: PlacementStrategy::RelativeTo {
                    parent: RuleKey(2),
                    offsets: &[],
                },
                required: 0,
                maximum: 1,
                extra_chance: 1.0,
                priority: 1,
                loot: None,
            },
            PropPlacementRule {
                key: RuleKey(2),
                alternatives: &[PropId::Chair],
                strategy: PlacementStrategy::RelativeTo {
                    parent: RuleKey(1),
                    offsets: &[],
                },
                required: 0,
                maximum: 1,
                extra_chance: 1.0,
                priority: 1,
                loot: None,
            },
        ];
        assert!(matches!(
            order_rules(CYCLIC),
            Err(FitError::ImpossibleRoom { .. })
        ));
    }
}
